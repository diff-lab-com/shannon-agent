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

/// Truncate text to a maximum character length at a word boundary
pub fn truncate_text(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        return text.to_string();
    }
    let truncated = &text[..max_chars];
    // Try to break at the last space to avoid cutting words
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
