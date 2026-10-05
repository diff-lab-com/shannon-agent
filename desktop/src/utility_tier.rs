//! S3-3 — utility tier 槽位化: the desktop's first consumption chain for the
//! providers.toml v2 `auxiliary: HashMap<AuxRole, ActiveTarget>` map (review
//! 2026-10-05 §6 S3-3, §5.2 战略机会; redteam §1.5 / 裁定⑦).
//!
//! Until this module the schema slot existed with **zero consumers** (every
//! reference in the repo was a `HashMap::new()` construction). This module is
//! the write path (Settings → Models "Utility slots" selects →
//! `set_utility_slot` → the surgical `ProviderConfigStore::set_auxiliary_target`)
//! plus the read side the send path consults.
//!
//! ## Scope ruling (裁定②) and the honest consumption verdict
//!
//! The batch scope is two slots: **compaction** (`AuxRole::Compression`) and
//! **session summary** (`AuxRole::TitleGeneration`). Their engine-side status
//! at this baseline differs, and this module does not paper over that:
//!
//! - **Compaction** has a real consumption point:
//!   `QueryEngine::with_auxiliary_compaction_client` — when the send path
//!   resolves `auxiliary.compression`, the agent loop's background
//!   summarization request (`CompactEngine::with_llm_summarizer`) goes to the
//!   auxiliary target (its own provider/base_url/credential/model).
//! - **Session summary** has NO LLM-backed generation point at this baseline:
//!   the desktop's session auto-title is deterministic truncation (no LLM
//!   call), the away-summary is rule-based, and `/handoff` is REPL-only. The
//!   slot is therefore landed as **schema + write path + UI ready** with no
//!   consumption wiring — deliberately NOT faked onto an unrelated call site
//!   (the "不要造假消费点" red line). It activates the moment a summary/title
//!   generation point reads `auxiliary.title_generation` through the same
//!   `lookup_auxiliary_target` helper.
//!
//! ## Orthogonality contract (裁定⑦)
//!
//! Utility slots serve the **background-task channel only** and never touch
//! the R5-5 interactive precedence chain
//! (`session override > phase tier > global default`):
//!
//! - [`lookup_auxiliary_target`] reads exactly one thing: the active model
//!   profile's `auxiliary` entry for the role. It takes no session state, no
//!   approval mode, no phase-tier preference, and no `DesktopConfig` — there
//!   is no code path through which a session pin or a phase tier could reach
//!   it (pinned structurally by the pure signature, behaviorally by the tests
//!   below).
//! - The `unattended_paths_pin_global_config` table is untouched: unattended
//!   run constructors keep reading `state.client_config` directly and never
//!   route through this module.
//! - The write path is a surgical field write that can never move the user's
//!   `active_target` (unlike `upsert_profile`, which repoints it).
//! - No client-config rebuild is performed after a slot write: the global
//!   default is definitionally unaffected.
//!
//! ## Failure semantics
//!
//! A configured slot whose provider slot has since vanished from the profile
//! roster (hand edit, profile switch, delete) **falls back to the default
//! behavior with a `warn`** — a stale utility slot must never break a
//! background task, mirroring the stale-session-override contract on the
//! interactive side.

use crate::commands::AppState;
use crate::commands_profiles::with_model_profile_store;
use crate::events;
use crate::events::event_names;
use shannon_types::provider_config::{ActiveTarget, AuxRole, ProviderModelConfig, ProviderProfile};

/// The two in-scope slots (裁定②). Order is the UI's display order:
/// compaction first (the consumed slot), session summary second.
pub const UTILITY_ROLES: [AuxRole; 2] = [AuxRole::Compression, AuxRole::TitleGeneration];

/// Canonical wire name of a role — the schema's serde `snake_case` spelling,
/// produced THROUGH serde so a future `AuxRole` variant can never drift from
/// the spelling providers.toml actually accepts (`AuxRole` is
/// `#[non_exhaustive]`; a static match here would silently mislabel it).
pub fn aux_role_slug(role: AuxRole) -> String {
    serde_json::to_value(role)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{role:?}"))
}

/// Strict parse of the canonical wire name (inverse of [`aux_role_slug`]):
/// accepts exactly the serde-recognized snake_case names and nothing else.
/// Aliases are deliberately NOT accepted — the value is written by the
/// desktop UI's dropdown, and a strict parser keeps `set_utility_slot`
/// honest (same posture as `phase_tier::PhaseTier::from_pref`).
pub fn aux_role_from_str(raw: &str) -> Option<AuxRole> {
    serde_json::from_value(serde_json::Value::String(raw.trim().to_string())).ok()
}

/// Outcome of looking up one auxiliary slot against a store snapshot.
/// The three states carry the exact fallback semantics the消费链 needs:
/// `NotConfigured` / `Dangling` both mean "use the default behavior", and
/// only `Resolved` carries a target.
#[derive(Debug, Clone, PartialEq)]
pub enum AuxLookup {
    /// No `auxiliary` entry for this role — the slot follows the default
    /// (the consumer rides the session's own model). The default state;
    /// users who never touch the feature stay here forever.
    NotConfigured,
    /// The slot names `(provider_id, model_id)` and `provider_id` exists in
    /// the active profile's roster. `slot` is that roster entry — the source
    /// for provider identity, base URL, credential and extra headers.
    Resolved {
        target: ActiveTarget,
        /// Boxed: `ProviderProfile` is several hundred bytes and the enum
        /// must stay cheap to move (clippy::large_enum_variant).
        slot: Box<ProviderProfile>,
    },
    /// The slot is configured but its provider no longer resolves in the
    /// roster (deleted / profile switched / hand edit). The consumer must
    /// fall back to the default behavior; the command layer `warn`s.
    Dangling {
        provider_id: String,
        model_id: String,
    },
}

/// The pure orthogonality seam: resolve one auxiliary role against a
/// providers.toml snapshot. Reads ONLY the active model profile's
/// `auxiliary` map and its roster — no session state, no phase tier, no
/// desktop config, no unattended pinning can reach this function (see the
/// module docs).
pub fn lookup_auxiliary_target(config: &ProviderModelConfig, role: AuxRole) -> AuxLookup {
    let Some(mp) = config.active_model_profile() else {
        return AuxLookup::NotConfigured;
    };
    let Some(target) = mp.auxiliary.get(&role) else {
        return AuxLookup::NotConfigured;
    };
    match mp.providers.iter().find(|p| p.id == target.provider_id) {
        Some(slot) => AuxLookup::Resolved {
            target: target.clone(),
            slot: Box::new(slot.clone()),
        },
        None => AuxLookup::Dangling {
            provider_id: target.provider_id.clone(),
            model_id: target.model_id.clone(),
        },
    }
}

/// Build the auxiliary target's [`shannon_engine::api::LlmClientConfig`] from
/// the resolved roster slot. Provider identity, base URL, credential and
/// extra headers resolve from the slot — the SAME sources the interactive
/// session-override path (`apply_session_override`) uses, so the auxiliary
/// request is indistinguishable from a first-class target of that provider.
///
/// Everything else stays at the engine defaults: a utility client inherits
/// none of the interactive chain's behavioral knobs (max_tokens, timeout,
/// reasoning effort, and — deliberately — NOT the failover chain or the
/// session-pin `suppress_failover` flag, which are interactive-chain
/// semantics).
pub fn auxiliary_client_config(
    resolved: &AuxLookup,
) -> Option<shannon_engine::api::LlmClientConfig> {
    use shannon_core::provider_resolver::{
        llm_provider_from_slug, resolve_credential, resolve_provider,
    };

    let AuxLookup::Resolved { target, slot } = resolved else {
        return None;
    };
    let provider = llm_provider_from_slug(&slot.id)
        .unwrap_or_else(|| resolve_provider(&slot.kind, &slot.base_url));
    Some(shannon_engine::api::LlmClientConfig {
        api_key: resolve_credential(&slot.credential),
        base_url: slot.base_url.clone(),
        model: target.model_id.clone(),
        provider,
        extra_headers: slot.extra_headers.clone(),
        ..shannon_engine::api::LlmClientConfig::default()
    })
}

/// The send path's seam: resolve the compaction utility slot into a client,
/// or `None` for the default behavior. A dangling slot logs the fallback
/// warn here (once per send) — the engine-side consumer just sees `None`.
pub(crate) async fn resolve_auxiliary_client(
    state: &AppState,
    role: AuxRole,
) -> Option<shannon_engine::api::LlmClient> {
    let lookup = {
        let store = state.provider_store.lock().await;
        lookup_auxiliary_target(store.config(), role)
    };
    match &lookup {
        AuxLookup::NotConfigured => None,
        AuxLookup::Dangling {
            provider_id,
            model_id,
        } => {
            tracing::warn!(
                role = aux_role_slug(role),
                provider = %provider_id,
                model = %model_id,
                "utility slot target no longer resolvable (provider absent from the active \
                 profile roster) — falling back to the default model for this background task"
            );
            None
        }
        AuxLookup::Resolved { .. } => {
            auxiliary_client_config(&lookup).map(shannon_engine::api::LlmClient::new)
        }
    }
}

// ---------------------------------------------------------------------------
// Wire surface: Settings → Models "Utility slots"
// ---------------------------------------------------------------------------

/// One slot row of `get_utility_slots`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UtilitySlotStatus {
    /// Canonical role slug (`compression` | `title_generation`).
    pub role: String,
    /// Configured provider slot id, `None` = follows the global default.
    pub provider: Option<String>,
    /// Configured model id, `None` = follows the global default.
    pub model: Option<String>,
    /// False when the slot is configured but its provider no longer resolves
    /// in the roster — the UI renders the fallback warning for this state.
    pub resolves: bool,
}

/// One roster row offered as a slot target: a provider slot in the active
/// profile plus the models the slot can serve.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UtilityRosterEntry {
    pub provider_id: String,
    pub display_name: String,
    /// Candidate model ids: the slot's curated declarations (S2-1 vault)
    /// first, then the slot's tier-resolved models, then — when the slot IS
    /// the profile's active target with a concrete id — that model. Deduped,
    /// order stable.
    pub models: Vec<String>,
}

/// Read shape of `get_utility_slots`: the two slot rows plus the candidate
/// roster, in one snapshot so the section renders from a consistent store.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UtilitySlotsView {
    pub slots: Vec<UtilitySlotStatus>,
    pub roster: Vec<UtilityRosterEntry>,
    /// The active profile key the view was built from (display-only).
    pub profile: String,
}

/// Candidate models of one roster slot (see [`UtilityRosterEntry::models`]).
fn slot_candidates(
    mp: &shannon_types::provider_config::ModelProfile,
    slot: &ProviderProfile,
) -> Vec<String> {
    use shannon_core::provider_resolver::{llm_provider_from_slug, resolve_provider};

    let mut out: Vec<String> = Vec::new();
    let push = |id: &str, out: &mut Vec<String>| {
        let id = id.trim();
        if !id.is_empty() && !out.iter().any(|e| e == id) {
            out.push(id.to_string());
        }
    };
    // 1. Curated declarations (the S2-1 model vault).
    for spec in &slot.models {
        push(&spec.id, &mut out);
    }
    // 2. Tier-resolved models — the same resolution the phase-tier preview
    //    displays, so both dropdown families offer the same vocabulary.
    let provider = llm_provider_from_slug(&slot.id)
        .unwrap_or_else(|| resolve_provider(&slot.kind, &slot.base_url));
    for tier in ["pro", "standard", "fast"] {
        if let Some(id) = shannon_core::model_registry::resolve_tier(tier, &provider, &slot.tiers) {
            push(&id, &mut out);
        }
    }
    // 3. The slot's concrete active-target model, when it is one.
    if mp.active_target.provider_id == slot.id {
        push(&mp.active_target.model_id, &mut out);
    }
    out
}

/// Pure core of [`get_utility_slots`] (also the unit-test seam).
pub(crate) fn utility_slots_view(config: &ProviderModelConfig) -> UtilitySlotsView {
    let profile = config.active_profile_key().to_string();
    let mp = config.active_model_profile();
    let slots = UTILITY_ROLES
        .iter()
        .map(|&role| {
            let (provider, model, resolves) = match lookup_auxiliary_target(config, role) {
                AuxLookup::NotConfigured => (None, None, false),
                AuxLookup::Resolved { target, .. } => {
                    (Some(target.provider_id), Some(target.model_id), true)
                }
                AuxLookup::Dangling {
                    provider_id,
                    model_id,
                } => (Some(provider_id), Some(model_id), false),
            };
            UtilitySlotStatus {
                role: aux_role_slug(role).to_string(),
                provider,
                model,
                resolves,
            }
        })
        .collect();
    let roster = mp
        .map(|mp| {
            mp.providers
                .iter()
                .map(|slot| UtilityRosterEntry {
                    provider_id: slot.id.clone(),
                    display_name: slot.display_name.clone(),
                    models: slot_candidates(mp, slot),
                })
                .collect()
        })
        .unwrap_or_default();
    UtilitySlotsView {
        slots,
        roster,
        profile,
    }
}

/// Read the two utility slots + the candidate roster for Settings → Models.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn get_utility_slots(
    state: tauri::State<'_, AppState>,
) -> Result<UtilitySlotsView, String> {
    let config = {
        let store = state.provider_store.lock().await;
        store.config().clone()
    };
    Ok(utility_slots_view(&config))
}

/// Echo of a committed slot write.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UtilitySlotOutcome {
    pub role: String,
    /// `None` = the slot was cleared (follows the global default again).
    pub provider: Option<String>,
    pub model: Option<String>,
}

/// Write (or clear) one utility slot. `provider` + `model` both `None` (or
/// empty strings) clear the slot; both `Some` assign it. The provider must
/// exist in the active profile's roster — a target that could never resolve
/// is rejected at write time (the same contract `set_session_model` holds),
/// and the write is the surgical store mutator, so the user's active
/// provider/model pointer can never move as a side effect.
///
/// Deliberately NO client-config rebuild: the utility slots feed the
/// background-task channel only — the interactive global default is
/// definitionally unaffected (that is the orthogonality红线, not an
/// optimization).
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn set_utility_slot(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    role: String,
    provider: Option<String>,
    model: Option<String>,
) -> Result<UtilitySlotOutcome, String> {
    use tauri::Emitter;

    let role = aux_role_from_str(&role)
        .ok_or_else(|| format!("set_utility_slot: unknown utility role `{role}`"))?;

    let provider = provider.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let model = model.as_deref().map(str::trim).filter(|s| !s.is_empty());
    match (provider, model) {
        (None, None) => {
            let existed = with_model_profile_store(&state, |store| {
                store
                    .clear_auxiliary_target(role)
                    .map_err(|e| format!("could not clear the utility slot: {e}"))
            })
            .await?;
            if !existed {
                tracing::debug!(role = aux_role_slug(role), "utility slot already clear");
            }
        }
        (Some(provider), Some(model)) => {
            let provider = provider.to_string();
            let model = model.to_string();
            with_model_profile_store(&state, |store| {
                store
                    .set_auxiliary_target(role, &provider, &model)
                    .map_err(|e| {
                        format!(
                            "could not persist the utility slot for `{}`: {e}",
                            aux_role_slug(role)
                        )
                    })
            })
            .await?;
        }
        _ => {
            return Err(
                "set_utility_slot: provider and model must be set (or cleared) together"
                    .to_string(),
            );
        }
    }

    let _ = app_handle.emit(
        event_names::CONFIG_UPDATED,
        events::ConfigUpdatedPayload {
            key: "utility_slots".into(),
            value: aux_role_slug(role).to_string(),
        },
    );

    Ok(UtilitySlotOutcome {
        role: aux_role_slug(role).to_string(),
        provider: provider.map(str::to_string),
        model: model.map(str::to_string),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use shannon_core::provider_config_store::ProviderConfigStore;
    use shannon_types::provider_config::{
        CredentialRef, CredentialScope, ModelProfile, ProviderKind, ProviderTiers, Scope,
    };
    use std::collections::HashMap;

    /// Two managed providers (the active Anthropic connection + an
    /// OpenAI-compatible GLM slot) — the same minimal roster shape the
    /// session-override fixtures use, so the orthogonality pin mirrors the
    /// interactive precedence tests.
    fn fixture_config() -> ProviderModelConfig {
        let anthropic = ProviderProfile {
            id: "anthropic".to_string(),
            kind: ProviderKind::Anthropic,
            display_name: "Anthropic".to_string(),
            base_url: "https://api.anthropic.com".to_string(),
            models_url: None,
            credential: CredentialRef::Env {
                var: "SOR_TEST_ANTHROPIC_KEY".to_string(),
            },
            extra_headers: HashMap::new(),
            default_max_tokens: None,
            fallback_models: Vec::new(),
            quirks: Default::default(),
            tiers: ProviderTiers::default(),
            models: Vec::new(),
        };
        let mut glm_headers = HashMap::new();
        glm_headers.insert("X-Glm".to_string(), "yes".to_string());
        let glm_tiers = ProviderTiers {
            fast: Some("glm-5.3-air".to_string()),
            ..Default::default()
        };
        let glm = ProviderProfile {
            id: "zhipu".to_string(),
            kind: ProviderKind::OpenAiCompatible,
            display_name: "GLM (Zhipu)".to_string(),
            base_url: "https://open.bigmodel.cn/api/paas/v4".to_string(),
            models_url: None,
            credential: CredentialRef::Env {
                var: "SOR_TEST_GLM_KEY".to_string(),
            },
            extra_headers: glm_headers,
            default_max_tokens: None,
            fallback_models: Vec::new(),
            quirks: Default::default(),
            tiers: glm_tiers,
            models: Vec::new(),
        };
        let mut profiles = HashMap::new();
        profiles.insert(
            "default".to_string(),
            ModelProfile {
                name: "default".to_string(),
                active_target: ActiveTarget {
                    provider_id: "anthropic".to_string(),
                    model_id: "claude-sonnet-4-6".to_string(),
                    scope: Scope::Global,
                },
                providers: vec![anthropic, glm],
                auxiliary: HashMap::new(),
                credential_scope: CredentialScope::Shared,
            },
        );
        ProviderModelConfig {
            version: ProviderModelConfig::VERSION,
            active_profile: String::new(),
            profiles,
            gateway: Default::default(),
        }
    }

    fn config_with_aux(role: AuxRole, provider: &str, model: &str) -> ProviderModelConfig {
        let mut cfg = fixture_config();
        let mp = cfg.active_model_profile_mut().unwrap();
        mp.auxiliary.insert(
            role,
            ActiveTarget {
                provider_id: provider.to_string(),
                model_id: model.to_string(),
                scope: Scope::Global,
            },
        );
        cfg
    }

    #[test]
    fn aux_role_slug_round_trips_strictly() {
        for &role in UTILITY_ROLES.iter() {
            assert_eq!(aux_role_from_str(&aux_role_slug(role)), Some(role));
        }
        // The consumed slot's canonical wire name is the schema's.
        assert_eq!(aux_role_slug(AuxRole::Compression), "compression");
        assert_eq!(aux_role_slug(AuxRole::TitleGeneration), "title_generation");
        // Strict parser: no aliases, no junk.
        assert_eq!(aux_role_from_str("compaction"), None);
        assert_eq!(aux_role_from_str("title-generation"), None);
        assert_eq!(aux_role_from_str(""), None);
        assert_eq!(aux_role_from_str("summarize"), None);
    }

    #[test]
    fn unconfigured_slot_reports_not_configured() {
        let cfg = fixture_config();
        assert_eq!(
            lookup_auxiliary_target(&cfg, AuxRole::Compression),
            AuxLookup::NotConfigured
        );
        // A dangling ACTIVE pointer (no profile at all) degrades the same way.
        let mut ghost = fixture_config();
        ghost.active_profile = "ghost".to_string();
        assert_eq!(
            lookup_auxiliary_target(&ghost, AuxRole::Compression),
            AuxLookup::NotConfigured
        );
    }

    #[test]
    fn configured_slot_resolves_against_the_exact_roster_id() {
        let cfg = config_with_aux(AuxRole::Compression, "zhipu", "glm-5.3-air");
        let AuxLookup::Resolved { target, slot } =
            lookup_auxiliary_target(&cfg, AuxRole::Compression)
        else {
            panic!("expected Resolved");
        };
        assert_eq!(target.provider_id, "zhipu");
        assert_eq!(target.model_id, "glm-5.3-air");
        // The slot carries the credential/endpoint sources the client
        // builder needs.
        assert_eq!(slot.base_url, "https://open.bigmodel.cn/api/paas/v4");
    }

    /// The红线 pin: a configured utility slot resolves to ITS OWN target —
    /// never to the profile's active target — even when the interactive
    /// chain (session override > phase tier > global default) points
    /// somewhere else entirely. The lookup takes only `(config, role)`, so
    /// no session pin, phase tier or unattended mode can influence it
    /// structurally; this test pins the behavior against the actual
    /// interactive resolution for contrast.
    #[tokio::test]
    async fn utility_slot_is_orthogonal_to_the_interactive_precedence_chain() {
        use crate::commands::AppState;

        // Utility slot → zhipu/glm-5.3-air, while the global default (and
        // therefore every interactive resolution) stays anthropic.
        let cfg = config_with_aux(AuxRole::Compression, "zhipu", "glm-5.3-air");
        let state = AppState::new();
        *state.provider_store.lock().await = ProviderConfigStore::from_config(cfg.clone());

        // 1. The interactive chain resolves to the ACTIVE target…
        let session_target = cfg
            .active_model_profile()
            .map(|mp| mp.active_target.model_id.clone())
            .unwrap_or_default();
        assert_eq!(session_target, "claude-sonnet-4-6");
        // 2. …while the utility resolver — reading the very same store
        //    snapshot — resolves the slot target. Same config, different
        //    channel: the slot is invisible to the interactive chain and the
        //    chain is invisible to the slot.
        let client = resolve_auxiliary_client(&state, AuxRole::Compression)
            .await
            .expect("slot resolves");
        assert_eq!(client.model(), "glm-5.3-air");
        assert_eq!(client.base_url(), "https://open.bigmodel.cn/api/paas/v4");
        assert_eq!(*client.provider(), shannon_engine::api::LlmProvider::Zhipu);

        // 3. A session override / phase tier CANNOT be expressed to this
        //    resolver — pin it by construction: re-resolving with the
        //    interactive layers "set" (they live on SessionState /
        //    DesktopConfig, none of which the resolver reads) yields the
        //    identical client. The resolution is a pure function of the
        //    store snapshot + role.
        let again = resolve_auxiliary_client(&state, AuxRole::Compression)
            .await
            .unwrap();
        assert_eq!(again.model(), client.model());
    }

    /// The fallback pin: a slot whose provider vanished from the roster
    /// resolves to NONE (the consumer keeps the default behavior) — and the
    /// warn side effect is the resolver's job, not the consumer's.
    #[tokio::test]
    async fn dangling_utility_slot_falls_back_to_default() {
        use crate::commands::AppState;

        let cfg = config_with_aux(AuxRole::Compression, "ghost", "vanished-model");
        let state = AppState::new();
        *state.provider_store.lock().await = ProviderConfigStore::from_config(cfg);

        match lookup_auxiliary_target(
            state.provider_store.lock().await.config(),
            AuxRole::Compression,
        ) {
            AuxLookup::Dangling {
                provider_id,
                model_id,
            } => {
                assert_eq!(provider_id, "ghost");
                assert_eq!(model_id, "vanished-model");
            }
            other => panic!("expected Dangling, got {other:?}"),
        }
        assert!(
            resolve_auxiliary_client(&state, AuxRole::Compression)
                .await
                .is_none(),
            "a dangling slot must fall back to the default (None = session client)"
        );
        // The OTHER slot is unaffected by the dangling one.
        assert!(
            resolve_auxiliary_client(&state, AuxRole::TitleGeneration)
                .await
                .is_none()
        );
    }

    #[test]
    fn auxiliary_client_config_resolves_credential_headers_and_defaults() {
        // SAFETY: unique var name touched only by this test; nextest runs
        // each test in its own process.
        unsafe {
            std::env::set_var("SOR_TEST_GLM_KEY", "glm-secret");
        }
        let cfg = config_with_aux(AuxRole::Compression, "zhipu", "glm-5.3-air");
        let lookup = lookup_auxiliary_target(&cfg, AuxRole::Compression);
        let out = auxiliary_client_config(&lookup).expect("resolves");

        assert_eq!(out.api_key, "glm-secret", "credential from the slot");
        assert_eq!(
            out.extra_headers.get("X-Glm").map(String::as_str),
            Some("yes"),
            "profile extra_headers apply"
        );
        assert_eq!(out.model, "glm-5.3-air");
        // Behavioral knobs stay engine-defaults — the utility client inherits
        // NOTHING from the interactive chain (no failover suppression, no
        // tuned retry policy, no effort dial).
        let base = shannon_engine::api::LlmClientConfig::default();
        assert_eq!(
            out.retry_config.suppress_failover, base.retry_config.suppress_failover,
            "no session-pin semantics leak into the utility client"
        );
        assert_eq!(out.max_tokens, base.max_tokens);
        assert_eq!(out.reasoning_effort, None);

        // NotConfigured / Dangling build nothing.
        assert!(auxiliary_client_config(&AuxLookup::NotConfigured).is_none());
    }

    #[test]
    fn utility_slots_view_reports_both_slots_and_the_roster() {
        let mut cfg = config_with_aux(AuxRole::Compression, "zhipu", "glm-5.3-air");
        // A dangling title_generation slot for the resolves=false row.
        cfg.active_model_profile_mut().unwrap().auxiliary.insert(
            AuxRole::TitleGeneration,
            ActiveTarget {
                provider_id: "ghost".to_string(),
                model_id: "m".to_string(),
                scope: Scope::Global,
            },
        );

        let view = utility_slots_view(&cfg);
        assert_eq!(view.profile, "default");
        assert_eq!(view.slots.len(), 2);
        let compression = &view.slots[0];
        assert_eq!(compression.role, "compression");
        assert_eq!(compression.provider.as_deref(), Some("zhipu"));
        assert_eq!(compression.model.as_deref(), Some("glm-5.3-air"));
        assert!(compression.resolves);
        let summary = &view.slots[1];
        assert_eq!(summary.role, "title_generation");
        assert_eq!(summary.provider.as_deref(), Some("ghost"));
        assert!(
            !summary.resolves,
            "dangling slot must surface as non-resolving"
        );

        // The roster offers both slots with candidate models: the tier pin
        // (glm-5.3-air) resolves for zhipu; anthropic has catalog tiers.
        let zhipu = view
            .roster
            .iter()
            .find(|r| r.provider_id == "zhipu")
            .unwrap();
        assert!(zhipu.models.iter().any(|m| m == "glm-5.3-air"));
        assert_eq!(zhipu.display_name, "GLM (Zhipu)");
        let anthropic = view
            .roster
            .iter()
            .find(|r| r.provider_id == "anthropic")
            .unwrap();
        assert!(
            anthropic.models.iter().any(|m| m.contains("haiku")),
            "anthropic's catalog fast tier is a candidate: {:?}",
            anthropic.models
        );
        // The active target's concrete model is offered for its slot.
        assert!(anthropic.models.iter().any(|m| m == "claude-sonnet-4-6"));
    }

    #[test]
    fn utility_slots_view_defaults_to_empty_when_no_profile() {
        let view = utility_slots_view(&ProviderModelConfig::default());
        assert_eq!(view.slots.len(), UTILITY_ROLES.len());
        assert!(view.slots.iter().all(|s| s.provider.is_none()));
        assert!(view.roster.is_empty());
    }
}
