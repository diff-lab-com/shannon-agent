//! Ratatui widgets for Shannon UI

pub mod select;
pub mod progress;
pub mod dialog;

use ratatui::{
    layout::{Alignment, Direction, Rect, Constraint},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};
use std::collections::VecDeque;

/// Header bar widget showing session information
pub struct HeaderWidget;

impl HeaderWidget {
    /// Get welcome message
    fn welcome_message() -> Vec<Span<'static>> {
        vec![
            Span::styled("Welcome to ", Style::default().fg(Color::Green)),
            Span::styled("Shannon", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled("! ", Style::default().fg(Color::Green)),
        ]
    }

    /// Get tip message
    fn tip_message() -> Vec<Span<'static>> {
        vec![
            Span::styled("Tip: ", Style::default().fg(Color::Yellow)),
            Span::styled("Type /help for commands", Style::default().fg(Color::White)),
        ]
    }

    /// Render the header bar with all session information
    pub fn render(
        frame: &mut Frame,
        area: Rect,
        model: Option<&str>,
        tokens_used: Option<u64>,
        working_dir: &str,
    ) {
        // Split header area into left and right sections
        let chunks = ratatui::layout::Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        let left_area = chunks[0];
        let right_area = chunks[1];

        // Left side: Welcome + Tips (using helper methods)
        let mut left_spans: Vec<Span<'static>> = Self::welcome_message();
        left_spans.push(Span::styled(" | ", Style::default().fg(Color::DarkGray)));
        left_spans.extend(Self::tip_message());

        let left_paragraph = Paragraph::new(Line::from(left_spans))
            .block(
                Block::default()
                    .borders(Borders::ALL & !Borders::RIGHT & !Borders::BOTTOM)
                    .border_style(Style::default().fg(Color::Cyan))
            )
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true });

        frame.render_widget(left_paragraph, left_area);

        // Right side: Model | Tokens | Working Directory
        let mut right_spans: Vec<Span<'static>> = Vec::new();

        if let Some(m) = model {
            right_spans.push(Span::styled("Model: ", Style::default().fg(Color::Gray)));
            right_spans.push(Span::styled(m.to_string(), Style::default().fg(Color::Cyan)));
            right_spans.push(Span::styled(" | ", Style::default().fg(Color::DarkGray)));
        }

        right_spans.push(Span::styled("Tokens: ", Style::default().fg(Color::Gray)));
        let tokens = tokens_used.unwrap_or(0);
        right_spans.push(Span::styled(tokens.to_string(), Style::default().fg(Color::Yellow)));
        right_spans.push(Span::styled(" | ", Style::default().fg(Color::DarkGray)));

        // Truncate working directory if too long
        let display_dir = if working_dir.len() > 20 {
            format!("...{}", &working_dir[working_dir.len() - 20..])
        } else {
            working_dir.to_string()
        };
        right_spans.push(Span::styled("Dir: ", Style::default().fg(Color::Gray)));
        right_spans.push(Span::styled(display_dir, Style::default().fg(Color::White)));

        let right_paragraph = Paragraph::new(Line::from(right_spans))
            .block(
                Block::default()
                    .borders(Borders::ALL & !Borders::LEFT & !Borders::BOTTOM)
                    .border_style(Style::default().fg(Color::Cyan))
            )
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true });

        frame.render_widget(right_paragraph, right_area);
    }

    /// Get the recommended height for the header bar
    pub fn height() -> usize {
        1 // Single line header
    }
}

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

    /// Render enhanced status bar with spinner animation and optional progress bar
    pub fn render_with_spinner(
        frame: &mut Frame,
        area: Rect,
        status: &str,
        model: Option<&str>,
        tokens_used: Option<u64>,
        spinner: Option<&crate::widgets::progress::SpinnerWidget>,
        progress_bar: Option<&crate::widgets::progress::ProgressBarWidget>,
    ) {
        // Build span with owned strings for proper lifetime
        let mut span_vec: Vec<Span<'static>> = Vec::new();

        // Show spinner frame when processing
        if let Some(sp) = spinner {
            if status != "Ready" {
                let frame_str = sp.current_char().to_string();
                span_vec.push(Span::styled(frame_str, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));
                span_vec.push(Span::raw(" "));
            }
        }

        span_vec.push(Span::styled("Status: ", Style::default().fg(Color::Gray)));
        span_vec.push(Span::styled(status.to_string(), Style::default().fg(Color::White)));

        if let Some(m) = model {
            span_vec.push(Span::styled(" | Model: ", Style::default().fg(Color::Gray)));
            span_vec.push(Span::styled(m.to_string(), Style::default().fg(Color::Cyan)));
        }

        if let Some(t) = tokens_used {
            span_vec.push(Span::styled(" | Tokens: ", Style::default().fg(Color::Gray)));
            span_vec.push(Span::styled(t.to_string(), Style::default().fg(Color::Yellow)));
        }

        // If a progress bar is provided with active progress, show inline progress
        if let Some(pb) = progress_bar {
            let pct = pb.percentage();
            if pct > 0.0 {
                span_vec.push(Span::styled("  ", Style::default()));
                // Inline progress bar: [████████░░░░] 45.2%
                let bar_width = 12usize;
                let filled = (pb.progress() * bar_width as f64) as usize;
                let mut bar_str = String::from("[");
                for i in 0..bar_width {
                    if i < filled {
                        bar_str.push('█');
                    } else {
                        bar_str.push('░');
                    }
                }
                bar_str.push(']');
                span_vec.push(Span::styled(bar_str, Style::default().fg(Color::Cyan)));
                span_vec.push(Span::styled(
                    format!(" {pct:.0}%"),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ));
            }
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

    /// Add a message to the chat, returns the message index
    pub fn add_message(&mut self, role: ChatRole, content: String) -> usize {
        let message = ChatMessage {
            role,
            content,
            timestamp: chrono::Utc::now(),
        };

        let index = self.messages.len();
        self.messages.push_back(message);

        // Auto-scroll to bottom
        if !self.messages.is_empty() {
            self.scroll_offset = self.messages.len() - 1;
        }

        index
    }

    /// Update an existing message by index (for streaming updates)
    pub fn update_message(&mut self, index: usize, content: String) {
        if let Some(msg) = self.messages.get_mut(index) {
            msg.content = content;
            // Update timestamp to reflect the update time
            msg.timestamp = chrono::Utc::now();
        }
    }

    /// Update the last message (convenience method for streaming)
    pub fn update_last_message(&mut self, content: String) {
        if !self.messages.is_empty() {
            let last_index = self.messages.len() - 1;
            self.update_message(last_index, content);
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

        for msg in self.messages.iter() {
            let (role_name, role_color) = match msg.role {
                ChatRole::User => ("User", Color::Green),
                ChatRole::Assistant => ("Assistant", Color::Cyan),
                ChatRole::System => ("System", Color::Yellow),
                ChatRole::Tool => ("Tool", Color::Magenta),
            };

            let timestamp = msg.timestamp.format("%H:%M:%S").to_string();

            // Format message (use char-aware truncation for UTF-8 safety)
            let formatted_content = if msg.content.chars().count() > 80 {
                let truncated: String = msg.content.chars().take(77).collect();
                format!("{truncated}...")
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

    /// Get a message by index
    pub fn get_message(&self, index: usize) -> Option<&ChatMessage> {
        self.messages.get(index)
    }

    /// Get the last message
    pub fn last_message(&self) -> Option<&ChatMessage> {
        self.messages.back()
    }

    /// Get the number of messages
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Iterate all messages with their indices
    pub fn iter_messages(&self) -> impl Iterator<Item = (usize, &ChatMessage)> {
        self.messages.iter().enumerate()
    }

    /// Rewind the conversation by removing the last `n` turns.
    ///
    /// A "turn" starts with a User message and includes all subsequent
    /// non-User messages (Assistant, Tool, System) until the next User
    /// message. Returns the number of messages actually removed.
    pub fn rewind(&mut self, turns: usize) -> usize {
        if turns == 0 || self.messages.is_empty() {
            return 0;
        }

        // Walk backwards to find where each turn starts
        let mut turns_found = 0;
        let mut cutoff = self.messages.len(); // exclusive upper bound

        for i in (0..self.messages.len()).rev() {
            if self.messages[i].role == ChatRole::User {
                turns_found += 1;
                cutoff = i;
                if turns_found >= turns {
                    break;
                }
            }
        }

        if turns_found == 0 {
            return 0;
        }

        let removed = self.messages.len() - cutoff;
        self.messages.truncate(cutoff);

        // Fix scroll offset
        if !self.messages.is_empty() {
            self.scroll_offset = self.messages.len() - 1;
        } else {
            self.scroll_offset = 0;
        }

        removed
    }
}

/// Input prompt widget (multi-line enabled)
pub struct PromptWidget {
    /// Inner input buffer with full multi-line support
    buffer: crate::repl_enhancement::InputBuffer,
    placeholder: String,
}

impl PromptWidget {
    /// Create a new prompt widget
    pub fn new() -> Self {
        Self {
            buffer: crate::repl_enhancement::InputBuffer::new(),
            placeholder: "Type your message...".to_string(),
        }
    }

    /// Set the placeholder text
    pub fn with_placeholder(mut self, placeholder: String) -> Self {
        self.placeholder = placeholder;
        self
    }

    /// Get the current input text
    pub fn input(&self) -> String {
        self.buffer.text()
    }

    /// Add a character to the input
    pub fn add_char(&mut self, c: char) {
        self.buffer.insert_char(c);
    }

    /// Remove the character before the cursor
    pub fn backspace(&mut self) {
        self.buffer.backspace();
    }

    /// Clear the input
    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    /// Set the input text
    pub fn set_input(&mut self, input: String) {
        self.buffer.set_text(&input);
    }

    /// Insert a newline (for multi-line editing)
    pub fn insert_newline(&mut self) {
        self.buffer.newline();
    }

    /// Move cursor left
    pub fn cursor_left(&mut self) {
        self.buffer.move_left();
    }

    /// Move cursor right
    pub fn cursor_right(&mut self) {
        self.buffer.move_right();
    }

    /// Move cursor up
    pub fn cursor_up(&mut self) {
        self.buffer.move_up();
    }

    /// Move cursor down
    pub fn cursor_down(&mut self) {
        self.buffer.move_down();
    }

    /// Get the text of the current line at the cursor
    pub fn current_line(&self) -> String {
        self.buffer.current_line().to_string()
    }

    /// Get the word at or near the cursor
    pub fn current_word(&self) -> String {
        self.buffer.current_word()
    }

    /// Insert text at the cursor position
    pub fn insert_text(&mut self, text: &str) {
        self.buffer.insert_text(text);
    }

    /// Get current cursor position (column)
    pub fn cursor_position(&self) -> usize {
        self.buffer.cursor_col()
    }

    /// Render the prompt widget
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let input_text = self.input();
        let display_text = if input_text.is_empty() {
            self.placeholder.clone()
        } else {
            input_text
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
    /// Returns (header_area, chat_area, prompt_area, status_area, full_area)
    pub fn layout(area: Rect) -> (Rect, Rect, Rect, Rect, Rect) {
        let chunks = ratatui::layout::Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(HeaderWidget::height() as u16), // Header bar
                Constraint::Min(0),      // Chat area (flexible)
                Constraint::Length(3),   // Input prompt
                Constraint::Length(3),   // Status bar
            ])
            .split(area);

        let header_area = chunks[0];
        let chat_area = chunks[1];
        let prompt_area = chunks[2];
        let status_area = chunks[3];

        (header_area, chat_area, prompt_area, status_area, area)
    }

    /// Render the complete UI
    pub fn render_complete(
        frame: &mut Frame,
        chat: &ChatWidget,
        prompt: &PromptWidget,
        status: &str,
        model: Option<&str>,
        tokens_used: Option<u64>,
        working_dir: &str,
    ) {
        Self::render_complete_with_spinner(frame, chat, prompt, status, model, tokens_used, working_dir, None, None);
    }

    /// Render the complete UI with spinner animation support
    #[allow(clippy::too_many_arguments)]
    pub fn render_complete_with_spinner(
        frame: &mut Frame,
        chat: &ChatWidget,
        prompt: &PromptWidget,
        status: &str,
        model: Option<&str>,
        tokens_used: Option<u64>,
        working_dir: &str,
        spinner: Option<&crate::widgets::progress::SpinnerWidget>,
        progress_bar: Option<&crate::widgets::progress::ProgressBarWidget>,
    ) {
        let area = frame.area();

        let (header_area, chat_area, prompt_area, status_area, _) = Self::layout(area);

        // Render each widget
        HeaderWidget::render(frame, header_area, model, tokens_used, working_dir);
        chat.render(frame, chat_area);
        prompt.render(frame, prompt_area);
        StatusBarWidget::render_with_spinner(frame, status_area, status, model, tokens_used, spinner, progress_bar);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Chat Widget Tests ─────────────────────────────────────────────

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
    fn test_chat_widget_multiple_messages() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::User, "First".to_string());
        chat.add_message(ChatRole::Assistant, "Second".to_string());
        chat.add_message(ChatRole::System, "Third".to_string());
        assert_eq!(chat.len(), 3);
    }

    #[test]
    fn test_chat_widget_clear() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::User, "Hello".to_string());
        chat.add_message(ChatRole::Assistant, "Hi".to_string());
        assert_eq!(chat.len(), 2);
        chat.clear();
        assert_eq!(chat.len(), 0);
        assert!(chat.is_empty());
    }

    #[test]
    fn test_chat_widget_update_message() {
        let mut chat = ChatWidget::new(100);
        let index = chat.add_message(ChatRole::Assistant, "Initial".to_string());
        chat.update_message(index, "Updated".to_string());
        assert_eq!(chat.messages[index].content, "Updated");
    }

    #[test]
    fn test_chat_widget_update_last_message() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::Assistant, "First".to_string());
        chat.add_message(ChatRole::Assistant, "Second".to_string());
        chat.update_last_message("Last Updated".to_string());
        assert_eq!(chat.messages.len(), 2);
        assert_eq!(chat.messages[1].content, "Last Updated");
    }

    #[test]
    fn test_chat_widget_scroll() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::User, "Msg1".to_string());
        chat.add_message(ChatRole::User, "Msg2".to_string());
        chat.add_message(ChatRole::User, "Msg3".to_string());
        assert_eq!(chat.scroll_offset, 2); // Auto-scrolls to bottom
        chat.scroll_up();
        assert_eq!(chat.scroll_offset, 1);
        chat.scroll_down();
        assert_eq!(chat.scroll_offset, 2);
    }

    // ── Prompt Widget Tests ────────────────────────────────────────────

    #[test]
    fn test_prompt_widget_creation() {
        let prompt = PromptWidget::new();
        assert_eq!(prompt.input(), "");
        assert_eq!(prompt.cursor_position(), 0);
    }

    #[test]
    fn test_prompt_widget_add_char() {
        let mut prompt = PromptWidget::new();
        prompt.add_char('a');
        assert_eq!(prompt.input(), "a");
        assert_eq!(prompt.cursor_position(), 1);
    }

    #[test]
    fn test_prompt_widget_backspace() {
        let mut prompt = PromptWidget::new();
        prompt.add_char('a');
        prompt.add_char('b');
        prompt.backspace();
        assert_eq!(prompt.input(), "a");
        assert_eq!(prompt.cursor_position(), 1);
    }

    #[test]
    fn test_prompt_widget_clear() {
        let mut prompt = PromptWidget::new();
        prompt.add_char('a');
        prompt.add_char('b');
        prompt.clear();
        assert_eq!(prompt.input(), "");
        assert_eq!(prompt.cursor_position(), 0);
    }

    #[test]
    fn test_prompt_widget_cursor_movement() {
        let mut prompt = PromptWidget::new();
        prompt.set_input("abc".to_string());
        assert_eq!(prompt.cursor_position(), 3);
        prompt.cursor_left();
        assert_eq!(prompt.cursor_position(), 2);
        prompt.cursor_left();
        assert_eq!(prompt.cursor_position(), 1);
        prompt.cursor_right();
        assert_eq!(prompt.cursor_position(), 2);
        prompt.cursor_right();
        assert_eq!(prompt.cursor_position(), 3);
    }

    #[test]
    fn test_prompt_widget_set_input() {
        let mut prompt = PromptWidget::new();
        prompt.set_input("test input".to_string());
        assert_eq!(prompt.input(), "test input");
        assert_eq!(prompt.cursor_position(), 10);
    }

    #[test]
    fn test_prompt_widget_with_placeholder() {
        let prompt = PromptWidget::new().with_placeholder("Enter command...".to_string());
        assert_eq!(prompt.placeholder, "Enter command...");
    }

    // ── Header Widget Tests ────────────────────────────────────────────

    #[test]
    fn test_header_widget_height() {
        assert_eq!(HeaderWidget::height(), 1);
    }

    #[test]
    fn test_header_widget_welcome_message() {
        let spans = HeaderWidget::welcome_message();
        assert!(!spans.is_empty());
        assert_eq!(spans.len(), 3); // "Welcome to " + "Shannon" + "! "
    }

    #[test]
    fn test_header_widget_tip_message() {
        let spans = HeaderWidget::tip_message();
        assert!(!spans.is_empty());
        assert_eq!(spans.len(), 2); // "Tip: " + tip text
    }

    // ── Main Layout Widget Tests ───────────────────────────────────────

    #[test]
    fn test_main_layout_widget_returns_five_chunks() {
        // Create a test area (100x20)
        let area = Rect::new(0, 0, 100, 20);
        let (header, chat, prompt, status, full) = MainLayoutWidget::layout(area);

        // Header should be at top with height 1
        assert_eq!(header.y, 1); // margin(1) + 0
        assert_eq!(header.height, 1);

        // Chat should be below header and be flexible
        assert_eq!(chat.y, 2); // margin + header
        assert!(chat.height > 0); // Flexible size

        // Prompt should be below chat with height 3
        assert_eq!(prompt.height, 3);

        // Status should be at bottom with height 3
        assert_eq!(status.height, 3);

        // Full area should match input area
        assert_eq!(full, area);
    }

    #[test]
    fn test_main_layout_widget_chat_area_is_flexible() {
        let small_area = Rect::new(0, 0, 80, 10);
        let (_, small_chat, _, _, _) = MainLayoutWidget::layout(small_area);

        let large_area = Rect::new(0, 0, 80, 30);
        let (_, large_chat, _, _, _) = MainLayoutWidget::layout(large_area);

        // Chat area should grow with available space
        assert!(large_chat.height > small_chat.height);
    }

    #[test]
    fn test_main_layout_widget_fixed_sizes() {
        let area = Rect::new(0, 0, 100, 20);
        let (header, _, prompt, status, _) = MainLayoutWidget::layout(area);

        // Header, prompt, and status should have fixed heights
        assert_eq!(header.height, 1);
        assert_eq!(prompt.height, 3);
        assert_eq!(status.height, 3);
    }

    #[test]
    fn test_main_layout_widget_margins() {
        let area = Rect::new(0, 0, 100, 20);
        let (header, _, _, _, _) = MainLayoutWidget::layout(area);

        // Check that margin(1) is applied
        assert_eq!(header.x, 1);
        assert_eq!(header.y, 1);
        assert!(header.width < 100); // Reduced by margin
    }

    // ── Chat Message Tests ─────────────────────────────────────────────

    #[test]
    fn test_chat_message_creation() {
        let msg = ChatMessage {
            role: ChatRole::User,
            content: "Test message".to_string(),
            timestamp: chrono::Utc::now(),
        };
        assert_eq!(msg.content, "Test message");
        assert_eq!(msg.role, ChatRole::User);
    }

    #[test]
    fn test_chat_role_colors() {
        let (user_name, user_color) = match ChatRole::User {
            ChatRole::User => ("User", Color::Green),
            _ => panic!("Wrong role"),
        };
        assert_eq!(user_name, "User");
        assert_eq!(user_color, Color::Green);
    }

    #[test]
    fn test_all_chat_roles_have_colors() {
        let roles = vec![
            (ChatRole::User, "User", Color::Green),
            (ChatRole::Assistant, "Assistant", Color::Cyan),
            (ChatRole::System, "System", Color::Yellow),
            (ChatRole::Tool, "Tool", Color::Magenta),
        ];

        for (role, expected_name, expected_color) in roles {
            let (name, color) = match role {
                ChatRole::User => ("User", Color::Green),
                ChatRole::Assistant => ("Assistant", Color::Cyan),
                ChatRole::System => ("System", Color::Yellow),
                ChatRole::Tool => ("Tool", Color::Magenta),
            };
            assert_eq!(name, expected_name);
            assert_eq!(color, expected_color);
        }
    }

    // ── Integration Tests ──────────────────────────────────────────────

    #[test]
    fn test_chat_prompt_workflow() {
        let mut chat = ChatWidget::new(10);
        let mut prompt = PromptWidget::new();

        // User types message
        prompt.add_char('H');
        prompt.add_char('e');
        prompt.add_char('l');
        prompt.add_char('l');
        prompt.add_char('o');
        assert_eq!(prompt.input(), "Hello");

        // Add to chat
        chat.add_message(ChatRole::User, prompt.input().to_string());
        assert_eq!(chat.len(), 1);

        // Clear prompt
        prompt.clear();
        assert_eq!(prompt.input(), "");
    }

    #[test]
    fn test_multiple_chat_updates() {
        let mut chat = ChatWidget::new(10);
        let idx = chat.add_message(ChatRole::Assistant, "Thinking...".to_string());

        // Simulate streaming updates
        for i in 1..=5 {
            chat.update_message(idx, format!("Step {i} complete"));
        }

        assert_eq!(chat.messages[idx].content, "Step 5 complete");
    }

    // ── Rewind Tests ─────────────────────────────────────────────────

    #[test]
    fn test_rewind_single_turn() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::User, "Hello".to_string());
        chat.add_message(ChatRole::Assistant, "Hi there".to_string());
        chat.add_message(ChatRole::User, "How are you?".to_string());
        chat.add_message(ChatRole::Assistant, "I'm fine".to_string());
        assert_eq!(chat.len(), 4);

        let removed = chat.rewind(1);
        assert_eq!(removed, 2); // last user + assistant
        assert_eq!(chat.len(), 2);
        assert_eq!(chat.get_message(0).unwrap().content, "Hello");
        assert_eq!(chat.get_message(1).unwrap().content, "Hi there");
    }

    #[test]
    fn test_rewind_multiple_turns() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::User, "Q1".to_string());
        chat.add_message(ChatRole::Assistant, "A1".to_string());
        chat.add_message(ChatRole::User, "Q2".to_string());
        chat.add_message(ChatRole::Assistant, "A2".to_string());
        chat.add_message(ChatRole::User, "Q3".to_string());
        chat.add_message(ChatRole::Assistant, "A3".to_string());
        assert_eq!(chat.len(), 6);

        let removed = chat.rewind(2);
        assert_eq!(removed, 4); // Q2+A2+Q3+A3
        assert_eq!(chat.len(), 2);
        assert_eq!(chat.get_message(0).unwrap().content, "Q1");
    }

    #[test]
    fn test_rewind_all_turns() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::User, "Q1".to_string());
        chat.add_message(ChatRole::Assistant, "A1".to_string());
        assert_eq!(chat.len(), 2);

        let removed = chat.rewind(1);
        assert_eq!(removed, 2);
        assert!(chat.is_empty());
    }

    #[test]
    fn test_rewind_with_tool_messages() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::User, "Run tests".to_string());
        chat.add_message(ChatRole::Tool, "bash: cargo test".to_string());
        chat.add_message(ChatRole::Tool, "output: all passed".to_string());
        chat.add_message(ChatRole::Assistant, "Tests passed".to_string());
        chat.add_message(ChatRole::User, "Now commit".to_string());
        chat.add_message(ChatRole::Assistant, "Done".to_string());
        assert_eq!(chat.len(), 6);

        // Rewind 1 turn removes "Now commit" + "Done"
        let removed = chat.rewind(1);
        assert_eq!(removed, 2);
        assert_eq!(chat.len(), 4);

        // Rewind 1 more turn removes "Run tests" + all tool + assistant
        let removed = chat.rewind(1);
        assert_eq!(removed, 4);
        assert!(chat.is_empty());
    }

    #[test]
    fn test_rewind_with_system_messages() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::System, "Session started".to_string());
        chat.add_message(ChatRole::User, "Q1".to_string());
        chat.add_message(ChatRole::Assistant, "A1".to_string());
        assert_eq!(chat.len(), 3);

        // Rewind 1 turn: system message stays, user+assistant removed
        let removed = chat.rewind(1);
        assert_eq!(removed, 2); // User + Assistant only
        assert_eq!(chat.len(), 1);
        assert_eq!(chat.get_message(0).unwrap().role, ChatRole::System);
    }

    #[test]
    fn test_rewind_empty_chat() {
        let mut chat = ChatWidget::new(100);
        let removed = chat.rewind(1);
        assert_eq!(removed, 0);
        assert!(chat.is_empty());
    }

    #[test]
    fn test_rewind_zero_turns() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::User, "Q1".to_string());
        let removed = chat.rewind(0);
        assert_eq!(removed, 0);
        assert_eq!(chat.len(), 1);
    }

    #[test]
    fn test_rewind_more_than_available() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::User, "Q1".to_string());
        chat.add_message(ChatRole::Assistant, "A1".to_string());

        // Ask for 5 turns when only 1 exists
        let removed = chat.rewind(5);
        assert_eq!(removed, 2);
        assert!(chat.is_empty());
    }

    #[test]
    fn test_rewind_no_user_messages() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::System, "System msg".to_string());
        chat.add_message(ChatRole::Assistant, "Assistant msg".to_string());

        let removed = chat.rewind(1);
        assert_eq!(removed, 0); // No user messages to anchor a turn
        assert_eq!(chat.len(), 2);
    }

    #[test]
    fn test_rewind_fixes_scroll_offset() {
        let mut chat = ChatWidget::new(100);
        chat.add_message(ChatRole::User, "Q1".to_string());
        chat.add_message(ChatRole::Assistant, "A1".to_string());
        chat.add_message(ChatRole::User, "Q2".to_string());
        chat.add_message(ChatRole::Assistant, "A2".to_string());
        // scroll_offset should be 3 (last message index)

        chat.rewind(1);
        // scroll_offset should be updated to 1 (new last message)
        assert_eq!(chat.scroll_offset, 1);
    }
}
