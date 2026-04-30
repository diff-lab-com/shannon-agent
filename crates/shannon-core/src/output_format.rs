//! NDJSON output format for structured, machine-readable event streaming.
//!
//! This module provides the [`OutputEvent`] enum and supporting types for emitting
//! newline-delimited JSON events during non-interactive (headless) execution.
//! Each event is a single JSON object on one line, making it easy for CI/CD
//! pipelines and other tools to parse the stream incrementally.
//!
//! # Usage
//!
//! ```rust,ignore
//! use shannon_core::output_format::{OutputEvent, ExitCode};
//!
//! // Emit a text delta
//! let event = OutputEvent::TextDelta { content: "Hello".into() };
//! println!("{}", event.to_ndjson());
//! // {"type":"text_delta","content":"Hello"}
//!
//! // Signal completion
//! let event = OutputEvent::Done { exit_code: ExitCode::SUCCESS as i32 };
//! println!("{}", event.to_ndjson());
//! // {"type":"done","exit_code":0}
//! ```

use serde::Serialize;

/// Well-known exit codes for non-interactive / CI/CD execution.
///
/// These codes let callers distinguish between success, generic errors,
/// timeouts, rate limiting, context window overflow, and permission failures.
pub mod exit_codes {
    /// Task completed successfully.
    pub const SUCCESS: i32 = 0;
    /// General error (API error, tool failure, etc.).
    pub const ERROR: i32 = 1;
    /// Maximum turns reached before the assistant signalled completion.
    pub const TURN_LIMIT: i32 = 2;
    /// Request timed out.
    pub const TIMEOUT: i32 = 3;
    /// Rate-limited by the upstream API provider.
    pub const RATE_LIMITED: i32 = 4;
    /// Conversation exceeded the model's context window.
    pub const CONTEXT_OVERFLOW: i32 = 5;
    /// A required permission was denied in non-interactive mode.
    pub const PERMISSION_DENIED: i32 = 6;
}

/// A single NDJSON event emitted during structured output streaming.
///
/// Each variant maps to a `"type"` discriminator field so consumers can
/// route events without inspecting every key.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum OutputEvent {
    /// Incremental text content from the assistant.
    #[serde(rename = "text_delta")]
    TextDelta { content: String },

    /// The assistant requested a tool invocation.
    #[serde(rename = "tool_use")]
    ToolUse {
        name: String,
        input: serde_json::Value,
    },

    /// A tool invocation finished.
    #[serde(rename = "tool_result")]
    ToolResult {
        name: String,
        output: String,
        is_error: bool,
    },

    /// An error that is not tied to a specific tool.
    #[serde(rename = "error")]
    Error { message: String },

    /// The session ended.
    #[serde(rename = "done")]
    Done { exit_code: i32 },
}

impl OutputEvent {
    /// Serialize this event as a single NDJSON line (JSON followed by `\n`).
    ///
    /// Returns an empty string on serialization failure (which should not
    /// happen with valid [`OutputEvent`] values).
    pub fn to_ndjson(&self) -> String {
        match serde_json::to_string(self) {
            Ok(json) => format!("{json}\n"),
            Err(_) => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── OutputEvent serialization ────────────────────────────────────────

    #[test]
    fn test_text_delta_serialization() {
        let event = OutputEvent::TextDelta {
            content: "Hello, world!".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"text_delta""#));
        assert!(json.contains(r#""content":"Hello, world!""#));
    }

    #[test]
    fn test_tool_use_serialization() {
        let event = OutputEvent::ToolUse {
            name: "Bash".into(),
            input: serde_json::json!({"command": "ls"}),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"tool_use""#));
        assert!(json.contains(r#""name":"Bash""#));
        assert!(json.contains(r#"ls"#));
    }

    #[test]
    fn test_tool_result_serialization() {
        let event = OutputEvent::ToolResult {
            name: "Read".into(),
            output: "file contents".into(),
            is_error: false,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"tool_result""#));
        assert!(json.contains(r#""is_error":false"#));
    }

    #[test]
    fn test_tool_result_error_serialization() {
        let event = OutputEvent::ToolResult {
            name: "Bash".into(),
            output: "command not found".into(),
            is_error: true,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""is_error":true"#));
    }

    #[test]
    fn test_error_serialization() {
        let event = OutputEvent::Error {
            message: "API rate limit exceeded".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"error""#));
        assert!(json.contains(r#"rate limit"#));
    }

    #[test]
    fn test_done_serialization() {
        let event = OutputEvent::Done {
            exit_code: exit_codes::SUCCESS,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"done""#));
        assert!(json.contains(r#""exit_code":0"#));
    }

    #[test]
    fn test_done_error_serialization() {
        let event = OutputEvent::Done {
            exit_code: exit_codes::ERROR,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""exit_code":1"#));
    }

    // ── to_ndjson format ─────────────────────────────────────────────────

    #[test]
    fn test_to_ndjson_ends_with_newline() {
        let event = OutputEvent::TextDelta {
            content: "hi".into(),
        };
        let line = event.to_ndjson();
        assert!(line.ends_with('\n'), "NDJSON line must end with newline");
        // Exactly one newline at the end
        assert_eq!(line.matches('\n').count(), 1);
    }

    #[test]
    fn test_to_ndjson_valid_json() {
        let event = OutputEvent::ToolUse {
            name: "Edit".into(),
            input: serde_json::json!({"path": "/tmp/f.rs"}),
        };
        let line = event.to_ndjson();
        // Strip trailing newline and parse
        let parsed: serde_json::Value =
            serde_json::from_str(line.trim()).expect("NDJSON must be valid JSON");
        assert_eq!(parsed["type"], "tool_use");
    }

    #[test]
    fn test_to_ndjson_one_json_per_line() {
        let events = vec![
            OutputEvent::TextDelta { content: "a".into() },
            OutputEvent::TextDelta { content: "b".into() },
            OutputEvent::Done { exit_code: 0 },
        ];
        let output: String = events.iter().map(|e| e.to_ndjson()).collect();
        // Each event produces exactly one line
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 3);
        // Each line is valid JSON
        for line in &lines {
            let _: serde_json::Value =
                serde_json::from_str(line).expect("each line must be valid JSON");
        }
    }

    // ── Exit code constants ──────────────────────────────────────────────

    #[test]
    fn test_exit_code_values() {
        assert_eq!(exit_codes::SUCCESS, 0);
        assert_eq!(exit_codes::ERROR, 1);
        assert_eq!(exit_codes::TURN_LIMIT, 2);
        assert_eq!(exit_codes::TIMEOUT, 3);
        assert_eq!(exit_codes::RATE_LIMITED, 4);
        assert_eq!(exit_codes::CONTEXT_OVERFLOW, 5);
        assert_eq!(exit_codes::PERMISSION_DENIED, 6);
    }

    #[test]
    fn test_exit_codes_are_distinct() {
        let codes = [
            exit_codes::SUCCESS,
            exit_codes::ERROR,
            exit_codes::TURN_LIMIT,
            exit_codes::TIMEOUT,
            exit_codes::RATE_LIMITED,
            exit_codes::CONTEXT_OVERFLOW,
            exit_codes::PERMISSION_DENIED,
        ];
        // All codes must be unique
        let mut seen = std::collections::HashSet::new();
        for code in codes {
            assert!(seen.insert(code), "duplicate exit code: {code}");
        }
    }

    #[test]
    fn test_exit_code_done_event_roundtrip() {
        for code in [
            exit_codes::SUCCESS,
            exit_codes::ERROR,
            exit_codes::CONTEXT_OVERFLOW,
            exit_codes::PERMISSION_DENIED,
        ] {
            let event = OutputEvent::Done { exit_code: code };
            let json = serde_json::to_string(&event).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed["exit_code"], code);
        }
    }
}
