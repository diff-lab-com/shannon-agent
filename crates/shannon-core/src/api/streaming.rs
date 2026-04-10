//! Streaming response types and SSE implementation.

use futures::{Stream, task::{Context, Poll}};

use super::error::ApiError;
use super::types::StreamEvent;

/// Stream of API events
pub type MessageStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, ApiError>> + Send>>;

use std::pin::Pin;

/// SSE stream implementation
#[allow(dead_code)]
pub struct SseStream {
    response: reqwest::Response,
    buffer: String,
    done: bool,
}

impl SseStream {
    pub(crate) fn new(response: reqwest::Response) -> Self {
        Self {
            response,
            buffer: String::new(),
            done: false,
        }
    }

    fn parse_sse_line(&mut self, line: &str) -> Option<Result<StreamEvent, ApiError>> {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with(':') {
            return None;
        }

        // Parse SSE format: "data: {...}"
        if let Some(json_str) = line.strip_prefix("data: ") {
            if json_str == "[DONE]" {
                self.done = true;
                return Some(Ok(StreamEvent::MessageStop));
            }

            match serde_json::from_str::<StreamEvent>(json_str) {
                Ok(event) => Some(Ok(event)),
                Err(e) => Some(Err(ApiError::InvalidResponse(format!(
                    "Failed to parse SSE event: {}", e
                )))),
            }
        } else {
            Some(Err(ApiError::InvalidResponse(format!(
                "Invalid SSE line: {}", line
            ))))
        }
    }
}

impl Stream for SseStream {
    type Item = Result<StreamEvent, ApiError>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        if self.done {
            return Poll::Ready(None);
        }

        // Process buffered lines first
        while let Some(pos) = self.buffer.find('\n') {
            let line = self.buffer[..pos].to_string();
            self.buffer = self.buffer[pos + 1..].to_string();

            if let Some(event) = self.parse_sse_line(&line) {
                return Poll::Ready(Some(event));
            }
        }

        Poll::Ready(None)
    }
}
