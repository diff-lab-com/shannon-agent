//! Session replayer — loads recorded sessions for deterministic replay testing.

use crate::QueryEvent;
use crate::vcr::{Vcr, VcrConfig, VcrRecording};
use serde_json::Value;
use std::path::Path;

use super::types::RecordingEntry;

/// Replays a recorded session by loading JSONL and providing mock LLM responses.
pub struct SessionReplayer {
    entries: Vec<RecordingEntry>,
    path: std::path::PathBuf,
}

impl SessionReplayer {
    /// Load a recording from a JSONL file.
    pub fn load_from_file(path: &Path) -> Result<Self, String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("Failed to read recording: {e}"))?;

        let entries: Vec<RecordingEntry> = content
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|line| serde_json::from_str(line).map_err(|e| format!("Parse error: {e}")))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            entries,
            path: path.to_path_buf(),
        })
    }

    /// Get all recorded LLM responses, ordered by turn.
    pub fn llm_responses(&self) -> Vec<(usize, Value)> {
        self.entries
            .iter()
            .filter_map(|e| match e {
                RecordingEntry::LlmResponse { turn, body } => Some((*turn, body.clone())),
                _ => None,
            })
            .collect()
    }

    /// Get all recorded LLM request/response pairs.
    pub fn llm_exchanges(&self) -> Vec<(usize, Value, Value)> {
        let requests: Vec<(usize, Value)> = self
            .entries
            .iter()
            .filter_map(|e| match e {
                RecordingEntry::LlmRequest { turn, body, .. } => Some((*turn, body.clone())),
                _ => None,
            })
            .collect();

        let responses: Vec<(usize, Value)> = self.llm_responses();

        requests
            .into_iter()
            .zip(responses)
            .filter_map(|(req, resp)| {
                if req.0 == resp.0 {
                    Some((req.0, req.1, resp.1))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get all recorded tool calls.
    pub fn tool_calls(&self) -> Vec<ToolCallRecord> {
        self.entries
            .iter()
            .filter_map(|e| match e {
                RecordingEntry::ToolCall {
                    tool,
                    input,
                    result,
                    is_error,
                    duration_ms,
                } => Some(ToolCallRecord {
                    tool: tool.clone(),
                    input: input.clone(),
                    result: result.clone(),
                    is_error: *is_error,
                    duration_ms: *duration_ms,
                }),
                _ => None,
            })
            .collect()
    }

    /// Get all recorded query events.
    pub fn query_events(&self) -> Vec<&QueryEvent> {
        self.entries
            .iter()
            .filter_map(|e| match e {
                RecordingEntry::QueryEvent { event } => Some(event),
                _ => None,
            })
            .collect()
    }

    /// Get recorded user messages.
    pub fn user_messages(&self) -> Vec<(usize, String)> {
        self.entries
            .iter()
            .filter_map(|e| match e {
                RecordingEntry::UserMessage { content, turn } => Some((*turn, content.clone())),
                _ => None,
            })
            .collect()
    }

    /// Verify that actual query events match the recorded sequence.
    /// Returns a list of mismatches (empty = perfect match).
    pub fn verify_query_events(&self, actual: &[QueryEvent]) -> Vec<String> {
        let recorded: Vec<&QueryEvent> = self.query_events();
        let mut mismatches = Vec::new();

        if recorded.len() != actual.len() {
            mismatches.push(format!(
                "Event count mismatch: recorded {} events, got {}",
                recorded.len(),
                actual.len()
            ));
            return mismatches;
        }

        for (i, (recorded_event, actual_event)) in recorded.iter().zip(actual.iter()).enumerate() {
            // Compare by variant name (not exact content, since UUIDs/timestamps differ)
            let recorded_variant = variant_name(recorded_event);
            let actual_variant = variant_name(actual_event);
            if recorded_variant != actual_variant {
                mismatches.push(format!(
                    "Event {i}: expected {recorded_variant}, got {actual_variant}"
                ));
            }
        }

        mismatches
    }

    /// Convert recorded LLM responses into a Vcr instance for replay.
    pub fn into_vcr(self) -> Vcr {
        let dir = self
            .path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("_vcr_replay");

        let mut vcr = Vcr::new(VcrConfig::replay_with_dir(&dir));

        for (turn, request, response) in self.llm_exchanges() {
            let recording = VcrRecording::new(request, response, vec![format!("turn-{turn}")]);
            vcr.insert_recording(recording);
        }

        vcr
    }

    /// Total number of turns in the recording.
    pub fn total_turns(&self) -> usize {
        self.entries
            .iter()
            .filter_map(|e| match e {
                RecordingEntry::UserMessage { turn, .. } => Some(*turn),
                _ => None,
            })
            .max()
            .unwrap_or(0)
    }
}

/// A recorded tool call.
#[derive(Debug, Clone)]
pub struct ToolCallRecord {
    pub tool: String,
    pub input: Value,
    pub result: String,
    pub is_error: bool,
    pub duration_ms: u64,
}

/// Get the variant name of a QueryEvent for comparison.
fn variant_name(event: &QueryEvent) -> &'static str {
    match event {
        QueryEvent::Started { .. } => "Started",
        QueryEvent::Text { .. } => "Text",
        QueryEvent::ToolUseRequest { .. } => "ToolUseRequest",
        QueryEvent::ToolUseResult { .. } => "ToolUseResult",
        QueryEvent::TurnCompleted { .. } => "TurnCompleted",
        QueryEvent::Completed { .. } => "Completed",
        QueryEvent::Failed { .. } => "Failed",
        QueryEvent::Progress { .. } => "Progress",
        QueryEvent::ToolProgress { .. } => "ToolProgress",
        QueryEvent::Thinking { .. } => "Thinking",
        QueryEvent::Usage { .. } => "Usage",
        QueryEvent::Cost { .. } => "Cost",
        QueryEvent::Info { .. } => "Info",
        QueryEvent::Warning { .. } => "Warning",
        QueryEvent::ConversationUpdate { .. } => "ConversationUpdate",
        QueryEvent::RateLimit { .. } => "RateLimit",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use uuid::Uuid;

    fn create_test_recording(dir: &std::path::Path) -> std::path::PathBuf {
        use crate::recording::recorder::SessionRecorder;
        let mut recorder = SessionRecorder::new("replay-test", "test-model", dir.to_path_buf());
        recorder.record_user_message("fix the error");
        recorder.record_llm_exchange(
            &json!({"model": "test", "messages": [{"role": "user", "content": "fix"}]}),
            &json!({"content": [{"type": "text", "text": "checking"}]}),
        );
        let qid = uuid::Uuid::new_v4();
        recorder.record_query_event(&QueryEvent::Started { query_id: qid });
        recorder.record_query_event(&QueryEvent::Text {
            query_id: qid,
            content: "Let me check".to_string(),
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
        recorder.record_query_event(&QueryEvent::Completed { query_id: qid });
        recorder.finish(500).unwrap()
    }

    #[test]
    fn test_replayer_loads_recording() {
        let dir = std::env::temp_dir().join(format!("shannon_replay_test_{}", Uuid::new_v4()));
        let path = create_test_recording(&dir);

        let replayer = SessionReplayer::load_from_file(&path).unwrap();
        assert_eq!(replayer.total_turns(), 1);
        assert_eq!(replayer.user_messages().len(), 1);
        assert_eq!(replayer.llm_responses().len(), 1);
        assert_eq!(replayer.tool_calls().len(), 1);
        assert_eq!(replayer.query_events().len(), 5); // Started, Text, ToolUseRequest, ToolUseResult, Completed

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_replayer_verifies_events() {
        let dir = std::env::temp_dir().join(format!("shannon_replay_test_{}", Uuid::new_v4()));
        let path = create_test_recording(&dir);

        let replayer = SessionReplayer::load_from_file(&path).unwrap();

        // Matching events
        let qid = uuid::Uuid::new_v4();
        let actual = vec![
            QueryEvent::Started { query_id: qid },
            QueryEvent::Text {
                query_id: qid,
                content: "other".to_string(),
            },
            QueryEvent::ToolUseRequest {
                query_id: qid,
                tool_use_id: "tu_1".to_string(),
                tool_name: "Read".to_string(),
                tool_input: serde_json::json!({"path": "other.rs"}),
            },
            QueryEvent::ToolUseResult {
                query_id: qid,
                tool_use_id: "tu_1".to_string(),
                tool_name: "Read".to_string(),
                result: "different content".to_string(),
                is_error: false,
            },
            QueryEvent::Completed { query_id: qid },
        ];
        let mismatches = replayer.verify_query_events(&actual);
        assert!(
            mismatches.is_empty(),
            "Expected no mismatches: {mismatches:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_replayer_detects_mismatches() {
        let dir = std::env::temp_dir().join(format!("shannon_replay_test_{}", Uuid::new_v4()));
        let path = create_test_recording(&dir);

        let replayer = SessionReplayer::load_from_file(&path).unwrap();

        let qid = uuid::Uuid::new_v4();
        let actual = vec![
            QueryEvent::Started { query_id: qid },
            QueryEvent::Completed { query_id: qid }, // Wrong: skipped Text, ToolUseResult
        ];
        let mismatches = replayer.verify_query_events(&actual);
        assert!(!mismatches.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_replayer_tool_call_extraction() {
        let dir = std::env::temp_dir().join(format!("shannon_replay_test_{}", Uuid::new_v4()));
        let path = create_test_recording(&dir);

        let replayer = SessionReplayer::load_from_file(&path).unwrap();
        let tool_calls = replayer.tool_calls();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].tool, "Read");
        assert_eq!(tool_calls[0].result, "fn main() {}");
        assert!(!tool_calls[0].is_error);
        assert!(tool_calls[0].duration_ms > 0 || tool_calls[0].duration_ms == 0); // just verify field exists

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_replayer_llm_exchange_pairs() {
        let dir = std::env::temp_dir().join(format!("shannon_replay_test_{}", Uuid::new_v4()));
        let path = create_test_recording(&dir);

        let replayer = SessionReplayer::load_from_file(&path).unwrap();
        let exchanges = replayer.llm_exchanges();
        assert_eq!(exchanges.len(), 1);
        let (turn, req, resp) = &exchanges[0];
        assert_eq!(*turn, 1);
        assert!(req.get("model").is_some());
        assert!(resp.get("content").is_some());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_replayer_into_vcr() {
        let dir = std::env::temp_dir().join(format!("shannon_replay_test_{}", Uuid::new_v4()));
        let path = create_test_recording(&dir);

        let replayer = SessionReplayer::load_from_file(&path).unwrap();
        let vcr = replayer.into_vcr();
        assert!(vcr.len() > 0, "Vcr should contain at least one recording");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
