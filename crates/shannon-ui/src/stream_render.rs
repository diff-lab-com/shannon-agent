//! Streaming markdown renderer with incremental parsing
//!
//! Separates parsing from rendering for better performance during LLM streaming.
//! During streaming: plain text with minimal formatting.
//! After completion: full syntax highlighting via syntect.

/// A renderable segment parsed from markdown input
#[derive(Debug, Clone, PartialEq)]
pub enum Segment {
    Text { content: String },
    CodeBlock {
        lang: Option<String>,
        content: String,
        complete: bool,
    },
    InlineCode { content: String },
}

/// Incremental markdown parser that tracks parse state across chunks
#[derive(Debug)]
pub struct StreamingMarkdownParser {
    buffer: String,
    segments: Vec<Segment>,
    in_code_fence: bool,
    code_lang: Option<String>,
    code_buffer: String,
}

impl StreamingMarkdownParser {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            segments: Vec::new(),
            in_code_fence: false,
            code_lang: None,
            code_buffer: String::new(),
        }
    }

    pub fn append(&mut self, chunk: &str) {
        self.buffer.push_str(chunk);
        self.parse_incremental();
    }

    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    pub fn finalize(&mut self) {
        // Flush any remaining buffer as a text segment or append to code block
        if !self.buffer.is_empty() {
            if self.in_code_fence {
                self.code_buffer.push_str(&self.buffer);
            } else {
                self.segments.push(Segment::Text {
                    content: self.buffer.clone(),
                });
            }
            self.buffer.clear();
        }

        // If we're still in a code fence, flush it as incomplete
        if self.in_code_fence {
            self.segments.push(Segment::CodeBlock {
                lang: self.code_lang.clone(),
                content: self.code_buffer.clone(),
                complete: false,
            });
            self.code_buffer.clear();
            self.in_code_fence = false;
        }
    }

    fn parse_incremental(&mut self) {
        while let Some(newline_pos) = self.buffer.find('\n') {
            let line = self.buffer[..newline_pos].to_string();
            self.buffer = self.buffer[newline_pos + 1..].to_string();
            self.process_line(&line);
        }
    }

    fn process_line(&mut self, line: &str) {
        let trimmed = line.trim();

        if self.in_code_fence {
            if trimmed.starts_with("```") {
                self.segments.push(Segment::CodeBlock {
                    lang: self.code_lang.take(),
                    content: self.code_buffer.clone(),
                    complete: true,
                });
                self.code_buffer.clear();
                self.in_code_fence = false;
            } else {
                if !self.code_buffer.is_empty() {
                    self.code_buffer.push('\n');
                }
                self.code_buffer.push_str(line);
            }
        } else if trimmed.starts_with("```") {
            self.in_code_fence = true;
            self.code_lang = if trimmed.len() > 3 {
                Some(trimmed[3..].trim().to_string())
            } else {
                None
            };
            self.code_buffer.clear();
        } else {
            self.segments.push(Segment::Text {
                content: line.to_string(),
            });
        }
    }
}

/// Renderer that handles both streaming and final rendering
pub struct StreamingRenderer {
    parser: StreamingMarkdownParser,
    finalized: bool,
}

impl StreamingRenderer {
    pub fn new() -> Self {
        Self {
            parser: StreamingMarkdownParser::new(),
            finalized: false,
        }
    }

    pub fn on_chunk(&mut self, chunk: &str) {
        if !self.finalized {
            self.parser.append(chunk);
        }
    }

    pub fn on_complete(&mut self) {
        self.parser.finalize();
        self.finalized = true;
    }

    /// Render current state as plain text lines (fast, for streaming)
    pub fn render_streaming(&self, _width: u16) -> Vec<ratatui::text::Line<'static>> {
        let mut lines = Vec::new();
        for seg in self.parser.segments() {
            match seg {
                Segment::Text { content } => {
                    lines.push(ratatui::text::Line::from(ratatui::text::Span::raw(content.clone())));
                }
                Segment::CodeBlock { content, .. } => {
                    for l in content.lines() {
                        lines.push(ratatui::text::Line::from(ratatui::text::Span::raw(l.to_string())));
                    }
                }
                Segment::InlineCode { content } => {
                    lines.push(ratatui::text::Line::from(ratatui::text::Span::raw(content.clone())));
                }
            }
        }
        lines
    }

    pub fn is_finalized(&self) -> bool {
        self.finalized
    }
}

impl Default for StreamingMarkdownParser {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for StreamingRenderer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parser_plain_text() {
        let mut parser = StreamingMarkdownParser::new();
        parser.append("Hello\nWorld\n");
        parser.finalize();
        assert_eq!(parser.segments().len(), 2);
    }

    #[test]
    fn test_parser_code_block() {
        let mut parser = StreamingMarkdownParser::new();
        parser.append("Before\n```rust\nfn main() {}\n```\nAfter\n");
        parser.finalize();
        assert!(parser.segments().iter().any(|s| matches!(s, Segment::CodeBlock { complete: true, .. })));
    }

    #[test]
    fn test_streaming_renderer() {
        let mut renderer = StreamingRenderer::new();
        renderer.on_chunk("Hello ");
        renderer.on_chunk("World\n");
        renderer.on_complete();
        let lines = renderer.render_streaming(80);
        assert!(!lines.is_empty());
    }

    #[test]
    fn test_incomplete_code_fence() {
        let mut parser = StreamingMarkdownParser::new();
        parser.append("```rust\nfn foo() {}\n");
        parser.finalize();
        assert!(parser.segments().iter().any(|s| matches!(s, Segment::CodeBlock { .. })));
    }

    #[test]
    fn test_streaming_renderer_default() {
        let renderer = StreamingRenderer::default();
        assert!(!renderer.is_finalized());
    }
}
