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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_state_new() {
        let state = AppState::new();
        let messages = state.messages.blocking_lock();
        assert!(messages.is_empty());
        assert!(!*state.querying.blocking_lock());
        assert_eq!(*state.model.blocking_lock(), "claude-sonnet");
    }

    #[test]
    fn test_chat_message_serialization() {
        let msg = ChatMessage {
            role: "user".to_string(),
            content: "hello world".to_string(),
            timestamp: 1700000000,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: ChatMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.role, "user");
        assert_eq!(deserialized.content, "hello world");
        assert_eq!(deserialized.timestamp, 1700000000);
    }

    #[test]
    fn test_chat_message_roles() {
        for role in &["user", "assistant", "system"] {
            let msg = ChatMessage {
                role: role.to_string(),
                content: "test".to_string(),
                timestamp: 0,
            };
            assert_eq!(msg.role, *role);
        }
    }

    #[test]
    fn test_status_response_serialization() {
        let resp = StatusResponse {
            model: "claude-opus".to_string(),
            querying: true,
            message_count: 42,
            working_dir: "/home/user".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: StatusResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.model, "claude-opus");
        assert!(deserialized.querying);
        assert_eq!(deserialized.message_count, 42);
    }

    #[test]
    fn test_model_info_serialization() {
        let info = ModelInfo {
            id: "gpt-4".to_string(),
            name: "GPT-4".to_string(),
            provider: "openai".to_string(),
            context_window: 128_000,
        };
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: ModelInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "gpt-4");
        assert_eq!(deserialized.context_window, 128_000);
    }

    #[test]
    fn test_tool_info_serialization() {
        let info = ToolInfo {
            name: "bash".to_string(),
            description: "Execute shell commands".to_string(),
            enabled: true,
        };
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: ToolInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "bash");
        assert!(deserialized.enabled);
    }

    #[test]
    fn test_config_update_serialization() {
        let update = ConfigUpdate {
            key: "model".to_string(),
            value: "claude-opus".to_string(),
        };
        let json = serde_json::to_string(&update).unwrap();
        let deserialized: ConfigUpdate = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.key, "model");
        assert_eq!(deserialized.value, "claude-opus");
    }

    #[test]
    fn test_chrono_timestamp_reasonable() {
        let ts = chrono_timestamp();
        // Should be after 2024-01-01 and before 2030-01-01
        assert!(ts > 1704067200, "timestamp should be after 2024-01-01");
        assert!(ts < 1893456000, "timestamp should be before 2030-01-01");
    }

    #[tokio::test]
    async fn test_app_state_default_model() {
        let state = AppState::new();
        let model = state.model.lock().await;
        assert_eq!(*model, "claude-sonnet");
    }

    #[tokio::test]
    async fn test_app_state_querying_toggle() {
        let state = AppState::new();
        {
            let mut q = state.querying.lock().await;
            *q = true;
        }
        assert!(*state.querying.lock().await);
        {
            let mut q = state.querying.lock().await;
            *q = false;
        }
        assert!(!*state.querying.lock().await);
    }

    #[tokio::test]
    async fn test_app_state_messages_push() {
        let state = AppState::new();
        {
            let mut msgs = state.messages.lock().await;
            msgs.push(ChatMessage {
                role: "user".to_string(),
                content: "hello".to_string(),
                timestamp: 100,
            });
            msgs.push(ChatMessage {
                role: "assistant".to_string(),
                content: "hi".to_string(),
                timestamp: 101,
            });
        }
        let msgs = state.messages.lock().await;
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[1].content, "hi");
    }

    #[test]
    fn test_status_response_default_fields() {
        let resp = StatusResponse {
            model: String::new(),
            querying: false,
            message_count: 0,
            working_dir: String::new(),
        };
        assert!(!resp.querying);
        assert_eq!(resp.message_count, 0);
    }

    #[test]
    fn test_all_structs_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<AppState>();
        assert_send_sync::<ChatMessage>();
        assert_send_sync::<StatusResponse>();
        assert_send_sync::<ModelInfo>();
        assert_send_sync::<ToolInfo>();
        assert_send_sync::<ConfigUpdate>();
    }

    #[test]
    fn test_tool_info_disabled() {
        let info = ToolInfo {
            name: "dangerous".to_string(),
            description: "A dangerous tool".to_string(),
            enabled: false,
        };
        assert!(!info.enabled);
    }
}
