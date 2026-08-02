//! Chat-related Tauri commands (extracted from `commands.rs`).
//!
//! First step of the commands.rs decomposition (R2-A3 / P1.1). The chat
//! domain is the smallest cohesive cluster that touches AppState directly
//! without dragging in session/config/mcp plumbing — good template for the
//! later, larger extractions.
//!
//! Functions stay registered under their original `commands::*` path via
//! `pub use crate::commands_chat::*;` in `commands.rs`, so the invoke_handler
//! list in `main.rs` does not change.

use crate::commands::{AppState, ChatMessage, ModelInfo, StatusResponse, ToolInfo};

/// Get all conversation messages.
///
/// P0-4: reads from the active session in `state.registry` instead of the
/// (removed) `state.messages` field.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn get_conversation(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ChatMessage>, String> {
    let session = state.registry.get_or_create_active();
    let messages = session.messages.lock().await;
    Ok(messages.clone())
}

/// List available models for the current provider.
///
/// Routed through `shannon_core::model_registry::merged_models_for_provider`
/// so the desktop shell finally shares one source of truth with the CLI
/// (ADR-0005 Phase 2 / task 4) — the previous hard-coded `match` returned a
/// stale, three-model snapshot that diverged from `MODEL_CATALOG` and the
/// dynamic models.dev overlay. Unknown context windows render as `0` (the
/// UI surfaces "unknown" rather than fabricating a value, P0-2 honest-cost).
///
/// Honors the desktop's `enabled_providers` allowlist
/// (`shannon_core::model_registry::effective_provider_allowlist`):
/// - `None` (no desktop override) → fall back to env-var allowlist
/// - `Some(slice)` → only return models whose provider slug is in the
///   slice. The engine env vars are ignored when the desktop has an
///   explicit override (P4.9 precedence).
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn list_models(state: tauri::State<'_, AppState>) -> Result<Vec<ModelInfo>, String> {
    use shannon_core::model_registry::effective_provider_allowlist;

    let provider_str = state.client_config.read().await.provider.to_string();
    let allowlist = {
        let dc = state.desktop_config.read().await;
        dc.enabled_providers.clone()
    };
    list_models_for(
        &provider_str,
        effective_provider_allowlist(allowlist.as_deref()),
    )
}

/// Pure helper backing [`list_models`] and (in tests) `get_provider_allowlist`.
/// Given the active provider slug and the resolved allowlist, build the
/// wire [`ModelInfo`] vec.
///
/// `allowlist = Some(vec![])` means "user toggled every provider off" —
/// we return an empty list. `allowlist = Some(non_empty)` filters by
/// slug case-insensitively. `allowlist = None` means no restriction
/// (engine env-var allowlist already applied by
/// [`effective_provider_allowlist`], so this case never reaches a
/// restrictive filter).
fn list_models_for(
    provider_str: &str,
    allowlist: Option<Vec<String>>,
) -> Result<Vec<ModelInfo>, String> {
    use shannon_core::model_registry::merged_models_for_provider;
    use shannon_core::query_engine::pricing_for_model_opt;

    let provider = llm_provider_from_slug(provider_str)
        .ok_or_else(|| format!("unknown provider slug `{provider_str}`; cannot list models"))?;

    // Allowlist short-circuit: `Some(vec![])` ⇒ user toggled every
    // provider off in the desktop UI. Return empty so the picker shows
    // the "no models" state rather than a stale default.
    if let Some(slice) = allowlist.as_deref() {
        if slice.is_empty() {
            return Ok(Vec::new());
        }
        let active_slug = provider.to_string().to_lowercase();
        let allowed = slice.iter().any(|s| s.eq_ignore_ascii_case(&active_slug));
        if !allowed {
            return Ok(Vec::new());
        }
    }

    let models = merged_models_for_provider(provider);

    Ok(models
        .into_iter()
        .map(|m| {
            let pricing = pricing_for_model_opt(m.id);
            ModelInfo {
                id: m.id.to_string(),
                name: m.display_name.to_string(),
                provider: provider_str.to_string(),
                context_window: m.context_window,
                price_in: pricing.as_ref().map(|p| p.input_price_per_mtok),
                price_out: pricing.as_ref().map(|p| p.output_price_per_mtok),
                tier: None,
                dynamic: None,
            }
        })
        .collect())
}

/// Return the currently-effective provider allowlist for the desktop UI
/// (ADR-0005 P4.9). Reads the desktop's persisted `enabled_providers`
/// override and merges with the engine's `SHANNON_*_PROVIDERS` env vars
/// via [`shannon_core::model_registry::effective_provider_allowlist`].
///
/// Return shape:
/// - `Some(vec)` when an explicit or env-var allowlist is in effect.
///   `Some(vec![])` ⇒ user toggled every provider off.
/// - `None` ⇒ no restriction (full catalog visible).
///
/// The UI uses this to render the Settings → Provider visibility
/// checkboxes in their current state (a "Reset to default" button sends
/// `null` to clear the desktop override).
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn get_provider_allowlist(
    state: tauri::State<'_, AppState>,
) -> Result<Option<Vec<String>>, String> {
    use shannon_core::model_registry::effective_provider_allowlist;
    let dc = state.desktop_config.read().await;
    Ok(effective_provider_allowlist(
        dc.enabled_providers.as_deref(),
    ))
}

/// Map a desktop provider slug (e.g. `"anthropic"`, `"openai-compatible"`) to
/// the engine's `LlmProvider` so we can walk `model_registry`. The
/// `openai-compatible` catch-all (GLM / Zhipu / Moonshot / …) maps to
/// `LlmProvider::OpenAI` for catalog-walking purposes — the actual request
/// still goes through the user's custom `base_url`. Unknown slugs return
/// `None` and the caller surfaces a friendly error.
fn llm_provider_from_slug(s: &str) -> Option<shannon_engine::api::LlmProvider> {
    use shannon_engine::api::LlmProvider;
    match s {
        "anthropic" => Some(LlmProvider::Anthropic),
        "openai" => Some(LlmProvider::OpenAI),
        "ollama" => Some(LlmProvider::Ollama),
        "gemini" => Some(LlmProvider::Gemini),
        "deepseek" => Some(LlmProvider::DeepSeek),
        // openai-compatible: collapse to OpenAI for catalog walking — the
        // real provider is whatever the user's `base_url` points at.
        "openai-compatible" => Some(LlmProvider::OpenAI),
        _ => None,
    }
}

/// Get current application status.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn get_status(state: tauri::State<'_, AppState>) -> Result<StatusResponse, String> {
    let cc = state.client_config.read().await;
    let model = cc.model.clone();
    let provider = cc.provider.to_string();
    drop(cc);
    let session = state.registry.get_or_create_active();
    let querying = session.querying.lock().await;
    let messages = session.messages.lock().await;
    let working_dir = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".into());

    Ok(StatusResponse {
        model,
        provider,
        querying: *querying,
        message_count: messages.len(),
        working_dir,
    })
}

/// Cancel the current query.
#[tauri::command]
pub async fn cancel_query(
    state: tauri::State<'_, AppState>,
    _app_handle: tauri::AppHandle,
) -> Result<(), String> {
    // P0-4: cancel the active session's in-flight query.
    let session = state.registry.get_or_create_active();

    // Take the cancellation token and cancel it
    let token_opt = {
        let mut token_guard = session.cancellation_token.lock().await;
        token_guard.take()
    };

    if let Some(token) = token_opt {
        token.cancel();
    }

    // Clear querying flag
    {
        let mut querying = session.querying.lock().await;
        *querying = false;
    }

    Ok(())
}

/// List available tools.
#[tauri::command]
pub async fn list_tools(state: tauri::State<'_, AppState>) -> Result<Vec<ToolInfo>, String> {
    let tools = state.tools.list_tools_info();
    Ok(tools
        .into_iter()
        .map(|t| ToolInfo {
            name: t.name,
            description: t.description,
            enabled: true,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    // === Provider allowlist filter (ADR-0005 P4.9) ===

    #[test]
    fn list_models_for_returns_empty_when_allowlist_is_empty() {
        // `Some(vec![])` is the user-set "hide every provider" state.
        // The picker should show the "no models" state rather than
        // falling back to the full catalog.
        let out = list_models_for("anthropic", Some(vec![])).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn list_models_for_filters_when_active_slug_not_in_allowlist() {
        // The desktop's active provider is `openai`, but the
        // allowlist only includes `anthropic`. The picker must
        // surface no models for the (filtered-out) active provider.
        let out = list_models_for("openai", Some(vec!["anthropic".into()])).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn list_models_for_returns_catalog_when_active_slug_in_allowlist() {
        // Allowlist matches the active provider → return the full
        // catalog for that provider.
        let out =
            list_models_for("anthropic", Some(vec!["anthropic".into(), "openai".into()])).unwrap();
        assert!(!out.is_empty(), "anthropic has catalog entries");
        assert!(out.iter().all(|m| m.provider == "anthropic"));
    }

    #[test]
    fn list_models_for_returns_full_catalog_when_allowlist_is_none() {
        // No restriction (engine env-var allowlist already applied
        // upstream, so this case is "no restriction" from this
        // function's view).
        let out = list_models_for("anthropic", None).unwrap();
        assert!(!out.is_empty());
        assert!(out.iter().all(|m| m.provider == "anthropic"));
    }

    #[test]
    fn list_models_for_allowlist_match_is_case_insensitive() {
        // The catalog slugs are lowercase ("anthropic"); a user
        // typing "Anthropic" in the env var must still hit.
        let out = list_models_for("anthropic", Some(vec!["ANTHROPIC".into()])).unwrap();
        assert!(!out.is_empty());
    }
}
