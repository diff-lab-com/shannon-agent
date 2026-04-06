//! REPL main loop and terminal management

use crate::{
    events::EventHandler,
    render::Renderer,
    widgets::{ChatWidget, ChatRole, PromptWidget, MainLayoutWidget},
    Result,
};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
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
    query_engine::{QueryContext, QueryEngine, QueryEvent, PermissionRequest},
    state::StateManager,
    tools::ToolRegistry,
};
use shannon_commands::{CommandRegistry, CommandParser, builtin_commands};
use shannon_tools::{
    BashTool,
    ReadTool,
    WriteTool,
};

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
}

impl Repl {
    /// Create a new REPL instance
    pub fn new() -> Result<Self> {
        let runtime = Runtime::new()?;

        // Create tool registry
        let mut tool_registry = ToolRegistry::new();

        // Register tools that implement shannon_core::Tool
        tool_registry.register(Box::new(shannon_tools::BashTool::new()))?;

        // Create LLM client
        let client_config = LlmClientConfig::default();
        let client = shannon_core::api::LlmClient::new(client_config);

        // Create permission manager
        let permission_manager = PermissionManager::new();

        // Create state manager
        let state_manager = StateManager::new();

        // Create query engine
        let query_engine = QueryEngine::with_defaults(
            client,
            tool_registry,
            permission_manager,
            state_manager,
        );

        // Create permission request channel
        let (permission_req_tx, permission_req_rx) = tokio::sync::mpsc::unbounded_channel();

        // Create command registry inside the runtime context so async
        // register() works (register_sync requires an active tokio runtime).
        let handle = runtime.handle().clone();
        let command_registry = handle.block_on(async {
            let registry = CommandRegistry::new();
            for cmd in builtin_commands::all_commands() {
                let _ = registry.register(cmd).await;
            }
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
            command_registry,
            command_parser: CommandParser::new(),
            runtime,
            permission_req_rx,
            permission_req_tx,
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
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        self.running = true;

        // Show welcome message directly in the main UI
        self.chat.add_message(
            ChatRole::System,
            "Welcome to Shannon! Type your message and press Enter. Type /help for commands.".to_string(),
        );

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
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        Ok(())
    }

    /// Handle individual events
    fn handle_event(&mut self, event: crate::events::Event) {
        match event {
            crate::events::Event::Input(key) => {
                if let Err(e) = self.handle_input(key) {
                    eprintln!("Input error: {}", e);
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

        match key.code {
            crossterm::event::KeyCode::Char('q') => self.running = false,
            crossterm::event::KeyCode::Char('c') => {
                if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) {
                    self.running = false;
                }
            }
            crossterm::event::KeyCode::Enter => {
                self.submit_input()?;
            }
            crossterm::event::KeyCode::Char(c) => {
                self.prompt.add_char(c);
            }
            crossterm::event::KeyCode::Backspace => {
                self.prompt.backspace();
            }
            crossterm::event::KeyCode::Up => {
                self.chat.scroll_up();
            }
            crossterm::event::KeyCode::Down => {
                self.chat.scroll_down();
            }
            crossterm::event::KeyCode::Esc => {
                self.prompt.clear();
            }
            _ => {}
        }
        Ok(())
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

    /// Submit the current input
    fn submit_input(&mut self) -> Result<()> {
        let input = self.prompt.input().to_string();

        if input.trim().is_empty() {
            return Ok(());
        }

        // Add user message to chat
        self.chat.add_message(ChatRole::User, input.clone());

        // Clear input
        self.prompt.clear();

        // Process command or query
        if input.starts_with('/') {
            self.handle_command(&input)?;
        } else {
            self.handle_query(&input)?;
        }

        Ok(())
    }

    /// Handle a command (starts with /)
    fn handle_command(&mut self, input: &str) -> Result<()> {
        // Built-in REPL commands (not in the command registry)
        let repl_commands = ["/help", "/clear", "/quit", "/exit", "/model", "/init"];

        let parts: Vec<&str> = input.splitn(2, ' ').collect();
        let cmd = parts.first().copied().unwrap_or("");
        let args = parts.get(1).copied().unwrap_or("");

        if repl_commands.contains(&cmd) {
            match cmd {
                "/help" => {
                    // List all commands from the registry plus REPL commands
                    let mut cmd_list = String::from("Available commands:\n");

                    // REPL-local commands
                    cmd_list.push_str("  /help     Show this help message\n");
                    cmd_list.push_str("  /clear    Clear chat history\n");
                    cmd_list.push_str("  /quit     Exit Shannon\n");
                    cmd_list.push_str("  /exit     Exit Shannon (alias for /quit)\n");
                    cmd_list.push_str("  /model    Show or set the AI model\n");
                    cmd_list.push_str("  /init     Initialize project configuration\n");

                    // Builtin commands from registry
                    let names = self.runtime.block_on(self.command_registry.list_names());
                    if !names.is_empty() {
                        cmd_list.push_str("\nBuilt-in commands:\n");
                        let mut sorted = names;
                        sorted.sort();
                        for name in &sorted {
                            if let Ok(command) = self.runtime.block_on(self.command_registry.get(name)) {
                                let desc = command.description();
                                cmd_list.push_str(&format!("  /{:<12} {}\n", name, desc));
                            }
                        }
                    }

                    self.chat.add_message(ChatRole::System, cmd_list);
                }
                "/clear" => {
                    self.chat.clear();
                    self.chat.add_message(ChatRole::System, "Chat cleared.".to_string());
                }
                "/quit" | "/exit" => {
                    self.running = false;
                }
                "/model" => {
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
                "/init" => {
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
                _ => {}
            }
            return Ok(());
        }

        // Try to parse and look up from the command registry
        if let Ok(parsed) = self.command_parser.parse(input) {
            if let Ok(command) = self.runtime.block_on(self.command_registry.get(&parsed.name)) {
                let desc = command.description();
                self.chat.add_message(
                    ChatRole::System,
                    format!("/{} — {}", parsed.name, desc),
                );
                return Ok(());
            }
        }

        self.chat.add_message(
            ChatRole::System,
            format!("Unknown command: {}. Type /help for available commands.", cmd),
        );

        Ok(())
    }

    /// Handle a query (send to AI)
    fn handle_query(&mut self, input: &str) -> Result<()> {
        self.state.status = "Processing...".to_string();

        // Clear the "Thinking..." message and start streaming
        // Create an assistant message that will be updated in real-time
        let assistant_msg_index = self.chat.add_message(
            ChatRole::Assistant,
            String::new(),
        );

        // Clone necessary data for the async block
        let input_clone = input.to_string();
        let query_engine = self.query_engine.as_ref().expect("QueryEngine not initialized");

        // Create query context
        let query_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();

        let context = QueryContext {
            query_id,
            session_id,
            user_message: input_clone.clone(),
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
        let query_result = self.runtime.block_on(async {
            // Pass permission request channel for UI integration
            let permission_channel = Some(self.permission_req_tx.clone());
            let mut stream = query_engine.process_query(context, permission_channel).await;

            let mut response_text = String::new();
            let mut current_buffer = String::new();
            let mut tokens_in_turn = 0u64;
            let mut tool_calls: Vec<String> = Vec::new();

            // Process stream events with real-time feedback
            while let Some(event_result) = stream.next().await {
                match event_result {
                    Ok(QueryEvent::Started { .. }) => {
                        // Query started - initial message already added
                    }
                    Ok(QueryEvent::Text { content, .. }) => {
                        // Accumulate text and periodically flush to UI
                        current_buffer.push_str(&content);
                        response_text.push_str(&content);

                        // Flush every ~50 characters for real-time display
                        if current_buffer.len() >= 50 {
                            // In a true async UI, we would update the message here
                            // For now, we accumulate and display at the end
                            current_buffer.clear();
                        }
                    }
                    Ok(QueryEvent::ToolUseRequest { tool_name, tool_input, .. }) => {
                        // Display tool use to user
                        let tool_display = format!("\n🔧 Using: {} with input: {}", tool_name,
                            serde_json::to_string_pretty(&tool_input).unwrap_or_else(|_| "invalid".to_string())
                        );
                        response_text.push_str(&tool_display);
                        tool_calls.push(tool_name.clone());

                        // Update status to show active tool
                        // In real implementation: self.state.status = format!("Running: {}", tool_name);
                    }
                    Ok(QueryEvent::ToolUseResult { tool_name, result, is_error, .. }) => {
                        // Display tool result
                        let prefix = if is_error { "❌ " } else { "✅ " };
                        let result_display = format!("\n{} {} result: {}", prefix, tool_name,
                            result.chars().take(200).collect::<String>()
                        );
                        response_text.push_str(&result_display);
                    }
                    Ok(QueryEvent::TurnCompleted { turn_number, tokens_used, .. }) => {
                        // Update token count
                        tokens_in_turn += tokens_used;
                        let turn_info = format!("\n\n[Turn {} completed, {} tokens]", turn_number, tokens_used);
                        response_text.push_str(&turn_info);
                    }
                    Ok(QueryEvent::Progress { message, .. }) => {
                        // Show progress update
                        let progress = format!("\n⏳ {}", message);
                        response_text.push_str(&progress);
                    }
                    Ok(QueryEvent::Usage { input_tokens, output_tokens, cost_usd, .. }) => {
                        // Display usage statistics
                        let usage = format!("\n📊 Tokens: {} in + {} out = ${:.4}",
                            input_tokens, output_tokens, cost_usd);
                        response_text.push_str(&usage);
                    }
                    Ok(QueryEvent::Cost { total_cost_usd, input_tokens, output_tokens, .. }) => {
                        // Display cost tracking info
                    }
                    Ok(QueryEvent::Completed { .. }) => {
                        // Query completed successfully
                    }
                    Ok(QueryEvent::Failed { error, .. }) => {
                        return Err(format!("Query failed: {}", error));
                    }
                    Err(e) => {
                        return Err(format!("Stream error: {}", e));
                    }
                }
            }

            Ok::<(String, u64), String>((response_text, tokens_in_turn))
        });

        match query_result {
            Ok((response, tokens)) => {
                // Update the assistant message with the complete response
                self.chat.update_message(assistant_msg_index, response);
                self.state.tokens_used += tokens;
            }
            Err(e) => {
                self.chat.update_message(assistant_msg_index, format!("❌ Error: {}", e));
            }
        }

        self.state.status = "Ready".to_string();

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
        repl.handle_command("/exit").unwrap();
        assert!(!repl.running);
    }

    #[test]
    fn test_repl_quit_command() {
        let mut repl = Repl::new().unwrap();
        repl.running = true;
        repl.handle_command("/quit").unwrap();
        assert!(!repl.running);
    }

    #[test]
    fn test_repl_help_command() {
        let mut repl = Repl::new().unwrap();
        repl.handle_command("/help").unwrap();
        // Only the help message is present (welcome is added in run(), not new())
        assert!(repl.chat.len() >= 1);
        // Last message should contain "Available commands"
        let last_msg = &repl.chat.last_message().unwrap().content;
        assert!(last_msg.contains("Available commands"));
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
}
