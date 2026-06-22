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
pub async fn get_conversation(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ChatMessage>, String> {
    let messages = state.messages.lock().await;
    Ok(messages.clone())
}

/// List available models for the current provider.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn list_models(state: tauri::State<'_, AppState>) -> Result<Vec<ModelInfo>, String> {
    let provider = state.provider.lock().await;
    Ok(match provider.as_str() {
        "anthropic" => vec![
            ModelInfo {
                id: "claude-sonnet-4-6".into(),
                name: "Claude Sonnet 4.6".into(),
                provider: "anthropic".into(),
                context_window: 200_000,
            },
            ModelInfo {
                id: "claude-opus-4-7".into(),
                name: "Claude Opus 4.7".into(),
                provider: "anthropic".into(),
                context_window: 200_000,
            },
            ModelInfo {
                id: "claude-haiku-4-5-20251001".into(),
                name: "Claude Haiku 4.5".into(),
                provider: "anthropic".into(),
                context_window: 200_000,
            },
        ],
        "openai" => vec![
            ModelInfo {
                id: "gpt-4.1".into(),
                name: "GPT-4.1".into(),
                provider: "openai".into(),
                context_window: 1_047_576,
            },
            ModelInfo {
                id: "gpt-4.1-mini".into(),
                name: "GPT-4.1 Mini".into(),
                provider: "openai".into(),
                context_window: 1_047_576,
            },
            ModelInfo {
                id: "o3".into(),
                name: "o3".into(),
                provider: "openai".into(),
                context_window: 200_000,
            },
        ],
        "deepseek" => vec![
            ModelInfo {
                id: "deepseek-chat".into(),
                name: "DeepSeek Chat".into(),
                provider: "deepseek".into(),
                context_window: 128_000,
            },
            ModelInfo {
                id: "deepseek-reasoner".into(),
                name: "DeepSeek Reasoner".into(),
                provider: "deepseek".into(),
                context_window: 128_000,
            },
        ],
        "ollama" => vec![ModelInfo {
            id: "qwen3:8b".into(),
            name: "Qwen3 8B (local)".into(),
            provider: "ollama".into(),
            context_window: 32_000,
        }],
        _ => vec![ModelInfo {
            id: "default".into(),
            name: "Default Model".into(),
            provider: provider.clone(),
            context_window: 128_000,
        }],
    })
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
