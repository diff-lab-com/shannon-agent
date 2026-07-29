//! On-disk persistence for the v2 [`ProviderModelConfig`] (ADR-0005 Phase 4).
//!
//! The canonical provider/model/credential profile written by `/connect` and
//! read by the engine at launch so a connected provider survives restart with
//! **no environment variables**. Lives at `~/.shannon/providers.toml` (TOML,
//! `0600`, atomic writes).
//!
//! ## Decision A1 — no plaintext here
//! The profile carries only a [`CredentialRef::Store { service }`] reference;
//! the secret itself lives solely in the credential store
//! (`~/.shannon/credentials/<service>.json`, `0600`), written by `/connect`
//! via [`crate::credential_manager`]. `providers.toml` never holds an API key.
//!
//! ## Engine precedence
//! [`unified_config::ConfigBuilder`] loads this file into a `connected` layer
//! merged as **CLI overrides > connected > env vars > flat config.toml**, so a
//! connected profile wins over ambient `SHANNON_*` env vars (the `/connect`
//! "works without env vars" contract) while `--provider`/`--model` still
//! override a single invocation.
//!
//! [`CredentialRef::Store { service }`]: shannon_types::provider_config::CredentialRef::Store
//! [`unified_config::ConfigBuilder`]: crate::unified_config::ConfigBuilder

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use shannon_types::provider_config::ProviderModelConfig;
use tracing::{debug, warn};

/// `~/.shannon/providers.toml`; `None` if the home directory is unknowable.
pub fn default_path() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".shannon").join("providers.toml"))
}

/// Load the v2 provider config from `path` (or [`default_path`]). Returns
/// `None` when the file is absent, and **logs + returns `None` on a parse
/// error** so a corrupt file never blocks launch — the synthesis fallback
/// takes over instead.
pub fn load(path: Option<&Path>) -> Option<ProviderModelConfig> {
    let path = path.map(Path::to_path_buf).or_else(default_path)?;
    let content = fs::read_to_string(&path).ok()?;
    match toml::from_str::<ProviderModelConfig>(&content) {
        Ok(cfg) => {
            debug!(path = %path.display(), "loaded v2 provider config");
            Some(cfg)
        }
        Err(e) => {
            warn!(
                path = %path.display(),
                error = %e,
                "providers.toml unreadable; ignoring and falling back to synthesis"
            );
            None
        }
    }
}

/// Atomically persist `cfg` to `path` (or [`default_path`]), creating parent
/// directories and setting owner-only (`0600`) permissions. Returns the path
/// written.
///
/// The temp-file + `chmod 0600` + `rename` sequence means the final path is
/// never observable in a world-readable state and a crash mid-write cannot
/// leave a partial profile (a stale `<name>.toml.tmp` is harmless and ignored
/// by [`load`]).
pub fn save(cfg: &ProviderModelConfig, path: Option<&Path>) -> io::Result<PathBuf> {
    let path = path.map(Path::to_path_buf).or_else(default_path).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "cannot determine home directory for providers.toml",
        )
    })?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = toml::to_string_pretty(cfg)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    atomic_write_secure(&path, &content)?;
    debug!(path = %path.display(), "saved v2 provider config");
    Ok(path)
}

/// Atomic, owner-only write (mirrors [`crate::credential_manager`]'s pattern).
fn atomic_write_secure(path: &Path, content: &str) -> io::Result<()> {
    let tmp = path.with_extension("toml.tmp");
    fs::write(&tmp, content)?;
    #[cfg(unix)]
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use shannon_types::provider_config::{
        ActiveTarget, CredentialRef, CredentialScope, ModelProfile, ProviderKind, ProviderProfile,
        ProviderTiers, Scope,
    };
    use std::collections::HashMap;

    /// A unique temp path so parallel nextest processes never collide.
    fn tmp_path() -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "shannon_pcs_{}_{}.toml",
            std::process::id(),
            nanos
        ))
    }

    fn anthropic_connect_config() -> ProviderModelConfig {
        let profile = ProviderProfile {
            id: "anthropic".to_string(),
            kind: ProviderKind::Anthropic,
            display_name: "anthropic".to_string(),
            base_url: "https://api.anthropic.com".to_string(),
            models_url: None,
            credential: CredentialRef::Store {
                service: "anthropic".to_string(),
            },
            extra_headers: HashMap::new(),
            default_max_tokens: None,
            fallback_models: Vec::new(),
            quirks: Default::default(),
            tiers: ProviderTiers::default(),
        };
        let mut profiles = HashMap::new();
        profiles.insert(
            "default".to_string(),
            ModelProfile {
                name: "default".to_string(),
                active_target: ActiveTarget {
                    provider_id: "anthropic".to_string(),
                    model_id: "claude-sonnet-4-20250514".to_string(),
                    scope: Scope::Global,
                },
                providers: vec![profile],
                auxiliary: HashMap::new(),
                credential_scope: CredentialScope::Shared,
            },
        );
        ProviderModelConfig {
            version: ProviderModelConfig::VERSION,
            profiles,
            gateway: Default::default(),
        }
    }

    #[test]
    fn save_then_load_round_trips_store_credential() {
        let path = tmp_path();
        let original = anthropic_connect_config();

        save(&original, Some(&path)).unwrap();
        assert!(path.exists());
        // No leftover temp file.
        assert!(!path.with_extension("toml.tmp").exists());

        let loaded = load(Some(&path)).expect("should parse back");
        assert_eq!(loaded, original);

        // The credential survives as a Store reference — never plaintext.
        let default = loaded.profiles.get("default").unwrap();
        let active = &default.active_target;
        assert_eq!(active.provider_id, "anthropic");
        assert_eq!(active.model_id, "claude-sonnet-4-20250514");
        let cred = &default.providers[0].credential;
        match cred {
            CredentialRef::Store { service } => assert_eq!(service, "anthropic"),
            other => panic!("expected Store credential, got {other:?}"),
        }
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn save_sets_owner_only_permissions_on_unix() {
        let path = tmp_path();
        save(&anthropic_connect_config(), Some(&path)).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "providers.toml must be owner-only (0600)");
        }
        #[cfg(not(unix))]
        {
            let _ = path;
        }
    }

    #[test]
    fn load_returns_none_when_file_absent() {
        let path = tmp_path();
        assert!(load(Some(&path)).is_none());
    }

    #[test]
    fn load_returns_none_and_logs_on_corrupt_file() {
        // A corrupt file must never block launch — load degrades to None so
        // the engine falls back to synthesis.
        let path = tmp_path();
        fs::write(&path, "this is = not = valid toml").unwrap();
        assert!(load(Some(&path)).is_none());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn save_creates_parent_directory() {
        let dir = std::env::temp_dir().join(format!(
            "shannon_pcs_dir_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let path = dir.join("nested/providers.toml");
        save(&anthropic_connect_config(), Some(&path)).unwrap();
        assert!(path.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn default_path_under_shannon_home() {
        if let Some(p) = default_path() {
            assert!(p.ends_with("providers.toml"));
            assert!(p.to_string_lossy().contains(".shannon"));
        }
    }
}
