//! Tauri IPC commands bridging the web UI to Shannon Core.
//!
//! Each command is exposed via `#[tauri::command]` and invoked from
//! JavaScript as `invoke("command_name", { args })`.

use serde::{Deserialize, Serialize};
use shannon_core::query_engine::{QueryContext, QueryEngine, QueryEvent};
use shannon_core::tools::ToolRegistry;
use shannon_engine::api::client::LlmClient;
use shannon_engine::api::types::LlmClientConfig;
use shannon_engine::permissions::{ApprovalMode, PermissionManager};
use shannon_engine::state::StateManager;
use shannon_mcp::McpProcessPool;
use shannon_skills::SkillRegistry;
use shannon_tools::register_default_tools;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::{Mutex, RwLock, oneshot};

use crate::commands_agents::resolve_working_dir;
#[cfg(test)]
use crate::commands_billing::iso_days_ago;
use crate::config::{self, DesktopConfig};
use crate::events::event_names;
use crate::events::{self};
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
    pub(crate) client_config: Arc<RwLock<LlmClientConfig>>,
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
    pub(crate) pending_permissions: Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>,
    /// Session metadata for session list.
    pub(crate) sessions: Arc<Mutex<Vec<SessionMeta>>>,
    /// Cancellation token for the current query.
    pub(crate) cancellation_token: Arc<Mutex<Option<CancellationToken>>>,
    /// Currently active session ID.
    pub(crate) current_session_id: Arc<Mutex<Option<String>>>,
    /// Background tasks.
    pub(crate) background_tasks: Arc<Mutex<Vec<BackgroundTaskMeta>>>,
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
    /// Usage ledger (`~/.shannon/usage.jsonl`) — append-only token/cache/cost.
    pub(crate) usage_store: Arc<crate::commands_usage::UsageStore>,
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
    pub(crate) gateway_supervisor:
        Arc<tokio::sync::Mutex<Option<crate::gateway_supervisor::GatewaySupervisor>>>,
    /// Result of the startup engine discovery probe (`engine_discovery`).
    /// `None` until `setup()` runs the probe; `Some(Hosted)` once the
    /// loopback server is spawned; `Some(External)` when another engine
    /// was already serving on 33420.
    pub engine_mode: Arc<std::sync::RwLock<Option<crate::engine_discovery::EngineMode>>>,
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
pub(crate) struct BackgroundTaskMeta {
    pub(crate) id: String,
    pub(crate) prompt: String,
    pub(crate) status: String, // "running", "completed", "failed"
    pub(crate) started_at: i64,
    pub(crate) completed_at: Option<i64>,
    pub(crate) output: String,
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

    let bytes = fs::read(path).map_err(|e| format!("Failed to read file: {e}"))?;
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

/// Response from send_message containing the query ID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageResponse {
    pub query_id: String,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
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
            usage_store: Arc::new(crate::commands_usage::UsageStore::new()),
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
            gateway_supervisor: Arc::new(tokio::sync::Mutex::new(None)),
            engine_mode: Arc::new(std::sync::RwLock::new(None)),
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

        if let Some(wh_cfg) = crate::commands_notifications::load_desktop_webhook_config() {
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

    pub(crate) fn build_client_config(cfg: &DesktopConfig) -> LlmClientConfig {
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
    let message_for_skill_loop = message.clone();
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
    let provider_arc = state.provider.clone();
    let usage_store_arc = state.usage_store.clone();
    let notifier_arc = state.notifier.clone();

    let return_qid = qid_str.clone();
    tokio::spawn(async move {
        let stream = engine.process_query(context, None).await;
        let mut final_content = String::new();

        let query_start = std::time::Instant::now();
        let mut tool_call_count: usize = 0;
        let mut tool_names_used: std::collections::HashSet<String> =
            std::collections::HashSet::new();

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
                        tool_call_count += 1;
                        tool_names_used.insert(tool_name.clone());
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
                        cache_creation_tokens,
                        cache_read_tokens,
                        ..
                    } => {
                        // Persist to the local usage ledger. Best-effort:
                        // a log write failure must never break the stream.
                        let model_now = model_arc.lock().await.clone();
                        let provider_now = provider_arc.lock().await.clone();
                        let _ = usage_store_arc.append(&crate::commands_usage::record_event(
                            &model_now,
                            &provider_now,
                            input_tokens,
                            output_tokens,
                            cache_creation_tokens,
                            cache_read_tokens,
                            cost_usd,
                        ));
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
                                    let core_msgs: Vec<shannon_engine::api::Message> = msgs
                                        .iter()
                                        .map(|m| shannon_engine::api::Message {
                                            role: m.role.clone(),
                                            content: shannon_engine::api::MessageContent::Text(
                                                m.content.clone(),
                                            ),
                                        })
                                        .collect();
                                    let meta = shannon_engine::state::SessionPersistMetadata {
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
                        crate::commands_notifications::fire_query_notification_logged(
                            &notifier_arc,
                            crate::commands_notifications::NotificationKind::Completed,
                            "query_completed",
                        );

                        // Skill loop evaluation hook (spawned, non-blocking)
                        let app_clone = app.clone();
                        let user_prompt = message_for_skill_loop.clone();
                        let elapsed_secs = query_start.elapsed().as_secs();
                        let task_tool_call_count = tool_call_count;
                        let task_tool_names_used = tool_names_used.clone();
                        tokio::spawn(async move {
                            use tauri::Manager;
                            let cfg = crate::config::load_config();
                            if cfg.skill_loop_enabled {
                                if cfg.skill_loop_enabled {
                                    let duration_met =
                                        elapsed_secs >= cfg.skill_loop_min_duration_secs;
                                    let tools_met =
                                        task_tool_call_count >= cfg.skill_loop_min_tool_calls;

                                    if duration_met || tools_met {
                                        use shannon_core::skill_loop::{
                                            TaskEvaluation, TaskOutcome,
                                        };

                                        let evaluation = TaskEvaluation {
                                            duration_secs: elapsed_secs,
                                            tool_call_count: task_tool_call_count,
                                            user_prompt,
                                            outcome: TaskOutcome::Success,
                                            tool_names_used: task_tool_names_used,
                                            started_at: None,
                                            completed_at: None,
                                        };

                                        let client_config = {
                                            let state_guard = app_clone.state::<AppState>();
                                            state_guard.client_config.read().await.clone()
                                        };
                                        let client = shannon_engine::api::client::LlmClient::new(
                                            client_config,
                                        );

                                        // Reduce the evaluation result to a Send-only bool
                                        // first: evaluate_task returns Result<_, Box<dyn
                                        // Error>> and Box<dyn Error> is !Send, so the whole
                                        // result must be dropped before the generate await
                                        // below (else the spawned future is !Send).
                                        let suggest = match shannon_core::skill_loop::evaluate_task(
                                            &client,
                                            evaluation.clone(),
                                        )
                                        .await
                                        {
                                            Ok(result) => result.suggest,
                                            Err(e) => {
                                                tracing::warn!(
                                                    error = %e,
                                                    "skill loop evaluate failed (non-blocking)"
                                                );
                                                false
                                            }
                                        };

                                        if suggest {
                                            // Generate a proposal draft so the user can
                                            // review it. Generation is automatic; only install
                                            // (approve) is manual. Non-blocking on failure — a
                                            // failed generation simply won't surface a proposal.
                                            match shannon_core::skill_loop::generate_skill_proposal(
                                                &client, evaluation,
                                            )
                                            .await
                                            {
                                                Ok(proposal) => {
                                                    match crate::commands_skill_loop::save_proposal_and_count(
                                                        &proposal,
                                                    ) {
                                                        Ok(count) => {
                                                            let _ = app_clone.emit(
                                                                crate::events::event_names::SKILL_PROPOSAL_AVAILABLE,
                                                                crate::commands_skill_loop::SkillProposalCountPayload {
                                                                    pending_count: count,
                                                                },
                                                            );
                                                        }
                                                        Err(e) => {
                                                            tracing::warn!(
                                                                error = %e,
                                                                "skill loop save proposal failed (non-blocking)"
                                                            );
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    tracing::warn!(
                                                        error = %e,
                                                        "skill loop generate proposal failed (non-blocking)"
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        });
                    }
                    QueryEvent::Failed { error, .. } => {
                        let _ = app.emit(
                            event_names::QUERY_FAILED,
                            events::QueryFailedPayload {
                                query_id: qid_str.clone(),
                                error: error.clone(),
                            },
                        );
                        crate::commands_notifications::fire_query_notification_logged(
                            &notifier_arc,
                            crate::commands_notifications::NotificationKind::Failed(error),
                            "query_failed",
                        );
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
                    crate::commands_notifications::fire_query_notification_logged(
                        &notifier_arc,
                        crate::commands_notifications::NotificationKind::Failed(e.to_string()),
                        "query_failed",
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

// Update configuration.
// Session lifecycle commands (new/list/search/load/export/switch/
// set_working_dir/delete/rename/duplicate/branch_session) extracted to
// `commands_sessions.rs`. Registered in main.rs as commands_sessions::*.

// save_text_file extracted to `commands_files.rs` (registered as
// commands_files::save_text_file in main.rs).
// request_permission + respond_permission extracted to `commands_permissions.rs`
// (registered as commands_permissions::* in main.rs).

// Skill loop commands extracted to `commands_skill_loop.rs` (registered as
// commands_skill_loop::* in main.rs).

pub(crate) fn chrono_timestamp() -> i64 {
    // Milliseconds since UNIX_EPOCH. All UI consumers construct
    // `new Date(ts)` which interprets the argument as milliseconds.
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn provider_from_str(s: &str) -> shannon_engine::api::types::LlmProvider {
    use shannon_engine::api::types::LlmProvider;
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
    let provider = state.provider.lock().await.clone();
    let usage_store = state.usage_store.clone();
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

        // Clone before `model` is moved into QueryMetadata so usage events in
        // the stream below can be attributed (mirrors send_message).
        let model_for_usage = model.clone();

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
                    QueryEvent::Usage {
                        input_tokens,
                        output_tokens,
                        cost_usd,
                        cache_creation_tokens,
                        cache_read_tokens,
                        ..
                    } => {
                        // Persist to the local usage ledger. Best-effort: a log
                        // write failure must never break the task. No QUERY_USAGE
                        // emit here — background tasks aren't tied to a visible
                        // chat, so a live-usage signal has no consumer and could
                        // surface as a phantom UI update.
                        let _ = usage_store.append(&crate::commands_usage::record_event(
                            &model_for_usage,
                            &provider,
                            input_tokens,
                            output_tokens,
                            cache_creation_tokens,
                            cache_read_tokens,
                            cost_usd,
                        ));
                    }
                    QueryEvent::Completed { .. } => break,
                    QueryEvent::Failed { error, .. } => {
                        final_output = format!("Task failed: {error}");
                        break;
                    }
                    _ => {}
                },
                Err(e) => {
                    final_output = format!("Task error: {e}");
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands_sessions::branch_session_internal;

    #[test]
    fn test_app_state_new() {
        let state = AppState::new();
        let messages = state.messages.blocking_lock();
        assert!(messages.is_empty());
        assert!(!*state.querying.blocking_lock());
        assert_eq!(state.notifier.handler_count(), 0);
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
        assert_send_sync::<SendMessageResponse>();
    }

    // P6: Branch session tests
    #[tokio::test]
    async fn test_branch_session_creates_correct_metadata() {
        let state = AppState::new();

        // Create parent session with 4 messages using string roles
        let parent_id = uuid::Uuid::new_v4();
        let parent_id_str = parent_id.to_string();
        let messages = vec![
            shannon_engine::api::Message {
                role: "user".into(),
                content: shannon_engine::api::MessageContent::Text("msg 1".into()),
            },
            shannon_engine::api::Message {
                role: "assistant".into(),
                content: shannon_engine::api::MessageContent::Text("resp 1".into()),
            },
            shannon_engine::api::Message {
                role: "user".into(),
                content: shannon_engine::api::MessageContent::Text("msg 2".into()),
            },
            shannon_engine::api::Message {
                role: "assistant".into(),
                content: shannon_engine::api::MessageContent::Text("resp 2".into()),
            },
        ];

        let parent_metadata = shannon_engine::state::SessionPersistMetadata {
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
        let messages = vec![shannon_engine::api::Message {
            role: "user".into(),
            content: shannon_engine::api::MessageContent::Text("single message".into()),
        }];

        let parent_metadata = shannon_engine::state::SessionPersistMetadata {
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
        let messages = vec![shannon_engine::api::Message {
            role: "user".into(),
            content: shannon_engine::api::MessageContent::Text("only message".into()),
        }];

        state
            .state_manager
            .save_session(
                &parent_id,
                &messages,
                &shannon_engine::state::SessionPersistMetadata {
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
        let messages = vec![shannon_engine::api::Message {
            role: "user".into(),
            content: shannon_engine::api::MessageContent::Text("message".into()),
        }];

        let metadata = shannon_engine::state::SessionPersistMetadata {
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
        use shannon_engine::permissions::ApprovalMode;
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
        use shannon_engine::permissions::ApprovalMode;
        assert_eq!(parse_approval_mode("SUGGEST"), ApprovalMode::Suggest);
        assert_eq!(parse_approval_mode("Plan"), ApprovalMode::Plan);
        assert_eq!(parse_approval_mode("FULL_AUTO"), ApprovalMode::FullAuto);
    }

    #[test]
    fn parse_approval_mode_unknown_falls_back_to_suggest() {
        use shannon_engine::permissions::ApprovalMode;
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
        use shannon_engine::api::types::LlmProvider;
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
