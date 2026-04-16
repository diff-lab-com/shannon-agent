//! REPL main loop and terminal management

mod commands;
mod input;
mod query;
mod render;

use crate::{
    events::EventHandler,
    render::Renderer,
    repl_enhancement::{DiffData, ReplHistory, ReplRenderer},
    vim::VimHandler,
    widgets::{
        ChatWidget, ChatRole, PromptWidget,
        progress::{ProgressBarWidget, SpinnerWidget, MultiProgressWidget},
    },
    Result,
};
use rust_i18n::t;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    Terminal,
};
use std::io;
use tokio::runtime::Runtime;


// Import core functionality
use shannon_core::{
    api::LlmClientConfig,
    permissions::PermissionManager,
    plugin_tool::register_plugin_tools,
    plugins::PluginManager,
    query_engine::QueryEngine,
    state::StateManager,
    tools::ToolRegistry,
};
use shannon_commands::{CommandRegistry, CommandParser, builtin_commands, SharedExecutor};

// Tool registration
use shannon_tools::register_default_tools;
use crate::skill_bridge::register_skills_as_tools;

/// Application state for the REPL
#[derive(Debug, Clone)]
pub struct ReplState {
    /// Current status message
    pub status: String,
    /// Model name being used
    pub model: Option<String>,
    /// Total tokens used
    pub tokens_used: u64,
    /// Total cost in USD accumulated across all queries
    pub total_cost_usd: f64,
    /// Working directory for the session
    pub working_directory: String,
    /// Welcome screen active
    pub welcome_active: bool,
    /// Active permission dialog (if any)
    pub permission_dialog: Option<shannon_core::permissions::PermissionPrompt>,
    /// Permission response channel sender (if dialog is active)
    pub permission_response_tx: Option<tokio::sync::mpsc::UnboundedSender<shannon_core::permissions::PermissionChoice>>,
    /// Active confirm/alert dialog (if any)
    pub active_dialog: Option<crate::widgets::dialog::DialogWidget>,
    /// Pending action to execute when dialog is confirmed
    pub pending_dialog_action: Option<String>,
    /// Currently active tool name (for progress display)
    pub active_tool: Option<String>,
    /// Spinner widget for progress indication
    pub spinner: SpinnerWidget,
    /// Progress bar widget for tool execution progress
    pub progress_bar: ProgressBarWidget,
    /// Whether the progress bar is currently visible (tool is executing)
    pub progress_bar_visible: bool,
    /// Number of steps completed in current query
    pub query_steps_done: usize,
    /// Total steps estimated for current query (0 = indeterminate)
    pub query_steps_total: usize,
    /// Active input dialog (if any)
    pub input_dialog: Option<Box<crate::widgets::dialog::InputDialog>>,
    /// Callback action when input dialog is submitted
    pub input_dialog_action: Option<String>,
    /// Active fuzzy picker for command palette (Ctrl+P)
    pub fuzzy_picker: Option<crate::widgets::select::FuzzyPickerWidget>,
    /// Active file selector for /browse command
    pub file_selector: Option<crate::widgets::select::FileSelectorWidget>,
    /// Multi-progress widget for tracking parallel tool execution
    pub multi_progress: MultiProgressWidget,
    /// Whether multi-progress is visible (tools running in parallel)
    pub multi_progress_visible: bool,
    /// Active multi-select widget (e.g., for /select-tools)
    pub multi_select: Option<crate::widgets::select::MultiSelectWidget>,
    /// Current completion suggestions to display (populated by Tab, cleared by typing)
    pub completion_suggestions: Vec<String>,
    /// Index of the currently highlighted completion suggestion
    pub completion_suggestion_index: usize,
    /// Plan mode state
    pub plan: PlanState,
    /// Execution sandbox mode (direct or Docker isolation)
    pub sandbox_mode: shannon_tools::SandboxMode,
    /// Whether incremental reverse search (Ctrl+R) is active
    pub incremental_search_active: bool,
    /// Current search query for incremental search
    pub incremental_search_query: String,
    /// Match index within search results
    pub incremental_search_match_index: usize,
    /// Input saved before entering incremental search (restored on cancel)
    pub incremental_search_saved_input: String,
}

/// State for plan mode
#[derive(Debug, Clone, Default)]
pub struct PlanState {
    /// Whether plan mode is active
    pub active: bool,
    /// The plan content (markdown steps)
    pub content: String,
    /// Plan description (what user wants to accomplish)
    pub description: String,
    /// Whether the plan has been approved
    pub approved: bool,
}

impl Default for ReplState {
    fn default() -> Self {
        // Get current working directory
        let cwd = std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| ".".to_string());

        Self {
            status: "Ready".to_string(),
            model: Some("claude-3-5-sonnet".to_string()),
            tokens_used: 0,
            total_cost_usd: 0.0,
            working_directory: cwd,
            welcome_active: false,
            permission_dialog: None,
            permission_response_tx: None,
            active_dialog: None,
            pending_dialog_action: None,
            active_tool: None,
            spinner: SpinnerWidget::new(),
            progress_bar: ProgressBarWidget::new(),
            progress_bar_visible: false,
            query_steps_done: 0,
            query_steps_total: 0,
            input_dialog: None,
            input_dialog_action: None,
            fuzzy_picker: None,
            file_selector: None,
            multi_progress: MultiProgressWidget::new(),
            multi_progress_visible: false,
            multi_select: None,
            completion_suggestions: Vec::new(),
            completion_suggestion_index: 0,
            plan: PlanState::default(),
            sandbox_mode: shannon_tools::SandboxMode::Direct,
            incremental_search_active: false,
            incremental_search_query: String::new(),
            incremental_search_match_index: 0,
            incremental_search_saved_input: String::new(),
        }
    }
}

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
    pub(crate) permission_req_rx: tokio::sync::mpsc::UnboundedReceiver<shannon_core::query_engine::PermissionRequest>,
    /// Permission request sender (from REPL to QueryEngine)
    pub(crate) permission_req_tx: tokio::sync::mpsc::UnboundedSender<shannon_core::query_engine::PermissionRequest>,
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
    /// Plugin manager for discovering, loading, and managing plugins
    pub(crate) plugin_manager: PluginManager,
    /// Vim key handler for vim mode support (yy/yw/p yank/paste)
    pub(crate) vim_handler: VimHandler,
    /// Multi-agent team coordinator (lazy-initialized on /team create)
    pub(crate) team_coordinator: Option<shannon_agents::AgentCoordinator>,
    /// Sub-agent registry for background agent management
    pub(crate) agent_registry: Option<std::sync::Arc<shannon_agents::SubAgentRegistry>>,
    /// Model routing rules: (pattern, model_name) pairs
    pub(crate) model_routes: Vec<(String, String)>,
    /// Checkpoint manager for undo/revert operations
    pub(crate) checkpoint_manager: shannon_core::CheckpointManager,
    /// Desktop notification dispatcher
    pub(crate) notifier: shannon_core::notifier::Notifier,
    /// Whether desktop notifications are enabled
    pub(crate) notifications_enabled: bool,
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

impl Repl {
    /// Create a new REPL instance
    pub fn new() -> Result<Self> {
        let runtime = Runtime::new()?;

        // Create tool registry and register all tools
        let mut tool_registry = ToolRegistry::new();
        let agent_context_handle = register_default_tools(&mut tool_registry).map_err(|e| anyhow::anyhow!("Failed to register tools: {e}"))?;

        // Load and register skills from shannon-skills as tools
        register_skills_as_tools(&mut tool_registry);

        // Discover and load plugins, register their tools
        let mut plugin_manager = PluginManager::new();
        let plugin_results = runtime.block_on(plugin_manager.discover_and_load_all());
        match &plugin_results {
            Ok(loaded) if !loaded.is_empty() => {
                tracing::info!("Loaded {} plugin(s): {:?}", loaded.len(), loaded);
            }
            Ok(_) => {
                tracing::debug!("No plugins found");
            }
            Err(e) => {
                tracing::warn!("Plugin discovery failed: {}", e);
            }
        }
        register_plugin_tools(&plugin_manager, &mut tool_registry);

        // Discover MCP server configurations from ~/.shannon/mcp_servers.json
        {
            let mut mcp_registry = shannon_core::mcp_advanced::McpServerRegistry::new();
            let mcp_count = mcp_registry.load_from_default_paths();
            if mcp_count > 0 {
                tracing::info!("Discovered {} MCP server configuration(s)", mcp_count);
                // Register discovered servers' tools as McpTool adapters
                for config in mcp_registry.enabled_servers() {
                    let tool_name = format!("mcp_{}", config.name);
                    let description = format!(
                        "Execute tool calls on MCP server '{}' ({})",
                        config.name, config.transport_type
                    );
                    let input_schema = serde_json::json!({
                        "type": "object",
                        "properties": {
                            "tool_name": {
                                "type": "string",
                                "description": "Name of the tool to call on the MCP server"
                            },
                            "arguments": {
                                "type": "object",
                                "description": "Arguments to pass to the MCP tool"
                            }
                        },
                        "required": ["tool_name"]
                    });
                    let mcp_tool = shannon_core::mcp_tool_adapter::McpToolAdapter::new(
                        config.name.clone(),
                        config.command.clone(),
                        config.args.clone(),
                        config.env.clone(),
                        description,
                        input_schema,
                    );
                    if let Err(e) = tool_registry.register(Box::new(mcp_tool)) {
                        tracing::debug!("MCP tool registration skipped: {}", e);
                    } else {
                        tracing::info!("Registered MCP tool: {}", tool_name);
                    }
                }
            }
        }

        // Create LLM client
        let client_config = LlmClientConfig::default();

        // Inject client config into AgentTool for sub-agent execution
        if let Ok(mut guard) = agent_context_handle.lock() {
            *guard = Some(shannon_tools::AgentToolContext {
                client_config: client_config.clone(),
            });
        }

        // Validate config and show warning if not fully configured
        if let Err(e) = client_config.validate() {
            eprintln!("Warning: {e}");
        }
        tracing::info!("LLM config: {}", client_config.describe());

        let client = if client_config.provider.requires_auth() {
            shannon_core::api::LlmClient::new(client_config)
        } else {
            shannon_core::api::LlmClient::new_unauthenticated(client_config)
        };

        // Create permission manager
        let permission_manager = PermissionManager::new();

        // Create state manager
        let state_manager = StateManager::new();

        // Create query engine with optional memory store
        let base_engine = QueryEngine::with_defaults(
            client,
            tool_registry,
            permission_manager,
            state_manager,
        );

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

            // 1. Load full CLAUDE.md hierarchy (global → project → parents)
            let mem_manager = shannon_core::project_memory::ProjectMemoryManager::new(cwd.clone());
            if let Ok(merged) = mem_manager.load_merged() {
                if !merged.instructions.is_empty() {
                    let resolved = shannon_core::project_memory::resolve_imports(
                        &merged.instructions, &cwd,
                    );
                    query_engine.append_system_prompt(&format!(
                        "# Project Instructions\n\n{resolved}"
                    ));
                }
                tracing::info!("Loaded {} project memory source(s)", merged.sources.len());
            }

            // 2. Load MEMORY.md index (first 200 lines)
            if let Some(memory_content) = shannon_core::project_memory::load_memory_index(&cwd) {
                query_engine.append_system_prompt(&memory_content);
            }

            // 3. Load git context (branch, recent commits, status)
            if let Some(git_ctx) = shannon_core::project_instructions::git_context(&cwd) {
                query_engine.append_system_prompt(&git_ctx);
            }
        }

        // Create permission request channel
        let (permission_req_tx, permission_req_rx) = tokio::sync::mpsc::unbounded_channel();

        // Create command registry inside the runtime context so register_sync
        // can access the tokio runtime handle.
        let command_registry = runtime.block_on(async {
            let registry = CommandRegistry::new();
            builtin_commands::register_all(&registry);
            registry
        });

        // Wrap the executor in SharedExecutor for concurrent command dispatch
        let shared_executor = {
            use shannon_commands::CommandExecutor;
            SharedExecutor::new(CommandExecutor::new(command_registry.clone()))
        };

        Ok(Self {
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
            plugin_manager,
            vim_handler: VimHandler::new(),
            team_coordinator: None,
            agent_registry: None,
            model_routes: Vec::new(),
            checkpoint_manager: shannon_core::CheckpointManager::new(),
            notifier: {
                let mut n = shannon_core::notifier::Notifier::new();
                // Add desktop notifier if available
                if shannon_core::notifier::DesktopNotifier::is_available() {
                    n.add_handler(Box::new(shannon_core::notifier::DesktopNotifier::new()));
                }
                n
            },
            notifications_enabled: false, // Disabled by default; enable via /notify
        })
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
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        // Enable bracketed paste mode for proper multi-line paste handling
        execute!(stdout, crossterm::event::EnableBracketedPaste)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        self.running = true;

        // Fire SessionStart hooks (Claude Code compatible lifecycle)
        if let Some(ref engine) = self.query_engine {
            let session_id = engine.session_id().to_string();
            let hook_mgr = engine.hook_manager();
            let event = shannon_core::hooks::HookEvent::SessionStart {
                session_id: session_id.clone(),
            };
            self.runtime.block_on(async {
                let mgr = hook_mgr.read().await;
                if let Err(e) = mgr.run_hooks(&event).await {
                    tracing::debug!("SessionStart hook error: {e}");
                }
            });
        }

        // Show welcome message rendered through the markdown renderer
        let welcome_md = self.renderer.render_markdown(
            &format!("# {}\n\n{}", t!("repl.welcome"), t!("repl.welcome_help"))
        );
        let welcome_text: String = welcome_md.iter()
            .flat_map(|line| line.spans.iter().map(|s| s.content.clone()))
            .collect::<Vec<_>>()
            .join("");
        self.chat.add_message(
            ChatRole::System,
            welcome_text,
        );

        // Check for updates on startup (non-blocking)
        if let Some(update_msg) = self.check_for_updates() {
            self.chat.add_message(ChatRole::System, update_msg);
        }

        // Main event loop
        while self.running {
            // Check for permission requests (non-blocking)
            if self.state.permission_dialog.is_none() {
                if let Ok(permission_req) = self.permission_req_rx.try_recv() {
                    // Store the permission prompt and response channel
                    self.state.permission_dialog = Some(permission_req.prompt.clone());
                    self.state.permission_response_tx = Some(permission_req.response_tx);
                }
            }

            // Draw UI
            render::draw_frame(&mut terminal, self)?;

            // Handle events
            if let Some(event) = self.events.next()? {
                self.handle_event(event);
            }
        }

        // Fire SessionEnd hooks before shutting down
        if let Some(ref engine) = self.query_engine {
            let session_id = engine.session_id().to_string();
            let hook_mgr = engine.hook_manager();
            let event = shannon_core::hooks::HookEvent::SessionEnd {
                session_id: session_id.clone(),
            };
            self.runtime.block_on(async {
                let mgr = hook_mgr.read().await;
                if let Err(e) = mgr.run_hooks(&event).await {
                    tracing::debug!("SessionEnd hook error: {e}");
                }
            });
        }

        // Restore terminal
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            crossterm::event::DisableBracketedPaste
        )?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen
        )?;
        terminal.show_cursor()?;

        // Print session cost summary to stdout after terminal is restored
        if let Some(ref engine) = self.query_engine {
            if let Ok(tracker) = engine.cost_tracker().read() {
                let total_cost = tracker.total_cost();
                if tracker.total_input_tokens > 0 {
                    println!();
                    println!("── Session Summary ──");
                    println!("  Tokens: {} in + {} out  |  Cost: ${total_cost:.4}",
                        tracker.total_input_tokens, tracker.total_output_tokens);
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
    fn handle_event(&mut self, event: crate::events::Event) {
        match event {
            crate::events::Event::Input(key) => {
                if let Err(e) = input::handle_input(self, key) {
                    // Display error in UI chat instead of stderr to prevent escape sequence leakage
                    self.chat.add_message(
                        ChatRole::System,
                        format!("Input error: {e}")
                    );
                }
            }
            crate::events::Event::Paste(content) => {
                // Insert pasted text preserving newlines (bracketed paste)
                self.prompt.insert_text(&content);
                self.state.completion_suggestions.clear();
            }
            crate::events::Event::Tick => {
                // Advance spinner animation during query processing
                if self.state.status != "Ready" {
                    self.state.spinner.tick();
                }
            }
        }
    }

    /// Check for Shannon updates on startup (non-blocking)
    fn check_for_updates(&self) -> Option<String> {
        use shannon_core::updater::{AutoUpdater, UpdaterConfig};
        use std::time::Duration;

        let config = UpdaterConfig {
            repo: "shannon-code/shannon".to_string(),
            check_interval: Duration::from_secs(86400),
            enabled: true,
            include_prereleases: false,
        };
        let mut updater = AutoUpdater::new(config);

        match self.runtime.block_on(updater.check_for_update()) {
            shannon_core::updater::UpdateStatus::UpdateAvailable { current, latest, release } => {
                Some(format!(
                    "Update available: {} → {} ({}). Download: {}",
                    current, latest, release.tag_name, release.html_url
                ))
            }
            shannon_core::updater::UpdateStatus::CheckFailed { error } => {
                // Silently ignore update check failures — don't block startup
                let _ = error;
                None
            }
            _ => None,
        }
    }

    /// Get the current REPL state
    pub fn state(&self) -> &ReplState {
        &self.state
    }

    /// Get mutable reference to the REPL state
    pub fn state_mut(&mut self) -> &mut ReplState {
        &mut self.state
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
            commands::submit_input(self)?;
            // Output last system/assistant message
            if let Some(msg) = self.chat.last_message() {
                println!("{}", msg.content);
            }
        } else {
            // Process as AI query
            query::handle_query(self, &input)?;
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

// ── UiAdapter Implementation for Repl ─────────────────────────────────────

use crate::adapter::{UiAdapter, UiError, UiResult, DisplayMessage};
use async_trait::async_trait;

/// Implement UiAdapter for Repl to allow it to be used as a UI backend.
#[async_trait]
impl UiAdapter for Repl {
    fn supports_streaming(&self) -> bool {
        true // Terminal UI supports streaming output
    }

    async fn display(&self, message: &DisplayMessage) -> UiResult<()> {
        // The TUI event loop handles rendering via the chat widget.
        // This method exists so the Repl satisfies the trait; actual output
        // flows through QueryEvent streams in the main loop.
        let _ = message;
        Ok(())
    }

    async fn display_progress(&self, message: &str, percent: Option<u8>) -> UiResult<()> {
        // Update status with progress message.
        let _ = (message, percent);
        Ok(())
    }

    async fn read_input(&self, prompt: &str) -> UiResult<String> {
        // In the current terminal UI, input is handled by the event loop.
        let _ = prompt;
        Err(UiError::NotSupported(
            "read_input not supported in terminal UI - use the prompt widget instead".to_string(),
        ))
    }

    async fn confirm(&self, message: &str) -> UiResult<bool> {
        // Confirmation is handled through dialog widgets in the event loop.
        let _ = message;
        Err(UiError::NotSupported(
            "confirm not directly supported - use dialog widgets instead".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests;
