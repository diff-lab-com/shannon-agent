//! Helper functions for text extraction, token estimation, and code detection.

use crate::api::{ContentBlock, Message, MessageContent, ToolResultContent};
use std::collections::HashSet;

/// Extract all text content from a message
pub fn extract_text_content(msg: &Message) -> String {
    match &msg.content {
        MessageContent::Text(text) => text.clone(),
        MessageContent::Blocks(blocks) => {
            let mut parts = Vec::new();
            for block in blocks {
                match block {
                    ContentBlock::Text { text } => parts.push(text.clone()),
                    ContentBlock::ToolUse { name, input, .. } => {
                        parts.push(format!(
                            "[Tool: {}({})]",
                            name,
                            truncate_text(
                                &serde_json::to_string(input).unwrap_or_default(),
                                100
                            )
                        ));
                    }
                    ContentBlock::ToolResult { content, is_error, .. } => {
                        let prefix = if is_error.unwrap_or(false) {
                            "[Tool Error]"
                        } else {
                            "[Tool Result]"
                        };
                        let result_text = match content {
                            Some(ToolResultContent::Single(s)) => s.clone(),
                            Some(ToolResultContent::Multiple(blocks)) => {
                                let texts: Vec<&str> = blocks
                                    .iter()
                                    .filter_map(|b| match b {
                                        ContentBlock::Text { text } => Some(text.as_str()),
                                        _ => None,
                                    })
                                    .collect();
                                texts.join("\n")
                            }
                            None => String::new(),
                        };
                        parts.push(format!("{} {}", prefix, truncate_text(&result_text, 200)));
                    }
                    ContentBlock::Image { .. } => {
                        parts.push("[Image]".to_string());
                    }
                    ContentBlock::Thinking { .. } => {}
                }
            }
            parts.join("\n")
        }
    }
}

/// Truncate text to a maximum byte length, respecting UTF-8 boundaries.
pub fn truncate_text(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    // Find the last safe UTF-8 boundary at or before max_bytes
    let end = text
        .char_indices()
        .take_while(|(i, _)| *i < max_bytes)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    if end == 0 {
        return text
            .chars()
            .next()
            .map_or_else(String::new, |c| format!("{c}..."));
    }
    // Try to break at the last space to avoid cutting words
    let truncated = &text[..end];
    if let Some(space_pos) = truncated.rfind(' ') {
        format!("{}...", &text[..space_pos])
    } else {
        format!("{truncated}...")
    }
}

/// Estimate token count for a text string, accounting for CJK characters.
///
/// CJK characters typically tokenize as ~1.5 tokens each (not 4 chars/token like ASCII).
/// This produces more accurate estimates for mixed-language content.
pub fn estimate_text_tokens(text: &str) -> usize {
    let mut ascii_chars: usize = 0;
    let mut cjk_chars: usize = 0;
    for ch in text.chars() {
        let cp = ch as u32;
        if (0x4E00..=0x9FFF).contains(&cp) // CJK Unified Ideographs
            || (0x3400..=0x4DBF).contains(&cp) // CJK Extension A
            || (0x3000..=0x303F).contains(&cp) // CJK Symbols
            || (0x3040..=0x309F).contains(&cp) // Hiragana
            || (0x30A0..=0x30FF).contains(&cp) // Katakana
            || (0xAC00..=0xD7AF).contains(&cp) // Hangul Syllables
            || (0xF900..=0xFAFF).contains(&cp) // CJK Compatibility Ideographs
            || (0xFF00..=0xFFEF).contains(&cp)
        {
            // Fullwidth Forms
            cjk_chars += 1;
        } else {
            ascii_chars += ch.len_utf8();
        }
    }
    let ascii_tokens = ascii_chars / 4;
    let cjk_tokens = (cjk_chars as f32 * 1.5).ceil() as usize;
    (ascii_tokens + cjk_tokens).max(1)
}

/// Estimate token count for a single message
pub fn estimate_message_tokens(msg: &Message) -> usize {
    match &msg.content {
        MessageContent::Text(text) => estimate_text_tokens(text),
        MessageContent::Blocks(blocks) => {
            let mut total = 0usize;
            for block in blocks {
                match block {
                    ContentBlock::Text { text } => total += estimate_text_tokens(text),
                    ContentBlock::ToolUse { name, input, .. } => {
                        total += estimate_text_tokens(name);
                        total += serde_json::to_string(input)
                            .map_or(0, |s| estimate_text_tokens(&s));
                    }
                    ContentBlock::ToolResult { content, .. } => {
                        if let Some(c) = content {
                            match c {
                                ToolResultContent::Single(s) => total += estimate_text_tokens(s),
                                ToolResultContent::Multiple(blocks) => {
                                    for b in blocks {
                                        if let ContentBlock::Text { text } = b {
                                            total += estimate_text_tokens(text);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    ContentBlock::Image { .. } => total += 100,
                    ContentBlock::Thinking { .. } => {}
                }
            }
            total.max(1)
        }
    }
}

/// Estimate total token count for a slice of messages
pub fn estimate_tokens(messages: &[Message]) -> usize {
    messages.iter().map(estimate_message_tokens).sum()
}

/// Estimate tokens for a Vec of messages (owned)
pub fn original_tokens_from(messages: &[Message]) -> usize {
    estimate_tokens(messages)
}

/// Extract tool use info from a message
pub(crate) struct ToolUseInfo {
    pub id: String,
    pub name: String,
}

pub(crate) fn extract_tool_uses(msg: &Message) -> Vec<ToolUseInfo> {
    let mut tool_uses = Vec::new();
    if let MessageContent::Blocks(blocks) = &msg.content {
        for block in blocks {
            if let ContentBlock::ToolUse { id, name, .. } = block {
                tool_uses.push(ToolUseInfo {
                    id: id.clone(),
                    name: name.clone(),
                });
            }
        }
    }
    tool_uses
}

/// Check if a message contains tool_result blocks for the given tool use IDs
pub(crate) fn contains_tool_result_for(msg: &Message, tool_uses: &[ToolUseInfo]) -> bool {
    let tool_use_ids: HashSet<&str> = tool_uses.iter().map(|t| t.id.as_str()).collect();

    if let MessageContent::Blocks(blocks) = &msg.content {
        for block in blocks {
            if let ContentBlock::ToolResult { tool_use_id, .. } = block {
                if tool_use_ids.contains(tool_use_id.as_str()) {
                    return true;
                }
            }
        }
    }
    false
}

/// Heuristic to detect messages that likely contain code or code-related content.
pub fn looks_like_code(msg: &Message) -> bool {
    let text = extract_text_content(msg);
    // Check for common code indicators.
    let has_code_fence = text.contains("```");
    let has_file_path = text.contains(".rs")
        || text.contains(".toml")
        || text.contains(".py")
        || text.contains(".ts")
        || text.contains(".js")
        || text.contains("fn ")
        || text.contains("pub fn")
        || text.contains("impl ")
        || text.contains("use ");
    let has_tool_use = matches!(
        &msg.content,
        MessageContent::Blocks(blocks) if blocks.iter().any(|b| matches!(b, ContentBlock::ToolUse { .. } | ContentBlock::ToolResult { .. }))
    );

    has_code_fence || has_file_path || has_tool_use
}

/// Tools that typically return content-rich results worth preserving in summaries.
pub fn is_content_rich_tool(name: &str) -> bool {
    matches!(
        name,
        "Read"
            | "Grep"
            | "Glob"
            | "Bash"
            | "WebSearch"
            | "WebFetch"
            | "Fetch"
            | "search"
            | "code_search"
            | "ast_grep_search"
    )
}

/// Get the appropriate truncation limit for a tool result based on tool name.
pub fn tool_result_preview_limit(tool_name: &str) -> usize {
    if is_content_rich_tool(tool_name) {
        400
    } else {
        150
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_msg(role: &str, text: &str) -> Message {
        Message { role: role.into(), content: MessageContent::Text(text.into()) }
    }

    // ── extract_text_content ─────────────────────────────────────────────

    #[test]
    fn test_extract_text_simple() {
        let msg = text_msg("user", "hello world");
        assert_eq!(extract_text_content(&msg), "hello world");
    }

    #[test]
    fn test_extract_text_from_blocks() {
        let msg = Message {
            role: "assistant".into(),
            content: MessageContent::Blocks(vec![
                ContentBlock::Text { text: "Hello".into() },
                ContentBlock::Text { text: "World".into() },
            ]),
        };
        let text = extract_text_content(&msg);
        assert!(text.contains("Hello"));
        assert!(text.contains("World"));
    }

    // ── truncate_text ────────────────────────────────────────────────────

    #[test]
    fn test_truncate_short_text_unchanged() {
        assert_eq!(truncate_text("hello", 100), "hello");
    }

    #[test]
    fn test_truncate_long_text_with_space() {
        let text = "The quick brown fox jumps over the lazy dog";
        let result = truncate_text(text, 20);
        assert!(result.len() < text.len());
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_truncate_long_text_no_space() {
        let text = "abcdefghijklmnop";
        let result = truncate_text(text, 5);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_truncate_empty() {
        assert_eq!(truncate_text("", 10), "");
    }

    #[test]
    fn test_truncate_exact_length() {
        assert_eq!(truncate_text("12345", 5), "12345");
    }

    // ── estimate_text_tokens ─────────────────────────────────────────────

    #[test]
    fn test_estimate_ascii_tokens() {
        // ~4 chars per token
        let tokens = estimate_text_tokens("Hello world test string");
        assert!(tokens > 0);
        assert!(tokens < 10); // ~24 chars / 4 = ~6 tokens
    }

    #[test]
    fn test_estimate_cjk_tokens() {
        // CJK: ~1.5 tokens per char
        let tokens = estimate_text_tokens("你好世界");
        assert!(tokens >= 4); // 4 chars * 1.5 = 6 tokens (min 1)
    }

    #[test]
    fn test_estimate_mixed_tokens() {
        let tokens = estimate_text_tokens("Hello 你好 world");
        assert!(tokens > 0);
    }

    #[test]
    fn test_estimate_empty_min_one() {
        let tokens = estimate_text_tokens("");
        assert_eq!(tokens, 1); // .max(1)
    }

    // ── estimate_message_tokens ──────────────────────────────────────────

    #[test]
    fn test_estimate_text_message() {
        let msg = text_msg("user", "Hello, this is a test message with some content.");
        let tokens = estimate_message_tokens(&msg);
        assert!(tokens > 0);
    }

    #[test]
    fn test_estimate_blocks_message() {
        let msg = Message {
            role: "assistant".into(),
            content: MessageContent::Blocks(vec![
                ContentBlock::Text { text: "Here's the code:".into() },
            ]),
        };
        let tokens = estimate_message_tokens(&msg);
        assert!(tokens > 0);
    }

    #[test]
    fn test_estimate_image_message() {
        let msg = Message {
            role: "assistant".into(),
            content: MessageContent::Blocks(vec![
                ContentBlock::Image {
                    source: crate::api::ImageSource {
                        source_type: "base64".into(),
                        media_type: "image/png".into(),
                        data: "abc".into(),
                    },
                },
            ]),
        };
        let tokens = estimate_message_tokens(&msg);
        assert_eq!(tokens, 100); // Fixed cost for images
    }

    // ── estimate_tokens (slice) ──────────────────────────────────────────

    #[test]
    fn test_estimate_tokens_empty() {
        assert_eq!(estimate_tokens(&[]), 0);
    }

    #[test]
    fn test_estimate_tokens_multiple() {
        let msgs = [
            text_msg("user", "Hello"),
            text_msg("assistant", "Hi there"),
        ];
        let total = estimate_tokens(&msgs);
        let sum = estimate_message_tokens(&msgs[0]) + estimate_message_tokens(&msgs[1]);
        assert_eq!(total, sum);
    }

    // ── looks_like_code ──────────────────────────────────────────────────

    #[test]
    fn test_looks_like_code_fence() {
        let msg = text_msg("assistant", "```rust\nfn main() {}\n```");
        assert!(looks_like_code(&msg));
    }

    #[test]
    fn test_looks_like_code_file_extension() {
        let msg = text_msg("assistant", "Edit the file src/main.rs");
        assert!(looks_like_code(&msg));
    }

    #[test]
    fn test_looks_like_code_fn_keyword() {
        let msg = text_msg("assistant", "Use `fn foo()` to define");
        assert!(looks_like_code(&msg));
    }

    #[test]
    fn test_looks_like_plain_text() {
        let msg = text_msg("assistant", "Sure, I can help with that.");
        assert!(!looks_like_code(&msg));
    }

    #[test]
    fn test_looks_like_code_tool_use() {
        let msg = Message {
            role: "assistant".into(),
            content: MessageContent::Blocks(vec![
                ContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "bash".into(),
                    input: serde_json::json!({"command": "ls"}),
                },
            ]),
        };
        assert!(looks_like_code(&msg));
    }

    // ── is_content_rich_tool ─────────────────────────────────────────────

    #[test]
    fn test_content_rich_tools() {
        assert!(is_content_rich_tool("Read"));
        assert!(is_content_rich_tool("Grep"));
        assert!(is_content_rich_tool("Glob"));
        assert!(is_content_rich_tool("Bash"));
        assert!(is_content_rich_tool("WebSearch"));
        assert!(is_content_rich_tool("search"));
    }

    #[test]
    fn test_non_content_rich_tools() {
        assert!(!is_content_rich_tool("Edit"));
        assert!(!is_content_rich_tool("Write"));
        assert!(!is_content_rich_tool("Unknown"));
    }

    // ── tool_result_preview_limit ────────────────────────────────────────

    #[test]
    fn test_preview_limit_rich_tool() {
        assert_eq!(tool_result_preview_limit("Read"), 400);
        assert_eq!(tool_result_preview_limit("Bash"), 400);
    }

    #[test]
    fn test_preview_limit_other_tool() {
        assert_eq!(tool_result_preview_limit("Edit"), 150);
        assert_eq!(tool_result_preview_limit("Write"), 150);
    }

    // ── original_tokens_from ─────────────────────────────────────────────

    #[test]
    fn test_original_tokens_from_empty() {
        assert_eq!(original_tokens_from(&[]), 0);
    }

    #[test]
    fn test_original_tokens_from_matches_estimate() {
        let msgs = [text_msg("user", "test message")];
        assert_eq!(original_tokens_from(&msgs), estimate_tokens(&msgs));
    }
}
