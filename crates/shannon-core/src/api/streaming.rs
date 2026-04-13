//! Streaming response types and SSE implementation.
//!
//! Handles Server-Sent Events (SSE) streaming from LLM API providers.
//! Properly buffers partial events that span HTTP chunk boundaries.
//! Supports automatic reconnection using `Last-Event-ID` when the
//! connection drops mid-stream.

use futures::{Stream, StreamExt, task::{Context, Poll}};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use super::adapter::OpenaiStreamState;
use super::error::ApiError;
use super::types::{LlmProvider, StreamEvent};

/// Stream of API events
pub type MessageStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, ApiError>> + Send>>;

/// Internal byte-chunk stream type from reqwest
type ByteChunkStream = Pin<Box<dyn Stream<Item = Result<Vec<u8>, reqwest::Error>> + Send>>;

/// Shared last-event-id tracker for reconnection support.
///
/// Wrapped in `Arc<Mutex<>>` so both the inner `SseStream` and the
/// outer `ResumableSseStream` can read/update it.
pub type LastEventId = Arc<Mutex<Option<String>>>;

/// SSE stream that properly handles chunk boundaries.
///
/// Reads chunks from reqwest's byte stream, buffers partial lines,
/// and emits complete SSE events. Handles the common case where
/// a single SSE `data:` line spans multiple HTTP chunks.
///
/// Tracks the last SSE `id:` field for reconnection support.
pub struct SseStream {
    chunks: ByteChunkStream,
    buffer: String,
    pending_events: Vec<Result<StreamEvent, ApiError>>,
    done: bool,
    provider: LlmProvider,
    openai_state: OpenaiStreamState,
    /// Tracks the last SSE event ID seen for reconnection.
    last_event_id: LastEventId,
}

impl SseStream {
    /// Create a new SSE stream from a reqwest response.
    ///
    /// Takes ownership of the response and consumes its byte stream.
    /// The `last_event_id` tracker is shared so callers can read the
    /// latest ID for reconnection.
    pub fn new(response: reqwest::Response, provider: LlmProvider, last_event_id: LastEventId) -> Self {
        let byte_stream = response.bytes_stream();
        // Convert Bytes to Vec<u8> to avoid direct dependency on bytes crate
        let mapped = Box::pin(byte_stream.map(|result| result.map(|b| b.to_vec())));
        Self {
            chunks: mapped,
            buffer: String::new(),
            pending_events: Vec::new(),
            done: false,
            provider,
            openai_state: OpenaiStreamState::new(),
            last_event_id,
        }
    }

    /// Parse all complete SSE lines from the buffer, queuing parsed events.
    /// Incomplete lines remain in the buffer for the next chunk.
    fn drain_buffer(&mut self) {
        while let Some(newline_pos) = self.buffer.find('\n') {
            let line = self.buffer[..newline_pos].to_string();
            self.buffer = self.buffer[newline_pos + 1..].to_string();

            let events = self.parse_sse_line(&line);
            self.pending_events.extend(events);
        }
    }

    /// Parse a single SSE line into events using provider-specific normalization.
    ///
    /// Returns a `Vec` because a single SSE chunk can produce multiple logical
    /// events (e.g. multiple simultaneous tool-call starts from OpenAI/Ollama).
    fn parse_sse_line(&mut self, line: &str) -> Vec<Result<StreamEvent, ApiError>> {
        let line = line.trim();

        // Skip empty lines and SSE comments
        if line.is_empty() || line.starts_with(':') {
            return vec![];
        }

        // SSE event fields: only process "data:" lines
        // Capture SSE event ID for reconnection support
        if let Some(id) = line.strip_prefix("id:") {
            let id = id.trim();
            if !id.is_empty() {
                if let Ok(mut guard) = self.last_event_id.lock() {
                    *guard = Some(id.to_string());
                }
            }
            return vec![];
        }

        let json_str = if let Some(s) = line.strip_prefix("data: ") {
            s
        } else if let Some(s) = line.strip_prefix("data:") {
            s.trim()
        } else {
            // Ignore other SSE fields (event:, retry:)
            return vec![];
        };

        if json_str == "[DONE]" {
            return vec![Ok(StreamEvent::MessageStop)];
        }

        super::adapter::normalize_sse_event(json_str, &self.provider, &mut self.openai_state)
    }
}

impl Stream for SseStream {
    type Item = Result<StreamEvent, ApiError>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        // Return any pending events first
        if !self.pending_events.is_empty() {
            return Poll::Ready(Some(self.pending_events.remove(0)));
        }

        if self.done {
            return Poll::Ready(None);
        }

        // Try to get more data from the HTTP stream
        loop {
            match self.chunks.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(data))) => {
                    let text = String::from_utf8_lossy(&data);
                    self.buffer.push_str(&text);
                    self.drain_buffer();

                    if !self.pending_events.is_empty() {
                        return Poll::Ready(Some(self.pending_events.remove(0)));
                    }
                    // No complete events yet — continue reading
                }
                Poll::Ready(Some(Err(e))) => {
                    self.done = true;
                    return Poll::Ready(Some(Err(ApiError::HttpError(e))));
                }
                Poll::Ready(None) => {
                    // Stream ended — process any remaining data in buffer
                    if !self.buffer.trim().is_empty() {
                        let remaining = std::mem::take(&mut self.buffer);
                        let events = self.parse_sse_line(&remaining);
                        self.pending_events.extend(events);
                        if !self.pending_events.is_empty() {
                            self.done = true;
                            return Poll::Ready(Some(self.pending_events.remove(0)));
                        }
                    }
                    self.done = true;
                    return Poll::Ready(None);
                }
                Poll::Pending => {
                    return Poll::Pending;
                }
            }
        }
    }
}

/// Create a MessageStream from a reqwest response.
///
/// Properly handles SSE events that span HTTP chunk boundaries
/// by buffering partial lines until complete.
pub fn sse_stream_from_response(response: reqwest::Response, provider: LlmProvider) -> MessageStream {
    let last_event_id = Arc::new(Mutex::new(None));
    let sse = SseStream::new(response, provider, last_event_id);
    Box::pin(sse)
}

/// Create a resumable MessageStream that can reconnect on connection drops.
///
/// When the underlying SSE stream ends prematurely (not via `MessageStop`),
/// this wrapper uses `send_message_stream_resumable` to reconnect with
/// `Last-Event-ID`, up to `max_reconnects` times.
pub fn sse_stream_from_response_resumable(
    response: reqwest::Response,
    provider: LlmProvider,
    client: super::client::LlmClient,
    messages: Vec<super::types::Message>,
    tools: Option<Vec<super::types::ToolDefinition>>,
    system: Option<String>,
    max_reconnects: u32,
) -> MessageStream {
    let last_event_id = Arc::new(Mutex::new(None));
    let sse = SseStream::new(response, provider, last_event_id.clone());
    let resumable = ResumableSseStream {
        inner: Box::pin(sse),
        last_event_id,
        client,
        messages,
        tools,
        system,
        reconnects_remaining: max_reconnects,
        reconnecting: false,
        saw_message_stop: false,
        pending_reconnect: None,
    };
    Box::pin(resumable)
}

/// Wrapper around a `MessageStream` that handles automatic reconnection.
///
/// When the inner stream ends without a `MessageStop` event (indicating
/// an unexpected connection drop), this wrapper reconnects using the
/// tracked `Last-Event-ID` so the provider can replay missed events.
struct ResumableSseStream {
    inner: MessageStream,
    last_event_id: LastEventId,
    client: super::client::LlmClient,
    messages: Vec<super::types::Message>,
    tools: Option<Vec<super::types::ToolDefinition>>,
    system: Option<String>,
    reconnects_remaining: u32,
    reconnecting: bool,
    saw_message_stop: bool,
    pending_reconnect: Option<tokio::sync::oneshot::Receiver<Result<MessageStream, ApiError>>>,
}

impl Stream for ResumableSseStream {
    type Item = Result<StreamEvent, ApiError>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        // If we're in reconnection state, check if the reconnect completed
        if self.reconnecting {
            if let Some(ref mut rx) = self.pending_reconnect {
                match Pin::new(rx).poll(cx) {
                    Poll::Ready(Ok(Ok(new_stream))) => {
                        self.inner = new_stream;
                        self.reconnecting = false;
                        self.pending_reconnect = None;
                        // Fall through to poll the new inner stream
                    }
                    Poll::Ready(Ok(Err(e))) => {
                        self.reconnecting = false;
                        self.pending_reconnect = None;
                        return Poll::Ready(Some(Err(e)));
                    }
                    Poll::Ready(Err(_)) => {
                        self.reconnecting = false;
                        self.pending_reconnect = None;
                        return Poll::Ready(None);
                    }
                    Poll::Pending => {
                        return Poll::Pending;
                    }
                }
            }
        }

        // Poll inner stream (every branch returns, so no actual looping)
        match self.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(event))) => {
                if matches!(event, StreamEvent::MessageStop) {
                    self.saw_message_stop = true;
                }
                Poll::Ready(Some(Ok(event)))
            }
            Poll::Ready(Some(Err(e))) => {
                // Only reconnect on connection/transport errors, not parse errors
                let is_connection_error = matches!(
                    &e,
                    ApiError::HttpError(_)
                );
                if !is_connection_error || self.reconnects_remaining == 0 {
                    Poll::Ready(Some(Err(e)))
                } else {
                    self.start_reconnect(cx);
                    Poll::Pending
                }
            }
            Poll::Ready(None) => {
                // Stream ended
                if self.saw_message_stop || self.reconnects_remaining == 0 {
                    Poll::Ready(None)
                } else {
                    // Premature end — reconnect
                    self.start_reconnect(cx);
                    Poll::Pending
                }
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl ResumableSseStream {
    /// Initiate an asynchronous reconnection using the tracked last event ID.
    fn start_reconnect(&mut self, cx: &mut Context<'_>) {
        self.reconnects_remaining -= 1;
        let eid = self.last_event_id.lock().ok().and_then(|g| g.clone());
        tracing::info!(
            "Stream dropped unexpectedly. Reconnecting ({} attempts left, last_event_id={:?})",
            self.reconnects_remaining,
            eid,
        );

        let (tx, rx) = tokio::sync::oneshot::channel();
        let config = self.client.config().clone();
        let messages = self.messages.clone();
        let tools = self.tools.clone();
        let system = self.system.clone();

        tokio::spawn(async move {
            let reconnect_client = super::client::LlmClient::new(config);
            let result = reconnect_client
                .send_message_stream_resumable(messages, tools, system, eid)
                .await;
            let _ = tx.send(result);
        });

        self.reconnecting = true;
        self.pending_reconnect = Some(rx);

        // Wake the waker when the spawned task completes
        cx.waker().wake_by_ref();
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::adapter::OpenaiStreamState;
    use crate::api::types::{ContentDelta, StreamEvent};

    /// Helper to parse SSE lines into events
    fn parse_sse_lines(lines: &[&str], provider: LlmProvider) -> Vec<Result<StreamEvent, crate::api::error::ApiError>> {
        let mut events = Vec::new();
        let mut state = OpenaiStreamState::new();

        for line in lines {
            let line = line.trim();
            if line.is_empty() || line.starts_with(':') {
                continue;
            }

            if let Some(json_str) = line.strip_prefix("data: ").or_else(|| line.strip_prefix("data:")) {
                let json_str = json_str.trim();
                if json_str == "[DONE]" {
                    events.push(Ok(StreamEvent::MessageStop));
                    continue;
                }
                let mut result_events = crate::api::adapter::normalize_sse_event(json_str, &provider, &mut state);
                events.append(&mut result_events);
            }
        }
        events
    }

    // -- Anthropic SSE parsing --

    #[test]
    fn test_anthropic_message_start() {
        let lines = vec![
            r#"data: {"type":"message_start","message":{"id":"msg_123","role":"assistant","content":[],"model":"claude-3","stop_reason":null,"usage":{"input_tokens":10,"output_tokens":0}}}"#,
        ];
        let events = parse_sse_lines(&lines, LlmProvider::Anthropic);
        assert_eq!(events.len(), 1);
        match &events[0] {
            Ok(StreamEvent::MessageStart { message }) => {
                assert_eq!(message.id, "msg_123");
            }
            other => panic!("Expected MessageStart, got {other:?}"),
        }
    }

    #[test]
    fn test_anthropic_content_block_delta() {
        let lines = vec![
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#,
        ];
        let events = parse_sse_lines(&lines, LlmProvider::Anthropic);
        assert_eq!(events.len(), 1);
        match &events[0] {
            Ok(StreamEvent::ContentBlockDelta { delta, .. }) => {
                assert_eq!(delta, &ContentDelta::TextDelta { text: "Hello".to_string() });
            }
            other => panic!("Expected ContentBlockDelta, got {other:?}"),
        }
    }

    #[test]
    fn test_anthropic_message_stop() {
        let lines = vec![
            "data: [DONE]",
        ];
        let events = parse_sse_lines(&lines, LlmProvider::Anthropic);
        assert_eq!(events.len(), 1);
        match &events[0] {
            Ok(StreamEvent::MessageStop) => {},
            other => panic!("Expected MessageStop, got {other:?}"),
        }
    }

    // -- OpenAI SSE parsing --

    #[test]
    fn test_openai_streaming_text() {
        let lines = vec![
            r#"data: {"choices":[{"delta":{"content":"Hello"},"index":0}]}"#,
            r#"data: {"choices":[{"delta":{"content":" world"},"index":0}]}"#,
            r#"data: {"choices":[{"delta":{},"finish_reason":"stop","index":0}]}"#,
        ];
        let events = parse_sse_lines(&lines, LlmProvider::OpenAI);
        assert!(events.len() >= 2);

        // First event should be text delta
        match &events[0] {
            Ok(StreamEvent::ContentBlockDelta { delta, .. }) => {
                assert_eq!(delta, &ContentDelta::TextDelta { text: "Hello".to_string() });
            }
            other => panic!("Expected ContentBlockDelta, got {other:?}"),
        }

        // Last event should be MessageDelta with finish_reason
        let last = events.last().unwrap();
        match last {
            Ok(StreamEvent::MessageDelta { delta, .. }) => {
                assert_eq!(delta.stop_reason, Some("stop".to_string()));
            }
            other => panic!("Expected MessageDelta at end, got {other:?}"),
        }
    }

    #[test]
    fn test_openai_usage_chunk() {
        let lines = vec![
            r#"data: {"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":20,"total_tokens":30}}"#,
        ];
        let events = parse_sse_lines(&lines, LlmProvider::OpenAI);
        assert_eq!(events.len(), 1);
        match &events[0] {
            Ok(StreamEvent::MessageDelta { usage, .. }) => {
                assert_eq!(usage.input_tokens, 10);
                assert_eq!(usage.output_tokens, 20);
            }
            other => panic!("Expected MessageDelta with usage, got {other:?}"),
        }
    }

    #[test]
    fn test_openai_tool_call_streaming() {
        let lines = vec![
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_abc","type":"function","function":{"name":"bash","arguments":""}}]},"index":0}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\\"command\\""}}]},"index":0}]}"#,
        ];
        let events = parse_sse_lines(&lines, LlmProvider::OpenAI);
        assert!(events.len() >= 2);

        // First event should be ContentBlockStart
        match &events[0] {
            Ok(StreamEvent::ContentBlockStart { .. }) => {},
            other => panic!("Expected ContentBlockStart, got {other:?}"),
        }

        // Second event should be ContentBlockDelta with arguments
        match &events[1] {
            Ok(StreamEvent::ContentBlockDelta { delta, .. }) => {
                match delta {
                    ContentDelta::InputJsonDelta { .. } => {},
                    _ => panic!("Expected InputJsonDelta, got {delta:?}"),
                }
            }
            other => panic!("Expected ContentBlockDelta, got {other:?}"),
        }
    }

    // -- Ollama SSE parsing --

    #[test]
    fn test_ollama_streaming_text() {
        let lines = vec![
            r#"data: {"message":{"role":"assistant","content":"Hello"}}"#,
            r#"data: {"message":{"role":"assistant","content":" world"}}"#,
            r#"data: {"done":true,"prompt_eval_count":5,"eval_count":10}"#,
        ];
        let events = parse_sse_lines(&lines, LlmProvider::Ollama);
        assert!(events.len() >= 3);

        // First two should be text deltas
        match &events[0] {
            Ok(StreamEvent::ContentBlockDelta { delta, .. }) => {
                assert_eq!(delta, &ContentDelta::TextDelta { text: "Hello".to_string() });
            }
            other => panic!("Expected ContentBlockDelta, got {other:?}"),
        }

        // Last should be MessageDelta with usage
        let last = events.last().unwrap();
        match last {
            Ok(StreamEvent::MessageDelta { usage, delta, .. }) => {
                assert_eq!(usage.input_tokens, 5);
                assert_eq!(usage.output_tokens, 10);
                assert_eq!(delta.stop_reason, Some("end_turn".to_string()));
            }
            other => panic!("Expected MessageDelta, got {other:?}"),
        }
    }

    #[test]
    fn test_ollama_tool_call() {
        let lines = vec![
            r#"data: {"message":{"role":"assistant","tool_calls":[{"function":{"name":"bash","arguments":{"command":"ls"}}}]}}"#,
        ];
        let events = parse_sse_lines(&lines, LlmProvider::Ollama);
        // Should have ContentBlockStart + ContentBlockStop
        assert_eq!(events.len(), 2);

        match &events[0] {
            Ok(StreamEvent::ContentBlockStart { .. }) => {},
            other => panic!("Expected ContentBlockStart, got {other:?}"),
        }

        match &events[1] {
            Ok(StreamEvent::ContentBlockStop { .. }) => {},
            other => panic!("Expected ContentBlockStop, got {other:?}"),
        }
    }

    // -- SSE comment and empty line handling --

    #[test]
    fn test_sse_comments_ignored() {
        let lines = vec![
            ": this is a comment",
            "",
            "data: {\"type\":\"ping\"}",
        ];
        let events = parse_sse_lines(&lines, LlmProvider::Anthropic);
        // Should only have the ping event, comments and empty lines ignored
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn test_sse_multiple_events_per_line() {
        // Test that we can handle multiple events
        let lines = vec![
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"A"}}"#,
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"B"}}"#,
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"C"}}"#,
        ];
        let events = parse_sse_lines(&lines, LlmProvider::Anthropic);
        assert_eq!(events.len(), 3);
    }

    // -- Provider-specific edge cases --

    #[test]
    fn test_openai_empty_choices() {
        let lines = vec![
            r#"data: {"choices":[]}"#,
        ];
        let events = parse_sse_lines(&lines, LlmProvider::OpenAI);
        // Should return empty, not error
        assert!(events.is_empty());
    }

    #[test]
    fn test_ollama_empty_content() {
        let lines = vec![
            r#"data: {"message":{"content":""}}"#,
        ];
        let events = parse_sse_lines(&lines, LlmProvider::Ollama);
        // Empty content should be skipped
        assert!(events.is_empty());
    }

    #[test]
    fn test_invalid_json_returns_error() {
        let lines = vec![
            "data: {invalid json}",
        ];
        let events = parse_sse_lines(&lines, LlmProvider::OpenAI);
        assert_eq!(events.len(), 1);
        assert!(events[0].is_err());
    }

    #[test]
    fn test_anthropic_passthrough_preserves_all_fields() {
        let lines = vec![
            r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"input_tokens":100,"output_tokens":50}}"#,
        ];
        let events = parse_sse_lines(&lines, LlmProvider::Anthropic);
        assert_eq!(events.len(), 1);
        match &events[0] {
            Ok(StreamEvent::MessageDelta { delta, usage, .. }) => {
                assert_eq!(delta.stop_reason, Some("end_turn".to_string()));
                assert_eq!(usage.input_tokens, 100);
                assert_eq!(usage.output_tokens, 50);
            }
            other => panic!("Expected MessageDelta, got {other:?}"),
        }
    }

    // -- Last-Event-ID tracking --

    #[test]
    fn test_sse_id_field_captured() {
        let last_event_id = Arc::new(Mutex::new(None));
        let mut sse = SseStream::for_test(last_event_id.clone());

        // Simulate SSE lines with id: fields
        sse.buffer = "id: evt_001\ndata: {\"type\":\"ping\"}\n\n".to_string();
        sse.drain_buffer();
        assert_eq!(
            last_event_id.lock().unwrap().as_deref(),
            Some("evt_001"),
            "Should capture SSE id field"
        );

        // Update with a new id
        sse.buffer = "id: evt_002\ndata: {\"type\":\"ping\"}\n\n".to_string();
        sse.drain_buffer();
        assert_eq!(
            last_event_id.lock().unwrap().as_deref(),
            Some("evt_002"),
            "Should update to latest id"
        );
    }

    #[test]
    fn test_sse_id_empty_ignored() {
        let last_event_id = Arc::new(Mutex::new(None));
        let mut sse = SseStream::for_test(last_event_id.clone());

        // Pre-set an id
        *last_event_id.lock().unwrap() = Some("evt_100".to_string());

        // Empty id line should not clear the existing value
        sse.buffer = "id:\ndata: {\"type\":\"ping\"}\n\n".to_string();
        sse.drain_buffer();
        assert_eq!(
            last_event_id.lock().unwrap().as_deref(),
            Some("evt_100"),
            "Empty id should not overwrite existing value"
        );
    }

    /// Test-only constructor for SseStream that doesn't require an HTTP response.
    impl SseStream {
        fn for_test(last_event_id: LastEventId) -> Self {
            // Create a no-op byte stream (immediately returns None)
            let byte_stream = Box::pin(futures::stream::empty());
            Self {
                chunks: byte_stream,
                buffer: String::new(),
                pending_events: Vec::new(),
                done: false,
                provider: LlmProvider::Anthropic,
                openai_state: OpenaiStreamState::new(),
                last_event_id,
            }
        }
    }
}
