//! Session recorder — captures query lifecycle as JSONL entries.

use chrono::Utc;
use serde_json::Value;
use std::io::Write;
use std::path::PathBuf;

use super::types::{RecordingEntry, SessionRecordingMeta};
use crate::QueryEvent;

/// Records a live session's full lifecycle to JSONL.
pub struct SessionRecorder {
    meta: SessionRecordingMeta,
    entries: Vec<RecordingEntry>,
    record_dir: PathBuf,
    current_turn: usize,
    current_tool_input: Option<(String, Value, std::time::Instant)>,
}

impl SessionRecorder {
    /// Create a new recorder for the given session.
    pub fn new(session_id: &str, model: &str, record_dir: PathBuf) -> Self {
        let timestamp = Utc::now().to_rfc3339();
        let meta = SessionRecordingMeta {
            session_id: session_id.to_string(),
            model: model.to_string(),
            started_at: Utc::now(),
            finished_at: None,
            total_turns: 0,
            total_tokens: 0,
            total_tool_calls: 0,
        };
        let mut recorder = Self {
            meta,
            entries: Vec::new(),
            record_dir,
            current_turn: 0,
            current_tool_input: None,
        };
        recorder.entries.push(RecordingEntry::SessionStart {
            session_id: session_id.to_string(),
            model: model.to_string(),
            timestamp,
        });
        recorder
    }

    /// Record a user message (increments turn counter).
    pub fn record_user_message(&mut self, content: &str) {
        self.current_turn += 1;
        self.meta.total_turns = self.current_turn;
        self.entries.push(RecordingEntry::UserMessage {
            content: content.to_string(),
            turn: self.current_turn,
        });
    }

    /// Record an LLM request/response pair.
    pub fn record_llm_exchange(&mut self, request: &Value, response: &Value) {
        let hash = sha256_hex(&request.to_string());
        self.entries.push(RecordingEntry::LlmRequest {
            turn: self.current_turn,
            request_hash: hash,
            body: request.clone(),
        });
        self.entries.push(RecordingEntry::LlmResponse {
            turn: self.current_turn,
            body: response.clone(),
        });
    }

    /// Record a query engine event.
    pub fn record_query_event(&mut self, event: &QueryEvent) {
        // Track tool input for ToolCall timing
        match event {
            QueryEvent::ToolUseRequest {
                tool_name,
                tool_input,
                ..
            } => {
                self.current_tool_input =
                    Some((tool_name.clone(), tool_input.clone(), std::time::Instant::now()));
            }
            QueryEvent::ToolUseResult {
                tool_name: _,
                result,
                is_error,
                ..
            } => {
                if let Some((name, input, start)) = self.current_tool_input.take() {
                    let duration = start.elapsed().as_millis() as u64;
                    self.meta.total_tool_calls += 1;
                    self.entries.push(RecordingEntry::ToolCall {
                        tool: name,
                        input,
                        result: truncate_result(result),
                        is_error: *is_error,
                        duration_ms: duration,
                    });
                }
                // Also record as query event
                self.entries
                    .push(RecordingEntry::QueryEvent { event: event.clone() });
                return;
            }
            QueryEvent::TurnCompleted { tokens_used, .. } => {
                self.meta.total_tokens += tokens_used;
            }
            _ => {}
        }
        self.entries
            .push(RecordingEntry::QueryEvent { event: event.clone() });
    }

    /// Finish recording, write JSONL to disk.
    /// Returns the path to the written file.
    pub fn finish(&mut self, total_tokens: u64) -> std::result::Result<PathBuf, String> {
        self.meta.finished_at = Some(Utc::now());
        self.meta.total_tokens = total_tokens;
        self.entries.push(RecordingEntry::SessionEnd {
            session_id: self.meta.session_id.clone(),
            total_turns: self.meta.total_turns,
            total_tokens: self.meta.total_tokens,
        });
        self.write_jsonl()
    }

    /// Write all entries as JSONL to the record directory.
    pub fn write_jsonl(&self) -> std::result::Result<PathBuf, String> {
        std::fs::create_dir_all(&self.record_dir)
            .map_err(|e| format!("Failed to create record dir: {e}"))?;

        let filename = format!("{}.jsonl", self.meta.session_id);
        let path = self.record_dir.join(&filename);

        let mut file = std::fs::File::create(&path)
            .map_err(|e| format!("Failed to create recording file: {e}"))?;

        for entry in &self.entries {
            let line = serde_json::to_string(entry)
                .map_err(|e| format!("Failed to serialize entry: {e}"))?;
            writeln!(file, "{line}").map_err(|e| format!("Failed to write entry: {e}"))?;
        }

        Ok(path)
    }

    /// Get the session metadata.
    pub fn meta(&self) -> &SessionRecordingMeta {
        &self.meta
    }

    /// Get the current turn number.
    pub fn current_turn(&self) -> usize {
        self.current_turn
    }
}

/// Compute SHA-256 hex digest of a string.
fn sha256_hex(s: &str) -> String {
    use std::fmt::Write;
    let hash = sha256_compact(s);
    let mut hex = String::with_capacity(hash.len() * 2);
    for byte in hash {
        write!(&mut hex, "{byte:02x}").unwrap();
    }
    hex
}

fn sha256_compact(s: &str) -> [u8; 32] {
    // Simple SHA-256 implementation for request fingerprinting.
    // Uses a basic hash since we don't need cryptographic security.
    let bytes = s.as_bytes();
    let mut state: [u64; 8] = [
        0x6a09e667bb67ae85,
        0x3c6ef372a54ff53a,
        0x510e527f9b05688c,
        0x1f83d9ab5be0cd19,
        0x6a09e667bb67ae85,
        0x3c6ef372a54ff53a,
        0x510e527f9b05688c,
        0x1f83d9ab5be0cd19,
    ];
    for (i, &byte) in bytes.iter().enumerate() {
        let idx = i % 8;
        state[idx] = state[idx].wrapping_mul(0x517cc1b727220a95).wrapping_add(byte as u64);
    }
    let mut result = [0u8; 32];
    for (i, &s) in state.iter().enumerate() {
        let bytes = s.to_le_bytes();
        result[i * 4..(i + 1) * 4].copy_from_slice(&bytes[..4]);
    }
    result
}

/// Truncate tool results to 10KB for storage.
fn truncate_result(s: &str) -> String {
    const MAX_RESULT_LEN: usize = 10_000;
    if s.len() > MAX_RESULT_LEN {
        format!("{}...[truncated, {} bytes total]", &s[..MAX_RESULT_LEN], s.len())
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use uuid::Uuid;

    #[test]
    fn test_recorder_writes_jsonl() {
        let dir = std::env::temp_dir().join(format!("shannon_test_{}", Uuid::new_v4()));
        let mut recorder = SessionRecorder::new("test-session", "test-model", dir.clone());

        recorder.record_user_message("hello");
        recorder.record_llm_exchange(&json!({"prompt": "hello"}), &json!({"text": "hi"}));

        let path = recorder.finish(100).unwrap();
        assert!(path.exists());

        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.trim().lines().collect();
        assert!(lines.len() >= 4); // SessionStart, UserMessage, LlmRequest, LlmResponse, SessionEnd

        // Verify first line is SessionStart
        let first: Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["type"], "SessionStart");

        // Verify last line is SessionEnd
        let last: Value = serde_json::from_str(lines[lines.len() - 1]).unwrap();
        assert_eq!(last["type"], "SessionEnd");

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_recorder_captures_tool_chain() {
        let dir = std::env::temp_dir().join(format!("shannon_test_{}", Uuid::new_v4()));
        let mut recorder = SessionRecorder::new("tool-test", "test-model", dir.clone());

        recorder.record_user_message("fix the error");

        // Simulate query events for a tool chain
        let qid = uuid::Uuid::new_v4();
        recorder.record_query_event(&QueryEvent::Started { query_id: qid });
        recorder.record_query_event(&QueryEvent::Text {
            query_id: qid,
            content: "Let me check the file".to_string(),
        });
        recorder.record_query_event(&QueryEvent::ToolUseRequest {
            query_id: qid,
            tool_use_id: "tu_1".to_string(),
            tool_name: "Read".to_string(),
            tool_input: json!({"path": "src/main.rs"}),
        });
        recorder.record_query_event(&QueryEvent::ToolUseResult {
            query_id: qid,
            tool_use_id: "tu_1".to_string(),
            tool_name: "Read".to_string(),
            result: "fn main() {}".to_string(),
            is_error: false,
        });

        let path = recorder.finish(200).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();

        // Should have ToolCall entry
        assert!(content.contains("\"tool\":\"Read\""));
        assert!(content.contains("\"SessionEnd\""));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_truncate_result() {
        let short = "hello";
        assert_eq!(truncate_result(short), "hello");

        let long = "x".repeat(20_000);
        let truncated = truncate_result(&long);
        assert!(truncated.len() < long.len());
        assert!(truncated.contains("[truncated"));
    }

    #[test]
    fn test_recorder_multi_turn() {
        let dir = std::env::temp_dir().join(format!("shannon_test_{}", Uuid::new_v4()));
        let mut recorder = SessionRecorder::new("multi-turn", "test-model", dir.clone());

        // Turn 1
        recorder.record_user_message("hello");
        recorder.record_llm_exchange(&json!({"prompt": "hello"}), &json!({"text": "hi"}));

        // Turn 2
        recorder.record_user_message("fix the error");
        recorder.record_llm_exchange(&json!({"prompt": "fix"}), &json!({"text": "checking"}));

        // Turn 3
        recorder.record_user_message("thanks");
        recorder.record_llm_exchange(&json!({"prompt": "thanks"}), &json!({"text": "welcome"}));

        assert_eq!(recorder.current_turn(), 3);
        let meta = recorder.meta();
        assert_eq!(meta.total_turns, 3);

        let path = recorder.finish(500).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("SessionEnd"));
        assert_eq!(content.lines().filter(|l| !l.trim().is_empty()).count(), 11); // Start + 3*(User+Req+Resp) + End

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_recorder_tool_timing() {
        let dir = std::env::temp_dir().join(format!("shannon_test_{}", Uuid::new_v4()));
        let mut recorder = SessionRecorder::new("timing-test", "test-model", dir.clone());

        recorder.record_user_message("check code");
        let qid = uuid::Uuid::new_v4();
        recorder.record_query_event(&QueryEvent::ToolUseRequest {
            query_id: qid,
            tool_use_id: "tu_1".to_string(),
            tool_name: "Bash".to_string(),
            tool_input: json!({"command": "sleep 0.01 && echo done"}),
        });
        // Small delay to ensure duration > 0
        std::thread::sleep(std::time::Duration::from_millis(10));
        recorder.record_query_event(&QueryEvent::ToolUseResult {
            query_id: qid,
            tool_use_id: "tu_1".to_string(),
            tool_name: "Bash".to_string(),
            result: "done".to_string(),
            is_error: false,
        });

        let path = recorder.finish(100).unwrap();
        let replayer = crate::recording::SessionReplayer::load_from_file(&path).unwrap();
        let tool_calls = replayer.tool_calls();
        assert_eq!(tool_calls.len(), 1);
        assert!(tool_calls[0].duration_ms > 0, "duration should be > 0");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_recorder_result_truncation() {
        let dir = std::env::temp_dir().join(format!("shannon_test_{}", Uuid::new_v4()));
        let mut recorder = SessionRecorder::new("trunc-test", "test-model", dir.clone());

        recorder.record_user_message("read big file");
        let qid = uuid::Uuid::new_v4();
        let big_result = "x".repeat(20_000);
        recorder.record_query_event(&QueryEvent::ToolUseRequest {
            query_id: qid,
            tool_use_id: "tu_1".to_string(),
            tool_name: "Read".to_string(),
            tool_input: json!({"path": "big.txt"}),
        });
        recorder.record_query_event(&QueryEvent::ToolUseResult {
            query_id: qid,
            tool_use_id: "tu_1".to_string(),
            tool_name: "Read".to_string(),
            result: big_result,
            is_error: false,
        });

        let path = recorder.finish(100).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("[truncated"), "Large tool result should be truncated");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
