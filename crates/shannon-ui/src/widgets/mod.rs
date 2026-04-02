//! Ratatui widgets for Shannon UI

use ratatui::{
    layout::{Alignment, Direction, Rect, Constraint},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap, ListState, Scrollbar, ScrollbarOrientation},
    Frame,
};
use std::collections::VecDeque;

/// Welcome widget for initial screen
pub struct WelcomeWidget;

impl WelcomeWidget {
    /// Render the welcome message
    pub fn render(frame: &mut Frame, area: Rect) {
        let title = vec![
            Line::from(vec![
                Span::styled("Shannon", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::from(" - "),
                Span::styled("Terminal AI Agent Interface", Style::default().fg(Color::White)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Press ", Style::default().fg(Color::Gray)),
                Span::styled("q", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::from(" to quit"),
            ]),
            Line::from(vec![
                Span::styled("Press ", Style::default().fg(Color::Gray)),
                Span::styled("Ctrl+C", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::from(" to exit"),
            ]),
        ];

        let paragraph = Paragraph::new(title)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan))
                    .title(" Welcome ")
            )
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });

        frame.render_widget(paragraph, area);
    }
}

/// Status bar widget
pub struct StatusBarWidget;

impl StatusBarWidget {
    /// Render the status bar
    pub fn render(frame: &mut Frame, area: Rect, message: &str) {
        let line = vec![
            Span::styled(" Status: ", Style::default().fg(Color::Gray)),
            Span::styled(message, Style::default().fg(Color::White)),
        ];

        let paragraph = Paragraph::new(Line::from(line))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::DarkGray))
            )
            .alignment(Alignment::Left);

        frame.render_widget(paragraph, area);
    }

    /// Render enhanced status bar with more information
    pub fn render_enhanced(
        frame: &mut Frame,
        area: Rect,
        status: &str,
        model: Option<&str>,
        tokens_used: Option<u64>,
    ) {
        // Build span with owned strings for proper lifetime
        let mut span_vec: Vec<Span<'static>> = vec![
            Span::styled(" Status: ", Style::default().fg(Color::Gray)),
            Span::styled(status.to_string(), Style::default().fg(Color::White)),
        ];

        if let Some(m) = model {
            span_vec.push(Span::styled(" | Model: ", Style::default().fg(Color::Gray)));
            span_vec.push(Span::styled(m.to_string(), Style::default().fg(Color::Cyan)));
        }

        if let Some(t) = tokens_used {
            span_vec.push(Span::styled(" | Tokens: ", Style::default().fg(Color::Gray)));
            span_vec.push(Span::styled(t.to_string(), Style::default().fg(Color::Yellow)));
        }

        let paragraph = Paragraph::new(Line::from(span_vec))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::DarkGray))
            )
            .alignment(Alignment::Left);

        frame.render_widget(paragraph, area);
    }
}

/// Chat message widget
pub struct ChatWidget {
    messages: VecDeque<ChatMessage>,
    scroll_offset: usize,
}

/// A single chat message
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Role of the chat message sender
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatRole {
    User,
    Assistant,
    System,
    Tool,
}

impl ChatWidget {
    /// Create a new chat widget
    pub fn new(capacity: usize) -> Self {
        Self {
            messages: VecDeque::with_capacity(capacity),
            scroll_offset: 0,
        }
    }

    /// Add a message to the chat
    pub fn add_message(&mut self, role: ChatRole, content: String) {
        let message = ChatMessage {
            role,
            content,
            timestamp: chrono::Utc::now(),
        };

        self.messages.push_back(message);

        // Auto-scroll to bottom
        if self.messages.len() > 0 {
            self.scroll_offset = self.messages.len() - 1;
        }
    }

    /// Clear all messages
    pub fn clear(&mut self) {
        self.messages.clear();
        self.scroll_offset = 0;
    }

    /// Scroll up
    pub fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    /// Scroll down
    pub fn scroll_down(&mut self) {
        if !self.messages.is_empty() {
            self.scroll_offset = (self.scroll_offset + 1).min(self.messages.len() - 1);
        }
    }

    /// Render the chat widget
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let mut list_items = Vec::new();

        for (i, msg) in self.messages.iter().enumerate() {
            let (role_name, role_color) = match msg.role {
                ChatRole::User => ("User", Color::Green),
                ChatRole::Assistant => ("Assistant", Color::Cyan),
                ChatRole::System => ("System", Color::Yellow),
                ChatRole::Tool => ("Tool", Color::Magenta),
            };

            let timestamp = msg.timestamp.format("%H:%M:%S").to_string();

            // Format message
            let formatted_content = if msg.content.len() > 80 {
                format!("{}...", &msg.content[..77])
            } else {
                msg.content.clone()
            };

            let item = ListItem::new(Line::from(vec![
                Span::styled("[", Style::default().fg(Color::DarkGray)),
                Span::styled(timestamp.clone(), Style::default().fg(Color::DarkGray)),
                Span::styled("] ", Style::default().fg(Color::DarkGray)),
                Span::styled(role_name, Style::default().fg(role_color).add_modifier(Modifier::BOLD)),
                Span::styled(": ", Style::default().fg(Color::Gray)),
                Span::styled(formatted_content, Style::default().fg(Color::White)),
            ]));

            list_items.push(item);
        }

        let list = List::new(list_items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan))
                    .title(" Chat ")
            );

        frame.render_widget(list, area);
    }

    /// Get the number of messages
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Check if the chat is empty
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

/// Input prompt widget
pub struct PromptWidget {
    input: String,
    cursor_position: usize,
    placeholder: String,
}

impl PromptWidget {
    /// Create a new prompt widget
    pub fn new() -> Self {
        Self {
            input: String::new(),
            cursor_position: 0,
            placeholder: "Type your message...".to_string(),
        }
    }

    /// Set the placeholder text
    pub fn with_placeholder(mut self, placeholder: String) -> Self {
        self.placeholder = placeholder;
        self
    }

    /// Get the current input
    pub fn input(&self) -> &str {
        &self.input
    }

    /// Add a character to the input
    pub fn add_char(&mut self, c: char) {
        self.input.insert(self.cursor_position, c);
        self.cursor_position += 1;
    }

    /// Remove the character before the cursor
    pub fn backspace(&mut self) {
        if self.cursor_position > 0 {
            self.input.remove(self.cursor_position - 1);
            self.cursor_position -= 1;
        }
    }

    /// Move the cursor left
    pub fn cursor_left(&mut self) {
        self.cursor_position = self.cursor_position.saturating_sub(1);
    }

    /// Move the cursor right
    pub fn cursor_right(&mut self) {
        self.cursor_position = (self.cursor_position + 1).min(self.input.len());
    }

    /// Clear the input
    pub fn clear(&mut self) {
        self.input.clear();
        self.cursor_position = 0;
    }

    /// Set the input text
    pub fn set_input(&mut self, input: String) {
        self.cursor_position = input.len();
        self.input = input;
    }

    /// Render the prompt widget
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let display_text = if self.input.is_empty() {
            self.placeholder.clone()
        } else {
            self.input.clone()
        };

        let text = Text::from(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(&display_text, Style::default().fg(Color::White)),
        ]));

        let paragraph = Paragraph::new(text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan))
                    .title(" Input ")
            )
            .alignment(Alignment::Left);

        frame.render_widget(paragraph, area);
    }
}

impl Default for PromptWidget {
    fn default() -> Self {
        Self::new()
    }
}

/// Main UI layout widget
pub struct MainLayoutWidget;

impl MainLayoutWidget {
    /// Create the main layout chunks
    pub fn layout(area: Rect) -> (Rect, Rect, Rect, Rect) {
        let chunks = ratatui::layout::Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Min(0),   // Chat area (flexible)
                Constraint::Length(3),  // Input prompt
                Constraint::Length(3),  // Status bar
            ])
            .split(area);

        let chat_area = chunks[0];
        let prompt_area = chunks[1];
        let status_area = chunks[2];

        (chat_area, prompt_area, status_area, area)
    }

    /// Render the complete UI
    pub fn render_complete(
        frame: &mut Frame,
        chat: &ChatWidget,
        prompt: &PromptWidget,
        status: &str,
        model: Option<&str>,
        tokens_used: Option<u64>,
    ) {
        let area = frame.area();

        let (chat_area, prompt_area, status_area, _) = Self::layout(area);

        // Render each widget
        chat.render(frame, chat_area);
        prompt.render(frame, prompt_area);
        StatusBarWidget::render_enhanced(frame, status_area, status, model, tokens_used);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_widget_creation() {
        let chat = ChatWidget::new(100);
        assert_eq!(chat.len(), 0);
        assert!(chat.is_empty());
    }

    #[test]
    fn test_chat_widget_add_message() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::User, "Hello".to_string());
        assert_eq!(chat.len(), 1);
        assert!(!chat.is_empty());
    }

    #[test]
    fn test_prompt_widget_creation() {
        let prompt = PromptWidget::new();
        assert_eq!(prompt.input(), "");
        assert_eq!(prompt.cursor_position, 0);
    }

    #[test]
    fn test_prompt_widget_add_char() {
        let mut prompt = PromptWidget::new();
        prompt.add_char('a');
        assert_eq!(prompt.input(), "a");
        assert_eq!(prompt.cursor_position, 1);
    }

    #[test]
    fn test_prompt_widget_backspace() {
        let mut prompt = PromptWidget::new();
        prompt.add_char('a');
        prompt.add_char('b');
        prompt.backspace();
        assert_eq!(prompt.input(), "a");
        assert_eq!(prompt.cursor_position, 1);
    }

    #[test]
    fn test_prompt_widget_clear() {
        let mut prompt = PromptWidget::new();
        prompt.add_char('a');
        prompt.add_char('b');
        prompt.clear();
        assert_eq!(prompt.input(), "");
        assert_eq!(prompt.cursor_position, 0);
    }
}
