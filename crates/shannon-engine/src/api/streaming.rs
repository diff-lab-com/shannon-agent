//! Streaming response types and SSE implementation.
//!
//! Handles Server-Sent Events (SSE) streaming from LLM API providers.
//! Properly buffers partial events that span HTTP chunk boundaries.
//! Supports automatic reconnection using `Last-Event-ID` when the
//! connection drops mid-stream.

use futures::{
    Stream, StreamExt,
    task::{Context, Poll},
};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::adapter::OpenaiStreamState;
use super::error::ApiError;
use super::types::{LlmProvider, StreamEvent};

/// Stream of API events
pub type MessageStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, ApiError>> + Send>>;

/// Internal byte-chunk stream type from reqwest
type ByteChunkStream = Pin<Box<dyn Stream<Item = Result<Vec<u8>, reqwest::Error>> + Send>>;

/// Content-level stream idle watchdog budget, from `SHANNON_STREAM_IDLE_SECS`.
///
/// The HTTP `read_timeout` (see `LlmClient::build_client`) is computed per
/// byte read: a server that keeps the connection open with SSE keepalive
/// bytes (comment lines, empty lines, `ping` frames) never trips it, so a
/// response that silently produces no content for minutes — observed as a
/// single 312s gap on glm-5.3-flash (P1b event log) — used to hang until the
/// caller's outer budget was consumed. This watchdog measures the gap
/// between *content* events instead: anything except keepalives resets it.
///
/// Default `0` = disabled, preserving current behavior for interactive
/// sessions where very long thinking gaps are legitimate. Eval runners opt
/// in explicitly (e.g. 360). When the budget is exceeded the request is
/// aborted with [`ApiError::Timeout`], which the existing machinery already
/// treats as retryable (`RetryConfig::is_retryable`) and reconnectable
/// (`ResumableSseStream`).
pub(crate) fn stream_idle_timeout_from_env() -> Option<Duration> {
    let raw = std::env::var("SHANNON_STREAM_IDLE_SECS").ok()?;
    if raw.trim().is_empty() {
        return None;
    }
    let secs = raw.trim().parse::<u64>().unwrap_or_else(|_| {
        tracing::warn!("Invalid SHANNON_STREAM_IDLE_SECS={raw:?} — ignoring (expected seconds)");
        0
    });
    (secs > 0).then(|| Duration::from_secs(secs))
}

/// True for events that carry no response content and must not reset the
/// idle watchdog: provider keepalive pings. SSE comment lines and empty
/// lines never even become events (dropped in `parse_sse_line`).
fn is_keepalive_event(event: &StreamEvent) -> bool {
    matches!(event, StreamEvent::Ping)
}

/// Shared last-event-id tracker for reconnection support.
///
/// Wrapped in `Arc<Mutex<>>` so both the inner `SseStream` and the
/// outer `ResumableSseStream` can read/update it.
pub type LastEventId = Arc<Mutex<Option<String>>>;

/// Quick check whether a string has balanced braces (ignoring strings).
/// Used to avoid passing incomplete JSON to the parser after a TCP split.
fn looks_like_complete_json(s: &str) -> bool {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    for ch in s.chars() {
        if escape {
            escape = false;
            continue;
        }
        if ch == '\\' && in_string {
            escape = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        if ch == '{' {
            depth += 1;
        } else if ch == '}' {
            depth -= 1;
            if depth == 0 {
                return true;
            }
        }
    }
    false
}

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
    pending_events: std::collections::VecDeque<Result<StreamEvent, ApiError>>,
    done: bool,
    provider: LlmProvider,
    openai_state: OpenaiStreamState,
    /// Tracks the last SSE event ID seen for reconnection.
    last_event_id: LastEventId,
    /// Content-level idle watchdog budget (A5). `None` = disabled (default,
    /// `SHANNON_STREAM_IDLE_SECS` unset/0) — zero behavior change.
    idle_timeout: Option<Duration>,
    /// Armed when the watchdog is enabled and reset by every non-keepalive
    /// event; firing it aborts the request (see `poll_next`). Created lazily
    /// inside `poll_next` so `SseStream` can be built outside a Tokio
    /// runtime.
    idle_timer: Option<Pin<Box<tokio::time::Sleep>>>,
}

impl SseStream {
    /// Create a new SSE stream from a reqwest response.
    ///
    /// Takes ownership of the response and consumes its byte stream.
    /// The `last_event_id` tracker is shared so callers can read the
    /// latest ID for reconnection.
    pub fn new(
        response: reqwest::Response,
        provider: LlmProvider,
        last_event_id: LastEventId,
    ) -> Self {
        Self::with_idle_timeout(
            response,
            provider,
            last_event_id,
            stream_idle_timeout_from_env(),
        )
    }

    /// Like [`Self::new`] with an explicit idle-watchdog budget (tests pass
    /// one directly; production goes through [`Self::new`] and reads
    /// `SHANNON_STREAM_IDLE_SECS`).
    fn with_idle_timeout(
        response: reqwest::Response,
        provider: LlmProvider,
        last_event_id: LastEventId,
        idle_timeout: Option<Duration>,
    ) -> Self {
        let byte_stream = response.bytes_stream();
        // Convert Bytes to Vec<u8> to avoid direct dependency on bytes crate
        let mapped = Box::pin(byte_stream.map(|result| result.map(|b| b.to_vec())));
        Self {
            chunks: mapped,
            buffer: String::new(),
            pending_events: std::collections::VecDeque::new(),
            done: false,
            provider,
            openai_state: OpenaiStreamState::new(),
            last_event_id,
            idle_timeout,
            idle_timer: None,
        }
    }

    /// Reset the idle watchdog after a content event was observed. A no-op
    /// when the watchdog is disabled or the event is a keepalive.
    fn note_content(&mut self, event: &StreamEvent) {
        if is_keepalive_event(event) {
            return;
        }
        let Some(timeout) = self.idle_timeout else {
            return;
        };
        let timer = self.idle_timer.get_or_insert_with(|| {
            Box::pin(tokio::time::sleep(timeout))
        });
        timer
            .as_mut()
            .reset(tokio::time::Instant::now() + timeout);
    }

    /// Parse all complete SSE lines from the buffer, queuing parsed events.
    /// Incomplete lines remain in the buffer for the next chunk.
    /// For Ollama NDJSON, validates JSON brace balance before parsing to
    /// handle chunks split across TCP packet boundaries.
    fn drain_buffer(&mut self) {
        while let Some(newline_pos) = self.buffer.find('\n') {
            let line = self.buffer[..newline_pos].to_string();
            self.buffer = self.buffer[newline_pos + 1..].to_string();

            // For Ollama NDJSON: validate JSON completeness before parsing
            if matches!(self.provider, LlmProvider::Ollama)
                && line.trim_start().starts_with('{')
                && !looks_like_complete_json(&line)
            {
                // Incomplete JSON — put back and wait for more data
                self.buffer = format!("{}\n{}", line, self.buffer);
                break;
            }

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
                let mut guard = self.last_event_id.lock().unwrap_or_else(|e| e.into_inner());
                *guard = Some(id.to_string());
            }
            return vec![];
        }

        let json_str = if let Some(s) = line.strip_prefix("data: ") {
            s
        } else if let Some(s) = line.strip_prefix("data:") {
            s.trim()
        } else if matches!(self.provider, LlmProvider::Ollama) && line.starts_with('{') {
            // Ollama returns NDJSON (raw JSON per line) without "data:" prefix
            line
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

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Return any pending events first
        if !self.pending_events.is_empty() {
            let event = self
                .pending_events
                .pop_front()
                .expect("checked non-empty");
            if let Ok(event) = &event {
                self.note_content(event);
            }
            return Poll::Ready(Some(event));
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
                        let event = self
                            .pending_events
                            .pop_front()
                            .expect("checked non-empty");
                        if let Ok(event) = &event {
                            self.note_content(event);
                        }
                        return Poll::Ready(Some(event));
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
                            let event = self
                                .pending_events
                                .pop_front()
                                .expect("checked non-empty");
                            if let Ok(event) = &event {
                                self.note_content(event);
                            }
                            self.done = true;
                            return Poll::Ready(Some(event));
                        }
                    }
                    self.done = true;
                    return Poll::Ready(None);
                }
                Poll::Pending => {
                    // Awaiting more bytes from the wire. If the content-level
                    // idle watchdog is enabled, its timer owns the wakeup for
                    // the deadline: when it fires, no non-keepalive event has
                    // arrived for `idle_timeout` — abort the request into the
                    // existing retry/reconnect path (ApiError::Timeout is
                    // classified retryable/reconnectable). The byte-level
                    // keepalives that would keep `read_timeout` happy never
                    // reach this point as events, so they cannot postpone the
                    // deadline.
                    if let Some(timeout) = self.idle_timeout {
                        // Arm on first pending poll so the wait for the very
                        // first content event is covered too.
                        let timer = self
                            .idle_timer
                            .get_or_insert_with(|| Box::pin(tokio::time::sleep(timeout)));
                        match timer.as_mut().poll(cx) {
                            Poll::Ready(()) => {
                                self.done = true;
                                tracing::warn!(
                                    "Stream idle watchdog: no content event for {timeout:?} — \
                                     aborting request for retry"
                                );
                                return Poll::Ready(Some(Err(ApiError::Timeout)));
                            }
                            Poll::Pending => return Poll::Pending,
                        }
                    }
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
pub fn sse_stream_from_response(
    response: reqwest::Response,
    provider: LlmProvider,
) -> MessageStream {
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
        initial_reconnects: max_reconnects,
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
    initial_reconnects: u32,
    reconnecting: bool,
    saw_message_stop: bool,
    pending_reconnect: Option<tokio::sync::oneshot::Receiver<Result<MessageStream, ApiError>>>,
}

impl Stream for ResumableSseStream {
    type Item = Result<StreamEvent, ApiError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
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
                // Reconnect on connection/transport errors and retryable server errors
                let is_reconnectable = matches!(
                    &e,
                    ApiError::HttpError(_)
                        | ApiError::Timeout
                        | ApiError::StreamEndedUnexpectedly
                        | ApiError::RateLimitExceeded { .. }
                        | ApiError::ApiError {
                            status: 500..=599,
                            ..
                        }
                );
                if !is_reconnectable || self.reconnects_remaining == 0 {
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
        let eid = self
            .last_event_id
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();

        // Exponential backoff: 1s, 2s, 4s, 8s, ...
        let attempts_used = self.initial_reconnects - self.reconnects_remaining;
        let backoff_secs = if attempts_used == 0 {
            1
        } else {
            1u64 << (attempts_used - 1).min(4)
        };

        tracing::info!(
            "Stream dropped unexpectedly. Reconnecting in {backoff_secs}s ({} attempts left, last_event_id={:?})",
            self.reconnects_remaining,
            eid,
        );

        let (tx, rx) = tokio::sync::oneshot::channel();
        let config = self.client.config().clone();
        let messages = self.messages.clone();
        let tools = self.tools.clone();
        let system = self.system.clone();

        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
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
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::api::adapter::OpenaiStreamState;
    use crate::api::types::{ContentDelta, StreamEvent};

    /// Helper to parse SSE lines into events
    fn parse_sse_lines(
        lines: &[&str],
        provider: LlmProvider,
    ) -> Vec<Result<StreamEvent, crate::api::error::ApiError>> {
        let mut events = Vec::new();
        let mut state = OpenaiStreamState::new();

        for line in lines {
            let line = line.trim();
            if line.is_empty() || line.starts_with(':') {
                continue;
            }

            let json_str = if let Some(s) = line
                .strip_prefix("data: ")
                .or_else(|| line.strip_prefix("data:"))
            {
                s.trim()
            } else if matches!(provider, LlmProvider::Ollama) && line.starts_with('{') {
                // Ollama returns NDJSON without "data:" prefix
                line
            } else {
                continue;
            };

            if json_str == "[DONE]" {
                events.push(Ok(StreamEvent::MessageStop));
                continue;
            }
            let mut result_events =
                crate::api::adapter::normalize_sse_event(json_str, &provider, &mut state);
            events.append(&mut result_events);
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
                assert_eq!(
                    delta,
                    &ContentDelta::TextDelta {
                        text: "Hello".to_string()
                    }
                );
            }
            other => panic!("Expected ContentBlockDelta, got {other:?}"),
        }
    }

    #[test]
    fn test_anthropic_message_stop() {
        let lines = vec!["data: [DONE]"];
        let events = parse_sse_lines(&lines, LlmProvider::Anthropic);
        assert_eq!(events.len(), 1);
        match &events[0] {
            Ok(StreamEvent::MessageStop) => {}
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
                assert_eq!(
                    delta,
                    &ContentDelta::TextDelta {
                        text: "Hello".to_string()
                    }
                );
            }
            other => panic!("Expected ContentBlockDelta, got {other:?}"),
        }

        // Last event should be MessageDelta with finish_reason
        let last = events.last().unwrap();
        match last {
            Ok(StreamEvent::MessageDelta { delta, .. }) => {
                // "stop" is normalized to "end_turn" for consistent handling across providers
                assert_eq!(delta.stop_reason, Some("end_turn".to_string()));
            }
            other => panic!("Expected MessageDelta at end, got {other:?}"),
        }
    }

    #[test]
    fn test_openai_usage_chunk_with_cached_tokens() {
        let lines = vec![
            r#"data: {"choices":[],"usage":{"prompt_tokens":100,"completion_tokens":50,"total_tokens":150,"prompt_tokens_details":{"cached_tokens":80}}}"#,
        ];
        let events = parse_sse_lines(&lines, LlmProvider::OpenAI);
        assert_eq!(events.len(), 1);
        match &events[0] {
            Ok(StreamEvent::MessageDelta { usage, .. }) => {
                assert_eq!(usage.input_tokens, 100);
                assert_eq!(usage.output_tokens, 50);
                assert_eq!(usage.cache_read_input_tokens, 80);
            }
            other => panic!("Expected MessageDelta with usage, got {other:?}"),
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
            Ok(StreamEvent::ContentBlockStart { .. }) => {}
            other => panic!("Expected ContentBlockStart, got {other:?}"),
        }

        // Second event should be ContentBlockDelta with arguments
        match &events[1] {
            Ok(StreamEvent::ContentBlockDelta { delta, .. }) => match delta {
                ContentDelta::InputJsonDelta { .. } => {}
                _ => panic!("Expected InputJsonDelta, got {delta:?}"),
            },
            other => panic!("Expected ContentBlockDelta, got {other:?}"),
        }
    }

    #[test]
    fn test_zhipu_terminal_chunk_with_usage_synthesizes_content_block_stop() {
        // Zhipu/GLM coding-plan shape: delta.tool_calls frames repeat the SAME
        // id while streaming argument fragments, and the TERMINAL chunk packs
        // `finish_reason: "tool_calls"` AND real usage into one frame. The
        // usage early-return in normalize_openai_event used to skip the
        // finish_reason branch entirely, so no ContentBlockStop was ever
        // synthesized and the engine broadcast no ToolUseRequest for the tool
        // call (dogfood 2026-08-27). The terminal chunk must close open tool
        // blocks BEFORE the usage MessageDelta.
        let lines = vec![
            r#"data: {"choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_z1","type":"function","function":{"name":"bash","arguments":""}}]},"finish_reason":null}]}"#,
            r#"data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_z1","function":{"arguments":"{\"command\":"}}]},"finish_reason":null}]}"#,
            r#"data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_z1","function":{"arguments":"\"ls\"}"}}]},"finish_reason":null}]}"#,
            r#"data: {"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":100,"completion_tokens":20,"total_tokens":120}}"#,
            "data: [DONE]",
        ];
        let events = parse_sse_lines(&lines, LlmProvider::OpenAI);

        // Exactly one ContentBlockStart despite the id repeating every frame.
        let starts: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Ok(StreamEvent::ContentBlockStart { index, .. }) => Some(*index),
                _ => None,
            })
            .collect();
        assert_eq!(starts, vec![0], "one start for the one real tool call");

        // Argument fragments stream as InputJsonDelta (including the empty
        // first-frame arguments:"").
        let arg_fragments: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Ok(StreamEvent::ContentBlockDelta {
                    delta: ContentDelta::InputJsonDelta { partial_json },
                    ..
                }) => Some(partial_json.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(arg_fragments, vec!["", "{\"command\":", "\"ls\"}"]);

        // The synthesized ContentBlockStop must precede the usage MessageDelta.
        let stop_idx = events
            .iter()
            .position(|e| matches!(e, Ok(StreamEvent::ContentBlockStop { index }) if *index == 0));
        let msg_delta_idx = events
            .iter()
            .position(|e| matches!(e, Ok(StreamEvent::MessageDelta { .. })));
        assert_eq!(stop_idx, Some(4), "stop synthesized after the three deltas");
        assert!(
            stop_idx.is_some() && msg_delta_idx.is_some() && stop_idx < msg_delta_idx,
            "ContentBlockStop (idx {stop_idx:?}) must precede MessageDelta (idx {msg_delta_idx:?})"
        );

        // Terminal MessageDelta carries BOTH the stop reason and the usage.
        match &events[msg_delta_idx.expect("checked above")] {
            Ok(StreamEvent::MessageDelta { delta, usage, .. }) => {
                assert_eq!(delta.stop_reason, Some("tool_calls".to_string()));
                assert_eq!(usage.input_tokens, 100);
                assert_eq!(usage.output_tokens, 20);
            }
            other => panic!("Expected terminal MessageDelta, got {other:?}"),
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
                assert_eq!(
                    delta,
                    &ContentDelta::TextDelta {
                        text: "Hello".to_string()
                    }
                );
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
        // Should have ContentBlockStart + InputJsonDelta + ContentBlockStop
        assert_eq!(events.len(), 3);

        match &events[0] {
            Ok(StreamEvent::ContentBlockStart { .. }) => {}
            other => panic!("Expected ContentBlockStart, got {other:?}"),
        }

        match &events[1] {
            Ok(StreamEvent::ContentBlockDelta {
                delta: ContentDelta::InputJsonDelta { .. },
                ..
            }) => {}
            other => panic!("Expected ContentBlockDelta InputJsonDelta, got {other:?}"),
        }

        match &events[2] {
            Ok(StreamEvent::ContentBlockStop { .. }) => {}
            other => panic!("Expected ContentBlockStop, got {other:?}"),
        }
    }

    #[test]
    fn test_ollama_ndjson_without_data_prefix() {
        // Ollama's /api/chat returns raw NDJSON without "data:" prefix
        let lines = vec![
            r#"{"model":"glm-4","message":{"role":"assistant","content":"Hello!"},"done":false}"#,
            r#"{"model":"glm-4","message":{"role":"assistant","content":" How can I help?"},"done":false}"#,
            r#"{"model":"glm-4","message":{"role":"assistant","content":""},"done":true,"prompt_eval_count":10,"eval_count":8}"#,
        ];
        let events = parse_sse_lines(&lines, LlmProvider::Ollama);
        assert!(events.len() >= 3);

        // First two should be text deltas
        match &events[0] {
            Ok(StreamEvent::ContentBlockDelta { delta, .. }) => {
                assert_eq!(
                    delta,
                    &ContentDelta::TextDelta {
                        text: "Hello!".to_string()
                    }
                );
            }
            other => panic!("Expected ContentBlockDelta, got {other:?}"),
        }

        match &events[1] {
            Ok(StreamEvent::ContentBlockDelta { delta, .. }) => {
                assert_eq!(
                    delta,
                    &ContentDelta::TextDelta {
                        text: " How can I help?".to_string()
                    }
                );
            }
            other => panic!("Expected ContentBlockDelta, got {other:?}"),
        }

        // Last should be MessageDelta with usage
        let last = events.last().unwrap();
        match last {
            Ok(StreamEvent::MessageDelta { usage, .. }) => {
                assert_eq!(usage.input_tokens, 10);
                assert_eq!(usage.output_tokens, 8);
            }
            other => panic!("Expected MessageDelta, got {other:?}"),
        }
    }

    // -- SSE comment and empty line handling --

    #[test]
    fn test_sse_comments_ignored() {
        let lines = vec![": this is a comment", "", "data: {\"type\":\"ping\"}"];
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
        let lines = vec![r#"data: {"choices":[]}"#];
        let events = parse_sse_lines(&lines, LlmProvider::OpenAI);
        // Should return empty, not error
        assert!(events.is_empty());
    }

    #[test]
    fn test_ollama_empty_content() {
        let lines = vec![r#"data: {"message":{"content":""}}"#];
        let events = parse_sse_lines(&lines, LlmProvider::Ollama);
        // Empty content should be skipped
        assert!(events.is_empty());
    }

    #[test]
    fn test_invalid_json_returns_error() {
        let lines = vec!["data: {invalid json}"];
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

    // -- Edge case: Anthropic message_delta before content_block_start --

    #[test]
    fn test_anthropic_message_delta_before_content_block_start() {
        // Anthropic sometimes sends message_delta (with stop_reason) before
        // the content_block_stop event. This should not panic or lose events.
        let lines = vec![
            r#"data: {"type":"message_start","message":{"id":"msg_edge1","role":"assistant","content":[],"model":"claude-3","stop_reason":null,"usage":{"input_tokens":10,"output_tokens":0}}}"#,
            r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hi"}}"#,
            r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"input_tokens":10,"output_tokens":2}}"#,
            r#"data: {"type":"content_block_stop","index":0}"#,
            "data: [DONE]",
        ];
        let events = parse_sse_lines(&lines, LlmProvider::Anthropic);
        // Should get: MessageStart, ContentBlockStart, ContentBlockDelta, MessageDelta, ContentBlockStop, MessageStop
        assert!(
            events.len() >= 4,
            "Expected at least 4 events, got {}",
            events.len()
        );

        // Verify the events are all Ok
        for (i, e) in events.iter().enumerate() {
            assert!(
                e.is_ok(),
                "Event {i} should be Ok, got Err: {:?}",
                e.as_ref().err()
            );
        }
    }

    // -- Edge case: Anthropic mixed text + tool_use in same stream --

    #[test]
    fn test_anthropic_mixed_text_and_tool_use() {
        // A single Anthropic SSE stream that produces both text content and a tool_use block
        let lines = vec![
            r#"data: {"type":"message_start","message":{"id":"msg_mix","role":"assistant","content":[],"model":"claude-3","stop_reason":null,"usage":{"input_tokens":20,"output_tokens":0}}}"#,
            r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Let me search for that."}}"#,
            r#"data: {"type":"content_block_stop","index":0}"#,
            r#"data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"tool_001","name":"web_search","input":{}}}"#,
            r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"query\":"}}"#,
            r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"\"rust\"}"}}"#,
            r#"data: {"type":"content_block_stop","index":1}"#,
            r#"data: {"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"input_tokens":20,"output_tokens":30}}"#,
            "data: [DONE]",
        ];
        let events = parse_sse_lines(&lines, LlmProvider::Anthropic);

        let text_deltas: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Ok(StreamEvent::ContentBlockDelta { delta, .. }) => {
                    if let ContentDelta::TextDelta { text } = delta {
                        Some(text.clone())
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect();

        assert!(
            text_deltas.iter().any(|t| t.contains("search")),
            "Should have text delta with 'search', got: {text_deltas:?}"
        );

        // Verify all events parsed successfully
        for (i, e) in events.iter().enumerate() {
            assert!(
                e.is_ok(),
                "Event {i} should be Ok, got: {:?}",
                e.as_ref().err()
            );
        }
    }

    // -- Edge case: OpenAI delta with no content or tool_calls fields at all --

    #[test]
    fn test_openai_delta_completely_empty() {
        // OpenAI sometimes sends chunks with an empty choices array or
        // delta objects with no content/tool_calls fields (role-only deltas, etc.)
        let lines = vec![
            r#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","created":1234,"model":"gpt-4","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}"#,
            r#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","created":1234,"model":"gpt-4","choices":[{"index":0,"delta":{},"finish_reason":null}]}"#,
            r#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","created":1234,"model":"gpt-4","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}"#,
        ];
        let events = parse_sse_lines(&lines, LlmProvider::OpenAI);
        // The empty delta and role-only delta should not produce content events
        let text_events: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Ok(StreamEvent::ContentBlockDelta { delta, .. }) => Some(delta.clone()),
                _ => None,
            })
            .collect();

        // Only the third chunk with "content":"Hello" should produce a text delta
        assert_eq!(
            text_events.len(),
            1,
            "Expected exactly 1 text delta from 3 chunks"
        );
        match &text_events[0] {
            ContentDelta::TextDelta { text } => assert_eq!(text, "Hello"),
            other => panic!("Expected TextDelta, got {other:?}"),
        }
    }

    // -- Edge case: Ollama streaming with done:true as final chunk --

    #[test]
    fn test_ollama_done_true_final_chunk() {
        let lines = vec![
            r#"data: {"message":{"role":"assistant","content":"Hi"}}"#,
            r#"data: {"message":{"role":"assistant","content":" there"}}"#,
            r#"data: {"done":true,"total_duration":123456789}"#,
        ];
        let events = parse_sse_lines(&lines, LlmProvider::Ollama);
        let text_deltas: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Ok(StreamEvent::ContentBlockDelta { delta, .. }) => {
                    if let ContentDelta::TextDelta { text } = delta {
                        Some(text.as_str())
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect();

        assert_eq!(text_deltas, vec!["Hi", " there"]);
        // done:true should not produce a content event
    }

    // -- Edge case: Multiple malformed JSON lines interspersed with valid ones --

    #[test]
    fn test_mixed_valid_invalid_sse_lines() {
        let lines = vec![
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"good"}}"#,
            r#"data: {broken"#,
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"recovered"}}"#,
        ];
        let events = parse_sse_lines(&lines, LlmProvider::Anthropic);

        let ok_count = events.iter().filter(|e| e.is_ok()).count();
        let err_count = events.iter().filter(|e| e.is_err()).count();

        assert_eq!(ok_count, 2, "Should have 2 successful events");
        assert_eq!(err_count, 1, "Should have 1 error from malformed JSON");

        // Verify the valid events' content
        let texts: Vec<String> = events
            .iter()
            .filter_map(|e| match e {
                Ok(StreamEvent::ContentBlockDelta { delta, .. }) => {
                    if let ContentDelta::TextDelta { text } = delta {
                        Some(text.clone())
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect();
        assert_eq!(texts, vec!["good", "recovered"]);
    }

    /// Test-only constructor for SseStream that doesn't require an HTTP response.
    impl SseStream {
        fn for_test(last_event_id: LastEventId) -> Self {
            // Create a no-op byte stream (immediately returns None)
            let byte_stream = Box::pin(futures::stream::empty());
            Self {
                chunks: byte_stream,
                buffer: String::new(),
                pending_events: std::collections::VecDeque::new(),
                done: false,
                provider: LlmProvider::Anthropic,
                openai_state: OpenaiStreamState::new(),
                last_event_id,
                idle_timeout: None,
                idle_timer: None,
            }
        }

        /// Test-only constructor with an injected byte-chunk stream and an
        /// explicit idle-watchdog budget (bypasses the env knob).
        fn for_test_with_chunks(
            chunks: ByteChunkStream,
            provider: LlmProvider,
            idle_timeout: Option<Duration>,
        ) -> Self {
            Self {
                chunks,
                buffer: String::new(),
                pending_events: std::collections::VecDeque::new(),
                done: false,
                provider,
                openai_state: OpenaiStreamState::new(),
                last_event_id: Arc::new(Mutex::new(None)),
                idle_timeout,
                idle_timer: None,
            }
        }
    }

    // ── Content-level stream idle watchdog (A5) ─────────────────────────
    //
    // The byte-level `read_timeout` cannot see content stalls: keepalive
    // bytes reset it forever. These tests drive `SseStream` with injected
    // fake byte streams (reqwest::Error is unconstructible here, and the
    // fakes model healthy-but-idle connections, so they never error).

    /// Fake byte-chunk stream emitting `pieces` with `gap` between chunks.
    /// `cycle = true` repeats the pieces forever (a server holding the
    /// connection open); `cycle = false` ends the stream after the last one.
    fn fake_chunks(pieces: Vec<Vec<u8>>, gap: Duration, cycle: bool) -> ByteChunkStream {
        let stream = futures::stream::unfold(
            (pieces, 0usize),
            move |(pieces, i): (Vec<Vec<u8>>, usize)| async move {
                tokio::time::sleep(gap).await;
                let piece = if cycle {
                    pieces.get(i % pieces.len())?
                } else {
                    pieces.get(i)?
                };
                Some((Ok(piece.clone()), (pieces, i + 1)))
            },
        );
        Box::pin(stream)
    }

    fn data_line(text: &str) -> Vec<u8> {
        format!(
            "data: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"{text}\"}}}}\n\n"
        )
        .into_bytes()
    }

    fn keepalive_line() -> Vec<u8> {
        b": keepalive\n\n".to_vec()
    }

    fn done_line() -> Vec<u8> {
        b"data: [DONE]\n\n".to_vec()
    }

    /// Keepalive bytes keep flowing but no content event ever arrives: the
    /// byte-level read_timeout would never fire, the content-level watchdog
    /// must abort the request once the idle budget is exhausted.
    #[tokio::test]
    async fn idle_watchdog_aborts_keepalive_only_stream() {
        use futures::StreamExt;
        use std::time::Instant;

        let chunks = fake_chunks(
            vec![keepalive_line()],
            Duration::from_millis(100),
            true,
        );
        let mut sse = SseStream::for_test_with_chunks(
            chunks,
            LlmProvider::Anthropic,
            Some(Duration::from_millis(400)),
        );

        let started = Instant::now();
        let mut saw_err = None;
        while let Some(event) = sse.next().await {
            if let Err(e) = event {
                saw_err = Some(e);
                break;
            }
        }
        let elapsed = started.elapsed();

        let err = saw_err.expect("watchdog must abort a stream that never produces content");
        assert!(
            matches!(err, ApiError::Timeout),
            "expected Timeout from the idle watchdog, got {err:?}"
        );
        assert!(
            elapsed >= Duration::from_millis(350),
            "fired at {elapsed:?} — must not fire before the idle budget"
        );
        assert!(
            elapsed < Duration::from_secs(3),
            "fired at {elapsed:?} — the watchdog, not the endless keepalive stream, must end it"
        );
    }

    /// Content arrives at intervals below the idle budget: every event
    /// resets the watchdog and the stream runs to completion even though
    /// the total transfer exceeds the budget.
    #[tokio::test]
    async fn idle_watchdog_tolerates_slow_but_flowing_content() {
        use futures::StreamExt;

        let mut pieces = vec![data_line("x"); 8];
        pieces.push(done_line());
        let chunks = fake_chunks(pieces, Duration::from_millis(150), false);
        let mut sse = SseStream::for_test_with_chunks(
            chunks,
            LlmProvider::Anthropic,
            Some(Duration::from_millis(400)),
        );

        let mut texts = 0;
        let mut saw_stop = false;
        while let Some(event) = sse.next().await {
            match event {
                Ok(StreamEvent::ContentBlockDelta { .. }) => texts += 1,
                Ok(StreamEvent::MessageStop) => saw_stop = true,
                Ok(_) => {}
                Err(e) => panic!("flowing content must never trip the watchdog: {e:?}"),
            }
        }
        assert_eq!(texts, 8, "every content delta must be delivered");
        assert!(saw_stop, "stream must end normally");
    }

    /// Watchdog disabled (the default: `SHANNON_STREAM_IDLE_SECS` unset or 0)
    /// adds no behavior — a stream with a long content gap between keepalives
    /// is delivered untouched. Armed at the same budget this stream would be
    /// aborted mid-gap.
    #[tokio::test]
    async fn idle_watchdog_disabled_keeps_current_behavior() {
        use futures::StreamExt;

        let pieces = vec![
            data_line("before"),
            keepalive_line(),
            keepalive_line(),
            keepalive_line(),
            keepalive_line(),
            keepalive_line(),
            data_line("after"),
            done_line(),
        ];
        let chunks = fake_chunks(pieces, Duration::from_millis(150), false);
        let mut sse =
            SseStream::for_test_with_chunks(chunks, LlmProvider::Anthropic, None);

        let mut text = String::new();
        let mut saw_stop = false;
        while let Some(event) = sse.next().await {
            match event {
                Ok(StreamEvent::ContentBlockDelta {
                    delta: ContentDelta::TextDelta { text: t },
                    ..
                }) => text.push_str(&t),
                Ok(StreamEvent::MessageStop) => saw_stop = true,
                Ok(_) => {}
                Err(e) => panic!("disabled watchdog must never abort: {e:?}"),
            }
        }
        assert_eq!(text, "beforeafter", "all content delivered across the gap");
        assert!(saw_stop, "stream must end normally");
    }

    /// `SHANNON_STREAM_IDLE_SECS` parsing: unset/empty/0/garbage → disabled;
    /// a positive value → that budget. Env mutation is process-global, so
    /// the case group holds the module env lock and restores prior values.
    #[test]
    fn stream_idle_timeout_env_parsing() {
        use std::sync::{Mutex, OnceLock};
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let _guard = ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("env-var test mutex poisoned");

        let saved = std::env::var("SHANNON_STREAM_IDLE_SECS").ok();

        // Unset → disabled.
        unsafe {
            std::env::remove_var("SHANNON_STREAM_IDLE_SECS");
        };
        assert_eq!(stream_idle_timeout_from_env(), None);

        // 0 → explicitly disabled (eval default; keeps current behavior).
        unsafe {
            std::env::set_var("SHANNON_STREAM_IDLE_SECS", "0");
        };
        assert_eq!(stream_idle_timeout_from_env(), None);

        // Empty → no-op (matches the `${VAR-default}` wrapper convention).
        unsafe {
            std::env::set_var("SHANNON_STREAM_IDLE_SECS", "");
        };
        assert_eq!(stream_idle_timeout_from_env(), None);

        // Garbage → disabled with a warning, not a panic.
        unsafe {
            std::env::set_var("SHANNON_STREAM_IDLE_SECS", "soon");
        };
        assert_eq!(stream_idle_timeout_from_env(), None);

        // Positive value → that budget.
        unsafe {
            std::env::set_var("SHANNON_STREAM_IDLE_SECS", "360");
        };
        assert_eq!(
            stream_idle_timeout_from_env(),
            Some(Duration::from_secs(360))
        );

        // Restore the prior process state for other tests.
        match saved.as_deref() {
            Some(v) => unsafe {
                std::env::set_var("SHANNON_STREAM_IDLE_SECS", v);
            },
            None => unsafe {
                std::env::remove_var("SHANNON_STREAM_IDLE_SECS");
            },
        }
    }
}
