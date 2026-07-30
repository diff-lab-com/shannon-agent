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

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use fs2::FileExt;
use shannon_engine::api::LlmProvider;
use shannon_types::provider_config::{
    CredentialRef, ProviderKind, ProviderModelConfig, ProviderProfile, ProviderTiers,
};
use tracing::{debug, warn};

/// `~/.shannon/providers.toml`; `None` if the home directory is unknowable.
pub fn default_path() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".shannon").join("providers.toml"))
}

/// Sibling sidecar lockfile for cross-process `flock(LOCK_EX)` on
/// `providers.toml`. Chosen as `<path>.lock` so the `.shannon/` directory
/// listing stays clean (no stray `.providers.toml.lock` file), and so the
/// lock target is stable regardless of which path the store resolves to
/// (default, test override, or future per-team file).
fn lockfile_for(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".lock");
    PathBuf::from(s)
}

/// Acquire a blocking `flock(LOCK_EX)` on the sidecar lockfile for `path`,
/// creating the lockfile (and any missing parent directories) on demand.
/// Returns an RAII `File` whose drop releases the lock automatically via
/// `fs2::FileExt` — `fs2` `File` does **not** auto-unlock on drop, so the
/// caller must keep the `File` alive for the entire critical section, then
/// call `FileExt::unlock(&file)` before dropping to avoid the OS releasing
/// the lock only on `close(2)` (which can race with `read()` on Windows).
/// Acquire a blocking `flock(LOCK_EX)` on the sidecar lockfile for `path`,
/// creating the lockfile (and any missing parent directories) on demand.
/// Returns an RAII `File` whose drop releases the lock automatically via
/// `fs2::FileExt` — `fs2` `File` does **not** auto-unlock on drop, but
/// Linux `flock(2)` semantics release on `close(2)`, so dropping the guard
/// unwinds the lock. Callers that need explicit unlock can call
/// [`FileExt::unlock`] on the returned `File` directly.
pub fn acquire_exclusive_lock(path: &Path) -> io::Result<File> {
    let lock_path = lockfile_for(path);
    if let Some(parent) = lock_path.parent() {
        // Best-effort create; `OpenOptions::create` only succeeds if the
        // parent already exists, so we always run this first. Silent when
        // the parent is already there (this is the steady-state case).
        let _ = fs::create_dir_all(parent);
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)?;
    // Blocking acquire. Matches the in-process `tokio::sync::Mutex` semantic:
    // when two shannon processes (desktop + CLI) race, the loser waits
    // rather than failing fast. Saves only take milliseconds, so contention
    // is bounded; blocking preserves the "load-mutate-save" atomicity the
    // R-M-W sequences in `land_profile_in_engine_store` rely on.
    FileExt::lock_exclusive(&file)?;
    Ok(file)
}

/// Explicitly unlock + close a lockfile acquired by [`acquire_exclusive_lock`].
/// Logs (does not propagate) unlock failures — the OS will release the lock
/// on `close(2)` either way, so a stray `unlock` error is recoverable.
fn release_exclusive_lock(file: &File) {
    if let Err(e) = FileExt::unlock(file) {
        warn!(error = %e, "could not release providers.toml lock; OS will release on close");
    }
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
///
/// Acquires an exclusive cross-process `flock(LOCK_EX)` on the sidecar
/// `<path>.lock` file for the entire write so a concurrent shannon process
/// (e.g. the desktop app while the CLI runs `providers add`) cannot tear the
/// file. Blocks if another process currently holds the lock — saves only
/// take milliseconds, so contention is bounded; this matches the in-process
/// `tokio::sync::Mutex` semantic on `AppState::provider_store` so callers
/// transitioning from one to the other don't observe a behaviour change.
pub fn save(cfg: &ProviderModelConfig, path: Option<&Path>) -> io::Result<PathBuf> {
    let path = path
        .map(Path::to_path_buf)
        .or_else(default_path)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "cannot determine home directory for providers.toml",
            )
        })?;
    let lock = acquire_exclusive_lock(&path)?;
    let result = save_locked(cfg, &path);
    release_exclusive_lock(&lock);
    result
}

/// Write `cfg` to `path` **without** acquiring the cross-process flock.
/// **The caller MUST hold the flock** (via [`acquire_exclusive_lock`]) on
/// `path` (or the corresponding `<path>.lock` file) for the entire write.
/// Re-entrant on the same fd — `acquire_exclusive_lock` and `save` are the
/// safe outer wrappers; desktop callers that already wrap a load-mutate-save
/// in a flock must call `save_locked` to avoid double-locking.
fn save_locked(cfg: &ProviderModelConfig, path: &Path) -> io::Result<PathBuf> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content =
        toml::to_string_pretty(cfg).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    atomic_write_secure(path, &content)?;
    debug!(path = %path.display(), "saved v2 provider config");
    Ok(path.to_path_buf())
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

/// Editable handle around [`ProviderModelConfig`] used by the REPL's
/// `/connect` + `/model --save` flows. Holds the in-memory representation
/// while callers mutate it (`ensure_provider`, `set_tier`, ...); `save`
/// atomically writes it back to `~/.shannon/providers.toml`.
///
/// Wire-through layer between `shannon-ui` (`config.rs`) and the
/// `ProviderModelConfig` schema defined in `shannon-types`. Free functions
/// `load` / `save` still exist for read-only callers (config builders,
/// migration tooling) — this struct is the **write** path.
pub struct ProviderConfigStore {
    config: ProviderModelConfig,
    /// Cached path returned by [`ProviderConfigStore::save`]. `None` until a
    /// successful save, then pinned so subsequent saves overwrite the same
    /// file (matches `ConfigBuilder`'s load-and-merge contract).
    last_path: Option<PathBuf>,
}

impl Default for ProviderConfigStore {
    fn default() -> Self {
        Self {
            config: ProviderModelConfig::default(),
            last_path: None,
        }
    }
}

impl ProviderConfigStore {
    /// Load the persisted config from [`default_path`], or return an empty
    /// in-memory store if the file is absent / corrupt. Corrupt files degrade
    /// gracefully — same contract as the free [`load`] function.
    ///
    /// Acquires the cross-process `flock(LOCK_EX)` on the sidecar lockfile
    /// for the duration of the read so the file cannot change under us
    /// while we're parsing it. Blocks if a concurrent writer (desktop /
    /// CLI save) holds the lock — a `save` completes in milliseconds, so
    /// contention is bounded. Pair with [`Self::save`] / [`Self::save_locked`]
    /// for the matching write-side protection.
    pub fn load_or_default() -> Self {
        let path = default_path();
        let Some(path) = path else {
            // No home dir — match the old "graceful empty" contract.
            return Self {
                config: ProviderModelConfig::default(),
                last_path: None,
            };
        };
        let lock = acquire_exclusive_lock(&path).expect(
            "providers.toml lock acquisition only fails on I/O errors; \
             if the lockfile's parent dir is unwritable the user has bigger problems",
        );
        let config = load(Some(&path)).unwrap_or_default();
        release_exclusive_lock(&lock);
        Self {
            config,
            last_path: Some(path),
        }
    }

    /// Test-only: load from an explicit `path`. The production
    /// `load_or_default` uses [`default_path`] and acquires the cross-process
    /// flock; this helper exists so unit tests can point at a tempfile
    /// without touching `~/.shannon/`. Still acquires the same flock so
    /// the lock semantics match production.
    #[doc(hidden)]
    pub fn load_or_default_at(path: &Path) -> Self {
        let lock = acquire_exclusive_lock(path)
            .expect("test lock acquisition should not fail on a fresh tempfile");
        let config = load(Some(path)).unwrap_or_default();
        release_exclusive_lock(&lock);
        Self {
            config,
            last_path: Some(path.to_path_buf()),
        }
    }

    /// Build a store from a pre-populated `ProviderModelConfig`. The
    /// path is cached so the next `save()` lands at the same location
    /// `load_or_default` would have read from. Used by the desktop's
    /// one-shot `providers.json → providers.toml` migration to
    /// hand off a freshly-built config without going through the
    /// disk round-trip.
    pub fn from_config(config: ProviderModelConfig) -> Self {
        Self {
            config,
            last_path: default_path(),
        }
    }

    /// Borrow the underlying [`ProviderModelConfig`] for read-only callers
    /// (e.g. the engine layer that merges `connected` over `env vars`).
    pub fn config(&self) -> &ProviderModelConfig {
        &self.config
    }

    /// The cached providers.toml path (the path [`Self::save`] /
    /// [`Self::save_locked`] would write to). `None` only when no home
    /// directory is discoverable — i.e. the same condition that would
    /// make `save()` fail anyway. Callers that wrap a load-mutate-save
    /// critical section in a cross-process `flock` use this to acquire
    /// the lock on the same path before calling [`Self::save_locked`].
    pub fn last_path(&self) -> Option<&Path> {
        self.last_path.as_deref()
    }

    /// Get-or-create the profile for the given provider, mutating the
    /// `"default"` [`shannon_types::provider_config::ModelProfile`] in place.
    ///
    /// Returns `&mut ProviderProfile` so callers can set per-tier overrides
    /// (e.g. `/model --tier fast gpt-4o --save` writes
    /// `providers[0].tiers.fast = "gpt-4o"`). If no `default` profile exists
    /// yet, creates one with a single synthetic provider slot.
    ///
    /// `ProviderProfile` has no `Default` impl (required fields like
    /// `base_url` and `credential` have no universal default), so we
    /// synthesize the slot from the provider's known canonical base URL.
    pub fn ensure_provider(&mut self, provider: &LlmProvider) -> &mut ProviderProfile {
        let id = crate::provider_resolver::llm_provider_id(provider);
        let profile = self
            .config
            .profiles
            .entry("default".to_string())
            .or_insert_with(default_model_profile);

        if let Some(idx) = profile.providers.iter().position(|p| p.id == id) {
            return &mut profile.providers[idx];
        }
        profile.providers.push(synthesize_provider_profile(
            provider,
            &id,
            provider.default_base_url(),
        ));
        profile.providers.last_mut().expect("just pushed")
    }

    /// Set a per-tier model override on the given provider. Writes through
    /// `ensure_provider`, so callers can chain `.set_tier(...)` without
    /// checking whether the provider slot already exists.
    pub fn set_tier(
        &mut self,
        provider: &LlmProvider,
        tier: shannon_types::provider_config::TierName,
        model_id: &str,
    ) -> &mut Self {
        let profile = self.ensure_provider(provider);
        match tier {
            shannon_types::provider_config::TierName::Fast => {
                profile.tiers.fast = Some(model_id.to_string());
            }
            shannon_types::provider_config::TierName::Standard => {
                profile.tiers.standard = Some(model_id.to_string());
            }
            shannon_types::provider_config::TierName::Pro => {
                profile.tiers.pro = Some(model_id.to_string());
            }
            shannon_types::provider_config::TierName::Auto => {
                // `Auto` is input-only: the command layer resolves it to a
                // concrete tier before persisting, so `Auto` never has a
                // persisted key. Silently ignoring it here is defense-in-depth
                // against a caller that violates that contract — `save` still
                // persists the other tiers.
            }
        }
        self
    }

    /// Set the active provider + model on the `"default"` profile. The engine
    /// read-back ([`crate::provider_resolver::resolve_active_target`]) reads
    /// `active_target` — **not** the per-tier overrides written by [`set_tier`]
    /// — so calling this is what makes a `/model --tier ... --save` choice
    /// actually survive a restart (ADR-0005 Phase 4). Reuses [`ensure_provider`]
    /// so `active_target.provider_id` always points at a real entry in
    /// `providers`.
    pub fn set_active(&mut self, provider: &LlmProvider, model_id: &str) -> &mut Self {
        let id = crate::provider_resolver::llm_provider_id(provider);
        self.ensure_provider(provider);
        if let Some(profile) = self.config.profiles.get_mut("default") {
            profile.active_target.provider_id = id;
            profile.active_target.model_id = model_id.to_string();
        }
        self
    }

    /// Insert or replace a fully-built [`ProviderProfile`] under the
    /// `"default"` model profile, keyed by `profile.id`. The desktop uses
    /// this to land managed connections (e.g. two distinct
    /// `openai-compatible` endpoints like `glm` and `kimi`) without
    /// collapsing them to the engine's `OpenAI` slot — unlike
    /// [`ensure_provider`], which derives a fixed id from `&LlmProvider`
    /// and so cannot distinguish between two `openai-compatible`
    /// connections.
    ///
    /// Behavior:
    /// - If a slot with the same id exists, it is replaced wholesale
    ///   (`extra_headers`, `tiers`, `default_max_tokens`, etc. all
    ///   reflect the new profile). Callers that want to preserve
    ///   existing fields must read them out first.
    /// - If absent, the profile is appended and `active_target` is
    ///   repointed at the new id with `model_id`.
    /// - Other slots in `default.providers` are left untouched.
    ///
    /// This is the write path the desktop shell uses for managed
    /// provider connections (ADR-0005 Phase 2 task 4 — full
    /// `ProviderConnection → ProviderProfile` migration). It is **not**
    /// the REPL's path: the REPL continues to use `ensure_provider` +
    /// `set_tier` + `set_active`, which preserve its tier-override
    /// semantics.
    pub fn upsert_profile(&mut self, profile: ProviderProfile, model_id: &str) -> &mut Self {
        let profile_id = profile.id.clone();
        let model_profile = self
            .config
            .profiles
            .entry("default".to_string())
            .or_insert_with(default_model_profile);

        if let Some(idx) = model_profile
            .providers
            .iter()
            .position(|p| p.id == profile_id)
        {
            model_profile.providers[idx] = profile;
        } else {
            model_profile.providers.push(profile);
        }

        model_profile.active_target.provider_id = profile_id;
        model_profile.active_target.model_id = model_id.to_string();
        self
    }

    /// Remove the provider slot whose `id` matches `profile_id` from the
    /// `"default"` model profile. If that slot was the active target, the
    /// active pointer is cleared to `""` (the resolver will then fall
    /// back to synthesis on the next request). No-op if no slot matches
    /// — the desktop's `delete_provider` flow needs idempotence, not a
    /// 404.
    pub fn remove_profile(&mut self, profile_id: &str) -> &mut Self {
        if let Some(model_profile) = self.config.profiles.get_mut("default") {
            model_profile.providers.retain(|p| p.id != profile_id);
            if model_profile.active_target.provider_id == profile_id {
                model_profile.active_target.provider_id = String::new();
                model_profile.active_target.model_id = String::new();
            }
        }
        self
    }

    /// Atomically persist to the cached path (or [`default_path`]).
    ///
    /// Acquires the cross-process `flock(LOCK_EX)` on the sidecar lockfile
    /// for the duration of the write (see [`save`]) so a concurrent shannon
    /// process cannot tear the file. If the caller already holds the
    /// flock (e.g. `land_profile_in_engine_store` wrapping a load-mutate-save
    /// critical section), call [`Self::save_locked`] instead — re-locking on
    /// the same fd would either deadlock or stall the inner call.
    pub fn save(&self) -> io::Result<PathBuf> {
        save(&self.config, self.last_path.as_deref())
    }

    /// Write `self.config` to the cached path **without** acquiring the
    /// cross-process flock. **The caller MUST already hold the flock**
    /// on the same sidecar `<path>.lock` file for the entire write
    /// (acquired via [`acquire_exclusive_lock`], or implicitly by being
    /// inside `save` / `load_or_default`'s outer wrapper). Re-entrant on
    /// the same fd — the lock reference lives in the caller's frame, not
    /// the store's. Used by desktop command helpers that already wrap a
    /// load-mutate-save in a flock and need to avoid double-locking.
    ///
    /// # Panics
    /// If [`Self::last_path`] is `None` (no home dir discoverable + no
    /// caller-supplied path). The flock acquire would have surfaced the
    /// same problem first, so this is a "shouldn't happen" — a clear panic
    /// beats a silent skipped write.
    pub fn save_locked(&self) -> io::Result<PathBuf> {
        let path = self.last_path.as_deref().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "save_locked requires a cached path (call load_or_default first)",
            )
        })?;
        save_locked(&self.config, path)
    }

    /// Test-only: persist to an explicit path. Production callers should use
    /// `save()` so the write lands under the cached `~/.shannon/providers.toml`.
    #[doc(hidden)]
    pub fn save_at(&self, path: &Path) -> io::Result<PathBuf> {
        save(&self.config, Some(path))
    }
}

/// Coarse wire-protocol discriminator (mirrors `provider_resolver`'s private
/// helper — duplicated here so this module stays the sole owner of the
/// `ProviderConfigStore` write path).
fn llm_provider_to_kind(p: &LlmProvider) -> ProviderKind {
    use shannon_engine::api::LlmProvider as P;
    match p {
        P::Anthropic => ProviderKind::Anthropic,
        P::OpenAI => ProviderKind::OpenAi,
        P::Ollama => ProviderKind::Ollama,
        P::Gemini => ProviderKind::Gemini,
        P::DeepSeek => ProviderKind::Deepseek,
        // All other registered providers (Azure, Bedrock, Mistral, Groq,
        // Together, OpenRouter, Cohere, Fireworks, Perplexity, xAI, AI21,
        // SiliconFlow, Zhipu, Moonshot, Minimax, DashScope, Cloudflare,
        // Replicate, Custom) speak an OpenAI-compatible wire format. Fine-
        // grained identity is recovered from `base_url` at resolution time.
        _ => ProviderKind::OpenAiCompatible,
    }
}

/// Build a `ProviderProfile` from a runtime `LlmProvider`, populating the
/// fields `ProviderProfile` requires (no `Default` impl exists).
fn synthesize_provider_profile(
    provider: &LlmProvider,
    id: &str,
    base_url: &str,
) -> ProviderProfile {
    ProviderProfile {
        id: id.to_string(),
        kind: llm_provider_to_kind(provider),
        display_name: id.to_string(),
        base_url: base_url.to_string(),
        models_url: None,
        credential: CredentialRef::Store {
            service: id.to_string(),
        },
        extra_headers: HashMap::new(),
        default_max_tokens: None,
        fallback_models: Vec::new(),
        quirks: Default::default(),
        tiers: ProviderTiers::default(),
    }
}

/// Empty `ModelProfile` scaffold used when `ensure_provider` boots a fresh
/// store. The caller fills `active_target` and `providers` via
/// `ensure_provider` / direct mutation; leaving the active target blank is
/// intentional (the engine falls back to synthesis).
fn default_model_profile() -> shannon_types::provider_config::ModelProfile {
    use shannon_types::provider_config::{ActiveTarget, CredentialScope, Scope};
    shannon_types::provider_config::ModelProfile {
        name: "default".to_string(),
        active_target: ActiveTarget {
            provider_id: String::new(),
            model_id: String::new(),
            scope: Scope::Global,
        },
        providers: Vec::new(),
        auxiliary: HashMap::new(),
        credential_scope: CredentialScope::Shared,
    }
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
        std::env::temp_dir().join(format!("shannon_pcs_{}_{}.toml", std::process::id(), nanos))
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

    // ---- ProviderConfigStore (Task 17) ----
    //
    // The `ProviderConfigStore` wrapper is the **write** path that the REPL's
    // `/model --tier ... --save` command flows through. It owns a
    // `ProviderModelConfig`, supports `ensure_provider` (get-or-create the
    // provider slot) + `set_tier` (write the canonical tier override), and
    // `save` (atomic write to `~/.shannon/providers.toml`).
    //
    // These tests pin the contract the REPL relies on so a refactor can't
    // silently break the wire-through.

    #[test]
    fn store_set_tier_uses_canonical_keys_only() {
        use shannon_types::provider_config::TierName as ProviderTier;
        let mut store = ProviderConfigStore::load_or_default();
        // "haiku" / "sonnet" / "opus" are user-input aliases — the
        // persistence layer must only ever see the canonical names
        // `fast` / `standard` / `pro`.
        store.set_tier(
            &LlmProvider::Anthropic,
            ProviderTier::Fast,
            "claude-haiku-4-5",
        );
        store.set_tier(
            &LlmProvider::Anthropic,
            ProviderTier::Standard,
            "claude-sonnet-4",
        );
        store.set_tier(&LlmProvider::Anthropic, ProviderTier::Pro, "claude-opus-4");

        let profile = store.ensure_provider(&LlmProvider::Anthropic);
        assert_eq!(profile.tiers.fast.as_deref(), Some("claude-haiku-4-5"));
        assert_eq!(profile.tiers.standard.as_deref(), Some("claude-sonnet-4"));
        assert_eq!(profile.tiers.pro.as_deref(), Some("claude-opus-4"));
    }

    #[test]
    fn store_ensure_provider_is_idempotent() {
        use shannon_types::provider_config::TierName as ProviderTier;
        let mut store = ProviderConfigStore::load_or_default();
        store.set_tier(&LlmProvider::Anthropic, ProviderTier::Fast, "haiku-A");
        // Calling ensure_provider twice must NOT create a duplicate slot —
        // the second call should return the same profile we just populated.
        let p1 = store.ensure_provider(&LlmProvider::Anthropic).id.clone();
        let p2 = store.ensure_provider(&LlmProvider::Anthropic).id.clone();
        assert_eq!(p1, p2);
        assert_eq!(p1, "anthropic");
        // And there should be exactly one Anthropic slot under "default".
        let default = store
            .config()
            .profiles
            .get("default")
            .expect("default profile");
        let anthropic_count = default
            .providers
            .iter()
            .filter(|p| p.id == "anthropic")
            .count();
        assert_eq!(anthropic_count, 1, "ensure_provider must dedupe by id");
        // And the previously-set tier override must still be present.
        let anthropic = default
            .providers
            .iter()
            .find(|p| p.id == "anthropic")
            .unwrap();
        assert_eq!(anthropic.tiers.fast.as_deref(), Some("haiku-A"));
    }

    #[test]
    fn store_save_then_load_round_trips_tier_override() {
        use shannon_types::provider_config::TierName as ProviderTier;
        let path = tmp_path();

        let mut store = ProviderConfigStore::load_or_default();
        store.set_tier(
            &LlmProvider::Anthropic,
            ProviderTier::Fast,
            "claude-haiku-4-5",
        );
        store.set_tier(&LlmProvider::OpenAI, ProviderTier::Standard, "gpt-4o");
        store.save_at(&path).unwrap();

        // Re-load via the free `load` function and verify the overrides
        // survived a write+read cycle through the TOML layer.
        let loaded = load(Some(&path)).expect("should parse back");
        let default = loaded.profiles.get("default").unwrap();
        let anthropic = default
            .providers
            .iter()
            .find(|p| p.id == "anthropic")
            .expect("anthropic slot");
        assert_eq!(anthropic.tiers.fast.as_deref(), Some("claude-haiku-4-5"));
        let openai = default
            .providers
            .iter()
            .find(|p| p.id == "openai")
            .expect("openai slot");
        assert_eq!(openai.tiers.standard.as_deref(), Some("gpt-4o"));

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn store_set_tier_auto_is_ignored_silently() {
        use shannon_types::provider_config::TierName as ProviderTier;
        // The REPL rejects `--tier auto` at parse time, but `set_tier` is a
        // defense-in-depth seam: it must not silently corrupt the file by
        // writing a phantom `auto` field (the schema doesn't allow it).
        let mut store = ProviderConfigStore::load_or_default();
        store.set_tier(&LlmProvider::Anthropic, ProviderTier::Auto, "ignored");
        let profile = store.ensure_provider(&LlmProvider::Anthropic);
        assert!(profile.tiers.fast.is_none());
        assert!(profile.tiers.standard.is_none());
        assert!(profile.tiers.pro.is_none());
    }

    #[test]
    fn store_set_active_writes_active_target() {
        // set_active must populate `active_target` so the engine read-back
        // (resolve_active_target) returns the chosen model — the contract
        // that makes /model --tier ... --save survive restart.
        use crate::provider_resolver::resolve_active_target;
        let mut store = ProviderConfigStore::load_or_default();
        store.set_active(&LlmProvider::Anthropic, "claude-haiku-4-5");
        let rt = resolve_active_target(store.config()).expect("active target resolves");
        assert_eq!(rt.model_id, "claude-haiku-4-5");
        assert_eq!(rt.provider, LlmProvider::Anthropic);
    }

    #[test]
    fn store_set_active_survives_save_load_cycle() {
        // End-to-end "survives restart": persist → reload → resolve_active_target
        // returns the set_active model.
        use crate::provider_resolver::resolve_active_target;
        let path = tmp_path();
        let mut store = ProviderConfigStore::load_or_default();
        store.set_active(&LlmProvider::OpenAI, "gpt-4o");
        store.save_at(&path).unwrap();
        let loaded = load(Some(&path)).expect("should parse back");
        let rt = resolve_active_target(&loaded).expect("active resolves after reload");
        assert_eq!(rt.model_id, "gpt-4o");
        assert_eq!(rt.provider, LlmProvider::OpenAI);
        let _ = fs::remove_file(&path);
    }

    // ---- ProviderConfigStore::upsert_profile + remove_profile (Phase 2 task 4) ----
    //
    // These back the desktop's managed-provider write path. The contract
    // differs from `ensure_provider` in two ways:
    //   1. The id is supplied by the caller (e.g. "glm" for an
    //      openai-compatible slot) — `ensure_provider` would collapse
    //      "glm" and "kimi" to the same "openai" id and lose identity.
    //   2. The full profile is replaced wholesale (no merging) — the
    //      desktop treats `save_provider` as an upsert, not a
    //      tier-by-tier mutation.

    fn sample_profile(id: &str, kind: ProviderKind, base_url: &str) -> ProviderProfile {
        ProviderProfile {
            id: id.to_string(),
            kind,
            display_name: id.to_string(),
            base_url: base_url.to_string(),
            models_url: None,
            credential: CredentialRef::Store {
                service: id.to_string(),
            },
            extra_headers: HashMap::new(),
            default_max_tokens: None,
            fallback_models: Vec::new(),
            quirks: Default::default(),
            tiers: ProviderTiers::default(),
        }
    }

    #[test]
    fn upsert_profile_inserts_new_slot_and_points_active_target() {
        let mut store = ProviderConfigStore::default();
        let profile = sample_profile(
            "glm",
            ProviderKind::OpenAiCompatible,
            "https://open.bigmodel.cn/api/paas/v4",
        );

        store.upsert_profile(profile, "glm-4.6");

        let model_profile = store
            .config()
            .profiles
            .get("default")
            .expect("default model profile");
        assert_eq!(model_profile.providers.len(), 1);
        assert_eq!(model_profile.providers[0].id, "glm");
        assert_eq!(
            model_profile.providers[0].kind,
            ProviderKind::OpenAiCompatible
        );
        assert_eq!(model_profile.active_target.provider_id, "glm");
        assert_eq!(model_profile.active_target.model_id, "glm-4.6");
    }

    #[test]
    fn upsert_profile_keeps_two_openai_compatible_slots_distinct() {
        // This is the *raison d'être* of upsert_profile vs ensure_provider:
        // two managed openai-compatible connections (e.g. "glm" + "kimi")
        // must NOT collapse to a single "openai" id.
        let mut store = ProviderConfigStore::default();
        store.upsert_profile(
            sample_profile(
                "glm",
                ProviderKind::OpenAiCompatible,
                "https://open.bigmodel.cn/api/paas/v4",
            ),
            "glm-4.6",
        );
        store.upsert_profile(
            sample_profile(
                "kimi",
                ProviderKind::OpenAiCompatible,
                "https://api.moonshot.cn/v1",
            ),
            "moonshot-v1-128k",
        );

        let model_profile = store
            .config()
            .profiles
            .get("default")
            .expect("default model profile");
        assert_eq!(model_profile.providers.len(), 2);
        let ids: Vec<&str> = model_profile
            .providers
            .iter()
            .map(|p| p.id.as_str())
            .collect();
        assert!(ids.contains(&"glm"));
        assert!(ids.contains(&"kimi"));
        // Active target points at the most-recent upsert.
        assert_eq!(model_profile.active_target.provider_id, "kimi");
        assert_eq!(model_profile.active_target.model_id, "moonshot-v1-128k");
    }

    #[test]
    fn upsert_profile_replaces_existing_slot_wholesale() {
        let mut store = ProviderConfigStore::default();
        let mut profile = sample_profile(
            "glm",
            ProviderKind::OpenAiCompatible,
            "https://old.example/v1",
        );
        profile.tiers.fast = Some("glm-flash".into());
        profile.extra_headers.insert("X-Custom".into(), "v1".into());
        store.upsert_profile(profile, "glm-flash");

        // Second upsert: different tiers, different headers, different model.
        let mut new_profile = sample_profile(
            "glm",
            ProviderKind::OpenAiCompatible,
            "https://new.example/v2",
        );
        new_profile.tiers.standard = Some("glm-4.6".into());
        new_profile
            .extra_headers
            .insert("X-Custom".into(), "v2".into());
        new_profile.default_max_tokens = Some(8192);
        store.upsert_profile(new_profile, "glm-4.6");

        let model_profile = store
            .config()
            .profiles
            .get("default")
            .expect("default model profile");
        assert_eq!(
            model_profile.providers.len(),
            1,
            "second upsert must replace, not duplicate"
        );
        let glm = &model_profile.providers[0];
        assert_eq!(glm.base_url, "https://new.example/v2");
        assert_eq!(
            glm.tiers.fast, None,
            "replacement is wholesale — old tiers are gone"
        );
        assert_eq!(glm.tiers.standard.as_deref(), Some("glm-4.6"));
        assert_eq!(
            glm.extra_headers.get("X-Custom").map(String::as_str),
            Some("v2")
        );
        assert_eq!(glm.default_max_tokens, Some(8192));
    }

    #[test]
    fn upsert_profile_survives_save_load_cycle() {
        let path = tmp_path();
        let mut store = ProviderConfigStore::default();
        store.upsert_profile(
            sample_profile(
                "glm",
                ProviderKind::OpenAiCompatible,
                "https://open.bigmodel.cn/api/paas/v4",
            ),
            "glm-4.6",
        );
        store.save_at(&path).unwrap();

        let loaded = load(Some(&path)).expect("should parse back");
        use crate::provider_resolver::resolve_active_target;
        let rt = resolve_active_target(&loaded).expect("active target resolves after reload");
        // The desktop "glm" id is preserved verbatim through the TOML layer
        // (no synthesis shenanigans).
        assert_eq!(rt.profile.id, "glm");
        assert_eq!(rt.profile.kind, ProviderKind::OpenAiCompatible);
        assert_eq!(rt.model_id, "glm-4.6");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn remove_profile_drops_slot_and_clears_active_when_was_active() {
        let mut store = ProviderConfigStore::default();
        store.upsert_profile(
            sample_profile(
                "glm",
                ProviderKind::OpenAiCompatible,
                "https://open.bigmodel.cn/api/paas/v4",
            ),
            "glm-4.6",
        );
        store.upsert_profile(
            sample_profile(
                "kimi",
                ProviderKind::OpenAiCompatible,
                "https://api.moonshot.cn/v1",
            ),
            "moonshot-v1-128k",
        );
        // Re-point active at glm so the test exercises both branches.
        store.upsert_profile(
            sample_profile(
                "glm",
                ProviderKind::OpenAiCompatible,
                "https://open.bigmodel.cn/api/paas/v4",
            ),
            "glm-4.6",
        );

        store.remove_profile("glm");

        let model_profile = store
            .config()
            .profiles
            .get("default")
            .expect("default model profile");
        assert_eq!(model_profile.providers.len(), 1);
        assert_eq!(model_profile.providers[0].id, "kimi");
        assert_eq!(model_profile.active_target.provider_id, "");
        assert_eq!(model_profile.active_target.model_id, "");
    }

    #[test]
    fn remove_profile_is_idempotent_for_unknown_id() {
        let mut store = ProviderConfigStore::default();
        store.upsert_profile(
            sample_profile(
                "glm",
                ProviderKind::OpenAiCompatible,
                "https://open.bigmodel.cn/api/paas/v4",
            ),
            "glm-4.6",
        );
        // No panic, no error — the desktop delete_provider flow must be
        // idempotent against concurrent saves.
        store.remove_profile("does-not-exist");
        let model_profile = store
            .config()
            .profiles
            .get("default")
            .expect("default model profile");
        assert_eq!(model_profile.providers.len(), 1);
        assert_eq!(model_profile.providers[0].id, "glm");
    }

    // ---- Cross-process flock (ADR-0005 P3.6) ----
    //
    // The `flock(LOCK_EX)` wrapping in `ProviderConfigStore::save` /
    // `load_or_default` (and the public `acquire_exclusive_lock` helper
    // that desktop command helpers use) protects against the desktop app
    // and a concurrent `shannon` CLI invocation both loading the same
    // `~/.shannon/providers.toml`, mutating in-memory, and last-writer-
    // winning each other. The tests below pin the lock semantics:
    //   * the lockfile is auto-created on a fresh path (no panic);
    //   * the lock is released between successive `save()` calls in the
    //     same process (so the second save doesn't deadlock on itself);
    //   * two threads contending on the same path serialize — neither
    //     tears the other's write;
    //   * a `save_locked` call from inside an outer `acquire_exclusive_lock`
    //     scope does NOT deadlock.

    /// A `load_or_default_at(path)` -> mutate -> `save()` round trip on a
    /// never-before-touched path must not panic. The lockfile is created
    /// on the fly by `acquire_exclusive_lock`.
    #[test]
    fn load_or_default_does_not_panic_when_lockfile_missing() {
        let dir = std::env::temp_dir().join(format!(
            "shannon_pcs_lockmiss_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("providers.toml");
        let lock_path = dir.join("providers.toml.lock");
        assert!(!lock_path.exists(), "lockfile must not pre-exist");

        // load_or_default_at creates the lockfile as a side effect.
        let mut store = ProviderConfigStore::load_or_default_at(&path);
        assert!(
            lock_path.exists(),
            "lockfile must be created on first acquire"
        );
        store.upsert_profile(
            sample_profile(
                "anthropic",
                ProviderKind::Anthropic,
                "https://api.anthropic.com",
            ),
            "claude-sonnet-4-20250514",
        );
        // save() acquires the same flock and writes — must not deadlock
        // even though the lockfile existed from load_or_default_at.
        store.save().unwrap();

        // And the file actually round-trips.
        let loaded = load(Some(&path)).expect("should parse back");
        assert!(
            loaded.profiles["default"]
                .providers
                .iter()
                .any(|p| p.id == "anthropic")
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// Two consecutive `save()` calls in the same process must both
    /// succeed — the first save releases the flock before returning, so
    /// the second acquire doesn't block on the same fd. This is the
    /// "save_acquires_and_releases_flock" contract.
    #[test]
    fn save_acquires_and_releases_flock() {
        let dir = std::env::temp_dir().join(format!(
            "shannon_pcs_lockrel_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("providers.toml");

        let mut store = ProviderConfigStore::load_or_default_at(&path);
        store.upsert_profile(
            sample_profile(
                "anthropic",
                ProviderKind::Anthropic,
                "https://api.anthropic.com",
            ),
            "claude-sonnet-4-20250514",
        );
        store.save().expect("first save must succeed");

        store.upsert_profile(
            sample_profile("openai", ProviderKind::OpenAi, "https://api.openai.com"),
            "gpt-4o",
        );
        // If the first save failed to release the flock, this would
        // block until timeout / EDEADLK. nextest's per-test timeout would
        // catch it; we also assert the result is `Ok`.
        store
            .save()
            .expect("second save must succeed (lock was released)");

        // Both writes landed.
        let loaded = load(Some(&path)).expect("should parse back");
        let ids: Vec<&str> = loaded.profiles["default"]
            .providers
            .iter()
            .map(|p| p.id.as_str())
            .collect();
        assert!(ids.contains(&"anthropic"));
        assert!(ids.contains(&"openai"));

        let _ = fs::remove_dir_all(&dir);
    }

    /// Two threads, each doing the canonical load-mutate-save sequence
    /// wrapped in an exclusive `flock` (the same R-M-W pattern as
    /// `land_profile_in_engine_store`), must observe each other's writes
    /// — neither thread's load can miss the other's already-committed
    /// upserts. Without the flock, a concurrent `save` could land between
    /// our load and our `save_locked`, silently clobbering the other
    /// thread's profile (last-writer-wins per write). With the flock,
    /// the *entire* R-M-W section is serialized: thread A's full load →
    /// upsert → save_locked sequence completes before thread B's load
    /// sees the file, so B's load picks up A's writes.
    #[test]
    fn flock_serializes_concurrent_rmw_in_two_threads() {
        use std::sync::Arc;
        use std::thread;

        let dir = std::env::temp_dir().join(format!(
            "shannon_pcs_lock2t_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = Arc::new(dir.join("providers.toml"));

        // Seed the file so both threads start from the same baseline.
        {
            let mut seed = ProviderConfigStore::load_or_default_at(&path);
            seed.upsert_profile(
                sample_profile("seed", ProviderKind::Anthropic, "https://api.anthropic.com"),
                "claude-haiku-4-5",
            );
            seed.save().unwrap();
        }

        // Canonical desktop-pattern R-M-W: acquire flock → load →
        // mutate → save_locked → release flock. This is the exact
        // critical section that `land_profile_in_engine_store` runs.
        let p1 = Arc::clone(&path);
        let p2 = Arc::clone(&path);
        let h1 = thread::spawn(move || {
            for i in 0..5 {
                let _flock = acquire_exclusive_lock(&p1).expect("A lock");
                let mut s = ProviderConfigStore::load_or_default_at(&p1);
                s.upsert_profile(
                    sample_profile(
                        &format!("thread-A-{i}"),
                        ProviderKind::Anthropic,
                        "https://api.anthropic.com",
                    ),
                    "claude-sonnet-4-20250514",
                );
                s.save_locked().expect("A save_locked must succeed");
            }
        });
        let h2 = thread::spawn(move || {
            for i in 0..5 {
                let _flock = acquire_exclusive_lock(&p2).expect("B lock");
                let mut s = ProviderConfigStore::load_or_default_at(&p2);
                s.upsert_profile(
                    sample_profile(
                        &format!("thread-B-{i}"),
                        ProviderKind::OpenAi,
                        "https://api.openai.com",
                    ),
                    "gpt-4o",
                );
                s.save_locked().expect("B save_locked must succeed");
            }
        });
        h1.join().expect("thread A must not panic");
        h2.join().expect("thread B must not panic");

        // The flock-guarded R-M-W means every committed write was visible
        // to the OTHER thread's NEXT load — so all 10 profiles land.
        // Without the flock, last-writer-wins per write could lose some
        // profiles entirely.
        let loaded = load(Some(&path)).expect("should parse back");
        let ids: Vec<String> = loaded.profiles["default"]
            .providers
            .iter()
            .map(|p| p.id.clone())
            .collect();
        for i in 0..5 {
            assert!(
                ids.contains(&format!("thread-A-{i}")),
                "thread A's profile {i} must be on disk; got {ids:?}"
            );
            assert!(
                ids.contains(&format!("thread-B-{i}")),
                "thread B's profile {i} must be on disk; got {ids:?}"
            );
        }

        let _ = fs::remove_dir_all(&dir);
    }

    /// `save_locked` from inside an outer `acquire_exclusive_lock` scope
    /// must NOT deadlock. This is the contract the desktop's
    /// `land_profile_in_engine_store` / `remove_profile_from_engine_store`
    /// rely on — they wrap a R-M-W in the flock, then call
    /// `save_locked` (NOT `save`) so the inner acquire-then-call doesn't
    /// re-lock the same fd.
    #[test]
    fn save_locked_does_not_deadlock_inside_outer_lock() {
        let dir = std::env::temp_dir().join(format!(
            "shannon_pcs_locked_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("providers.toml");

        let mut store = ProviderConfigStore::load_or_default_at(&path);
        store.upsert_profile(
            sample_profile(
                "anthropic",
                ProviderKind::Anthropic,
                "https://api.anthropic.com",
            ),
            "claude-sonnet-4-20250514",
        );

        // Manually wrap the save in the same lock `save` would acquire —
        // this is exactly what the desktop command helpers do. If
        // `save_locked` is misnamed and tries to re-acquire, this hangs
        // / EDEADLKs.
        let _flock = acquire_exclusive_lock(&path).expect("outer lock");
        store
            .save_locked()
            .expect("save_locked inside outer flock must succeed");

        // Verify the write landed (the outer lock guard is still alive;
        // the file is committed because `save_locked` skips atomic-rename
        // only via the explicit path).
        let loaded = load(Some(&path)).expect("should parse back");
        assert!(
            loaded.profiles["default"]
                .providers
                .iter()
                .any(|p| p.id == "anthropic")
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
