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

        // Respect max_tokens: estimate ~4 chars per token and trim summary_parts
        let max_chars = _max_tokens.saturating_mul(4);
        let header_budget = summary.len();
        let footer_budget = 200; // reserve for tools/files/errors sections
        let parts_budget = max_chars.saturating_sub(header_budget + footer_budget);

        let mut parts_text = summary_parts.join("\n");
        if parts_text.len() > parts_budget && parts_budget > 0 {
            // Truncate to budget, finding a valid UTF-8 char boundary
            let mut cut = parts_budget;
            while cut > 0 && !parts_text.is_char_boundary(cut) {
                cut -= 1;
            }
            parts_text.truncate(cut);
            parts_text.push_str("\n... (truncated)");
        }

        summary.push_str(&parts_text);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{ContentBlock, Message, MessageContent, ToolResultContent};
    use crate::compact::types::Summarizer;

    fn text_message(role: &str, text: &str) -> Message {
        Message { role: role.to_string(), content: MessageContent::Text(text.to_string()) }
    }

    // ── RuleBasedSummarizer::summarize ───────────────────────────────────

    #[test]
    fn test_summarize_empty_returns_error() {
        let s = RuleBasedSummarizer::new();
        let result = s.summarize(&[], 1000);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("No messages"));
    }

    #[test]
    fn test_summarize_single_user_text() {
        let s = RuleBasedSummarizer::new();
        let msgs = vec![text_message("user", "Hello, how are you?")];
        let result = s.summarize(&msgs, 1000).unwrap();
        assert!(result.contains("[Conversation summary"));
        assert!(result.contains("User: Hello"));
        assert!(result.contains("1 turns"));
    }

    #[test]
    fn test_summarize_multiple_roles() {
        let s = RuleBasedSummarizer::new();
        let msgs = vec![
            text_message("user", "What is Rust?"),
            text_message("assistant", "Rust is a systems programming language."),
            text_message("user", "Tell me more."),
        ];
        let result = s.summarize(&msgs, 2000).unwrap();
        assert!(result.contains("User: What is Rust?"));
        assert!(result.contains("Assistant: Rust is"));
        assert!(result.contains("3 turns"));
        assert!(result.contains("3 messages"));
    }

    #[test]
    fn test_summarize_system_role_label() {
        let s = RuleBasedSummarizer::new();
        let msgs = vec![text_message("system", "You are helpful.")];
        let result = s.summarize(&msgs, 1000).unwrap();
        assert!(result.contains("System: You are helpful"));
    }

    #[test]
    fn test_summarize_with_tool_use_and_result() {
        let s = RuleBasedSummarizer::new();
        let msgs = vec![
            Message {
                role: "assistant".to_string(),
                content: MessageContent::Blocks(vec![
                    ContentBlock::ToolUse {
                        id: "tu_1".to_string(),
                        name: "Read".to_string(),
                        input: serde_json::json!({"file_path": "/tmp/test.rs"}),
                    },
                ]),
            },
            Message {
                role: "user".to_string(),
                content: MessageContent::Blocks(vec![
                    ContentBlock::ToolResult {
                        tool_use_id: "tu_1".to_string(),
                        content: Some(ToolResultContent::Single("fn main() {}".to_string())),
                        is_error: Some(false),
                    },
                ]),
            },
        ];
        let result = s.summarize(&msgs, 2000).unwrap();
        assert!(result.contains("Tool: Read"));
        assert!(result.contains("Result [Read]"));
        assert!(result.contains("fn main()"));
        assert!(result.contains("Tools used: Read"));
    }

    #[test]
    fn test_summarize_tool_error() {
        let s = RuleBasedSummarizer::new();
        let msgs = vec![
            Message {
                role: "assistant".to_string(),
                content: MessageContent::Blocks(vec![
                    ContentBlock::ToolUse {
                        id: "tu_1".to_string(),
                        name: "Bash".to_string(),
                        input: serde_json::json!({"command": "rm -rf /"}),
                    },
                ]),
            },
            Message {
                role: "user".to_string(),
                content: MessageContent::Blocks(vec![
                    ContentBlock::ToolResult {
                        tool_use_id: "tu_1".to_string(),
                        content: Some(ToolResultContent::Single("Permission denied".to_string())),
                        is_error: Some(true),
                    },
                ]),
            },
        ];
        let result = s.summarize(&msgs, 2000).unwrap();
        assert!(result.contains("Result (error)"));
        assert!(result.contains("Permission denied"));
        assert!(result.contains("Errors encountered"));
    }

    #[test]
    fn test_summarize_file_path_extraction() {
        let s = RuleBasedSummarizer::new();
        let msgs = vec![text_message("user", "Read src/main.rs and Cargo.toml for me")];
        let result = s.summarize(&msgs, 2000).unwrap();
        assert!(result.contains("Files referenced"));
        assert!(result.contains("src/main.rs"));
        assert!(result.contains("Cargo.toml"));
    }

    #[test]
    fn test_summarize_truncation_with_small_budget() {
        let s = RuleBasedSummarizer::new();
        let long_text: String = "x ".repeat(500);
        let msgs = vec![text_message("user", &long_text)];
        let result = s.summarize(&msgs, 10).unwrap();
        assert!(result.contains("truncated") || result.len() < long_text.len());
    }

    #[test]
    fn test_summarize_image_block_omitted() {
        let s = RuleBasedSummarizer::new();
        let msgs = vec![Message {
            role: "user".to_string(),
            content: MessageContent::Blocks(vec![
                ContentBlock::Image { source: crate::api::ImageSource::base64("image/png", "abc") },
            ]),
        }];
        let result = s.summarize(&msgs, 1000).unwrap();
        assert!(result.contains("Image (omitted from summary)"));
    }

    #[test]
    fn test_summarize_thinking_block_skipped() {
        let s = RuleBasedSummarizer::new();
        let msgs = vec![Message {
            role: "assistant".to_string(),
            content: MessageContent::Blocks(vec![
                ContentBlock::Thinking { thinking: "deep thoughts".to_string() },
                ContentBlock::Text { text: "Here's my answer.".to_string() },
            ]),
        }];
        let result = s.summarize(&msgs, 1000).unwrap();
        assert!(!result.contains("deep thoughts"));
        assert!(result.contains("Here's my answer"));
    }

    #[test]
    fn test_summarize_tool_result_multiple_blocks() {
        let s = RuleBasedSummarizer::new();
        let msgs = vec![
            Message {
                role: "assistant".to_string(),
                content: MessageContent::Blocks(vec![
                    ContentBlock::ToolUse {
                        id: "tu_1".to_string(),
                        name: "Grep".to_string(),
                        input: serde_json::json!({"pattern": "fn main"}),
                    },
                ]),
            },
            Message {
                role: "user".to_string(),
                content: MessageContent::Blocks(vec![
                    ContentBlock::ToolResult {
                        tool_use_id: "tu_1".to_string(),
                        content: Some(ToolResultContent::Multiple(vec![
                            ContentBlock::Text { text: "main.rs:1:fn main()".to_string() },
                            ContentBlock::Text { text: "lib.rs:5:fn main_test()".to_string() },
                        ])),
                        is_error: Some(false),
                    },
                ]),
            },
        ];
        let result = s.summarize(&msgs, 2000).unwrap();
        assert!(result.contains("main.rs:1"));
        assert!(result.contains("lib.rs:5"));
    }

    #[test]
    fn test_summarize_tool_result_empty() {
        let s = RuleBasedSummarizer::new();
        let msgs = vec![
            Message {
                role: "assistant".to_string(),
                content: MessageContent::Blocks(vec![
                    ContentBlock::ToolUse {
                        id: "tu_1".to_string(),
                        name: "Read".to_string(),
                        input: serde_json::json!({"file_path": "/tmp/empty"}),
                    },
                ]),
            },
            Message {
                role: "user".to_string(),
                content: MessageContent::Blocks(vec![
                    ContentBlock::ToolResult {
                        tool_use_id: "tu_1".to_string(),
                        content: None,
                        is_error: None,
                    },
                ]),
            },
        ];
        let result = s.summarize(&msgs, 1000).unwrap();
        assert!(result.contains("(empty)"));
    }

    #[test]
    fn test_summarize_unknown_tool_result() {
        let s = RuleBasedSummarizer::new();
        let msgs = vec![Message {
            role: "user".to_string(),
            content: MessageContent::Blocks(vec![
                ContentBlock::ToolResult {
                    tool_use_id: "orphan_id".to_string(),
                    content: Some(ToolResultContent::Single("some output".to_string())),
                    is_error: Some(false),
                },
            ]),
        }];
        let result = s.summarize(&msgs, 1000).unwrap();
        assert!(result.contains("Result [unknown]"));
    }

    // ── RuleBasedSummarizer::micro_summarize ────────────────────────────

    #[test]
    fn test_micro_summarize_text() {
        let s = RuleBasedSummarizer::new();
        let msg = text_message("assistant", "A long response about Rust programming.");
        let result = s.micro_summarize(&msg, 1000).unwrap();
        assert!(result.contains("[Compressed assistant message]"));
        assert!(result.contains("Rust programming"));
    }

    #[test]
    fn test_micro_summarize_blocks() {
        let s = RuleBasedSummarizer::new();
        let msg = Message {
            role: "assistant".to_string(),
            content: MessageContent::Blocks(vec![
                ContentBlock::Text { text: "Hello world".to_string() },
            ]),
        };
        let result = s.micro_summarize(&msg, 1000).unwrap();
        assert!(result.contains("[Compressed assistant message]"));
        assert!(result.contains("Hello world"));
    }

    // ── Default / Debug / Send+Sync ──────────────────────────────────────

    #[test]
    fn test_default() {
        let s = RuleBasedSummarizer::default();
        let msgs = vec![text_message("user", "hi")];
        assert!(s.summarize(&msgs, 100).is_ok());
    }

    #[test]
    fn test_debug_impl() {
        let s = RuleBasedSummarizer::new();
        let dbg = format!("{s:?}");
        assert!(dbg.contains("RuleBasedSummarizer"));
    }

    #[test]
    fn test_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<RuleBasedSummarizer>();
    }
}
