//! Streaming response handling and conversation state management.

use crate::query_engine::types::QueryEngineConfig;
use shannon_engine::api::Message;

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
        use shannon_engine::compact::helpers::estimate_text_tokens;
        let mut total: usize = 0;
        for msg in &self.messages {
            total += match &msg.content {
                shannon_engine::api::MessageContent::Text(text) => estimate_text_tokens(text),
                shannon_engine::api::MessageContent::Blocks(blocks) => {
                    let mut block_tokens = 0;
                    for block in blocks {
                        match block {
                            shannon_engine::api::ContentBlock::Text { text } => {
                                block_tokens += estimate_text_tokens(text)
                            }
                            shannon_engine::api::ContentBlock::ToolUse { name, input, .. } => {
                                block_tokens += estimate_text_tokens(name);
                                block_tokens += serde_json::to_string(input)
                                    .map_or(0, |s| estimate_text_tokens(&s));
                            }
                            shannon_engine::api::ContentBlock::ToolResult {
                                content: Some(c),
                                ..
                            } => match c {
                                shannon_engine::api::ToolResultContent::Single(s) => {
                                    block_tokens += estimate_text_tokens(s)
                                }
                                shannon_engine::api::ToolResultContent::Multiple(blocks) => {
                                    for b in blocks {
                                        match b {
                                            shannon_engine::api::ContentBlock::Text { text } => {
                                                block_tokens += estimate_text_tokens(text)
                                            }
                                            shannon_engine::api::ContentBlock::ToolUse {
                                                name,
                                                input,
                                                ..
                                            } => {
                                                block_tokens += estimate_text_tokens(name);
                                                block_tokens += serde_json::to_string(input)
                                                    .map_or(0, |s| estimate_text_tokens(&s));
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            },
                            shannon_engine::api::ContentBlock::ToolResult {
                                content: None, ..
                            } => {}
                            shannon_engine::api::ContentBlock::Image { .. } => block_tokens += 100,
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
        use shannon_engine::compact::helpers::estimate_text_tokens;
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
    ///
    /// Cache-aware: preserves the first message pair (user + assistant) to avoid
    /// breaking prompt cache prefixes on providers like Anthropic.
    pub fn compress(&mut self, config: &QueryEngineConfig) {
        if self.messages.len() <= config.keep_recent_messages + 1 {
            return; // Not enough messages to compress
        }

        let keep_count = config.keep_recent_messages;
        let split_point = self.messages.len().saturating_sub(keep_count);

        // Preserve at least the first message pair for cache prefix stability.
        // The system prompt + first exchange forms the cache prefix on Anthropic.
        let min_preserve = 2;
        if split_point <= min_preserve {
            return; // Can't compress without breaking cache prefix
        }

        match config.compression_strategy {
            crate::query_engine::types::CompressionStrategy::SummarizeOld => {
                // Drain messages between the cache prefix and the recent tail
                let old_messages: Vec<Message> =
                    self.messages.drain(min_preserve..split_point).collect();
                let summary = Self::summarize_messages(&old_messages);

                // Create a summary message as a system message
                let summary_msg = shannon_engine::api::Message {
                    role: "system".to_string(),
                    content: shannon_engine::api::MessageContent::Text(format!(
                        "[Previous conversation summary]\n\n{summary}"
                    )),
                };

                // Insert summary after the preserved cache prefix
                self.messages.insert(min_preserve, summary_msg);
            }
            crate::query_engine::types::CompressionStrategy::TruncateOldest => {
                // Drop the oldest messages between cache prefix and recent tail
                self.messages.drain(min_preserve..split_point);
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
            if let shannon_engine::api::MessageContent::Blocks(blocks) = &msg.content {
                for block in blocks {
                    if let shannon_engine::api::ContentBlock::ToolUse { id, name, .. } = block {
                        tool_names.insert(id.clone(), name.clone());
                    }
                }
            }
        }

        for msg in messages {
            match &msg.content {
                shannon_engine::api::MessageContent::Text(text) => {
                    let role = if msg.role == "user" {
                        "User"
                    } else {
                        "Assistant"
                    };
                    let preview = truncate_summary(text, 150);
                    summary_parts.push(format!("{role}: {preview}"));
                    turn_count += 1;
                }
                shannon_engine::api::MessageContent::Blocks(blocks) => {
                    for block in blocks {
                        match block {
                            shannon_engine::api::ContentBlock::ToolUse { name, input, .. } => {
                                let input_json = serde_json::to_string(input).unwrap_or_default();
                                let input_preview = truncate_summary(&input_json, 80);
                                summary_parts.push(format!("Tool: {name}({input_preview})"));
                                turn_count += 1;
                            }
                            shannon_engine::api::ContentBlock::ToolResult {
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
                                let err_tag = if is_error.unwrap_or(false) {
                                    " (error)"
                                } else {
                                    ""
                                };
                                let result_text = match content {
                                    Some(shannon_engine::api::ToolResultContent::Single(s)) => {
                                        truncate_summary(s, limit)
                                    }
                                    Some(shannon_engine::api::ToolResultContent::Multiple(
                                        results,
                                    )) => {
                                        let text: String = results
                                            .iter()
                                            .filter_map(|b| match b {
                                                shannon_engine::api::ContentBlock::Text {
                                                    text,
                                                } => Some(text.as_str()),
                                                _ => None,
                                            })
                                            .collect::<Vec<_>>()
                                            .join(" ");
                                        truncate_summary(&text, limit)
                                    }
                                    None => "(empty)".to_string(),
                                };
                                summary_parts
                                    .push(format!("Result{err_tag} [{tool_name}]: {result_text}"));
                            }
                            shannon_engine::api::ContentBlock::Text { text } => {
                                summary_parts
                                    .push(format!("Text: {}", truncate_summary(text, 100)));
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
    ) {
        300
    } else {
        100
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query_engine::types::CompressionStrategy;
    use shannon_engine::api::{
        ContentBlock, ImageSource, Message, MessageContent, ToolResultContent,
    };

    fn text_msg(role: &str, text: &str) -> Message {
        Message {
            role: role.into(),
            content: MessageContent::Text(text.into()),
        }
    }

    fn test_config() -> QueryEngineConfig {
        QueryEngineConfig {
            max_context_tokens: Some(1000),
            compression_threshold: 0.8,
            keep_recent_messages: 2,
            compression_strategy: CompressionStrategy::TruncateOldest,
            system_prompt: None,
            ..Default::default()
        }
    }

    // ── ConversationState::default ──────────────────────────────────────

    #[test]
    fn test_default_state() {
        let state = ConversationState::default();
        assert!(state.messages.is_empty());
        assert_eq!(state.turn_count, 0);
        assert_eq!(state.total_tokens, 0);
        assert_eq!(state.total_cost, 0.0);
    }

    // ── estimate_tokens ─────────────────────────────────────────────────

    #[test]
    fn test_estimate_tokens_empty() {
        let state = ConversationState::default();
        assert_eq!(state.estimate_tokens(), 0);
    }

    #[test]
    fn test_estimate_tokens_text_messages() {
        let state = ConversationState {
            messages: vec![
                text_msg("user", "Hello world"),
                text_msg("assistant", "Hi there"),
            ],
            ..Default::default()
        };
        let tokens = state.estimate_tokens();
        assert!(tokens > 0);
    }

    #[test]
    fn test_estimate_tokens_blocks_message() {
        let state = ConversationState {
            messages: vec![Message {
                role: "assistant".into(),
                content: MessageContent::Blocks(vec![
                    ContentBlock::Text {
                        text: "Here's the result:".into(),
                    },
                    ContentBlock::ToolUse {
                        id: "t1".into(),
                        name: "Read".into(),
                        input: serde_json::json!({"file": "main.rs"}),
                    },
                    ContentBlock::ToolResult {
                        tool_use_id: "t1".into(),
                        content: Some(ToolResultContent::Single("fn main() {}".into())),
                        is_error: None,
                    },
                ]),
            }],
            ..Default::default()
        };
        let tokens = state.estimate_tokens();
        assert!(tokens > 0);
    }

    #[test]
    fn test_estimate_tokens_image() {
        let state = ConversationState {
            messages: vec![Message {
                role: "user".into(),
                content: MessageContent::Blocks(vec![ContentBlock::Image {
                    source: ImageSource {
                        source_type: "base64".into(),
                        media_type: "image/png".into(),
                        data: "abc123".into(),
                    },
                }]),
            }],
            ..Default::default()
        };
        assert_eq!(state.estimate_tokens(), 100);
    }

    #[test]
    fn test_estimate_tokens_cjk_content() {
        let state = ConversationState {
            messages: vec![text_msg("user", "你好世界这是一段中文内容")],
            ..Default::default()
        };
        let tokens = state.estimate_tokens();
        assert!(tokens > 0);
    }

    // ── estimate_tokens_with_system_prompt ──────────────────────────────

    #[test]
    fn test_estimate_tokens_no_system_prompt() {
        let state = ConversationState {
            messages: vec![text_msg("user", "Hello")],
            ..Default::default()
        };
        let without = state.estimate_tokens();
        let with_none = state.estimate_tokens_with_system_prompt(None);
        assert_eq!(without, with_none);
    }

    #[test]
    fn test_estimate_tokens_with_system_prompt() {
        let state = ConversationState {
            messages: vec![text_msg("user", "Hello")],
            ..Default::default()
        };
        let without = state.estimate_tokens();
        let with = state.estimate_tokens_with_system_prompt(Some("You are a helpful assistant"));
        assert!(with > without);
    }

    // ── needs_compression ───────────────────────────────────────────────

    #[test]
    fn test_needs_compression_no_max_tokens() {
        let mut config = test_config();
        config.max_context_tokens = None;
        let state = ConversationState {
            messages: vec![text_msg("user", &"x".repeat(10000))],
            ..Default::default()
        };
        assert!(!state.needs_compression(&config));
    }

    #[test]
    fn test_needs_compression_under_threshold() {
        let state = ConversationState {
            messages: vec![text_msg("user", "short")],
            ..Default::default()
        };
        assert!(!state.needs_compression(&test_config()));
    }

    #[test]
    fn test_needs_compression_over_threshold() {
        let config = test_config(); // 1000 tokens * 0.8 = 800 threshold
        // Need ~3200+ chars to exceed 800 tokens (at ~4 chars/token)
        let state = ConversationState {
            messages: vec![text_msg("user", &"x ".repeat(2000))],
            ..Default::default()
        };
        assert!(state.needs_compression(&config));
    }

    // ── compress (TruncateOldest) ───────────────────────────────────────

    #[test]
    fn test_compress_truncate_too_few_messages() {
        let mut config = test_config();
        config.keep_recent_messages = 5;
        let mut state = ConversationState {
            messages: vec![text_msg("user", "hi"), text_msg("assistant", "hello")],
            ..Default::default()
        };
        let original_len = state.messages.len();
        state.compress(&config);
        assert_eq!(state.messages.len(), original_len);
    }

    #[test]
    fn test_compress_truncate_removes_old_messages() {
        let config = test_config(); // keep_recent = 2
        let mut state = ConversationState {
            messages: (0..6)
                .map(|i| {
                    text_msg(
                        if i % 2 == 0 { "user" } else { "assistant" },
                        &format!("msg {i}"),
                    )
                })
                .collect(),
            ..Default::default()
        };
        state.compress(&config);
        // Should keep first 2 (cache prefix) + last 2 (keep_recent) = 4
        assert!(state.messages.len() <= 4);
    }

    #[test]
    fn test_compress_preserves_cache_prefix() {
        let config = test_config();
        let first_msg = text_msg("user", "cache prefix user");
        let second_msg = text_msg("assistant", "cache prefix assistant");
        let mut state = ConversationState {
            messages: vec![
                first_msg.clone(),
                second_msg.clone(),
                text_msg("user", "old msg"),
                text_msg("assistant", "old reply"),
                text_msg("user", "recent msg"),
                text_msg("assistant", "recent reply"),
            ],
            ..Default::default()
        };
        state.compress(&config);
        // First two messages should always be preserved
        let first_text = match &state.messages[0].content {
            MessageContent::Text(t) => t.clone(),
            _ => String::new(),
        };
        assert!(first_text.contains("cache prefix"));
    }

    // ── compress (SummarizeOld) ─────────────────────────────────────────

    #[test]
    fn test_compress_summarize_inserts_summary() {
        let mut config = test_config();
        config.compression_strategy = CompressionStrategy::SummarizeOld;
        let mut state = ConversationState {
            messages: vec![
                text_msg("user", "first user"),
                text_msg("assistant", "first assistant"),
                text_msg("user", "middle user question"),
                text_msg("assistant", "middle assistant answer"),
                text_msg("user", "recent user"),
                text_msg("assistant", "recent assistant"),
            ],
            ..Default::default()
        };
        state.compress(&config);
        // Should have: first pair + summary + recent pair
        assert!(state.messages.len() >= 3);
        // Check that a summary message was inserted
        let has_summary = state
            .messages
            .iter()
            .any(|m| matches!(&m.content, MessageContent::Text(t) if t.contains("summary")));
        assert!(has_summary);
    }

    // ── summarize_messages ──────────────────────────────────────────────

    #[test]
    fn test_summarize_text_messages() {
        let msgs = vec![
            text_msg("user", "How do I read a file?"),
            text_msg("assistant", "Use the Read tool."),
        ];
        let summary = ConversationState::summarize_messages(&msgs);
        assert!(summary.contains("User"));
        assert!(summary.contains("Assistant"));
        assert!(summary.contains("How do I"));
    }

    #[test]
    fn test_summarize_tool_use_messages() {
        let msgs = vec![Message {
            role: "assistant".into(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
                id: "t1".into(),
                name: "Read".into(),
                input: serde_json::json!({"file": "main.rs"}),
            }]),
        }];
        let summary = ConversationState::summarize_messages(&msgs);
        assert!(summary.contains("Tool:"));
        assert!(summary.contains("Read"));
    }

    #[test]
    fn test_summarize_tool_result_message() {
        let msgs = vec![Message {
            role: "user".into(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: Some(ToolResultContent::Single("fn main() {}".into())),
                is_error: Some(false),
            }]),
        }];
        let summary = ConversationState::summarize_messages(&msgs);
        assert!(summary.contains("Result"));
    }

    #[test]
    fn test_summarize_error_result() {
        let msgs = vec![Message {
            role: "user".into(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: Some(ToolResultContent::Single("file not found".into())),
                is_error: Some(true),
            }]),
        }];
        let summary = ConversationState::summarize_messages(&msgs);
        assert!(summary.contains("error"));
    }

    #[test]
    fn test_summarize_empty_tool_result() {
        let msgs = vec![Message {
            role: "user".into(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: None,
                is_error: None,
            }]),
        }];
        let summary = ConversationState::summarize_messages(&msgs);
        assert!(summary.contains("empty"));
    }

    // ── truncate_summary ────────────────────────────────────────────────

    #[test]
    fn test_truncate_summary_short_text() {
        assert_eq!(truncate_summary("hello", 100), "hello");
    }

    #[test]
    fn test_truncate_summary_long_text() {
        let text = "The quick brown fox jumps over the lazy dog and keeps going";
        let result = truncate_summary(text, 20);
        assert!(result.len() < text.len());
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_truncate_summary_exact_length() {
        assert_eq!(truncate_summary("12345", 5), "12345");
    }

    #[test]
    fn test_truncate_summary_empty() {
        assert_eq!(truncate_summary("", 10), "");
    }

    // ── content_rich_limit ──────────────────────────────────────────────

    #[test]
    fn test_content_rich_limit_for_rich_tools() {
        assert_eq!(content_rich_limit("Read"), 300);
        assert_eq!(content_rich_limit("Grep"), 300);
        assert_eq!(content_rich_limit("Bash"), 300);
        assert_eq!(content_rich_limit("WebSearch"), 300);
    }

    #[test]
    fn test_content_rich_limit_for_other_tools() {
        assert_eq!(content_rich_limit("Edit"), 100);
        assert_eq!(content_rich_limit("Write"), 100);
        assert_eq!(content_rich_limit("Unknown"), 100);
    }

    // ── Send/Sync ───────────────────────────────────────────────────────

    #[test]
    fn test_types_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ConversationState>();
    }
}
