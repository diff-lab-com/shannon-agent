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
            .unwrap_or_default(),
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
/// Returns a `Vec` because a single SSE chunk can contain multiple logical
/// events (e.g. several simultaneous tool-call starts from OpenAI).
///
/// `openai_state` is only used for `OpenAI` provider and must be the
/// per-stream state — never shared across concurrent streams.
pub fn normalize_sse_event(
    json_str: &str,
    provider: &LlmProvider,
    openai_state: &mut OpenaiStreamState,
) -> Vec<Result<StreamEvent, ApiError>> {
    match provider {
        LlmProvider::Anthropic | LlmProvider::Custom => {
            // Anthropic SSE events are already in our StreamEvent format
            match serde_json::from_str::<StreamEvent>(json_str) {
                Ok(event) => vec![Ok(event)],
                Err(e) => vec![Err(ApiError::InvalidResponse(format!(
                    "Failed to parse Anthropic SSE event: {e} (data: {json_str})"
                )))],
            }
        }
        LlmProvider::OpenAI => normalize_openai_event(json_str, openai_state),
        LlmProvider::Ollama => normalize_ollama_event(json_str),
    }
}

// ── Non-Streaming Response Normalization ────────────────────────────────────

/// OpenAI non-streaming response shape (differs from Anthropic MessageResponse).
#[derive(Deserialize)]
struct OpenAiMessageResponse {
    id: Option<String>,
    choices: Vec<OpenAiMessageChoice>,
    model: Option<String>,
    usage: Option<OpenAiUsage>,
}

#[derive(Deserialize)]
struct OpenAiMessageChoice {
    message: OpenAiResponseMessage,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct OpenAiResponseMessage {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiResponseToolCall>>,
}

#[derive(Deserialize)]
struct OpenAiResponseToolCall {
    id: String,
    #[serde(rename = "type")]
    #[allow(dead_code)] // Deserialized from OpenAI wire format; used for type discrimination
    call_type: Option<String>,
    function: OpenAiResponseFunction,
}

#[derive(Deserialize)]
struct OpenAiResponseFunction {
    name: String,
    arguments: String,
}

/// Ollama non-streaming response shape.
#[derive(Deserialize)]
struct OllamaMessageResponse {
    message: Option<OllamaResponseMessage>,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    prompt_eval_count: Option<u32>,
    #[serde(default)]
    eval_count: Option<u32>,
    model: Option<String>,
}

#[derive(Deserialize)]
struct OllamaResponseMessage {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OllamaToolCall>>,
}

/// Normalize a provider-specific non-streaming JSON response into our
/// `MessageResponse` type used throughout the codebase.
///
/// Anthropic responses are already in `MessageResponse` format, so they pass
/// through directly. OpenAI and Ollama have different shapes and are converted.
pub fn normalize_response(
    json_str: &str,
    provider: &LlmProvider,
) -> Result<super::types::MessageResponse, ApiError> {
    match provider {
        LlmProvider::Anthropic | LlmProvider::Custom => {
            serde_json::from_str(json_str).map_err(|e| {
                ApiError::InvalidResponse(format!(
                    "Failed to parse Anthropic response: {e}"
                ))
            })
        }
        LlmProvider::OpenAI => {
            let resp: OpenAiMessageResponse = serde_json::from_str(json_str).map_err(|e| {
                ApiError::InvalidResponse(format!("Failed to parse OpenAI response: {e}"))
            })?;

            let choice = resp.choices.into_iter().next().ok_or_else(|| {
                ApiError::InvalidResponse("OpenAI response has no choices".to_string())
            })?;

            let mut content = Vec::new();

            // Text content
            if let Some(text) = choice.message.content {
                if !text.is_empty() {
                    content.push(ContentBlock::Text { text });
                }
            }

            // Tool calls
            if let Some(tool_calls) = choice.message.tool_calls {
                for tc in tool_calls {
                    let input: Value = serde_json::from_str(&tc.function.arguments).unwrap_or(Value::Null);
                    content.push(ContentBlock::ToolUse {
                        id: tc.id,
                        name: tc.function.name,
                        input,
                    });
                }
            }

            Ok(super::types::MessageResponse {
                id: resp.id.unwrap_or_default(),
                role: "assistant".to_string(),
                content,
                model: resp.model.unwrap_or_default(),
                stop_reason: choice.finish_reason,
                usage: resp
                    .usage
                    .map(|u| super::types::Usage {
                        input_tokens: u.prompt_tokens.unwrap_or(0),
                        output_tokens: u.completion_tokens.unwrap_or(0),
                    })
                    .unwrap_or(super::types::Usage {
                        input_tokens: 0,
                        output_tokens: 0,
                    }),
            })
        }
        LlmProvider::Ollama => {
            let resp: OllamaMessageResponse = serde_json::from_str(json_str).map_err(|e| {
                ApiError::InvalidResponse(format!("Failed to parse Ollama response: {e}"))
            })?;

            let msg = resp.message.ok_or_else(|| {
                ApiError::InvalidResponse("Ollama response has no message".to_string())
            })?;

            let mut content = Vec::new();

            if let Some(text) = msg.content {
                if !text.is_empty() {
                    content.push(ContentBlock::Text { text });
                }
            }

            if let Some(tool_calls) = msg.tool_calls {
                for (idx, tc) in tool_calls.into_iter().enumerate() {
                    content.push(ContentBlock::ToolUse {
                        id: format!("call_{idx}"),
                        name: tc.function.name,
                        input: tc.function.arguments,
                    });
                }
            }

            Ok(super::types::MessageResponse {
                id: String::new(),
                role: "assistant".to_string(),
                content,
                model: resp.model.unwrap_or_default(),
                stop_reason: if resp.done {
                    Some("end_turn".to_string())
                } else {
                    None
                },
                usage: super::types::Usage {
                    input_tokens: resp.prompt_eval_count.unwrap_or(0),
                    output_tokens: resp.eval_count.unwrap_or(0),
                },
            })
        }
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

/// Per-stream state for OpenAI response normalization.
///
/// Previously a `static mut` global — now owned by each `SseStream` to
/// avoid data races when multiple streams run concurrently.
pub struct OpenaiStreamState {
    pub tool_index: usize,
}

impl OpenaiStreamState {
    pub fn new() -> Self {
        Self { tool_index: 0 }
    }

    pub fn next_tool_index(&mut self) -> usize {
        let idx = self.tool_index;
        self.tool_index += 1;
        idx
    }

    pub fn reset(&mut self) {
        self.tool_index = 0;
    }
}

impl Default for OpenaiStreamState {
    fn default() -> Self {
        Self::new()
    }
}

fn normalize_openai_event(
    json_str: &str,
    state: &mut OpenaiStreamState,
) -> Vec<Result<StreamEvent, ApiError>> {
    let chunk: OpenAiChunk = match serde_json::from_str(json_str) {
        Ok(c) => c,
        Err(e) => {
            return vec![Err(ApiError::InvalidResponse(format!(
                "Failed to parse OpenAI chunk: {e} (data: {json_str})"
            )))];
        }
    };

    // If we have usage info, emit a MessageDelta with usage
    if let Some(usage) = chunk.usage {
        return vec![Ok(StreamEvent::MessageDelta {
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
        })];
    }

    let choice = match chunk.choices.first() {
        Some(c) => c,
        None => return vec![],
    };

    // Finish reason → end events
    if let Some(ref reason) = choice.finish_reason {
        state.reset();
        return vec![Ok(StreamEvent::MessageDelta {
            delta: MessageDeltaDelta {
                stop_reason: Some(reason.clone()),
                stop_sequence: None,
            },
            usage: Usage {
                input_tokens: 0,
                output_tokens: 0,
            },
        })];
    }

    // Tool calls — return ALL events, not just the first one
    if let Some(ref tool_calls) = choice.delta.tool_calls {
        let mut events = Vec::new();
        for tc in tool_calls {
            let idx = tc.index.unwrap_or_else(|| state.next_tool_index());

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
        return events.into_iter().map(Ok).collect();
    }

    // Text content
    if let Some(ref content) = choice.delta.content {
        return vec![Ok(StreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::TextDelta {
                text: content.clone(),
            },
        })];
    }

    vec![]
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

fn normalize_ollama_event(json_str: &str) -> Vec<Result<StreamEvent, ApiError>> {
    let chunk: OllamaChunk = match serde_json::from_str(json_str) {
        Ok(c) => c,
        Err(e) => {
            return vec![Err(ApiError::InvalidResponse(format!(
                "Failed to parse Ollama chunk: {e} (data: {json_str})"
            )))];
        }
    };

    if chunk.done {
        // Final chunk with usage info
        return vec![Ok(StreamEvent::MessageDelta {
            delta: MessageDeltaDelta {
                stop_reason: Some("end_turn".to_string()),
                stop_sequence: None,
            },
            usage: Usage {
                input_tokens: chunk.prompt_eval_count.unwrap_or(0),
                output_tokens: chunk.eval_count.unwrap_or(0),
            },
        })];
    }

    if let Some(ref msg) = chunk.message {
        // Tool calls — return ALL events (start + stop for each)
        if let Some(ref tool_calls) = msg.tool_calls {
            let mut events = Vec::new();
            for (idx, tc) in tool_calls.iter().enumerate() {
                events.push(StreamEvent::ContentBlockStart {
                    index: idx,
                    content_block: ContentBlock::ToolUse {
                        id: format!("call_{idx}"),
                        name: tc.function.name.clone(),
                        input: tc.function.arguments.clone(),
                    },
                });
                events.push(StreamEvent::ContentBlockStop { index: idx });
            }
            return events.into_iter().map(Ok).collect();
        }

        // Text content
        if let Some(ref content) = msg.content {
            if !content.is_empty() {
                return vec![Ok(StreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::TextDelta {
                        text: content.clone(),
                    },
                })];
            }
        }
    }

    vec![]
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

    fn fresh_state() -> OpenaiStreamState {
        OpenaiStreamState::new()
    }

    #[test]
    fn test_anthropic_sse_passthrough() {
        let event_json = r#"{"type":"message_stop"}"#;
        let result = normalize_sse_event(event_json, &LlmProvider::Anthropic, &mut fresh_state());
        assert!(result.len() == 1);
        assert!(matches!(&result[0], Ok(StreamEvent::MessageStop)));
    }

    #[test]
    fn test_anthropic_text_delta() {
        let event_json = r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}"#;
        let result = normalize_sse_event(event_json, &LlmProvider::Anthropic, &mut fresh_state());
        match &result[0] {
            Ok(StreamEvent::ContentBlockDelta { delta, .. }) => {
                assert_eq!(
                    delta,
                    &ContentDelta::TextDelta {
                        text: "hi".to_string()
                    }
                );
            }
            other => panic!("Expected ContentBlockDelta, got {other:?}"),
        }
    }

    // -- OpenAI SSE normalization --

    #[test]
    fn test_openai_text_delta() {
        let chunk_json = r#"{"choices":[{"delta":{"content":"hello"},"index":0}]}"#;
        let result = normalize_sse_event(chunk_json, &LlmProvider::OpenAI, &mut fresh_state());
        match &result[0] {
            Ok(StreamEvent::ContentBlockDelta { delta, .. }) => {
                assert_eq!(
                    delta,
                    &ContentDelta::TextDelta {
                        text: "hello".to_string()
                    }
                );
            }
            other => panic!("Expected ContentBlockDelta, got {other:?}"),
        }
    }

    #[test]
    fn test_openai_finish_reason() {
        let chunk_json = r#"{"choices":[{"delta":{},"finish_reason":"stop","index":0}]}"#;
        let result = normalize_sse_event(chunk_json, &LlmProvider::OpenAI, &mut fresh_state());
        match &result[0] {
            Ok(StreamEvent::MessageDelta { delta, .. }) => {
                assert_eq!(delta.stop_reason, Some("stop".to_string()));
            }
            other => panic!("Expected MessageDelta, got {other:?}"),
        }
    }

    #[test]
    fn test_openai_usage_event() {
        let chunk_json = r#"{"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":20,"total_tokens":30}}"#;
        let result = normalize_sse_event(chunk_json, &LlmProvider::OpenAI, &mut fresh_state());
        match &result[0] {
            Ok(StreamEvent::MessageDelta { usage, .. }) => {
                assert_eq!(usage.input_tokens, 10);
                assert_eq!(usage.output_tokens, 20);
            }
            other => panic!("Expected MessageDelta with usage, got {other:?}"),
        }
    }

    #[test]
    fn test_openai_tool_call_start() {
        let chunk_json = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_abc","type":"function","function":{"name":"bash","arguments":""}}]},"index":0}]}"#;
        let result = normalize_sse_event(chunk_json, &LlmProvider::OpenAI, &mut fresh_state());
        match &result[0] {
            Ok(StreamEvent::ContentBlockStart { content_block, .. }) => {
                match content_block {
                    ContentBlock::ToolUse { id, name, .. } => {
                        assert_eq!(id, "call_abc");
                        assert_eq!(name, "bash");
                    }
                    other => panic!("Expected ToolUse block, got {other:?}"),
                }
            }
            other => panic!("Expected ContentBlockStart, got {other:?}"),
        }
    }

    #[test]
    fn test_openai_multiple_tool_calls_in_one_chunk() {
        let chunk_json = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_a","type":"function","function":{"name":"bash","arguments":""}},{"index":1,"id":"call_b","type":"function","function":{"name":"read","arguments":""}}]},"index":0}]}"#;
        let mut state = fresh_state();
        let result = normalize_sse_event(chunk_json, &LlmProvider::OpenAI, &mut state);
        // Both tool calls should produce events (not just the first).
        // Each produces ContentBlockStart + ContentBlockDelta (for the empty arguments).
        assert!(result.len() >= 2, "Expected >= 2 events for 2 tool calls, got {}", result.len());
        // Verify we got events for BOTH tool indices
        let indices: Vec<usize> = result.iter().filter_map(|e| match e {
            Ok(StreamEvent::ContentBlockStart { index, .. }) => Some(*index),
            _ => None,
        }).collect();
        assert!(indices.contains(&0), "Missing ContentBlockStart for tool index 0");
        assert!(indices.contains(&1), "Missing ContentBlockStart for tool index 1");
    }

    // -- Ollama SSE normalization --

    #[test]
    fn test_ollama_text_delta() {
        let chunk_json = r#"{"message":{"content":"world","role":"assistant"}}"#;
        let result = normalize_sse_event(chunk_json, &LlmProvider::Ollama, &mut fresh_state());
        match &result[0] {
            Ok(StreamEvent::ContentBlockDelta { delta, .. }) => {
                assert_eq!(
                    delta,
                    &ContentDelta::TextDelta {
                        text: "world".to_string()
                    }
                );
            }
            other => panic!("Expected ContentBlockDelta, got {other:?}"),
        }
    }

    #[test]
    fn test_ollama_done_event() {
        let chunk_json = r#"{"done":true,"prompt_eval_count":50,"eval_count":100}"#;
        let result = normalize_sse_event(chunk_json, &LlmProvider::Ollama, &mut fresh_state());
        match &result[0] {
            Ok(StreamEvent::MessageDelta { usage, delta, .. }) => {
                assert_eq!(usage.input_tokens, 50);
                assert_eq!(usage.output_tokens, 100);
                assert_eq!(delta.stop_reason, Some("end_turn".to_string()));
            }
            other => panic!("Expected MessageDelta, got {other:?}"),
        }
    }

    #[test]
    fn test_ollama_empty_content_skipped() {
        let chunk_json = r#"{"message":{"content":"","role":"assistant"}}"#;
        let result = normalize_sse_event(chunk_json, &LlmProvider::Ollama, &mut fresh_state());
        assert!(result.is_empty());
    }

    #[test]
    fn test_ollama_multiple_tool_calls() {
        let chunk_json = r#"{"message":{"role":"assistant","tool_calls":[{"function":{"name":"bash","arguments":{"command":"ls"}}},{"function":{"name":"read","arguments":{"path":"foo.rs"}}}]}}"#;
        let result = normalize_sse_event(chunk_json, &LlmProvider::Ollama, &mut fresh_state());
        // 2 tool calls × (start + stop) = 4 events
        assert_eq!(result.len(), 4, "Expected 4 events for 2 Ollama tool calls, got {}", result.len());
    }

    // -- Round-trip: no panic on malformed JSON --

    #[test]
    fn test_malformed_json_returns_error() {
        let result = normalize_sse_event("not json", &LlmProvider::OpenAI, &mut fresh_state());
        assert!(result[0].is_err());

        let result = normalize_sse_event("not json", &LlmProvider::Ollama, &mut fresh_state());
        assert!(result[0].is_err());

        // Anthropic also returns error for invalid JSON
        let result = normalize_sse_event("not json", &LlmProvider::Anthropic, &mut fresh_state());
        assert!(result[0].is_err());
    }

    // -- Non-streaming response normalization --

    #[test]
    fn test_normalize_openai_response_text() {
        let resp = r#"{"id":"chatcmpl-123","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"Hello!"},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":2}}"#;
        let result = normalize_response(resp, &LlmProvider::OpenAI).unwrap();
        assert_eq!(result.role, "assistant");
        assert_eq!(result.content.len(), 1);
        assert_eq!(result.stop_reason, Some("stop".to_string()));
        assert_eq!(result.usage.input_tokens, 5);
        assert_eq!(result.usage.output_tokens, 2);
    }

    #[test]
    fn test_normalize_openai_response_with_tool_calls() {
        let resp = r#"{"id":"chatcmpl-456","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":null,"tool_calls":[{"id":"call_abc","type":"function","function":{"name":"bash","arguments":"{\"command\":\"ls\"}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":10,"completion_tokens":5}}"#;
        let result = normalize_response(resp, &LlmProvider::OpenAI).unwrap();
        assert_eq!(result.stop_reason, Some("tool_calls".to_string()));
        // Should have 1 tool_use block
        let tool_blocks: Vec<_> = result.content.iter().filter_map(|b| match b {
            ContentBlock::ToolUse { name, .. } => Some(name.clone()),
            _ => None,
        }).collect();
        assert_eq!(tool_blocks, vec!["bash"]);
    }

    #[test]
    fn test_normalize_ollama_response_text() {
        let resp = r#"{"model":"llama3","message":{"role":"assistant","content":"Hi there"},"done":true,"prompt_eval_count":5,"eval_count":3}"#;
        let result = normalize_response(resp, &LlmProvider::Ollama).unwrap();
        assert_eq!(result.role, "assistant");
        assert_eq!(result.content.len(), 1);
        assert_eq!(result.stop_reason, Some("end_turn".to_string()));
        assert_eq!(result.usage.input_tokens, 5);
        assert_eq!(result.usage.output_tokens, 3);
    }

    #[test]
    fn test_normalize_ollama_response_with_tool_calls() {
        let resp = r#"{"model":"llama3","message":{"role":"assistant","content":"","tool_calls":[{"function":{"name":"read","arguments":{"path":"foo.rs"}}}]},"done":true,"eval_count":10}"#;
        let result = normalize_response(resp, &LlmProvider::Ollama).unwrap();
        let tool_blocks: Vec<_> = result.content.iter().filter_map(|b| match b {
            ContentBlock::ToolUse { name, .. } => Some(name.clone()),
            _ => None,
        }).collect();
        assert_eq!(tool_blocks, vec!["read"]);
    }

    #[test]
    fn test_normalize_anthropic_response_passthrough() {
        let resp = r#"{"id":"msg_123","type":"message","role":"assistant","content":[{"type":"text","text":"Hello"}],"model":"claude-3","stop_reason":"end_turn","usage":{"input_tokens":5,"output_tokens":1}}"#;
        let result = normalize_response(resp, &LlmProvider::Anthropic).unwrap();
        assert_eq!(result.id, "msg_123");
        assert_eq!(result.content.len(), 1);
    }

    // -- Additional edge case tests --

    #[test]
    fn test_openai_empty_delta() {
        // OpenAI sometimes sends empty deltas
        let chunk_json = r#"{"choices":[{"delta":{},"index":0}]}"#;
        let result = normalize_sse_event(chunk_json, &LlmProvider::OpenAI, &mut fresh_state());
        // Should return empty vec, not an error
        assert!(result.is_empty());
    }

    #[test]
    fn test_openai_no_choices() {
        // Handle chunks with no choices array
        let chunk_json = r#"{"choices":[]}"#;
        let result = normalize_sse_event(chunk_json, &LlmProvider::OpenAI, &mut fresh_state());
        assert!(result.is_empty());
    }

    #[test]
    fn test_openai_tool_call_without_id() {
        // Tool call delta with arguments but no id (continuation)
        let chunk_json = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"command\""}}]},"index":0}]}"#;
        let result = normalize_sse_event(chunk_json, &LlmProvider::OpenAI, &mut fresh_state());
        assert_eq!(result.len(), 1);
        match &result[0] {
            Ok(StreamEvent::ContentBlockDelta { delta, .. }) => {
                assert_eq!(delta, &ContentDelta::InputJsonDelta {
                    partial_json: r#"{"command""#.to_string()
                });
            }
            other => panic!("Expected ContentBlockDelta, got {other:?}"),
        }
    }

    #[test]
    fn test_openai_tool_call_name_only() {
        // Tool call with id and name but no arguments yet
        let chunk_json = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_123","type":"function","function":{"name":"bash"}}]},"index":0}]}"#;
        let result = normalize_sse_event(chunk_json, &LlmProvider::OpenAI, &mut fresh_state());
        assert_eq!(result.len(), 1);
        match &result[0] {
            Ok(StreamEvent::ContentBlockStart { content_block, .. }) => {
                match content_block {
                    ContentBlock::ToolUse { id, name, input } => {
                        assert_eq!(id, "call_123");
                        assert_eq!(name, "bash");
                        assert_eq!(input, &serde_json::Value::Null);
                    }
                    other => panic!("Expected ToolUse block, got {other:?}"),
                }
            }
            other => panic!("Expected ContentBlockStart, got {other:?}"),
        }
    }

    #[test]
    fn test_openai_finish_reason_with_usage() {
        // When finish_reason appears, it should emit MessageDelta
        let chunk_json = r#"{"choices":[{"delta":{},"finish_reason":"stop","index":0}]}"#;
        let result = normalize_sse_event(chunk_json, &LlmProvider::OpenAI, &mut fresh_state());
        assert_eq!(result.len(), 1);
        match &result[0] {
            Ok(StreamEvent::MessageDelta { delta, .. }) => {
                assert_eq!(delta.stop_reason, Some("stop".to_string()));
            }
            other => panic!("Expected MessageDelta, got {other:?}"),
        }
    }

    #[test]
    fn test_openai_stream_state_reset_on_finish() {
        // Verify state resets when finish_reason is received
        let mut state = fresh_state();
        state.tool_index = 5;

        let chunk_json = r#"{"choices":[{"delta":{},"finish_reason":"stop","index":0}]}"#;
        normalize_sse_event(chunk_json, &LlmProvider::OpenAI, &mut state);

        // State should be reset
        assert_eq!(state.tool_index, 0);
    }

    #[test]
    fn test_openai_consecutive_text_deltas() {
        // Multiple text chunks should all be emitted
        let chunk1 = r#"{"choices":[{"delta":{"content":"Hello"},"index":0}]}"#;
        let chunk2 = r#"{"choices":[{"delta":{"content":" world"},"index":0}]}"#;
        let chunk3 = r#"{"choices":[{"delta":{"content":"!"},"index":0}]}"#;

        let r1 = normalize_sse_event(chunk1, &LlmProvider::OpenAI, &mut fresh_state());
        let r2 = normalize_sse_event(chunk2, &LlmProvider::OpenAI, &mut fresh_state());
        let r3 = normalize_sse_event(chunk3, &LlmProvider::OpenAI, &mut fresh_state());

        assert_eq!(r1.len(), 1);
        assert_eq!(r2.len(), 1);
        assert_eq!(r3.len(), 1);

        match &r1[0] {
            Ok(StreamEvent::ContentBlockDelta { delta, .. }) => {
                assert_eq!(delta, &ContentDelta::TextDelta { text: "Hello".to_string() });
            }
            _ => panic!("Expected text delta"),
        }
    }

    #[test]
    fn test_ollama_empty_message() {
        // Ollama chunk with no message field
        let chunk_json = r#"{"done":false}"#;
        let result = normalize_sse_event(chunk_json, &LlmProvider::Ollama, &mut fresh_state());
        assert!(result.is_empty());
    }

    #[test]
    fn test_ollama_tool_call_with_empty_arguments() {
        // Tool call with empty arguments object
        let chunk_json = r#"{"message":{"tool_calls":[{"function":{"name":"bash","arguments":{}}}]}}"#;
        let result = normalize_sse_event(chunk_json, &LlmProvider::Ollama, &mut fresh_state());
        assert_eq!(result.len(), 2); // start + stop
        match &result[0] {
            Ok(StreamEvent::ContentBlockStart { content_block, .. }) => {
                match content_block {
                    ContentBlock::ToolUse { input, .. } => {
                        assert_eq!(input, &serde_json::json!({}));
                    }
                    _ => panic!("Expected ToolUse"),
                }
            }
            _ => panic!("Expected ContentBlockStart"),
        }
    }

    #[test]
    fn test_ollama_done_with_no_usage() {
        // Ollama done event without usage counts
        let chunk_json = r#"{"done":true}"#;
        let result = normalize_sse_event(chunk_json, &LlmProvider::Ollama, &mut fresh_state());
        assert_eq!(result.len(), 1);
        match &result[0] {
            Ok(StreamEvent::MessageDelta { usage, delta, .. }) => {
                assert_eq!(usage.input_tokens, 0);
                assert_eq!(usage.output_tokens, 0);
                assert_eq!(delta.stop_reason, Some("end_turn".to_string()));
            }
            other => panic!("Expected MessageDelta, got {other:?}"),
        }
    }

    #[test]
    fn test_normalize_openai_response_empty_content() {
        // OpenAI response with null content (tool calls only)
        let resp = r#"{"id":"chatcmpl-789","choices":[{"message":{"role":"assistant","content":null},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":8,"completion_tokens":3}}"#;
        let result = normalize_response(resp, &LlmProvider::OpenAI).unwrap();
        assert_eq!(result.content.len(), 0); // No content blocks
        assert_eq!(result.stop_reason, Some("tool_calls".to_string()));
    }

    #[test]
    fn test_normalize_openai_response_no_usage() {
        // OpenAI response without usage field
        let resp = r#"{"id":"chatcmpl-999","choices":[{"message":{"role":"assistant","content":"Hi"},"finish_reason":"stop"}]}"#;
        let result = normalize_response(resp, &LlmProvider::OpenAI).unwrap();
        assert_eq!(result.content.len(), 1);
        assert_eq!(result.usage.input_tokens, 0);
        assert_eq!(result.usage.output_tokens, 0);
    }

    #[test]
    fn test_normalize_ollama_response_no_usage() {
        // Ollama response without usage counts
        let resp = r#"{"model":"llama3","message":{"role":"assistant","content":"Hello"},"done":true}"#;
        let result = normalize_response(resp, &LlmProvider::Ollama).unwrap();
        assert_eq!(result.usage.input_tokens, 0);
        assert_eq!(result.usage.output_tokens, 0);
    }

    #[test]
    fn test_normalize_openai_invalid_tool_args() {
        // Tool call with invalid JSON arguments
        let resp = r#"{"id":"chatcmpl-111","choices":[{"message":{"role":"assistant","tool_calls":[{"id":"call_123","function":{"name":"bash","arguments":"not json"}}]},"finish_reason":"tool_calls"}]}"#;
        let result = normalize_response(resp, &LlmProvider::OpenAI).unwrap();
        // Should parse but have null arguments
        match &result.content[0] {
            ContentBlock::ToolUse { input, .. } => {
                assert_eq!(input, &serde_json::Value::Null);
            }
            _ => panic!("Expected ToolUse block"),
        }
    }

    #[test]
    fn test_openai_tool_index_auto_increment() {
        // When index is missing, auto-increment from state
        let mut state = fresh_state();

        // First tool call without index
        let chunk1 = r#"{"choices":[{"delta":{"tool_calls":[{"id":"call_a","function":{"name":"bash"}}]},"index":0}]}"#;
        let r1 = normalize_sse_event(chunk1, &LlmProvider::OpenAI, &mut state);
        assert_eq!(state.tool_index, 1);

        // Second tool call without index
        let chunk2 = r#"{"choices":[{"delta":{"tool_calls":[{"id":"call_b","function":{"name":"read"}}]},"index":0}]}"#;
        let r2 = normalize_sse_event(chunk2, &LlmProvider::OpenAI, &mut state);
        assert_eq!(state.tool_index, 2);

        // Both should have been assigned different indices
        match &r1[0] {
            Ok(StreamEvent::ContentBlockStart { index, .. }) => {
                assert_eq!(*index, 0);
            }
            _ => panic!("Expected index 0"),
        }

        match &r2[0] {
            Ok(StreamEvent::ContentBlockStart { index, .. }) => {
                assert_eq!(*index, 1);
            }
            _ => panic!("Expected index 1"),
        }
    }
}
