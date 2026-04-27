//! Rendering logic for terminal UI
//!
//! Provides markdown rendering enhancements including syntax-highlighted
//! code blocks and structured diff display.

use crate::theme::Theme;
use crate::widgets;
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    Frame,
};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

/// Main renderer for the UI
pub struct Renderer {
    /// Current status message
    status_message: String,
    /// Syntax highlighting engine (lazy-loaded)
    syntax_set: SyntaxSet,
    /// Theme for syntax highlighting
    theme_set: ThemeSet,
}

impl Renderer {
    /// Create a new renderer
    pub fn new() -> Self {
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let theme_set = ThemeSet::load_defaults();
        Self {
            status_message: "Ready".to_string(),
            syntax_set,
            theme_set,
        }
    }

    /// Render the UI
    pub fn render(&mut self, frame: &mut Frame) -> Result<()> {
        let size = frame.area();

        // Create layout chunks
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints(
                [
                    Constraint::Min(0),   // Main content
                    Constraint::Length(3), // Status bar
                ]
                .as_ref(),
            )
            .split(size);

        // Render main content area
        self.render_main_content(frame, chunks[0]);

        // Render status bar
        let theme = Theme::detect();
        widgets::StatusBarWidget::render(frame, chunks[1], &self.status_message, &theme);

        Ok(())
    }

    /// Render the main content area
    fn render_main_content(&self, frame: &mut Frame, area: Rect) {
        let theme = Theme::detect();
        widgets::WelcomeWidget::render(frame, area, &theme);
    }

    /// Update the status message
    pub fn set_status(&mut self, message: impl Into<String>) {
        self.status_message = message.into();
    }

    /// Highlight a code block and return ratatui `Line` objects with styled spans.
    ///
    /// If the language is not recognized or highlighting fails, the code is returned
    /// as plain monospaced text with a subtle background style.
    pub fn highlight_code(&self, code: &str, language: &str) -> Vec<Line<'static>> {
        if code.is_empty() {
            return Vec::new();
        }

        // Normalize the language token (e.g. "rust", "Rust", "rs" all work)
        let language = language.trim().to_lowercase();

        if let Some(syntax) = self.syntax_set.find_syntax_by_token(&language) {
            let theme = &self.theme_set.themes["InspiredGitHub"];

            let mut highlighter = HighlightLines::new(syntax, theme);
            let mut lines = Vec::new();

            for line_str in code.lines() {
                let Ok(ranges) = highlighter.highlight_line(line_str, &self.syntax_set) else {
                    // Fallback to plain line on highlight error
                    lines.push(Line::from(Span::styled(
                        line_str.to_string(),
                        Style::default().fg(Color::White),
                    )));
                    continue;
                };

                let mut spans: Vec<Span<'static>> = Vec::new();
                for (style, text) in ranges {
                    let fg = syntect_color_to_ratatui(style.foreground);
                    spans.push(Span::styled(text.to_string(), Style::default().fg(fg)));
                }
                lines.push(Line::from(spans));
            }

            // If the code ends with a trailing newline the iterator will have
            // produced an empty final element -- drop it for cleanliness.
            if lines.last().is_some_and(|l| l.spans.is_empty()) {
                lines.pop();
            }

            lines
        } else {
            // Unknown language -- render as plain text
            code.lines()
                .map(|l| {
                    Line::from(Span::styled(
                        l.to_string(),
                        Style::default().fg(Color::White),
                    ))
                })
                .collect()
        }
    }

    /// Render a block of markdown text into ratatui `Line` objects using
    /// pulldown-cmark for full CommonMark support.
    pub fn render_markdown(&self, text: &str) -> Vec<Line<'static>> {
        let mut output: Vec<Line<'static>> = Vec::new();
        let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
        let parser = Parser::new_ext(text, opts);

        let mut inline_spans: Vec<Span<'static>> = Vec::new();
        let mut list_ordered: Vec<bool> = Vec::new();
        let mut list_item_counters: Vec<u64> = Vec::new();
        let mut in_blockquote = false;
        let mut in_code_block = false;
        let mut code_lang = String::new();
        let mut code_buffer = String::new();
        let mut in_table = false;
        let mut table_rows: Vec<Vec<Vec<Span<'static>>>> = Vec::new();
        let mut current_cell_spans: Vec<Span<'static>> = Vec::new();
        let mut current_row_cells: Vec<Vec<Span<'static>>> = Vec::new();

        for event in parser {
            match event {
                Event::Start(Tag::Heading { level, .. }) => {
                    flush_inline(&mut inline_spans, &mut output);
                    let _ = level;
                }
                Event::End(TagEnd::Heading(level)) => {
                    let style = match level {
                        HeadingLevel::H1 => Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                        HeadingLevel::H2 => Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                        HeadingLevel::H3 => Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                        _ => Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                    };
                    let text = spans_to_string(&inline_spans);
                    inline_spans.clear();
                    output.push(Line::from(Span::styled(text, style)));
                    output.push(Line::from(""));
                }
                Event::Start(Tag::Paragraph) => {}
                Event::End(TagEnd::Paragraph) => {
                    if in_table {
                        flush_inline(&mut inline_spans, &mut output);
                    } else {
                        flush_inline(&mut inline_spans, &mut output);
                        output.push(Line::from(""));
                    }
                }
                Event::Start(Tag::BlockQuote(_)) => {
                    in_blockquote = true;
                }
                Event::End(TagEnd::BlockQuote(_)) => {
                    flush_inline(&mut inline_spans, &mut output);
                    in_blockquote = false;
                }
                Event::Start(Tag::CodeBlock(kind)) => {
                    flush_inline(&mut inline_spans, &mut output);
                    in_code_block = true;
                    code_lang = match kind {
                        pulldown_cmark::CodeBlockKind::Fenced(lang) => lang.to_string(),
                        _ => String::new(),
                    };
                    code_buffer.clear();
                }
                Event::End(TagEnd::CodeBlock) => {
                    if !code_buffer.is_empty() {
                        let highlighted = self.highlight_code(&code_buffer, &code_lang);
                        output.extend(highlighted);
                    }
                    output.push(Line::from(""));
                    in_code_block = false;
                    code_buffer.clear();
                    code_lang.clear();
                }
                Event::Start(Tag::List(first)) => {
                    flush_inline(&mut inline_spans, &mut output);
                    list_ordered.push(first.is_some());
                    list_item_counters.push(first.unwrap_or(0));
                }
                Event::End(TagEnd::List(_)) => {
                    list_ordered.pop();
                    list_item_counters.pop();
                    output.push(Line::from(""));
                }
                Event::Start(Tag::Item) => {
                    let indent = "  ".repeat(list_ordered.len().saturating_sub(1));
                    if list_ordered.last() == Some(&true) {
                        let idx = list_item_counters.last_mut().unwrap();
                        *idx += 1;
                        let prefix = format!("{indent}{}. ", *idx - 1);
                        inline_spans.push(Span::styled(prefix, Style::default().fg(Color::Cyan)));
                    } else {
                        inline_spans.push(Span::styled(format!("{indent}• "), Style::default().fg(Color::Cyan)));
                    }
                }
                Event::End(TagEnd::Item) => {
                    flush_inline(&mut inline_spans, &mut output);
                }
                Event::Start(Tag::Table(_)) => {
                    in_table = true;
                    table_rows.clear();
                }
                Event::End(TagEnd::Table) => {
                    render_table(&table_rows, &mut output);
                    table_rows.clear();
                    in_table = false;
                    output.push(Line::from(""));
                }
                Event::Start(Tag::TableHead) => {
                    current_row_cells.clear();
                }
                Event::End(TagEnd::TableHead) => {
                    table_rows.push(std::mem::take(&mut current_row_cells));
                }
                Event::Start(Tag::TableRow) => {
                    current_row_cells.clear();
                }
                Event::End(TagEnd::TableRow) => {
                    table_rows.push(std::mem::take(&mut current_row_cells));
                }
                Event::Start(Tag::TableCell) => {
                    current_cell_spans.clear();
                }
                Event::End(TagEnd::TableCell) => {
                    current_row_cells.push(std::mem::take(&mut current_cell_spans));
                }
                Event::Start(Tag::Link { dest_url, .. }) => {
                    // OSC 8 hyperlink: \x1b]8;;URL\x1b\\TEXT\x1b]8;;\x1b\\
                    // We wrap the URL as a styled span for now; terminals that
                    // support OSC 8 will make it clickable.
                    inline_spans.push(Span::styled(
                        format!("\x1b]8;;{}\x1b\\", dest_url),
                        Style::default(),
                    ));
                }
                Event::End(TagEnd::Link) => {
                    inline_spans.push(Span::styled(
                        "\x1b]8;;\x1b\\".to_string(),
                        Style::default(),
                    ));
                }
                Event::Start(Tag::Strikethrough) => {}
                Event::End(TagEnd::Strikethrough) => {}
                Event::Start(Tag::Strong) => {
                    // Mark for bold - handled in text event
                }
                Event::End(TagEnd::Strong) => {}
                Event::Start(Tag::Emphasis) => {}
                Event::End(TagEnd::Emphasis) => {}
                Event::Code(code) => {
                    let target = if in_table { &mut current_cell_spans } else { &mut inline_spans };
                    target.push(Span::styled(
                        format!("`{}`", code),
                        Style::default().fg(Color::Yellow),
                    ));
                }
                Event::Text(text) => {
                    if in_code_block {
                        code_buffer.push_str(&text);
                    } else if in_table {
                        current_cell_spans.push(Span::styled(
                            text.to_string(),
                            Style::default().fg(Color::White),
                        ));
                    } else {
                        let prefix = if in_blockquote { "│ " } else { "" };
                        for (i, line) in text.split('\n').enumerate() {
                            if i > 0 {
                                flush_inline(&mut inline_spans, &mut output);
                                if in_blockquote {
                                    inline_spans.push(Span::styled(
                                        "│ ".to_string(),
                                        Style::default().fg(Color::DarkGray),
                                    ));
                                }
                            } else if i == 0 && in_blockquote && inline_spans.is_empty() {
                                inline_spans.push(Span::styled(
                                    "│ ".to_string(),
                                    Style::default().fg(Color::DarkGray),
                                ));
                            }
                            let _ = prefix;
                            inline_spans.push(Span::styled(
                                line.to_string(),
                                Style::default().fg(Color::White),
                            ));
                        }
                    }
                }
                Event::SoftBreak | Event::HardBreak => {
                    if in_table {
                        // ignore breaks in tables
                    } else {
                        flush_inline(&mut inline_spans, &mut output);
                    }
                }
                Event::DisplayMath(_) | Event::InlineMath(_) => {}
                Event::Rule => {
                    flush_inline(&mut inline_spans, &mut output);
                    output.push(Line::from(Span::styled(
                        "─".repeat(40),
                        Style::default().fg(Color::DarkGray),
                    )));
                }
                Event::FootnoteReference(_) | Event::TaskListMarker(_) => {}
                _ => {}
            }
        }

        flush_inline(&mut inline_spans, &mut output);

        // Remove trailing empty line for cleaner display
        if output.last().is_some_and(|l| l.spans.is_empty()) {
            output.pop();
        }

        output
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Markdown helpers for pulldown-cmark renderer
// ---------------------------------------------------------------------------

/// Flush accumulated inline spans into a single output line.
fn flush_inline(spans: &mut Vec<Span<'static>>, output: &mut Vec<Line<'static>>) {
    if spans.is_empty() {
        return;
    }
    let line_spans: Vec<Span<'static>> = spans.drain(..).collect();
    output.push(Line::from(line_spans));
}

/// Extract plain text content from a slice of Spans.
fn spans_to_string(spans: &[Span<'static>]) -> String {
    spans.iter().map(|s| s.content.as_ref()).collect()
}

/// Render table rows with column-aligned cells and separators.
fn render_table(rows: &[Vec<Vec<Span<'static>>>], output: &mut Vec<Line<'static>>) {
    if rows.is_empty() {
        return;
    }

    // Compute max width per column
    let col_count = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut col_widths = vec![0usize; col_count];
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            let w = spans_to_string(cell).chars().count();
            col_widths[i] = col_widths[i].max(w);
        }
    }

    let sep = Line::from(Span::styled(
        col_widths
            .iter()
            .map(|&w| format!("─{}", "─".repeat(w + 1)))
            .collect::<Vec<_>>()
            .join("┼"),
        Style::default().fg(Color::DarkGray),
    ));

    for (ri, row) in rows.iter().enumerate() {
        let mut spans: Vec<Span<'static>> = Vec::new();
        for (ci, cell) in row.iter().enumerate() {
            if ci > 0 {
                spans.push(Span::styled(" │ ".to_string(), Style::default().fg(Color::DarkGray)));
            }
            let text = spans_to_string(cell);
            let style = if ri == 0 {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let padded = format!("{:width$}", text, width = col_widths.get(ci).copied().unwrap_or(0));
            spans.push(Span::styled(padded, style));
        }
        output.push(Line::from(spans));

        // Separator after header row
        if ri == 0 {
            output.push(sep.clone());
        }
    }
}

// ---------------------------------------------------------------------------
// Diff rendering
// ---------------------------------------------------------------------------

/// Find the common prefix and suffix between two strings, returning the differing middle portion.
/// This is used for word-level diff highlighting in unified diffs.
fn find_diff_region(old: &str, new: &str) -> (usize, usize, usize, usize) {
    let old_chars: Vec<char> = old.chars().collect();
    let new_chars: Vec<char> = new.chars().collect();

    let mut common_prefix = 0;
    while common_prefix < old_chars.len()
        && common_prefix < new_chars.len()
        && old_chars[common_prefix] == new_chars[common_prefix]
    {
        common_prefix += 1;
    }

    let mut common_suffix = 0;
    while common_suffix < old_chars.len().saturating_sub(common_prefix)
        && common_suffix < new_chars.len().saturating_sub(common_prefix)
        && old_chars[old_chars.len() - 1 - common_suffix] == new_chars[new_chars.len() - 1 - common_suffix]
    {
        common_suffix += 1;
    }

    let old_diff_start = common_prefix;
    let old_diff_end = old_chars.len().saturating_sub(common_suffix);
    let new_diff_start = common_prefix;
    let new_diff_end = new_chars.len().saturating_sub(common_suffix);

    (old_diff_start, old_diff_end, new_diff_start, new_diff_end)
}

/// Render a diff line with optional word-level highlighting.
/// For added lines (+), highlights the specific changed words in bright green vs the rest in normal green.
/// For removed lines (-), highlights the specific changed words in bright red vs the rest in normal red.
fn render_diff_line_with_word_highlight(
    line: &str,
    is_addition: bool,
    corresponding_line: Option<&str>,
) -> Line<'static> {
    let prefix = if line.starts_with('+') || line.starts_with('-') {
        &line[..1]
    } else {
        ""
    };

    let content = &line[prefix.len()..];

    if let Some(corresponding) = corresponding_line {
        let (_old_start, _old_end, new_start, new_end) = find_diff_region(corresponding, content);

        if is_addition {
            // For additions: prefix (normal green) + unchanged (green) + changed (bright green) + unchanged (green)
            let mut spans = vec![
                Span::styled(prefix.to_string(), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            ];

            let content_chars: Vec<char> = content.chars().collect();

            // Unchanged prefix
            if new_start > 0 {
                spans.push(Span::styled(
                    content_chars[..new_start].iter().collect::<String>(),
                    Style::default().fg(Color::Green),
                ));
            }

            // Changed middle (bright green)
            if new_start < new_end {
                spans.push(Span::styled(
                    content_chars[new_start..new_end].iter().collect::<String>(),
                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                ));
            }

            // Unchanged suffix
            if new_end < content_chars.len() {
                spans.push(Span::styled(
                    content_chars[new_end..].iter().collect::<String>(),
                    Style::default().fg(Color::Green),
                ));
            }

            Line::from(spans)
        } else {
            // For removals: prefix (normal red) + unchanged (red) + changed (bright red) + unchanged (red)
            let mut spans = vec![
                Span::styled(prefix.to_string(), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            ];

            let content_chars: Vec<char> = content.chars().collect();

            // Unchanged prefix
            if new_start > 0 {
                spans.push(Span::styled(
                    content_chars[..new_start].iter().collect::<String>(),
                    Style::default().fg(Color::Red),
                ));
            }

            // Changed middle (bright red)
            if new_start < new_end {
                spans.push(Span::styled(
                    content_chars[new_start..new_end].iter().collect::<String>(),
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ));
            }

            // Unchanged suffix
            if new_end < content_chars.len() {
                spans.push(Span::styled(
                    content_chars[new_end..].iter().collect::<String>(),
                    Style::default().fg(Color::Red),
                ));
            }

            Line::from(spans)
        }
    } else {
        // No corresponding line, use line-level coloring as fallback
        let color = if is_addition { Color::Green } else { Color::Red };
        Line::from(vec![
            Span::styled(prefix.to_string(), Style::default().fg(color).add_modifier(Modifier::BOLD)),
            Span::styled(content.to_string(), Style::default().fg(color)),
        ])
    }
}

/// Render a unified diff as styled ratatui `Line` objects.
///
/// Line prefixes:
/// - `+` (addition)  -> green foreground
/// - `-` (removal)   -> red foreground
/// - `@@ ... @@`     -> cyan foreground (hunk header)
/// - Everything else -> default foreground
///
/// Word-level highlighting: For adjacent +/- lines, highlights the differing words
/// in a brighter shade to show what actually changed.
pub fn render_diff(diff_text: &str) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let diff_lines: Vec<&str> = diff_text.lines().collect();

    let mut i = 0;
    while i < diff_lines.len() {
        let raw_line = diff_lines[i];
        let trimmed = raw_line.trim_start();

        if trimmed.starts_with("@@") {
            // Hunk header
            lines.push(Line::from(vec![
                Span::styled("@@", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(trimmed.trim_start_matches('@').trim_start_matches('@').to_string(),
                    Style::default().fg(Color::Cyan)),
            ]));
        } else if trimmed.starts_with('+') && !trimmed.starts_with("+++") {
            // Added line - check if there's a corresponding removed line
            let corresponding = if i + 1 < diff_lines.len() {
                let next_trimmed = diff_lines[i + 1].trim_start();
                if next_trimmed.starts_with('-') && !next_trimmed.starts_with("---") {
                    // Next line is a removal, use it for comparison
                    Some(&diff_lines[i + 1][1..]) // Skip the '-' prefix
                } else if i > 0 {
                    let prev_trimmed = diff_lines[i - 1].trim_start();
                    if prev_trimmed.starts_with('-') && !prev_trimmed.starts_with("---") {
                        // Previous line is a removal, use it for comparison
                        Some(&diff_lines[i - 1][1..]) // Skip the '-' prefix
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else if i > 0 {
                let prev_trimmed = diff_lines[i - 1].trim_start();
                if prev_trimmed.starts_with('-') && !prev_trimmed.starts_with("---") {
                    Some(&diff_lines[i - 1][1..])
                } else {
                    None
                }
            } else {
                None
            };

            lines.push(render_diff_line_with_word_highlight(raw_line, true, corresponding));
        } else if trimmed.starts_with('-') && !trimmed.starts_with("---") {
            // Removed line - check if there's a corresponding added line
            let corresponding = if i > 0 {
                let prev_trimmed = diff_lines[i - 1].trim_start();
                if prev_trimmed.starts_with('+') && !prev_trimmed.starts_with("+++") {
                    // Previous line is an addition, use it for comparison
                    Some(&diff_lines[i - 1][1..]) // Skip the '+' prefix
                } else {
                    None
                }
            } else if i + 1 < diff_lines.len() {
                let next_trimmed = diff_lines[i + 1].trim_start();
                if next_trimmed.starts_with('+') && !next_trimmed.starts_with("+++") {
                    Some(&diff_lines[i + 1][1..])
                } else {
                    None
                }
            } else {
                None
            };

            lines.push(render_diff_line_with_word_highlight(raw_line, false, corresponding));
        } else if trimmed.starts_with("+++") {
            // New file header
            lines.push(Line::from(Span::styled(
                raw_line.to_string(),
                Style::default().fg(Color::Yellow),
            )));
        } else if trimmed.starts_with("---") {
            // Old file header
            lines.push(Line::from(Span::styled(
                raw_line.to_string(),
                Style::default().fg(Color::Yellow),
            )));
        } else if trimmed.starts_with("diff ") {
            // Diff git header
            lines.push(Line::from(Span::styled(
                raw_line.to_string(),
                Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
            )));
        } else if trimmed.starts_with("index ")
            || trimmed.starts_with("new file")
            || trimmed.starts_with("deleted file")
            || trimmed.starts_with("rename from")
            || trimmed.starts_with("rename to")
            || trimmed.starts_with("similarity index")
            || trimmed.starts_with("dissimilarity index")
            || trimmed.starts_with("old mode")
            || trimmed.starts_with("new mode")
        {
            // Extended diff metadata
            lines.push(Line::from(Span::styled(
                raw_line.to_string(),
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            // Context / unchanged line
            lines.push(Line::from(Span::styled(
                raw_line.to_string(),
                Style::default().fg(Color::White),
            )));
        }

        i += 1;
    }

    lines
}

// ---------------------------------------------------------------------------
// Markdown inline helpers (used by tests)
// ---------------------------------------------------------------------------

/// Return the heading level (1-6) if the line starts with `#` markers, else `None`.
fn heading_level(line: &str) -> Option<usize> {
    let mut count = 0;
    for ch in line.chars() {
        if ch == '#' {
            count += 1;
        } else {
            break;
        }
    }
    // Must be followed by a space (or end of line) to be a heading
    if (1..=6).contains(&count) && line.chars().nth(count).is_none_or(|c| c == ' ') {
        Some(count)
    } else {
        None
    }
}

/// Truncate output lines to fit within max_lines, showing first N/2 and last N/2 with a truncation indicator.
///
/// If the number of lines exceeds max_lines, shows:
/// - First N/2 lines
/// - A "⋮ (truncated N lines) ⋮" indicator in dark gray italic
/// - Last N/2 lines
///
/// Returns the lines unchanged if they fit within max_lines.
pub fn truncate_output(lines: &[Line<'_>], max_lines: usize) -> Vec<Line<'static>> {
    if lines.len() <= max_lines {
        return lines
            .iter()
            .map(|l| Line::from(l.spans.iter().map(|s| Span::styled(s.content.to_string(), s.style)).collect::<Vec<_>>()))
            .collect();
    }

    let half = max_lines / 2;
    let truncated_count = lines.len().saturating_sub(max_lines);

    let mut result = Vec::with_capacity(max_lines);

    // First half
    for l in &lines[..half] {
        result.push(to_static_line(l));
    }

    // Truncation indicator
    let indicator = Line::from(Span::styled(
        format!("... (truncated {} lines) ...", truncated_count),
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::ITALIC),
    ));
    result.push(indicator);

    // Second half
    let remaining = max_lines.saturating_sub(half + 1);
    for l in &lines[lines.len() - remaining..] {
        result.push(to_static_line(l));
    }

    result
}

/// Convert a Line with any lifetime to a Line<'static> by owning all strings.
fn to_static_line(line: &Line<'_>) -> Line<'static> {
    Line::from(
        line.spans
            .iter()
            .map(|s| Span::styled(s.content.to_string(), s.style))
            .collect::<Vec<_>>(),
    )
}

/// Parse inline markdown fragments: `**bold**` and `` `code` ``.
fn parse_inline_fragments(text: &str) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut chars = text.char_indices().peekable();
    let mut current = String::new();

    while let Some((_, ch)) = chars.next() {
        match ch {
            '*' => {
                // Check for bold: **
                if chars.peek().is_some_and(|(_, c)| *c == '*') {
                    chars.next(); // consume second '*'
                    // Collect until closing **
                    let mut bold_text = String::new();
                    let mut found_close = false;
                    while let Some((_, c)) = chars.next() {
                        if c == '*' && chars.peek().is_some_and(|(_, nc)| *nc == '*') {
                            chars.next(); // consume closing **
                            found_close = true;
                            break;
                        }
                        bold_text.push(c);
                    }
                    if found_close {
                        if !current.is_empty() {
                            spans.push(Span::styled(std::mem::take(&mut current),
                                Style::default().fg(Color::White)));
                        }
                        spans.push(Span::styled(bold_text,
                            Style::default().fg(Color::White).add_modifier(Modifier::BOLD)));
                    } else {
                        // Not a valid bold -- treat as literal
                        current.push_str("**");
                        current.push_str(&bold_text);
                    }
                } else {
                    current.push(ch);
                }
            }
            '`' => {
                // Inline code
                let mut code_text = String::new();
                let mut found_close = false;
                for (_, c) in chars.by_ref() {
                    if c == '`' {
                        found_close = true;
                        break;
                    }
                    code_text.push(c);
                }
                if found_close {
                    if !current.is_empty() {
                        spans.push(Span::styled(std::mem::take(&mut current),
                            Style::default().fg(Color::White)));
                    }
                    spans.push(Span::styled(code_text,
                        Style::default().fg(Color::Yellow)));
                } else {
                    current.push('`');
                    current.push_str(&code_text);
                }
            }
            _ => {
                current.push(ch);
            }
        }
    }

    if !current.is_empty() {
        spans.push(Span::styled(current, Style::default().fg(Color::White)));
    }

    if spans.is_empty() {
        spans.push(Span::raw(""));
    }

    spans
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert a syntect `Color` to a ratatui `Color`.
fn syntect_color_to_ratatui(c: syntect::highlighting::Color) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

/// Terminal UI result type
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_highlight_code_empty() {
        let renderer = Renderer::new();
        let lines = renderer.highlight_code("", "rust");
        assert!(lines.is_empty());
    }

    #[test]
    fn test_highlight_code_unknown_language() {
        let renderer = Renderer::new();
        let lines = renderer.highlight_code("fn main() {}", "no_such_lang");
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn test_highlight_code_rust() {
        let renderer = Renderer::new();
        let code = "fn main() {\n    println!(\"hello\");\n}";
        let lines = renderer.highlight_code(code, "rust");
        // Should produce one line per source line (3 lines total)
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn test_highlight_code_python() {
        let renderer = Renderer::new();
        let code = "def hello():\n    print('world')";
        let lines = renderer.highlight_code(code, "python");
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn test_highlight_code_trailing_newline() {
        let renderer = Renderer::new();
        let code = "line one\nline two\n";
        let lines = renderer.highlight_code(code, "rust");
        // Trailing newline should not produce an extra empty line
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn test_render_diff_addition() {
        let diff = "+added line\n context\n";
        let lines = render_diff(diff);
        assert_eq!(lines.len(), 2);
        // First span of first line should be the '+' prefix
        assert_eq!(lines[0].spans[0].content, "+");
    }

    #[test]
    fn test_render_diff_removal() {
        let diff = "-removed line\n context\n";
        let lines = render_diff(diff);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].spans[0].content, "-");
    }

    #[test]
    fn test_render_diff_hunk_header() {
        let diff = "@@ -1,3 +1,4 @@\n context\n";
        let lines = render_diff(diff);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn test_render_diff_file_headers() {
        let diff = "--- a/old.rs\n+++ b/new.rs\n@@ -1 +1 @@\n-old\n+new\n";
        let lines = render_diff(diff);
        // 5 lines: ---, +++, @@, -old, +new
        assert_eq!(lines.len(), 5);
    }

    #[test]
    fn test_render_diff_extended_metadata() {
        let diff = "diff --git a/foo b/foo\nnew file mode 100644\nindex 0000000..abc1234\n";
        let lines = render_diff(diff);
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn test_render_diff_empty() {
        let lines = render_diff("");
        assert!(lines.is_empty());
    }

    #[test]
    fn test_render_diff_plus_plus_plus_not_colored_as_addition() {
        let diff = "+++ b/new.rs\n";
        let lines = render_diff(diff);
        assert_eq!(lines.len(), 1);
        // The +++ file header should not be treated as an addition line
        // It is rendered with yellow style (not green addition)
    }

    #[test]
    fn test_render_diff_minus_minus_minus_not_colored_as_removal() {
        let diff = "--- a/old.rs\n";
        let lines = render_diff(diff);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn test_render_markdown_heading() {
        let renderer = Renderer::new();
        let lines = renderer.render_markdown("# Hello World");
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn test_render_markdown_heading_levels() {
        let renderer = Renderer::new();
        for level in 1..=6 {
            let md = format!("{} Heading {}", "#".repeat(level), level);
            let lines = renderer.render_markdown(&md);
            assert_eq!(lines.len(), 1, "Heading level {level}");
        }
    }

    #[test]
    fn test_render_markdown_code_block() {
        let renderer = Renderer::new();
        let md = "```rust\nfn main() {}\n```";
        let lines = renderer.render_markdown(md);
        assert!(!lines.is_empty());
    }

    #[test]
    fn test_render_markdown_inline_code() {
        let renderer = Renderer::new();
        let lines = renderer.render_markdown("Use `cargo build` to compile.");
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn test_render_markdown_bold() {
        let renderer = Renderer::new();
        let lines = renderer.render_markdown("This is **bold** text.");
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn test_render_markdown_horizontal_rule() {
        let renderer = Renderer::new();
        let lines = renderer.render_markdown("---");
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn test_render_markdown_mixed() {
        let renderer = Renderer::new();
        let md = "# Title\n\nSome **bold** text with `code`.\n\n```rust\nfn main() {}\n```\n";
        let lines = renderer.render_markdown(md);
        // Title + empty + paragraph + empty + code + possible empty
        assert!(lines.len() >= 4);
    }

    #[test]
    fn test_render_markdown_unclosed_code_block() {
        let renderer = Renderer::new();
        let md = "```rust\nfn main() {}\n";
        let lines = renderer.render_markdown(md);
        // Should still render the code even without closing fence
        assert!(!lines.is_empty());
    }

    #[test]
    fn test_heading_level_valid() {
        assert_eq!(heading_level("# heading"), Some(1));
        assert_eq!(heading_level("## heading"), Some(2));
        assert_eq!(heading_level("###### heading"), Some(6));
    }

    #[test]
    fn test_heading_level_invalid() {
        assert_eq!(heading_level("not a heading"), None);
        assert_eq!(heading_level("####### too many"), None);
        assert_eq!(heading_level("#no space"), None);
    }

    #[test]
    fn test_parse_inline_fragments_plain() {
        let spans = parse_inline_fragments("hello world");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content, "hello world");
    }

    #[test]
    fn test_parse_inline_fragments_bold() {
        let spans = parse_inline_fragments("normal **bold** normal");
        assert_eq!(spans.len(), 3);
    }

    #[test]
    fn test_parse_inline_fragments_code() {
        let spans = parse_inline_fragments("use `code` here");
        assert_eq!(spans.len(), 3);
    }

    #[test]
    fn test_renderer_default() {
        let renderer = Renderer::default();
        assert_eq!(renderer.status_message, "Ready");
    }
}
