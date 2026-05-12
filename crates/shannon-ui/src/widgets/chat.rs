//! Chat message widget with markdown rendering and search highlighting

use crate::tool_format::strip_ansi;
use crate::theme::Theme;
use crate::render::Renderer;
use std::collections::{HashMap, VecDeque};
use std::sync::LazyLock;
use parking_lot::Mutex;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use syntect::parsing::SyntaxSet;

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    Frame,
};

/// Lazy-initialized syntect state for diff syntax highlighting.
static DIFF_SYNTAX: LazyLock<(SyntaxSet, syntect::highlighting::Theme)> = LazyLock::new(|| {
    let ss = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();
    let theme = ts.themes["base16-eighties.dark"].clone();
    (ss, theme)
});

/// Lazy-initialized renderer for code syntax highlighting.
static CODE_RENDERER: LazyLock<Renderer> = LazyLock::new(|| {
    Renderer::new()
});

/// Convert a syntect Color to a ratatui Color.
fn syntect_to_ratatui(c: syntect::highlighting::Color) -> ratatui::style::Color {
    ratatui::style::Color::Rgb(c.r, c.g, c.b)
}

// ── Syntax highlighting cache ──────────────────────────────────────────

/// Cache for syntax-highlighted code blocks to avoid re-highlighting on every frame.
struct SyntaxCache {
    cache: HashMap<u64, Vec<Line<'static>>>,
    capacity: usize,
    order: std::collections::VecDeque<u64>,
}

impl SyntaxCache {
    fn new(capacity: usize) -> Self {
        Self {
            cache: HashMap::new(),
            capacity,
            order: std::collections::VecDeque::new(),
        }
    }

    fn compute_key(lang: &str, code: &str) -> u64 {
        let mut h: u64 = code.len() as u64;
        // Hash language fully, then code with sampling for long blocks
        for b in lang.bytes() {
            h = h.wrapping_mul(31).wrapping_add(b as u64);
        }
        let bytes = code.as_bytes();
        let len = bytes.len();
        if len <= 8192 {
            for &b in bytes {
                h = h.wrapping_mul(31).wrapping_add(b as u64);
            }
        } else {
            // Sample: first 4096 + last 4096 bytes for very long blocks
            for &b in &bytes[..4096] {
                h = h.wrapping_mul(31).wrapping_add(b as u64);
            }
            for &b in &bytes[len.saturating_sub(4096)..] {
                h = h.wrapping_mul(31).wrapping_add(b as u64);
            }
        }
        h
    }

    fn get(&self, key: u64) -> Option<&Vec<Line<'static>>> {
        self.cache.get(&key)
    }

    fn insert(&mut self, key: u64, lines: Vec<Line<'static>>) {
        if self.cache.len() >= self.capacity {
            if let Some(old_key) = self.order.pop_front() {
                self.cache.remove(&old_key);
            }
        }
        self.cache.insert(key, lines);
        self.order.push_back(key);
    }
}

/// Lazy-initialized syntax highlighting cache.
static SYNTAX_CACHE: LazyLock<Mutex<SyntaxCache>> = LazyLock::new(|| {
    Mutex::new(SyntaxCache::new(256))
});

/// Highlight code with caching. Returns cached result if available.
pub(super) fn highlight_code_cached(code: &str, lang: &str, theme: &crate::theme::Theme) -> Vec<Line<'static>> {
    let key = SyntaxCache::compute_key(lang, code);

    {
        let cache = SYNTAX_CACHE.lock();
        if let Some(lines) = cache.get(key) {
            return lines.clone();
        }
    }

    let lines = CODE_RENDERER.highlight_code(code, lang, theme);

    {
        let mut cache = SYNTAX_CACHE.lock();
        cache.insert(key, lines.clone());
    }

    lines
}

// ── Message height estimation ──────────────────────────────────────────

/// Estimate the number of terminal rows a message will occupy.
/// Chat message widget
pub struct ChatWidget {
    /// All chat messages
    pub messages: VecDeque<ChatMessage>,
    /// Current scroll offset (index of the focused message)
    pub scroll_offset: usize,
    /// Whether tool output messages are shown in collapsed (single-line) form
    pub collapsed_tools: bool,
    /// Whether streaming is active (show trailing cursor)
    pub streaming_active: bool,
    /// Inner width from last render (for height-weighted scrollbar positioning)
    last_inner_width: std::sync::atomic::AtomicUsize,
    /// Copy feedback: (message_index, timestamp) for showing "✓ Copied" on code blocks
    pub copy_feedback: Option<(usize, std::time::Instant)>,
    /// Last rendered area (for scrollbar hit testing)
    pub last_render_area: std::sync::Mutex<Option<Rect>>,
    /// Number of messages already committed to terminal scrollback (inline viewport mode)
    committed_count: usize,
    /// Terminal width at last commit (for resize reflow detection)
    committed_width: std::sync::atomic::AtomicU16,
    /// Column-based render path (exact-height virtual scrolling)
    column: super::column::ColumnRenderable,
    /// Lines buffered for scrollback insertion on next draw_frame()
    pub pending_scrollback: Vec<ratatui::text::Line<'static>>,
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
    /// When tool execution started (for duration display)
    pub start_time: Option<chrono::DateTime<chrono::Utc>>,
    /// How long the tool took, in seconds (set on completion)
    pub duration_secs: Option<f64>,
    /// Spinner frame index for running tools (cycles through braille dots)
    pub spinner_frame: usize,
    /// Whether this tool message is individually folded (expanded when false)
    pub folded: bool,
    /// Exit code from tool execution (None if not applicable or not captured)
    pub exit_code: Option<i32>,
}

/// Role of the chat message sender
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatRole {
    User,
    Assistant,
    System,
    Tool,
}

/// Keep at most this many committed messages in memory. Older ones are
/// already in terminal scrollback and can be evicted.
const MAX_COMMITTED_RETAIN: usize = 200;
/// Only trim when total message count exceeds this threshold.
const TRIM_THRESHOLD: usize = 500;

impl ChatWidget {
    /// Create a new chat widget
    pub fn new(capacity: usize) -> Self {
        Self {
            messages: VecDeque::with_capacity(capacity),
            scroll_offset: 0,
            collapsed_tools: true,
            streaming_active: false,
            last_inner_width: std::sync::atomic::AtomicUsize::new(80),
            copy_feedback: None,
            last_render_area: std::sync::Mutex::new(None),
            committed_count: 0,
            committed_width: std::sync::atomic::AtomicU16::new(0),
            column: super::column::ColumnRenderable::new(),
            pending_scrollback: Vec::new(),
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
            start_time: None,
            duration_secs: None,
            spinner_frame: 0,
            folded: true,
            exit_code: None,
        };

        let index = self.messages.len();
        self.messages.push_back(message.clone());
        self.column.push(super::renderable::MessageCell::new(message, self.collapsed_tools));
        self.mark_continuation(self.messages.back().unwrap().role);

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
            start_time: None,
            duration_secs: None,
            spinner_frame: 0,
            folded: true,
            exit_code: None,
        };

        let index = self.messages.len();
        self.messages.push_back(message.clone());
        self.column.push(super::renderable::MessageCell::new(message, self.collapsed_tools));
        self.mark_continuation(self.messages.back().unwrap().role);

        if !self.messages.is_empty() {
            self.scroll_offset = self.messages.len() - 1;
        }

        index
    }

    /// Update an existing message by index (for streaming updates)
    pub fn update_message(&mut self, index: usize, content: String) {
        if let Some(msg) = self.messages.get_mut(index) {
            msg.content = content;
            msg.timestamp = chrono::Utc::now();
            if let Some(cell) = self.column.get_mut(index) {
                cell.set_message(msg.clone());
            }
        }
    }

    /// Update a streaming message with newline-gated optimization.
    /// When `has_newline` is false, skips height cache invalidation (line count unchanged).
    pub fn update_streaming_message(&mut self, index: usize, content: String, has_newline: bool) {
        if let Some(msg) = self.messages.get_mut(index) {
            msg.content = content;
            msg.timestamp = chrono::Utc::now();
            if let Some(cell) = self.column.get_mut(index) {
                cell.update_streaming(msg.content.clone(), has_newline);
            }
        }
    }

    /// Update the last message (convenience method for streaming)
    pub fn update_last_message(&mut self, content: String) {
        if !self.messages.is_empty() {
            let last_index = self.messages.len() - 1;
            self.update_message(last_index, content);
        }
    }

    /// Add a tool result message with tool name and error status.
    ///
    /// If `start_time` is provided, computes the duration from start to now
    /// and stores it for display alongside the tool result.
    pub fn add_tool_message(
        &mut self,
        tool_name: String,
        content: String,
        is_error: bool,
        start_time: Option<chrono::DateTime<chrono::Utc>>,
    ) -> usize {
        let now = chrono::Utc::now();
        let duration_secs = start_time.map(|st| {
            (now - st).num_milliseconds() as f64 / 1000.0
        });
        let message = ChatMessage {
            role: ChatRole::Tool,
            content,
            timestamp: now,
            image_lines: None,
            is_error,
            tool_name: Some(tool_name),
            start_time,
            duration_secs,
            spinner_frame: 0,
            folded: true,
            exit_code: None,
        };
        let index = self.messages.len();
        self.messages.push_back(message.clone());
        self.column.push(super::renderable::MessageCell::new(message, self.collapsed_tools));
        self.mark_continuation(ChatRole::Tool);
        if !self.messages.is_empty() {
            self.scroll_offset = self.messages.len() - 1;
        }
        index
    }

    /// Clear all messages
    pub fn clear(&mut self) {
        self.messages.clear();
        self.column.clear();
        self.scroll_offset = 0;
        self.committed_count = 0;
        self.committed_width.store(0, std::sync::atomic::Ordering::Relaxed);
    }

    /// Mark all current messages as committed (e.g., after session restore).
    /// This excludes them from the viewport — they should be in scrollback.
    pub fn mark_all_committed(&mut self) {
        self.committed_count = self.messages.len();
    }

    /// Return the inner height of the chat area from the last render, with fallback.
    pub fn chat_viewport_height(&self) -> u16 {
        self.last_render_area.lock()
            .ok()
            .and_then(|ra| ra.map(|r| r.height.saturating_sub(2)))
            .unwrap_or(20)
    }

    /// Scroll up by `n` lines.
    pub fn scroll_up_by(&mut self, n: usize) {
        for _ in 0..n {
            self.scroll_up();
        }
    }

    /// Scroll down by `n` lines.
    pub fn scroll_down_by(&mut self, n: usize) {
        for _ in 0..n {
            self.scroll_down();
        }
    }

    /// Scroll up by one line. Scrolls within the current cell first,
    /// then moves to the previous message and scrolls to its bottom.
    pub fn scroll_up(&mut self) {
        if self.messages.is_empty() {
            return;
        }

        let scroll_y = self.column.cell_scroll(self.scroll_offset);
        if scroll_y > 0 {
            self.column.set_cell_scroll(self.scroll_offset, scroll_y - 1);
        } else if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
            // Scroll the previous cell to its bottom for continuous scrolling
            let width = self.last_inner_width.load(std::sync::atomic::Ordering::Relaxed) as u16;
            let desired = self.column.cell_height(self.scroll_offset, width);
            let allocated = self.column.cell_allocated_height(self.scroll_offset);
            // If the cell was never rendered (allocated == 0), estimate from viewport
            let allocated = if allocated == 0 {
                let vh = self.last_render_area.lock()
                    .ok()
                    .and_then(|guard| guard.map(|r| r.height))
                    .unwrap_or(24);
                desired.min(vh)
            } else {
                allocated
            };
            if desired > allocated && allocated > 0 {
                let max_scroll = desired.saturating_sub(allocated);
                self.column.set_cell_scroll(self.scroll_offset, max_scroll);
            } else {
                self.column.set_cell_scroll(self.scroll_offset, 0);
            }
        }
    }

    /// Scroll down by one line. Scrolls within the current cell first,
    /// then moves to the next message when at the bottom of the cell.
    /// Uses the cell's actual allocated render height for accurate max_scroll.
    pub fn scroll_down(&mut self) {
        if self.messages.is_empty() {
            return;
        }

        let width = self.last_inner_width.load(std::sync::atomic::Ordering::Relaxed) as u16;
        let desired = self.column.cell_height(self.scroll_offset, width);
        let scroll_y = self.column.cell_scroll(self.scroll_offset);
        let allocated = self.column.cell_allocated_height(self.scroll_offset);

        // Skip per-cell scrolling if no render has happened yet (allocated == 0).
        // Just move to the next message instead.
        if allocated == 0 {
            if self.scroll_offset < self.messages.len() - 1 {
                self.scroll_offset += 1;
            }
            return;
        }

        let visible_h = allocated;

        if desired > visible_h {
            let max_scroll = desired.saturating_sub(visible_h);
            if scroll_y < max_scroll {
                self.column.set_cell_scroll(self.scroll_offset, scroll_y + 1);
                return;
            }
        }

        if self.scroll_offset < self.messages.len() - 1 {
            self.scroll_offset += 1;
        }
    }

    /// Scroll to latest message (bottom)
    pub fn scroll_to_latest(&mut self) {
        if !self.messages.is_empty() {
            self.scroll_offset = self.messages.len() - 1;
            // Reset cell scroll when jumping to latest
            self.column.set_cell_scroll(self.scroll_offset, 0);
        }
    }

    /// Scroll to oldest message (index 0)
    pub fn scroll_to_top(&mut self) {
        if self.messages.is_empty() { return; }
        self.scroll_offset = 0;
        self.column.set_cell_scroll(self.scroll_offset, 0);
    }

    /// Scroll to a fractional position (0.0 = top/oldest, 1.0 = bottom/latest).
    /// Uses height-weighted mapping so drag feels smooth regardless of message sizes.
    pub fn scroll_to_ratio(&mut self, ratio: f64) {
        if self.messages.is_empty() { return; }
        let start = 0;
        let msg_count = self.messages.len();
        if start >= msg_count { return; }
        let visible_count = msg_count - start;
        if visible_count == 1 {
            self.scroll_offset = start;
            return;
        }

        // Compute cumulative heights for all messages.
        // No gap lines — ColumnRenderable::layout doesn't add gaps between cells.
        let mut cumulative: Vec<usize> = Vec::with_capacity(visible_count);
        let mut total_rows: usize = 0;
        let width = self.last_inner_width.load(std::sync::atomic::Ordering::Relaxed) as u16;
        for i in start..msg_count {
            total_rows += self.column.cell_height(i, width) as usize;
            cumulative.push(total_rows);
        }

        if total_rows == 0 { return; }

        // ratio 0.0 = top (oldest message), 1.0 = bottom (newest)
        let target_row = (ratio * (total_rows as f64 - 1.0)).round() as usize;

        // Find message index whose cumulative height contains target_row
        for (i, &cum) in cumulative.iter().enumerate() {
            if cum > target_row {
                self.scroll_offset = start + i;
                return;
            }
        }
        self.scroll_offset = msg_count - 1;
    }

    /// Toggle the fold state of a tool message by index.
    pub fn toggle_fold(&mut self, index: usize) {
        if let Some(msg) = self.messages.get_mut(index) {
            msg.folded = !msg.folded;
            if let Some(cell) = self.column.get_mut(index) {
                cell.set_message(msg.clone());
            }
        }
    }

    /// Toggle the fold state of the last tool message.
    pub fn toggle_last_tool_fold(&mut self) {
        for i in (0..self.messages.len()).rev() {
            if self.messages[i].role == ChatRole::Tool {
                self.toggle_fold(i);
                return;
            }
        }
    }


    /// Render the chat widget using ColumnRenderable (exact-height virtual scrolling).
    pub fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        theme: &Theme,
        search: Option<&crate::widgets::renderable::SearchParams>,
        auto_follow: bool,
    ) {
        tracing::debug!(
            "ChatWidget::render msgs={} committed={} scroll_offset={} area={}x{} streaming={}",
            self.messages.len(), self.committed_count, self.scroll_offset,
            area.width, area.height, self.streaming_active
        );
        let inner_width = area.width as usize;
        self.last_inner_width.store(inner_width, std::sync::atomic::Ordering::Relaxed);

        if let Ok(mut ra) = self.last_render_area.lock() {
            *ra = Some(area);
        }

        // Thin top separator with scroll position info
        let total = self.messages.len();
        let is_at_bottom = total == 0 || self.scroll_offset >= total.saturating_sub(1);
        let label = if total == 0 {
            " Chat ".to_string()
        } else if is_at_bottom {
            format!(" {total} ")
        } else {
            let pct = if total > 1 {
                (self.scroll_offset * 100) / (total - 1)
            } else {
                100
            };
            format!(" [{pct}%] {}/{} ", self.scroll_offset + 1, total)
        };
        let label = if !auto_follow && !is_at_bottom && total > 0 {
            format!("{label}↑")
        } else {
            label
        };
        let dash_count = (area.width as usize)
            .saturating_sub(UnicodeWidthStr::width(label.as_str()))
            .saturating_sub(1);
        let sep = format!("─{label}{}", "─".repeat(dash_count));
        let sep_line = ratatui::widgets::Paragraph::new(ratatui::text::Line::from(vec![
            ratatui::text::Span::styled(sep, ratatui::style::Style::default().fg(theme.border)),
        ]));
        let sep_area = Rect::new(area.x, area.y, area.width, 1);
        frame.render_widget(sep_line, sep_area);

        // Content area starts below separator, uses full width (no side borders)
        let inner = Rect::new(area.x, area.y + 1, area.width, area.height.saturating_sub(1));

        // Welcome screen when chat is empty
        if self.messages.is_empty() {
            let sep_w = 40.min(inner.width.saturating_sub(4) as usize);
            let b = ratatui::style::Style::default();
            let prim = b.fg(theme.primary);
            let bold_prim = b.fg(theme.primary).add_modifier(ratatui::style::Modifier::BOLD);
            let dim = b.fg(theme.text_dim);
            let muted = b.fg(theme.muted);
            let accent = b.fg(theme.secondary).add_modifier(ratatui::style::Modifier::BOLD);
            let border = b.fg(theme.border_dim);
            let sep = "\u{2500}".repeat(sep_w);

            let mut welcome_lines = vec![
                ratatui::text::Line::from(""),
                ratatui::text::Line::from(vec![
                    ratatui::text::Span::styled("      ", b),
                    ratatui::text::Span::styled("\u{2588}", bold_prim),
                    ratatui::text::Span::styled("\u{2584}", prim),
                    ratatui::text::Span::styled("  Shannon", bold_prim),
                ]),
                ratatui::text::Line::from(vec![
                    ratatui::text::Span::styled("      AI code assistant \u{00B7} multi-provider \u{00B7} MCP extensions", dim),
                ]),
                ratatui::text::Line::from(""),
                ratatui::text::Line::from(vec![
                    ratatui::text::Span::styled(format!("  {sep}"), border),
                ]),
                ratatui::text::Line::from(""),
                ratatui::text::Line::from(vec![
                    ratatui::text::Span::styled("  Try asking:", muted),
                ]),
                ratatui::text::Line::from(vec![
                    ratatui::text::Span::styled("    \u{25B8} ", b.fg(theme.primary)),
                    ratatui::text::Span::styled("\"Explain the architecture of this project\"", dim),
                ]),
                ratatui::text::Line::from(vec![
                    ratatui::text::Span::styled("    \u{25B8} ", b.fg(theme.primary)),
                    ratatui::text::Span::styled("\"Fix the failing tests in the auth module\"", dim),
                ]),
                ratatui::text::Line::from(vec![
                    ratatui::text::Span::styled("    \u{25B8} ", b.fg(theme.primary)),
                    ratatui::text::Span::styled("\"Add error handling to the API client\"", dim),
                ]),
                ratatui::text::Line::from(""),
                ratatui::text::Line::from(vec![
                    ratatui::text::Span::styled("  ", b),
                    ratatui::text::Span::styled("/help", accent),
                    ratatui::text::Span::styled(" commands  ", muted),
                    ratatui::text::Span::styled("/config", accent),
                    ratatui::text::Span::styled(" settings  ", muted),
                    ratatui::text::Span::styled("/theme", accent),
                    ratatui::text::Span::styled(" appearance", muted),
                ]),
            ];

            // Keyboard shortcuts row (only if enough vertical space)
            if inner.height >= 16 {
                welcome_lines.push(ratatui::text::Line::from(""));
                welcome_lines.push(ratatui::text::Line::from(vec![
                    ratatui::text::Span::styled("  ", b),
                    ratatui::text::Span::styled("Ctrl+E", accent),
                    ratatui::text::Span::styled(" editor  ", muted),
                    ratatui::text::Span::styled("Ctrl+F", accent),
                    ratatui::text::Span::styled(" fold  ", muted),
                    ratatui::text::Span::styled("Ctrl+G", accent),
                    ratatui::text::Span::styled(" pager  ", muted),
                    ratatui::text::Span::styled("F11", accent),
                    ratatui::text::Span::styled(" fullscreen", muted),
                ]));
            }

            let welcome = ratatui::widgets::Paragraph::new(welcome_lines);
            frame.render_widget(welcome, inner);
            return;
        }

        // Render visible cells using ColumnRenderable
        let buf = frame.buffer_mut();
        self.column.render(inner, buf, theme, self.scroll_offset, 0, search, self.streaming_active);
    }

    /// Render all messages including committed ones (used by transcript pager).
    /// Does NOT update last_inner_width/last_render_area — those belong to the main viewport.
    pub fn render_full(&self, frame: &mut Frame, area: Rect, theme: &Theme, scroll: usize) {
        let block = ratatui::widgets::Block::default()
            .borders(ratatui::widgets::Borders::ALL)
            .border_style(ratatui::style::Style::default().fg(theme.border))
            .title(" Transcript ");
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let buf = frame.buffer_mut();
        // start=0 renders all messages including committed (for transcript pager)
        self.column.render(inner, buf, theme, scroll, 0, None, false);
    }

    /// Find all occurrences of `query` in chat messages.
    ///
    /// Returns a list of `(message_index, byte_start, byte_end)` tuples.
    /// The search is case-insensitive.
    pub fn find_search_matches(&self, query: &str) -> Vec<(usize, usize, usize)> {
        if query.is_empty() {
            return Vec::new();
        }
        let query_lower = query.to_lowercase();
        let mut matches = Vec::new();
        for (msg_idx, msg) in self.messages.iter().enumerate() {
            let clean = strip_ansi(&msg.content);
            let content_lower = clean.to_lowercase();
            let mut search_from = 0;
            while let Some(pos) = content_lower[search_from..].find(&query_lower) {
                let abs_pos = search_from + pos;
                let match_end = abs_pos + query_lower.len();
                matches.push((msg_idx, abs_pos, match_end));
                // Advance past the match (non-overlapping) to stay on valid UTF-8 boundaries
                search_from = match_end;
                if search_from >= content_lower.len() {
                    break;
                }
            }
        }
        matches
    }

    /// Set continuation flag on the last-pushed cell based on previous message role.
    fn mark_continuation(&mut self, current_role: ChatRole) {
        let idx = self.messages.len().saturating_sub(1);
        if idx == 0 { return; }
        let prev_is_same_role = self.messages.get(idx - 1).map(|m| m.role == current_role).unwrap_or(false);
        if prev_is_same_role {
            if let Some(cell) = self.column.get_mut(idx) {
                cell.set_continuation(true);
            }
        }
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

    /// Get the last assistant message (searches backwards)
    pub fn last_assistant_message(&self) -> Option<&ChatMessage> {
        self.messages.iter().rev().find(|m| m.role == ChatRole::Assistant)
    }

    /// Remove and return the last message
    pub fn pop_last(&mut self) -> Option<ChatMessage> {
        let msg = self.messages.pop_back();
        if msg.is_some() {
            self.column.pop();
            if self.committed_count > self.messages.len() {
                self.committed_count = self.messages.len();
            }
        }
        msg
    }

    /// Get the number of messages
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Get the number of committed messages.
    pub fn committed_count(&self) -> usize {
        self.committed_count
    }

    /// Get the last rendered inner width of the chat area.
    pub fn last_inner_width(&self) -> u16 {
        self.last_inner_width.load(std::sync::atomic::Ordering::Relaxed) as u16
    }

    /// Get a reference to the message deque for rendering.
    pub fn messages(&self) -> &std::collections::VecDeque<ChatMessage> {
        &self.messages
    }

    /// Update committed count after progressive commit.
    pub fn set_committed_count(&mut self, count: usize) {
        self.committed_count = count.min(self.messages.len());
    }

    /// Total desired height of uncommitted cells at the given inner width.
    /// Whether the viewport is showing the latest messages (auto-follow eligible).
    pub fn is_at_bottom(&self) -> bool {
        if self.messages.is_empty() {
            return true;
        }
        self.scroll_offset >= self.messages.len().saturating_sub(1)
    }

    /// Iterate all messages with their indices
    pub fn iter_messages(&self) -> impl Iterator<Item = (usize, &ChatMessage)> {
        self.messages.iter().enumerate()
    }

    /// Render uncommitted messages as ratatui Lines for history injection.
    /// Returns (lines, total_height). Marks messages as committed.
    /// Skips the last message if streaming is active (it stays in viewport).
    /// Uses `MessageCell::lines()` for consistent markdown rendering with the viewport.
    pub fn commit_to_lines(&mut self, width: u16, theme: &Theme) -> (Vec<ratatui::text::Line<'static>>, u16) {
        if self.committed_count >= self.messages.len() {
            return (Vec::new(), 0);
        }

        // Always keep the last message in the viewport to avoid blank space between rounds.
        // During streaming, the last message IS the streaming message.
        // After streaming, the last message is the final assistant response.
        let commit_end = if !self.messages.is_empty() {
            self.messages.len().saturating_sub(1)
        } else {
            0
        };

        if self.committed_count >= commit_end {
            return (Vec::new(), 0);
        }

        tracing::debug!(
            "commit_to_lines: range={}..{} msgs={} streaming={}",
            self.committed_count, commit_end, self.messages.len(), self.streaming_active
        );

        let mut all_lines: Vec<ratatui::text::Line<'static>> = Vec::new();

        for i in self.committed_count..commit_end {
            let lines = if let Some(cell) = self.column.get(i) {
                cell.lines(width, theme)
            } else {
                let msg = &self.messages[i];
                let cell = super::renderable::MessageCell::new(msg.clone(), self.collapsed_tools);
                cell.lines(width, theme)
            };
            all_lines.extend(lines);
        }

        self.committed_count = commit_end;
        self.committed_width.store(width, std::sync::atomic::Ordering::Relaxed);
        let height = all_lines.len() as u16;
        (all_lines, height)
    }

    /// Check if committed content needs reflow due to terminal width change.
    pub fn needs_reflow(&self, current_width: u16) -> bool {
        let cw = self.committed_width.load(std::sync::atomic::Ordering::Relaxed);
        cw > 0 && cw != current_width && self.committed_count > 0
    }

    /// Invalidate all cell height caches (e.g., after terminal resize).
    pub fn invalidate_all_cells(&self) {
        self.column.invalidate_all();
    }

    /// Re-render all committed messages at a new width for scrollback reflow.
    /// Returns (lines, height) suitable for `insert_before` to overwrite scrollback.
    pub fn re_render_committed(&self, width: u16, theme: &Theme) -> (Vec<ratatui::text::Line<'static>>, u16) {
        if self.committed_count == 0 {
            return (Vec::new(), 0);
        }

        let mut all_lines: Vec<ratatui::text::Line<'static>> = Vec::new();

        for i in 0..self.committed_count {
            let lines = if let Some(cell) = self.column.get(i) {
                cell.lines(width, theme)
            } else {
                let msg = &self.messages[i];
                let cell = super::renderable::MessageCell::new(msg.clone(), self.collapsed_tools);
                cell.lines(width, theme)
            };
            all_lines.extend(lines);
        }

        let height = all_lines.len() as u16;
        (all_lines, height)
    }

    /// Reset committed count (e.g. after rewind or clear).
    pub fn reset_committed(&mut self) {
        self.committed_count = 0;
    }

    /// Evict old committed messages when total count exceeds the threshold.
    /// Committed messages are already in terminal scrollback, so removing them
    /// only affects the full-transcript pager — the terminal still has them.
    pub fn trim_old_committed(&mut self) {
        if self.messages.len() <= TRIM_THRESHOLD {
            return;
        }
        // How many committed messages to remove
        let excess = self.messages.len().saturating_sub(TRIM_THRESHOLD);
        // Don't remove more than (committed - MAX_COMMITTED_RETAIN)
        let removable = self.committed_count.saturating_sub(MAX_COMMITTED_RETAIN);
        let to_remove = excess.min(removable);
        if to_remove == 0 {
            return;
        }
        tracing::debug!(
            "trim_old_committed: removing {to_remove} oldest messages (total={}, committed={})",
            self.messages.len(), self.committed_count
        );
        // Drain from front
        for _ in 0..to_remove {
            self.messages.pop_front();
            self.column.pop_front();
        }
        self.committed_count = self.committed_count.saturating_sub(to_remove);
        self.scroll_offset = self.scroll_offset.saturating_sub(to_remove);
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
        self.column.truncate(cutoff);

        // Fix committed_count if it's now beyond the message list
        if self.committed_count > cutoff {
            self.committed_count = cutoff;
        }

        // Fix scroll offset
        if !self.messages.is_empty() {
            self.scroll_offset = self.messages.len() - 1;
        } else {
            self.scroll_offset = 0;
        }

        removed
    }
}

// ── Helper types and functions ──────────────────────────────────────────

/// Markdown segment: plain text, header, fenced code block, list, blockquote, or horizontal rule.
pub(super) enum MdSegment {
    /// Regular text lines
    Text(Vec<String>),
    /// Markdown header (## Header)
    Header { level: usize, text: String },
    /// Fenced code block with optional language tag
    CodeBlock { lang: Option<String>, code: String },
    /// Unordered list items (bullet points)
    UnorderedList(Vec<String>),
    /// Ordered list items (numbered)
    OrderedList(Vec<String>),
    /// Task list items (checkboxes): Vec of (checked, text)
    TaskList(Vec<(bool, String)>),
    /// Blockquote lines
    Blockquote(Vec<String>),
    /// Horizontal rule (thematic break)
    HorizontalRule,
    /// Table: (headers, rows)
    Table { headers: Vec<String>, rows: Vec<Vec<String>> },
}
/// Try to detect and extract a markdown table wrapped inside a code fence.
///
/// LLMs sometimes wrap tables in \`\`\`md fences, which causes them to render
/// as plain code instead of proper tables. This detects that pattern and
/// returns the parsed headers and rows.
fn try_unwrap_table(code: &str) -> Option<(Vec<String>, Vec<Vec<String>>)> {
    let lines: Vec<&str> = code.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    if lines.len() < 3 { return None; }

    // All lines must be pipe-delimited
    if !lines.iter().all(|l| l.starts_with('|') && l.ends_with('|')) {
        return None;
    }

    // Second line must be a separator (only |, -, :, spaces)
    let sep = lines[1].trim_matches('|').trim();
    if !sep.chars().all(|c| c == '-' || c == ':' || c == ' ') || !sep.contains('-') {
        return None;
    }

    // Parse header (first line)
    let headers: Vec<String> = lines[0].trim_matches('|')
        .split('|')
        .map(|c| c.trim().to_string())
        .collect();

    if headers.is_empty() || headers.iter().all(|h| h.is_empty()) {
        return None;
    }

    // Parse data rows (skip header + separator)
    let rows: Vec<Vec<String>> = lines[2..].iter()
        .map(|l| {
            l.trim_matches('|')
                .split('|')
                .map(|c| c.trim().to_string())
                .collect()
        })
        .collect();

    Some((headers, rows))
}

/// Parse content into markdown segments using pulldown-cmark.
pub(super) fn parse_markdown_segments(content: &str) -> Vec<MdSegment> {
    use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_TASKLISTS);

    let mut segments = Vec::new();
    let mut current_text: Vec<String> = Vec::new();
    let mut code_lang: Option<String> = None;
    let mut code_lines: Vec<String> = Vec::new();
    let mut in_code = false;
    let mut in_heading = false;
    let mut heading_level: usize = 0;
    let mut heading_text = String::new();
    let mut in_list = false;
    let mut list_ordered = false;
    let mut list_items: Vec<String> = Vec::new();
    let mut in_blockquote = false;
    let mut blockquote_lines: Vec<String> = Vec::new();
    let mut in_strikethrough = false;
    let mut _in_link = false;
    let mut link_url: Option<String> = None;
    // Table state
    let mut _in_table = false;
    let mut _in_table_head = false;
    let mut _in_table_row = false;
    let mut in_table_cell = false;
    let mut table_headers: Vec<String> = Vec::new();
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut current_row: Vec<String> = Vec::new();
    let mut current_cell = String::new();
    // Task list detection
    let mut is_task_list = false;
    let mut task_items: Vec<(bool, String)> = Vec::new();

    let flush_text = |segments: &mut Vec<MdSegment>, text: &mut Vec<String>| {
        if !text.is_empty() {
            segments.push(MdSegment::Text(std::mem::take(text)));
        }
    };

    for event in Parser::new_ext(content, opts) {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                flush_text(&mut segments, &mut current_text);
                in_code = true;
                code_lang = match kind {
                    CodeBlockKind::Fenced(l) => {
                        if l.is_empty() { None } else { Some(l.to_string()) }
                    }
                    CodeBlockKind::Indented => None,
                };
            }
            Event::End(TagEnd::CodeBlock) => {
                let code_content = code_lines.join("\n");
                // Fence unwrapping: LLMs often wrap tables in ```md or ```markdown
                // fences. Detect and unwrap so they render as proper tables.
                let lang_str = code_lang.as_deref();
                if matches!(lang_str, Some("md") | Some("markdown")) {
                    if let Some((headers, rows)) = try_unwrap_table(&code_content) {
                        segments.push(MdSegment::Table { headers, rows });
                        code_lines.clear();
                        in_code = false;
                        continue;
                    }
                }
                segments.push(MdSegment::CodeBlock {
                    lang: code_lang.take(),
                    code: code_content,
                });
                code_lines.clear();
                in_code = false;
            }
            Event::Start(Tag::Heading { level, .. }) => {
                flush_text(&mut segments, &mut current_text);
                in_heading = true;
                heading_level = level as usize;
                heading_text.clear();
            }
            Event::End(TagEnd::Heading(_)) => {
                segments.push(MdSegment::Header {
                    level: heading_level,
                    text: heading_text.trim().to_string(),
                });
                in_heading = false;
            }
            Event::Start(Tag::List(start_number)) => {
                flush_text(&mut segments, &mut current_text);
                in_list = true;
                list_ordered = start_number.is_some();
                list_items.clear();
                is_task_list = false;
                task_items.clear();
            }
            Event::End(TagEnd::List(_)) => {
                if is_task_list && !task_items.is_empty() {
                    segments.push(MdSegment::TaskList(std::mem::take(&mut task_items)));
                } else {
                    let items = std::mem::take(&mut list_items);
                    if list_ordered {
                        segments.push(MdSegment::OrderedList(items));
                    } else {
                        segments.push(MdSegment::UnorderedList(items));
                    }
                }
                in_list = false;
                is_task_list = false;
            }
            Event::Start(Tag::Item) => {
                // Reset item-level state
            }
            Event::End(TagEnd::Item) => {
                // Detect task list pattern: [x] or [ ] at start of last item
                if let Some(last) = list_items.last() {
                    let trimmed = last.trim_start();
                    if let Some(rest) = trimmed.strip_prefix("[x] ").or_else(|| trimmed.strip_prefix("[X] ")) {
                        is_task_list = true;
                        task_items.push((true, rest.to_string()));
                        list_items.pop();
                    } else if let Some(rest) = trimmed.strip_prefix("[ ] ") {
                        is_task_list = true;
                        task_items.push((false, rest.to_string()));
                        list_items.pop();
                    }
                }
            }
            Event::Start(Tag::BlockQuote(_)) => {
                flush_text(&mut segments, &mut current_text);
                in_blockquote = true;
                blockquote_lines.clear();
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                segments.push(MdSegment::Blockquote(std::mem::take(&mut blockquote_lines)));
                in_blockquote = false;
            }
            Event::Rule => {
                flush_text(&mut segments, &mut current_text);
                segments.push(MdSegment::HorizontalRule);
            }
            Event::Text(text) if in_table_cell => {
                current_cell.push_str(&text);
            }
            Event::Text(text) if in_code => {
                code_lines.extend(text.lines().map(|l| l.to_string()));
            }
            Event::Text(text) if in_heading => {
                heading_text.push_str(&text);
            }
            Event::Text(text) if in_list => {
                let text_str = if in_strikethrough { format!("~~{text}~~") } else { text.to_string() };
                // Collect text into the last list item or a new one
                let lines: Vec<&str> = text_str.lines().collect();
                for (i, line) in lines.iter().enumerate() {
                    if i == 0 {
                        if let Some(last) = list_items.last_mut() {
                            if !last.is_empty() {
                                last.push(' ');
                            }
                            last.push_str(line.trim());
                        } else {
                            list_items.push(line.trim().to_string());
                        }
                    } else {
                        list_items.push(line.trim().to_string());
                    }
                }
            }
            Event::Text(text) if in_blockquote => {
                let text_str = if in_strikethrough { format!("~~{text}~~") } else { text.to_string() };
                for line in text_str.lines() {
                    blockquote_lines.push(line.to_string());
                }
            }
            Event::Text(text) => {
                let text_str = if in_strikethrough { format!("~~{text}~~") } else { text.to_string() };
                current_text.extend(text_str.lines().map(|l| l.to_string()));
            }
            Event::SoftBreak | Event::HardBreak => {
                if in_heading {
                    heading_text.push(' ');
                }
                // In lists and blockquotes, soft breaks are handled by Text event splitting
            }
            Event::Code(code) if in_heading => {
                heading_text.push_str(&code);
            }
            Event::Code(code) if in_list => {
                if let Some(last) = list_items.last_mut() {
                    last.push_str(&code);
                } else {
                    list_items.push(code.to_string());
                }
            }
            Event::Code(code) => {
                current_text.push(code.to_string());
            }
            Event::Start(Tag::Strikethrough) => {
                in_strikethrough = true;
            }
            Event::End(TagEnd::Strikethrough) => {
                in_strikethrough = false;
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                _in_link = true;
                link_url = if dest_url.is_empty() { None } else { Some(dest_url.to_string()) };
            }
            Event::End(TagEnd::Link) => {
                if let Some(url) = link_url.take() {
                    // Append URL after link text for terminal display
                    let target = if in_list {
                        list_items.last_mut()
                    } else if in_blockquote {
                        blockquote_lines.last_mut()
                    } else {
                        current_text.last_mut()
                    };
                    if let Some(last) = target {
                        last.push_str(&format!(" ({url})"));
                    }
                }
                _in_link = false;
            }
            // ── Table support ──
            Event::Start(Tag::Table(_)) => {
                flush_text(&mut segments, &mut current_text);
                _in_table = true;
                table_headers.clear();
                table_rows.clear();
            }
            Event::End(TagEnd::Table) => {
                segments.push(MdSegment::Table {
                    headers: std::mem::take(&mut table_headers),
                    rows: std::mem::take(&mut table_rows),
                });
                _in_table = false;
            }
            Event::Start(Tag::TableHead) => {
                _in_table_head = true;
                current_row.clear();
            }
            Event::End(TagEnd::TableHead) => {
                table_headers = std::mem::take(&mut current_row);
                _in_table_head = false;
            }
            Event::Start(Tag::TableRow) => {
                _in_table_row = true;
                current_row.clear();
            }
            Event::End(TagEnd::TableRow) => {
                table_rows.push(std::mem::take(&mut current_row));
                _in_table_row = false;
            }
            Event::Start(Tag::TableCell) => {
                in_table_cell = true;
                current_cell.clear();
            }
            Event::End(TagEnd::TableCell) => {
                current_row.push(current_cell.trim().to_string());
                in_table_cell = false;
            }
            _ => {}
        }
    }

    // Flush remaining
    flush_text(&mut segments, &mut current_text);

    segments
}

/// Parse inline markdown formatting (**bold**, *italic*, `code`) into styled Spans.
pub(super) fn parse_inline_formatting(text: &str, base_color: ratatui::style::Color, theme: &crate::theme::Theme) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut pos = 0;
    let bytes = text.as_bytes();

    while pos < text.len() {
        if bytes[pos] == b'[' {
            // [link text](url) — render as underlined text
            if let Some(close_bracket) = text[pos + 1..].find(']') {
                let link_text_end = pos + 1 + close_bracket;
                let url_start = link_text_end + 1;
                if url_start < text.len() && bytes[url_start] == b'(' {
                    if let Some(close_paren) = text[url_start + 1..].find(')') {
                        let link_text = &text[pos + 1..link_text_end];
                        spans.push(Span::styled(
                            link_text.to_string(),
                            Style::default().fg(theme.link).add_modifier(Modifier::UNDERLINED),
                        ));
                        pos = url_start + 1 + close_paren + 1;
                        continue;
                    }
                }
            }
            // Not a valid link — treat [ as plain text
            spans.push(Span::styled(
                text[pos..pos+1].to_string(),
                Style::default().fg(base_color),
            ));
            pos += 1;
        } else if bytes[pos] == b'`' {
            // `inline code`
            let search_start = pos + 1;
            if let Some(end) = text[search_start..].find('`') {
                let close_start = search_start + end;
                let code_text = &text[search_start..close_start];
                spans.push(Span::styled(
                    format!(" {code_text} "),
                    Style::default().fg(theme.inline_code).bg(theme.inline_code_bg),
                ));
                pos = close_start + 1;
                continue;
            }
        } else if bytes[pos] == b'*' && pos + 1 < text.len() && bytes[pos + 1] == b'*' {
            // **bold**
            let search_start = pos + 2;
            if let Some(end) = text[search_start..].find("**") {
                let close_start = search_start + end;
                let bold_text = &text[search_start..close_start];
                spans.push(Span::styled(
                    bold_text.to_string(),
                    Style::default().fg(theme.bold_text).add_modifier(Modifier::BOLD),
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
                    Style::default().fg(theme.italic_text).add_modifier(Modifier::ITALIC),
                ));
                pos = close_start + 1;
                continue;
            }
        } else if bytes[pos] == b'~' && pos + 1 < text.len() && bytes[pos + 1] == b'~' {
            // ~~strikethrough~~
            let search_start = pos + 2;
            if let Some(end) = text[search_start..].find("~~") {
                let close_start = search_start + end;
                let strike_text = &text[search_start..close_start];
                spans.push(Span::styled(
                    strike_text.to_string(),
                    Style::default().fg(theme.text_dim).add_modifier(Modifier::CROSSED_OUT),
                ));
                pos = close_start + 2;
                continue;
            }
        }
        // Plain character — collect until next *, `, or ~ or end
        let plain_start = pos;
        while pos < text.len() && bytes[pos] != b'*' && bytes[pos] != b'`' && bytes[pos] != b'~' {
            pos += 1;
        }
        if pos > plain_start {
            spans.push(Span::styled(
                text[plain_start..pos].to_string(),
                Style::default().fg(base_color),
            ));
        } else {
            // Unmatched *, `, or ~, treat as plain
            spans.push(Span::styled(
                text[pos..pos+1].to_string(),
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

/// Wrap a line of text to fit within `max_cols` terminal columns, returning multiple lines.
/// Uses Unicode display width so CJK characters (2 columns each) are handled correctly.
/// Word-boundary wrapping with mid-word fallback for long unbroken strings.
pub(crate) fn wrap_line(s: &str, max_cols: usize) -> Vec<String> {
    if max_cols == 0 {
        return if s.is_empty() { vec![String::new()] } else { vec![s.to_string()] };
    }
    if unicode_width(s) <= max_cols {
        return if s.is_empty() { vec![String::new()] } else { vec![s.to_string()] };
    }

    let mut lines = Vec::new();
    let mut current = String::new();
    let mut col = 0usize;

    for word in s.split_whitespace() {
        let wcol = unicode_width(word);
        if col == 0 {
            if wcol > max_cols {
                // Break mid-word
                let mut buf = String::new();
                let mut buf_col = 0;
                for ch in word.chars() {
                    let cw = unicode_width_char(ch);
                    if buf_col + cw > max_cols {
                        lines.push(std::mem::take(&mut buf));
                        buf_col = 0;
                    }
                    buf.push(ch);
                    buf_col += cw;
                }
                if !buf.is_empty() {
                    current = buf;
                    col = buf_col;
                }
            } else {
                current.push_str(word);
                col = wcol;
            }
        } else if col + 1 + wcol <= max_cols {
            current.push(' ');
            current.push_str(word);
            col += 1 + wcol;
        } else if wcol > max_cols {
            lines.push(std::mem::take(&mut current));
            col = 0;
            let mut buf = String::new();
            let mut buf_col = 0;
            for ch in word.chars() {
                let cw = unicode_width_char(ch);
                if buf_col + cw > max_cols {
                    lines.push(std::mem::take(&mut buf));
                    buf_col = 0;
                }
                buf.push(ch);
                buf_col += cw;
            }
            if !buf.is_empty() {
                current = buf;
                col = buf_col;
            }
        } else {
            lines.push(std::mem::take(&mut current));
            current.push_str(word);
            col = wcol;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Return the Unicode display width of a string (CJK = 2 columns).
fn unicode_width(s: &str) -> usize {
    s.chars().map(unicode_width_char).sum()
}

/// Return the Unicode display width of a single character.
fn unicode_width_char(ch: char) -> usize {
    UnicodeWidthChar::width(ch).unwrap_or(0)
}


/// Truncate a string to fit within `max_cols` terminal columns (unicode display width),
/// appending "…" if truncated.
pub(crate) fn truncate_to(s: &str, max_cols: usize) -> String {
    if unicode_width(s) <= max_cols {
        s.to_string()
    } else if max_cols > 1 {
        let mut result = String::new();
        let mut w = 0;
        for ch in s.chars() {
            let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
            if w + cw > max_cols - 1 {
                break;
            }
            result.push(ch);
            w += cw;
        }
        format!("{result}…")
    } else {
        "…".to_string()
    }
}


/// Detect the programming language from diff header lines.
pub fn detect_diff_language(content: &str) -> Option<String> {
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
/// When `word_color` is Some, changed words within the line are highlighted
/// with that color for word-level diff emphasis.
pub fn highlight_diff_line(
    line: &str,
    lang: Option<&str>,
    base_color: ratatui::style::Color,
    word_color: Option<ratatui::style::Color>,
) -> Vec<Span<'static>> {
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
            if let Ok(ranges) = highlighter.highlight_line(content, ss) {
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
        }
    }

    // Fallback: plain content with word-level highlighting for changed words
    if let Some(wc) = word_color {
        let word_spans = highlight_diff_words(content, base_color, wc);
        spans.extend(word_spans);
    } else {
        spans.push(Span::styled(content.to_string(), Style::default().fg(base_color)));
    }
    spans
}

/// Highlight individual changed words within a diff content line.
/// Detects word boundaries (whitespace, punctuation transitions) and applies
/// `word_color` to tokens that look like changed content (not whitespace).
fn highlight_diff_words(content: &str, base_color: ratatui::style::Color, word_color: ratatui::style::Color) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut current = String::new();
    let mut in_word = false;

    for ch in content.chars() {
        let is_word_char = ch.is_alphanumeric() || ch == '_' || ch == '-';
        if is_word_char != in_word && !current.is_empty() {
            let color = if in_word { word_color } else { base_color };
            spans.push(Span::styled(std::mem::take(&mut current), Style::default().fg(color)));
        }
        current.push(ch);
        in_word = is_word_char;
    }
    if !current.is_empty() {
        let color = if in_word { word_color } else { base_color };
        spans.push(Span::styled(current, Style::default().fg(color)));
    }

    if spans.is_empty() {
        spans.push(Span::styled(content.to_string(), Style::default().fg(base_color)));
    }
    spans
}

/// Highlight search matches within a line of text, producing styled Spans.
///
/// Non-matching text uses `base_color`. Matching text uses `theme.selection_bg`
/// background with `theme.primary` foreground. The focused match (if any) gets
/// an additional BOLD modifier for visual distinction.
pub(super) fn highlight_search_in_text(
    text: &str,
    base_color: ratatui::style::Color,
    query: &str,
    focused_in_cell: bool,
    theme: &Theme,
) -> Vec<Span<'static>> {
    if query.is_empty() || text.is_empty() {
        return parse_inline_formatting(text, base_color, theme);
    }

    // Char-level case-insensitive search to avoid Unicode case-folding byte-length issues
    // (e.g. German ß → "ss" expands from 2 to 3 bytes)
    let query_lower: Vec<char> = query.to_lowercase().chars().collect();
    if query_lower.is_empty() {
        return parse_inline_formatting(text, base_color, theme);
    }

    let text_lower: Vec<char> = text.to_lowercase().chars().collect();
    let qlen = query_lower.len();

    // Find char-level match positions
    let mut match_char_ranges: Vec<(usize, usize)> = Vec::new();
    let mut ci = 0;
    while ci + qlen <= text_lower.len() {
        if text_lower[ci..ci + qlen] == query_lower[..] {
            match_char_ranges.push((ci, ci + qlen));
            ci += qlen;
        } else {
            ci += 1;
        }
    }

    if match_char_ranges.is_empty() {
        return parse_inline_formatting(text, base_color, theme);
    }

    // Build byte-offset map: char_index → byte_offset in original text
    let byte_offsets: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
    let text_byte_len = text.len();

    let mut spans = Vec::new();
    let mut last_end = 0;

    for (cs, ce) in &match_char_ranges {
        let start = byte_offsets.get(*cs).copied().unwrap_or(text_byte_len);
        let end = byte_offsets.get(*ce).copied().unwrap_or(text_byte_len);
        if start > last_end {
            spans.extend(parse_inline_formatting(&text[last_end..start], base_color, theme));
        }

        let matched_text = &text[start..end];
        let highlight_style = if focused_in_cell {
            Style::default()
                .fg(theme.primary)
                .bg(theme.selection_bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(theme.primary)
                .bg(theme.selection_bg)
        };
        spans.push(Span::styled(matched_text.to_string(), highlight_style));

        last_end = end;
    }

    // Remaining text after last match
    if last_end < text_byte_len {
        spans.extend(parse_inline_formatting(&text[last_end..], base_color, theme));
    }

    spans
}
