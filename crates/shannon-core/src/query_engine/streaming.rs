//! Streaming response handling and conversation state management.

use crate::api::Message;
use crate::query_engine::types::QueryEngineConfig;

/// Conversation state for tracking messages
#[derive(Debug, Clone)]
pub struct ConversationState {
    pub messages: Vec<Message>,
    pub turn_count: usize,
    pub total_tokens: u64,
    pub total_cost: f64,
}

impl Default for ConversationState {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            turn_count: 0,
            total_tokens: 0,
            total_cost: 0.0,
        }
    }
}

impl ConversationState {
    /// Estimate the token count of the current conversation.
    /// Uses CJK-aware token estimation for better accuracy with mixed-language content.
    pub fn estimate_tokens(&self) -> usize {
        use crate::compact::helpers::estimate_text_tokens;
        let mut total: usize = 0;
        for msg in &self.messages {
            total += match &msg.content {
                crate::api::MessageContent::Text(text) => estimate_text_tokens(text),
                crate::api::MessageContent::Blocks(blocks) => {
                    let mut block_tokens = 0;
                    for block in blocks {
                        match block {
                            crate::api::ContentBlock::Text { text } => {
                                block_tokens += estimate_text_tokens(text)
                            }
                            crate::api::ContentBlock::ToolUse { name, input, .. } => {
                                block_tokens += estimate_text_tokens(name);
                                block_tokens += serde_json::to_string(input)
                                    .map_or(0, |s| estimate_text_tokens(&s));
                            }
                            crate::api::ContentBlock::ToolResult { content: Some(c), .. } => {
                                match c {
                                    crate::api::ToolResultContent::Single(s) => {
                                        block_tokens += estimate_text_tokens(s)
                                    }
                                    crate::api::ToolResultContent::Multiple(blocks) => {
                                        for b in blocks {
                                            match b {
                                                crate::api::ContentBlock::Text { text } => {
                                                    block_tokens += estimate_text_tokens(text)
                                                }
                                                crate::api::ContentBlock::ToolUse {
                                                    name, input, ..
                                                } => {
                                                    block_tokens += estimate_text_tokens(name);
                                                    block_tokens += serde_json::to_string(input)
                                                        .map_or(0, |s| estimate_text_tokens(&s));
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                }
                            }
                            crate::api::ContentBlock::ToolResult { content: None, .. } => {}
                            crate::api::ContentBlock::Image { .. } => block_tokens += 100,
                            _ => {}
                        }
                    }
                    block_tokens
                }
            };
        }
        total
    }

    /// Estimate tokens including an optional system prompt.
    /// This gives a more accurate picture of total context usage.
    pub fn estimate_tokens_with_system_prompt(&self, system_prompt: Option<&str>) -> usize {
        use crate::compact::helpers::estimate_text_tokens;
        let msg_tokens = self.estimate_tokens();
        let system_tokens = system_prompt.map(estimate_text_tokens).unwrap_or(0);
        msg_tokens + system_tokens
    }

    /// Check if the conversation needs compression based on config.
    /// Includes system prompt in token budget for accurate threshold detection.
    pub fn needs_compression(&self, config: &QueryEngineConfig) -> bool {
        if let Some(max_tokens) = config.max_context_tokens {
            let threshold = (max_tokens as f32 * config.compression_threshold) as usize;
            let tokens = self.estimate_tokens_with_system_prompt(config.system_prompt.as_deref());
            tokens > threshold
        } else {
            false
        }
    }

    /// Compress the conversation using the strategy specified in config.
    ///
    /// - [`CompressionStrategy::SummarizeOld`]: Keeps the most recent messages in
    ///   full and replaces older messages with a short summary.
    /// - [`CompressionStrategy::TruncateOldest`]: Simply drops the oldest messages,
    ///   keeping only the most recent ones.
    pub fn compress(&mut self, config: &QueryEngineConfig) {
        if self.messages.len() <= config.keep_recent_messages + 1 {
            return; // Not enough messages to compress
        }

        let keep_count = config.keep_recent_messages;
        let split_point = self.messages.len().saturating_sub(keep_count);

        match config.compression_strategy {
            crate::query_engine::types::CompressionStrategy::SummarizeOld => {
                let old_messages: Vec<Message> = self.messages.drain(..split_point).collect();
                let summary = Self::summarize_messages(&old_messages);

                // Create a summary message as a system message
                let summary_msg = crate::api::Message {
                    role: "system".to_string(),
                    content: crate::api::MessageContent::Text(
                        format!("[Previous conversation summary]\n\n{summary}"),
                    ),
                };

                // Insert summary at the beginning
                self.messages.insert(0, summary_msg);
            }
            crate::query_engine::types::CompressionStrategy::TruncateOldest => {
                // Simply drop the oldest messages without summary
                self.messages.drain(..split_point);
            }
        }
    }

    /// Generate a summary of messages with tool-aware truncation.
    fn summarize_messages(messages: &[Message]) -> String {
        use std::collections::HashMap;

        let mut summary_parts = Vec::new();
        let mut turn_count = 0;

        // Build tool_use_id -> tool_name map for context-aware summarization
        let mut tool_names: HashMap<String, String> = HashMap::new();
        for msg in messages {
            if let crate::api::MessageContent::Blocks(blocks) = &msg.content {
                for block in blocks {
                    if let crate::api::ContentBlock::ToolUse { id, name, .. } = block {
                        tool_names.insert(id.clone(), name.clone());
                    }
                }
            }
        }

        for msg in messages {
            match &msg.content {
                crate::api::MessageContent::Text(text) => {
                    let role = if msg.role == "user" { "User" } else { "Assistant" };
                    let preview = truncate_summary(text, 150);
                    summary_parts.push(format!("{role}: {preview}"));
                    turn_count += 1;
                }
                crate::api::MessageContent::Blocks(blocks) => {
                    for block in blocks {
                        match block {
                            crate::api::ContentBlock::ToolUse { name, input, .. } => {
                                let input_json =
                                    serde_json::to_string(input).unwrap_or_default();
                                let input_preview = truncate_summary(&input_json, 80);
                                summary_parts
                                    .push(format!("Tool: {name}({input_preview})"));
                                turn_count += 1;
                            }
                            crate::api::ContentBlock::ToolResult {
                                tool_use_id,
                                content,
                                is_error,
                                ..
                            } => {
                                let tool_name = tool_names
                                    .get(tool_use_id)
                                    .map(|s| s.as_str())
                                    .unwrap_or("unknown");
                                let limit = content_rich_limit(tool_name);
                                let err_tag =
                                    if is_error.unwrap_or(false) { " (error)" } else { "" };
                                let result_text = match content {
                                    Some(crate::api::ToolResultContent::Single(s)) => {
                                        truncate_summary(s, limit)
                                    }
                                    Some(crate::api::ToolResultContent::Multiple(results)) => {
                                        let text: String = results
                                            .iter()
                                            .filter_map(|b| match b {
                                                crate::api::ContentBlock::Text { text } => {
                                                    Some(text.as_str())
                                                }
                                                _ => None,
                                            })
                                            .collect::<Vec<_>>()
                                            .join(" ");
                                        truncate_summary(&text, limit)
                                    }
                                    None => "(empty)".to_string(),
                                };
                                summary_parts.push(format!(
                                    "Result{err_tag} [{tool_name}]: {result_text}"
                                ));
                            }
                            crate::api::ContentBlock::Text { text } => {
                                summary_parts.push(format!(
                                    "Text: {}",
                                    truncate_summary(text, 100)
                                ));
                                turn_count += 1;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        format!(
            "Summary of {turn_count} turns:\n{}",
            summary_parts.join("\n")
        )
    }
}

/// Truncate text for summary display, respecting UTF-8 boundaries.
fn truncate_summary(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let limit = max_bytes.saturating_sub(3);
    let end = text
        .char_indices()
        .take_while(|(i, _)| *i <= limit)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    if end == 0 {
        return text
            .chars()
            .next()
            .map_or_else(String::new, |c| format!("{c}..."));
    }
    let truncated = &text[..end];
    if let Some(space_pos) = truncated.rfind(' ') {
        format!("{}...", &text[..space_pos])
    } else {
        format!("{truncated}...")
    }
}

/// Get the truncation limit for a tool result based on whether it's content-rich.
fn content_rich_limit(tool_name: &str) -> usize {
    if matches!(
        tool_name,
        "Read" | "Grep" | "Glob" | "Bash"
            | "WebSearch" | "WebFetch" | "Fetch"
            | "search" | "code_search" | "ast_grep_search"
    ) {
        300
    } else {
        100
    }
}
