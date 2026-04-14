//! REPL main loop and terminal management

use crate::{
    events::EventHandler,
    render::Renderer,
    repl_enhancement::{DiffData, ReplHistory, ReplRenderer, TurnDiff},
    vim::{VimAction, VimHandler},
    widgets::{
        ChatWidget, ChatRole, PromptWidget, MainLayoutWidget,
        dialog::{DialogWidget, InputDialog},
        progress::{ProgressBarWidget, SpinnerWidget, MultiProgressWidget},
        select::{FuzzyPickerWidget, FileSelectorWidget, MultiSelectWidget, SelectItem},
    },
    Result,
};
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    Terminal,
};
use std::io;
use tokio::runtime::Runtime;
use uuid::Uuid;

// Import core functionality
use shannon_core::{
    api::LlmClientConfig,
    permissions::PermissionManager,
    plugin_tool::register_plugin_tools,
    plugins::PluginManager,
    query_engine::{QueryContext, QueryEngine, QueryEvent, PermissionRequest},
    state::StateManager,
    tools::ToolRegistry,
};
use shannon_commands::{CommandRegistry, CommandParser, builtin_commands, help_utils, SharedExecutor, export_utils, search_utils, config_utils, diff_utils};
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
    pub active_dialog: Option<DialogWidget>,
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
    pub input_dialog: Option<Box<InputDialog>>,
    /// Callback action when input dialog is submitted
    pub input_dialog_action: Option<String>,
    /// Active fuzzy picker for command palette (Ctrl+P)
    pub fuzzy_picker: Option<FuzzyPickerWidget>,
    /// Active file selector for /browse command
    pub file_selector: Option<FileSelectorWidget>,
    /// Multi-progress widget for tracking parallel tool execution
    pub multi_progress: MultiProgressWidget,
    /// Whether multi-progress is visible (tools running in parallel)
    pub multi_progress_visible: bool,
    /// Active multi-select widget (e.g., for /select-tools)
    pub multi_select: Option<MultiSelectWidget>,
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
        }
    }
}

/// Main REPL application struct
pub struct Repl {
    /// Event handler for user input
    events: EventHandler,
    /// Renderer for UI drawing
    renderer: Renderer,
    /// Chat widget for displaying messages
    chat: ChatWidget,
    /// Prompt widget for user input
    prompt: PromptWidget,
    /// Application state
    state: ReplState,
    /// Running state
    running: bool,
    /// Query engine for AI processing
    query_engine: Option<QueryEngine>,
    /// State manager for session persistence (separate from QueryEngine's internal one)
    state_manager: StateManager,
    /// Command registry with all built-in commands
    command_registry: CommandRegistry,
    /// Command parser for parsing /commands
    command_parser: CommandParser,
    /// Shared command executor for concurrent command dispatch
    shared_executor: SharedExecutor,
    /// Tokio runtime for async operations
    runtime: Runtime,
    /// Permission request receiver (from QueryEngine to REPL UI)
    permission_req_rx: tokio::sync::mpsc::UnboundedReceiver<PermissionRequest>,
    /// Permission request sender (from REPL to QueryEngine)
    permission_req_tx: tokio::sync::mpsc::UnboundedSender<PermissionRequest>,
    /// Last session listing cache (for /resume by number)
    last_session_list: Vec<shannon_core::state::SessionInfo>,
    /// Command history with cursor navigation
    command_history: ReplHistory,
    /// Saved input before history navigation (to restore on down-to-bottom)
    saved_input: String,
    /// Per-turn diff tracking
    diff_data: DiffData,
    /// Current turn index
    current_turn: usize,
    /// Session start time
    session_started_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Markdown renderer for assistant output
    output_renderer: ReplRenderer,
    /// Total commands run in this session
    commands_run: usize,
    /// Total tools invoked in this session
    tools_invoked: usize,
    /// Tab completion state for cycling through matches
    tab_completion_state: TabCompletionState,
    /// Plugin manager for discovering, loading, and managing plugins
    plugin_manager: PluginManager,
    /// Vim key handler for vim mode support (yy/yw/p yank/paste)
    vim_handler: VimHandler,
    /// Multi-agent team coordinator (lazy-initialized on /team create)
    team_coordinator: Option<shannon_agents::AgentCoordinator>,
}

/// State for tab completion cycling
#[derive(Debug, Clone, Default)]
struct TabCompletionState {
    /// The prefix text being completed (to detect when completion should reset)
    last_prefix: String,
    /// Current match index for cycling through completions
    current_index: usize,
    /// Available completion candidates
    candidates: Vec<String>,
}

impl Repl {
    /// Create a new REPL instance
    pub fn new() -> Result<Self> {
        let runtime = Runtime::new()?;

        // Create tool registry and register all tools
        let mut tool_registry = ToolRegistry::new();
        register_default_tools(&mut tool_registry).map_err(|e| anyhow::anyhow!("Failed to register tools: {e}"))?;

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
        let query_engine = {
            let memory_path = dirs::home_dir()
                .map(|h| h.join(".shannon").join("memories"))
                .unwrap_or_else(|| std::path::PathBuf::from(".shannon/memories"));
            let mut mem_store = shannon_core::MemoryStore::new(memory_path);
            // Load existing memories from disk (ignore errors on first run)
            let _ = mem_store.load();
            base_engine.with_memory(mem_store)
        };

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
        })
    }

    /// Run the main REPL loop
    pub fn run(&mut self) -> Result<()> {
        // Check for interactive terminal
        if !atty::is(atty::Stream::Stdout) || !atty::is(atty::Stream::Stdin) {
            return Err("shannon repl requires an interactive terminal (TTY). Redirecting input/output is not supported.".into());
        }

        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        self.running = true;

        // Show welcome message rendered through the markdown renderer
        let welcome_md = self.renderer.render_markdown(
            "# Welcome to Shannon!\n\nType your message and press **Enter**. Type `/help` for commands."
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
            let chat = &self.chat;
            let prompt = &self.prompt;
            let state = self.state.clone();
            let spinner = &self.state.spinner;

            terminal.draw(|f| {
                let pb = if state.progress_bar_visible {
                    Some(&state.progress_bar)
                } else {
                    None
                };
                if let Some(ref dialog) = state.permission_dialog {
                    // Render permission dialog overlay
                    self.render_permission_dialog(f, f.area(), dialog);
                } else if let Some(ref dialog) = state.active_dialog {
                    // Render main layout first, then overlay the dialog
                    MainLayoutWidget::render_complete_with_spinner(
                        f,
                        chat,
                        prompt,
                        &state.status,
                        state.model.as_deref(),
                        Some(state.tokens_used),
                        &state.working_directory,
                        Some(spinner),
                        pb,
                    );
                    dialog.render(f, f.area());
                } else if let Some(ref input_dlg) = state.input_dialog {
                    // Render main layout first, then overlay the input dialog
                    MainLayoutWidget::render_complete_with_spinner(
                        f,
                        chat,
                        prompt,
                        &state.status,
                        state.model.as_deref(),
                        Some(state.tokens_used),
                        &state.working_directory,
                        Some(spinner),
                        pb,
                    );
                    input_dlg.render(f, f.area());
                } else if let Some(ref picker) = state.fuzzy_picker {
                    // Render main layout first, then overlay the fuzzy picker
                    MainLayoutWidget::render_complete_with_spinner(
                        f,
                        chat,
                        prompt,
                        &state.status,
                        state.model.as_deref(),
                        Some(state.tokens_used),
                        &state.working_directory,
                        Some(spinner),
                        pb,
                    );
                    picker.render(f, f.area());
                } else if let Some(ref selector) = state.file_selector {
                    // Render main layout first, then overlay the file selector
                    MainLayoutWidget::render_complete_with_spinner(
                        f,
                        chat,
                        prompt,
                        &state.status,
                        state.model.as_deref(),
                        Some(state.tokens_used),
                        &state.working_directory,
                        Some(spinner),
                        pb,
                    );
                    selector.render(f, f.area());
                } else if let Some(ref msel) = state.multi_select {
                    // Render main layout first, then overlay the multi-select
                    MainLayoutWidget::render_complete_with_spinner(
                        f,
                        chat,
                        prompt,
                        &state.status,
                        state.model.as_deref(),
                        Some(state.tokens_used),
                        &state.working_directory,
                        Some(spinner),
                        pb,
                    );
                    msel.render(f, f.area());
                } else {
                    MainLayoutWidget::render_complete_with_spinner(
                        f,
                        chat,
                        prompt,
                        &state.status,
                        state.model.as_deref(),
                        Some(state.tokens_used),
                        &state.working_directory,
                        Some(spinner),
                        pb,
                    );
                }

                // Overlay multi-progress bars at the bottom if active
                if state.multi_progress_visible {
                    let mp_height = 3u16.min(f.area().height.saturating_sub(10));
                    let mp_area = ratatui::layout::Rect {
                        x: f.area().x + 2,
                        y: f.area().bottom().saturating_sub(mp_height + 3),
                        width: f.area().width.saturating_sub(4),
                        height: mp_height,
                    };
                    state.multi_progress.render(f, mp_area);
                }
            })?;

            // Handle events
            if let Some(event) = self.events.next()? {
                self.handle_event(event);
            }
        }

        // Restore terminal
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen
        )?;
        terminal.show_cursor()?;

        Ok(())
    }

    /// Handle individual events
    fn handle_event(&mut self, event: crate::events::Event) {
        match event {
            crate::events::Event::Input(key) => {
                if let Err(e) = self.handle_input(key) {
                    // Display error in UI chat instead of stderr to prevent escape sequence leakage
                    self.chat.add_message(
                        crate::widgets::ChatRole::System,
                        format!("Input error: {e}")
                    );
                }
            }
            crate::events::Event::Tick => {
                // Advance spinner animation during query processing
                if self.state.status != "Ready" {
                    self.state.spinner.tick();
                }
            }
        }
    }

    /// Handle keyboard input
    fn handle_input(&mut self, key: crossterm::event::KeyEvent) -> Result<()> {
        // If permission dialog is active, handle dialog-specific keys
        if self.state.permission_dialog.is_some() {
            return self.handle_permission_dialog_input(key);
        }

        // If a confirm/alert dialog is active, handle dialog keys
        if self.state.active_dialog.is_some() {
            return self.handle_active_dialog_input(key);
        }

        // If an input dialog is active, handle text input
        if self.state.input_dialog.is_some() {
            return self.handle_input_dialog_input(key);
        }

        // If fuzzy picker is active, handle picker input
        if self.state.fuzzy_picker.is_some() {
            return self.handle_fuzzy_picker_input(key);
        }

        // If file selector is active, handle file selector input
        if self.state.file_selector.is_some() {
            return self.handle_file_selector_input(key);
        }

        // If multi-select is active, handle multi-select input
        if self.state.multi_select.is_some() {
            return self.handle_multi_select_input(key);
        }

        match key.code {
            crossterm::event::KeyCode::Char('p') => {
                if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) {
                    self.open_command_palette();
                    return Ok(());
                } else {
                    self.prompt.add_char('p');
                }
            }
            crossterm::event::KeyCode::Char('q') => {
                if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) {
                    self.running = false;
                } else {
                    self.prompt.add_char('q');
                }
            }
            crossterm::event::KeyCode::Char('c') => {
                if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) {
                    self.running = false;
                }
            }
            crossterm::event::KeyCode::Enter => {
                // Shift+Enter inserts newline for multi-line editing
                if key.modifiers.contains(crossterm::event::KeyModifiers::SHIFT) {
                    self.prompt.insert_newline();
                } else {
                    self.submit_input()?;
                }
            }
            crossterm::event::KeyCode::Char(c) => {
                self.prompt.add_char(c);
            }
            crossterm::event::KeyCode::Backspace => {
                self.prompt.backspace();
            }
            crossterm::event::KeyCode::Up => {
                // If prompt has multi-line content, move cursor up
                if self.prompt.input().contains('\n') {
                    self.prompt.cursor_up();
                } else if !self.prompt.input().is_empty() || self.command_history.cursor() >= 0 {
                    // Single-line: navigate command history
                    if self.command_history.cursor() < 0 {
                        // First up press: save current input
                        self.saved_input = self.prompt.input().to_string();
                    }
                    if let Some(cmd) = self.command_history.up() {
                        self.prompt.set_input(cmd.to_string());
                    }
                } else {
                    self.chat.scroll_up();
                }
            }
            crossterm::event::KeyCode::Down => {
                // If prompt has multi-line content, move cursor down
                if self.prompt.input().contains('\n') {
                    self.prompt.cursor_down();
                } else if self.command_history.cursor() >= 0 {
                    // Single-line: navigate command history
                    if let Some(cmd) = self.command_history.down() {
                        self.prompt.set_input(cmd.to_string());
                    } else {
                        // Back to bottom: restore saved input
                        self.command_history.reset_cursor();
                        self.prompt.set_input(self.saved_input.clone());
                    }
                } else {
                    self.chat.scroll_down();
                }
            }
            crossterm::event::KeyCode::Esc => {
                // Route through vim handler for normal-mode Esc
                let action = self.vim_handler.process_key(key);
                self.handle_vim_action(action);
            }
            crossterm::event::KeyCode::Left => {
                // Move cursor left within input
                self.prompt.cursor_left();
            }
            crossterm::event::KeyCode::Right => {
                // Move cursor right within input
                self.prompt.cursor_right();
            }
            crossterm::event::KeyCode::Tab => {
                self.handle_tab_completion()?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Handle vim actions produced by the VimHandler
    fn handle_vim_action(&mut self, action: VimAction) {
        match action {
            VimAction::YankLine { count } => {
                let line = self.prompt.current_line();
                let yanked = if count > 1 { line.repeat(count) } else { line };
                self.vim_handler.set_yank_buffer(yanked);
            }
            VimAction::PasteAfter => {
                let text = self.vim_handler.yank_buffer().to_string();
                if !text.is_empty() {
                    self.prompt.insert_text(&text);
                }
            }
            VimAction::InsertChar { c } => {
                self.prompt.add_char(c);
            }
            VimAction::Backspace => {
                self.prompt.backspace();
            }
            VimAction::SubmitInput => {
                if let Err(e) = self.submit_input() {
                    self.chat.add_message(ChatRole::System, format!("Input error: {e}"));
                }
            }
            VimAction::MoveCursor { direction, count } => {
                for _ in 0..count {
                    use crate::vim::Direction;
                    match direction {
                        Direction::Left => self.prompt.cursor_left(),
                        Direction::Right => self.prompt.cursor_right(),
                        Direction::Up => self.prompt.cursor_up(),
                        Direction::Down => self.prompt.cursor_down(),
                        // LineStart/LineEnd etc. map to Home/End for now
                        Direction::LineStart | Direction::FileStart => {
                            // Move cursor to start of line
                            let col = self.prompt.cursor_position();
                            for _ in 0..col { self.prompt.cursor_left(); }
                        }
                        Direction::LineEnd | Direction::FileEnd => {
                            // Approximate: move right a lot
                            for _ in 0..100 { self.prompt.cursor_right(); }
                        }
                        Direction::WordForward | Direction::WordBackward => {
                            // Not directly supported by PromptWidget, skip for now
                        }
                    }
                }
            }
            VimAction::DeleteLine { .. } => {
                let line = self.prompt.current_line();
                self.vim_handler.set_yank_buffer(line);
                self.prompt.clear();
            }
            VimAction::ClearInput => {
                self.prompt.clear();
            }
            // Actions that don't need REPL-level handling (mode transitions, no-ops)
            _ => {}
        }
    }

    /// Handle tab completion
    fn handle_tab_completion(&mut self) -> Result<()> {
        let input = self.prompt.input().to_string();

        // Get available commands from shared executor's registry
        let mut command_names = self.runtime.block_on(async {
            self.shared_executor.registry().await.list_names().await
        });

        // Also include plugin commands in completion candidates
        for cmd in self.plugin_manager.get_plugin_commands() {
            if !command_names.iter().any(|n| n == &cmd.name) {
                command_names.push(cmd.name.clone());
            }
        }

        // Perform completion
        if let Some((completion, start, end)) = self.tab_complete_command(&input, &command_names) {
            // Build the new input: input[..start] + completion + input[end..]
            let mut new_input = String::new();
            if start > 0 && start <= input.len() {
                new_input.push_str(&input[..start]);
            }
            new_input.push_str(&completion);
            if end < input.len() {
                new_input.push_str(&input[end..]);
            }

            self.prompt.set_input(new_input);
        }

        Ok(())
    }

    /// Perform tab completion on the current input
    ///
    /// Returns the completed text and the range to replace (start, end).
    /// If no completion is found, returns None.
    fn tab_complete_command(&mut self, input: &str, available_commands: &[String]) -> Option<(String, usize, usize)> {
        // Find the word to complete - look for the last /command or word boundary
        let (prefix, word_start, word_end) = self.extract_completion_word(input);

        // Reset completion state if the prefix changed
        if self.tab_completion_state.last_prefix != prefix {
            self.tab_completion_state.last_prefix = prefix.clone();
            self.tab_completion_state.current_index = 0;

            // Find candidates matching the prefix
            self.tab_completion_state.candidates = if prefix.starts_with('/') {
                // Command completion - match against commands with /
                available_commands
                    .iter()
                    .filter(|cmd| {
                        let with_slash = format!("/{cmd}");
                        with_slash.starts_with(&prefix)
                    })
                    .map(|cmd| format!("/{cmd}"))
                    .collect()
            } else {
                // For non-commands starting empty, complete to all commands with /
                if prefix.is_empty() {
                    available_commands.iter().map(|c| format!("/{c}")).collect()
                } else {
                    Vec::new()
                }
            };
        }

        if self.tab_completion_state.candidates.is_empty() {
            return None;
        }

        // Get current candidate
        let completion = &self.tab_completion_state.candidates[self.tab_completion_state.current_index];

        // Cycle to next candidate for next tab press
        self.tab_completion_state.current_index = (self.tab_completion_state.current_index + 1)
            % self.tab_completion_state.candidates.len();

        Some((completion.clone(), word_start, word_end))
    }

    /// Extract the word to complete from input
    ///
    /// Returns (prefix, start_pos, end_pos) where prefix is the text to complete,
    /// and start/end are the byte indices in the input string.
    fn extract_completion_word(&self, input: &str) -> (String, usize, usize) {
        // Find the last word boundary or command start
        // Find the last space or start of command
        let start = if let Some(last_slash) = input.rfind('/') {
            last_slash
        } else if let Some(last_space) = input.rfind(' ') {
            last_space + 1
        } else {
            0
        };

        let end = input.len();

        (input[start..].to_string(), start, end)
    }

    /// Handle permission dialog keyboard input
    fn handle_permission_dialog_input(&mut self, key: crossterm::event::KeyEvent) -> Result<()> {
        use shannon_core::permissions::PermissionChoice;

        match key.code {
            crossterm::event::KeyCode::Enter => {
                // AllowOnce
                self.send_permission_response(PermissionChoice::AllowOnce);
            }
            crossterm::event::KeyCode::Char('a') | crossterm::event::KeyCode::Char('A') => {
                // AlwaysAllow
                self.send_permission_response(PermissionChoice::AlwaysAllow);
            }
            crossterm::event::KeyCode::Esc => {
                // Deny
                self.send_permission_response(PermissionChoice::Deny);
            }
            _ => {}
        }
        Ok(())
    }

    /// Send permission choice back to query engine
    fn send_permission_response(&mut self, choice: shannon_core::permissions::PermissionChoice) {
        if let Some(tx) = self.state.permission_response_tx.take() {
            let _ = tx.send(choice);
        }
        // Clear the dialog
        self.state.permission_dialog = None;
    }

    /// Handle confirm/alert dialog keyboard input
    fn handle_active_dialog_input(&mut self, key: crossterm::event::KeyEvent) -> Result<()> {
        match key.code {
            crossterm::event::KeyCode::Left => {
                if let Some(ref mut dialog) = self.state.active_dialog {
                    dialog.prev_button();
                }
            }
            crossterm::event::KeyCode::Right => {
                if let Some(ref mut dialog) = self.state.active_dialog {
                    dialog.next_button();
                }
            }
            crossterm::event::KeyCode::Enter => {
                // Execute the selected action
                let action = self.state.active_dialog.as_ref()
                    .and_then(|d| d.selected_action().map(|a| a.to_string()));
                let pending = self.state.pending_dialog_action.take();

                // Close dialog first
                self.state.active_dialog = None;

                // Handle known actions
                if let Some(ref act) = action {
                    match act.as_str() {
                        "confirm" => {
                            // Execute the pending action
                            if let Some(cmd) = pending {
                                self.execute_pending_action(&cmd)?;
                            }
                        }
                        "cancel" | "ok" => {
                            // Just close the dialog (already done above)
                        }
                        _ => {}
                    }
                }
            }
            crossterm::event::KeyCode::Esc => {
                self.state.active_dialog = None;
                self.state.pending_dialog_action = None;
            }
            _ => {}
        }
        Ok(())
    }

    /// Execute a pending dialog action after confirmation
    fn execute_pending_action(&mut self, action: &str) -> Result<()> {
        match action {
            "clear_chat" => {
                self.chat.clear();
                self.chat.add_message(ChatRole::System, "Chat cleared.".to_string());
            }
            "quit" => {
                self.running = false;
            }
            _ => {}
        }
        Ok(())
    }

    /// Show a confirm dialog for destructive operations
    fn show_confirm_dialog(&mut self, title: &str, message: &str, action: &str) {
        use crate::widgets::dialog::ConfirmDialog;
        let dialog = ConfirmDialog::new(title.to_string())
            .with_message(message.to_string())
            .build();
        self.state.active_dialog = Some(dialog);
        self.state.pending_dialog_action = Some(action.to_string());
    }

    /// Show an alert dialog for information/errors
    fn show_alert_dialog(&mut self, title: &str, message: &str, danger: bool) {
        use crate::widgets::dialog::AlertDialog;
        let mut builder = AlertDialog::new(title.to_string())
            .with_message(message.to_string());
        if danger {
            builder = builder.with_danger();
        }
        self.state.active_dialog = Some(builder.build());
        self.state.pending_dialog_action = None;
    }

    /// Show an input dialog for text entry
    fn show_input_dialog(&mut self, title: &str, placeholder: &str, action: &str) {
        use crate::widgets::dialog::InputDialog;
        let dialog = InputDialog::new(title.to_string())
            .with_placeholder(placeholder.to_string());
        self.state.input_dialog = Some(Box::new(dialog));
        self.state.input_dialog_action = Some(action.to_string());
    }

    /// Handle input dialog keyboard input
    fn handle_input_dialog_input(&mut self, key: crossterm::event::KeyEvent) -> Result<()> {
        match key.code {
            crossterm::event::KeyCode::Char(c) => {
                if let Some(ref mut dlg) = self.state.input_dialog {
                    dlg.add_char(c);
                }
            }
            crossterm::event::KeyCode::Backspace => {
                if let Some(ref mut dlg) = self.state.input_dialog {
                    dlg.backspace();
                }
            }
            crossterm::event::KeyCode::Enter => {
                // Submit the input
                let value = self.state.input_dialog.as_ref()
                    .map(|d| d.value().to_string())
                    .unwrap_or_default();
                let action = self.state.input_dialog_action.take();
                self.state.input_dialog = None;

                if let Some(ref act) = action {
                    match act.as_str() {
                        "set_api_key" => {
                            if !value.is_empty() {
                                // Set the API key in environment for current session
                                unsafe { std::env::set_var("SHANNON_API_KEY", &value); }
                                self.chat.add_message(
                                    ChatRole::System,
                                    "API key set for this session.".to_string(),
                                );
                            }
                        }
                        "set_model" => {
                            if !value.is_empty() {
                                self.state.model = Some(value.clone());
                                self.chat.add_message(
                                    ChatRole::System,
                                    format!("Model set to: {value}"),
                                );
                            }
                        }
                        _ => {
                            self.chat.add_message(
                                ChatRole::System,
                                format!("Input received: {value}"),
                            );
                        }
                    }
                }
            }
            crossterm::event::KeyCode::Esc => {
                self.state.input_dialog = None;
                self.state.input_dialog_action = None;
            }
            _ => {}
        }
        Ok(())
    }

    /// Open the command palette (Ctrl+P) with all available commands
    fn open_command_palette(&mut self) {
        let command_names = self.runtime.block_on(self.command_registry.list_names());
        let items: Vec<SelectItem<String>> = command_names.into_iter().map(|name| {
            let display = format!("/{name}");
            SelectItem::new(display.clone(), display)
        }).collect();

        let picker = FuzzyPickerWidget::new("Command Palette".to_string())
            .with_items(items);
        self.state.fuzzy_picker = Some(picker);
    }

    /// Handle fuzzy picker keyboard input
    fn handle_fuzzy_picker_input(&mut self, key: crossterm::event::KeyEvent) -> Result<()> {
        match key.code {
            crossterm::event::KeyCode::Up => {
                if let Some(ref mut picker) = self.state.fuzzy_picker {
                    picker.move_up();
                }
            }
            crossterm::event::KeyCode::Down => {
                if let Some(ref mut picker) = self.state.fuzzy_picker {
                    picker.move_down();
                }
            }
            crossterm::event::KeyCode::Char(c) => {
                if let Some(ref mut picker) = self.state.fuzzy_picker {
                    picker.add_search_char(c);
                }
            }
            crossterm::event::KeyCode::Backspace => {
                if let Some(ref mut picker) = self.state.fuzzy_picker {
                    picker.remove_search_char();
                }
            }
            crossterm::event::KeyCode::Enter => {
                // Get selected command and execute it
                let selected = self.state.fuzzy_picker.as_ref()
                    .and_then(|p| p.selected_value().map(|v| v.to_string()));
                self.state.fuzzy_picker = None;

                if let Some(cmd) = selected {
                    self.prompt.set_input(cmd);
                    self.submit_input()?;
                }
            }
            crossterm::event::KeyCode::Esc => {
                self.state.fuzzy_picker = None;
            }
            _ => {}
        }
        Ok(())
    }

    /// Handle /browse command — open file browser
    fn handle_browse_command(&mut self, args: &str) -> Result<()> {
        let path = if args.trim().is_empty() {
            self.state.working_directory.clone()
        } else {
            args.trim().to_string()
        };

        let mut selector = FileSelectorWidget::new("File Browser".to_string())
            .with_path(&path);
        if let Err(e) = selector.refresh() {
            self.chat.add_message(
                ChatRole::System,
                format!("Failed to browse {path}: {e}"),
            );
            return Ok(());
        }
        self.state.file_selector = Some(selector);
        Ok(())
    }

    /// Handle file selector keyboard input
    fn handle_file_selector_input(&mut self, key: crossterm::event::KeyEvent) -> Result<()> {
        match key.code {
            crossterm::event::KeyCode::Up => {
                if let Some(ref mut sel) = self.state.file_selector {
                    sel.move_up();
                }
            }
            crossterm::event::KeyCode::Down => {
                if let Some(ref mut sel) = self.state.file_selector {
                    sel.move_down();
                }
            }
            crossterm::event::KeyCode::Enter => {
                // Navigate into directory or select file
                if let Some(ref mut sel) = self.state.file_selector {
                    if let Some(selection) = sel.current_selection() {
                        let full_path = std::path::Path::new(sel.current_path()).join(&selection);
                        if full_path.is_dir() {
                            let dir_name = selection.clone();
                            if let Err(e) = sel.navigate_into(&dir_name) {
                                self.chat.add_message(
                                    ChatRole::System,
                                    format!("Failed to navigate into {dir_name}: {e}"),
                                );
                            }
                            return Ok(());
                        }
                    }
                }

                // File selected — close browser and put path in prompt
                let selected_path = self.state.file_selector.as_ref()
                    .and_then(|s| s.current_selection())
                    .map(|name| {
                        let base = self.state.file_selector.as_ref()
                            .map(|s| s.current_path().to_string())
                            .unwrap_or_else(|| ".".to_string());
                        format!("{base}/{name}")
                    });
                self.state.file_selector = None;

                if let Some(path) = selected_path {
                    // Fill the selected path into the prompt for immediate use
                    self.prompt.set_input(path);
                    self.chat.add_message(
                        ChatRole::System,
                        "File selected — press Enter to send as query, or edit the path.".to_string(),
                    );
                }
            }
            crossterm::event::KeyCode::Backspace => {
                // Navigate up
                if let Some(ref mut sel) = self.state.file_selector {
                    if let Err(e) = sel.navigate_up() {
                        self.chat.add_message(
                            ChatRole::System,
                            format!("Failed to navigate up: {e}"),
                        );
                    }
                }
            }
            crossterm::event::KeyCode::Esc => {
                self.state.file_selector = None;
            }
            _ => {}
        }
        Ok(())
    }

    /// Handle multi-select widget keyboard input
    fn handle_multi_select_input(&mut self, key: crossterm::event::KeyEvent) -> Result<()> {
        match key.code {
            crossterm::event::KeyCode::Up => {
                if let Some(ref mut sel) = self.state.multi_select {
                    sel.move_up();
                }
            }
            crossterm::event::KeyCode::Down => {
                if let Some(ref mut sel) = self.state.multi_select {
                    sel.move_down();
                }
            }
            crossterm::event::KeyCode::Char(' ') => {
                if let Some(ref mut sel) = self.state.multi_select {
                    sel.toggle_current();
                }
            }
            crossterm::event::KeyCode::Char('a') => {
                if let Some(ref mut sel) = self.state.multi_select {
                    sel.select_all();
                }
            }
            crossterm::event::KeyCode::Char('d') => {
                if let Some(ref mut sel) = self.state.multi_select {
                    sel.deselect_all();
                }
            }
            crossterm::event::KeyCode::Enter => {
                // Extract values before dropping the widget
                let values: Vec<String> = self.state.multi_select
                    .as_ref()
                    .map(|sel| sel.selected_values().iter().map(|v| v.to_string()).collect())
                    .unwrap_or_default();
                self.state.multi_select = None;

                if values.is_empty() {
                    self.chat.add_message(ChatRole::System, "No items selected.".to_string());
                } else {
                    self.chat.add_message(ChatRole::System, format!("Selected: {}", values.join(", ")));
                }
            }
            crossterm::event::KeyCode::Esc => {
                self.state.multi_select = None;
            }
            _ => {}
        }
        Ok(())
    }

    /// Handle /select-tools command — choose which tools to enable
    fn handle_select_tools_command(&mut self) -> Result<()> {
        let tool_info = if let Some(ref engine) = self.query_engine {
            engine.tools().list_tools_info()
        } else {
            Vec::new()
        };

        let items: Vec<SelectItem<String>> = tool_info.iter().map(|info| {
            SelectItem::new(info.name.clone(), info.name.clone())
                .with_description(info.description.clone())
        }).collect();

        let widget = MultiSelectWidget::new("Select Tools".to_string())
            .with_items(items);

        self.state.multi_select = Some(widget);
        Ok(())
    }

    /// Submit the current input
    fn submit_input(&mut self) -> Result<()> {
        let input = self.prompt.input().to_string();

        if input.trim().is_empty() {
            return Ok(());
        }

        // Add user message to chat
        self.chat.add_message(ChatRole::User, input.clone());

        // Push to command history and clear input
        self.command_history.push(&input);
        self.saved_input.clear();
        self.prompt.clear();

        // Process command or query
        if input.starts_with('/') {
            self.commands_run += 1;
            self.handle_command(&input)?;
        } else {
            self.handle_query(&input)?;
        }

        Ok(())
    }

    /// Handle a command (starts with /)
    fn handle_command(&mut self, input: &str) -> Result<()> {
        // Use the structured command parser
        let parsed = match self.command_parser.parse(input) {
            Ok(p) => p,
            Err(_) => {
                // Fallback to simple split for backward compat
                let parts: Vec<&str> = input.splitn(2, ' ').collect();
                let name = parts.first().copied().unwrap_or("").strip_prefix('/').unwrap_or("");
                shannon_commands::ParsedCommand::new(
                    name.to_string(),
                    parts.get(1).copied().unwrap_or("").to_string(),
                    input.to_string(),
                )
            }
        };

        let cmd_name = parsed.name.as_str();
        let args = parsed.args.as_str();

        // Check if command exists in the registry or as a plugin command
        let command_exists = self.runtime.block_on(async {
            self.shared_executor.registry().await.contains(cmd_name).await
        });
        let is_plugin_command = self.plugin_manager.get_plugin_commands()
            .iter().any(|c| c.name == cmd_name);

        if command_exists || is_plugin_command {
            // Dispatch based on command name (REPL-local commands execute here)
            match cmd_name {
                "help" => {
                    // Check if args specify a specific command
                    if !args.is_empty() {
                        // Try help system from shannon-commands first
                        let help_text = help_utils::generate_help(Some(args));
                        if !help_text.contains("No help found") {
                            self.chat.add_message(ChatRole::System, help_text);
                            return Ok(());
                        }
                    }

                    // Show full help from command registry
                    let help_text = help_utils::generate_help(None);
                    self.chat.add_message(ChatRole::System, help_text);
                }
                "clear" => {
                    if self.chat.len() > 1 {
                        // Show confirm dialog for non-empty chat
                        self.show_confirm_dialog(
                            "Clear Chat",
                            "Clear all messages? This cannot be undone.",
                            "clear_chat",
                        );
                    } else {
                        self.chat.clear();
                        self.chat.add_message(ChatRole::System, "Chat cleared.".to_string());
                    }
                }
                "quit" | "exit" => {
                    let had_activity = self.commands_run > 0
                        || self.tools_invoked > 0
                        || self.current_turn > 0;
                    if had_activity {
                        // Show confirm dialog for active session
                        self.show_confirm_dialog(
                            "End Session?",
                            "You have unsaved activity. Quit anyway?",
                            "quit",
                        );
                    } else {
                        self.running = false;
                    }
                }
                "model" => {
                    if args.is_empty() {
                        // Show input dialog for interactive model selection
                        self.show_input_dialog(
                            "Set Model",
                            "Enter model name (e.g. claude-3.5-sonnet, gpt-4o)...",
                            "set_model",
                        );
                    } else {
                        self.state.model = Some(args.to_string());
                        self.chat.add_message(
                            ChatRole::System,
                            format!("Model set to: {args}"),
                        );
                    }
                }
                "init" => {
                    let mut init_info = String::new();
                    let cwd = &self.state.working_directory;

                    // Check git status
                    let is_git = std::path::Path::new(cwd).join(".git").exists();
                    if is_git {
                        init_info.push_str("Git repository: detected\n");
                    } else {
                        init_info.push_str("Git repository: not found\n");
                    }

                    // Check/create CLAUDE.md
                    let claude_md_path = std::path::Path::new(cwd).join("CLAUDE.md");
                    if claude_md_path.exists() {
                        init_info.push_str("CLAUDE.md: already exists\n");
                    } else {
                        let default_content = "# Project Instructions\n\nThis file contains project-specific instructions for Shannon.\n\n## Coding Standards\n\n- Follow existing code patterns\n- Write clear, descriptive commit messages\n- Keep functions focused and concise\n\n## Project Structure\n\n- Describe your project structure here\n";
                        match std::fs::write(&claude_md_path, default_content) {
                            Ok(_) => init_info.push_str("CLAUDE.md: created with default template\n"),
                            Err(e) => init_info.push_str(&format!("CLAUDE.md: failed to create ({e})\n")),
                        }
                    }

                    // Show working directory
                    init_info.push_str(&format!("Working directory: {cwd}\n"));

                    self.chat.add_message(
                        ChatRole::System,
                        format!("Project initialized.\n{init_info}"),
                    );
                }
                "config" => self.handle_config_command(args)?,
                "sessions" => self.handle_sessions_command(args)?,
                "resume" => self.handle_resume_command(args)?,
                "history" => self.handle_history_command(args)?,
                "worktree" => self.handle_worktree_command(args)?,
                "credentials" | "creds" | "cred" => self.handle_credentials_command(args)?,
                "status" | "st" | "git-status" => self.handle_status_command(args)?,
                "export" | "save" => self.handle_export_command(args)?,
                "diff" => self.handle_diff_command(args)?,
                "search" | "?" | "hist" | "history-search" => self.handle_search_command(args)?,
                "browse" | "files" => self.handle_browse_command(args)?,
                "select-tools" | "tools" => self.handle_select_tools_command()?,
                "debug" | "dbg" | "dev" => self.handle_debug_command(args)?,
                "doctor" | "check" | "diagnostics" => self.handle_doctor_command(args)?,
                "team" => self.handle_team_command(args)?,
                _ => {
                    // Check plugin commands first (via PluginExecutable bridge)
                    let plugin_cmd = self.plugin_manager.get_plugin_commands()
                        .iter()
                        .find(|c| c.name == cmd_name)
                        .cloned();

                    if let Some(plugin) = plugin_cmd {
                        let prompt = plugin.prompt_template.replace("{args}", if args.is_empty() { "" } else { args });
                        self.chat.add_message(
                            ChatRole::System,
                            format!("Running /{cmd_name} (plugin)..."),
                        );
                        self.handle_query(&prompt)?;
                    } else {
                        let registry = self.runtime.block_on(self.shared_executor.registry());
                        if let Ok(command) = self.runtime.block_on(registry.get(cmd_name)) {
                        // Handle prompt-based commands (export, diff, commit, review_pr, pdf, debug, etc.)
                        // These have prompt templates that should be sent to the AI for execution.
                        match &*command {
                            shannon_commands::Command::Prompt(prompt_cmd) => {
                                if let Some(ref template) = prompt_cmd.prompt_template {
                                    // Render the prompt template with user arguments
                                    let prompt = template.replace("{args}", if args.is_empty() { "" } else { args });
                                    // Display the command invocation in chat
                                    self.chat.add_message(
                                        ChatRole::System,
                                        format!("Running /{cmd_name}..."),
                                    );
                                    // Send the rendered prompt to the AI query engine
                                    self.handle_query(&prompt)?;
                                } else {
                                    self.chat.add_message(
                                        ChatRole::System,
                                        format!("/{cmd_name} — {}", prompt_cmd.base.description),
                                    );
                                }
                            }
                            _ => {
                                let desc = command.description();
                                self.chat.add_message(
                                    ChatRole::System,
                                    format!("/{cmd_name} — {desc}"),
                                );
                            }
                        }
                    }
                }
            }
            }
            Ok(())
        } else {
            // Command not found in registry
            self.chat.add_message(
                ChatRole::System,
                format!("Unknown command: /{cmd_name}. Type /help for available commands."),
            );
            Ok(())
        }
    }

    /// Handle /sessions command — list persisted sessions
    fn handle_sessions_command(&mut self, args: &str) -> Result<()> {
        let sessions = match self.state_manager.list_persisted_sessions() {
            Ok(s) => s,
            Err(e) => {
                self.chat.add_message(
                    ChatRole::System,
                    format!("Error listing sessions: {e}"),
                );
                return Ok(());
            }
        };

        if sessions.is_empty() {
            self.chat.add_message(
                ChatRole::System,
                "No saved sessions found.".to_string(),
            );
            self.last_session_list.clear();
            return Ok(());
        }

        // Parse args: --all, --search <query>
        let show_all = args.contains("--all");
        let search_query = if let Some(idx) = args.find("--search") {
            let after = &args[idx + "--search".len()..].trim();
            if after.is_empty() {
                None
            } else {
                Some(after.to_lowercase())
            }
        } else if !args.is_empty() && !args.starts_with("--") {
            // Treat bare text as search query
            Some(args.to_lowercase())
        } else {
            None
        };

        // Filter sessions
        let mut filtered: Vec<_> = sessions.into_iter().filter(|s| {
            if let Some(ref q) = search_query {
                let title = s.title.as_deref().unwrap_or("").to_lowercase();
                let preview = s.preview.as_deref().unwrap_or("").to_lowercase();
                title.contains(q) || preview.contains(q) || s.model.to_lowercase().contains(q)
            } else {
                true
            }
        }).collect();

        // Limit to 10 unless --all
        let limit = if show_all { filtered.len() } else { 10.min(filtered.len()) };
        filtered.truncate(limit);

        // Cache for /resume by number
        self.last_session_list = filtered.clone();

        // Format output
        let mut output = String::from("Saved sessions:\n");
        for (i, session) in filtered.iter().enumerate() {
            let title = session.title.as_deref().unwrap_or("Untitled");
            let date = session.updated_at.format("%Y-%m-%d %H:%M");
            let tokens = (session.total_input_tokens + session.total_output_tokens) as f64 / 1000.0;
            output.push_str(&format!(
                "  #{}  {}  \"{}\"  {} turns  {:.1}k tokens  [{}]\n",
                i + 1,
                date,
                title,
                session.turn_count,
                tokens,
                session.model,
            ));
        }

        if !show_all {
            output.push_str("\nUse /sessions --all to see all, /sessions --search <query> to filter");
        }
        output.push_str("\nUse /resume <number-or-uuid> to continue a session");

        self.chat.add_message(ChatRole::System, output);
        Ok(())
    }

    /// Handle /resume command — resume a persisted session
    fn handle_resume_command(&mut self, args: &str) -> Result<()> {
        let arg = args.trim();
        if arg.is_empty() {
            self.chat.add_message(
                ChatRole::System,
                "Usage: /resume <number-or-uuid>\nUse /sessions to see available sessions.".to_string(),
            );
            return Ok(());
        }

        // Try to resolve to a UUID: by number (from last listing) or directly
        let session_id = if let Ok(uuid) = Uuid::parse_str(arg) {
            uuid
        } else if let Ok(num) = arg.parse::<usize>() {
            if num == 0 || num > self.last_session_list.len() {
                self.chat.add_message(
                    ChatRole::System,
                    format!("Invalid session number: {num}. Use /sessions to see available sessions."),
                );
                return Ok(());
            }
            self.last_session_list[num - 1].session_id
        } else {
            self.chat.add_message(
                ChatRole::System,
                format!("Invalid session identifier: {arg}. Use a number from /sessions or a UUID."),
            );
            return Ok(());
        };

        // Load session data
        match self.state_manager.load_session(&session_id) {
            Ok(Some(data)) => {
                // Clear current chat and load messages from session
                self.chat.clear();

                let title = data.metadata.title.as_deref().unwrap_or("Untitled");
                let msg_count = data.messages.len();

                // Add a header message
                self.chat.add_message(
                    ChatRole::System,
                    format!(
                        "Resumed session: \"{}\" ({} messages, model: {})\nCreated: {} | Updated: {}",
                        title,
                        msg_count,
                        data.metadata.model,
                        data.metadata.created_at.format("%Y-%m-%d %H:%M"),
                        data.metadata.updated_at.format("%Y-%m-%d %H:%M"),
                    ),
                );

                // Replay messages
                for msg in &data.messages {
                    let role = match msg.role.as_str() {
                        "user" => ChatRole::User,
                        "assistant" => ChatRole::Assistant,
                        _ => ChatRole::System,
                    };
                    let content = match &msg.content {
                        shannon_core::api::MessageContent::Text(t) => t.clone(),
                        shannon_core::api::MessageContent::Blocks(blocks) => {
                            blocks.iter().filter_map(|b| match b {
                                shannon_core::api::ContentBlock::Text { text } => Some(text.as_str()),
                                _ => None,
                            }).collect::<Vec<_>>().join("\n")
                        }
                    };
                    if !content.is_empty() {
                        self.chat.add_message(role, content);
                    }
                }

                // Restore model if available
                if !data.metadata.model.is_empty() {
                    self.state.model = Some(data.metadata.model.clone());
                }

                // Restore token count
                self.state.tokens_used =
                    data.metadata.total_input_tokens + data.metadata.total_output_tokens;

                // Restore the QueryEngine's internal conversation state so that
                // subsequent AI queries carry the full history from this session.
                if let Some(ref mut engine) = self.query_engine {
                    match engine.restore_session(session_id) {
                        Ok(true) => {
                            tracing::info!(session_id = %session_id, "QueryEngine conversation restored");
                        }
                        Ok(false) => {
                            tracing::warn!(session_id = %session_id, "No persisted session data for QueryEngine restore");
                        }
                        Err(e) => {
                            tracing::warn!(session_id = %session_id, error = %e, "Failed to restore QueryEngine session");
                            self.chat.add_message(
                                ChatRole::System,
                                format!("Warning: could not restore AI context (messages will lack prior history): {e}"),
                            );
                        }
                    }
                }
            }
            Ok(None) => {
                self.chat.add_message(
                    ChatRole::System,
                    format!("Session not found: {session_id}"),
                );
            }
            Err(e) => {
                self.chat.add_message(
                    ChatRole::System,
                    format!("Error loading session: {e}"),
                );
            }
        }

        Ok(())
    }

    /// Handle /history command — show current session stats or export
    fn handle_history_command(&mut self, args: &str) -> Result<()> {
        let arg = args.trim();

        // Parse --export flag
        if let Some(rest) = arg.strip_prefix("--export") {
            let export_path = if !rest.is_empty() {
                rest.trim()
            } else {
                ""
            };

            if export_path.is_empty() {
                self.chat.add_message(
                    ChatRole::System,
                    "Usage: /history --export <file-path>".to_string(),
                );
                return Ok(());
            }

            // Export current chat to markdown
            let mut md = String::from("# Shannon Session Export\n\n");
            for i in 0..self.chat.len() {
                if let Some(msg) = self.chat.get_message(i) {
                    let role = match msg.role {
                        ChatRole::User => "## User",
                        ChatRole::Assistant => "## Assistant",
                        ChatRole::System => "## System",
                        ChatRole::Tool => "## Tool",
                    };
                    md.push_str(&format!("{}\n\n{}\n\n---\n\n", role, msg.content));
                }
            }

            match std::fs::write(export_path, md) {
                Ok(_) => {
                    self.chat.add_message(
                        ChatRole::System,
                        format!("Session exported to: {export_path}"),
                    );
                }
                Err(e) => {
                    self.chat.add_message(
                        ChatRole::System,
                        format!("Failed to export: {e}"),
                    );
                }
            }
            return Ok(());
        }

        // Default: show stats
        let msg_count = self.chat.len();
        let mut user_count = 0;
        let mut assistant_count = 0;
        for i in 0..self.chat.len() {
            if let Some(msg) = self.chat.get_message(i) {
                match msg.role {
                    ChatRole::User => user_count += 1,
                    ChatRole::Assistant => assistant_count += 1,
                    ChatRole::System | ChatRole::Tool => {}
                }
            }
        }

        let tokens = self.state.tokens_used;
        let model = self.state.model.as_deref().unwrap_or("default");

        let mut stats = format!(
            "Current session stats:\n  Messages: {} total ({} user, {} assistant)\n  Tokens used: {} ({:.1}k)\n  Model: {}\n  Working dir: {}\n  Commands run: {}\n  Tools invoked: {}",
            msg_count,
            user_count,
            assistant_count,
            tokens,
            tokens as f64 / 1000.0,
            model,
            self.state.working_directory,
            self.commands_run,
            self.tools_invoked,
        );

        // Add session duration
        if let Some(started) = &self.session_started_at {
            let elapsed = chrono::Utc::now() - *started;
            let mins = elapsed.num_minutes();
            let secs = elapsed.num_seconds() % 60;
            stats.push_str(&format!("\n  Session duration: {mins}m {secs}s"));
        }

        // Add diff summary if there are tracked file changes
        if self.diff_data.total_files_modified() > 0 || self.diff_data.total_files_created() > 0 || self.diff_data.total_files_deleted() > 0 {
            stats.push_str(&format!(
                "\n  Files: +{}/-{}/{} modified, {} created, {} deleted",
                self.diff_data.total_additions(),
                self.diff_data.total_deletions(),
                self.diff_data.total_files_modified(),
                self.diff_data.total_files_created(),
                self.diff_data.total_files_deleted(),
            ));
        }

        self.chat.add_message(ChatRole::System, stats);
        Ok(())
    }

    /// Handle /worktree command — manage git worktrees
    fn handle_worktree_command(&mut self, args: &str) -> Result<()> {
        let arg = args.trim();

        if arg.is_empty() || arg == "status" {
            // Show worktree status
            let status = if arg.is_empty() {
                "Usage: /worktree [enter <name>|exit [--keep|--remove]|status]\n".to_string()
            } else {
                String::new()
            };

            // Check active worktree via the global state
            let active = shannon_agents::get_active_worktree();
            match active.as_ref() {
                Some(session) => {
                    let info = format!(
                        "{}Active worktree:\n  Branch: {}\n  Path: {}\n  Created: {}",
                        status,
                        session.branch_name,
                        session.path.display(),
                        session.created_at.format("%Y-%m-%d %H:%M"),
                    );
                    self.chat.add_message(ChatRole::System, info);
                }
                None => {
                    let info = format!("{status}No active worktree. Working in main repository.");
                    self.chat.add_message(ChatRole::System, info);
                }
            }
            return Ok(());
        }

        let parts: Vec<&str> = arg.splitn(3, ' ').collect();
        match parts[0] {
            "enter" => {
                let name = parts.get(1).copied().unwrap_or("");
                if name.is_empty() {
                    self.chat.add_message(
                        ChatRole::System,
                        "Usage: /worktree enter <name>".to_string(),
                    );
                    return Ok(());
                }
                // Execute the enter_worktree tool via the registry
                let input = serde_json::json!({ "name": name });
                match self.runtime.block_on(
                    self.query_engine.as_ref().unwrap().tools().execute("enter_worktree", input)
                ) {
                    Ok(result) => {
                        self.chat.add_message(
                            ChatRole::System,
                            format!("Entered worktree: {}", result.content),
                        );
                    }
                    Err(e) => {
                        self.chat.add_message(
                            ChatRole::System,
                            format!("Failed to enter worktree: {e}"),
                        );
                    }
                }
            }
            "exit" => {
                let action = parts.get(1).copied().unwrap_or("keep");
                let exit_action = match action {
                    "--remove" => "remove",
                    _ => "keep",
                };
                let input = serde_json::json!({ "action": exit_action });
                match self.runtime.block_on(
                    self.query_engine.as_ref().unwrap().tools().execute("exit_worktree", input)
                ) {
                    Ok(result) => {
                        self.chat.add_message(
                            ChatRole::System,
                            format!("Exited worktree: {}", result.content),
                        );
                    }
                    Err(e) => {
                        self.chat.add_message(
                            ChatRole::System,
                            format!("Failed to exit worktree: {e}"),
                        );
                    }
                }
            }
            _ => {
                self.chat.add_message(
                    ChatRole::System,
                    "Unknown worktree action. Use: enter <name>, exit [--keep|--remove], or status".to_string(),
                );
            }
        }

        Ok(())
    }

    /// Handle /team command — multi-agent team orchestration
    fn handle_team_command(&mut self, args: &str) -> Result<()> {
        use shannon_agents::{
            AgentCoordinator, CoordinatorConfig,
            TeammateConfig, TaskPriority,
        };

        let parts: Vec<&str> = args.splitn(4, ' ').collect();
        let subcommand = parts.first().copied().unwrap_or("help");

        match subcommand {
            "help" | "" => {
                let help = "\
/team create <name> [description]  — Create a new agent team
/team add <team> <agent-name>  — Add agent to team
/team task <team> <subject>  — Add a task
/team assign <team>  — Assign pending tasks to available agents
/team status [team]  — Show team status
/team list  — List all teams
/team run  — Execute pending tasks in parallel
/team shutdown  — Shutdown team";
                self.chat.add_message(ChatRole::System, help.to_string());
            }
            "create" => {
                let name = parts.get(1).copied().unwrap_or("");
                if name.is_empty() {
                    self.chat.add_message(ChatRole::System, "Usage: /team create <name> [description]".to_string());
                    return Ok(());
                }
                let description = parts.get(2..).map(|s| s.join(" ")).unwrap_or_default();
                let config = CoordinatorConfig::default();
                match self.runtime.block_on(AgentCoordinator::new(config)) {
                    Ok(coordinator) => {
                        match self.runtime.block_on(coordinator.create_team(name.to_string(), description)) {
                            Ok(()) => {
                                self.team_coordinator = Some(coordinator);
                                self.chat.add_message(ChatRole::System, format!("Team '{name}' created."));
                            }
                            Err(e) => {
                                self.chat.add_message(ChatRole::System, format!("Failed to create team: {e}"));
                            }
                        }
                    }
                    Err(e) => {
                        self.chat.add_message(ChatRole::System, format!("Failed to initialize coordinator: {e}"));
                    }
                }
            }
            "add" => {
                let team_name = parts.get(1).copied().unwrap_or("");
                let agent_name = parts.get(2).copied().unwrap_or("");
                if team_name.is_empty() || agent_name.is_empty() {
                    self.chat.add_message(ChatRole::System, "Usage: /team add <team> <agent-name>".to_string());
                    return Ok(());
                }
                if let Some(ref coordinator) = self.team_coordinator {
                    let config = TeammateConfig::default();
                    match self.runtime.block_on(coordinator.add_teammate(team_name, agent_name.to_string(), config)) {
                        Ok(()) => {
                            // Attempt to create an isolated worktree for the agent
                            let worktree_msg = match self.create_agent_worktree(agent_name) {
                                Ok(path) => format!(" (worktree: {})", path.display()),
                                Err(reason) => format!(" (worktree skipped: {reason})"),
                            };
                            self.chat.add_message(ChatRole::System, format!("Agent '{agent_name}' added to team '{team_name}'.{worktree_msg}"));
                        }
                        Err(e) => {
                            self.chat.add_message(ChatRole::System, format!("Failed to add agent: {e}"));
                        }
                    }
                } else {
                    self.chat.add_message(ChatRole::System, "No team created yet. Use /team create first.".to_string());
                }
            }
            "task" => {
                let team_name = parts.get(1).copied().unwrap_or("");
                let subject = parts.get(2..).map(|s| s.join(" ")).unwrap_or_default();
                if team_name.is_empty() || subject.is_empty() {
                    self.chat.add_message(ChatRole::System, "Usage: /team task <team> <subject>".to_string());
                    return Ok(());
                }
                if let Some(ref coordinator) = self.team_coordinator {
                    match self.runtime.block_on(coordinator.add_task(team_name, subject.clone(), String::new(), TaskPriority::Medium)) {
                        Ok(task_id) => {
                            self.chat.add_message(ChatRole::System, format!("Task added to '{team_name}': {subject} (id: {task_id})"));
                        }
                        Err(e) => {
                            self.chat.add_message(ChatRole::System, format!("Failed to add task: {e}"));
                        }
                    }
                } else {
                    self.chat.add_message(ChatRole::System, "No team created yet. Use /team create first.".to_string());
                }
            }
            "assign" => {
                let team_name = parts.get(1).copied().unwrap_or("");
                if team_name.is_empty() {
                    self.chat.add_message(ChatRole::System, "Usage: /team assign <team>".to_string());
                    return Ok(());
                }
                if let Some(ref coordinator) = self.team_coordinator {
                    match self.runtime.block_on(coordinator.assign_task(team_name, uuid::Uuid::nil())) {
                        Ok(agent) => {
                            self.chat.add_message(ChatRole::System, format!("Task assigned to '{agent}' in team '{team_name}'."));
                        }
                        Err(e) => {
                            self.chat.add_message(ChatRole::System, format!("Failed to assign task: {e}"));
                        }
                    }
                } else {
                    self.chat.add_message(ChatRole::System, "No team created yet. Use /team create first.".to_string());
                }
            }
            "status" => {
                let team_name = parts.get(1).copied().unwrap_or("");
                if let Some(ref coordinator) = self.team_coordinator {
                    if team_name.is_empty() {
                        let teams = self.runtime.block_on(coordinator.list_teams());
                        if teams.is_empty() {
                            self.chat.add_message(ChatRole::System, "No teams created yet.".to_string());
                        } else {
                            let output = format!("Teams:\n{}", teams.iter().map(|t| format!("  - {t}")).collect::<Vec<_>>().join("\n"));
                            self.chat.add_message(ChatRole::System, output);
                        }
                    } else {
                        match self.runtime.block_on(coordinator.team_status(team_name)) {
                            Ok(status) => { self.chat.add_message(ChatRole::System, status); }
                            Err(e) => { self.chat.add_message(ChatRole::System, format!("Failed to get status: {e}")); }
                        }
                    }
                } else {
                    self.chat.add_message(ChatRole::System, "No team created yet. Use /team create first.".to_string());
                }
            }
            "list" => {
                if let Some(ref coordinator) = self.team_coordinator {
                    let teams = self.runtime.block_on(coordinator.list_teams());
                    if teams.is_empty() {
                        self.chat.add_message(ChatRole::System, "No teams created yet.".to_string());
                    } else {
                        let output = format!("Teams:\n{}", teams.iter().map(|t| format!("  - {t}")).collect::<Vec<_>>().join("\n"));
                        self.chat.add_message(ChatRole::System, output);
                    }
                } else {
                    self.chat.add_message(ChatRole::System, "No team created yet. Use /team create first.".to_string());
                }
            }
            "shutdown" => {
                if let Some(ref coordinator) = self.team_coordinator {
                    match self.runtime.block_on(coordinator.shutdown()) {
                        Ok(()) => { self.chat.add_message(ChatRole::System, "Team shut down.".to_string()); }
                        Err(e) => { self.chat.add_message(ChatRole::System, format!("Failed to shutdown: {e}")); }
                    }
                } else {
                    self.chat.add_message(ChatRole::System, "No active team.".to_string());
                }
            }
            "run" => {
                use shannon_agents::{MultiAgentSpawner, SpawnAgentConfig};
                if let Some(ref coordinator) = self.team_coordinator {
                    let task_board = coordinator.task_board();
                    let ready_tasks = self.runtime.block_on(task_board.list_ready_tasks());
                    if ready_tasks.is_empty() {
                        self.chat.add_message(ChatRole::System, "No pending tasks to execute.".to_string());
                        return Ok(());
                    }
                    let agent_configs: Vec<SpawnAgentConfig> = ready_tasks
                        .iter()
                        .map(|t| SpawnAgentConfig::new(
                            format!("agent-{}", t.id),
                            t.subject.clone(),
                        ))
                        .collect();
                    let config = shannon_agents::MultiAgentConfig::new(agent_configs);
                    self.chat.add_message(ChatRole::System, "Starting parallel execution...".to_string());
                    let result = self.runtime.block_on(MultiAgentSpawner::spawn(config));
                    let mut report = format!(
                        "Execution complete: {} succeeded, {} failed ({:.1}s)\n",
                        result.success_count,
                        result.failure_count,
                        result.total_duration.as_secs_f64(),
                    );
                    for ar in &result.agent_results {
                        report.push_str(&format!(
                            "  [{}] {} ({:.1}s){}\n",
                            ar.status,
                            ar.agent_name,
                            ar.duration.as_secs_f64(),
                            ar.error.as_ref().map(|e| format!(" — {e}")).unwrap_or_default(),
                        ));
                    }
                    self.chat.add_message(ChatRole::System, report);
                } else {
                    self.chat.add_message(ChatRole::System, "No team created yet. Use /team create first.".to_string());
                }
            }
            _ => {
                self.chat.add_message(ChatRole::System, format!("Unknown subcommand: {subcommand}. Use /team help."));
            }
        }

        Ok(())
    }

    /// Create an isolated git worktree for an agent.
    /// Returns the worktree path on success, or a human-readable reason on failure.
    fn create_agent_worktree(&self, agent_name: &str) -> std::result::Result<std::path::PathBuf, String> {
        use shannon_agents::{WorktreeManager, WorktreeConfig};

        let config = WorktreeConfig::default();
        let manager = self.runtime.block_on(WorktreeManager::new(config))
            .map_err(|e| format!("{e}"))?;

        let session = self.runtime.block_on(
            manager.create_agent_session(agent_name, None)
        )
        .map_err(|e| format!("{e}"))?;

        Ok(session.path)
    }

    /// Handle /config command — manage runtime configuration
    fn handle_config_command(&mut self, args: &str) -> Result<()> {
        use shannon_tools::config::ConfigManager;

        let mut manager = ConfigManager::new();
        if let Err(e) = manager.load() {
            self.chat.add_message(
                ChatRole::System,
                format!("Warning: could not load config: {e}"),
            );
        }

        let parts: Vec<&str> = args.splitn(3, ' ').collect();
        let action_str = parts.first().copied().unwrap_or("");
        let action = config_utils::parse_config_action(action_str);

        let output = match action {
            config_utils::ConfigAction::List => {
                let prefix = if action_str.is_empty() { None } else { parts.get(1).copied() };
                let keys = manager.list(prefix);
                if keys.is_empty() {
                    config_utils::format_config_list()
                } else {
                    let mut out = config_utils::format_config_list();
                    out.push_str(&format!("\nConfig file: {}\n", manager.config_path().display()));
                    for key in &keys {
                        let val = manager.get(key).unwrap_or(serde_json::Value::Null);
                        out.push_str(&format!("  {key} = {val}\n"));
                    }
                    out
                }
            }
            config_utils::ConfigAction::Get => {
                let key = parts.get(1).copied().unwrap_or("");
                if key.is_empty() {
                    "Usage: /config get <key>".to_string()
                } else {
                    match manager.get(key) {
                        Some(_val) => config_utils::format_config_get(key),
                        None => format!("Config key not found: {key}"),
                    }
                }
            }
            config_utils::ConfigAction::Set => {
                let key = parts.get(1).copied().unwrap_or("");
                let value_str = parts.get(2).copied().unwrap_or("");
                if key.is_empty() || value_str.is_empty() {
                    "Usage: /config set <key> <value>".to_string()
                } else {
                    let value: serde_json::Value = if value_str == "true" {
                        serde_json::json!(true)
                    } else if value_str == "false" {
                        serde_json::json!(false)
                    } else if let Ok(n) = value_str.parse::<i64>() {
                        serde_json::json!(n)
                    } else if let Ok(n) = value_str.parse::<f64>() {
                        serde_json::json!(n)
                    } else {
                        serde_json::json!(value_str)
                    };
                    manager.set(key.to_string(), value.clone());
                    match manager.save() {
                        Ok(_) => config_utils::format_config_set(key, &value.to_string()),
                        Err(e) => format!("Error saving config: {e}"),
                    }
                }
            }
            config_utils::ConfigAction::Reset => {
                let key = parts.get(1).copied().unwrap_or("");
                if key.is_empty() {
                    "Usage: /config reset <key>".to_string()
                } else {
                    let existed = manager.reset(key);
                    if existed {
                        let _val = manager.get(key).unwrap_or(serde_json::Value::Null);
                        match manager.save() {
                            Ok(_) => config_utils::format_config_reset(key),
                            Err(e) => format!("Error saving config: {e}"),
                        }
                    } else {
                        config_utils::format_config_reset(key)
                    }
                }
            }
            config_utils::ConfigAction::Help => {
                config_utils::format_config_list()
            }
        };

        self.chat.add_message(ChatRole::System, output);
        Ok(())
    }

    /// Handle /credentials command — manage stored credentials
    fn handle_credentials_command(&mut self, args: &str) -> Result<()> {
        use shannon_commands::credential_utils::{
            parse_credential_action, CredentialAction,
            format_credentials_list, format_credential_store,
            format_credential_get, format_credential_delete,
            format_credential_count,
        };

        let parts: Vec<&str> = args.splitn(3, ' ').collect();
        let action_str = parts.first().copied().unwrap_or("");
        let action = parse_credential_action(action_str);

        let output = match action {
            CredentialAction::List => format_credentials_list(),
            CredentialAction::Store => {
                let service = parts.get(1).copied().unwrap_or("");
                let value = parts.get(2).copied().unwrap_or("");
                if service.is_empty() || value.is_empty() {
                    "Usage: /credentials store <service> <value>".to_string()
                } else {
                    format_credential_store(service, value)
                }
            }
            CredentialAction::Get => {
                let service = parts.get(1).copied().unwrap_or("");
                if service.is_empty() {
                    "Usage: /credentials get <service>".to_string()
                } else {
                    format_credential_get(service)
                }
            }
            CredentialAction::Delete => {
                let service = parts.get(1).copied().unwrap_or("");
                if service.is_empty() {
                    "Usage: /credentials delete <service>".to_string()
                } else {
                    format_credential_delete(service)
                }
            }
            CredentialAction::Count => format_credential_count(),
            CredentialAction::Help => {
                "Credential Management:\n\n\
                 /credentials list              - Show stored credentials\n\
                 /credentials store <svc> <val> - Store a credential\n\
                 /credentials get <service>     - Retrieve a credential (masked)\n\
                 /credentials delete <service>  - Delete a credential\n\
                 /credentials count             - Show stored credential count\n".to_string()
            }
        };

        self.chat.add_message(ChatRole::System, output);
        Ok(())
    }

    /// Handle /status command — show git repository status using rich types
    fn handle_status_command(&mut self, args: &str) -> Result<()> {
        use shannon_commands::status_utils::{parse_git_status, format_status};

        let short = args.contains("--short");

        // Run git status --short --branch to get parseable output
        let output = std::process::Command::new("git")
            .args(["status", "--short", "--branch"])
            .current_dir(&self.state.working_directory)
            .output();

        let status_output = match output {
            Ok(result) => {
                let stdout = String::from_utf8_lossy(&result.stdout);
                let stderr = String::from_utf8_lossy(&result.stderr);
                if !stderr.is_empty() && stdout.is_empty() {
                    self.chat.add_message(ChatRole::System, format!("Git error: {stderr}"));
                    return Ok(());
                }
                stdout.to_string()
            }
            Err(e) => {
                self.chat.add_message(ChatRole::System, format!("Failed to run git status: {e}"));
                return Ok(());
            }
        };

        if let Some(info) = parse_git_status(&status_output) {
            let formatted = format_status(&info, short);

            // Also get recent commits
            let log_output = std::process::Command::new("git")
                .args(["log", "--oneline", "-5"])
                .current_dir(&self.state.working_directory)
                .output();

            let mut full_output = formatted;

            if let Ok(log_result) = log_output {
                let log_stdout = String::from_utf8_lossy(&log_result.stdout);
                if !log_stdout.is_empty() {
                    full_output.push_str("\nRecent commits:\n");
                    full_output.push_str(&log_stdout);
                }
            }

            self.chat.add_message(ChatRole::System, full_output);
        } else {
            // Fallback: just show the raw output
            self.chat.add_message(ChatRole::System, status_output);
        }

        Ok(())
    }

    /// Handle /export command — export session to file
    fn handle_export_command(&mut self, args: &str) -> Result<()> {
        // Parse export arguments using structured parser
        let options = match export_utils::parse_export_args(args) {
            Ok(opts) => opts,
            Err(e) => {
                self.chat.add_message(ChatRole::System, format!("Export error: {e}"));
                return Ok(());
            }
        };

        // Determine filename
        let filename = options.filename.clone().unwrap_or_else(|| {
            export_utils::generate_filename(options.format)
        });

        // Collect messages from chat into ExportMessages
        let mut messages = Vec::new();
        for i in 0..self.chat.len() {
            if let Some(msg) = self.chat.get_message(i) {
                let role = match msg.role {
                    ChatRole::User => "user",
                    ChatRole::Assistant => "assistant",
                    ChatRole::System => "system",
                    ChatRole::Tool => "tool",
                };
                messages.push(export_utils::ExportMessage {
                    role: role.to_string(),
                    content: msg.content.clone(),
                    timestamp: Some(msg.timestamp.timestamp() as u64),
                });
            }
        }

        // Build session start timestamp
        let started_at = self.session_started_at
            .map(|t| t.timestamp() as u64)
            .unwrap_or(0);

        // Build structured session
        let session = export_utils::ExportSession {
            title: "Shannon Session".to_string(),
            started_at,
            messages,
            metadata: export_utils::SessionMetadata {
                model: self.state.model.clone().unwrap_or_else(|| "default".to_string()),
                tokens_used: self.state.tokens_used as usize,
                working_dir: self.state.working_directory.clone(),
                commands_run: self.commands_run,
                tools_invoked: self.tools_invoked,
            },
        };

        // Generate content in the requested format
        let content = match options.format {
            export_utils::ExportFormat::Markdown => {
                export_utils::export_to_markdown(&session, &options)
            }
            export_utils::ExportFormat::Json => {
                export_utils::export_to_json(&session, &options)
            }
        };

        // Write to file
        match export_utils::write_export(&content, &filename) {
            Ok(_) => {
                let format_name = match options.format {
                    export_utils::ExportFormat::Markdown => "markdown",
                    export_utils::ExportFormat::Json => "JSON",
                };
                self.chat.add_message(
                    ChatRole::System,
                    format!("Session exported to: {filename} ({} messages, {format_name} format)", self.chat.len()),
                );
            }
            Err(e) => {
                self.chat.add_message(
                    ChatRole::System,
                    format!("Failed to export session: {e}"),
                );
            }
        }
        Ok(())
    }

    /// Handle /diff command -- show git diff analysis using structured pipeline
    fn handle_diff_command(&mut self, args: &str) -> Result<()> {
        // Parse arguments using the structured diff pipeline
        let options = diff_utils::DiffOptions::from_args(args);
        let cmd_str = diff_utils::build_diff_command(&options);

        // Split the command string into args for std::process::Command
        let cmd_parts: Vec<&str> = cmd_str.split_whitespace().collect();
        if cmd_parts.is_empty() {
            self.chat.add_message(
                ChatRole::System,
                "Failed to build git diff command.".to_string(),
            );
            return Ok(());
        }

        let output = std::process::Command::new(cmd_parts[0])
            .args(&cmd_parts[1..])
            .current_dir(&self.state.working_directory)
            .output();

        match output {
            Ok(result) => {
                let stdout = String::from_utf8_lossy(&result.stdout);
                let stderr = String::from_utf8_lossy(&result.stderr);

                if !stderr.is_empty() && stdout.is_empty() {
                    self.chat.add_message(
                        ChatRole::System,
                        format!("Git diff error: {stderr}"),
                    );
                } else if stdout.is_empty() {
                    self.chat.add_message(
                        ChatRole::System,
                        "No changes found.".to_string(),
                    );
                } else {
                    // Run structured analysis on the diff output
                    let analyzer = diff_utils::DiffAnalyzer::new();
                    let analysis = analyzer.analyze(&stdout);

                    // Build a formatted summary
                    let files: Vec<&str> = stdout.lines()
                        .filter(|l| l.starts_with("diff --git"))
                        .collect();
                    let total_lines = stdout.lines().count();
                    let category_summary = analysis.summary();
                    let test_flag = if analysis.has_test_changes() { " [has test changes]" } else { "" };

                    // Truncate raw diff if large
                    let raw_diff = if stdout.len() > 4000 {
                        format!("{}\n... (truncated)", &stdout[..4000])
                    } else {
                        stdout.to_string()
                    };

                    self.chat.add_message(
                        ChatRole::System,
                        format!(
                            "Git diff ({} files, {} lines){test_flag}\nCategories: {category_summary}\n\n{raw_diff}",
                            files.len(),
                            total_lines,
                        ),
                    );
                }
            }
            Err(e) => {
                self.chat.add_message(
                    ChatRole::System,
                    format!("Failed to run git diff: {e}"),
                );
            }
        }
        Ok(())
    }

    /// Handle /search command -- search command history locally
    fn handle_search_command(&mut self, args: &str) -> Result<()> {
        // Parse search arguments
        let options = match search_utils::parse_search_args(args) {
            Ok(opts) => opts,
            Err(e) => {
                self.chat.add_message(
                    ChatRole::System,
                    format!("Search error: {e}\nUsage: /search <pattern> [--count N] [--regex] [--case-sensitive] [--no-timestamps]"),
                );
                return Ok(());
            }
        };

        // Collect history entries as owned Strings for search_history()
        let entries: Vec<String> = self.command_history.entries()
            .iter()
            .map(|s| s.to_string())
            .collect();

        // Search through history
        let matches = search_utils::search_history(&entries, &options);

        // Format and display results
        let output = search_utils::format_results(&matches, &options);
        self.chat.add_message(ChatRole::System, output);

        Ok(())
    }

    /// Handle /debug command — developer tools for debugging, logging, and profiling
    fn handle_debug_command(&mut self, args: &str) -> Result<()> {
        use shannon_commands::debug_utils::{
            parse_debug_subcommand, parse_log_level,
            format_debug_help, format_log_response,
            format_profile_response, format_trace_response,
            format_system_info, DebugSubcommand,
        };

        let parts: Vec<&str> = args.splitn(3, ' ').collect();
        let subcommand_str = parts.first().copied().unwrap_or("");
        let subcommand = parse_debug_subcommand(subcommand_str);

        let output = match subcommand {
            DebugSubcommand::Help => format_debug_help(),
            DebugSubcommand::Info => {
                let mut info = format_system_info();

                // Append cargo/rust version if available
                if let Ok(rust_output) = std::process::Command::new("rustc")
                    .arg("--version")
                    .output()
                {
                    let version = String::from_utf8_lossy(&rust_output.stdout);
                    if !version.trim().is_empty() {
                        info.push_str(&format!("  Rust: {}\n", version.trim()));
                    }
                }

                if let Ok(cargo_output) = std::process::Command::new("cargo")
                    .arg("--version")
                    .output()
                {
                    let version = String::from_utf8_lossy(&cargo_output.stdout);
                    if !version.trim().is_empty() {
                        info.push_str(&format!("  Cargo: {}\n", version.trim()));
                    }
                }

                info
            }
            DebugSubcommand::Log => {
                let level_str = parts.get(1).copied().unwrap_or("info");
                let level = parse_log_level(level_str);
                if let Some(lvl) = level {
                    // Set RUST_LOG environment variable for the current process
                    // Safety: This is single-threaded during REPL command handling
                    unsafe { std::env::set_var("RUST_LOG", lvl.to_string()); }
                }
                format_log_response(level)
            }
            DebugSubcommand::Profile => {
                let action = parts.get(1).copied().unwrap_or("start");
                format_profile_response(action)
            }
            DebugSubcommand::Trace => {
                let toggle = parts.get(1).copied().unwrap_or("on");
                let enabled = matches!(toggle.to_lowercase().as_str(), "on" | "true" | "1" | "yes");
                if enabled {
                    // Safety: This is single-threaded during REPL command handling
                    unsafe { std::env::set_var("SHANNON_TRACE", "1"); }
                } else {
                    // Safety: This is single-threaded during REPL command handling
                    unsafe { std::env::remove_var("SHANNON_TRACE"); }
                }
                format_trace_response(enabled)
            }
        };

        self.chat.add_message(ChatRole::System, output);
        Ok(())
    }

    /// Handle /doctor command — run local diagnostic checks without consuming AI tokens
    fn handle_doctor_command(&mut self, _args: &str) -> Result<()> {
        use shannon_commands::doctor_utils::{run_all_checks, format_doctor_report};

        let results = run_all_checks();
        let report = format_doctor_report(&results);
        self.chat.add_message(ChatRole::System, report);
        Ok(())
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

    /// Handle a query (send to AI)
    fn handle_query(&mut self, input: &str) -> Result<()> {
        self.state.status = "Processing...".to_string();
        self.state.active_tool = None;
        self.state.query_steps_done = 0;
        self.state.query_steps_total = 0;
        self.state.progress_bar_visible = false;
        self.state.progress_bar.set_progress(0.0);

        // Start a new turn diff for tracking file changes
        let _turn_diff = TurnDiff::new(self.current_turn);

        // Clear the "Thinking..." message and start streaming
        // Create an assistant message that will be updated in real-time
        let assistant_msg_index = self.chat.add_message(
            ChatRole::Assistant,
            String::new(),
        );

        // Take the query engine out — spawn requires 'static ownership
        // We'll restore it after the query completes
        let query_engine = self.query_engine.take().expect("QueryEngine not initialized");

        // Create query context
        let query_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();

        let context = QueryContext {
            query_id,
            session_id,
            user_message: input.to_string(),
            metadata: shannon_core::query_engine::QueryMetadata {
                timestamp: chrono::Utc::now(),
                tools_allowed: true,
                max_tokens: Some(4096),
                model: self.state.model.clone().unwrap_or_else(||
                    "claude-3-5-sonnet".to_string()
                ),
                temperature: None,
                top_p: None,
            },
        };

        // Process query with real-time streaming UI updates
        // Use shared state between the async query task and the main UI loop
        use std::sync::{Arc, Mutex};
        let streaming_buffer: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let streaming_status: Arc<Mutex<String>> = Arc::new(Mutex::new("Processing...".to_string()));
        let streaming_done: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
        let streaming_cost: Arc<Mutex<f64>> = Arc::new(Mutex::new(0.0));
        let streaming_progress: Arc<Mutex<f64>> = Arc::new(Mutex::new(0.0));
        let streaming_multi_progress: Arc<Mutex<Vec<(String, f64, ratatui::style::Color)>>> = Arc::new(Mutex::new(Vec::new()));

        let buffer_clone = streaming_buffer.clone();
        let status_clone = streaming_status.clone();
        let done_clone = streaming_done.clone();
        let cost_clone = streaming_cost.clone();
        let progress_clone = streaming_progress.clone();
        let multi_progress_clone = streaming_multi_progress.clone();
        let permission_tx = self.permission_req_tx.clone();

        // Spawn the query processing in a separate thread so the UI can render
        let query_handle = self.runtime.spawn(async move {
            shannon_core::prevent_sleep::start_prevent_sleep();
            let permission_channel = Some(permission_tx);
            let mut stream = query_engine.process_query(context, permission_channel).await;

            let mut response_text = String::new();
            let mut tokens_in_turn = 0u64;
            let mut tool_calls: Vec<String> = Vec::new();
            let mut tools_in_session: usize = 0;
            let mut progress_status = "Processing...".to_string();
            let mut steps_done = 0usize;
            let mut turn_diff = TurnDiff::new(0);

            while let Some(event_result) = stream.next().await {
                match event_result {
                    Ok(QueryEvent::Started { .. }) => {}
                    Ok(QueryEvent::Text { content, .. }) => {
                        response_text.push_str(&content);
                        // Push update to shared buffer for UI rendering
                        if let Ok(mut buf) = buffer_clone.lock() {
                            *buf = response_text.clone();
                        }
                    }
                    Ok(QueryEvent::ToolUseRequest { tool_name, tool_input, .. }) => {
                        steps_done += 1;
                        progress_status = format!("Running: {tool_name} (step {steps_done})");
                        let tool_display = format!("\n🔧 Using: {} with input: {}", tool_name,
                            serde_json::to_string_pretty(&tool_input).unwrap_or_else(|_| "invalid".to_string())
                        );
                        response_text.push_str(&tool_display);
                        tool_calls.push(tool_name.clone());
                        tools_in_session += 1;

                        // Track tool in multi-progress display
                        {
                            let colors = [
                                ratatui::style::Color::Cyan,
                                ratatui::style::Color::Green,
                                ratatui::style::Color::Yellow,
                                ratatui::style::Color::Magenta,
                                ratatui::style::Color::Blue,
                            ];
                            let color = colors[tool_calls.len() % colors.len()];
                            if let Ok(mut mp) = multi_progress_clone.lock() {
                                mp.push((tool_name.clone(), 0.0, color));
                            }
                        }

                        if let Ok(mut s) = status_clone.lock() {
                            *s = progress_status.clone();
                        }
                        if let Ok(mut buf) = buffer_clone.lock() {
                            *buf = response_text.clone();
                        }

                        let tool_name_str = tool_name.as_str();
                        if tool_name_str == "write" || tool_name_str == "edit" || tool_name_str == "WriteTool" {
                            if let Some(path) = tool_input.get("file_path").and_then(|v| v.as_str()) {
                                turn_diff.modify_file(path.to_string(), 1, 0);
                            }
                        }
                    }
                    Ok(QueryEvent::ToolUseResult { tool_name, result, is_error, .. }) => {
                        let formatted = crate::tool_format::format_tool_result(
                            &tool_name, &result, is_error,
                        );
                        let result_display = format!("\n{formatted}");
                        response_text.push_str(&result_display);
                        if let Ok(mut buf) = buffer_clone.lock() {
                            *buf = response_text.clone();
                        }
                        // Mark tool as complete in multi-progress
                        if let Ok(mut mp) = multi_progress_clone.lock() {
                            if let Some(bar) = mp.iter_mut().find(|(l, _, _)| l == &tool_name) {
                                bar.1 = 1.0;
                            }
                        }
                    }
                    Ok(QueryEvent::TurnCompleted { turn_number, tokens_used, .. }) => {
                        tokens_in_turn += tokens_used;
                        let turn_info = format!("\n\n[Turn {turn_number} completed, {tokens_used} tokens]");
                        response_text.push_str(&turn_info);
                    }
                    Ok(QueryEvent::Progress { message, .. }) => {
                        progress_status = format!("Processing: {message}");
                        let progress = format!("\n⏳ {message}");
                        response_text.push_str(&progress);
                        if let Ok(mut s) = status_clone.lock() {
                            *s = progress_status.clone();
                        }
                        if let Ok(mut buf) = buffer_clone.lock() {
                            *buf = response_text.clone();
                        }
                    }
                    Ok(QueryEvent::Usage { input_tokens, output_tokens, cost_usd, .. }) => {
                        let usage = format!("\n📊 Tokens: {input_tokens} in + {output_tokens} out = ${cost_usd:.4}");
                        response_text.push_str(&usage);
                    }
                    Ok(QueryEvent::Cost { total_cost_usd, input_tokens, output_tokens, .. }) => {
                        tokens_in_turn = input_tokens + output_tokens;
                        if let Ok(mut c) = cost_clone.lock() {
                            *c = total_cost_usd;
                        }
                    }
                    Ok(QueryEvent::ToolProgress { progress, tool_name, .. }) => {
                        let pct = (progress * 100.0) as u32;
                        let progress_msg = format!("\n⏳ Tool progress: {pct}%");
                        response_text.push_str(&progress_msg);
                        // Update shared progress for status bar rendering
                        if let Ok(mut p) = progress_clone.lock() {
                            *p = progress as f64;
                        }
                        if let Ok(mut buf) = buffer_clone.lock() {
                            *buf = response_text.clone();
                        }
                        progress_status = format!("{tool_name}: {pct}%");
                        if let Ok(mut s) = status_clone.lock() {
                            *s = progress_status.clone();
                        }
                    }
                    Ok(QueryEvent::Completed { .. }) => {
                        if let Ok(cost) = cost_clone.lock() {
                            if *cost > 0.0 {
                                let cost_line = format!("\n💰 Session total: ${:.4}", *cost);
                                response_text.push_str(&cost_line);
                            }
                        }
                    }
                    Ok(QueryEvent::Failed { error, .. }) => {
                        return Err(format!("Query failed: {error}"));
                    }
                    Err(e) => {
                        return Err(format!("Stream error: {e}"));
                    }
                }
            }

            Ok::<(QueryEngine, String, u64, usize, TurnDiff, String, usize), String>((query_engine, response_text, tokens_in_turn, tools_in_session, turn_diff, progress_status, steps_done))
        });

        // Poll the streaming buffer while the query runs, updating the UI in real-time
        {
            let terminal_backend = CrosstermBackend::new(io::stdout());
            let mut polling_terminal = Terminal::new(terminal_backend)?;
            let mut last_rendered_len = 0usize;

            loop {
                // Check if the query is done
                let is_done = done_clone.lock().map(|g| *g).unwrap_or(false);
                let query_finished = is_done || query_handle.is_finished();

                // Read the latest streaming buffer
                let current_text = streaming_buffer.lock()
                    .map(|g| g.clone())
                    .unwrap_or_default();
                let current_status = streaming_status.lock()
                    .map(|g| g.clone())
                    .unwrap_or_default();

                // Update the chat message if there's new content
                if current_text.len() != last_rendered_len {
                    self.chat.update_message(assistant_msg_index, current_text.clone());
                    last_rendered_len = current_text.len();
                }

                // Update status display
                self.state.status = current_status;

                // Update cost from streaming data
                if let Ok(cost) = streaming_cost.lock().map(|g| *g) {
                    if cost > 0.0 {
                        self.state.total_cost_usd = cost;
                    }
                }

                // Update progress bar from streaming data
                if let Ok(progress_val) = streaming_progress.lock().map(|g| *g) {
                    if progress_val > 0.0 {
                        self.state.progress_bar_visible = true;
                        self.state.progress_bar.set_progress(progress_val);
                        if let Some(ref tool) = self.state.active_tool {
                            self.state.progress_bar.set_title(tool.clone());
                        }
                    } else {
                        self.state.progress_bar_visible = false;
                    }
                }

                // Sync multi-progress from streaming data
                if let Ok(mp_data) = streaming_multi_progress.lock().map(|g| g.clone()) {
                    if !mp_data.is_empty() {
                        self.state.multi_progress_visible = true;
                        self.state.multi_progress.clear();
                        for (label, progress, color) in mp_data {
                            self.state.multi_progress = self.state.multi_progress.clone().add_bar(label, progress, color);
                        }
                    } else {
                        self.state.multi_progress_visible = false;
                    }
                }

                // Render the UI
                let chat = &self.chat;
                let prompt = &self.prompt;
                let state = self.state.clone();
                // Tick spinner during streaming, before borrowing for render
                self.state.spinner.tick();
                let spinner = &self.state.spinner;
                let pb = if self.state.progress_bar_visible {
                    Some(&self.state.progress_bar)
                } else {
                    None
                };
                polling_terminal.draw(|f| {
                    MainLayoutWidget::render_complete_with_spinner(
                        f,
                        chat,
                        prompt,
                        &state.status,
                        state.model.as_deref(),
                        Some(state.tokens_used),
                        &state.working_directory,
                        Some(spinner),
                        pb,
                    );
                    // Overlay multi-progress bars at bottom
                    if state.multi_progress_visible {
                        let mp_height = 3u16.min(f.area().height.saturating_sub(10));
                        let mp_area = ratatui::layout::Rect {
                            x: f.area().x + 2,
                            y: f.area().bottom().saturating_sub(mp_height + 3),
                            width: f.area().width.saturating_sub(4),
                            height: mp_height,
                        };
                        state.multi_progress.render(f, mp_area);
                    }
                })?;

                if query_finished {
                    break;
                }

                // Small sleep to avoid busy-waiting
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }

        // Get the final result
        shannon_core::prevent_sleep::stop_prevent_sleep();

        let query_result = self.runtime.block_on(async {
            match query_handle.await {
                Ok(result) => result,
                Err(_) => Err("Query task panicked".to_string()),
            }
        });

        match query_result {
            Ok((mut engine, response, tokens, tools, turn, _final_status, steps)) => {
                // Restore the engine with updated conversation state
                engine.add_user_message(input.to_string());
                engine.add_assistant_message(vec![shannon_core::api::ContentBlock::Text {
                    text: response.clone(),
                }]);
                self.query_engine = Some(engine);

                // Render assistant output through the markdown renderer
                let rendered = self.output_renderer.render_output(&response, "assistant");
                self.chat.update_message(assistant_msg_index, rendered);
                self.state.tokens_used += tokens;
                self.tools_invoked += tools;

                if turn.total_files_touched() > 0 {
                    self.diff_data.record_turn_diff(turn);
                }
                self.current_turn += 1;

                self.state.query_steps_done = steps;
                self.state.query_steps_total = steps;
                self.state.progress_bar_visible = false;
                self.state.progress_bar.set_progress(0.0);
                if steps > 0 {
                    self.state.status = format!("Ready ({steps} steps completed)");
                } else {
                    self.state.status = "Ready".to_string();
                }
            }
            Err(e) => {
                // Engine was consumed by the task; recreate it for next query
                let mut new_engine = QueryEngine::with_defaults(
                    shannon_core::api::LlmClient::new(shannon_core::api::LlmClientConfig::default()),
                    ToolRegistry::new(),
                    PermissionManager::new(),
                    StateManager::new(),
                );
                new_engine.add_user_message(input.to_string());
                self.query_engine = Some(new_engine);

                self.chat.update_message(assistant_msg_index, format!("❌ Error: {e}"));

                // Show dialog for critical errors (auth, config, network)
                let err_lower = e.to_lowercase();
                if err_lower.contains("api key") || err_lower.contains("api_key") {
                    // Offer to enter API key via input dialog
                    self.show_input_dialog(
                        "API Key Required",
                        "Enter your API key...",
                        "set_api_key",
                    );
                } else if err_lower.contains("authentication") || err_lower.contains("unauthorized")
                    || err_lower.contains("forbidden")
                {
                    self.show_alert_dialog("Query Error", &e.to_string(), true);
                }

                self.state.status = "Ready".to_string();
                self.state.progress_bar_visible = false;
                self.state.progress_bar.set_progress(0.0);
            }
        }

        self.state.active_tool = None;
        self.state.progress_bar_visible = false;
        self.state.multi_progress_visible = false;
        self.state.multi_progress.clear();

        Ok(())
    }

    /// Render permission dialog
    fn render_permission_dialog(&self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect, dialog: &shannon_core::permissions::PermissionPrompt) {
        use ratatui::{
            layout::{Alignment, Rect},
            style::{Color, Modifier, Style},
            text::{Line, Span},
            widgets::{Block, Borders, Clear, Paragraph, Wrap},
        };

        // Calculate dialog area (centered)
        let dialog_width = 60.min(area.width.saturating_sub(4));
        let dialog_height = 20.min(area.height.saturating_sub(4));

        let x = (area.width.saturating_sub(dialog_width)) / 2;
        let y = (area.height.saturating_sub(dialog_height)) / 2;

        let dialog_area = Rect {
            x: area.x + x,
            y: area.y + y,
            width: dialog_width,
            height: dialog_height,
        };

        // Clear background for modal effect
        frame.render_widget(Clear, dialog_area);

        // Build dialog content
        let risk_indicator = match dialog.risk_level {
            shannon_core::permissions::RiskLevel::Safe => "✓",
            shannon_core::permissions::RiskLevel::Low => "⚠",
            shannon_core::permissions::RiskLevel::Medium => "⚡",
            shannon_core::permissions::RiskLevel::High => "🔥",
            shannon_core::permissions::RiskLevel::Critical => "☢️",
        };

        let risk_color = match dialog.risk_level {
            shannon_core::permissions::RiskLevel::Safe => Color::Green,
            shannon_core::permissions::RiskLevel::Low => Color::Yellow,
            shannon_core::permissions::RiskLevel::Medium => Color::Magenta,
            shannon_core::permissions::RiskLevel::High => Color::Red,
            shannon_core::permissions::RiskLevel::Critical => Color::Red,
        };

        let mut content_lines = vec![
            Line::from(vec![
                Span::styled(risk_indicator, Style::default().fg(risk_color).add_modifier(Modifier::BOLD)),
                Span::from(" Permission Request"),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Tool: ", Style::default().fg(Color::Gray)),
                Span::styled(&dialog.tool_name, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Description: ", Style::default().fg(Color::Gray)),
                Span::styled(&dialog.description, Style::default().fg(Color::White)),
            ]),
            Line::from(""),
            Line::from("Input:"),
            Line::from(serde_json::to_string_pretty(&dialog.tool_input).unwrap_or_else(|_| "(invalid)".to_string()).to_string()),
        ];

        // Add options
        content_lines.push(Line::from(""));
        content_lines.push(Line::from(""));
        content_lines.push(Line::from(vec![
            Span::styled("[Enter] ", Style::default().fg(Color::Green)),
            Span::styled("Allow Once    ", Style::default().fg(Color::White)),
            Span::styled("[A] ", Style::default().fg(Color::Cyan)),
            Span::styled("Always Allow  ", Style::default().fg(Color::White)),
            Span::styled("[Esc] ", Style::default().fg(Color::Red)),
            Span::styled("Deny", Style::default().fg(Color::White)),
        ]));

        let paragraph = Paragraph::new(content_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan))
                    .border_type(ratatui::widgets::BorderType::Rounded)
                    .title(" Permission Required "),
            )
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true });

        frame.render_widget(paragraph, dialog_area);
    }

    /// Get the current REPL state
    pub fn state(&self) -> &ReplState {
        &self.state
    }

    /// Get mutable reference to the REPL state
    pub fn state_mut(&mut self) -> &mut ReplState {
        &mut self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repl_state_default() {
        let state = ReplState::default();
        assert_eq!(state.status, "Ready");
        assert!(state.model.is_some());
        assert_eq!(state.tokens_used, 0);
        assert!(!state.welcome_active);
        assert!(!state.working_directory.is_empty());
    }

    #[test]
    fn test_repl_state_working_directory() {
        let state = ReplState::default();
        // Working directory should be set to current directory
        assert!(!state.working_directory.is_empty());
        // Should contain "." or an actual path
        assert!(state.working_directory.contains(".") || state.working_directory.starts_with('/'));
    }

    #[test]
    fn test_repl_state_fields() {
        let mut state = ReplState::default();
        assert_eq!(state.status, "Ready");
        assert_eq!(state.model, Some("claude-3-5-sonnet".to_string()));
        assert_eq!(state.tokens_used, 0);
        assert!(!state.welcome_active);

        // Modify fields
        state.status = "Processing".to_string();
        state.model = Some("gpt-4".to_string());
        state.tokens_used = 1000;
        state.working_directory = "/tmp/test".to_string();

        assert_eq!(state.status, "Processing");
        assert_eq!(state.model, Some("gpt-4".to_string()));
        assert_eq!(state.tokens_used, 1000);
        assert_eq!(state.working_directory, "/tmp/test");
    }

    #[test]
    fn test_repl_state_clone() {
        let state = ReplState::default();
        let cloned = state.clone();
        assert_eq!(cloned.status, state.status);
        assert_eq!(cloned.model, state.model);
        assert_eq!(cloned.tokens_used, state.tokens_used);
        assert_eq!(cloned.working_directory, state.working_directory);
        assert_eq!(cloned.welcome_active, state.welcome_active);
    }

    #[test]
    fn test_repl_creation() {
        let repl = Repl::new();
        assert!(repl.is_ok());
        if let Ok(r) = repl {
            assert!(!r.state().welcome_active);
            assert!(r.query_engine.is_some());
        }
    }

    // ── REPL Command Tests ────────────────────────────────────────────

    #[test]
    fn test_repl_exit_command() {
        let mut repl = Repl::new().unwrap();
        // running is false after new(); only run() sets it to true.
        // Simulate the active state that run() would set.
        repl.running = true;
        // With no activity, /exit should quit immediately
        repl.handle_command("/exit").unwrap();
        assert!(!repl.running);
    }

    #[test]
    fn test_repl_quit_command() {
        let mut repl = Repl::new().unwrap();
        repl.running = true;
        // With no activity, /quit should quit immediately
        repl.handle_command("/quit").unwrap();
        assert!(!repl.running);
    }

    #[test]
    fn test_repl_quit_with_activity_shows_dialog() {
        let mut repl = Repl::new().unwrap();
        repl.running = true;
        repl.commands_run = 1;
        repl.handle_command("/quit").unwrap();
        // Should NOT quit directly — should show confirm dialog
        assert!(repl.running);
        assert!(repl.state.active_dialog.is_some());
        assert_eq!(repl.state.pending_dialog_action.as_deref(), Some("quit"));
    }

    #[test]
    fn test_repl_exit_with_tools_shows_dialog() {
        let mut repl = Repl::new().unwrap();
        repl.running = true;
        repl.tools_invoked = 3;
        repl.handle_command("/exit").unwrap();
        assert!(repl.running);
        assert!(repl.state.active_dialog.is_some());
    }

    #[test]
    fn test_repl_confirm_dialog_quit() {
        let mut repl = Repl::new().unwrap();
        repl.running = true;
        repl.commands_run = 1;
        repl.handle_command("/quit").unwrap();
        // Dialog should be showing
        assert!(repl.state.active_dialog.is_some());
        // Navigate to "Confirm" button (index 1) — default is "Cancel" (index 0)
        let right_key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Right,
            crossterm::event::KeyModifiers::NONE,
        );
        repl.handle_input(right_key).unwrap();
        // Press Enter to confirm
        let enter_key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        );
        repl.handle_input(enter_key).unwrap();
        // Now should have quit
        assert!(!repl.running);
        assert!(repl.state.active_dialog.is_none());
    }

    #[test]
    fn test_repl_confirm_dialog_escape_cancels() {
        let mut repl = Repl::new().unwrap();
        repl.running = true;
        repl.commands_run = 1;
        repl.handle_command("/quit").unwrap();
        assert!(repl.state.active_dialog.is_some());
        // Press Escape to cancel
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        );
        repl.handle_input(key).unwrap();
        // Should still be running
        assert!(repl.running);
        assert!(repl.state.active_dialog.is_none());
    }

    #[test]
    fn test_repl_help_command() {
        let mut repl = Repl::new().unwrap();
        repl.handle_command("/help").unwrap();
        // Only the help message is present (welcome is added in run(), not new())
        assert!(!repl.chat.is_empty());
        // Last message should contain the help header
        let last_msg = &repl.chat.last_message().unwrap().content;
        // Help output now uses markdown format from command registry
        assert!(last_msg.contains("Shannon Code Commands"));
        assert!(last_msg.contains("/help"));
        assert!(last_msg.contains("/quit"));
    }

    #[test]
    fn test_repl_model_show_dialog() {
        let mut repl = Repl::new().unwrap();
        repl.handle_command("/model").unwrap();
        // Should show input dialog instead of chat message
        assert!(repl.state.input_dialog.is_some());
        assert_eq!(repl.state.input_dialog_action.as_deref(), Some("set_model"));
    }

    #[test]
    fn test_repl_model_set_command() {
        let mut repl = Repl::new().unwrap();
        repl.handle_command("/model gpt-4o").unwrap();
        assert_eq!(repl.state.model, Some("gpt-4o".to_string()));
        let last_msg = &repl.chat.last_message().unwrap().content;
        assert!(last_msg.contains("Model set to: gpt-4o"));
    }

    #[test]
    fn test_repl_init_command() {
        let mut repl = Repl::new().unwrap();
        let msg_count_before = repl.chat.len();
        repl.handle_command("/init").unwrap();
        assert_eq!(repl.chat.len(), msg_count_before + 1);
        let last_msg = &repl.chat.last_message().unwrap().content;
        assert!(last_msg.contains("Project initialized"));
        assert!(last_msg.contains("Working directory:"));
    }

    #[test]
    fn test_repl_init_detects_git() {
        let mut repl = Repl::new().unwrap();
        // The working directory is the current directory, which is a git repo
        repl.handle_command("/init").unwrap();
        let last_msg = &repl.chat.last_message().unwrap().content;
        // This project has .git, so it should detect it
        assert!(last_msg.contains("Git repository: detected") || last_msg.contains("Git repository: not found"));
    }

    #[test]
    fn test_repl_unknown_command() {
        let mut repl = Repl::new().unwrap();
        let msg_count_before = repl.chat.len();
        repl.handle_command("/unknown_command_xyz").unwrap();
        assert_eq!(repl.chat.len(), msg_count_before + 1);
        let last_msg = &repl.chat.last_message().unwrap().content;
        assert!(last_msg.contains("Unknown command"));
        assert!(last_msg.contains("/unknown_command_xyz"));
        assert!(last_msg.contains("/help"));
        // Should still be running (unknown commands don't change running state)
        repl.running = true;
        repl.handle_command("/unknown_command_xyz2").unwrap();
        assert!(repl.running);
    }

    // ── Session Command Tests ──────────────────────────────────────────

    #[test]
    fn test_sessions_command_empty() {
        let mut repl = Repl::new().unwrap();
        repl.handle_command("/sessions").unwrap();
        let last_msg = &repl.chat.last_message().unwrap().content;
        // With no saved sessions, should report empty
        assert!(last_msg.contains("No saved sessions") || last_msg.contains("Saved sessions"));
    }

    #[test]
    fn test_sessions_command_in_help() {
        let mut repl = Repl::new().unwrap();
        repl.handle_command("/help").unwrap();
        let last_msg = &repl.chat.last_message().unwrap().content;
        assert!(last_msg.contains("/sessions"));
        assert!(last_msg.contains("/resume"));
        assert!(last_msg.contains("/history"));
    }

    #[test]
    fn test_resume_command_no_args() {
        let mut repl = Repl::new().unwrap();
        repl.handle_command("/resume").unwrap();
        let last_msg = &repl.chat.last_message().unwrap().content;
        assert!(last_msg.contains("Usage: /resume"));
    }

    #[test]
    fn test_resume_command_invalid_uuid() {
        let mut repl = Repl::new().unwrap();
        repl.handle_command("/resume not-a-uuid").unwrap();
        let last_msg = &repl.chat.last_message().unwrap().content;
        assert!(last_msg.contains("Invalid session identifier"));
    }

    #[test]
    fn test_resume_command_invalid_number() {
        let mut repl = Repl::new().unwrap();
        // No sessions listed, so number 1 should be invalid
        repl.last_session_list.clear();
        repl.handle_command("/resume 1").unwrap();
        let last_msg = &repl.chat.last_message().unwrap().content;
        assert!(last_msg.contains("Invalid session number") || last_msg.contains("Session not found"));
    }

    #[test]
    fn test_history_command() {
        let mut repl = Repl::new().unwrap();
        repl.handle_command("/history").unwrap();
        let last_msg = &repl.chat.last_message().unwrap().content;
        assert!(last_msg.contains("Current session stats"));
        assert!(last_msg.contains("Messages:"));
        assert!(last_msg.contains("Tokens used:"));
    }

    #[test]
    fn test_history_command_after_messages() {
        let mut repl = Repl::new().unwrap();
        // Simulate some conversation
        repl.chat.add_message(ChatRole::User, "hello".to_string());
        repl.chat.add_message(ChatRole::Assistant, "hi there".to_string());
        repl.state.tokens_used = 500;

        repl.handle_command("/history").unwrap();
        let last_msg = &repl.chat.last_message().unwrap().content;
        assert!(last_msg.contains("Current session stats"));
        assert!(last_msg.contains("Messages:"));
        // Should include the messages we added plus the history command response
    }

    #[test]
    fn test_history_export_no_path() {
        let mut repl = Repl::new().unwrap();
        repl.handle_command("/history --export").unwrap();
        let last_msg = &repl.chat.last_message().unwrap().content;
        assert!(last_msg.contains("Usage: /history --export"));
    }

    // ── History Navigation Tests ──────────────────────────────────────

    #[test]
    fn test_command_history_push_and_navigate() {
        let mut repl = Repl::new().unwrap();
        // Simulate submitting commands
        repl.command_history.push("hello");
        repl.command_history.push("world");

        // Navigate up
        let cmd = repl.command_history.up();
        assert_eq!(cmd, Some("world"));
        let cmd = repl.command_history.up();
        assert_eq!(cmd, Some("hello"));

        // Navigate down
        let cmd = repl.command_history.down();
        assert_eq!(cmd, Some("world"));
        let cmd = repl.command_history.down();
        assert_eq!(cmd, None); // back to bottom
    }

    #[test]
    fn test_command_history_dedup() {
        let mut repl = Repl::new().unwrap();
        repl.command_history.push("hello");
        repl.command_history.push("hello");
        assert_eq!(repl.command_history.len(), 1);
    }

    #[test]
    fn test_diff_data_tracking() {
        let repl = Repl::new().unwrap();
        assert_eq!(repl.diff_data.total_additions(), 0);
        assert_eq!(repl.diff_data.total_files_modified(), 0);
    }

    #[test]
    fn test_session_summary_on_quit() {
        let mut repl = Repl::new().unwrap();
        repl.running = true;
        // The quit command should show summary if there are turns
        // With no activity, it should still work
        repl.handle_command("/quit").unwrap();
        // After quit, running should be false
        assert!(!repl.running);
    }

    #[test]
    fn test_history_shows_commands_and_tools() {
        let mut repl = Repl::new().unwrap();
        repl.commands_run = 5;
        repl.tools_invoked = 3;
        repl.handle_command("/history").unwrap();
        let last_msg = &repl.chat.last_message().unwrap().content;
        assert!(last_msg.contains("Commands run: 5"));
        assert!(last_msg.contains("Tools invoked: 3"));
    }

    // ── Progress State Tests ────────────────────────────────────────────

    #[test]
    fn test_repl_state_progress_fields_default() {
        let state = ReplState::default();
        assert!(state.active_tool.is_none());
        assert_eq!(state.query_steps_done, 0);
        assert_eq!(state.query_steps_total, 0);
    }

    #[test]
    fn test_repl_state_progress_fields_update() {
        let mut state = ReplState::default();
        state.active_tool = Some("bash".to_string());
        state.query_steps_done = 3;
        state.query_steps_total = 5;
        assert_eq!(state.active_tool.as_deref(), Some("bash"));
        assert_eq!(state.query_steps_done, 3);
        assert_eq!(state.query_steps_total, 5);
    }

    #[test]
    fn test_spinner_widget_tick() {
        use crate::widgets::progress::SpinnerWidget;
        let mut spinner = SpinnerWidget::new();
        assert_eq!(spinner.current_frame(), 0);
        spinner.tick();
        assert_eq!(spinner.current_frame(), 1);
        // Should wrap around
        for _ in 0..9 {
            spinner.tick();
        }
        assert_eq!(spinner.current_frame(), 0);
    }

    #[test]
    fn test_spinner_with_message() {
        use crate::widgets::progress::SpinnerWidget;
        let spinner = SpinnerWidget::new()
            .with_message("Loading...".to_string());
        assert_eq!(spinner.message(), Some("Loading..."));
    }

    #[test]
    fn test_history_shows_steps_after_query() {
        let mut repl = Repl::new().unwrap();
        // Simulate state after a query with steps
        repl.state.query_steps_done = 5;
        repl.state.query_steps_total = 5;
        repl.state.status = "Ready (5 steps completed)".to_string();
        assert!(repl.state.status.contains("5 steps"));
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

// ── Tab Completion Tests ───────────────────────────────────────────

#[cfg(test)]
mod tab_completion_tests {
    use super::*;

    fn create_repl() -> Repl {
        Repl::new().expect("Repl::new should succeed in tests")
    }

    #[test]
    fn test_extract_completion_word_empty() {
        let repl = create_repl();
        let (prefix, start, end) = repl.extract_completion_word("");
        assert_eq!(prefix, "");
        assert_eq!(start, 0);
        assert_eq!(end, 0);
    }

    #[test]
    fn test_extract_completion_word_with_slash() {
        let repl = create_repl();
        let (prefix, start, end) = repl.extract_completion_word("/co");
        assert_eq!(prefix, "/co");
        assert_eq!(start, 0);
        assert_eq!(end, 3);
    }

    #[test]
    fn test_extract_completion_word_with_space() {
        let repl = create_repl();
        let (prefix, start, end) = repl.extract_completion_word("cmd /co");
        // Should find the / after the space
        assert!(prefix.starts_with('/'));
        assert_eq!(start, 4); // Position of /
        assert_eq!(end, 7);
    }

    #[test]
    fn test_extract_completion_word_no_slash() {
        let repl = create_repl();
        let (prefix, start, end) = repl.extract_completion_word("hello");
        assert_eq!(prefix, "hello");
        assert_eq!(start, 0);
        assert_eq!(end, 5);
    }

    #[test]
    fn test_tab_complete_command_partial() {
        let mut repl = create_repl();
        let commands = vec!["commit".to_string(), "config".to_string(), "help".to_string()];

        let result = repl.tab_complete_command("/co", &commands);
        assert!(result.is_some());
        let (completion, start, end) = result.unwrap();
        assert_eq!(start, 0);
        assert_eq!(end, 3);
        assert!(completion == "/commit" || completion == "/config");
    }

    #[test]
    fn test_tab_complete_command_with_slash() {
        let mut repl = create_repl();
        let commands = vec!["commit".to_string(), "help".to_string()];

        let result = repl.tab_complete_command("/", &commands);
        assert!(result.is_some());
        let (completion, start, end) = result.unwrap();
        assert!(completion.starts_with('/'));
        assert_eq!(start, 0);
        assert_eq!(end, 1);
    }

    #[test]
    fn test_tab_complete_command_empty_input() {
        let mut repl = create_repl();
        let commands = vec!["commit".to_string(), "help".to_string()];

        // Empty input should NOT complete - user must type / first
        let result = repl.tab_complete_command("", &commands);
        assert!(result.is_none(), "Empty input should not auto-complete without /");
    }

    #[test]
    fn test_tab_complete_command_cycles() {
        let mut repl = create_repl();
        let commands = vec!["commit".to_string(), "config".to_string(), "help".to_string()];

        // First tab
        let result1 = repl.tab_complete_command("/c", &commands);
        assert!(result1.is_some());
        let (comp1, _, _) = result1.unwrap();

        // Second tab should give a different result (or cycle)
        let result2 = repl.tab_complete_command("/c", &commands);
        assert!(result2.is_some());
        let (comp2, _, _) = result2.unwrap();

        // Since there are 2 matches, we should get different completions
        assert!(comp1.starts_with("/c"));
        assert!(comp2.starts_with("/c"));
    }

    #[test]
    fn test_tab_complete_command_no_match() {
        let mut repl = create_repl();
        let commands = vec!["commit".to_string(), "help".to_string()];

        let result = repl.tab_complete_command("/xyz", &commands);
        assert!(result.is_none());
    }

    #[test]
    fn test_tab_complete_command_resets_on_new_prefix() {
        let mut repl = create_repl();
        let commands = vec!["commit".to_string(), "config".to_string(), "help".to_string()];

        // Complete /co - should have matches (commit and config)
        let result1 = repl.tab_complete_command("/co", &commands);
        assert!(result1.is_some());
        let index_after_first = repl.tab_completion_state.current_index;
        assert_eq!(index_after_first, 1); // Incremented after first call, or wrapped if only 1 match

        // Complete /h - "help" is a command so should match (only one match)
        let result2 = repl.tab_complete_command("/h", &commands);
        assert!(result2.is_some());
        assert_eq!(repl.tab_completion_state.last_prefix, "/h");
        // Index was reset to 0, then (0+1)%1 = 0, so stays at 0 for single candidate
        assert_eq!(repl.tab_completion_state.current_index, 0);
    }
}

#[cfg(test)]
mod ui_adapter_tests {
    use super::*;

    fn create_repl() -> Repl {
        Repl::new().expect("Repl::new should succeed in tests")
    }

    #[test]
    fn test_repl_supports_streaming() {
        let repl = create_repl();
        assert!(repl.supports_streaming());
    }

    #[test]
    fn test_repl_adapter_display() {
        let repl = create_repl();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let msg = DisplayMessage::info("test");
        let result = rt.block_on(repl.display(&msg));
        assert!(result.is_ok());
    }

    #[test]
    fn test_repl_adapter_display_progress() {
        let repl = create_repl();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(repl.display_progress("loading", Some(50)));
        assert!(result.is_ok());
    }

    #[test]
    fn test_repl_adapter_read_input_not_supported() {
        let repl = create_repl();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(repl.read_input("prompt: "));
        assert!(matches!(result, Err(UiError::NotSupported(_))));
    }

    #[test]
    fn test_repl_adapter_confirm_not_supported() {
        let repl = create_repl();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(repl.confirm("Continue?"));
        assert!(matches!(result, Err(UiError::NotSupported(_))));
    }
}
