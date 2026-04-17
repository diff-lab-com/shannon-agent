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

use crate::api::{
    ContentBlock, Message, MessageContent, ToolResultContent,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::{Duration, Instant};
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
    /// Fraction of max context to trigger auto-compact (default: 0.8)
    pub trigger_threshold: f32,
    /// Enable single-message compression for oversized results
    pub enable_micro_compact: bool,
    /// Token threshold for micro-compact (default: 4096)
    pub micro_compact_threshold: usize,
    /// Compress session memory entries too
    pub enable_session_memory_compact: bool,
    /// Maximum context window size in tokens (default: 200_000)
    pub max_context_tokens: usize,
}

impl Default for CompactConfig {
    fn default() -> Self {
        Self {
            max_output_tokens: 2000,
            keep_recent_count: 10,
            trigger_threshold: 0.8,
            enable_micro_compact: true,
            micro_compact_threshold: 4096,
            enable_session_memory_compact: true,
            max_context_tokens: 200_000,
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
    UserTurn {
        messages: Vec<GroupedMessage>,
    },
    /// An assistant turn (may include text + tool use blocks)
    AssistantTurn {
        messages: Vec<GroupedMessage>,
    },
    /// A tool use turn: groups the assistant's tool_use with the tool_result
    ToolUseTurn {
        tool_name: String,
        tool_use_id: String,
        messages: Vec<GroupedMessage>,
    },
    /// System messages (CLAUDE.md context, summaries, etc.)
    SystemMessage {
        messages: Vec<GroupedMessage>,
    },
}

impl MessageGroup {
    /// Total estimated tokens for all messages in this group
    pub fn total_tokens(&self) -> usize {
        match self {
            MessageGroup::UserTurn { messages } => messages.iter().map(|m| m.estimated_tokens).sum(),
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
            MessageGroup::ToolUseTurn { tool_name, messages, .. } => {
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
        let mut parts = Vec::new();
        for msg in messages {
            let role = &msg.role;
            let content_text = extract_text_content(msg);
            let preview = if content_text.len() > 500 {
                format!("{}...", &content_text[..497])
            } else {
                content_text
            };
            parts.push(format!("[{role}]: {preview}"));
        }
        parts.join("\n\n")
    }

    /// Build a micro-compact prompt for a single large message
    pub fn micro_compact_prompt(message: &Message, max_tokens: usize) -> String {
        let content = extract_text_content(message);
        format!(
            "Summarize the following {} message in under {} tokens, preserving \
            all key information, file paths, and data values:\n\n{}",
            message.role,
            max_tokens,
            if content.len() > 2000 {
                format!("{}...", &content[..1997])
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
    fn micro_summarize(&self, message: &Message, max_tokens: usize) -> Result<String, CompactError>;
}

/// A simple rule-based summarizer that does not call an AI API.
/// Useful for tests and as a fallback.
#[derive(Debug, Clone, Default)]
pub struct RuleBasedSummarizer;

impl RuleBasedSummarizer {
    pub fn new() -> Self {
        Self
    }
}

impl Summarizer for RuleBasedSummarizer {
    fn summarize(&self, messages: &[Message], _max_tokens: usize) -> Result<String, CompactError> {
        if messages.is_empty() {
            return Err(CompactError::NoMessagesToCompact);
        }

        let mut summary_parts = Vec::new();
        let mut turn_count = 0;
        let mut tool_names: HashSet<String> = HashSet::new();
        let mut file_paths: HashSet<String> = HashSet::new();
        let mut errors_encountered = Vec::new();

        for msg in messages {
            match &msg.content {
                MessageContent::Text(text) => {
                    let role_label = if msg.role == "user" {
                        "User"
                    } else if msg.role == "assistant" {
                        "Assistant"
                    } else {
                        "System"
                    };
                    let preview = truncate_text(text, 150);
                    summary_parts.push(format!("{role_label}: {preview}"));

                    // Extract file path patterns
                    for word in text.split_whitespace() {
                        if word.contains('/') && (word.ends_with(".rs") || word.ends_with(".toml")
                            || word.ends_with(".md") || word.ends_with(".json")
                            || word.ends_with(".yaml") || word.ends_with(".yml"))
                        {
                            file_paths.insert(word.to_string());
                        }
                    }
                    turn_count += 1;
                }
                MessageContent::Blocks(blocks) => {
                    for block in blocks {
                        match block {
                            ContentBlock::ToolUse { name, input, .. } => {
                                tool_names.insert(name.clone());
                                summary_parts.push(format!(
                                    "Tool call: {}({})",
                                    name,
                                    truncate_text(&serde_json::to_string(input).unwrap_or_default(), 100)
                                ));
                            }
                            ContentBlock::ToolResult { content, is_error, .. } => {
                                let is_err = is_error.unwrap_or(false);
                                let result_text = match content {
                                    Some(ToolResultContent::Single(s)) => truncate_text(s, 100),
                                    Some(ToolResultContent::Multiple(blocks)) => {
                                        let count = blocks.len();
                                        let text: String = blocks
                                            .iter()
                                            .filter_map(|b| match b {
                                                ContentBlock::Text { text } => Some(text.as_str()),
                                                _ => None,
                                            })
                                            .collect::<Vec<_>>()
                                            .join(" ");
                                        format!("{} items: {}", count, truncate_text(&text, 80))
                                    }
                                    None => "(empty)".to_string(),
                                };
                                if is_err {
                                    errors_encountered.push(result_text.clone());
                                }
                                summary_parts.push(format!(
                                    "Tool result{}: {}",
                                    if is_err { " (error)" } else { "" },
                                    result_text
                                ));
                            }
                            ContentBlock::Text { text } => {
                                summary_parts.push(format!("Text: {}", truncate_text(text, 100)));
                                turn_count += 1;
                            }
                            ContentBlock::Image { .. } => {
                                summary_parts.push("Image (omitted from summary)".to_string());
                            }
                            ContentBlock::Thinking { .. } => {
                                // Thinking blocks are internal and omitted from summaries
                            }
                        }
                    }
                }
            }
        }

        let mut summary = format!(
            "[Conversation summary - {} turns, {} messages]\n",
            turn_count,
            messages.len()
        );

        summary.push_str(&summary_parts.join("\n"));

        if !tool_names.is_empty() {
            summary.push_str(&format!(
                "\n\nTools used: {}",
                tool_names.into_iter().collect::<Vec<_>>().join(", ")
            ));
        }

        if !file_paths.is_empty() {
            summary.push_str(&format!(
                "\nFiles referenced: {}",
                file_paths.into_iter().collect::<Vec<_>>().join(", ")
            ));
        }

        if !errors_encountered.is_empty() {
            summary.push_str("\nErrors encountered:");
            for err in &errors_encountered {
                summary.push_str(&format!("\n  - {err}"));
            }
        }

        Ok(summary)
    }

    fn micro_summarize(&self, message: &Message, _max_tokens: usize) -> Result<String, CompactError> {
        let content = extract_text_content(message);
        Ok(format!(
            "[Compressed {} message]\n{}",
            message.role,
            truncate_text(&content, 500)
        ))
    }
}

/// AI-powered summarizer that uses the LLM to generate intelligent conversation summaries.
///
/// Unlike [`RuleBasedSummarizer`] which simply truncates and concatenates, this summarizer
/// sends the conversation to the LLM with a summarization prompt, producing a much more
/// useful summary that preserves key context (decisions, code snippets, tool results).
///
/// Uses `tokio::runtime::Handle::block_on` internally since the [`Summarizer`] trait is sync.
pub struct AiSummarizer {
    client: crate::api::LlmClient,
}

impl AiSummarizer {
    /// Create a new AI summarizer backed by the given LLM client.
    pub fn new(client: crate::api::LlmClient) -> Self {
        Self { client }
    }

    /// Build the summarization prompt from conversation messages.
    fn build_summary_prompt(messages: &[Message], max_tokens: usize) -> String {
        let mut parts = Vec::new();
        for msg in messages {
            let role = &msg.role;
            match &msg.content {
                MessageContent::Text(text) => {
                    parts.push(format!("[{role}]: {}", truncate_text(text, 500)));
                }
                MessageContent::Blocks(blocks) => {
                    for block in blocks {
                        match block {
                            ContentBlock::ToolUse { name, input, .. } => {
                                let input_str =
                                    serde_json::to_string(input).unwrap_or_default();
                                parts.push(format!(
                                    "[{role} tool_use]: {name}({})",
                                    truncate_text(&input_str, 300)
                                ));
                            }
                            ContentBlock::ToolResult { content, is_error, .. } => {
                                let is_err = is_error.unwrap_or(false);
                                let prefix = if is_err { "ERROR" } else { "result" };
                                let text = match content {
                                    Some(ToolResultContent::Single(s)) => truncate_text(s, 300),
                                    Some(ToolResultContent::Multiple(blks)) => blks
                                        .iter()
                                        .filter_map(|b| match b {
                                            ContentBlock::Text { text } => Some(text.as_str()),
                                            _ => None,
                                        })
                                        .collect::<Vec<_>>()
                                        .join(" ")
                                        .chars()
                                        .take(300)
                                        .collect(),
                                    None => "no content".to_string(),
                                };
                                parts.push(format!("[{role} {prefix}]: {text}"));
                            }
                            ContentBlock::Text { text } => {
                                parts.push(format!("[{role}]: {}", truncate_text(text, 500)));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        format!(
            "Summarize the following conversation concisely, preserving:\n\
             - Key decisions and their reasoning\n\
             - Important code snippets, file paths, and function names\n\
             - Tool call results that are still relevant\n\
             - Any errors encountered and their resolutions\n\
             - Unresolved issues or pending tasks\n\n\
             Conversation (target summary: ~{max_tokens} tokens):\n\
             ---\n{}\n---\n\n\
             Provide a concise but comprehensive summary:",
            parts.join("\n")
        )
    }
}

impl Summarizer for AiSummarizer {
    fn summarize(&self, messages: &[Message], max_tokens: usize) -> Result<String, CompactError> {
        if messages.is_empty() {
            return Err(CompactError::NoMessagesToCompact);
        }

        let prompt = Self::build_summary_prompt(messages, max_tokens);
        let request_messages = vec![crate::api::Message {
            role: "user".to_string(),
            content: crate::api::MessageContent::Text(prompt),
        }];

        let rt = tokio::runtime::Handle::current();
        let result = rt.block_on(async {
            self.client
                .send_message(request_messages, None, None)
                .await
                .map_err(|e| CompactError::SummarizationFailed(format!("AI summary failed: {e}")))
        });

        match result {
            Ok(blocks) => {
                let summary: String = blocks
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                if summary.trim().is_empty() {
                    Err(CompactError::SummarizationFailed(
                        "AI returned empty summary".to_string(),
                    ))
                } else {
                    Ok(summary)
                }
            }
            Err(e) => Err(e),
        }
    }

    fn micro_summarize(&self, message: &Message, max_tokens: usize) -> Result<String, CompactError> {
        // For micro-compact, use a simpler prompt focused on a single message
        let text = match &message.content {
            MessageContent::Text(t) => t.clone(),
            MessageContent::Blocks(blocks) => blocks
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        };

        if text.trim().is_empty() {
            return Err(CompactError::NoMessagesToCompact);
        }

        let prompt = format!(
            "Compress this text to ~{max_tokens} tokens, keeping key information:\n\n{}",
            truncate_text(&text, 3000)
        );

        let request_messages = vec![crate::api::Message {
            role: "user".to_string(),
            content: crate::api::MessageContent::Text(prompt),
        }];

        let rt = tokio::runtime::Handle::current();
        rt.block_on(async {
            self.client
                .send_message(request_messages, None, None)
                .await
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .map_err(|e| CompactError::SummarizationFailed(format!("AI micro-summary failed: {e}")))
        })
    }
}

// ============================================================================
// Compact Engine
// ============================================================================

/// Main compression engine for conversation context management
pub struct CompactEngine {
    config: CompactConfig,
    summarizer: Box<dyn Summarizer>,
    compacting: bool,
}

impl CompactEngine {
    /// Create a new compact engine with the given config and summarizer
    pub fn new(config: CompactConfig, summarizer: Box<dyn Summarizer>) -> Result<Self, CompactError> {
        config.validate()?;
        Ok(Self {
            config,
            summarizer,
            compacting: false,
        })
    }

    /// Create with default config and a rule-based summarizer (no AI needed)
    pub fn with_defaults() -> Result<Self, CompactError> {
        Self::new(CompactConfig::default(), Box::new(RuleBasedSummarizer::new()))
    }

    /// Create with an AI-powered summarizer for higher quality compression.
    pub fn with_ai_summarizer(client: crate::api::LlmClient) -> Result<Self, CompactError> {
        Self::new(CompactConfig::default(), Box::new(AiSummarizer::new(client)))
    }

    /// Get a reference to the config
    pub fn config(&self) -> &CompactConfig {
        &self.config
    }

    /// Update the configuration
    pub fn set_config(&mut self, config: CompactConfig) -> Result<(), CompactError> {
        config.validate()?;
        self.config = config;
        Ok(())
    }

    // ========================================================================
    // Analysis
    // ========================================================================

    /// Analyze the current conversation to determine the best compression strategy
    pub fn analyze_context(&self, messages: &[Message]) -> ContextAnalysis {
        let estimated_tokens = estimate_tokens(messages);
        let context_usage_ratio = if self.config.max_context_tokens > 0 {
            estimated_tokens as f32 / self.config.max_context_tokens as f32
        } else {
            0.0
        };
        let should_compact = context_usage_ratio >= self.config.trigger_threshold;

        // Count micro-compact candidates
        let micro_compact_candidates = if self.config.enable_micro_compact {
            messages
                .iter()
                .filter(|m| estimate_message_tokens(m) > self.config.micro_compact_threshold)
                .count()
        } else {
            0
        };

        // Determine recommended strategy
        let recommended_strategy = if micro_compact_candidates > 0
            && messages.len() <= self.config.keep_recent_count + 2
        {
            CompactStrategy::MicroCompress
        } else if should_compact && messages.len() > self.config.keep_recent_count {
            CompactStrategy::SummarizeOld
        } else if should_compact {
            CompactStrategy::TruncateOld
        } else {
            CompactStrategy::SummarizeOld // default recommendation
        };

        let compactable_message_count = if messages.len() > self.config.keep_recent_count {
            messages.len() - self.config.keep_recent_count
        } else {
            0
        };

        ContextAnalysis {
            estimated_tokens,
            should_compact,
            recommended_strategy,
            compactable_message_count,
            micro_compact_candidates,
            context_usage_ratio,
        }
    }

    /// Check if auto-compact should be triggered based on current messages
    pub fn auto_compact_check(&self, messages: &[Message]) -> bool {
        let analysis = self.analyze_context(messages);
        analysis.should_compact
    }

    // ========================================================================
    // Full Compaction
    // ========================================================================

    /// Perform full conversation compression using AI summarization
    pub fn compact(&mut self, messages: &mut Vec<Message>) -> Result<CompactResult, CompactError> {
        if self.compacting {
            return Err(CompactError::AlreadyInProgress);
        }

        let original_tokens = estimate_tokens(messages);
        if messages.is_empty() {
            return Err(CompactError::NoMessagesToCompact);
        }
        if messages.len() <= self.config.keep_recent_count + 1 {
            tracing::debug!(
                "Not enough messages to compact: {} <= {}",
                messages.len(),
                self.config.keep_recent_count + 1
            );
            return Ok(CompactResult::no_change(
                CompactStrategy::SummarizeOld,
                original_tokens,
            ));
        }

        self.compacting = true;
        let start = Instant::now();

        let result = self.do_compact(messages);

        self.compacting = false;

        match result {
            Ok(mut compact_result) => {
                compact_result.duration = start.elapsed();
                tracing::info!("{}", compact_result);
                Ok(compact_result)
            }
            Err(e) => {
                tracing::error!("Compaction failed: {}", e);
                Err(e)
            }
        }
    }

    fn do_compact(
        &self,
        messages: &mut Vec<Message>,
    ) -> Result<CompactResult, CompactError> {
        let keep_count = self.config.keep_recent_count;
        let split_point = messages.len().saturating_sub(keep_count);

        // Extract older messages for summarization
        let old_messages: Vec<Message> = messages[..split_point].to_vec();
        let messages_removed = old_messages.len();

        // Summarize the older messages
        let summary_text =
            self.summarizer
                .summarize(&old_messages, self.config.max_output_tokens)?;

        // Create a summary system message
        let summary_message = Message {
            role: "system".to_string(),
            content: MessageContent::Text(format!(
                "[Previous conversation summary - {messages_removed} messages compacted]\n\n{summary_text}"
            )),
        };

        // Replace old messages with the summary
        messages.drain(..split_point);
        messages.insert(0, summary_message);

        let compacted_tokens = estimate_tokens(messages);
        let reduction_ratio = if original_tokens_from(&old_messages) > 0 {
            1.0 - (compacted_tokens as f32 / (original_tokens_from(&old_messages) + compacted_tokens) as f32)
        } else {
            0.0
        };

        Ok(CompactResult {
            original_tokens: original_tokens_from(&old_messages) + compacted_tokens,
            compacted_tokens,
            reduction_ratio,
            messages_removed,
            messages_compacted: messages_removed,
            duration: Duration::ZERO, // set by caller
            strategy: CompactStrategy::SummarizeOld,
        })
    }

    // ========================================================================
    // Micro Compaction
    // ========================================================================

    /// Compress individual large messages/tool results in-place
    pub fn micro_compact(&self, messages: &mut [Message]) -> Result<CompactResult, CompactError> {
        let original_tokens = estimate_tokens(messages);
        if !self.config.enable_micro_compact {
            return Ok(CompactResult::no_change(
                CompactStrategy::MicroCompress,
                original_tokens,
            ));
        }

        let start = Instant::now();
        let mut messages_compacted = 0;

        for msg in messages.iter_mut() {
            let msg_tokens = estimate_message_tokens(msg);
            if msg_tokens > self.config.micro_compact_threshold {
                match self.summarizer.micro_summarize(msg, self.config.micro_compact_threshold / 2) {
                    Ok(compressed) => {
                        msg.content = MessageContent::Text(compressed);
                        messages_compacted += 1;
                        tracing::debug!(
                            "Micro-compacted {} message ({} tokens -> compressed)",
                            msg.role,
                            msg_tokens
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to micro-compact message: {}",
                            e
                        );
                    }
                }
            }
        }

        let compacted_tokens = estimate_tokens(messages);
        let reduction_ratio = if original_tokens > 0 {
            1.0 - (compacted_tokens as f32 / original_tokens as f32)
        } else {
            0.0
        };

        Ok(CompactResult {
            original_tokens,
            compacted_tokens,
            reduction_ratio,
            messages_removed: 0,
            messages_compacted,
            duration: start.elapsed(),
            strategy: CompactStrategy::MicroCompress,
        })
    }

    // ========================================================================
    // Session Memory Compaction
    // ========================================================================

    /// Compress session memory entries by summarizing them
    pub fn compact_session_memory(
        &self,
        memory_entries: &[Message],
    ) -> Result<CompactResult, CompactError> {
        if !self.config.enable_session_memory_compact {
            return Ok(CompactResult::no_change(
                CompactStrategy::SessionMemoryCompress,
                estimate_tokens(memory_entries),
            ));
        }
        if memory_entries.is_empty() {
            return Err(CompactError::NoMessagesToCompact);
        }

        let start = Instant::now();
        let original_tokens = estimate_tokens(memory_entries);

        let summary = self.summarizer.summarize(
            memory_entries,
            self.config.max_output_tokens,
        )?;

        let compacted_tokens = estimate_message_tokens(&Message {
            role: "system".to_string(),
            content: MessageContent::Text(format!(
                "[Session memory summary]\n\n{summary}"
            )),
        });

        let reduction_ratio = if original_tokens > 0 {
            1.0 - (compacted_tokens as f32 / original_tokens as f32)
        } else {
            0.0
        };

        Ok(CompactResult {
            original_tokens,
            compacted_tokens,
            reduction_ratio,
            messages_removed: memory_entries.len().saturating_sub(1),
            messages_compacted: memory_entries.len(),
            duration: start.elapsed(),
            strategy: CompactStrategy::SessionMemoryCompress,
        })
    }

    // ========================================================================
    // Message Grouping
    // ========================================================================

    /// Group related messages for smarter compression.
    ///
    /// Groups consecutive messages by role, and additionally groups
    /// assistant tool_use blocks with their corresponding tool_result blocks.
    pub fn group_messages(&self, messages: &[Message]) -> Vec<MessageGroup> {
        if messages.is_empty() {
            return Vec::new();
        }

        let mut groups: Vec<MessageGroup> = Vec::new();
        let mut i = 0;

        while i < messages.len() {
            let msg = &messages[i];
            let tokens = estimate_message_tokens(msg);

            match msg.role.as_str() {
                "system" => {
                    let mut group_messages = vec![GroupedMessage {
                        message: msg.clone(),
                        original_index: i,
                        estimated_tokens: tokens,
                    }];
                    // Consume consecutive system messages
                    while i + 1 < messages.len() && messages[i + 1].role == "system" {
                        i += 1;
                        group_messages.push(GroupedMessage {
                            message: messages[i].clone(),
                            original_index: i,
                            estimated_tokens: estimate_message_tokens(&messages[i]),
                        });
                    }
                    groups.push(MessageGroup::SystemMessage {
                        messages: group_messages,
                    });
                }
                "user" => {
                    let mut group_messages = vec![GroupedMessage {
                        message: msg.clone(),
                        original_index: i,
                        estimated_tokens: tokens,
                    }];
                    // Consume consecutive user messages (including tool_result messages)
                    while i + 1 < messages.len() && messages[i + 1].role == "user" {
                        i += 1;
                        group_messages.push(GroupedMessage {
                            message: messages[i].clone(),
                            original_index: i,
                            estimated_tokens: estimate_message_tokens(&messages[i]),
                        });
                    }
                    groups.push(MessageGroup::UserTurn {
                        messages: group_messages,
                    });
                }
                "assistant" => {
                    // Check if this assistant message contains tool_use blocks
                    let tool_uses = extract_tool_uses(msg);

                    if !tool_uses.is_empty() {
                        // Group the assistant message with subsequent tool results
                        let mut group_messages = vec![GroupedMessage {
                            message: msg.clone(),
                            original_index: i,
                            estimated_tokens: tokens,
                        }];

                        // Look ahead for tool_result messages
                        let remaining = &messages[i + 1..];
                        let mut consumed = 0;

                        for (j, next_msg) in remaining.iter().enumerate() {
                            if next_msg.role == "user" {
                                // Check if it contains tool_result blocks matching our tool_uses
                                if contains_tool_result_for(next_msg, &tool_uses) {
                                    group_messages.push(GroupedMessage {
                                        message: next_msg.clone(),
                                        original_index: i + 1 + j,
                                        estimated_tokens: estimate_message_tokens(next_msg),
                                    });
                                    consumed = j + 1;
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }

                        // Use the first tool name as the group label
                        let tool_name = tool_uses[0].name.clone();
                        let tool_use_id = tool_uses[0].id.clone();
                        i += consumed;

                        groups.push(MessageGroup::ToolUseTurn {
                            tool_name,
                            tool_use_id,
                            messages: group_messages,
                        });
                    } else {
                        let mut group_messages = vec![GroupedMessage {
                            message: msg.clone(),
                            original_index: i,
                            estimated_tokens: tokens,
                        }];
                        // Consume consecutive assistant messages
                        while i + 1 < messages.len() && messages[i + 1].role == "assistant" {
                            i += 1;
                            group_messages.push(GroupedMessage {
                                message: messages[i].clone(),
                                original_index: i,
                                estimated_tokens: estimate_message_tokens(&messages[i]),
                            });
                        }
                        groups.push(MessageGroup::AssistantTurn {
                            messages: group_messages,
                        });
                    }
                }
                _ => {
                    // Unknown role, treat as system
                    groups.push(MessageGroup::SystemMessage {
                        messages: vec![GroupedMessage {
                            message: msg.clone(),
                            original_index: i,
                            estimated_tokens: tokens,
                        }],
                    });
                }
            }
            i += 1;
        }

        groups
    }

    // ========================================================================
    // Post-Compact Cleanup
    // ========================================================================

    /// Clean up after compression: remove duplicate summaries, fix references
    pub fn post_compact_cleanup(&self, messages: &mut Vec<Message>) -> usize {
        let original_count = messages.len();
        let mut seen_summaries: HashSet<String> = HashSet::new();
        let mut cleaned: Vec<Message> = Vec::new();
        let mut consecutive_system_count = 0;

        for msg in messages.drain(..) {
            if let MessageContent::Text(text) = &msg.content {
                if text.starts_with("[Previous conversation summary") {
                    // Deduplicate consecutive summaries
                    let key = text.to_string();
                    if seen_summaries.contains(&key) {
                        tracing::debug!("Removing duplicate summary message");
                        continue;
                    }
                    seen_summaries.insert(key);
                }
            }

            // Track consecutive system messages and collapse them
            if msg.role == "system" {
                consecutive_system_count += 1;
                if consecutive_system_count > 3 {
                    // Too many consecutive system messages, merge them
                    if let Some(last) = cleaned.last_mut() {
                        if let MessageContent::Text(existing) = &mut last.content {
                            if let MessageContent::Text(new) = &msg.content {
                                existing.push_str("\n\n");
                                existing.push_str(new);
                                continue;
                            }
                        }
                    }
                }
            } else {
                consecutive_system_count = 0;
            }

            cleaned.push(msg);
        }

        let removed = original_count - cleaned.len();
        *messages = cleaned;

        if removed > 0 {
            tracing::info!("Post-compact cleanup removed {} messages", removed);
        }

        removed
    }

    // ========================================================================
    // Group-Based Compression
    // ========================================================================

    /// Compress using message groups for smarter summarization
    pub fn group_compact(&mut self, messages: &mut Vec<Message>) -> Result<CompactResult, CompactError> {
        let original_tokens = estimate_tokens(messages);
        if messages.is_empty() {
            return Err(CompactError::NoMessagesToCompact);
        }
        if messages.len() <= self.config.keep_recent_count + 1 {
            return Ok(CompactResult::no_change(
                CompactStrategy::GroupCompress,
                original_tokens,
            ));
        }

        let start = Instant::now();

        // Group messages to understand the conversation structure
        let groups = self.group_messages(messages);
        let keep_count = self.config.keep_recent_count;

        // Use index-based splitting: summarize older messages, keep recent
        let split_point = messages.len().saturating_sub(keep_count);
        let old_messages: Vec<Message> = messages[..split_point].to_vec();
        let messages_removed = old_messages.len();

        // Count how many groups are affected
        let old_indices: HashSet<usize> = (0..split_point).collect();
        let affected_groups = groups
            .iter()
            .filter(|g| g.messages().iter().any(|gm| old_indices.contains(&gm.original_index)))
            .count();

        let summary = self.summarizer.summarize(&old_messages, self.config.max_output_tokens)?;

        let summary_message = Message {
            role: "system".to_string(),
            content: MessageContent::Text(format!(
                "[Group-compacted summary - {messages_removed} messages in {affected_groups} groups]\n\n{summary}"
            )),
        };

        messages.drain(..split_point);
        messages.insert(0, summary_message);

        let compacted_tokens = estimate_tokens(messages);
        let reduction_ratio = if original_tokens > 0 {
            1.0 - (compacted_tokens as f32 / original_tokens as f32)
        } else {
            0.0
        };

        Ok(CompactResult {
            original_tokens,
            compacted_tokens,
            reduction_ratio,
            messages_removed,
            messages_compacted: messages_removed,
            duration: start.elapsed(),
            strategy: CompactStrategy::GroupCompress,
        })
    }

    // ========================================================================
    // 3-Tier Compaction with Re-injection
    // ========================================================================

    /// Run 3-tier compaction: micro-compress → summarize old → re-inject context.
    ///
    /// Tier 1: Micro-compact individual large messages (>micro_compact_threshold tokens).
    /// Tier 2: Summarize old conversation turns (keeps recent `keep_recent_count` messages).
    /// Tier 3: Re-inject a context anchor message summarizing what was compacted
    ///         so the LLM retains key information about prior work.
    ///
    /// The `reinjection_context` string is appended to the context anchor message,
    /// allowing callers to inject project memory / CLAUDE.md content / git status.
    pub fn compact_tiered(
        &mut self,
        messages: &mut Vec<Message>,
        reinjection_context: Option<&str>,
    ) -> Result<CompactResult, CompactError> {
        if messages.is_empty() {
            return Err(CompactError::NoMessagesToCompact);
        }

        let original_tokens = estimate_tokens(messages);
        if messages.len() <= self.config.keep_recent_count + 1 {
            return Ok(CompactResult::no_change(CompactStrategy::SummarizeOld, original_tokens));
        }

        let start = Instant::now();
        let mut total_micro_compacted = 0usize;
        let mut total_removed = 0usize;

        // ── Tier 1: Micro-compact large individual messages ──
        if self.config.enable_micro_compact {
            for msg in messages.iter_mut() {
                let tokens = estimate_message_tokens(msg);
                if tokens > self.config.micro_compact_threshold {
                    match self.summarizer.micro_summarize(msg, self.config.max_output_tokens) {
                        Ok(summary) => {
                            msg.content = MessageContent::Text(format!(
                                "[Micro-compressed from {} tokens]\n{summary}",
                                tokens
                            ));
                            total_micro_compacted += 1;
                        }
                        Err(_) => {
                            // Truncate as fallback
                            let text = extract_text_content(msg);
                            let truncated = truncate_text(&text, self.config.max_output_tokens * 4);
                            msg.content = MessageContent::Text(format!(
                                "[Truncated from {} tokens]\n{truncated}",
                                tokens
                            ));
                            total_micro_compacted += 1;
                        }
                    }
                }
            }
        }

        // ── Tier 2: Summarize old conversation turns ──
        if messages.len() > self.config.keep_recent_count + 1 {
            let keep_count = self.config.keep_recent_count;
            let split_point = messages.len().saturating_sub(keep_count);

            // Gather files edited and tools used from old messages for re-injection
            let (files_touched, tools_used) = extract_compaction_metadata(&messages[..split_point]);

            let old_messages: Vec<Message> = messages[..split_point].to_vec();
            total_removed = old_messages.len();

            let summary = self.summarizer.summarize(&old_messages, self.config.max_output_tokens)?;

            // Build re-injection anchor
            let mut anchor_parts = vec![format!(
                "[Previous conversation compacted - {} messages summarized]",
                total_removed
            )];
            anchor_parts.push(String::new());
            anchor_parts.push(summary);

            if !files_touched.is_empty() {
                anchor_parts.push(format!(
                    "\nFiles touched: {}",
                    files_touched.iter().take(20).cloned().collect::<Vec<_>>().join(", ")
                ));
            }
            if !tools_used.is_empty() {
                anchor_parts.push(format!(
                    "\nTools used: {}",
                    tools_used.iter().take(10).cloned().collect::<Vec<_>>().join(", ")
                ));
            }
            if let Some(ctx) = reinjection_context {
                if !ctx.is_empty() {
                    anchor_parts.push(String::new());
                    anchor_parts.push("--- Project Context ---".to_string());
                    anchor_parts.push(ctx.to_string());
                }
            }

            let anchor_message = Message {
                role: "system".to_string(),
                content: MessageContent::Text(anchor_parts.join("\n")),
            };

            messages.drain(..split_point);
            messages.insert(0, anchor_message);
        }

        // ── Post-compaction cleanup ──
        self.post_compact_cleanup(messages);

        let compacted_tokens = estimate_tokens(messages);
        let reduction_ratio = if original_tokens > 0 {
            1.0 - (compacted_tokens as f32 / original_tokens as f32)
        } else {
            0.0
        };

        Ok(CompactResult {
            original_tokens,
            compacted_tokens,
            reduction_ratio,
            messages_removed: total_removed,
            messages_compacted: total_micro_compacted + total_removed,
            duration: start.elapsed(),
            strategy: CompactStrategy::SummarizeOld,
        })
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Extract all text content from a message
fn extract_text_content(msg: &Message) -> String {
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
                    ContentBlock::Thinking { .. } => {
                        // Thinking blocks are internal and omitted
                    }
                }
            }
            parts.join("\n")
        }
    }
}

/// Truncate text to a maximum character length at a word boundary
fn truncate_text(text: &str, max_chars: usize) -> String {
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

/// Estimate token count for a single message
fn estimate_message_tokens(msg: &Message) -> usize {
    let chars = match &msg.content {
        MessageContent::Text(text) => text.len(),
        MessageContent::Blocks(blocks) => {
            let mut total = 0;
            for block in blocks {
                match block {
                    ContentBlock::Text { text } => total += text.len(),
                    ContentBlock::ToolUse { name, input, .. } => {
                        total += name.len();
                        total += serde_json::to_string(input)
                            .map_or(0, |s| s.len());
                    }
                    ContentBlock::ToolResult { content, .. } => {
                        if let Some(c) = content {
                            match c {
                                ToolResultContent::Single(s) => total += s.len(),
                                ToolResultContent::Multiple(blocks) => {
                                    for b in blocks {
                                        if let ContentBlock::Text { text } = b {
                                            total += text.len();
                                        }
                                    }
                                }
                            }
                        }
                    }
                    ContentBlock::Image { .. } => total += 100, // rough image token estimate
                    ContentBlock::Thinking { thinking } => total += thinking.len(),
                }
            }
            total
        }
    };
    // ~4 characters per token (rough approximation)
    (chars / 4).max(1)
}

/// Estimate total token count for a slice of messages
fn estimate_tokens(messages: &[Message]) -> usize {
    messages.iter().map(estimate_message_tokens).sum()
}

/// Estimate tokens for a Vec of messages (owned)
fn original_tokens_from(messages: &[Message]) -> usize {
    estimate_tokens(messages)
}

/// Extract tool use info from a message
struct ToolUseInfo {
    id: String,
    name: String,
}

fn extract_tool_uses(msg: &Message) -> Vec<ToolUseInfo> {
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
fn contains_tool_result_for(msg: &Message, tool_uses: &[ToolUseInfo]) -> bool {
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

/// Extract file paths and tool names from messages for re-injection context.
fn extract_compaction_metadata(messages: &[Message]) -> (HashSet<String>, HashSet<String>) {
    let mut files = HashSet::new();
    let mut tools = HashSet::new();

    // Regex-free path extraction: look for common file extensions
    let extensions = [
        ".rs", ".ts", ".tsx", ".js", ".jsx", ".py", ".go", ".java", ".toml",
        ".json", ".yaml", ".yml", ".md", ".html", ".css", ".sql", ".sh",
    ];

    for msg in messages {
        let text = extract_text_content(msg);

        // Extract tool names from ToolUse blocks
        if let MessageContent::Blocks(blocks) = &msg.content {
            for block in blocks {
                if let ContentBlock::ToolUse { name, input, .. } = block {
                    tools.insert(name.clone());
                    // Extract file paths from tool input
                    if let Some(path) = input.get("file_path").and_then(|v| v.as_str()) {
                        files.insert(path.to_string());
                    }
                    if let Some(path) = input.get("path").and_then(|v| v.as_str()) {
                        files.insert(path.to_string());
                    }
                    if let Some(cmd) = input.get("command").and_then(|v| v.as_str()) {
                        // Quick scan for paths in command strings
                        for word in cmd.split_whitespace() {
                            if extensions.iter().any(|ext| word.ends_with(ext)) {
                                files.insert(word.to_string());
                            }
                        }
                    }
                }
            }
        }

        // Extract file paths from text content
        for word in text.split_whitespace() {
            // Clean up surrounding punctuation
            let cleaned = word.trim_matches(|c: char| c == '"' || c == '\'' || c == '`' || c == ',' || c == ':' || c == ';');
            if extensions.iter().any(|ext| cleaned.ends_with(ext)) && cleaned.len() < 300 {
                files.insert(cleaned.to_string());
            }
        }
    }

    (files, tools)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Helper functions for test data --

    fn user_msg(text: &str) -> Message {
        Message {
            role: "user".to_string(),
            content: MessageContent::Text(text.to_string()),
        }
    }

    fn assistant_msg(text: &str) -> Message {
        Message {
            role: "assistant".to_string(),
            content: MessageContent::Text(text.to_string()),
        }
    }

    fn system_msg(text: &str) -> Message {
        Message {
            role: "system".to_string(),
            content: MessageContent::Text(text.to_string()),
        }
    }

    fn tool_use_msg(id: &str, name: &str, input: &str) -> Message {
        Message {
            role: "assistant".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
                id: id.to_string(),
                name: name.to_string(),
                input: serde_json::json!({"command": input}),
            }]),
        }
    }

    fn tool_result_msg(tool_use_id: &str, result: &str) -> Message {
        Message {
            role: "user".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: tool_use_id.to_string(),
                content: Some(ToolResultContent::Single(result.to_string())),
                is_error: Some(false),
            }]),
        }
    }

    fn large_user_msg() -> Message {
        // Create a message that exceeds the default micro_compact_threshold of 4096 tokens
        // ~4 chars per token, so we need ~16384 characters
        let long_text = "A".repeat(20000);
        user_msg(&long_text)
    }

    // -- CompactConfig tests --

    #[test]
    fn test_compact_config_default() {
        let config = CompactConfig::default();
        assert_eq!(config.max_output_tokens, 2000);
        assert_eq!(config.keep_recent_count, 10);
        assert!((config.trigger_threshold - 0.8).abs() < 0.001);
        assert!(config.enable_micro_compact);
        assert_eq!(config.micro_compact_threshold, 4096);
        assert!(config.enable_session_memory_compact);
        assert_eq!(config.max_context_tokens, 200_000);
    }

    #[test]
    fn test_compact_config_with_max_context() {
        let config = CompactConfig::with_max_context(100_000);
        assert_eq!(config.max_context_tokens, 100_000);
        assert_eq!(config.keep_recent_count, 10); // other defaults preserved
    }

    #[test]
    fn test_compact_config_validate_ok() {
        let config = CompactConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_compact_config_validate_zero_output_tokens() {
        let config = CompactConfig {
            max_output_tokens: 0,
            ..Default::default()
        };
        assert!(matches!(
            config.validate(),
            Err(CompactError::InvalidConfig(_))
        ));
    }

    #[test]
    fn test_compact_config_validate_zero_keep_count() {
        let config = CompactConfig {
            keep_recent_count: 0,
            ..Default::default()
        };
        assert!(matches!(
            config.validate(),
            Err(CompactError::InvalidConfig(_))
        ));
    }

    #[test]
    fn test_compact_config_validate_bad_threshold() {
        let config = CompactConfig {
            trigger_threshold: 0.0,
            ..Default::default()
        };
        assert!(matches!(
            config.validate(),
            Err(CompactError::InvalidConfig(_))
        ));

        let config = CompactConfig {
            trigger_threshold: 1.5,
            ..Default::default()
        };
        assert!(matches!(
            config.validate(),
            Err(CompactError::InvalidConfig(_))
        ));
    }

    // -- CompactEngine creation --

    #[test]
    fn test_engine_with_defaults() {
        let engine = CompactEngine::with_defaults();
        assert!(engine.is_ok());
        let engine = engine.unwrap();
        assert_eq!(engine.config().keep_recent_count, 10);
    }

    #[test]
    fn test_engine_invalid_config_rejected() {
        let config = CompactConfig {
            max_output_tokens: 0,
            ..Default::default()
        };
        let result = CompactEngine::new(config, Box::new(RuleBasedSummarizer::new()));
        assert!(result.is_err());
    }

    // -- Context analysis --

    #[test]
    fn test_analyze_context_empty() {
        let engine = CompactEngine::with_defaults().unwrap();
        let analysis = engine.analyze_context(&[]);
        assert_eq!(analysis.estimated_tokens, 0);
        assert!(!analysis.should_compact);
        assert_eq!(analysis.compactable_message_count, 0);
        assert_eq!(analysis.micro_compact_candidates, 0);
    }

    #[test]
    fn test_analyze_context_below_threshold() {
        let engine = CompactEngine::with_defaults().unwrap();
        let messages = vec![user_msg("Hello"), assistant_msg("Hi there")];
        let analysis = engine.analyze_context(&messages);
        assert!(!analysis.should_compact);
        assert_eq!(analysis.compactable_message_count, 0);
    }

    #[test]
    fn test_analyze_context_above_threshold() {
        let engine = CompactEngine::new(
            CompactConfig {
                max_context_tokens: 100,
                trigger_threshold: 0.8,
                keep_recent_count: 2,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        // Create messages that exceed the threshold
        let mut messages = Vec::new();
        for i in 0..50 {
            messages.push(user_msg(&format!(
                "This is message number {i} with enough text to accumulate tokens"
            )));
        }

        let analysis = engine.analyze_context(&messages);
        assert!(analysis.should_compact);
        assert!(analysis.compactable_message_count > 0);
    }

    #[test]
    fn test_analyze_context_micro_compact_candidates() {
        let engine = CompactEngine::with_defaults().unwrap();
        let messages = vec![
            user_msg("Small message"),
            large_user_msg(),
            assistant_msg("Normal response"),
        ];
        let analysis = engine.analyze_context(&messages);
        assert_eq!(analysis.micro_compact_candidates, 1);
    }

    // -- Auto-compact check --

    #[test]
    fn test_auto_compact_check_false() {
        let engine = CompactEngine::with_defaults().unwrap();
        let messages = vec![user_msg("Hello"), assistant_msg("Hi")];
        assert!(!engine.auto_compact_check(&messages));
    }

    #[test]
    fn test_auto_compact_check_true() {
        let engine = CompactEngine::new(
            CompactConfig {
                max_context_tokens: 50,
                trigger_threshold: 0.5,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut messages = Vec::new();
        for _ in 0..30 {
            messages.push(user_msg("Long enough message to add tokens"));
        }
        assert!(engine.auto_compact_check(&messages));
    }

    // -- Full compaction --

    #[test]
    fn test_compact_empty_messages() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        let mut messages: Vec<Message> = Vec::new();
        let result = engine.compact(&mut messages);
        assert!(matches!(result, Err(CompactError::NoMessagesToCompact)));
    }

    #[test]
    fn test_compact_too_few_messages() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        let mut messages = vec![user_msg("Hello"), assistant_msg("Hi")];
        let result = engine.compact(&mut messages);
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.messages_removed, 0);
        assert_eq!(result.strategy, CompactStrategy::SummarizeOld);
        // Messages should be unchanged
        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn test_compact_reduces_messages() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        let mut messages = Vec::new();
        for i in 0..20 {
            messages.push(user_msg(&format!("User message {i}")));
            messages.push(assistant_msg(&format!("Assistant response {i}")));
        }

        let original_count = messages.len();
        let result = engine.compact(&mut messages).unwrap();

        assert!(result.messages_removed > 0);
        assert!(messages.len() < original_count);
        // Should have: 1 summary + 10 recent = 11
        assert_eq!(messages.len(), 11);
        // First message should be the summary
        assert_eq!(messages[0].role, "system");
    }

    #[test]
    fn test_compact_preserves_recent_messages() {
        let mut engine = CompactEngine::new(
            CompactConfig {
                keep_recent_count: 4,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut messages = Vec::new();
        for i in 0..10 {
            messages.push(user_msg(&format!("User msg {i}")));
            messages.push(assistant_msg(&format!("Asst msg {i}")));
        }

        engine.compact(&mut messages).unwrap();

        // Should have 1 summary + 4 recent = 5
        assert_eq!(messages.len(), 5);

        // Last 4 should be the original recent messages
        let last_user = &messages[messages.len() - 2];
        let last_asst = &messages[messages.len() - 1];
        assert_eq!(last_user.role, "user");
        assert!(matches!(&last_user.content, MessageContent::Text(t) if t.contains("User msg 9")));
        assert_eq!(last_asst.role, "assistant");
    }

    #[test]
    fn test_compact_result_metrics() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        let mut messages = Vec::new();
        for i in 0..20 {
            messages.push(user_msg(&format!("Message {i}")));
            messages.push(assistant_msg(&format!("Response {i}")));
        }

        let result = engine.compact(&mut messages).unwrap();
        assert!(result.original_tokens > 0);
        assert!(result.duration >= Duration::ZERO);
        assert!(result.reduction_ratio >= 0.0);
        assert!(result.messages_compacted > 0);
    }

    // -- Micro compaction --

    #[test]
    fn test_micro_compact_large_messages() {
        let engine = CompactEngine::with_defaults().unwrap();
        let mut messages = vec![
            user_msg("Normal message"),
            large_user_msg(),
            assistant_msg("Normal response"),
        ];

        let result = engine.micro_compact(&mut messages).unwrap();
        assert_eq!(result.messages_compacted, 1);
        assert_eq!(result.strategy, CompactStrategy::MicroCompress);
        // The large message should now be compressed
        match &messages[1].content {
            MessageContent::Text(text) => {
                assert!(text.contains("Compressed"));
                assert!(text.len() < 20000);
            }
            _ => panic!("Expected text content after micro-compact"),
        }
    }

    #[test]
    fn test_micro_compact_disabled() {
        let engine = CompactEngine::new(
            CompactConfig {
                enable_micro_compact: false,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut messages = vec![user_msg("Hello"), large_user_msg()];
        let result = engine.micro_compact(&mut messages).unwrap();
        assert_eq!(result.messages_compacted, 0);
        // Messages unchanged
        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn test_micro_compact_empty() {
        let engine = CompactEngine::with_defaults().unwrap();
        let mut messages: Vec<Message> = Vec::new();
        let result = engine.micro_compact(&mut messages).unwrap();
        assert_eq!(result.messages_compacted, 0);
    }

    // -- Session memory compaction --

    #[test]
    fn test_session_memory_compact() {
        let engine = CompactEngine::with_defaults().unwrap();
        // Use longer memory entries so the summarizer can actually compress them
        let long_memory = "X".repeat(500);
        let memory_entries = vec![
            system_msg(&format!("Memory 1: {long_memory}")),
            system_msg(&format!("Memory 2: {long_memory}")),
            system_msg(&format!("Memory 3: {long_memory}")),
            system_msg(&format!("Memory 4: {long_memory}")),
            system_msg(&format!("Memory 5: {long_memory}")),
        ];

        let result = engine.compact_session_memory(&memory_entries).unwrap();
        assert!(result.messages_removed > 0);
        assert!(result.strategy == CompactStrategy::SessionMemoryCompress);
        // Rule-based summarizer truncates to ~150 chars per message, so with 5 long
        // entries the summary should be smaller than the originals
        assert!(result.reduction_ratio >= 0.0);
        assert!(result.original_tokens > result.compacted_tokens);
    }

    #[test]
    fn test_session_memory_compact_disabled() {
        let engine = CompactEngine::new(
            CompactConfig {
                enable_session_memory_compact: false,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let entries = vec![system_msg("Some memory")];
        let result = engine.compact_session_memory(&entries).unwrap();
        assert_eq!(result.messages_removed, 0);
    }

    #[test]
    fn test_session_memory_compact_empty() {
        let engine = CompactEngine::with_defaults().unwrap();
        let result = engine.compact_session_memory(&[]);
        assert!(matches!(result, Err(CompactError::NoMessagesToCompact)));
    }

    // -- Message grouping --

    #[test]
    fn test_group_messages_basic() {
        let engine = CompactEngine::with_defaults().unwrap();
        let messages = vec![
            user_msg("Hello"),
            assistant_msg("Hi there"),
            user_msg("How are you?"),
        ];

        let groups = engine.group_messages(&messages);
        assert_eq!(groups.len(), 3);
        assert!(matches!(&groups[0], MessageGroup::UserTurn { .. }));
        assert!(matches!(&groups[1], MessageGroup::AssistantTurn { .. }));
        assert!(matches!(&groups[2], MessageGroup::UserTurn { .. }));
    }

    #[test]
    fn test_group_messages_tool_use() {
        let engine = CompactEngine::with_defaults().unwrap();
        let messages = vec![
            user_msg("Run ls"),
            tool_use_msg("tool_1", "bash", "ls"),
            tool_result_msg("tool_1", "file1.txt\nfile2.txt"),
            assistant_msg("Here are your files"),
        ];

        let groups = engine.group_messages(&messages);
        // Should have: UserTurn, ToolUseTurn, AssistantTurn
        assert_eq!(groups.len(), 3);
        assert!(matches!(&groups[0], MessageGroup::UserTurn { .. }));
        assert!(matches!(
            &groups[1],
            MessageGroup::ToolUseTurn {
                tool_name,
                messages,
                ..
            } if tool_name == "bash" && messages.len() == 2
        ));
    }

    #[test]
    fn test_group_messages_consecutive_same_role() {
        let engine = CompactEngine::with_defaults().unwrap();
        let messages = vec![
            system_msg("System instruction 1"),
            system_msg("System instruction 2"),
            user_msg("Hello"),
        ];

        let groups = engine.group_messages(&messages);
        // Two system messages should be grouped together
        assert_eq!(groups.len(), 2);
        assert!(matches!(
            &groups[0],
            MessageGroup::SystemMessage { messages } if messages.len() == 2
        ));
    }

    #[test]
    fn test_group_messages_empty() {
        let engine = CompactEngine::with_defaults().unwrap();
        let groups = engine.group_messages(&[]);
        assert!(groups.is_empty());
    }

    // -- Post-compact cleanup --

    #[test]
    fn test_post_compact_cleanup_removes_duplicate_summaries() {
        let engine = CompactEngine::with_defaults().unwrap();
        let mut messages = vec![
            system_msg("[Previous conversation summary]\nContent A"),
            system_msg("[Previous conversation summary]\nContent A"),
            user_msg("Next question"),
        ];

        let removed = engine.post_compact_cleanup(&mut messages);
        assert_eq!(removed, 1);
        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn test_post_compact_cleanup_collapses_consecutive_system() {
        let engine = CompactEngine::with_defaults().unwrap();
        let mut messages = vec![
            system_msg("Summary 1"),
            system_msg("Summary 2"),
            system_msg("Summary 3"),
            system_msg("Summary 4"),
            system_msg("Summary 5"),
            user_msg("Question"),
        ];

        let removed = engine.post_compact_cleanup(&mut messages);
        assert!(removed > 0);
        // After cleanup, we should have fewer than 6 messages
        assert!(messages.len() < 6);
    }

    #[test]
    fn test_post_compact_cleanup_noop() {
        let engine = CompactEngine::with_defaults().unwrap();
        let mut messages = vec![
            user_msg("Hello"),
            assistant_msg("Hi"),
            user_msg("Question"),
        ];

        let removed = engine.post_compact_cleanup(&mut messages);
        assert_eq!(removed, 0);
        assert_eq!(messages.len(), 3);
    }

    // -- CompactResult display --

    #[test]
    fn test_compact_result_display() {
        let result = CompactResult {
            original_tokens: 10000,
            compacted_tokens: 3000,
            reduction_ratio: 0.7,
            messages_removed: 15,
            messages_compacted: 15,
            duration: Duration::from_millis(150),
            strategy: CompactStrategy::SummarizeOld,
        };

        let display = format!("{result}");
        assert!(display.contains("10000"));
        assert!(display.contains("3000"));
        assert!(display.contains("70.0%"));
        assert!(display.contains("15"));
        assert!(display.contains("summarize_old"));
    }

    #[test]
    fn test_compact_result_no_change() {
        let result = CompactResult::no_change(CompactStrategy::TruncateOld, 5000);
        assert_eq!(result.original_tokens, 5000);
        assert_eq!(result.compacted_tokens, 5000);
        assert_eq!(result.reduction_ratio, 0.0);
        assert_eq!(result.messages_removed, 0);
    }

    // -- Strategy display --

    #[test]
    fn test_strategy_display() {
        assert_eq!(format!("{}", CompactStrategy::TruncateOld), "truncate_old");
        assert_eq!(format!("{}", CompactStrategy::SummarizeOld), "summarize_old");
        assert_eq!(format!("{}", CompactStrategy::MicroCompress), "micro_compress");
        assert_eq!(format!("{}", CompactStrategy::GroupCompress), "group_compress");
        assert_eq!(format!("{}", CompactStrategy::SessionMemoryCompress), "session_memory_compress");
    }

    // -- MessageGroup --

    #[test]
    fn test_message_group_total_tokens() {
        let group = MessageGroup::UserTurn {
            messages: vec![
                GroupedMessage {
                    message: user_msg("Hello world"),
                    original_index: 0,
                    estimated_tokens: 3,
                },
                GroupedMessage {
                    message: user_msg("Second message"),
                    original_index: 1,
                    estimated_tokens: 4,
                },
            ],
        };
        assert_eq!(group.total_tokens(), 7);
    }

    #[test]
    fn test_message_group_label() {
        let group = MessageGroup::ToolUseTurn {
            tool_name: "bash".to_string(),
            tool_use_id: "tool_1".to_string(),
            messages: vec![GroupedMessage {
                message: tool_use_msg("tool_1", "bash", "ls"),
                original_index: 0,
                estimated_tokens: 5,
            }],
        };
        let label = group.label();
        assert!(label.contains("bash"));
        assert!(label.contains("1 messages"));
    }

    // -- CompactPrompt --

    #[test]
    fn test_compact_prompt_system_prompt() {
        let prompt = CompactPrompt::system_prompt(1000);
        assert!(prompt.contains("1000"));
        assert!(prompt.contains("summary"));
    }

    #[test]
    fn test_compact_prompt_conversation_to_summarize() {
        let messages = vec![user_msg("Hello"), assistant_msg("Hi there")];
        let prompt = CompactPrompt::conversation_to_summarize(&messages);
        assert!(prompt.contains("[user]: Hello"));
        assert!(prompt.contains("[assistant]: Hi there"));
    }

    #[test]
    fn test_compact_prompt_micro_compact() {
        let msg = user_msg("Some very long content here");
        let prompt = CompactPrompt::micro_compact_prompt(&msg, 500);
        assert!(prompt.contains("500"));
        assert!(prompt.contains("user"));
    }

    // -- Token estimation helpers --

    #[test]
    fn test_estimate_message_tokens_text() {
        let msg = user_msg("Hello world"); // 11 chars
        let tokens = estimate_message_tokens(&msg);
        // 11 / 4 = 2 (rounded down), but max(1)
        assert!((1..=5).contains(&tokens));
    }

    #[test]
    fn test_estimate_message_tokens_blocks() {
        let msg = tool_use_msg("t1", "bash", "echo hello");
        let tokens = estimate_message_tokens(&msg);
        assert!(tokens > 0);
    }

    #[test]
    fn test_estimate_tokens_slice() {
        let messages = vec![user_msg("A"), assistant_msg("B")];
        let tokens = estimate_tokens(&messages);
        assert!(tokens > 0);
    }

    #[test]
    fn test_truncate_text_short() {
        assert_eq!(truncate_text("Hello", 100), "Hello");
    }

    #[test]
    fn test_truncate_text_long() {
        let result = truncate_text(&"A".repeat(200), 100);
        assert!(result.len() < 200);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_extract_text_content_text_variant() {
        let msg = user_msg("Hello world");
        assert_eq!(extract_text_content(&msg), "Hello world");
    }

    #[test]
    fn test_extract_text_content_blocks_variant() {
        let msg = tool_use_msg("t1", "bash", "ls -la");
        let content = extract_text_content(&msg);
        assert!(content.contains("bash"));
        assert!(content.contains("[Tool:"));
    }

    // -- RuleBasedSummarizer --

    #[test]
    fn test_rule_based_summarizer_empty() {
        let summarizer = RuleBasedSummarizer::new();
        let result = summarizer.summarize(&[], 1000);
        assert!(matches!(result, Err(CompactError::NoMessagesToCompact)));
    }

    #[test]
    fn test_rule_based_summarizer_basic() {
        let summarizer = RuleBasedSummarizer::new();
        let messages = vec![user_msg("Hello"), assistant_msg("Hi there!")];
        let result = summarizer.summarize(&messages, 1000);
        assert!(result.is_ok());
        let summary = result.unwrap();
        assert!(summary.contains("User"));
        assert!(summary.contains("Hello"));
        assert!(summary.contains("Assistant"));
    }

    #[test]
    fn test_rule_based_summarizer_tool_uses() {
        let summarizer = RuleBasedSummarizer::new();
        let messages = vec![
            user_msg("Run ls"),
            tool_use_msg("t1", "bash", "ls"),
        ];
        let result = summarizer.summarize(&messages, 1000).unwrap();
        assert!(result.contains("bash"));
        assert!(result.contains("Tools used"));
    }

    #[test]
    fn test_rule_based_summarizer_file_paths() {
        let summarizer = RuleBasedSummarizer::new();
        let messages = vec![user_msg("Look at src/main.rs and Cargo.toml")];
        let result = summarizer.summarize(&messages, 1000).unwrap();
        assert!(result.contains("src/main.rs"));
        assert!(result.contains("Cargo.toml"));
        assert!(result.contains("Files referenced"));
    }

    #[test]
    fn test_rule_based_summarizer_errors() {
        let summarizer = RuleBasedSummarizer::new();
        let messages = vec![Message {
            role: "user".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: "t1".to_string(),
                content: Some(ToolResultContent::Single("Command not found".to_string())),
                is_error: Some(true),
            }]),
        }];
        let result = summarizer.summarize(&messages, 1000).unwrap();
        assert!(result.contains("Errors encountered"));
    }

    #[test]
    fn test_rule_based_micro_summarize() {
        let summarizer = RuleBasedSummarizer::new();
        let msg = user_msg("Long content that should be compressed");
        let result = summarizer.micro_summarize(&msg, 100);
        assert!(result.is_ok());
        let summary = result.unwrap();
        assert!(summary.contains("Compressed"));
        assert!(summary.contains("user"));
    }

    // -- Group compact --

    #[test]
    fn test_group_compact() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        let mut messages = Vec::new();
        for i in 0..20 {
            messages.push(user_msg(&format!("User message {i}")));
            messages.push(assistant_msg(&format!("Response {i}")));
        }

        let original_count = messages.len();
        let result = engine.group_compact(&mut messages).unwrap();
        assert!(result.messages_removed > 0);
        assert!(messages.len() < original_count);
        assert_eq!(messages[0].role, "system");
        assert!(matches!(&messages[0].content, MessageContent::Text(t) if t.contains("Group-compacted")));
    }

    #[test]
    fn test_group_compact_too_few_messages() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        let mut messages = vec![user_msg("Hello"), assistant_msg("Hi")];
        let result = engine.group_compact(&mut messages).unwrap();
        assert_eq!(result.messages_removed, 0);
        assert_eq!(messages.len(), 2); // unchanged
    }

    // -- Integration: full workflow --

    #[test]
    fn test_full_workflow_compact_then_cleanup() {
        let mut engine = CompactEngine::new(
            CompactConfig {
                max_context_tokens: 200,
                trigger_threshold: 0.5,
                keep_recent_count: 4,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut messages = Vec::new();
        for i in 0..25 {
            // Use longer messages to exceed the small max_context_tokens threshold
            messages.push(user_msg(&format!(
                "User message {i} with extra padding text to ensure we exceed token budget"
            )));
            messages.push(assistant_msg(&format!(
                "Response {i} with extra padding text to ensure we exceed token budget significantly"
            )));
        }

        // Auto-compact check
        assert!(engine.auto_compact_check(&messages));

        // Full compact
        let result = engine.compact(&mut messages).unwrap();
        assert!(result.messages_removed > 0);

        // Post-cleanup should not remove anything if there's only one summary
        let cleanup_removed = engine.post_compact_cleanup(&mut messages);
        assert_eq!(cleanup_removed, 0);
    }

    #[test]
    fn test_full_workflow_analyze_then_micro_compact() {
        let engine = CompactEngine::with_defaults().unwrap();

        let mut messages = vec![
            user_msg("Normal"),
            large_user_msg(),
            assistant_msg("Normal response"),
        ];

        let analysis = engine.analyze_context(&messages);
        assert_eq!(analysis.micro_compact_candidates, 1);

        let result = engine.micro_compact(&mut messages).unwrap();
        assert_eq!(result.messages_compacted, 1);
    }

    // -- Edge case: system prompt preserved during compression --

    #[test]
    fn test_compact_preserves_system_prompt_at_front() {
        let mut engine = CompactEngine::new(
            CompactConfig {
                keep_recent_count: 4,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut messages = vec![
            system_msg("You are a helpful coding assistant."),
        ];
        for i in 0..15 {
            messages.push(user_msg(&format!("User query {i}")));
            messages.push(assistant_msg(&format!("Response {i}")));
        }

        engine.compact(&mut messages).unwrap();

        // The original system prompt should still be present somewhere
        let has_system_prompt = messages.iter().any(|m| {
            matches!(&m.content, MessageContent::Text(t) if t.contains("helpful coding assistant"))
        });
        assert!(has_system_prompt, "System prompt should be preserved after compaction");
    }

    // -- Edge case: concurrent compact guard --

    #[test]
    fn test_compact_already_in_progress() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        // Manually set the compacting flag to simulate concurrent access
        engine.compacting = true;
        let mut messages = vec![user_msg("test")];
        let result = engine.compact(&mut messages);
        assert!(matches!(result, Err(CompactError::AlreadyInProgress)));
    }

    // -- Edge case: token estimation within reasonable bounds --

    #[test]
    fn test_token_estimation_reasonable_bounds() {
        // 100 chars ≈ 25 tokens (at 4 chars/token)
        let msg = user_msg(&"A".repeat(100));
        let tokens = estimate_message_tokens(&msg);
        assert!((20..=30).contains(&tokens), "100 chars should be ~25 tokens, got {tokens}");

        // 1000 chars ≈ 250 tokens
        let msg = user_msg(&"B".repeat(1000));
        let tokens = estimate_message_tokens(&msg);
        assert!((240..=260).contains(&tokens), "1000 chars should be ~250 tokens, got {tokens}");

        // Single char = 1 token (min(1))
        let msg = user_msg("X");
        let tokens = estimate_message_tokens(&msg);
        assert_eq!(tokens, 1);
    }

    // -- Edge case: group_compact with mixed tool_use and text messages --

    #[test]
    fn test_group_compact_mixed_tool_and_text() {
        let mut engine = CompactEngine::new(
            CompactConfig {
                keep_recent_count: 4,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut messages = vec![
            user_msg("Look at my project"),
            assistant_msg("Let me check."),
            tool_use_msg("t1", "bash", "find . -name '*.rs'"),
            tool_result_msg("t1", "src/main.rs\nsrc/lib.rs"),
            assistant_msg("I found your Rust files."),
            user_msg("How many lines?"),
            tool_use_msg("t2", "bash", "wc -l src/main.rs"),
            tool_result_msg("t2", "42 src/main.rs"),
            assistant_msg("42 lines total."),
            user_msg("Add a new function"),
            assistant_msg("Done! I've added the function."),
            user_msg("Now run the tests"),
            assistant_msg("All tests pass."),
            user_msg("Great, commit it"),
            assistant_msg("Committed."),
        ];

        let original_count = messages.len();
        let result = engine.group_compact(&mut messages).unwrap();
        assert!(result.messages_removed > 0);
        assert!(messages.len() < original_count);
        // Recent messages should be preserved
        let last_msg = messages.last().unwrap();
        assert_eq!(last_msg.role, "assistant");
    }

    // -- compact_tiered (3-tier compaction) --

    #[test]
    fn test_compact_tiered_empty_messages() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        let mut messages: Vec<Message> = vec![];
        let result = engine.compact_tiered(&mut messages, None);
        assert!(matches!(result, Err(CompactError::NoMessagesToCompact)));
    }

    #[test]
    fn test_compact_tiered_too_few_messages() {
        let mut engine = CompactEngine::with_defaults().unwrap();
        let mut messages = vec![user_msg("Hello"), assistant_msg("Hi")];
        let result = engine.compact_tiered(&mut messages, None).unwrap();
        // Default keep_recent_count is 10, so 2 messages => no change
        assert_eq!(result.messages_removed, 0);
        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn test_compact_tiered_basic() {
        let mut engine = CompactEngine::new(
            CompactConfig {
                keep_recent_count: 4,
                enable_micro_compact: false,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut messages: Vec<Message> = vec![];
        for i in 0..20 {
            messages.push(user_msg(&format!("User message {i}")));
            messages.push(assistant_msg(&format!("Response {i}")));
        }
        let original_count = messages.len();

        let result = engine.compact_tiered(&mut messages, None).unwrap();
        assert!(result.messages_removed > 0);
        assert!(messages.len() < original_count);
        // First message should be the anchor/summary
        assert_eq!(messages[0].role, "system");
        if let MessageContent::Text(t) = &messages[0].content {
            assert!(t.contains("compacted"));
        } else {
            panic!("Expected text content in anchor message");
        }
    }

    #[test]
    fn test_compact_tiered_with_reinjection() {
        let mut engine = CompactEngine::new(
            CompactConfig {
                keep_recent_count: 2,
                enable_micro_compact: false,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut messages: Vec<Message> = vec![];
        for i in 0..10 {
            messages.push(user_msg(&format!("Message {i}")));
            messages.push(assistant_msg(&format!("Reply {i}")));
        }

        let ctx = "Project: shannon-code\nBuild: cargo build";
        let result = engine.compact_tiered(&mut messages, Some(ctx)).unwrap();
        assert!(result.messages_removed > 0);

        // Check re-injection context appears in anchor
        let anchor = &messages[0];
        if let MessageContent::Text(t) = &anchor.content {
            assert!(t.contains("Project Context"));
            assert!(t.contains("shannon-code"));
        } else {
            panic!("Expected text content in anchor");
        }
    }

    #[test]
    fn test_compact_tiered_micro_compact() {
        let mut engine = CompactEngine::new(
            CompactConfig {
                keep_recent_count: 2,
                enable_micro_compact: true,
                micro_compact_threshold: 50, // very low threshold
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut messages: Vec<Message> = vec![];
        // One very large message that should trigger micro-compact
        messages.push(user_msg(&"A".repeat(500)));
        for i in 0..10 {
            messages.push(user_msg(&format!("Normal message {i}")));
            messages.push(assistant_msg(&format!("Reply {i}")));
        }

        let result = engine.compact_tiered(&mut messages, None).unwrap();
        assert!(result.messages_compacted > 0);
        // Original first message should be micro-compacted
        assert!(messages.iter().any(|m| {
            if let MessageContent::Text(t) = &m.content {
                t.contains("Micro-compressed") || t.contains("Truncated")
            } else {
                false
            }
        }));
    }

    #[test]
    fn test_compact_tiered_extracts_file_metadata() {
        let mut engine = CompactEngine::new(
            CompactConfig {
                keep_recent_count: 2,
                enable_micro_compact: false,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut messages: Vec<Message> = vec![];
        messages.push(user_msg("Edit src/main.rs"));
        messages.push(tool_use_msg("t1", "bash", "cat src/main.rs"));
        messages.push(tool_result_msg("t1", "fn main() {}"));
        messages.push(assistant_msg("I see main.rs"));
        messages.push(user_msg("Also check lib.rs"));
        messages.push(tool_use_msg("t2", "bash", "cat lib.rs"));
        messages.push(tool_result_msg("t2", "pub fn foo() {}"));
        messages.push(assistant_msg("Found foo in lib.rs"));
        messages.push(user_msg("Recent message"));
        messages.push(assistant_msg("Recent reply"));

        let result = engine.compact_tiered(&mut messages, None).unwrap();
        assert!(result.messages_removed > 0);

        let anchor = &messages[0];
        if let MessageContent::Text(t) = &anchor.content {
            assert!(t.contains("Files touched"), "Should list files touched: {t}");
            assert!(t.contains("Tools used"), "Should list tools used: {t}");
        }
    }

    #[test]
    fn test_compact_tiered_empty_reinjection_ignored() {
        let mut engine = CompactEngine::new(
            CompactConfig {
                keep_recent_count: 2,
                enable_micro_compact: false,
                ..Default::default()
            },
            Box::new(RuleBasedSummarizer::new()),
        )
        .unwrap();

        let mut messages: Vec<Message> = vec![];
        for i in 0..10 {
            messages.push(user_msg(&format!("Msg {i}")));
            messages.push(assistant_msg(&format!("Reply {i}")));
        }

        let _result = engine.compact_tiered(&mut messages, Some("")).unwrap();
        let anchor = &messages[0];
        if let MessageContent::Text(t) = &anchor.content {
            assert!(!t.contains("Project Context"), "Empty reinjection should be ignored");
        }
    }

    // -- extract_compaction_metadata --

    #[test]
    fn test_extract_compaction_metadata_empty() {
        let (files, tools) = extract_compaction_metadata(&[]);
        assert!(files.is_empty());
        assert!(tools.is_empty());
    }

    #[test]
    fn test_extract_compaction_metadata_from_tool_use() {
        let messages = vec![
            tool_use_msg("t1", "bash", "cat src/main.rs"),
            tool_use_msg("t2", "edit_file", ""),
        ];
        let (files, tools) = extract_compaction_metadata(&messages);
        assert!(tools.contains("bash"));
        assert!(tools.contains("edit_file"));
        assert!(files.contains("src/main.rs"));
    }

    #[test]
    fn test_extract_compaction_metadata_from_text() {
        let messages = vec![
            user_msg("Look at Cargo.toml and src/lib.rs for details"),
        ];
        let (files, _tools) = extract_compaction_metadata(&messages);
        assert!(files.contains("Cargo.toml"));
        assert!(files.contains("src/lib.rs"));
    }

    #[test]
    fn test_extract_compaction_metadata_from_tool_input_paths() {
        let msg = Message {
            role: "assistant".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
                id: "t1".to_string(),
                name: "read_file".to_string(),
                input: serde_json::json!({"file_path": "/home/user/project/config.toml"}),
            }]),
        };
        let (files, tools) = extract_compaction_metadata(&[msg]);
        assert!(files.contains("/home/user/project/config.toml"));
        assert!(tools.contains("read_file"));
    }

    #[test]
    fn test_extract_compaction_metadata_ignores_long_paths() {
        // Paths > 300 chars should be ignored
        let long_path = format!("src/{}.rs", "a".repeat(300));
        let messages = vec![user_msg(&format!("Check {long_path}"))];
        let (files, _) = extract_compaction_metadata(&messages);
        assert!(files.is_empty(), "Long paths should be ignored");
    }

    // -- Edge case: set_config validates new config --

    #[test]
    fn test_set_config_validates() {
        let mut engine = CompactEngine::with_defaults().unwrap();

        let valid = CompactConfig {
            max_output_tokens: 3000,
            ..Default::default()
        };
        assert!(engine.set_config(valid).is_ok());
        assert_eq!(engine.config().max_output_tokens, 3000);

        let invalid = CompactConfig {
            max_output_tokens: 0,
            ..Default::default()
        };
        assert!(engine.set_config(invalid).is_err());
        // Original config should be preserved after failed update
        assert_eq!(engine.config().max_output_tokens, 3000);
    }
}
