//! REPL main loop and terminal management

/// Number of lines above which a paste is shown as "[Pasted Text #N X lines]"
const PASTE_THRESHOLD_LINES: usize = 5;

mod adapter_impl;
mod at_reference;
mod commands;
mod custom_commands;
mod diagnostic_watcher;
mod helpers;
mod input;
mod mcp_completion;
pub(crate) mod preferences;
mod query;
pub(crate) mod render;
mod session;
mod sidebar;
mod source_watcher;
pub(crate) mod state;

use crate::{
    Result,
    events::EventHandler,
    render::Renderer,
    repl_enhancement::{DiffData, ReplHistory, ReplRenderer},
    theme::Theme,
    vim::{VimHandler, VimMode},
    widgets::{ChatRole, ChatWidget, PromptWidget, StreamingState},
};
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, TerminalOptions, Viewport, backend::CrosstermBackend};
use shannon_types::recover_lock;
use std::collections::HashMap;

/// Type alias for local (stdio) MCP server config: (name, command, args, env, oauth_scopes).
type LocalServerEntry = (
    String,
    String,
    Vec<String>,
    HashMap<String, String>,
    Vec<String>,
);
/// Type alias for remote (HTTP/SSE) MCP server config: (name, url, headers, oauth_scopes).
type HttpServerEntry = (String, String, HashMap<String, String>, Vec<String>);
use std::io::{self, Write as IoWrite};
use std::sync::Arc;
use tokio::runtime::Runtime;

// Import core functionality
use shannon_commands::{
    Command, CommandBase, CommandParser, CommandRegistry, ExecutionContext, PromptCommand,
    SharedExecutor, builtin_commands,
};
use shannon_core::{
    PromptInfo, api::LlmClientConfig, permissions::PermissionManager, query_engine::QueryEngine,
    recording::SessionRecorder, state::StateManager, tools::ToolRegistry,
};

// Tool registration
use crate::skill_bridge::register_skills_as_tools;
use shannon_mcp::{
    HeaderSource, McpProcessPool, discover_pooled_remote_tools, discover_pooled_tools,
};
use shannon_tools::register_default_tools_with_project_dir_ex;

// Re-export public types from state submodule
pub use state::{
    AgentDisplay, LoopState, PendingElicitation, PlanState, RalphState, ReplState, SidebarTab,
};

// Re-export custom_commands types used by other modules
pub(super) use custom_commands::{
    CustomCommandEntry, collect_custom_commands, dedup_custom_commands,
};
pub(crate) use custom_commands::{CustomCommandWatcher, SettingsWatcher};

/// Main REPL application struct
pub struct Repl {
    /// Event handler for user input
    pub(crate) events: EventHandler,
    /// Renderer for UI drawing
    pub(crate) renderer: Renderer,
    /// Chat widget for displaying messages
    pub(crate) chat: ChatWidget,
    /// Prompt widget for user input
    pub(crate) prompt: PromptWidget,
    /// Application state
    pub(crate) state: ReplState,
    /// Running state
    pub(crate) running: bool,
    /// Query engine for AI processing
    pub(crate) query_engine: Option<QueryEngine>,
    /// State manager for session persistence (separate from QueryEngine's internal one)
    pub(crate) state_manager: StateManager,
    /// Command registry with all built-in commands
    pub(crate) command_registry: CommandRegistry,
    /// Command parser for parsing /commands
    pub(crate) command_parser: CommandParser,
    /// Shared command executor for concurrent command dispatch
    pub(crate) shared_executor: SharedExecutor,
    /// Tokio runtime for async operations
    pub(crate) runtime: Runtime,
    /// Permission request receiver (from QueryEngine to REPL UI)
    pub(crate) permission_req_rx:
        tokio::sync::mpsc::UnboundedReceiver<shannon_core::query_engine::PermissionRequest>,
    /// Permission request sender (from REPL to QueryEngine)
    pub(crate) permission_req_tx:
        tokio::sync::mpsc::UnboundedSender<shannon_core::query_engine::PermissionRequest>,
    /// Last session listing cache (for /resume by number)
    pub(crate) last_session_list: Vec<shannon_core::state::SessionInfo>,
    /// Command history with cursor navigation
    pub(crate) command_history: ReplHistory,
    /// Saved input before history navigation (to restore on down-to-bottom)
    pub(crate) saved_input: String,
    /// Per-turn diff tracking
    pub(crate) diff_data: DiffData,
    /// Current turn index
    pub(crate) current_turn: usize,
    /// Session start time
    pub(crate) session_started_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Markdown renderer for assistant output
    pub(crate) output_renderer: ReplRenderer,
    /// Total commands run in this session
    pub(crate) commands_run: usize,
    /// Total tools invoked in this session
    pub(crate) tools_invoked: usize,
    /// Tab completion state for cycling through matches
    pub(crate) tab_completion_state: TabCompletionState,
    /// Vim key handler for vim mode support (yy/yw/p yank/paste)
    pub(crate) vim_handler: VimHandler,
    /// Multi-agent team coordinator (lazy-initialized on /team create)
    pub(crate) team_coordinator: Option<std::sync::Arc<shannon_agents::AgentCoordinator>>,
    /// Sub-agent registry for background agent management
    pub(crate) agent_registry: Option<std::sync::Arc<shannon_agents::SubAgentRegistry>>,
    /// Throttle timestamp for agent refresh (avoids block_on on every tick)
    pub(crate) last_agent_refresh: Option<std::time::Instant>,
    /// Coordinator event receiver for agent dashboard live updates
    pub(crate) coordinator_event_rx:
        Option<tokio::sync::broadcast::Receiver<shannon_agents::CoordinatorEvent>>,
    /// MCP process pool for hot-reload support
    pub(crate) mcp_pool: std::sync::Arc<McpProcessPool>,
    /// Tool registry for MCP hot-reload tool registration
    pub(crate) tool_registry: std::sync::Arc<shannon_core::tools::ToolRegistry>,
    /// MCP progress update receiver (from McpProcessPool to REPL UI)
    pub(crate) mcp_progress_rx:
        Option<tokio::sync::mpsc::UnboundedReceiver<(String, f64, Option<f64>)>>,
    /// Model routing rules: (pattern, model_name) pairs
    pub(crate) model_routes: Vec<(String, String)>,
    /// Checkpoint manager for undo/revert operations
    pub(crate) checkpoint_manager: shannon_core::CheckpointManager,
    /// Desktop notification dispatcher
    pub(crate) notifier: shannon_core::notifier::Notifier,
    /// Whether desktop notifications are enabled
    pub(crate) notifications_enabled: bool,
    /// Webhook receiver for external event injection
    pub(crate) webhook_receiver: Option<shannon_core::webhook::WebhookReceiver>,
    /// Instruction file watcher for hot-reloading CLAUDE.md / AGENTS.md / GEMINI.md
    pub(crate) instruction_watcher: Option<shannon_core::project_instructions::InstructionWatcher>,
    /// Custom command file watcher for hot-reloading .claude/commands/ and .shannon/commands/
    pub(crate) command_watcher: Option<CustomCommandWatcher>,
    /// Settings file watcher for hot-reloading settings.json / config.toml
    pub(crate) settings_watcher: Option<custom_commands::SettingsWatcher>,
    /// Source file watcher for detecting project code changes
    pub(crate) source_watcher: Option<source_watcher::SourceWatcher>,
    /// Streaming tool result cache (shared with ToolRegistry).
    /// Used to invalidate cached read-only tool results when source files change.
    pub(crate) streaming_cache: Option<std::sync::Arc<shannon_core::tool_cache::ToolResultCache>>,
    /// Background diagnostic check pending flag (debounce)
    pub(crate) diagnostic_pending: std::sync::Arc<tokio::sync::Mutex<bool>>,
    /// Background diagnostic result receiver
    pub(crate) diagnostic_rx: Option<diagnostic_watcher::DiagnosticReceiver>,
    /// Background update check result (deferred to avoid blocking startup)
    pub(crate) update_check_rx: Option<std::sync::Mutex<std::sync::mpsc::Receiver<String>>>,
    /// Crash-safe JSONL session recovery log (appends each turn with fsync)
    pub(crate) session_recovery: shannon_core::SessionRecovery,
    /// Shared plan-mode flag (clone of the one in QueryEngine)
    pub(crate) plan_mode_flag: std::sync::Arc<std::sync::RwLock<bool>>,
    /// Session recorder for deterministic replay testing
    pub(crate) session_recorder: Option<SessionRecorder>,
}

/// State for tab completion cycling
#[derive(Debug, Clone, Default)]
pub(crate) struct TabCompletionState {
    /// The prefix text being completed (to detect when completion should reset)
    pub(crate) last_prefix: String,
    /// Current match index for cycling through completions
    pub(crate) current_index: usize,
    /// Available completion candidates
    pub(crate) candidates: Vec<String>,
}

/// Load permission allow/deny rules from settings files into the PermissionManager.
///
/// Reads from (in order, later files override earlier):
/// 1. `~/.shannon/settings.json`  (user-level)
/// 2. `.shannon/settings.json`    (project-level)
/// 3. `.claude/settings.json`     (Claude Code compatibility)
///
/// Expected format:
/// ```json
/// {
///   "permissions": {
///     "allow": ["Tool(name)", "Bash(git *)"],
///     "deny": ["Bash(rm -rf *)"]
///   }
/// }
/// ```
fn load_permission_rules(pm: &mut PermissionManager) {
    let cwd = std::env::current_dir().unwrap_or_default();
    let home = dirs::home_dir();

    let mut paths = Vec::new();
    if let Some(ref h) = home {
        paths.push(h.join(".shannon").join("settings.json"));
    }
    paths.push(cwd.join(".shannon").join("settings.json"));
    paths.push(cwd.join(".claude").join("settings.json"));

    for path in paths {
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let doc: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("Skipping invalid settings file {}: {e}", path.display());
                continue;
            }
        };

        let perms = match doc.get("permissions") {
            Some(p) => p,
            None => continue,
        };

        if let Some(allow_arr) = perms.get("allow").and_then(|v| v.as_array()) {
            for item in allow_arr {
                if let Some(s) = item.as_str() {
                    // Simple tool names like "Bash" or glob patterns like "mcp__*"
                    if s.contains('(') || s.contains('*') || s.contains('?') {
                        pm.allow_pattern(s);
                    } else {
                        pm.allow_tool(s);
                    }
                }
            }
        }

        if let Some(deny_arr) = perms.get("deny").and_then(|v| v.as_array()) {
            for item in deny_arr {
                if let Some(s) = item.as_str() {
                    if s.contains('(') || s.contains('*') || s.contains('?') {
                        pm.deny_pattern(s);
                    } else {
                        pm.deny_tool(s);
                    }
                }
            }
        }

        tracing::info!("Loaded permission rules from {}", path.display());
    }
}

/// Extract a domain/URL from tool input for network-related tools.
/// Returns the URL string if the tool is a known network tool with a URL in its input.
fn extract_domain_from_tool(tool_name: &str, tool_input: &serde_json::Value) -> Option<String> {
    let url_str = match tool_name {
        "fetch" | "web_fetch" | "WebFetch" | "web-fetch" => tool_input
            .get("url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "web_search" | "WebSearch" | "web-search" | "tavily-search" => tool_input
            .get("query")
            .and_then(|v| v.as_str())
            .map(|q| format!("search: {q}")),
        _ => None,
    };
    // Truncate very long URLs for display
    url_str.map(|s| {
        if s.len() > 80 {
            let mut end = 77;
            while !s.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}...", &s[..end])
        } else {
            s
        }
    })
}

impl Repl {
    /// Minimal REPL for test mode — skips MCP, skills, memory, project instructions,
    /// but includes a lightweight query_engine with an unauthenticated LLM client.
    fn new_minimal(runtime: Runtime) -> Result<Self> {
        let _rt_guard = runtime.enter();

        let tool_registry = Arc::new(ToolRegistry::new());
        let mcp_pool = Arc::new(McpProcessPool::new());
        let (permission_req_tx, permission_req_rx) = tokio::sync::mpsc::unbounded_channel();

        let command_registry = {
            let registry = CommandRegistry::new();
            builtin_commands::register_all(&registry);
            registry
        };

        let shared_executor = {
            use shannon_commands::CommandExecutor;
            SharedExecutor::new(CommandExecutor::new(command_registry.clone()))
        };

        let client_config = LlmClientConfig::default();
        let client = shannon_core::api::LlmClient::new_unauthenticated(client_config);
        let permission_manager = PermissionManager::new();
        let state_manager = StateManager::new();
        let query_engine = QueryEngine::with_defaults_arc(
            client,
            tool_registry.clone(),
            permission_manager,
            state_manager,
        );

        let mut repl = Self {
            events: EventHandler::new(50)?,
            renderer: Renderer::new(),
            chat: ChatWidget::new(1000),
            prompt: PromptWidget::new(),
            state: ReplState::default(),
            running: false,
            query_engine: Some(query_engine),
            state_manager: StateManager::new(),
            command_registry,
            command_parser: CommandParser::new(),
            shared_executor,
            runtime,
            permission_req_rx,
            permission_req_tx,
            last_session_list: Vec::new(),
            command_history: ReplHistory::new(1000),
            saved_input: String::new(),
            diff_data: DiffData::new(),
            current_turn: 0,
            session_started_at: Some(chrono::Utc::now()),
            output_renderer: ReplRenderer::new(),
            commands_run: 0,
            tools_invoked: 0,
            tab_completion_state: TabCompletionState::default(),
            vim_handler: {
                let mut h = VimHandler::new();
                h.set_mode(VimMode::Insert);
                h
            },
            team_coordinator: None,
            agent_registry: None,
            last_agent_refresh: None,
            coordinator_event_rx: None,
            mcp_pool,
            tool_registry,
            mcp_progress_rx: None,
            model_routes: Vec::new(),
            checkpoint_manager: shannon_core::CheckpointManager::new(),
            notifier: shannon_core::notifier::Notifier::new(),
            notifications_enabled: false,
            webhook_receiver: None,
            instruction_watcher: None,
            command_watcher: None,
            settings_watcher: None,
            source_watcher: None,
            streaming_cache: None,
            diagnostic_pending: std::sync::Arc::new(tokio::sync::Mutex::new(false)),
            diagnostic_rx: None,
            update_check_rx: None,
            session_recovery: shannon_core::SessionRecovery::new().unwrap_or_default(),
            plan_mode_flag: std::sync::Arc::new(std::sync::RwLock::new(false)),
            session_recorder: None,
        };

        repl.sync_approval_mode_label();
        repl.state
            .spinner
            .set_static_mode(repl.state.reduced_motion);
        repl.renderer.set_theme(&repl.state.theme);
        repl.output_renderer.syntect_theme_name = repl.state.theme.syntect_theme_name().to_string();
        Ok(repl)
    }

    /// Create a new REPL instance
    pub fn new() -> Result<Self> {
        let runtime = Runtime::new()?;
        let _rt_guard = runtime.enter();

        // In test mode, use lightweight init (no tools, MCP, skills, memory).
        // The DockerSandbox::is_available() and MCP discovery are also skipped.
        if cfg!(test) {
            return Self::new_minimal(runtime);
        }

        // Create tool registry and register all tools (sandboxed to project dir)
        let project_dir = std::env::current_dir().unwrap_or_default();
        let mut tool_registry = ToolRegistry::new();
        let reg_result =
            register_default_tools_with_project_dir_ex(&mut tool_registry, &project_dir)
                .map_err(|e| anyhow::anyhow!("Failed to register tools: {e}"))?;
        let agent_context_handle = reg_result.agent_context_handle;
        let plan_mode_flag = reg_result.plan_manager.plan_mode_flag();

        // Load and register skills from shannon-skills as tools.
        // Also capture the formatted skills list for LLM context injection.
        let (_, skills_for_llm) = register_skills_as_tools(&mut tool_registry);

        // Discover MCP server configurations and register their tools dynamically.
        // Servers are batched to avoid file descriptor exhaustion:
        //   - Local (stdio) servers: batches of 3
        //   - Remote (http/sse) servers: batches of 20
        let mut discovered_mcp_prompts: Vec<(String, PromptInfo)> = Vec::new(); // populated during pooled discovery
        let mcp_pool = Arc::new(McpProcessPool::new()); // persistent pool for all MCP servers
        // Shared single-thread runtime for all MCP async operations (discovery, sampling, progress).
        // Reusing one runtime avoids ~30ms of overhead from creating three separate runtimes.
        let mcp_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .or_else(|_| tokio::runtime::Runtime::new())
            .map_err(|e| anyhow::anyhow!("failed to create MCP runtime: {e}"))?;
        if !cfg!(test) {
            let mut mcp_registry = shannon_core::mcp_advanced::McpServerRegistry::new();
            let mcp_count = mcp_registry.load_from_default_paths();
            if mcp_count > 0 {
                tracing::info!("Discovered {} MCP server configuration(s)", mcp_count);

                // Load approval state for MCP server gating
                let approval_path = std::path::PathBuf::from(".shannon/mcp_approvals.json");
                let mut approval_manager = shannon_core::McpApprovalManager::with_defaults();
                if let Err(e) = approval_manager.load_from_file(&approval_path) {
                    tracing::debug!("Could not load MCP approval state: {}", e);
                }

                let discovery_rt = &mcp_rt;

                // Classify servers into local (stdio) and remote (http/sse) buckets
                let mut local_servers: Vec<LocalServerEntry> = Vec::new();
                let mut http_servers: Vec<HttpServerEntry> = Vec::new(); // (name, url, headers, oauth_scopes)

                for config in mcp_registry.enabled_servers() {
                    // Check server approval before attempting discovery
                    let approval_transport = match config.transport_type {
                        shannon_core::mcp_advanced::TransportType::Stdio => {
                            shannon_core::mcp_server_approval::McpTransportType::Stdio
                        }
                        shannon_core::mcp_advanced::TransportType::Http => {
                            shannon_core::mcp_server_approval::McpTransportType::StreamableHttp
                        }
                        shannon_core::mcp_advanced::TransportType::Sse => {
                            shannon_core::mcp_server_approval::McpTransportType::Sse
                        }
                    };
                    let mut approval_req = shannon_core::McpServerApprovalRequest::new(
                        &config.name,
                        approval_transport,
                    );
                    if let Some(ref url) = config.url {
                        approval_req.server_url = Some(url.clone());
                    }
                    approval_req.capabilities.push("tools".to_string());
                    let decision = approval_manager
                        .request_approval(approval_req)
                        .unwrap_or(shannon_core::ApprovalDecision::Deny);
                    match decision {
                        shannon_core::ApprovalDecision::Deny => {
                            tracing::warn!(
                                "MCP server '{}' denied by approval policy, skipping",
                                config.name
                            );
                            continue;
                        }
                        shannon_core::ApprovalDecision::ApproveWithRestrictions { .. } => {
                            tracing::warn!(
                                "MCP server '{}' requires manual approval. \
                                 Use /mcp approve {} to enable on next startup.",
                                config.name,
                                config.name
                            );
                            continue;
                        }
                        shannon_core::ApprovalDecision::Approve => {}
                    }

                    match (&config.command, &config.url) {
                        (Some(cmd), _) => {
                            // Stdio transport
                            let entry = (
                                config.name.clone(),
                                cmd.clone(),
                                config.args.clone(),
                                config.env.clone(),
                                config.oauth_scopes.clone(),
                            );
                            local_servers.push(entry);
                        }
                        (None, Some(url)) => {
                            // HTTP/SSE transport — discover via HTTP
                            http_servers.push((
                                config.name.clone(),
                                url.clone(),
                                config.headers.clone(),
                                config.oauth_scopes.clone(),
                            ));
                        }
                        (None, None) => {
                            tracing::warn!(
                                "Skipping '{}' (no command or URL configured)",
                                config.name
                            );
                            continue;
                        }
                    }
                }

                const LOCAL_BATCH_SIZE: usize = 3;
                const REMOTE_BATCH_SIZE: usize = 20;

                // Use the persistent pool created above the discovery block.
                // This replaces one-shot process spawning with persistent connections,
                // eliminating per-call initialization overhead.
                let mcp_pool = mcp_pool.clone();

                // Collect all pooled MCP tool adapters
                let mut all_pooled_adapters: Vec<shannon_mcp::PooledMcpToolAdapter> = Vec::new();

                // Discover local (stdio) servers via persistent pool connections
                for batch in local_servers.chunks(LOCAL_BATCH_SIZE) {
                    let futures: Vec<_> = batch
                        .iter()
                        .map(|(name, cmd, args, env, _scopes)| {
                            discover_pooled_tools(mcp_pool.clone(), name, cmd, args, env)
                        })
                        .collect();
                    let results = discovery_rt.block_on(futures::future::join_all(futures));
                    for (result, (name, _, _, _, _scopes)) in results.into_iter().zip(batch.iter())
                    {
                        match result {
                            Ok(discovery) => {
                                let tool_count = discovery.tools.len();
                                all_pooled_adapters.extend(discovery.tools);
                                tracing::info!(
                                    "Discovered {} tool(s) from '{}' (pooled)",
                                    tool_count,
                                    name
                                );
                            }
                            Err(e) => {
                                tracing::warn!("MCP server '{}' discovery failed: {e}", name);
                            }
                        }
                    }
                }

                // Discover remote (http/sse) servers via persistent pool connections
                for batch in http_servers.chunks(REMOTE_BATCH_SIZE) {
                    let futures: Vec<_> = batch
                        .iter()
                        .map(|(name, url, headers, _scopes)| {
                            let header_sources: HashMap<String, HeaderSource> = headers
                                .iter()
                                .map(|(k, v)| (k.clone(), HeaderSource::Static(v.clone())))
                                .collect();
                            discover_pooled_remote_tools(
                                mcp_pool.clone(),
                                name,
                                url,
                                header_sources,
                                None,
                            )
                        })
                        .collect();
                    let results = discovery_rt.block_on(futures::future::join_all(futures));
                    for (result, (name, _, _, _scopes)) in results.into_iter().zip(batch.iter()) {
                        match result {
                            Ok(discovery) => {
                                let tool_count = discovery.tools.len();
                                all_pooled_adapters.extend(discovery.tools);
                                tracing::info!(
                                    "Discovered {} tool(s) from '{}' (pooled, remote)",
                                    tool_count,
                                    name
                                );
                            }
                            Err(e) => {
                                tracing::warn!("MCP server '{}' discovery failed: {e}", name);
                            }
                        }
                    }
                }

                // Auto-enable deferred schema loading when there are many MCP tools.
                // Note: deferred mode is set AFTER discovery for pooled adapters since the
                // adapters already stored their real schemas during discovery if the pool's
                // deferred flag was enabled. We set it now and rebuild with minimal schemas.
                if all_pooled_adapters.len() > shannon_core::DEFERRED_SCHEMA_THRESHOLD {
                    tracing::info!(
                        "Enabling deferred schema loading for {} MCP tools (threshold: {})",
                        all_pooled_adapters.len(),
                        shannon_core::DEFERRED_SCHEMA_THRESHOLD
                    );
                    mcp_pool.set_defer_tool_schemas(true);

                    // Build a DeferredSchemaStore from the pool's stored schemas
                    let store = shannon_core::DeferredSchemaStore::default();
                    for name in mcp_pool.deferred_schema_tool_names() {
                        if let Some(schema) = mcp_pool.get_deferred_schema(&name) {
                            recover_lock(store.lock()).insert(name, schema);
                        }
                    }
                    let search_tool = shannon_core::DeferredSchemaSearchTool::new(store);
                    if let Err(e) = tool_registry.register(Box::new(search_tool)) {
                        tracing::debug!("mcp__tool_search registration skipped: {}", e);
                    }
                }

                // Register all pooled MCP tool adapters
                for tool in all_pooled_adapters {
                    if let Err(e) = tool_registry.register(Box::new(tool)) {
                        tracing::debug!("MCP tool registration skipped: {}", e);
                    }
                }

                if mcp_pool.is_defer_tool_schemas() {
                    tracing::info!(
                        "Deferred mode active: {} tool schemas stored",
                        mcp_pool.deferred_schema_tool_names().len()
                    );
                }

                // Discover prompts from all connected servers and populate
                // discovered_mcp_prompts for slash-command registration below.
                let pooled_prompts = discovery_rt.block_on(mcp_pool.list_all_prompts());
                for (server_name, prompts) in pooled_prompts {
                    for p in prompts {
                        let arg_names = p
                            .arguments
                            .map(|args| args.into_iter().map(|a| a.name).collect())
                            .unwrap_or_default();
                        discovered_mcp_prompts.push((
                            server_name.clone(),
                            PromptInfo {
                                name: p.name,
                                description: p.description,
                                argument_names: arg_names,
                            },
                        ));
                    }
                }

                // Persist approval state (auto-approved servers, any new denies)
                if let Err(e) = approval_manager.save_to_file(&approval_path) {
                    tracing::debug!("Could not save MCP approval state: {}", e);
                }
            }
        }

        // Load plugins from ~/.shannon/plugins/
        if !cfg!(test) {
            let plugins_dir = dirs::home_dir()
                .unwrap_or_default()
                .join(".shannon")
                .join("plugins");
            let mut plugin_registry = shannon_core::plugin::PluginRegistry::new(plugins_dir);
            if runtime.block_on(plugin_registry.load_all()).is_ok() {
                let enabled = plugin_registry.list_enabled();
                if !enabled.is_empty() {
                    tracing::info!("Loaded {} plugin(s)", enabled.len());
                    for plugin in &enabled {
                        match plugin.manifest.kind() {
                            Ok(shannon_core::plugin::PluginKind::Tool { transport }) => {
                                if let Some(command) = transport.command() {
                                    let args = transport.args().to_vec();
                                    match runtime.block_on(shannon_core::discover_tools(
                                        &plugin.manifest.name,
                                        command,
                                        &args,
                                        &std::collections::HashMap::new(),
                                        None,
                                    )) {
                                        Ok(result) => {
                                            let tool_count = result.tools.len();
                                            for tool in result.tools {
                                                if let Err(e) =
                                                    tool_registry.register(Box::new(tool))
                                                {
                                                    tracing::debug!(
                                                        "Plugin tool registration skipped: {}",
                                                        e
                                                    );
                                                }
                                            }
                                            tracing::info!(
                                                "Registered {} tool(s) from plugin '{}'",
                                                tool_count,
                                                plugin.manifest.name
                                            );
                                        }
                                        Err(e) => {
                                            tracing::warn!(
                                                "Plugin '{}' tool discovery failed: {e}",
                                                plugin.manifest.name
                                            );
                                        }
                                    }
                                }
                            }
                            Ok(shannon_core::plugin::PluginKind::Command { name, description }) => {
                                tracing::info!(
                                    "Command plugin '{}' ({}) loaded",
                                    name,
                                    description
                                );
                            }
                            Ok(shannon_core::plugin::PluginKind::Skill {
                                trigger,
                                template: _,
                            }) => {
                                tracing::info!(
                                    "Skill plugin '{}' (trigger: '{}') loaded",
                                    plugin.manifest.name,
                                    trigger
                                );
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "Plugin '{}' has invalid config: {e}",
                                    plugin.manifest.name
                                );
                            }
                        }
                    }
                }
            }
        }

        // Create LLM client
        let client_config = LlmClientConfig::default();

        // Inject team context into AgentTool for sub-agent execution + team coordination
        // This requires a tokio runtime; skip gracefully in test contexts without one.
        let mut shared_coordinator: Option<std::sync::Arc<shannon_agents::AgentCoordinator>> = None;
        if let Ok(mut guard) = agent_context_handle.lock() {
            let team_ctx = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current()
                        .block_on(shannon_tools::AgentToolContext::new(client_config.clone()))
                })
            }));
            match team_ctx {
                Ok(Ok(ctx)) => {
                    // Inject shared LLM executor so teammates can make real LLM calls
                    let ctx = {
                        let llm_client =
                            shannon_core::api::LlmClient::new(ctx.client_config.clone());
                        let executor = shannon_agents::shared_executor(llm_client);
                        ctx.with_executor(executor)
                    };
                    // Register team coordination tools (team_task_create/update/list)
                    if let Err(e) = shannon_tools::register_team_tools(
                        &mut tool_registry,
                        ctx.coordinator.clone(),
                    ) {
                        tracing::warn!("Team tool registration failed: {e}");
                    }
                    shared_coordinator = Some(ctx.coordinator.clone());
                    *guard = Some(ctx);
                }
                Ok(Err(e)) if e.to_string().contains("Agent teams disabled") => {}
                Ok(Err(e)) => tracing::warn!("Team context init failed: {e}"),
                Err(_) => {} // No tokio runtime (test context) — team features disabled
            }
        }

        // Validate config and show warning if not fully configured
        if let Err(e) = client_config.validate() {
            eprintln!("Warning: {e}");
        }
        tracing::info!("LLM config: {}", client_config.describe());

        // Capture model name before client_config is moved
        let config_model = client_config.model.clone();

        let client = if client_config.provider.requires_auth() {
            shannon_core::api::LlmClient::new(client_config)
        } else {
            shannon_core::api::LlmClient::new_unauthenticated(client_config)
        };

        // Wrap tool registry in Arc so it can be shared with MCP callbacks
        // for dynamic tool re-registration.
        // Attach streaming cache for read-only tool result caching.
        let streaming_cache =
            std::sync::Arc::new(shannon_core::tool_cache::ToolResultCache::with_default());
        tool_registry.set_streaming_cache(streaming_cache.clone());
        let tool_registry = std::sync::Arc::new(tool_registry);

        // Wire MCP sampling and elicitation providers so MCP servers can
        // request LLM completions (sampling) and ask the user questions (elicitation).
        //
        // Bounded channel (capacity 16) caps memory growth if a misbehaving MCP
        // server floods elicitations. On full queue, the provider returns Cancel
        // to the server instead of blocking or dropping queued requests.
        const ELICITATION_CHANNEL_CAPACITY: usize = 16;
        let (elicitation_tx, elicitation_rx) =
            tokio::sync::mpsc::channel::<PendingElicitation>(ELICITATION_CHANNEL_CAPACITY);
        {
            let pool = mcp_pool.clone();
            let llm = std::sync::Arc::new(client.clone());
            let sampling = shannon_mcp::make_sampling_provider(llm);
            let elicitation_tx = elicitation_tx.clone();
            let elicitation = shannon_mcp::make_elicitation_provider(Some(std::sync::Arc::new(
                move |message: String, schema: Option<serde_json::Value>, server_name: String| {
                    let tx = elicitation_tx.clone();
                    Box::pin(async move {
                        let (responder, receiver) = tokio::sync::oneshot::channel();
                        let placeholder = schema
                            .as_ref()
                            .and_then(|s| s.get("placeholder"))
                            .and_then(|p| p.as_str())
                            .map(|s| s.to_string());
                        let pending = PendingElicitation {
                            server_name,
                            message,
                            placeholder,
                            responder,
                        };
                        // Bounded send: if the queue is full (server flooding the
                        // UI) or the TUI has shut down, return Cancel so the MCP
                        // client sees a determinate response instead of hanging.
                        if tx.try_send(pending).is_err() {
                            tracing::warn!("Elicitation channel full or closed; cancelling");
                            return (shannon_mcp::ElicitationAction::Cancel, None);
                        }
                        match receiver.await {
                            Ok(result) => (result.action, result.content),
                            Err(_) => (shannon_mcp::ElicitationAction::Cancel, None),
                        }
                    })
                },
            )));
            mcp_rt.block_on(async {
                pool.set_sampling_provider(sampling).await;
                pool.set_elicitation_provider(elicitation).await;
                // Expose the project directory as a filesystem root so MCP servers
                // (e.g. filesystem, git) know the workspace boundaries.
                let project_dir = std::env::current_dir().unwrap_or_default();
                pool.set_roots_provider(std::sync::Arc::new(move || {
                    let uri = format!("file://{}", project_dir.display());
                    vec![shannon_mcp::Root {
                        uri,
                        name: Some("project".to_string()),
                    }]
                }))
                .await;

                // Dynamic tool re-registration: when a server reports
                // tools/list_changed, swap out its old tools for the new ones.
                let reg = tool_registry.clone();
                pool.set_on_tools_changed(std::sync::Arc::new(move |server_name, new_tools| {
                    let prefix = format!("mcp__{server_name}__");
                    // Unregister old tools from this server.
                    {
                        let tools_to_remove: Vec<String> = reg
                            .list()
                            .into_iter()
                            .filter(|n| n.starts_with(&prefix))
                            .collect();
                        for name in tools_to_remove {
                            if let Err(e) = reg.unregister(&name) {
                                tracing::debug!("Dynamic unregister {}: {}", name, e);
                            }
                        }
                    }
                    // Register new tools.
                    for tool in new_tools {
                        if let Err(e) = reg.register(Box::new(tool)) {
                            tracing::debug!("Dynamic register: {}", e);
                        }
                    }
                    tracing::info!(
                        server = %server_name,
                        "Dynamically re-registered tools from notification"
                    );
                }))
                .await;
            });
        }

        // Start MCP config hot-reload watcher.
        // Polls config files every 5 seconds and applies changes dynamically.
        {
            let pool = mcp_pool.clone();
            let project_dir = std::env::current_dir().unwrap_or_default();
            pool.start_config_watcher(project_dir, std::time::Duration::from_secs(5));
        }

        // Wire MCP progress updates to the UI.
        // Progress notifications from MCP servers are forwarded to a channel
        // that the main event loop drains into the multi-progress widget.
        let mcp_progress_rx = {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<(String, f64, Option<f64>)>();
            let pool = mcp_pool.clone();
            mcp_rt.block_on(async move {
                pool.set_progress_callback(std::sync::Arc::new(
                    move |tool_name, progress, total| {
                        let _ = tx.send((tool_name.to_string(), progress, total));
                    },
                ))
                .await;
            });
            Some(rx)
        };

        // Create permission manager
        let mut permission_manager = PermissionManager::new();

        // Register destructive MCP tools with permission manager
        for name in tool_registry.destructive_tool_names() {
            permission_manager.register_destructive_tool(name);
        }

        // Load permission allow/deny rules from settings files
        load_permission_rules(&mut permission_manager);

        // Create state manager
        let state_manager = StateManager::new();

        // Create query engine with optional memory store
        let base_engine = QueryEngine::with_defaults_arc(
            client,
            tool_registry.clone(),
            permission_manager,
            state_manager,
        )
        .with_plan_mode_active(plan_mode_flag.clone());

        // Initialize memory store at ~/.shannon/memories/
        let mut query_engine = {
            let memory_path = dirs::home_dir()
                .map(|h| h.join(".shannon").join("memories"))
                .unwrap_or_else(|| std::path::PathBuf::from(".shannon/memories"));
            let mut mem_store = shannon_core::MemoryStore::new(memory_path);
            // Load existing memories from disk (ignore errors on first run)
            let _ = mem_store.load();
            base_engine.with_memory(mem_store)
        };

        // Auto-load project instructions (Claude Code compatible hierarchy)
        {
            let cwd = std::env::current_dir().unwrap_or_default();

            // 1. Load full CLAUDE.md hierarchy (global -> project -> parents)
            let mem_manager = shannon_core::project_memory::ProjectMemoryManager::new(cwd.clone());
            if let Ok(merged) = mem_manager.load_merged() {
                if !merged.instructions.is_empty() {
                    let resolved =
                        shannon_core::project_memory::resolve_imports(&merged.instructions, &cwd);
                    query_engine
                        .append_system_prompt(&format!("# Project Instructions\n\n{resolved}"));
                }
                tracing::info!("Loaded {} project memory source(s)", merged.sources.len());
            }

            // 2. Load MEMORY.md index (first 200 lines)
            if let Some(memory_content) = shannon_core::project_memory::load_memory_index(&cwd) {
                query_engine.append_system_prompt(&memory_content);
            }

            // 3. Load .claude/rules/*.md
            if let Some(rules) = shannon_core::project_memory::load_rules(&cwd) {
                query_engine.append_system_prompt(&rules);
            }

            // 4. Load git context (branch, recent commits, status)
            if let Some(git_ctx) = shannon_core::project_instructions::git_context(&cwd) {
                query_engine.append_system_prompt(&git_ctx);
            }

            // 5. Inject available skills list so the LLM knows what slash commands exist
            if !skills_for_llm.is_empty() {
                query_engine.append_system_prompt(&skills_for_llm);
            }

            // 6. Attach ContextInjector for hot-reload + compaction reinjection
            let storage_dir = dirs::home_dir()
                .map(|h| h.join(".shannon"))
                .unwrap_or_else(|| cwd.clone());
            let injector = shannon_core::query_engine::ContextInjector::new(cwd, storage_dir);
            query_engine = query_engine.with_context_injector(injector);
        }

        // Create permission request channel
        let (permission_req_tx, permission_req_rx) = tokio::sync::mpsc::unbounded_channel();

        // Create command registry inside the runtime context so register_sync
        // can access the tokio runtime handle.
        let command_registry = runtime.block_on(async {
            let registry = CommandRegistry::new();
            builtin_commands::register_all(&registry);

            // Register MCP prompts as slash commands: /mcp__{server}__{prompt}
            // Also expose a friendlier alias /{server}:{prompt} per ADR 0002 S5-2.
            for (server, prompt) in &discovered_mcp_prompts {
                let cmd_name = format!("mcp__{}__{}", server, prompt.name);
                let alias = format!("{}:{}", server, prompt.name);
                let arg_hint = if prompt.argument_names.is_empty() {
                    None
                } else {
                    Some(prompt.argument_names.join(", "))
                };
                let prompt_template = format!(
                    "Use the get_mcp_prompt tool to retrieve and execute the '{}' prompt from the '{}' MCP server with these arguments: {{args}}",
                    prompt.name, server
                );
                let command = Command::Prompt(Box::new(PromptCommand {
                    base: CommandBase {
                        name: cmd_name,
                        aliases: vec![alias],
                        description: prompt.description.clone(),
                        has_user_specified_description: false,
                        availability: vec![shannon_commands::CommandAvailability::All],
                        source: shannon_commands::CommandSource::Mcp,
                        is_enabled: true,
                        is_hidden: false,
                        argument_hint: arg_hint,
                        when_to_use: None,
                        version: None,
                        disable_model_invocation: false,
                        user_invocable: true,
                        is_workflow: false,
                        immediate: false,
                        is_sensitive: false,
                        user_facing_name: None,
                    },
                    progress_message: format!("Loading MCP prompt '{}' from '{}'", prompt.name, server),
                    content_length: 0,
                    arg_names: prompt.argument_names.clone(),
                    allowed_tools: vec!["get_mcp_prompt".to_string()],
                    model: None,
                    hooks: HashMap::new(),
                    context: ExecutionContext::Inline,
                    agent: None,
                    paths: Vec::new(),
                    prompt_template: Some(prompt_template),
                }));
                registry.register_sync(command);
            }

            // Discover custom commands from .claude/commands/ and .shannon/commands/
            // Claude Code compatible: .claude/commands/*.md -> /command-name
            // Subdirectories: .claude/commands/project/foo.md -> /project:foo
            {
                let mut custom_command_dirs: Vec<std::path::PathBuf> = Vec::new();

                // Project-level commands
                let cwd = std::env::current_dir().unwrap_or_default();
                custom_command_dirs.push(cwd.join(".claude").join("commands"));
                custom_command_dirs.push(cwd.join(".shannon").join("commands"));

                // User-level commands
                if let Some(home) = dirs::home_dir() {
                    custom_command_dirs.push(home.join(".claude").join("commands"));
                    custom_command_dirs.push(home.join(".shannon").join("commands"));
                }

                // Collect custom commands from all command directories
                let mut custom_commands: Vec<CustomCommandEntry> = Vec::new();
                for dir in &custom_command_dirs {
                    collect_custom_commands(dir, "", &mut custom_commands);
                }
                dedup_custom_commands(&mut custom_commands);

                for entry in &custom_commands {
                    let description = entry.description.clone()
                        .unwrap_or_else(|| format!("Custom command (from {})", entry.path.display()));
                    let arg_names = if entry.arguments.is_empty() {
                        vec!["$ARGUMENTS".to_string()]
                    } else {
                        entry.arguments.clone()
                    };
                    let argument_hint = if entry.arguments.is_empty() {
                        Some("$ARGUMENTS".to_string())
                    } else {
                        Some(entry.arguments.join(" "))
                    };
                    let command = Command::Prompt(Box::new(PromptCommand {
                        base: CommandBase {
                            name: entry.name.clone(),
                            aliases: Vec::new(),
                            description,
                            has_user_specified_description: entry.description.is_some(),
                            availability: vec![shannon_commands::CommandAvailability::All],
                            source: shannon_commands::CommandSource::Builtin,
                            is_enabled: true,
                            is_hidden: false,
                            argument_hint,
                            when_to_use: None,
                            version: None,
                            disable_model_invocation: false,
                            user_invocable: true,
                            is_workflow: false,
                            immediate: false,
                            is_sensitive: false,
                            user_facing_name: None,
                        },
                        progress_message: format!("Running /{}...", entry.name),
                        content_length: entry.template.len(),
                        arg_names,
                        allowed_tools: entry.allowed_tools.clone(),
                        model: entry.model.clone(),
                        hooks: HashMap::new(),
                        context: ExecutionContext::Inline,
                        agent: entry.agent.clone(),
                        paths: Vec::new(),
                        prompt_template: Some(entry.template.clone()),
                    }));
                    registry.register_sync(command);
                }
                if !custom_commands.is_empty() {
                    tracing::info!("Registered {} custom command(s) from .claude/commands/ and .shannon/commands/", custom_commands.len());
                }
            }

            // Load plugins from ~/.shannon/plugins/
            {
                let plugins_dir = dirs::home_dir()
                    .unwrap_or_default()
                    .join(".shannon")
                    .join("plugins");
                let mut plugin_registry = shannon_core::plugin::PluginRegistry::new(plugins_dir);
                if plugin_registry.load_all().await.is_ok() {
                    let enabled = plugin_registry.list_enabled();
                    if !enabled.is_empty() {
                        tracing::info!("Loaded {} plugin(s)", enabled.len());
                        for plugin in &enabled {
                            match plugin.manifest.kind() {
                                Ok(shannon_core::plugin::PluginKind::Tool { transport }) => {
                                    if let Some(command) = transport.command() {
                                        let args = transport.args().to_vec();
                                        match shannon_core::discover_tools(
                                            &plugin.manifest.name,
                                            command,
                                            &args,
                                            &std::collections::HashMap::new(),
                                            None,
                                        ).await {
                                            Ok(result) => {
                                                let tool_count = result.tools.len();
                                                for tool in result.tools {
                                                    if let Err(e) = tool_registry.register(Box::new(tool)) {
                                                        tracing::debug!("Plugin tool registration skipped: {}", e);
                                                    }
                                                }
                                                tracing::info!(
                                                    "Registered {} tool(s) from plugin '{}'",
                                                    tool_count,
                                                    plugin.manifest.name
                                                );
                                            }
                                            Err(e) => {
                                                tracing::warn!("Plugin '{}' tool discovery failed: {e}", plugin.manifest.name);
                                            }
                                        }
                                    }
                                }
                                Ok(shannon_core::plugin::PluginKind::Command { name, description }) => {
                                    let plugin_dir = plugin.path.parent()
                                        .map(|p| p.to_path_buf())
                                        .unwrap_or_default();
                                    let entry_path = plugin_dir.join(&plugin.manifest.entry);
                                    let template = std::fs::read_to_string(&entry_path)
                                        .unwrap_or_else(|_| plugin.manifest.entry.clone());
                                    let cmd = Command::Prompt(Box::new(PromptCommand {
                                        base: CommandBase {
                                            name: format!("plugin:{name}"),
                                            aliases: Vec::new(),
                                            description: description.clone(),
                                            has_user_specified_description: !description.is_empty(),
                                            availability: vec![shannon_commands::CommandAvailability::All],
                                            source: shannon_commands::CommandSource::Plugin,
                                            is_enabled: true,
                                            is_hidden: false,
                                            argument_hint: Some("$ARGUMENTS".to_string()),
                                            when_to_use: None,
                                            version: Some(plugin.manifest.version.clone()),
                                            disable_model_invocation: false,
                                            user_invocable: true,
                                            is_workflow: false,
                                            immediate: false,
                                            is_sensitive: false,
                                            user_facing_name: None,
                                        },
                                        progress_message: format!("Running /{name}..."),
                                        content_length: template.len(),
                                        arg_names: vec!["$ARGUMENTS".to_string()],
                                        allowed_tools: Vec::new(),
                                        model: None,
                                        hooks: HashMap::new(),
                                        context: ExecutionContext::Inline,
                                        agent: None,
                                        paths: Vec::new(),
                                        prompt_template: Some(template),
                                    }));
                                    registry.register_sync(cmd);
                                    tracing::info!("Registered command '/plugin:{}' from plugin '{}'", name, plugin.manifest.name);
                                }
                                Ok(shannon_core::plugin::PluginKind::Skill { trigger, template }) => {
                                    let plugin_dir = plugin.path.parent()
                                        .map(|p| p.to_path_buf())
                                        .unwrap_or_default();
                                    let entry_path = plugin_dir.join(&plugin.manifest.entry);
                                    // Use entry file content if it exists, otherwise use inline template
                                    let prompt_template = if entry_path.exists() {
                                        std::fs::read_to_string(&entry_path).unwrap_or(template.clone())
                                    } else {
                                        template.clone()
                                    };
                                    // Strip leading slash from trigger for command name
                                    let cmd_name = trigger.trim_start_matches('/');
                                    let cmd = Command::Prompt(Box::new(PromptCommand {
                                        base: CommandBase {
                                            name: format!("plugin:{cmd_name}"),
                                            aliases: vec![trigger.clone()],
                                            description: plugin.manifest.description.clone(),
                                            has_user_specified_description: true,
                                            availability: vec![shannon_commands::CommandAvailability::All],
                                            source: shannon_commands::CommandSource::Plugin,
                                            is_enabled: true,
                                            is_hidden: false,
                                            argument_hint: Some("$ARGUMENTS".to_string()),
                                            when_to_use: None,
                                            version: Some(plugin.manifest.version.clone()),
                                            disable_model_invocation: false,
                                            user_invocable: true,
                                            is_workflow: false,
                                            immediate: false,
                                            is_sensitive: false,
                                            user_facing_name: Some(trigger.clone()),
                                        },
                                        progress_message: format!("Running skill /{trigger}…"),
                                        content_length: prompt_template.len(),
                                        arg_names: vec!["$ARGUMENTS".to_string()],
                                        allowed_tools: Vec::new(),
                                        model: None,
                                        hooks: HashMap::new(),
                                        context: ExecutionContext::Inline,
                                        agent: None,
                                        paths: Vec::new(),
                                        prompt_template: Some(prompt_template),
                                    }));
                                    registry.register_sync(cmd);
                                    tracing::info!("Registered skill '/{}' from plugin '{}'", trigger, plugin.manifest.name);
                                }
                                Err(e) => {
                                    tracing::warn!("Plugin '{}' has invalid config: {e}", plugin.manifest.name);
                                }
                            }
                        }
                    }
                }
            }

            registry
        });

        // Wrap the executor in SharedExecutor for concurrent command dispatch
        let shared_executor = {
            use shannon_commands::CommandExecutor;
            SharedExecutor::new(CommandExecutor::new(command_registry.clone()))
        };

        let mut repl = Self {
            events: EventHandler::new(50)?,
            renderer: Renderer::new(),
            chat: ChatWidget::new(1000),
            prompt: PromptWidget::new(),
            state: {
                let mut s = ReplState::default();
                let prefs = preferences::load_preferences();
                if let Some(model) = prefs.model {
                    s.model = Some(model);
                } else if !config_model.is_empty() {
                    s.model = Some(config_model);
                }
                if let Some(provider) = prefs.provider {
                    s.selected_provider = Some(provider);
                }
                if let Some(theme_name) = prefs.theme {
                    if let Some(theme) = Theme::named(&theme_name) {
                        s.theme = theme;
                    }
                }
                s.pending_elicitation_tx = Some(elicitation_tx);
                s.pending_elicitation_rx = Some(elicitation_rx);
                s
            },
            running: false,
            query_engine: Some(query_engine),
            state_manager: StateManager::new(),
            command_registry,
            command_parser: CommandParser::new(),
            shared_executor,
            runtime,
            permission_req_rx,
            permission_req_tx,
            last_session_list: Vec::new(),
            command_history: ReplHistory::new(1000),
            saved_input: String::new(),
            diff_data: DiffData::new(),
            current_turn: 0,
            session_started_at: Some(chrono::Utc::now()),
            output_renderer: ReplRenderer::new(),
            commands_run: 0,
            tools_invoked: 0,
            tab_completion_state: TabCompletionState::default(),
            vim_handler: {
                let mut h = VimHandler::new();
                h.set_mode(VimMode::Insert);
                h
            },
            team_coordinator: shared_coordinator,
            agent_registry: None,
            last_agent_refresh: None,
            coordinator_event_rx: None,
            mcp_pool,
            tool_registry,
            mcp_progress_rx,
            model_routes: Vec::new(),
            checkpoint_manager: shannon_core::CheckpointManager::new(),
            notifier: {
                use shannon_core::notifier::{Cooldown, Notifier};
                let mut n = Notifier::new().with_cooldown(Cooldown::new());
                // Add desktop notifier if available
                if shannon_core::notifier::DesktopNotifier::is_available() {
                    n.add_handler(Box::new(shannon_core::notifier::DesktopNotifier::new()));
                }
                n
            },
            notifications_enabled: false, // Disabled by default; enable via /notify
            webhook_receiver: None,
            instruction_watcher: {
                let cwd = std::env::current_dir().unwrap_or_default();
                if cwd.exists() {
                    Some(shannon_core::project_instructions::InstructionWatcher::new(
                        cwd,
                    ))
                } else {
                    None
                }
            },
            command_watcher: Some(CustomCommandWatcher::new()),
            settings_watcher: Some(SettingsWatcher::new()),
            source_watcher: Some(source_watcher::SourceWatcher::new(
                std::env::current_dir().unwrap_or_default(),
            )),
            streaming_cache: Some(streaming_cache),
            diagnostic_pending: std::sync::Arc::new(tokio::sync::Mutex::new(false)),
            diagnostic_rx: None,
            update_check_rx: None,
            session_recovery: shannon_core::SessionRecovery::new().unwrap_or_default(),
            plan_mode_flag: plan_mode_flag.clone(),
            session_recorder: None,
        };

        // Pre-query Ollama model info so context_window is correct from the start
        if let Some(ref mut engine) = repl.query_engine {
            repl.runtime.block_on(engine.pre_resolve_context());
        }
        // Sync context window from engine (handles Ollama models with custom num_ctx)
        if let Some(ref engine) = repl.query_engine {
            repl.state.context_window = engine.resolved_context_window();
        }

        repl.sync_approval_mode_label();
        repl.state
            .spinner
            .set_static_mode(repl.state.reduced_motion);
        repl.renderer.set_theme(&repl.state.theme);
        repl.output_renderer.syntect_theme_name = repl.state.theme.syntect_theme_name().to_string();
        Ok(repl)
    }

    /// Run the main REPL loop
    pub fn run(&mut self) -> Result<()> {
        // Check for interactive terminal
        if !atty::is(atty::Stream::Stdout) || !atty::is(atty::Stream::Stdin) {
            // Stdin pipe mode: read input and process as a single query
            return self.run_pipe_mode();
        }

        // Setup terminal
        enable_raw_mode()?;

        // Panic-safety guard: ensure terminal is restored even if we panic.
        let _cleanup_guard = {
            struct TerminalGuard;
            impl Drop for TerminalGuard {
                fn drop(&mut self) {
                    let mut stdout = io::stdout();
                    let _ = crossterm::execute!(stdout, crossterm::event::DisableBracketedPaste);
                    let _ = crossterm::execute!(stdout, crossterm::cursor::Show);
                    let _ = disable_raw_mode();
                }
            }
            TerminalGuard
        };

        let mut stdout = io::stdout();
        // Enable bracketed paste mode for proper multi-line paste handling
        execute!(stdout, crossterm::event::EnableBracketedPaste)?;

        let backend = CrosstermBackend::new(stdout);
        let term_size = crossterm::terminal::size().unwrap_or((80, 24));
        let viewport_h = term_size.1.saturating_sub(2).max(6);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(viewport_h),
            },
        )?;

        self.running = true;

        // Detect initial git branch for status bar display
        self.refresh_git_branch();

        // Load persistent command history from ~/.shannon/history.jsonl
        {
            let history_path = dirs::home_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join(".shannon")
                .join("history.jsonl");
            self.command_history = ReplHistory::load_from_file(&history_path, 500);
        }

        // Restore persisted UI state from previous session
        self.load_ui_state();

        // Fire SessionStart hooks (Claude Code compatible lifecycle)
        if let Some(ref engine) = self.query_engine {
            let session_id = engine.session_id().to_string();
            let hook_mgr = engine.hook_manager();
            let event = shannon_core::hooks::HookEvent::SessionStart { session_id };
            self.runtime.block_on(async {
                let mgr = hook_mgr.read().await;
                if let Err(e) = mgr.run_hooks(&event).await {
                    tracing::debug!("SessionStart hook error: {e}");
                }
            });
        }

        // Check for updates in background to avoid blocking startup
        {
            let (tx, rx) = std::sync::mpsc::channel::<String>();
            self.update_check_rx = Some(std::sync::Mutex::new(rx));
            let config = shannon_core::updater::UpdaterConfig {
                repo: "shannon-code/shannon".to_string(),
                check_interval: std::time::Duration::from_secs(86400),
                enabled: true,
                include_prereleases: false,
            };
            std::thread::spawn(move || {
                let mut updater = shannon_core::updater::AutoUpdater::new(config);
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .ok();
                let status = rt
                    .as_ref()
                    .map(|rt| rt.block_on(updater.check_for_update()));
                if let Some(shannon_core::updater::UpdateStatus::UpdateAvailable {
                    current,
                    latest,
                    release,
                }) = status
                {
                    let msg = format!(
                        "Update available: {} → {} ({}). Download: {}",
                        current, latest, release.tag_name, release.html_url
                    );
                    let _ = tx.send(msg);
                }
            });
        }

        // Auto-restore the most recent session if it was active within the last 2 hours.
        self.auto_restore_last_session();

        // Main event loop
        while self.running {
            // Poll deferred update check result
            let update_msg = self
                .update_check_rx
                .as_ref()
                .and_then(|rx| rx.lock().ok().and_then(|guard| guard.try_recv().ok()));
            if let Some(msg) = update_msg {
                self.chat.add_message(ChatRole::System, msg);
                self.update_check_rx = None;
            }

            // Check for permission requests (non-blocking)
            // Defer showing permission dialogs during streaming to avoid interrupting reading
            if self.state.permission_dialog.is_none() && !self.state.streaming_active {
                if let Ok(permission_req) = self.permission_req_rx.try_recv() {
                    // Store the permission prompt and response channel
                    self.state.permission_dialog = Some(permission_req.prompt.clone());
                    self.state.permission_response_tx = Some(permission_req.response_tx);

                    // T2: fire an informational notification when a permission
                    // prompt becomes visible. Default is info-only (no inline
                    // approve button) so the user must return to the terminal
                    // to act — this is a security trade-off documented in the
                    // notifications roadmap. 10s cooldown per tool so repeated
                    // prompts within a short window coalesce.
                    if self.notifications_enabled {
                        let tool_name = permission_req.prompt.tool_name.clone();
                        let body: String = permission_req
                            .prompt
                            .description
                            .lines()
                            .take(3)
                            .collect::<Vec<_>>()
                            .join("\n");
                        let notification = shannon_core::notifier::Notification {
                            title: format!("Permission required: {tool_name}"),
                            body,
                            level: shannon_core::notifier::NotificationLevel::Warning,
                            id: format!("permission-request-{tool_name}"),
                            timestamp: chrono::Utc::now(),
                            source: Some(format!("permission:request:{tool_name}")),
                            action_id: None,
                        };
                        let _ = self.notifier.notify_dedup(&notification, 10_000);
                    }

                    // Also populate the tool approval widget for enhanced display
                    let risk = match permission_req.prompt.risk_level {
                        shannon_core::permissions::RiskLevel::Safe
                        | shannon_core::permissions::RiskLevel::Low => {
                            crate::widgets::tool_approval::RiskLevel::Low
                        }
                        shannon_core::permissions::RiskLevel::Medium => {
                            crate::widgets::tool_approval::RiskLevel::Medium
                        }
                        shannon_core::permissions::RiskLevel::High
                        | shannon_core::permissions::RiskLevel::Critical => {
                            crate::widgets::tool_approval::RiskLevel::High
                        }
                    };
                    // Extract domain/URL from tool input for network tools
                    let domain = extract_domain_from_tool(
                        &permission_req.prompt.tool_name,
                        &permission_req.prompt.tool_input,
                    );
                    self.state.tool_approval.show_request(
                        crate::widgets::tool_approval::ToolApprovalRequest {
                            tool_name: permission_req.prompt.tool_name.clone(),
                            description: permission_req.prompt.description.clone(),
                            risk_level: risk,
                            detail: None,
                            domain,
                        },
                        permission_req.prompt.diff_preview.clone(),
                    );
                }
            }

            // Drain MCP progress updates into the multi-progress widget
            if let Some(ref mut rx) = self.mcp_progress_rx {
                let mut had_updates = false;
                while let Ok((tool_name, progress, total)) = rx.try_recv() {
                    if !had_updates {
                        self.state.multi_progress_visible = true;
                        had_updates = true;
                    }
                    let pct = if let Some(t) = total {
                        if t > 0.0 {
                            (progress / t).clamp(0.0, 1.0)
                        } else {
                            progress.clamp(0.0, 1.0)
                        }
                    } else {
                        progress.clamp(0.0, 1.0)
                    };
                    self.state.multi_progress.add_or_update(
                        &tool_name,
                        pct,
                        self.state.theme.accent,
                    );
                }
            }

            // Refresh agent states for sidebar display
            if self.agent_registry.is_some() {
                self.refresh_agents();
            }

            // Drain coordinator events into agent dashboard
            if let Some(ref mut rx) = self.coordinator_event_rx {
                while let Ok(event) = rx.try_recv() {
                    if let Some(ref mut dashboard) = self.state.agent_dashboard {
                        dashboard.handle_coordinator_event(&event);
                    }
                }
            }

            // Auto-create dashboard when agents appear
            if !self.state.active_agents.is_empty() && self.state.agent_dashboard.is_none() {
                let mut dashboard = crate::widgets::agent_bar::AgentDashboardState::new();
                dashboard.sync_from_agents(&self.state.active_agents);
                self.state.agent_dashboard = Some(dashboard);
            }
            // Sync dashboard entries from current agent state
            if let Some(ref mut dashboard) = self.state.agent_dashboard {
                dashboard.sync_from_agents(&self.state.active_agents);
            }
            // Fetch task board summary for the dashboard (P0-2: task ratio)
            if let Some(ref coordinator) = self.team_coordinator {
                if let Some(ref mut dashboard) = self.state.agent_dashboard {
                    let task_board = coordinator.task_board();
                    let summary = self.runtime.block_on(task_board.summary());
                    dashboard.task_summary = Some(summary);
                }
            }
            // Auto-remove dashboard when no agents (but keep if expanded)
            if self.state.active_agents.is_empty() {
                if let Some(ref dashboard) = self.state.agent_dashboard {
                    if !dashboard.expanded {
                        self.state.agent_dashboard = None;
                    }
                }
            }

            // Check custom command files for filesystem changes (notify-based)
            self.check_reload_commands();

            // Check settings files for changes
            self.check_reload_settings();

            // Check source file changes (for diagnostic triggering)
            let source_changes = self.check_source_changes();
            if !source_changes.is_empty() {
                let count = source_changes.len();
                let preview = source_changes
                    .iter()
                    .take(3)
                    .map(|p| p.rsplit('/').next().unwrap_or(p))
                    .collect::<Vec<_>>()
                    .join(", ");
                let suffix = if count > 3 {
                    &format!(" +{} more", count - 3)
                } else {
                    ""
                };
                self.chat.add_message(
                    crate::widgets::ChatRole::System,
                    format!("[Source changed: {preview}{suffix}]"),
                );
                self.state.diagnostic_store.mark_stale();

                // Invalidate streaming cache entries for changed files
                if let Some(ref cache) = self.streaming_cache {
                    for path in &source_changes {
                        cache.invalidate_path(path);
                    }
                }
            }

            // Trigger background diagnostic refresh when stale
            if self.state.diagnostic_store.is_stale() {
                if let Some(ref mut rx) = self.diagnostic_rx {
                    // Check if a previous run completed
                    if let Some(result) = diagnostic_watcher::try_receive(rx) {
                        let count = self.state.diagnostic_store.update_from_cli(&result);
                        if count > 0 {
                            self.chat.add_message(
                                ChatRole::System,
                                format!("[Diagnostics: {count} issue(s) found]"),
                            );
                        } else {
                            self.chat.add_message(
                                ChatRole::System,
                                "[Diagnostics: ✓ No issues]".to_string(),
                            );
                        }
                        self.diagnostic_rx = None;
                    }
                } else {
                    // Spawn a new diagnostic run
                    let project_dir = std::path::PathBuf::from(&self.state.working_directory);
                    if let Some(rx) = diagnostic_watcher::spawn_diagnostic_run(
                        project_dir,
                        self.diagnostic_pending.clone(),
                    ) {
                        self.diagnostic_rx = Some(rx);
                    }
                }
            }

            // Check scheduled routines and inject due prompts
            let due = self.state.routine_manager.drain_due();
            for (name, prompt) in due {
                self.chat
                    .add_message(ChatRole::System, format!("[Routine: {name}] {prompt}"));
            }

            // Check cron-based scheduled tasks and inject due prompts
            if std::env::var("SHANNON_DISABLE_CRON").is_err() {
                let cron_due = self.state.cron_tool.drain_due();
                for job in cron_due {
                    let overdue = if job.was_overdue { " (catch-up)" } else { "" };
                    let next = match &job.next_run {
                        Some(n) => format!(" — next: {n}"),
                        None => String::new(),
                    };
                    self.chat.add_message(
                        ChatRole::System,
                        format!("[Scheduled: {:.8}]{overdue}{next} {}", job.id, job.prompt),
                    );
                }
            }

            render::draw_frame(&mut terminal, self)?;

            // Handle events
            if let Some(event) = self.events.next()? {
                self.handle_event(event, Some(&mut terminal));
            }
        }

        // Save command history to ~/.shannon/history.jsonl
        {
            let history_path = dirs::home_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join(".shannon")
                .join("history.jsonl");
            self.command_history.save_to_file(&history_path);
        }

        // Fire SessionEnd hooks before shutting down
        if let Some(ref engine) = self.query_engine {
            let session_id = engine.session_id().to_string();
            let hook_mgr = engine.hook_manager();
            let event = shannon_core::hooks::HookEvent::SessionEnd { session_id };
            self.runtime.block_on(async {
                let mgr = hook_mgr.read().await;
                if let Err(e) = mgr.run_hooks(&event).await {
                    tracing::debug!("SessionEnd hook error: {e}");
                }
            });

            // Auto-save session for --resume support
            if self.current_turn > 0 {
                let messages = engine.conversation_history();
                let metadata = shannon_core::state::SessionPersistMetadata {
                    model: self.state.model.clone().unwrap_or_default(),
                    created_at: self.session_started_at.unwrap_or_else(chrono::Utc::now),
                    updated_at: chrono::Utc::now(),
                    total_input_tokens: self.state.tokens_used,
                    total_output_tokens: 0,
                    turn_count: messages.iter().filter(|m| m.role == "user").count(),
                    title: None,
                    parent_session_id: None,
                    branch_point_message_index: None,
                };
                if let Err(e) =
                    self.state_manager
                        .save_session(&engine.session_id(), &messages, &metadata)
                {
                    tracing::debug!("Auto-save session error: {e}");
                }
            }
        }

        // Persist UI state for next session
        self.save_ui_state();

        // Restore terminal — disable bracketed-paste BEFORE raw mode to prevent escape leakage
        execute!(
            terminal.backend_mut(),
            crossterm::event::DisableBracketedPaste
        )?;
        terminal.show_cursor()?;
        // Flush all escape sequences while still in raw mode, then disable.
        IoWrite::flush(terminal.backend_mut())?;
        disable_raw_mode()?;

        // Print session cost summary to stdout after terminal is restored
        if let Some(ref engine) = self.query_engine {
            if let Ok(tracker) = engine.cost_tracker().read() {
                let total_cost = tracker.total_cost();
                if tracker.total_input_tokens > 0 {
                    println!();
                    println!("── Session Summary ──");
                    println!(
                        "  Tokens: {} in + {} out  |  Cost: ${total_cost:.4}",
                        tracker.total_input_tokens, tracker.total_output_tokens
                    );
                    if let Some(budget) = tracker.budget_limit_usd {
                        let pct = (total_cost / budget) * 100.0;
                        println!("  Budget: ${total_cost:.4} / ${budget:.2} ({pct:.0}%)");
                    }
                    println!("  Model: {}", tracker.model_name);
                    if let Some(started) = &self.session_started_at {
                        let elapsed = chrono::Utc::now() - *started;
                        let mins = elapsed.num_minutes();
                        let secs = elapsed.num_seconds() % 60;
                        println!("  Duration: {mins}m {secs}s");
                    }
                    println!("─────────────────────");
                }
            }
        }

        Ok(())
    }

    /// Handle individual events
    fn handle_event(&mut self, event: crate::events::Event, terminal: Option<&mut query::Term>) {
        match event {
            crate::events::Event::Input(key) => {
                if let Err(e) = input::handle_input(self, key, terminal) {
                    // Display error in UI chat instead of stderr to prevent escape sequence leakage
                    self.chat
                        .add_message(ChatRole::System, format!("Input error: {e}"));
                }
            }
            crate::events::Event::Paste(content) => {
                let line_count = content.lines().count();
                if line_count > PASTE_THRESHOLD_LINES {
                    self.state.paste_counter += 1;
                    let num = self.state.paste_counter;
                    self.state.pasted_texts.insert(num, content);
                    let display = format!("[Pasted Text #{num} {line_count} lines]");
                    self.prompt.insert_text(&display);
                } else {
                    self.prompt.insert_text(&content);
                }
                self.state.completion_suggestions.clear();
            }
            crate::events::Event::Mouse(mouse) => {
                if self.state.mouse_capture_enabled {
                    input::handle_mouse(self, mouse);
                }
            }
            crate::events::Event::Tick => {
                // Advance spinner animation during query processing
                if self.state.status != "Ready" {
                    // Update streaming state for status indicator
                    self.state.streaming_state = if self.state.thinking_phase {
                        StreamingState::Thinking
                    } else if self.state.streaming_active {
                        StreamingState::Generating {
                            elapsed_secs: self
                                .state
                                .streaming_start
                                .map(|t| t.elapsed().as_secs())
                                .unwrap_or(0),
                        }
                    } else if let Some(ref tool) = self.state.active_tool {
                        StreamingState::CallingTool { name: tool.clone() }
                    } else {
                        StreamingState::Idle
                    };

                    // Set phase based on current state for diverse animation
                    let phase = if self.state.thinking_phase {
                        crate::widgets::progress::SpinnerPhase::Thinking
                    } else if self.state.streaming_active {
                        crate::widgets::progress::SpinnerPhase::Streaming
                    } else if self.state.active_tool.is_some() {
                        crate::widgets::progress::SpinnerPhase::Tool
                    } else {
                        crate::widgets::progress::SpinnerPhase::Default
                    };
                    self.state.spinner.set_phase(phase);
                    self.state.spinner.tick();
                }

                // Auto-dismiss toast after 5 seconds
                if let Some((_, started)) = self.state.toast {
                    if started.elapsed().as_secs() >= 5 {
                        self.state.toast = None;
                    }
                }

                // Refresh custom statusline (throttled internally)
                self.refresh_statusline();

                // Drain pending MCP elicitations: if no dialog is open and a
                // request is queued, surface it as an InputDialog. The dialog
                // submits/cancels via action "__elicit__" which is handled in
                // input.rs (handle_input_dialog_input).
                if self.state.input_dialog.is_none() && self.state.active_elicitation.is_none() {
                    if let Some(rx) = self.state.pending_elicitation_rx.as_mut() {
                        if let Ok(req) = rx.try_recv() {
                            use crate::widgets::dialog::InputDialog;
                            // Stronger visual distinction: MCP servers are
                            // untrusted third-party code, so label them as
                            // EXTERNAL rather than blending in with system
                            // dialogs. Include the originating server name
                            // (passed through the elicitation provider) and
                            // cap message length to keep the title bar readable.
                            const MAX_MSG_LEN: usize = 200;
                            let truncated = if req.message.chars().count() > MAX_MSG_LEN {
                                let mut s: String = req.message.chars().take(MAX_MSG_LEN).collect();
                                s.push('…');
                                s
                            } else {
                                req.message.clone()
                            };
                            let title =
                                format!("[EXTERNAL MCP · {}] {}", req.server_name, truncated);
                            let placeholder = req
                                .placeholder
                                .clone()
                                .unwrap_or_else(|| "Type response...".to_string());
                            let dlg = InputDialog::new(title).with_placeholder(placeholder);
                            self.state.input_dialog = Some(Box::new(dlg));
                            self.state.input_dialog_action = Some("__elicit__".to_string());
                            self.state.active_elicitation = Some(req);
                        }
                    }
                }
            }
            crate::events::Event::Resize(_cols, _rows) => {
                // Reflow committed scrollback if terminal width changed
                let width = _cols;
                if self.chat.needs_reflow(width) {
                    let (lines, _height) = self.chat.re_render_committed(width, &self.state.theme);
                    if !lines.is_empty() {
                        self.chat.pending_scrollback = lines;
                    }
                }
                // Force cells to recompute height on next render
                self.chat.invalidate_all_cells();
            }
        }
    }

    /// Run in pipe mode: read stdin, process as a single query, output result.
    fn run_pipe_mode(&mut self) -> Result<()> {
        use std::io::Read;
        let mut input = String::new();
        std::io::stdin().read_to_string(&mut input)?;
        let input = input.trim().to_string();

        if input.is_empty() {
            return Err("No input provided on stdin.".into());
        }

        // Process the input as a query (no TUI needed)
        self.chat.add_message(ChatRole::User, input.clone());

        if input.starts_with('/') {
            // Handle commands in pipe mode
            commands::submit_input(self, None)?;
            // Output last system/assistant message
            if let Some(msg) = self.chat.last_message() {
                println!("{}", msg.content);
            }
        } else {
            // Process as AI query
            query::handle_query(self, &input, &mut None)?;
            // Output the assistant response
            if let Some(msg) = self.chat.last_message() {
                if msg.role == ChatRole::Assistant {
                    println!("{}", msg.content);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default, clippy::overly_complex_bool_expr)]
mod tests;
