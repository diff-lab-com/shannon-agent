//! Tauri IPC commands bridging the web UI to Shannon Core.
//!
//! Each command is exposed via `#[tauri::command]` and invoked from
//! JavaScript as `invoke("command_name", { args })`.

use serde::{Deserialize, Serialize};
use shannon_core::query_engine::{
    PermissionRequest as EnginePermissionRequest, QueryContext, QueryEngine, QueryEvent,
};
use shannon_core::settings::SettingsManager;
use shannon_core::tools::ToolRegistry;
use shannon_engine::api::client::LlmClient;
use shannon_engine::api::types::LlmClientConfig;
use shannon_engine::permissions::{ApprovalMode, PermissionManager, PermissionRuleChecker};
use shannon_engine::state::StateManager;
use shannon_mcp::McpProcessPool;
use shannon_skills::SkillRegistry;
use shannon_tools::register_default_tools_with_providers;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{Emitter, Manager};
use tokio::sync::{Mutex, RwLock};

use crate::commands_agents::resolve_working_dir;
#[cfg(test)]
use crate::commands_billing::iso_days_ago;
use crate::commands_permissions::PendingPermission;
use crate::config::{self, DesktopConfig};
use crate::events::event_names;
use crate::events::{self};
use crate::session_registry::SessionRegistry;
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
    /// Per-session state registry. Holds the active session's messages /
    /// querying flag / cancellation token, plus the "focused" session
    /// pointer (P0-4 / `query-coordinator-concurrency`). Spike scope: most
    /// single-session command paths resolve the active session
    /// via `registry.get_or_create_active()`. P1-1 exception: `send_message`
    /// and `cancel_query` route explicitly via
    /// `registry.resolve_explicit_or_active` (multi-window), so they never
    /// read or move the pointer when a sessionId is supplied.
    pub(crate) registry: Arc<SessionRegistry>,
    /// LLM client config — used to build clients on demand. P1.2-B:
    /// this is the single source of truth for the active `model` /
    /// `provider`; the legacy `Arc<Mutex<String>>` mirrors were
    /// removed when the engine `ProviderConfigStore` took ownership.
    /// `pub` (not `pub(crate)`) so the `shannon-desktop` binary
    /// crate's `main.rs` can read it for the tray status label —
    /// `pub(crate)` only spans lib-internal code, not the bin.
    pub client_config: Arc<RwLock<LlmClientConfig>>,
    /// Engine-side `~/.shannon/providers.toml` write path. Held behind
    /// an in-process `Mutex` so the three desktop commands that touch
    /// it (`save_provider`, `set_active_provider`, `delete_provider`)
    /// can't clobber each other via the load-mutate-save race that
    /// `ProviderConfigStore::save` is otherwise vulnerable to. On
    /// startup the in-memory state is loaded from disk; subsequent
    /// edits round-trip through this single instance.
    pub(crate) provider_store:
        Arc<tokio::sync::Mutex<shannon_core::provider_config_store::ProviderConfigStore>>,
    /// Tool registry with default tools.
    pub(crate) tools: Arc<ToolRegistry>,
    /// Permission manager.
    // KEEP: AppState owns the PermissionManager so the desktop shell can
    // consult it before dispatching tool calls. The interactive prompt
    // pipeline is wired through send_message's own engine-scoped manager
    // (see commands_permissions::prompt_user); this shared instance remains
    // for future cross-session policy decisions.
    #[allow(dead_code)]
    permissions: Arc<RwLock<PermissionManager>>,
    /// Session state manager.
    pub(crate) state_manager: Arc<StateManager>,
    /// Query engine configuration.
    qe_config: Arc<RwLock<shannon_core::query_engine::QueryEngineConfig>>,
    /// Desktop config (persisted).
    pub(crate) desktop_config: Arc<RwLock<DesktopConfig>>,
    /// Pending permission requests (request_id -> sender + tool name, so
    /// "always allow" can persist a rule for the tool).
    pub(crate) pending_permissions: Arc<Mutex<HashMap<String, PendingPermission>>>,
    /// Session metadata for session list. (P0-4: kept on AppState for
    /// now; this is the *display* list (titles, message counts), not the
    /// per-session query state. Migrating this into the registry is
    /// deferred until the UI uses session keys end-to-end.)
    pub(crate) sessions: Arc<Mutex<Vec<SessionMeta>>>,
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
    /// Live desktop goal runners, keyed by session id (P0-2). One active
    /// runner per session; while one exists, manual sends to that session
    /// are rejected (see `send_message`) — goal and manual input are
    /// mutually exclusive.
    pub(crate) goal_runs: Arc<crate::goal_commands::GoalRunRegistry>,
    /// P1-2 — best-of-N batch runs (live handles + on-disk records + the
    /// branch-execution semaphore).
    pub(crate) batch_runs: Arc<crate::batch_commands::BatchRunRegistry>,
    /// Open session windows — label → session id (P1-1). Mirrored into
    /// `DesktopConfig.open_session_windows` for restart restore.
    pub(crate) session_windows: crate::session_window_commands::SessionWindowRegistry,
    /// P1-5 C-1 — dev-server preview lifecycle owner (single instance,
    /// process-group kill on stop/exit, ≤500-line log ring, capture source).
    /// Also backs the desktop-only `preview_screenshot` engine tool.
    pub(crate) preview: Arc<crate::preview_commands::PreviewManager>,
    /// P1-5 D — integrated terminal PTY sessions (≤4, process trees owned
    /// here and killed on app exit; output coalesced ≤16 ms per emit).
    pub(crate) terminals: Arc<crate::terminal_commands::TerminalManager>,
    /// SQLite inbox store (`~/.shannon/inbox.db`, P0-3). Lazily opened on
    /// first use so a failing on-disk open degrades to an in-memory store
    /// (with a warning) instead of poisoning every inbox command.
    pub(crate) inbox_store: std::sync::OnceLock<Arc<shannon_core::inbox_store::InboxStore>>,
    /// Usage ledger (`~/.shannon/usage.jsonl`) — append-only token/cache/cost.
    pub(crate) usage_store: Arc<crate::commands_usage::UsageStore>,
    /// Shared memory store (`~/.shannon/memories/`, P2-4b). One instance per
    /// process: every engine the desktop constructs attaches this handle
    /// (`.with_memory_arc`) so memory injection and auto-extraction converge;
    /// the Memory page commands operate on the same instance, so page edits
    /// reach the injection path without any reload dance.
    pub(crate) memory_store: crate::commands_memory::SharedMemoryStore,
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
    // `pub` (not `pub(crate)`) so the bin crate's main.rs setup can hand the
    // directory to the agent_message_watcher.
    pub agent_message_history: Arc<shannon_agents::message_history::MessageHistoryStore>,
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

/// Model info for the model selector. The optional fields are populated
/// when `list_models` is routed through the engine model registry (ADR-0005
/// Phase 2 / task 4); `null` means unknown / not in pricing SSOT.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub provider: String,
    /// Tokens. `0` means unknown — the UI should render "unknown" instead
    /// of fabricating a number (P0-2 honest cost/context).
    pub context_window: usize,
    /// Per-million-token input price (USD). `None` = unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_in: Option<f64>,
    /// Per-million-token output price (USD). `None` = unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_out: Option<f64>,
    /// Tier label (`fast` / `standard` / `pro`). Optional — not all
    /// catalog entries carry tier metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    /// Whether this entry comes from the dynamic models.dev overlay rather
    /// than the static catalog. Surfaces a freshness indicator in the UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic: Option<bool>,
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
    /// The L0 session store over this app's sessions directory (§4.6).
    ///
    /// Every session read/write outside the live query path projects from or
    /// curates the event log through here.
    pub(crate) fn l0_store(&self) -> shannon_core::session_log::SessionStore {
        shannon_core::session_log::SessionStore::new(
            self.state_manager.sessions_dir().to_path_buf(),
        )
    }

    /// Create a new AppState, initializing the LLM client from env/config.
    pub fn new() -> Self {
        let desktop_config = config::load_config();

        // P1.1 (ADR-0005): load the provider store first — the runtime
        // client config is built from its resolved active target
        // (`build_client_config` below), not from `DesktopConfig`'s
        // singular provider fields (those legacy mirrors are gone).
        let provider_store =
            shannon_core::provider_config_store::ProviderConfigStore::load_or_default();

        // Build the engine-side `ShannonConfig` carrying only the behavioural
        // overrides (`max_tokens`/`temperature`) from the desktop's legacy
        // `DesktopConfig`. Provider identity, base_url, model and credential
        // are sourced from `provider_store` via `build_client_config` below.
        let shannon_overrides = shannon_core::unified_config::ShannonConfig {
            max_tokens: desktop_config.max_tokens.map(|v| v as usize),
            temperature: desktop_config.temperature,
            ..Default::default()
        };
        let client_config =
            Self::build_client_config(&provider_store, &shannon_overrides).unwrap_or_default();

        // Initialize tool registry behind a DynamicWorld decorator so a
        // remote target can be attached later without a registry rebuild.
        let mut tool_registry = ToolRegistry::new();
        let assembly = shannon_remote::assembly::assemble_dynamic();
        // P1-3: the persisted `sandbox.mode` config (off|local|landlock)
        // decorates the execution worlds at assembly time — the same seam
        // the TUI's env flag feeds. Invalid/unavailable configs degrade
        // loudly here and run unrestricted (never silently fake-restrict).
        let sandboxed_providers = match crate::sandbox_assembly::effective_sandbox_providers(
            desktop_config
                .sandbox
                .as_ref()
                .and_then(|s| s.mode.as_deref()),
            desktop_config.working_dir.as_deref(),
            &assembly.providers,
        ) {
            Ok(providers) => providers,
            Err(e) => {
                tracing::error!("sandbox disabled, continuing unrestricted: {e}");
                None
            }
        };
        let _agent_context = {
            let _ = &assembly;
            register_default_tools_with_providers(
                &mut tool_registry,
                sandboxed_providers.as_ref().unwrap_or(&assembly.providers),
            )
            .expect("Failed to register default tools")
        };

        // P1-5 C-1 — dev-server preview manager + the desktop-only
        // `preview_screenshot` engine tool bound to it. Registration happens
        // here, NOT in `register_default_tools`, so CLI/headless surfaces
        // never see the tool (it is meaningless without the desktop panel).
        let preview = Arc::new(crate::preview_commands::PreviewManager::new());
        shannon_tools::preview::register_preview_screenshot_tool(
            &mut tool_registry,
            Arc::new(crate::preview_commands::ManagerPreviewAccess::new(
                preview.clone(),
            )),
        )
        .expect("Failed to register preview_screenshot tool");

        Self {
            registry: Arc::new(SessionRegistry::new()),
            client_config: Arc::new(RwLock::new(client_config)),
            provider_store: Arc::new(tokio::sync::Mutex::new(provider_store)),
            tools: Arc::new(tool_registry),
            permissions: Arc::new(RwLock::new(PermissionManager::new())),
            state_manager: Arc::new(StateManager::new()),
            qe_config: Arc::new(RwLock::new(
                shannon_core::query_engine::QueryEngineConfig::default(),
            )),
            desktop_config: Arc::new(RwLock::new(desktop_config)),
            pending_permissions: Arc::new(Mutex::new(HashMap::new())),
            sessions: Arc::new(Mutex::new(Vec::new())),
            background_tasks: Arc::new(Mutex::new(Vec::new())),
            skill_registry: Arc::new(SkillRegistry::new()),
            mcp_pool: Arc::new(McpProcessPool::new()),
            scheduled_task_store: Arc::new(
                shannon_core::scheduled_task_store::ScheduledTaskStore::new(),
            ),
            scheduled_runs_store: Arc::new(shannon_core::scheduled_runs::ScheduledRunsStore::new()),
            triage_store: Arc::new(crate::scheduled_commands::TriageStore::new()),
            goal_runs: Arc::new(crate::goal_commands::GoalRunRegistry::new()),
            batch_runs: Arc::new(crate::batch_commands::BatchRunRegistry::new()),
            session_windows: crate::session_window_commands::SessionWindowRegistry::default(),
            preview,
            terminals: Arc::new(crate::terminal_commands::TerminalManager::new()),
            inbox_store: std::sync::OnceLock::new(),
            usage_store: Arc::new(crate::commands_usage::UsageStore::new()),
            memory_store: crate::commands_memory::open_shared_store(),
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

    /// Build the runtime `LlmClientConfig` from the engine `ProviderConfigStore`
    /// (v2 active target) plus the engine-side `ShannonConfig` for
    /// behavioural overrides (`max_tokens`/`timeout`/`temperature`).
    ///
    /// Returns `None` when the store has no resolvable active target — the
    /// caller is expected to fall back to [`LlmClientConfig::default`] in that
    /// case (which reads `SHANNON_*` env vars).
    ///
    /// P1.1 (ADR-0005): the legacy path that read singular fields off
    /// `DesktopConfig` (`provider`/`api_key`/`base_url`/`model`) is removed.
    /// Provider identity, base_url, model and credential are now sourced
    /// from the `"default"` [`crate::provider_resolver::ResolvedTarget`] of
    /// `provider_store`. The legacy fields are scheduled for full removal
    /// in T2.
    pub(crate) fn build_client_config(
        provider_store: &shannon_core::provider_config_store::ProviderConfigStore,
        shannon_config: &shannon_core::unified_config::ShannonConfig,
    ) -> Option<LlmClientConfig> {
        use shannon_core::provider_resolver::resolve_active_target;
        use shannon_core::unified_config::build_client_from_resolved;

        let rt = resolve_active_target(provider_store.config())?;
        Some(build_client_from_resolved(shannon_config, rt))
    }
}

/// Send a user message and stream the AI response via Tauri events.
///
/// P0-4 spike scope: `messages`, `querying`, `cancellation_token` and the
/// session ID are now sourced from the active session in `state.registry`
/// instead of from `AppState` directly. The active session is materialised
/// lazily on first call. The hard-rejection ("A query is already in
/// progress") now fires per-session rather than globally.
///
/// P1-1 (multi-window routing fix): `session_id` — when provided, the send
/// is routed to **that** session (registered via `new_session` /
/// `switch_session`); an unknown id is a hard error and the shared
/// active-session pointer is **never touched**, so one window's send can no
/// longer silently land in another window's session. Without the parameter
/// the legacy active-session fallback applies (back-compat).
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn send_message(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    message: String,
    file_paths: Option<Vec<String>>,
    budget_bypass: Option<bool>,
    session_id: Option<String>,
) -> Result<SendMessageResponse, String> {
    // P1-1: explicit sessionId routes to that session without touching the
    // shared active pointer; no sessionId keeps the legacy active fallback
    // (materialises lazily on the "first call ever" case).
    let (_, active_session) = state
        .registry
        .resolve_explicit_or_active(session_id.as_deref())?;
    let session_id = active_session.session_id;

    // P0-2: a desktop goal run owns this session while active — a manual
    // send would interleave with the unattended turn loop. The composer
    // gates on the same condition via `get_goal_run`; this is the backend
    // backstop. (Defence in depth; not a drive-by change.)
    if state.goal_runs.blocks_session(&session_id) {
        return Err(
            "A goal run is active on this session — pause or stop it from the Tasks page before sending messages"
                .into(),
        );
    }

    // P0-4: session-budget pre-turn guard (logic in the generic helper so
    // it stays testable — see `enforce_pre_turn_budget`).
    let budget_cap_usd = crate::cost_commands::session_budget_usd(&state, session_id);
    enforce_pre_turn_budget(
        &state,
        &app_handle,
        session_id,
        budget_cap_usd,
        budget_bypass,
    )
    .await?;
    // Prevent concurrent queries — check and set in a single lock scope to avoid TOCTOU race
    {
        let mut querying = active_session.querying.lock().await;
        if *querying {
            return Err("A query is already in progress".into());
        }
        *querying = true;
    }

    // Create cancellation token
    let cancel_token = CancellationToken::new();
    {
        let mut token_guard = active_session.cancellation_token.lock().await;
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

    // Route image attachments into the multimodal query path so the model
    // actually sees them. The `FileAttachment`s stored on the ChatMessage
    // below are display-only (chat history / UI chips); only these content
    // blocks reach the LLM. SVG is excluded — vision providers accept
    // png/jpeg/gif/webp only.
    let image_blocks: Vec<shannon_engine::api::ContentBlock> = attachments
        .as_ref()
        .map(|list| {
            list.iter()
                .filter_map(|att| {
                    let b64 = att.base64_data.as_ref()?;
                    let media_type = att.media_type.as_deref()?;
                    if !matches!(
                        media_type,
                        "image/png" | "image/jpeg" | "image/gif" | "image/webp"
                    ) {
                        return None;
                    }
                    Some(shannon_engine::api::ContentBlock::Image {
                        source: shannon_engine::api::ImageSource::base64(
                            media_type.to_string(),
                            b64.clone(),
                        ),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    // Tier-1 auto-title: capture emptiness before the push — this message is
    // the session's first user message iff the buffer was empty.
    let first_user_message = {
        let mut messages = active_session.messages.lock().await;
        let first = messages.is_empty();
        messages.push(ChatMessage {
            role: "user".into(),
            content: message.clone(),
            timestamp: now,
            file_attachments: attachments,
        });
        first
    };
    // /rewind bookkeeping: 0-based index this turn will occupy (count of user
    // messages before this send, computed after the push above).
    let rewind_turn_index = {
        let messages = active_session.messages.lock().await;
        messages.iter().filter(|m| m.role == "user").count() - 1
    };

    // Promote the first user message to the session title while the title
    // is still the generated placeholder. User renames are never touched;
    // the UI refreshes its session rail off the emitted SESSIONS_UPDATED.
    if first_user_message {
        crate::commands_sessions::auto_title_from_first_message(
            &state,
            &app_handle,
            session_id,
            &message,
        )
        .await;
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

    // Create a new PermissionManager instance configured from shared state.
    // Persisted deny/ask/allow rules (~/.shannon/settings.json, including the
    // rules written by "Always allow" in the permission modal) feed the rule
    // checker so previously granted tools stop re-prompting.
    let mut permissions = PermissionManager::new();
    // P1-3: the active permission profile (strict/balanced/permissive or a
    // custom `.shannon/profiles/*.toml` name) contributes its rule
    // side-effects (deny list, active-profile record). The configured
    // `approval_mode` is applied AFTER so a manual mode edit stays
    // authoritative over the profile-derived mode.
    crate::automation_commands::apply_active_profile(
        &mut permissions,
        desktop_cfg.active_permission_profile.as_deref(),
    );
    permissions.set_approval_mode(approval_mode);
    let mut settings = SettingsManager::new();
    if let Err(e) = settings.load_from_files() {
        eprintln!("send_message: failed to load permission rules: {e}");
    } else {
        let rules = &settings.settings_mut().permissions;
        permissions.set_rule_checker(PermissionRuleChecker::from_rule_strings(
            &rules.deny,
            &rules.ask,
            &rules.allow,
        ));
    }

    // Interactive permission channel: the engine forwards PermissionPrompt
    // verdicts here; a forwarder task surfaces each as a Tauri
    // PERMISSION_REQUEST and maps the user's scoped answer back to a
    // PermissionChoice.
    let (perm_tx, mut perm_rx) = tokio::sync::mpsc::unbounded_channel::<EnginePermissionRequest>();

    let _state_mgr = state.state_manager.clone();
    let _qe_config = state.qe_config.read().await.clone();

    let mut engine = crate::commands_memory::attach_shared_memory(
        QueryEngine::with_defaults_arc(client, tools, permissions, StateManager::new()),
        &state.memory_store,
    );
    // Bind the engine to the REAL session and restore prior turns. Both the
    // L0 tee (events.jsonl path) and the conversation clone at the top of
    // process_query key off engine state — a fresh engine with a random id
    // logged every send under a throwaway session directory and sent the
    // model zero prior turns. Session history lives in the L0 log; restore
    // is cheap (one projection) and keeps the log as the single source.
    engine.set_session_id(session_id);
    match engine.restore_session(session_id) {
        Ok(true) => {}
        Ok(false) => {} // brand-new session — nothing on disk yet
        Err(e) => tracing::warn!("history restore failed for {session_id}: {e}"),
    }

    // P0-4: stash the engine on the session so subsequent queries on the
    // same session reuse the same `Arc<ToolRegistry>` / hook manager /
    // triggered-routine registry instead of paying re-init cost. We
    // `take()` first so the clone below doesn't double-init: if a second
    // `send_message` races in (the per-session `querying` guard above
    // already prevents that, but defence-in-depth), the second caller
    // still gets a freshly built engine from this same code path.
    let engine_for_session = engine.clone();
    {
        let mut slot = active_session.query_engine.lock().await;
        *slot = Some(engine_for_session);
    }

    // Create query context
    let model = state.client_config.read().await.model.clone();
    let message_for_skill_loop = message.clone();
    let context = QueryContext {
        query_id,
        session_id,
        user_message: message,
        attachments: image_blocks,
        metadata: shannon_core::query_engine::QueryMetadata {
            timestamp: chrono::Utc::now(),
            tools_allowed: true,
            max_tokens: None,
            model,
            temperature: None,
            top_p: None,
        },
    };

    // Spawn the query in a background task, streaming events to frontend.
    // P0-4: per-session flags live on the `Arc<SessionState>` clone.
    let app = app_handle.clone();
    let cancel_token_clone = cancel_token.clone();
    let client_config_arc = state.client_config.clone();
    let usage_store_arc = state.usage_store.clone();
    let notifier_arc = state.notifier.clone();
    let session_for_task = active_session.clone();
    // P1-1: owner session stamped onto every `query:*` payload so
    // multi-window shells can filter streams per window.
    let session_id_str = session_for_task.session_id.to_string();
    // P0-4 mid-turn budget guard basis: spend already on the ledger before
    // this turn started. The streaming Usage handler folds each event's
    // cost into the guard, which enforces the cap (>=100% cancel +
    // one-shot `budget:exceeded`, >=80% one-shot `budget:warning`).
    // `None` cap = no accounting.
    let mut budget_guard = budget_cap_usd.map(|cap| {
        crate::cost_commands::BudgetTurnGuard::new(
            cap,
            crate::cost_commands::session_spent_usd(&state, &session_id.to_string()),
        )
    });

    // P2-5b: per-session in-process fan-out. Every event the loop
    // emits to the Tauri wire is also pushed onto `session_for_task`'s
    // mpsc channel so a future in-process consumer (the thread
    // switcher being built in a follow-up iteration) can subscribe to
    // *this session's* stream without conflating it with siblings.
    // Best-effort — channel send errors are silently ignored (the
    // Tauri wire + `messages` buffer still cover the user-visible path).
    let session = session_for_task.clone();
    let session_for_inproc = session.clone();
    let route_event = move |evt: crate::session_registry::SessionEvent| {
        session_for_inproc.try_send_event(evt);
    };
    let return_qid = qid_str.clone();
    // Engine→UI permission bridge: each prompt from the query pipeline
    // becomes a pending Tauri permission; the scoped user decision maps back
    // onto the engine's choice enum (AlwaysAllow also lands in the engine's
    // in-session memory via process_permission_choice).
    let app_for_permissions = app_handle.clone();
    let session_id_for_permissions = session_id_str.clone();
    tokio::spawn(async move {
        use shannon_engine::permissions::PermissionChoice;
        // P1-3: engine DecisionReason → wire PermissionReason (frozen
        // camelCase shape) so the approval dialog can show why it fired.
        fn wire_reason(
            reason: &shannon_engine::permissions::DecisionReason,
        ) -> shannon_types::events::PermissionReason {
            use shannon_engine::permissions::ReasonSource;
            shannon_types::events::PermissionReason {
                source: match reason.source {
                    ReasonSource::Rule => "rule",
                    ReasonSource::Llm => "llm",
                    ReasonSource::Default => "default",
                }
                .to_string(),
                rule_name: reason.rule_name.clone(),
                confidence: reason.confidence.map(f64::from),
            }
        }
        while let Some(request) = perm_rx.recv().await {
            let prompt = &request.prompt;
            let risk = match prompt.risk_level {
                shannon_engine::permissions::RiskLevel::Safe
                | shannon_engine::permissions::RiskLevel::Low => "low",
                shannon_engine::permissions::RiskLevel::Medium => "medium",
                shannon_engine::permissions::RiskLevel::High
                | shannon_engine::permissions::RiskLevel::Critical => "high",
            };
            let decision = crate::commands_permissions::prompt_user(
                &app_for_permissions.state::<AppState>(),
                &app_for_permissions,
                prompt.tool_name.clone(),
                prompt.tool_input.clone(),
                risk.to_string(),
                300,
                Some(session_id_for_permissions.clone()),
                Some(wire_reason(&prompt.reason)),
            )
            .await;
            let choice = match decision {
                crate::commands_permissions::PermissionDecision::AllowOnce => {
                    PermissionChoice::AllowOnce
                }
                crate::commands_permissions::PermissionDecision::AlwaysAllow => {
                    PermissionChoice::AlwaysAllow
                }
                crate::commands_permissions::PermissionDecision::Deny => PermissionChoice::Deny,
            };
            let _ = request.response_tx.send(choice);
        }
    });
    tokio::spawn(async move {
        let stream = engine.process_query(context, Some(perm_tx)).await;
        let mut final_content = String::new();

        let query_start = std::time::Instant::now();
        let mut tool_call_count: usize = 0;
        let mut tool_names_used: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        // /rewind: file paths mutated by this turn's write/edit tool calls.
        let mut turn_files: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

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
                        session_id: Some(session_id_str.clone()),
                    },
                );
                route_event(crate::session_registry::SessionEvent::Status(
                    crate::session_registry::SessionEventStatus::Cancelled,
                ));
                break;
            }

            match event_result {
                Ok(event) => match event {
                    QueryEvent::Text { content, .. } => {
                        final_content.push_str(&content);
                        let payload = events::QueryTextPayload {
                            query_id: qid_str.clone(),
                            content,
                            session_id: Some(session_id_str.clone()),
                        };
                        route_event(crate::session_registry::SessionEvent::QueryText(
                            payload.clone(),
                        ));
                        let _ = app.emit(event_names::QUERY_TEXT, payload);
                    }
                    QueryEvent::ToolUseRequest {
                        tool_use_id,
                        tool_name,
                        tool_input,
                        ..
                    } => {
                        tool_call_count += 1;
                        tool_names_used.insert(tool_name.clone());
                        if let Some(path) =
                            crate::commands_rewind::mutated_file_path(&tool_name, &tool_input)
                        {
                            turn_files.insert(path);
                        }
                        let payload = events::ToolStartPayload {
                            query_id: qid_str.clone(),
                            tool_use_id,
                            tool_name,
                            tool_input,
                            session_id: Some(session_id_str.clone()),
                        };
                        route_event(crate::session_registry::SessionEvent::ToolStart(
                            payload.clone(),
                        ));
                        let _ = app.emit(event_names::QUERY_TOOL_START, payload);
                    }
                    QueryEvent::ToolUseResult {
                        tool_use_id,
                        tool_name,
                        result,
                        is_error,
                        ..
                    } => {
                        let payload = events::ToolResultPayload {
                            query_id: qid_str.clone(),
                            tool_use_id,
                            tool_name,
                            result,
                            is_error,
                            session_id: Some(session_id_str.clone()),
                        };
                        route_event(crate::session_registry::SessionEvent::ToolResult(
                            payload.clone(),
                        ));
                        let _ = app.emit(event_names::QUERY_TOOL_RESULT, payload);
                    }
                    QueryEvent::ToolProgress {
                        tool_use_id,
                        tool_name,
                        progress,
                        message: msg,
                        ..
                    } => {
                        let payload = events::ToolProgressPayload {
                            query_id: qid_str.clone(),
                            tool_use_id,
                            tool_name,
                            progress,
                            message: msg,
                            session_id: Some(session_id_str.clone()),
                        };
                        route_event(crate::session_registry::SessionEvent::ToolProgress(
                            payload.clone(),
                        ));
                        let _ = app.emit(event_names::QUERY_TOOL_PROGRESS, payload);
                    }
                    QueryEvent::Thinking { content, .. } => {
                        let payload = events::ThinkingPayload {
                            query_id: qid_str.clone(),
                            content,
                            session_id: Some(session_id_str.clone()),
                        };
                        route_event(crate::session_registry::SessionEvent::Thinking(
                            payload.clone(),
                        ));
                        let _ = app.emit(event_names::QUERY_THINKING, payload);
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
                        let cc_now = client_config_arc.read().await;
                        let model_now = cc_now.model.clone();
                        let provider_now = cc_now.provider.to_string();
                        drop(cc_now);
                        let _ = usage_store_arc.append(&crate::commands_usage::record_event(
                            &model_now,
                            &provider_now,
                            crate::commands_usage::UsageTotals {
                                input_tokens,
                                output_tokens,
                                cache_creation_tokens,
                                cache_read_tokens,
                                cost_usd,
                            },
                            Some(&session_id.to_string()),
                        ));
                        let payload = events::UsagePayload {
                            query_id: qid_str.clone(),
                            input_tokens,
                            output_tokens,
                            cost_usd,
                            session_id: Some(session_id_str.clone()),
                        };
                        route_event(crate::session_registry::SessionEvent::Usage(
                            payload.clone(),
                        ));
                        let _ = app.emit(event_names::QUERY_USAGE, payload);

                        // P0-4 mid-turn budget enforcement (logic in the
                        // generic helper — see `enforce_mid_turn_usage`):
                        // first cap crossing cancels via the SAME token
                        // `cancel_query` pulls (`query:cancelled` at the
                        // loop top), exceeded latched to one emit; the
                        // first 80% crossing warns once; a jump straight
                        // past 100% emits exceeded only.
                        enforce_mid_turn_usage(
                            &app,
                            &mut budget_guard,
                            &cancel_token_clone,
                            &session_id,
                            cost_usd,
                        );
                    }
                    QueryEvent::Completed { .. } => {
                        // Save final assistant message into the per-session buffer.
                        {
                            let mut messages = session_for_task.messages.lock().await;
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

                        // (§4.6) Conversation already durable in events.jsonl
                        // via the engine tee. /rewind still needs the per-turn
                        // checkpoint + content snapshots though — the L0 log
                        // is append-only, so rewind reverts via these.
                        {
                            let files: Vec<String> = turn_files.iter().cloned().collect();
                            let prompt = message_for_skill_loop.clone();
                            let working_dir = crate::commands_agents::resolve_working_dir(
                                &app.state::<AppState>(),
                            )
                            .await;
                            crate::commands_rewind::record_turn(
                                &session_id.to_string(),
                                rewind_turn_index,
                                &files,
                                &prompt,
                                &working_dir,
                            );
                        }

                        let _ = app.emit(
                            event_names::QUERY_COMPLETED,
                            events::QueryCompletedPayload {
                                query_id: qid_str.clone(),
                                session_id: Some(session_id_str.clone()),
                            },
                        );
                        route_event(crate::session_registry::SessionEvent::Status(
                            crate::session_registry::SessionEventStatus::Completed,
                        ));
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
                                session_id: Some(session_id_str.clone()),
                            },
                        );
                        route_event(crate::session_registry::SessionEvent::Status(
                            crate::session_registry::SessionEventStatus::Failed(error.clone()),
                        ));
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
                    let err_string = e.to_string();
                    let _ = app.emit(
                        event_names::QUERY_FAILED,
                        events::QueryFailedPayload {
                            query_id: qid_str.clone(),
                            error: err_string.clone(),
                            session_id: Some(session_id_str.clone()),
                        },
                    );
                    route_event(crate::session_registry::SessionEvent::Status(
                        crate::session_registry::SessionEventStatus::Failed(err_string.clone()),
                    ));
                    crate::commands_notifications::fire_query_notification_logged(
                        &notifier_arc,
                        crate::commands_notifications::NotificationKind::Failed(err_string),
                        "query_failed",
                    );
                }
            }
        }

        // Clear per-session querying flag and cancellation token.
        {
            let mut q = session_for_task.querying.lock().await;
            *q = false;
        }
        {
            let mut token_guard = session_for_task.cancellation_token.lock().await;
            *token_guard = None;
        }
    });

    Ok(SendMessageResponse {
        query_id: return_qid,
    })
}

// ── P0-4: send_message budget enforcement (injectable boundary) ──────────
//
// The `#[tauri::command]` `send_message` is `AppHandle<Wry>`-concrete, so
// the mock-runtime test harness cannot drive it directly. All budget logic
// therefore lives in these two runtime-generic helpers, which the command
// calls and the tests exercise with `tauri::test::mock_app()` — the same
// split used by the goal runner. Behavior is unchanged: the helpers carry
// everything budget-related (sidecar/ledger reads, verdicts, emits, the
// cancel token), the command just supplies its arguments.

/// Pre-turn guard: when the session has a cap and `bypass` is not set,
/// cumulative ledger spend at/over the cap rejects the send and fires
/// `budget:exceeded` (the frontend offers continue-once / raise-budget /
/// stop). `bypass` is the "continue once" choice: it exempts *exactly this
/// send's* pre-turn check — the mid-turn guard still enforces the cap.
/// Runs before the querying flag is set, so a rejected send leaves no
/// trace on the session.
pub(crate) async fn enforce_pre_turn_budget<R: tauri::Runtime>(
    state: &AppState,
    app: &tauri::AppHandle<R>,
    session_id: uuid::Uuid,
    budget_cap_usd: Option<f64>,
    bypass: Option<bool>,
) -> Result<(), String> {
    let Some(cap) = budget_cap_usd else {
        return Ok(());
    };
    if bypass.unwrap_or(false) {
        return Ok(());
    }
    let spent = crate::cost_commands::session_spent_usd(state, &session_id.to_string());
    if crate::cost_commands::budget_verdict(spent, cap)
        == crate::cost_commands::BudgetVerdict::Exceeded
    {
        crate::cost_commands::emit_budget_status(app, false, &session_id.to_string(), spent, cap);
        return Err(format!(
            "Session budget exceeded: spent ${spent:.4} of ${cap:.4} — continue (ignore once), raise the budget, or stop"
        ));
    }
    Ok(())
}

/// Mid-turn guard: fold one streaming `Usage` event into the turn's
/// [`crate::cost_commands::BudgetTurnGuard`]; on the first cap crossing
/// emit `budget:exceeded` (latched — one emit per turn) and cancel via the
/// SAME [`tokio_util::sync::CancellationToken`] the `cancel_query` command
/// pulls, so the stream breaks through the identical path; on the first
/// 80% crossing emit a one-shot `budget:warning`. A single event large
/// enough to jump straight past 100% emits exceeded only — the Exceeded
/// arm is checked first.
pub(crate) fn enforce_mid_turn_usage<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    guard: &mut Option<crate::cost_commands::BudgetTurnGuard>,
    cancel_token: &CancellationToken,
    session_id: &uuid::Uuid,
    cost_usd: f64,
) {
    let Some(g) = guard.as_mut() else {
        return;
    };
    match g.on_usage(cost_usd) {
        crate::cost_commands::BudgetTurnAction::Exceeded {
            spent_usd,
            budget_usd,
        } => {
            crate::cost_commands::emit_budget_status(
                app,
                false,
                &session_id.to_string(),
                spent_usd,
                budget_usd,
            );
            cancel_token.cancel();
        }
        crate::cost_commands::BudgetTurnAction::Warn {
            spent_usd,
            budget_usd,
        } => {
            crate::cost_commands::emit_budget_status(
                app,
                true,
                &session_id.to_string(),
                spent_usd,
                budget_usd,
            );
        }
        crate::cost_commands::BudgetTurnAction::Quiet => {}
    }
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
    let model = client_config.model.clone();
    let provider = client_config.provider.to_string();
    let usage_store = state.usage_store.clone();
    let approval_mode_str = state.desktop_config.read().await.approval_mode.clone();
    // P2-4b: hand the shared memory handle to the spawned task — the runner
    // attaches it to its engine instead of leaving memory: None.
    let memory_store = state.memory_store.clone();

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
        // Honour persisted deny/allow rules (no interactive channel here —
        // background tasks run unattended, so prompts would auto-allow anyway).
        let mut settings = SettingsManager::new();
        if settings.load_from_files().is_ok() {
            let rules = &settings.settings_mut().permissions;
            permissions.set_rule_checker(PermissionRuleChecker::from_rule_strings(
                &rules.deny,
                &rules.ask,
                &rules.allow,
            ));
        }

        let engine = crate::commands_memory::attach_shared_memory(
            QueryEngine::with_defaults_arc(client, tools, permissions, StateManager::new()),
            &memory_store,
        );

        let query_id = uuid::Uuid::new_v4();
        let _qid_str = query_id.to_string();

        // Clone before `model` is moved into QueryMetadata so usage events in
        // the stream below can be attributed (mirrors send_message).
        let model_for_usage = model.clone();

        let context = QueryContext {
            query_id,
            session_id: uuid::Uuid::new_v4(),
            user_message: prompt.clone(),
            attachments: Vec::new(),
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
                            crate::commands_usage::UsageTotals {
                                input_tokens,
                                output_tokens,
                                cache_creation_tokens,
                                cache_read_tokens,
                                cost_usd,
                            },
                            None,
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
    /// Seed alternating user/assistant engine messages into a session's L0
    /// log (§4.6): desktop flows project history from this record only.
    fn seed_l0_messages(
        state: &AppState,
        session_id: uuid::Uuid,
        messages: &[shannon_engine::api::Message],
    ) {
        use shannon_types::session_event::{
            AssistantChunkPayload, TurnEndPayload, TurnStartPayload, UserMessagePayload,
        };
        let mut w = shannon_core::session_log::SessionLogWriter::open_layout(
            state.l0_store().container(),
            &session_id.to_string(),
        )
        .expect("open fresh log");
        for m in messages {
            let text = match &m.content {
                shannon_engine::api::MessageContent::Text(t) => t.clone(),
                _ => String::new(),
            };
            if m.role == "user" {
                w.record(shannon_types::session_event::SessionEventBody::TurnStart(
                    TurnStartPayload { query_id: None },
                ));
                w.record(shannon_types::session_event::SessionEventBody::UserMessage(
                    UserMessagePayload {
                        source: UserMessagePayload::SOURCE_USER.into(),
                        content: text,
                    },
                ));
            } else {
                w.record(
                    shannon_types::session_event::SessionEventBody::AssistantChunk(
                        AssistantChunkPayload {
                            delta: text,
                            thinking: false,
                        },
                    ),
                );
                w.record(shannon_types::session_event::SessionEventBody::TurnEnd(
                    TurnEndPayload {
                        reason: TurnEndPayload::REASON_COMPLETED.into(),
                        usage: None,
                        error: None,
                    },
                ));
            }
        }
        w.close().expect("close seeded log");
    }

    use super::*;
    use crate::commands_sessions::branch_session_internal;

    #[test]
    fn test_app_state_new() {
        let state = AppState::new();
        // P0-4: messages/querying moved into the active session in the
        // registry. Lazily create one to verify the empty initial state.
        let session = state.registry.get_or_create_active();
        let messages = session.messages.blocking_lock();
        assert!(messages.is_empty());
        assert!(!*session.querying.blocking_lock());
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
            price_in: None,
            price_out: None,
            tier: None,
            dynamic: None,
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
        let session = state.registry.get_or_create_active();
        {
            let mut q = session.querying.lock().await;
            *q = true;
        }
        assert!(*session.querying.lock().await);
        {
            let mut q = session.querying.lock().await;
            *q = false;
        }
        assert!(!*session.querying.lock().await);
    }

    #[tokio::test]
    async fn test_app_state_messages_push() {
        let state = AppState::new();
        let session = state.registry.get_or_create_active();
        {
            let mut msgs = session.messages.lock().await;
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
        let msgs = session.messages.lock().await;
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

        seed_l0_messages(&state, parent_id, &messages);

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
            .l0_store()
            .load(&branch_uuid)
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

        seed_l0_messages(&state, parent_id, &messages);

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

        seed_l0_messages(&state, parent_id, &messages);

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

        seed_l0_messages(&state, parent_id, &messages);

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

// ── P1.1 (ADR-0005): `build_client_config` reads from the v2 ProviderConfigStore ────
// These tests pin the contract that the desktop runtime client construction
// consumes `ProviderProfile` data (not the legacy `DesktopConfig` singular
// fields). The provider/store fixtures are constructed in-memory — no disk
// reads, no `~/.shannon/providers.toml` writes.

#[cfg(test)]
mod build_client_config_tests {
    use super::*;
    use shannon_core::provider_config_store::ProviderConfigStore;
    use shannon_core::unified_config::ShannonConfig;
    use shannon_engine::api::LlmProvider;
    use shannon_types::provider_config::{
        CredentialRef, CredentialScope, ModelProfile, ProviderKind, ProviderProfile, ProviderTiers,
        Scope,
    };
    use std::collections::HashMap;

    /// Build a single-active-profile `ProviderModelConfig` and wrap it in a
    /// `ProviderConfigStore`. No disk access — safe under nextest isolation.
    fn store_with_active(profile: ProviderProfile, model: &str) -> ProviderConfigStore {
        use shannon_types::provider_config::{ActiveTarget, ProviderModelConfig};
        let mut profiles = HashMap::new();
        profiles.insert(
            "default".to_string(),
            ModelProfile {
                name: "default".to_string(),
                active_target: ActiveTarget {
                    provider_id: profile.id.clone(),
                    model_id: model.to_string(),
                    scope: Scope::Global,
                },
                providers: vec![profile],
                auxiliary: HashMap::new(),
                credential_scope: CredentialScope::Shared,
            },
        );
        ProviderConfigStore::from_config(ProviderModelConfig {
            version: ProviderModelConfig::VERSION,
            profiles,
            gateway: Default::default(),
        })
    }

    fn anthropic_profile(cred_var: &str, base_url: &str) -> ProviderProfile {
        ProviderProfile {
            id: "anthropic".to_string(),
            kind: ProviderKind::Anthropic,
            display_name: "anthropic".to_string(),
            base_url: base_url.to_string(),
            models_url: None,
            credential: CredentialRef::Env {
                var: cred_var.to_string(),
            },
            extra_headers: HashMap::new(),
            default_max_tokens: None,
            fallback_models: Vec::new(),
            quirks: Default::default(),
            tiers: ProviderTiers::default(),
        }
    }

    #[test]
    fn returns_none_when_store_has_no_active_target() {
        // Empty store → no `default` profile → no resolved target → None.
        let store = ProviderConfigStore::default();
        let cfg = ShannonConfig::default();
        assert!(AppState::build_client_config(&store, &cfg).is_none());
    }

    #[test]
    fn returns_none_when_profile_present_but_no_active_target() {
        // Defensive: even with a profile in the store, if `active_target`
        // is blank (the post-`remove_profile` state) we get None.
        let mut store = ProviderConfigStore::default();
        let profile = anthropic_profile("UNUSED", "https://api.anthropic.com");
        // Use ensure_provider so the active_target stays blank.
        let _ = store.ensure_provider(&LlmProvider::Anthropic);
        // Sanity: a profile was added but active_target.provider_id == "".
        let cfg = ShannonConfig::default();
        assert!(AppState::build_client_config(&store, &cfg).is_none());
        // Suppress the unused-var warning without changing behaviour.
        let _ = profile;
    }

    #[test]
    fn single_active_profile_returns_some_with_matching_fields() {
        // SAFETY: unique key read only by this test thread; no concurrent
        // set/remove of the same key elsewhere.
        unsafe { std::env::set_var("BCC_TEST_KEY", "resolved-key") };
        let store = store_with_active(
            anthropic_profile("BCC_TEST_KEY", "https://api.anthropic.com"),
            "claude-sonnet-4-6",
        );
        let cfg = ShannonConfig::default();

        let out =
            AppState::build_client_config(&store, &cfg).expect("active target should resolve");
        assert_eq!(out.provider, LlmProvider::Anthropic);
        assert_eq!(out.base_url, "https://api.anthropic.com");
        assert_eq!(out.model, "claude-sonnet-4-6");
        assert_eq!(out.api_key, "resolved-key");
        // SAFETY: see above.
        unsafe { std::env::remove_var("BCC_TEST_KEY") };
    }

    #[test]
    fn extra_headers_round_trip_from_profile_to_config() {
        let mut profile = anthropic_profile("BCC_UNSET", "https://api.anthropic.com");
        profile
            .extra_headers
            .insert("X-Foo".to_string(), "bar".to_string());
        profile
            .extra_headers
            .insert("X-Trace".to_string(), "abc-123".to_string());
        let store = store_with_active(profile, "claude-sonnet-4-6");
        let cfg = ShannonConfig::default();

        let out =
            AppState::build_client_config(&store, &cfg).expect("active target should resolve");
        assert_eq!(
            out.extra_headers.get("X-Foo").map(String::as_str),
            Some("bar")
        );
        assert_eq!(
            out.extra_headers.get("X-Trace").map(String::as_str),
            Some("abc-123")
        );
    }

    #[test]
    fn default_max_tokens_overrides_when_no_cfg_override() {
        // No `cfg.max_tokens` → use profile.default_max_tokens.
        let mut profile = anthropic_profile("BCC_UNSET", "https://api.anthropic.com");
        profile.default_max_tokens = Some(8192);
        let store = store_with_active(profile, "claude-sonnet-4-6");
        let cfg = ShannonConfig::default();

        let out =
            AppState::build_client_config(&store, &cfg).expect("active target should resolve");
        assert_eq!(out.max_tokens, 8192);
    }

    #[test]
    fn cfg_max_tokens_wins_when_both_set() {
        // Explicit `cfg.max_tokens` beats the profile's `default_max_tokens`.
        let mut profile = anthropic_profile("BCC_UNSET", "https://api.anthropic.com");
        profile.default_max_tokens = Some(8192);
        let store = store_with_active(profile, "claude-sonnet-4-6");
        let mut cfg = ShannonConfig::default();
        cfg.max_tokens = Some(1024);

        let out =
            AppState::build_client_config(&store, &cfg).expect("active target should resolve");
        assert_eq!(out.max_tokens, 1024);
    }

    #[test]
    fn engine_fallback_when_neither_max_tokens_set() {
        // Neither `cfg.max_tokens` nor `profile.default_max_tokens` set →
        // engine fallback to 4096.
        let profile = anthropic_profile("BCC_UNSET", "https://api.anthropic.com");
        let store = store_with_active(profile, "claude-sonnet-4-6");
        let cfg = ShannonConfig::default();

        let out =
            AppState::build_client_config(&store, &cfg).expect("active target should resolve");
        assert_eq!(out.max_tokens, 4096);
    }

    #[test]
    fn ollama_profile_no_api_key_uses_300s_timeout() {
        // Ollama branch: empty api_key + provider-conditional timeout (300s).
        // SAFETY: unique key read only by this test thread.
        unsafe { std::env::remove_var("BCC_OLLAMA_KEY") };
        let profile = ProviderProfile {
            id: "ollama".to_string(),
            kind: ProviderKind::Ollama,
            display_name: "ollama".to_string(),
            base_url: "http://localhost:11434".to_string(),
            models_url: None,
            credential: CredentialRef::Env {
                var: "BCC_OLLAMA_KEY".to_string(),
            },
            extra_headers: HashMap::new(),
            default_max_tokens: None,
            fallback_models: Vec::new(),
            quirks: Default::default(),
            tiers: ProviderTiers::default(),
        };
        let store = store_with_active(profile, "llama3");
        let cfg = ShannonConfig::default();

        let out =
            AppState::build_client_config(&store, &cfg).expect("active target should resolve");
        assert_eq!(out.provider, LlmProvider::Ollama);
        assert_eq!(out.api_key, "", "ollama has no credential");
        assert_eq!(
            out.timeout_seconds, 300,
            "Ollama auto-uses the 300s timeout branch"
        );
        assert_eq!(out.base_url, "http://localhost:11434");
        assert_eq!(out.model, "llama3");
    }
}

// ── P0-4: budget enforcement tests (injectable boundary) ────────────────
// `send_message` is `AppHandle<Wry>`-concrete, so the budget logic was
// extracted into the runtime-generic `enforce_pre_turn_budget` /
// `enforce_mid_turn_usage` helpers (see above) and is tested here through
// `tauri::test::mock_app()` with an AppState whose sessions dir + usage
// ledger are redirected into a tempdir — the two stores the budget path
// reads (`pub(crate)` fields, same crate). Mid-turn tests drive the real
// `BudgetTurnGuard` + the SAME `CancellationToken` type the `cancel_query`
// command pulls, so a cap crossing is asserted to cancel through the
// identical mechanism as the user-facing cancel button.
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod budget_enforcement_tests {
    use super::*;
    use crate::commands_usage::{UsageTotals, record_event};
    use shannon_core::session_log::SessionSidecar;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tauri::Listener;

    fn mock_app() -> tauri::AppHandle<tauri::test::MockRuntime> {
        tauri::test::mock_app().handle().clone()
    }

    /// AppState with the budget-path stores redirected into `dir`.
    /// `AppState::new()` only *reads* ambient config (providers.toml /
    /// desktop config / default tools) — nothing here writes outside `dir`.
    fn budget_test_state(dir: &std::path::Path) -> AppState {
        let mut state = AppState::new();
        state.state_manager = Arc::new(
            StateManager::with_sessions_dir(dir.join("sessions")).expect("temp sessions dir"),
        );
        state.usage_store = Arc::new(crate::commands_usage::UsageStore::with_path(
            dir.join("usage.jsonl"),
        ));
        state
    }

    fn seed_budget(state: &AppState, session_id: uuid::Uuid, cap: f64) {
        state
            .l0_store()
            .save_sidecar_replace(
                &session_id,
                &SessionSidecar {
                    budget_usd: Some(cap),
                    ..Default::default()
                },
            )
            .expect("seed sidecar budget");
    }

    fn seed_spend(state: &AppState, session_id: &uuid::Uuid, cost: f64) {
        state
            .usage_store
            .append(&record_event(
                "budget-path-test-model",
                "anthropic",
                UsageTotals {
                    input_tokens: 1,
                    output_tokens: 1,
                    cache_creation_tokens: 0,
                    cache_read_tokens: 0,
                    cost_usd: cost,
                },
                Some(&session_id.to_string()),
            ))
            .expect("seed ledger spend");
    }

    /// Session-filtered counters for the two budget events.
    struct BudgetEventCounters {
        exceeded: Arc<AtomicUsize>,
        warned: Arc<AtomicUsize>,
        listeners: Vec<tauri::EventId>,
    }

    impl BudgetEventCounters {
        fn exceeded(&self) -> usize {
            self.exceeded.load(Ordering::SeqCst)
        }
        fn warned(&self) -> usize {
            self.warned.load(Ordering::SeqCst)
        }
    }

    fn count_budget_events(
        app: &tauri::AppHandle<tauri::test::MockRuntime>,
        session: &str,
    ) -> BudgetEventCounters {
        let warned = Arc::new(AtomicUsize::new(0));
        let exceeded = Arc::new(AtomicUsize::new(0));
        let mut listeners = Vec::new();

        let ex_counter = exceeded.clone();
        let ex_session = session.to_string();
        listeners.push(app.listen_any(event_names::BUDGET_EXCEEDED, move |e| {
            let Ok(p) =
                serde_json::from_str::<shannon_types::events::BudgetStatusPayload>(e.payload())
            else {
                return;
            };
            if p.session_id == ex_session {
                ex_counter.fetch_add(1, Ordering::SeqCst);
            }
        }));
        let w_counter = warned.clone();
        let w_session = session.to_string();
        listeners.push(app.listen_any(event_names::BUDGET_WARNING, move |e| {
            let Ok(p) =
                serde_json::from_str::<shannon_types::events::BudgetStatusPayload>(e.payload())
            else {
                return;
            };
            if p.session_id == w_session {
                w_counter.fetch_add(1, Ordering::SeqCst);
            }
        }));

        BudgetEventCounters {
            exceeded,
            warned,
            listeners,
        }
    }

    fn unlisten_all(app: &tauri::AppHandle<tauri::test::MockRuntime>, c: &BudgetEventCounters) {
        for id in &c.listeners {
            app.unlisten(*id);
        }
    }

    /// Captured payloads for one event name — used to pin the frozen wire
    /// shape (sessionId/spentUsd/budgetUsd) end-to-end.
    fn capture_payloads(
        app: &tauri::AppHandle<tauri::test::MockRuntime>,
        name: &str,
    ) -> Arc<std::sync::Mutex<Vec<shannon_types::events::BudgetStatusPayload>>> {
        let sink: Arc<std::sync::Mutex<Vec<shannon_types::events::BudgetStatusPayload>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink_for_cb = sink.clone();
        app.listen_any(name, move |e| {
            if let Ok(p) =
                serde_json::from_str::<shannon_types::events::BudgetStatusPayload>(e.payload())
            {
                sink_for_cb.lock().unwrap().push(p);
            }
        });
        sink
    }

    // ① Pre-turn: spend at the cap (first-limit-wins boundary) rejects the
    // send with an explicit error and emits budget:exceeded exactly once.
    #[tokio::test]
    async fn pre_turn_exhausted_budget_rejects_and_emits_exceeded() {
        let app = mock_app();
        let tmp = tempfile::tempdir().unwrap();
        let state = budget_test_state(tmp.path());
        let sid = state.registry.get_or_create_active().session_id;
        seed_budget(&state, sid, 1.0);
        seed_spend(&state, &sid, 0.5);
        let counters = count_budget_events(&app, &sid.to_string());
        let payloads = capture_payloads(&app, event_names::BUDGET_EXCEEDED);

        // Under the cap (0.5 of 1.0): allowed, no events.
        enforce_pre_turn_budget(&state, &app, sid, Some(1.0), None)
            .await
            .expect("spend below the cap must pass");
        assert_eq!(counters.exceeded(), 0);

        // Push spend to 1.5 (over the 1.0 cap): rejected, explicit error,
        // one emit with the frozen payload.
        seed_spend(&state, &sid, 1.0);
        let err = enforce_pre_turn_budget(&state, &app, sid, Some(1.0), None)
            .await
            .expect_err("spend at the cap must reject the send");
        assert!(err.contains("budget exceeded"), "explicit error: {err}");

        assert_eq!(counters.exceeded(), 1, "exactly one exceeded emit");
        let seen = payloads.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].session_id, sid.to_string());
        assert!((seen[0].spent_usd - 1.5).abs() < 1e-9);
        assert!((seen[0].budget_usd - 1.0).abs() < 1e-9);
        assert_eq!(counters.warned(), 0, "pre-turn reject never warns");
        unlisten_all(&app, &counters);
    }

    // ② Bypass is per-call: exempts the send it is attached to, and the
    // next (bypass-less) call rejects again — nothing is stored.
    #[tokio::test]
    async fn pre_turn_bypass_exempts_one_send_only() {
        let app = mock_app();
        let tmp = tempfile::tempdir().unwrap();
        let state = budget_test_state(tmp.path());
        let sid = state.registry.get_or_create_active().session_id;
        seed_budget(&state, sid, 1.0);
        seed_spend(&state, &sid, 1.0);

        // With bypass: accepted despite the exhausted budget.
        enforce_pre_turn_budget(&state, &app, sid, Some(1.0), Some(true))
            .await
            .expect("bypass must exempt this send's pre-turn check");

        // Immediately after, bypass-less: rejected again.
        let err = enforce_pre_turn_budget(&state, &app, sid, Some(1.0), None)
            .await
            .expect_err("the follow-up bypass-less send must reject");
        assert!(err.contains("budget exceeded"), "{err}");

        // No cap at all: always fine, bypass or not.
        enforce_pre_turn_budget(&state, &app, sid, None, None)
            .await
            .expect("no cap = no check");
    }

    // ③ Mid-turn: the first cap crossing cancels through the SAME
    // CancellationToken the cancel_query command pulls, exceeded fires
    // exactly once, and buffered events after the cancel stay silent.
    #[tokio::test]
    async fn mid_turn_exceeded_cancels_token_and_latches_to_one_emit() {
        let app = mock_app();
        let tmp = tempfile::tempdir().unwrap();
        let state = budget_test_state(tmp.path());
        let sid = state.registry.get_or_create_active().session_id;
        let counters = count_budget_events(&app, &sid.to_string());

        // The exact wiring send_message uses: the token created for the
        // turn is cloned into the session slot (cancel_query cancels that
        // slot) and the same token guards the stream loop.
        let turn_token = tokio_util::sync::CancellationToken::new();
        let cancel_token_clone = turn_token.clone();

        let mut guard = Some(crate::cost_commands::BudgetTurnGuard::new(1.0, 0.5));
        assert!(!turn_token.is_cancelled());

        // Event 1: 0.6 → spent 1.1 >= cap → exceeded + cancel.
        enforce_mid_turn_usage(&app, &mut guard, &cancel_token_clone, &sid, 0.6);
        assert!(turn_token.is_cancelled(), "cap crossing must cancel");
        assert_eq!(counters.exceeded(), 1);

        // Events 2..n: already buffered when the cancel lands — the latch
        // must keep them silent, and the token stays cancelled.
        enforce_mid_turn_usage(&app, &mut guard, &cancel_token_clone, &sid, 0.1);
        enforce_mid_turn_usage(&app, &mut guard, &cancel_token_clone, &sid, 0.1);
        assert_eq!(counters.exceeded(), 1, "exceeded is latched to one emit");
        assert_eq!(counters.warned(), 0);
        unlisten_all(&app, &counters);
    }

    // ④ Mid-turn: the 80% crossing warns exactly once; a single event that
    // jumps straight from below the band to >= 100% must NOT warn (the
    // Exceeded arm is checked first) — pinned here via the real guard.
    #[tokio::test]
    async fn mid_turn_warning_is_one_shot_and_jump_past_cap_never_warns() {
        let app = mock_app();
        let tmp = tempfile::tempdir().unwrap();
        let state = budget_test_state(tmp.path());
        let sid = state.registry.get_or_create_active().session_id;
        let counters = count_budget_events(&app, &sid.to_string());
        let cancel = tokio_util::sync::CancellationToken::new();

        // Warning band: 7.0 (70%, quiet) → +1.5 (85%, warn once) → quiet.
        let mut guard = Some(crate::cost_commands::BudgetTurnGuard::new(10.0, 0.0));
        enforce_mid_turn_usage(&app, &mut guard, &cancel, &sid, 7.0);
        assert_eq!(counters.warned(), 0);
        assert!(!cancel.is_cancelled());
        enforce_mid_turn_usage(&app, &mut guard, &cancel, &sid, 1.5);
        assert_eq!(counters.warned(), 1, "one-shot warning at the 80% line");
        enforce_mid_turn_usage(&app, &mut guard, &cancel, &sid, 0.5);
        assert_eq!(counters.warned(), 1, "still under the cap: no re-warn");
        assert_eq!(counters.exceeded(), 0);
        assert!(!cancel.is_cancelled(), "under the cap: no cancel");

        // Jump-past-cap turn: from 0 straight to 2.0 of a 1.0 cap.
        let jump_counters = count_budget_events(&app, &sid.to_string());
        let jump_cancel = tokio_util::sync::CancellationToken::new();
        let mut guard = Some(crate::cost_commands::BudgetTurnGuard::new(1.0, 0.0));
        enforce_mid_turn_usage(&app, &mut guard, &jump_cancel, &sid, 2.0);
        enforce_mid_turn_usage(&app, &mut guard, &jump_cancel, &sid, 0.5);
        assert_eq!(jump_counters.exceeded(), 1);
        assert_eq!(
            jump_counters.warned(),
            0,
            "no stray warning on a jump straight past the cap"
        );
        assert!(jump_cancel.is_cancelled());
        unlisten_all(&app, &counters);
        unlisten_all(&app, &jump_counters);
    }

    // ⑤ No cap on the session: the guard is a complete no-op (no events,
    // no cancel) even for arbitrarily large usage.
    #[tokio::test]
    async fn mid_turn_without_a_cap_is_a_noop() {
        let app = mock_app();
        let tmp = tempfile::tempdir().unwrap();
        let state = budget_test_state(tmp.path());
        let sid = state.registry.get_or_create_active().session_id;
        let counters = count_budget_events(&app, &sid.to_string());
        let cancel = tokio_util::sync::CancellationToken::new();

        let mut guard: Option<crate::cost_commands::BudgetTurnGuard> = None;
        enforce_mid_turn_usage(&app, &mut guard, &cancel, &sid, 999.0);
        assert_eq!(counters.exceeded(), 0);
        assert_eq!(counters.warned(), 0);
        assert!(!cancel.is_cancelled());
        unlisten_all(&app, &counters);
    }
}
