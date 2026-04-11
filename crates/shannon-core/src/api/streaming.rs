//! Streaming response types and SSE implementation.
//!
//! Handles Server-Sent Events (SSE) streaming from LLM API providers.
//! Properly buffers partial events that span HTTP chunk boundaries.

use futures::{Stream, StreamExt, task::{Context, Poll}};
use std::pin::Pin;

use super::error::ApiError;
use super::types::{LlmProvider, StreamEvent};

/// Stream of API events
pub type MessageStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, ApiError>> + Send>>;

/// Internal byte-chunk stream type from reqwest
type ByteChunkStream = Pin<Box<dyn Stream<Item = Result<Vec<u8>, reqwest::Error>> + Send>>;

/// SSE stream that properly handles chunk boundaries.
///
/// Reads chunks from reqwest's byte stream, buffers partial lines,
/// and emits complete SSE events. Handles the common case where
/// a single SSE `data:` line spans multiple HTTP chunks.
pub struct SseStream {
    chunks: ByteChunkStream,
    buffer: String,
    pending_events: Vec<Result<StreamEvent, ApiError>>,
    done: bool,
    provider: LlmProvider,
}

impl SseStream {
    /// Create a new SSE stream from a reqwest response.
    ///
    /// Takes ownership of the response and consumes its byte stream.
    pub fn new(response: reqwest::Response, provider: LlmProvider) -> Self {
        let byte_stream = response.bytes_stream();
        // Convert Bytes to Vec<u8> to avoid direct dependency on bytes crate
        let mapped = Box::pin(byte_stream.map(|result| result.map(|b| b.to_vec())));
        Self {
            chunks: mapped,
            buffer: String::new(),
            pending_events: Vec::new(),
            done: false,
            provider,
        }
    }

    /// Parse all complete SSE lines from the buffer, queuing parsed events.
    /// Incomplete lines remain in the buffer for the next chunk.
    fn drain_buffer(&mut self) {
        while let Some(newline_pos) = self.buffer.find('\n') {
            let line = self.buffer[..newline_pos].to_string();
            self.buffer = self.buffer[newline_pos + 1..].to_string();

            if let Some(event) = self.parse_sse_line(&line) {
                self.pending_events.push(event);
            }
        }
    }

    /// Parse a single SSE line into an event using provider-specific normalization.
    fn parse_sse_line(&self, line: &str) -> Option<Result<StreamEvent, ApiError>> {
        let line = line.trim();

        // Skip empty lines and SSE comments
        if line.is_empty() || line.starts_with(':') {
            return None;
        }

        // SSE event fields: only process "data:" lines
        let json_str = if let Some(s) = line.strip_prefix("data: ") {
            s
        } else if let Some(s) = line.strip_prefix("data:") {
            s.trim()
        } else {
            // Ignore other SSE fields (event:, id:, retry:)
            return None;
        };

        if json_str == "[DONE]" {
            return Some(Ok(StreamEvent::MessageStop));
        }

        super::adapter::normalize_sse_event(json_str, &self.provider)
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
                        if let Some(event) = self.parse_sse_line(&remaining) {
                            self.done = true;
                            return Poll::Ready(Some(event));
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
    let sse = SseStream::new(response, provider);
    Box::pin(sse)
}
