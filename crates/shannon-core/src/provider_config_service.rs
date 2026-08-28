//! `ProviderConfigService` — the single semantic write path for
//! `~/.shannon/providers.toml` (ADR-0008 Decision 3 / P2-5).
//!
//! All three front-ends — the REPL (`/connect`, `/disconnect`,
//! `/model --save`), the CLI (`shannon providers add` / `remove`), and
//! the desktop (`configure()`) — route writes through this service so
//! they cannot diverge on the on-disk shape or clobber each other.
//!
//! ## Concurrency model (P2-2 S1-1 / S1-4)
//!
//! Two locks guard every write:
//! 1. **In-process** — `tokio::sync::Mutex<ProviderConfigStore>` in the
//!    desktop's `AppState` (the CLI and REPL are single-writer per
//!    invocation, so they skip this layer).
//! 2. **Cross-process** — `flock(LOCK_EX)` on a `<providers.toml>.lock`
//!    sidecar, acquired by [`ProviderConfigService::lock`].
//!
//! The desktop takes them in that order (mutex → flock); reverse order
//! deadlocks. `lock` returns a [`LockedService`] RAII guard that
//! releases the flock on drop.
//!
//! **Lock-then-reload (the lost-update fix).** A snapshot read at
//! construction time goes stale if another writer commits before this
//! service acquires the flock, so every write does
//! `lock → reload_locked → mutate → save_locked` — re-reading inside the
//! flock so each writer composes on the freshest committed state rather
//! than overwriting it. The seven bare methods (`connect` / `upsert` /
//! `disconnect` / `disconnect_by_slug` / `set_active` / `set_tier` /
//! `set_max_tokens`) bake this sequence in, so single-mutation callers
//! need no explicit locking; batched or custom read-modify-write (the
//! desktop's `configure()` arms) calls `lock` +
//! [`LockedService::reload_locked`] directly. The property is pinned by
//! `tests/provider_cross_process_consistency.rs`.
//!
//! [`crate::provider_config_store::ProviderConfigStore`] keeps its raw
//! mutators as the implementation layer the service composes over;
//! production code reaches them through the service (or a
//! `LockedService`), not directly.
//!
//! ## What changed and why
//!
//! The REPL's `/connect` used to build a fresh single-provider config and
//! `save()` it — an **overwrite** that silently dropped every other connected
//! provider (`/connect A` then `/connect B` lost `A`). The CLI's
//! `providers add` already **upserted** (merge). `ProviderConfigService::connect`
//! unifies both on the additive upsert, so the file's shape no longer depends
//! on which front-end wrote it.
//!
//! ## Scope boundary
//!
//! The service owns the lock → reload → mutate → persist sequence for one
//! user intent (connect / disconnect / set-active / set-tier /
//! set-max-tokens). It does
//! **not** own the API key (that stays in the credential store, decision A1)
//! or the running engine (callers do `apply_model_selection` +
//! `reload_credential`). Keeping persistence separate from session/runtime
//! concerns is what lets `apply_connect` be split into step functions later
//! (P3-4).

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

use shannon_engine::api::LlmProvider;
use shannon_types::provider_config::{CredentialRef, ProviderProfile, ProviderTiers, TierName};

use crate::model_registry::models_for_provider;
use crate::provider_config_store::ProviderConfigStore;
use crate::provider_resolver::{llm_provider_from_slug, llm_provider_id, llm_provider_to_kind};

/// Outcome of [`ProviderConfigService::connect`] — what the caller needs to
/// drive the live session: switch the engine to `provider` / `model_id`,
/// store the API key under `service`, and report `saved_path` to the user.
#[derive(Debug, Clone)]
pub struct ConnectedProvider {
    /// The engine provider that was connected.
    pub provider: LlmProvider,
    /// Resolved active model id (catalog default when none requested).
    pub model_id: String,
    /// Credential-store service name the profile references (== provider slug).
    pub service: String,
    /// Where `providers.toml` was written.
    pub saved_path: PathBuf,
}

/// Outcome of [`ProviderConfigService::disconnect`] /
/// [`ProviderConfigService::disconnect_by_slug`].
#[derive(Debug, Clone)]
pub struct DisconnectOutcome {
    /// `true` when there was a matching slot to remove.
    pub was_connected: bool,
    /// `true` when the removed slot was the active target. Lets callers
    /// distinguish "removed a non-active provider" from "removed the active
    /// provider and none remain" — both have `next_active: None`.
    pub was_active: bool,
    /// When disconnecting cleared the active target, the slug of the next
    /// still-connected provider to switch to (deterministic: first remaining).
    /// `None` when the active target was untouched or no providers remain.
    pub next_active: Option<String>,
    /// Where `providers.toml` was written (`None` when nothing was removed).
    pub saved_path: Option<PathBuf>,
}

/// The single semantic write path for `~/.shannon/providers.toml`.
///
/// Construct with [`ProviderConfigService::load`] for production (reads
/// `~/.shannon/providers.toml`, starts empty when absent) or
/// [`ProviderConfigService::load_at`] for tests. Every mutating method performs
/// the load-mutate-persist sequence internally, so callers cannot forget the
/// persist half.
pub struct ProviderConfigService {
    store: ProviderConfigStore,
}

impl ProviderConfigService {
    /// Load `~/.shannon/providers.toml` (or start empty) and wrap it.
    pub fn load() -> Self {
        Self {
            store: ProviderConfigStore::load_or_default(),
        }
    }

    /// Load from (and later persist to) `path`. For hermetic tests — the
    /// store pins this path so [`ProviderConfigService::connect`] and friends
    /// never touch the user's real `~/.shannon/`.
    pub fn load_at(path: &Path) -> Self {
        Self {
            store: ProviderConfigStore::load_or_default_at(path),
        }
    }

    /// Wrap a store the caller already loaded. The service persists to the
    /// store's pinned path. Lets a caller that already holds a
    /// `ProviderConfigStore` route its write through the service without a
    /// reload — the CLI's `run_providers_add` does this so the command layer
    /// has exactly one write path (ADR-0008 P2-5 step 2).
    pub fn from_store(store: ProviderConfigStore) -> Self {
        Self { store }
    }

    /// Borrow the underlying store for read-only access. Callers inside a
    /// [`LockedService`] critical section use this to read the post-reload
    /// state — the desktop's `configure()` arms identify the active
    /// provider on the freshest committed snapshot this way.
    pub fn store(&self) -> &ProviderConfigStore {
        &self.store
    }

    /// Slugs currently connected — read from the in-memory config the service
    /// holds (no extra disk read).
    pub fn connected_slugs(&self) -> HashSet<String> {
        self.store
            .config()
            .profiles
            .get("default")
            .map(|mp| mp.providers.iter().map(|p| p.id.clone()).collect())
            .unwrap_or_default()
    }

    /// Connect (upsert) a provider. This is the additive replacement for the
    /// REPL's former overwrite path — connecting a second provider no longer
    /// drops the first (ADR-0008 P2-5 / Decision 3).
    ///
    /// `model` defaults to the provider's first catalog model; `base_url`
    /// defaults to its canonical URL. `make_active` pins `active_target` at
    /// the new provider (true for REPL `/connect`; the CLI maps `--set-active`
    /// to it). When `false`, the caller's current selection is restored if it
    /// resolves to a known catalog provider; an unknown/custom previous
    /// selection is left on the new provider (rare, acceptable).
    ///
    /// Does NOT store the API key or touch the running engine — those are the
    /// caller's session concerns.
    pub fn connect(
        &mut self,
        provider: LlmProvider,
        model: Option<&str>,
        base_url: Option<&str>,
        make_active: bool,
    ) -> io::Result<ConnectedProvider> {
        let mut locked = self.lock()?;
        locked.reload_locked()?;
        locked.connect(provider, model, base_url, make_active)
    }

    /// Insert or replace a fully-built [`ProviderProfile`] — the entry point
    /// for callers that construct a profile from richer inputs than a single
    /// [`LlmProvider`] (the CLI's `providers add` with `--kind openai-compatible`,
    /// custom `--base-url`, `--extra-header`, …). Additive: other providers are
    /// kept. `make_active` pins `active_target` at the new profile (`true`
    /// preserves the CLI's current "the new provider becomes active" behavior);
    /// `false` restores the prior selection. Persists to disk.
    pub fn upsert(
        &mut self,
        profile: ProviderProfile,
        model_id: &str,
        make_active: bool,
    ) -> io::Result<PathBuf> {
        let mut locked = self.lock()?;
        locked.reload_locked()?;
        locked.upsert(profile, model_id, make_active)
    }

    /// Upsert + `active_target` handling, without persisting. Shared by
    /// [`Self::connect`] and [`Self::upsert`] so the make-active restore logic
    /// lives in one place.
    ///
    /// `upsert_profile` always repoints `active_target` at the new id. For
    /// `make_active = false` we snapshot the previous selection first and
    /// restore it after (best-effort: a custom/unknown prior slug is left on
    /// the new provider — rare, acceptable).
    fn upsert_profile_with_active(
        &mut self,
        profile: ProviderProfile,
        model_id: &str,
        make_active: bool,
    ) {
        let prev_active = if make_active {
            None
        } else {
            self.store
                .config()
                .profiles
                .get("default")
                .map(|mp| mp.active_target.clone())
        };

        self.store.upsert_profile(profile, model_id);

        if !make_active {
            if let Some(prev) = prev_active {
                if !prev.provider_id.is_empty() {
                    if let Some(prev_provider) = llm_provider_from_slug(&prev.provider_id) {
                        self.store.set_active(&prev_provider, &prev.model_id);
                    }
                }
            }
        }
    }

    /// Disconnect (remove) a provider slot. Idempotent — returns
    /// `was_connected = false` when there was nothing to remove. When the
    /// removed slot was the active selection, [`DisconnectOutcome::next_active`]
    /// names a remaining provider to switch to (the caller does the actual
    /// engine switch — a session concern).
    /// Disconnect (remove) a provider identified by its canonical engine
    /// slug. Delegates to [`Self::disconnect_by_slug`] after resolving the
    /// slug via [`llm_provider_id`]. Idempotent.
    ///
    /// Prefer this in REPL/`/disconnect` flows that start from a resolved
    /// [`LlmProvider`]; use [`Self::disconnect_by_slug`] when the caller only
    /// has the raw stored id string (e.g. the CLI's `providers remove <ID>`,
    /// where `<ID>` may be a custom slug like `glm` that does not round-trip
    /// through [`llm_provider_id`]).
    pub fn disconnect(&mut self, provider: &LlmProvider) -> io::Result<DisconnectOutcome> {
        let mut locked = self.lock()?;
        locked.reload_locked()?;
        locked.disconnect(provider)
    }

    /// Disconnect (remove) a provider identified by its **raw stored id**
    /// (the `ProviderProfile.id` string in `providers.toml`). This is the
    /// string-based entry point for callers — the CLI's `providers remove
    /// <ID>` — that know the stored id directly and must not canonicalize it
    /// (a user who ran `providers add glm` stored `id = "glm"`, which
    /// [`llm_provider_id`] would otherwise map back to `"zhipu"` and miss).
    /// Idempotent: removing an unknown slug returns `was_connected: false`
    /// and writes nothing. Persists to disk.
    pub fn disconnect_by_slug(&mut self, slug: &str) -> io::Result<DisconnectOutcome> {
        let mut locked = self.lock()?;
        locked.reload_locked()?;
        locked.disconnect_by_slug(slug)
    }

    /// Lookup + in-memory remove + `next_active` computation, **without
    /// persisting**. Used by [`LockedService::disconnect_by_slug`] (which
    /// follows with `save_locked()` because the caller already holds the
    /// flock). The bare [`Self::disconnect_by_slug`] delegates to that
    /// locked path (lock → reload → here → `save_locked`), so the
    /// disconnect semantics live in one place.
    fn disconnect_by_slug_unpersisted(&mut self, slug: &str) -> DisconnectOutcome {
        let default = self.store.config().profiles.get("default");
        let was_connected = default
            .map(|mp| mp.providers.iter().any(|p| p.id == slug))
            .unwrap_or(false);
        let was_active = default
            .map(|mp| mp.active_target.provider_id == slug)
            .unwrap_or(false);

        if !was_connected {
            return DisconnectOutcome {
                was_connected: false,
                was_active: false,
                next_active: None,
                saved_path: None,
            };
        }

        self.store.remove_profile(slug);

        // `remove_profile` clears `active_target` when it pointed at the
        // removed slot; offer the first remaining slug so the REPL/CLI can
        // switch deterministically.
        let next_active = if was_active {
            self.store
                .config()
                .profiles
                .get("default")
                .and_then(|mp| mp.providers.first().map(|p| p.id.clone()))
        } else {
            None
        };

        DisconnectOutcome {
            was_connected: true,
            was_active,
            next_active,
            saved_path: None,
        }
    }

    /// Pin `active_target` at `provider` / `model` and persist.
    pub fn set_active(&mut self, provider: &LlmProvider, model: &str) -> io::Result<PathBuf> {
        let mut locked = self.lock()?;
        locked.reload_locked()?;
        locked.set_active(provider, model)
    }

    /// Set a per-tier model override on `provider` and persist.
    pub fn set_tier(
        &mut self,
        provider: &LlmProvider,
        tier: TierName,
        model: &str,
    ) -> io::Result<PathBuf> {
        let mut locked = self.lock()?;
        locked.reload_locked()?;
        locked.set_tier(provider, tier, model)
    }

    /// Set or clear (`None`) the per-provider `default_max_tokens` and persist.
    pub fn set_max_tokens(
        &mut self,
        provider: &LlmProvider,
        max_tokens: Option<u32>,
    ) -> io::Result<PathBuf> {
        let mut locked = self.lock()?;
        locked.reload_locked()?;
        locked.set_max_tokens(provider, max_tokens)
    }

    /// Hand back the underlying store for callers that need raw access
    /// (desktop low-level paths).
    pub fn into_inner(self) -> ProviderConfigStore {
        self.store
    }

    /// Acquire an exclusive `flock` on the underlying `providers.toml`
    /// and return a [`LockedService`] that mutates + persists **without**
    /// re-acquiring the lock. The flock is released when the
    /// `LockedService` is dropped.
    ///
    /// This is the **only** public entry point that exposes the
    /// cross-process flock directly. The bare `connect` / `upsert` /
    /// `disconnect` / `disconnect_by_slug` / `set_active` / `set_tier` /
    /// `set_max_tokens` methods all route through this (lock → reload →
    /// mutate → `save_locked`), so they would deadlock (Linux: hang,
    /// macOS: `EDEADLK`) if called after `lock()` until the returned
    /// guard is dropped — use the [`LockedService`] equivalents inside a
    /// held lock.
    ///
    /// **Lock-ordering contract (P2-2 S1-1)**: when also holding a
    /// process-internal mutex on the `ProviderConfigStore` (the
    /// desktop's `AppState::provider_store: Arc<Mutex<...>>`), acquire
    /// that mutex **first**, then call `lock()`. Reverse order
    /// deadlocks against the in-process mutex.
    pub fn lock(&mut self) -> io::Result<LockedService<'_>> {
        let path = self
            .store
            .last_path()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "cannot determine providers.toml path (no home directory)",
                )
            })?
            .to_path_buf();
        let flock = crate::provider_config_store::acquire_exclusive_lock(&path)?;
        Ok(LockedService {
            svc: self,
            _flock: flock,
        })
    }
}

/// RAII handle returned by [`ProviderConfigService::lock`]. Mutates the
/// service's in-memory state and persists via [`Self::save_locked`]
/// (which does **not** re-acquire the flock — the caller already holds
/// it). Drop releases the flock.
///
/// Mirrors `tokio::sync::MutexGuard` / `std::sync::MutexGuard` shape
/// so call-sites read like nested guards:
/// ```ignore
/// let mut svc = ...;
/// let mut locked = svc.lock()?;
/// locked.upsert(...)?;
/// locked.save_locked()?;
/// # locked drops -> flock released
/// ```
pub struct LockedService<'a> {
    svc: &'a mut ProviderConfigService,
    /// RAII: `File::drop` calls `close(2)` which releases the OS
    /// `flock(LOCK_EX)` on Linux. `fs2` `File` does not auto-unlock
    /// on its own `Drop` impl — the OS-level close is what frees the
    /// lock, which is what we want for panic safety.
    _flock: std::fs::File,
}

impl<'a> LockedService<'a> {
    /// Borrow the underlying service for read-only access. The flock
    /// is held for the lifetime of this `LockedService`.
    pub fn service(&self) -> &ProviderConfigService {
        self.svc
    }

    /// Mutably borrow the underlying service. Use the mutating
    /// **non-persisting** helpers like `upsert_profile_with_active` and
    /// then call [`Self::save_locked`] to commit, or use the convenience
    /// methods below which combine mutate + persist.
    pub fn service_mut(&mut self) -> &mut ProviderConfigService {
        self.svc
    }

    /// Re-read `providers.toml` from disk into the in-memory store,
    /// applying any committed writes from other processes / front-ends.
    /// The caller already holds the flock via this guard, so the on-disk
    /// state is consistent. Call immediately after
    /// [`ProviderConfigService::lock`] (before mutating) so the
    /// subsequent mutate + save composes on the freshest committed state
    /// — the fix for the load-then-lock stale-read window that would
    /// otherwise lose updates across processes.
    ///
    /// Always returns `Ok`; see [`ProviderConfigStore::reload_locked`]
    /// for the graceful-degradation contract.
    pub fn reload_locked(&mut self) -> io::Result<()> {
        self.svc.store.reload_locked()
    }

    /// Connect (upsert) a provider and persist. The flock is held
    /// throughout — no second acquire. Additive: other connected
    /// providers are kept (ADR-0008 P2-5 Decision 3).
    pub fn connect(
        &mut self,
        provider: LlmProvider,
        model: Option<&str>,
        base_url: Option<&str>,
        make_active: bool,
    ) -> io::Result<ConnectedProvider> {
        let provider_id = llm_provider_id(&provider);
        let profile = build_profile_for_provider(&provider, base_url);
        let model_id = model
            .map(|s| s.to_string())
            .or_else(|| {
                models_for_provider(provider.clone())
                    .first()
                    .map(|m| m.id.to_string())
            })
            .unwrap_or_else(|| "default".to_string());

        self.svc
            .upsert_profile_with_active(profile, &model_id, make_active);
        let saved_path = self.svc.store.save_locked()?;
        Ok(ConnectedProvider {
            provider,
            model_id,
            service: provider_id,
            saved_path,
        })
    }

    /// Upsert a fully-built [`ProviderProfile`] and persist.
    /// `make_active` semantics match [`ProviderConfigService::upsert`].
    pub fn upsert(
        &mut self,
        profile: ProviderProfile,
        model_id: &str,
        make_active: bool,
    ) -> io::Result<PathBuf> {
        self.svc
            .upsert_profile_with_active(profile, model_id, make_active);
        self.svc.store.save_locked()
    }

    /// Disconnect (remove) a provider and persist via `save_locked` (the
    /// flock is already held by this guard). Delegates to
    /// [`ProviderConfigService::disconnect_by_slug`] semantics. Idempotent.
    pub fn disconnect(&mut self, provider: &LlmProvider) -> io::Result<DisconnectOutcome> {
        self.disconnect_by_slug(&llm_provider_id(provider))
    }

    /// Disconnect by raw stored id (the CLI `providers remove <ID>` path),
    /// persisting via `save_locked`. See
    /// [`ProviderConfigService::disconnect_by_slug`] for the slug semantics.
    pub fn disconnect_by_slug(&mut self, slug: &str) -> io::Result<DisconnectOutcome> {
        let mut outcome = self.svc.disconnect_by_slug_unpersisted(slug);
        if outcome.was_connected {
            outcome.saved_path = Some(self.svc.store.save_locked()?);
        }
        Ok(outcome)
    }

    /// Pin `active_target` at `provider` / `model` and persist.
    pub fn set_active(&mut self, provider: &LlmProvider, model: &str) -> io::Result<PathBuf> {
        self.svc.store.set_active(provider, model);
        self.svc.store.save_locked()
    }

    /// Set a per-tier model override on `provider` and persist.
    pub fn set_tier(
        &mut self,
        provider: &LlmProvider,
        tier: TierName,
        model: &str,
    ) -> io::Result<PathBuf> {
        self.svc.store.set_tier(provider, tier, model);
        self.svc.store.save_locked()
    }

    /// Set or clear the per-provider `default_max_tokens` and persist.
    pub fn set_max_tokens(
        &mut self,
        provider: &LlmProvider,
        max_tokens: Option<u32>,
    ) -> io::Result<PathBuf> {
        self.svc.store.set_default_max_tokens(provider, max_tokens);
        self.svc.store.save_locked()
    }

    /// Persist whatever is in the in-memory config to disk. **The
    /// caller MUST hold the flock** (typically via
    /// [`ProviderConfigService::lock`]); this does not re-acquire it.
    pub fn save_locked(&mut self) -> io::Result<PathBuf> {
        self.svc.store.save_locked()
    }
}

/// Build the [`ProviderProfile`] for a provider — the shared shape both
/// `connect` and (until step 4) `build_connect_profile` produce. Field values
/// mirror [`crate::provider_resolver::build_connect_profile`] exactly so the
/// on-disk shape is identical whether a provider was added via `/connect` or
/// `providers add` (ADR-0008 P2-5 test T5 guards against drift).
fn build_profile_for_provider(
    provider: &LlmProvider,
    base_url_override: Option<&str>,
) -> ProviderProfile {
    let id = llm_provider_id(provider);
    ProviderProfile {
        id: id.clone(),
        kind: llm_provider_to_kind(provider),
        display_name: id.clone(),
        base_url: base_url_override
            .map(|s| s.to_string())
            .unwrap_or_else(|| provider.default_base_url().to_string()),
        models_url: None,
        credential: CredentialRef::Store {
            service: id.clone(),
        },
        extra_headers: std::collections::HashMap::new(),
        default_max_tokens: None,
        fallback_models: Vec::new(),
        quirks: Default::default(),
        tiers: ProviderTiers::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Fresh service over a temp dir — never touches `~/.shannon/`.
    fn service() -> (ProviderConfigService, TempDir) {
        let dir = TempDir::new().expect("temp dir");
        let svc = ProviderConfigService::load_at(&dir.path().join("providers.toml"));
        (svc, dir)
    }

    #[test]
    fn connect_then_connect_keeps_both_providers() {
        // T1 — the bug fix. The old REPL overwrite path would have dropped
        // Anthropic once OpenAI was connected.
        let (mut svc, _dir) = service();
        let _ = svc
            .connect(LlmProvider::Anthropic, None, None, true)
            .unwrap();
        let _ = svc.connect(LlmProvider::OpenAI, None, None, true).unwrap();
        let connected = svc.connected_slugs();
        assert!(
            connected.contains("anthropic"),
            "anthropic must survive a second connect"
        );
        assert!(connected.contains("openai"), "openai must be connected");
    }

    #[test]
    fn connect_then_disconnect_removes_only_that_provider() {
        // T2.
        let (mut svc, _dir) = service();
        let _ = svc
            .connect(LlmProvider::Anthropic, None, None, true)
            .unwrap();
        let _ = svc.connect(LlmProvider::OpenAI, None, None, true).unwrap();

        let outcome = svc.disconnect(&LlmProvider::OpenAI).unwrap();
        assert!(outcome.was_connected);
        let connected = svc.connected_slugs();
        assert!(connected.contains("anthropic"));
        assert!(!connected.contains("openai"));
    }

    #[test]
    fn disconnect_active_returns_next_active_slug() {
        // T3 — disconnecting the active selection offers the remaining slug.
        let (mut svc, _dir) = service();
        let _ = svc
            .connect(LlmProvider::Anthropic, None, None, true)
            .unwrap();
        let _ = svc.connect(LlmProvider::OpenAI, None, None, true).unwrap();

        let outcome = svc.disconnect(&LlmProvider::OpenAI).unwrap();
        // OpenAI was made active by the second connect; a remaining slug is offered.
        assert_eq!(outcome.next_active.as_deref(), Some("anthropic"));
    }

    #[test]
    fn connect_make_active_false_preserves_prior_selection() {
        // T4 — the CLI --set-active=false path must not steal active_target.
        let (mut svc, _dir) = service();
        let _ = svc
            .connect(LlmProvider::Anthropic, None, None, true)
            .unwrap();
        let anthropic_active = svc
            .store
            .config()
            .profiles
            .get("default")
            .unwrap()
            .active_target
            .provider_id
            .clone();
        assert_eq!(anthropic_active, "anthropic");

        // Add OpenAI without making it active.
        let _ = svc.connect(LlmProvider::OpenAI, None, None, false).unwrap();
        let still_active = svc
            .store
            .config()
            .profiles
            .get("default")
            .unwrap()
            .active_target
            .provider_id
            .clone();
        assert_eq!(
            still_active, "anthropic",
            "make_active=false must preserve the prior selection"
        );
        // But OpenAI is still present.
        assert!(svc.connected_slugs().contains("openai"));
    }

    #[test]
    fn disconnect_unknown_provider_is_idempotent_noop() {
        let (mut svc, _dir) = service();
        let outcome = svc.disconnect(&LlmProvider::OpenAI).unwrap();
        assert!(!outcome.was_connected);
        assert!(!outcome.was_active);
        assert!(outcome.next_active.is_none());
        assert!(outcome.saved_path.is_none());
    }

    #[test]
    fn disconnect_by_slug_matches_raw_stored_id() {
        // The CLI `providers remove <ID>` path: <ID> is the raw stored id,
        // which for a custom provider added via `upsert` may be any string
        // (e.g. "my-gateway") that does not round-trip through
        // `llm_provider_id`. `disconnect_by_slug` must match it directly.
        let (mut svc, _dir) = service();
        let _ = svc
            .connect(LlmProvider::Anthropic, None, None, true)
            .unwrap();
        let custom = ProviderProfile {
            id: "my-gateway".into(),
            kind: shannon_types::provider_config::ProviderKind::OpenAiCompatible,
            display_name: "my-gateway".into(),
            base_url: "https://gateway.example.com/v1".into(),
            models_url: None,
            credential: CredentialRef::Store {
                service: "my-gateway".into(),
            },
            extra_headers: std::collections::HashMap::new(),
            default_max_tokens: None,
            fallback_models: Vec::new(),
            quirks: Default::default(),
            tiers: ProviderTiers::default(),
        };
        svc.upsert(custom, "gpt-4o", true).unwrap();
        // `my-gateway` became active (make_active=true); disconnecting it by
        // raw slug must report was_active and offer anthropic as next.
        let outcome = svc.disconnect_by_slug("my-gateway").unwrap();
        assert!(outcome.was_connected, "custom slug must match");
        assert!(outcome.was_active, "custom slug was the active target");
        assert_eq!(outcome.next_active.as_deref(), Some("anthropic"));
        let connected = svc.connected_slugs();
        assert!(!connected.contains("my-gateway"));
        assert!(connected.contains("anthropic"));
    }

    #[test]
    fn disconnect_by_slug_canonical_matches_disconnect_provider() {
        // disconnect_by_slug(llm_provider_id(X)) must equal disconnect(&X):
        // the provider-based path delegates to the slug-based path.
        let (mut svc_a, _dir_a) = service();
        let (mut svc_b, _dir_b) = service();
        for svc in [&mut svc_a, &mut svc_b] {
            let _ = svc
                .connect(LlmProvider::Anthropic, None, None, true)
                .unwrap();
            let _ = svc.connect(LlmProvider::OpenAI, None, None, true).unwrap();
        }
        let by_provider = svc_a.disconnect(&LlmProvider::OpenAI).unwrap();
        let by_slug = svc_b
            .disconnect_by_slug(&llm_provider_id(&LlmProvider::OpenAI))
            .unwrap();
        assert_eq!(by_provider.was_connected, by_slug.was_connected);
        assert_eq!(by_provider.was_active, by_slug.was_active);
        assert_eq!(by_provider.next_active, by_slug.next_active);
    }

    #[test]
    fn disconnect_by_slug_unknown_is_idempotent_noop() {
        let (mut svc, _dir) = service();
        let outcome = svc.disconnect_by_slug("does-not-exist").unwrap();
        assert!(!outcome.was_connected);
        assert!(!outcome.was_active);
        assert!(outcome.next_active.is_none());
        assert!(outcome.saved_path.is_none());
    }

    #[test]
    fn disconnect_non_active_reports_was_active_false() {
        // Removing a non-active provider: was_active must be false so callers
        // can skip the "no other provider remains" warning.
        let (mut svc, _dir) = service();
        let _ = svc
            .connect(LlmProvider::Anthropic, None, None, true)
            .unwrap();
        let _ = svc.connect(LlmProvider::OpenAI, None, None, true).unwrap();
        // Anthropic is NOT active (OpenAI took active). Removing anthropic:
        let outcome = svc.disconnect(&LlmProvider::Anthropic).unwrap();
        assert!(outcome.was_connected);
        assert!(!outcome.was_active, "anthropic was not the active target");
        assert!(outcome.next_active.is_none(), "active target untouched");
    }

    #[test]
    fn connect_writes_durable_rereadable_file() {
        // T7 (config half): a connect round-trips through disk so a fresh
        // load sees the provider as connected.
        let (mut svc, dir) = service();
        let path = dir.path().join("providers.toml");
        let _ = svc
            .connect(LlmProvider::Anthropic, None, None, true)
            .unwrap();
        drop(svc);

        let reloaded = ProviderConfigService::load_at(&path);
        assert!(reloaded.connected_slugs().contains("anthropic"));
    }

    #[test]
    fn connect_persists_resolved_model_as_active_target() {
        let (mut svc, _dir) = service();
        let connected = svc
            .connect(
                LlmProvider::Anthropic,
                Some("claude-sonnet-4-6"),
                None,
                true,
            )
            .unwrap();
        assert_eq!(connected.model_id, "claude-sonnet-4-6");
        let active = &svc
            .store
            .config()
            .profiles
            .get("default")
            .unwrap()
            .active_target;
        assert_eq!(active.provider_id, "anthropic");
        assert_eq!(active.model_id, "claude-sonnet-4-6");
    }

    #[test]
    fn upsert_accepts_prebuilt_profile_and_keeps_existing_providers() {
        // The CLI's `providers add` builds a richer profile than `connect`
        // (custom kind, base url, headers) and hands it to `upsert`. This
        // proves that path is additive — a prior connect survives — and that
        // the make_active=true default pins the new profile (matching the
        // CLI's historical "new provider becomes active" behavior).
        let (mut svc, _dir) = service();
        let _ = svc
            .connect(LlmProvider::Anthropic, None, None, true)
            .unwrap();

        let custom = ProviderProfile {
            id: "my-gateway".into(),
            kind: shannon_types::provider_config::ProviderKind::OpenAiCompatible,
            display_name: "my-gateway".into(),
            base_url: "https://gateway.example.com/v1".into(),
            models_url: None,
            credential: CredentialRef::Store {
                service: "my-gateway".into(),
            },
            extra_headers: std::collections::HashMap::from([("X-Foo".into(), "bar".into())]),
            default_max_tokens: None,
            fallback_models: Vec::new(),
            quirks: Default::default(),
            tiers: ProviderTiers::default(),
        };
        svc.upsert(custom, "gpt-4o", true).unwrap();

        let connected = svc.connected_slugs();
        assert!(connected.contains("anthropic"), "anthropic must survive");
        assert!(connected.contains("my-gateway"), "custom provider added");

        // make_active=true → new profile is the active target.
        let active = &svc
            .store
            .config()
            .profiles
            .get("default")
            .unwrap()
            .active_target;
        assert_eq!(active.provider_id, "my-gateway");
        assert_eq!(active.model_id, "gpt-4o");
    }

    #[test]
    fn upsert_make_active_false_preserves_prior_selection() {
        // The CLI passes make_active=true today (behavior-compat), but the
        // service exposes false so --set-active can be wired later. Guard the
        // restore logic for the prebuilt-profile path too.
        let (mut svc, _dir) = service();
        let _ = svc
            .connect(
                LlmProvider::Anthropic,
                Some("claude-sonnet-4-6"),
                None,
                true,
            )
            .unwrap();

        let custom = ProviderProfile {
            id: "openai".into(),
            kind: shannon_types::provider_config::ProviderKind::OpenAi,
            display_name: "openai".into(),
            base_url: LlmProvider::OpenAI.default_base_url().to_string(),
            models_url: None,
            credential: CredentialRef::Store {
                service: "openai".into(),
            },
            extra_headers: std::collections::HashMap::new(),
            default_max_tokens: None,
            fallback_models: Vec::new(),
            quirks: Default::default(),
            tiers: ProviderTiers::default(),
        };
        svc.upsert(custom, "gpt-4o", false).unwrap();

        assert!(svc.connected_slugs().contains("openai"));
        let active = &svc
            .store
            .config()
            .profiles
            .get("default")
            .unwrap()
            .active_target;
        assert_eq!(
            active.provider_id, "anthropic",
            "make_active=false must preserve the prior selection"
        );
    }

    // ── /connect profile shape (migrated from provider_resolver's
    // build_connect_profile tests — ADR-0008 P2-5 step 4) ──────────────
    //
    // `ProviderConfigService::connect` is now the only producer of the on-disk
    // profile shape, so the field-by-field shape + A1 (no-plaintext) checks
    // live here next to it.

    /// Find a connected provider's profile by slug from the service's
    /// in-memory config (test helper).
    fn profile_for<'a>(svc: &'a ProviderConfigService, slug: &str) -> &'a ProviderProfile {
        svc.store
            .config()
            .profiles
            .get("default")
            .and_then(|mp| mp.providers.iter().find(|p| p.id == slug))
            .unwrap_or_else(|| panic!("provider {slug} should be connected"))
    }

    #[test]
    fn connect_anthropic_uses_store_credential_and_catalog_default_model() {
        let (mut svc, _dir) = service();
        let connected = svc
            .connect(LlmProvider::Anthropic, None, None, true)
            .unwrap();
        assert_eq!(connected.provider, LlmProvider::Anthropic);
        assert_eq!(connected.service, "anthropic");
        // No model requested → the provider's first catalog model.
        assert!(
            connected.model_id.starts_with("claude-"),
            "got {}",
            connected.model_id
        );
        // Credential is a Store reference (A1: no plaintext), keyed at the slug.
        let p = profile_for(&svc, "anthropic");
        match &p.credential {
            CredentialRef::Store { service } => assert_eq!(service, "anthropic"),
            other => panic!("expected Store credential, got {other:?}"),
        }
        // Active target points at anthropic + the resolved model.
        let active = &svc
            .store
            .config()
            .profiles
            .get("default")
            .unwrap()
            .active_target;
        assert_eq!(active.provider_id, "anthropic");
        assert_eq!(active.model_id, connected.model_id);
    }

    #[test]
    fn connect_respects_explicit_model() {
        let (mut svc, _dir) = service();
        let connected = svc
            .connect(LlmProvider::OpenAI, Some("gpt-4o"), None, true)
            .unwrap();
        assert_eq!(connected.provider, LlmProvider::OpenAI);
        assert_eq!(connected.service, "openai");
        assert_eq!(connected.model_id, "gpt-4o");
    }

    #[test]
    fn connect_base_url_override_wins_over_default() {
        let (mut svc, _dir) = service();
        svc.connect(
            LlmProvider::Anthropic,
            None,
            Some("https://proxy.example.com"),
            true,
        )
        .unwrap();
        assert_eq!(
            profile_for(&svc, "anthropic").base_url,
            "https://proxy.example.com"
        );
    }

    #[test]
    fn connect_uses_provider_default_base_url_for_ollama() {
        // Ollama needs no auth, but the profile still carries a Store ref so
        // the shape is uniform (the stored value is simply empty/unused).
        let (mut svc, _dir) = service();
        let connected = svc
            .connect(LlmProvider::Ollama, Some("llama3"), None, true)
            .unwrap();
        assert_eq!(connected.service, "ollama");
        assert_eq!(connected.model_id, "llama3");
        let p = profile_for(&svc, "ollama");
        assert_eq!(p.base_url, "http://localhost:11434");
        match &p.credential {
            CredentialRef::Store { service } => assert_eq!(service, "ollama"),
            other => panic!("expected Store credential, got {other:?}"),
        }
    }

    #[test]
    fn connect_providers_toml_trips_no_secret_scanner_matches() {
        // A1 regression: the on-disk providers.toml written by the /connect
        // path (now `ProviderConfigService::connect`) carries only
        // CredentialRef::Store references (service slugs), never plaintext
        // keys. The gitleaks-derived SecretScanner must find nothing for every
        // provider with a key-shaped rule.
        use crate::team_memory_sync::SecretScanner;
        let scanner = SecretScanner::new();
        assert!(
            !scanner.rule_ids().is_empty(),
            "scanner must have default rules"
        );

        let dir = tempfile::TempDir::new().expect("temp dir");
        for provider in [
            LlmProvider::Anthropic,
            LlmProvider::OpenAI,
            LlmProvider::DeepSeek,
            LlmProvider::Zhipu,
        ] {
            let path = dir
                .path()
                .join(format!("{}.toml", llm_provider_id(&provider)));
            let mut svc = ProviderConfigService::load_at(&path);
            svc.connect(provider.clone(), None, None, true).unwrap();
            drop(svc);
            let matches = scanner
                .scan_file(&path)
                .expect("scan should read the saved file");
            assert!(
                matches.is_empty(),
                "providers.toml for {provider:?} tripped the secret scanner: {matches:?}"
            );
        }
    }

    // ===== Hand-appended block preservation (providers.toml data integrity) =====
    //
    // Regression tests for the field-reported data loss: a hand-appended
    // `[[profiles.default.providers]]` block placed after the trailing
    // `[gateway]` table must survive a semantic write when schema-valid, and
    // the write must REFUSE (leaving the file untouched) when the file fails
    // Shannon's schema — never silently destroy it. The store-level pins live
    // in `provider_config_store::tests`; these exercise the service path
    // (`connect` = lock → reload → mutate → save_locked) end to end.

    /// Canonical tool-written config + hand-appended glm-plan block after
    /// `[gateway]`, schema-valid. `connect` must keep it.
    const HAND_APPEND_VALID: &str = r#"version = 2

[profiles.default]
name = "default"
credential_scope = "shared"

[profiles.default.active_target]
provider_id = "minimax"
model_id = "MiniMax-M3"
scope = "global"

[[profiles.default.providers]]
id = "minimax"
kind = "openai-compatible"
display_name = "minimax"
base_url = "https://api.minimax.chat"

[profiles.default.providers.credential]
backend = "store"
service = "minimax"

[profiles.default.providers.quirks]
temperature_strategy = "default"
send_temperature = true

[profiles.default.providers.tiers]

[gateway]
multiplex_profiles = false
profile_routes = []

[[profiles.default.providers]]
id = "glm-plan"
kind = "openai-compatible"
display_name = "glm-plan"
base_url = "https://open.bigmodel.cn/api/paas/v4"

[profiles.default.providers.credential]
backend = "store"
service = "glm-plan"
"#;

    /// Same shape, but the hand block carries one unknown field — enough for
    /// `deny_unknown_fields` on `ProviderProfile` to reject the WHOLE file.
    const HAND_APPEND_BROKEN: &str = r#"version = 2

[profiles.default]
name = "default"
credential_scope = "shared"

[profiles.default.active_target]
provider_id = "minimax"
model_id = "MiniMax-M3"
scope = "global"

[[profiles.default.providers]]
id = "minimax"
kind = "openai-compatible"
display_name = "minimax"
base_url = "https://api.minimax.chat"

[profiles.default.providers.credential]
backend = "store"
service = "minimax"

[profiles.default.providers.quirks]
temperature_strategy = "default"
send_temperature = true

[profiles.default.providers.tiers]

[gateway]
multiplex_profiles = false
profile_routes = []

[[profiles.default.providers]]
id = "glm-plan"
kind = "openai-compatible"
display_name = "glm-plan"
base_url = "https://open.bigmodel.cn/api/paas/v4"
env_key = "ZHIPU_API_KEY"

[profiles.default.providers.credential]
backend = "store"
service = "glm-plan"
"#;

    #[test]
    fn connect_keeps_valid_hand_appended_block_after_gateway() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("providers.toml");
        std::fs::write(&path, HAND_APPEND_VALID).expect("seed hand-edited file");

        let mut svc = ProviderConfigService::load_at(&path);
        svc.connect(LlmProvider::Anthropic, None, None, true)
            .expect("connect over a hand-edited but valid file");

        let on_disk = std::fs::read_to_string(&path).expect("file exists");
        let reloaded =
            crate::provider_config_store::load(Some(&path)).expect("post-connect file must parse");
        let ids: Vec<&str> = reloaded.profiles["default"]
            .providers
            .iter()
            .map(|p| p.id.as_str())
            .collect();
        assert!(
            ids.contains(&"minimax") && ids.contains(&"glm-plan") && ids.contains(&"anthropic"),
            "hand-appended glm-plan must survive /connect; got {ids:?} in:\n{on_disk}"
        );
    }

    #[test]
    fn connect_refuses_and_preserves_file_when_unparseable() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("providers.toml");
        std::fs::write(&path, HAND_APPEND_BROKEN).expect("seed broken hand-edited file");

        let mut svc = ProviderConfigService::load_at(&path);
        let result = svc.connect(LlmProvider::Anthropic, None, None, true);
        assert!(
            result.is_err(),
            "connect must refuse to rewrite an unparseable providers.toml"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("refusing to overwrite") && err.contains("env_key"),
            "error must name the guard and the offending field; got: {err}"
        );

        // Byte-identical preservation — the user's hand edit (valid minimax
        // slot included) is untouched.
        let on_disk = std::fs::read_to_string(&path).expect("file still exists");
        assert_eq!(
            on_disk, HAND_APPEND_BROKEN,
            "a refused connect must leave the file byte-identical"
        );
    }

    // ===== P2-2 S1-1: LockedService (RAII) =====

    /// L1: `lock()` returns a usable `LockedService`; mutating through
    /// it persists without re-acquiring the flock.
    #[test]
    fn locked_connect_upserts_and_persists() {
        let (mut svc, dir) = service();
        {
            let mut locked = svc.lock().expect("lock");
            let _out = locked
                .connect(LlmProvider::Anthropic, None, None, true)
                .expect("connect through LockedService");
            // locked is still alive here; no second acquire.
        }
        // Re-read the on-disk file from the temp dir.
        let on_disk = std::fs::read_to_string(dir.path().join("providers.toml"))
            .expect("providers.toml must exist after LockedService drop");
        assert!(
            on_disk.contains("anthropic"),
            "anthropic must persist: {on_disk}"
        );
    }

    /// L2: `LockedService::disconnect` is the additive inverse of
    /// `connect` — it removes only the named provider.
    #[test]
    fn locked_disconnect_removes_only_target() {
        let (mut svc, _dir) = service();
        {
            let mut locked = svc.lock().expect("lock");
            locked
                .connect(LlmProvider::Anthropic, None, None, true)
                .unwrap();
            locked
                .connect(LlmProvider::OpenAI, None, None, true)
                .unwrap();
        }
        {
            let mut locked = svc.lock().expect("lock again");
            let outcome = locked.disconnect(&LlmProvider::OpenAI).unwrap();
            assert!(outcome.was_connected);
        }
        // Now drop locked; reopen to verify shape.
        let on_disk = std::fs::read_to_string(_dir.path().join("providers.toml")).unwrap();
        assert!(
            on_disk.contains("anthropic"),
            "anthropic survives: {on_disk}"
        );
        assert!(
            !on_disk.contains("\"id\" = \"openai\""),
            "openai removed: {on_disk}"
        );
    }

    /// L3: `lock()` fails fast (no panic, no hang) when the service
    /// has no `last_path` — i.e. the in-memory store was constructed
    /// via `from_config` without a subsequent save.
    #[test]
    fn lock_without_path_errors() {
        use crate::provider_config_store::ProviderConfigStore;
        // from_config pins no path; lock() must error before acquiring anything.
        let mut svc = ProviderConfigService::from_store(ProviderConfigStore::default());
        match svc.lock() {
            Ok(_) => panic!("lock without last_path must error"),
            Err(e) => assert_eq!(e.kind(), std::io::ErrorKind::NotFound, "got: {e:?}"),
        }
    }

    /// L4 (hazard, documented on `lock()`): the bare mutators route
    /// through `lock()` themselves, so calling one while a
    /// [`LockedService`] guard is alive re-enters the flock and
    /// deadlocks (Linux: hang; macOS: `EDEADLK`). There is no
    /// non-hanging assertion to make here, and a `#[ignore]`d test
    /// that never runs in CI provides no regression protection — the
    /// hazard is pinned by the `lock()` docstring and the bare-method
    /// routing is covered by the E2E tests in
    /// `tests/provider_cross_process_consistency.rs`. Use the
    /// `LockedService` equivalents inside a held lock.
    ///
    /// L5: two threads racing on the same service+path each
    /// acquire-release cleanly. The RAII guard must release the flock
    /// when dropped so a second thread can proceed.
    #[test]
    fn concurrent_lock_serializes_without_starvation() {
        use std::sync::{Arc, Mutex};
        use std::thread;

        let (mut svc, dir) = service();
        // Seed one provider so subsequent calls have something to do.
        svc.connect(LlmProvider::Anthropic, None, None, true)
            .unwrap();

        let svc = Arc::new(Mutex::new(svc));
        let dir_path = dir.path().join("providers.toml");
        let mut handles = Vec::new();

        for i in 0..8u32 {
            let svc = Arc::clone(&svc);
            let target = match i % 2 {
                0 => LlmProvider::OpenAI,
                _ => LlmProvider::Anthropic,
            };
            handles.push(thread::spawn(move || {
                let mut svc = svc.lock().expect("svc mutex");
                let mut locked = svc.lock().expect("flock");
                locked.connect(target, None, None, false).expect("connect");
            }));
        }
        for h in handles {
            h.join().expect("thread must not deadlock");
        }

        let on_disk = std::fs::read_to_string(&dir_path).expect("file written");
        // At least one OpenAI + the seeded Anthropic must be present.
        assert!(on_disk.contains("anthropic"));
        assert!(on_disk.contains("openai"));
    }
}
