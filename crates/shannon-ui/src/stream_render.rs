//! Streaming markdown renderer with syntax highlighting.
//!
//! Parses partial (streaming) markdown content into segments and renders them
//! as styled ratatui [`Line`] objects.  Code blocks are highlighted via
//! **syntect** so that streamed assistant responses appear with full syntax
//! colouring as they arrive, not only after the message is complete.

use std::collections::HashMap;

use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

use crate::theme::Theme;

// ---------------------------------------------------------------------------
// Segment types
// ---------------------------------------------------------------------------

/// A parsed chunk of streaming markdown content.
#[derive(Debug, Clone)]
enum Segment {
    /// Plain text line (may contain inline formatting).
    Text(String),
    /// Fenced code block.
    CodeBlock {
        /// Language hint extracted from the opening fence (may be empty).
        lang: String,
        /// Raw code content accumulated so far.
        content: String,
    },
}

// ---------------------------------------------------------------------------
// StreamingRenderer
// ---------------------------------------------------------------------------

/// Renders streaming markdown into ratatui [`Line`] objects with syntax
/// highlighting for fenced code blocks.
///
/// ## Caching
///
/// Highlighted code lines are cached keyed by `(lang, content_hash)`.  During
/// streaming, only newly appended lines need re-highlighting — existing lines
/// are served from the cache.
///
/// ## Fallback
///
/// If syntect cannot find a syntax definition for the given language (or the
/// language string is empty), code is rendered as plain monospaced text.
#[derive(Debug)]
pub struct StreamingRenderer {
    /// Accumulated raw markdown content received so far.
    content: String,
    /// Parsed segments derived from `content`.
    segments: Vec<Segment>,
    /// Whether we are currently inside a fenced code block.
    in_code_block: bool,
    /// Language hint for the current open code block.
    current_lang: String,

    // -- syntect state (loaded once, reused) --
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,

    /// Cache: `(lang, content_hash)` -> highlighted ratatui lines.
    highlight_cache: HashMap<(String, u64), Vec<Line<'static>>>,
}

impl Default for StreamingRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamingRenderer {
    /// Create a new streaming renderer with default syntect assets.
    pub fn new() -> Self {
        Self {
            content: String::new(),
            segments: Vec::new(),
            in_code_block: false,
            current_lang: String::new(),
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme_set: ThemeSet::load_defaults(),
            highlight_cache: HashMap::new(),
        }
    }

    /// Append incremental streaming text and re-parse segments.
    pub fn append(&mut self, chunk: &str) {
        self.content.push_str(chunk);
        self.reparse();
    }

    /// Replace the entire content and re-parse.
    pub fn set_content(&mut self, content: &str) {
        self.content = content.to_string();
        self.reparse();
    }

    /// Get the raw accumulated content.
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Clear all state.
    pub fn clear(&mut self) {
        self.content.clear();
        self.segments.clear();
        self.in_code_block = false;
        self.current_lang.clear();
        self.highlight_cache.clear();
    }

    // -- Segment parsing -----------------------------------------------------

    /// Re-parse `content` into segments from scratch.
    ///
    /// This is called on every incremental update.  For typical streaming
    /// chunks (a few characters each) the cost is negligible compared to
    /// rendering.  A more advanced approach would parse incrementally, but
    /// that adds substantial complexity for marginal gain.
    fn reparse(&mut self) {
        self.segments.clear();
        self.in_code_block = false;
        self.current_lang.clear();

        let mut code_buffer = String::new();
        let mut code_lang = String::new();

        for line in self.content.lines() {
            let trimmed = line.trim_start();

            if self.in_code_block {
                if trimmed.starts_with("```") {
                    // Close code block.
                    self.segments.push(Segment::CodeBlock {
                        lang: code_lang.clone(),
                        content: code_buffer.clone(),
                    });
                    code_buffer.clear();
                    code_lang.clear();
                    self.in_code_block = false;
                } else {
                    if !code_buffer.is_empty() {
                        code_buffer.push('\n');
                    }
                    code_buffer.push_str(line);
                }
            } else if trimmed.starts_with("```") {
                // Open code block.
                self.in_code_block = true;
                code_lang = trimmed.trim_start_matches('`').trim().to_string();
                self.current_lang = code_lang.clone();
            } else {
                self.segments.push(Segment::Text(line.to_string()));
            }
        }

        // Handle an unclosed code block (common during streaming).
        if self.in_code_block && !code_buffer.is_empty() {
            self.segments.push(Segment::CodeBlock {
                lang: code_lang,
                content: code_buffer,
            });
        }
    }

    // -- Rendering -----------------------------------------------------------

    /// Render accumulated content as plain text (no highlighting).
    ///
    /// Fast path: returns `Span::raw()` lines suitable for simple display.
    pub fn render_streaming(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        for segment in &self.segments {
            match segment {
                Segment::Text(text) => {
                    lines.push(Line::from(Span::raw(text.clone())));
                }
                Segment::CodeBlock { content, .. } => {
                    for code_line in content.lines() {
                        lines.push(Line::from(Span::raw(code_line.to_string())));
                    }
                }
            }
        }

        // Ensure at least one line so the cursor always has somewhere to be.
        if lines.is_empty() {
            lines.push(Line::from(Span::raw("")));
        }

        let _ = width; // width used for future word-wrap; currently unused.
        lines
    }

    /// Render accumulated content with full syntax highlighting and theme
    /// colours.
    ///
    /// - Code blocks are highlighted via syntect (cached for performance).
    /// - Inline code markers (`...`) in text segments are styled with
    ///   `theme.secondary`.
    /// - All other text uses `theme.text`.
    pub fn render_streaming_highlighted(&mut self, width: u16, theme: &Theme) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = Vec::new();

        for segment in &self.segments {
            match segment {
                Segment::Text(text) => {
                    let spans = render_text_with_inline_code(text, theme);
                    lines.push(Line::from(spans));
                }
                Segment::CodeBlock { lang, content } => {
                    let cache_key = (lang.clone(), simple_hash(content));
                    if let Some(cached) = self.highlight_cache.get(&cache_key) {
                        lines.extend(cached.iter().cloned());
                    } else {
                        let highlighted = self.highlight_code(content, lang, theme);
                        // Store in cache for next frame.
                        self.highlight_cache.insert(cache_key, highlighted.clone());
                        lines.extend(highlighted);
                    }
                }
            }
        }

        if lines.is_empty() {
            lines.push(Line::from(Span::styled("", Style::default().fg(theme.text))));
        }

        let _ = width;
        lines
    }

    // -- Syntax highlighting -------------------------------------------------

    /// Highlight a code block using syntect.
    ///
    /// Falls back to plain monospaced text when the language is unknown.
    fn highlight_code(
        &self,
        code: &str,
        lang: &str,
        theme: &Theme,
    ) -> Vec<Line<'static>> {
        if code.is_empty() {
            return Vec::new();
        }

        let lang_lower = lang.trim().to_lowercase();

        if let Some(syntax) = self.syntax_set.find_syntax_by_token(&lang_lower) {
            let theme_name = theme.syntect_theme_name();
            let syn_theme = self.theme_set.themes.get(theme_name)
                .unwrap_or_else(|| &self.theme_set.themes["base16-eighties.dark"]);
            let mut highlighter = HighlightLines::new(syntax, syn_theme);
            let mut result = Vec::new();

            for line_str in code.lines() {
                match highlighter.highlight_line(line_str, &self.syntax_set) {
                    Ok(ranges) => {
                        let mut spans: Vec<Span<'static>> = Vec::new();
                        for (style, text) in ranges {
                            let fg = syntect_color_to_ratatui(style.foreground);
                            spans.push(Span::styled(text.to_string(), Style::default().fg(fg)));
                        }
                        result.push(Line::from(spans));
                    }
                    Err(_) => {
                        // Fallback to plain styled line on error.
                        result.push(Line::from(Span::styled(
                            line_str.to_string(),
                            Style::default().fg(theme.text),
                        )));
                    }
                }
            }

            // Drop trailing empty line produced by a trailing newline.
            if result.last().is_some_and(|l| l.spans.is_empty()) {
                result.pop();
            }

            result
        } else {
            // Unknown language: plain text with default code style.
            code.lines()
                .map(|l| {
                    Line::from(Span::styled(
                        l.to_string(),
                        Style::default().fg(theme.text),
                    ))
                })
                .collect()
        }
    }
}

// ---------------------------------------------------------------------------
// Inline code rendering
// ---------------------------------------------------------------------------

/// Parse a text line and return styled spans, detecting inline `code`
/// markers and styling them with `theme.secondary`.
fn render_text_with_inline_code(text: &str, theme: &Theme) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut chars = text.char_indices().peekable();
    let mut current = String::new();

    while let Some((_, ch)) = chars.next() {
        if ch == '`' {
            // Flush accumulated plain text.
            if !current.is_empty() {
                spans.push(Span::styled(
                    std::mem::take(&mut current),
                    Style::default().fg(theme.text),
                ));
            }
            // Collect until closing backtick.
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
                spans.push(Span::styled(code_text, Style::default().fg(theme.secondary)));
            } else {
                spans.push(Span::styled(
                    format!("`{code_text}"),
                    Style::default().fg(theme.text),
                ));
            }
        } else {
            current.push(ch);
        }
    }

    if !current.is_empty() {
        spans.push(Span::styled(current, Style::default().fg(theme.text)));
    }

    if spans.is_empty() {
        spans.push(Span::styled(String::new(), Style::default().fg(theme.text)));
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

/// Fast, non-cryptographic hash for cache keys.
fn simple_hash(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_theme() -> Theme {
        Theme::default_dark()
    }

    #[test]
    fn streaming_renderer_new() {
        let r = StreamingRenderer::new();
        assert!(r.content.is_empty());
        assert!(r.segments.is_empty());
    }

    #[test]
    fn streaming_renderer_append_text() {
        let mut r = StreamingRenderer::new();
        r.append("hello ");
        r.append("world");
        assert_eq!(r.content(), "hello world");
        // Two text segments (one per append, reparse joins into one line).
        assert_eq!(r.segments.len(), 1);
    }

    #[test]
    fn streaming_renderer_plain_render() {
        let mut r = StreamingRenderer::new();
        r.set_content("line one\nline two");
        let lines = r.render_streaming(80);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn streaming_renderer_highlighted_text() {
        let theme = test_theme();
        let mut r = StreamingRenderer::new();
        r.set_content("plain text");
        let lines = r.render_streaming_highlighted(80, &theme);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn streaming_renderer_highlighted_code_block() {
        let theme = test_theme();
        let mut r = StreamingRenderer::new();
        r.set_content("```rust\nfn main() {}\n```");
        let lines = r.render_streaming_highlighted(80, &theme);
        // The code block should produce at least one line.
        assert!(!lines.is_empty());
    }

    #[test]
    fn streaming_renderer_unknown_language_fallback() {
        let theme = test_theme();
        let mut r = StreamingRenderer::new();
        r.set_content("```no_such_lang\nsome code\n```");
        let lines = r.render_streaming_highlighted(80, &theme);
        assert!(!lines.is_empty());
        // Should fall back to plain text rendering.
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn streaming_renderer_unclosed_code_block() {
        let theme = test_theme();
        let mut r = StreamingRenderer::new();
        r.set_content("```rust\nfn main() {");
        let lines = r.render_streaming_highlighted(80, &theme);
        assert!(!lines.is_empty());
    }

    #[test]
    fn streaming_renderer_inline_code() {
        let theme = test_theme();
        let mut r = StreamingRenderer::new();
        r.set_content("use `cargo build` to compile");
        let lines = r.render_streaming_highlighted(80, &theme);
        assert_eq!(lines.len(), 1);
        // Should have 3 spans: plain, inline-code, plain.
        assert_eq!(lines[0].spans.len(), 3);
    }

    #[test]
    fn streaming_renderer_caches_highlights() {
        let theme = test_theme();
        let mut r = StreamingRenderer::new();
        r.set_content("```rust\nfn main() {}\n```");
        let _ = r.render_streaming_highlighted(80, &theme);
        // Second call should hit cache.
        let lines = r.render_streaming_highlighted(80, &theme);
        assert!(!lines.is_empty());
        assert_eq!(r.highlight_cache.len(), 1);
    }

    #[test]
    fn streaming_renderer_clear() {
        let mut r = StreamingRenderer::new();
        r.append("some text");
        r.clear();
        assert!(r.content.is_empty());
        assert!(r.segments.is_empty());
        assert!(r.highlight_cache.is_empty());
    }

    #[test]
    fn render_inline_code_basic() {
        let theme = test_theme();
        let spans = render_text_with_inline_code("use `code` here", &theme);
        assert_eq!(spans.len(), 3); // plain, code, plain
    }

    #[test]
    fn render_inline_code_no_code() {
        let theme = test_theme();
        let spans = render_text_with_inline_code("plain text", &theme);
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn render_inline_code_unclosed_backtick() {
        let theme = test_theme();
        let spans = render_text_with_inline_code("some `unclosed text", &theme);
        // The plain text before the backtick is one span, the unclosed
        // backtick + remaining text forms a second span.
        assert_eq!(spans.len(), 2);
    }

    #[test]
    fn simple_hash_deterministic() {
        let a = simple_hash("hello");
        let b = simple_hash("hello");
        assert_eq!(a, b);
        let c = simple_hash("world");
        assert_ne!(a, c);
    }

    #[test]
    fn streaming_renderer_empty_content() {
        let theme = test_theme();
        let mut r = StreamingRenderer::new();
        let lines = r.render_streaming_highlighted(80, &theme);
        // Should produce at least one (empty) line.
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn streaming_renderer_multiple_code_blocks() {
        let theme = test_theme();
        let mut r = StreamingRenderer::new();
        r.set_content("```rust\nfn a() {}\n```\nmiddle text\n```python\ndef b():\n    pass\n```");
        let lines = r.render_streaming_highlighted(80, &theme);
        // rust block (1 line) + middle text (1 line) + python block (2 lines)
        assert_eq!(lines.len(), 4);
    }
}
