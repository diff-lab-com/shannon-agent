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
    api::ClaudeClientConfig,
    permissions::{PermissionManager, PermissionPrompt, PermissionChoice, RiskLevel},
    query_engine::{QueryContext, QueryEngine, QueryEngineConfig, QueryEvent, PermissionRequest},
    state::StateManager,
    tools::ToolRegistry,
};
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
    /// Welcome screen active
    pub welcome_active: bool,
    /// Active permission dialog (if any)
    pub permission_dialog: Option<shannon_core::permissions::PermissionPrompt>,
    /// Permission response channel sender (if dialog is active)
    pub permission_response_tx: Option<tokio::sync::mpsc::UnboundedSender<shannon_core::permissions::PermissionChoice>>,
}

impl Default for ReplState {
    fn default() -> Self {
        Self {
            status: "Ready".to_string(),
            model: Some("claude-3-5-sonnet".to_string()),
            tokens_used: 0,
            welcome_active: true,
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

        // Create Claude client
        let client_config = ClaudeClientConfig::default();
        let client = shannon_core::api::ClaudeClient::new(client_config);

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

        Ok(Self {
            events: EventHandler::new(250)?,
            renderer: Renderer::new(),
            chat: ChatWidget::new(1000),
            prompt: PromptWidget::new(),
            state: ReplState::default(),
            running: false,
            query_engine: Some(query_engine),
            runtime,
            permission_req_rx,
            permission_req_tx,
        })
    }

    /// Run the main REPL loop
    pub fn run(&mut self) -> Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        self.running = true;

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
                if state.welcome_active {
                    crate::widgets::WelcomeWidget::render(f, f.area());
                } else if let Some(ref dialog) = state.permission_dialog {
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
                if self.state.welcome_active {
                    self.state.welcome_active = false;
                    self.chat.add_message(
                        ChatRole::System,
                        "Welcome to Shannon! Type your message and press Enter.".to_string(),
                    );
                } else {
                    self.submit_input()?;
                }
            }
            crossterm::event::KeyCode::Char(c) => {
                if !self.state.welcome_active {
                    self.prompt.add_char(c);
                }
            }
            crossterm::event::KeyCode::Backspace => {
                if !self.state.welcome_active {
                    self.prompt.backspace();
                }
            }
            crossterm::event::KeyCode::Up => {
                if !self.state.welcome_active {
                    self.chat.scroll_up();
                }
            }
            crossterm::event::KeyCode::Down => {
                if !self.state.welcome_active {
                    self.chat.scroll_down();
                }
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
        let parts: Vec<&str> = input.splitn(2, ' ').collect();
        let cmd = parts.first().copied().unwrap_or("");
        let args = parts.get(1).copied().unwrap_or("");

        match cmd {
            "/help" => {
                self.chat.add_message(
                    ChatRole::System,
                    "Available commands: /help, /clear, /quit, /model <name>".to_string(),
                );
            }
            "/clear" => {
                self.chat.clear();
                self.chat.add_message(
                    ChatRole::System,
                    "Chat cleared.".to_string(),
                );
            }
            "/quit" => {
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
            _ => {
                self.chat.add_message(
                    ChatRole::System,
                    format!("Unknown command: {}", cmd),
                );
            }
        }

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
        assert!(state.welcome_active);
    }

    #[test]
    fn test_repl_creation() {
        let repl = Repl::new();
        assert!(repl.is_ok());
        if let Ok(r) = repl {
            assert!(r.state().welcome_active);
            assert!(r.query_engine.is_some());
        }
    }
}
