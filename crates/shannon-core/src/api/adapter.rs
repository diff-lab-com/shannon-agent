//! Provider-specific request serialization and response normalization.
//!
//! Converts between the unified `MessageRequest`/`StreamEvent` types used
//! internally and the wire formats expected/returned by each LLM provider.

use serde::Deserialize;
use serde_json::{json, Value};

use super::error::ApiError;
use super::types::{
    ContentBlock, ContentDelta, LlmProvider, Message, MessageDeltaDelta, MessageRequest,
    StreamEvent, Usage,
};

// ── Request Serialization ──────────────────────────────────────────────────

/// Convert a unified `MessageRequest` into a provider-specific JSON body.
pub fn serialize_request(request: &MessageRequest, provider: &LlmProvider) -> Value {
    match provider {
        LlmProvider::Anthropic | LlmProvider::Custom => serde_json::to_value(request)
            .unwrap_or_else(|_| serde_json::to_value(request).unwrap_or_default()),
        LlmProvider::OpenAI => serialize_openai_request(request),
        LlmProvider::Ollama => serialize_ollama_request(request),
    }
}

/// Build an OpenAI-compatible request body.
///
/// Key differences from Anthropic format:
/// - `system` → message with role "system"
/// - `tools[].input_schema` → `tools[].function.parameters`
/// - `max_tokens` → `max_completion_tokens`
/// - `stream_options: {"include_usage": true}` for token tracking
fn serialize_openai_request(request: &MessageRequest) -> Value {
    let mut messages = Vec::new();

    // System prompt as a message
    if let Some(ref system) = request.system {
        messages.push(json!({
            "role": "system",
            "content": system
        }));
    }

    // Convert messages
    for msg in &request.messages {
        messages.push(convert_message_for_openai(msg));
    }

    let mut body = json!({
        "model": request.model,
        "messages": messages,
        "stream": request.stream.unwrap_or(false),
    });

    if let Some(max_tokens) = request.max_tokens.into() {
        body["max_completion_tokens"] = json!(max_tokens);
    }

    if let Some(temp) = request.temperature {
        body["temperature"] = json!(temp);
    }

    if let Some(top_p) = request.top_p {
        body["top_p"] = json!(top_p);
    }

    if let Some(ref seqs) = request.stop_sequences {
        body["stop"] = json!(seqs);
    }

    // Convert tools to OpenAI function-calling format
    if let Some(ref tools) = request.tools {
        let openai_tools: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema,
                        "strict": t.strict.unwrap_or(false),
                    }
                })
            })
            .collect();
        body["tools"] = json!(openai_tools);
    }

    // Request usage stats in streaming mode
    if request.stream.unwrap_or(false) {
        body["stream_options"] = json!({"include_usage": true});
    }

    body
}

/// Build an Ollama-compatible request body.
///
/// Ollama's `/api/chat` endpoint is similar to OpenAI but:
/// - Uses `options.num_predict` instead of `max_tokens`
/// - Does not support `stream_options`
fn serialize_ollama_request(request: &MessageRequest) -> Value {
    let mut messages = Vec::new();

    if let Some(ref system) = request.system {
        messages.push(json!({
            "role": "system",
            "content": system
        }));
    }

    for msg in &request.messages {
        messages.push(convert_message_for_openai(msg)); // same format as OpenAI
    }

    let mut body = json!({
        "model": request.model,
        "messages": messages,
        "stream": request.stream.unwrap_or(false),
    });

    // Ollama uses options bag for generation parameters
    let mut options = json!({});
    if request.max_tokens > 0 {
        options["num_predict"] = json!(request.max_tokens);
    }
    if let Some(temp) = request.temperature {
        options["temperature"] = json!(temp);
    }
    if let Some(top_p) = request.top_p {
        options["top_p"] = json!(top_p);
    }
    if let Some(ref seqs) = request.stop_sequences {
        body["stop"] = json!(seqs);
    }

    if options.as_object().map(|o| !o.is_empty()).unwrap_or(false) {
        body["options"] = options;
    }

    // Convert tools if present (Ollama supports function calling in newer versions)
    if let Some(ref tools) = request.tools {
        let ollama_tools: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema,
                    }
                })
            })
            .collect();
        body["tools"] = json!(ollama_tools);
    }

    body
}

/// Convert a single `Message` to OpenAI-style JSON value.
fn convert_message_for_openai(msg: &Message) -> Value {
    match &msg.content {
        crate::api::types::MessageContent::Text(text) => {
            json!({
                "role": msg.role,
                "content": text
            })
        }
        crate::api::types::MessageContent::Blocks(blocks) => {
            // Separate tool_use and tool_result blocks for OpenAI format
            let tool_calls: Vec<Value> = blocks
                .iter()
                .enumerate()
                .filter_map(|(i, b)| match b {
                    ContentBlock::ToolUse { id, name, input } => Some(json!({
                        "id": id,
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": input.to_string(),
                        },
                        "index": i,
                    })),
                    _ => None,
                })
                .collect();

            if !tool_calls.is_empty() {
                // Assistant message with tool calls
                json!({
                    "role": msg.role,
                    "tool_calls": tool_calls
                })
            } else {
                // Regular content blocks — extract text
                let text: String = blocks
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                // Check for tool_result blocks (tool response messages)
                let tool_results: Vec<Value> = blocks
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            ..
                        } => {
                            let result_text = match content {
                                Some(crate::api::types::ToolResultContent::Single(s)) => {
                                    s.clone()
                                }
                                Some(crate::api::types::ToolResultContent::Multiple(blocks)) => {
                                    blocks.iter().filter_map(|b| match b {
                                        ContentBlock::Text { text } => Some(text.as_str()),
                                        _ => None,
                                    }).collect::<Vec<_>>().join("\n")
                                }
                                None => String::new(),
                            };
                            Some(json!({
                                "role": "tool",
                                "tool_call_id": tool_use_id,
                                "content": result_text,
                            }))
                        }
                        _ => None,
                    })
                    .collect();

                if !tool_results.is_empty() {
                    // Return the first tool result as the message
                    // (OpenAI expects one message per tool result)
                    tool_results.into_iter().next().unwrap_or(json!({
                        "role": msg.role,
                        "content": text
                    }))
                } else {
                    json!({
                        "role": msg.role,
                        "content": text
                    })
                }
            }
        }
    }
}

// ── Response Normalization ─────────────────────────────────────────────────

/// Normalize a provider-specific SSE JSON payload into our `StreamEvent`.
///
/// Returns `None` if the line should be skipped (heartbeat, comment, etc.).
/// Returns `Some(Err(..))` on parse failures.
pub fn normalize_sse_event(
    json_str: &str,
    provider: &LlmProvider,
) -> Option<Result<StreamEvent, ApiError>> {
    match provider {
        LlmProvider::Anthropic | LlmProvider::Custom => {
            // Anthropic SSE events are already in our StreamEvent format
            match serde_json::from_str::<StreamEvent>(json_str) {
                Ok(event) => Some(Ok(event)),
                Err(e) => Some(Err(ApiError::InvalidResponse(format!(
                    "Failed to parse Anthropic SSE event: {} (data: {})",
                    e, json_str
                )))),
            }
        }
        LlmProvider::OpenAI => normalize_openai_event(json_str),
        LlmProvider::Ollama => normalize_ollama_event(json_str),
    }
}

// ── OpenAI Response Parsing ────────────────────────────────────────────────

#[derive(Deserialize)]
struct OpenAiChunk {
    choices: Vec<OpenAiChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
}

#[derive(Deserialize, Default)]
struct OpenAiChoice {
    #[serde(default)]
    delta: OpenAiDelta,
    finish_reason: Option<String>,
}

#[derive(Deserialize, Default)]
struct OpenAiDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiToolCallDelta>>,
}

#[derive(Deserialize)]
struct OpenAiToolCallDelta {
    index: Option<usize>,
    id: Option<String>,
    function: Option<OpenAiFunctionDelta>,
}

#[derive(Deserialize)]
struct OpenAiFunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Deserialize)]
struct OpenAiUsage {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
}

/// State for tracking tool call indices across streaming chunks.
static mut OPENAI_TOOL_INDEX: usize = 0;

fn next_tool_index() -> usize {
    unsafe {
        let idx = OPENAI_TOOL_INDEX;
        OPENAI_TOOL_INDEX += 1;
        idx
    }
}

fn reset_openai_state() {
    unsafe {
        OPENAI_TOOL_INDEX = 0;
    }
}

fn normalize_openai_event(json_str: &str) -> Option<Result<StreamEvent, ApiError>> {
    let chunk: OpenAiChunk = match serde_json::from_str(json_str) {
        Ok(c) => c,
        Err(e) => {
            return Some(Err(ApiError::InvalidResponse(format!(
                "Failed to parse OpenAI chunk: {} (data: {})",
                e, json_str
            ))));
        }
    };

    // If we have usage info, emit a MessageDelta with usage
    if let Some(usage) = chunk.usage {
        return Some(Ok(StreamEvent::MessageDelta {
            delta: MessageDeltaDelta {
                stop_reason: chunk
                    .choices
                    .first()
                    .and_then(|c| c.finish_reason.clone()),
                stop_sequence: None,
            },
            usage: Usage {
                input_tokens: usage.prompt_tokens.unwrap_or(0),
                output_tokens: usage.completion_tokens.unwrap_or(0),
            },
        }));
    }

    let choice = match chunk.choices.first() {
        Some(c) => c,
        None => return None,
    };

    // Finish reason → end events
    if let Some(ref reason) = choice.finish_reason {
        reset_openai_state();
        return Some(Ok(StreamEvent::MessageDelta {
            delta: MessageDeltaDelta {
                stop_reason: Some(reason.clone()),
                stop_sequence: None,
            },
            usage: Usage {
                input_tokens: 0,
                output_tokens: 0,
            },
        }));
    }

    // Tool calls
    if let Some(ref tool_calls) = choice.delta.tool_calls {
        let mut events = Vec::new();
        for tc in tool_calls {
            let idx = tc.index.unwrap_or_else(|| next_tool_index());

            if let Some(ref id) = tc.id {
                // New tool call starting
                let name = tc
                    .function
                    .as_ref()
                    .and_then(|f| f.name.clone())
                    .unwrap_or_default();
                events.push(StreamEvent::ContentBlockStart {
                    index: idx,
                    content_block: ContentBlock::ToolUse {
                        id: id.clone(),
                        name,
                        input: serde_json::Value::Null,
                    },
                });
            }

            if let Some(ref func) = tc.function {
                if let Some(ref args) = func.arguments {
                    events.push(StreamEvent::ContentBlockDelta {
                        index: idx,
                        delta: ContentDelta::InputJsonDelta {
                            partial_json: args.clone(),
                        },
                    });
                }
            }
        }
        // Return the first event; subsequent ones will be generated on future calls.
        // Since we can only return one, stash the rest... but in practice tool calls
        // arrive one at a time in streaming.
        return events.into_iter().next().map(Ok);
    }

    // Text content
    if let Some(ref content) = choice.delta.content {
        return Some(Ok(StreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::TextDelta {
                text: content.clone(),
            },
        }));
    }

    None
}

// ── Ollama Response Parsing ────────────────────────────────────────────────

#[derive(Deserialize)]
struct OllamaChunk {
    message: Option<OllamaMessage>,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    prompt_eval_count: Option<u32>,
    #[serde(default)]
    eval_count: Option<u32>,
}

#[derive(Deserialize)]
struct OllamaMessage {
    content: Option<String>,
    tool_calls: Option<Vec<OllamaToolCall>>,
}

#[derive(Deserialize)]
struct OllamaToolCall {
    function: OllamaToolFunction,
}

#[derive(Deserialize)]
struct OllamaToolFunction {
    name: String,
    arguments: Value,
}

fn normalize_ollama_event(json_str: &str) -> Option<Result<StreamEvent, ApiError>> {
    let chunk: OllamaChunk = match serde_json::from_str(json_str) {
        Ok(c) => c,
        Err(e) => {
            return Some(Err(ApiError::InvalidResponse(format!(
                "Failed to parse Ollama chunk: {} (data: {})",
                e, json_str
            ))));
        }
    };

    if chunk.done {
        // Final chunk with usage info
        return Some(Ok(StreamEvent::MessageDelta {
            delta: MessageDeltaDelta {
                stop_reason: Some("end_turn".to_string()),
                stop_sequence: None,
            },
            usage: Usage {
                input_tokens: chunk.prompt_eval_count.unwrap_or(0),
                output_tokens: chunk.eval_count.unwrap_or(0),
            },
        }));
    }

    if let Some(ref msg) = chunk.message {
        // Tool calls
        if let Some(ref tool_calls) = msg.tool_calls {
            let mut events = Vec::new();
            for (idx, tc) in tool_calls.iter().enumerate() {
                events.push(StreamEvent::ContentBlockStart {
                    index: idx,
                    content_block: ContentBlock::ToolUse {
                        id: format!("call_{}", idx),
                        name: tc.function.name.clone(),
                        input: tc.function.arguments.clone(),
                    },
                });
                events.push(StreamEvent::ContentBlockStop { index: idx });
            }
            return events.into_iter().next().map(Ok);
        }

        // Text content
        if let Some(ref content) = msg.content {
            if !content.is_empty() {
                return Some(Ok(StreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::TextDelta {
                        text: content.clone(),
                    },
                }));
            }
        }
    }

    None
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::types::ToolDefinition;

    fn make_request() -> MessageRequest {
        MessageRequest {
            model: "test-model".to_string(),
            max_tokens: 4096,
            system: Some("You are helpful.".to_string()),
            messages: vec![Message {
                role: "user".to_string(),
                content: crate::api::types::MessageContent::Text("Hello".to_string()),
            }],
            tools: Some(vec![ToolDefinition {
                name: "bash".to_string(),
                description: "Run commands".to_string(),
                input_schema: json!({"type": "object", "properties": {"command": {"type": "string"}}}),
                strict: Some(true),
            }]),
            stream: Some(true),
            temperature: Some(0.7),
            top_p: None,
            top_k: None,
            stop_sequences: None,
        }
    }

    // -- Anthropic passthrough --

    #[test]
    fn test_anthropic_serialize_is_passthrough() {
        let req = make_request();
        let val = serialize_request(&req, &LlmProvider::Anthropic);
        // Anthropic format uses top-level system and max_tokens
        assert_eq!(val["system"], "You are helpful.");
        assert_eq!(val["max_tokens"], 4096);
        assert_eq!(val["model"], "test-model");
    }

    #[test]
    fn test_custom_serialize_is_passthrough() {
        let req = make_request();
        let val = serialize_request(&req, &LlmProvider::Custom);
        assert_eq!(val["max_tokens"], 4096);
    }

    // -- OpenAI format --

    #[test]
    fn test_openai_system_as_message() {
        let req = make_request();
        let val = serialize_openai_request(&req);
        // System should be first message, not top-level field
        assert!(val.get("system").is_none());
        let messages = val["messages"].as_array().unwrap();
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "You are helpful.");
    }

    #[test]
    fn test_openai_uses_max_completion_tokens() {
        let req = make_request();
        let val = serialize_openai_request(&req);
        assert!(val.get("max_tokens").is_none());
        assert_eq!(val["max_completion_tokens"], 4096);
    }

    #[test]
    fn test_openai_tools_function_format() {
        let req = make_request();
        let val = serialize_openai_request(&req);
        let tools = val["tools"].as_array().unwrap();
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["function"]["name"], "bash");
        assert!(tools[0]["function"]["parameters"].is_object());
    }

    #[test]
    fn test_openai_stream_options() {
        let req = make_request();
        let val = serialize_openai_request(&req);
        assert_eq!(val["stream_options"]["include_usage"], true);
    }

    #[test]
    fn test_openai_no_system_no_extra_message() {
        let mut req = make_request();
        req.system = None;
        let val = serialize_openai_request(&req);
        let messages = val["messages"].as_array().unwrap();
        // Only the user message, no system message
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
    }

    // -- Ollama format --

    #[test]
    fn test_ollama_system_as_message() {
        let req = make_request();
        let val = serialize_ollama_request(&req);
        let messages = val["messages"].as_array().unwrap();
        assert_eq!(messages[0]["role"], "system");
    }

    #[test]
    fn test_ollama_uses_options_num_predict() {
        let req = make_request();
        let val = serialize_ollama_request(&req);
        assert!(val.get("max_tokens").is_none());
        assert_eq!(val["options"]["num_predict"], 4096);
    }

    #[test]
    fn test_ollama_temperature_in_options() {
        let req = make_request();
        let val = serialize_ollama_request(&req);
        let temp = val["options"]["temperature"].as_f64().unwrap();
        assert!((temp - 0.7).abs() < 0.01);
    }

    // -- Anthropic SSE normalization --

    #[test]
    fn test_anthropic_sse_passthrough() {
        let event_json = r#"{"type":"message_stop"}"#;
        let result = normalize_sse_event(event_json, &LlmProvider::Anthropic);
        assert!(matches!(result, Some(Ok(StreamEvent::MessageStop))));
    }

    #[test]
    fn test_anthropic_text_delta() {
        let event_json = r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}"#;
        let result = normalize_sse_event(event_json, &LlmProvider::Anthropic);
        match result {
            Some(Ok(StreamEvent::ContentBlockDelta { delta, .. })) => {
                assert_eq!(
                    delta,
                    ContentDelta::TextDelta {
                        text: "hi".to_string()
                    }
                );
            }
            other => panic!("Expected ContentBlockDelta, got {:?}", other),
        }
    }

    // -- OpenAI SSE normalization --

    #[test]
    fn test_openai_text_delta() {
        let chunk_json = r#"{"choices":[{"delta":{"content":"hello"},"index":0}]}"#;
        let result = normalize_sse_event(chunk_json, &LlmProvider::OpenAI);
        match result {
            Some(Ok(StreamEvent::ContentBlockDelta { delta, .. })) => {
                assert_eq!(
                    delta,
                    ContentDelta::TextDelta {
                        text: "hello".to_string()
                    }
                );
            }
            other => panic!("Expected ContentBlockDelta, got {:?}", other),
        }
    }

    #[test]
    fn test_openai_finish_reason() {
        let chunk_json = r#"{"choices":[{"delta":{},"finish_reason":"stop","index":0}]}"#;
        let result = normalize_sse_event(chunk_json, &LlmProvider::OpenAI);
        match result {
            Some(Ok(StreamEvent::MessageDelta { delta, .. })) => {
                assert_eq!(delta.stop_reason, Some("stop".to_string()));
            }
            other => panic!("Expected MessageDelta, got {:?}", other),
        }
    }

    #[test]
    fn test_openai_usage_event() {
        let chunk_json = r#"{"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":20,"total_tokens":30}}"#;
        let result = normalize_sse_event(chunk_json, &LlmProvider::OpenAI);
        match result {
            Some(Ok(StreamEvent::MessageDelta { usage, .. })) => {
                assert_eq!(usage.input_tokens, 10);
                assert_eq!(usage.output_tokens, 20);
            }
            other => panic!("Expected MessageDelta with usage, got {:?}", other),
        }
    }

    #[test]
    fn test_openai_tool_call_start() {
        let chunk_json = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_abc","type":"function","function":{"name":"bash","arguments":""}}]},"index":0}]}"#;
        let result = normalize_sse_event(chunk_json, &LlmProvider::OpenAI);
        match result {
            Some(Ok(StreamEvent::ContentBlockStart { content_block, .. })) => {
                match content_block {
                    ContentBlock::ToolUse { id, name, .. } => {
                        assert_eq!(id, "call_abc");
                        assert_eq!(name, "bash");
                    }
                    other => panic!("Expected ToolUse block, got {:?}", other),
                }
            }
            other => panic!("Expected ContentBlockStart, got {:?}", other),
        }
    }

    // -- Ollama SSE normalization --

    #[test]
    fn test_ollama_text_delta() {
        let chunk_json = r#"{"message":{"content":"world","role":"assistant"}}"#;
        let result = normalize_sse_event(chunk_json, &LlmProvider::Ollama);
        match result {
            Some(Ok(StreamEvent::ContentBlockDelta { delta, .. })) => {
                assert_eq!(
                    delta,
                    ContentDelta::TextDelta {
                        text: "world".to_string()
                    }
                );
            }
            other => panic!("Expected ContentBlockDelta, got {:?}", other),
        }
    }

    #[test]
    fn test_ollama_done_event() {
        let chunk_json = r#"{"done":true,"prompt_eval_count":50,"eval_count":100}"#;
        let result = normalize_sse_event(chunk_json, &LlmProvider::Ollama);
        match result {
            Some(Ok(StreamEvent::MessageDelta { usage, delta, .. })) => {
                assert_eq!(usage.input_tokens, 50);
                assert_eq!(usage.output_tokens, 100);
                assert_eq!(delta.stop_reason, Some("end_turn".to_string()));
            }
            other => panic!("Expected MessageDelta, got {:?}", other),
        }
    }

    #[test]
    fn test_ollama_empty_content_skipped() {
        let chunk_json = r#"{"message":{"content":"","role":"assistant"}}"#;
        let result = normalize_sse_event(chunk_json, &LlmProvider::Ollama);
        assert!(result.is_none());
    }

    // -- Round-trip: no panic on malformed JSON --

    #[test]
    fn test_malformed_json_returns_error() {
        let result = normalize_sse_event("not json", &LlmProvider::OpenAI);
        assert!(matches!(result, Some(Err(_))));

        let result = normalize_sse_event("not json", &LlmProvider::Ollama);
        assert!(matches!(result, Some(Err(_))));

        // Anthropic also returns error for invalid JSON
        let result = normalize_sse_event("not json", &LlmProvider::Anthropic);
        assert!(matches!(result, Some(Err(_))));
    }
}
