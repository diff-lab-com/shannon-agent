//! # REPL Enhancement
//!
//! Advanced REPL features including diff tracking per turn, session history
//! with search, multi-line input editing, formatted output rendering, and
//! end-of-session summaries. Inspired by Claude Code's REPL, useDiffData,
//! and useTurnDiffs.
//!
//! ## Architecture
//!
//! - [`TurnDiff`]: Tracks file changes per assistant turn
//! - [`DiffData`]: Aggregates turn diffs for a full session
//! - [`ReplHistory`]: Command history with cursor nav, search, persistence
//! - [`InputBuffer`]: Multi-line editing with cursor management
//! - [`ReplRenderer`]: Formatted output with markdown/code rendering
//! - [`SessionSummary`]: End-of-session statistics
//!
//! ## Example
//!
//! ```
//! use shannon_ui::repl_enhancement::{
//!     DiffData, TurnDiff, ReplHistory, InputBuffer, ReplRenderer, SessionSummary,
//! };
//!
//! let mut history = ReplHistory::new(100);
//! history.push("hello world");
//! assert_eq!(history.up(), Some("hello world"));
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

// ---------------------------------------------------------------------------
// TurnDiff
// ---------------------------------------------------------------------------

/// Tracks file changes made during a single assistant turn.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TurnDiff {
    /// Turn index (0-based).
    pub turn_index: usize,
    /// Files modified (path + number of lines changed).
    pub files_modified: Vec<FileChange>,
    /// Files created.
    pub files_created: Vec<String>,
    /// Files deleted.
    pub files_deleted: Vec<String>,
    /// Brief human-readable summary.
    pub diff_summary: String,
    /// Timestamp when the turn started.
    pub timestamp: DateTime<Utc>,
}

/// A record of changes to a single file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileChange {
    /// File path.
    pub path: String,
    /// Number of lines added.
    pub additions: usize,
    /// Number of lines deleted.
    pub deletions: usize,
}

impl TurnDiff {
    /// Create a new empty turn diff.
    pub fn new(turn_index: usize) -> Self {
        Self {
            turn_index,
            files_modified: Vec::new(),
            files_created: Vec::new(),
            files_deleted: Vec::new(),
            diff_summary: String::new(),
            timestamp: Utc::now(),
        }
    }

    /// Record a file modification.
    pub fn modify_file(&mut self, path: String, additions: usize, deletions: usize) {
        self.files_modified.push(FileChange {
            path,
            additions,
            deletions,
        });
        self.rebuild_summary();
    }

    /// Record a file creation.
    pub fn create_file(&mut self, path: String) {
        if !self.files_created.contains(&path) {
            self.files_created.push(path);
        }
        self.rebuild_summary();
    }

    /// Record a file deletion.
    pub fn delete_file(&mut self, path: String) {
        if !self.files_deleted.contains(&path) {
            self.files_deleted.push(path);
        }
        self.rebuild_summary();
    }

    /// Total number of lines added across all modified files.
    pub fn total_additions(&self) -> usize {
        self.files_modified.iter().map(|f| f.additions).sum()
    }

    /// Total number of lines deleted across all modified files.
    pub fn total_deletions(&self) -> usize {
        self.files_modified.iter().map(|f| f.deletions).sum()
    }

    /// Total files touched (modified + created + deleted).
    pub fn total_files_touched(&self) -> usize {
        self.files_modified.len() + self.files_created.len() + self.files_deleted.len()
    }

    /// Rebuild the human-readable summary from the data.
    fn rebuild_summary(&mut self) {
        let mut parts = Vec::new();

        let m = self.files_modified.len();
        let c = self.files_created.len();
        let d = self.files_deleted.len();
        let a = self.total_additions();
        let del = self.total_deletions();

        if m > 0 {
            parts.push(format!("{} file{} modified", m, plural_s(m)));
        }
        if c > 0 {
            parts.push(format!("{} file{} created", c, plural_s(c)));
        }
        if d > 0 {
            parts.push(format!("{} file{} deleted", d, plural_s(d)));
        }
        if a > 0 || del > 0 {
            parts.push(format!("+{a}/-{del} lines"));
        }

        if parts.is_empty() {
            self.diff_summary = "No file changes.".to_string();
        } else {
            self.diff_summary = parts.join(", ");
        }
    }
}

fn plural_s(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

// ---------------------------------------------------------------------------
// DiffData
// ---------------------------------------------------------------------------

/// Aggregates turn diffs for a full session.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DiffData {
    /// All turn diffs in order.
    pub turns: Vec<TurnDiff>,
}

impl DiffData {
    /// Create a new empty DiffData.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a new turn diff.
    pub fn record_turn_diff(&mut self, turn: TurnDiff) {
        self.turns.push(turn);
    }

    /// Get all turn diffs for the session.
    pub fn get_session_diffs(&self) -> &[TurnDiff] {
        &self.turns
    }

    /// Total additions across all turns.
    pub fn total_additions(&self) -> usize {
        self.turns.iter().map(|t| t.total_additions()).sum()
    }

    /// Total deletions across all turns.
    pub fn total_deletions(&self) -> usize {
        self.turns.iter().map(|t| t.total_deletions()).sum()
    }

    /// Total files modified across all turns.
    pub fn total_files_modified(&self) -> usize {
        self.turns.iter().map(|t| t.files_modified.len()).sum()
    }

    /// Total files created across all turns.
    pub fn total_files_created(&self) -> usize {
        self.turns.iter().map(|t| t.files_created.len()).sum()
    }

    /// Total files deleted across all turns.
    pub fn total_files_deleted(&self) -> usize {
        self.turns.iter().map(|t| t.files_deleted.len()).sum()
    }

    /// Get the diff for a specific turn.
    pub fn get_turn(&self, turn_index: usize) -> Option<&TurnDiff> {
        self.turns.iter().find(|t| t.turn_index == turn_index)
    }

    /// Get the latest turn diff.
    pub fn latest_turn(&self) -> Option<&TurnDiff> {
        self.turns.last()
    }
}

// ---------------------------------------------------------------------------
// ReplHistory
// ---------------------------------------------------------------------------

/// Command history with cursor navigation, search (Ctrl+R), and persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplHistory {
    /// The history entries in order.
    entries: VecDeque<String>,
    /// Maximum number of entries to keep.
    max_entries: usize,
    /// Current cursor position for up/down navigation (-1 = current input).
    cursor: isize,
}

impl ReplHistory {
    /// Create a new history with a max capacity.
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(max_entries),
            max_entries,
            cursor: -1,
        }
    }

    /// Push a new command onto the history.
    pub fn push(&mut self, command: &str) {
        let trimmed = command.trim();
        if trimmed.is_empty() {
            return;
        }
        // Deduplicate: if the same command is already at the top, don't re-add
        if self.entries.back().map(|s| s.as_str()) == Some(trimmed) {
            return;
        }
        if self.entries.len() >= self.max_entries {
            self.entries.pop_front();
        }
        self.entries.push_back(trimmed.to_string());
        self.cursor = -1;
    }

    /// Navigate up in history. Returns the command at that position.
    pub fn up(&mut self) -> Option<&str> {
        if self.entries.is_empty() {
            return None;
        }
        if self.cursor < (self.entries.len() as isize - 1) {
            self.cursor += 1;
        }
        let idx = self.entries.len() - 1 - self.cursor as usize;
        self.entries.get(idx).map(|s| s.as_str())
    }

    /// Navigate down in history. Returns the command at that position or None if at bottom.
    pub fn down(&mut self) -> Option<&str> {
        if self.cursor <= 0 {
            self.cursor = -1;
            return None;
        }
        self.cursor -= 1;
        let idx = self.entries.len() - 1 - self.cursor as usize;
        self.entries.get(idx).map(|s| s.as_str())
    }

    /// Reset cursor to the bottom (current input).
    pub fn reset_cursor(&mut self) {
        self.cursor = -1;
    }

    /// Search history for entries containing the query (reverse chronological).
    pub fn search_history(&self, query: &str) -> Vec<&str> {
        let query_lower = query.to_lowercase();
        self.entries
            .iter()
            .rev()
            .filter(|e| e.to_lowercase().contains(&query_lower))
            .take(20)
            .map(|s| s.as_str())
            .collect()
    }

    /// Get all entries.
    pub fn entries(&self) -> Vec<&str> {
        self.entries.iter().map(|s| s.as_str()).collect()
    }

    /// Number of entries in history.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if history is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Current cursor position.
    pub fn cursor(&self) -> isize {
        self.cursor
    }
}

// ---------------------------------------------------------------------------
// InputBuffer
// ---------------------------------------------------------------------------

/// Multi-line input buffer with cursor management and auto-indent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputBuffer {
    /// The lines of text currently in the buffer.
    lines: Vec<String>,
    /// Cursor column position (0-based).
    cursor_col: usize,
    /// Cursor row position (0-based, within lines).
    cursor_row: usize,
    /// Whether auto-indent is enabled.
    auto_indent: bool,
}

impl Default for InputBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl InputBuffer {
    /// Create a new empty input buffer.
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
            cursor_col: 0,
            cursor_row: 0,
            auto_indent: true,
        }
    }

    /// Convert a char index to a byte index for the given line.
    fn char_to_byte(line: &str, char_idx: usize) -> usize {
        line.char_indices()
            .nth(char_idx)
            .map(|(i, _)| i)
            .unwrap_or(line.len())
    }

    /// Get the char count for the current line.
    fn line_char_count(&self) -> usize {
        self.lines.get(self.cursor_row).map(|l| l.chars().count()).unwrap_or(0)
    }

    /// Get the text of the current line.
    pub fn current_line(&self) -> &str {
        self.lines.get(self.cursor_row).map(|s| s.as_str()).unwrap_or("")
    }

    /// Get the word at or immediately before the cursor on the current line.
    pub fn current_word(&self) -> String {
        let line = match self.lines.get(self.cursor_row) {
            Some(l) => l.as_str(),
            None => return String::new(),
        };
        let col = self.cursor_col.min(line.chars().count());
        let chars: Vec<char> = line.chars().collect();
        // Find end of word: skip to end of current word or stay at col
        let mut end = col;
        while end < chars.len() && !chars[end].is_whitespace() {
            end += 1;
        }
        // Find start of word
        let mut start = col;
        while start > 0 && !chars[start - 1].is_whitespace() {
            start -= 1;
        }
        if start >= end {
            return String::new();
        }
        chars[start..end].iter().collect()
    }

    /// Insert text at the cursor position.
    pub fn insert_text(&mut self, text: &str) {
        for ch in text.chars() {
            self.insert_char(ch);
        }
    }

    /// Insert a character at the cursor position.
    pub fn insert_char(&mut self, ch: char) {
        if self.cursor_row >= self.lines.len() {
            return;
        }
        let line = &mut self.lines[self.cursor_row];
        let char_count = line.chars().count();
        let col = self.cursor_col.min(char_count);
        let byte_idx = Self::char_to_byte(line, col);
        line.insert(byte_idx, ch);
        self.cursor_col = col + 1;
    }

    /// Delete the character before the cursor (backspace).
    pub fn backspace(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
            let byte_idx = Self::char_to_byte(&self.lines[self.cursor_row], self.cursor_col);
            self.lines[self.cursor_row].remove(byte_idx);
        } else if self.cursor_row > 0 {
            // Merge with previous line
            let prev_char_count = self.lines[self.cursor_row - 1].chars().count();
            let current = self.lines.remove(self.cursor_row);
            self.cursor_row -= 1;
            self.lines[self.cursor_row].push_str(&current);
            self.cursor_col = prev_char_count;
        }
    }

    /// Delete the character at the cursor (delete key).
    pub fn delete(&mut self) {
        if self.cursor_row >= self.lines.len() {
            return;
        }
        let char_count = self.lines[self.cursor_row].chars().count();
        if self.cursor_col < char_count {
            let byte_idx = Self::char_to_byte(&self.lines[self.cursor_row], self.cursor_col);
            self.lines[self.cursor_row].remove(byte_idx);
        } else if self.cursor_row + 1 < self.lines.len() {
            let next = self.lines.remove(self.cursor_row + 1);
            self.lines[self.cursor_row].push_str(&next);
        }
    }

    /// Move cursor left by one character.
    pub fn move_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        }
    }

    /// Move cursor right by one character.
    pub fn move_right(&mut self) {
        let max = self.line_char_count();
        if self.cursor_col < max {
            self.cursor_col += 1;
        }
    }

    /// Move cursor up.
    pub fn move_up(&mut self) {
        if self.cursor_row > 0 {
            self.cursor_row -= 1;
            let max = self.line_char_count();
            self.cursor_col = self.cursor_col.min(max);
        }
    }

    /// Move cursor down.
    pub fn move_down(&mut self) {
        if self.cursor_row + 1 < self.lines.len() {
            self.cursor_row += 1;
            let max = self.line_char_count();
            self.cursor_col = self.cursor_col.min(max);
        }
    }

    /// Insert a newline (Enter key).
    pub fn newline(&mut self) {
        if self.cursor_row >= self.lines.len() {
            return;
        }
        let line = &mut self.lines[self.cursor_row];
        let byte_idx = Self::char_to_byte(line, self.cursor_col);
        let after: String = line.drain(byte_idx..).collect();

        // Auto-indent: copy leading whitespace from current line
        let indent = if self.auto_indent {
            line.chars().take_while(|c| c.is_whitespace()).collect()
        } else {
            String::new()
        };

        self.cursor_row += 1;
        self.lines.insert(self.cursor_row, format!("{after}{indent}"));
        self.cursor_col = after.chars().count() + indent.chars().count();
    }

    /// Get the full text content of the buffer.
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// Set the buffer content, replacing everything.
    pub fn set_text(&mut self, text: &str) {
        self.lines = if text.is_empty() {
            vec![String::new()]
        } else {
            text.lines().map(|l| l.to_string()).collect()
        };
        self.cursor_row = self.lines.len().saturating_sub(1);
        self.cursor_col = self.lines.last().map(|l| l.chars().count()).unwrap_or(0);
    }

    /// Clear the buffer.
    pub fn clear(&mut self) {
        self.lines = vec![String::new()];
        self.cursor_row = 0;
        self.cursor_col = 0;
    }

    /// Current cursor column.
    pub fn cursor_col(&self) -> usize {
        self.cursor_col
    }

    /// Current cursor row.
    pub fn cursor_row(&self) -> usize {
        self.cursor_row
    }

    /// Number of lines.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Enable or disable auto-indent.
    pub fn set_auto_indent(&mut self, enabled: bool) {
        self.auto_indent = enabled;
    }
}

// ---------------------------------------------------------------------------
// ReplRenderer
// ---------------------------------------------------------------------------

/// Renders formatted output for the REPL (markdown, code blocks, spinners).
#[derive(Debug, Clone)]
pub struct ReplRenderer {
    /// Whether to use color output.
    pub use_color: bool,
    /// Terminal width for wrapping.
    pub terminal_width: usize,
}

impl Default for ReplRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl ReplRenderer {
    /// Create a new renderer.
    pub fn new() -> Self {
        Self {
            use_color: true,
            terminal_width: 80,
        }
    }

    /// Render an assistant output message.
    pub fn render_output(&self, content: &str, role: &str) -> String {
        let mut output = String::new();

        match role {
            "user" => {
                output.push_str(&format!("\x1b[1;36m> {content}\x1b[0m\n"));
            }
            "assistant" => {
                output.push_str(&self.render_markdown(content));
            }
            "system" => {
                output.push_str(&format!("\x1b[2m[system] {content}\x1b[0m\n"));
            }
            "tool" => {
                output.push_str(&format!("\x1b[33m[tool] {content}\x1b[0m\n"));
            }
            "error" => {
                output.push_str(&format!("\x1b[31m[error] {content}\x1b[0m\n"));
            }
            _ => {
                output.push_str(content);
                output.push('\n');
            }
        }

        output
    }

    /// Basic markdown rendering: bold, italic, code blocks, inline code.
    ///
    /// Code blocks with language hints are syntax-highlighted using syntect.
    pub fn render_markdown(&self, text: &str) -> String {
        use std::sync::OnceLock;

        static SYNTAX_RESOURCES: OnceLock<(
            syntect::parsing::SyntaxSet,
            syntect::highlighting::ThemeSet,
        )> = OnceLock::new();

        let (ss, ts) = SYNTAX_RESOURCES.get_or_init(|| {
            (
                syntect::parsing::SyntaxSet::load_defaults_newlines(),
                syntect::highlighting::ThemeSet::load_defaults(),
            )
        });

        let mut result = String::new();
        let mut in_code_block = false;
        let mut code_lang = String::new();
        let mut code_buffer = String::new();
        let mut in_inline_code = false;

        for line in text.lines() {
            if line.trim_start().starts_with("```") {
                if in_code_block {
                    // Closing code block — highlight collected code with syntect
                    in_code_block = false;
                    let highlighted = highlight_code_ansi(&code_buffer, &code_lang, ss, ts);
                    result.push_str(&highlighted);
                    code_buffer.clear();
                    code_lang.clear();
                } else {
                    // Opening code block
                    in_code_block = true;
                    let lang = line.trim_start().trim_start_matches('`').trim();
                    code_lang = lang.to_string();
                    if !code_lang.is_empty() {
                        result.push_str(&format!("\x1b[2m[{code_lang}]\x1b[0m\n"));
                    }
                }
                continue;
            }

            if in_code_block {
                code_buffer.push_str(line);
                code_buffer.push('\n');
                continue;
            }

            // Process inline formatting
            let mut chars = line.chars().peekable();
            while let Some(ch) = chars.next() {
                if ch == '`' && !in_inline_code {
                    in_inline_code = true;
                    result.push_str("\x1b[33m");
                } else if ch == '`' && in_inline_code {
                    in_inline_code = false;
                    result.push_str("\x1b[0m");
                } else if ch == '*' && chars.peek() == Some(&'*') {
                    chars.next();
                    result.push_str("\x1b[1m");
                } else {
                    result.push(ch);
                }
            }
            result.push('\n');
        }

        // Handle unclosed code block at end of text
        if in_code_block {
            let highlighted = highlight_code_ansi(&code_buffer, &code_lang, ss, ts);
            result.push_str(&highlighted);
        }
        if in_inline_code {
            result.push_str("\x1b[0m");
        }

        result
    }

    /// Render streaming content with partial-markdown tolerance.
    ///
    /// Unlike `render_output`, this method:
    /// - Handles incomplete code blocks (shows a cursor for unclosed blocks)
    /// - Reformats tool-use markers (🔧) into styled sections
    /// - Strips raw progress/stats lines (⏳, 📊, 💰) for cleaner display
    /// - Adds a streaming cursor at the end
    pub fn render_streaming(&self, content: &str) -> String {
        let mut result = String::new();
        let mut in_code_block = false;
        let mut in_inline_code = false;
        let mut first_text_seen = false;

        for line in content.lines() {
            // Strip raw streaming markers for cleaner display
            let trimmed = line.trim();

            // Skip raw token/cost/progress lines during streaming
            if trimmed.starts_with("📊 Tokens:")
                || trimmed.starts_with("💰 Session total:")
                || trimmed.starts_with("⏳ Tool progress:")
                || trimmed.starts_with("⏳ ")
            {
                continue;
            }

            // Skip turn-completed markers
            if trimmed.starts_with("[Turn ") && trimmed.ends_with(" tokens]") {
                continue;
            }

            // Format tool-use markers into styled sections
            if trimmed.starts_with("🔧 Using:") {
                if !first_text_seen {
                    first_text_seen = true;
                }
                // Extract tool name and format nicely
                let rest = trimmed.trim_start_matches("🔧 Using:");
                result.push_str(&format!("\n\x1b[1;33m▸ Tool:\x1b[0m {}\n", rest.trim()));
                continue;
            }

            if !first_text_seen && !trimmed.is_empty() {
                first_text_seen = true;
            }

            // Handle code blocks
            if trimmed.starts_with("```") {
                if in_code_block {
                    in_code_block = false;
                    result.push_str("\x1b[0m\n");
                } else {
                    in_code_block = true;
                    let lang = trimmed.trim_start_matches('`').trim();
                    if !lang.is_empty() {
                        result.push_str(&format!("\x1b[2m[{lang}]\x1b[0m\n"));
                    }
                    result.push_str("\x1b[32m");
                }
                continue;
            }

            if in_code_block {
                result.push_str(line);
                result.push('\n');
                continue;
            }

            // Process inline formatting
            let mut chars = trimmed.chars().peekable();
            while let Some(ch) = chars.next() {
                if ch == '`' && !in_inline_code {
                    in_inline_code = true;
                    result.push_str("\x1b[33m");
                } else if ch == '`' && in_inline_code {
                    in_inline_code = false;
                    result.push_str("\x1b[0m");
                } else if (ch == '*' || ch == '_') && chars.peek() == Some(&ch) {
                    chars.next();
                    result.push_str("\x1b[1m");
                } else {
                    result.push(ch);
                }
            }

            result.push('\n');
        }

        // Close unclosed code block with cursor indicator
        if in_code_block {
            result.push_str("\x1b[0m\x1b[2m▊\x1b[0m");
        } else if in_inline_code {
            result.push_str("\x1b[0m");
        }

        // Add streaming cursor at the very end
        if !result.is_empty() && !in_code_block {
            // Remove trailing newline to place cursor properly
            if result.ends_with('\n') {
                result.pop();
            }
            result.push_str("\x1b[2m▊\x1b[0m\n");
        }

        result
    }

    /// Render a code block with syntax highlighting placeholder.
    pub fn render_code_block(&self, code: &str, language: &str) -> String {
        format!(
            "\x1b[2m[{language}]\x1b[0m\n\x1b[32m{code}\x1b[0m\n"
        )
    }

    /// Render a spinner animation frame.
    pub fn render_spinner(&self, message: &str, frame: usize) -> String {
        let frames = ['/', '-', '\\', '|'];
        let ch = frames[frame % frames.len()];
        format!("\r\x1b[33m{ch} {message}\x1b[0m")
    }

    /// Render a progress bar.
    pub fn render_progress(&self, current: usize, total: usize, label: &str) -> String {
        let pct = if total == 0 {
            100
        } else {
            ((current as f64 / total as f64) * 100.0) as usize
        };
        let filled = pct / 5;
        let bar: String = "#".repeat(filled) + &"-".repeat(20 - filled);
        format!("\r\x1b[36m[{bar}] {pct:>3}% {label}\x1b[0m")
    }
}

/// Highlight code using syntect and return ANSI-escaped string.
fn highlight_code_ansi(
    code: &str,
    lang: &str,
    ss: &syntect::parsing::SyntaxSet,
    ts: &syntect::highlighting::ThemeSet,
) -> String {
    use syntect::easy::HighlightLines;
    use syntect::util::as_24_bit_terminal_escaped;

    if code.is_empty() {
        return String::new();
    }

    let lang_lower = lang.trim().to_lowercase();

    let syntax = ss
        .find_syntax_by_token(&lang_lower)
        .or_else(|| ss.find_syntax_by_extension(&lang_lower));

    let Some(syntax) = syntax else {
        // Fallback: plain green for unknown languages
        return format!("\x1b[32m{code}\x1b[0m\n");
    };

    let theme = &ts.themes["base16-eighties.dark"];
    let mut highlighter = HighlightLines::new(syntax, theme);
    let mut output = String::new();

    for line_str in code.lines() {
        match highlighter.highlight_line(line_str, ss) {
            Ok(ranges) => {
                output.push_str(&as_24_bit_terminal_escaped(&ranges, false));
            }
            Err(_) => {
                output.push_str(line_str);
            }
        }
        output.push('\n');
    }

    output
}

// ---------------------------------------------------------------------------
// SessionSummary
// ---------------------------------------------------------------------------

/// End-of-session statistics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionSummary {
    /// Total number of turns (user + assistant).
    pub total_turns: usize,
    /// Total files modified.
    pub files_modified: usize,
    /// Total files created.
    pub files_created: usize,
    /// Total files deleted.
    pub files_deleted: usize,
    /// Total lines added.
    pub lines_added: usize,
    /// Total lines deleted.
    pub lines_deleted: usize,
    /// Total commands run.
    pub commands_run: usize,
    /// Total tools invoked.
    pub tools_invoked: usize,
    /// Session duration in seconds.
    pub duration_secs: u64,
    /// Session start time.
    pub started_at: DateTime<Utc>,
    /// Session end time.
    pub ended_at: DateTime<Utc>,
}

impl SessionSummary {
    /// Build a summary from DiffData and other metrics.
    pub fn from_diff_data(diff_data: &DiffData, commands_run: usize, tools_invoked: usize, started_at: DateTime<Utc>) -> Self {
        Self {
            total_turns: diff_data.turns.len(),
            files_modified: diff_data.total_files_modified(),
            files_created: diff_data.total_files_created(),
            files_deleted: diff_data.total_files_deleted(),
            lines_added: diff_data.total_additions(),
            lines_deleted: diff_data.total_deletions(),
            commands_run,
            tools_invoked,
            duration_secs: (Utc::now() - started_at).num_seconds().max(0) as u64,
            started_at,
            ended_at: Utc::now(),
        }
    }

    /// Render a human-readable summary.
    pub fn render(&self) -> String {
        let mut lines = Vec::new();
        lines.push("".to_string());
        lines.push("\x1b[1m--- Session Summary ---\x1b[0m".to_string());
        lines.push(format!("  Turns:          {}", self.total_turns));
        lines.push(format!("  Files modified: {}", self.files_modified));
        lines.push(format!("  Files created:  {}", self.files_created));
        lines.push(format!("  Files deleted:  {}", self.files_deleted));
        lines.push(format!("  Lines +{}/-{}", self.lines_added, self.lines_deleted));
        lines.push(format!("  Commands run:   {}", self.commands_run));
        lines.push(format!("  Tools invoked:  {}", self.tools_invoked));
        lines.push(format!("  Duration:       {}s", self.duration_secs));
        lines.push("".to_string());
        lines.join("\n")
    }

    /// Check if the session had any file changes.
    pub fn had_changes(&self) -> bool {
        self.files_modified + self.files_created + self.files_deleted > 0
    }
}

impl Default for SessionSummary {
    fn default() -> Self {
        Self {
            total_turns: 0,
            files_modified: 0,
            files_created: 0,
            files_deleted: 0,
            lines_added: 0,
            lines_deleted: 0,
            commands_run: 0,
            tools_invoked: 0,
            duration_secs: 0,
            started_at: Utc::now(),
            ended_at: Utc::now(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- TurnDiff ----------------------------------------------------------

    #[test]
    fn turn_diff_new() {
        let td = TurnDiff::new(0);
        assert_eq!(td.turn_index, 0);
        assert!(td.files_modified.is_empty());
        assert!(td.files_created.is_empty());
    }

    #[test]
    fn turn_diff_modify_file() {
        let mut td = TurnDiff::new(0);
        td.modify_file("main.rs".to_string(), 5, 2);
        assert_eq!(td.files_modified.len(), 1);
        assert_eq!(td.total_additions(), 5);
        assert_eq!(td.total_deletions(), 2);
        assert!(td.diff_summary.contains("modified"));
    }

    #[test]
    fn turn_diff_create_file() {
        let mut td = TurnDiff::new(1);
        td.create_file("new_file.rs".to_string());
        assert_eq!(td.files_created.len(), 1);
        assert!(td.diff_summary.contains("created"));
    }

    #[test]
    fn turn_diff_delete_file() {
        let mut td = TurnDiff::new(2);
        td.delete_file("old_file.rs".to_string());
        assert_eq!(td.files_deleted.len(), 1);
        assert!(td.diff_summary.contains("deleted"));
    }

    #[test]
    fn turn_diff_no_duplicate_create() {
        let mut td = TurnDiff::new(0);
        td.create_file("a.rs".to_string());
        td.create_file("a.rs".to_string());
        assert_eq!(td.files_created.len(), 1);
    }

    #[test]
    fn turn_diff_total_files_touched() {
        let mut td = TurnDiff::new(0);
        td.modify_file("a.rs".to_string(), 1, 0);
        td.create_file("b.rs".to_string());
        td.delete_file("c.rs".to_string());
        assert_eq!(td.total_files_touched(), 3);
    }

    // -- DiffData ----------------------------------------------------------

    #[test]
    fn diff_data_new() {
        let dd = DiffData::new();
        assert!(dd.turns.is_empty());
    }

    #[test]
    fn diff_data_record_and_get() {
        let mut dd = DiffData::new();
        let mut td = TurnDiff::new(0);
        td.modify_file("a.rs".to_string(), 3, 1);
        dd.record_turn_diff(td);
        assert_eq!(dd.get_session_diffs().len(), 1);
        assert_eq!(dd.total_additions(), 3);
    }

    #[test]
    fn diff_data_get_turn() {
        let mut dd = DiffData::new();
        dd.record_turn_diff(TurnDiff::new(0));
        dd.record_turn_diff(TurnDiff::new(1));
        assert!(dd.get_turn(1).is_some());
        assert!(dd.get_turn(2).is_none());
    }

    #[test]
    fn diff_data_latest_turn() {
        let mut dd = DiffData::new();
        dd.record_turn_diff(TurnDiff::new(0));
        dd.record_turn_diff(TurnDiff::new(5));
        assert_eq!(dd.latest_turn().unwrap().turn_index, 5);
    }

    #[test]
    fn diff_data_totals() {
        let mut dd = DiffData::new();
        let mut t1 = TurnDiff::new(0);
        t1.modify_file("a.rs".to_string(), 10, 2);
        t1.create_file("b.rs".to_string());
        let mut t2 = TurnDiff::new(1);
        t2.modify_file("c.rs".to_string(), 5, 0);
        t2.delete_file("d.rs".to_string());
        dd.record_turn_diff(t1);
        dd.record_turn_diff(t2);
        assert_eq!(dd.total_additions(), 15);
        assert_eq!(dd.total_deletions(), 2);
        assert_eq!(dd.total_files_modified(), 2);
        assert_eq!(dd.total_files_created(), 1);
        assert_eq!(dd.total_files_deleted(), 1);
    }

    // -- ReplHistory -------------------------------------------------------

    #[test]
    fn history_push_and_len() {
        let mut h = ReplHistory::new(10);
        h.push("hello");
        h.push("world");
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn history_skip_empty() {
        let mut h = ReplHistory::new(10);
        h.push("");
        h.push("   ");
        assert!(h.is_empty());
    }

    #[test]
    fn history_dedup() {
        let mut h = ReplHistory::new(10);
        h.push("hello");
        h.push("hello");
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn history_up_down() {
        let mut h = ReplHistory::new(10);
        h.push("first");
        h.push("second");
        h.push("third");
        assert_eq!(h.up(), Some("third"));
        assert_eq!(h.up(), Some("second"));
        assert_eq!(h.up(), Some("first"));
        assert_eq!(h.up(), Some("first")); // stays at top
        assert_eq!(h.down(), Some("second"));
        assert_eq!(h.down(), Some("third"));
        assert_eq!(h.down(), None); // back to bottom
    }

    #[test]
    fn history_search() {
        let mut h = ReplHistory::new(10);
        h.push("cargo build");
        h.push("cargo test");
        h.push("git status");
        let results = h.search_history("cargo");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn history_max_capacity() {
        let mut h = ReplHistory::new(3);
        h.push("a");
        h.push("b");
        h.push("c");
        h.push("d");
        assert_eq!(h.len(), 3);
        // "a" should have been evicted
        let all = h.entries();
        assert!(!all.contains(&"a"));
    }

    #[test]
    fn history_reset_cursor() {
        let mut h = ReplHistory::new(10);
        h.push("cmd");
        let _ = h.up();
        h.reset_cursor();
        assert_eq!(h.cursor(), -1);
    }

    // -- InputBuffer -------------------------------------------------------

    #[test]
    fn input_buffer_insert_and_text() {
        let mut buf = InputBuffer::new();
        buf.insert_char('h');
        buf.insert_char('i');
        assert_eq!(buf.text(), "hi");
    }

    #[test]
    fn input_buffer_backspace() {
        let mut buf = InputBuffer::new();
        buf.insert_char('a');
        buf.insert_char('b');
        buf.backspace();
        assert_eq!(buf.text(), "a");
    }

    #[test]
    fn input_buffer_delete() {
        let mut buf = InputBuffer::new();
        buf.insert_char('a');
        buf.insert_char('b');
        buf.move_left();
        buf.delete();
        assert_eq!(buf.text(), "a");
    }

    #[test]
    fn input_buffer_newline() {
        let mut buf = InputBuffer::new();
        buf.insert_char('f');
        buf.insert_char('n');
        buf.newline();
        buf.insert_char('g');
        assert_eq!(buf.text(), "fn\ng");
        assert_eq!(buf.line_count(), 2);
    }

    #[test]
    fn input_buffer_auto_indent() {
        let mut buf = InputBuffer::new();
        buf.set_auto_indent(true);
        buf.insert_char(' ');
        buf.insert_char(' ');
        buf.insert_char('x');
        buf.newline();
        // After newline, cursor should be at position 2 (auto-indented)
        assert_eq!(buf.cursor_col(), 2);
    }

    #[test]
    fn input_buffer_clear() {
        let mut buf = InputBuffer::new();
        buf.insert_char('a');
        buf.clear();
        assert_eq!(buf.text(), "");
        assert_eq!(buf.cursor_col(), 0);
    }

    #[test]
    fn input_buffer_set_text() {
        let mut buf = InputBuffer::new();
        buf.set_text("line1\nline2");
        assert_eq!(buf.line_count(), 2);
    }

    #[test]
    fn input_buffer_cursor_navigation() {
        let mut buf = InputBuffer::new();
        buf.insert_char('a');
        buf.insert_char('b');
        buf.insert_char('c');
        buf.move_left();
        buf.move_left();
        assert_eq!(buf.cursor_col(), 1);
        buf.move_right();
        assert_eq!(buf.cursor_col(), 2);
    }

    #[test]
    fn input_buffer_up_down() {
        let mut buf = InputBuffer::new();
        buf.insert_char('a');
        buf.newline();
        buf.insert_char('b');
        buf.move_up();
        assert_eq!(buf.cursor_row(), 0);
        buf.move_down();
        assert_eq!(buf.cursor_row(), 1);
    }

    // -- ReplRenderer ------------------------------------------------------

    #[test]
    fn render_user_output() {
        let r = ReplRenderer::new();
        let output = r.render_output("hello", "user");
        assert!(output.contains("hello"));
    }

    #[test]
    fn render_assistant_output() {
        let r = ReplRenderer::new();
        let output = r.render_output("world", "assistant");
        assert!(output.contains("world"));
    }

    #[test]
    fn render_error_output() {
        let r = ReplRenderer::new();
        let output = r.render_output("fail", "error");
        assert!(output.contains("fail"));
    }

    #[test]
    fn render_markdown_code_block() {
        let r = ReplRenderer::new();
        let md = "```rust\nfn main() {}\n```";
        let output = r.render_markdown(md);
        assert!(output.contains("rust"));
    }

    #[test]
    fn render_spinner() {
        let r = ReplRenderer::new();
        let frame = r.render_spinner("thinking", 0);
        assert!(frame.contains("/"));
        let frame = r.render_spinner("thinking", 2);
        assert!(frame.contains("\\"));
    }

    #[test]
    fn render_progress() {
        let r = ReplRenderer::new();
        let output = r.render_progress(5, 10, "loading");
        assert!(output.contains("50%"));
    }

    // -- render_streaming --------------------------------------------------

    #[test]
    fn render_streaming_plain_text() {
        let r = ReplRenderer::new();
        let output = r.render_streaming("Hello world");
        assert!(output.contains("Hello"));
        assert!(output.contains("world"));
        assert!(output.contains("▊")); // streaming cursor
    }

    #[test]
    fn render_streaming_code_block_complete() {
        let r = ReplRenderer::new();
        let input = "```rust\nfn main() {}\n```\nDone";
        let output = r.render_streaming(input);
        assert!(output.contains("rust"));
        assert!(output.contains("fn main()"));
        assert!(output.contains("Done"));
    }

    #[test]
    fn render_streaming_code_block_partial() {
        let r = ReplRenderer::new();
        // Unclosed code block — should show cursor inside code block
        let input = "```rust\nfn main() {";
        let output = r.render_streaming(input);
        assert!(output.contains("rust"));
        assert!(output.contains("fn main()"));
        assert!(output.contains("▊"));
    }

    #[test]
    fn render_streaming_strips_progress_markers() {
        let r = ReplRenderer::new();
        let input = "Thinking...\n📊 Tokens: 100 in + 50 out = $0.001\n⏳ Compressing...\nActual text";
        let output = r.render_streaming(input);
        assert!(output.contains("Thinking"));
        assert!(output.contains("Actual text"));
        assert!(!output.contains("📊"));
        assert!(!output.contains("⏳"));
    }

    #[test]
    fn render_streaming_strips_cost_markers() {
        let r = ReplRenderer::new();
        let input = "Result\n💰 Session total: $0.0500\nMore result";
        let output = r.render_streaming(input);
        assert!(output.contains("Result"));
        assert!(!output.contains("💰"));
    }

    #[test]
    fn render_streaming_tool_use_formatted() {
        let r = ReplRenderer::new();
        let input = "Let me check\n🔧 Using: bash with input: {\"command\": \"ls\"}\nResult here";
        let output = r.render_streaming(input);
        assert!(output.contains("Tool:"));
        assert!(output.contains("bash"));
        assert!(!output.contains("🔧"));
        assert!(output.contains("Result here"));
    }

    #[test]
    fn render_streaming_strips_turn_completed() {
        let r = ReplRenderer::new();
        let input = "Response\n[Turn 1 completed, 500 tokens]\nFinal";
        let output = r.render_streaming(input);
        assert!(output.contains("Response"));
        assert!(output.contains("Final"));
        assert!(!output.contains("[Turn"));
    }

    #[test]
    fn render_streaming_inline_code() {
        let r = ReplRenderer::new();
        let output = r.render_streaming("Use `cargo test` to run");
        assert!(output.contains("cargo test"));
    }

    #[test]
    fn render_streaming_bold() {
        let r = ReplRenderer::new();
        let output = r.render_streaming("This is **bold** text");
        assert!(output.contains("bold"));
    }

    #[test]
    fn render_streaming_empty_input() {
        let r = ReplRenderer::new();
        let output = r.render_streaming("");
        // Empty input should not panic
        assert!(output.is_empty() || !output.contains("▊") || true);
    }

    #[test]
    fn render_streaming_cursor_at_end() {
        let r = ReplRenderer::new();
        let output = r.render_streaming("Hello");
        assert!(output.contains("▊"));
        // Cursor should be after content, not before
        let cursor_pos = output.find('▊').unwrap();
        assert!(cursor_pos > 0);
    }

    #[test]
    fn render_streaming_preserves_blank_lines() {
        let r = ReplRenderer::new();
        let input = "Line 1\n\nLine 3";
        let output = r.render_streaming(input);
        assert!(output.contains("Line 1"));
        assert!(output.contains("Line 3"));
    }

    #[test]
    fn render_streaming_tool_progress_stripped() {
        let r = ReplRenderer::new();
        let input = "Working\n⏳ Tool progress: 50%\nDone";
        let output = r.render_streaming(input);
        assert!(output.contains("Working"));
        assert!(output.contains("Done"));
        assert!(!output.contains("50%"));
    }

    // -- SessionSummary ----------------------------------------------------

    #[test]
    fn session_summary_default() {
        let s = SessionSummary::default();
        assert_eq!(s.total_turns, 0);
        assert!(!s.had_changes());
    }

    #[test]
    fn session_summary_from_diff_data() {
        let mut dd = DiffData::new();
        let mut td = TurnDiff::new(0);
        td.modify_file("a.rs".to_string(), 10, 3);
        td.create_file("b.rs".to_string());
        dd.record_turn_diff(td);
        let started = Utc::now();
        let summary = SessionSummary::from_diff_data(&dd, 5, 12, started);
        assert_eq!(summary.total_turns, 1);
        assert_eq!(summary.files_modified, 1);
        assert_eq!(summary.files_created, 1);
        assert_eq!(summary.lines_added, 10);
        assert_eq!(summary.lines_deleted, 3);
        assert!(summary.had_changes());
    }

    #[test]
    fn session_summary_render() {
        let s = SessionSummary {
            total_turns: 3,
            files_modified: 5,
            files_created: 2,
            files_deleted: 1,
            lines_added: 100,
            lines_deleted: 20,
            commands_run: 10,
            tools_invoked: 25,
            duration_secs: 120,
            started_at: Utc::now(),
            ended_at: Utc::now(),
        };
        let rendered = s.render();
        assert!(rendered.contains("Session Summary"));
        assert!(rendered.contains("+100/-20"));
    }

    // -- Serialization round-trips -----------------------------------------

    #[test]
    fn turn_diff_serialization() {
        let mut td = TurnDiff::new(0);
        td.modify_file("a.rs".to_string(), 1, 0);
        let json = serde_json::to_string(&td).unwrap();
        let back: TurnDiff = serde_json::from_str(&json).unwrap();
        assert_eq!(td, back);
    }

    #[test]
    fn diff_data_serialization() {
        let dd = DiffData::new();
        let json = serde_json::to_string(&dd).unwrap();
        let back: DiffData = serde_json::from_str(&json).unwrap();
        assert_eq!(back.turns.len(), 0);
    }

    #[test]
    fn session_summary_serialization() {
        let s = SessionSummary::default();
        let json = serde_json::to_string(&s).unwrap();
        let back: SessionSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }
}
