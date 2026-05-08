//! Chat message widget with markdown rendering and search highlighting

use crate::tool_format::{strip_ansi, tool_category, ToolCategory};
use crate::theme::Theme;
use crate::render::Renderer;
use std::collections::{HashMap, VecDeque};
use std::sync::LazyLock;
use parking_lot::Mutex;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

use ratatui::{
    layout::{Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Scrollbar, ScrollbarOrientation, ScrollbarState},
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
        for (i, b) in lang.bytes().chain(code.bytes()).enumerate() {
            h = h.wrapping_mul(31).wrapping_add(b as u64);
            if i > 1024 { break; }
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
fn highlight_code_cached(code: &str, lang: &str) -> Vec<Line<'static>> {
    let key = SyntaxCache::compute_key(lang, code);

    {
        let cache = SYNTAX_CACHE.lock();
        if let Some(lines) = cache.get(key) {
            return lines.clone();
        }
    }

    let lines = CODE_RENDERER.highlight_code(code, lang);

    {
        let mut cache = SYNTAX_CACHE.lock();
        cache.insert(key, lines.clone());
    }

    lines
}

// ── Message height cache for virtual scrolling ─────────────────────────

/// Cached height calculations for messages, keyed by (index, content_hash).
pub struct MessageHeightCache {
    heights: HashMap<(usize, u64), usize>,
    order: VecDeque<(usize, u64)>,
    capacity: usize,
}

/// Simple hash of content for cache invalidation.
fn content_hash(s: &str) -> u64 {
    let mut h: u64 = s.len() as u64;
    for (i, b) in s.bytes().enumerate() {
        h = h.wrapping_mul(31).wrapping_add(b as u64);
        if i > 64 { break; }
    }
    h
}

impl Default for MessageHeightCache {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageHeightCache {
    const DEFAULT_CAPACITY: usize = 512;

    pub fn new() -> Self {
        Self {
            heights: HashMap::new(),
            order: VecDeque::new(),
            capacity: Self::DEFAULT_CAPACITY,
        }
    }

    /// Get or compute the approximate height of a message in terminal rows.
    pub fn get_or_compute(
        &mut self,
        index: usize,
        msg: &ChatMessage,
        collapsed: bool,
        width: usize,
    ) -> usize {
        let hash = content_hash(&msg.content);
        let key = (index, hash);
        if let Some(&h) = self.heights.get(&key) {
            return h;
        }

        // Evict oldest entry if at capacity
        if self.heights.len() >= self.capacity {
            if let Some(old_key) = self.order.pop_front() {
                self.heights.remove(&old_key);
            }
        }

        let height = estimate_message_height(msg, collapsed, width);
        self.heights.insert(key, height);
        self.order.push_back(key);
        height
    }

    /// Invalidate a specific message (content changed during streaming).
    pub fn invalidate(&mut self, index: usize) {
        self.heights.retain(|(idx, _), _| *idx != index);
        self.order.retain(|(idx, _)| *idx != index);
    }

    /// Invalidate all (e.g., on resize).
    pub fn invalidate_all(&mut self) {
        self.heights.clear();
        self.order.clear();
    }
}

/// Estimate the number of terminal rows a message will occupy.
fn estimate_message_height(msg: &ChatMessage, collapsed: bool, width: usize) -> usize {
    if width == 0 { return 1; }

    // Collapsed tool messages: single line
    if msg.role == ChatRole::Tool && collapsed {
        return 1;
    }

    let content = strip_ansi(&msg.content);
    let mut height = 0;

    // Estimate prefix length for width calculation
    let prefix_len = 14 + 3; // "[HH:MM:SS] XXX > " approximate
    let text_width = width.saturating_sub(prefix_len);

    // Parse segments to count lines
    let segments = parse_markdown_segments(&content);
    for seg in &segments {
        match seg {
            MdSegment::Text(lines) => {
                height += lines.iter()
                    .map(|l| wrap_line(l, text_width).len().max(1))
                    .sum::<usize>()
                    .max(1);
            }
            MdSegment::CodeBlock { code, .. } => {
                let code_lines = code.lines().count();
                // Header + footer + code lines
                let total = 2 + code_lines;
                // Folding: if > 20 lines, show 10 head + fold indicator + 5 tail = 16
                height += if total > 20 { 10 + 1 + 5 + 2 } else { total + 2 };
            }
            MdSegment::Header { .. } => {
                height += 1;
            }
        }
    }

    // Image lines
    if let Some(ref imgs) = msg.image_lines {
        height += imgs.len();
    }

    height.max(1)
}

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
    /// Cached message heights for virtual scrolling
    pub height_cache: MessageHeightCache,
    /// Inner width from last render (for height-weighted scrollbar positioning)
    last_inner_width: std::sync::atomic::AtomicUsize,
    /// Copy feedback: (message_index, timestamp) for showing "✓ Copied" on code blocks
    pub copy_feedback: Option<(usize, std::time::Instant)>,
    /// Per-message render cache (Mutex for thread-safe interior mutability)
    pub render_cache: Mutex<super::MessageRenderCache>,
    /// Last rendered area (for scrollbar hit testing)
    pub last_render_area: std::sync::Mutex<Option<Rect>>,
    /// Number of messages already committed to terminal scrollback (inline viewport mode)
    committed_count: usize,
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
            height_cache: MessageHeightCache::new(),
            last_inner_width: std::sync::atomic::AtomicUsize::new(80),
            copy_feedback: None,
            render_cache: Mutex::new(super::MessageRenderCache::new(128)),
            last_render_area: std::sync::Mutex::new(None),
            committed_count: 0,
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
            start_time: None,
            duration_secs: None,
            spinner_frame: 0,
            folded: true,
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
            msg.timestamp = chrono::Utc::now();
            self.height_cache.invalidate(index);
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

    /// Scroll to a fractional position (0.0 = top/oldest, 1.0 = bottom/latest).
    /// Uses height-weighted mapping so drag feels smooth regardless of message sizes.
    pub fn scroll_to_ratio(&mut self, ratio: f64) {
        if self.messages.is_empty() { return; }
        let msg_count = self.messages.len();
        if msg_count == 1 {
            self.scroll_offset = 0;
            return;
        }

        // Compute cumulative heights to find total visual rows
        let mut cumulative: Vec<usize> = Vec::with_capacity(msg_count);
        let mut total_rows: usize = 0;
        for (i, msg) in self.messages.iter().enumerate() {
            let h = estimate_message_height(msg, self.collapsed_tools, self.last_inner_width.load(std::sync::atomic::Ordering::Relaxed));
            total_rows += h;
            if i + 1 < msg_count { total_rows += 1; } // gap
            cumulative.push(total_rows);
        }

        if total_rows == 0 { return; }

        // ratio 0.0 = top (oldest), 1.0 = bottom (newest)
        let target_row = (ratio * (total_rows as f64 - 1.0)).round() as usize;

        // Find message index whose cumulative height contains target_row
        for (i, &cum) in cumulative.iter().enumerate() {
            if cum > target_row {
                self.scroll_offset = i;
                return;
            }
        }
        self.scroll_offset = msg_count - 1;
    }

    /// Toggle the fold state of a tool message by index.
    pub fn toggle_fold(&mut self, index: usize) {
        if let Some(msg) = self.messages.get_mut(index) {
            msg.folded = !msg.folded;
            self.height_cache.invalidate(index);
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

    /// Render the chat widget with optional search highlighting.
    ///
    /// When `search_query` is Some, matching text in non-tool messages is highlighted
    /// using `theme.selection_bg` background and `theme.primary` foreground. The
    /// currently focused match (given by `focused_match`) gets a brighter highlight.
    pub fn render_with_search(
        &self,
        frame: &mut Frame,
        area: Rect,
        theme: &Theme,
        search_query: Option<&str>,
        search_matches: &[(usize, usize, usize)], // (msg_index, byte_start, byte_end)
        focused_match_idx: Option<usize>,          // index into search_matches
        show_all: bool,
    ) {
        let mut list_items = Vec::new();
        let inner_width = area.width.saturating_sub(2) as usize; // subtract borders
        self.last_inner_width.store(inner_width, std::sync::atomic::Ordering::Relaxed);

        // Store rendered area for scrollbar hit testing
        if let Ok(mut ra) = self.last_render_area.lock() {
            *ra = Some(area);
        }
        let visible_rows = area.height.saturating_sub(2) as usize;

        // Only render uncommitted messages in the viewport.
        // Committed messages live in terminal scrollback (injected via insert_before).
        // show_all=true is for the transcript pager (rendered on alternate screen).
        let messages: Vec<&ChatMessage> = if show_all {
            self.messages.iter().collect()
        } else {
            self.messages.iter().skip(self.committed_count).collect()
        };

        // Build a lookup: relative_msg_index -> list of (match_global_idx, byte_start, byte_end)
        let mut matches_by_msg: std::collections::HashMap<usize, Vec<(usize, usize, usize)>> =
            std::collections::HashMap::new();
        for (global_idx, &(msg_idx, start, end)) in search_matches.iter().enumerate() {
            matches_by_msg.entry(msg_idx).or_default().push((global_idx, start, end));
        }
        let msg_count = messages.len();

        // Virtual scrolling: compute approximate message heights and determine
        // which messages fall within the visible window.
        let (vis_start, vis_end) = if msg_count > 0 && visible_rows > 0 {
            // Accumulate heights from the end (latest messages shown at bottom).
            // scroll_offset is absolute; remap to relative index in filtered messages.
            let rel_scroll = self.scroll_offset;
            let focused = rel_scroll.min(msg_count - 1);

            // Walk backwards from focused message accumulating rows
            let mut end_idx = focused;
            let mut rows_used = 0usize;
            for i in (0..=focused).rev() {
                let h = estimate_message_height(messages[i], self.collapsed_tools, inner_width);
                if rows_used + h > visible_rows { break; }
                rows_used += h;
                // Include 1-row gap (separator or blank line) between messages
                if i > 0 {
                    rows_used += 1;
                }
                end_idx = i;
            }

            // Walk forward from focused to fill remaining rows
            let mut fwd_idx = focused;
            let mut fwd_rows = 0usize;
            for i in focused..msg_count {
                let h = estimate_message_height(messages[i], self.collapsed_tools, inner_width);
                if fwd_rows + h > visible_rows { break; }
                fwd_rows += h;
                if i + 1 < msg_count {
                    fwd_rows += 1;
                }
                fwd_idx = i;
            }

            // Buffer of 3 messages on each side for smooth scrolling
            let start = end_idx.saturating_sub(3);
            let end = (fwd_idx + 3).min(msg_count - 1);
            (start, end)
        } else {
            (0, msg_count.saturating_sub(1))
        };

        for (msg_idx, msg) in messages.iter().enumerate() {
            // Virtual scrolling: skip messages outside the visible window
            if msg_count > 20 && (msg_idx < vis_start || msg_idx > vis_end) {
                continue;
            }

            // Add gap between messages
            if msg_idx > 0 {
                list_items.push(ListItem::new(Line::from("")));
            }

            // Collapsed/folded tool messages: single-line summary with category color/icon
            if msg.role == ChatRole::Tool && (self.collapsed_tools || msg.folded) {
                let timestamp = msg.timestamp.format("%H:%M:%S").to_string();
                let clean_content = strip_ansi(&msg.content);
                let first_line = clean_content.lines().next().unwrap_or("");
                let tool_label = msg.tool_name.as_deref().unwrap_or("tool");

                // Determine category-specific icon, prefix, and color
                let cat = tool_category(tool_label);
                let (icon, prefix, cat_color) = match cat {
                    ToolCategory::Read => ("\u{25B8}", "", theme.tool_read),       // ▸ read
                    ToolCategory::Write => ("\u{270E}", "", theme.tool_write),     // ✎ write
                    ToolCategory::Search => ("\u{229B}", "", theme.tool_search),  // ⊛ search
                    ToolCategory::Bash => ("", "$ ", theme.tool_bash),             // $ bash
                    ToolCategory::Agent => ("\u{25C6}", "", theme.tool_read),      // ◆ agent (uses read color)
                };

                // Build summary text
                let summary_text = if msg.tool_name.is_some() {
                    let detail = first_line.strip_prefix(tool_label)
                        .map(|s| s.trim_start_matches([':', ' ']))
                        .unwrap_or(first_line);
                    if detail.is_empty() {
                        format!("{prefix}{tool_label}")
                    } else {
                        format!("{prefix}{tool_label}: {detail}")
                    }
                } else {
                    first_line.to_string()
                };

                // Calculate available width for summary
                let ts_len = timestamp.len();
                // Reserve space for: "[HH:MM:SS] ▸ summary  (2.3s) ✓"
                let duration_str = msg.duration_secs.map(|d| {
                    if d < 0.1 { format!("({:.0?}ms)", (d * 1000.0) as u32) }
                    else if d < 60.0 { format!("({d:.1}s)") }
                    else { format!("({:.0}m{:.0}s)", d / 60.0, d % 60.0) }
                });
                let duration_display_len = duration_str.as_ref().map(|s| s.len() + 1).unwrap_or(0);
                let status_len = 2; // " ✓" or " ✗"
                let icon_prefix_len = if icon.is_empty() { 0 } else { icon.chars().count() + 1 };
                let max_summary_width = inner_width
                    .saturating_sub(ts_len + 4 + icon_prefix_len + duration_display_len + status_len);

                let summary = if summary_text.chars().count() > max_summary_width {
                    let truncated: String = summary_text.chars().take(max_summary_width.saturating_sub(3)).collect();
                    format!("{truncated}...")
                } else {
                    summary_text
                };

                // Status icon: ✓ success, ✗ error
                let (status_icon, status_color) = if msg.is_error {
                    ("\u{2717}", theme.error)  // ✗
                } else {
                    ("\u{2713}", theme.success) // ✓
                };

                // Build the line spans
                let mut spans: Vec<Span<'static>> = Vec::new();

                // Timestamp: [HH:MM:SS]
                spans.push(Span::styled("[", Style::default().fg(theme.muted)));
                spans.push(Span::styled(timestamp, Style::default().fg(theme.muted)));
                spans.push(Span::styled("] ", Style::default().fg(theme.muted)));

                // Category icon
                if !icon.is_empty() {
                    spans.push(Span::styled(
                        format!("{icon} "),
                        Style::default().fg(cat_color),
                    ));
                }

                // Summary text in category color
                spans.push(Span::styled(
                    truncate_to(&summary, max_summary_width),
                    Style::default().fg(cat_color),
                ));

                // Duration display (dim)
                if let Some(ref dur) = duration_str {
                    spans.push(Span::styled(
                        format!(" {dur}"),
                        Style::default().fg(theme.muted),
                    ));
                }

                // Status icon
                spans.push(Span::styled(" ", Style::default()));
                spans.push(Span::styled(status_icon, Style::default().fg(status_color)));

                // Line count hint for individually folded messages
                if !self.collapsed_tools && msg.folded {
                    let line_count = clean_content.lines().count();
                    if line_count > 1 {
                        spans.push(Span::styled(
                            format!(" [+{}]", line_count.saturating_sub(1)),
                            Style::default().fg(theme.muted),
                        ));
                    }
                }

                list_items.push(ListItem::new(Line::from(spans)));
                continue;
            }

            let (role_prefix, _role_name, role_color) = match msg.role {
                ChatRole::User => ("You", "You", theme.user_msg),
                ChatRole::Assistant => ("AI", "AI", theme.assistant_msg),
                ChatRole::System => ("SYS", "System", theme.system_msg),
                ChatRole::Tool => {
                    // Use category color for tool messages when tool_name is known
                    let cat_color = msg.tool_name.as_deref()
                        .map(|name| {
                            let cat = tool_category(name);
                            match cat {
                                ToolCategory::Read => theme.tool_read,
                                ToolCategory::Write => theme.tool_write,
                                ToolCategory::Search => theme.tool_search,
                                ToolCategory::Bash => theme.tool_bash,
                                ToolCategory::Agent => theme.tool_read,
                            }
                        })
                        .unwrap_or(theme.tool_msg);
                    ("Tool", "Tool", cat_color)  // ⚙
                }
            };

            let timestamp = msg.timestamp.format("%H:%M:%S").to_string();

            // Build the role prefix line: "[HH:MM:SS] You > " or "[HH:MM:SS] AI > "
            // For tool messages, the prefix is "[HH:MM:SS] Tool ⚙: "
            let (_prefix_display, prefix_len) = if msg.role == ChatRole::Tool {
                let prefix = format!("[{timestamp}] Tool \u{2699}: ");
                (prefix.clone(), prefix.chars().count())
            } else {
                let prefix = format!("[{timestamp}] {role_prefix} > ");
                (prefix.clone(), prefix.chars().count())
            };

            // Strip ANSI escape codes — ratatui doesn't interpret them
            let clean_content = strip_ansi(&msg.content);

            // Detect and render diff content inline for tool messages
            if msg.role == ChatRole::Tool && is_diff_content(&clean_content) {
                let lang = detect_diff_language(&clean_content);
                let indent = " ".repeat(prefix_len);
                let mut first_line = true;
                for line in clean_content.lines().take(50) {
                    let color = diff_line_color(line, theme);
                    let word_color = if line.starts_with('+') && !line.starts_with("+++") {
                        Some(theme.diff_added_word)
                    } else if line.starts_with('-') && !line.starts_with("---") {
                        Some(theme.diff_removed_word)
                    } else {
                        None
                    };
                    let diff_spans = highlight_diff_line(line, lang.as_deref(), color, word_color);
                    if first_line {
                        let mut spans = vec![
                            Span::styled("[", Style::default().fg(theme.muted)),
                            Span::styled(timestamp.clone(), Style::default().fg(theme.muted)),
                            Span::styled("] ", Style::default().fg(theme.muted)),
                            Span::styled("Tool ", Style::default().fg(role_color).add_modifier(Modifier::BOLD)),
                            Span::styled("\u{2699}", Style::default().fg(role_color)),
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

            // Try render cache for non-search messages
            let should_cache = search_query.is_none();
            if should_cache {
                let hash = super::content_hash(&msg.content);
                if let Some(cached) = self.render_cache.lock().get(msg_idx, hash, area.width) {
                    list_items.extend(cached.iter().cloned().map(ListItem::new));
                    if let Some(ref img_lines) = msg.image_lines {
                        for img_line in img_lines {
                            list_items.push(ListItem::new(img_line.clone()));
                        }
                    }
                    continue;
                }
            }

            let mut msg_lines: Vec<Line<'static>> = Vec::new();

            // Parse into segments: normal text and code blocks
            let segments = parse_markdown_segments(&clean_content);
            let mut first_line = true;

            for segment in &segments {
                match segment {
                    MdSegment::Text(lines) => {
                        for content_line in lines {
                            let available = inner_width.saturating_sub(prefix_len);
                            let wrapped = wrap_line(content_line, available);

                            for wrapped_text in &wrapped {
                                if first_line {
                                    let mut spans = vec![
                                        Span::styled("[", Style::default().fg(theme.muted)),
                                        Span::styled(timestamp.clone(), Style::default().fg(theme.muted)),
                                        Span::styled("] ", Style::default().fg(theme.muted)),
                                        Span::styled(format!("{role_prefix} > "), Style::default().fg(role_color).add_modifier(Modifier::BOLD)),
                                    ];

                                    if let Some(query) = search_query {
                                        if !query.is_empty() {
                                            let msg_matches = matches_by_msg.get(&msg_idx);
                                            spans.extend(highlight_search_in_text(
                                                wrapped_text, theme.text, query, msg_matches, focused_match_idx, true,
                                            ));
                                        } else {
                                            spans.extend(parse_inline_formatting(wrapped_text, theme.text));
                                        }
                                    } else {
                                        spans.extend(parse_inline_formatting(wrapped_text, theme.text));
                                    }
                                    msg_lines.push(Line::from(spans));
                                    first_line = false;
                                } else {
                                    let indent = " ".repeat(prefix_len);
                                    let mut spans = vec![
                                        Span::styled(indent, Style::default().fg(theme.muted)),
                                    ];

                                    if let Some(query) = search_query {
                                        if !query.is_empty() {
                                            let msg_matches = matches_by_msg.get(&msg_idx);
                                            spans.extend(highlight_search_in_text(
                                                wrapped_text, theme.text, query, msg_matches, focused_match_idx, false,
                                            ));
                                        } else {
                                            spans.extend(parse_inline_formatting(wrapped_text, theme.text));
                                        }
                                    } else {
                                        spans.extend(parse_inline_formatting(wrapped_text, theme.text));
                                    }
                                    msg_lines.push(Line::from(spans));
                                }
                            }
                        }
                    }
                    MdSegment::CodeBlock { lang, code } => {
                        let indent_len = prefix_len.saturating_sub(2); // slightly less indent for code
                        let indent = " ".repeat(indent_len);
                        let available = inner_width.saturating_sub(indent_len);
                        let border_style = Style::default().fg(theme.muted);

                        // Parse filename hint from lang string: e.g. "rust:src/main.rs"
                        let lang_str = lang.as_deref().unwrap_or("code");
                        let (display_lang, filename_hint) = if let Some(colon_pos) = lang_str.find(':') {
                            let (l, f) = lang_str.split_at(colon_pos);
                            (l.to_string(), Some(f[1..].to_string()))
                        } else {
                            (lang_str.to_string(), None)
                        };

                        // Title bar: ╭─ rust ─ src/main.rs ─ [copy] ╮
                        let title_content = match &filename_hint {
                            Some(fname) => format!(" {display_lang} ─ {fname} "),
                            None => format!(" {display_lang} "),
                        };

                        // Check if this message has active copy feedback (within 2 seconds)
                        let copy_hint = if let Some((feedback_idx, timestamp)) = self.copy_feedback {
                            if feedback_idx == msg_idx && timestamp.elapsed() < std::time::Duration::from_secs(2) {
                                " ✓ Copied"
                            } else {
                                " [copy]"
                            }
                        } else {
                            " [copy]"
                        };

                        let header_with_hint = format!("╭─{title_content}─{copy_hint}╮");
                        msg_lines.push(Line::from(vec![
                            Span::styled(indent.clone(), Style::default().fg(theme.muted)),
                            Span::styled(truncate_to(&header_with_hint, available), border_style),
                        ]));

                        // Code lines with syntax highlighting (cached)
                        let highlighted_lines = highlight_code_cached(code, &display_lang);
                        let total_lines = highlighted_lines.len();
                        let fold_threshold = 20;
                        let fold_head = 10;
                        let fold_tail = 5;
                        let line_num_width = 7; // " 123 │ "
                        let code_width = available.saturating_sub(line_num_width);
                        let cont_prefix = "  → ";

                        if total_lines > fold_threshold {
                            // Show first fold_head lines with line numbers
                            for (i, line) in highlighted_lines.iter().enumerate().take(fold_head) {
                                let line_num = format!("{:>4} │ ", i + 1);
                                let code_spans: Vec<Span<'static>> = line.spans.iter().map(|s| Span::styled(s.content.to_string(), s.style)).collect();
                                let wrapped = wrap_code_spans(&code_spans, code_width);
                                for (wi, wrapped_line) in wrapped.iter().enumerate() {
                                    let mut line_spans = vec![
                                        Span::styled(indent.clone(), Style::default().fg(theme.muted)),
                                    ];
                                    if wi == 0 {
                                        line_spans.push(Span::styled(line_num.clone(), Style::default().fg(theme.text_dim)));
                                    } else {
                                        line_spans.push(Span::styled(cont_prefix.to_string(), Style::default().fg(theme.text_dim)));
                                    }
                                    line_spans.extend(wrapped_line.iter().map(|s| Span::styled(s.content.clone(), s.style)));
                                    msg_lines.push(Line::from(line_spans));
                                }
                            }

                            // Fold indicator
                            let folded_count = total_lines - fold_head - fold_tail;
                            let fold_msg = format!("│   ... {} lines folded ...", folded_count.max(1));
                            msg_lines.push(Line::from(vec![
                                Span::styled(indent.clone(), Style::default().fg(theme.muted)),
                                Span::styled(fold_msg, Style::default().fg(theme.muted).add_modifier(Modifier::ITALIC)),
                            ]));

                            // Show last fold_tail lines with line numbers
                            let tail_start = total_lines.saturating_sub(fold_tail);
                            for (i, line) in highlighted_lines[tail_start..].iter().enumerate() {
                                let line_num = format!("{:>4} │ ", tail_start + i + 1);
                                let code_spans: Vec<Span<'static>> = line.spans.iter().map(|s| Span::styled(s.content.to_string(), s.style)).collect();
                                let wrapped = wrap_code_spans(&code_spans, code_width);
                                for (wi, wrapped_line) in wrapped.iter().enumerate() {
                                    let mut line_spans = vec![
                                        Span::styled(indent.clone(), Style::default().fg(theme.muted)),
                                    ];
                                    if wi == 0 {
                                        line_spans.push(Span::styled(line_num.clone(), Style::default().fg(theme.text_dim)));
                                    } else {
                                        line_spans.push(Span::styled(cont_prefix.to_string(), Style::default().fg(theme.text_dim)));
                                    }
                                    line_spans.extend(wrapped_line.iter().map(|s| Span::styled(s.content.clone(), s.style)));
                                    msg_lines.push(Line::from(line_spans));
                                }
                            }
                        } else {
                            // Show all lines with line numbers
                            for (i, line) in highlighted_lines.iter().enumerate() {
                                let line_num = format!("{:>4} │ ", i + 1);
                                let code_spans: Vec<Span<'static>> = line.spans.iter().map(|s| Span::styled(s.content.to_string(), s.style)).collect();
                                let wrapped = wrap_code_spans(&code_spans, code_width);
                                for (wi, wrapped_line) in wrapped.iter().enumerate() {
                                    let mut line_spans = vec![
                                        Span::styled(indent.clone(), Style::default().fg(theme.muted)),
                                    ];
                                    if wi == 0 {
                                        line_spans.push(Span::styled(line_num.clone(), Style::default().fg(theme.text_dim)));
                                    } else {
                                        line_spans.push(Span::styled(cont_prefix.to_string(), Style::default().fg(theme.text_dim)));
                                    }
                                    line_spans.extend(wrapped_line.iter().map(|s| Span::styled(s.content.clone(), s.style)));
                                    msg_lines.push(Line::from(line_spans));
                                }
                            }
                        }

                        // Footer: ╰──────────────────────╯
                        let footer_width = title_content.chars().count() + 2;
                        let footer = format!("╰{:─>width$}╯", "", width = footer_width);
                        msg_lines.push(Line::from(vec![
                            Span::styled(indent, Style::default().fg(theme.muted)),
                            Span::styled(truncate_to(&footer, available), border_style),
                        ]));
                        first_line = false;
                    }
                    MdSegment::Header { level, text } => {
                        let indent_len = prefix_len;
                        let indent = " ".repeat(indent_len);
                        let available = inner_width.saturating_sub(indent_len);
                        let style = match level {
                            1 => Style::default().fg(theme.primary).add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                            2 => Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
                            _ => Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
                        };
                        let wrapped = wrap_line(text, available);
                        for wrapped_text in &wrapped {
                            msg_lines.push(Line::from(vec![
                                Span::styled(indent.clone(), Style::default().fg(theme.muted)),
                                Span::styled(wrapped_text.clone(), style),
                            ]));
                        }
                        first_line = false;
                    }
                }
            }

            // If the message has inline image preview lines, render them
            if let Some(ref img_lines) = msg.image_lines {
                for img_line in img_lines {
                    msg_lines.push(img_line.clone());
                }
            }

            // Cache and push rendered lines
            if should_cache {
                let hash = super::content_hash(&msg.content);
                self.render_cache.lock().insert(msg_idx, hash, area.width, msg_lines.clone());
            }
            list_items.extend(msg_lines.into_iter().map(ListItem::new));
        }

        // Streaming cursor: append a blinking cursor block at the end
        if self.streaming_active {
            list_items.push(ListItem::new(Line::from(
                Span::styled(crate::a11y::cursor().to_string(), Style::default().fg(theme.primary).add_modifier(Modifier::SLOW_BLINK)),
            )));
        }

        // Slice list_items to fit the visible area.
        // inner height = area height minus top/bottom borders (2 rows)
        let visible_rows = area.height.saturating_sub(2) as usize;
        let total = list_items.len();
        let mut scroll_start = 0usize;
        let items = if total > visible_rows {
            // Show the latest messages (from the bottom).
            // When scroll_offset < last message, user scrolled up → show from earlier.
            // Default scroll_offset = last msg index → show latest.
            let max_start = total.saturating_sub(visible_rows);
            // Use scroll_offset to determine how far back to show.
            // scroll_offset = msg index; map to approximate line offset.
            let scroll_back = self.messages.len().saturating_sub(1).saturating_sub(self.scroll_offset);
            let start = max_start.saturating_sub(scroll_back).min(max_start);
            scroll_start = start;

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

        // Render scrollbar when content overflows
        if total > visible_rows && visible_rows > 0 && area.height > 2 {
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .thumb_style(Style::default().fg(theme.muted))
                .track_style(Style::default().fg(theme.border));
            let mut sb_state = ScrollbarState::new(total)
                .viewport_content_length(visible_rows)
                .position(scroll_start);
            let sb_area = area.inner(Margin { vertical: 1, horizontal: 1 });
            frame.render_stateful_widget(scrollbar, sb_area, &mut sb_state);
        }
    }

    /// Render the chat widget (no search highlighting).
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        self.render_with_search(frame, area, theme, None, &[], None, false);
    }

    /// Render all messages including committed ones (used by transcript pager).
    pub fn render_full(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        self.render_with_search(frame, area, theme, None, &[], None, true);
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
                matches.push((msg_idx, abs_pos, abs_pos + query.len()));
                search_from = abs_pos + 1;
                if search_from >= content_lower.len() {
                    break;
                }
            }
        }
        matches
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

    /// Render uncommitted messages to ANSI text and mark them committed.
    /// Returns ANSI string ready to write to stdout (for terminal scrollback).
    /// `width` is the terminal width for line wrapping.
    pub fn commit_to_ansi(&mut self, width: u16) -> String {
        if self.committed_count >= self.messages.len() {
            return String::new();
        }

        let mut output = String::new();
        let content_width = width.saturating_sub(2) as usize;

        for msg in self.messages.iter().skip(self.committed_count) {
            let ts = msg.timestamp.format("%H:%M:%S").to_string();

            // Role prefix with color
            let (role_name, role_color) = match msg.role {
                ChatRole::User => ("You", "\x1b[32m"),
                ChatRole::Assistant => ("AI", "\x1b[36m"),
                ChatRole::System => ("Sys", "\x1b[33m"),
                ChatRole::Tool => ("Tool", "\x1b[35m"),
            };
            output.push_str(&format!("\x1b[90m[{ts}]\x1b[0m {role_color}{role_name}\x1b[0m"));

            // Duration for tool messages
            if msg.role == ChatRole::Tool {
                if let Some(dur) = msg.duration_secs {
                    output.push_str(&format!(" \x1b[90m({dur:.1}s)\x1b[0m"));
                }
            }
            output.push('\n');

            // Content with wrapping
            let clean = strip_ansi(&msg.content);
            let indent = 2;
            let available = content_width.saturating_sub(indent);

            if msg.role == ChatRole::Tool && self.collapsed_tools {
                // Collapsed tool: first line only
                let first_line = clean.lines().next().unwrap_or("");
                let total_lines = clean.lines().count();
                if total_lines <= 1 {
                    output.push_str(&format!("  {first_line}\n"));
                } else {
                    output.push_str(&format!("  {first_line} \x1b[90m[+{total_lines}]\x1b[0m\n"));
                }
            } else {
                for content_line in clean.lines() {
                    let wrapped = wrap_line(content_line, available);
                    for w in &wrapped {
                        output.push_str(&format!("  {w}\n"));
                    }
                }
            }

            output.push('\n');
        }

        self.committed_count = self.messages.len();
        output
    }

    /// Render uncommitted messages as ratatui Lines for history injection.
    /// Returns (lines, total_height). Marks messages as committed.
    /// Skips the last message if streaming is active (it stays in viewport).
    pub fn commit_to_lines(&mut self, width: u16) -> (Vec<ratatui::text::Line<'static>>, u16) {
        if self.committed_count >= self.messages.len() {
            return (Vec::new(), 0);
        }

        // Don't commit the last message if streaming (it stays in viewport for live rendering)
        let commit_end = if self.streaming_active && !self.messages.is_empty() {
            self.messages.len().saturating_sub(1)
        } else {
            self.messages.len()
        };

        if self.committed_count >= commit_end {
            return (Vec::new(), 0);
        }

        let mut all_lines: Vec<ratatui::text::Line<'static>> = Vec::new();
        let content_width = width.saturating_sub(2) as usize;

        for msg in self.messages.iter().skip(self.committed_count).take(commit_end - self.committed_count) {
            let ts = msg.timestamp.format("%H:%M:%S").to_string();

            // Role prefix with style
            let (role_name, role_style) = match msg.role {
                ChatRole::User => ("You", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                ChatRole::Assistant => ("AI", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                ChatRole::System => ("Sys", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                ChatRole::Tool => ("Tool", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
            };

            let mut header_spans = vec![
                Span::styled(format!("[{ts}]"), Style::default().fg(Color::DarkGray)),
                Span::raw(" "),
                Span::styled(role_name, role_style),
            ];

            if msg.role == ChatRole::Tool {
                if let Some(dur) = msg.duration_secs {
                    header_spans.push(Span::styled(format!(" ({dur:.1}s)"), Style::default().fg(Color::DarkGray)));
                }
            }

            all_lines.push(ratatui::text::Line::from(header_spans));

            // Content with wrapping
            let clean = strip_ansi(&msg.content);
            let indent = 2;
            let available = content_width.saturating_sub(indent);

            if msg.role == ChatRole::Tool && self.collapsed_tools {
                let first_line = clean.lines().next().unwrap_or("");
                let total_lines = clean.lines().count();
                if total_lines <= 1 {
                    all_lines.push(ratatui::text::Line::from(format!("  {first_line}")));
                } else {
                    all_lines.push(ratatui::text::Line::from(vec![
                        Span::raw(format!("  {first_line} ")),
                        Span::styled(format!("[+{total_lines}]"), Style::default().fg(Color::DarkGray)),
                    ]));
                }
            } else {
                for content_line in clean.lines() {
                    let wrapped = wrap_line(content_line, available);
                    for w in &wrapped {
                        all_lines.push(ratatui::text::Line::from(format!("  {w}")));
                    }
                }
            }

            all_lines.push(ratatui::text::Line::from(""));
        }

        self.committed_count = commit_end;
        let height = all_lines.len() as u16;
        (all_lines, height)
    }

    /// Reset committed count (e.g. after rewind or clear).
    pub fn reset_committed(&mut self) {
        self.committed_count = 0;
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

// ── Helper types and functions ──────────────────────────────────────────

/// Markdown segment: plain text, header, or fenced code block.
pub(super) enum MdSegment {
    /// Regular text lines
    Text(Vec<String>),
    /// Markdown header (## Header)
    Header { level: usize, text: String },
    /// Fenced code block with optional language tag
    CodeBlock { lang: Option<String>, code: String },
}

/// Parse content into markdown segments (text and code blocks).
pub(super) fn parse_markdown_segments(content: &str) -> Vec<MdSegment> {
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
            if header_level > 0 && header_level <= 6 {
                let rest = &line[header_level..];
                if rest.starts_with(' ') {
                    // Flush accumulated text before the header
                    if !current_text.is_empty() {
                        segments.push(MdSegment::Text(std::mem::take(&mut current_text)));
                    }
                    let header_text = rest.strip_prefix(' ').unwrap_or(rest).trim().to_string();
                    segments.push(MdSegment::Header { level: header_level, text: header_text });
                } else {
                    current_text.push(line.to_string());
                }
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

/// Wrap a line of text to fit within `max_chars`, returning multiple lines.
/// Word-boundary wrapping with mid-word fallback for long unbroken strings.
pub(crate) fn wrap_line(s: &str, max_chars: usize) -> Vec<String> {
    if max_chars == 0 {
        return if s.is_empty() { vec![String::new()] } else { vec![s.to_string()] };
    }
    if s.chars().count() <= max_chars {
        return if s.is_empty() { vec![String::new()] } else { vec![s.to_string()] };
    }

    let mut lines = Vec::new();
    let mut current = String::new();
    let mut len = 0usize;

    for word in s.split_whitespace() {
        let wlen = word.chars().count();
        if len == 0 {
            // First word on this line — if it exceeds max_chars, break it mid-word
            if wlen > max_chars {
                let chars: Vec<char> = word.chars().collect();
                let mut pos = 0;
                while pos < chars.len() {
                    let end = std::cmp::min(pos + max_chars, chars.len());
                    let chunk: String = chars[pos..end].iter().collect();
                    if pos + max_chars < chars.len() {
                        lines.push(chunk);
                    } else {
                        current = chunk;
                        len = current.chars().count();
                    }
                    pos = end;
                }
            } else {
                current.push_str(word);
                len = wlen;
            }
        } else if len + 1 + wlen <= max_chars {
            current.push(' ');
            current.push_str(word);
            len += 1 + wlen;
        } else if wlen > max_chars {
            // Word too long even on a new line — flush current, then break mid-word
            lines.push(std::mem::take(&mut current));
            len = 0;
            let chars: Vec<char> = word.chars().collect();
            let mut pos = 0;
            while pos < chars.len() {
                let end = std::cmp::min(pos + max_chars, chars.len());
                let chunk: String = chars[pos..end].iter().collect();
                if pos + max_chars < chars.len() {
                    lines.push(chunk);
                } else {
                    current = chunk;
                    len = current.chars().count();
                }
                pos = end;
            }
        } else {
            lines.push(std::mem::take(&mut current));
            current.push_str(word);
            len = wlen;
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

/// Wrap syntax-highlighted code spans to fit within `max_chars`.
/// Splits at character boundaries, preserving span styling.
/// Returns one or more lines, each a Vec of Spans.
fn wrap_code_spans(spans: &[Span<'static>], max_chars: usize) -> Vec<Vec<Span<'static>>> {
    if max_chars == 0 || spans.is_empty() {
        return vec![spans.to_vec()];
    }

    let total: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    if total <= max_chars {
        return vec![spans.to_vec()];
    }

    let mut result: Vec<Vec<Span<'static>>> = Vec::new();
    let mut current_line: Vec<Span<'static>> = Vec::new();
    let mut remaining = max_chars;

    for span in spans {
        let char_vec: Vec<char> = span.content.chars().collect();
        let char_count = char_vec.len();

        if char_count <= remaining {
            current_line.push(span.clone());
            remaining -= char_count;
            continue;
        }

        // Split this span: take what fits
        if remaining > 0 {
            let head: String = char_vec[..remaining].iter().collect();
            current_line.push(Span::styled(head, span.style));
        }
        result.push(std::mem::take(&mut current_line));

        // Process remaining chars in max_chars chunks
        let mut pos = remaining;
        while pos < char_count {
            let end = std::cmp::min(pos + max_chars, char_count);
            let chunk: String = char_vec[pos..end].iter().collect();
            result.push(vec![Span::styled(chunk, span.style)]);
            pos = end;
        }
        remaining = max_chars;
    }

    if !current_line.is_empty() {
        result.push(current_line);
    }

    if result.is_empty() {
        result.push(spans.to_vec());
    }
    result
}

/// Truncate a string to fit within `max_chars` characters, appending "…" if truncated.
pub(crate) fn truncate_to(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else if max_chars > 1 {
        let truncated: String = s.chars().take(max_chars - 1).collect();
        format!("{truncated}…")
    } else {
        "…".to_string()
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
    msg_matches: Option<&Vec<(usize, usize, usize)>>,
    _focused_match_idx: Option<usize>,
    _is_first_line: bool,
) -> Vec<Span<'static>> {
    if query.is_empty() || text.is_empty() {
        return parse_inline_formatting(text, base_color);
    }

    let query_lower = query.to_lowercase();
    let text_lower = text.to_lowercase();

    // Find all match positions within this text snippet
    let mut match_positions: Vec<(usize, usize)> = Vec::new();
    let mut search_from = 0;
    while let Some(pos) = text_lower[search_from..].find(&query_lower) {
        let abs_pos = search_from + pos;
        match_positions.push((abs_pos, abs_pos + query.len()));
        search_from = abs_pos + 1;
        if search_from >= text_lower.len() {
            break;
        }
    }

    if match_positions.is_empty() {
        return parse_inline_formatting(text, base_color);
    }

    // Determine if any of these matches are the focused one
    // (We approximate by checking if msg_matches has a focused index)
    let has_any_match = msg_matches.is_some();

    let theme = Theme::detect();
    let mut spans = Vec::new();
    let mut last_end = 0;

    for (start, end) in &match_positions {
        // Text before the match
        if *start > last_end {
            let before = &text[last_end..*start];
            spans.extend(parse_inline_formatting(before, base_color));
        }

        // The matched text - highlighted
        let matched_text = &text[*start..*end];
        let highlight_style = if has_any_match {
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

        last_end = *end;
    }

    // Remaining text after last match
    if last_end < text.len() {
        let remaining = &text[last_end..];
        spans.extend(parse_inline_formatting(remaining, base_color));
    }

    spans
}
