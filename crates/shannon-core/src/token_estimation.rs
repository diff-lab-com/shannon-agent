//! Token Estimation
//!
//! Provides token count estimation for content before sending to the API.
//! Supports both rough estimation and file-type-aware estimation.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Default bytes-per-token ratio for text content
const DEFAULT_BYTES_PER_TOKEN: usize = 4;

/// Token estimation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenEstimate {
    pub estimated_tokens: usize,
    pub bytes_per_token: usize,
    pub method: String,
}

/// Token estimation modes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EstimationMethod {
    /// Rough estimation based on character count
    Rough,
    /// File-type-aware estimation
    FileType,
    /// Precise count (requires API call, not implemented here)
    Precise,
}

/// Simple message summary for token estimation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessageSummary {
    pub role: String,
    pub content: String,
}

pub struct TokenEstimator;

impl TokenEstimator {
    pub fn new() -> Self {
        Self
    }

    /// Get the appropriate bytes-per-token ratio for a file type
    pub fn bytes_per_token_for_type(&self, file_extension: &str) -> usize {
        match file_extension {
            "json" | "jsonl" | "jsonc" => 2, // Dense JSON has many single-char tokens
            _ => DEFAULT_BYTES_PER_TOKEN,
        }
    }

    /// Rough token count estimation based on character count
    pub fn rough_estimate(&self, content: &str) -> usize {
        content.len() / DEFAULT_BYTES_PER_TOKEN
    }

    /// File-type-aware token estimation
    pub fn estimate_for_file_type(&self, content: &str, file_extension: &str) -> usize {
        content.len() / self.bytes_per_token_for_type(file_extension)
    }

    /// Estimate tokens for a structured content block (handling nested JSON)
    pub fn estimate_for_content(&self, content: &str) -> usize {
        content.len() / DEFAULT_BYTES_PER_TOKEN
    }

    /// Estimate tokens for a JSON value (stringify + estimate)
    pub fn estimate_for_value(&self, value: &Value) -> usize {
        let s = serde_json::to_string(value).unwrap_or_default();
        if s.is_empty() {
            return 0;
        }
        s.len() / DEFAULT_BYTES_PER_TOKEN
    }

    /// Estimate tokens for a conversation message array
    pub fn estimate_for_messages(&self, messages: &[ConversationMessageSummary]) -> usize {
        messages
            .iter()
            .map(|m| self.rough_estimate(&m.content))
            .sum()
    }

    /// Create a TokenEstimate result
    pub fn create_estimate(&self, content: &str, method: EstimationMethod) -> TokenEstimate {
        let (tokens, bpt) = match method {
            EstimationMethod::Rough => (self.rough_estimate(content), DEFAULT_BYTES_PER_TOKEN),
            EstimationMethod::FileType => (self.estimate_for_content(content), DEFAULT_BYTES_PER_TOKEN),
            EstimationMethod::Precise => (self.rough_estimate(content), DEFAULT_BYTES_PER_TOKEN),
        };
        TokenEstimate {
            estimated_tokens: tokens,
            bytes_per_token: bpt,
            method: format!("{:?}", method),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rough_estimate() {
        let est = TokenEstimator::new();
        // "Hello world" = 11 chars / 4 = 2 tokens (integer division)
        assert_eq!(est.rough_estimate("Hello world"), 2);
    }

    #[test]
    fn test_json_estimate() {
        let est = TokenEstimator::new();
        let json = serde_json::json!({"a": 1, "b": 2});
        let count = est.estimate_for_value(&json);
        assert!(count >= 1); // Should produce a valid estimate
    }

    #[test]
    fn test_text_estimate() {
        let est = TokenEstimator::new();
        let text = "This is a regular text string";
        let count = est.estimate_for_file_type(text, "txt");
        assert!(count > 0);
    }

    #[test]
    fn test_bytes_per_token() {
        let est = TokenEstimator::new();
        assert_eq!(est.bytes_per_token_for_type("json"), 2);
        assert_eq!(est.bytes_per_token_for_type("jsonl"), 2);
        assert_eq!(est.bytes_per_token_for_type("rs"), 4);
        assert_eq!(est.bytes_per_token_for_type("py"), 4);
    }

    #[test]
    fn test_array_estimate() {
        let est = TokenEstimator::new();
        let arr = serde_json::json!(["hello", "world", "test"]);
        let count = est.estimate_for_value(&arr);
        assert!(count >= 1); // ["hello","world","test"] = 26 chars / 4 = 6
    }

    #[test]
    fn test_message_estimation() {
        let est = TokenEstimator::new();
        let msgs = vec![
            ConversationMessageSummary {
                role: "user".into(),
                content: "Hello".into(),
            },
            ConversationMessageSummary {
                role: "assistant".into(),
                content: "Hi there!".into(),
            },
        ];
        let count = est.estimate_for_messages(&msgs);
        // "Hello" (5/4=1) + "Hi there!" (9/4=2) = 3
        assert_eq!(count, 3);
    }

    #[test]
    fn test_empty_content() {
        let est = TokenEstimator::new();
        assert_eq!(est.rough_estimate(""), 0);
        // Value::Null serializes to "null" (4 bytes / 4 = 1 token)
        assert_eq!(est.estimate_for_value(&Value::Null), 1);
    }

    #[test]
    fn test_create_estimate() {
        let est = TokenEstimator::new();
        let estimate = est.create_estimate("hello world", EstimationMethod::Rough);
        assert_eq!(estimate.estimated_tokens, 2);
        assert_eq!(estimate.bytes_per_token, 4);
    }
}
