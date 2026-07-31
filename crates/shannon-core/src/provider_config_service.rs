//! `ProviderConfigService` — the single semantic write path for
//! `~/.shannon/providers.toml` (ADR-0008 Decision 3 / P2-5).
//!
//! Both REPL commands (`/connect`, `/disconnect`) and the CLI
//! (`shannon providers add`) route through this service so the two front-ends
//! cannot diverge on the on-disk shape again. It is a thin layer *over*
//! [`crate::provider_config_store::ProviderConfigStore`], which keeps its raw
//! mutators for the desktop and tests.
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
//! The service owns the load → mutate → persist sequence for one user intent
//! (connect / disconnect / set-active / set-tier / set-max-tokens). It does
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

/// Outcome of [`ProviderConfigService::disconnect`].
#[derive(Debug, Clone)]
pub struct DisconnectOutcome {
    /// `true` when there was a matching slot to remove.
    pub was_connected: bool,
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

        // `upsert_profile` always repoints `active_target` at the new id. For
        // `make_active = false` (e.g. CLI without --set-active) snapshot the
        // previous selection first and restore it after.
        let prev_active = if make_active {
            None
        } else {
            self.store
                .config()
                .profiles
                .get("default")
                .map(|mp| mp.active_target.clone())
        };

        self.store.upsert_profile(profile, &model_id);

        if !make_active {
            if let Some(prev) = prev_active {
                if !prev.provider_id.is_empty() {
                    if let Some(prev_provider) = llm_provider_from_slug(&prev.provider_id) {
                        self.store.set_active(&prev_provider, &prev.model_id);
                    }
                }
            }
        }

        let saved_path = self.store.save()?;
        Ok(ConnectedProvider {
            provider,
            model_id,
            service: provider_id,
            saved_path,
        })
    }

    /// Disconnect (remove) a provider slot. Idempotent — returns
    /// `was_connected = false` when there was nothing to remove. When the
    /// removed slot was the active selection, [`DisconnectOutcome::next_active`]
    /// names a remaining provider to switch to (the caller does the actual
    /// engine switch — a session concern).
    pub fn disconnect(&mut self, provider: &LlmProvider) -> io::Result<DisconnectOutcome> {
        let slug = llm_provider_id(provider);
        let default = self.store.config().profiles.get("default");
        let was_connected = default
            .map(|mp| mp.providers.iter().any(|p| p.id == slug))
            .unwrap_or(false);
        let was_active = default
            .map(|mp| mp.active_target.provider_id == slug)
            .unwrap_or(false);

        if !was_connected {
            return Ok(DisconnectOutcome {
                was_connected: false,
                next_active: None,
                saved_path: None,
            });
        }

        self.store.remove_profile(&slug);
        let saved_path = self.store.save()?;

        // `remove_profile` clears `active_target` when it pointed at the
        // removed slot; offer the first remaining slug so the REPL can switch.
        let next_active = if was_active {
            self.store
                .config()
                .profiles
                .get("default")
                .and_then(|mp| mp.providers.first().map(|p| p.id.clone()))
        } else {
            None
        };

        Ok(DisconnectOutcome {
            was_connected: true,
            next_active,
            saved_path: Some(saved_path),
        })
    }

    /// Pin `active_target` at `provider` / `model` and persist.
    pub fn set_active(&mut self, provider: &LlmProvider, model: &str) -> io::Result<PathBuf> {
        self.store.set_active(provider, model);
        self.store.save()
    }

    /// Set a per-tier model override on `provider` and persist.
    pub fn set_tier(
        &mut self,
        provider: &LlmProvider,
        tier: TierName,
        model: &str,
    ) -> io::Result<PathBuf> {
        self.store.set_tier(provider, tier, model);
        self.store.save()
    }

    /// Set or clear (`None`) the per-provider `default_max_tokens` and persist.
    pub fn set_max_tokens(
        &mut self,
        provider: &LlmProvider,
        max_tokens: Option<u32>,
    ) -> io::Result<PathBuf> {
        self.store.set_default_max_tokens(provider, max_tokens);
        self.store.save()
    }

    /// Hand back the underlying store for callers that need raw access
    /// (desktop low-level paths).
    pub fn into_inner(self) -> ProviderConfigStore {
        self.store
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
        assert!(outcome.next_active.is_none());
        assert!(outcome.saved_path.is_none());
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
}
