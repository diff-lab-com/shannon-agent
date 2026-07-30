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
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn get_conversation(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ChatMessage>, String> {
    let messages = state.messages.lock().await;
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
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn list_models(state: tauri::State<'_, AppState>) -> Result<Vec<ModelInfo>, String> {
    use shannon_core::model_registry::merged_models_for_provider;
    use shannon_core::query_engine::pricing_for_model_opt;

    let provider_str = state.provider.lock().await.clone();
    let provider = llm_provider_from_slug(&provider_str)
        .ok_or_else(|| format!("unknown provider slug `{provider_str}`; cannot list models"))?;
    let models = merged_models_for_provider(provider);

    Ok(models
        .into_iter()
        .map(|m| {
            let pricing = pricing_for_model_opt(m.id);
            ModelInfo {
                id: m.id.to_string(),
                name: m.display_name.to_string(),
                provider: provider_str.clone(),
                context_window: m.context_window,
                price_in: pricing.as_ref().map(|p| p.input_price_per_mtok),
                price_out: pricing.as_ref().map(|p| p.output_price_per_mtok),
                tier: None,
                dynamic: None,
            }
        })
        .collect())
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
    let model = state.model.lock().await;
    let provider = state.provider.lock().await;
    let querying = state.querying.lock().await;
    let messages = state.messages.lock().await;
    let working_dir = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".into());

    Ok(StatusResponse {
        model: model.clone(),
        provider: provider.clone(),
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
    // Take the cancellation token and cancel it
    let token_opt = {
        let mut token_guard = state.cancellation_token.lock().await;
        token_guard.take()
    };

    if let Some(token) = token_opt {
        token.cancel();
    }

    // Clear querying flag
    {
        let mut querying = state.querying.lock().await;
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
