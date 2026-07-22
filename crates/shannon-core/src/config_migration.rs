//! Φ1b — runtime adapter that turns a v1 [`ShannonConfig`] into a v2
//! [`ProviderModelConfig`] **and** persists the legacy plaintext credentials
//! so the resulting env references resolve.
//!
//! This is the runtime half of decision **C1** (one-shot v1→v2 migration).
//! The pure structural mapping lives in [`shannon_types::migration`]
//! (Φ1a, no I/O, no `shannon-core` dependency); this module is its
//! `shannon-core` counterpart and supplies:
//!
//! 1. [`to_legacy_v1_config`] — a field-by-field adapter from
//!    [`ShannonConfig`] into [`LegacyV1Config`] (a free function, because an
//!    `impl From<&ShannonConfig> for LegacyV1Config` would violate the orphan
//!    rule — both the trait and the target type are foreign to this crate).
//! 2. [`plan_migration`] — runs the Φ1a mapping and pairs every migrated
//!    [`CredentialRef::Env`] reference with the v1 plaintext value that should
//!    back it, returning a [`MigrationPlan`].
//! 3. [`persist_secrets`] — merge-writes the secrets to a shell-sourceable
//!    `secrets.env` at `0600`, **without clobbering unrelated keys**.
//!
//! # Decision A1 (no plaintext in v2 config)
//! The migrated [`ProviderModelConfig`] never contains plaintext — it only
//! carries `CredentialRef::Env { var }` references. The plaintext lives solely
//! in the returned [`SecretBinding`]s, which [`persist_secrets`] writes to
//! `~/.shannon/secrets.env` (chmod 0600). Secret-bearing types implement
//! `Debug` manually with values redacted, so accidental `{:?}` logging cannot
//! leak them.

use crate::unified_config::{ProviderEntry, ShannonConfig};
use shannon_types::migration::{LegacyV1Config, LegacyV1ProviderEntry, MigrationError, migrate_v1};
use shannon_types::provider_config::{CredentialRef, ProviderModelConfig};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

/// A migrated v2 config plus the legacy secrets that must be persisted so its
/// env references resolve.
///
/// `Debug` is manual: secret **values** are redacted (only the count is shown)
/// to honour decision A1 — the plaintext must never appear in logs.
pub struct MigrationPlan {
    /// The v2 config. Contains **no** plaintext (A1).
    pub config: ProviderModelConfig,
    /// `(env_var, plaintext_value)` pairs to write to `secrets.env`.
    pub secrets: Vec<SecretBinding>,
}

impl std::fmt::Debug for MigrationPlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MigrationPlan")
            .field("config", &self.config)
            .field(
                "secrets",
                &format!("{} binding(s) [values redacted]", self.secrets.len()),
            )
            .finish()
    }
}

/// One credential to persist: the env-var name a migrated `CredentialRef::Env`
/// references, and the v1 plaintext value that should back it.
///
/// `Debug` redacts `value` to avoid leaking it in logs.
#[derive(Clone)]
pub struct SecretBinding {
    pub var: String,
    pub value: String,
}

impl std::fmt::Debug for SecretBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretBinding")
            .field("var", &self.var)
            .field("value", &"<redacted>")
            .finish()
    }
}

/// Adapt a v1 [`ShannonConfig`] into the migrator's [`LegacyV1Config`] input.
///
/// A free function (not `impl From`) to stay clear of the orphan rule. Only the
/// provider/model-relevant fields are carried; `max_tokens` (v1 `usize`) is
/// narrowed to `u32` losslessly (`None` on overflow, which v1's own clamp
/// already prevents in practice).
pub fn to_legacy_v1_config(cfg: &ShannonConfig) -> LegacyV1Config {
    let providers = cfg
        .providers
        .as_ref()
        .map(|m| {
            m.iter()
                .map(|(k, v)| (k.clone(), to_legacy_provider_entry(v)))
                .collect()
        })
        .unwrap_or_default();
    LegacyV1Config {
        model: cfg.model.clone(),
        provider: cfg.provider.clone(),
        api_key: cfg.api_key.clone(),
        base_url: cfg.base_url.clone(),
        max_tokens: cfg.max_tokens.and_then(|v| u32::try_from(v).ok()),
        providers,
    }
}

fn to_legacy_provider_entry(e: &ProviderEntry) -> LegacyV1ProviderEntry {
    LegacyV1ProviderEntry {
        api_key: e.api_key.clone(),
        api_key_env: e.api_key_env.clone(),
        base_url: e.base_url.clone(),
        model: e.model.clone(),
    }
}

/// Run the Φ1a migration and collect the legacy secrets that must be persisted.
///
/// Each migrated provider profile whose credential is `CredentialRef::Env` is
/// paired with the v1 plaintext that backs it (named `[providers.<name>]`
/// keys, with the top-level `api_key` winning for the top-level provider —
/// matching v1's own resolution order). Providers with only an `api_key_env`
/// name and no plaintext produce **no** binding.
pub fn plan_migration(cfg: &ShannonConfig) -> Result<MigrationPlan, MigrationError> {
    let legacy = to_legacy_v1_config(cfg);
    let config = migrate_v1(&legacy)?;

    // name → plaintext, v1 resolution order (top-level api_key wins for its provider).
    let mut name_to_secret: HashMap<String, String> = HashMap::new();
    if let Some(providers) = &cfg.providers {
        for (name, entry) in providers {
            if let Some(key) = &entry.api_key {
                name_to_secret.insert(name.clone(), key.clone());
            }
        }
    }
    if let (Some(name), Some(key)) = (&cfg.provider, &cfg.api_key) {
        name_to_secret.insert(name.clone(), key.clone());
    }

    let mut secrets: Vec<SecretBinding> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for profile in config
        .profiles
        .get("default")
        .into_iter()
        .flat_map(|p| p.providers.iter())
    {
        if let CredentialRef::Env { var } = &profile.credential {
            if !seen.insert(var.as_str()) {
                continue; // two providers may share an api_key_env name
            }
            if let Some(value) = name_to_secret.get(&profile.id) {
                secrets.push(SecretBinding {
                    var: var.clone(),
                    value: value.clone(),
                });
            }
        }
    }

    Ok(MigrationPlan { config, secrets })
}

/// Default location for the persisted secrets file: `~/.shannon/secrets.env`.
/// `None` if the user's home directory cannot be determined.
pub fn default_secrets_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".shannon").join("secrets.env"))
}

/// Merge-write `bindings` into a shell-sourceable `secrets.env` at `path`.
///
/// - Existing `KEY=value` lines are preserved and **updated in place**; new
///   keys are appended. Lines that are neither a binding nor a comment-free
///   `KEY=value` are dropped on rewrite (this is a managed file).
/// - Values are single-quote-escaped so the file is safe to `source` / load
///   with `dotenvy` regardless of special characters.
/// - The file (and its parent directory) is created with restrictive
///   permissions (`0600` / `0700`) on Unix.
pub fn persist_secrets(bindings: &[SecretBinding], path: &Path) -> std::io::Result<()> {
    // Load existing KEY=value entries, preserving order.
    let mut entries: Vec<(String, String)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    if let Ok(text) = fs::read_to_string(path) {
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = trimmed.split_once('=') {
                let key = k.trim().to_string();
                if key.is_empty() {
                    continue;
                }
                index.insert(key.clone(), entries.len());
                entries.push((key, unquote(v.trim())));
            }
        }
    }

    // Merge: overwrite existing, append new.
    for b in bindings {
        if let Some(&i) = index.get(&b.var) {
            entries[i].1 = b.value.clone();
        } else {
            index.insert(b.var.clone(), entries.len());
            entries.push((b.var.clone(), b.value.clone()));
        }
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        restrict_dir(parent)?;
    }

    let mut out = String::new();
    out.push_str(
        "# ~/.shannon/secrets.env — migrated v1 plaintext api_key (C1/A1). chmod 0600. Keep secret.\n",
    );
    for (k, v) in &entries {
        out.push_str(&format!("{k}={}\n", shell_quote_single(v)));
    }
    fs::write(path, out)?;
    restrict_file(path)?;
    Ok(())
}

/// Wrap `v` in single quotes, escaping embedded single quotes as `'\''` so the
/// result is safe inside a single-quoted shell word.
fn shell_quote_single(v: &str) -> String {
    let mut out = String::with_capacity(v.len() + 2);
    out.push('\'');
    for ch in v.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

/// Inverse of [`shell_quote_single`] for the values we read back: strip one
/// matching pair of surrounding quotes and unescape `'\''`.
fn unquote(v: &str) -> String {
    let v = v.trim();
    if v.len() >= 2 {
        let first = v.chars().next();
        let last = v.chars().last();
        if first == Some('\'') && last == Some('\'') {
            return v[1..v.len() - 1].replace("'\\''", "'");
        }
        if first == Some('"') && last == Some('"') {
            return v[1..v.len() - 1].to_string();
        }
    }
    v.to_string()
}

#[cfg(unix)]
fn restrict_file(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict_file(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn restrict_dir(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn restrict_dir(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::unified_config::ProviderEntry;
    use std::collections::HashMap;

    fn binding(var: &str, value: &str) -> SecretBinding {
        SecretBinding {
            var: var.to_string(),
            value: value.to_string(),
        }
    }

    #[test]
    fn adapter_maps_fields_and_narrows_max_tokens() {
        let mut providers = HashMap::new();
        providers.insert(
            "openai".to_string(),
            ProviderEntry {
                api_key: Some("sk-openai".to_string()),
                api_key_env: Some("OPENAI_KEY".to_string()),
                base_url: Some("https://api.openai.com/v1".to_string()),
                model: Some("gpt-4o".to_string()),
            },
        );
        let cfg = ShannonConfig {
            model: Some("claude-x".to_string()),
            provider: Some("anthropic".to_string()),
            api_key: Some("sk-top".to_string()),
            base_url: Some("https://api.anthropic.com".to_string()),
            max_tokens: Some(8192),
            providers: Some(providers),
            ..Default::default()
        };
        let legacy = to_legacy_v1_config(&cfg);
        assert_eq!(legacy.model.as_deref(), Some("claude-x"));
        assert_eq!(legacy.provider.as_deref(), Some("anthropic"));
        assert_eq!(legacy.api_key.as_deref(), Some("sk-top"));
        assert_eq!(
            legacy.base_url.as_deref(),
            Some("https://api.anthropic.com")
        );
        assert_eq!(legacy.max_tokens, Some(8192_u32));
        let openai = legacy.providers.get("openai").unwrap();
        assert_eq!(openai.api_key.as_deref(), Some("sk-openai"));
        assert_eq!(openai.api_key_env.as_deref(), Some("OPENAI_KEY"));
        assert_eq!(
            openai.base_url.as_deref(),
            Some("https://api.openai.com/v1")
        );
        assert_eq!(openai.model.as_deref(), Some("gpt-4o"));

        // None providers → empty map (not panic).
        let empty = to_legacy_v1_config(&ShannonConfig::default());
        assert!(empty.providers.is_empty());
        assert_eq!(empty.max_tokens, None);
    }

    #[test]
    fn plan_pairs_env_cred_with_top_level_plaintext() {
        let cfg = ShannonConfig {
            provider: Some("anthropic".to_string()),
            api_key: Some("sk-SECRET".to_string()),
            ..Default::default()
        };
        let plan = plan_migration(&cfg).unwrap();
        // Config has NO plaintext (A1).
        let json = serde_json::to_string(&plan.config).unwrap();
        assert!(
            !json.contains("sk-SECRET"),
            "plaintext leaked into v2 config: {json}"
        );
        // Secret is captured in the binding.
        assert_eq!(plan.secrets.len(), 1);
        assert_eq!(plan.secrets[0].var, "SHANNON_ANTHROPIC_API_KEY");
        assert_eq!(plan.secrets[0].value, "sk-SECRET");
    }

    #[test]
    fn plan_respects_explicit_api_key_env_as_binding_var() {
        let mut providers = HashMap::new();
        providers.insert(
            "openai".to_string(),
            ProviderEntry {
                api_key: Some("sk-oai".to_string()),
                api_key_env: Some("MY_OPENAI_KEY".to_string()),
                ..Default::default()
            },
        );
        let cfg = ShannonConfig {
            providers: Some(providers),
            ..Default::default()
        };
        let plan = plan_migration(&cfg).unwrap();
        assert_eq!(plan.secrets.len(), 1);
        assert_eq!(plan.secrets[0].var, "MY_OPENAI_KEY");
        assert_eq!(plan.secrets[0].value, "sk-oai");
    }

    #[test]
    fn plan_named_provider_secret_uses_default_env_var() {
        let mut providers = HashMap::new();
        providers.insert(
            "deepseek".to_string(),
            ProviderEntry {
                api_key: Some("sk-ds".to_string()),
                ..Default::default()
            },
        );
        let cfg = ShannonConfig {
            providers: Some(providers),
            ..Default::default()
        };
        let plan = plan_migration(&cfg).unwrap();
        assert_eq!(plan.secrets.len(), 1);
        assert_eq!(plan.secrets[0].var, "SHANNON_DEEPSEEK_API_KEY");
        assert_eq!(plan.secrets[0].value, "sk-ds");
    }

    #[test]
    fn plan_no_binding_when_only_env_name_present() {
        let mut providers = HashMap::new();
        providers.insert(
            "openai".to_string(),
            ProviderEntry {
                api_key_env: Some("OPENAI_KEY".to_string()),
                base_url: Some("https://api.openai.com/v1".to_string()),
                model: Some("gpt-4o".to_string()),
                ..Default::default()
            },
        );
        let cfg = ShannonConfig {
            providers: Some(providers),
            ..Default::default()
        };
        let plan = plan_migration(&cfg).unwrap();
        // Config still carries the env ref, but there's no plaintext to persist.
        assert!(
            plan.secrets.is_empty(),
            "no plaintext should yield no binding"
        );
    }

    #[test]
    fn plan_empty_config_is_no_provider() {
        assert_eq!(
            plan_migration(&ShannonConfig::default()).unwrap_err(),
            MigrationError::NoProvider
        );
    }

    #[test]
    fn debug_does_not_leak_secret_values() {
        let plan = MigrationPlan {
            config: ProviderModelConfig {
                version: 2,
                profiles: HashMap::new(),
                gateway: Default::default(),
            },
            secrets: vec![binding("SHANNON_X_API_KEY", "sk-SUPER-SECRET")],
        };
        let dbg = format!("{plan:?}");
        assert!(!dbg.contains("sk-SUPER-SECRET"));
        assert!(dbg.contains("1 binding(s)"));

        let s_dbg = format!("{:?}", binding("VAR", "sk-LEAK"));
        assert!(!s_dbg.contains("sk-LEAK"));
        assert!(s_dbg.contains("<redacted>"));
    }

    #[test]
    fn persist_creates_file_with_0600_and_merges() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.env");

        persist_secrets(&[binding("SHANNON_ANTHROPIC_API_KEY", "sk-1")], &path).unwrap();
        assert!(path.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "secrets file must be 0600");
        }
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("SHANNON_ANTHROPIC_API_KEY='sk-1'"));

        // Second write: append a new key + overwrite the existing one.
        persist_secrets(&[binding("SHANNON_OPENAI_API_KEY", "sk-2")], &path).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("SHANNON_ANTHROPIC_API_KEY='sk-1'"));
        assert!(text.contains("SHANNON_OPENAI_API_KEY='sk-2'"));
    }

    #[test]
    fn persist_preserves_unrelated_keys_and_overwrites_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.env");
        // Pre-existing file with an unrelated key + a comment.
        fs::write(&path, "# keep me\nUNRELATED=foo\nSHANNON_X_API_KEY=old\n").unwrap();

        persist_secrets(&[binding("SHANNON_X_API_KEY", "new")], &path).unwrap();
        let parsed: HashMap<String, String> = dotenvy::from_path_iter(&path)
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            parsed.get("UNRELATED").map(String::as_str),
            Some("foo"),
            "unrelated key kept"
        );
        assert_eq!(
            parsed.get("SHANNON_X_API_KEY").map(String::as_str),
            Some("new"),
            "binding overwritten in place"
        );
    }

    #[test]
    fn persist_quotes_special_values_and_roundtrips_via_dotenvy() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.env");
        let nasty = "p@ss 'with' spaces #and $pecial \n\\\n";
        persist_secrets(&[binding("WEIRD_KEY", nasty)], &path).unwrap();

        let parsed: HashMap<String, String> = dotenvy::from_path_iter(&path)
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(parsed.get("WEIRD_KEY").map(String::as_str), Some(nasty));
    }

    #[test]
    fn default_secrets_path_is_under_home() {
        let path = default_secrets_path();
        // In test environments HOME is set; just assert shape when present.
        if let Some(p) = path {
            assert!(p.ends_with("secrets.env"));
            assert!(p.to_string_lossy().contains(".shannon"));
        }
    }
}
