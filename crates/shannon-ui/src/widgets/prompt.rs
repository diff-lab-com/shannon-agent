//! Input prompt widget (multi-line enabled)

use crate::theme::Theme;
use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Input prompt widget (multi-line enabled)
pub struct PromptWidget {
    /// Inner input buffer with full multi-line support
    pub(super) buffer: crate::repl_enhancement::InputBuffer,
    /// Placeholder text
    pub(super) placeholder: String,
    /// Vim mode label for display ("INSERT" or "NORMAL")
    vim_mode: String,
    /// Optional border color override (set via /color)
    border_color_override: Option<ratatui::style::Color>,
}

impl PromptWidget {
    /// Create a new prompt widget
    pub fn new() -> Self {
        Self {
            buffer: crate::repl_enhancement::InputBuffer::new(),
            placeholder: "Type your message...".to_string(),
            vim_mode: "INSERT".to_string(),
            border_color_override: None,
        }
    }

    /// Set border color override (from /color command)
    pub fn set_border_color(&mut self, color: Option<ratatui::style::Color>) {
        self.border_color_override = color;
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
                    .border_style(Style::default().fg(self.border_color_override.unwrap_or(theme.border)))
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
