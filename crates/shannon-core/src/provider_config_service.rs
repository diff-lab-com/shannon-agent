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
use shannon_types::provider_config::{
    CredentialRef, ProviderModelConfig, ProviderProfile, ProviderTiers, TierName,
    validate_profile_name,
};

use crate::model_registry::models_for_provider;
use crate::provider_config_store::{ImportConflict, ProviderConfigStore};
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

/// R3-2: one named model profile in the `/profiles` + desktop listing.
/// Serde so the desktop IPC surface can return it verbatim; `active` is
/// derived from the config's `active_profile` pointer (a name is never
/// stored twice).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProfileSummary {
    /// The `profiles` map key (also the `/profiles use <name>` argument).
    pub name: String,
    /// Whether this profile is the config's active one.
    pub active: bool,
    /// How many provider slots the profile carries.
    pub provider_count: usize,
    /// `active_target.provider_id` ("" when the profile has no target yet).
    pub active_provider_id: String,
    /// `active_target.model_id` ("" when the profile has no target yet).
    pub active_model_id: String,
}

/// R3-2: the full profile listing — the desktop's `list_profiles` payload.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProfileList {
    /// The key in-process resolution uses (never empty: `"default"` when the
    /// config's `active_profile` is unset).
    pub active_profile: String,
    /// Every profile, sorted by name.
    pub profiles: Vec<ProfileSummary>,
}

/// R3-2: outcome of a successful profile switch
/// (`set_active_profile`) — everything a caller needs to re-resolve the
/// running engine without re-reading the file.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProfileSwitch {
    /// The profile that is now active.
    pub profile: String,
    /// The profile's active provider slot id.
    pub provider_id: String,
    /// The profile's active model id.
    pub model_id: String,
    /// Where `providers.toml` was written.
    pub saved_path: PathBuf,
}

/// R3-2: outcome of `create_profile` (new empty profile, switched active).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CreatedProfile {
    /// The freshly created (and now active) profile name.
    pub name: String,
    /// Where `providers.toml` was written.
    pub saved_path: PathBuf,
}

/// R3-2: outcome of `rename_profile`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RenamedProfile {
    /// The name before the rename.
    pub old: String,
    /// The name after the rename.
    pub new: String,
    /// True when the renamed profile was the active one (the active pointer
    /// followed the rename; the live target itself is unchanged).
    pub was_active: bool,
    /// Where `providers.toml` was written.
    pub saved_path: PathBuf,
}

/// R3-2: outcome of `delete_profile`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DeletedProfile {
    /// The profile that was removed.
    pub removed: String,
    /// When the deleted profile was active and `force` applied: the profile
    /// that became active ("default" when it exists, else the first
    /// remaining by name). `None` when the active pointer was untouched or
    /// nothing remained to fall back to.
    pub fallback: Option<String>,
    /// Where `providers.toml` was written.
    pub saved_path: PathBuf,
}

/// R4-2 (config export/import): outcome of
/// [`ProviderConfigService::import_snapshot`] — what the merge did plus
/// where it persisted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportOutcome {
    /// Counts and the resulting active-profile pointer.
    pub summary: crate::provider_config_store::ImportSummary,
    /// Where `providers.toml` was written.
    pub saved_path: PathBuf,
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

    /// Slugs connected on the **active** profile — read from the in-memory
    /// config the service holds (no extra disk read). The profile-free
    /// global view remains
    /// [`crate::provider_config_store::connected_slugs`] (flattens every
    /// profile; used by the dashboards).
    pub fn connected_slugs(&self) -> HashSet<String> {
        self.store
            .config()
            .active_model_profile()
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
                .active_model_profile()
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
        let default = self.store.config().active_model_profile();
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

    /// Insert or replace one per-model metadata declaration (R2-4) on the
    /// provider slot with the raw stored id `provider_id` and persist.
    /// Errors when no such slot exists or the spec is invalid.
    pub fn set_model_meta(
        &mut self,
        provider_id: &str,
        spec: shannon_types::provider_config::ModelSpec,
    ) -> io::Result<PathBuf> {
        let mut locked = self.lock()?;
        locked.reload_locked()?;
        locked.set_model_meta(provider_id, spec)
    }

    /// Remove the per-model declaration `model_id` from provider slot
    /// `provider_id` and persist (when something was removed). Idempotent.
    pub fn remove_model_meta(&mut self, provider_id: &str, model_id: &str) -> io::Result<PathBuf> {
        let mut locked = self.lock()?;
        locked.reload_locked()?;
        locked.remove_model_meta(provider_id, model_id)
    }

    /// Hand back the underlying store for callers that need raw access
    /// (desktop low-level paths).
    pub fn into_inner(self) -> ProviderConfigStore {
        self.store
    }

    // ── R3-2: named model-profile management (multi-profile phase 2) ────

    /// List every named model profile with the active one marked
    /// (`/profiles`, the desktop's `list_profiles`). Read-only over the
    /// in-memory snapshot this service was constructed from — construct
    /// fresh (`load`) for the latest on-disk state. Names are sorted; the
    /// `active_profile` key is never empty (`"default"` when unset).
    pub fn list_profiles(&self) -> ProfileList {
        let active_key = self.store.config().active_profile_key().to_string();
        let mut profiles: Vec<ProfileSummary> = self
            .store
            .config()
            .profiles
            .iter()
            .map(|(name, mp)| ProfileSummary {
                name: name.clone(),
                active: *name == active_key,
                provider_count: mp.providers.len(),
                active_provider_id: mp.active_target.provider_id.clone(),
                active_model_id: mp.active_target.model_id.clone(),
            })
            .collect();
        profiles.sort_by(|a, b| a.name.cmp(&b.name));
        ProfileList {
            active_profile: active_key,
            profiles,
        }
    }

    /// Switch the active profile to `name` and persist
    /// (`/profiles use <name>`, the desktop's `set_active_profile`).
    ///
    /// Refuses (does not persist) when:
    /// - the profile does not exist (`NotFound`, message lists the
    ///   available ones),
    /// - it has no provider slots (`InvalidData`: empty profile), or
    /// - it has no usable `active_target` — the slot it names is absent or
    ///   the pointer is blank (`InvalidData`).
    ///
    /// Returns a [`ProfileSwitch`] so callers can re-resolve the running
    /// engine (`provider_id` / `model_id`) without re-reading the file.
    /// Engine state itself is a session concern — the caller applies it
    /// (REPL: `apply_model_selection`).
    pub fn set_active_profile(&mut self, name: &str) -> io::Result<ProfileSwitch> {
        let mut locked = self.lock()?;
        locked.reload_locked()?;
        locked.set_active_profile(name)
    }

    /// Create an empty named profile and switch to it, persisting both
    /// (`/profiles new <name>`). Refuses duplicates (`AlreadyExists`) and
    /// invalid names (`InvalidData`, via
    /// [`shannon_types::provider_config::validate_profile_name`]). Unlike
    /// [`Self::set_active_profile`], the switch to an **empty** profile is
    /// intentional — this is the "start fresh" flow; the empty profile
    /// resolves to synthesis until the user connects a provider into it.
    pub fn create_profile(&mut self, name: &str) -> io::Result<CreatedProfile> {
        let mut locked = self.lock()?;
        locked.reload_locked()?;
        locked.create_profile(name)
    }

    /// Rename a model profile and persist (`/profiles rename <old> <new>`).
    /// The profile's contents move wholesale; when the renamed profile was
    /// active, the active pointer follows it. Errors: unknown `old`
    /// (`NotFound` + available list), duplicate/invalid `new`
    /// (`AlreadyExists` / `InvalidData`).
    pub fn rename_profile(&mut self, old: &str, new: &str) -> io::Result<RenamedProfile> {
        let mut locked = self.lock()?;
        locked.reload_locked()?;
        locked.rename_profile(old, new)
    }

    /// Delete a named model profile and persist
    /// (`/profiles delete <name> [--force]`).
    ///
    /// Guards (refusal leaves the file untouched):
    /// - the last remaining profile can never be deleted;
    /// - the **active** profile requires `force = true`;
    /// - unknown names are a `NotFound` listing what exists.
    ///
    /// With `force`, the active pointer falls back to `"default"` (when it
    /// exists and is not the deleted one), else to the first remaining
    /// profile (sorted) — [`DeletedProfile::fallback`] names it so the
    /// caller can re-resolve the engine.
    pub fn delete_profile(&mut self, name: &str, force: bool) -> io::Result<DeletedProfile> {
        let mut locked = self.lock()?;
        locked.reload_locked()?;
        locked.delete_profile(name, force)
    }

    /// R4-2 (config export/import): provider slots an import of `snapshot`
    /// would overwrite — sorted `(profile, provider id)` pairs. Read-only
    /// over the in-memory snapshot; call before [`Self::import_snapshot`] to
    /// implement the default refuse-on-conflict contract and list the
    /// conflicts for the user.
    pub fn import_conflicts(&self, snapshot: &ProviderModelConfig) -> Vec<ImportConflict> {
        self.store.import_conflicts(snapshot)
    }

    /// R4-2 (config export/import): overlay a portable snapshot (the
    /// `ProviderModelConfig` payload of `shannon providers export`) onto the
    /// live `providers.toml` and persist — the one semantic write for imports,
    /// same lock → reload → mutate → save sequence as every other write so a
    /// concurrent desktop/CLI writer cannot be clobbered. Conflict detection
    /// runs **after** the reload, so the refusal (without `force`) reflects
    /// the freshest committed state.
    ///
    /// `set_active` switches the active-profile pointer after the merge (the
    /// profile must exist with at least one provider slot); `None` adopts the
    /// snapshot's pointer only on a fresh machine (no connected providers
    /// before the import). See
    /// [`crate::provider_config_store::ProviderConfigStore::apply_import_snapshot`]
    /// for the full merge policy. Credentials are never touched.
    pub fn import_snapshot(
        &mut self,
        snapshot: &ProviderModelConfig,
        force: bool,
        set_active: Option<&str>,
    ) -> io::Result<ImportOutcome> {
        let mut locked = self.lock()?;
        locked.reload_locked()?;
        locked.import_snapshot(snapshot, force, set_active)
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

    /// Insert or replace one per-model metadata declaration (R2-4) and
    /// persist. The flock is already held by this guard.
    pub fn set_model_meta(
        &mut self,
        provider_id: &str,
        spec: shannon_types::provider_config::ModelSpec,
    ) -> io::Result<PathBuf> {
        self.svc.store.set_model_meta(provider_id, spec)?;
        self.svc.store.save_locked()
    }

    /// Remove one per-model metadata declaration (R2-4) and persist — only
    /// when an entry was actually removed (idempotent no-op otherwise).
    pub fn remove_model_meta(&mut self, provider_id: &str, model_id: &str) -> io::Result<PathBuf> {
        if self.svc.store.remove_model_meta(provider_id, model_id)? {
            self.svc.store.save_locked()
        } else {
            // Nothing changed; hand back the pinned path without touching
            // the file (matches disconnect_by_slug's "no write on no-op").
            self.svc
                .store
                .last_path()
                .map(Path::to_path_buf)
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::NotFound, "no providers.toml path pinned")
                })
        }
    }

    // ── R3-2: named model-profile management (locked variants) ──────────

    /// Switch the active profile (validate → persist). See
    /// [`ProviderConfigService::set_active_profile`] for the contract.
    pub fn set_active_profile(&mut self, name: &str) -> io::Result<ProfileSwitch> {
        let list = self.svc.list_profiles();
        let summary = list
            .profiles
            .iter()
            .find(|p| p.name == name)
            .ok_or_else(|| unknown_profile_error(name, &list))?;
        if summary.provider_count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "refusing to switch to profile '{name}': it has no providers yet; \
                     add one with /connect first"
                ),
            ));
        }
        let target_has_slot = self
            .svc
            .store
            .config()
            .profiles
            .get(name)
            .is_some_and(|mp| {
                mp.providers
                    .iter()
                    .any(|p| p.id == summary.active_provider_id)
            });
        if summary.active_provider_id.is_empty()
            || summary.active_model_id.is_empty()
            || !target_has_slot
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "refusing to switch to profile '{name}': it has no active model — \
                     set one with /connect or /model first"
                ),
            ));
        }
        self.svc.store.set_active_profile_key(name);
        let saved_path = self.svc.store.save_locked()?;
        Ok(ProfileSwitch {
            profile: name.to_string(),
            provider_id: summary.active_provider_id.clone(),
            model_id: summary.active_model_id.clone(),
            saved_path,
        })
    }

    /// Create an empty named profile + switch (validate name → persist).
    /// See [`ProviderConfigService::create_profile`].
    pub fn create_profile(&mut self, name: &str) -> io::Result<CreatedProfile> {
        let name = validate_profile_name(name)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        self.svc.store.insert_model_profile(&name)?;
        self.svc.store.set_active_profile_key(&name);
        let saved_path = self.svc.store.save_locked()?;
        Ok(CreatedProfile { name, saved_path })
    }

    /// Rename a model profile (validate → persist). See
    /// [`ProviderConfigService::rename_profile`].
    pub fn rename_profile(&mut self, old: &str, new: &str) -> io::Result<RenamedProfile> {
        let new = validate_profile_name(new)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let was_active = self.svc.store.config().active_profile_key() == old;
        self.svc.store.rename_model_profile(old, &new)?;
        let saved_path = self.svc.store.save_locked()?;
        Ok(RenamedProfile {
            old: old.to_string(),
            new,
            was_active,
            saved_path,
        })
    }

    /// Delete a model profile (guards → persist). See
    /// [`ProviderConfigService::delete_profile`].
    pub fn delete_profile(&mut self, name: &str, force: bool) -> io::Result<DeletedProfile> {
        let fallback = self.svc.store.remove_model_profile(name, force)?;
        let saved_path = self.svc.store.save_locked()?;
        Ok(DeletedProfile {
            removed: name.to_string(),
            fallback,
            saved_path,
        })
    }

    /// R4-2: overlay a portable snapshot onto the store and persist, inside
    /// the flock this guard holds. See
    /// [`ProviderConfigService::import_snapshot`] for the contract.
    pub fn import_snapshot(
        &mut self,
        snapshot: &ProviderModelConfig,
        force: bool,
        set_active: Option<&str>,
    ) -> io::Result<ImportOutcome> {
        let summary = self
            .svc
            .store
            .apply_import_snapshot(snapshot, force, set_active)?;
        let saved_path = self.svc.store.save_locked()?;
        Ok(ImportOutcome {
            summary,
            saved_path,
        })
    }

    /// Persist whatever is in the in-memory config to disk. **The
    /// caller MUST hold the flock** (typically via
    /// [`ProviderConfigService::lock`]); this does not re-acquire it.
    pub fn save_locked(&mut self) -> io::Result<PathBuf> {
        self.svc.store.save_locked()
    }
}

/// The `NotFound` error for an unknown profile name, listing what exists
/// (R3-2: every front-end shows the same "available profiles" hint).
fn unknown_profile_error(name: &str, list: &ProfileList) -> io::Error {
    let available = list
        .profiles
        .iter()
        .map(|p| p.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    io::Error::new(
        io::ErrorKind::NotFound,
        if available.is_empty() {
            format!("profile '{name}' not found (no profiles configured)")
        } else {
            format!("profile '{name}' not found; available profiles: {available}")
        },
    )
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
        models: Vec::new(),
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
            .expect("connect must succeed");
        let _ = svc
            .connect(LlmProvider::OpenAI, None, None, true)
            .expect("connect must succeed");
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
            .expect("connect must succeed");
        let _ = svc
            .connect(LlmProvider::OpenAI, None, None, true)
            .expect("connect must succeed");

        let outcome = svc
            .disconnect(&LlmProvider::OpenAI)
            .expect("connect must succeed");
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
            .expect("connect must succeed");
        let _ = svc
            .connect(LlmProvider::OpenAI, None, None, true)
            .expect("connect must succeed");

        let outcome = svc
            .disconnect(&LlmProvider::OpenAI)
            .expect("connect must succeed");
        // OpenAI was made active by the second connect; a remaining slug is offered.
        assert_eq!(outcome.next_active.as_deref(), Some("anthropic"));
    }

    #[test]
    fn connect_make_active_false_preserves_prior_selection() {
        // T4 — the CLI --set-active=false path must not steal active_target.
        let (mut svc, _dir) = service();
        let _ = svc
            .connect(LlmProvider::Anthropic, None, None, true)
            .expect("connect must succeed");
        let anthropic_active = svc
            .store
            .config()
            .profiles
            .get("default")
            .expect("connect must succeed")
            .active_target
            .provider_id
            .clone();
        assert_eq!(anthropic_active, "anthropic");

        // Add OpenAI without making it active.
        let _ = svc
            .connect(LlmProvider::OpenAI, None, None, false)
            .expect("connect must succeed");
        let still_active = svc
            .store
            .config()
            .profiles
            .get("default")
            .expect("connect must succeed")
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
        let outcome = svc
            .disconnect(&LlmProvider::OpenAI)
            .expect("disconnect must succeed");
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
            .expect("connect must succeed");
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
            models: Vec::new(),
        };
        svc.upsert(custom, "gpt-4o", true)
            .expect("upsert must succeed");
        // `my-gateway` became active (make_active=true); disconnecting it by
        // raw slug must report was_active and offer anthropic as next.
        let outcome = svc
            .disconnect_by_slug("my-gateway")
            .expect("disconnect must succeed");
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
                .expect("connect must succeed");
            let _ = svc
                .connect(LlmProvider::OpenAI, None, None, true)
                .expect("connect must succeed");
        }
        let by_provider = svc_a
            .disconnect(&LlmProvider::OpenAI)
            .expect("connect must succeed");
        let by_slug = svc_b
            .disconnect_by_slug(&llm_provider_id(&LlmProvider::OpenAI))
            .expect("connect must succeed");
        assert_eq!(by_provider.was_connected, by_slug.was_connected);
        assert_eq!(by_provider.was_active, by_slug.was_active);
        assert_eq!(by_provider.next_active, by_slug.next_active);
    }

    #[test]
    fn disconnect_by_slug_unknown_is_idempotent_noop() {
        let (mut svc, _dir) = service();
        let outcome = svc
            .disconnect_by_slug("does-not-exist")
            .expect("disconnect must succeed");
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
            .expect("connect must succeed");
        let _ = svc
            .connect(LlmProvider::OpenAI, None, None, true)
            .expect("connect must succeed");
        // Anthropic is NOT active (OpenAI took active). Removing anthropic:
        let outcome = svc
            .disconnect(&LlmProvider::Anthropic)
            .expect("connect must succeed");
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
            .expect("connect must succeed");
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
            .expect("connect must succeed");
        assert_eq!(connected.model_id, "claude-sonnet-4-6");
        let active = &svc
            .store
            .config()
            .profiles
            .get("default")
            .expect("default profile exists")
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
            .expect("connect must succeed");

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
            models: Vec::new(),
        };
        svc.upsert(custom, "gpt-4o", true)
            .expect("upsert must succeed");

        let connected = svc.connected_slugs();
        assert!(connected.contains("anthropic"), "anthropic must survive");
        assert!(connected.contains("my-gateway"), "custom provider added");

        // make_active=true → new profile is the active target.
        let active = &svc
            .store
            .config()
            .profiles
            .get("default")
            .expect("upsert must succeed")
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
            .expect("connect must succeed");

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
            models: Vec::new(),
        };
        svc.upsert(custom, "gpt-4o", false)
            .expect("upsert must succeed");

        assert!(svc.connected_slugs().contains("openai"));
        let active = &svc
            .store
            .config()
            .profiles
            .get("default")
            .expect("upsert must succeed")
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
            .expect("connect must succeed");
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
            .expect("default profile exists")
            .active_target;
        assert_eq!(active.provider_id, "anthropic");
        assert_eq!(active.model_id, connected.model_id);
    }

    #[test]
    fn connect_respects_explicit_model() {
        let (mut svc, _dir) = service();
        let connected = svc
            .connect(LlmProvider::OpenAI, Some("gpt-4o"), None, true)
            .expect("connect must succeed");
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
        .expect("connect must succeed");
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
            .expect("connect must succeed");
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
            svc.connect(provider.clone(), None, None, true)
                .expect("connect must succeed");
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
        let err = result
            .expect_err("connect must have been refused")
            .to_string();
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
                .expect("connect must succeed");
            locked
                .connect(LlmProvider::OpenAI, None, None, true)
                .expect("connect must succeed");
        }
        {
            let mut locked = svc.lock().expect("lock again");
            let outcome = locked
                .disconnect(&LlmProvider::OpenAI)
                .expect("connect must succeed");
            assert!(outcome.was_connected);
        }
        // Now drop locked; reopen to verify shape.
        let on_disk = std::fs::read_to_string(_dir.path().join("providers.toml"))
            .expect("connect must succeed");
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
            .expect("connect must succeed");

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

    // ===== Per-model metadata declarations (R2-4) =====

    fn meta_spec(
        id: &str,
        context_window: u32,
        cost_in: f64,
        cost_out: f64,
    ) -> shannon_types::provider_config::ModelSpec {
        shannon_types::provider_config::ModelSpec {
            id: id.to_string(),
            display_name: None,
            context_window: Some(context_window),
            max_output: Some(8_192),
            cost_per_m_input: Some(cost_in),
            cost_per_m_output: Some(cost_out),
            capabilities: vec![shannon_types::provider_config::ModelCapability::Vision],
        }
    }

    #[test]
    fn set_model_meta_persists_through_service_write_path() {
        let (mut svc, dir) = service();
        let _ = svc
            .connect(LlmProvider::Anthropic, None, None, true)
            .expect("connect must succeed");
        // Raw stored id (== llm_provider_id for the canonical connect path).
        let saved = svc
            .set_model_meta("anthropic", meta_spec("claude-custom", 200_000, 3.0, 15.0))
            .expect("set_model_meta must persist");
        assert!(saved.exists());

        // Reload from disk: the declaration survives, with values intact.
        let reloaded = ProviderConfigService::load_at(&dir.path().join("providers.toml"));
        let models = &reloaded.store().config().profiles["default"].providers[0].models;
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "claude-custom");
        assert_eq!(models[0].context_window, Some(200_000));
        assert_eq!(models[0].cost_per_m_input, Some(3.0));
    }

    #[test]
    fn set_model_meta_replaces_entry_with_same_id() {
        let (mut svc, _dir) = service();
        let _ = svc
            .connect(LlmProvider::Anthropic, None, None, true)
            .expect("connect must succeed");
        svc.set_model_meta("anthropic", meta_spec("m", 100_000, 1.0, 2.0))
            .expect("first set");
        svc.set_model_meta("anthropic", meta_spec("m", 128_000, 0.5, 1.5))
            .expect("second set replaces");
        let models = &svc.store().config().profiles["default"].providers[0].models;
        assert_eq!(models.len(), 1, "ids stay unique through the service");
        assert_eq!(models[0].context_window, Some(128_000));
    }

    #[test]
    fn set_model_meta_unknown_provider_is_an_error() {
        let (mut svc, _dir) = service();
        let _ = svc
            .connect(LlmProvider::Anthropic, None, None, true)
            .expect("connect must succeed");
        let err = svc
            .set_model_meta("ghost-provider", meta_spec("m", 1, 0.0, 0.0))
            .expect_err("unknown provider must error");
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn remove_model_meta_is_idempotent_and_persists_only_on_removal() {
        let (mut svc, dir) = service();
        let _ = svc
            .connect(LlmProvider::Anthropic, None, None, true)
            .expect("connect must succeed");
        svc.set_model_meta("anthropic", meta_spec("m", 1_000, 0.0, 0.0))
            .expect("set");
        let saved = svc
            .remove_model_meta("anthropic", "m")
            .expect("remove must persist");
        assert!(saved.exists());
        // Second remove: no-op, no error.
        svc.remove_model_meta("anthropic", "m")
            .expect("idempotent remove");
        let reloaded = ProviderConfigService::load_at(&dir.path().join("providers.toml"));
        let models = &reloaded.store().config().profiles["default"].providers[0].models;
        assert!(models.is_empty());
    }

    // ===== R3-2: named model-profile management =====

    use shannon_types::provider_config::{
        ActiveTarget, CredentialScope, ModelProfile, ProviderKind, ProviderModelConfig, Scope,
    };

    /// Seed a two-profile config on disk (`default` → anthropic, plus any
    /// extras the caller passes) and hand back a service over it. Going
    /// through the file keeps the tests on the same public surface the
    /// front-ends use.
    fn service_with_profiles(extra: Vec<(&str, ModelProfile)>) -> (ProviderConfigService, TempDir) {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("providers.toml");
        let anthropic = ProviderProfile {
            id: "anthropic".to_string(),
            kind: ProviderKind::Anthropic,
            display_name: "anthropic".to_string(),
            base_url: LlmProvider::Anthropic.default_base_url().to_string(),
            models_url: None,
            credential: CredentialRef::Store {
                service: "anthropic".to_string(),
            },
            extra_headers: std::collections::HashMap::new(),
            default_max_tokens: None,
            fallback_models: Vec::new(),
            quirks: Default::default(),
            tiers: ProviderTiers::default(),
            models: Vec::new(),
        };
        let mut profiles = std::collections::HashMap::from([(
            "default".to_string(),
            ModelProfile {
                name: "default".to_string(),
                active_target: ActiveTarget {
                    provider_id: "anthropic".to_string(),
                    model_id: "claude-sonnet-4-6".to_string(),
                    scope: Scope::Global,
                },
                providers: vec![anthropic],
                auxiliary: std::collections::HashMap::new(),
                credential_scope: CredentialScope::Shared,
            },
        )]);
        for (name, mp) in extra {
            profiles.insert(name.to_string(), mp);
        }
        crate::provider_config_store::save(
            &ProviderModelConfig {
                version: ProviderModelConfig::VERSION,
                active_profile: String::new(),
                profiles,
                gateway: Default::default(),
            },
            Some(&path),
        )
        .expect("seed file");
        (ProviderConfigService::load_at(&path), dir)
    }

    /// `default` + a `work` profile (openai / gpt-4o).
    fn service_with_work_profile() -> (ProviderConfigService, TempDir) {
        let work = ModelProfile {
            name: "work".to_string(),
            active_target: ActiveTarget {
                provider_id: "openai".to_string(),
                model_id: "gpt-4o".to_string(),
                scope: Scope::Global,
            },
            providers: vec![ProviderProfile {
                id: "openai".to_string(),
                kind: ProviderKind::OpenAi,
                display_name: "openai".to_string(),
                base_url: LlmProvider::OpenAI.default_base_url().to_string(),
                models_url: None,
                credential: CredentialRef::Store {
                    service: "openai".to_string(),
                },
                extra_headers: std::collections::HashMap::new(),
                default_max_tokens: None,
                fallback_models: Vec::new(),
                quirks: Default::default(),
                tiers: ProviderTiers::default(),
                models: Vec::new(),
            }],
            auxiliary: std::collections::HashMap::new(),
            credential_scope: CredentialScope::Shared,
        };
        service_with_profiles(vec![("work", work)])
    }

    #[test]
    fn list_profiles_marks_active_and_sorts_names() {
        let (svc, _dir) = service_with_work_profile();
        let list = svc.list_profiles();
        assert_eq!(list.active_profile, "default");
        let names: Vec<&str> = list.profiles.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["default", "work"], "sorted");
        let default = &list.profiles[0];
        assert!(default.active && default.provider_count == 1);
        assert_eq!(default.active_provider_id, "anthropic");
        assert_eq!(default.active_model_id, "claude-sonnet-4-6");
        let work = &list.profiles[1];
        assert!(!work.active && work.active_provider_id == "openai");
    }

    #[test]
    fn set_active_profile_switches_and_persists() {
        let (mut svc, dir) = service_with_work_profile();
        let sw = svc.set_active_profile("work").expect("switch");
        assert_eq!(sw.profile, "work");
        assert_eq!(sw.provider_id, "openai");
        assert_eq!(sw.model_id, "gpt-4o");

        // Durable: a fresh load resolves the work profile's target.
        let reloaded = ProviderConfigService::load_at(&dir.path().join("providers.toml"));
        assert_eq!(reloaded.list_profiles().active_profile, "work");
        let rt = crate::provider_resolver::resolve_active_target(reloaded.store().config())
            .expect("work resolves");
        assert_eq!(rt.provider, LlmProvider::OpenAI);
        assert_eq!(rt.model_id, "gpt-4o");
        // And connected_slugs follows the active profile.
        assert!(reloaded.connected_slugs().contains("openai"));
    }

    #[test]
    fn set_active_profile_refuses_unknown_and_empty() {
        let (mut svc, _dir) = service_with_work_profile();
        // Unknown → NotFound listing what exists.
        let err = svc
            .set_active_profile("ghost")
            .expect_err("unknown profile must error");
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert!(err.to_string().contains("default") && err.to_string().contains("work"));

        // Empty profile → refused with the reason. (create_profile already
        // switched to the fresh "hollow"; the refused set must not change
        // anything further.)
        svc.create_profile("hollow").expect("create empty");
        let before = svc.list_profiles();
        let err = svc
            .set_active_profile("hollow")
            .expect_err("expected failure");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("no providers"), "{err}");
        assert_eq!(svc.list_profiles(), before);
    }

    #[test]
    fn set_active_profile_refuses_targeting_gap() {
        // A profile with a provider slot but a dangling active_target —
        // switching would leave the engine with nothing to resolve.
        let broken = ModelProfile {
            name: "broken".to_string(),
            active_target: ActiveTarget {
                provider_id: "ghost-slot".to_string(),
                model_id: "m".to_string(),
                scope: Scope::Global,
            },
            providers: vec![build_profile_for_provider(&LlmProvider::Ollama, None)],
            auxiliary: std::collections::HashMap::new(),
            credential_scope: CredentialScope::Shared,
        };
        let (mut svc, _dir) = service_with_profiles(vec![("broken", broken)]);
        let err = svc
            .set_active_profile("broken")
            .expect_err("expected failure");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("no active model"), "{err}");
        assert_eq!(svc.list_profiles().active_profile, "default");
    }

    #[test]
    fn set_active_profile_refuses_blank_model_id() {
        // A provider slot with a blank `active_target.model_id` (hand-edited
        // file) — switching would leave the engine with an empty model.
        let blank_model = ModelProfile {
            name: "blank".to_string(),
            active_target: ActiveTarget {
                provider_id: "ollama".to_string(),
                model_id: String::new(),
                scope: Scope::Global,
            },
            providers: vec![build_profile_for_provider(&LlmProvider::Ollama, None)],
            auxiliary: std::collections::HashMap::new(),
            credential_scope: CredentialScope::Shared,
        };
        let (mut svc, _dir) = service_with_profiles(vec![("blank", blank_model)]);
        let err = svc
            .set_active_profile("blank")
            .expect_err("blank model must refuse");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("no active model"), "{err}");
        assert_eq!(svc.list_profiles().active_profile, "default");
    }

    #[test]
    fn create_profile_refuses_duplicates_and_bad_names_and_persists_switch() {
        let (mut svc, dir) = service_with_work_profile();
        svc.create_profile("personal").expect("create");
        let err = svc
            .create_profile("personal")
            .expect_err("expected failure");
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        let err = svc
            .create_profile("two words")
            .expect_err("expected failure");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);

        let reloaded = ProviderConfigService::load_at(&dir.path().join("providers.toml"));
        let list = reloaded.list_profiles();
        assert_eq!(list.active_profile, "personal");
        let personal = list
            .profiles
            .iter()
            .find(|p| p.name == "personal")
            .expect("personal profile listed");
        assert_eq!(personal.provider_count, 0);
        // The scaffold is named after its key.
        assert_eq!(
            reloaded.store().config().profiles["personal"].name,
            "personal"
        );
    }

    #[test]
    fn rename_profile_moves_contents_and_follows_active_pointer() {
        let (mut svc, dir) = service_with_work_profile();
        // Rename a non-active profile: contents move, pointer untouched.
        let renamed = svc.rename_profile("work", "gig").expect("rename");
        assert_eq!(
            (renamed.old.as_str(), renamed.new.as_str()),
            ("work", "gig")
        );
        assert!(!renamed.was_active);
        assert_eq!(svc.list_profiles().active_profile, "default");

        // Rename the active profile: the pointer follows.
        svc.rename_profile("default", "main")
            .expect("rename active");
        let list = svc.list_profiles();
        assert_eq!(list.active_profile, "main");
        assert!(list.profiles.iter().any(|p| p.name == "gig"));

        // Durable, and resolution still finds the (renamed) active profile.
        let reloaded = ProviderConfigService::load_at(&dir.path().join("providers.toml"));
        assert_eq!(reloaded.list_profiles().active_profile, "main");
        let rt = crate::provider_resolver::resolve_active_target(reloaded.store().config())
            .expect("renamed active profile resolves");
        assert_eq!(rt.provider, LlmProvider::Anthropic);

        // Guards: unknown old, duplicate new.
        assert_eq!(
            svc.rename_profile("ghost", "x")
                .expect_err("expected failure")
                .kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(
            svc.rename_profile("gig", "main")
                .expect_err("expected failure")
                .kind(),
            io::ErrorKind::AlreadyExists
        );
    }

    #[test]
    fn delete_profile_guards_and_force_fallback_chain() {
        let (mut svc, dir) = service_with_work_profile();
        // Active without --force refuses.
        let err = svc
            .delete_profile("default", false)
            .expect_err("expected failure");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("refusing to delete the active"));

        // Non-active deletes cleanly; pointer untouched.
        let deleted = svc.delete_profile("work", false).expect("delete work");
        assert_eq!(deleted.removed, "work");
        assert!(deleted.fallback.is_none());

        // Last remaining profile refuses even with --force.
        let err = svc
            .delete_profile("default", true)
            .expect_err("expected failure");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("only profile"), "{err}");
        assert_eq!(svc.list_profiles().active_profile, "default");

        // Force-delete with a "default" present falls back to it.
        // (create_profile already switches to the fresh profile.)
        svc.create_profile("temp").expect("create");
        let deleted = svc.delete_profile("temp", true).expect("force delete");
        assert_eq!(deleted.fallback.as_deref(), Some("default"));

        // Durable.
        let reloaded = ProviderConfigService::load_at(&dir.path().join("providers.toml"));
        assert_eq!(reloaded.list_profiles().active_profile, "default");
    }

    #[test]
    fn delete_profile_force_without_default_falls_to_first_sorted() {
        let (mut svc, _dir) = service_with_work_profile();
        // Rename default away so the fallback chain hits first-sorted.
        svc.rename_profile("default", "aaa").expect("rename");
        svc.set_active_profile("aaa").expect("switch");
        let deleted = svc.delete_profile("aaa", true).expect("force delete");
        // Remaining: work → becomes active.
        assert_eq!(deleted.fallback.as_deref(), Some("work"));
        assert_eq!(svc.list_profiles().active_profile, "work");
    }

    #[test]
    fn writes_land_in_the_active_profile() {
        // The phase-2 coherence contract: with a non-default profile active,
        // /connect-style writes (connect/set_active/set_tier) land in THAT
        // profile, and resolution reads them from there.
        let (mut svc, dir) = service_with_work_profile();
        svc.set_active_profile("work").expect("switch");
        let connected = svc
            .connect(LlmProvider::Ollama, Some("llama3"), None, true)
            .expect("connect into work");

        let reloaded = ProviderConfigService::load_at(&dir.path().join("providers.toml"));
        // Ollama landed in `work`, not `default`.
        let work = reloaded
            .store()
            .config()
            .profiles
            .get("work")
            .expect("work exists");
        assert!(work.providers.iter().any(|p| p.id == "ollama"));
        assert_eq!(work.active_target.provider_id, "ollama");
        assert_eq!(work.active_target.model_id, connected.model_id);
        // `default` was untouched.
        let default = reloaded
            .store()
            .config()
            .profiles
            .get("default")
            .expect("default profile listed");
        assert!(!default.providers.iter().any(|p| p.id == "ollama"));
        assert_eq!(default.active_target.provider_id, "anthropic");
        // And resolution over the fresh load serves the work profile.
        let rt = crate::provider_resolver::resolve_active_target(reloaded.store().config())
            .expect("work resolves");
        assert_eq!(rt.provider, LlmProvider::Ollama);
    }

    #[test]
    fn active_profile_round_trips_through_toml_and_is_skipped_for_default() {
        // Byte-compat: a config whose active profile is "default" (or unset)
        // omits the key entirely; an explicit pointer survives the cycle.
        let (mut svc, dir) = service();
        let path = dir.path().join("providers.toml");
        svc.connect(LlmProvider::Anthropic, None, None, true)
            .expect("connect");
        svc.store.save().expect("save default-active");
        let on_disk = std::fs::read_to_string(&path).expect("file written");
        assert!(
            !on_disk.contains("active_profile"),
            "default-active file must omit the key:\n{on_disk}"
        );

        svc.create_profile("work").expect("create");
        svc.store.save().expect("save work-active");
        let on_disk = std::fs::read_to_string(&path).expect("file written");
        assert!(
            on_disk.contains("active_profile = \"work\""),
            "explicit pointer persists:\n{on_disk}"
        );
        let reloaded = ProviderConfigService::load_at(&path);
        assert_eq!(reloaded.list_profiles().active_profile, "work");
    }

    // ── R4-2: config export/import ───────────────────────────────────────

    fn import_profile(id: &str, base_url: &str, model: &str) -> ProviderProfile {
        ProviderProfile {
            id: id.to_string(),
            kind: shannon_types::provider_config::ProviderKind::OpenAiCompatible,
            display_name: id.to_string(),
            base_url: base_url.to_string(),
            models_url: None,
            credential: CredentialRef::Store {
                service: id.to_string(),
            },
            extra_headers: std::collections::HashMap::new(),
            default_max_tokens: None,
            fallback_models: Vec::new(),
            quirks: Default::default(),
            tiers: ProviderTiers {
                standard: Some(model.to_string()),
                ..Default::default()
            },
            models: Vec::new(),
        }
    }

    #[test]
    fn import_snapshot_persists_and_conflicts_recheck_under_lock() {
        let (mut svc, dir) = service();
        let _ = svc
            .connect(LlmProvider::Anthropic, None, None, true)
            .expect("seed");

        // Snapshot carrying an anthropic slot that conflicts with the live
        // one (same id) plus a brand-new glm slot.
        let snapshot = ProviderModelConfig {
            version: ProviderModelConfig::VERSION,
            active_profile: String::new(),
            profiles: std::iter::once((
                "default".to_string(),
                shannon_types::provider_config::ModelProfile {
                    name: "default".to_string(),
                    active_target: shannon_types::provider_config::ActiveTarget {
                        provider_id: "anthropic".to_string(),
                        model_id: "claude-from-snapshot".to_string(),
                        scope: shannon_types::provider_config::Scope::Global,
                    },
                    providers: vec![
                        import_profile(
                            "anthropic",
                            "https://api.anthropic.com",
                            "claude-from-snapshot",
                        ),
                        import_profile("glm", "https://open.bigmodel.cn/v1", "glm-4.6"),
                    ],
                    auxiliary: std::collections::HashMap::new(),
                    credential_scope: shannon_types::provider_config::CredentialScope::Shared,
                },
            ))
            .collect(),
            gateway: Default::default(),
        };

        // Without force: refused, listing the conflict.
        assert!(!svc.import_conflicts(&snapshot).is_empty());
        let err = svc
            .import_snapshot(&snapshot, false, None)
            .expect_err("conflict refuses");
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);

        // With force: the anthropic slot is replaced, glm added, persisted.
        let outcome = svc
            .import_snapshot(&snapshot, true, None)
            .expect("forced import");
        assert_eq!(outcome.summary.providers_replaced, 1);
        assert_eq!(outcome.summary.providers_added, 1);
        // The snapshot's active target followed the force-replaced slot.
        assert_eq!(
            outcome.summary.active_profile, "default",
            "pointer unchanged (was already default)"
        );
        assert!(!outcome.summary.active_pointer_applied);

        // The write landed on disk (a fresh reader sees it).
        let reloaded = ProviderConfigService::load_at(&dir.path().join("providers.toml"));
        assert!(reloaded.connected_slugs().contains("glm"));
        assert!(reloaded.connected_slugs().contains("anthropic"));
    }

    #[test]
    fn import_snapshot_fresh_machine_adopts_snapshot_pointer() {
        let (mut svc, _dir) = service();
        let snapshot = ProviderModelConfig {
            version: ProviderModelConfig::VERSION,
            active_profile: "work".to_string(),
            profiles: std::iter::once((
                "work".to_string(),
                shannon_types::provider_config::ModelProfile {
                    name: "work".to_string(),
                    active_target: shannon_types::provider_config::ActiveTarget {
                        provider_id: "glm".to_string(),
                        model_id: "glm-4.6".to_string(),
                        scope: shannon_types::provider_config::Scope::Global,
                    },
                    providers: vec![import_profile(
                        "glm",
                        "https://open.bigmodel.cn/v1",
                        "glm-4.6",
                    )],
                    auxiliary: std::collections::HashMap::new(),
                    credential_scope: shannon_types::provider_config::CredentialScope::Shared,
                },
            ))
            .collect(),
            gateway: Default::default(),
        };
        let outcome = svc
            .import_snapshot(&snapshot, false, None)
            .expect("fresh import");
        assert!(outcome.summary.active_pointer_applied);
        assert_eq!(outcome.summary.active_profile, "work");

        let reloaded = ProviderConfigService::load_at(&_dir.path().join("providers.toml"));
        assert_eq!(reloaded.list_profiles().active_profile, "work");
    }
}
