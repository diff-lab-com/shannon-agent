//! P5 Data source installers.
//!
//! Data sources don't "install" anything from the network — they persist
//! adapter-specific config to `~/.shannon/data-sources/<slug>.toml`. The
//! frontend form prompts for the fields declared in
//! `data_source_catalog::DataSourceField`, then `install_data_source` writes
//! the file. At query time the native adapter loads the file and connects.
//!
//! For the MVP, credentials live in the same TOML file. A future iteration
//! should move secrets to the OS keychain (`InstallTarget::Keychain`).

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
}

/// Persist a data source config to disk.
///
/// `slug` becomes the file name. `config` is the user-supplied form values
/// (vault_path, imap_host, password, etc.). Each value is written into the
/// TOML file under its key.
pub fn install_data_source(
    slug: &str,
    kind: &str,
    name: &str,
    config: &BTreeMap<String, String>,
) -> Result<InstalledDataSource, InstallError> {
    if slug.trim().is_empty() {
        return Err(InstallError::Format("data source slug is required".into()));
    }
    if slug.contains('/') || slug.contains('\\') || slug.contains("..") {
        return Err(InstallError::Format(format!(
            "invalid data source slug: {slug}"
        )));
    }
    let root = shannon_data_sources_root();
    std::fs::create_dir_all(&root)?;

    let file_path = root.join(format!("{slug}.toml"));
    let body = render_toml(slug, kind, name, config);
    std::fs::write(&file_path, body)?;

    let installed_at = file_metadata_rfc3339(&file_path);
    Ok(InstalledDataSource {
        slug: slug.to_string(),
        kind: kind.to_string(),
        name: name.to_string(),
        path: file_path.display().to_string(),
        installed_at,
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
    let root = shannon_data_sources_root();
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&root) else {
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
        if let Some(parsed) = parse_data_source_toml(&body, &path) {
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
/// data-sources root as a traversal guard.
pub fn remove_installed_data_source(slug: &str) -> Result<(), InstallError> {
    if slug.contains('/') || slug.contains('\\') || slug.contains("..") {
        return Err(InstallError::Format(format!(
            "invalid data source slug: {slug}"
        )));
    }
    let root = shannon_data_sources_root();
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
    Ok(())
}

/// Best-effort "is this slug already installed?" lookup.
pub fn is_data_source_installed(slug: &str) -> bool {
    shannon_data_sources_root()
        .join(format!("{slug}.toml"))
        .exists()
}

/// Read the config block of an installed data source. Empty map if missing.
///
/// Used by the test-connection command and by the adapter at query time.
pub fn read_data_source_config(slug: &str) -> Result<BTreeMap<String, String>, InstallError> {
    let root = shannon_data_sources_root();
    let file = root.join(format!("{slug}.toml"));
    let body = std::fs::read_to_string(&file)
        .map_err(|e| InstallError::Io(format!("read {}: {e}", file.display())))?;
    Ok(parse_config_section(&body))
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
    use std::sync::{Mutex, OnceLock};

    static HOME_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    fn home_lock() -> &'static Mutex<()> {
        HOME_LOCK.get_or_init(|| Mutex::new(()))
    }

    fn lock_home() -> std::sync::MutexGuard<'static, ()> {
        home_lock().lock().unwrap_or_else(|p| p.into_inner())
    }

    #[test]
    fn install_data_source_writes_toml_file() {
        let _g = lock_home();
        let tmp = tempfile::tempdir().expect("tmp");
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }
        let mut config = BTreeMap::new();
        config.insert("vault_path".into(), "/home/user/MyVault".into());
        let installed =
            install_data_source("obsidian-vault", "obsidian", "Obsidian Vault", &config)
                .expect("install");
        assert_eq!(installed.slug, "obsidian-vault");
        assert_eq!(installed.kind, "obsidian");
        assert!(installed.path.ends_with("obsidian-vault.toml"));
        assert!(is_data_source_installed("obsidian-vault"));

        let body = std::fs::read_to_string(&installed.path).unwrap();
        assert!(body.contains("[data_source]"));
        assert!(body.contains("slug = \"obsidian-vault\""));
        assert!(body.contains("[config]"));
        assert!(body.contains("vault_path = \"/home/user/MyVault\""));

        remove_installed_data_source("obsidian-vault").expect("remove");
        assert!(!is_data_source_installed("obsidian-vault"));
    }

    #[test]
    fn install_data_source_rejects_traversal_slug() {
        let _g = lock_home();
        let tmp = tempfile::tempdir().expect("tmp");
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }
        let result = install_data_source("../escape", "obsidian", "x", &BTreeMap::new());
        assert!(result.is_err());
    }

    #[test]
    fn install_data_source_rejects_empty_slug() {
        let _g = lock_home();
        let tmp = tempfile::tempdir().expect("tmp");
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }
        let result = install_data_source("", "obsidian", "x", &BTreeMap::new());
        assert!(result.is_err());
    }

    #[test]
    fn list_installed_handles_missing_dir() {
        let _g = lock_home();
        let tmp = tempfile::tempdir().expect("tmp");
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }
        let rows = list_installed_data_sources();
        assert!(rows.is_empty());
    }

    #[test]
    fn list_installed_returns_sorted_rows() {
        let _g = lock_home();
        let tmp = tempfile::tempdir().expect("tmp");
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }
        install_data_source("zeta", "obsidian", "Z", &BTreeMap::new()).unwrap();
        install_data_source("alpha", "obsidian", "A", &BTreeMap::new()).unwrap();
        let rows = list_installed_data_sources();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].slug, "alpha");
        assert_eq!(rows[1].slug, "zeta");
    }

    #[test]
    fn remove_rejects_missing_slug() {
        let _g = lock_home();
        let tmp = tempfile::tempdir().expect("tmp");
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }
        let result = remove_installed_data_source("never-installed");
        assert!(result.is_err());
    }

    #[test]
    fn read_config_round_trips_values() {
        let _g = lock_home();
        let tmp = tempfile::tempdir().expect("tmp");
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }
        let mut config = BTreeMap::new();
        config.insert("imap_host".into(), "imap.example.com".into());
        config.insert("username".into(), "you@example.com".into());
        install_data_source("email-imap", "email_imap", "Email", &config).unwrap();
        let loaded = read_data_source_config("email-imap").expect("read");
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
}
