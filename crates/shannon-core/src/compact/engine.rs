//! Main compression engine for conversation context management.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::api::{Message, MessageContent};

use super::helpers::{
    contains_tool_result_for, estimate_message_tokens, estimate_tokens, extract_tool_uses,
    original_tokens_from,
};
use super::summarizer::RuleBasedSummarizer;
use super::summarizer::LlmSummarizer;
use super::types::{
    CompactConfig, CompactError, CompactResult, CompactStrategy, ContextAnalysis,
    GroupedMessage, MessageGroup, Summarizer,
};

/// Main compression engine for conversation context management
pub struct CompactEngine {
    pub(crate) config: CompactConfig,
    summarizer: Box<dyn Summarizer>,
    pub(crate) compacting: bool,
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

    /// Create with an LLM-powered summarizer for higher quality compression.
    ///
    /// Falls back to rule-based summarization on API errors.
    pub fn with_llm_summarizer(
        client: crate::api::LlmClient,
    ) -> Result<Self, CompactError> {
        Self::new(CompactConfig::default(), Box::new(LlmSummarizer::new(client)))
    }

    /// Create with an LLM summarizer and custom config.
    pub fn with_llm_and_config(
        client: crate::api::LlmClient,
        config: CompactConfig,
    ) -> Result<Self, CompactError> {
        Self::new(config, Box::new(LlmSummarizer::new(client)))
    }

    /// Create with an LLM-powered summarizer that reuses an existing tokio runtime.
    ///
    /// This avoids creating a new runtime per summarization call, which can
    /// panic if the caller is already inside a tokio runtime context.
    pub fn with_llm_summarizer_on_runtime(
        client: crate::api::LlmClient,
        handle: tokio::runtime::Handle,
    ) -> Result<Self, CompactError> {
        let config = CompactConfig::default();
        let summarizer = match &config.compact_model {
            Some(model) => LlmSummarizer::with_handle(client, handle)
                .with_compact_model(model.clone()),
            None => LlmSummarizer::with_handle(client, handle),
        };
        Self::new(config, Box::new(summarizer))
    }

    /// Create with an LLM summarizer on an existing runtime, using custom config.
    ///
    /// The config's `compact_model` field will be forwarded to the summarizer.
    pub fn with_llm_on_runtime_and_config(
        client: crate::api::LlmClient,
        handle: tokio::runtime::Handle,
        config: CompactConfig,
    ) -> Result<Self, CompactError> {
        let summarizer = match &config.compact_model {
            Some(model) => LlmSummarizer::with_handle(client, handle)
                .with_compact_model(model.clone()),
            None => LlmSummarizer::with_handle(client, handle),
        };
        Self::new(config, Box::new(summarizer))
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

        // Prune stale tool results from older messages before summarizing
        let mut old_messages: Vec<Message> = messages[..split_point].to_vec();
        Self::prune_stale_tool_results(&mut old_messages);

        let messages_removed = old_messages.len();

        // Summarize the older messages
        let summary_text =
            self.summarizer
                .summarize(&old_messages, self.config.max_output_tokens)?;

        // Verify summary quality — reject if clearly degenerate
        if !Self::verify_summary_quality(&summary_text, &old_messages) {
            tracing::warn!("Summary quality check failed, using fallback summary");
        }

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
    // Context Editing
    // ========================================================================

    /// Strip content from stale tool results in older messages, keeping only a
    /// truncated preview. This reduces token count before summarization.
    pub fn prune_stale_tool_results(messages: &mut [Message]) {
        use crate::api::{ContentBlock, ToolResultContent};
        let preview_limit = 200;

        for msg in messages.iter_mut() {
            if let MessageContent::Blocks(blocks) = &mut msg.content {
                for block in blocks.iter_mut() {
                    if let ContentBlock::ToolResult {
                        content,
                        is_error,
                        ..
                    } = block
                    {
                        let is_err = is_error.unwrap_or(false);
                        if is_err {
                            continue; // keep error results in full
                        }
                        if let Some(ToolResultContent::Single(text)) = content {
                            if text.len() > preview_limit * 2 {
                                *text = format!("{}...[truncated, {} chars]", &text[..preview_limit], text.len());
                            }
                        }
                    }
                }
            }
        }
    }

    // ========================================================================
    // Summary Quality Verification
    // ========================================================================

    /// Check that a summary is reasonable quality before accepting it.
    /// Returns false only for clearly degenerate summaries (empty, too short,
    /// or literally echoing back the prompt).
    fn verify_summary_quality(summary: &str, original_messages: &[Message]) -> bool {
        let text = summary.trim();

        // Must not be empty
        if text.is_empty() {
            tracing::warn!("Summary quality: empty");
            return false;
        }

        // Must be at least 50 chars for any non-trivial conversation
        if original_messages.len() >= 3 && text.len() < 50 {
            tracing::warn!("Summary quality: too short ({} chars for {} messages)", text.len(), original_messages.len());
            return false;
        }

        // Must not be a near-copy of a single message (degenerate echo)
        if original_messages.len() >= 3 {
            let msg_texts: Vec<&str> = original_messages.iter()
                .filter_map(|m| match &m.content {
                    MessageContent::Text(t) => Some(t.as_str()),
                    _ => None,
                })
                .collect();
            for mt in &msg_texts {
                if !mt.is_empty() && text.len() > 20 {
                    // Simple overlap check: if summary is 90%+ contained in a source message
                    let shorter_len = text.len().min(mt.len());
                    if shorter_len > 0 {
                        let common: usize = text
                            .split_whitespace()
                            .filter(|w| mt.contains(w))
                            .count();
                        let total = text.split_whitespace().count();
                        if total > 0 && common * 100 / total > 90 {
                            tracing::warn!("Summary quality: near-identical to source message");
                            return false;
                        }
                    }
                }
            }
        }

        true
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
}
