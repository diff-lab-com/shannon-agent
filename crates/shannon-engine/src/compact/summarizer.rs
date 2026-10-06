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
                                let limit = super::helpers::tool_result_preview_limit(tool_name);
                                let result_text = match content {
                                    Some(ToolResultContent::Single(s)) => truncate_text(s, limit),
                                    Some(ToolResultContent::Multiple(blocks)) => {
                                        let text: String = blocks
                                            .iter()
                                            .filter_map(|b| match b {
                                                ContentBlock::Text { text } => Some(text.as_str()),
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
                                summary_parts.push(format!("Text: {}", truncate_text(text, 100)));
                                turn_count += 1;
                            }
                            ContentBlock::Image { .. } => {
                                summary_parts.push("Image (omitted from summary)".to_string());
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

    fn micro_summarize(
        &self,
        message: &Message,
        _max_tokens: usize,
    ) -> Result<String, CompactError> {
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
    /// With no stored handle each call to `summarize` / `micro_summarize`
    /// reuses the *ambient* tokio runtime when the caller is already inside
    /// one (blocking via `block_in_place`), and only builds a temporary
    /// runtime on a plain thread. Prefer `with_handle` when a specific
    /// runtime should be used.
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
    ///
    /// Runtime nesting rules (regression legacy-④: this used to build a
    /// fresh [`tokio::runtime::Runtime`] unconditionally, so any caller
    /// already inside a runtime — e.g. the agent-loop producer task running
    /// the loop-level compaction — died with "Cannot start a runtime from
    /// within a runtime" before the request was even sent):
    ///
    /// - ambient multi-thread runtime → hand the worker core back with
    ///   [`tokio::task::block_in_place`] and block on the stored handle
    ///   (or the ambient one when none is stored); this is the documented
    ///   way to block inside a runtime and introduces no new runtime;
    /// - ambient current-thread runtime → blocking can never be safe (the
    ///   single thread *is* the driver), so return `Err` and let the
    ///   caller fall back to the rule-based summarizer instead of panicking;
    /// - no ambient runtime (plain thread) → the stored handle, or a fresh
    ///   temporary runtime when none was given — the historical behavior,
    ///   byte-identical.
    fn block_on_llm<F, T>(&self, fut: F) -> Result<T, String>
    where
        F: std::future::Future<Output = Result<T, String>>,
    {
        if let Ok(ambient) = tokio::runtime::Handle::try_current() {
            if ambient.runtime_flavor() != tokio::runtime::RuntimeFlavor::MultiThread {
                return Err("refusing to block inside a current-thread runtime; \
                     the rule-based fallback will be used"
                    .to_string());
            }
            let handle = self.runtime_handle.clone().unwrap_or(ambient);
            return tokio::task::block_in_place(|| handle.block_on(fut));
        }
        match &self.runtime_handle {
            Some(handle) => handle.block_on(fut),
            None => match tokio::runtime::Runtime::new() {
                Ok(rt) => rt.block_on(fut),
                Err(e) => Err(format!("Failed to create runtime: {e}")),
            },
        }
    }

    /// Build the messages payload for a summarization request.
    fn build_summarize_messages(&self, messages: &[Message], max_tokens: usize) -> Vec<Message> {
        vec![
            Message {
                role: "system".to_string(),
                content: MessageContent::Text(CompactPrompt::system_prompt(max_tokens)),
            },
            Message {
                role: "user".to_string(),
                content: MessageContent::Text(CompactPrompt::conversation_to_summarize(messages)),
            },
        ]
    }

    /// Build the messages payload for a micro-compact request.
    fn build_micro_messages(&self, message: &Message, max_tokens: usize) -> Vec<Message> {
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
                content: MessageContent::Text(CompactPrompt::micro_compact_prompt(
                    message, max_tokens,
                )),
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

    fn micro_summarize(
        &self,
        message: &Message,
        max_tokens: usize,
    ) -> Result<String, CompactError> {
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
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::api::{ContentBlock, Message, MessageContent, ToolResultContent};
    use crate::compact::types::Summarizer;

    fn text_message(role: &str, text: &str) -> Message {
        Message {
            role: role.to_string(),
            content: MessageContent::Text(text.to_string()),
        }
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
                content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
                    id: "tu_1".to_string(),
                    name: "Read".to_string(),
                    input: serde_json::json!({"file_path": "/tmp/test.rs"}),
                }]),
            },
            Message {
                role: "user".to_string(),
                content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: "tu_1".to_string(),
                    content: Some(ToolResultContent::Single("fn main() {}".to_string())),
                    is_error: Some(false),
                }]),
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
                content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
                    id: "tu_1".to_string(),
                    name: "Bash".to_string(),
                    input: serde_json::json!({"command": "rm -rf /"}),
                }]),
            },
            Message {
                role: "user".to_string(),
                content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: "tu_1".to_string(),
                    content: Some(ToolResultContent::Single("Permission denied".to_string())),
                    is_error: Some(true),
                }]),
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
        let msgs = vec![text_message(
            "user",
            "Read src/main.rs and Cargo.toml for me",
        )];
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
            content: MessageContent::Blocks(vec![ContentBlock::Image {
                source: crate::api::ImageSource::base64("image/png", "abc"),
            }]),
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
                ContentBlock::Thinking {
                    thinking: "deep thoughts".to_string(),
                },
                ContentBlock::Text {
                    text: "Here's my answer.".to_string(),
                },
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
                content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
                    id: "tu_1".to_string(),
                    name: "Grep".to_string(),
                    input: serde_json::json!({"pattern": "fn main"}),
                }]),
            },
            Message {
                role: "user".to_string(),
                content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: "tu_1".to_string(),
                    content: Some(ToolResultContent::Multiple(vec![
                        ContentBlock::Text {
                            text: "main.rs:1:fn main()".to_string(),
                        },
                        ContentBlock::Text {
                            text: "lib.rs:5:fn main_test()".to_string(),
                        },
                    ])),
                    is_error: Some(false),
                }]),
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
                content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
                    id: "tu_1".to_string(),
                    name: "Read".to_string(),
                    input: serde_json::json!({"file_path": "/tmp/empty"}),
                }]),
            },
            Message {
                role: "user".to_string(),
                content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: "tu_1".to_string(),
                    content: None,
                    is_error: None,
                }]),
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
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: "orphan_id".to_string(),
                content: Some(ToolResultContent::Single("some output".to_string())),
                is_error: Some(false),
            }]),
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
            content: MessageContent::Blocks(vec![ContentBlock::Text {
                text: "Hello world".to_string(),
            }]),
        };
        let result = s.micro_summarize(&msg, 1000).unwrap();
        assert!(result.contains("[Compressed assistant message]"));
        assert!(result.contains("Hello world"));
    }

    // ── Default / Debug / Send+Sync ──────────────────────────────────────

    #[test]
    fn test_default() {
        let s = RuleBasedSummarizer;
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

    // ── LlmSummarizer: runtime nesting (legacy-④ regression) ─────────────

    /// Minimal local HTTP mock: answers every request with one fixed
    /// Anthropic-style non-streaming JSON message and records the raw
    /// request bodies (same shape as the core agent-loop wire mocks).
    struct WireMockServer {
        base_url: String,
        bodies: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl WireMockServer {
        fn start(text: &'static str) -> Self {
            use std::io::{Read, Write};

            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
            let port = listener.local_addr().expect("addr").port();
            let bodies = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let captured = bodies.clone();
            // Detached accept loop: lives until process exit (one connection
            // per request — every response carries `Connection: close`).
            std::thread::spawn(move || {
                for mut stream in listener.incoming().flatten() {
                    let mut buf = vec![0u8; 1 << 16];
                    let mut read = 0usize;
                    loop {
                        match stream.read(&mut buf[read..]) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => read += n,
                        }
                        let s = String::from_utf8_lossy(&buf[..read]).to_string();
                        if let Some(header_end) = s.find("\r\n\r\n") {
                            let cl: usize = s[..header_end]
                                .to_ascii_lowercase()
                                .split("\r\n")
                                .find_map(|l| l.strip_prefix("content-length:"))
                                .and_then(|v| v.trim().parse().ok())
                                .unwrap_or(0);
                            if read >= header_end + 4 + cl {
                                break;
                            }
                        }
                        if read == buf.len() {
                            break;
                        }
                    }
                    captured
                        .lock()
                        .unwrap()
                        .push(String::from_utf8_lossy(&buf[..read]).to_string());
                    let body = format!(
                        r#"{{"id":"msg_mock","role":"assistant","content":[{{"type":"text","text":"{text}"}}],"model":"mock-model","stop_reason":"end_turn","usage":{{"input_tokens":1,"output_tokens":1}}}}"#
                    );
                    let http = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    stream.write_all(http.as_bytes()).ok();
                    stream.flush().ok();
                }
            });
            Self {
                base_url: format!("http://127.0.0.1:{port}"),
                bodies,
            }
        }

        fn request_bodies(&self) -> Vec<String> {
            self.bodies.lock().unwrap().clone()
        }
    }

    fn mock_client(base_url: &str) -> crate::api::LlmClient {
        crate::api::LlmClient::new(crate::api::LlmClientConfig {
            alternate_api_keys: Vec::new(),
            thinking_type: None,
            api_key: "test-key".to_string(),
            base_url: base_url.to_string(),
            model: "mock-model".to_string(),
            provider: crate::api::LlmProvider::Anthropic,
            ..Default::default()
        })
    }

    fn summarize_input() -> Vec<Message> {
        (0..4)
            .map(|i| text_message("user", &format!("turn {i}: discussed module_{i}")))
            .collect()
    }

    /// Legacy-④ regression pin. The loop-level compaction constructs
    /// `LlmSummarizer::new` (no stored handle) INSIDE the agent-loop's
    /// producer task. Pre-fix, `block_on_llm` built a fresh `Runtime` there
    /// and the task died with "Cannot start a runtime from within a
    /// runtime" before the request was ever sent. Post-fix the ambient
    /// multi-thread runtime is reused via `block_in_place` and the
    /// summarization request physically reaches the wire.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn llm_summarize_from_inside_a_multi_thread_runtime_hits_the_wire() {
        let server = WireMockServer::start("LLM SUMMARY BODY");
        let client = mock_client(&server.base_url);

        // Mirror the agent loop: summarize runs inside a spawned task on a
        // runtime worker, not on the `block_on` caller thread.
        let task = tokio::spawn(async move {
            let summarizer = LlmSummarizer::new(client);
            summarizer.summarize(&summarize_input(), 1000)
        });
        let summary = task
            .await
            .expect("summarize task must not panic")
            .expect("summarize must succeed");

        assert_eq!(summary, "LLM SUMMARY BODY");
        let bodies = server.request_bodies();
        assert_eq!(
            bodies.len(),
            1,
            "exactly one LLM request must physically reach the wire: {bodies:?}"
        );
        assert!(
            bodies[0].contains("mock-model"),
            "the request must carry the client's model: {}",
            bodies[0]
        );
        assert!(
            bodies[0].contains("turn 0"),
            "the request must carry the conversation being summarized: {}",
            bodies[0]
        );
    }

    /// Legacy-④ companion: inside a CURRENT-THREAD runtime blocking can
    /// never be safe (the single thread IS the driver). Pre-fix this
    /// panicked exactly like the multi-thread case; post-fix the
    /// summarizer degrades to the rule-based fallback instead of
    /// panicking, and no request is sent.
    #[tokio::test]
    async fn llm_summarize_inside_a_current_thread_runtime_falls_back_without_panicking() {
        let server = WireMockServer::start("LLM SUMMARY BODY");
        let summarizer = LlmSummarizer::new(mock_client(&server.base_url));

        let summary = summarizer
            .summarize(&summarize_input(), 1000)
            .expect("must fall back, not panic");

        assert!(
            summary.contains("[Conversation summary"),
            "expected the rule-based fallback shape, got: {summary}"
        );
        assert!(
            server.request_bodies().is_empty(),
            "no LLM request can be driven on a current-thread runtime"
        );
    }

    /// The `with_handle` contract (desktop/repl callers of
    /// `with_llm_summarizer_on_runtime`): a stored handle used from a
    /// plain thread outside any runtime keeps working exactly as before
    /// the fix — real request, no fallback.
    #[test]
    fn llm_summarize_with_stored_handle_from_a_plain_thread_hits_the_wire() {
        let server = WireMockServer::start("STORED HANDLE SUMMARY");
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let summarizer =
            LlmSummarizer::with_handle(mock_client(&server.base_url), rt.handle().clone());

        let summary = summarizer
            .summarize(&summarize_input(), 1000)
            .expect("summarize must succeed");

        assert_eq!(summary, "STORED HANDLE SUMMARY");
        assert_eq!(server.request_bodies().len(), 1);
    }
}
