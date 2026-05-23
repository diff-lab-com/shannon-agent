//! Input prompt widget (multi-line enabled)

use crate::theme::Theme;
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use rust_i18n::t;
use unicode_width::UnicodeWidthStr;

fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

fn char_display_width(c: char) -> usize {
    unicode_width::UnicodeWidthChar::width(c).unwrap_or(0)
}

/// Maximum input size (1 MB) to prevent OOM from large pastes.
const MAX_INPUT_SIZE: usize = 1024 * 1024;

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
            placeholder: t!("ui.placeholder").to_string(),
            vim_mode: t!("ui.vim_insert").to_string(),
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

    /// Add a character with auto-pairing for brackets and quotes.
    pub fn add_char_smart(&mut self, c: char) {
        if self.buffer.text().len() >= MAX_INPUT_SIZE {
            return;
        }
        match c {
            '(' => {
                self.buffer.insert_char('(');
                self.buffer.insert_char(')');
                self.buffer.move_left();
            }
            '[' => {
                self.buffer.insert_char('[');
                self.buffer.insert_char(']');
                self.buffer.move_left();
            }
            '{' => {
                self.buffer.insert_char('{');
                self.buffer.insert_char('}');
                self.buffer.move_left();
            }
            ')' | ']' | '}' => {
                let line = self.buffer.current_line();
                let col = self.buffer.cursor_col();
                let chars: Vec<char> = line.chars().collect();
                if col < chars.len() && chars[col] == c {
                    self.buffer.move_right();
                } else {
                    self.buffer.insert_char(c);
                }
            }
            _ => {
                self.buffer.insert_char(c);
            }
        }
    }

    /// Get the current input text
    pub fn input(&self) -> String {
        self.buffer.text()
    }

    /// Add a character to the input
    pub fn add_char(&mut self, c: char) {
        if self.buffer.text().len() < MAX_INPUT_SIZE {
            self.buffer.insert_char(c);
        }
    }

    /// Remove the character before the cursor
    pub fn backspace(&mut self) {
        self.buffer.backspace();
    }

    /// Remove the character at the cursor (forward delete)
    pub fn delete_forward(&mut self) {
        self.buffer.delete();
    }

    /// Kill from cursor to end of line. Returns killed text.
    pub fn kill_line(&mut self) -> String {
        self.buffer.kill_line()
    }

    /// Kill from cursor to start of line. Returns killed text.
    pub fn kill_to_start(&mut self) -> String {
        self.buffer.kill_to_start()
    }

    /// Kill the word before the cursor. Returns killed text.
    pub fn kill_word_back(&mut self) -> String {
        self.buffer.kill_word_back()
    }

    /// Find the column range of a text object on the current line.
    pub fn find_text_object_range(&self, inner: bool, target: char) -> Option<(usize, usize)> {
        self.buffer.find_text_object_range(inner, target)
    }

    /// Delete characters in a column range on the current line.
    pub fn delete_col_range(&mut self, start: usize, end: usize) -> String {
        self.buffer.delete_col_range(start, end)
    }

    /// Get text at column range on current line (no deletion).
    pub fn text_at_cols(&self, start: usize, end: usize) -> String {
        self.buffer.text_at_cols(start, end)
    }

    /// Delete the current line and return its content.
    pub fn delete_current_line(&mut self) -> String {
        self.buffer.delete_current_line()
    }

    /// Clear the input
    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    /// Set the input text, truncating if it exceeds the size limit.
    pub fn set_input(&mut self, input: String) {
        if input.len() <= MAX_INPUT_SIZE {
            self.buffer.set_text(&input);
        } else {
            let mut end = MAX_INPUT_SIZE;
            while !input.is_char_boundary(end) && end > 0 {
                end -= 1;
            }
            self.buffer.set_text(&input[..end]);
        }
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

    /// Insert text at the cursor position, respecting the size limit.
    pub fn insert_text(&mut self, text: &str) {
        let remaining = MAX_INPUT_SIZE.saturating_sub(self.buffer.text().len());
        if remaining == 0 {
            return;
        }
        if text.len() <= remaining {
            self.buffer.insert_text(text);
        } else {
            let mut end = remaining;
            while !text.is_char_boundary(end) && end > 0 {
                end -= 1;
            }
            self.buffer.insert_text(&text[..end]);
        }
    }

    /// Get current cursor position (column)
    pub fn cursor_position(&self) -> usize {
        self.buffer.cursor_col()
    }

    /// Get current cursor row (0-based)
    pub fn cursor_row(&self) -> usize {
        self.buffer.cursor_row()
    }

    /// Get the number of lines in the input buffer
    pub fn line_count(&self) -> usize {
        self.buffer.line_count()
    }

    /// Get the length of the current line in characters.
    pub fn current_line_len(&self) -> usize {
        self.buffer.current_line().chars().count()
    }

    /// Move cursor to the start of the next word (or end of line).
    /// Vim `w` semantics: skip punctuation, land on next word-start.
    pub fn cursor_word_forward(&mut self) {
        let line = self.buffer.current_line();
        let col = self.buffer.cursor_col();
        let chars: Vec<char> = line.chars().collect();
        let len = chars.len();
        if col >= len {
            return;
        }
        let mut i = col;
        // Skip current word (alphanumeric/underscore)
        let at_word = i < len && (chars[i].is_alphanumeric() || chars[i] == '_');
        if at_word {
            while i < len && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
        } else if i < len && !chars[i].is_whitespace() {
            // Skip punctuation
            while i < len
                && !chars[i].is_alphanumeric()
                && chars[i] != '_'
                && !chars[i].is_whitespace()
            {
                i += 1;
            }
        }
        // Skip whitespace
        while i < len && chars[i].is_whitespace() {
            i += 1;
        }
        let steps = i.saturating_sub(col);
        for _ in 0..steps {
            self.buffer.move_right();
        }
    }

    /// Move cursor to the start of the previous word (or start of line).
    /// Vim `b` semantics: move back to previous word-start.
    pub fn cursor_word_back(&mut self) {
        let line = self.buffer.current_line();
        let col = self.buffer.cursor_col();
        if col == 0 {
            return;
        }
        let chars: Vec<char> = line.chars().collect();
        let mut i = col;
        // Skip whitespace backward
        while i > 0 && chars[i - 1].is_whitespace() {
            i -= 1;
        }
        if i == 0 {
            let steps = col - i;
            for _ in 0..steps {
                self.buffer.move_left();
            }
            return;
        }
        // Move back over word/punctuation
        let at_word = chars[i - 1].is_alphanumeric() || chars[i - 1] == '_';
        if at_word {
            while i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
                i -= 1;
            }
        } else {
            while i > 0
                && !chars[i - 1].is_alphanumeric()
                && chars[i - 1] != '_'
                && !chars[i - 1].is_whitespace()
            {
                i -= 1;
            }
        }
        let steps = col - i;
        for _ in 0..steps {
            self.buffer.move_left();
        }
    }

    /// Compute how many terminal rows the prompt needs, given the available width.
    /// Returns a value clamped to [MIN_PROMPT_HEIGHT, MAX_PROMPT_HEIGHT].
    pub fn needed_height(&self, available_width: u16) -> u16 {
        const MAX_PROMPT_HEIGHT: u16 = 10;
        const MIN_PROMPT_HEIGHT: u16 = 3;

        let inner_width = available_width.saturating_sub(2) as usize; // 2 prefix chars
        if inner_width == 0 {
            return MIN_PROMPT_HEIGHT;
        }
        let input = self.input();
        if input.is_empty() {
            return MIN_PROMPT_HEIGHT;
        }

        let rows: usize = input
            .split('\n')
            .map(|line| {
                let w = display_width(line);
                if w == 0 { 1 } else { w.div_ceil(inner_width) }
            })
            .sum();

        let needed = (rows + 2) as u16; // +2 for top border + bottom hint
        needed.clamp(MIN_PROMPT_HEIGHT, MAX_PROMPT_HEIGHT)
    }

    /// Wrap a single logical line into chunks that fit within `max_width` display columns.
    fn wrap_line(line: &str, max_width: usize) -> Vec<String> {
        if max_width == 0 || line.is_empty() {
            return vec![line.to_string()];
        }
        let mut result = Vec::new();
        let mut current = String::new();
        let mut current_width = 0;
        for c in line.chars() {
            let w = char_display_width(c);
            if current_width + w > max_width && !current.is_empty() {
                result.push(std::mem::take(&mut current));
                current_width = 0;
            }
            current.push(c);
            current_width += w;
        }
        if !current.is_empty() || result.is_empty() {
            result.push(current);
        }
        result
    }

    /// Compute the (display_row, display_col) of the cursor, accounting for wrapping
    /// and Unicode display widths (CJK chars = 2 columns).
    fn cursor_display_pos(&self, inner_width: usize) -> (usize, usize) {
        let cursor_row = self.buffer.cursor_row();
        let cursor_col = self.buffer.cursor_col();
        let input = self.input();
        let lines: Vec<&str> = input.split('\n').collect();

        let mut display_row: usize = 0;
        for (row_idx, line) in lines.iter().enumerate() {
            let line_width = display_width(line);
            let wrapped_count = if line.is_empty() {
                1
            } else if inner_width > 0 {
                line_width.div_ceil(inner_width)
            } else {
                1
            };

            if row_idx == cursor_row {
                // cursor_col is a character index; convert to display column
                let cursor_display_col: usize =
                    line.chars().take(cursor_col).map(char_display_width).sum();
                let wrap_row = if inner_width > 0 {
                    cursor_display_col / inner_width
                } else {
                    0
                };
                let wrap_col = if inner_width > 0 {
                    cursor_display_col % inner_width
                } else {
                    cursor_display_col
                };
                return (display_row + wrap_row, wrap_col);
            }
            display_row += wrapped_count;
        }
        (display_row, cursor_col)
    }

    /// Render the prompt widget
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme, mode: Option<&str>) {
        let input_text = self.input();
        let inner_width = area.width.saturating_sub(2) as usize; // 2 prefix chars (no side borders)

        let mut display_lines: Vec<Line<'static>> = Vec::new();

        if input_text.is_empty() {
            display_lines.push(Line::from(vec![
                Span::styled(
                    "> ",
                    Style::default()
                        .fg(theme.primary)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(self.placeholder.clone(), Style::default().fg(theme.muted)),
            ]));
        } else {
            let logical_lines: Vec<&str> = input_text.split('\n').collect();
            for (line_idx, logical_line) in logical_lines.iter().enumerate() {
                let wrapped = Self::wrap_line(logical_line, inner_width);
                for (wrap_idx, chunk) in wrapped.iter().enumerate() {
                    let prefix = if line_idx == 0 && wrap_idx == 0 {
                        Span::styled(
                            "> ",
                            Style::default()
                                .fg(theme.primary)
                                .add_modifier(Modifier::BOLD),
                        )
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

        // Explicit /color override takes precedence; otherwise derive from approval mode
        let border_color = self.border_color_override.unwrap_or({
            match mode {
                Some("ASK") | Some("PLAN") => theme.accent,
                Some("EDIT") => theme.success,
                Some("AUTO") => theme.primary,
                Some("FULL") => theme.error,
                _ => theme.border_dim,
            }
        });
        // Bottom border — plain horizontal line (keyboard hints are shown by
        // the centralized KeyHintWidget at the screen bottom).
        let w = area.width as usize;
        let bottom_line = ratatui::text::Line::from(vec![Span::styled(
            "─".repeat(w),
            Style::default().fg(border_color),
        )]);

        let paragraph = Paragraph::new(display_lines)
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(border_color))
                    .title(title)
                    .title_bottom(bottom_line),
            )
            .alignment(Alignment::Left);

        frame.render_widget(paragraph, area);

        // Show cursor (always, even on empty input so user sees where to type)
        if inner_width > 0 {
            if input_text.is_empty() {
                // Place cursor right after the "> " prompt
                let cursor_x = area.x + 2;
                let cursor_y = area.y + 1;
                if cursor_y < area.bottom() - 1 && cursor_x < area.right() {
                    frame.set_cursor_position((cursor_x, cursor_y));
                }
            } else {
                let (disp_row, disp_col) = self.cursor_display_pos(inner_width);
                let cursor_x = area.x + 2 + disp_col as u16;
                let cursor_y = area.y + 1 + disp_row as u16;
                if cursor_y < area.bottom() - 1 && cursor_x < area.right() {
                    frame.set_cursor_position((cursor_x, cursor_y));
                }
            }
        }
    }
}

impl Default for PromptWidget {
    fn default() -> Self {
        Self::new()
    }
}
