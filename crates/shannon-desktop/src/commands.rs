//! Tauri IPC commands bridging the web UI to Shannon Core.
//!
//! Each command is exposed via `#[tauri::command]` and invoked from
//! JavaScript as `invoke("command_name", { args })`.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared application state accessible to all Tauri commands.
pub struct AppState {
    /// Current conversation messages for display.
    messages: Arc<Mutex<Vec<ChatMessage>>>,
    /// Whether a query is currently in progress.
    querying: Arc<Mutex<bool>>,
    /// Current model identifier.
    model: Arc<Mutex<String>>,
}

/// A chat message displayed in the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    pub timestamp: i64,
}

/// Status response for the desktop UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusResponse {
    pub model: String,
    pub querying: bool,
    pub message_count: usize,
    pub working_dir: String,
}

/// Model info for the model selector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub context_window: usize,
}

/// Tool info for the tools panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub enabled: bool,
}

/// Configuration update payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigUpdate {
    pub key: String,
    pub value: String,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            messages: Arc::new(Mutex::new(Vec::new())),
            querying: Arc::new(Mutex::new(false)),
            model: Arc::new(Mutex::new("claude-sonnet".into())),
        }
    }
}

/// Send a user message and get an AI response.
#[tauri::command]
pub async fn send_message(
    state: tauri::State<'_, AppState>,
    message: String,
) -> Result<String, String> {
    // Mark as querying
    {
        let mut querying = state.querying.lock().await;
        *querying = true;
    }

    // Add user message
    let now = chrono_timestamp();
    {
        let mut messages = state.messages.lock().await;
        messages.push(ChatMessage {
            role: "user".into(),
            content: message.clone(),
            timestamp: now,
        });
    }

    // TODO: Connect to QueryEngine for actual LLM interaction.
    // For now, return a placeholder acknowledging the scaffold.
    let response = format!(
        "[Shannon Desktop] Received: \"{}\"\n\n\
         The desktop app is scaffolded and ready for integration.\n\
         Connect to shannon-core's QueryEngine to enable full AI responses.",
        if message.len() > 100 { &message[..100] } else { &message }
    );

    // Add assistant message
    {
        let mut messages = state.messages.lock().await;
        messages.push(ChatMessage {
            role: "assistant".into(),
            content: response.clone(),
            timestamp: chrono_timestamp(),
        });
    }

    // Mark as done
    {
        let mut querying = state.querying.lock().await;
        *querying = false;
    }

    Ok(response)
}

/// Get all conversation messages.
#[tauri::command]
pub async fn get_conversation(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ChatMessage>, String> {
    let messages = state.messages.lock().await;
    Ok(messages.clone())
}

/// List available models.
#[tauri::command]
pub async fn list_models() -> Result<Vec<ModelInfo>, String> {
    // TODO: Query model_registry from shannon-core
    Ok(vec![
        ModelInfo {
            id: "claude-sonnet".into(),
            name: "Claude Sonnet".into(),
            provider: "anthropic".into(),
            context_window: 200_000,
        },
        ModelInfo {
            id: "claude-opus".into(),
            name: "Claude Opus".into(),
            provider: "anthropic".into(),
            context_window: 200_000,
        },
        ModelInfo {
            id: "gpt-4".into(),
            name: "GPT-4".into(),
            provider: "openai".into(),
            context_window: 128_000,
        },
    ])
}

/// Get current application status.
#[tauri::command]
pub async fn get_status(
    state: tauri::State<'_, AppState>,
) -> Result<StatusResponse, String> {
    let model = state.model.lock().await;
    let querying = state.querying.lock().await;
    let messages = state.messages.lock().await;
    let working_dir = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".into());

    Ok(StatusResponse {
        model: model.clone(),
        querying: *querying,
        message_count: messages.len(),
        working_dir,
    })
}

/// Cancel the current query.
#[tauri::command]
pub async fn cancel_query(
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let mut querying = state.querying.lock().await;
    *querying = false;
    Ok(())
}

/// List available tools.
#[tauri::command]
pub async fn list_tools() -> Result<Vec<ToolInfo>, String> {
    // TODO: Query tool registry from shannon-core
    Ok(vec![
        ToolInfo { name: "bash".into(), description: "Execute shell commands".into(), enabled: true },
        ToolInfo { name: "read".into(), description: "Read file contents".into(), enabled: true },
        ToolInfo { name: "write".into(), description: "Write file contents".into(), enabled: true },
        ToolInfo { name: "edit".into(), description: "Edit files with diff".into(), enabled: true },
        ToolInfo { name: "grep".into(), description: "Search file contents".into(), enabled: true },
        ToolInfo { name: "glob".into(), description: "Find files by pattern".into(), enabled: true },
    ])
}

/// Update configuration.
#[tauri::command]
pub async fn configure(
    state: tauri::State<'_, AppState>,
    update: ConfigUpdate,
) -> Result<(), String> {
    match update.key.as_str() {
        "model" => {
            let mut model = state.model.lock().await;
            *model = update.value;
            Ok(())
        }
        _ => Err(format!("Unknown config key: {}", update.key)),
    }
}

fn chrono_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
