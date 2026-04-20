//! Ratatui widgets for Shannon UI

use crate::tool_format::strip_ansi;
use crate::theme::Theme;

pub mod select;
pub mod progress;
pub mod dialog;
pub mod diff_viewer;

use ratatui::{
    layout::{Alignment, Direction, Rect, Constraint},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};
use std::collections::VecDeque;

/// Header bar widget showing session information
pub struct HeaderWidget;

impl HeaderWidget {
    /// Get welcome message
    fn welcome_message(theme: &Theme) -> Vec<Span<'static>> {
        vec![
            Span::styled("Welcome to ", Style::default().fg(theme.success)),
            Span::styled("Shannon", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
            Span::styled("! ", Style::default().fg(theme.success)),
        ]
    }

    /// Get tip message
    fn tip_message(theme: &Theme) -> Vec<Span<'static>> {
        vec![
            Span::styled("Tip: ", Style::default().fg(theme.secondary)),
            Span::styled("/help", Style::default().fg(theme.text)),
        ]
    }

    /// Render the header bar with all session information
    pub fn render(
        frame: &mut Frame,
        area: Rect,
        model: Option<&str>,
        tokens_used: Option<u64>,
        working_dir: &str,
        theme: &Theme,
    ) {
        // Split header area into left and right sections
        let chunks = ratatui::layout::Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        let left_area = chunks[0];
        let right_area = chunks[1];

        // Left side: Welcome + Tips (using helper methods)
        let mut left_spans: Vec<Span<'static>> = Self::welcome_message(theme);
        left_spans.push(Span::styled(" | ", Style::default().fg(theme.muted)));
        left_spans.extend(Self::tip_message(theme));

        let left_paragraph = Paragraph::new(Line::from(left_spans))
            .block(
                Block::default()
                    .borders(Borders::TOP | Borders::BOTTOM)
                    .border_style(Style::default().fg(theme.border))
            )
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true });

        frame.render_widget(left_paragraph, left_area);

        // Right side: Model | Tokens | Working Directory
        let mut right_spans: Vec<Span<'static>> = Vec::new();

        if let Some(m) = model {
            right_spans.push(Span::styled("Model: ", Style::default().fg(theme.text_dim)));
            right_spans.push(Span::styled(m.to_string(), Style::default().fg(theme.primary)));
            right_spans.push(Span::styled(" | ", Style::default().fg(theme.muted)));
        }

        right_spans.push(Span::styled("Tokens: ", Style::default().fg(theme.text_dim)));
        let tokens = tokens_used.unwrap_or(0);
        right_spans.push(Span::styled(tokens.to_string(), Style::default().fg(theme.secondary)));
        right_spans.push(Span::styled(" | ", Style::default().fg(theme.muted)));

        // Truncate working directory if too long
        let display_dir = if working_dir.len() > 20 {
            format!("...{}", &working_dir[working_dir.len() - 20..])
        } else {
            working_dir.to_string()
        };
        right_spans.push(Span::styled("Dir: ", Style::default().fg(theme.text_dim)));
        right_spans.push(Span::styled(display_dir, Style::default().fg(theme.text)));

        let right_paragraph = Paragraph::new(Line::from(right_spans))
            .block(
                Block::default()
                    .borders(Borders::TOP | Borders::BOTTOM)
                    .border_style(Style::default().fg(theme.border))
            )
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true });

        frame.render_widget(right_paragraph, right_area);
    }

    /// Get the recommended height for the header bar
    pub fn height() -> usize {
        3 // Top border + content + bottom padding
    }
}

/// Welcome widget for initial screen
pub struct WelcomeWidget;

impl WelcomeWidget {
    /// Render the welcome message
    pub fn render(frame: &mut Frame, area: Rect, theme: &Theme) {
        let title = vec![
            Line::from(vec![
                Span::styled("Shannon", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
                Span::from(" - "),
                Span::styled("Terminal AI Agent Interface", Style::default().fg(theme.text)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Press ", Style::default().fg(theme.text_dim)),
                Span::styled("q", Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD)),
                Span::from(" to quit"),
            ]),
            Line::from(vec![
                Span::styled("Press ", Style::default().fg(theme.text_dim)),
                Span::styled("Ctrl+C", Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD)),
                Span::from(" to exit"),
            ]),
        ];

        let paragraph = Paragraph::new(title)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border))
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
    pub fn render(frame: &mut Frame, area: Rect, message: &str, theme: &Theme) {
        let line = vec![
            Span::styled(" Status: ", Style::default().fg(theme.text_dim)),
            Span::styled(message, Style::default().fg(theme.text)),
        ];

        let paragraph = Paragraph::new(Line::from(line))
            .style(Style::default().bg(theme.context_bar_bg))
            .alignment(Alignment::Left);

        frame.render_widget(paragraph, area);
    }

    /// Render enhanced status bar with spinner animation and optional progress bar.
    /// Dense single-line format for maximum screen real estate.
    pub fn render_with_spinner(
        frame: &mut Frame,
        area: Rect,
        status: &str,
        model: Option<&str>,
        tokens_used: Option<u64>,
        spinner: Option<&crate::widgets::progress::SpinnerWidget>,
        progress_bar: Option<&crate::widgets::progress::ProgressBarWidget>,
        theme: &Theme,
    ) {
        // Build span with owned strings for proper lifetime
        let mut span_vec: Vec<Span<'static>> = Vec::new();

        // Separator helper
        let sep = || -> Span<'static> {
            Span::styled(" │ ", Style::default().fg(theme.muted))
        };

        // Show spinner frame when processing
        if let Some(sp) = spinner {
            if status != "Ready" {
                let frame_str = sp.current_char().to_string();
                span_vec.push(Span::styled(frame_str, Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)));
                span_vec.push(Span::raw(" "));
            }
        }

        span_vec.push(Span::styled(status.to_string(), Style::default().fg(theme.text)));

        if let Some(m) = model {
            span_vec.push(sep());
            span_vec.push(Span::styled(m.to_string(), Style::default().fg(theme.primary)));
        }

        if let Some(t) = tokens_used {
            span_vec.push(sep());
            span_vec.push(Span::styled(format!("Ctx: {t}"), Style::default().fg(theme.secondary)));
        }

        // If a progress bar is provided with active progress, show inline progress
        if let Some(pb) = progress_bar {
            let pct = pb.percentage();
            if pct > 0.0 {
                span_vec.push(sep());
                // Inline progress bar: [████████░░░░] 45.2%
                let bar_width = 10usize;
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
                span_vec.push(Span::styled(bar_str, Style::default().fg(theme.primary)));
                span_vec.push(Span::styled(
                    format!(" {pct:.0}%"),
                    Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD),
                ));
            }
        }

        let paragraph = Paragraph::new(Line::from(span_vec))
            .style(Style::default().bg(theme.context_bar_bg))
            .alignment(Alignment::Left);

        frame.render_widget(paragraph, area);
    }
}

/// Chat message widget
pub struct ChatWidget {
    messages: VecDeque<ChatMessage>,
    scroll_offset: usize,
    /// Whether tool output messages are shown in collapsed (single-line) form
    pub collapsed_tools: bool,
}

/// A single chat message
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Optional inline image preview rendered as half-block characters
    pub image_lines: Option<Vec<ratatui::text::Line<'static>>>,
    /// Whether this tool message represents an error
    pub is_error: bool,
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
            collapsed_tools: true,
        }
    }

    /// Add a message to the chat, returns the message index
    pub fn add_message(&mut self, role: ChatRole, content: String) -> usize {
        let message = ChatMessage {
            role,
            content,
            timestamp: chrono::Utc::now(),
            image_lines: None,
            is_error: false,
        };

        let index = self.messages.len();
        self.messages.push_back(message);

        // Auto-scroll to bottom
        if !self.messages.is_empty() {
            self.scroll_offset = self.messages.len() - 1;
        }

        index
    }

    /// Add a message with an inline image preview.
    ///
    /// The `image_lines` are pre-rendered half-block character lines
    /// from the `terminal_image` module.
    pub fn add_message_with_image(
        &mut self,
        role: ChatRole,
        content: String,
        image_lines: Vec<ratatui::text::Line<'static>>,
    ) -> usize {
        let message = ChatMessage {
            role,
            content,
            timestamp: chrono::Utc::now(),
            image_lines: Some(image_lines),
            is_error: false,
        };

        let index = self.messages.len();
        self.messages.push_back(message);

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
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let mut list_items = Vec::new();
        let inner_width = area.width.saturating_sub(2) as usize; // subtract borders

        for msg in self.messages.iter() {
            // Collapsed tool messages: single-line summary
            if msg.role == ChatRole::Tool && self.collapsed_tools {
                let timestamp = msg.timestamp.format("%H:%M:%S").to_string();
                let clean_content = strip_ansi(&msg.content);
                let first_line = clean_content.lines().next().unwrap_or("");
                let prefix_len = timestamp.len() + 6; // "[HH:MM:SS] ⏵ "
                let available = inner_width.saturating_sub(prefix_len);
                let summary = if first_line.chars().count() > available {
                    let truncated: String = first_line.chars().take(available.saturating_sub(3)).collect();
                    format!("{truncated}...")
                } else {
                    first_line.to_string()
                };
                let (status_icon, status_color) = if msg.is_error {
                    ("✗", theme.error)
                } else {
                    ("✓", theme.success)
                };
                let item = ListItem::new(Line::from(vec![
                    Span::styled("[", Style::default().fg(theme.muted)),
                    Span::styled(timestamp, Style::default().fg(theme.muted)),
                    Span::styled("] ", Style::default().fg(theme.muted)),
                    Span::styled("⏵ ", Style::default().fg(theme.tool_msg)),
                    Span::styled(summary, Style::default().fg(theme.text_dim)),
                    Span::styled(" ", Style::default().fg(theme.text_dim)),
                    Span::styled(status_icon, Style::default().fg(status_color)),
                ]));
                list_items.push(item);
                continue;
            }

            let (role_name, role_color) = match msg.role {
                ChatRole::User => ("User", theme.user_msg),
                ChatRole::Assistant => ("Assistant", theme.assistant_msg),
                ChatRole::System => ("System", theme.system_msg),
                ChatRole::Tool => ("Tool", theme.tool_msg),
            };

            let timestamp = msg.timestamp.format("%H:%M:%S").to_string();

            // Strip ANSI escape codes — ratatui doesn't interpret them
            let clean_content = strip_ansi(&msg.content);
            let content_lines: Vec<&str> = clean_content.split('\n').collect();

            for (i, content_line) in content_lines.iter().enumerate() {
                if i == 0 {
                    // First line: timestamp + role + content
                    let prefix_len = timestamp.len() + role_name.len() + 5; // "[00:00:00] Role: "
                    let available = inner_width.saturating_sub(prefix_len);
                    let text = if content_line.chars().count() > available {
                        let truncated: String = content_line.chars().take(available.saturating_sub(3)).collect();
                        format!("{truncated}...")
                    } else {
                        content_line.to_string()
                    };

                    let item = ListItem::new(Line::from(vec![
                        Span::styled("[", Style::default().fg(theme.muted)),
                        Span::styled(timestamp.clone(), Style::default().fg(theme.muted)),
                        Span::styled("] ", Style::default().fg(theme.muted)),
                        Span::styled(role_name, Style::default().fg(role_color).add_modifier(Modifier::BOLD)),
                        Span::styled(": ", Style::default().fg(theme.text_dim)),
                        Span::styled(text, Style::default().fg(theme.text)),
                    ]));
                    list_items.push(item);
                } else {
                    // Continuation lines: indented
                    let indent_len = timestamp.len() + role_name.len() + 5;
                    let indent = " ".repeat(indent_len);
                    let available = inner_width.saturating_sub(indent_len);
                    let text = if content_line.chars().count() > available {
                        let truncated: String = content_line.chars().take(available.saturating_sub(3)).collect();
                        format!("{truncated}...")
                    } else {
                        content_line.to_string()
                    };

                    let item = ListItem::new(Line::from(vec![
                        Span::styled(indent, Style::default().fg(theme.muted)),
                        Span::styled(text, Style::default().fg(theme.text)),
                    ]));
                    list_items.push(item);
                }
            }

            // If the message has inline image preview lines, render them
            if let Some(ref img_lines) = msg.image_lines {
                for img_line in img_lines {
                    list_items.push(ListItem::new(img_line.clone()));
                }
            }
        }

        // Slice list_items to fit the visible area.
        // inner height = area height minus top/bottom borders (2 rows)
        let visible_rows = area.height.saturating_sub(2) as usize;
        let total = list_items.len();
        let items = if total > visible_rows {
            // Show the latest messages (from the bottom).
            // When scroll_offset < last message, user scrolled up → show from earlier.
            // Default scroll_offset = last msg index → show latest.
            let max_start = total.saturating_sub(visible_rows);
            // Use scroll_offset to determine how far back to show.
            // scroll_offset = msg index; map to approximate line offset.
            let scroll_back = self.messages.len().saturating_sub(1).saturating_sub(self.scroll_offset);
            let start = max_start.saturating_sub(scroll_back).min(max_start);
            list_items[start..].to_vec()
        } else {
            list_items
        };

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border))
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

    /// Remove and return the last message
    pub fn pop_last(&mut self) -> Option<ChatMessage> {
        self.messages.pop_back()
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

    /// Get current cursor row (0-based)
    pub fn cursor_row(&self) -> usize {
        self.buffer.cursor_row()
    }

    /// Compute how many terminal rows the prompt needs, given the available width.
    /// Returns a value clamped to [MIN_PROMPT_HEIGHT, MAX_PROMPT_HEIGHT].
    pub fn needed_height(&self, available_width: u16) -> u16 {
        const MAX_PROMPT_HEIGHT: u16 = 10;
        const MIN_PROMPT_HEIGHT: u16 = 3;

        let inner_width = available_width.saturating_sub(4) as usize; // 2 borders + 2 prefix
        if inner_width == 0 {
            return MIN_PROMPT_HEIGHT;
        }
        let input = self.input();
        if input.is_empty() {
            return MIN_PROMPT_HEIGHT;
        }

        let rows: usize = input.split('\n').map(|line| {
            let chars = line.chars().count();
            if chars == 0 { 1 } else { (chars + inner_width - 1) / inner_width }
        }).sum();

        let needed = (rows + 2) as u16; // +2 for top/bottom borders
        needed.clamp(MIN_PROMPT_HEIGHT, MAX_PROMPT_HEIGHT)
    }

    /// Wrap a single logical line into chunks that fit within `max_width` characters.
    fn wrap_line(line: &str, max_width: usize) -> Vec<String> {
        if max_width == 0 || line.is_empty() {
            return vec![line.to_string()];
        }
        let chars: Vec<char> = line.chars().collect();
        let mut result = Vec::new();
        let mut start = 0;
        while start < chars.len() {
            let end = (start + max_width).min(chars.len());
            result.push(chars[start..end].iter().collect());
            start = end;
        }
        result
    }

    /// Compute the (display_row, display_col) of the cursor, accounting for wrapping.
    fn cursor_display_pos(&self, inner_width: usize) -> (usize, usize) {
        let cursor_row = self.buffer.cursor_row();
        let cursor_col = self.buffer.cursor_col();
        let input = self.input();
        let lines: Vec<&str> = input.split('\n').collect();

        let mut display_row: usize = 0;
        for (row_idx, line) in lines.iter().enumerate() {
            let wrapped_count = if line.is_empty() {
                1
            } else {
                let c = line.chars().count();
                if inner_width > 0 { (c + inner_width - 1) / inner_width } else { 1 }
            };

            if row_idx == cursor_row {
                let wrap_row = if inner_width > 0 { cursor_col / inner_width } else { 0 };
                let wrap_col = if inner_width > 0 { cursor_col % inner_width } else { cursor_col };
                return (display_row + wrap_row, wrap_col);
            }
            display_row += wrapped_count;
        }
        (display_row, cursor_col)
    }

    /// Render the prompt widget
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let input_text = self.input();
        let inner_width = area.width.saturating_sub(4) as usize; // 2 borders + 2 prefix

        let mut display_lines: Vec<Line<'static>> = Vec::new();

        if input_text.is_empty() {
            display_lines.push(Line::from(vec![
                Span::styled("> ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
                Span::styled(self.placeholder.clone(), Style::default().fg(theme.muted)),
            ]));
        } else {
            let logical_lines: Vec<&str> = input_text.split('\n').collect();
            for (line_idx, logical_line) in logical_lines.iter().enumerate() {
                let wrapped = Self::wrap_line(logical_line, inner_width);
                for (wrap_idx, chunk) in wrapped.iter().enumerate() {
                    let prefix = if line_idx == 0 && wrap_idx == 0 {
                        Span::styled("> ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD))
                    } else {
                        Span::styled("  ", Style::default())
                    };
                    display_lines.push(Line::from(vec![
                        prefix,
                        Span::styled(chunk.clone(), Style::default().fg(theme.text)),
                    ]));
                }
            }
        }

        let paragraph = Paragraph::new(display_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border))
                    .title(" Input "),
            )
            .alignment(Alignment::Left);

        frame.render_widget(paragraph, area);

        // Show cursor
        if !input_text.is_empty() && inner_width > 0 {
            let (disp_row, disp_col) = self.cursor_display_pos(inner_width);
            let cursor_x = area.x + 1 + 2 + disp_col as u16; // border + prefix + col
            let cursor_y = area.y + 1 + disp_row as u16;     // top border + row
            if cursor_y < area.bottom() - 1 && cursor_x < area.right() - 1 {
                frame.set_cursor_position((cursor_x, cursor_y));
            }
        }
    }
}

impl Default for PromptWidget {
    fn default() -> Self {
        Self::new()
    }
}

/// Data needed to render the sidebar
pub struct SidebarInfo {
    /// Model name
    pub model: Option<String>,
    /// Tokens used so far
    pub tokens_used: u64,
    /// Total session cost in USD
    pub cost_usd: f64,
    /// Number of tools invoked
    pub tools_invoked: usize,
    /// Modified files: (path, additions, deletions)
    pub modified_files: Vec<(String, usize, usize)>,
    /// Total additions across all files
    pub total_additions: usize,
    /// Total deletions across all files
    pub total_deletions: usize,
}

/// Right sidebar panel showing session metadata
pub struct SidebarWidget;

/// Minimum terminal width for the sidebar to appear
const SIDEBAR_WIDTH: u16 = 28;
const MIN_MAIN_WIDTH: u16 = 50;

impl SidebarWidget {
    /// Render the sidebar panel
    pub fn render(frame: &mut Frame, area: Rect, info: &SidebarInfo, theme: &Theme) {
        let block = Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(theme.border))
            .title(Line::from(Span::styled(" Info ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD))));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let mut lines: Vec<Line<'static>> = Vec::new();
        let w = inner.width as usize;

        // Model section
        lines.push(Line::from(Span::styled("Model", Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD))));
        let model_name = info.model.as_deref().unwrap_or("unknown");
        lines.push(Line::from(Span::styled(truncate_to(model_name, w), Style::default().fg(theme.primary))));
        lines.push(Line::from(""));

        // Context usage
        lines.push(Line::from(Span::styled("Context", Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD))));
        let tokens_str = format_tokens(info.tokens_used);
        lines.push(Line::from(Span::styled(tokens_str, Style::default().fg(theme.text))));
        // Simple token bar
        let bar_filled = (info.tokens_used % 1000) as usize / 100; // rough visual
        let bar_str = format!("{}{}", "█".repeat(bar_filled.min(w/2)), "░".repeat((w/2).saturating_sub(bar_filled)));
        lines.push(Line::from(Span::styled(truncate_to(&bar_str, w), Style::default().fg(theme.secondary))));
        lines.push(Line::from(""));

        // Cost
        lines.push(Line::from(Span::styled("Cost", Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD))));
        let cost_str = format!("${:.4}", info.cost_usd);
        lines.push(Line::from(Span::styled(cost_str, Style::default().fg(theme.warning))));
        lines.push(Line::from(""));

        // Tools
        lines.push(Line::from(Span::styled("Tools", Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD))));
        lines.push(Line::from(Span::styled(info.tools_invoked.to_string(), Style::default().fg(theme.text))));
        lines.push(Line::from(""));

        // Modified files
        if !info.modified_files.is_empty() {
            lines.push(Line::from(Span::styled("Files", Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD))));
            let _total = format!("+{} -{}", info.total_additions, info.total_deletions);
            lines.push(Line::from(vec![
                Span::styled("+", Style::default().fg(theme.success)),
                Span::styled(info.total_additions.to_string(), Style::default().fg(theme.success)),
                Span::styled(" ", Style::default().fg(theme.text_dim)),
                Span::styled("-", Style::default().fg(theme.error)),
                Span::styled(info.total_deletions.to_string(), Style::default().fg(theme.error)),
            ]));
            for (path, adds, dels) in info.modified_files.iter().take(8) {
                let fname = path.split('/').next_back().unwrap_or(path);
                let changes = if *adds > 0 && *dels > 0 {
                    format!("+{}-{}", adds, dels)
                } else if *adds > 0 {
                    format!("+{}", adds)
                } else {
                    format!("-{}", dels)
                };
                lines.push(Line::from(vec![
                    Span::styled(truncate_to(fname, w - 8), Style::default().fg(theme.text)),
                    Span::styled(" ", Style::default().fg(theme.text_dim)),
                    Span::styled(changes, Style::default().fg(theme.muted)),
                ]));
            }
            if info.modified_files.len() > 8 {
                lines.push(Line::from(Span::styled(
                    format!("  ...+{} more", info.modified_files.len() - 8),
                    Style::default().fg(theme.muted),
                )));
            }
        }

        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, inner);
    }

    /// Check if the terminal is wide enough for the sidebar
    pub fn fits(total_width: u16) -> bool {
        total_width >= MIN_MAIN_WIDTH + SIDEBAR_WIDTH
    }

    /// Width the sidebar occupies (including border)
    pub fn width() -> u16 {
        SIDEBAR_WIDTH
    }
}

/// Truncate a string to fit within `max_chars` characters, appending "…" if truncated.
fn truncate_to(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else if max_chars > 1 {
        let truncated: String = s.chars().take(max_chars - 1).collect();
        format!("{truncated}…")
    } else {
        "…".to_string()
    }
}

/// Format token count as a human-readable string.
fn format_tokens(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}k", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}

/// Main UI layout widget
pub struct MainLayoutWidget;

impl MainLayoutWidget {
    /// Create the main layout chunks
    /// Returns (header_area, chat_area, prompt_area, status_area, full_area)
    pub fn layout(area: Rect, prompt_height: u16) -> (Rect, Rect, Rect, Rect, Rect) {
        let chunks = ratatui::layout::Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(HeaderWidget::height() as u16), // Header bar
                Constraint::Min(0),              // Chat area (flexible)
                Constraint::Length(prompt_height), // Input prompt (dynamic)
                Constraint::Length(1),            // Status bar (compressed single line)
            ])
            .split(area);

        let header_area = chunks[0];
        let chat_area = chunks[1];
        let prompt_area = chunks[2];
        let status_area = chunks[3];

        (header_area, chat_area, prompt_area, status_area, area)
    }

    /// Create layout with optional sidebar.
    /// When sidebar is visible and terminal is wide enough, splits the middle area horizontally.
    /// Returns (header_area, chat_area, prompt_area, status_area, sidebar_area, full_area)
    pub fn layout_with_sidebar(area: Rect, prompt_height: u16, sidebar_visible: bool) -> (Rect, Rect, Rect, Rect, Option<Rect>, Rect) {
        let (header_area, chat_area, prompt_area, status_area, full) = Self::layout(area, prompt_height);

        if sidebar_visible && SidebarWidget::fits(area.width) {
            // Split the vertical strip (header + chat + prompt) horizontally
            // The sidebar spans header + chat rows
            let sidebar_h = SidebarWidget::width();
            let _main_width = area.width.saturating_sub(sidebar_h);

            // Re-split the whole area with sidebar
            let h_chunks = ratatui::layout::Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Min(MIN_MAIN_WIDTH),
                    Constraint::Length(sidebar_h),
                ])
                .split(area);

            // Now re-do the vertical layout on the left chunk
            let v_chunks = ratatui::layout::Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([
                    Constraint::Length(HeaderWidget::height() as u16),
                    Constraint::Min(0),
                    Constraint::Length(prompt_height),
                    Constraint::Length(1),
                ])
                .split(h_chunks[0]);

            let sidebar_area = ratatui::layout::Rect {
                x: h_chunks[1].x,
                y: h_chunks[1].y + 1, // account for margin
                width: h_chunks[1].width,
                height: h_chunks[1].height.saturating_sub(2), // top+bottom margin
            };

            return (v_chunks[0], v_chunks[1], v_chunks[2], v_chunks[3], Some(sidebar_area), full);
        }

        (header_area, chat_area, prompt_area, status_area, None, full)
    }

    /// Render the complete UI
    #[allow(clippy::too_many_arguments)]
    pub fn render_complete(
        frame: &mut Frame,
        chat: &ChatWidget,
        prompt: &PromptWidget,
        status: &str,
        model: Option<&str>,
        tokens_used: Option<u64>,
        working_dir: &str,
        theme: &Theme,
    ) {
        Self::render_complete_with_spinner(frame, chat, prompt, status, model, tokens_used, working_dir, None, None, None, theme);
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
        sidebar_info: Option<&SidebarInfo>,
        theme: &Theme,
    ) {
        let area = frame.area();

        let prompt_height = prompt.needed_height(area.width);
        let sidebar_visible = sidebar_info.is_some();
        let (header_area, chat_area, prompt_area, status_area, sidebar_area, _) =
            Self::layout_with_sidebar(area, prompt_height, sidebar_visible);

        // Render each widget
        HeaderWidget::render(frame, header_area, model, tokens_used, working_dir, theme);
        chat.render(frame, chat_area, theme);
        prompt.render(frame, prompt_area, theme);
        StatusBarWidget::render_with_spinner(frame, status_area, status, model, tokens_used, spinner, progress_bar, theme);

        // Render sidebar if visible and there's space
        if let (Some(info), Some(sb_area)) = (sidebar_info, sidebar_area) {
            if sb_area.width > 5 && sb_area.height > 3 {
                SidebarWidget::render(frame, sb_area, info, theme);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

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
        assert_eq!(HeaderWidget::height(), 3);
    }

    #[test]
    fn test_header_widget_welcome_message() {
        let theme = Theme::default_dark();
        let spans = HeaderWidget::welcome_message(&theme);
        assert!(!spans.is_empty());
        assert_eq!(spans.len(), 3); // "Welcome to " + "Shannon" + "! "
    }

    #[test]
    fn test_header_widget_tip_message() {
        let theme = Theme::default_dark();
        let spans = HeaderWidget::tip_message(&theme);
        assert!(!spans.is_empty());
        assert_eq!(spans.len(), 2); // "Tip: " + tip text
    }

    // ── Main Layout Widget Tests ───────────────────────────────────────

    #[test]
    fn test_main_layout_widget_returns_five_chunks() {
        // Create a test area (100x20)
        let area = Rect::new(0, 0, 100, 20);
        let (header, chat, prompt, status, full) = MainLayoutWidget::layout(area, 3);

        // Header should be at top with height 3
        assert_eq!(header.y, 1); // margin(1)
        assert_eq!(header.height, 3);

        // Chat should be below header and be flexible
        assert_eq!(chat.y, 4); // margin(1) + header(3)
        assert!(chat.height > 0); // Flexible size

        // Prompt should be below chat with height 3
        assert_eq!(prompt.height, 3);

        // Status should be at bottom with height 1 (compressed)
        assert_eq!(status.height, 1);

        // Full area should match input area
        assert_eq!(full, area);
    }

    #[test]
    fn test_main_layout_widget_chat_area_is_flexible() {
        let small_area = Rect::new(0, 0, 80, 10);
        let (_, small_chat, _, _, _) = MainLayoutWidget::layout(small_area, 3);

        let large_area = Rect::new(0, 0, 80, 30);
        let (_, large_chat, _, _, _) = MainLayoutWidget::layout(large_area, 3);

        // Chat area should grow with available space
        assert!(large_chat.height > small_chat.height);
    }

    #[test]
    fn test_main_layout_widget_fixed_sizes() {
        let area = Rect::new(0, 0, 100, 20);
        let (header, _, prompt, status, _) = MainLayoutWidget::layout(area, 3);

        // Header and prompt have fixed heights; status bar is 1 line (compressed)
        assert_eq!(header.height, 3);
        assert_eq!(prompt.height, 3);
        assert_eq!(status.height, 1);
    }

    #[test]
    fn test_main_layout_widget_margins() {
        let area = Rect::new(0, 0, 100, 20);
        let (header, _, _, _, _) = MainLayoutWidget::layout(area, 3);

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
            image_lines: None,
            is_error: false,
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
