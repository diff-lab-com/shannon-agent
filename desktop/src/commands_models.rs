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
}
