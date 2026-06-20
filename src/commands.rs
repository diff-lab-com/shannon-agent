//! Tauri IPC commands bridging the web UI to Shannon Core.
//!
//! Each command is exposed via `#[tauri::command]` and invoked from
//! JavaScript as `invoke("command_name", { args })`.

use serde::{Deserialize, Serialize};
use shannon_core::api::client::LlmClient;
use shannon_core::api::types::LlmClientConfig;
use shannon_core::permissions::{ApprovalMode, PermissionManager};
use shannon_core::query_engine::{QueryContext, QueryEngine, QueryEvent};
use shannon_core::state::StateManager;
use shannon_core::tools::ToolRegistry;
use shannon_mcp::McpProcessPool;
use shannon_skills::SkillRegistry;
use shannon_tools::register_default_tools;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::{Mutex, RwLock, oneshot};

use crate::config::{self, DesktopConfig};
use crate::events::event_names;
use crate::events::{self, HunkAction};
use tokio_util::sync::CancellationToken;

/// Parse approval mode string into ApprovalMode enum
fn parse_approval_mode(mode_str: &str) -> ApprovalMode {
    match mode_str.to_lowercase().as_str() {
        "suggest" | "default" => ApprovalMode::Suggest,
        "plan" => ApprovalMode::Plan,
        "auto" => ApprovalMode::Auto,
        "auto_edit" | "autoedit" => ApprovalMode::AutoEdit,
        "full_auto" | "fullauto" => ApprovalMode::FullAuto,
        "readonly" | "read-only" => ApprovalMode::Readonly,
        "plan_ro" | "plan-ro" | "planreadonly" => ApprovalMode::PlanReadonly,
        "bypass_permissions" | "bypasspermissions" => ApprovalMode::BypassPermissions,
        "dont_ask" | "dontask" => ApprovalMode::DontAsk,
        "confirm" => ApprovalMode::Suggest, // "confirm" maps to Suggest (ask each time)
        _ => ApprovalMode::Suggest,         // Default to safe mode
    }
}

/// Resolve the plugins directory (`~/.shannon/plugins/`).
///
/// Falls back to `<config_dir>/shannon/plugins` if `$HOME` is unset. The
/// directory is *not* created here; callers should rely on PluginRegistry's
/// `ensure_dir` for that.
fn plugin_registry_dir() -> std::path::PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    base.join("shannon").join("plugins")
}

/// Shared application state accessible to all Tauri commands.
pub struct AppState {
    /// Current conversation messages for the active session.
    pub(crate) messages: Arc<Mutex<Vec<ChatMessage>>>,
    /// Whether a query is currently in progress.
    pub(crate) querying: Arc<Mutex<bool>>,
    /// Current model identifier.
    pub(crate) model: Arc<Mutex<String>>,
    /// Current provider name.
    pub(crate) provider: Arc<Mutex<String>>,
    /// LLM client config — used to build clients on demand.
    client_config: Arc<RwLock<LlmClientConfig>>,
    /// Tool registry with default tools.
    pub(crate) tools: Arc<ToolRegistry>,
    /// Permission manager.
    // KEEP: AppState owns the PermissionManager so the desktop shell can
    // eventually consult it before dispatching tool calls. Hooked into
    // send_message / request_permission once the permission-prompt UI lands.
    #[allow(dead_code)]
    permissions: Arc<RwLock<PermissionManager>>,
    /// Session state manager.
    pub(crate) state_manager: Arc<StateManager>,
    /// Query engine configuration.
    qe_config: Arc<RwLock<shannon_core::query_engine::QueryEngineConfig>>,
    /// Desktop config (persisted).
    pub(crate) desktop_config: Arc<RwLock<DesktopConfig>>,
    /// Pending permission requests (request_id -> sender).
    pending_permissions: Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>,
    /// Session metadata for session list.
    pub(crate) sessions: Arc<Mutex<Vec<SessionMeta>>>,
    /// Cancellation token for the current query.
    pub(crate) cancellation_token: Arc<Mutex<Option<CancellationToken>>>,
    /// Currently active session ID.
    pub(crate) current_session_id: Arc<Mutex<Option<String>>>,
    /// Background tasks.
    background_tasks: Arc<Mutex<Vec<BackgroundTaskMeta>>>,
    /// Skill registry for skill discovery and listing.
    pub(crate) skill_registry: Arc<SkillRegistry>,
    /// MCP process pool for real server connections.
    pub(crate) mcp_pool: Arc<McpProcessPool>,
    /// Scheduled task store (`~/.shannon/scheduled-tasks/`).
    pub(crate) scheduled_task_store: Arc<shannon_core::scheduled_task_store::ScheduledTaskStore>,
    /// Execution history store (`~/.shannon/scheduled-runs/`).
    pub(crate) scheduled_runs_store: Arc<shannon_core::scheduled_runs::ScheduledRunsStore>,
    /// Triage items needing user attention.
    pub(crate) triage_store: Arc<crate::scheduled_commands::TriageStore>,
    /// Triggered-routine enabled/disabled overrides.
    pub(crate) routine_overrides: Arc<crate::scheduled_commands::RoutineOverrideStore>,
    /// Triggered-routine registry (reloaded on demand).
    pub(crate) triggered_registry:
        Arc<tokio::sync::RwLock<shannon_core::triggered_routines::TriggeredRoutineRegistry>>,
    /// Plugin registry (`~/.shannon/plugins/`). Accepts both Shannon
    /// `plugin.toml` and Claude Code `.claude-plugin/plugin.json` formats,
    /// plus packaged `.dxt` / `.mcpb` archives.
    pub(crate) plugin_registry: Arc<tokio::sync::RwLock<shannon_core::plugin::PluginRegistry>>,
    /// Append-only inter-agent message history (`~/.shannon/agent-messages/`).
    pub(crate) agent_message_history: Arc<shannon_agents::message_history::MessageHistoryStore>,
    /// Native OS notification dispatcher (P3). Empty by default; populated
    /// with a `TauriNotificationHandler` once `AppHandle` is available in
    /// `main.rs` setup via `attach_notification_handler`.
    pub(crate) notifier: Arc<shannon_core::notifier::Notifier>,
    /// Inbound listener supervisor (Phase 2). Owns the Slack + Telegram
    /// worker tasks. None until the first config is saved.
    pub(crate) inbound_listener: Arc<tokio::sync::Mutex<Option<crate::inbound::InboundListener>>>,
}

/// Session metadata for session list.
#[derive(Debug, Clone)]
pub(crate) struct SessionMeta {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) created_at: i64,
    pub(crate) message_count: usize,
    pub(crate) working_dir: Option<String>,
    pub(crate) parent_id: Option<String>,
    pub(crate) branch_point: Option<usize>,
}

/// Background task metadata.
#[derive(Debug, Clone)]
struct BackgroundTaskMeta {
    id: String,
    prompt: String,
    status: String, // "running", "completed", "failed"
    started_at: i64,
    completed_at: Option<i64>,
    output: String,
}

/// Task info for the task board.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInfo {
    pub id: String,
    pub title: String,
    pub status: String,
    pub assignee: Option<String>,
    pub priority: Option<String>,
    pub description: Option<String>,
    /// IDs of tasks this task depends on (waits on). JSON: `blockedBy`.
    #[serde(default)]
    pub blocked_by: Vec<String>,
    /// IDs of tasks that wait on this task. JSON: `blocks`.
    #[serde(default)]
    pub blocks: Vec<String>,
    /// Optional due date as unix seconds. JSON: `dueDate`.
    #[serde(default)]
    pub due_date: Option<i64>,
    /// Active form label for in-progress status. JSON: `activeForm`.
    #[serde(default)]
    pub active_form: Option<String>,
    /// Execution semantics for this task's downstream chain. JSON: `executionMode`.
    /// `serial` (default) means each task in `blocks` waits for the previous to
    /// finish. `parallel` means all `blocks` run concurrently once this completes.
    #[serde(default)]
    pub execution_mode: Option<String>,
    /// Team / session subdir name the task file lives in. Empty when the task
    /// lives at the top level of `.claude/tasks/`.
    #[serde(default)]
    pub team: Option<String>,
}

/// Payload for `update_task`. All fields optional except `id`.
/// Writes through to `.claude/tasks/{team}/{id}.json` (creates the file if missing).
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateTaskPayload {
    pub id: String,
    pub status: Option<String>,
    pub assignee: Option<String>,
    pub priority: Option<String>,
    pub due_date: Option<i64>,
    /// When set, writes `executionMode` to the task JSON.
    pub execution_mode: Option<String>,
}

/// Agent info for the dashboard (derived from background tasks).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub model: String,
    pub status: String,
    pub task: Option<String>,
    pub progress: Option<u32>,
    pub tools_used: Option<u32>,
    pub duration: Option<i64>,
}

/// A chat message displayed in the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    pub timestamp: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_attachments: Option<Vec<FileAttachment>>,
}

/// File attachment for chat messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileAttachment {
    pub name: String,
    pub path: String,
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base64_data: Option<String>,
}

/// Detect media type from file extension.
fn detect_media_type(path: &str) -> Option<String> {
    use std::path::Path;
    let ext = Path::new(path).extension()?.to_str()?;
    match ext.to_lowercase().as_str() {
        "png" => Some("image/png".to_string()),
        "jpg" | "jpeg" => Some("image/jpeg".to_string()),
        "gif" => Some("image/gif".to_string()),
        "webp" => Some("image/webp".to_string()),
        "svg" => Some("image/svg+xml".to_string()),
        _ => None,
    }
}

/// Read file and convert to base64, returning (base64_string, media_type).
///
/// Security: `path` must already be validated by the caller — see
/// `validate_attachment_path`. This helper does no path checking on its own
/// because callers sometimes pass already-canonicalized paths.
fn file_to_base64(path: &str) -> Result<(String, String), String> {
    use base64::Engine;
    use std::fs;

    let bytes = fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;
    let media_type =
        detect_media_type(path).unwrap_or_else(|| "application/octet-stream".to_string());
    let base64_string = base64::engine::general_purpose::STANDARD.encode(&bytes);

    Ok((base64_string, media_type))
}

/// Status response for the desktop UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusResponse {
    pub model: String,
    pub provider: String,
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

/// Provider switch request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSwitchRequest {
    pub provider: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: String,
}

/// Response from send_message containing the query ID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageResponse {
    pub query_id: String,
}

impl AppState {
    /// Create a new AppState, initializing the LLM client from env/config.
    pub fn new() -> Self {
        let desktop_config = config::load_config();
        let client_config = Self::build_client_config(&desktop_config);

        let model = desktop_config
            .model
            .clone()
            .unwrap_or_else(|| "claude-sonnet-4-6".into());
        let provider = desktop_config
            .provider
            .clone()
            .unwrap_or_else(|| "anthropic".into());

        // Initialize tool registry with default tools
        let mut tool_registry = ToolRegistry::new();
        let _agent_context =
            register_default_tools(&mut tool_registry).expect("Failed to register default tools");

        Self {
            messages: Arc::new(Mutex::new(Vec::new())),
            querying: Arc::new(Mutex::new(false)),
            model: Arc::new(Mutex::new(model)),
            provider: Arc::new(Mutex::new(provider)),
            client_config: Arc::new(RwLock::new(client_config)),
            tools: Arc::new(tool_registry),
            permissions: Arc::new(RwLock::new(PermissionManager::new())),
            state_manager: Arc::new(StateManager::new()),
            qe_config: Arc::new(RwLock::new(
                shannon_core::query_engine::QueryEngineConfig::default(),
            )),
            desktop_config: Arc::new(RwLock::new(desktop_config)),
            pending_permissions: Arc::new(Mutex::new(HashMap::new())),
            sessions: Arc::new(Mutex::new(Vec::new())),
            cancellation_token: Arc::new(Mutex::new(None)),
            current_session_id: Arc::new(Mutex::new(None)),
            background_tasks: Arc::new(Mutex::new(Vec::new())),
            skill_registry: Arc::new(SkillRegistry::new()),
            mcp_pool: Arc::new(McpProcessPool::new()),
            scheduled_task_store: Arc::new(
                shannon_core::scheduled_task_store::ScheduledTaskStore::new(),
            ),
            scheduled_runs_store: Arc::new(shannon_core::scheduled_runs::ScheduledRunsStore::new()),
            triage_store: Arc::new(crate::scheduled_commands::TriageStore::new()),
            routine_overrides: Arc::new(crate::scheduled_commands::RoutineOverrideStore::new()),
            triggered_registry: Arc::new(tokio::sync::RwLock::new(
                shannon_core::triggered_routines::TriggeredRoutineRegistry::load_from_dirs(),
            )),
            plugin_registry: Arc::new(tokio::sync::RwLock::new(
                shannon_core::plugin::PluginRegistry::new(plugin_registry_dir()),
            )),
            agent_message_history: Arc::new(
                shannon_agents::message_history::MessageHistoryStore::new(),
            ),
            notifier: Arc::new(shannon_core::notifier::Notifier::new()),
            inbound_listener: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    /// Attach the Tauri notification handler to the dispatcher and enable
    /// cooldown + level filtering. Called once from `main.rs` setup() once
    /// the `AppHandle` is available. Idempotent — replacing any handler
    /// previously registered under the `"tauri"` name.
    ///
    /// Also attaches a `WebhookHandler` when `[notifications.webhook]` is
    /// configured in `.shannon.toml` (Slack / Discord / Feishu / WeChat Work
    /// / custom / raw templates).
    pub fn attach_notification_handler(&mut self, app: tauri::AppHandle) {
        use shannon_core::notifier::{Cooldown, NotificationLevel, Notifier};

        let mut notifier = Notifier::new()
            .with_cooldown(Cooldown::new())
            .with_minimum_level(NotificationLevel::Info);
        notifier.add_handler(Box::new(
            crate::notifications::TauriNotificationHandler::new(app),
        ));

        if let Some(wh_cfg) = load_desktop_webhook_config() {
            match shannon_core::notifier::WebhookHandler::new(wh_cfg) {
                Ok(handler) => {
                    tracing::info!("notifications: webhook handler attached");
                    notifier.add_handler(Box::new(handler));
                }
                Err(e) => {
                    tracing::warn!(error = %e, "notifications: webhook handler init failed");
                }
            }
        }

        self.notifier = Arc::new(notifier);
    }

    fn build_client_config(cfg: &DesktopConfig) -> LlmClientConfig {
        let provider_str = cfg.provider.as_deref().unwrap_or("anthropic");
        let provider = provider_from_str(provider_str);
        let api_key = cfg
            .api_key
            .clone()
            .filter(|k| !k.is_empty())
            .unwrap_or_else(|| provider.resolve_api_key_from_env());
        let base_url = cfg
            .base_url
            .clone()
            .unwrap_or_else(|| provider.default_base_url().to_string());
        let model = cfg
            .model
            .clone()
            .unwrap_or_else(|| "claude-sonnet-4-6".into());

        LlmClientConfig {
            api_key,
            base_url,
            model,
            provider,
            ..LlmClientConfig::default()
        }
    }
}

/// Send a user message and stream the AI response via Tauri events.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn send_message(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    message: String,
    file_paths: Option<Vec<String>>,
) -> Result<SendMessageResponse, String> {
    // Prevent concurrent queries — check and set in a single lock scope to avoid TOCTOU race
    {
        let mut querying = state.querying.lock().await;
        if *querying {
            return Err("A query is already in progress".into());
        }
        *querying = true;
    }

    // Create cancellation token
    let cancel_token = CancellationToken::new();
    {
        let mut token_guard = state.cancellation_token.lock().await;
        *token_guard = Some(cancel_token.clone());
    }

    // Add user message
    let now = chrono_timestamp();
    // Resolve working directory once for attachment-path validation below.
    let attachment_working_dir = resolve_working_dir(&state).await;
    let attachments = file_paths.and_then(|paths| {
        if paths.is_empty() {
            None
        } else {
            Some(
                paths
                    .into_iter()
                    .filter_map(|path| {
                        // Security: reject any attachment path that resolves
                        // outside the working directory. A compromised
                        // frontend must not be able to exfiltrate
                        // `~/.ssh/id_rsa`, `~/.shannon/desktop/config.json`,
                        // or any other sensitive file via the attachment
                        // pipeline.
                        let canonical =
                            crate::resolve_path_in_working_dir(&path, &attachment_working_dir)
                                .ok()?;
                        let canonical_str = canonical.to_string_lossy().into_owned();
                        std::path::Path::new(&canonical)
                            .file_name()
                            .and_then(|name| name.to_str())
                            .and_then(|name_str| {
                                std::fs::metadata(&canonical).ok().and_then(|meta| {
                                    // Try to read file and convert to base64 for images
                                    file_to_base64(&canonical_str).ok().map(
                                        |(base64_data, media_type)| FileAttachment {
                                            name: name_str.to_string(),
                                            path: canonical_str.clone(),
                                            size: meta.len(),
                                            media_type: Some(media_type),
                                            base64_data: Some(base64_data),
                                        },
                                    )
                                })
                            })
                    })
                    .collect::<Vec<_>>(),
            )
        }
    });

    {
        let mut messages = state.messages.lock().await;
        messages.push(ChatMessage {
            role: "user".into(),
            content: message.clone(),
            timestamp: now,
            file_attachments: attachments,
        });
    }

    let query_id = uuid::Uuid::new_v4();
    let qid_str = query_id.to_string();

    // Build the query engine
    let client_config = state.client_config.read().await.clone();
    let client = LlmClient::new(client_config);
    let tools = state.tools.clone();

    // Create PermissionManager from shared state with config-based approval mode
    let desktop_cfg = state.desktop_config.read().await;
    let approval_mode_str = desktop_cfg.approval_mode.as_deref().unwrap_or("confirm");
    let approval_mode = parse_approval_mode(approval_mode_str);

    // Create a new PermissionManager instance configured from shared state
    let mut permissions = PermissionManager::new();
    permissions.set_approval_mode(approval_mode);

    let _state_mgr = state.state_manager.clone();
    let _qe_config = state.qe_config.read().await.clone();

    let engine = QueryEngine::with_defaults_arc(client, tools, permissions, StateManager::new());

    // Create query context
    let model = state.model.lock().await.clone();
    let context = QueryContext {
        query_id,
        session_id: uuid::Uuid::new_v4(),
        user_message: message,
        metadata: shannon_core::query_engine::QueryMetadata {
            timestamp: chrono::Utc::now(),
            tools_allowed: true,
            max_tokens: None,
            model,
            temperature: None,
            top_p: None,
        },
    };

    // Spawn the query in a background task, streaming events to frontend
    let querying_flag = state.querying.clone();
    let messages_arc = state.messages.clone();
    let app = app_handle.clone();
    let cancel_token_clone = cancel_token.clone();
    let current_session_id_arc = state.current_session_id.clone();
    let state_mgr_arc = state.state_manager.clone();
    let model_arc = state.model.clone();
    let notifier_arc = state.notifier.clone();

    let return_qid = qid_str.clone();
    tokio::spawn(async move {
        let stream = engine.process_query(context, None).await;
        let mut final_content = String::new();

        // Consume the stream using futures::StreamExt
        use futures::StreamExt;
        let mut pin_stream = std::pin::pin!(stream);

        while let Some(event_result) = pin_stream.next().await {
            // Check for cancellation
            if cancel_token_clone.is_cancelled() {
                let _ = app.emit(
                    event_names::QUERY_CANCELLED,
                    events::QueryCancelledPayload {
                        query_id: qid_str.clone(),
                    },
                );
                break;
            }

            match event_result {
                Ok(event) => match event {
                    QueryEvent::Text { content, .. } => {
                        final_content.push_str(&content);
                        let _ = app.emit(
                            event_names::QUERY_TEXT,
                            events::QueryTextPayload {
                                query_id: qid_str.clone(),
                                content,
                            },
                        );
                    }
                    QueryEvent::ToolUseRequest {
                        tool_use_id,
                        tool_name,
                        tool_input,
                        ..
                    } => {
                        let _ = app.emit(
                            event_names::QUERY_TOOL_START,
                            events::ToolStartPayload {
                                query_id: qid_str.clone(),
                                tool_use_id,
                                tool_name,
                                tool_input,
                            },
                        );
                    }
                    QueryEvent::ToolUseResult {
                        tool_use_id,
                        tool_name,
                        result,
                        is_error,
                        ..
                    } => {
                        let _ = app.emit(
                            event_names::QUERY_TOOL_RESULT,
                            events::ToolResultPayload {
                                query_id: qid_str.clone(),
                                tool_use_id,
                                tool_name,
                                result,
                                is_error,
                            },
                        );
                    }
                    QueryEvent::ToolProgress {
                        tool_use_id,
                        tool_name,
                        progress,
                        message: msg,
                        ..
                    } => {
                        let _ = app.emit(
                            event_names::QUERY_TOOL_PROGRESS,
                            events::ToolProgressPayload {
                                query_id: qid_str.clone(),
                                tool_use_id,
                                tool_name,
                                progress,
                                message: msg,
                            },
                        );
                    }
                    QueryEvent::Thinking { content, .. } => {
                        let _ = app.emit(
                            event_names::QUERY_THINKING,
                            events::ThinkingPayload {
                                query_id: qid_str.clone(),
                                content,
                            },
                        );
                    }
                    QueryEvent::Usage {
                        input_tokens,
                        output_tokens,
                        cost_usd,
                        ..
                    } => {
                        let _ = app.emit(
                            event_names::QUERY_USAGE,
                            events::UsagePayload {
                                query_id: qid_str.clone(),
                                input_tokens,
                                output_tokens,
                                cost_usd,
                            },
                        );
                    }
                    QueryEvent::Completed { .. } => {
                        // Save final assistant message
                        {
                            let mut messages = messages_arc.lock().await;
                            messages.push(ChatMessage {
                                role: "assistant".into(),
                                content: if final_content.is_empty() {
                                    "(no text response)".into()
                                } else {
                                    final_content.clone()
                                },
                                timestamp: chrono_timestamp(),
                                file_attachments: None,
                            });
                        }

                        // Auto-persist to StateManager
                        {
                            let session_id_opt = current_session_id_arc.lock().await.clone();
                            if let Some(sid) = session_id_opt {
                                let msgs = messages_arc.lock().await.clone();
                                let model = model_arc.lock().await.clone();
                                if let Ok(session_uuid) = uuid::Uuid::parse_str(&sid) {
                                    let core_msgs: Vec<shannon_core::api::Message> = msgs
                                        .iter()
                                        .map(|m| shannon_core::api::Message {
                                            role: m.role.clone(),
                                            content: shannon_core::api::MessageContent::Text(
                                                m.content.clone(),
                                            ),
                                        })
                                        .collect();
                                    let meta = shannon_core::state::SessionPersistMetadata {
                                        model,
                                        turn_count: core_msgs.len() / 2,
                                        ..Default::default()
                                    };
                                    let _ = state_mgr_arc.save_session(
                                        &session_uuid,
                                        &core_msgs,
                                        &meta,
                                    );
                                }
                            }
                        }

                        let _ = app.emit(
                            event_names::QUERY_COMPLETED,
                            events::QueryCompletedPayload {
                                query_id: qid_str.clone(),
                            },
                        );
                        let _ = fire_query_notification(&notifier_arc, NotificationKind::Completed);
                    }
                    QueryEvent::Failed { error, .. } => {
                        let _ = app.emit(
                            event_names::QUERY_FAILED,
                            events::QueryFailedPayload {
                                query_id: qid_str.clone(),
                                error: error.clone(),
                            },
                        );
                        let _ =
                            fire_query_notification(&notifier_arc, NotificationKind::Failed(error));
                    }
                    // Ignore other events in MVP
                    _ => {}
                },
                Err(e) => {
                    let _ = app.emit(
                        event_names::QUERY_FAILED,
                        events::QueryFailedPayload {
                            query_id: qid_str.clone(),
                            error: e.to_string(),
                        },
                    );
                    let _ = fire_query_notification(
                        &notifier_arc,
                        NotificationKind::Failed(e.to_string()),
                    );
                }
            }
        }

        // Clear querying flag and cancellation token
        {
            let mut q = querying_flag.lock().await;
            *q = false;
        }
    });

    Ok(SendMessageResponse {
        query_id: return_qid,
    })
}

// Chat-related commands (get_conversation, list_models, get_status,
// cancel_query, list_tools) live in `commands_chat.rs`. They are registered
// in main.rs's invoke_handler as `commands_chat::*` — Tauri's #[command]
// macro generates module-local helpers (`__cmd__*`, `__tauri_command_name_*`)
// that must be referenced from the module they were defined in, so a
// re-export here would not work.

/// Update configuration.
#[tauri::command]
pub async fn configure(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    update: ConfigUpdate,
) -> Result<(), String> {
    match update.key.as_str() {
        "model" => {
            let mut model = state.model.lock().await;
            *model = update.value.clone();
            let mut cfg = state.client_config.write().await;
            cfg.model = update.value;

            // Update desktop config and persist
            let mut desktop_cfg = state.desktop_config.write().await;
            desktop_cfg.model = Some((*model).clone());
            drop(desktop_cfg);

            let desktop_cfg = state.desktop_config.read().await;
            config::save_config(&desktop_cfg)?;

            // Emit config updated event
            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: "model".into(),
                    value: (*model).clone(),
                },
            );

            Ok(())
        }
        "api_key" => {
            let mut desktop_cfg = state.desktop_config.write().await;
            desktop_cfg.api_key = Some(update.value.clone());

            // Update client config
            let mut cfg = state.client_config.write().await;
            cfg.api_key = update.value.clone();

            drop(desktop_cfg);
            let desktop_cfg = state.desktop_config.read().await;
            config::save_config(&desktop_cfg)?;

            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: "api_key".into(),
                    value: "***".into(),
                },
            );

            Ok(())
        }
        "base_url" => {
            let mut desktop_cfg = state.desktop_config.write().await;
            desktop_cfg.base_url = Some(update.value.clone());

            let mut cfg = state.client_config.write().await;
            cfg.base_url = update.value.clone();

            drop(desktop_cfg);
            let desktop_cfg = state.desktop_config.read().await;
            config::save_config(&desktop_cfg)?;

            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: "base_url".into(),
                    value: update.value,
                },
            );

            Ok(())
        }
        "provider" => {
            let mut provider = state.provider.lock().await;
            *provider = update.value.clone();

            let mut desktop_cfg = state.desktop_config.write().await;
            desktop_cfg.provider = Some((*provider).clone());

            drop(desktop_cfg);
            let desktop_cfg = state.desktop_config.read().await;
            config::save_config(&desktop_cfg)?;

            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: "provider".into(),
                    value: update.value,
                },
            );

            Ok(())
        }
        "working_dir" => {
            let mut desktop_cfg = state.desktop_config.write().await;
            desktop_cfg.working_dir = Some(update.value.clone());

            drop(desktop_cfg);
            let desktop_cfg = state.desktop_config.read().await;
            config::save_config(&desktop_cfg)?;

            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: "working_dir".into(),
                    value: update.value,
                },
            );

            Ok(())
        }
        "theme" => {
            let mut desktop_cfg = state.desktop_config.write().await;
            desktop_cfg.theme = Some(update.value.clone());

            drop(desktop_cfg);
            let desktop_cfg = state.desktop_config.read().await;
            config::save_config(&desktop_cfg)?;

            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: "theme".into(),
                    value: update.value,
                },
            );

            Ok(())
        }
        "approval_mode" => {
            let mut desktop_cfg = state.desktop_config.write().await;
            desktop_cfg.approval_mode = Some(update.value.clone());

            drop(desktop_cfg);
            let desktop_cfg = state.desktop_config.read().await;
            config::save_config(&desktop_cfg)?;

            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: "approval_mode".into(),
                    value: update.value,
                },
            );

            Ok(())
        }
        "strategic_focus" => {
            let mut desktop_cfg = state.desktop_config.write().await;
            desktop_cfg.strategic_focus = Some(update.value.clone());

            drop(desktop_cfg);
            let desktop_cfg = state.desktop_config.read().await;
            config::save_config(&desktop_cfg)?;

            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: "strategic_focus".into(),
                    value: update.value,
                },
            );

            Ok(())
        }
        "performance_strategy" => {
            let strategy = update.value.clone();
            if !matches!(strategy.as_str(), "speed" | "balanced" | "high-quality") {
                return Err(format!("Invalid performance_strategy: {strategy}"));
            }
            let mut desktop_cfg = state.desktop_config.write().await;
            desktop_cfg.performance_strategy = Some(strategy.clone());

            drop(desktop_cfg);
            let desktop_cfg = state.desktop_config.read().await;
            config::save_config(&desktop_cfg)?;

            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: "performance_strategy".into(),
                    value: strategy,
                },
            );

            Ok(())
        }
        "memory_enabled" | "telemetry" | "encryption" | "debug_console" => {
            let enabled = match update.value.to_ascii_lowercase().as_str() {
                "true" => true,
                "false" => false,
                _ => {
                    return Err(format!(
                        "Invalid boolean for {}: {}",
                        update.key, update.value
                    ));
                }
            };
            let mut desktop_cfg = state.desktop_config.write().await;
            match update.key.as_str() {
                "memory_enabled" => desktop_cfg.memory_enabled = Some(enabled),
                "telemetry" => desktop_cfg.telemetry_enabled = Some(enabled),
                "encryption" => desktop_cfg.encryption_enabled = Some(enabled),
                "debug_console" => desktop_cfg.debug_console = Some(enabled),
                _ => unreachable!(),
            }

            drop(desktop_cfg);
            let desktop_cfg = state.desktop_config.read().await;
            config::save_config(&desktop_cfg)?;

            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: update.key.clone(),
                    value: update.value,
                },
            );

            Ok(())
        }
        "temperature" => {
            let parsed: f32 = update
                .value
                .parse()
                .map_err(|e| format!("Invalid temperature: {e}"))?;
            let mut desktop_cfg = state.desktop_config.write().await;
            desktop_cfg.temperature = Some(parsed);

            drop(desktop_cfg);
            let desktop_cfg = state.desktop_config.read().await;
            config::save_config(&desktop_cfg)?;

            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: "temperature".into(),
                    value: update.value,
                },
            );

            Ok(())
        }
        "max_tokens" => {
            let parsed: u32 = update
                .value
                .parse()
                .map_err(|e| format!("Invalid max_tokens: {e}"))?;
            let mut desktop_cfg = state.desktop_config.write().await;
            desktop_cfg.max_tokens = Some(parsed);

            drop(desktop_cfg);
            let desktop_cfg = state.desktop_config.read().await;
            config::save_config(&desktop_cfg)?;

            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: "max_tokens".into(),
                    value: update.value,
                },
            );

            Ok(())
        }
        "plan" => {
            let mut desktop_cfg = state.desktop_config.write().await;
            desktop_cfg.plan = Some(update.value.clone());

            drop(desktop_cfg);
            let desktop_cfg = state.desktop_config.read().await;
            config::save_config(&desktop_cfg)?;

            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: "plan".into(),
                    value: update.value,
                },
            );

            Ok(())
        }
        "clear_cache" => {
            // Clear in-memory conversation state. Session history on disk is
            // preserved; this drops the active conversation buffer.
            let mut messages = state.messages.lock().await;
            messages.clear();
            Ok(())
        }
        "factory_reset" => {
            // Reset desktop config to defaults. Does not touch session files
            // — the user is warned in the UI before invoking.
            let default_cfg = DesktopConfig::default();
            let mut desktop_cfg = state.desktop_config.write().await;
            *desktop_cfg = default_cfg.clone();
            drop(desktop_cfg);
            config::save_config(&default_cfg)?;

            let _ = app_handle.emit(
                event_names::CONFIG_UPDATED,
                events::ConfigUpdatedPayload {
                    key: "factory_reset".into(),
                    value: "true".into(),
                },
            );

            Ok(())
        }
        "cancel_subscription" => {
            // Local OSS app: no subscription system. Acknowledge the request
            // and clear any persisted plan so the UI reflects the downgrade.
            let mut desktop_cfg = state.desktop_config.write().await;
            desktop_cfg.plan = None;
            drop(desktop_cfg);
            let desktop_cfg = state.desktop_config.read().await;
            config::save_config(&desktop_cfg)?;

            Ok(())
        }
        _ => Err(format!("Unknown config key: {}", update.key)),
    }
}

/// Switch to a different LLM provider.
#[tauri::command]
pub async fn switch_provider(
    state: tauri::State<'_, AppState>,
    request: ProviderSwitchRequest,
) -> Result<(), String> {
    // Preserve existing config, only update provider fields
    let existing = state.desktop_config.read().await;
    let new_config = DesktopConfig {
        provider: Some(request.provider.clone()),
        api_key: request.api_key.clone().or_else(|| existing.api_key.clone()),
        base_url: request
            .base_url
            .clone()
            .or_else(|| existing.base_url.clone()),
        model: Some(request.model.clone()),
        working_dir: existing.working_dir.clone(),
        theme: existing.theme.clone(),
        mcp_servers: existing.mcp_servers.clone(),
        approval_mode: existing.approval_mode.clone(),
        strategic_focus: existing.strategic_focus.clone(),
        performance_strategy: existing.performance_strategy.clone(),
        memory_enabled: existing.memory_enabled,
        telemetry_enabled: existing.telemetry_enabled,
        encryption_enabled: existing.encryption_enabled,
        debug_console: existing.debug_console,
        temperature: existing.temperature,
        max_tokens: existing.max_tokens,
        plan: existing.plan.clone(),
    };
    drop(existing);

    let client_config = AppState::build_client_config(&new_config);

    // Update all state
    {
        let mut c = state.client_config.write().await;
        *c = client_config;
    }
    {
        let mut m = state.model.lock().await;
        *m = request.model.clone();
    }
    {
        let mut p = state.provider.lock().await;
        *p = request.provider;
    }
    {
        let mut dc = state.desktop_config.write().await;
        *dc = new_config.clone();
    }

    // Persist
    config::save_config(&new_config)?;

    Ok(())
}

/// Get the current desktop config (for settings panel).
#[tauri::command]
pub async fn get_config(state: tauri::State<'_, AppState>) -> Result<DesktopConfig, String> {
    let cfg = state.desktop_config.read().await;
    // Redact API key for display
    let mut display = cfg.clone();
    if display.api_key.is_some() {
        display.api_key = Some("***".into());
    }
    Ok(display)
}

// Session lifecycle commands (new/list/search/load/export/switch/
// set_working_dir/delete/rename/duplicate/branch_session) extracted to
// `commands_sessions.rs`. Registered in main.rs as commands_sessions::*.

/// Save a text payload (e.g. an exported session) to disk. The frontend
/// pairs this with @tauri-apps/plugin-dialog's save() to let the user
/// choose the destination — the backend stays out of UI concerns.
#[tauri::command]
pub async fn save_text_file(path: String, content: String) -> Result<(), String> {
    let target = std::path::Path::new(&path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {e}", parent.display()))?;
    }
    std::fs::write(target, content)
        .map_err(|e| format!("Failed to write {}: {e}", target.display()))
}

/// Request permission for a tool execution.
#[tauri::command]
pub async fn request_permission(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    tool: String,
    input: serde_json::Value,
    risk: String,
) -> Result<bool, String> {
    let request_id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = oneshot::channel();

    // Store the sender
    {
        let mut pending = state.pending_permissions.lock().await;
        pending.insert(request_id.clone(), tx);
    }

    // Emit event to frontend
    let _ = app_handle.emit(
        events::event_names::PERMISSION_REQUEST,
        events::PermissionRequest {
            tool: tool.clone(),
            input: input.clone(),
            risk: risk.clone(),
            request_id: request_id.clone(),
        },
    );

    // Wait for response with 30s timeout
    let timeout = tokio::time::Duration::from_secs(30);
    let result = tokio::time::timeout(timeout, rx).await;

    // Clean up
    {
        let mut pending = state.pending_permissions.lock().await;
        pending.remove(&request_id);
    }

    match result {
        Ok(Ok(allowed)) => Ok(allowed),
        Ok(Err(_)) => Ok(false), // Sender dropped
        Err(_) => Ok(false),     // Timeout
    }
}

/// Respond to a permission request.
#[tauri::command]
pub async fn respond_permission(
    state: tauri::State<'_, AppState>,
    request_id: String,
    allow: bool,
) -> Result<(), String> {
    let mut pending = state.pending_permissions.lock().await;
    if let Some(tx) = pending.remove(&request_id) {
        // Send response, ignoring errors if receiver dropped
        let _ = tx.send(allow);
        Ok(())
    } else {
        Err(format!("Permission request not found: {}", request_id))
    }
}

/// File diff result for the diff viewer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiff {
    pub old_content: String,
    pub new_content: String,
    pub file_name: String,
    pub language: String,
}

/// A node in the file tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTreeNode {
    pub name: String,
    pub path: String,
    #[serde(rename = "type")]
    pub node_type: String, // "file" or "directory"
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<FileTreeNode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

/// Working directory info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkingDirInfo {
    pub root: String,
    pub branch: String,
    pub modified_files: Vec<String>,
    pub status: String, // "clean", "dirty", "merge-conflict"
}

/// Get the diff for a file (working tree vs last committed, or old vs new content).
#[tauri::command]
pub async fn get_file_diff(path: String) -> Result<FileDiff, String> {
    use std::process::Command;

    // Validate path is within CWD to prevent path traversal
    let file_path = std::path::Path::new(&path);
    let canonical = file_path
        .canonicalize()
        .map_err(|e| format!("Invalid path: {e}"))?;
    let cwd = std::env::current_dir()
        .map_err(|e| format!("Cannot determine CWD: {e}"))?
        .canonicalize()
        .map_err(|e| format!("Cannot canonicalize CWD: {e}"))?;
    if !canonical.starts_with(&cwd) {
        return Err("Path outside workspace".to_string());
    }

    let file_name = file_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.clone());

    // Detect language from extension
    let language = file_path
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_else(|| "plaintext".to_string());

    // Try git diff first
    let dir = file_path.parent().unwrap_or(std::path::Path::new("."));
    let git_output = Command::new("git")
        .args(["diff", "HEAD", "--", &path])
        .current_dir(dir)
        .output();

    let (old_content, new_content) = match git_output {
        Ok(output) if output.status.success() && !output.stdout.is_empty() => {
            // Parse unified diff - for simplicity, just read current file as new
            // and reconstruct old from git show
            let new = std::fs::read_to_string(&path).unwrap_or_default();
            let old_output = Command::new("git")
                .args(["show", &format!("HEAD:{}", path)])
                .current_dir(dir)
                .output();
            let old = match old_output {
                Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
                _ => String::new(),
            };
            (old, new)
        }
        _ => {
            // Not a git repo or no changes - read file as new, empty old
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            (String::new(), content)
        }
    };

    Ok(FileDiff {
        old_content,
        new_content,
        file_name,
        language,
    })
}



pub(crate) fn chrono_timestamp() -> i64 {
    // Milliseconds since UNIX_EPOCH. All UI consumers construct
    // `new Date(ts)` which interprets the argument as milliseconds.
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn provider_from_str(s: &str) -> shannon_core::api::types::LlmProvider {
    use shannon_core::api::types::LlmProvider;
    match s {
        "anthropic" => LlmProvider::Anthropic,
        "openai" => LlmProvider::OpenAI,
        "ollama" => LlmProvider::Ollama,
        "deepseek" => LlmProvider::DeepSeek,
        "gemini" => LlmProvider::Gemini,
        "mistral" => LlmProvider::Mistral,
        "groq" => LlmProvider::Groq,
        "openrouter" => LlmProvider::OpenRouter,
        "xai" => LlmProvider::Xai,
        _ => LlmProvider::Custom,
    }
}

/// Apply diff with hunk actions.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn apply_diff(
    state: tauri::State<'_, AppState>,
    file_path: String,
    hunks: Vec<HunkAction>,
) -> Result<(), String> {
    use std::fs;
    use std::io::Write;

    // Security: validate the file path is inside the working directory. The
    // previous `contains("..")` check was insufficient — it allowed absolute
    // paths like `/etc/hosts`, and did not catch symlinks that escape the
    // workspace. Canonicalize + starts_with closes all three holes at once.
    let working_dir = resolve_working_dir(&state).await;
    let path = crate::resolve_path_in_working_dir(&file_path, &working_dir)?;
    if !path.is_file() {
        return Err(format!("File not found: {}", path.display()));
    }
    let file_path = path.to_string_lossy().into_owned();

    // Read current file content
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read file {}: {}", file_path, e))?;

    let mut lines: Vec<&str> = content.lines().collect();

    // Apply hunk actions in reverse order to maintain line numbers
    let mut sorted_hunks: Vec<_> = hunks.iter().enumerate().collect();
    sorted_hunks.sort_by_key(|(idx, h)| (std::cmp::Reverse(h.line_start), *idx));

    for (idx, hunk) in sorted_hunks {
        if hunk.line_start == 0 || hunk.line_end == 0 {
            continue; // Invalid hunk
        }

        let start_idx = (hunk.line_start - 1) as usize;
        let end_idx = hunk.line_end as usize;

        if start_idx >= lines.len() || end_idx > lines.len() {
            return Err(format!("Hunk {} out of bounds for file {}", idx, file_path));
        }

        match hunk.action.as_str() {
            "accept" => {
                // Keep the lines (do nothing)
            }
            "reject" => {
                // Remove the lines by replacing with empty strings
                for i in start_idx..end_idx {
                    lines[i] = "";
                }
            }
            _ => {
                return Err(format!("Unknown action {} in hunk {}", hunk.action, idx));
            }
        }
    }

    // Write back the modified content
    let modified_content = lines.join("\n") + "\n";
    let mut file = fs::File::create(&file_path)
        .map_err(|e| format!("Failed to create file {}: {}", file_path, e))?;
    file.write_all(modified_content.as_bytes())
        .map_err(|e| format!("Failed to write file {}: {}", file_path, e))?;

    Ok(())
}

/// Start a new background task.
#[tauri::command]
pub async fn start_background_task(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    prompt: String,
) -> Result<String, String> {
    let task_id = uuid::Uuid::new_v4().to_string();
    let now = chrono_timestamp();

    let task = BackgroundTaskMeta {
        id: task_id.clone(),
        prompt: prompt.clone(),
        status: "running".into(),
        started_at: now,
        completed_at: None,
        output: String::new(),
    };

    // Add task to state
    {
        let mut tasks = state.background_tasks.lock().await;
        tasks.push(task);
    }

    // Emit background tasks updated event
    let _ = app_handle.emit(event_names::BACKGROUND_TASKS_UPDATED, ());

    // Execute the prompt in a real async background task
    let tasks_arc = state.background_tasks.clone();
    let app_handle_clone = app_handle.clone();
    let task_id_clone = task_id.clone();
    let client_config = state.client_config.read().await.clone();
    let tools = state.tools.clone();
    let _qe_config = state.qe_config.read().await.clone();
    let model = state.model.lock().await.clone();
    let approval_mode_str = state.desktop_config.read().await.approval_mode.clone();

    tokio::spawn(async move {
        // Build query engine for this task
        let client = LlmClient::new(client_config);

        // Create PermissionManager — use configured approval mode for background tasks
        let mut permissions = PermissionManager::new();
        let mode = approval_mode_str
            .as_deref()
            .and_then(|s| match s {
                "full_auto" => Some(ApprovalMode::FullAuto),
                "auto_edit" => Some(ApprovalMode::AutoEdit),
                "auto" => Some(ApprovalMode::Auto),
                "plan" => Some(ApprovalMode::Plan),
                _ => None,
            })
            .unwrap_or(ApprovalMode::FullAuto);
        permissions.set_approval_mode(mode);

        let engine =
            QueryEngine::with_defaults_arc(client, tools, permissions, StateManager::new());

        let query_id = uuid::Uuid::new_v4();
        let _qid_str = query_id.to_string();

        let context = QueryContext {
            query_id,
            session_id: uuid::Uuid::new_v4(),
            user_message: prompt.clone(),
            metadata: shannon_core::query_engine::QueryMetadata {
                timestamp: chrono::Utc::now(),
                tools_allowed: true,
                max_tokens: None,
                model,
                temperature: None,
                top_p: None,
            },
        };

        let mut final_output = String::new();

        // Process the query and collect output
        let stream = engine.process_query(context, None).await;
        use futures::StreamExt;
        let mut pin_stream = std::pin::pin!(stream);

        while let Some(event_result) = pin_stream.next().await {
            match event_result {
                Ok(event) => match event {
                    QueryEvent::Text { content, .. } => {
                        final_output.push_str(&content);
                    }
                    QueryEvent::Completed { .. } => break,
                    QueryEvent::Failed { error, .. } => {
                        final_output = format!("Task failed: {}", error);
                        break;
                    }
                    _ => {}
                },
                Err(e) => {
                    final_output = format!("Task error: {}", e);
                    break;
                }
            }
        }

        // Update task with results
        let mut tasks = tasks_arc.lock().await;
        if let Some(task) = tasks.iter_mut().find(|t| t.id == task_id_clone) {
            task.status = "completed".into();
            task.completed_at = Some(chrono_timestamp());
            task.output = final_output.clone();
        }

        // Emit update event
        let _ = app_handle_clone.emit(
            event_names::BACKGROUND_TASK_UPDATE,
            events::BackgroundTaskUpdate {
                task_id: task_id_clone.clone(),
                status: "completed".into(),
                prompt,
                output: final_output,
                started_at: now,
                completed_at: Some(chrono_timestamp()),
            },
        );

        let _ = app_handle_clone.emit(event_names::BACKGROUND_TASKS_UPDATED, ());
    });

    Ok(task_id)
}

/// Get all background tasks.
#[tauri::command]
pub async fn get_background_tasks(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<events::BackgroundTaskInfo>, String> {
    let tasks = state.background_tasks.lock().await;
    Ok(tasks
        .iter()
        .map(|t| events::BackgroundTaskInfo {
            task_id: t.id.clone(),
            prompt: t.prompt.clone(),
            status: t.status.clone(),
            started_at: t.started_at,
            completed_at: t.completed_at,
            output: t.output.clone(),
        })
        .collect())
}

/// Cancel a background task.
#[tauri::command]
pub async fn cancel_background_task(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
) -> Result<bool, String> {
    let mut tasks = state.background_tasks.lock().await;
    if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
        if task.status == "running" {
            task.status = "cancelled".into();
            task.completed_at = Some(chrono_timestamp());
            task.output = "Task cancelled by user".into();

            // Emit update event
            let _ = app_handle.emit(
                event_names::BACKGROUND_TASK_UPDATE,
                events::BackgroundTaskUpdate {
                    task_id: id.clone(),
                    status: "cancelled".into(),
                    prompt: task.prompt.clone(),
                    output: "Task cancelled by user".into(),
                    started_at: task.started_at,
                    completed_at: task.completed_at,
                },
            );

            let _ = app_handle.emit(event_names::BACKGROUND_TASKS_UPDATED, ());
            Ok(true)
        } else {
            Err("Task is not running".into())
        }
    } else {
        Err("Task not found".into())
    }
}

/// List active agents (derived from background tasks).
#[tauri::command]
pub async fn list_agents(state: tauri::State<'_, AppState>) -> Result<Vec<AgentInfo>, String> {
    let tasks = state.background_tasks.lock().await;
    let agents: Vec<AgentInfo> = tasks
        .iter()
        .map(|t| {
            let status = match t.status.as_str() {
                "running" => "running",
                "completed" => "completed",
                "failed" => "failed",
                _ => "pending",
            };
            let duration = t.completed_at.map(|end| end - t.started_at);
            AgentInfo {
                id: t.id.clone(),
                name: "Background Agent".into(),
                model: "default".into(),
                status: status.into(),
                task: Some(t.prompt.clone()),
                progress: None,
                tools_used: None,
                duration,
            }
        })
        .collect();
    Ok(agents)
}

/// Serializable view of a recorded inter-agent message.
///
/// Mirrors `shannon_agents::message_history::MessageRecord` for the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessageEntry {
    pub message_id: String,
    pub team: String,
    pub from: String,
    pub to: String,
    pub content_preview: String,
    pub content_kind: String,
    pub priority: String,
    pub timestamp: i64,
}

/// List inter-agent messages for a team (most recent first).
///
/// Pass `team=None` to scan all teams (`<adhoc>` plus any team dirs).
#[tauri::command]
pub async fn list_agent_messages(
    state: tauri::State<'_, AppState>,
    team: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<AgentMessageEntry>, String> {
    let store = state.agent_message_history.clone();
    let limit = limit.unwrap_or(100).min(500);
    let mut out: Vec<AgentMessageEntry> = Vec::new();

    let teams: Vec<String> = match team {
        Some(t) => vec![t],
        None => list_message_team_dirs(&store),
    };

    for t in teams {
        match store.list_by_team(&t, limit) {
            Ok(records) => {
                for r in records {
                    out.push(AgentMessageEntry {
                        message_id: r.message_id,
                        team: r.team,
                        from: r.from,
                        to: r.to,
                        content_preview: r.content_preview,
                        content_kind: r.content_kind.as_str().into(),
                        priority: r.priority,
                        timestamp: r.timestamp.timestamp(),
                    });
                }
            }
            Err(e) => tracing::warn!(error = %e, team = %t, "list_agent_messages: skipping team"),
        }
    }

    out.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    out.truncate(limit);
    Ok(out)
}

/// Enumerate teams that have at least one recorded message directory.
#[tauri::command]
pub async fn list_agent_message_teams(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, String> {
    Ok(list_message_team_dirs(&state.agent_message_history))
}

fn list_message_team_dirs(
    store: &shannon_agents::message_history::MessageHistoryStore,
) -> Vec<String> {
    let base = store.base_dir();
    let mut teams = Vec::new();
    if let Ok(entries) = std::fs::read_dir(base) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                if let Some(name) = entry.file_name().to_str() {
                    teams.push(name.to_string());
                }
            }
        }
    }
    teams.sort();
    teams
}

/// Record an inter-agent message into the append-only history.
///
/// Used by the desktop UI's "Agent Messages" panel for manual / test injection
/// until real team agents are wired in. Real team agents write directly via
/// `AgentCoordinator::record_to_history` (see `shannon-agents`).
#[tauri::command]
pub async fn record_agent_message(
    state: tauri::State<'_, AppState>,
    team: String,
    from: String,
    to: String,
    content: String,
    priority: Option<String>,
) -> Result<String, String> {
    use shannon_agents::message_history::{ContentKind, MessageRecord};

    let priority = priority.unwrap_or_else(|| "normal".into());
    let record = MessageRecord {
        message_id: uuid::Uuid::new_v4().to_string(),
        team,
        from,
        to,
        content_preview: MessageRecord::truncate_preview(&content),
        content_kind: ContentKind::Text,
        priority,
        timestamp: chrono::Utc::now(),
        revision: 0,
    };
    state
        .agent_message_history
        .record(&record)
        .map_err(|e| e.to_string())
}

/// Serializable view of an agent definition loaded from disk.
///
/// Mirrors `shannon_skills::agent_loader::AgentDefinition` minus the
/// file-system-only fields. Used by the desktop UI's "My Agents" panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefinitionInfo {
    pub name: String,
    pub description: String,
    pub tools: Vec<String>,
    pub model: String,
    pub prompt: String,
    pub source_path: String,
}

/// Resolve the working directory used for agent file discovery / creation.
///
/// Prefers the persisted `working_dir`, falls back to the process cwd.
async fn resolve_working_dir(state: &AppState) -> std::path::PathBuf {
    let cfg = state.desktop_config.read().await;
    cfg.working_dir
        .clone()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        })
}

/// List agent definitions (`.claude/agents/*.md` and `.shannon/agents/*.md`)
/// discovered from the working directory upward.
#[tauri::command]
pub async fn list_agent_definitions(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<AgentDefinitionInfo>, String> {
    let cwd = resolve_working_dir(&state).await;
    let dirs = shannon_skills::agent_loader::discover_agent_directories(&cwd);
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for dir in dirs {
        let agents = shannon_skills::agent_loader::load_agents_from_directory(&dir)
            .map_err(|e| e.to_string())?;
        for a in agents {
            if seen.insert(a.name.clone()) {
                out.push(AgentDefinitionInfo {
                    name: a.name,
                    description: a.description,
                    tools: a.tools,
                    model: format!("{:?}", a.model).to_ascii_lowercase(),
                    prompt: a.prompt,
                    source_path: a.source_path.to_string_lossy().into_owned(),
                });
            }
        }
    }
    Ok(out)
}

/// Create a new agent definition by writing `.claude/agents/<name>.md`.
///
/// The file uses Claude Code-compatible YAML frontmatter so the same
/// definition works in `claude code`, Codex, and Shannon. Returns the
/// absolute path of the created file.
#[tauri::command]
pub async fn create_agent_definition(
    state: tauri::State<'_, AppState>,
    name: String,
    model: Option<String>,
    system_prompt: Option<String>,
    tools: Vec<String>,
) -> Result<String, String> {
    let sanitized = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        return Err("Agent name is required".into());
    }

    let cwd = resolve_working_dir(&state).await;
    let agents_dir = cwd.join(".claude").join("agents");
    std::fs::create_dir_all(&agents_dir).map_err(|e| e.to_string())?;
    let file_path = agents_dir.join(format!("{sanitized}.md"));
    if file_path.exists() {
        return Err(format!("Agent '{sanitized}' already exists"));
    }

    let model_line = model
        .as_deref()
        .filter(|m| !m.trim().is_empty())
        .unwrap_or("sonnet");
    let tools_line = if tools.is_empty() {
        "Read, Glob, Grep, Bash".to_string()
    } else {
        tools
            .iter()
            .map(|t| {
                let t = t.trim();
                let first = t.chars().next().map(|c| c.to_ascii_uppercase());
                let rest: String = t.chars().skip(1).collect();
                first.map(|f| format!("{f}{rest}")).unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let description = format!("Agent created via Shannon Desktop: {sanitized}");
    let prompt_body = system_prompt
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("You are a helpful agent. Complete the task thoroughly.");

    let body = format!(
        "---\n\
         name: {sanitized}\n\
         description: {description}\n\
         tools: {tools_line}\n\
         model: {model_line}\n\
         ---\n\n\
         {prompt_body}\n"
    );
    std::fs::write(&file_path, body).map_err(|e| e.to_string())?;
    Ok(file_path.to_string_lossy().into_owned())
}

/// Delete an agent definition file. Only deletes files inside the
/// discovered agent directories to prevent arbitrary file deletion.
#[tauri::command]
pub async fn delete_agent_definition(
    state: tauri::State<'_, AppState>,
    name: String,
) -> Result<bool, String> {
    let sanitized = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>();
    let cwd = resolve_working_dir(&state).await;
    let dirs = shannon_skills::agent_loader::discover_agent_directories(&cwd);
    for dir in dirs {
        let candidate = dir.join(format!("{sanitized}.md"));
        if candidate.exists() {
            // Ensure the resolved path is inside `dir` (no traversal).
            let canonical_dir = dir.canonicalize().map_err(|e| e.to_string())?;
            let canonical_candidate = candidate.canonicalize().map_err(|e| e.to_string())?;
            if !canonical_candidate.starts_with(&canonical_dir) {
                return Err("Refusing to delete file outside agent directory".into());
            }
            std::fs::remove_file(&canonical_candidate).map_err(|e| e.to_string())?;
            return Ok(true);
        }
    }
    Ok(false)
}

/// List tasks from .claude/tasks/ directory (team task system).
///
/// Recurses into team subdirectories: `.claude/tasks/{team}/{id}.json`. Also
/// accepts top-level `.json` files for backward compatibility. Parses
/// `blockedBy`, `blocks`, `dueDate`, `activeForm`, `owner`, and `priority`
/// from the JSON shape used by the Claude Code / Shannon task format.
#[tauri::command]
pub async fn list_tasks() -> Result<Vec<TaskInfo>, String> {
    let tasks_dir = std::path::Path::new(".claude/tasks");
    if !tasks_dir.is_dir() {
        return Ok(Vec::new());
    }

    let canonical_root = tasks_dir
        .canonicalize()
        .map_err(|e| format!("Invalid tasks dir: {e}"))?;

    let mut tasks = Vec::new();
    collect_tasks_recursive(&canonical_root, &canonical_root, &mut tasks)?;
    tasks.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(tasks)
}

/// Recursively walk `dir`, parse any `*.json` file as a TaskInfo-like record,
/// and append to `out`. Skips symlinks pointing outside `root`. The team
/// (session subdir name) is derived from the parent directory of each file
/// relative to `root` and assigned to the parsed TaskInfo.
fn collect_tasks_recursive(
    dir: &std::path::Path,
    root: &std::path::Path,
    out: &mut Vec<TaskInfo>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("Cannot read tasks dir {}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let canonical = match path.canonicalize() {
            Ok(c) => c,
            Err(_) => continue,
        };
        if !canonical.starts_with(root) {
            continue;
        }
        if canonical.is_dir() {
            // Recurse into team/session subdirectory.
            collect_tasks_recursive(&canonical, root, out)?;
            continue;
        }
        if path.extension().map(|e| e == "json").unwrap_or(false) {
            let content = match std::fs::read_to_string(&path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let task: serde_json::Value = match serde_json::from_str(&content) {
                Ok(v) => v,
                Err(_) => continue,
            };
            // Derive team name from parent dir relative to root.
            // e.g. `.claude/tasks/<session-uuid>/3.json` → team = "<session-uuid>".
            // Top-level files (`.claude/tasks/3.json`) → team = None.
            let team = path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .filter(|_name| {
                    // Drop when the parent IS the root.
                    path.parent()
                        .and_then(|p| p.canonicalize().ok())
                        .map(|canon_parent| canon_parent != *root)
                        .unwrap_or(true)
                })
                .map(String::from);
            if let Some(parsed) = parse_task_value(&task, team) {
                out.push(parsed);
            }
        }
    }
    Ok(())
}

/// Convert a raw JSON value (from disk) into a `TaskInfo`. Returns `None`
/// when the value lacks an `id` field. Field names follow the Shannon task
/// schema: `id`, `subject`, `status`, `owner`, `description`, `priority`,
/// `dueDate`, `activeForm`, `blocks`, `blockedBy`, `executionMode`.
fn parse_task_value(task: &serde_json::Value, team: Option<String>) -> Option<TaskInfo> {
    let id = task.get("id").and_then(|v| v.as_str())?.to_string();
    let title = task
        .get("subject")
        .and_then(|v| v.as_str())
        .unwrap_or("Untitled")
        .to_string();
    let status = task
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("pending")
        .to_string();
    let owner = task
        .get("owner")
        .and_then(|v| v.as_str())
        .map(String::from)
        .filter(|o| !o.is_empty());
    let assignee = task
        .get("assignee")
        .and_then(|v| v.as_str())
        .map(String::from)
        .filter(|o| !o.is_empty())
        .or(owner);
    let priority = task
        .get("priority")
        .and_then(|v| v.as_str())
        .map(String::from)
        .filter(|o| !o.is_empty());
    let description = task
        .get("description")
        .and_then(|v| v.as_str())
        .map(String::from);
    let active_form = task
        .get("activeForm")
        .and_then(|v| v.as_str())
        .map(String::from);
    let due_date = task
        .get("dueDate")
        .and_then(|v| v.as_i64())
        .or_else(|| task.get("due_date").and_then(|v| v.as_i64()));
    let execution_mode = task
        .get("executionMode")
        .and_then(|v| v.as_str())
        .or_else(|| task.get("execution_mode").and_then(|v| v.as_str()))
        .map(String::from)
        .filter(|o| o == "parallel" || o == "serial");
    let blocked_by = collect_string_array(task, "blockedBy")
        .into_iter()
        .chain(collect_string_array(task, "blocked_by"))
        .collect();
    let blocks = collect_string_array(task, "blocks");
    Some(TaskInfo {
        id,
        title,
        status,
        assignee,
        priority,
        description,
        blocked_by,
        blocks,
        due_date,
        active_form,
        execution_mode,
        team,
    })
}

/// Read a JSON object field as a `Vec<String>`. Accepts arrays of strings
/// or arrays of objects with an `id` field.
fn collect_string_array(obj: &serde_json::Value, key: &str) -> Vec<String> {
    let arr = match obj.get(key).and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return Vec::new(),
    };
    arr.iter()
        .filter_map(|v| {
            v.as_str()
                .map(String::from)
                .or_else(|| v.get("id").and_then(|i| i.as_str()).map(String::from))
        })
        .collect()
}

/// Update a task's mutable fields (status, assignee, priority, due_date) and
/// persist back to `.claude/tasks/{team}/{id}.json`. Searches all team
/// subdirectories for the matching id; if not found, creates a new file at
/// `.claude/tasks/<adhoc>/{id}.json`. Returns the updated TaskInfo.
#[tauri::command]
pub async fn update_task(payload: UpdateTaskPayload) -> Result<TaskInfo, String> {
    let tasks_dir = std::path::Path::new(".claude/tasks");
    let canonical_root = match tasks_dir.canonicalize() {
        Ok(c) => c,
        Err(_) => {
            std::fs::create_dir_all(tasks_dir)
                .map_err(|e| format!("Cannot create tasks dir: {e}"))?;
            tasks_dir
                .canonicalize()
                .map_err(|e| format!("Invalid tasks dir: {e}"))?
        }
    };

    let existing = find_task_file(&canonical_root, &payload.id)?;
    let target_path = match existing {
        Some(p) => p,
        None => {
            let adhoc = canonical_root.join("<adhoc>");
            std::fs::create_dir_all(&adhoc).map_err(|e| format!("Cannot create adhoc dir: {e}"))?;
            adhoc.join(format!("{}.json", payload.id))
        }
    };

    // Read existing JSON (or start from {} if missing) so we preserve fields
    // we don't manage (e.g. activeForm, description) on write-back.
    let mut doc: serde_json::Value = std::fs::read_to_string(&target_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if doc.get("id").is_none() {
        doc["id"] = serde_json::Value::String(payload.id.clone());
    }
    if let Some(status) = payload.status {
        doc["status"] = serde_json::Value::String(status);
    }
    if let Some(assignee) = payload.assignee {
        doc["assignee"] = serde_json::Value::String(assignee);
    }
    if let Some(priority) = payload.priority {
        doc["priority"] = serde_json::Value::String(priority);
    }
    if let Some(due) = payload.due_date {
        doc["dueDate"] = serde_json::Value::Number(serde_json::Number::from(due));
    }
    if let Some(mode) = payload.execution_mode {
        if mode == "parallel" || mode == "serial" {
            doc["executionMode"] = serde_json::Value::String(mode);
        }
    }

    // Atomic write: temp file + rename.
    let serialized =
        serde_json::to_string_pretty(&doc).map_err(|e| format!("Serialize failed: {e}"))?;
    let tmp = target_path.with_extension("json.tmp");
    std::fs::write(&tmp, serialized).map_err(|e| format!("Write failed: {e}"))?;
    std::fs::rename(&tmp, &target_path).map_err(|e| format!("Rename failed: {e}"))?;

    // team is derived from path during list_tasks; not recoverable here
    // since we operate on the doc only. Pass None.
    parse_task_value(&doc, None).ok_or_else(|| "Updated task is missing id".into())
}

/// Find the JSON file for a given task id by walking the tasks root.
/// Returns the canonical path if found.
fn find_task_file(root: &std::path::Path, id: &str) -> Result<Option<std::path::PathBuf>, String> {
    let target_name = format!("{id}.json");
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let canonical = match dir.canonicalize() {
            Ok(c) => c,
            Err(_) => continue,
        };
        if !canonical.starts_with(root) {
            continue;
        }
        let entries = match std::fs::read_dir(&canonical) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path
                .file_name()
                .map(|n| n == target_name.as_str())
                .unwrap_or(false)
            {
                return Ok(Some(path));
            }
        }
    }
    Ok(None)
}

/// Recursively read a directory and return a file tree.
#[tauri::command]
pub async fn get_file_tree(path: String) -> Result<Vec<FileTreeNode>, String> {
    use std::fs;
    let root = std::path::Path::new(&path);
    if !root.is_dir() {
        return Err("Path is not a directory".into());
    }
    fn build_tree(dir: &std::path::Path) -> Result<Vec<FileTreeNode>, String> {
        let mut entries: Vec<std::fs::DirEntry> = fs::read_dir(dir)
            .map_err(|e| format!("Cannot read dir: {e}"))?
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                !name.starts_with('.') && name != "target" && name != "node_modules"
            })
            .collect();
        entries.sort_by(|a, b| {
            let a_is_dir = a.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let b_is_dir = b.file_type().map(|t| t.is_dir()).unwrap_or(false);
            b_is_dir.cmp(&a_is_dir).then_with(|| {
                a.file_name()
                    .to_string_lossy()
                    .cmp(&b.file_name().to_string_lossy())
            })
        });
        let mut nodes = Vec::new();
        for entry in entries {
            let name = entry.file_name().to_string_lossy().to_string();
            let entry_path = entry.path().to_string_lossy().to_string();
            let metadata = entry
                .metadata()
                .map_err(|e| format!("Metadata error: {e}"))?;
            if metadata.is_dir() {
                let children = build_tree(&entry.path())?;
                nodes.push(FileTreeNode {
                    name,
                    path: entry_path,
                    node_type: "directory".into(),
                    children,
                    modified: None,
                    size: None,
                });
            } else {
                nodes.push(FileTreeNode {
                    name,
                    path: entry_path,
                    node_type: "file".into(),
                    children: Vec::new(),
                    modified: None,
                    size: Some(metadata.len()),
                });
            }
        }
        Ok(nodes)
    }
    build_tree(root)
}

/// Get working directory info including git branch and modified files.
#[tauri::command]
pub async fn get_working_dir_info() -> Result<WorkingDirInfo, String> {
    use std::process::Command;
    let cwd = std::env::current_dir().map_err(|e| format!("Cannot determine CWD: {e}"))?;
    let root = cwd.to_string_lossy().to_string();
    let branch = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(&cwd)
        .output()
        .ok()
        .and_then(|o| if o.status.success() { Some(o) } else { None })
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".into());
    let modified: Vec<String> = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&cwd)
        .output()
        .ok()
        .and_then(|o| if o.status.success() { Some(o) } else { None })
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(|line| line.get(3..).map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let has_conflicts = Command::new("git")
        .args(["diff", "--name-only", "--diff-filter=U"])
        .current_dir(&cwd)
        .output()
        .ok()
        .and_then(|o| if o.status.success() { Some(o) } else { None })
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);
    let status = if has_conflicts {
        "merge-conflict".into()
    } else if !modified.is_empty() {
        "dirty".into()
    } else {
        "clean".into()
    };
    Ok(WorkingDirInfo {
        root,
        branch,
        modified_files: modified,
        status,
    })
}

// ─── Onboarding seed (#75) ──────────────────────────────────────────────────
//
// First-run sample data so the Tasks / Today surfaces aren't empty. Idempotent:
// if `.claude/tasks/` already holds any `.json` file, seed is a no-op. Sample
// tasks are intentionally generic ("Read the README", "Sketch a quick design")
// so they make sense in any working directory.

/// Report returned by `seed_sample_data` so the UI can tell the user what landed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedReport {
    /// Number of sample task files written. Zero when tasks already existed.
    pub tasks_seeded: usize,
}

/// Three onboarding tasks. IDs are stable so re-seeding is a no-op.
const SAMPLE_TASKS: &[(&str, &str, &str, &str, &[&str])] = &[
    (
        "sample-welcome-1",
        "Read the project README",
        "Open README.md and skim the architecture overview to get oriented.",
        "todo",
        &["getting-started"],
    ),
    (
        "sample-welcome-2",
        "Sketch a quick design",
        "Capture your initial idea as a 1-page note — what problem, what user, what shape.",
        "todo",
        &["design", "draft"],
    ),
    (
        "sample-welcome-3",
        "Run the test suite",
        "Execute `cargo test --workspace` (or the project's documented command) to confirm a clean baseline.",
        "in-progress",
        &["validation"],
    ),
];

/// Write sample tasks to `.claude/tasks/` on first run.
///
/// No-op when the directory already contains any `*.json` file (idempotent).
/// Creates the directory if missing. Returns the count of tasks written.
#[tauri::command]
pub async fn seed_sample_data() -> Result<SeedReport, String> {
    seed_sample_data_in(std::path::Path::new(".claude/tasks")).await
}

/// Path-parameterised core. The Tauri command above hard-codes `.claude/tasks`
/// (the location `list_tasks` reads from); tests call this with a tempdir so
/// they don't race on the process working directory.
async fn seed_sample_data_in(tasks_dir: &std::path::Path) -> Result<SeedReport, String> {
    std::fs::create_dir_all(tasks_dir).map_err(|e| format!("create tasks dir: {e}"))?;

    // Idempotent guard — if anything is already there, don't seed.
    let has_existing = std::fs::read_dir(tasks_dir)
        .map_err(|e| format!("read tasks dir: {e}"))?
        .filter_map(Result::ok)
        .any(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"));
    if has_existing {
        return Ok(SeedReport { tasks_seeded: 0 });
    }

    let now = chrono_timestamp();
    let due_in_24h = now + 24 * 60 * 60;

    for (id, title, description, status, tags) in SAMPLE_TASKS.iter().copied() {
        let body = serde_json::json!({
            "id": id,
            "title": title,
            "description": description,
            "status": status,
            "priority": "medium",
            "tags": tags,
            "dueDate": due_in_24h,
            "createdAt": now,
            "activeForm": match status {
                "in-progress" => Some("Working on sample task".to_string()),
                _ => None,
            },
        });
        let path = tasks_dir.join(format!("{id}.json"));
        let pretty = serde_json::to_string_pretty(&body)
            .map_err(|e| format!("serialize sample task {id}: {e}"))?;
        std::fs::write(&path, pretty)
            .map_err(|e| format!("write sample task {}: {e}", path.display()))?;
    }

    Ok(SeedReport {
        tasks_seeded: SAMPLE_TASKS.len(),
    })
}

/// Payload for `send_notification`.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct NotificationPayload {
    pub title: String,
    pub body: String,
    /// Reserved for future use. Current tauri-plugin-notification v2 API
    /// surface on the desktop's pinned shannon-core rev does not expose
    /// per-level icon mapping; level is currently informational only.
    #[serde(default)]
    pub level: Option<String>,
}

/// Fire a native OS notification via `tauri-plugin-notification`.
///
/// The desktop's pinned `shannon-core` rev (`a19a15d` = v0.5.2) exposes the
/// P1 notifications orchestrator. The frontend "Test notification" button
/// invokes this command directly; query-event firing sites
/// (`fire_query_notification` below) go through the shared `Notifier` on
/// `AppState` so they benefit from cooldown + level filtering.
#[tauri::command]
pub async fn send_notification(
    app: tauri::AppHandle,
    payload: NotificationPayload,
) -> Result<(), String> {
    use tauri_plugin_notification::NotificationExt;

    app.notification()
        .builder()
        .title(payload.title)
        .body(payload.body)
        .show()
        .map_err(|e| format!("notification show failed: {e}"))
}

/// Query-event notification kinds used by `fire_query_notification`.
enum NotificationKind {
    Completed,
    Failed(String),
}

/// Fire a cooldown-aware notification for a query lifecycle event.
///
/// `Completed` uses `source = "query_complete"` with a 0ms window (always
/// fires — each query completion is worth surfacing). `Failed` uses
/// `source = "query_error"` with a 5000ms window to coalesce cascading
/// errors (e.g. repeated API timeouts within a retry storm).
///
/// Returns whether the notification was actually dispatched (`Ok(false)`
/// means suppressed by cooldown). Production callers discard the result.
fn fire_query_notification(
    notifier: &shannon_core::notifier::Notifier,
    kind: NotificationKind,
) -> Result<bool, shannon_core::notifier::NotifierError> {
    use chrono::Utc;
    use shannon_core::notifier::{Notification, NotificationLevel};

    let (title, body, level, source, window_ms) = match kind {
        NotificationKind::Completed => (
            "Shannon".to_string(),
            "Query complete".to_string(),
            NotificationLevel::Info,
            "query_complete".to_string(),
            0_u64,
        ),
        NotificationKind::Failed(err) => {
            let body = if err.chars().count() > 200 {
                let truncated: String = err.chars().take(197).collect();
                format!("{truncated}...")
            } else {
                err
            };
            (
                "Shannon — query failed".to_string(),
                body,
                NotificationLevel::Error,
                "query_error".to_string(),
                5_000_u64,
            )
        }
    };

    let notification = Notification {
        title,
        body,
        level,
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        source: Some(source),
        action_id: None,
    };

    notifier.notify_dedup(&notification, window_ms)
}

/// Best-effort load of `[notifications.webhook]` from `~/.shannon/config.toml`
/// and `.shannon.toml` (project-local). Returns `None` on any error — never
/// panics the app on config issues.
fn load_desktop_webhook_config() -> Option<shannon_core::notifier::WebhookConfig> {
    let cfg = shannon_core::unified_config::ConfigBuilder::new()
        .load_global_toml()
        .load_local_toml()
        .build();
    cfg.notifications.and_then(|n| n.webhook)
}

/// Serializable webhook config mirror — keeps the TS boundary clean and
/// avoids leaking `WebhookTemplate`'s serde-tagged enum directly.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WebhookConfigDto {
    pub url: String,
    pub template: String,
    pub secret: Option<String>,
    pub timeout_ms: u64,
    pub include_body: bool,
}

#[tauri::command]
pub async fn get_webhook_config() -> Result<Option<WebhookConfigDto>, String> {
    let cfg = load_desktop_webhook_config();
    Ok(cfg.map(|c| WebhookConfigDto {
        url: c.url,
        template: template_to_str(&c.template),
        secret: c.secret,
        timeout_ms: c.timeout_ms,
        include_body: c.include_body,
    }))
}

#[tauri::command]
pub async fn save_webhook_config(dto: WebhookConfigDto) -> Result<(), String> {
    save_webhook_config_to_disk(&dto)
}

#[tauri::command]
pub async fn clear_webhook_config() -> Result<(), String> {
    clear_webhook_config_on_disk()
}

fn template_to_str(t: &shannon_core::notifier::WebhookTemplate) -> String {
    use shannon_core::notifier::WebhookTemplate::*;
    match t {
        Slack => "slack".into(),
        Discord => "discord".into(),
        Feishu => "feishu".into(),
        Wechat => "wechat".into(),
        Teams => "teams".into(),
        Telegram => "telegram".into(),
        DingTalk => "dingtalk".into(),
        Raw => "raw".into(),
        Custom(s) => format!("custom:{s}"),
    }
}

fn template_from_str(s: &str) -> shannon_core::notifier::WebhookTemplate {
    use shannon_core::notifier::WebhookTemplate::*;
    match s {
        "slack" => Slack,
        "discord" => Discord,
        "feishu" => Feishu,
        "wechat" => Wechat,
        "teams" => Teams,
        "telegram" => Telegram,
        "dingtalk" => DingTalk,
        "raw" => Raw,
        other => {
            if let Some(custom) = other.strip_prefix("custom:") {
                Custom(custom.to_string())
            } else {
                Raw
            }
        }
    }
}

/// Resolve the config file path used for the webhook section. Prefers the
/// project-local `.shannon.toml` if it exists; otherwise uses the global
/// `~/.shannon/config.toml` (creating the parent directory if missing).
fn resolve_webhook_config_path() -> Result<std::path::PathBuf, String> {
    let local = std::path::Path::new(".shannon.toml");
    if local.exists() {
        return Ok(local.to_path_buf());
    }
    let home = dirs::home_dir().ok_or_else(|| "could not resolve $HOME".to_string())?;
    let dir = home.join(".shannon");
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    Ok(dir.join("config.toml"))
}

/// Read-modify-write the `[notifications.webhook]` table on disk. Preserves
/// all other top-level tables and keys. Comments and formatting are lost
/// (TOML round-trip via `toml::Value`), which is acceptable for a settings
/// UI write path.
fn save_webhook_config_to_disk(dto: &WebhookConfigDto) -> Result<(), String> {
    let path = resolve_webhook_config_path()?;
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut root: toml::Value =
        toml::from_str(&existing).unwrap_or(toml::Value::Table(toml::value::Table::new()));

    let table = root.as_table_mut().ok_or_else(|| {
        "config root is not a table — refusing to overwrite user config".to_string()
    })?;

    let template_val = match template_from_str(&dto.template) {
        shannon_core::notifier::WebhookTemplate::Custom(s) => toml::Value::Table({
            let mut t = toml::value::Table::new();
            t.insert("custom".into(), toml::Value::String(s));
            t
        }),
        t => toml::Value::String(template_to_str(&t)),
    };

    let mut wh = toml::value::Table::new();
    wh.insert("url".into(), toml::Value::String(dto.url.clone()));
    if let Some(s) = &dto.secret {
        wh.insert("secret".into(), toml::Value::String(s.clone()));
    }
    wh.insert("template".into(), template_val);
    wh.insert(
        "timeout_ms".into(),
        toml::Value::Integer(dto.timeout_ms as i64),
    );
    wh.insert(
        "include_body".into(),
        toml::Value::Boolean(dto.include_body),
    );

    let notifications = table
        .entry("notifications")
        .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
    let notif_table = notifications
        .as_table_mut()
        .ok_or_else(|| "notifications is not a table".to_string())?;
    notif_table.insert("webhook".into(), toml::Value::Table(wh));

    let serialized = toml::to_string_pretty(&root).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(&path, serialized).map_err(|e| format!("write {}: {e}", path.display()))?;
    tracing::info!(path = %path.display(), "webhook config saved");
    Ok(())
}

fn clear_webhook_config_on_disk() -> Result<(), String> {
    let path = resolve_webhook_config_path()?;
    if !path.exists() {
        return Ok(());
    }
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let Ok(mut root) = toml::from_str::<toml::Value>(&existing) else {
        return Ok(());
    };
    if let Some(table) = root.as_table_mut() {
        if let Some(notif) = table
            .get_mut("notifications")
            .and_then(|v| v.as_table_mut())
        {
            notif.remove("webhook");
        }
    }
    let serialized = toml::to_string_pretty(&root).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(&path, serialized).map_err(|e| format!("write {}: {e}", path.display()))?;
    tracing::info!(path = %path.display(), "webhook config cleared");
    Ok(())
}

// --- Inbound notifications (P5 Phase 1) --------------------------------------
//
// Stores bot tokens + allow-lists for Slack and Telegram so a future
// listener/poller can route inbound messages into Shannon. Phase 1 only
// persists configuration; the actual listener ships in a later phase.

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SlackInboundDto {
    pub bot_token: String,
    pub trigger_word: String,
    pub allowed_channels: Vec<String>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TelegramInboundDto {
    pub bot_token: String,
    pub trigger_word: String,
    pub allowed_chats: Vec<String>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct InboundConfigDto {
    pub slack: Option<SlackInboundDto>,
    pub telegram: Option<TelegramInboundDto>,
}

#[tauri::command]
pub async fn get_inbound_config() -> Result<InboundConfigDto, String> {
    let path = resolve_webhook_config_path()?;
    if !path.exists() {
        return Ok(InboundConfigDto::default());
    }
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let Ok(root) = toml::from_str::<toml::Value>(&existing) else {
        return Ok(InboundConfigDto::default());
    };
    let Some(notif) = root.get("notifications").and_then(|v| v.as_table()) else {
        return Ok(InboundConfigDto::default());
    };
    let Some(inbound) = notif.get("inbound").and_then(|v| v.as_table()) else {
        return Ok(InboundConfigDto::default());
    };
    let slack = inbound
        .get("slack")
        .and_then(|v| v.as_table())
        .map(|t| SlackInboundDto {
            bot_token: t
                .get("bot_token")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            trigger_word: t
                .get("trigger_word")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            allowed_channels: t
                .get("allowed_channels")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
        });
    let telegram = inbound
        .get("telegram")
        .and_then(|v| v.as_table())
        .map(|t| TelegramInboundDto {
            bot_token: t
                .get("bot_token")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            trigger_word: t
                .get("trigger_word")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            allowed_chats: t
                .get("allowed_chats")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
        });
    Ok(InboundConfigDto { slack, telegram })
}

#[tauri::command]
pub async fn save_inbound_config(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    dto: InboundConfigDto,
) -> Result<(), String> {
    let path = resolve_webhook_config_path()?;
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut root: toml::Value =
        toml::from_str(&existing).unwrap_or(toml::Value::Table(toml::value::Table::new()));
    let table = root
        .as_table_mut()
        .ok_or_else(|| "config root is not a table".to_string())?;
    let notifications = table
        .entry("notifications")
        .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
    let notif_table = notifications
        .as_table_mut()
        .ok_or_else(|| "notifications is not a table".to_string())?;
    let mut inbound = toml::value::Table::new();
    if let Some(s) = &dto.slack {
        let mut t = toml::value::Table::new();
        t.insert("bot_token".into(), toml::Value::String(s.bot_token.clone()));
        t.insert(
            "trigger_word".into(),
            toml::Value::String(s.trigger_word.clone()),
        );
        t.insert(
            "allowed_channels".into(),
            toml::Value::Array(
                s.allowed_channels
                    .iter()
                    .map(|c| toml::Value::String(c.clone()))
                    .collect(),
            ),
        );
        inbound.insert("slack".into(), toml::Value::Table(t));
    }
    if let Some(tg) = &dto.telegram {
        let mut t = toml::value::Table::new();
        t.insert(
            "bot_token".into(),
            toml::Value::String(tg.bot_token.clone()),
        );
        t.insert(
            "trigger_word".into(),
            toml::Value::String(tg.trigger_word.clone()),
        );
        t.insert(
            "allowed_chats".into(),
            toml::Value::Array(
                tg.allowed_chats
                    .iter()
                    .map(|c| toml::Value::String(c.clone()))
                    .collect(),
            ),
        );
        inbound.insert("telegram".into(), toml::Value::Table(t));
    }
    notif_table.insert("inbound".into(), toml::Value::Table(inbound));
    let serialized = toml::to_string_pretty(&root).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(&path, serialized).map_err(|e| format!("write {}: {e}", path.display()))?;
    tracing::info!(path = %path.display(), "inbound config saved");
    restart_inbound_listener(&state, &app_handle, &dto).await;
    Ok(())
}

#[tauri::command]
pub async fn clear_inbound_config(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let path = resolve_webhook_config_path()?;
    if !path.exists() {
        let mut listener = state.inbound_listener.lock().await;
        if let Some(h) = listener.as_mut() {
            h.stop().await;
        }
        *listener = None;
        return Ok(());
    }
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let Ok(mut root) = toml::from_str::<toml::Value>(&existing) else {
        return Ok(());
    };
    if let Some(table) = root.as_table_mut() {
        if let Some(notif) = table
            .get_mut("notifications")
            .and_then(|v| v.as_table_mut())
        {
            notif.remove("inbound");
        }
    }
    let serialized = toml::to_string_pretty(&root).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(&path, serialized).map_err(|e| format!("write {}: {e}", path.display()))?;
    tracing::info!(path = %path.display(), "inbound config cleared");
    let mut listener = state.inbound_listener.lock().await;
    if let Some(h) = listener.as_mut() {
        h.stop().await;
    }
    *listener = None;
    Ok(())
}

/// (Re)spawn inbound workers to match `dto`. Stops any existing workers
/// first so callers can mutate config and observe the listener reflect it.
async fn restart_inbound_listener(
    state: &AppState,
    app_handle: &tauri::AppHandle,
    dto: &InboundConfigDto,
) {
    let mut listener = state.inbound_listener.lock().await;
    if let Some(h) = listener.as_mut() {
        h.stop().await;
    }
    let slack = dto.slack.as_ref().map(|s| crate::inbound::SlackConfig {
        bot_token: s.bot_token.clone(),
        trigger_word: s.trigger_word.clone(),
        allowed_channels: s.allowed_channels.clone(),
    });
    let telegram = dto
        .telegram
        .as_ref()
        .map(|t| crate::inbound::TelegramConfig {
            bot_token: t.bot_token.clone(),
            trigger_word: t.trigger_word.clone(),
            allowed_chats: t.allowed_chats.clone(),
        });
    *listener = Some(crate::inbound::InboundListener::start(
        app_handle.clone(),
        slack,
        telegram,
    ));
}

#[tauri::command]
pub async fn get_inbound_listener_status(
    state: tauri::State<'_, AppState>,
) -> Result<crate::inbound::InboundListenerStatus, String> {
    let listener = state.inbound_listener.lock().await;
    Ok(listener.as_ref().map(|h| h.status()).unwrap_or_default())
}

#[tauri::command]
pub async fn stop_inbound_listener(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut listener = state.inbound_listener.lock().await;
    if let Some(h) = listener.as_mut() {
        h.stop().await;
    }
    *listener = None;
    Ok(())
}

/// Auto-start the listener from app setup if inbound config already exists.
/// Called from `main.rs::setup()` after `AppState` is constructed.
pub async fn bootstrap_inbound_listener(state: &AppState, app_handle: &tauri::AppHandle) {
    let dto = match get_inbound_config().await {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(error = %e, "inbound bootstrap: could not read config");
            return;
        }
    };
    if dto.slack.is_none() && dto.telegram.is_none() {
        return;
    }
    restart_inbound_listener(state, app_handle, &dto).await;
}

// --- Billing (P0-c) ---------------------------------------------------------
//
// The billing surface is intentionally a demo right now — the UI shows a
// "Demo mode" banner. These commands return deterministic sample data so the
// settings page can render end-to-end while the real billing backend is
// wired in a later phase. Shapes mirror `ui/src/types/index.ts`.

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BillingPlanDto {
    pub name: String,
    pub price: u32,
    pub token_limit: u64,
    pub features: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CostRecordDto {
    pub date: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BillingHistoryDto {
    pub id: String,
    pub date: String,
    pub description: String,
    pub amount: f64,
    pub status: String,
}

#[tauri::command]
pub async fn get_billing_plan() -> Result<BillingPlanDto, String> {
    Ok(BillingPlanDto {
        name: "Pro".into(),
        price: 24,
        token_limit: 2_000_000,
        features: vec![
            "Unlimited sessions".into(),
            "5 concurrent agents".into(),
            "Claude Sonnet + Opus access".into(),
            "MCP marketplace".into(),
            "Priority support".into(),
        ],
    })
}

#[tauri::command]
pub async fn get_cost_history(days: u32) -> Result<Vec<CostRecordDto>, String> {
    let count = days.clamp(1, 90) as usize;
    let mut out = Vec::with_capacity(count);
    for i in (0..count).rev() {
        let base = 8.0 + ((i as f64) / 2.0).sin() * 3.0;
        let cost = ((base * 100.0).round()) / 100.0;
        let date = iso_days_ago(i as i64);
        out.push(CostRecordDto {
            date,
            input_tokens: (cost * 25_000.0) as u64,
            output_tokens: (cost * 8_000.0) as u64,
            cost_usd: cost.max(2.0),
        });
    }
    Ok(out)
}

#[tauri::command]
pub async fn get_billing_history() -> Result<Vec<BillingHistoryDto>, String> {
    let months = ["June", "May", "April", "March", "February", "January"];
    let year = 2026;
    let mut out = Vec::with_capacity(months.len());
    for (i, m) in months.iter().enumerate() {
        let amount = if *m == "February" { 38.0 } else { 24.0 };
        out.push(BillingHistoryDto {
            id: format!("inv-{year}-{:02}", i + 1),
            date: iso_days_ago((i as i64) * 30),
            description: format!("Pro plan — {m} {year}"),
            amount,
            status: "paid".into(),
        });
    }
    Ok(out)
}

fn iso_days_ago(days: i64) -> String {
    use chrono::{DateTime, Days, Utc};
    let now: DateTime<Utc> = Utc::now();
    let target = now
        .checked_sub_days(Days::new(days.max(0) as u64))
        .unwrap_or(now);
    target.format("%Y-%m-%d").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands_sessions::branch_session_internal;
    use shannon_core::notifier::{Cooldown, LogNotifier, Notifier};

    #[test]
    fn test_app_state_new() {
        let state = AppState::new();
        let messages = state.messages.blocking_lock();
        assert!(messages.is_empty());
        assert!(!*state.querying.blocking_lock());
        assert_eq!(state.notifier.handler_count(), 0);
    }

    #[test]
    fn test_fire_query_notification_completed_always_fires() {
        let mut notifier = Notifier::new().with_cooldown(Cooldown::new());
        notifier.add_handler(Box::new(LogNotifier::new()));
        let first = fire_query_notification(&notifier, NotificationKind::Completed).unwrap();
        assert!(first);
        let second = fire_query_notification(&notifier, NotificationKind::Completed).unwrap();
        assert!(second, "completed has 0ms window — no cooldown");
    }

    #[test]
    fn test_fire_query_notification_failed_coalesces() {
        let mut notifier = Notifier::new().with_cooldown(Cooldown::new());
        notifier.add_handler(Box::new(LogNotifier::new()));
        let first =
            fire_query_notification(&notifier, NotificationKind::Failed("api timeout".into()))
                .unwrap();
        assert!(first);
        let second =
            fire_query_notification(&notifier, NotificationKind::Failed("api timeout 2".into()))
                .unwrap();
        assert!(
            !second,
            "second failure within 5s window should be coalesced"
        );
    }

    #[test]
    fn test_chat_message_serialization() {
        let msg = ChatMessage {
            role: "user".to_string(),
            content: "hello world".to_string(),
            timestamp: 1700000000,
            file_attachments: None,
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
                file_attachments: None,
            };
            assert_eq!(msg.role, *role);
        }
    }

    #[test]
    fn test_status_response_serialization() {
        let resp = StatusResponse {
            model: "claude-opus".to_string(),
            provider: "anthropic".to_string(),
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
    fn test_provider_switch_request_serialization() {
        let req = ProviderSwitchRequest {
            provider: "openai".to_string(),
            api_key: Some("sk-test".to_string()),
            base_url: None,
            model: "gpt-4.1".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: ProviderSwitchRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.provider, "openai");
        assert_eq!(deserialized.api_key, Some("sk-test".to_string()));
    }

    #[test]
    fn test_send_message_response_serialization() {
        let resp = SendMessageResponse {
            query_id: "abc-123".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: SendMessageResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.query_id, "abc-123");
    }

    #[test]
    fn test_chrono_timestamp_reasonable() {
        let ts = chrono_timestamp();
        // Milliseconds since epoch — bounds are 2024-01-01 and 2030-01-01 in ms.
        assert!(ts > 1704067200000, "timestamp should be after 2024-01-01");
        assert!(ts < 1893456000000, "timestamp should be before 2030-01-01");
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
                file_attachments: None,
            });
            msgs.push(ChatMessage {
                role: "assistant".to_string(),
                content: "hi".to_string(),
                timestamp: 101,
                file_attachments: None,
            });
        }
        let msgs = state.messages.lock().await;
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[1].content, "hi");
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
        assert_send_sync::<ProviderSwitchRequest>();
        assert_send_sync::<SendMessageResponse>();
        assert_send_sync::<FileDiff>();
    }

    #[test]
    fn test_file_diff_serialization() {
        let diff = FileDiff {
            old_content: "old text".to_string(),
            new_content: "new text".to_string(),
            file_name: "test.rs".to_string(),
            language: "rust".to_string(),
        };
        let json = serde_json::to_string(&diff).unwrap();
        let deserialized: FileDiff = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.old_content, "old text");
        assert_eq!(deserialized.new_content, "new text");
        assert_eq!(deserialized.file_name, "test.rs");
        assert_eq!(deserialized.language, "rust");
    }

    #[test]
    fn test_seed_sample_data_writes_three_tasks() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tasks_dir = tmp.path().join(".claude/tasks");

        let rt = tokio::runtime::Runtime::new().expect("rt");
        let report = rt.block_on(seed_sample_data_in(&tasks_dir)).expect("seed");

        assert_eq!(report.tasks_seeded, 3);

        let entries: Vec<_> = std::fs::read_dir(&tasks_dir)
            .expect("read dir")
            .filter_map(Result::ok)
            .collect();
        assert_eq!(entries.len(), 3, "exactly three sample tasks written");

        // Each file should parse as JSON and carry the expected id + status.
        let mut ids = Vec::new();
        for entry in &entries {
            let body: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(entry.path()).expect("read"))
                    .expect("parse");
            ids.push(body["id"].as_str().unwrap_or("").to_string());
            assert!(
                body["title"].as_str().is_some(),
                "title field present on {:?}",
                entry.path()
            );
        }
        ids.sort();
        assert_eq!(
            ids,
            vec![
                "sample-welcome-1".to_string(),
                "sample-welcome-2".to_string(),
                "sample-welcome-3".to_string(),
            ]
        );
    }

    #[test]
    fn test_notification_payload_deserializes_with_optional_level() {
        let json = serde_json::json!({
            "title": "Hello",
            "body": "World",
        });
        let p: NotificationPayload = serde_json::from_value(json).expect("parse");
        assert_eq!(p.title, "Hello");
        assert_eq!(p.body, "World");
        assert!(p.level.is_none());
    }

    #[test]
    fn test_notification_payload_deserializes_with_level() {
        let json = serde_json::json!({
            "title": "Boom",
            "body": "broken",
            "level": "error",
        });
        let p: NotificationPayload = serde_json::from_value(json).expect("parse");
        assert_eq!(p.level.as_deref(), Some("error"));
    }

    #[test]
    fn test_seed_sample_data_is_idempotent() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tasks_dir = tmp.path().join(".claude/tasks");

        let rt = tokio::runtime::Runtime::new().expect("rt");
        let first = rt
            .block_on(seed_sample_data_in(&tasks_dir))
            .expect("seed 1");
        assert_eq!(first.tasks_seeded, 3);

        // Second call should be a no-op — dir already has json files.
        let second = rt
            .block_on(seed_sample_data_in(&tasks_dir))
            .expect("seed 2");
        assert_eq!(second.tasks_seeded, 0);

        let count = std::fs::read_dir(&tasks_dir)
            .expect("read dir")
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
            .count();
        assert_eq!(count, 3, "no duplicate files after re-seed");
    }

    // P6: Branch session tests
    #[tokio::test]
    async fn test_branch_session_creates_correct_metadata() {
        let state = AppState::new();

        // Create parent session with 4 messages using string roles
        let parent_id = uuid::Uuid::new_v4();
        let parent_id_str = parent_id.to_string();
        let messages = vec![
            shannon_core::api::Message {
                role: "user".into(),
                content: shannon_core::api::MessageContent::Text("msg 1".into()),
            },
            shannon_core::api::Message {
                role: "assistant".into(),
                content: shannon_core::api::MessageContent::Text("resp 1".into()),
            },
            shannon_core::api::Message {
                role: "user".into(),
                content: shannon_core::api::MessageContent::Text("msg 2".into()),
            },
            shannon_core::api::Message {
                role: "assistant".into(),
                content: shannon_core::api::MessageContent::Text("resp 2".into()),
            },
        ];

        let parent_metadata = shannon_core::state::SessionPersistMetadata {
            model: "claude-3".into(),
            turn_count: 2,
            title: Some("Parent Session".into()),
            ..Default::default()
        };

        state
            .state_manager
            .save_session(&parent_id, &messages, &parent_metadata)
            .expect("save parent");

        // Add parent to sessions list
        let parent_meta = SessionMeta {
            id: parent_id_str.clone(),
            title: "Parent Session".into(),
            created_at: 1700000000,
            message_count: 4,
            working_dir: None,
            parent_id: None,
            branch_point: None,
        };
        state.sessions.lock().await.push(parent_meta);

        // Branch at message index 1 (should include first 2 messages)
        let branch_result = branch_session_internal(&state, None, parent_id_str.clone(), 1)
            .await
            .expect("branch_session_internal");

        // Verify branch session metadata
        assert_eq!(
            branch_result.message_count, 2,
            "branch has only first 2 messages"
        );
        assert_eq!(branch_result.parent_id, Some(parent_id_str.clone()));
        assert_eq!(branch_result.branch_point, Some(1));
        assert!(branch_result.title.contains("Branch of"));

        // Verify branch session data
        let branch_uuid = uuid::Uuid::parse_str(&branch_result.id).expect("parse uuid");
        let branch_data = state
            .state_manager
            .load_session(&branch_uuid)
            .expect("load branch")
            .expect("branch data exists");

        assert_eq!(branch_data.messages.len(), 2, "branch has 2 messages");
    }

    #[tokio::test]
    async fn test_branch_session_preserves_parent_fields() {
        let state = AppState::new();

        // Create parent session with working dir
        let parent_id = uuid::Uuid::new_v4();
        let parent_id_str = parent_id.to_string();
        let messages = vec![shannon_core::api::Message {
            role: "user".into(),
            content: shannon_core::api::MessageContent::Text("single message".into()),
        }];

        let parent_metadata = shannon_core::state::SessionPersistMetadata {
            model: "claude-3".into(),
            turn_count: 0,
            title: Some("Parent".into()),
            ..Default::default()
        };

        state
            .state_manager
            .save_session(&parent_id, &messages, &parent_metadata)
            .expect("save parent");

        let parent_meta = SessionMeta {
            id: parent_id_str.clone(),
            title: "Parent".into(),
            created_at: 1700000000,
            message_count: 1,
            working_dir: Some("/home/user/project".into()),
            parent_id: None,
            branch_point: None,
        };
        state.sessions.lock().await.push(parent_meta);

        // Branch at message index 0
        let branch_result = branch_session_internal(&state, None, parent_id_str.clone(), 0)
            .await
            .expect("branch_session_internal");

        // Verify working_dir is inherited
        assert_eq!(branch_result.working_dir, Some("/home/user/project".into()));
        assert_eq!(branch_result.parent_id, Some(parent_id_str));
    }

    #[tokio::test]
    async fn test_branch_session_rejects_out_of_bounds_branch_point() {
        let state = AppState::new();

        let parent_id = uuid::Uuid::new_v4();
        let parent_id_str = parent_id.to_string();
        let messages = vec![shannon_core::api::Message {
            role: "user".into(),
            content: shannon_core::api::MessageContent::Text("only message".into()),
        }];

        state
            .state_manager
            .save_session(
                &parent_id,
                &messages,
                &shannon_core::state::SessionPersistMetadata {
                    model: "claude-3".into(),
                    turn_count: 0,
                    title: Some("Parent".into()),
                    ..Default::default()
                },
            )
            .expect("save parent");

        state.sessions.lock().await.push(SessionMeta {
            id: parent_id_str.clone(),
            title: "Parent".into(),
            created_at: 1700000000,
            message_count: 1,
            working_dir: None,
            parent_id: None,
            branch_point: None,
        });

        // branch_point == len() should fail
        let err = branch_session_internal(&state, None, parent_id_str.clone(), 1)
            .await
            .expect_err("branch at len() should fail");
        assert!(err.contains("out of bounds"));

        // branch_point >> len() should also fail
        let err = branch_session_internal(&state, None, parent_id_str, usize::MAX)
            .await
            .expect_err("branch at usize::MAX should fail");
        assert!(err.contains("out of bounds"));
    }

    // Integration test: verify the Tauri command wrapper delegates correctly.
    // The `#[tauri::command]` macro handles parameter deserialization and
    // invokes the internal function, so we test that delegation path.
    #[tokio::test]
    async fn test_branch_session_command_rejects_unknown_parent_id() {
        let state = AppState::new();

        // Try to branch from a session that doesn't exist
        let unknown_id = uuid::Uuid::new_v4().to_string();

        let err = branch_session_internal(&state, None, unknown_id, 0)
            .await
            .expect_err("branch from unknown parent should fail");
        assert!(err.contains("not found") || err.contains("unknown"));
    }

    #[tokio::test]
    async fn test_branch_session_command_zero_branch_point() {
        let state = AppState::new();

        let parent_id = uuid::Uuid::new_v4();
        let parent_id_str = parent_id.to_string();
        let messages = vec![shannon_core::api::Message {
            role: "user".into(),
            content: shannon_core::api::MessageContent::Text("message".into()),
        }];

        let metadata = shannon_core::state::SessionPersistMetadata {
            model: "claude-3".into(),
            turn_count: 0,
            title: Some("Parent".into()),
            ..Default::default()
        };

        state
            .state_manager
            .save_session(&parent_id, &messages, &metadata)
            .expect("save parent");

        state.sessions.lock().await.push(SessionMeta {
            id: parent_id_str.clone(),
            title: "Parent".into(),
            created_at: 1700000000,
            message_count: 1,
            working_dir: None,
            parent_id: None,
            branch_point: None,
        });

        // Branch at index 0 should work (includes first message)
        let result = branch_session_internal(&state, None, parent_id_str, 0)
            .await
            .expect("branch at index 0 should succeed");

        assert_eq!(result.message_count, 1);
        assert_eq!(result.branch_point, Some(0));
    }
}

// --- Security hardening tests (audit issues #1, #2, #4, #10) ---

#[test]
fn resolve_path_in_working_dir_accepts_inside_relative() {
    let tmp = tempfile::tempdir().unwrap();
    let sub = tmp.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    let file = sub.join("a.rs");
    std::fs::write(&file, "x").unwrap();

    let resolved = crate::resolve_path_in_working_dir("sub/a.rs", tmp.path())
        .expect("relative path inside working dir should resolve");
    assert_eq!(resolved, file.canonicalize().unwrap());
}

#[test]
fn resolve_path_in_working_dir_accepts_inside_absolute() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("a.rs");
    std::fs::write(&file, "x").unwrap();

    let resolved = crate::resolve_path_in_working_dir(&file.to_string_lossy(), tmp.path())
        .expect("absolute path inside working dir should resolve");
    assert_eq!(resolved, file.canonicalize().unwrap());
}

#[test]
fn resolve_path_in_working_dir_rejects_dotdot_traversal() {
    let tmp = tempfile::tempdir().unwrap();
    // Create a sibling directory *outside* the working dir (same parent).
    let parent = tmp.path().parent().unwrap().to_path_buf();
    let sibling = parent.join("shannon_test_sibling_target");
    let _ = std::fs::create_dir(&sibling);
    let target = sibling.join("secret.txt");
    std::fs::write(&target, "secret").ok();

    // `../shannon_test_sibling_target/secret.txt` escapes the working dir.
    let rel = "../shannon_test_sibling_target/secret.txt";
    let err = crate::resolve_path_in_working_dir(rel, tmp.path())
        .expect_err("path traversal via .. must be rejected");
    assert!(
        err.contains("outside"),
        "expected 'outside' in error, got: {err}"
    );

    // Cleanup the sibling we created outside the tempdir.
    let _ = std::fs::remove_dir_all(&sibling);
}

#[test]
fn resolve_path_in_working_dir_rejects_absolute_outside_path() {
    // Security #10 regression: `apply_diff`'s old `contains("..")` check
    // let `/etc/hosts` through. The new helper must reject it.
    let tmp = tempfile::tempdir().unwrap();
    let err = crate::resolve_path_in_working_dir("/etc/hosts", tmp.path())
        .expect_err("absolute path outside working dir must be rejected");
    // On Linux /etc/hosts exists, so we expect the "outside" error. On
    // other platforms it may be "not found" — either is a valid rejection.
    assert!(
        err.contains("outside") || err.contains("not found"),
        "unexpected error: {err}"
    );
}

#[test]
fn resolve_path_in_working_dir_rejects_missing_path() {
    let tmp = tempfile::tempdir().unwrap();
    let err = crate::resolve_path_in_working_dir("does_not_exist.rs", tmp.path())
        .expect_err("missing path should fail canonicalize");
    assert!(err.contains("not found"));
}

// ── Top-level unit tests for high-value pure functions ───────────────
// These complement `mod tests` above. Kept at module scope so they can
// invoke private helpers directly without going through `super::*`.

#[cfg(test)]
mod pure_function_tests {
    use super::*;

    // ── parse_approval_mode: covers all 11 variants + fallback ───────

    #[test]
    fn parse_approval_mode_maps_every_documented_alias() {
        use shannon_core::permissions::ApprovalMode;
        assert_eq!(parse_approval_mode("suggest"), ApprovalMode::Suggest);
        assert_eq!(parse_approval_mode("default"), ApprovalMode::Suggest);
        assert_eq!(parse_approval_mode("plan"), ApprovalMode::Plan);
        assert_eq!(parse_approval_mode("auto"), ApprovalMode::Auto);
        assert_eq!(parse_approval_mode("auto_edit"), ApprovalMode::AutoEdit);
        assert_eq!(parse_approval_mode("autoedit"), ApprovalMode::AutoEdit);
        assert_eq!(parse_approval_mode("full_auto"), ApprovalMode::FullAuto);
        assert_eq!(parse_approval_mode("fullauto"), ApprovalMode::FullAuto);
        assert_eq!(parse_approval_mode("readonly"), ApprovalMode::Readonly);
        assert_eq!(parse_approval_mode("read-only"), ApprovalMode::Readonly);
        assert_eq!(parse_approval_mode("plan_ro"), ApprovalMode::PlanReadonly);
        assert_eq!(parse_approval_mode("plan-ro"), ApprovalMode::PlanReadonly);
        assert_eq!(
            parse_approval_mode("planreadonly"),
            ApprovalMode::PlanReadonly
        );
        assert_eq!(
            parse_approval_mode("bypass_permissions"),
            ApprovalMode::BypassPermissions
        );
        assert_eq!(
            parse_approval_mode("bypasspermissions"),
            ApprovalMode::BypassPermissions
        );
        assert_eq!(parse_approval_mode("dont_ask"), ApprovalMode::DontAsk);
        assert_eq!(parse_approval_mode("dontask"), ApprovalMode::DontAsk);
        assert_eq!(parse_approval_mode("confirm"), ApprovalMode::Suggest);
    }

    #[test]
    fn parse_approval_mode_is_case_insensitive() {
        use shannon_core::permissions::ApprovalMode;
        assert_eq!(parse_approval_mode("SUGGEST"), ApprovalMode::Suggest);
        assert_eq!(parse_approval_mode("Plan"), ApprovalMode::Plan);
        assert_eq!(parse_approval_mode("FULL_AUTO"), ApprovalMode::FullAuto);
    }

    #[test]
    fn parse_approval_mode_unknown_falls_back_to_suggest() {
        use shannon_core::permissions::ApprovalMode;
        assert_eq!(parse_approval_mode(""), ApprovalMode::Suggest);
        assert_eq!(parse_approval_mode("yolo"), ApprovalMode::Suggest);
        assert_eq!(parse_approval_mode("sudo"), ApprovalMode::Suggest);
    }

    // ── detect_media_type ─────────────────────────────────────────────

    #[test]
    fn detect_media_type_returns_image_mimes() {
        assert_eq!(detect_media_type("logo.png").as_deref(), Some("image/png"));
        assert_eq!(
            detect_media_type("photo.jpg").as_deref(),
            Some("image/jpeg")
        );
        assert_eq!(
            detect_media_type("photo.jpeg").as_deref(),
            Some("image/jpeg")
        );
        assert_eq!(detect_media_type("anim.gif").as_deref(), Some("image/gif"));
        assert_eq!(
            detect_media_type("shot.webp").as_deref(),
            Some("image/webp")
        );
        assert_eq!(
            detect_media_type("icon.svg").as_deref(),
            Some("image/svg+xml")
        );
    }

    #[test]
    fn detect_media_type_is_case_insensitive_on_extension() {
        assert_eq!(detect_media_type("PHOTO.PNG").as_deref(), Some("image/png"));
        assert_eq!(
            detect_media_type("Photo.JPG").as_deref(),
            Some("image/jpeg")
        );
    }

    #[test]
    fn detect_media_type_returns_none_for_non_image_or_missing_ext() {
        assert!(detect_media_type("doc.pdf").is_none());
        assert!(detect_media_type("video.mp4").is_none());
        assert!(detect_media_type("noext").is_none());
        assert!(detect_media_type("").is_none());
    }

    // ── provider_from_str ─────────────────────────────────────────────

    #[test]
    fn provider_from_str_maps_known_providers() {
        use shannon_core::api::types::LlmProvider;
        assert_eq!(provider_from_str("anthropic"), LlmProvider::Anthropic);
        assert_eq!(provider_from_str("openai"), LlmProvider::OpenAI);
        assert_eq!(provider_from_str("ollama"), LlmProvider::Ollama);
        assert_eq!(provider_from_str("deepseek"), LlmProvider::DeepSeek);
        assert_eq!(provider_from_str("gemini"), LlmProvider::Gemini);
        assert_eq!(provider_from_str("mistral"), LlmProvider::Mistral);
        assert_eq!(provider_from_str("groq"), LlmProvider::Groq);
        assert_eq!(provider_from_str("openrouter"), LlmProvider::OpenRouter);
        assert_eq!(provider_from_str("xai"), LlmProvider::Xai);
    }

    // ── iso_days_ago ──────────────────────────────────────────────────

    #[test]
    fn iso_days_ago_returns_iso_date_string() {
        let s = iso_days_ago(7);
        assert!(is_iso_date(&s), "expected ISO date, got {s}");
    }

    #[test]
    fn iso_days_ago_zero_returns_today() {
        let now = chrono::Utc::now().format("%Y-%m-%d").to_string();
        assert_eq!(iso_days_ago(0), now);
    }

    #[test]
    fn iso_days_ago_negative_clamps_to_zero() {
        let now = chrono::Utc::now().format("%Y-%m-%d").to_string();
        assert_eq!(iso_days_ago(-5), now);
    }

    // ── template_to_str / template_from_str roundtrip ─────────────────

    #[test]
    fn webhook_template_str_roundtrip_preserves_known_variants() {
        use shannon_core::notifier::WebhookTemplate::*;
        for t in [
            Slack, Discord, Feishu, Wechat, Teams, Telegram, DingTalk, Raw,
        ] {
            let s = template_to_str(&t);
            let back = template_from_str(&s);
            assert_eq!(template_to_str(&back), s, "roundtrip not stable for {s}");
        }
    }

    #[test]
    fn webhook_template_custom_roundtrip() {
        use shannon_core::notifier::WebhookTemplate;
        let original = WebhookTemplate::Custom("X".repeat(120));
        let s = template_to_str(&original);
        assert!(s.starts_with("custom:"));
        let back = template_from_str(&s);
        assert_eq!(template_to_str(&back), s);
    }

    #[test]
    fn webhook_template_from_str_unknown_falls_back_to_raw() {
        use shannon_core::notifier::WebhookTemplate;
        assert_eq!(template_from_str("unknown"), WebhookTemplate::Raw);
        assert_eq!(template_from_str(""), WebhookTemplate::Raw);
    }

    fn is_iso_date(s: &str) -> bool {
        let b = s.as_bytes();
        b.len() == 10
            && b[4] == b'-'
            && b[7] == b'-'
            && b[..4].iter().all(|c| c.is_ascii_digit())
            && b[5..7].iter().all(|c| c.is_ascii_digit())
            && b[8..10].iter().all(|c| c.is_ascii_digit())
    }
}
