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
    permissions::PermissionManager,
    query_engine::{QueryContext, QueryEngine, QueryEngineConfig, QueryEvent},
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
}

impl Default for ReplState {
    fn default() -> Self {
        Self {
            status: "Ready".to_string(),
            model: Some("claude-3-5-sonnet".to_string()),
            tokens_used: 0,
            welcome_active: true,
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

        Ok(Self {
            events: EventHandler::new(250)?,
            renderer: Renderer::new(),
            chat: ChatWidget::new(1000),
            prompt: PromptWidget::new(),
            state: ReplState::default(),
            running: false,
            query_engine: Some(query_engine),
            runtime,
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
            // Draw UI
            let chat = &self.chat;
            let prompt = &self.prompt;
            let state = self.state.clone();

            terminal.draw(|f| {
                if state.welcome_active {
                    crate::widgets::WelcomeWidget::render(f, f.area());
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

        // Add system message indicating processing
        self.chat.add_message(
            ChatRole::System,
            "Thinking...".to_string(),
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

        // Process query synchronously (in real implementation, use async properly)
        // For now, we'll use a blocking approach within the runtime
        let query_result = self.runtime.block_on(async {
            let mut stream = query_engine.process_query(context).await;

            let mut response_text = String::new();
            let mut completed = false;

            // Process stream events
            while let Some(event_result) = stream.next().await {
                match event_result {
                    Ok(QueryEvent::Started { .. }) => {
                        // Query started
                    }
                    Ok(QueryEvent::Text { content, .. }) => {
                        // Append text and update UI
                        response_text.push_str(&content);
                    }
                    Ok(QueryEvent::ToolUseRequest { tool_name, tool_input, .. }) => {
                        // Tool use requested - in production, execute tool here
                    }
                    Ok(QueryEvent::ToolUseResult { tool_name, result, is_error, .. }) => {
                        // Tool execution result
                    }
                    Ok(QueryEvent::TurnCompleted { tokens_used, .. }) => {
                        // Update token count
                    }
                    Ok(QueryEvent::Completed { .. }) => {
                        completed = true;
                    }
                    Ok(QueryEvent::Failed { error, .. }) => {
                        return Err(format!("Query failed: {}", error));
                    }
                    Ok(QueryEvent::Progress { message, .. }) => {
                        // Progress update
                    }
                    Ok(QueryEvent::Usage { .. }) => {
                        // Usage statistics
                    }
                    Err(e) => {
                        return Err(format!("Stream error: {}", e));
                    }
                }
            }

            Ok::<String, String>(response_text)
        });

        match query_result {
            Ok(response) => {
                self.chat.add_message(
                    ChatRole::Assistant,
                    response,
                );
            }
            Err(e) => {
                self.chat.add_message(
                    ChatRole::System,
                    format!("Error: {}", e),
                );
            }
        }

        self.state.status = "Ready".to_string();
        self.state.tokens_used += input.len() as u64; // Rough estimate

        Ok(())
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
