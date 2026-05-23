//! Tests for shannon-desktop command logic.
//!
//! The commands module is gated behind `#[cfg(feature = "tauri")]` because the
//! handler signatures depend on `tauri::State`.  We replicate the pure-data
//! types and AppState here so we can test the logic without pulling in the
//! Tauri runtime.

use std::sync::Arc;
use tokio::sync::Mutex;

// ── Replicated types (mirror of commands.rs) ──────────────────────

#[derive(Debug, Clone)]
struct ChatMessage {
    role: String,
    content: String,
    timestamp: i64,
}

#[derive(Debug, Clone)]
struct StatusResponse {
    model: String,
    querying: bool,
    message_count: usize,
    working_dir: String,
}

#[derive(Debug, Clone)]
struct ModelInfo {
    id: String,
    name: String,
    provider: String,
    context_window: usize,
}

#[derive(Debug, Clone)]
struct ToolInfo {
    name: String,
    description: String,
    enabled: bool,
}

#[derive(Debug, Clone)]
struct ConfigUpdate {
    key: String,
    value: String,
}

struct AppState {
    messages: Arc<Mutex<Vec<ChatMessage>>>,
    querying: Arc<Mutex<bool>>,
    model: Arc<Mutex<String>>,
}

impl AppState {
    fn new() -> Self {
        Self {
            messages: Arc::new(Mutex::new(Vec::new())),
            querying: Arc::new(Mutex::new(false)),
            model: Arc::new(Mutex::new("claude-sonnet".into())),
        }
    }
}

fn chrono_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

// ── Replicated logic (mirror of command bodies) ───────────────────

async fn send_message(state: &AppState, message: String) -> Result<String, String> {
    {
        let mut querying = state.querying.lock().await;
        *querying = true;
    }

    let now = chrono_timestamp();
    {
        let mut messages = state.messages.lock().await;
        messages.push(ChatMessage {
            role: "user".into(),
            content: message.clone(),
            timestamp: now,
        });
    }

    let response = format!(
        "[Shannon Desktop] Received: \"{}\"\n\n\
         The desktop app is scaffolded and ready for integration.\n\
         Connect to shannon-core's QueryEngine to enable full AI responses.",
        if message.len() > 100 {
            &message[..100]
        } else {
            &message
        }
    );

    {
        let mut messages = state.messages.lock().await;
        messages.push(ChatMessage {
            role: "assistant".into(),
            content: response.clone(),
            timestamp: chrono_timestamp(),
        });
    }

    {
        let mut querying = state.querying.lock().await;
        *querying = false;
    }

    Ok(response)
}

async fn get_conversation(state: &AppState) -> Vec<ChatMessage> {
    state.messages.lock().await.clone()
}

fn list_models() -> Vec<ModelInfo> {
    vec![
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
    ]
}

async fn get_status(state: &AppState) -> StatusResponse {
    let model = state.model.lock().await;
    let querying = state.querying.lock().await;
    let messages = state.messages.lock().await;
    let working_dir = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".into());

    StatusResponse {
        model: model.clone(),
        querying: *querying,
        message_count: messages.len(),
        working_dir,
    }
}

async fn cancel_query(state: &AppState) {
    let mut querying = state.querying.lock().await;
    *querying = false;
}

fn list_tools() -> Vec<ToolInfo> {
    vec![
        ToolInfo {
            name: "bash".into(),
            description: "Execute shell commands".into(),
            enabled: true,
        },
        ToolInfo {
            name: "read".into(),
            description: "Read file contents".into(),
            enabled: true,
        },
        ToolInfo {
            name: "write".into(),
            description: "Write file contents".into(),
            enabled: true,
        },
        ToolInfo {
            name: "edit".into(),
            description: "Edit files with diff".into(),
            enabled: true,
        },
        ToolInfo {
            name: "grep".into(),
            description: "Search file contents".into(),
            enabled: true,
        },
        ToolInfo {
            name: "glob".into(),
            description: "Find files by pattern".into(),
            enabled: true,
        },
    ]
}

async fn configure(state: &AppState, update: ConfigUpdate) -> Result<(), String> {
    match update.key.as_str() {
        "model" => {
            let mut model = state.model.lock().await;
            *model = update.value;
            Ok(())
        }
        _ => Err(format!("Unknown config key: {}", update.key)),
    }
}

// ═══════════════════════════════════════════════════════════════════
//  Tests
// ═══════════════════════════════════════════════════════════════════

// ── AppState::new ─────────────────────────────────────────────────

#[tokio::test]
async fn app_state_new_has_querying_false() {
    let state = AppState::new();
    let querying = *state.querying.lock().await;
    assert!(!querying, "new AppState should have querying = false");
}

#[tokio::test]
async fn app_state_new_has_empty_messages() {
    let state = AppState::new();
    let messages = state.messages.lock().await;
    assert!(messages.is_empty(), "new AppState should have no messages");
}

#[tokio::test]
async fn app_state_new_default_model_is_claude_sonnet() {
    let state = AppState::new();
    let model = state.model.lock().await;
    assert_eq!(*model, "claude-sonnet");
}

// ── send_message ──────────────────────────────────────────────────

#[tokio::test]
async fn send_message_pushes_user_and_assistant_messages() {
    let state = AppState::new();
    let result = send_message(&state, "hello world".into()).await;
    assert!(result.is_ok());

    let messages = state.messages.lock().await;
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].role, "user");
    assert_eq!(messages[0].content, "hello world");
    assert_eq!(messages[1].role, "assistant");
    assert!(messages[1].content.contains("hello world"));
}

#[tokio::test]
async fn send_message_toggles_querying_flag() {
    let state = AppState::new();
    assert!(!*state.querying.lock().await);

    let _ = send_message(&state, "test".into()).await;
    assert!(
        !*state.querying.lock().await,
        "querying should be false after send_message completes"
    );
}

#[tokio::test]
async fn send_message_response_contains_echo() {
    let state = AppState::new();
    let msg = "check this out";
    let response = send_message(&state, msg.into()).await.unwrap();
    assert!(
        response.contains(msg),
        "response should echo the user message"
    );
}

#[tokio::test]
async fn send_message_truncates_long_input_in_echo() {
    let state = AppState::new();
    let long_msg = "x".repeat(200);
    let response = send_message(&state, long_msg.clone()).await.unwrap();
    assert!(
        response.contains(&"x".repeat(100)),
        "response should contain first 100 chars"
    );
    assert!(
        !response.contains(&long_msg),
        "response should not contain the full 200-char message"
    );
}

#[tokio::test]
async fn send_message_timestamps_are_positive() {
    let state = AppState::new();
    let _ = send_message(&state, "timing".into()).await;
    let messages = state.messages.lock().await;
    assert!(
        messages[0].timestamp > 0,
        "user message timestamp should be positive"
    );
    assert!(
        messages[1].timestamp > 0,
        "assistant message timestamp should be positive"
    );
}

// ── get_conversation ──────────────────────────────────────────────

#[tokio::test]
async fn get_conversation_returns_all_messages_in_order() {
    let state = AppState::new();
    let _ = send_message(&state, "first".into()).await;
    let _ = send_message(&state, "second".into()).await;

    let conv = get_conversation(&state).await;
    assert_eq!(conv.len(), 4, "two send_message calls = 4 messages");
    assert_eq!(conv[0].content, "first");
    assert_eq!(conv[0].role, "user");
    assert_eq!(conv[1].role, "assistant");
    assert_eq!(conv[2].content, "second");
    assert_eq!(conv[2].role, "user");
    assert_eq!(conv[3].role, "assistant");
}

#[tokio::test]
async fn get_conversation_empty_when_no_messages() {
    let state = AppState::new();
    let conv = get_conversation(&state).await;
    assert!(conv.is_empty());
}

// ── list_models ───────────────────────────────────────────────────

#[tokio::test]
async fn list_models_returns_expected_count() {
    let models = list_models();
    assert_eq!(models.len(), 3);
}

#[tokio::test]
async fn list_models_structure() {
    let models = list_models();
    let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
    assert!(ids.contains(&"claude-sonnet"));
    assert!(ids.contains(&"claude-opus"));
    assert!(ids.contains(&"gpt-4"));

    for m in &models {
        assert!(!m.name.is_empty(), "model name should not be empty");
        assert!(!m.provider.is_empty(), "provider should not be empty");
        assert!(m.context_window > 0, "context_window should be positive");
    }
}

#[tokio::test]
async fn list_models_providers() {
    let models = list_models();
    let anthropic: Vec<_> = models
        .iter()
        .filter(|m| m.provider == "anthropic")
        .collect();
    let openai: Vec<_> = models.iter().filter(|m| m.provider == "openai").collect();
    assert_eq!(anthropic.len(), 2, "should have 2 anthropic models");
    assert_eq!(openai.len(), 1, "should have 1 openai model");
}

// ── get_status ────────────────────────────────────────────────────

#[tokio::test]
async fn get_status_initial_state() {
    let state = AppState::new();
    let status = get_status(&state).await;

    assert_eq!(status.model, "claude-sonnet");
    assert!(!status.querying);
    assert_eq!(status.message_count, 0);
    assert!(!status.working_dir.is_empty());
}

#[tokio::test]
async fn get_status_after_messages() {
    let state = AppState::new();
    let _ = send_message(&state, "hi".into()).await;
    let status = get_status(&state).await;

    assert_eq!(status.message_count, 2);
    assert!(!status.querying);
}

#[tokio::test]
async fn get_status_reflects_model_change() {
    let state = AppState::new();
    configure(
        &state,
        ConfigUpdate {
            key: "model".into(),
            value: "gpt-4".into(),
        },
    )
    .await
    .unwrap();
    let status = get_status(&state).await;
    assert_eq!(status.model, "gpt-4");
}

// ── cancel_query ──────────────────────────────────────────────────

#[tokio::test]
async fn cancel_query_sets_querying_false() {
    let state = AppState::new();
    *state.querying.lock().await = true;
    assert!(*state.querying.lock().await);

    cancel_query(&state).await;
    assert!(!*state.querying.lock().await);
}

#[tokio::test]
async fn cancel_query_when_already_false() {
    let state = AppState::new();
    assert!(!*state.querying.lock().await);
    cancel_query(&state).await;
    assert!(!*state.querying.lock().await);
}

// ── configure ─────────────────────────────────────────────────────

#[tokio::test]
async fn configure_updates_model() {
    let state = AppState::new();
    let result = configure(
        &state,
        ConfigUpdate {
            key: "model".into(),
            value: "claude-opus".into(),
        },
    )
    .await;
    assert!(result.is_ok());

    let model = state.model.lock().await;
    assert_eq!(*model, "claude-opus");
}

#[tokio::test]
async fn configure_unknown_key_returns_error() {
    let state = AppState::new();
    let result = configure(
        &state,
        ConfigUpdate {
            key: "unknown_key".into(),
            value: "v".into(),
        },
    )
    .await;
    assert!(result.is_err());
    assert!(matches!(result, Err(ref e) if e.contains("Unknown config key")));
}

// ── list_tools ────────────────────────────────────────────────────

#[tokio::test]
async fn list_tools_returns_expected_count() {
    let tools = list_tools();
    assert_eq!(tools.len(), 6);
}

#[tokio::test]
async fn list_tools_all_enabled() {
    let tools = list_tools();
    for tool in &tools {
        assert!(
            tool.enabled,
            "tool '{}' should be enabled by default",
            tool.name
        );
    }
}

#[tokio::test]
async fn list_tools_has_expected_names() {
    let tools = list_tools();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"bash"));
    assert!(names.contains(&"read"));
    assert!(names.contains(&"write"));
    assert!(names.contains(&"edit"));
    assert!(names.contains(&"grep"));
    assert!(names.contains(&"glob"));
}

#[tokio::test]
async fn list_tools_have_nonempty_descriptions() {
    let tools = list_tools();
    for tool in &tools {
        assert!(
            !tool.description.is_empty(),
            "tool '{}' should have a description",
            tool.name
        );
    }
}
