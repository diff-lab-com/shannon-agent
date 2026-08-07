//! ADR-0009 — Provider Store Read Facade.
//!
//! Typed, cheaply-cloneable snapshot of the desktop's provider read views,
//! produced under one short-lived lock and released before any further
//! `.await`. This is the read-side complement to ADR-0008 Decision 3 /
//! Wave 6's write-path `ProviderConfigService`: one type, one read
//! contract, one place that decides lock granularity and the `"default"`
//! profile invariant.
//!
//! See `docs/adr/0009-provider-store-read-facade.md`.

use tokio::sync::Mutex;

use shannon_core::provider_config_store::ProviderConfigStore;
use shannon_types::provider_config::ProviderProfile;

use crate::config::{self, ProviderConnection, ProvidersFile};

/// Cheaply cloneable, `Send` snapshot of the desktop's provider read views.
///
/// Built under one short-lived mutex acquisition (see [`Self::capture`]) and
/// released before any further `.await`, so callers can hold the owned
/// snapshot across async work without blocking writers or risking
/// re-entrant deadlock (ADR-0009 Decision 2).
///
/// The `"default"` model profile is the single source of user-managed
/// desktop connections; that invariant lives here, in one type, rather than
/// being re-hardcoded at every read site (ADR-0009 Context). Reads do not
/// take the write path's file `flock` — they read the in-memory cache,
/// matching today's behavior and ADR-0005's "engine store is the read
/// authority" model.
#[derive(Debug, Clone)]
pub struct ProviderReadSnapshot {
    /// Id of the active provider, or `None` when no active target is set
    /// (empty store / no profile selected). Mirrors the `"default"`
    /// profile's `active_target.provider_id`.
    pub active_provider_id: Option<String>,
    /// Active model id, surfaced for read paths that need it (e.g.
    /// `configure('base_url')` re-lands the active profile with its model).
    /// `None` when no active target is set.
    pub active_model_id: Option<String>,
    /// Canonical ADR-0005 provider profiles under the `"default"` model
    /// profile. Single-digit cardinality in practice, so the clone under
    /// the lock is negligible against the contention it removes.
    pub providers: Vec<ProviderProfile>,
}

impl Default for ProviderReadSnapshot {
    fn default() -> Self {
        Self {
            active_provider_id: None,
            active_model_id: None,
            providers: Vec::new(),
        }
    }
}

impl ProviderReadSnapshot {
    /// One lock acquisition, one projection, immediate release (Decision 2).
    ///
    /// Accepts `&Mutex<ProviderConfigStore>`; an `&Arc<Mutex<…>>` (the
    /// shape `AppState.provider_store` has) deref-coerces to it.
    pub async fn capture(store: &Mutex<ProviderConfigStore>) -> Self {
        let guard = store.lock().await;
        Self::from_store(&guard)
    }

    /// Non-async projection over a borrowed store — for call sites that
    /// already hold the guard for a legitimate reason, or for unit tests
    /// that build a store directly.
    pub fn from_store(store: &ProviderConfigStore) -> Self {
        let cfg = store.config();
        let Some(default_profile) = cfg.profiles.get("default") else {
            return Self::default();
        };
        let active_provider_id = empty_to_none(&default_profile.active_target.provider_id);
        let active_model_id = empty_to_none(&default_profile.active_target.model_id);
        Self {
            active_provider_id,
            active_model_id,
            providers: default_profile.providers.clone(),
        }
    }

    /// The active provider's profile, if any. Common read pattern behind
    /// `configure('api_key')` / `configure('base_url')`.
    pub fn active_profile(&self) -> Option<&ProviderProfile> {
        let id = self.active_provider_id.as_ref()?;
        self.providers.iter().find(|p| &p.id == id)
    }

    /// Wire projection — generates the legacy `ProviderConnection` DTO in
    /// exactly one place (ADR-0009 Decision 3: keep the wire type for now;
    /// retire it in a separate phase gated by the `Welcome.tsx` rewrite).
    pub fn to_providers_file(&self) -> ProvidersFile {
        let providers: Vec<ProviderConnection> = self
            .providers
            .iter()
            .map(|p| config::from_provider_profile(&p.id, p))
            .collect();
        ProvidersFile {
            active_provider_id: self.active_provider_id.clone(),
            providers,
        }
    }
}

fn empty_to_none(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shannon_types::provider_config::{CredentialRef, ProviderKind, ProviderTiers};

    fn store_with_profile(
        id: &str,
        kind: ProviderKind,
        base_url: &str,
        model_id: &str,
    ) -> Box<ProviderConfigStore> {
        let mut store = ProviderConfigStore::default();
        let profile = ProviderProfile {
            id: id.to_string(),
            kind,
            display_name: format!("{id} label"),
            base_url: base_url.to_string(),
            models_url: None,
            credential: CredentialRef::Store {
                service: id.to_string(),
            },
            extra_headers: std::collections::HashMap::new(),
            default_max_tokens: None,
            fallback_models: Vec::new(),
            quirks: Default::default(),
            tiers: ProviderTiers::default(),
        };
        store.upsert_profile(profile, model_id);
        Box::new(store)
    }

    #[test]
    fn snapshot_is_default_when_store_has_no_default_profile() {
        let store = ProviderConfigStore::default();
        let snap = ProviderReadSnapshot::from_store(&store);
        assert_eq!(snap.active_provider_id, None);
        assert_eq!(snap.active_model_id, None);
        assert!(snap.providers.is_empty());
    }

    #[test]
    fn snapshot_reports_active_provider_and_model_from_engine_store() {
        let store = store_with_profile(
            "anthropic-main",
            ProviderKind::Anthropic,
            "https://api.anthropic.com",
            "claude-opus-4-8",
        );
        let snap = ProviderReadSnapshot::from_store(&store);
        assert_eq!(snap.active_provider_id.as_deref(), Some("anthropic-main"));
        assert_eq!(snap.active_model_id.as_deref(), Some("claude-opus-4-8"));
        assert_eq!(snap.providers.len(), 1);
        assert_eq!(snap.providers[0].id, "anthropic-main");
    }

    #[test]
    fn active_profile_returns_the_canonical_profile() {
        let store = store_with_profile(
            "openai-main",
            ProviderKind::OpenAi,
            "https://api.openai.com",
            "gpt-4o",
        );
        let snap = ProviderReadSnapshot::from_store(&store);
        let active = snap
            .active_profile()
            .expect("active provider should be set");
        assert_eq!(active.id, "openai-main");
        assert_eq!(active.base_url, "https://api.openai.com");
    }

    #[test]
    fn active_profile_is_none_when_active_id_unsets() {
        let mut store = *store_with_profile(
            "anthropic-main",
            ProviderKind::Anthropic,
            "https://api.anthropic.com",
            "claude-opus-4-8",
        );
        store.remove_profile("anthropic-main");
        let snap = ProviderReadSnapshot::from_store(&store);
        assert!(snap.active_provider_id.is_none());
        assert!(snap.providers.is_empty());
        assert!(snap.active_profile().is_none());
    }

    #[test]
    fn to_providers_file_round_trips_wire_shape() {
        // Decision 3 acceptance: the wire shape (active id + ProviderConnection
        // vec) is identical to the legacy `providers_file_from_store` output.
        let store = store_with_profile(
            "anthropic-main",
            ProviderKind::Anthropic,
            "https://api.anthropic.com",
            "claude-opus-4-8",
        );
        let snap = ProviderReadSnapshot::from_store(&store);
        let file = snap.to_providers_file();
        assert_eq!(file.active_provider_id.as_deref(), Some("anthropic-main"));
        assert_eq!(file.providers.len(), 1);
        assert_eq!(file.providers[0].id, "anthropic-main");
        // API key must never serialize onto the wire (A1).
        assert_eq!(file.providers[0].api_key, None);
    }

    /// Row 5 (C3-b) — read-view parity. The desktop's
    /// `ProviderReadSnapshot::to_providers_file()` and the CLI's
    /// `ProviderConfigStore::config()` read the SAME store, so the desktop
    /// wire projection must faithfully reproduce the CLI's view: matching
    /// active id, a 1:1 provider vec (id + base_url), and no api_key on the
    /// wire. This is the ADR-0009 read-facade acceptance gate promoted to a
    /// multi-provider cross-surface check (the single-provider shape is
    /// pinned by `to_providers_file_round_trips_wire_shape` above).
    #[test]
    fn read_view_cli_vs_desktop_match() {
        let mut store = ProviderConfigStore::default();
        // Two providers of different kinds — the projection must keep both
        // distinct (no OpenAI collapse of the compatible slot) and preserve
        // each base_url.
        let anthropic = ProviderProfile {
            id: "anthropic-main".into(),
            kind: ProviderKind::Anthropic,
            display_name: "Anthropic".into(),
            base_url: "https://api.anthropic.com".into(),
            models_url: None,
            credential: CredentialRef::Store {
                service: "anthropic-main".into(),
            },
            extra_headers: std::collections::HashMap::new(),
            default_max_tokens: None,
            fallback_models: Vec::new(),
            quirks: Default::default(),
            tiers: ProviderTiers::default(),
        };
        let glm = ProviderProfile {
            id: "glm".into(),
            kind: ProviderKind::OpenAiCompatible,
            display_name: "GLM".into(),
            base_url: "https://open.bigmodel.cn/api/paas/v4".into(),
            models_url: None,
            credential: CredentialRef::Store {
                service: "glm".into(),
            },
            extra_headers: std::collections::HashMap::new(),
            default_max_tokens: None,
            fallback_models: Vec::new(),
            quirks: Default::default(),
            tiers: ProviderTiers::default(),
        };
        store.upsert_profile(anthropic, "claude-opus-4-8");
        store.upsert_profile(glm, "glm-4.6"); // last upsert → active

        // CLI side: the source-of-truth config both surfaces read from.
        let cli_default = store
            .config()
            .profiles
            .get("default")
            .expect("default profile")
            .clone();
        // Desktop side: the read facade's wire projection.
        let file = ProviderReadSnapshot::from_store(&store).to_providers_file();

        // Active target agrees across the two surfaces.
        assert_eq!(
            file.active_provider_id.as_deref(),
            Some(cli_default.active_target.provider_id.as_str()),
            "active provider id diverges between CLI config and desktop wire",
        );
        // Provider vec agrees on count.
        assert_eq!(
            file.providers.len(),
            cli_default.providers.len(),
            "provider count diverges between CLI config and desktop wire",
        );

        // Each desktop wire entry maps 1:1 to a CLI config provider by id,
        // with matching base_url and no api_key serialised.
        for conn in &file.providers {
            let cli_p = cli_default
                .providers
                .iter()
                .find(|p| p.id == conn.id)
                .unwrap_or_else(|| panic!("desktop wire id `{}` missing from CLI config", conn.id));
            assert_eq!(
                conn.base_url.as_deref(),
                Some(cli_p.base_url.as_str()),
                "base_url diverges for `{}`",
                conn.id
            );
            assert_eq!(
                conn.api_key, None,
                "api_key must never serialize onto the wire for `{}`",
                conn.id
            );
        }

        // Both providers survived the projection (no OpenAI collapse).
        let wire_ids: Vec<&str> = file.providers.iter().map(|p| p.id.as_str()).collect();
        assert!(
            wire_ids.contains(&"anthropic-main") && wire_ids.contains(&"glm"),
            "expected both providers on the wire, got {wire_ids:?}"
        );
    }

    #[test]
    fn snapshot_is_send_and_cloneable() {
        // Decision 2 contract: the snapshot is owned + Send so it can be
        // held across `.await` after the guard drops. This compiles only if
        // both hold.
        fn assert_send_clone<T: Send + Clone>() {}
        assert_send_clone::<ProviderReadSnapshot>();
    }
}
