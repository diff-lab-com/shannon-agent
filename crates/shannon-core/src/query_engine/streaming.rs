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
        let system_tokens = system_prompt.map(|sp| estimate_text_tokens(sp)).unwrap_or(0);
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

    /// Generate a summary of messages
    fn summarize_messages(messages: &[Message]) -> String {
        let mut summary_parts = Vec::new();
        let mut turn_count = 0;

        for msg in messages {
            match &msg.content {
                crate::api::MessageContent::Text(text) => {
                    let role = if msg.role == "user" { "User" } else { "Assistant" };
                    // Take first 100 chars of each message for the summary
                    let preview = if text.len() > 100 {
                        let end = text.char_indices().take_while(|(i, _)| *i <= 97).last().map(|(i, c)| i + c.len_utf8()).unwrap_or(text.len());
                        format!("{}...", &text[..end.min(text.len())])
                    } else {
                        text.clone()
                    };
                    summary_parts.push(format!("{role}: {preview}"));
                    turn_count += 1;
                }
                crate::api::MessageContent::Blocks(blocks) => {
                    let mut tool_uses = Vec::new();
                    for block in blocks {
                        if let crate::api::ContentBlock::ToolUse { name, .. } = block {
                            tool_uses.push(name.clone());
                        } else if let crate::api::ContentBlock::ToolResult { content, .. } = block {
                            if let Some(crate::api::ToolResultContent::Single(result)) = content {
                                summary_parts.push(format!(
                                    "Tool result: {}",
                                    if result.len() > 80 {
                                        let end = result.char_indices().take_while(|(i, _)| *i <= 77).last().map(|(i, c)| i + c.len_utf8()).unwrap_or(result.len());
                                        format!("{}...", &result[..end.min(result.len())])
                                    } else {
                                        result.clone()
                                    }
                                ));
                            } else if let Some(crate::api::ToolResultContent::Multiple(results)) =
                                content
                            {
                                summary_parts.push(format!("Tool results: {} items", results.len()));
                            }
                        }
                    }
                    if !tool_uses.is_empty() {
                        summary_parts.push(format!("Tools used: {}", tool_uses.join(", ")));
                    }
                }
            }
        }

        format!(
            "Summary of {} turns:\n{}",
            turn_count,
            summary_parts.join("\n")
        )
    }
}
