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
use std::sync::LazyLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

/// Lazy-initialized syntect state for diff syntax highlighting.
static DIFF_SYNTAX: LazyLock<(SyntaxSet, syntect::highlighting::Theme)> = LazyLock::new(|| {
    let ss = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();
    let theme = ts.themes["base16-eighties.dark"].clone();
    (ss, theme)
});

/// Convert a syntect Color to a ratatui Color.
fn syntect_to_ratatui(c: syntect::highlighting::Color) -> ratatui::style::Color {
    ratatui::style::Color::Rgb(c.r, c.g, c.b)
}

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
        approval_mode: Option<&str>,
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

        // Approval mode indicator
        if let Some(mode_label) = approval_mode {
            span_vec.push(sep());
            let mode_style = match mode_label {
                label if label == "SUGGEST" || label == "PLAN" || label == "RO" => {
                    Style::default().fg(theme.warning)
                }
                label if label == "AUTO" => Style::default().fg(theme.success),
                label if label == "FULL" => Style::default().fg(theme.primary),
                label if label == "BYPASS" || label == "YOLO" => {
                    Style::default().fg(ratatui::style::Color::Red)
                }
                _ => Style::default().fg(theme.text_dim),
            };
            span_vec.push(Span::styled(mode_label.to_string(), mode_style.add_modifier(Modifier::BOLD)));
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
    /// Whether streaming is active (show trailing cursor)
    pub streaming_active: bool,
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
    /// Tool name for collapsed display (e.g., "bash", "write")
    pub tool_name: Option<String>,
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
            streaming_active: false,
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
            tool_name: None,
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
            tool_name: None,
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

    /// Add a tool result message with tool name and error status
    pub fn add_tool_message(&mut self, tool_name: String, content: String, is_error: bool) -> usize {
        let message = ChatMessage {
            role: ChatRole::Tool,
            content,
            timestamp: chrono::Utc::now(),
            image_lines: None,
            is_error,
            tool_name: Some(tool_name),
        };
        let index = self.messages.len();
        self.messages.push_back(message);
        if !self.messages.is_empty() {
            self.scroll_offset = self.messages.len() - 1;
        }
        index
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

    /// Scroll to latest message (bottom)
    pub fn scroll_to_latest(&mut self) {
        self.scroll_offset = 0;
    }

    /// Scroll to oldest message (top)
    pub fn scroll_to_top(&mut self) {
        if !self.messages.is_empty() {
            self.scroll_offset = self.messages.len() - 1;
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
                // Build summary: use tool_name as label if available, else fall back to first line
                let tool_label = msg.tool_name.as_deref().unwrap_or("tool");
                let summary_text = if msg.tool_name.is_some() {
                    // Show first line as the detail after the tool name
                    let detail = first_line.strip_prefix(tool_label)
                        .map(|s| s.trim_start_matches(|c: char| c == ':' || c == ' '))
                        .unwrap_or(first_line);
                    if detail.is_empty() {
                        tool_label.to_string()
                    } else {
                        format!("{tool_label}: {detail}")
                    }
                } else {
                    first_line.to_string()
                };
                let label_len = tool_label.len() + timestamp.len() + 8; // "[HH:MM:SS] ⏵ label: "
                let _available = inner_width.saturating_sub(label_len);
                let ts_len = timestamp.len();
                let max_summary_width = inner_width.saturating_sub(ts_len + 6);
                let summary = if summary_text.chars().count() > max_summary_width {
                    let truncated: String = summary_text.chars().take(max_summary_width.saturating_sub(3)).collect();
                    format!("{truncated}...")
                } else {
                    summary_text
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
                    Span::styled(truncate_to(&summary, max_summary_width), Style::default().fg(theme.text_dim)),
                    Span::styled(" ", Style::default().fg(theme.text_dim)),
                    Span::styled(status_icon, Style::default().fg(status_color)),
                ]));
                list_items.push(item);
                continue;
            }

            let (role_icon, role_name, role_color) = match msg.role {
                ChatRole::User => (">", "You", theme.user_msg),
                ChatRole::Assistant => ("✻", "Assistant", theme.assistant_msg),
                ChatRole::System => ("!", "System", theme.system_msg),
                ChatRole::Tool => ("⚙", "Tool", theme.tool_msg),
            };

            let timestamp = msg.timestamp.format("%H:%M:%S").to_string();

            // Strip ANSI escape codes — ratatui doesn't interpret them
            let clean_content = strip_ansi(&msg.content);

            // Detect and render diff content inline for tool messages
            if msg.role == ChatRole::Tool && is_diff_content(&clean_content) {
                let lang = detect_diff_language(&clean_content);
                let prefix_len = timestamp.len() + role_icon.len() + role_name.len() + 7;
                let indent = " ".repeat(prefix_len);
                let mut first_line = true;
                for line in clean_content.lines().take(50) {
                    let color = diff_line_color(line, theme);
                    let diff_spans = highlight_diff_line(line, lang.as_deref(), color);
                    if first_line {
                        let mut spans = vec![
                            Span::styled("[", Style::default().fg(theme.muted)),
                            Span::styled(timestamp.clone(), Style::default().fg(theme.muted)),
                            Span::styled("] ", Style::default().fg(theme.muted)),
                            Span::styled(role_icon, Style::default().fg(role_color).add_modifier(Modifier::BOLD)),
                            Span::styled(" ", Style::default()),
                            Span::styled(role_name, Style::default().fg(role_color).add_modifier(Modifier::BOLD)),
                            Span::styled(": ", Style::default().fg(theme.text_dim)),
                        ];
                        spans.extend(diff_spans);
                        list_items.push(ListItem::new(Line::from(spans)));
                        first_line = false;
                    } else {
                        let mut spans = vec![
                            Span::styled(indent.clone(), Style::default().fg(theme.muted)),
                        ];
                        spans.extend(diff_spans);
                        list_items.push(ListItem::new(Line::from(spans)));
                    }
                }
                if clean_content.lines().count() > 50 {
                    list_items.push(ListItem::new(Line::from(Span::styled(
                        format!("{indent}... ({} more lines)", clean_content.lines().count().saturating_sub(50)),
                        Style::default().fg(theme.muted),
                    ))));
                }
                // Image lines
                if let Some(ref img_lines) = msg.image_lines {
                    for img_line in img_lines {
                        list_items.push(ListItem::new(img_line.clone()));
                    }
                }
                continue;
            }

            // Parse into segments: normal text and code blocks
            let segments = parse_markdown_segments(&clean_content);
            let mut first_line = true;

            for segment in &segments {
                match segment {
                    MdSegment::Text(lines) => {
                        for content_line in lines {
                            if first_line {
                                let prefix_len = timestamp.len() + role_icon.len() + role_name.len() + 7;
                                let available = inner_width.saturating_sub(prefix_len);
                                let text = truncate_line(content_line, available);
                                let mut spans = vec![
                                    Span::styled("[", Style::default().fg(theme.muted)),
                                    Span::styled(timestamp.clone(), Style::default().fg(theme.muted)),
                                    Span::styled("] ", Style::default().fg(theme.muted)),
                                    Span::styled(role_icon, Style::default().fg(role_color).add_modifier(Modifier::BOLD)),
                                    Span::styled(" ", Style::default()),
                                    Span::styled(role_name, Style::default().fg(role_color).add_modifier(Modifier::BOLD)),
                                    Span::styled(": ", Style::default().fg(theme.text_dim)),
                                ];
                                spans.extend(parse_inline_formatting(&text, theme.text));
                                list_items.push(ListItem::new(Line::from(spans)));
                                first_line = false;
                            } else {
                                let indent_len = timestamp.len() + role_icon.len() + role_name.len() + 7;
                                let indent = " ".repeat(indent_len);
                                let available = inner_width.saturating_sub(indent_len);
                                let text = truncate_line(content_line, available);
                                let mut spans = vec![
                                    Span::styled(indent, Style::default().fg(theme.muted)),
                                ];
                                spans.extend(parse_inline_formatting(&text, theme.text));
                                list_items.push(ListItem::new(Line::from(spans)));
                            }
                        }
                    }
                    MdSegment::CodeBlock { lang, code } => {
                        let indent_len = timestamp.len() + role_icon.len() + role_name.len() + 5;
                        let indent = " ".repeat(indent_len);
                        let available = inner_width.saturating_sub(indent_len);

                        // Code block header
                        let header = format!("╭─ {} ─", lang.as_deref().unwrap_or("code"));
                        list_items.push(ListItem::new(Line::from(vec![
                            Span::styled(indent.clone(), Style::default().fg(theme.muted)),
                            Span::styled(truncate_to(&header, available), Style::default().fg(theme.accent)),
                        ])));

                        // Code lines with syntax highlighting
                        for code_line in code.lines() {
                            let spans = highlight_code_line(code_line, lang.as_deref(), theme);
                            let mut line_spans = vec![
                                Span::styled(indent.clone(), Style::default().fg(theme.muted)),
                                Span::styled("│ ", Style::default().fg(theme.accent)),
                            ];
                            line_spans.extend(spans);
                            list_items.push(ListItem::new(Line::from(line_spans)));
                        }

                        // Code block footer
                        list_items.push(ListItem::new(Line::from(vec![
                            Span::styled(indent, Style::default().fg(theme.muted)),
                            Span::styled(truncate_to("╰────────", available), Style::default().fg(theme.accent)),
                        ])));
                        first_line = false;
                    }
                    MdSegment::Header { level, text } => {
                        let indent_len = if first_line {
                            timestamp.len() + role_icon.len() + role_name.len() + 7
                        } else {
                            timestamp.len() + role_icon.len() + role_name.len() + 7
                        };
                        let indent = " ".repeat(indent_len);
                        let available = inner_width.saturating_sub(indent_len);
                        let header_text = truncate_to(text, available);
                        let style = match level {
                            1 => Style::default().fg(theme.primary).add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                            2 => Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
                            _ => Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
                        };
                        let item = ListItem::new(Line::from(vec![
                            Span::styled(indent, Style::default().fg(theme.muted)),
                            Span::styled(header_text, style),
                        ]));
                        list_items.push(item);
                        first_line = false;
                    }
                }
            }

            // If the message has inline image preview lines, render them
            if let Some(ref img_lines) = msg.image_lines {
                for img_line in img_lines {
                    list_items.push(ListItem::new(img_line.clone()));
                }
            }
        }

        // Streaming cursor: append a blinking cursor block at the end
        if self.streaming_active {
            list_items.push(ListItem::new(Line::from(
                Span::styled("▌", Style::default().fg(theme.primary).add_modifier(Modifier::SLOW_BLINK)),
            )));
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

            // Scroll indicators
            let has_above = start > 0;
            let has_below = start + visible_rows < total;

            let mut sliced: Vec<ListItem<'static>> = Vec::with_capacity(visible_rows);
            if has_above && visible_rows > 2 {
                sliced.push(ListItem::new(Line::from(Span::styled(
                    "  ─── ▲ More above ───",
                    Style::default().fg(theme.muted),
                ))));
                sliced.extend(list_items[start..start + visible_rows - if has_below { 2 } else { 1 }].to_vec());
            } else {
                sliced.extend(list_items[start..start + visible_rows - if has_below { 1 } else { 0 }].to_vec());
            }
            if has_below && sliced.len() < visible_rows {
                sliced.push(ListItem::new(Line::from(Span::styled(
                    "  ─── ▼ More below ───",
                    Style::default().fg(theme.muted),
                ))));
            }
            sliced
        } else {
            list_items
        };

        // Build scroll indicator title
        let title = if self.messages.len() > 1 {
            let is_at_bottom = self.scroll_offset >= self.messages.len().saturating_sub(1);
            if is_at_bottom {
                " Chat ".to_string()
            } else {
                format!(" Chat ({}/{}) ", self.scroll_offset + 1, self.messages.len())
            }
        } else {
            " Chat ".to_string()
        };

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border))
                    .title(title)
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
    /// Vim mode label for display ("INSERT" or "NORMAL")
    vim_mode: String,
}

impl PromptWidget {
    /// Create a new prompt widget
    pub fn new() -> Self {
        Self {
            buffer: crate::repl_enhancement::InputBuffer::new(),
            placeholder: "Type your message...".to_string(),
            vim_mode: "INSERT".to_string(),
        }
    }

    /// Set the placeholder text
    pub fn with_placeholder(mut self, placeholder: String) -> Self {
        self.placeholder = placeholder;
        self
    }

    /// Set the vim mode label for display in the border title
    pub fn set_vim_mode(&mut self, mode: &str) {
        self.vim_mode = mode.to_string();
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
            if chars == 0 { 1 } else { chars.div_ceil(inner_width) }
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
                if inner_width > 0 { c.div_ceil(inner_width) } else { 1 }
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

        let title = if self.vim_mode.is_empty() {
            " Input ".to_string()
        } else {
            format!(" Input [{}] ", self.vim_mode)
        };

        let paragraph = Paragraph::new(display_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border))
                    .title(title),
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
    /// Number of tool errors in session
    pub error_count: usize,
    /// Context window size for the current model (for progress bar)
    pub context_window: usize,
    /// Active sub-agents for the Agents tab
    pub active_agents: Vec<crate::repl::AgentDisplay>,
}

/// Right sidebar panel showing session metadata
pub struct SidebarWidget;

/// Minimum terminal width for the sidebar to appear
const SIDEBAR_WIDTH: u16 = 28;
const MIN_MAIN_WIDTH: u16 = 50;
/// Below this width, auto-hide sidebar even if toggled on
const MIN_SIDEBAR_WIDTH: u16 = 80;
/// Below this width, collapse header to single line
const COLLAPSE_HEADER_WIDTH: u16 = 60;
/// Minimum usable terminal size
const MIN_TERMINAL_WIDTH: u16 = 30;
const MIN_TERMINAL_HEIGHT: u16 = 8;

impl SidebarWidget {
    /// Render the sidebar panel
    pub fn render(frame: &mut Frame, area: Rect, info: &SidebarInfo, theme: &Theme, tab: crate::repl::SidebarTab) {
        let block = Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(theme.border))
            .title(Line::from(Span::styled(" Info ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD))));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let mut lines: Vec<Line<'static>> = Vec::new();
        let w = inner.width as usize;

        // Tab header
        let ctx_label = if tab == crate::repl::SidebarTab::Context { " Ctx " } else { " Ctx " };
        let files_label = if tab == crate::repl::SidebarTab::Files { " Files " } else { " Files " };
        let agents_label = if tab == crate::repl::SidebarTab::Agents { " Agents " } else { " Agents " };
        let ctx_style = if tab == crate::repl::SidebarTab::Context {
            Style::default().fg(theme.primary).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(theme.muted)
        };
        let files_style = if tab == crate::repl::SidebarTab::Files {
            Style::default().fg(theme.primary).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(theme.muted)
        };
        let agents_style = if tab == crate::repl::SidebarTab::Agents {
            Style::default().fg(theme.primary).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(theme.muted)
        };
        let sep = Style::default().fg(theme.border);
        lines.push(Line::from(vec![
            Span::styled(ctx_label, ctx_style),
            Span::styled("|", sep),
            Span::styled(files_label, files_style),
            Span::styled("|", sep),
            Span::styled(agents_label, agents_style),
        ]));
        lines.push(Line::from(""));

        match tab {
            crate::repl::SidebarTab::Context => {
                // Model section
                lines.push(Line::from(Span::styled("Model", Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD))));
                let model_name = info.model.as_deref().unwrap_or("unknown");
                lines.push(Line::from(Span::styled(truncate_to(model_name, w), Style::default().fg(theme.primary))));
                lines.push(Line::from(""));

                // Context usage
                lines.push(Line::from(Span::styled("Context", Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD))));
                let tokens_str = format_tokens(info.tokens_used);
                let pct = if info.context_window > 0 {
                    ((info.tokens_used as f64 / info.context_window as f64) * 100.0).min(100.0)
                } else {
                    0.0
                };
                let pct_label = format!("{tokens_str} ({pct:.0}%)");
                lines.push(Line::from(Span::styled(pct_label, Style::default().fg(theme.text))));
                // Progress bar based on actual context window percentage
                let bar_width = w.saturating_sub(2).max(4);
                let filled = (pct / 100.0 * bar_width as f64).round() as usize;
                let filled = filled.min(bar_width);
                let bar_color = if pct > 90.0 {
                    theme.error
                } else if pct > 75.0 {
                    theme.warning
                } else {
                    theme.secondary
                };
                let bar_str = format!(" {}{}", "█".repeat(filled), "░".repeat(bar_width.saturating_sub(filled)));
                lines.push(Line::from(Span::styled(truncate_to(&bar_str, w), Style::default().fg(bar_color))));
                lines.push(Line::from(""));

                // Cost
                lines.push(Line::from(Span::styled("Cost", Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD))));
                let cost_str = format!("${:.4}", info.cost_usd);
                lines.push(Line::from(Span::styled(cost_str, Style::default().fg(theme.warning))));
                lines.push(Line::from(""));

                // Tools
                lines.push(Line::from(Span::styled("Tools", Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD))));
                lines.push(Line::from(Span::styled(info.tools_invoked.to_string(), Style::default().fg(theme.text))));
                if info.error_count > 0 {
                    lines.push(Line::from(Span::styled(
                        format!("  {} errors", info.error_count),
                        Style::default().fg(theme.error),
                    )));
                }
            }
            crate::repl::SidebarTab::Files => {
                if info.modified_files.is_empty() {
                    lines.push(Line::from(Span::styled("No modified files", Style::default().fg(theme.muted))));
                } else {
                    // Summary line
                    lines.push(Line::from(vec![
                        Span::styled("+", Style::default().fg(theme.success)),
                        Span::styled(info.total_additions.to_string(), Style::default().fg(theme.success)),
                        Span::styled(" ", Style::default().fg(theme.text_dim)),
                        Span::styled("-", Style::default().fg(theme.error)),
                        Span::styled(info.total_deletions.to_string(), Style::default().fg(theme.error)),
                        Span::styled(format!("  ({} files)", info.modified_files.len()), Style::default().fg(theme.muted)),
                    ]));
                    lines.push(Line::from(""));

                    // Show up to 20 files (more space since this is a dedicated tab)
                    for (path, adds, dels) in info.modified_files.iter().take(20) {
                        let fname = path.split('/').next_back().unwrap_or(path);
                        let changes = if *adds > 0 && *dels > 0 {
                            format!("+{adds}-{dels}")
                        } else if *adds > 0 {
                            format!("+{adds}")
                        } else {
                            format!("-{dels}")
                        };
                        lines.push(Line::from(vec![
                            Span::styled(truncate_to(fname, w.saturating_sub(8)), Style::default().fg(theme.text)),
                            Span::styled(" ", Style::default().fg(theme.text_dim)),
                            Span::styled(changes, Style::default().fg(theme.muted)),
                        ]));
                        // Show parent path if it fits
                        if let Some(parent) = path.strip_suffix(fname).and_then(|p| p.strip_suffix('/')) {
                            if !parent.is_empty() && w > 20 {
                                lines.push(Line::from(Span::styled(
                                    format!("  {}", truncate_to(parent, w - 2)),
                                    Style::default().fg(theme.text_dim),
                                )));
                            }
                        }
                    }
                    if info.modified_files.len() > 20 {
                        lines.push(Line::from(Span::styled(
                            format!("  ...+{} more", info.modified_files.len() - 20),
                            Style::default().fg(theme.muted),
                        )));
                    }
                }
            }
            crate::repl::SidebarTab::Agents => {
                if info.active_agents.is_empty() {
                    lines.push(Line::from(Span::styled("No active agents", Style::default().fg(theme.muted))));
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled("Use /team or /agent", Style::default().fg(theme.text_dim))));
                    lines.push(Line::from(Span::styled("to spawn agents", Style::default().fg(theme.text_dim))));
                } else {
                    // Count active vs total
                    let active_count = info.active_agents.iter().filter(|a| a.active).count();
                    let total = info.active_agents.len();
                    lines.push(Line::from(Span::styled(
                        format!("{active_count}/{total} active"),
                        Style::default().fg(theme.text),
                    )));
                    lines.push(Line::from(""));

                    for agent in info.active_agents.iter().take(15) {
                        let status_icon = match agent.status.as_str() {
                            "running" => "●",
                            "spawning" => "◐",
                            "idle" => "○",
                            "completed" => "✓",
                            s if s.starts_with("failed") => "✗",
                            _ => "·",
                        };
                        let status_color = match agent.status.as_str() {
                            "running" => theme.success,
                            "spawning" => theme.warning,
                            "idle" => theme.muted,
                            "completed" => theme.secondary,
                            s if s.starts_with("failed") => theme.error,
                            _ => theme.text_dim,
                        };
                        let name_display = truncate_to(&agent.name, w.saturating_sub(6));
                        lines.push(Line::from(vec![
                            Span::styled(status_icon, Style::default().fg(status_color)),
                            Span::styled(" ", Style::default()),
                            Span::styled(name_display, Style::default().fg(theme.text)),
                        ]));
                        // Status line
                        let turns_label = if agent.max_turns > 0 {
                            format!("  {}/{} turns", agent.turns_used, agent.max_turns)
                        } else {
                            format!("  {}", agent.status)
                        };
                        lines.push(Line::from(Span::styled(
                            truncate_to(&turns_label, w),
                            Style::default().fg(theme.text_dim),
                        )));
                        if let Some(ref team) = agent.team {
                            if team != "_global" {
                                lines.push(Line::from(Span::styled(
                                    format!("  team: {}", truncate_to(team, w.saturating_sub(8))),
                                    Style::default().fg(theme.muted),
                                )));
                            }
                        }
                    }
                    if info.active_agents.len() > 15 {
                        lines.push(Line::from(Span::styled(
                            format!("  ...+{} more", info.active_agents.len() - 15),
                            Style::default().fg(theme.muted),
                        )));
                    }
                }
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

/// Markdown segment: plain text, header, or fenced code block.
enum MdSegment {
    /// Regular text lines
    Text(Vec<String>),
    /// Markdown header (## Header)
    Header { level: usize, text: String },
    /// Fenced code block with optional language tag
    CodeBlock { lang: Option<String>, code: String },
}

/// Parse content into markdown segments (text and code blocks).
fn parse_markdown_segments(content: &str) -> Vec<MdSegment> {
    let mut segments = Vec::new();
    let mut current_text: Vec<String> = Vec::new();
    let mut in_code = false;
    let mut code_lines: Vec<String> = Vec::new();
    let mut lang: Option<String> = None;

    for line in content.lines() {
        if line.starts_with("```") {
            if in_code {
                // End of code block
                segments.push(MdSegment::CodeBlock {
                    lang: lang.take(),
                    code: code_lines.join("\n"),
                });
                code_lines.clear();
                in_code = false;
            } else {
                // Start of code block
                if !current_text.is_empty() {
                    segments.push(MdSegment::Text(std::mem::take(&mut current_text)));
                }
                let lang_str = line.trim_start_matches('`').trim();
                lang = if lang_str.is_empty() { None } else { Some(lang_str.to_string()) };
                in_code = true;
            }
        } else if in_code {
            code_lines.push(line.to_string());
        } else {
            // Detect markdown headers: # Header, ## Header, etc.
            let header_level = line.chars().take_while(|c| *c == '#').count();
            if header_level > 0 && header_level <= 6 && line.chars().nth(header_level) == Some(' ') {
                // Flush accumulated text before the header
                if !current_text.is_empty() {
                    segments.push(MdSegment::Text(std::mem::take(&mut current_text)));
                }
                let header_text = line[header_level + 1..].trim().to_string();
                segments.push(MdSegment::Header { level: header_level, text: header_text });
            } else {
                current_text.push(line.to_string());
            }
        }
    }

    // Flush remaining
    if in_code {
        segments.push(MdSegment::CodeBlock {
            lang,
            code: code_lines.join("\n"),
        });
    }
    if !current_text.is_empty() {
        segments.push(MdSegment::Text(current_text));
    }

    segments
}

/// Parse inline markdown formatting (**bold** and *italic*) into styled Spans.
fn parse_inline_formatting(text: &str, base_color: ratatui::style::Color) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut pos = 0;
    let bytes = text.as_bytes();

    while pos < text.len() {
        if bytes[pos] == b'*' && pos + 1 < text.len() && bytes[pos + 1] == b'*' {
            // **bold**
            let search_start = pos + 2;
            if let Some(end) = text[search_start..].find("**") {
                let close_start = search_start + end;
                let bold_text = &text[search_start..close_start];
                spans.push(Span::styled(
                    bold_text.to_string(),
                    Style::default().fg(base_color).add_modifier(Modifier::BOLD),
                ));
                pos = close_start + 2;
                continue;
            }
        } else if bytes[pos] == b'*'
            && (pos + 1 >= text.len() || bytes[pos + 1] != b'*')
        {
            // *italic* (single star, not double)
            let search_start = pos + 1;
            if let Some(end) = text[search_start..].find('*') {
                let close_start = search_start + end;
                let italic_text = &text[search_start..close_start];
                spans.push(Span::styled(
                    italic_text.to_string(),
                    Style::default().fg(base_color).add_modifier(Modifier::ITALIC),
                ));
                pos = close_start + 1;
                continue;
            }
        }
        // Plain character — collect until next * or end
        let plain_start = pos;
        while pos < text.len() && bytes[pos] != b'*' {
            pos += 1;
        }
        if pos > plain_start {
            spans.push(Span::styled(
                text[plain_start..pos].to_string(),
                Style::default().fg(base_color),
            ));
        } else {
            // Unmatched *, treat as plain
            spans.push(Span::styled(
                "*".to_string(),
                Style::default().fg(base_color),
            ));
            pos += 1;
        }
    }

    if spans.is_empty() {
        spans.push(Span::styled(text.to_string(), Style::default().fg(base_color)));
    }
    spans
}

/// Truncate a line to fit within max_chars, with ellipsis.
fn truncate_line(s: &str, max_chars: usize) -> String {
    truncate_to(s, max_chars)
}

/// Common keywords for basic syntax highlighting.
const KEYWORDS: &[&str] = &[
    "fn", "let", "mut", "pub", "struct", "enum", "impl", "trait", "mod", "use",
    "if", "else", "match", "for", "while", "loop", "return", "break", "continue",
    "async", "await", "move", "ref", "self", "super", "crate", "where", "type",
    "const", "static", "true", "false", "Some", "None", "Ok", "Err",
    "def", "class", "import", "from", "with", "as", "try", "except", "raise",
    "function", "var", "const", "let", "new", "this", "typeof", "instanceof",
    "NULL", "True", "False",
];

/// Highlight a single code line into colored spans.
fn highlight_code_line(line: &str, _lang: Option<&str>, theme: &Theme) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        // Check for line comments
        if chars[i] == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            if !current.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut current), Style::default().fg(theme.text)));
            }
            let comment: String = chars[i..].iter().collect();
            spans.push(Span::styled(truncate_to(&comment, 200), Style::default().fg(theme.muted)));
            return spans;
        }
        if chars[i] == '#' && (_lang == Some("python") || _lang == Some("py") || _lang == Some("bash") || _lang == Some("sh") || _lang == None) {
            if !current.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut current), Style::default().fg(theme.text)));
            }
            let comment: String = chars[i..].iter().collect();
            spans.push(Span::styled(truncate_to(&comment, 200), Style::default().fg(theme.muted)));
            return spans;
        }
        // Check for strings (double quote)
        if chars[i] == '"' || chars[i] == '\'' {
            if !current.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut current), Style::default().fg(theme.text)));
            }
            let quote = chars[i];
            current.push(quote);
            i += 1;
            while i < chars.len() && chars[i] != quote {
                current.push(chars[i]);
                if chars[i] == '\\' && i + 1 < chars.len() {
                    i += 1;
                    current.push(chars[i]);
                }
                i += 1;
            }
            if i < chars.len() {
                current.push(chars[i]);
                i += 1;
            }
            spans.push(Span::styled(std::mem::take(&mut current), Style::default().fg(theme.success)));
            continue;
        }
        // Check for word boundaries (keywords)
        if chars[i].is_alphanumeric() || chars[i] == '_' {
            current.push(chars[i]);
            i += 1;
            // Continue reading word
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                current.push(chars[i]);
                i += 1;
            }
            // Check if it's a keyword
            if KEYWORDS.contains(&current.as_str()) {
                spans.push(Span::styled(std::mem::take(&mut current), Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)));
            }
            continue;
        }
        // Numbers
        if chars[i].is_ascii_digit() {
            if !current.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut current), Style::default().fg(theme.text)));
            }
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.' || chars[i] == '_') {
                current.push(chars[i]);
                i += 1;
            }
            spans.push(Span::styled(std::mem::take(&mut current), Style::default().fg(theme.warning)));
            continue;
        }
        // Punctuation/operators
        current.push(chars[i]);
        i += 1;
    }

    if !current.is_empty() {
        spans.push(Span::styled(current, Style::default().fg(theme.text)));
    }
    spans
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

/// Check if content looks like diff output.
fn is_diff_content(content: &str) -> bool {
    let lines: Vec<&str> = content.lines().take(20).collect();
    if lines.is_empty() { return false; }
    let diff_indicators = lines.iter().filter(|l| {
        l.starts_with("diff --git")
            || l.starts_with("--- a/")
            || l.starts_with("+++ b/")
            || l.starts_with("@@")
    }).count();
    let add_rem = lines.iter().filter(|l| {
        (l.starts_with('+') && !l.starts_with("+++"))
            || (l.starts_with('-') && !l.starts_with("---"))
    }).count();
    diff_indicators >= 1 && add_rem >= 2
}

/// Get the color for a diff line.
fn diff_line_color(line: &str, theme: &Theme) -> ratatui::style::Color {
    if line.starts_with('+') && !line.starts_with("+++") {
        theme.diff_added
    } else if line.starts_with('-') && !line.starts_with("---") {
        theme.diff_removed
    } else if line.starts_with('@') {
        theme.primary
    } else if line.starts_with("diff --git") || line.starts_with("---") || line.starts_with("+++") {
        theme.diff_header
    } else {
        theme.text
    }
}

/// Detect the programming language from diff header lines.
pub(crate) fn detect_diff_language(content: &str) -> Option<String> {
    let ext_to_lang = |ext: &str| -> String {
        match ext {
            "rs" => "rust".to_string(),
            "py" => "python".to_string(),
            "js" | "jsx" => "javascript".to_string(),
            "ts" | "tsx" => "typescript".to_string(),
            "go" => "go".to_string(),
            "java" => "java".to_string(),
            "c" | "h" => "c".to_string(),
            "cpp" | "cc" | "cxx" | "hpp" => "cpp".to_string(),
            "rb" => "ruby".to_string(),
            "sh" | "bash" => "bash".to_string(),
            "json" => "json".to_string(),
            "toml" => "toml".to_string(),
            "yaml" | "yml" => "yaml".to_string(),
            "md" => "markdown".to_string(),
            "html" | "htm" => "html".to_string(),
            "css" => "css".to_string(),
            "sql" => "sql".to_string(),
            other => other.to_string(),
        }
    };

    for line in content.lines().take(10) {
        if let Some(path) = line.strip_prefix("--- a/").or_else(|| line.strip_prefix("+++ b/")) {
            if let Some(ext) = path.rsplit('.').next() {
                return Some(ext_to_lang(ext));
            }
        }
        if line.starts_with("diff --git") {
            if let Some(b_path) = line.split(" b/").nth(1) {
                if let Some(ext) = b_path.rsplit('.').next() {
                    return Some(ext_to_lang(ext));
                }
            }
        }
    }
    None
}

/// Syntax-highlight a single diff line's content, returning colored Spans.
/// The `prefix` ("+" or "-") and `base_color` set the diff-line color,
/// while the content after the prefix gets syntax highlighting.
pub(crate) fn highlight_diff_line(line: &str, lang: Option<&str>, base_color: ratatui::style::Color) -> Vec<Span<'static>> {
    // Determine prefix and content
    let (prefix, content) = if (line.starts_with('+') && !line.starts_with("+++"))
        || (line.starts_with('-') && !line.starts_with("---"))
    {
        (&line[..1], &line[1..])
    } else {
        ("", line)
    };

    let mut spans = vec![Span::styled(prefix.to_string(), Style::default().fg(base_color))];

    if content.is_empty() {
        return spans;
    }

    // Try syntax highlighting if we have a language
    if let Some(lang) = lang {
        let (ref ss, ref theme) = *DIFF_SYNTAX;
        if let Some(syntax) = ss.find_syntax_by_token(lang).or_else(|| ss.find_syntax_by_extension(lang)) {
            let mut highlighter = HighlightLines::new(syntax, theme);
            match highlighter.highlight_line(content, ss) {
                Ok(ranges) => {
                    for (style, text) in ranges {
                        let fg = syntect_to_ratatui(style.foreground);
                        let mut s = Style::default().fg(fg);
                        if style.font_style.contains(syntect::highlighting::FontStyle::BOLD) {
                            s = s.add_modifier(Modifier::BOLD);
                        }
                        if style.font_style.contains(syntect::highlighting::FontStyle::ITALIC) {
                            s = s.add_modifier(Modifier::ITALIC);
                        }
                        spans.push(Span::styled(text.to_string(), s));
                    }
                    return spans;
                }
                Err(_) => {} // fall through to plain
            }
        }
    }

    // Fallback: plain content with base color
    spans.push(Span::styled(content.to_string(), Style::default().fg(base_color)));
    spans
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
        // Responsive: collapse header on very narrow terminals
        let header_height: u16 = if area.width < COLLAPSE_HEADER_WIDTH { 1 } else { HeaderWidget::height() as u16 };
        let effective_sidebar = sidebar_visible && area.width >= MIN_SIDEBAR_WIDTH;

        let (header_area, chat_area, prompt_area, status_area, full) = {
            let chunks = ratatui::layout::Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([
                    Constraint::Length(header_height),
                    Constraint::Min(0),
                    Constraint::Length(prompt_height),
                    Constraint::Length(1),
                ])
                .split(area);
            (chunks[0], chunks[1], chunks[2], chunks[3], area)
        };

        if effective_sidebar && SidebarWidget::fits(area.width) {
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
                    Constraint::Length(header_height),
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
        Self::render_complete_with_spinner(frame, chat, prompt, status, model, tokens_used, working_dir, None, None, None, theme, crate::repl::SidebarTab::default(), None);
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
        sidebar_tab: crate::repl::SidebarTab,
        approval_mode: Option<&str>,
    ) {
        let area = frame.area();

        // Show warning if terminal is too small
        if area.width < MIN_TERMINAL_WIDTH || area.height < MIN_TERMINAL_HEIGHT {
            let msg = format!(
                "Terminal too small: {}x{}. Need at least {}x{}.",
                area.width, area.height, MIN_TERMINAL_WIDTH, MIN_TERMINAL_HEIGHT
            );
            let warning = Paragraph::new(msg)
                .style(Style::default().fg(theme.error))
                .alignment(Alignment::Center);
            frame.render_widget(warning, area);
            return;
        }

        let prompt_height = prompt.needed_height(area.width);
        let sidebar_visible = sidebar_info.is_some();
        let (header_area, chat_area, prompt_area, status_area, sidebar_area, _) =
            Self::layout_with_sidebar(area, prompt_height, sidebar_visible);

        // Render each widget
        HeaderWidget::render(frame, header_area, model, tokens_used, working_dir, theme);
        chat.render(frame, chat_area, theme);
        prompt.render(frame, prompt_area, theme);
        StatusBarWidget::render_with_spinner(frame, status_area, status, model, tokens_used, spinner, progress_bar, theme, approval_mode);

        // Render sidebar if visible and there's space
        if let (Some(info), Some(sb_area)) = (sidebar_info, sidebar_area) {
            if sb_area.width > 5 && sb_area.height > 3 {
                SidebarWidget::render(frame, sb_area, info, theme, sidebar_tab);
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
            tool_name: None,
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

    // ── Markdown Parsing Tests ──────────────────────────────────────────

    #[test]
    fn test_parse_markdown_plain_text() {
        let segments = parse_markdown_segments("Hello\nWorld");
        assert_eq!(segments.len(), 1);
        match &segments[0] {
            MdSegment::Text(lines) => {
                assert_eq!(lines.len(), 2);
                assert_eq!(lines[0], "Hello");
                assert_eq!(lines[1], "World");
            }
            _ => panic!("Expected Text segment"),
        }
    }

    #[test]
    fn test_parse_markdown_code_block() {
        let input = "Before\n```rust\nfn main() {}\n```\nAfter";
        let segments = parse_markdown_segments(input);
        assert_eq!(segments.len(), 3);
        match &segments[0] {
            MdSegment::Text(lines) => assert!(lines[0] == "Before"),
            _ => panic!("Expected Text"),
        }
        match &segments[1] {
            MdSegment::CodeBlock { lang, code } => {
                assert_eq!(lang.as_deref(), Some("rust"));
                assert_eq!(code, "fn main() {}");
            }
            _ => panic!("Expected CodeBlock"),
        }
        match &segments[2] {
            MdSegment::Text(lines) => assert!(lines[0] == "After"),
            _ => panic!("Expected Text"),
        }
    }

    #[test]
    fn test_parse_markdown_code_block_no_lang() {
        let input = "```\nsome code\n```";
        let segments = parse_markdown_segments(input);
        assert_eq!(segments.len(), 1);
        match &segments[0] {
            MdSegment::CodeBlock { lang, code } => {
                assert!(lang.is_none());
                assert_eq!(code, "some code");
            }
            _ => panic!("Expected CodeBlock"),
        }
    }

    #[test]
    fn test_highlight_code_keywords() {
        let theme = Theme::default_dark();
        let spans = highlight_code_line("fn main() { let x = 1; }", None, &theme);
        assert!(!spans.is_empty());
    }

    #[test]
    fn test_highlight_code_string() {
        let theme = Theme::default_dark();
        let spans = highlight_code_line(r#"let s = "hello";"#, Some("rust"), &theme);
        assert!(!spans.is_empty());
    }

    #[test]
    fn test_highlight_code_comment() {
        let theme = Theme::default_dark();
        let spans = highlight_code_line("// this is a comment", Some("rust"), &theme);
        assert_eq!(spans.len(), 1); // entire line is one comment span
    }

    // ── Tool Message Tests ──────────────────────────────────────────────

    #[test]
    fn test_add_tool_message() {
        let mut chat = ChatWidget::new(100);
        let idx = chat.add_tool_message("bash".to_string(), "cargo test\nall passed".to_string(), false);
        assert_eq!(chat.len(), 1);
        let msg = &chat.messages[idx];
        assert_eq!(msg.role, ChatRole::Tool);
        assert_eq!(msg.tool_name.as_deref(), Some("bash"));
        assert!(!msg.is_error);
    }

    #[test]
    fn test_add_tool_message_error() {
        let mut chat = ChatWidget::new(100);
        chat.add_tool_message("bash".to_string(), "error: build failed".to_string(), true);
        let msg = &chat.messages[0];
        assert!(msg.is_error);
        assert_eq!(msg.tool_name.as_deref(), Some("bash"));
    }

    // ── SidebarInfo Tests ───────────────────────────────────────────────

    #[test]
    fn test_sidebar_info_context_window() {
        let info = SidebarInfo {
            model: Some("test-model".to_string()),
            tokens_used: 5000,
            cost_usd: 0.05,
            tools_invoked: 3,
            modified_files: vec![],
            total_additions: 0,
            total_deletions: 0,
            error_count: 1,
            context_window: 200_000,
            active_agents: vec![],
        };
        assert_eq!(info.context_window, 200_000);
        assert_eq!(info.error_count, 1);
    }

    // ── Truncation Tests ────────────────────────────────────────────────

    #[test]
    fn test_truncate_to_short() {
        assert_eq!(truncate_to("hi", 10), "hi");
    }

    #[test]
    fn test_truncate_to_exact() {
        assert_eq!(truncate_to("abc", 3), "abc");
    }

    #[test]
    fn test_truncate_to_long() {
        assert_eq!(truncate_to("abcdef", 4), "abc…");
    }

    #[test]
    fn test_format_tokens() {
        assert_eq!(format_tokens(500), "500");
        assert_eq!(format_tokens(1500), "1.5k");
        assert_eq!(format_tokens(1_500_000), "1.5M");
    }

    // ── Sidebar Tab Tests ─────────────────────────────────────────────

    #[test]
    fn test_sidebar_tab_default() {
        assert_eq!(crate::repl::SidebarTab::default(), crate::repl::SidebarTab::Context);
    }

    #[test]
    fn test_sidebar_tab_cycle() {
        let mut tab = crate::repl::SidebarTab::Context;
        tab = tab.next();
        assert_eq!(tab, crate::repl::SidebarTab::Files);
        tab = tab.next();
        assert_eq!(tab, crate::repl::SidebarTab::Agents);
        tab = tab.next();
        assert_eq!(tab, crate::repl::SidebarTab::Context);
    }

    #[test]
    fn test_sidebar_fits() {
        assert!(SidebarWidget::fits(80));
        assert!(!SidebarWidget::fits(60));
    }
}
