//! P5 Data source installers.
//!
//! Data sources don't "install" anything from the network — they persist
//! adapter-specific config to `~/.shannon/data-sources/<slug>.toml`. The
//! frontend form prompts for the fields declared in
//! `data_source_catalog::DataSourceField`, then `install_data_source` writes
//! the file. At query time the native adapter loads the file and connects.
//!
//! For the MVP the credentials lived in the same TOML file; F5 (R7-④
//! batch 2) moves them into the OS keyring (`InstallTarget::Keychain`,
//! realised): credential fields (the catalog's `password`-kind keys — IMAP
//! password, Notion integration token, …) live under
//! `shannon/datasource/<slug>` (JSON map field → value) whenever the
//! keyring is available; the TOML keeps only the non-secret config
//! (host/port/user/enabled). Plaintext in the TOML is a *fallback*: the
//! startup migration drains it into the keyring, a failed keyring write
//! keeps it (owner-only 0600) with a warn, and reads prefer the keyring
//! while it still exists (tolerance window).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::installer::InstallError;

/// Where data source config files live. Today: `~/.shannon/data-sources/`.
fn shannon_data_sources_root() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".shannon").join("data-sources"))
        .unwrap_or_else(|| PathBuf::from("/tmp/shannon-data-sources"))
}

/// Wire type: a stored data source config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledDataSource {
    /// Adapter slug — matches `DataSourceAdapter::slug`.
    pub slug: String,
    /// Adapter kind (`obsidian`, `email_imap`).
    pub kind: String,
    /// Display name (human-readable).
    pub name: String,
    /// Path to the config file under `~/.shannon/data-sources/`.
    pub path: String,
    /// RFC3339 install timestamp.
    #[serde(default)]
    pub installed_at: Option<String>,
    /// F5 (A8): where this source's credentials live — `"keyring"` when the
    /// secret store holds them, `"plaintext_file"` for the degraded
    /// owner-only-0600 fallback, `None` for sources without credential
    /// fields. Drives the data-sources page's credential-storage line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_storage: Option<&'static str>,
}

/// Split a form-value map into (non-secret fields, secret fields) — the
/// secret set comes from the catalog's `password`-kind field keys.
fn split_secret_fields(
    config: &BTreeMap<String, String>,
) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
    let secret_keys = super::data_source_catalog::secret_field_keys();
    let mut plain = BTreeMap::new();
    let mut secrets = BTreeMap::new();
    for (key, value) in config {
        if secret_keys.contains(key) {
            secrets.insert(key.clone(), value.clone());
        } else {
            plain.insert(key.clone(), value.clone());
        }
    }
    (plain, secrets)
}

/// Serialize one source's secret fields into the keyring payload (a JSON
/// object so multiple credential fields travel atomically).
fn secrets_payload(secrets: &BTreeMap<String, String>) -> Result<String, serde_json::Error> {
    serde_json::to_string(secrets)
}

/// Deserialize a keyring payload back into the secret-field map. An
/// unparseable payload is a degraded read (treated as absent), never a
/// hard error — the plaintext fallback still applies.
fn parse_secrets_payload(raw: &str) -> Option<BTreeMap<String, String>> {
    match serde_json::from_str::<BTreeMap<String, String>>(raw) {
        Ok(map) => Some(map),
        Err(e) => {
            tracing::warn!(
                domain = "datasource",
                error = %e,
                "keyring data-source payload is not valid JSON — treating as absent"
            );
            None
        }
    }
}

/// Persist a data source config to disk.
///
/// `slug` becomes the file name. `config` is the user-supplied form values
/// (vault_path, imap_host, password, etc.). Each value is written into the
/// TOML file under its key. With a working keyring the credential fields
/// move into `shannon/datasource/<slug>` immediately and the file is
/// written without them.
pub fn install_data_source(
    slug: &str,
    kind: &str,
    name: &str,
    config: &BTreeMap<String, String>,
) -> Result<InstalledDataSource, InstallError> {
    install_data_source_in_with_store(
        &shannon_data_sources_root(),
        slug,
        kind,
        name,
        config,
        crate::secret_store::global().as_deref(),
    )
}

/// Implementation that writes to an explicit `root` directory instead of
/// resolving `~/.shannon/data-sources/` from `$HOME`. Tests drive this with a
/// tempdir so they never mutate the process-global `HOME` env var — that
/// mutation raced with unrelated tests reading `dirs::home_dir()` on another
/// thread, which was the source of a pre-existing parallel-test flake.
/// [`install_data_source`] against an explicit `root` with an injected
/// secret store (`None` = the degraded plaintext-only shape; tests inject a
/// mock, never the real keyring; `root` instead of `$HOME` keeps tests off
/// the process-global HOME env var — see the history of this module).
fn install_data_source_in_with_store(
    root: &Path,
    slug: &str,
    kind: &str,
    name: &str,
    config: &BTreeMap<String, String>,
    store: Option<&dyn crate::secret_store::SecretStore>,
) -> Result<InstalledDataSource, InstallError> {
    if slug.trim().is_empty() {
        return Err(InstallError::Format("data source slug is required".into()));
    }
    if slug.contains('/') || slug.contains('\\') || slug.contains("..") {
        return Err(InstallError::Format(format!(
            "invalid data source slug: {slug}"
        )));
    }
    std::fs::create_dir_all(root)?;

    // F5: with a store, the credential fields go straight to the keyring —
    // the TOML carries only the non-secret config. A failed keyring write
    // keeps the plaintext in the file (0600) and warns; the startup
    // migration retries on the next launch.
    let (mut file_config, secrets) = split_secret_fields(config);
    if !secrets.is_empty() && store.is_none() {
        // Degraded mode: the whole form (secrets included) stays in the file.
        file_config = config.clone();
    }
    if let Some(store) = store {
        if !secrets.is_empty() {
            match secrets_payload(&secrets)
                .map_err(|e| e.to_string())
                .and_then(|raw| store.put(&crate::secret_store::datasource_key(slug), &raw))
            {
                Ok(()) => {}
                Err(e) => {
                    tracing::warn!(
                        domain = "datasource",
                        slug,
                        error = %e,
                        "keyring write failed — data source credentials stay \
                         as plaintext in the TOML (owner-only 0600)"
                    );
                    file_config = config.clone();
                }
            }
        }
    }

    let file_path = root.join(format!("{slug}.toml"));
    let body = render_toml(slug, kind, name, &file_config);
    // Whatever plaintext remains carries credentials — owner-only atomic
    // write (R6): the file is 0600 from the instant it exists.
    crate::secret_files::write_atomic_owner_only(&file_path, body.as_bytes())?;

    // The row's storage verdict mirrors what actually landed.
    let keyring_hit = matches!(
        store.map(|s| s.get(&crate::secret_store::datasource_key(slug))),
        Some(Ok(Some(_)))
    );
    let credential_storage = if keyring_hit {
        Some("keyring")
    } else if secrets.is_empty() {
        None
    } else {
        Some("plaintext_file")
    };

    let installed_at = file_metadata_rfc3339(&file_path);
    Ok(InstalledDataSource {
        slug: slug.to_string(),
        kind: kind.to_string(),
        name: name.to_string(),
        path: file_path.display().to_string(),
        installed_at,
        credential_storage,
    })
}

/// Render a TOML config body for a data source.
///
/// Format:
/// ```toml
/// [data_source]
/// slug = "obsidian-vault"
/// kind = "obsidian"
/// name = "Obsidian Vault"
/// installed_at = 2026-06-15T12:34:56Z
///
/// [config]
/// vault_path = "/home/user/MyVault"
/// ```
fn render_toml(slug: &str, kind: &str, name: &str, config: &BTreeMap<String, String>) -> String {
    let now = Utc::now().to_rfc3339();
    let mut out = String::new();
    out.push_str("[data_source]\n");
    out.push_str(&format!("slug = {}\n", toml_encode(slug)));
    out.push_str(&format!("kind = {}\n", toml_encode(kind)));
    out.push_str(&format!("name = {}\n", toml_encode(name)));
    out.push_str(&format!("installed_at = {}\n", toml_encode(&now)));
    out.push('\n');
    out.push_str("[config]\n");
    if config.is_empty() {
        out.push_str("# no fields supplied\n");
    }
    for (key, value) in config {
        out.push_str(&format!("{} = {}\n", key, toml_encode(value)));
    }
    out
}

/// Basic TOML string encoder — wraps in double quotes and escapes the few
/// characters that need it. We hand-roll this to keep the dep tree small;
/// the values come from the UI form, so they're arbitrary user strings.
fn toml_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// Scan `~/.shannon/data-sources/` for installed configs.
pub fn list_installed_data_sources() -> Vec<InstalledDataSource> {
    list_installed_data_sources_in(&shannon_data_sources_root())
}

/// `list_installed_data_sources` against an explicit `root` (see
/// [`install_data_source_in`] for why tests avoid `$HOME`).
fn list_installed_data_sources_in(root: &Path) -> Vec<InstalledDataSource> {
    list_installed_data_sources_in_with_store(root, crate::secret_store::global().as_deref())
}

/// [`list_installed_data_sources_in`] with an injected secret store (tests
/// inject a mock; `None` = degraded plaintext-only listing).
fn list_installed_data_sources_in_with_store(
    root: &Path,
    store: Option<&dyn crate::secret_store::SecretStore>,
) -> Vec<InstalledDataSource> {
    let secret_keys = super::data_source_catalog::secret_field_keys();
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        let Ok(body) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(mut parsed) = parse_data_source_toml(&body, &path) {
            // F5 (A8) per-row credential-storage verdict: keyring when the
            // store holds this source's secrets, the degraded plaintext-file
            // token when the TOML still carries a credential (pre-migration
            // or a failed keyring write), nothing for secret-less sources.
            let config = parse_config_section(&body);
            let has_plain_secret = config
                .iter()
                .any(|(k, v)| secret_keys.contains(k) && !v.is_empty());
            let keyring_hit = matches!(
                store.map(|s| s.get(&crate::secret_store::datasource_key(&parsed.slug))),
                Some(Ok(Some(_)))
            );
            parsed.credential_storage = if keyring_hit {
                Some(crate::secret_store::storage_mode(true))
            } else if has_plain_secret {
                Some(crate::secret_store::storage_mode(false))
            } else {
                None
            };
            out.push(parsed);
        }
    }
    out.sort_by(|a, b| a.slug.cmp(&b.slug));
    out
}

/// Parse a stored data source TOML file.
fn parse_data_source_toml(body: &str, path: &Path) -> Option<InstalledDataSource> {
    let slug = extract_toml_string(body, "slug")?;
    let kind = extract_toml_string(body, "kind")?;
    let name = extract_toml_string(body, "name").unwrap_or_else(|| slug.clone());
    let installed_at = file_metadata_rfc3339(path);
    Some(InstalledDataSource {
        slug,
        kind,
        name,
        path: path.display().to_string(),
        installed_at,
        // Filled in by the list path (store-aware); the bare parse is
        // store-less by construction.
        credential_storage: None,
    })
}

/// Extract the value of a `key = "value"` line from the `[data_source]`
/// section. Returns the unquoted/ unescaped value. None if not found.
fn extract_toml_string(body: &str, key: &str) -> Option<String> {
    let needle = format!("{key} = ");
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(&needle) {
            return Some(toml_decode(rest.trim()));
        }
    }
    None
}

/// Inverse of `toml_encode` — strips surrounding quotes and unescapes.
fn toml_decode(s: &str) -> String {
    let s = s.trim();
    if s.len() < 2 || !s.starts_with('"') || !s.ends_with('"') {
        return s.to_string();
    }
    let inner = &s[1..s.len() - 1];
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// RFC3339 modification time for a config file. None if unavailable.
fn file_metadata_rfc3339(path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?;
    let dur = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    DateTime::<Utc>::from_timestamp(dur.as_secs() as i64, 0).map(|dt| dt.to_rfc3339())
}

/// Remove a data source config file by slug. Refuses paths outside the
/// data-sources root as a traversal guard. F5: the source's keyring entry
/// (`shannon/datasource/<slug>`) is deleted with the file — an orphaned
/// credential nobody can see through the UI anymore would be a new leak
/// surface (best-effort: a delete failure warns, never blocks).
pub fn remove_installed_data_source(slug: &str) -> Result<(), InstallError> {
    remove_installed_data_source_in_with_store(
        &shannon_data_sources_root(),
        slug,
        crate::secret_store::global().as_deref(),
    )
}

/// [`remove_installed_data_source`] against an explicit `root` with an
/// injected secret store: the keyring entry is deleted together with the
/// file (tests inject a mock; `None` = degraded mode, nothing to clean;
/// explicit `root` keeps tests off the process-global HOME env var).
fn remove_installed_data_source_in_with_store(
    root: &Path,
    slug: &str,
    store: Option<&dyn crate::secret_store::SecretStore>,
) -> Result<(), InstallError> {
    if slug.contains('/') || slug.contains('\\') || slug.contains("..") {
        return Err(InstallError::Format(format!(
            "invalid data source slug: {slug}"
        )));
    }
    let file = root.join(format!("{slug}.toml"));
    if !file.exists() {
        return Err(InstallError::Io(format!(
            "{slug} is not installed at {}",
            file.display()
        )));
    }
    let canonical_root = root
        .canonicalize()
        .map_err(|e| InstallError::Io(format!("canonicalize root: {e}")))?;
    let canonical_target = file
        .canonicalize()
        .map_err(|e| InstallError::Io(format!("canonicalize target: {e}")))?;
    if !canonical_target.starts_with(&canonical_root) {
        return Err(InstallError::Format(format!(
            "refusing to remove path outside data-sources root: {}",
            canonical_target.display()
        )));
    }
    std::fs::remove_file(&canonical_target)?;
    crate::secret_store::delete_datasource_secret(store, slug);
    Ok(())
}

/// Best-effort "is this slug already installed?" lookup.
pub fn is_data_source_installed(slug: &str) -> bool {
    is_data_source_installed_in(&shannon_data_sources_root(), slug)
}

/// `is_data_source_installed` against an explicit `root` (see
/// [`install_data_source_in`] for why tests avoid `$HOME`).
fn is_data_source_installed_in(root: &Path, slug: &str) -> bool {
    root.join(format!("{slug}.toml")).exists()
}

/// Read the config block of an installed data source. Empty map if missing.
///
/// Used by the test-connection command and by the adapter at query time.
/// F5: credential fields resolve **keyring-first** — the keyring payload
/// (`shannon/datasource/<slug>` JSON map) wins over any plaintext leftover;
/// the TOML value is the tolerance-window fallback (and the only source in
/// degraded mode).
pub fn read_data_source_config(slug: &str) -> Result<BTreeMap<String, String>, InstallError> {
    read_data_source_config_in_with_store(
        &shannon_data_sources_root(),
        slug,
        crate::secret_store::global().as_deref(),
    )
}

/// Read the `kind` of an installed data source from its `[data_source]`
/// section. `query_data_source` dispatches on this; the config map returned
/// by [`read_data_source_config`] deliberately holds only `[config]` (the
/// install form prefills from it and must not see meta keys), so the kind
/// has to come from here.
pub fn read_data_source_kind(slug: &str) -> Result<String, InstallError> {
    read_data_source_kind_in(&shannon_data_sources_root(), slug)
}

fn read_data_source_kind_in(root: &Path, slug: &str) -> Result<String, InstallError> {
    let file = root.join(format!("{slug}.toml"));
    let body = std::fs::read_to_string(&file)
        .map_err(|e| InstallError::Io(format!("read {}: {e}", file.display())))?;
    let mut in_meta = false;
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_meta = trimmed == "[data_source]";
            continue;
        }
        if !in_meta {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("kind = ") {
            return Ok(toml_decode(rest.trim()));
        }
    }
    Err(InstallError::Io(format!(
        "missing kind in [data_source] of {}",
        file.display()
    )))
}

/// One-time, idempotent migration of data-source credential fields into the
/// OS keyring (R7-④ batch 2 / A8). Runs at startup over every
/// `data-sources/*.toml`:
///
/// 1. Keyring already holds the payload → any leftover plaintext credential
///    fields are stripped from the TOML (crash-recovery for a run
///    interrupted between the keyring put and the file rewrite).
/// 2. Otherwise, plaintext credential fields present → write the payload,
///    then rewrite the TOML without them. A **failed keyring write keeps
///    the plaintext** (owner-only 0600) and warns — loading is never
///    blocked.
///
/// Only the catalog's `password`-kind fields migrate; host/port/user/enabled
/// stay in the TOML verbatim (the file is re-rendered through the same
/// `render_toml` the installer uses, so its shape is unchanged).
///
/// Returns the number of sources whose plaintext credentials were removed
/// this run (re-runs are no-ops).
pub fn migrate_data_source_secrets() -> usize {
    migrate_data_source_secrets_in(
        &shannon_data_sources_root(),
        crate::secret_store::global().as_deref(),
    )
}

/// Store-injected core of [`migrate_data_source_secrets`] (tests pass a
/// [`MockSecretStore`](crate::secret_store::MockSecretStore); `None` = the
/// degraded plaintext fallback, a no-op).
pub fn migrate_data_source_secrets_in(
    root: &Path,
    store: Option<&dyn crate::secret_store::SecretStore>,
) -> usize {
    let Some(store) = store else {
        return 0; // degraded mode: credentials deliberately stay in the 0600 file
    };
    let Ok(entries) = std::fs::read_dir(root) else {
        return 0;
    };
    let secret_keys = super::data_source_catalog::secret_field_keys();
    let mut migrated = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        let Ok(body) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(meta) = parse_data_source_toml(&body, &path) else {
            continue;
        };
        let config = parse_config_section(&body);
        let (_, secrets) = split_secret_fields(&config);
        if secrets.is_empty() {
            continue;
        }
        let key = crate::secret_store::datasource_key(&meta.slug);
        // Crash-recovery: a keyring hit means the credential is already
        // stored — just make sure the plaintext copy is gone.
        let already_stored = matches!(store.get(&key), Ok(Some(_)));
        if !already_stored {
            let raw = match secrets_payload(&secrets) {
                Ok(raw) => raw,
                Err(e) => {
                    tracing::warn!(
                        domain = "datasource",
                        slug = %meta.slug,
                        error = %e,
                        "data-source credential serialization failed — file left as-is"
                    );
                    continue;
                }
            };
            if let Err(e) = store.put(&key, &raw) {
                tracing::warn!(
                    domain = "datasource",
                    slug = %meta.slug,
                    error = %e,
                    "keyring write failed — data source credentials stay as \
                     plaintext in the TOML (owner-only 0600)"
                );
                continue;
            }
        }
        // Rewrite the TOML without the credential fields.
        let plain: BTreeMap<String, String> = config
            .iter()
            .filter(|(k, _)| !secret_keys.contains(*k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let new_body = render_toml(&meta.slug, &meta.kind, &meta.name, &plain);
        if let Err(e) = crate::secret_files::write_atomic_owner_only(&path, new_body.as_bytes()) {
            // The keyring already holds the credential, so the next
            // startup's crash-recovery leg finishes the job. Warn loudly:
            // the plaintext copy is still on disk until then.
            tracing::warn!(
                domain = "datasource",
                slug = %meta.slug,
                error = %e,
                "post-migration TOML rewrite failed — plaintext credentials \
                 remain (0600) and will be stripped on the next startup"
            );
            continue;
        }
        migrated += 1;
    }
    if migrated > 0 {
        tracing::info!(
            domain = "datasource",
            count = migrated,
            "migrated data-source credentials into the OS keyring"
        );
    }
    migrated
}

/// [`read_data_source_config`] against an explicit `root` with an injected
/// secret store (tests inject a mock; `None` = degraded plaintext-only
/// read; explicit `root` keeps tests off the process-global HOME env var).
fn read_data_source_config_in_with_store(
    root: &Path,
    slug: &str,
    store: Option<&dyn crate::secret_store::SecretStore>,
) -> Result<BTreeMap<String, String>, InstallError> {
    let file = root.join(format!("{slug}.toml"));
    let body = std::fs::read_to_string(&file)
        .map_err(|e| InstallError::Io(format!("read {}: {e}", file.display())))?;
    let mut config = parse_config_section(&body);
    if let Some(store) = store {
        if let Some(secrets) = store
            .get(&crate::secret_store::datasource_key(slug))
            .ok()
            .flatten()
            .as_deref()
            .and_then(parse_secrets_payload)
        {
            // Keyring values win; plaintext leftovers are the fallback.
            for (key, value) in secrets {
                config.insert(key, value);
            }
        }
    }
    Ok(config)
}

fn parse_config_section(body: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut in_config = false;
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_config = trimmed == "[config]";
            continue;
        }
        if !in_config {
            continue;
        }
        if let Some(eq_idx) = trimmed.find('=') {
            let key = trimmed[..eq_idx].trim();
            let value = toml_decode(trimmed[eq_idx + 1..].trim());
            if !key.is_empty() {
                out.insert(key.to_string(), value);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // NOTE: these tests drive the `_in` variants with a tempdir instead of
    // mutating `$HOME`. The old form did `std::env::set_var("HOME", …)`, which
    // is process-global and unsynchronized — it raced with unrelated tests in
    // other modules that read `dirs::home_dir()` on another thread, causing an
    // intermittent parallel-test flake. The `_in` variants make the on-disk
    // root explicit, so no env mutation (and no `unsafe`) is needed.

    #[test]
    fn install_data_source_writes_toml_file() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        let mut config = BTreeMap::new();
        config.insert("vault_path".into(), "/home/user/MyVault".into());
        let installed = install_data_source_in_with_store(
            root,
            "obsidian-vault",
            "obsidian",
            "Obsidian Vault",
            &config,
            None,
        )
        .expect("install");
        assert_eq!(installed.slug, "obsidian-vault");
        assert_eq!(installed.kind, "obsidian");
        assert!(installed.path.ends_with("obsidian-vault.toml"));
        assert!(is_data_source_installed_in(root, "obsidian-vault"));

        let body = std::fs::read_to_string(&installed.path).unwrap();
        assert!(body.contains("[data_source]"));
        assert!(body.contains("slug = \"obsidian-vault\""));
        assert!(body.contains("[config]"));
        assert!(body.contains("vault_path = \"/home/user/MyVault\""));

        remove_installed_data_source_in_with_store(root, "obsidian-vault", None).expect("remove");
        assert!(!is_data_source_installed_in(root, "obsidian-vault"));
    }

    #[test]
    fn install_data_source_rejects_traversal_slug() {
        let tmp = tempfile::tempdir().expect("tmp");
        let result = install_data_source_in_with_store(
            tmp.path(),
            "../escape",
            "obsidian",
            "x",
            &BTreeMap::new(),
            None,
        );
        assert!(result.is_err());
    }

    #[test]
    fn install_data_source_rejects_empty_slug() {
        let tmp = tempfile::tempdir().expect("tmp");
        let result = install_data_source_in_with_store(
            tmp.path(),
            "",
            "obsidian",
            "x",
            &BTreeMap::new(),
            None,
        );
        assert!(result.is_err());
    }

    /// R6: the TOML carries plaintext credentials (IMAP password, Notion
    /// integration token) — the installed file must be owner-only (0600),
    /// including when it replaces a pre-existing world-readable file.
    #[cfg(unix)]
    #[test]
    fn install_data_source_writes_toml_0600() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        let mut config = BTreeMap::new();
        config.insert("password".into(), "hunter2".into());

        let first = install_data_source_in_with_store(
            root,
            "imap-home",
            "email_imap",
            "Home",
            &config,
            None,
        )
        .expect("install");
        let mode = |p: &str| std::fs::metadata(p).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode(&first.path), 0o600, "fresh install must be 0600");

        // A pre-existing 0644 file (e.g. from an older build) is replaced by
        // the rename and comes back 0600 — no batch migration needed.
        std::fs::set_permissions(
            std::path::Path::new(&first.path),
            std::fs::Permissions::from_mode(0o644),
        )
        .expect("set 0644");
        assert_eq!(mode(&first.path), 0o644);

        let second = install_data_source_in_with_store(
            root,
            "imap-home",
            "email_imap",
            "Home",
            &config,
            None,
        )
        .expect("reinstall");
        assert_eq!(second.path, first.path);
        assert_eq!(mode(&second.path), 0o600, "rewrite must land 0600");
    }

    #[test]
    fn list_installed_handles_missing_dir() {
        let tmp = tempfile::tempdir().expect("tmp");
        let rows = list_installed_data_sources_in(tmp.path());
        assert!(rows.is_empty());
    }

    #[test]
    fn list_installed_returns_sorted_rows() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        install_data_source_in_with_store(root, "zeta", "obsidian", "Z", &BTreeMap::new(), None)
            .unwrap();
        install_data_source_in_with_store(root, "alpha", "obsidian", "A", &BTreeMap::new(), None)
            .unwrap();
        let rows = list_installed_data_sources_in(root);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].slug, "alpha");
        assert_eq!(rows[1].slug, "zeta");
    }

    #[test]
    fn remove_rejects_missing_slug() {
        let tmp = tempfile::tempdir().expect("tmp");
        let result =
            remove_installed_data_source_in_with_store(tmp.path(), "never-installed", None);
        assert!(result.is_err());
    }

    #[test]
    fn read_config_round_trips_values() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        let mut config = BTreeMap::new();
        config.insert("imap_host".into(), "imap.example.com".into());
        config.insert("username".into(), "you@example.com".into());
        install_data_source_in_with_store(root, "email-imap", "email_imap", "Email", &config, None)
            .unwrap();
        let loaded = read_data_source_config_in_with_store(root, "email-imap", None).expect("read");
        assert_eq!(loaded.get("imap_host").unwrap(), "imap.example.com");
        assert_eq!(loaded.get("username").unwrap(), "you@example.com");
    }

    #[test]
    fn toml_encode_handles_special_chars() {
        assert_eq!(toml_encode("simple"), "\"simple\"");
        assert_eq!(toml_encode("a\"b"), "\"a\\\"b\"");
        assert_eq!(toml_encode("line\nbreak"), "\"line\\nbreak\"");
    }

    #[test]
    fn toml_decode_inverts_encode() {
        let cases = vec!["simple", "a\"b", "line\nbreak", "tab\tchar", "back\\slash"];
        for case in cases {
            let encoded = toml_encode(case);
            let decoded = toml_decode(&encoded);
            assert_eq!(decoded, case, "round-trip failed for {case:?}");
        }
    }

    #[test]
    fn parse_config_section_ignores_data_source_block() {
        let body = r#"
[data_source]
slug = "obsidian-vault"
kind = "obsidian"

[config]
vault_path = "/vault"
include_attachments = "true"
"#;
        let config = parse_config_section(body);
        assert_eq!(config.get("vault_path").unwrap(), "/vault");
        assert_eq!(config.get("include_attachments").unwrap(), "true");
        assert!(!config.contains_key("slug"));
    }

    #[test]
    fn read_data_source_kind_reads_meta_section() {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            root.path().join("obsidian-vault.toml"),
            r#"[data_source]
slug = "obsidian-vault"
kind = "obsidian"

[config]
vault_path = "/vault"
kind = "should-not-win"
"#,
        )
        .expect("write toml");
        // Section-aware: the `[config]` decoy must not win over `[data_source]`.
        assert_eq!(
            read_data_source_kind_in(root.path(), "obsidian-vault").expect("kind"),
            "obsidian"
        );
        // Missing kind → explicit error naming the file.
        std::fs::write(root.path().join("no-kind.toml"), "[config]\nx = \"1\"\n").expect("write");
        let err = read_data_source_kind_in(root.path(), "no-kind")
            .unwrap_err()
            .to_string();
        assert!(err.contains("missing kind"), "got: {err}");
    }

    // ── F5 (R7-④ batch 2 / A8): data-source credentials → OS keyring ──────

    use crate::secret_store::MockSecretStore;

    /// A "credential-less" secret set for the catalog's password-kind keys.
    fn secret_field_keys_now() -> std::collections::BTreeSet<String> {
        super::super::data_source_catalog::secret_field_keys()
    }

    /// Catalog truth: the IMAP password and the Notion integration token are
    /// among the secret-kind fields (the three plaintext faces A8 names).
    #[test]
    fn catalog_marks_imap_password_and_notion_token_as_secrets() {
        let keys = secret_field_keys_now();
        assert!(
            keys.contains("password"),
            "IMAP password must be a secret field"
        );
        assert!(
            keys.contains("integration_token"),
            "Notion integration token must be a secret field"
        );
        assert!(!keys.contains("imap_host"), "host is not a credential");
        assert!(!keys.contains("username"), "user is not a credential");
    }

    /// Acceptance ① (IMAP password): install with a working store → the
    /// password lands in the keyring (`shannon/datasource/<slug>` JSON
    /// map), the TOML keeps host/port/user but not the password, the file
    /// stays 0600, and reads resolve the password from the keyring.
    #[cfg(unix)]
    #[test]
    fn install_imap_password_moves_to_keyring_and_toml_stays_0600() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        let mut config = BTreeMap::new();
        config.insert("imap_host".into(), "imap.example.com".into());
        config.insert("imap_port".into(), "993".into());
        config.insert("username".into(), "you@example.com".into());
        config.insert("password".into(), "hunter2".into());

        let store = MockSecretStore::new();
        let installed = install_data_source_in_with_store(
            root,
            "imap-home",
            "email_imap",
            "Home",
            &config,
            Some(&store),
        )
        .expect("install");

        // Keyring payload: exactly the secret fields.
        let key = crate::secret_store::datasource_key("imap-home");
        let raw = store.value(&key).expect("password in keyring");
        let secrets: BTreeMap<String, String> = serde_json::from_str(&raw).unwrap();
        assert_eq!(secrets.get("password").map(String::as_str), Some("hunter2"));
        assert_eq!(secrets.len(), 1, "only credential fields migrate");

        // The TOML keeps the non-secret config only — and stays 0600.
        let body = std::fs::read_to_string(&installed.path).unwrap();
        assert!(body.contains("imap_host = \"imap.example.com\""), "{body}");
        assert!(body.contains("username = \"you@example.com\""), "{body}");
        assert!(
            !body.contains("hunter2"),
            "password must not remain on disk: {body}"
        );
        let mode = std::fs::metadata(&installed.path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "the fallback file shape stays owner-only");

        // The row reports where its credential lives.
        assert_eq!(installed.credential_storage, Some("keyring"));

        // Read path resolves the password from the keyring.
        let resolved =
            read_data_source_config_in_with_store(root, "imap-home", Some(&store)).unwrap();
        assert_eq!(
            resolved.get("password").map(String::as_str),
            Some("hunter2")
        );
        assert_eq!(
            resolved.get("imap_host").map(String::as_str),
            Some("imap.example.com")
        );
    }

    /// Acceptance ① (Notion token): same contract for the integration token,
    /// including the startup migration path over a pre-existing plaintext
    /// TOML (the F3-era shape): migrate → keyring payload → plaintext
    /// stripped → read back from the keyring → idempotent re-run.
    #[test]
    fn migrate_notion_token_drains_plaintext_toml() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        let mut config = BTreeMap::new();
        config.insert("database_id".into(), "abc".into());
        config.insert("integration_token".into(), "secret_123".into());
        // Seed the F3-era plaintext shape through the store-less installer.
        install_data_source_in_with_store(root, "notion-work", "notion", "Work", &config, None)
            .expect("seed plaintext install");

        let store = MockSecretStore::new();
        assert_eq!(migrate_data_source_secrets_in(root, Some(&store)), 1);

        let raw = store
            .value(&crate::secret_store::datasource_key("notion-work"))
            .expect("token in keyring after migration");
        let secrets: BTreeMap<String, String> = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            secrets.get("integration_token").map(String::as_str),
            Some("secret_123")
        );

        let body =
            std::fs::read_to_string(root.join("notion-work.toml")).expect("read migrated toml");
        assert!(
            !body.contains("secret_123"),
            "plaintext token must be gone: {body}"
        );
        assert!(
            body.contains("database_id = \"abc\""),
            "non-secret fields stay: {body}"
        );
        assert!(
            body.contains("[data_source]"),
            "meta section survives the rewrite: {body}"
        );

        // Idempotent.
        assert_eq!(migrate_data_source_secrets_in(root, Some(&store)), 0);

        // Read path (fetchers see the resolved credential).
        let resolved =
            read_data_source_config_in_with_store(root, "notion-work", Some(&store)).unwrap();
        assert_eq!(
            resolved.get("integration_token").map(String::as_str),
            Some("secret_123")
        );
    }

    /// Acceptance ③: a failed keyring write keeps the plaintext (0600) and
    /// warns; reads fall back to the TOML copy.
    #[test]
    fn failed_keyring_write_keeps_toml_plaintext_and_warns() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        let mut config = BTreeMap::new();
        config.insert("imap_host".into(), "imap.example.com".into());
        config.insert("password".into(), "hunter2".into());

        let store = MockSecretStore::failing_writes();
        let (capture, installed) = crate::secret_store::test_support::capture_warnings(|| {
            install_data_source_in_with_store(
                root,
                "imap-home",
                "email_imap",
                "Home",
                &config,
                Some(&store),
            )
        });
        let installed = installed.expect("install must not fail on a degraded keyring");
        let body = std::fs::read_to_string(&installed.path).unwrap();
        assert!(
            body.contains("hunter2"),
            "plaintext must survive the failed write: {body}"
        );
        assert_eq!(installed.credential_storage, Some("plaintext_file"));
        assert!(
            capture
                .warnings()
                .iter()
                .any(|w| w.contains("keyring write failed")),
            "degradation must be visible: {:?}",
            capture.warnings()
        );
        // The read path falls back to the plaintext copy.
        let resolved =
            read_data_source_config_in_with_store(root, "imap-home", Some(&store)).unwrap();
        assert_eq!(
            resolved.get("password").map(String::as_str),
            Some("hunter2")
        );
    }

    /// Degraded mode (`None` store): the whole form — secrets included —
    /// stays in the 0600 TOML, exactly the F3 status quo.
    #[test]
    fn install_without_store_keeps_the_f3_plaintext_shape() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        let mut config = BTreeMap::new();
        config.insert("password".into(), "hunter2".into());
        let installed = install_data_source_in_with_store(
            root,
            "imap-home",
            "email_imap",
            "Home",
            &config,
            None,
        )
        .expect("install");
        let body = std::fs::read_to_string(&installed.path).unwrap();
        assert!(body.contains("password = \"hunter2\""), "{body}");
        assert_eq!(installed.credential_storage, Some("plaintext_file"));
    }

    /// Acceptance ④ (uninstall cleanup): removing a source deletes its
    /// keyring entry with the file.
    #[test]
    fn uninstall_deletes_the_keyring_entry() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        let mut config = BTreeMap::new();
        config.insert("password".into(), "hunter2".into());
        let store = MockSecretStore::new();
        install_data_source_in_with_store(
            root,
            "imap-home",
            "email_imap",
            "Home",
            &config,
            Some(&store),
        )
        .expect("install");
        assert!(store.contains(&crate::secret_store::datasource_key("imap-home")));

        remove_installed_data_source_in_with_store(root, "imap-home", Some(&store))
            .expect("remove");
        assert!(
            !store.contains(&crate::secret_store::datasource_key("imap-home")),
            "uninstall must not orphan the keyring entry"
        );
    }

    /// The per-row credential-storage verdict on the list path: keyring
    /// rows say "keyring", a plaintext leftover (failed write) says
    /// "plaintext_file", secret-less rows say nothing.
    #[test]
    fn list_reports_credential_storage_per_row() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        let mut with_secret = BTreeMap::new();
        with_secret.insert("imap_host".into(), "h".into());
        with_secret.insert("password".into(), "p".into());
        let store = MockSecretStore::new();
        install_data_source_in_with_store(
            root,
            "alpha-imap",
            "email_imap",
            "A",
            &with_secret,
            Some(&store),
        )
        .unwrap();
        install_data_source_in_with_store(
            root,
            "vault",
            "obsidian",
            "B",
            &BTreeMap::from([("vault_path".into(), "/v".into())]),
            None,
        )
        .unwrap();
        // A plaintext leftover: keyring exists but the write failed.
        let failing = MockSecretStore::failing_writes();
        install_data_source_in_with_store(
            root,
            "beta-imap",
            "email_imap",
            "C",
            &with_secret,
            Some(&failing),
        )
        .unwrap();

        // Seed the keyring entry for alpha via a working store re-install.
        let rows = list_installed_data_sources_in_with_store(root, Some(&store));
        let alpha = rows.iter().find(|r| r.slug == "alpha-imap").unwrap();
        assert_eq!(alpha.credential_storage, Some("keyring"));
        let vault = rows.iter().find(|r| r.slug == "vault").unwrap();
        assert_eq!(
            vault.credential_storage, None,
            "secret-less rows carry no verdict"
        );
        let beta = rows.iter().find(|r| r.slug == "beta-imap").unwrap();
        assert_eq!(
            beta.credential_storage,
            Some("plaintext_file"),
            "a plaintext leftover must be visible as the degraded mode"
        );
    }
}
