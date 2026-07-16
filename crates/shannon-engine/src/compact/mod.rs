//! # Context Compression Module
//!
//! Advanced conversation compression system for Shannon Code. Provides intelligent
//! context management through multiple compression strategies:
//!
//! - **Full Compaction**: AI-powered summarization of older conversation turns
//! - **Micro Compaction**: Compression of individual oversized messages/tool results
//! - **Message Grouping**: Groups related messages (tool call + result) for smarter compression
//! - **Session Memory Compaction**: Compresses accumulated session memory entries
//! - **Auto-Compact**: Automatic trigger when context approaches the model's limit
//!
//! ## Architecture
//!
//! The [`CompactEngine`] orchestrates all compression strategies. It uses a pluggable
//! [`Summarizer`] trait so the AI summarization backend can be mocked in tests.
//!
//! ```text
//! Conversation exceeds threshold
//!     |
//!     v
//! analyze_context() --> determines strategy
//!     |
//!     v
//! compact() / micro_compact() / compact_session_memory()
//!     |
//!     v
//! post_compact_cleanup() --> removes duplicates, fixes references
//! ```

pub mod compact_messages;
pub mod engine;
pub mod helpers;
pub mod protection;
pub mod summarizer;
pub mod types;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;

// Re-export all public types to maintain the same public API as the original
// single-file module.
pub use compact_messages::{
    CompactMessagesResult, CompactionConfig, CompactionStrategy, compact_messages,
};

pub use engine::CompactEngine;
pub use helpers::{
    estimate_message_tokens, estimate_text_tokens, estimate_tokens, looks_like_code,
};
pub use protection::compact_messages_with_protection;
pub use protection::{MessageProtector, classify_message_priority};
pub use summarizer::{LlmSummarizer, RuleBasedSummarizer};
pub use types::{
    CompactConfig, CompactError, CompactPrompt, CompactResult, CompactStrategy, ContextAnalysis,
    GroupedMessage, MessageGroup, Summarizer,
};
