//! Core types for context compression.

use crate::api::Message;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during context compression
#[derive(Error, Debug)]
pub enum CompactError {
    #[error("No messages to compact")]
    NoMessagesToCompact,

    #[error("Summarization failed: {0}")]
    SummarizationFailed(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Token estimation error: {0}")]
    TokenEstimationError(String),

    #[error("Compression already in progress")]
    AlreadyInProgress,

    #[error("Compact duration exceeded limit")]
    Timeout,
}

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for the context compression engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactConfig {
    /// Maximum tokens for compact summary (default: 2000)
    pub max_output_tokens: usize,
    /// Number of recent messages to keep in full (default: 10)
    pub keep_recent_count: usize,
    /// Fraction of max context to trigger auto-compact (default: 0.75)
    pub trigger_threshold: f32,
    /// Enable single-message compression for oversized results
    pub enable_micro_compact: bool,
    /// Token threshold for micro-compact (default: 4096)
    pub micro_compact_threshold: usize,
    /// Compress session memory entries too
    pub enable_session_memory_compact: bool,
    /// Maximum context window size in tokens (default: 200_000)
    pub max_context_tokens: usize,
    /// Optional model override for compaction (e.g. use a smaller/cheaper model).
    /// When set, the LLM summarizer will use this model instead of the main client model.
    /// Defaults to None (use the main conversation model).
    pub compact_model: Option<String>,
}

impl Default for CompactConfig {
    fn default() -> Self {
        Self {
            max_output_tokens: 2000,
            keep_recent_count: 10,
            trigger_threshold: 0.75,
            enable_micro_compact: true,
            micro_compact_threshold: 4096,
            enable_session_memory_compact: true,
            max_context_tokens: 200_000,
            compact_model: None,
        }
    }
}

impl CompactConfig {
    /// Create a config with a specific max context size
    pub fn with_max_context(max_context_tokens: usize) -> Self {
        Self {
            max_context_tokens,
            ..Default::default()
        }
    }

    /// Validate the configuration values
    pub fn validate(&self) -> Result<(), CompactError> {
        if self.max_output_tokens == 0 {
            return Err(CompactError::InvalidConfig(
                "max_output_tokens must be > 0".to_string(),
            ));
        }
        if self.keep_recent_count == 0 {
            return Err(CompactError::InvalidConfig(
                "keep_recent_count must be > 0".to_string(),
            ));
        }
        if self.trigger_threshold <= 0.0 || self.trigger_threshold > 1.0 {
            return Err(CompactError::InvalidConfig(
                "trigger_threshold must be in (0.0, 1.0]".to_string(),
            ));
        }
        if self.max_context_tokens == 0 {
            return Err(CompactError::InvalidConfig(
                "max_context_tokens must be > 0".to_string(),
            ));
        }
        Ok(())
    }
}

// ============================================================================
// Strategy
// ============================================================================

/// Compression strategy to apply
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompactStrategy {
    /// Remove oldest messages beyond threshold (lossy, fast)
    TruncateOld,
    /// Summarize older messages, keep recent in full
    SummarizeOld,
    /// Compress individual large messages/tool results
    MicroCompress,
    /// Group related messages and compress groups
    GroupCompress,
    /// Full session memory compression
    SessionMemoryCompress,
}

impl std::fmt::Display for CompactStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompactStrategy::TruncateOld => write!(f, "truncate_old"),
            CompactStrategy::SummarizeOld => write!(f, "summarize_old"),
            CompactStrategy::MicroCompress => write!(f, "micro_compress"),
            CompactStrategy::GroupCompress => write!(f, "group_compress"),
            CompactStrategy::SessionMemoryCompress => write!(f, "session_memory_compress"),
        }
    }
}

// ============================================================================
// Message Grouping
// ============================================================================

/// A single message within a group, with metadata
#[derive(Debug, Clone)]
pub struct GroupedMessage {
    /// The original message
    pub message: Message,
    /// Index of this message in the original conversation
    pub original_index: usize,
    /// Estimated token count
    pub estimated_tokens: usize,
}

/// Groups of related messages for intelligent compression
#[derive(Debug, Clone)]
pub enum MessageGroup {
    /// A user turn (may include multiple user messages in sequence)
    UserTurn { messages: Vec<GroupedMessage> },
    /// An assistant turn (may include text + tool use blocks)
    AssistantTurn { messages: Vec<GroupedMessage> },
    /// A tool use turn: groups the assistant's tool_use with the tool_result
    ToolUseTurn {
        tool_name: String,
        tool_use_id: String,
        messages: Vec<GroupedMessage>,
    },
    /// System messages (CLAUDE.md context, summaries, etc.)
    SystemMessage { messages: Vec<GroupedMessage> },
}

impl MessageGroup {
    /// Total estimated tokens for all messages in this group
    pub fn total_tokens(&self) -> usize {
        match self {
            MessageGroup::UserTurn { messages } => {
                messages.iter().map(|m| m.estimated_tokens).sum()
            }
            MessageGroup::AssistantTurn { messages } => {
                messages.iter().map(|m| m.estimated_tokens).sum()
            }
            MessageGroup::ToolUseTurn { messages, .. } => {
                messages.iter().map(|m| m.estimated_tokens).sum()
            }
            MessageGroup::SystemMessage { messages } => {
                messages.iter().map(|m| m.estimated_tokens).sum()
            }
        }
    }

    /// Get all messages in the group as a slice
    pub fn messages(&self) -> &[GroupedMessage] {
        match self {
            MessageGroup::UserTurn { messages } => messages,
            MessageGroup::AssistantTurn { messages } => messages,
            MessageGroup::ToolUseTurn { messages, .. } => messages,
            MessageGroup::SystemMessage { messages } => messages,
        }
    }

    /// A display label for the group
    pub fn label(&self) -> String {
        match self {
            MessageGroup::UserTurn { messages } => {
                format!("UserTurn ({} messages)", messages.len())
            }
            MessageGroup::AssistantTurn { messages } => {
                format!("AssistantTurn ({} messages)", messages.len())
            }
            MessageGroup::ToolUseTurn {
                tool_name,
                messages,
                ..
            } => {
                format!("ToolUse[{}] ({} messages)", tool_name, messages.len())
            }
            MessageGroup::SystemMessage { messages } => {
                format!("SystemMessage ({} messages)", messages.len())
            }
        }
    }
}

// ============================================================================
// Compact Result
// ============================================================================

/// Result of a compression operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactResult {
    /// Estimated token count before compression
    pub original_tokens: usize,
    /// Estimated token count after compression
    pub compacted_tokens: usize,
    /// Fraction of tokens removed (0.0 to 1.0)
    pub reduction_ratio: f32,
    /// Number of messages removed entirely
    pub messages_removed: usize,
    /// Number of messages compressed (content replaced)
    pub messages_compacted: usize,
    /// Wall-clock duration of the compression
    pub duration: Duration,
    /// Strategy that was applied
    pub strategy: CompactStrategy,
}

impl CompactResult {
    /// Create a no-op result when nothing was compressed
    pub fn no_change(strategy: CompactStrategy, original_tokens: usize) -> Self {
        Self {
            original_tokens,
            compacted_tokens: original_tokens,
            reduction_ratio: 0.0,
            messages_removed: 0,
            messages_compacted: 0,
            duration: Duration::ZERO,
            strategy,
        }
    }
}

impl std::fmt::Display for CompactResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Compact[{}]: {} -> {} tokens ({:.1}% reduction, {} removed, {} compacted, {:.2}s)",
            self.strategy,
            self.original_tokens,
            self.compacted_tokens,
            self.reduction_ratio * 100.0,
            self.messages_removed,
            self.messages_compacted,
            self.duration.as_secs_f64(),
        )
    }
}

// ============================================================================
// Analysis Result
// ============================================================================

/// Result of analyzing the conversation context
#[derive(Debug, Clone)]
pub struct ContextAnalysis {
    /// Current estimated token count
    pub estimated_tokens: usize,
    /// Whether auto-compact should be triggered
    pub should_compact: bool,
    /// Which strategy is recommended
    pub recommended_strategy: CompactStrategy,
    /// Number of messages that would be compacted
    pub compactable_message_count: usize,
    /// Number of oversized messages suitable for micro-compact
    pub micro_compact_candidates: usize,
    /// Fraction of context used (0.0 to 1.0)
    pub context_usage_ratio: f32,
}

// ============================================================================
// Compact Prompt
// ============================================================================

/// Generates the system prompt for AI-based conversation summarization
#[derive(Debug, Clone)]
pub struct CompactPrompt;

impl CompactPrompt {
    /// Build the summarization system prompt
    pub fn system_prompt(max_tokens: usize) -> String {
        format!(
            "You are a conversation compression assistant. Your task is to produce a concise \
            summary of the conversation below, preserving:\n\n\
            1. The user's goals and intent\n\
            2. Key decisions made\n\
            3. Important findings and conclusions\n\
            4. File paths and code references that were discussed\n\
            5. Tool calls that were made and their results (abbreviated)\n\
            6. Any errors encountered and their resolutions\n\
            7. Pending tasks or next steps\n\n\
            The summary must be under {max_tokens} tokens. Focus on information that would be \
            needed to continue the conversation productively. Omit redundant explanations, \
            failed attempts that were abandoned, and verbose tool output.\n\n\
            Format the summary as a structured but readable text. Use headings if helpful."
        )
    }

    /// Build the user message containing the conversation to summarize
    pub fn conversation_to_summarize(messages: &[Message]) -> String {
        use super::helpers::extract_text_content;

        let mut parts = Vec::new();
        for msg in messages {
            let role = &msg.role;
            let content_text = extract_text_content(msg);
            let preview = if content_text.len() > 500 {
                let mut end = 497;
                while !content_text.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}...", &content_text[..end])
            } else {
                content_text
            };
            parts.push(format!("[{role}]: {preview}"));
        }
        parts.join("\n\n")
    }

    /// Build a micro-compact prompt for a single large message
    pub fn micro_compact_prompt(message: &Message, max_tokens: usize) -> String {
        use super::helpers::extract_text_content;

        let content = extract_text_content(message);
        format!(
            "Summarize the following {} message in under {} tokens, preserving \
            all key information, file paths, and data values:\n\n{}",
            message.role,
            max_tokens,
            if content.len() > 2000 {
                let mut end = 1997;
                while !content.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}...", &content[..end])
            } else {
                content
            }
        )
    }
}

// ============================================================================
// Summarizer Trait
// ============================================================================

/// Trait for AI-based summarization. Can be mocked in tests.
pub trait Summarizer: Send + Sync {
    /// Summarize a list of messages into a concise text summary
    fn summarize(&self, messages: &[Message], max_tokens: usize) -> Result<String, CompactError>;

    /// Compress a single message's content
    fn micro_summarize(&self, message: &Message, max_tokens: usize)
    -> Result<String, CompactError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── CompactError ─────────────────────────────────────────────────────

    #[test]
    fn test_compact_error_display() {
        assert_eq!(
            CompactError::NoMessagesToCompact.to_string(),
            "No messages to compact"
        );
        assert!(
            CompactError::SummarizationFailed("timeout".into())
                .to_string()
                .contains("timeout")
        );
        assert!(
            CompactError::InvalidConfig("bad".into())
                .to_string()
                .contains("bad")
        );
        assert!(
            CompactError::TokenEstimationError("overflow".into())
                .to_string()
                .contains("overflow")
        );
        assert_eq!(
            CompactError::AlreadyInProgress.to_string(),
            "Compression already in progress"
        );
        assert_eq!(
            CompactError::Timeout.to_string(),
            "Compact duration exceeded limit"
        );
    }

    // ── CompactConfig ────────────────────────────────────────────────────

    #[test]
    fn test_default_config() {
        let cfg = CompactConfig::default();
        assert_eq!(cfg.max_output_tokens, 2000);
        assert_eq!(cfg.keep_recent_count, 10);
        assert!((cfg.trigger_threshold - 0.75).abs() < f32::EPSILON);
        assert!(cfg.enable_micro_compact);
        assert_eq!(cfg.micro_compact_threshold, 4096);
        assert!(cfg.enable_session_memory_compact);
        assert_eq!(cfg.max_context_tokens, 200_000);
        assert!(cfg.compact_model.is_none());
    }

    #[test]
    fn test_config_with_max_context() {
        let cfg = CompactConfig::with_max_context(100_000);
        assert_eq!(cfg.max_context_tokens, 100_000);
        assert_eq!(cfg.max_output_tokens, 2000); // default preserved
    }

    #[test]
    fn test_config_validate_ok() {
        let cfg = CompactConfig::default();
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_config_validate_zero_output_tokens() {
        let mut cfg = CompactConfig::default();
        cfg.max_output_tokens = 0;
        let err = cfg.validate().unwrap_err();
        assert!(matches!(err, CompactError::InvalidConfig(_)));
        assert!(err.to_string().contains("max_output_tokens"));
    }

    #[test]
    fn test_config_validate_zero_keep_recent() {
        let mut cfg = CompactConfig::default();
        cfg.keep_recent_count = 0;
        let err = cfg.validate().unwrap_err();
        assert!(err.to_string().contains("keep_recent_count"));
    }

    #[test]
    fn test_config_validate_threshold_zero() {
        let mut cfg = CompactConfig::default();
        cfg.trigger_threshold = 0.0;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_config_validate_threshold_over_one() {
        let mut cfg = CompactConfig::default();
        cfg.trigger_threshold = 1.5;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_config_validate_threshold_boundary_one() {
        let mut cfg = CompactConfig::default();
        cfg.trigger_threshold = 1.0;
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_config_validate_zero_max_context() {
        let mut cfg = CompactConfig::default();
        cfg.max_context_tokens = 0;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_config_serialization_roundtrip() {
        let cfg = CompactConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let parsed: CompactConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.max_output_tokens, cfg.max_output_tokens);
        assert_eq!(parsed.keep_recent_count, cfg.keep_recent_count);
        assert_eq!(parsed.compact_model, cfg.compact_model);
    }

    // ── CompactStrategy ──────────────────────────────────────────────────

    #[test]
    fn test_strategy_display() {
        assert_eq!(CompactStrategy::TruncateOld.to_string(), "truncate_old");
        assert_eq!(CompactStrategy::SummarizeOld.to_string(), "summarize_old");
        assert_eq!(CompactStrategy::MicroCompress.to_string(), "micro_compress");
        assert_eq!(CompactStrategy::GroupCompress.to_string(), "group_compress");
        assert_eq!(
            CompactStrategy::SessionMemoryCompress.to_string(),
            "session_memory_compress"
        );
    }

    #[test]
    fn test_strategy_serialization_roundtrip() {
        for strategy in [
            CompactStrategy::TruncateOld,
            CompactStrategy::SummarizeOld,
            CompactStrategy::MicroCompress,
            CompactStrategy::GroupCompress,
            CompactStrategy::SessionMemoryCompress,
        ] {
            let json = serde_json::to_string(&strategy).unwrap();
            let parsed: CompactStrategy = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, strategy);
        }
    }

    // ── CompactResult ────────────────────────────────────────────────────

    #[test]
    fn test_compact_result_no_change() {
        let result = CompactResult::no_change(CompactStrategy::TruncateOld, 5000);
        assert_eq!(result.original_tokens, 5000);
        assert_eq!(result.compacted_tokens, 5000);
        assert_eq!(result.reduction_ratio, 0.0);
        assert_eq!(result.messages_removed, 0);
        assert_eq!(result.messages_compacted, 0);
        assert_eq!(result.duration, Duration::ZERO);
        assert_eq!(result.strategy, CompactStrategy::TruncateOld);
    }

    #[test]
    fn test_compact_result_display() {
        let result = CompactResult {
            original_tokens: 10000,
            compacted_tokens: 3000,
            reduction_ratio: 0.7,
            messages_removed: 5,
            messages_compacted: 3,
            duration: Duration::from_millis(1500),
            strategy: CompactStrategy::SummarizeOld,
        };
        let display = result.to_string();
        assert!(display.contains("summarize_old"));
        assert!(display.contains("10000"));
        assert!(display.contains("3000"));
        assert!(display.contains("70.0%"));
        assert!(display.contains("5"));
        assert!(display.contains("3"));
    }

    #[test]
    fn test_compact_result_serialization_roundtrip() {
        let result = CompactResult {
            original_tokens: 8000,
            compacted_tokens: 2000,
            reduction_ratio: 0.75,
            messages_removed: 4,
            messages_compacted: 2,
            duration: Duration::from_secs(3),
            strategy: CompactStrategy::GroupCompress,
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: CompactResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.original_tokens, result.original_tokens);
        assert_eq!(parsed.compacted_tokens, result.compacted_tokens);
        assert_eq!(parsed.strategy, result.strategy);
    }

    // ── ContextAnalysis ──────────────────────────────────────────────────

    #[test]
    fn test_context_analysis_fields() {
        let analysis = ContextAnalysis {
            estimated_tokens: 150_000,
            should_compact: true,
            recommended_strategy: CompactStrategy::SummarizeOld,
            compactable_message_count: 20,
            micro_compact_candidates: 3,
            context_usage_ratio: 0.85,
        };
        assert!(analysis.should_compact);
        assert_eq!(analysis.estimated_tokens, 150_000);
        assert_eq!(analysis.compactable_message_count, 20);
        assert!((analysis.context_usage_ratio - 0.85).abs() < f32::EPSILON);
    }

    // ── CompactPrompt ────────────────────────────────────────────────────

    #[test]
    fn test_system_prompt_contains_key_elements() {
        let prompt = CompactPrompt::system_prompt(2000);
        assert!(prompt.contains("2000"));
        assert!(prompt.contains("goals"));
        assert!(prompt.contains("summary"));
    }

    #[test]
    fn test_micro_compact_prompt() {
        let msg = Message {
            role: "user".to_string(),
            content: crate::api::MessageContent::Text(
                "This is a very long tool output...".to_string(),
            ),
        };
        let prompt = CompactPrompt::micro_compact_prompt(&msg, 500);
        assert!(prompt.contains("500"));
        assert!(prompt.contains("user"));
        assert!(prompt.contains("tool output"));
    }

    // ── Send/Sync assertions ─────────────────────────────────────────────

    #[test]
    fn test_types_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CompactConfig>();
        assert_send_sync::<CompactStrategy>();
        assert_send_sync::<CompactResult>();
        assert_send_sync::<CompactError>();
    }
}
