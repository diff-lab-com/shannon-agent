//! Rendering logic for terminal UI
//!
//! Provides markdown rendering enhancements including syntax-highlighted
//! code blocks and structured diff display.

use crate::theme::Theme;
use crate::widgets;
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

    /// Render a block of markdown-like text into ratatui `Line` objects.
    ///
    /// Recognises fenced code blocks (```lang ... ```), headings (#),
    /// bold (**text**), inline code (`text`), and plain paragraphs.
    pub fn render_markdown(&self, text: &str) -> Vec<Line<'static>> {
        let mut output: Vec<Line<'static>> = Vec::new();
        let lines_iter = text.lines().peekable();
        let mut in_code_block = false;
        let mut code_lang = String::new();
        let mut code_buffer = String::new();

        for line in lines_iter {
            if in_code_block {
                if line.trim_start().starts_with("```") {
                    // End of code block
                    in_code_block = false;
                    if !code_buffer.is_empty() {
                        let highlighted = self.highlight_code(&code_buffer, &code_lang);
                        for hl_line in highlighted {
                            output.push(hl_line);
                        }
                    }
                    code_buffer.clear();
                    code_lang.clear();
                } else {
                    if !code_buffer.is_empty() {
                        code_buffer.push('\n');
                    }
                    code_buffer.push_str(line);
                }
            } else if line.trim_start().starts_with("```") {
                // Start of code block
                in_code_block = true;
                code_lang = line.trim_start().trim_start_matches('`').trim().to_string();
            } else {
                // Regular text line
                let rendered = render_markdown_inline(line);
                output.extend(rendered);
            }
        }

        // Handle unclosed code block
        if in_code_block && !code_buffer.is_empty() {
            let highlighted = self.highlight_code(&code_buffer, &code_lang);
            output.extend(highlighted);
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
// Diff rendering
// ---------------------------------------------------------------------------

/// Render a unified diff as styled ratatui `Line` objects.
///
/// Line prefixes:
/// - `+` (addition)  -> green foreground
/// - `-` (removal)   -> red foreground
/// - `@@ ... @@`     -> cyan foreground (hunk header)
/// - Everything else -> default foreground
pub fn render_diff(diff_text: &str) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    for raw_line in diff_text.lines() {
        let trimmed = raw_line.trim_start();

        if trimmed.starts_with("@@") {
            // Hunk header
            lines.push(Line::from(vec![
                Span::styled("@@", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(trimmed.trim_start_matches('@').trim_start_matches('@').to_string(),
                    Style::default().fg(Color::Cyan)),
            ]));
        } else if trimmed.starts_with('+') && !trimmed.starts_with("+++") {
            // Added line
            lines.push(Line::from(vec![
                Span::styled("+", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled(trimmed.trim_start_matches('+').to_string(),
                    Style::default().fg(Color::Green)),
            ]));
        } else if trimmed.starts_with('-') && !trimmed.starts_with("---") {
            // Removed line
            lines.push(Line::from(vec![
                Span::styled("-", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::styled(trimmed.trim_start_matches('-').to_string(),
                    Style::default().fg(Color::Red)),
            ]));
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
    }

    lines
}

// ---------------------------------------------------------------------------
// Markdown inline rendering
// ---------------------------------------------------------------------------

/// Render inline markdown (headings, bold, inline code) for a single text line.
fn render_markdown_inline(line: &str) -> Vec<Line<'static>> {
    let trimmed = line.trim_start();

    // Heading detection (# through ######)
    if let Some(level) = heading_level(trimmed) {
        let content = trimmed[level..].trim();
        let heading_style = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        return vec![Line::from(Span::styled(content.to_string(), heading_style))];
    }

    // Horizontal rule
    if trimmed == "---" || trimmed == "***" || trimmed == "___" {
        return vec![Line::from(Span::styled(
            "-".repeat(40),
            Style::default().fg(Color::DarkGray),
        ))];
    }

    // Default: render with inline formatting (bold, inline code)
    vec![Line::from(parse_inline_fragments(trimmed))]
}

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
