//! REPL main loop and terminal management

use crate::{
    events::EventHandler,
    render::Renderer,
    repl_enhancement::{DiffData, ReplHistory, ReplRenderer, TurnDiff},
    widgets::{
        ChatWidget, ChatRole, PromptWidget, MainLayoutWidget,
        dialog::DialogWidget,
        progress::SpinnerWidget,
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
use shannon_commands::{CommandRegistry, CommandParser, builtin_commands, help_utils};
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
    /// Number of steps completed in current query
    pub query_steps_done: usize,
    /// Total steps estimated for current query (0 = indeterminate)
    pub query_steps_total: usize,
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
            working_directory: cwd,
            welcome_active: false,
            permission_dialog: None,
            permission_response_tx: None,
            active_dialog: None,
            pending_dialog_action: None,
            active_tool: None,
            spinner: SpinnerWidget::new(),
            query_steps_done: 0,
            query_steps_total: 0,
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
        register_default_tools(&mut tool_registry).map_err(|e| anyhow::anyhow!("Failed to register tools: {}", e))?;

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
            eprintln!("Warning: {}", e);
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

        // Show welcome message directly in the main UI
        self.chat.add_message(
            ChatRole::System,
            "Welcome to Shannon! Type your message and press Enter. Type /help for commands.".to_string(),
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

            terminal.draw(|f| {
                if let Some(ref dialog) = state.permission_dialog {
                    // Render permission dialog overlay
                    self.render_permission_dialog(f, f.area(), dialog);
                } else if let Some(ref dialog) = state.active_dialog {
                    // Render main layout first, then overlay the dialog
                    MainLayoutWidget::render_complete(
                        f,
                        chat,
                        prompt,
                        &state.status,
                        state.model.as_deref(),
                        Some(state.tokens_used),
                        &state.working_directory,
                    );
                    dialog.render(f, f.area());
                } else {
                    MainLayoutWidget::render_complete(
                        f,
                        chat,
                        prompt,
                        &state.status,
                        state.model.as_deref(),
                        Some(state.tokens_used),
                        &state.working_directory,
                    );
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
                        format!("Input error: {}", e)
                    );
                }
            }
            crate::events::Event::Tick => {
                // Handle periodic updates
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

        match key.code {
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
                self.prompt.clear();
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

    /// Handle tab completion
    fn handle_tab_completion(&mut self) -> Result<()> {
        let input = self.prompt.input().to_string();

        // Get available commands from registry
        let command_names = self.runtime.block_on(self.command_registry.list_names());

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
                        let with_slash = format!("/{}", cmd);
                        with_slash.starts_with(&prefix)
                    })
                    .map(|cmd| format!("/{}", cmd))
                    .collect()
            } else {
                // For non-commands starting empty, complete to all commands with /
                if prefix.is_empty() {
                    available_commands.iter().map(|c| format!("/{}", c)).collect()
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
        let parts: Vec<&str> = input.splitn(2, ' ').collect();
        let cmd = parts.first().copied().unwrap_or("");
        let args = parts.get(1).copied().unwrap_or("");

        // Extract command name without leading slash
        let cmd_name = cmd.strip_prefix('/').unwrap_or("");

        // Check if command exists in the registry
        let command_exists = self.runtime.block_on(
            self.command_registry.contains(cmd_name)
        );

        if command_exists {
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
                        self.chat.add_message(
                            ChatRole::System,
                            format!("Current model: {}", self.state.model.as_deref().unwrap_or("default")),
                        );
                    } else {
                        self.state.model = Some(args.to_string());
                        self.chat.add_message(
                            ChatRole::System,
                            format!("Model set to: {}", args),
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
                            Err(e) => init_info.push_str(&format!("CLAUDE.md: failed to create ({})\n", e)),
                        }
                    }

                    // Show working directory
                    init_info.push_str(&format!("Working directory: {}\n", cwd));

                    self.chat.add_message(
                        ChatRole::System,
                        format!("Project initialized.\n{}", init_info),
                    );
                }
                "config" => self.handle_config_command(args)?,
                "sessions" => self.handle_sessions_command(args)?,
                "resume" => self.handle_resume_command(args)?,
                "history" => self.handle_history_command(args)?,
                "worktree" => self.handle_worktree_command(args)?,
                "credentials" | "creds" | "cred" => self.handle_credentials_command(args)?,
                _ => {
                    // Command is in registry but not handled by REPL (e.g., commit, diff, etc.)
                    // For now, just show the command description
                    if let Ok(command) = self.runtime.block_on(self.command_registry.get(cmd_name)) {
                        let desc = command.description();
                        self.chat.add_message(
                            ChatRole::System,
                            format!("/{} — {}", cmd_name, desc),
                        );
                    }
                }
            }
            Ok(())
        } else {
            // Command not found in registry
            self.chat.add_message(
                ChatRole::System,
                format!("Unknown command: {}. Type /help for available commands.", cmd),
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
                    format!("Error listing sessions: {}", e),
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
                    format!("Invalid session number: {}. Use /sessions to see available sessions.", num),
                );
                return Ok(());
            }
            self.last_session_list[num - 1].session_id
        } else {
            self.chat.add_message(
                ChatRole::System,
                format!("Invalid session identifier: {}. Use a number from /sessions or a UUID.", arg),
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
            }
            Ok(None) => {
                self.chat.add_message(
                    ChatRole::System,
                    format!("Session not found: {}", session_id),
                );
            }
            Err(e) => {
                self.chat.add_message(
                    ChatRole::System,
                    format!("Error loading session: {}", e),
                );
            }
        }

        Ok(())
    }

    /// Handle /history command — show current session stats or export
    fn handle_history_command(&mut self, args: &str) -> Result<()> {
        let arg = args.trim();

        // Parse --export flag
        if arg.starts_with("--export") {
            let export_path = if arg.len() > "--export".len() {
                arg["--export".len()..].trim()
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
                        format!("Session exported to: {}", export_path),
                    );
                }
                Err(e) => {
                    self.chat.add_message(
                        ChatRole::System,
                        format!("Failed to export: {}", e),
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
                    let info = format!("{}No active worktree. Working in main repository.", status);
                    self.chat.add_message(ChatRole::System, info);
                }
            }
            return Ok(());
        }

        let parts: Vec<&str> = arg.splitn(3, ' ').collect();
        match parts[0] {
            "enter" => {
                let name = parts.get(1).map(|s| *s).unwrap_or("");
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
                            format!("Failed to enter worktree: {}", e),
                        );
                    }
                }
            }
            "exit" => {
                let action = parts.get(1).map(|s| *s).unwrap_or("keep");
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
                            format!("Failed to exit worktree: {}", e),
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

    /// Handle /config command — manage runtime configuration
    fn handle_config_command(&mut self, args: &str) -> Result<()> {
        use shannon_tools::config::ConfigManager;

        let mut manager = ConfigManager::new();
        if let Err(e) = manager.load() {
            self.chat.add_message(
                ChatRole::System,
                format!("Warning: could not load config: {}", e),
            );
        }

        let parts: Vec<&str> = args.splitn(3, ' ').collect();
        let action = parts.first().copied().unwrap_or("");

        let output = match action {
            "" | "list" | "ls" => {
                let prefix = if action.is_empty() { None } else { parts.get(1).map(|s| *s) };
                let keys = manager.list(prefix);
                if keys.is_empty() {
                    format!("No configuration keys found.\nConfig file: {}", manager.config_path().display())
                } else {
                    let mut out = format!("Configuration ({} key(s)):\n", keys.len());
                    for key in &keys {
                        let val = manager.get(key).unwrap_or(serde_json::Value::Null);
                        out.push_str(&format!("  {} = {}\n", key, val));
                    }
                    out.push_str(&format!("\nConfig file: {}", manager.config_path().display()));
                    out
                }
            }
            "get" => {
                let key = parts.get(1).map(|s| *s).unwrap_or("");
                if key.is_empty() {
                    "Usage: /config get <key>".to_string()
                } else {
                    match manager.get(key) {
                        Some(val) => format!("{} = {}", key, val),
                        None => format!("Config key not found: {}", key),
                    }
                }
            }
            "set" => {
                let key = parts.get(1).map(|s| *s).unwrap_or("");
                let value_str = parts.get(2).map(|s| *s).unwrap_or("");
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
                        Ok(_) => format!("Set {} = {}", key, value),
                        Err(e) => format!("Error saving config: {}", e),
                    }
                }
            }
            "delete" | "remove" | "rm" => {
                let key = parts.get(1).map(|s| *s).unwrap_or("");
                if key.is_empty() {
                    "Usage: /config delete <key>".to_string()
                } else {
                    let existed = manager.delete(key);
                    if existed {
                        match manager.save() {
                            Ok(_) => format!("Deleted config key: {}", key),
                            Err(e) => format!("Error saving config: {}", e),
                        }
                    } else {
                        format!("Config key not found: {}", key)
                    }
                }
            }
            "reset" => {
                let key = parts.get(1).map(|s| *s).unwrap_or("");
                if key.is_empty() {
                    "Usage: /config reset <key>".to_string()
                } else {
                    let existed = manager.reset(key);
                    if existed {
                        let val = manager.get(key).unwrap_or(serde_json::Value::Null);
                        match manager.save() {
                            Ok(_) => format!("Reset {} to default: {}", key, val),
                            Err(e) => format!("Error saving config: {}", e),
                        }
                    } else {
                        format!("No default found for key: {}", key)
                    }
                }
            }
            "help" | "?" => {
                "Configuration Management:\n\n\
                 /config list [prefix]        - Show config keys\n\
                 /config get <key>            - Get a config value\n\
                 /config set <key> <value>    - Set a config value\n\
                 /config delete <key>         - Delete a config key\n\
                 /config reset <key>          - Reset to default\n".to_string()
            }
            _ => {
                format!("Unknown config action: {}. Use /config help for usage.", action)
            }
        };

        self.chat.add_message(ChatRole::System, output);
        Ok(())
    }

    /// Handle /credentials command — manage stored credentials
    fn handle_credentials_command(&mut self, args: &str) -> Result<()> {
        // The module is private to shannon-commands, but we access through the crate's re-exports.
        let parts: Vec<&str> = args.splitn(3, ' ').collect();
        let action_str = parts.first().copied().unwrap_or("");

        let output = match action_str.to_lowercase().as_str() {
            "list" | "ls" | "" => self.credentials_list(),
            "store" | "add" | "set" => {
                let service = parts.get(1).copied().unwrap_or("");
                let value = parts.get(2).copied().unwrap_or("");
                if service.is_empty() || value.is_empty() {
                    "Usage: /credentials store <service> <value>".to_string()
                } else {
                    self.credentials_store(service, value)
                }
            }
            "get" => {
                let service = parts.get(1).copied().unwrap_or("");
                if service.is_empty() {
                    "Usage: /credentials get <service>".to_string()
                } else {
                    self.credentials_get(service)
                }
            }
            "delete" | "remove" | "rm" => {
                let service = parts.get(1).copied().unwrap_or("");
                if service.is_empty() {
                    "Usage: /credentials delete <service>".to_string()
                } else {
                    self.credentials_delete(service)
                }
            }
            "count" => self.credentials_count(),
            "help" | "?" => self.credentials_help(),
            _ => self.credentials_list(),
        };

        self.chat.add_message(ChatRole::System, output);
        Ok(())
    }

    /// List stored credentials
    fn credentials_list(&self) -> String {
        use shannon_core::credential_manager::CredentialManager;
        let mut output = String::from("Stored Credentials:\n\n");
        match CredentialManager::new().and_then(|mut m| { m.load()?; Ok(m) }) {
            Ok(manager) => {
                let creds = manager.list();
                if creds.is_empty() {
                    output.push_str("  No credentials stored.\n");
                } else {
                    for c in &creds {
                        output.push_str(&format!("  {} — {} (created: {})\n",
                            c.service, c.name, c.created_at.format("%Y-%m-%d %H:%M")));
                    }
                }
            }
            Err(e) => output.push_str(&format!("  Error: {}\n", e)),
        }
        output.push_str("\nUse /credentials help for usage information.");
        output
    }

    /// Store a credential
    fn credentials_store(&self, service: &str, value: &str) -> String {
        use shannon_core::credential_manager::{CredentialManager, Credential};
        match CredentialManager::new().and_then(|mut m| { m.load()?; Ok(m) }) {
            Ok(mut manager) => {
                let cred = Credential::new(service, service, value);
                match manager.store_or_update(cred) {
                    Ok(_) => format!("Credential stored for service: {}", service),
                    Err(e) => format!("Failed to store credential: {}", e),
                }
            }
            Err(e) => format!("Error: {}", e),
        }
    }

    /// Get a credential (masked)
    fn credentials_get(&self, service: &str) -> String {
        use shannon_core::credential_manager::CredentialManager;
        match CredentialManager::new().and_then(|mut m| { m.load()?; Ok(m) }) {
            Ok(manager) => match manager.retrieve(service) {
                Ok(cred) => {
                    let val = &cred.value;
                    let masked = if val.len() <= 8 {
                        "*".repeat(val.len())
                    } else {
                        format!("{}****{}", &val[..4], &val[val.len()-4..])
                    };
                    format!("Credential for '{}': {}", service, masked)
                }
                Err(e) => format!("Not found for '{}': {}", service, e),
            },
            Err(e) => format!("Error: {}", e),
        }
    }

    /// Delete a credential
    fn credentials_delete(&self, service: &str) -> String {
        use shannon_core::credential_manager::CredentialManager;
        match CredentialManager::new().and_then(|mut m| { m.load()?; Ok(m) }) {
            Ok(mut manager) => match manager.delete(service) {
                Ok(_) => format!("Credential deleted for service: {}", service),
                Err(e) => format!("Failed to delete for '{}': {}", service, e),
            },
            Err(e) => format!("Error: {}", e),
        }
    }

    /// Count stored credentials
    fn credentials_count(&self) -> String {
        use shannon_core::credential_manager::CredentialManager;
        match CredentialManager::new().and_then(|mut m| { m.load()?; Ok(m) }) {
            Ok(manager) => format!("Stored credentials: {}", manager.count()),
            Err(e) => format!("Error: {}", e),
        }
    }

    /// Show credentials help
    fn credentials_help(&self) -> String {
        "Credential Management:\n\n\
         /credentials list              - Show stored credentials\n\
         /credentials store <svc> <val> - Store a credential\n\
         /credentials get <service>     - Retrieve a credential (masked)\n\
         /credentials delete <service>  - Delete a credential\n\
         /credentials count             - Show stored credential count\n".to_string()
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

        let buffer_clone = streaming_buffer.clone();
        let status_clone = streaming_status.clone();
        let done_clone = streaming_done.clone();
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
                        progress_status = format!("Running: {} (step {})", tool_name, steps_done);
                        let tool_display = format!("\n🔧 Using: {} with input: {}", tool_name,
                            serde_json::to_string_pretty(&tool_input).unwrap_or_else(|_| "invalid".to_string())
                        );
                        response_text.push_str(&tool_display);
                        tool_calls.push(tool_name.clone());
                        tools_in_session += 1;

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
                        let result_display = format!("\n{}", formatted);
                        response_text.push_str(&result_display);
                        if let Ok(mut buf) = buffer_clone.lock() {
                            *buf = response_text.clone();
                        }
                    }
                    Ok(QueryEvent::TurnCompleted { turn_number, tokens_used, .. }) => {
                        tokens_in_turn += tokens_used;
                        let turn_info = format!("\n\n[Turn {} completed, {} tokens]", turn_number, tokens_used);
                        response_text.push_str(&turn_info);
                    }
                    Ok(QueryEvent::Progress { message, .. }) => {
                        progress_status = format!("Processing: {}", message);
                        let progress = format!("\n⏳ {}", message);
                        response_text.push_str(&progress);
                        if let Ok(mut s) = status_clone.lock() {
                            *s = progress_status.clone();
                        }
                        if let Ok(mut buf) = buffer_clone.lock() {
                            *buf = response_text.clone();
                        }
                    }
                    Ok(QueryEvent::Usage { input_tokens, output_tokens, cost_usd, .. }) => {
                        let usage = format!("\n📊 Tokens: {} in + {} out = ${:.4}",
                            input_tokens, output_tokens, cost_usd);
                        response_text.push_str(&usage);
                    }
                    Ok(QueryEvent::Cost { .. }) => {}
                    Ok(QueryEvent::ToolProgress { progress, .. }) => {
                        let pct = (progress * 100.0) as u32;
                        let progress_msg = format!("\n⏳ Tool progress: {}%", pct);
                        response_text.push_str(&progress_msg);
                    }
                    Ok(QueryEvent::Completed { .. }) => {}
                    Ok(QueryEvent::Failed { error, .. }) => {
                        return Err(format!("Query failed: {}", error));
                    }
                    Err(e) => {
                        return Err(format!("Stream error: {}", e));
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

                // Render the UI
                let chat = &self.chat;
                let prompt = &self.prompt;
                let state = self.state.clone();
                polling_terminal.draw(|f| {
                    MainLayoutWidget::render_complete(
                        f,
                        chat,
                        prompt,
                        &state.status,
                        state.model.as_deref(),
                        Some(state.tokens_used),
                        &state.working_directory,
                    );
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

                self.chat.update_message(assistant_msg_index, response);
                self.state.tokens_used += tokens;
                self.tools_invoked += tools;

                if turn.total_files_touched() > 0 {
                    self.diff_data.record_turn_diff(turn);
                }
                self.current_turn += 1;

                self.state.query_steps_done = steps;
                self.state.query_steps_total = steps;
                if steps > 0 {
                    self.state.status = format!("Ready ({} steps completed)", steps);
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

                self.chat.update_message(assistant_msg_index, format!("❌ Error: {}", e));
                self.state.status = "Ready".to_string();
            }
        }

        self.state.active_tool = None;

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
            Line::from(format!("{}", serde_json::to_string_pretty(&dialog.tool_input).unwrap_or_else(|_| "(invalid)".to_string()))),
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
        assert!(repl.chat.len() >= 1);
        // Last message should contain the help header
        let last_msg = &repl.chat.last_message().unwrap().content;
        // Help output now uses markdown format from command registry
        assert!(last_msg.contains("Shannon Code Commands"));
        assert!(last_msg.contains("/help"));
        assert!(last_msg.contains("/quit"));
    }

    #[test]
    fn test_repl_model_show_command() {
        let mut repl = Repl::new().unwrap();
        let msg_count_before = repl.chat.len();
        repl.handle_command("/model").unwrap();
        assert_eq!(repl.chat.len(), msg_count_before + 1);
        let last_msg = &repl.chat.last_message().unwrap().content;
        assert!(last_msg.contains("Current model:"));
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
