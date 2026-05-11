//! Renderable trait and MessageCell abstraction for the Codex dual-layer architecture.
//!
//! Phase 1: Provides the `Renderable` trait with `render(area, buf, theme)` and
//! `desired_height(width)`, plus `MessageCell` which wraps a `ChatMessage` and
//! computes exact heights via `Paragraph::line_count(width)`.
//!
//! This module is wired into `ChatWidget` but does **not** replace the active
//! render path yet.  Phase 2 will switch rendering to use these cells.

use super::chat::{
    ChatMessage, ChatRole, MdSegment,
    parse_markdown_segments, parse_inline_formatting, highlight_search_in_text,
    wrap_line, highlight_code_cached, truncate_to,
};
use crate::tool_format::{strip_ansi, tool_category, ToolCategory};
use crate::theme::Theme;

use std::sync::atomic::{AtomicU16, Ordering};
use parking_lot::Mutex;
use ratatui::{
    layout::Rect,
    text::{Line, Span},
    style::{Modifier, Style},
    widgets::{Clear, Paragraph, Widget},
};
#[cfg(test)]
use ratatui::style::Color;

// ── Search highlighting ──────────────────────────────────────────────────

/// Search parameters for highlighting matches in rendered cells.
pub struct SearchParams<'a> {
    pub query: &'a str,
    /// All search matches: (message_index, char_start, char_end).
    /// Byte offsets are approximate (relative to lowercased content) and only
    /// used for identifying which cell contains the focused match via `cell_index`.
    pub matches: &'a [(usize, usize, usize)],
    /// Index into `matches` of the currently focused match.
    pub focused_idx: Option<usize>,
    /// Index of the cell being rendered (set per-cell by ColumnRenderable).
    pub cell_index: usize,
}

impl SearchParams<'_> {
    /// Whether the focused search match falls within this cell.
    pub fn focused_in_cell(&self) -> bool {
        self.focused_idx.is_some_and(|fi| {
            self.matches.get(fi).is_some_and(|&(mi, _, _)| mi == self.cell_index)
        })
    }
}

// ── Renderable trait ────────────────────────────────────────────────────

/// A cell that can render itself into a ratatui buffer and report its exact height.
pub trait Renderable {
    /// Render into the given area of the buffer.
    fn render(&self, area: Rect, buf: &mut ratatui::buffer::Buffer, theme: &Theme);

    /// Exact height needed at the given column width.
    fn desired_height(&self, width: u16) -> u16;

    /// Render with vertical scroll offset within the cell.
    ///
    /// When `desired_height(width) > area.height`, `scroll_y` specifies how many
    /// rows to skip from the top of the cell content. Delegates to ratatui's
    /// built-in `Paragraph::scroll((scroll_y, 0))`.
    fn render_scrolled(
        &self,
        area: Rect,
        buf: &mut ratatui::buffer::Buffer,
        theme: &Theme,
        scroll_y: u16,
    ) {
        let _ = scroll_y;
        self.render(area, buf, theme);
    }

    /// Render with search highlighting active.
    fn render_with_search(
        &self,
        area: Rect,
        buf: &mut ratatui::buffer::Buffer,
        theme: &Theme,
        search: &SearchParams<'_>,
        scroll_y: u16,
    ) {
        let _ = (search, scroll_y);
        self.render(area, buf, theme);
    }
}

// ── MessageCell ─────────────────────────────────────────────────────────

/// A single chat message cell implementing `Renderable`.
///
/// Wraps a `ChatMessage` and produces `Paragraph`-based rendering with
/// exact height via `Paragraph::line_count(width)`.
pub struct MessageCell {
    message: ChatMessage,
    collapsed: bool,
    /// Cached width (u16::MAX means no cache).
    cached_width: AtomicU16,
    /// Cached height (u16::MAX means no cache).
    cached_height: AtomicU16,
    /// Cached lines from last build_lines() call (avoids double-build per frame).
    cached_lines: Mutex<Option<(u16, Vec<Line<'static>>)>>,
}

impl MessageCell {
    pub fn new(message: ChatMessage, collapsed: bool) -> Self {
        Self {
            message,
            collapsed,
            cached_width: AtomicU16::new(u16::MAX),
            cached_height: AtomicU16::new(u16::MAX),
            cached_lines: Mutex::new(None),
        }
    }

    /// Invalidate cached height (e.g., on content change during streaming).
    pub fn invalidate_cache(&self) {
        self.cached_width.store(u16::MAX, Ordering::Relaxed);
        self.cached_height.store(u16::MAX, Ordering::Relaxed);
        *self.cached_lines.lock() = None;
    }

    /// Build styled lines for this message, with trailing blank line for spacing.
    pub fn lines(&self, width: u16, theme: &Theme) -> Vec<Line<'static>> {
        let mut l = self.build_lines(width, theme, None);
        if !l.is_empty() {
            l.push(Line::from(""));
        }
        l
    }

    /// Replace the message content (e.g., during streaming updates).
    pub fn set_message(&mut self, message: ChatMessage) {
        self.message = message;
        self.invalidate_cache();
    }

    /// Lightweight streaming update: only invalidates lines cache, preserves height
    /// cache when `has_newline` is false (line count hasn't changed).
    pub fn update_streaming(&mut self, content: String, has_newline: bool) {
        self.message.content = content;
        // Always clear lines cache (text changed, needs re-render)
        *self.cached_lines.lock() = None;
        if has_newline {
            // Structural change — height may have changed
            self.cached_width.store(u16::MAX, Ordering::Relaxed);
            self.cached_height.store(u16::MAX, Ordering::Relaxed);
        }
        // When no newline: height cache is preserved, avoids re-computing layout
    }

    /// Build the styled Lines for this message at the given width.
    ///
    /// When `search` is `Some`, text spans use `highlight_search_in_text` for
    /// match highlighting. Search only affects colors, not line count.
    fn build_lines(&self, width: u16, theme: &Theme, search: Option<&SearchParams<'_>>) -> Vec<Line<'static>> {
        // Return cached lines if width matches and no search override.
        // Search highlighting changes span colors, so bypass cache when active.
        if search.is_none() {
            let guard = self.cached_lines.lock();
            if let Some((cached_w, ref cached)) = *guard {
                if cached_w == width {
                    return cached.clone();
                }
            }
            drop(guard);
        }

        let msg = &self.message;
        let inner_width = width as usize;
        let mut lines: Vec<Line<'static>> = Vec::new();

        // ── Collapsed tool messages ──
        if msg.role == ChatRole::Tool && (self.collapsed || msg.folded) {
            let timestamp = msg.timestamp.format("%H:%M:%S").to_string();
            let clean_content = strip_ansi(&msg.content);
            let first_line = clean_content.lines().next().unwrap_or("");
            let tool_label = msg.tool_name.as_deref().unwrap_or("tool");

            let cat = tool_category(tool_label);
            let (icon, prefix, cat_color) = match cat {
                ToolCategory::Read => ("\u{25B8}", "", theme.tool_read),
                ToolCategory::Write => ("\u{270E}", "", theme.tool_write),
                ToolCategory::Search => ("\u{229B}", "", theme.tool_search),
                ToolCategory::Bash => ("", "$ ", theme.tool_bash),
                ToolCategory::Agent => ("\u{25C6}", "", theme.tool_read),
            };

            let display = if unicode_width::UnicodeWidthStr::width(first_line) > 80 {
                truncate_to(first_line, 80)
            } else {
                first_line.to_string()
            };

            lines.push(Line::from(vec![
                Span::styled(format!("[{timestamp}] "), Style::default().fg(theme.muted)),
                Span::styled(format!("{icon}{tool_label} "), Style::default().fg(cat_color).add_modifier(Modifier::BOLD)),
                Span::styled(prefix.to_string(), Style::default().fg(cat_color)),
                Span::styled(display, Style::default().fg(theme.text_dim)),
            ]));
            return lines;
        }

        // ── Role label line ──
        let (role_name, role_color) = match msg.role {
            ChatRole::User => ("You", theme.user_msg),
            ChatRole::Assistant => ("AI", theme.assistant_msg),
            ChatRole::System => ("System", theme.system_msg),
            ChatRole::Tool => ("Tool", theme.tool_msg),
        };

        let timestamp = msg.timestamp.format("%H:%M:%S").to_string();

        let display_name = if msg.role == ChatRole::Tool {
            msg.tool_name.as_deref().unwrap_or("Tool").to_string()
        } else {
            role_name.to_string()
        };

        lines.push(Line::from(vec![
            Span::styled(format!("[{timestamp}] "), Style::default().fg(theme.muted)),
            Span::styled(format!("{display_name} > "), Style::default().fg(role_color).add_modifier(Modifier::BOLD)),
        ]));

        // ── Specialized tool output rendering ──
        if msg.role == ChatRole::Tool && !self.collapsed && !msg.folded {
            let tool_label = msg.tool_name.as_deref().unwrap_or("tool");
            let tool_width = inner_width.saturating_sub(4).max(20);
            let cat = tool_category(tool_label);
            let content = strip_ansi(&msg.content);

            // Duration badge
            if let Some(dur) = msg.duration_secs {
                let dur_str = if dur >= 60.0 {
                    format!("{}m{:.0}s", dur as u64 / 60, dur % 60.0)
                } else {
                    format!("{dur:.1}s")
                };
                let (status_icon, status_color) = if msg.is_error {
                    ("\u{2717}", theme.error)
                } else {
                    ("\u{2713}", theme.success)
                };
                let mut badge_spans = vec![
                    Span::styled(format!("  {status_icon} "), Style::default().fg(status_color)),
                    Span::styled(dur_str, Style::default().fg(theme.muted)),
                ];
                // Exit code display
                if let Some(code) = msg.exit_code {
                    if code != 0 {
                        badge_spans.push(Span::styled(
                            format!(" exit={code}"),
                            Style::default().fg(theme.error),
                        ));
                    }
                }
                lines.push(Line::from(badge_spans));
            }

            // Error messages get a red-tinted rendering
            if msg.is_error {
                for raw_line in content.lines() {
                    if raw_line.trim().is_empty() { continue; }
                    let wrapped = wrap_line(raw_line, tool_width);
                    for wl in wrapped {
                        lines.push(Line::from(vec![
                            Span::styled("  ", Style::default()),
                            Span::styled(wl, Style::default().fg(theme.error)),
                        ]));
                    }
                }
                return lines;
            }

            // Diff output detection: colorize +/- lines
            let is_diff = content.lines().take(5).all(|l| {
                l.starts_with("+++") || l.starts_with("---") || l.starts_with("@@")
                    || l.starts_with('+') || l.starts_with('-') || l.starts_with(' ')
                    || l.is_empty()
            }) && content.lines().any(|l| l.starts_with("+++") || l.starts_with("@@"));

            if is_diff {
                for raw_line in content.lines() {
                    let (prefix, color) = if raw_line.starts_with('+') && !raw_line.starts_with("+++") {
                        ("+", theme.success)
                    } else if raw_line.starts_with('-') && !raw_line.starts_with("---") {
                        ("-", theme.error)
                    } else if raw_line.starts_with('@') {
                        ("@", theme.accent)
                    } else {
                        ("", theme.text_dim)
                    };
                    lines.push(Line::from(vec![
                        Span::styled(format!(" {prefix}"), Style::default().fg(color)),
                        Span::styled(
                            if prefix.is_empty() { raw_line.to_string() } else { raw_line[1..].to_string() },
                            Style::default().fg(color),
                        ),
                    ]));
                }
                return lines;
            }

            // Bash output: detect $ prefix lines and render command distinctly
            if cat == ToolCategory::Bash {
                let row_budget: usize = if cat == ToolCategory::Agent { 3 } else { 10 };
                let mut in_output = false;
                let all_lines: Vec<String> = content.lines()
                    .filter(|l| !l.trim().is_empty())
                    .flat_map(|raw_line| {
                        if raw_line.trim_start().starts_with('$') || raw_line.starts_with("> Using: bash") {
                            let cmd = raw_line.trim_start().trim_start_matches('$').trim_start();
                            vec![format!("  $ {cmd}")]
                        } else {
                            wrap_line(raw_line, tool_width.saturating_sub(2))
                        }
                    })
                    .collect();

                if all_lines.len() > row_budget {
                    let head = row_budget / 2;
                    let tail = row_budget - head;
                    for line in &all_lines[..head] {
                        lines.push(Line::from(Span::styled(line.clone(), Style::default().fg(theme.text_dim))));
                    }
                    let hidden = all_lines.len() - row_budget;
                    lines.push(Line::from(Span::styled(
                        format!("  ... +{hidden} lines (Alt+F to expand)"),
                        Style::default().fg(theme.muted),
                    )));
                    for line in &all_lines[all_lines.len().saturating_sub(tail)..] {
                        lines.push(Line::from(Span::styled(line.clone(), Style::default().fg(theme.text_dim))));
                    }
                } else {
                    for raw_line in content.lines() {
                        if raw_line.trim().is_empty() { continue; }
                        if raw_line.trim_start().starts_with('$') || raw_line.starts_with("> Using: bash") {
                            let cmd = raw_line.trim_start().trim_start_matches('$').trim_start();
                            lines.push(Line::from(vec![
                                Span::styled("  $ ", Style::default().fg(theme.tool_bash).add_modifier(Modifier::BOLD)),
                                Span::styled(cmd.to_string(), Style::default().fg(theme.tool_bash)),
                            ]));
                            in_output = true;
                        } else if in_output {
                            let wrapped = wrap_line(raw_line, tool_width.saturating_sub(2));
                            for wl in wrapped {
                                lines.push(Line::from(vec![
                                    Span::styled("  ", Style::default()),
                                    Span::styled(wl, Style::default().fg(theme.text_dim)),
                                ]));
                            }
                        } else {
                            let wrapped = wrap_line(raw_line, tool_width);
                            for wl in wrapped {
                                lines.push(Line::from(Span::styled(wl, Style::default().fg(theme.text_dim))));
                            }
                        }
                    }
                }
                return lines;
            }

            // Generic expanded tool: show content with row-aware truncation
            let row_budget: usize = if cat == ToolCategory::Agent { 3 } else { 10 };
            let all_lines: Vec<String> = content.lines()
                .filter(|l| !l.trim().is_empty())
                .flat_map(|l| wrap_line(l, tool_width))
                .collect();

            if all_lines.len() > row_budget {
                let head = row_budget / 2;
                let tail = row_budget - head;
                for line in &all_lines[..head] {
                    lines.push(Line::from(Span::styled(line.clone(), Style::default().fg(theme.text_dim))));
                }
                let hidden = all_lines.len() - row_budget;
                lines.push(Line::from(Span::styled(
                    format!("  ... +{hidden} lines (Alt+F to expand)"),
                    Style::default().fg(theme.muted),
                )));
                for line in &all_lines[all_lines.len().saturating_sub(tail)..] {
                    lines.push(Line::from(Span::styled(line.clone(), Style::default().fg(theme.text_dim))));
                }
            } else {
                for raw_line in content.lines() {
                    if raw_line.trim().is_empty() { continue; }
                    let wrapped = wrap_line(raw_line, tool_width);
                    for wl in wrapped {
                        lines.push(Line::from(Span::styled(wl, Style::default().fg(theme.text_dim))));
                    }
                }
            }
            return lines;
        }

        // ── Content lines ──
        let content = strip_ansi(&msg.content);
        let segments = parse_markdown_segments(&content);

        let prefix_str = format!("[{timestamp}] {display_name} > ");
        let prefix_len = unicode_width::UnicodeWidthStr::width(prefix_str.as_str());
        let text_width = inner_width.saturating_sub(prefix_len).max(20);
        let content_width = inner_width.max(20);

        for seg in &segments {
            match seg {
                MdSegment::Text(text_lines) => {
                    for raw_line in text_lines {
                        if raw_line.trim().is_empty() {
                            continue;
                        }
                        let wrapped = wrap_line(raw_line, text_width);
                        for wl in wrapped {
                            let spans = if let Some(sp) = search {
                                highlight_search_in_text(&wl, theme.text, sp.query, sp.focused_in_cell(), theme)
                            } else {
                                parse_inline_formatting(&wl, theme.text)
                            };
                            lines.push(Line::from(spans));
                        }
                    }
                }
                MdSegment::Header { level, text } => {
                    let prefix = "#".repeat(*level);
                    lines.push(Line::from(vec![
                        Span::styled(format!("{prefix} "), Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
                        Span::styled(text.clone(), Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
                    ]));
                }
                MdSegment::CodeBlock { lang, code } => {
                    let lang_display = lang.as_deref().unwrap_or("");
                    lines.push(Line::from(vec![
                        Span::styled("─".repeat(inner_width.min(60)), Style::default().fg(theme.border_dim)),
                    ]));
                    if !lang_display.is_empty() {
                        lines.push(Line::from(vec![
                            Span::styled(format!(" {lang_display}"), Style::default().fg(theme.muted)),
                        ]));
                    }

                    let highlighted = if let Some(l) = lang {
                        highlight_code_cached(code, l)
                    } else {
                        code.lines().map(|l| Line::from(l.to_string())).collect()
                    };

                    let code_lines: Vec<Line<'static>> = if highlighted.len() > 20 && msg.folded && msg.role == ChatRole::Tool {
                        let mut folded = Vec::with_capacity(16);
                        for line in &highlighted[..10] {
                            folded.push(line.clone());
                        }
                        let hidden = highlighted.len() - 15;
                        folded.push(Line::from(
                            Span::styled(format!("  ... {hidden} more lines (press 'o' to expand)"), Style::default().fg(theme.muted))
                        ));
                        for line in highlighted.iter().rev().take(5).rev() {
                            folded.push(line.clone());
                        }
                        folded
                    } else {
                        highlighted
                    };

                    lines.extend(code_lines);
                    lines.push(Line::from(vec![
                        Span::styled("─".repeat(inner_width.min(60)), Style::default().fg(theme.border_dim)),
                    ]));
                }
                MdSegment::UnorderedList(items) => {
                    // "  • " = 4 chars prefix, wrap to content_width - 4
                    let ul_width = content_width.saturating_sub(4).max(20);
                    for item in items {
                        let wrapped = wrap_line(item, ul_width);
                        for (i, wl) in wrapped.iter().enumerate() {
                            if i == 0 {
                                lines.push(Line::from(vec![
                                    Span::styled("  • ", Style::default().fg(theme.accent)),
                                    Span::styled(wl.clone(), Style::default().fg(theme.text)),
                                ]));
                            } else {
                                lines.push(Line::from(vec![
                                    Span::styled("    ", Style::default()),
                                    Span::styled(wl.clone(), Style::default().fg(theme.text)),
                                ]));
                            }
                        }
                    }
                }
                MdSegment::OrderedList(items) => {
                    for (idx, item) in items.iter().enumerate() {
                        let num = format!("{}. ", idx + 1);
                        // "  " + num prefix
                        let ol_width = content_width.saturating_sub(num.len() + 2).max(20);
                        let wrapped = wrap_line(item, ol_width);
                        for (i, wl) in wrapped.iter().enumerate() {
                            if i == 0 {
                                lines.push(Line::from(vec![
                                    Span::styled(format!("  {num}"), Style::default().fg(theme.accent)),
                                    Span::styled(wl.clone(), Style::default().fg(theme.text)),
                                ]));
                            } else {
                                let pad = " ".repeat(num.len() + 2);
                                lines.push(Line::from(vec![
                                    Span::styled(pad, Style::default()),
                                    Span::styled(wl.clone(), Style::default().fg(theme.text)),
                                ]));
                            }
                        }
                    }
                }
                MdSegment::Blockquote(bq_lines) => {
                    // "  │ " = 4 chars prefix
                    let bq_width = content_width.saturating_sub(4).max(20);
                    for line in bq_lines {
                        let wrapped = wrap_line(line, bq_width);
                        for (i, wl) in wrapped.iter().enumerate() {
                            let prefix = if i == 0 { "  │ " } else { "    " };
                            lines.push(Line::from(vec![
                                Span::styled(prefix.to_string(), Style::default().fg(theme.accent)),
                                Span::styled(wl.clone(), Style::default().fg(theme.text_dim).add_modifier(Modifier::ITALIC)),
                            ]));
                        }
                    }
                }
                MdSegment::HorizontalRule => {
                    lines.push(Line::from(vec![
                        Span::styled("─".repeat(inner_width.min(60)), Style::default().fg(theme.border_dim)),
                    ]));
                }
            }
        }

        // Image lines
        if let Some(ref imgs) = msg.image_lines {
            lines.extend(imgs.clone());
        }

        if lines.len() == 1 {
            // Only the role line — add an empty content indicator
            lines.push(Line::from(Span::styled("(empty)", Style::default().fg(theme.muted))));
        }

        // Cache for non-search builds
        if search.is_none() {
            *self.cached_lines.lock() = Some((width, lines.clone()));
        }

        lines
    }
}

impl Renderable for MessageCell {
    fn render(&self, area: Rect, buf: &mut ratatui::buffer::Buffer, theme: &Theme) {
        Clear.render(area, buf);

        let lines = self.lines(area.width, theme);
        let paragraph = Paragraph::new(lines);
        paragraph.render(area, buf);
    }

    fn render_scrolled(
        &self,
        area: Rect,
        buf: &mut ratatui::buffer::Buffer,
        theme: &Theme,
        scroll_y: u16,
    ) {
        Clear.render(area, buf);

        let lines = self.lines(area.width, theme);
        let paragraph = Paragraph::new(lines).scroll((scroll_y, 0));
        paragraph.render(area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        let cached_w = self.cached_width.load(Ordering::Relaxed);
        let cached_h = self.cached_height.load(Ordering::Relaxed);
        if cached_w == width && cached_h != u16::MAX {
            return cached_h;
        }

        // Theme only affects colors, not line count — default_dark is safe for height calc.
        let theme = Theme::default_dark();
        let lines = self.build_lines(width, &theme, None);

        // Use Paragraph::line_count for accurate height (handles remaining wrapping
        // of long code lines or headers that build_lines doesn't pre-wrap).
        let paragraph = Paragraph::new(lines);
        let height = paragraph.line_count(width) as u16 + 1; // +1 for inter-message spacing

        self.cached_width.store(width, Ordering::Relaxed);
        self.cached_height.store(height, Ordering::Relaxed);
        height
    }

    fn render_with_search(
        &self,
        area: Rect,
        buf: &mut ratatui::buffer::Buffer,
        theme: &Theme,
        search: &SearchParams<'_>,
        scroll_y: u16,
    ) {
        Clear.render(area, buf);

        let lines = self.build_lines(area.width, theme, Some(search));
        let paragraph = Paragraph::new(lines).scroll((scroll_y, 0));
        paragraph.render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::chat::ChatRole;

    fn test_message(role: ChatRole, content: &str) -> ChatMessage {
        ChatMessage {
            role,
            content: content.to_string(),
            timestamp: chrono::Utc::now(),
            image_lines: None,
            is_error: false,
            tool_name: None,
            start_time: None,
            duration_secs: None,
            spinner_frame: 0,
            folded: true,
            exit_code: None,
        }
    }

    #[test]
    fn test_message_cell_user_height() {
        let msg = test_message(ChatRole::User, "Hello world");
        let cell = MessageCell::new(msg, false);
        // Role line + content line = at least 2
        let h = cell.desired_height(80);
        assert!(h >= 2, "expected at least 2 lines, got {h}");
    }

    #[test]
    fn test_message_cell_empty_content() {
        let msg = test_message(ChatRole::Assistant, "");
        let cell = MessageCell::new(msg, false);
        let h = cell.desired_height(80);
        // Role line + "(empty)" line
        assert!(h >= 2, "expected at least 2 lines for empty content, got {h}");
    }

    #[test]
    fn test_message_cell_caching() {
        let msg = test_message(ChatRole::User, "Test");
        let cell = MessageCell::new(msg, false);
        let h1 = cell.desired_height(80);
        let h2 = cell.desired_height(80);
        assert_eq!(h1, h2, "cached height should be stable");

        cell.invalidate_cache();
        assert_eq!(cell.cached_width.load(Ordering::Relaxed), u16::MAX, "cache should be cleared");
    }

    #[test]
    fn test_message_cell_cache_invalidation_on_width_change() {
        let msg = test_message(ChatRole::User, "A longer message that might wrap at narrow widths");
        let cell = MessageCell::new(msg, false);
        let h_wide = cell.desired_height(80);
        let h_narrow = cell.desired_height(20);
        // Narrow width should produce more or equal lines
        assert!(h_narrow >= h_wide, "narrower width should have >= lines: {h_narrow} vs {h_wide}");
    }

    #[test]
    fn test_message_cell_collapsed_tool() {
        let mut msg = test_message(ChatRole::Tool, "line 1\nline 2\nline 3");
        msg.tool_name = Some("bash".to_string());
        let cell = MessageCell::new(msg, true);
        let h = cell.desired_height(80);
        assert_eq!(h, 2, "collapsed tool should be 1 line + spacing, got {h}");
    }

    #[test]
    fn test_message_cell_code_block() {
        let msg = test_message(ChatRole::Assistant, "```rust\nfn main() {}\n```");
        let cell = MessageCell::new(msg, false);
        let h = cell.desired_height(80);
        // Role line + top border + code line + bottom border
        assert!(h >= 4, "expected at least 4 lines for code block, got {h}");
    }

    #[test]
    fn test_parse_inline_formatting_plain() {
        let spans = parse_inline_formatting("hello world", Color::White);
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_parse_inline_formatting_bold() {
        let spans = parse_inline_formatting("**bold**", Color::White);
        assert_eq!(spans.len(), 1);
        assert!(spans[0].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_parse_inline_formatting_mixed() {
        let spans = parse_inline_formatting("hello **world** end", Color::White);
        assert!(spans.len() >= 3);
    }
}
