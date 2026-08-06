//! TOML write-back for the engine-read flat config keys (ADR-0005 Phase 4).
//!
//! `/config set` historically wrote only to `~/.shannon/config.json` — a JSON
//! key-value store the engine never reads — so changing `model` or `provider`
//! did not affect the next launch. This module round-trips
//! `~/.shannon/config.toml` (the file [`crate::unified_config`] loads via
//! `load_global_toml`) as a generic TOML document and updates one flat key, so
//! `/config set model claude-sonnet-4-…` persists where the engine will see it
//! on the next launch.
//!
//! ## Decision A1 — no secrets here
//! Only a fixed allowlist of non-secret flat keys is writable
//! ([`WRITABLE_KEYS`]). API keys and other secrets are refused at the single
//! chokepoint `writable_key_kind`: they belong in the credential store
//! (`~/.shannon/credentials/<service>.json`, written by `/connect` /
//! `/credentials`), never in a config file. Reset is safe for any key — it only
//! deletes.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use tracing::{debug, warn};

/// The non-secret flat keys `/config set` may persist to `config.toml`.
///
/// **Secrets are intentionally absent** — see the module-level A1 note.
pub const WRITABLE_KEYS: &[&str] = &[
    "model",
    "provider",
    "max_tokens",
    "temperature",
    "timeout",
    "debug",
];

/// Native TOML type a writable key carries, used by [`coerce_value`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyKind {
    String,
    Integer,
    Float,
    Bool,
}

/// `~/.shannon/config.toml` — `None` if the home directory is unknowable.
pub fn default_config_toml_path() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".shannon").join("config.toml"))
}

/// True iff `key` is one of the non-secret flat keys [`set_global_config_key`]
/// will persist. Public so callers (e.g. `/config`) can branch without
/// re-hardcoding the allowlist.
pub fn is_writable_key(key: &str) -> bool {
    writable_key_kind(key).is_some()
}

/// Map an allowlisted key to its native TOML type. Returns `None` for anything
/// outside [`WRITABLE_KEYS`] — the A1 chokepoint that keeps secrets out.
fn writable_key_kind(key: &str) -> Option<KeyKind> {
    match key {
        "model" | "provider" => Some(KeyKind::String),
        "max_tokens" | "timeout" => Some(KeyKind::Integer),
        "temperature" => Some(KeyKind::Float),
        "debug" => Some(KeyKind::Bool),
        _ => None,
    }
}

/// Coerce the textual `value` into the native TOML [`toml::Value`] for `kind`.
/// A value that fails to parse as the native type (e.g. `max_tokens = abc`)
/// falls back to a string literal rather than being silently dropped — the
/// caller already typed it via `/config set`, so this only triggers on a
/// genuine type mismatch.
fn coerce_value(kind: KeyKind, value: &str) -> toml::Value {
    match kind {
        KeyKind::String => toml::Value::String(value.to_string()),
        KeyKind::Integer => value
            .parse::<i64>()
            .map(toml::Value::Integer)
            .unwrap_or_else(|_| toml::Value::String(value.to_string())),
        KeyKind::Float => value
            .parse::<f64>()
            .map(toml::Value::Float)
            .unwrap_or_else(|_| toml::Value::String(value.to_string())),
        KeyKind::Bool => match value {
            "true" => toml::Value::Boolean(true),
            "false" => toml::Value::Boolean(false),
            _ => toml::Value::String(value.to_string()),
        },
    }
}

/// Resolve `path` to a concrete [`PathBuf`], falling back to
/// [`default_config_toml_path`]. Errors when the home directory cannot be
/// determined.
fn resolve_path(path: Option<&Path>) -> io::Result<PathBuf> {
    path.map(Path::to_path_buf)
        .or_else(default_config_toml_path)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "cannot determine home directory for config.toml",
            )
        })
}

/// Atomically write `content` to `path` via a temp file + rename so a crash
/// mid-write cannot leave a partial config. Config files are non-secret (the
/// allowlist guarantees it), so no `chmod 0600` — the file keeps default,
/// tool-readable permissions.
fn atomic_write(path: &Path, content: &str) -> io::Result<()> {
    let tmp = path.with_extension("toml.tmp");
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Set `key` = `value` in the TOML config at `path` (or
/// [`default_config_toml_path`]), creating the file and `~/.shannon/` if
/// absent. Returns the path written.
///
/// `value` is coerced to the key's native TOML type (integer/float/bool for
/// the numeric and `debug` keys, string otherwise). Non-allowlisted keys
/// (notably `api_key`) return an [`io::Error`] — secrets must go to the
/// credential store, never here (decision A1). Pre-existing TOML content is
/// preserved verbatim aside from the targeted key; if the existing file is not
/// valid TOML it is rewritten from scratch (with a warning).
pub fn set_global_config_key(path: Option<&Path>, key: &str, value: &str) -> io::Result<PathBuf> {
    let kind = writable_key_kind(key).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "'{key}' is not a writable config.toml key — secrets belong in /credentials, not config files (A1)"
            ),
        )
    })?;

    let path = resolve_path(path)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut table: toml::Table = match fs::read_to_string(&path) {
        Ok(content) => toml::from_str(&content).unwrap_or_else(|e| {
            warn!(
                path = %path.display(),
                error = %e,
                "config.toml is not valid TOML; rewriting fresh (other keys lost)"
            );
            toml::Table::new()
        }),
        Err(e) if e.kind() == io::ErrorKind::NotFound => toml::Table::new(),
        Err(e) => return Err(e),
    };

    table.insert(key.to_string(), coerce_value(kind, value));
    let content =
        toml::to_string(&table).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    atomic_write(&path, &content)?;
    debug!(path = %path.display(), key, "wrote config.toml key");
    Ok(path)
}

/// Remove `key` from the TOML config at `path` (or default). Returns `true` if
/// the key was present and removed. Unlike [`set_global_config_key`], **any**
/// key is removable — reset only deletes, so it cannot leak a secret that is
/// not already on disk.
pub fn reset_global_config_key(path: Option<&Path>, key: &str) -> io::Result<bool> {
    let path = resolve_path(path)?;
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    let mut table: toml::Table = match toml::from_str(&content) {
        Ok(t) => t,
        Err(e) => {
            warn!(
                path = %path.display(),
                error = %e,
                "config.toml is not valid TOML; nothing to reset"
            );
            return Ok(false);
        }
    };
    let existed = table.remove(key).is_some();
    if existed {
        let content =
            toml::to_string(&table).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        atomic_write(&path, &content)?;
        debug!(path = %path.display(), key, "reset config.toml key");
    }
    Ok(existed)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// A unique temp `config.toml` path so parallel nextest processes never
    /// collide on the same file.
    fn tmp_path() -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "shannon_config_persist_{}_{}.toml",
            std::process::id(),
            nanos
        ))
    }

    #[test]
    fn is_writable_key_allowlist_excludes_secrets() {
        for k in WRITABLE_KEYS {
            assert!(is_writable_key(k), "{k} should be writable");
        }
        // Secrets and anything else are refused.
        assert!(!is_writable_key("api_key"));
        assert!(!is_writable_key("anthropic_api_key"));
        assert!(!is_writable_key("shannon_api_key"));
        assert!(!is_writable_key("base_url"));
        assert!(!is_writable_key("random_unknown_key"));
    }

    #[test]
    fn set_creates_file_with_string_key() {
        let path = tmp_path();
        assert!(!path.exists());

        set_global_config_key(Some(&path), "model", "claude-sonnet-4-20250514").unwrap();

        let parsed: toml::Table = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            parsed.get("model").and_then(|v| v.as_str()),
            Some("claude-sonnet-4-20250514")
        );
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn set_round_trips_all_native_types() {
        let path = tmp_path();
        set_global_config_key(Some(&path), "model", "gpt-4o").unwrap();
        set_global_config_key(Some(&path), "provider", "openai").unwrap();
        set_global_config_key(Some(&path), "max_tokens", "8192").unwrap();
        set_global_config_key(Some(&path), "temperature", "0.7").unwrap();
        set_global_config_key(Some(&path), "timeout", "120").unwrap();
        set_global_config_key(Some(&path), "debug", "true").unwrap();

        let parsed: toml::Table = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(parsed.get("model").and_then(|v| v.as_str()), Some("gpt-4o"));
        assert_eq!(
            parsed.get("provider").and_then(|v| v.as_str()),
            Some("openai")
        );
        assert_eq!(
            parsed.get("max_tokens").and_then(|v| v.as_integer()),
            Some(8192)
        );
        assert_eq!(
            parsed.get("temperature").and_then(|v| v.as_float()),
            Some(0.7)
        );
        assert_eq!(
            parsed.get("timeout").and_then(|v| v.as_integer()),
            Some(120)
        );
        assert_eq!(parsed.get("debug").and_then(|v| v.as_bool()), Some(true));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn set_preserves_unrelated_keys_and_comments_dropped() {
        let path = tmp_path();
        fs::write(&path, "model = \"old\"\nprovider = \"anthropic\"\n").unwrap();

        set_global_config_key(Some(&path), "model", "claude-haiku-4-5-20251001").unwrap();

        let parsed: toml::Table = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            parsed.get("model").and_then(|v| v.as_str()),
            Some("claude-haiku-4-5-20251001")
        );
        // The unrelated key survives the round-trip.
        assert_eq!(
            parsed.get("provider").and_then(|v| v.as_str()),
            Some("anthropic")
        );
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn set_overwrites_existing_value() {
        let path = tmp_path();
        set_global_config_key(Some(&path), "max_tokens", "1000").unwrap();
        set_global_config_key(Some(&path), "max_tokens", "4096").unwrap();

        let parsed: toml::Table = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            parsed.get("max_tokens").and_then(|v| v.as_integer()),
            Some(4096)
        );
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn set_refuses_secret_keys() {
        let path = tmp_path();
        let err = set_global_config_key(Some(&path), "api_key", "sk-LEAK").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("A1"));
        // The file must not have been created for a refused key.
        assert!(!path.exists());
    }

    #[test]
    fn set_refuses_unknown_keys() {
        let path = tmp_path();
        let err = set_global_config_key(Some(&path), "base_url", "https://x").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(!path.exists());
    }

    #[test]
    fn set_coerces_non_numeric_integer_to_string_fallback() {
        // A non-numeric value for an integer key is stored as a string rather
        // than dropped — round-trip still yields a value.
        let path = tmp_path();
        set_global_config_key(Some(&path), "max_tokens", "lots").unwrap();
        let parsed: toml::Table = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            parsed.get("max_tokens").and_then(|v| v.as_str()),
            Some("lots")
        );
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn reset_removes_present_key_and_returns_true() {
        let path = tmp_path();
        set_global_config_key(Some(&path), "model", "x").unwrap();
        set_global_config_key(Some(&path), "provider", "anthropic").unwrap();

        let removed = reset_global_config_key(Some(&path), "model").unwrap();
        assert!(removed);

        let parsed: toml::Table = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(parsed.get("model").is_none());
        // Sibling key untouched.
        assert_eq!(
            parsed.get("provider").and_then(|v| v.as_str()),
            Some("anthropic")
        );
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn reset_missing_key_returns_false_without_writing() {
        let path = tmp_path();
        // File doesn't exist yet.
        let removed = reset_global_config_key(Some(&path), "model").unwrap();
        assert!(!removed);
        assert!(!path.exists());

        // Also when the file exists but lacks the key.
        set_global_config_key(Some(&path), "provider", "anthropic").unwrap();
        let removed = reset_global_config_key(Some(&path), "model").unwrap();
        assert!(!removed);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn reset_accepts_any_key_including_secret() {
        // Reset only deletes; a pre-existing secret key (e.g. written by an
        // older build) can be purged — this cannot leak anything new.
        let path = tmp_path();
        fs::write(&path, "api_key = \"sk-leftover\"\nmodel = \"x\"\n").unwrap();
        let removed = reset_global_config_key(Some(&path), "api_key").unwrap();
        assert!(removed);
        let parsed: toml::Table = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(parsed.get("api_key").is_none());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn set_rewrites_invalid_toml_fresh() {
        let path = tmp_path();
        fs::write(&path, "this is not = valid = toml = at all").unwrap();
        // Invalid existing content does not panic; the file is rewritten with
        // just the new key.
        set_global_config_key(Some(&path), "model", "rebuilt").unwrap();
        let parsed: toml::Table = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            parsed.get("model").and_then(|v| v.as_str()),
            Some("rebuilt")
        );
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn default_config_toml_path_under_home() {
        if let Some(p) = default_config_toml_path() {
            assert!(p.ends_with("config.toml"));
            assert!(p.to_string_lossy().contains(".shannon"));
        }
    }
}
