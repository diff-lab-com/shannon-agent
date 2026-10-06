//! Streaming response handling and conversation state management.

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
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use shannon_engine::api::{
        ContentBlock, ImageSource, Message, MessageContent, ToolResultContent,
    };

    fn text_msg(role: &str, text: &str) -> Message {
        Message {
            role: role.into(),
            content: MessageContent::Text(text.into()),
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

    // ── Send/Sync ───────────────────────────────────────────────────────

    #[test]
    fn test_types_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ConversationState>();
    }
}
