//! # Message Types
//!
//! Common message types used across Shannon Code.
//! These are foundational types used in state persistence and API communication.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Message content for API requests and state persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    /// Simple text content
    Text(String),
    /// Structured content blocks
    Blocks(Vec<ContentBlock>),
}

/// Content block in messages
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },

    #[serde(rename = "image")]
    Image {
        source: ImageSource,
    },

    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },

    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: Option<ToolResultContent>,
        is_error: Option<bool>,
    },
}

/// Image source for image blocks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageSource {
    #[serde(rename = "type")]
    pub media_type: String,
    pub data: String,
}

/// Tool result content
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolResultContent {
    Single(String),
    Multiple(Vec<ContentBlock>),
}

/// Message in a conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: MessageContent,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_content_text() {
        let content = MessageContent::Text("Hello".to_string());
        let json = serde_json::to_string(&content).unwrap();
        assert_eq!(json, "\"Hello\"");
    }

    #[test]
    fn test_message_content_blocks() {
        let blocks = vec![ContentBlock::Text {
            text: "Hello".to_string(),
        }];
        let content = MessageContent::Blocks(blocks);
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains("text"));
    }

    #[test]
    fn test_message_serialization() {
        let msg = Message {
            role: "user".to_string(),
            content: MessageContent::Text("test".to_string()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("user"));
        assert!(json.contains("test"));
    }

    #[test]
    fn test_content_block_tool_use() {
        let block = ContentBlock::ToolUse {
            id: "tool_123".to_string(),
            name: "bash".to_string(),
            input: serde_json::json!({"command": "echo hello"}),
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("tool_use"));
        assert!(json.contains("bash"));
    }

    #[test]
    fn test_message_roundtrip() {
        let original = Message {
            role: "assistant".to_string(),
            content: MessageContent::Text("Response".to_string()),
        };
        let json = serde_json::to_string(&original).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(original.role, parsed.role);
        match (original.content, parsed.content) {
            (MessageContent::Text(a), MessageContent::Text(b)) => assert_eq!(a, b),
            _ => panic!("Content type mismatch"),
        }
    }
}
