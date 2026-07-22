//! Credential-persistence primitives for decision **A1**.
//!
//! This module began life as the runtime half of the C1 v1→v2 migration (Φ1b).
//! With shannon-code/desktop still **unreleased** (no-compat), the one-shot
//! migration is no longer needed and the migration-half (`plan_migration` /
//! `to_legacy_v1_config`) plus the Φ1a `shannon_types::migration` module have
//! been removed. What remains are the reusable secrets primitives that N2 uses
//! to materialise a shell-sourceable, chmod-0600 `~/.shannon/secrets.env`:
//!
//! - [`SecretBinding`] — one `(env_var, plaintext_value)` pair, with a
//!   manually-redacted `Debug` so values never leak into logs.
//! - [`persist_secrets`] — merge-writes bindings into `secrets.env` without
//!   clobbering unrelated keys.
//! - [`default_secrets_path`] — the conventional `~/.shannon/secrets.env`.
//!
//! # Decision A1 (no plaintext in the v2 config)
//! The v2 [`shannon_types::provider_config::ProviderModelConfig`] never stores
//! plaintext — it carries only `CredentialRef::Env { var }` references. The
//! plaintext lives solely in the [`SecretBinding`]s that [`persist_secrets`]
//! writes to `~/.shannon/secrets.env` (chmod 0600). Secret-bearing types
//! implement `Debug` manually with values redacted, so accidental `{:?}`
//! logging cannot leak them.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// One credential to persist: the env-var name a `CredentialRef::Env` references,
/// and the plaintext value that should back it.
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
    use std::collections::HashMap;

    fn binding(var: &str, value: &str) -> SecretBinding {
        SecretBinding {
            var: var.to_string(),
            value: value.to_string(),
        }
    }

    #[test]
    fn debug_redacts_secret_binding_value() {
        let s_dbg = format!("{:?}", binding("VAR", "sk-LEAK"));
        assert!(!s_dbg.contains("sk-LEAK"));
        assert!(s_dbg.contains("<redacted>"));
        assert!(s_dbg.contains("VAR"));
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
