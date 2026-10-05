//! S2-1 (模型仓固化) — the desktop's curated per-provider model vault.
//!
//! `Fetch models` results used to die in a transient `<datalist>` (review
//! P-N7). This module is the persistence half of the cure: a Tauri command
//! that固ies the user's multi-select into the provider slot's
//! `models: Vec<ModelSpec>` in `providers.toml` v2 (schema support landed in
//! R2-4; this is the desktop's first write entry).
//!
//! Write path: the same mutex+flock critical section
//! `commands_profiles::with_model_profile_store` uses (in-process provider
//! store mutex first, cross-process `providers.toml` flock second, reload →
//! mutate → persist inside it, unconditional restore). The store mutator
//! (`ProviderConfigStore::set_provider_models`) validates every spec and
//! enforces unique ids, so an invalid batch can never reach disk. Overwrite
//! semantics — the curated selection is authoritative; an empty selection
//! clears the vault (the picker then falls back to the unfiltered catalog,
//! 裁定③'s soft whitelist).
//!
//! After a successful write the global client config is rebuilt (the same
//! rebuild a profile switch performs): client construction re-registers the
//! declarations in `declared_models` and picks up the S2-3 `max_output`
//! clamp, so the next request follows the new vault without a restart.
//! `CONFIG_UPDATED { key: "provider_models" }` tells open windows to
//! refresh their catalog views.
//!
//! Capacity guardrails (裁定⑥) are **UI policy** (default zero selected,
//! soft cap 50, select-all confirm) enforced in `AddProviderModal`; the
//! command validates schema semantics only.

use crate::commands::AppState;
use crate::commands_profiles::with_model_profile_store;
use crate::events;
use crate::events::event_names;
use shannon_types::provider_config::{ModelCapability, ModelSpec};

/// Wire shape for one curated model. Only `id` is required — a user without
/// metadata固ies id-only specs (`{"id": "proxy-model-x"}`) and the catalog
/// keeps supplying pricing/context/vision for ids it knows (裁定③'s
/// "catalog/overlay = metadata supply" pin). Capability names are the
/// schema's snake_case set (`vision`, `tool_use`, …) so the wire stays
/// human-inspectable.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredModelInput {
    pub id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub context_window: Option<u32>,
    #[serde(default)]
    pub max_output: Option<u32>,
    #[serde(default)]
    pub cost_per_m_input: Option<f64>,
    #[serde(default)]
    pub cost_per_m_output: Option<f64>,
    #[serde(default)]
    pub capabilities: Option<Vec<String>>,
}

/// Parse one capability name into the schema enum. Same accepted set as the
/// store's serde layer (an unknown name is a schema error there) but with a
/// friendlier message naming the offending entry.
pub(crate) fn parse_capability(name: &str) -> Result<ModelCapability, String> {
    match name.trim().to_ascii_lowercase().as_str() {
        "reasoning" => Ok(ModelCapability::Reasoning),
        "coding" => Ok(ModelCapability::Coding),
        "speed" => Ok(ModelCapability::Speed),
        "cheap" => Ok(ModelCapability::Cheap),
        "vision" => Ok(ModelCapability::Vision),
        "tool_use" | "tools" | "tool-use" => Ok(ModelCapability::ToolUse),
        other => Err(format!(
            "unknown capability '{other}' — expected one of: reasoning, coding, speed, cheap, vision, tool_use"
        )),
    }
}

impl DeclaredModelInput {
    /// Project onto the schema `ModelSpec` with per-entry error context.
    pub fn into_spec(self, index: usize) -> Result<ModelSpec, String> {
        let mut capabilities = Vec::new();
        for name in self.capabilities.unwrap_or_default() {
            capabilities
                .push(parse_capability(&name).map_err(|e| format!("models[{index}]: {e}"))?);
        }
        let spec = ModelSpec {
            id: self.id,
            display_name: self.display_name,
            context_window: self.context_window,
            max_output: self.max_output,
            cost_per_m_input: self.cost_per_m_input,
            cost_per_m_output: self.cost_per_m_output,
            capabilities,
        };
        spec.validate()
            .map_err(|e| format!("models[{index}]: {e}"))?;
        Ok(spec)
    }
}

/// Echo of the committed vault — the UI re-renders its selection state from
/// this instead of assuming the write landed verbatim.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProviderModelsOutcome {
    /// The provider slot id the vault was written to.
    pub provider_id: String,
    /// The model profile key that was written ("default" or a named one).
    pub model_profile: String,
    /// The stored declarations (validated, canonical order).
    pub models: Vec<ModelSpec>,
}

// ---------------------------------------------------------------------------
// S3-4 (推荐降级链) — one-click recommended fallback chain.
//
// The engine's failover chain (R3-1) is fully wired: `fallback_models` on the
// active provider slot resolves through `resolve_failover_chain` and fires on
// 429/5xx before a stream starts. What was missing is the last mile: writing
// a good chain requires knowing the tier vocabulary and the qualified-id
// syntax, so the review (§6 S3-4 / §3 P-N13) asks for a one-click
// recommendation. Two commands:
//
// - `recommend_fallback_chain` — PURE COMPUTATION over the profile roster +
//   the tier catalog. Returns candidate hops; nothing is persisted and no
//   failover is enabled (opt-in stays opt-in until the user explicitly
//   applies).
// - `set_provider_fallback_models` — the explicit-confirmation write, through
//   the same store critical section `set_provider_models` uses and the
//   surgical `set_provider_fallback_models` field mutator, so applying a
//   chain can never move the user's active provider/model (the service-level
//   `upsert` repoints `active_target` and would do exactly that).
//
// Recommendation rules (pinned by the tests below):
//   1. Same family first: tier-resolved alternates of the provider itself,
//      walking tiers DESCENDING (pro → standard → fast — flagship first), at
//      most [`SAME_FAMILY_HOPS`] entries. Bare ids: same provider, model
//      swap. The tier walk reuses `resolve_tier`, so `providers.toml` tier
//      pins win, then catalog inference (declared models included).
//   2. Cross-provider fill: when the family yields fewer than
//      [`SAME_FAMILY_HOPS`] candidates, the remaining capacity (up to the
//      engine's [`MAX_FAILOVER_TARGETS`]) is filled from the SAME model
//      profile's other connected slots, one hop per provider (endpoint
//      diversity beats depth on a single backup), same tier-descending
//      walk. Qualified `provider/model` entries — exactly the syntax
//      `resolve_failover_chain` parses as a provider switch.
//   3. Never recommend the target itself: the slot's concrete current model
//      (known only when the slot IS the profile's active target with a
//      non-placeholder id) and duplicates are excluded, so the generated
//      chain is accepted by `resolve_failover_chain` without skips.
// ---------------------------------------------------------------------------

/// Same-family cap: a rich provider gets two clean intra-family hops
/// (flagship downgrade + budget downgrade) and stops there.
pub(crate) const SAME_FAMILY_HOPS: usize = 2;

/// One hop of a recommended chain. `entry` is the literal `fallback_models`
/// string; the rest is presentation/semantics data for the confirmation UI.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FallbackHop {
    /// Bare model id (same-provider hop) or `provider/model` (switch hop) —
    /// drop-in for the provider slot's `fallback_models` list.
    pub entry: String,
    /// The concrete model id (without the provider prefix).
    pub model: String,
    /// Owning provider slot id in the profile roster.
    pub provider_id: String,
    /// Display label of the owning slot (for the UI's per-hop line).
    pub provider_label: String,
    /// True for a bare entry: stays on the recommending provider, model swap
    /// only. False: the hop switches provider within the profile roster.
    pub same_provider: bool,
    /// Canonical tier the hop was picked for: `pro` | `standard` | `fast`.
    pub tier: String,
}

/// A recommended chain — candidates only; nothing is persisted by
/// `recommend_fallback_chain`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RecommendedFallbackChain {
    /// The provider slot the chain was generated for.
    pub provider_id: String,
    /// The model profile key the roster was read from.
    pub model_profile: String,
    /// The slot's concrete current model when known (the recommendation
    /// never includes it), `None` when the slot is not the profile's active
    /// target (or its active id is the `"default"` placeholder).
    pub current_model: Option<String>,
    /// Candidate hops in application order (index 0 = first failover hop).
    pub hops: Vec<FallbackHop>,
}

/// Echo of the committed chain from `set_provider_fallback_models` (sanitized
/// to exactly what was stored).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProviderFallbackOutcome {
    pub provider_id: String,
    pub model_profile: String,
    pub fallback_models: Vec<String>,
}

/// Resolve a roster slot to its engine `LlmProvider`: stored id first (the
/// desktop slug vocabulary — "glm", "deepseek", …), then kind/base-url
/// detection for fully-custom ids. Same order `resolve_failover_chain` uses.
fn slot_llm_provider(
    slot: &shannon_types::provider_config::ProviderProfile,
) -> shannon_engine::api::LlmProvider {
    use shannon_core::provider_resolver::{llm_provider_from_slug, resolve_provider};
    llm_provider_from_slug(&slot.id).unwrap_or_else(|| resolve_provider(&slot.kind, &slot.base_url))
}

/// The pure recommendation core (see the module docs for the rules).
/// `cfg`/`profile_key`/`provider_id` mirror what `resolve_failover_chain`
/// reads, so a generated chain is by construction compatible with it.
pub(crate) fn recommend_chain_for(
    cfg: &shannon_types::provider_config::ProviderModelConfig,
    profile_key: &str,
    provider_id: &str,
) -> Result<RecommendedFallbackChain, String> {
    use shannon_core::model_registry::resolve_tier;
    use shannon_engine::api::retry::MAX_FAILOVER_TARGETS;

    let mp = cfg.profiles.get(profile_key).ok_or_else(|| {
        format!(
            "recommend_fallback_chain: no model profile named '{profile_key}' in providers.toml"
        )
    })?;
    let slot = mp
        .providers
        .iter()
        .find(|p| p.id == provider_id)
        .ok_or_else(|| {
            format!(
                "recommend_fallback_chain: provider '{provider_id}' is not in the '{profile_key}' \
             profile roster"
            )
        })?;
    let home = slot_llm_provider(slot);

    // The slot's concrete current model — only knowable when it IS the
    // profile's active target and the id is concrete (not the "default"
    // placeholder a fresh activation writes).
    let current_model = (mp.active_target.provider_id == slot.id
        && !mp.active_target.model_id.is_empty()
        && mp.active_target.model_id != "default")
        .then(|| mp.active_target.model_id.clone());

    let mut hops: Vec<FallbackHop> = Vec::new();
    let mut taken: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Phase 1 — same family, tiers descending (pro → standard → fast).
    for tier in ["pro", "standard", "fast"] {
        if hops.len() >= SAME_FAMILY_HOPS {
            break;
        }
        let Some(id) = resolve_tier(tier, &home, &slot.tiers) else {
            continue;
        };
        let id = id.trim();
        if id.is_empty() || !taken.insert(id.to_string()) || current_model.as_deref() == Some(id) {
            continue;
        }
        hops.push(FallbackHop {
            entry: id.to_string(),
            model: id.to_string(),
            provider_id: slot.id.clone(),
            provider_label: slot.display_name.clone(),
            same_provider: true,
            tier: tier.to_string(),
        });
    }

    // Phase 2 — thin family: fill remaining capacity from the same profile's
    // other connected slots, one hop per provider, same tier walk. Qualified
    // entries only (`provider/model` — the switch-provider syntax).
    if hops.len() < SAME_FAMILY_HOPS {
        'tiers: for tier in ["pro", "standard", "fast"] {
            for other in &mp.providers {
                if hops.len() >= MAX_FAILOVER_TARGETS {
                    break 'tiers;
                }
                if other.id == slot.id || hops.iter().any(|h| h.provider_id == other.id) {
                    continue;
                }
                let other_provider = slot_llm_provider(other);
                let Some(id) = resolve_tier(tier, &other_provider, &other.tiers) else {
                    continue;
                };
                let id = id.trim();
                if id.is_empty() {
                    continue;
                }
                // An other-slot candidate that canonicalizes to the home
                // provider AND carries the current model would resolve to the
                // active target inside `resolve_failover_chain` and be
                // skipped — don't recommend it.
                if other_provider == home && current_model.as_deref() == Some(id) {
                    continue;
                }
                let entry = format!("{}/{id}", other.id);
                if !taken.insert(entry.clone()) {
                    continue;
                }
                hops.push(FallbackHop {
                    entry,
                    model: id.to_string(),
                    provider_id: other.id.clone(),
                    provider_label: other.display_name.clone(),
                    same_provider: false,
                    tier: tier.to_string(),
                });
            }
        }
    }

    Ok(RecommendedFallbackChain {
        provider_id: slot.id.clone(),
        model_profile: profile_key.to_string(),
        current_model,
        hops,
    })
}

/// Trim + drop-empty + order-preserving dedupe for incoming
/// `fallback_models` entries. Shared shape with what the AddProviderModal
/// already does client-side (`map(trim).filter(Boolean)`), plus the dedupe
/// `resolve_failover_chain` would otherwise waste hops on.
pub(crate) fn sanitize_fallback_entries(entries: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    entries
        .into_iter()
        .map(|e| e.trim().to_string())
        .filter(|e| !e.is_empty() && seen.insert(e.clone()))
        .collect()
}

/// S3-4: compute a recommended fallback chain for one provider slot
/// (candidates only — never persisted, never enables anything). Reads the
/// profile roster from the engine store and the tier catalog through
/// `resolve_tier` (the same resolution the Settings tier UI displays).
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn recommend_fallback_chain(
    state: tauri::State<'_, AppState>,
    provider_id: String,
    profile: Option<String>,
) -> Result<RecommendedFallbackChain, String> {
    let provider_id = provider_id.trim().to_string();
    if provider_id.is_empty() {
        return Err("recommend_fallback_chain: provider_id must not be empty".to_string());
    }
    if let Some(name) = profile.as_deref() {
        shannon_types::provider_config::validate_profile_name(name)
            .map_err(|e| format!("recommend_fallback_chain: {e}"))?;
    }
    // Snapshot-then-release: the recommendation is a pure computation, the
    // roster lock is only needed to clone the config out.
    let cfg = {
        let store = state.provider_store.lock().await;
        store.config().clone()
    };
    let profile_key = match profile.as_deref() {
        Some(name) => name.to_string(),
        None => cfg.active_profile_key().to_string(),
    };
    recommend_chain_for(&cfg, &profile_key, &provider_id)
}

/// S3-4: persist the user-confirmed fallback chain for one provider slot.
/// The chain is sanitized (trim / drop-empty / dedupe) and capped at the
/// engine's `MAX_FAILOVER_TARGETS` — an oversized chain is rejected with a
/// clear error instead of being silently truncated at resolve time. The
/// write is the surgical store mutator under the same mutex+flock critical
/// section `set_provider_models` uses, so the active provider/model pointer
/// can never move as a side effect (the behavioral red line: failover is
/// only ever enabled by THIS explicit user action).
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn set_provider_fallback_models(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    provider_id: String,
    fallback_models: Vec<String>,
    profile: Option<String>,
) -> Result<ProviderFallbackOutcome, String> {
    use tauri::Emitter;

    let provider_id = provider_id.trim().to_string();
    if provider_id.is_empty() {
        return Err("set_provider_fallback_models: provider_id must not be empty".to_string());
    }
    if let Some(name) = profile.as_deref() {
        shannon_types::provider_config::validate_profile_name(name)
            .map_err(|e| format!("set_provider_fallback_models: {e}"))?;
    }
    let chain = sanitize_fallback_entries(fallback_models);
    if chain.len() > shannon_engine::api::retry::MAX_FAILOVER_TARGETS {
        return Err(format!(
            "set_provider_fallback_models: the engine failover chain is capped at {} targets \
             (got {})",
            shannon_engine::api::retry::MAX_FAILOVER_TARGETS,
            chain.len()
        ));
    }

    // Read the active profile key before the critical section so the outcome
    // can name the profile that was written even when `profile` was None.
    let target_profile = match profile.as_deref() {
        Some(name) => name.to_string(),
        None => state
            .provider_store
            .lock()
            .await
            .config()
            .active_profile_key()
            .to_string(),
    };
    let target = target_profile.clone();
    let echo = chain.clone();

    with_model_profile_store(&state, |store| {
        store
            .set_provider_fallback_models(&provider_id, chain, profile.as_deref())
            .map_err(|e| format!("could not persist fallback chain for '{provider_id}': {e}"))
    })
    .await?;

    // Failover chains are resolved into the client config at build time for
    // the ACTIVE target — rebuild so a chain applied to the active provider
    // takes effect on the next request. Never blocks on resolution failure
    // (the store is already committed — same contract as the profile switch).
    if let Err(e) = crate::commands_config::rebuild_client_config_from_store(&state).await {
        tracing::warn!("set_provider_fallback_models: client config rebuild failed: {e}");
    }

    let _ = app_handle.emit(
        event_names::CONFIG_UPDATED,
        events::ConfigUpdatedPayload {
            key: "provider_fallback_models".into(),
            value: provider_id.clone(),
        },
    );

    Ok(ProviderFallbackOutcome {
        provider_id,
        model_profile: target,
        fallback_models: echo,
    })
}

/// 固化 (persist) the user's curated model selection for one provider slot:
/// replaces that slot's `models: Vec<ModelSpec>` wholesale in
/// `providers.toml` v2 and rebuilds the client config so the engine's
/// declared-metadata registry (pricing/context/clamp) follows immediately.
///
/// `profile` names the target model profile; `None` writes the active one
/// (the same resolution `save_provider`'s upsert path uses, so the modal's
/// save-then-curate sequence lands in the same profile).
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn set_provider_models(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    provider_id: String,
    models: Vec<DeclaredModelInput>,
    profile: Option<String>,
) -> Result<ProviderModelsOutcome, String> {
    use tauri::Emitter;

    let provider_id = provider_id.trim().to_string();
    if provider_id.is_empty() {
        return Err("set_provider_models: provider_id must not be empty".to_string());
    }
    // Validate the named profile up front (same shared contract
    // `/profiles new` enforces) so a bad name never reaches the store.
    if let Some(name) = profile.as_deref() {
        shannon_types::provider_config::validate_profile_name(name)
            .map_err(|e| format!("set_provider_models: {e}"))?;
    }
    let mut specs = Vec::with_capacity(models.len());
    for (i, input) in models.into_iter().enumerate() {
        specs.push(input.into_spec(i)?);
    }

    // Read the active profile key before the critical section so the outcome
    // can name the profile that was written even when `profile` was None.
    let target_profile = match profile.as_deref() {
        Some(name) => name.to_string(),
        None => state
            .provider_store
            .lock()
            .await
            .config()
            .active_profile_key()
            .to_string(),
    };
    let target = target_profile.clone();

    with_model_profile_store(&state, |store| {
        store
            .set_provider_models(&provider_id, specs, profile.as_deref())
            .map_err(|e| format!("could not persist model vault for '{provider_id}': {e}"))
    })
    .await?;

    // Re-register the declarations + re-clamp max_tokens for the active
    // target. Never blocks on resolution failure (same contract as the
    // profile-switch rebuild).
    if let Err(e) = crate::commands_config::rebuild_client_config_from_store(&state).await {
        tracing::warn!("set_provider_models: client config rebuild failed: {e}");
    }

    let _ = app_handle.emit(
        event_names::CONFIG_UPDATED,
        events::ConfigUpdatedPayload {
            key: "provider_models".into(),
            value: provider_id.clone(),
        },
    );

    // Echo the committed state straight from the (restored) in-memory store.
    let models = {
        let store = state.provider_store.lock().await;
        store
            .config()
            .profiles
            .get(&target)
            .and_then(|mp| mp.providers.iter().find(|p| p.id == provider_id))
            .map(|p| p.models.clone())
            .unwrap_or_default()
    };
    Ok(ProviderModelsOutcome {
        provider_id,
        model_profile: target,
        models,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn input(id: &str) -> DeclaredModelInput {
        DeclaredModelInput {
            id: id.to_string(),
            display_name: None,
            context_window: None,
            max_output: None,
            cost_per_m_input: None,
            cost_per_m_output: None,
            capabilities: None,
        }
    }

    #[test]
    fn id_only_input_projects_to_an_id_only_spec() {
        let spec = input("proxy-model-x").into_spec(0).unwrap();
        assert_eq!(spec.id, "proxy-model-x");
        assert_eq!(spec.context_window, None);
        assert_eq!(spec.max_output, None);
        assert!(spec.capabilities.is_empty());
        assert!(spec.validate().is_ok());
    }

    #[test]
    fn capability_names_parse_case_insensitively_and_reject_unknown() {
        assert_eq!(parse_capability("Vision").unwrap(), ModelCapability::Vision);
        assert_eq!(
            parse_capability("tool_use").unwrap(),
            ModelCapability::ToolUse
        );
        assert_eq!(parse_capability("tools").unwrap(), ModelCapability::ToolUse);
        let err = parse_capability("visionn").unwrap_err();
        assert!(err.contains("visionn"), "{err}");
        // The bad name surfaces through the spec projection with its index.
        let mut bad = input("m");
        bad.capabilities = Some(vec!["visionn".to_string()]);
        let err = bad.into_spec(2).unwrap_err();
        assert!(
            err.contains("models[2]") && err.contains("visionn"),
            "{err}"
        );
    }

    #[test]
    fn full_metadata_input_validates_schema_semantics() {
        let mut bad = input("m");
        bad.max_output = Some(0);
        let err = bad.into_spec(0).unwrap_err();
        assert!(err.contains("max_output"), "{err}");

        let mut good = input("m");
        good.context_window = Some(198_000);
        good.max_output = Some(32_768);
        good.cost_per_m_input = Some(0.5);
        good.cost_per_m_output = Some(2.0);
        good.capabilities = Some(vec!["vision".into(), "tool_use".into()]);
        let spec = good.into_spec(0).unwrap();
        assert_eq!(spec.context_window, Some(198_000));
        assert_eq!(
            spec.capabilities,
            vec![ModelCapability::Vision, ModelCapability::ToolUse]
        );
    }

    // ---- S2-2 (per-model metadata editor): partial-field update behavior ----
    //
    // The desktop editor edits ONE declaration but `set_provider_models`
    // replaces a provider's vault wholesale. Two contracts make that safe:
    //   1. a wire input carrying only some fields projects to a spec with
    //      the unset fields `None`/empty (no field-merge anywhere in the
    //      projection), and
    //   2. writing that partial spec REPLACES the stored declaration the
    //      same way — so clients MUST merge against the current vault
    //      client-side (the editor's `upsertVaultModel`) before submitting.
    // Pinned here so a "convenient" field-merge can never silently appear:
    // it would resurrect stale catalog metadata the user explicitly cleared
    // (blank field = clear the declaration).

    #[test]
    fn partial_metadata_input_projects_unset_fields_to_none() {
        let mut partial = input("proxy-model-x");
        partial.cost_per_m_input = Some(0.5);
        partial.cost_per_m_output = Some(2.0);
        partial.capabilities = Some(vec!["vision".into()]);
        let spec = partial.into_spec(0).unwrap();
        assert_eq!(spec.cost_per_m_input, Some(0.5));
        assert_eq!(spec.cost_per_m_output, Some(2.0));
        assert_eq!(spec.capabilities, vec![ModelCapability::Vision]);
        // Untouched fields are cleared, not inherited from anywhere.
        assert_eq!(spec.display_name, None);
        assert_eq!(spec.context_window, None);
        assert_eq!(spec.max_output, None);
    }

    #[tokio::test]
    async fn partial_spec_write_replaces_the_declaration_wholesale() {
        use crate::commands::AppState;
        use shannon_core::provider_config_store::ProviderConfigStore;
        use shannon_types::provider_config::{
            CredentialRef, ProviderKind, ProviderProfile, ProviderTiers,
        };
        use std::collections::HashMap;

        let state = AppState::new();
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("providers.toml");
        let slot = ProviderProfile {
            id: "glm".to_string(),
            kind: ProviderKind::OpenAiCompatible,
            display_name: "GLM".to_string(),
            base_url: "https://open.bigmodel.cn/api/paas/v4".to_string(),
            models_url: None,
            credential: CredentialRef::Env {
                var: "K".to_string(),
            },
            extra_headers: HashMap::new(),
            default_max_tokens: None,
            fallback_models: Vec::new(),
            quirks: Default::default(),
            tiers: ProviderTiers::default(),
            models: Vec::new(),
        };
        {
            let mut store = state.provider_store.lock().await;
            let mut cfg = ProviderConfigStore::load_or_default_at(&path);
            cfg.insert_model_profile("default").unwrap();
            cfg.upsert_profile(slot, "proxy-model-x");
            cfg.save().unwrap();
            *store = cfg;
        }

        // Seed a full declaration (every field set).
        {
            let mut store = state.provider_store.lock().await;
            store
                .set_provider_models(
                    "glm",
                    vec![ModelSpec {
                        id: "proxy-model-x".to_string(),
                        display_name: Some("Proxy".to_string()),
                        context_window: Some(198_000),
                        max_output: Some(32_768),
                        cost_per_m_input: Some(0.5),
                        cost_per_m_output: Some(2.0),
                        capabilities: vec![ModelCapability::Vision],
                    }],
                    None,
                )
                .unwrap();
        }

        // The editor's edit of ONE field (a pricing correction) submits the
        // merged spec; the write replaces the stored declaration wholesale.
        // A partial spec (only prices set) must NOT preserve the previous
        // context/capabilities behind the scenes.
        {
            let mut store = state.provider_store.lock().await;
            store
                .set_provider_models(
                    "glm",
                    vec![ModelSpec {
                        id: "proxy-model-x".to_string(),
                        display_name: None,
                        context_window: None,
                        max_output: None,
                        cost_per_m_input: Some(0.75),
                        cost_per_m_output: Some(3.0),
                        capabilities: Vec::new(),
                    }],
                    None,
                )
                .unwrap();
        }
        let spec = {
            let store = state.provider_store.lock().await;
            let glm = store.config().profiles["default"]
                .providers
                .iter()
                .find(|p| p.id == "glm")
                .unwrap();
            assert_eq!(glm.models.len(), 1);
            glm.models[0].clone()
        };
        assert_eq!(spec.cost_per_m_input, Some(0.75));
        assert_eq!(spec.cost_per_m_output, Some(3.0));
        // Overwrite semantics: nothing resurrects.
        assert_eq!(spec.display_name, None);
        assert_eq!(spec.context_window, None);
        assert_eq!(spec.max_output, None);
        assert!(spec.capabilities.is_empty());
    }

    #[tokio::test]
    async fn set_provider_models_persists_and_reloads_from_disk() {
        use crate::commands::AppState;
        use shannon_core::provider_config_store::ProviderConfigStore;
        use shannon_types::provider_config::{
            CredentialRef, ProviderKind, ProviderProfile, ProviderTiers,
        };
        use std::collections::HashMap;

        let state = AppState::new();
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("providers.toml");
        // Seed a store shaped like a configured desktop: "default" profile
        // with one provider slot (the shape `save_provider` lands).
        let slot = ProviderProfile {
            id: "glm".to_string(),
            kind: ProviderKind::OpenAiCompatible,
            display_name: "GLM".to_string(),
            base_url: "https://open.bigmodel.cn/api/paas/v4".to_string(),
            models_url: None,
            credential: CredentialRef::Env {
                var: "K".to_string(),
            },
            extra_headers: HashMap::new(),
            default_max_tokens: None,
            fallback_models: Vec::new(),
            quirks: Default::default(),
            tiers: ProviderTiers::default(),
            models: Vec::new(),
        };
        {
            let mut store = state.provider_store.lock().await;
            let mut cfg = ProviderConfigStore::load_or_default_at(&path);
            cfg.insert_model_profile("default").unwrap();
            cfg.upsert_profile(slot, "glm-5.3-flash");
            cfg.save().unwrap();
            *store = cfg;
        }

        // The exact mutator the command drives, under the same in-process
        // mutex the `with_model_profile_store` critical section holds (the
        // Tauri `State` wrapper isn't constructible in unit tests; the
        // mutex→flock→reload→persist→restore choreography around it is
        // `commands_profiles`' shared, already-pinned helper). Persistence
        // is pinned by the reload below.
        {
            let mut store = state.provider_store.lock().await;
            store
                .set_provider_models(
                    "glm",
                    vec![ModelSpec {
                        id: "glm-5.3-flash".to_string(),
                        display_name: None,
                        context_window: None,
                        max_output: Some(32_768),
                        cost_per_m_input: None,
                        cost_per_m_output: None,
                        capabilities: vec![ModelCapability::ToolUse],
                    }],
                    None,
                )
                .unwrap();
            store.save().unwrap();
        }

        // The write survives a reload from disk (flock + save_locked path).
        let reloaded = ProviderConfigStore::load_or_default_at(&path);
        let glm = reloaded.config().profiles["default"]
            .providers
            .iter()
            .find(|p| p.id == "glm")
            .unwrap();
        assert_eq!(glm.models.len(), 1);
        assert_eq!(glm.models[0].max_output, Some(32_768));
        assert_eq!(glm.models[0].capabilities, vec![ModelCapability::ToolUse]);
    }

    // ---- S3-4 (推荐降级链): recommendation rules ----
    //
    // Fixtures pin every slot's `tiers` so the tier walk resolves through
    // `resolve_tier`'s step 1 (explicit override) and never touches the
    // catalog — the rules under test stay hermetic.

    use shannon_types::provider_config::{
        ActiveTarget, CredentialRef, CredentialScope, ModelProfile, ProviderKind,
        ProviderModelConfig, ProviderProfile, ProviderTiers, Scope,
    };
    use std::collections::HashMap;

    fn chain_slot(
        id: &str,
        kind: ProviderKind,
        base_url: &str,
        tiers: ProviderTiers,
    ) -> ProviderProfile {
        ProviderProfile {
            id: id.to_string(),
            kind,
            display_name: id.to_string(),
            base_url: base_url.to_string(),
            models_url: None,
            credential: CredentialRef::Env {
                var: "K".to_string(),
            },
            extra_headers: HashMap::new(),
            default_max_tokens: None,
            fallback_models: Vec::new(),
            quirks: Default::default(),
            tiers,
            models: Vec::new(),
        }
    }

    fn tiers(pro: &str, standard: &str, fast: &str) -> ProviderTiers {
        let opt = |s: &str| (!s.is_empty()).then(|| s.to_string());
        ProviderTiers {
            pro: opt(pro),
            standard: opt(standard),
            fast: opt(fast),
        }
    }

    fn chain_cfg(
        slots: Vec<ProviderProfile>,
        active_provider: &str,
        active_model: &str,
    ) -> ProviderModelConfig {
        let mut profiles = HashMap::new();
        profiles.insert(
            "default".to_string(),
            ModelProfile {
                name: "default".to_string(),
                active_target: ActiveTarget {
                    provider_id: active_provider.to_string(),
                    model_id: active_model.to_string(),
                    scope: Scope::Global,
                },
                providers: slots,
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

    fn hop_entries(chain: &RecommendedFallbackChain) -> Vec<String> {
        chain.hops.iter().map(|h| h.entry.clone()).collect()
    }

    #[test]
    fn recommend_walks_same_family_tiers_descending_and_skips_the_current_model() {
        // Rich family: all three tiers pinned. The current model (the pro
        // pick) is excluded, the walk is descending, and the family cap
        // stops at two bare same-provider hops.
        let cfg = chain_cfg(
            vec![chain_slot(
                "anthropic",
                ProviderKind::Anthropic,
                "https://api.anthropic.com",
                tiers("cla-opus-x", "cla-sonnet-x", "cla-haiku-x"),
            )],
            "anthropic",
            "cla-opus-x",
        );
        let chain = recommend_chain_for(&cfg, "default", "anthropic").unwrap();
        assert_eq!(chain.current_model.as_deref(), Some("cla-opus-x"));
        assert_eq!(hop_entries(&chain), vec!["cla-sonnet-x", "cla-haiku-x"]);
        assert!(chain.hops.iter().all(|h| h.same_provider));
        let tier_sequence: Vec<String> = chain.hops.iter().map(|h| h.tier.clone()).collect();
        assert_eq!(tier_sequence, vec!["standard", "fast"]);

        // A concrete current model that is NOT a tier pick doesn't collide:
        // the walk starts at pro and caps at two.
        let cfg = chain_cfg(
            vec![chain_slot(
                "anthropic",
                ProviderKind::Anthropic,
                "https://api.anthropic.com",
                tiers("cla-opus-x", "cla-sonnet-x", "cla-haiku-x"),
            )],
            "anthropic",
            "cla-custom-main",
        );
        let chain = recommend_chain_for(&cfg, "default", "anthropic").unwrap();
        assert_eq!(hop_entries(&chain), vec!["cla-opus-x", "cla-sonnet-x"]);
        assert_eq!(chain.current_model.as_deref(), Some("cla-custom-main"));
    }

    #[test]
    fn recommend_fills_across_roster_providers_when_the_family_is_thin() {
        // Ollama has no catalog tiers and the slot pins nothing → phase 1
        // yields nothing, so the roster's other slots fill in with
        // qualified `provider/model` entries, one hop per provider, same
        // tier-descending walk.
        let cfg = chain_cfg(
            vec![
                chain_slot(
                    "ollama",
                    ProviderKind::Ollama,
                    "http://127.0.0.1:11434",
                    tiers("", "", ""),
                ),
                chain_slot(
                    "deepseek",
                    ProviderKind::Deepseek,
                    "https://api.deepseek.com",
                    tiers("ds-reasoner", "ds-chat", "ds-lite"),
                ),
                chain_slot(
                    "openai",
                    ProviderKind::OpenAi,
                    "https://api.openai.com/v1",
                    tiers("gpt-flag", "gpt-mid", "gpt-mini"),
                ),
            ],
            "ollama",
            "default", // placeholder — no current-model exclusion applies
        );
        let chain = recommend_chain_for(&cfg, "default", "ollama").unwrap();
        assert_eq!(chain.current_model, None);
        assert_eq!(
            hop_entries(&chain),
            vec!["deepseek/ds-reasoner", "openai/gpt-flag"],
            "pro tier first, one hop per provider, qualified switch syntax"
        );
        assert!(chain.hops.iter().all(|h| !h.same_provider));
        assert_eq!(chain.hops[0].tier, "pro");
        assert_eq!(chain.hops[1].tier, "pro");
    }

    #[test]
    fn recommend_chain_caps_at_the_engine_failover_limit() {
        use shannon_engine::api::retry::MAX_FAILOVER_TARGETS;
        let mut slots = vec![chain_slot(
            "ollama",
            ProviderKind::Ollama,
            "http://127.0.0.1:11434",
            tiers("", "", ""),
        )];
        for i in 0..5 {
            slots.push(chain_slot(
                &format!("prov{i}"),
                ProviderKind::OpenAiCompatible,
                &format!("https://relay{i}.example.com/v1"),
                tiers("flagship", "", ""),
            ));
        }
        let cfg = chain_cfg(slots, "ollama", "llama3");
        let chain = recommend_chain_for(&cfg, "default", "ollama").unwrap();
        assert_eq!(chain.hops.len(), MAX_FAILOVER_TARGETS);
    }

    #[test]
    fn recommend_excludes_cross_provider_candidates_carrying_the_current_model() {
        // Two slots canonicalizing to the SAME LlmProvider ("ollama" and
        // "local" both → Ollama; no catalog entries, so the tier walk is
        // pin-driven and hermetic). The other slot's candidates that carry
        // the current model would be skipped by resolve_failover_chain
        // (active-target echo) — the recommendation must not offer them.
        let cfg = chain_cfg(
            vec![
                chain_slot(
                    "ollama",
                    ProviderKind::Ollama,
                    "http://127.0.0.1:11434",
                    tiers("", "", ""),
                ),
                chain_slot(
                    "local",
                    ProviderKind::Ollama,
                    "http://127.0.0.1:11434",
                    tiers("llama3", "llama3", "llama-mini"),
                ),
            ],
            "ollama",
            "llama3",
        );
        let chain = recommend_chain_for(&cfg, "default", "ollama").unwrap();
        assert_eq!(
            hop_entries(&chain),
            vec!["local/llama-mini"],
            "current model never appears, in bare or qualified form"
        );
        assert!(!hop_entries(&chain).iter().any(|e| e == "local/llama3"));
    }

    #[test]
    fn generated_chain_resolves_through_resolve_failover_chain_without_skips() {
        // The acceptance pin from the review: a generated chain, written
        // verbatim into `fallback_models`, must resolve into the same number
        // of engine targets — no entry skipped (unconnected provider /
        // active-target echo / empty model).
        let mut cfg = chain_cfg(
            vec![
                chain_slot(
                    "anthropic",
                    ProviderKind::Anthropic,
                    "https://api.anthropic.com",
                    tiers("cla-opus-x", "", ""),
                ),
                chain_slot(
                    "deepseek",
                    ProviderKind::Deepseek,
                    "https://api.deepseek.com",
                    tiers("ds-reasoner", "ds-chat", "ds-lite"),
                ),
            ],
            "anthropic",
            "cla-custom-main",
        );
        let chain = recommend_chain_for(&cfg, "default", "anthropic").unwrap();
        assert!(!chain.hops.is_empty());
        let entries = hop_entries(&chain);
        cfg.profiles
            .get_mut("default")
            .unwrap()
            .providers
            .iter_mut()
            .find(|p| p.id == "anthropic")
            .unwrap()
            .fallback_models = entries.clone();

        let targets = shannon_core::unified_config::resolve_failover_chain(
            &cfg,
            "anthropic",
            "cla-custom-main",
        );
        assert_eq!(
            targets.len(),
            entries.len(),
            "every recommended hop must resolve to an engine target: {entries:?}"
        );
        for (hop, target) in chain.hops.iter().zip(&targets) {
            assert_eq!(target.model, hop.model);
            let expected_provider = if hop.same_provider {
                slot_llm_provider(
                    cfg.profiles["default"]
                        .providers
                        .iter()
                        .find(|p| p.id == "anthropic")
                        .unwrap(),
                )
            } else {
                slot_llm_provider(
                    cfg.profiles["default"]
                        .providers
                        .iter()
                        .find(|p| p.id == hop.provider_id)
                        .unwrap(),
                )
            };
            assert_eq!(target.provider, expected_provider, "hop {}", hop.entry);
        }
    }

    #[test]
    fn sanitize_fallback_entries_trims_dedupes_and_drops_empty() {
        let sanitized = sanitize_fallback_entries(vec![
            "  a ".to_string(),
            String::new(),
            "   ".to_string(),
            "a".to_string(),
            "b".to_string(),
        ]);
        assert_eq!(sanitized, vec!["a", "b"]);
        assert!(sanitize_fallback_entries(Vec::new()).is_empty());
    }
}
