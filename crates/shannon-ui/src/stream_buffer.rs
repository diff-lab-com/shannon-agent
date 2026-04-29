use std::collections::VecDeque;
use std::time::Instant;

const CATCHUP_QUEUE_THRESHOLD: usize = 8;
const CATCHUP_AGE_THRESHOLD_MS: u64 = 120;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StreamMode {
    Smooth,
    CatchUp,
}

pub struct StreamBuffer {
    pending_chunks: VecDeque<(String, Instant)>,
    mode: StreamMode,
    accumulated_text: String,
    needs_render: bool,
    smooth_count: u64,
    catchup_count: u64,
}

impl Default for StreamBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamBuffer {
    pub fn new() -> Self {
        Self {
            pending_chunks: VecDeque::new(),
            mode: StreamMode::Smooth,
            accumulated_text: String::new(),
            needs_render: false,
            smooth_count: 0,
            catchup_count: 0,
        }
    }

    pub fn push_chunk(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let now = Instant::now();
        self.pending_chunks.push_back((text.to_string(), now));
        self.accumulated_text.push_str(text);
        self.needs_render = true;
    }

    pub fn drain_for_render(&mut self) -> Option<String> {
        if self.pending_chunks.is_empty() {
            self.needs_render = false;
            return None;
        }

        let oldest_age = self
            .pending_chunks
            .front()
            .map(|(_, t)| t.elapsed().as_millis() as u64)
            .unwrap_or(0);

        let backpressure = self.pending_chunks.len() >= CATCHUP_QUEUE_THRESHOLD
            || oldest_age >= CATCHUP_AGE_THRESHOLD_MS;

        if backpressure {
            self.mode = StreamMode::CatchUp;
        }

        let result = match self.mode {
            StreamMode::CatchUp => {
                let batch: String = self
                    .pending_chunks
                    .drain(..)
                    .map(|(s, _)| s)
                    .collect();
                self.catchup_count += 1;
                self.mode = StreamMode::Smooth;
                batch
            }
            StreamMode::Smooth => {
                let (chunk, _) = self.pending_chunks.pop_front().unwrap();
                self.smooth_count += 1;
                chunk
            }
        };

        if self.pending_chunks.is_empty() {
            self.needs_render = false;
        }

        Some(result)
    }

    pub fn current_mode(&self) -> StreamMode {
        self.mode
    }

    pub fn accumulated_text(&self) -> &str {
        &self.accumulated_text
    }

    pub fn needs_render(&self) -> bool {
        self.needs_render
    }

    pub fn reset(&mut self) {
        self.pending_chunks.clear();
        self.mode = StreamMode::Smooth;
        self.accumulated_text.clear();
        self.needs_render = false;
        self.smooth_count = 0;
        self.catchup_count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_buffer_is_smooth_and_empty() {
        let buf = StreamBuffer::new();
        assert_eq!(buf.current_mode(), StreamMode::Smooth);
        assert!(buf.accumulated_text().is_empty());
        assert!(!buf.needs_render());
    }

    #[test]
    fn push_and_drain_single_chunk() {
        let mut buf = StreamBuffer::new();
        buf.push_chunk("hello");
        assert!(buf.needs_render());
        let chunk = buf.drain_for_render().unwrap();
        assert_eq!(chunk, "hello");
        assert_eq!(buf.accumulated_text(), "hello");
        assert!(!buf.needs_render());
    }

    #[test]
    fn drain_returns_none_when_empty() {
        let mut buf = StreamBuffer::new();
        assert!(buf.drain_for_render().is_none());
    }

    #[test]
    fn empty_push_is_ignored() {
        let mut buf = StreamBuffer::new();
        buf.push_chunk("");
        assert!(!buf.needs_render());
        assert!(buf.drain_for_render().is_none());
    }

    #[test]
    fn smooth_mode_drains_one_at_a_time() {
        let mut buf = StreamBuffer::new();
        buf.push_chunk("a");
        buf.push_chunk("b");
        buf.push_chunk("c");

        assert_eq!(buf.drain_for_render().unwrap(), "a");
        assert_eq!(buf.current_mode(), StreamMode::Smooth);
        assert_eq!(buf.drain_for_render().unwrap(), "b");
        assert_eq!(buf.drain_for_render().unwrap(), "c");
        assert!(buf.drain_for_render().is_none());
        assert_eq!(buf.accumulated_text(), "abc");
        assert_eq!(buf.smooth_count, 3);
        assert_eq!(buf.catchup_count, 0);
    }

    #[test]
    fn catchup_mode_on_queue_threshold() {
        let mut buf = StreamBuffer::new();
        for i in 0..CATCHUP_QUEUE_THRESHOLD {
            buf.push_chunk(&i.to_string());
        }

        let chunk = buf.drain_for_render().unwrap();
        assert!(chunk.len() >= CATCHUP_QUEUE_THRESHOLD);
        assert_eq!(buf.current_mode(), StreamMode::Smooth);
        assert_eq!(buf.catchup_count, 1);
        assert!(buf.drain_for_render().is_none());
    }

    #[test]
    fn accumulated_text_preserves_full_content() {
        let mut buf = StreamBuffer::new();
        buf.push_chunk("hello ");
        buf.push_chunk("world");
        assert_eq!(buf.accumulated_text(), "hello world");

        let _ = buf.drain_for_render();
        assert_eq!(buf.accumulated_text(), "hello world");
    }

    #[test]
    fn reset_clears_everything() {
        let mut buf = StreamBuffer::new();
        buf.push_chunk("data");
        let _ = buf.drain_for_render();
        buf.push_chunk("more");
        buf.reset();

        assert!(buf.accumulated_text().is_empty());
        assert!(!buf.needs_render());
        assert_eq!(buf.current_mode(), StreamMode::Smooth);
        assert_eq!(buf.smooth_count, 0);
        assert_eq!(buf.catchup_count, 0);
        assert!(buf.drain_for_render().is_none());
    }

    #[test]
    fn catchup_batches_all_pending() {
        let mut buf = StreamBuffer::new();
        for ch in "abcdefgh".chars() {
            buf.push_chunk(&ch.to_string());
        }

        let batch = buf.drain_for_render().unwrap();
        assert_eq!(batch, "abcdefgh");
        assert!(buf.drain_for_render().is_none());
    }

    #[test]
    fn smooth_then_catchup_then_smooth() {
        let mut buf = StreamBuffer::new();
        buf.push_chunk("x");
        buf.push_chunk("y");

        let first = buf.drain_for_render().unwrap();
        assert_eq!(first, "x");
        assert_eq!(buf.current_mode(), StreamMode::Smooth);

        for ch in "abcdefghijklmno".chars() {
            buf.push_chunk(&ch.to_string());
        }

        let batch = buf.drain_for_render().unwrap();
        assert!(batch.contains("abcdefghijklmno"));
        assert_eq!(buf.current_mode(), StreamMode::Smooth);
        assert!(buf.drain_for_render().is_none());

        buf.push_chunk("z");
        let last = buf.drain_for_render().unwrap();
        assert_eq!(last, "z");
        assert_eq!(buf.smooth_count, 2);
        assert_eq!(buf.catchup_count, 1);
    }
}
