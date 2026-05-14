//! Summarizer implementations: rule-based and LLM-powered.

use crate::api::{ContentBlock, Message, MessageContent, ToolResultContent};
use std::collections::HashSet;

use super::helpers::{extract_text_content, truncate_text};
use super::types::{CompactError, CompactPrompt, Summarizer};

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
        let mut tool_name_map: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
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
                        if word.contains('/')
                            && (word.ends_with(".rs")
                                || word.ends_with(".toml")
                                || word.ends_with(".md")
                                || word.ends_with(".json")
                                || word.ends_with(".yaml")
                                || word.ends_with(".yml"))
                        {
                            file_paths.insert(word.to_string());
                        }
                    }
                    turn_count += 1;
                }
                MessageContent::Blocks(blocks) => {
                    for block in blocks {
                        match block {
                            ContentBlock::ToolUse {
                                id, name, input, ..
                            } => {
                                tool_names.insert(name.clone());
                                tool_name_map.insert(id.clone(), name.clone());
                                summary_parts.push(format!(
                                    "Tool: {}({})",
                                    name,
                                    truncate_text(
                                        &serde_json::to_string(input).unwrap_or_default(),
                                        100
                                    )
                                ));
                            }
                            ContentBlock::ToolResult {
                                tool_use_id,
                                content,
                                is_error,
                                ..
                            } => {
                                let is_err = is_error.unwrap_or(false);
                                let tool_name = tool_name_map
                                    .get(tool_use_id)
                                    .map(|s| s.as_str())
                                    .unwrap_or("unknown");
                                let limit =
                                    super::helpers::tool_result_preview_limit(tool_name);
                                let result_text = match content {
                                    Some(ToolResultContent::Single(s)) => {
                                        truncate_text(s, limit)
                                    }
                                    Some(ToolResultContent::Multiple(blocks)) => {
                                        let text: String = blocks
                                            .iter()
                                            .filter_map(|b| match b {
                                                ContentBlock::Text { text } => {
                                                    Some(text.as_str())
                                                }
                                                _ => None,
                                            })
                                            .collect::<Vec<_>>()
                                            .join(" ");
                                        truncate_text(&text, limit)
                                    }
                                    None => "(empty)".to_string(),
                                };
                                if is_err {
                                    errors_encountered.push(result_text.clone());
                                }
                                summary_parts.push(format!(
                                    "Result{} [{}]: {}",
                                    if is_err { " (error)" } else { "" },
                                    tool_name,
                                    result_text
                                ));
                            }
                            ContentBlock::Text { text } => {
                                summary_parts
                                    .push(format!("Text: {}", truncate_text(text, 100)));
                                turn_count += 1;
                            }
                            ContentBlock::Image { .. } => {
                                summary_parts
                                    .push("Image (omitted from summary)".to_string());
                            }
                            ContentBlock::Thinking { .. } => {}
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

// ============================================================================
// LLM-Based Summarizer
// ============================================================================

/// AI-powered summarizer that uses the configured LLM to produce high-quality
/// conversation summaries. Falls back to [`RuleBasedSummarizer`] on errors.
///
/// When created with [`LlmSummarizer::with_handle`], reuses an existing tokio
/// runtime instead of creating a new one per call — this avoids "cannot start a
/// runtime from within a runtime" panics and cross-runtime `reqwest` issues.
pub struct LlmSummarizer {
    client: crate::api::LlmClient,
    fallback: RuleBasedSummarizer,
    runtime_handle: Option<tokio::runtime::Handle>,
    compact_model: Option<String>,
}

impl LlmSummarizer {
    /// Create a new LLM summarizer wrapping the given client.
    ///
    /// Each call to `summarize` / `micro_summarize` will create a temporary
    /// tokio runtime. Prefer [`with_handle`] when a runtime is already available.
    pub fn new(client: crate::api::LlmClient) -> Self {
        Self {
            client,
            fallback: RuleBasedSummarizer::new(),
            runtime_handle: None,
            compact_model: None,
        }
    }

    /// Create an LLM summarizer that reuses an existing tokio runtime handle.
    ///
    /// This avoids creating a new runtime per summarization call, which can
    /// panic if called from within an existing runtime context.
    pub fn with_handle(client: crate::api::LlmClient, handle: tokio::runtime::Handle) -> Self {
        Self {
            client,
            fallback: RuleBasedSummarizer::new(),
            runtime_handle: Some(handle),
            compact_model: None,
        }
    }

    /// Set a model override for compaction (e.g. a smaller/cheaper model).
    pub fn with_compact_model(mut self, model: String) -> Self {
        self.compact_model = Some(model);
        self
    }

    /// Return a client clone with the compact model override applied (if set).
    fn compact_client(&self) -> crate::api::LlmClient {
        let mut client = self.client.clone();
        if let Some(ref model) = self.compact_model {
            client.set_model(model.clone());
        }
        client
    }

    /// Execute an async LLM call using the stored handle or a fresh runtime.
    fn block_on_llm<F, T>(&self, fut: F) -> Result<T, String>
    where
        F: std::future::Future<Output = Result<T, String>>,
    {
        if let Some(handle) = &self.runtime_handle {
            handle.block_on(fut)
        } else {
            match tokio::runtime::Runtime::new() {
                Ok(rt) => rt.block_on(fut),
                Err(e) => Err(format!("Failed to create runtime: {e}")),
            }
        }
    }

    /// Build the messages payload for a summarization request.
    fn build_summarize_messages(
        &self,
        messages: &[Message],
        max_tokens: usize,
    ) -> Vec<Message> {
        vec![
            Message {
                role: "system".to_string(),
                content: MessageContent::Text(CompactPrompt::system_prompt(max_tokens)),
            },
            Message {
                role: "user".to_string(),
                content: MessageContent::Text(
                    CompactPrompt::conversation_to_summarize(messages),
                ),
            },
        ]
    }

    /// Build the messages payload for a micro-compact request.
    fn build_micro_messages(
        &self,
        message: &Message,
        max_tokens: usize,
    ) -> Vec<Message> {
        vec![
            Message {
                role: "system".to_string(),
                content: MessageContent::Text(
                    "You are a content compression assistant. Compress the following \
                     message while preserving all key information, file paths, data values, \
                     and code references. Output ONLY the compressed text, no meta-commentary."
                        .to_string(),
                ),
            },
            Message {
                role: "user".to_string(),
                content: MessageContent::Text(CompactPrompt::micro_compact_prompt(message, max_tokens)),
            },
        ]
    }
}

impl std::fmt::Debug for LlmSummarizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmSummarizer")
            .field("model", &self.client.model())
            .finish()
    }
}

impl Summarizer for LlmSummarizer {
    fn summarize(&self, messages: &[Message], max_tokens: usize) -> Result<String, CompactError> {
        if messages.is_empty() {
            return Err(CompactError::NoMessagesToCompact);
        }

        let payload = self.build_summarize_messages(messages, max_tokens);
        let client = self.compact_client();

        let result = self.block_on_llm(async {
            match client.send_message(payload, None, None).await {
                Ok(blocks) => {
                    let text: String = blocks
                        .into_iter()
                        .filter_map(|b| match b {
                            ContentBlock::Text { text } => Some(text),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    if text.trim().is_empty() {
                        Err("LLM returned empty summary".to_string())
                    } else {
                        Ok(text)
                    }
                }
                Err(e) => Err(format!("LLM summarization API error: {e}")),
            }
        });

        match result {
            Ok(summary) => Ok(summary),
            Err(reason) => {
                tracing::warn!(
                    "LLM summarization failed ({}), falling back to rule-based",
                    reason
                );
                self.fallback.summarize(messages, max_tokens)
            }
        }
    }

    fn micro_summarize(&self, message: &Message, max_tokens: usize) -> Result<String, CompactError> {
        let payload = self.build_micro_messages(message, max_tokens);
        let client = self.compact_client();

        let result = self.block_on_llm(async {
            match client.send_message(payload, None, None).await {
                Ok(blocks) => {
                    let text: String = blocks
                        .into_iter()
                        .filter_map(|b| match b {
                            ContentBlock::Text { text } => Some(text),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    if text.trim().is_empty() {
                        Err("LLM returned empty micro-summary".to_string())
                    } else {
                        Ok(text)
                    }
                }
                Err(e) => Err(format!("LLM micro-summarization API error: {e}")),
            }
        });

        match result {
            Ok(summary) => Ok(summary),
            Err(reason) => {
                tracing::warn!(
                    "LLM micro-summarization failed ({}), falling back to rule-based",
                    reason
                );
                self.fallback.micro_summarize(message, max_tokens)
            }
        }
    }
}
