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

/// Configuration for structured JSON output with schema validation.
///
/// When provided to headless mode, the assistant is instructed to return a
/// JSON object matching the given schema. The response is validated before
/// being emitted as the final output.
#[derive(Debug, Clone)]
pub struct StructuredOutputConfig {
    /// A JSON Schema (draft-07) that the assistant's response must satisfy.
    pub schema: serde_json::Value,
    /// Optional name for the structured output (used in system prompt).
    pub name: Option<String>,
}

impl StructuredOutputConfig {
    /// Create a new structured output config with the given JSON schema.
    pub fn new(schema: serde_json::Value) -> Self {
        Self { schema, name: None }
    }

    /// Set a descriptive name for the output type.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Generate a system prompt suffix instructing the model to return valid JSON.
    pub fn system_prompt_suffix(&self) -> String {
        let schema_str = serde_json::to_string_pretty(&self.schema)
            .unwrap_or_else(|_| self.schema.to_string());
        let type_name = self.name.as_deref().unwrap_or("the response");
        format!(
            "\n\nIMPORTANT: You MUST respond with a valid JSON object that conforms to this schema. \
             Do not include any text outside the JSON object.\n\n\
             Schema for {type_name}:\n```json\n{schema_str}\n```"
        )
    }

    /// Validate a response string against the configured schema.
    ///
    /// Returns `Ok(serde_json::Value)` if the response is valid JSON matching
    /// the schema, or `Err` with a descriptive message if validation fails.
    pub fn validate_response(&self, response: &str) -> Result<serde_json::Value, StructuredOutputError> {
        // Strip markdown code fences if present
        let trimmed = response
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();

        let value: serde_json::Value = serde_json::from_str(trimmed)
            .map_err(|e| StructuredOutputError::InvalidJson(e.to_string()))?;

        self.validate_value(&value)
    }

    /// Validate a parsed JSON value against the schema.
    fn validate_value(&self, value: &serde_json::Value) -> Result<serde_json::Value, StructuredOutputError> {
        let schema_type = self.schema.get("type").and_then(|t| t.as_str());

        // Basic type checking
        match schema_type {
            Some("object") if !value.is_object() => {
                return Err(StructuredOutputError::SchemaMismatch(
                    format!("Expected object, got {}", json_type_name(value)),
                ));
            }
            Some("array") if !value.is_array() => {
                return Err(StructuredOutputError::SchemaMismatch(
                    format!("Expected array, got {}", json_type_name(value)),
                ));
            }
            Some("string") if !value.is_string() => {
                return Err(StructuredOutputError::SchemaMismatch(
                    format!("Expected string, got {}", json_type_name(value)),
                ));
            }
            Some("number") if !value.is_number() => {
                return Err(StructuredOutputError::SchemaMismatch(
                    format!("Expected number, got {}", json_type_name(value)),
                ));
            }
            Some("boolean") if !value.is_boolean() => {
                return Err(StructuredOutputError::SchemaMismatch(
                    format!("Expected boolean, got {}", json_type_name(value)),
                ));
            }
            Some("integer") if !value.is_i64() && !value.is_u64() => {
                return Err(StructuredOutputError::SchemaMismatch(
                    format!("Expected integer, got {}", json_type_name(value)),
                ));
            }
            _ => {}
        }

        // Check required properties for objects
        if let (Some(required), Some(obj)) = (
            self.schema.get("required").and_then(|r| r.as_array()),
            value.as_object(),
        ) {
            for req in required {
                if let Some(key) = req.as_str() {
                    if !obj.contains_key(key) {
                        return Err(StructuredOutputError::SchemaMismatch(
                            format!("Missing required property: {key}"),
                        ));
                    }
                }
            }
        }

        Ok(value.clone())
    }
}

/// Errors from structured output validation.
#[derive(Debug, Clone, thiserror::Error)]
pub enum StructuredOutputError {
    #[error("Invalid JSON in response: {0}")]
    InvalidJson(String),
    #[error("Schema validation failed: {0}")]
    SchemaMismatch(String),
}

/// Get a human-readable type name for a JSON value.
fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
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
        let events = [OutputEvent::TextDelta { content: "a".into() },
            OutputEvent::TextDelta { content: "b".into() },
            OutputEvent::Done { exit_code: 0 }];
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

    // ── StructuredOutputConfig tests ──────────────────────────────────────

    #[test]
    fn test_structured_output_valid_object() {
        let config = StructuredOutputConfig::new(serde_json::json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "count": {"type": "integer"}
            },
            "required": ["name"]
        }));
        let result = config.validate_response(r#"{"name": "test", "count": 5}"#);
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val["name"], "test");
    }

    #[test]
    fn test_structured_output_missing_required_field() {
        let config = StructuredOutputConfig::new(serde_json::json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "integer"}
            },
            "required": ["name", "age"]
        }));
        let result = config.validate_response(r#"{"name": "test"}"#);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Missing required property: age"));
    }

    #[test]
    fn test_structured_output_wrong_type() {
        let config = StructuredOutputConfig::new(serde_json::json!({
            "type": "object"
        }));
        let result = config.validate_response(r#"[1, 2, 3]"#);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Expected object"));
    }

    #[test]
    fn test_structured_output_invalid_json() {
        let config = StructuredOutputConfig::new(serde_json::json!({"type": "object"}));
        let result = config.validate_response("not json at all");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid JSON"));
    }

    #[test]
    fn test_structured_output_strips_code_fences() {
        let config = StructuredOutputConfig::new(serde_json::json!({
            "type": "object",
            "properties": {"x": {"type": "number"}}
        }));
        let result = config.validate_response("```json\n{\"x\": 42}\n```");
        assert!(result.is_ok());
        assert_eq!(result.unwrap()["x"], 42);
    }

    #[test]
    fn test_structured_output_array_type() {
        let config = StructuredOutputConfig::new(serde_json::json!({"type": "array"}));
        let result = config.validate_response("[1, 2, 3]");
        assert!(result.is_ok());
    }

    #[test]
    fn test_structured_output_string_type() {
        let config = StructuredOutputConfig::new(serde_json::json!({"type": "string"}));
        let result = config.validate_response(r#""hello""#);
        assert!(result.is_ok());
    }

    #[test]
    fn test_structured_output_system_prompt_contains_schema() {
        let config = StructuredOutputConfig::new(serde_json::json!({
            "type": "object",
            "properties": {"result": {"type": "string"}}
        })).with_name("AnalysisResult");
        let prompt = config.system_prompt_suffix();
        assert!(prompt.contains("AnalysisResult"));
        assert!(prompt.contains("JSON"));
        assert!(prompt.contains("result"));
    }

    #[test]
    fn test_structured_output_config_with_name() {
        let config = StructuredOutputConfig::new(serde_json::json!({"type": "object"}))
            .with_name("MyOutput");
        assert_eq!(config.name, Some("MyOutput".to_string()));
    }

    #[test]
    fn test_json_type_name() {
        assert_eq!(super::json_type_name(&serde_json::Value::Null), "null");
        assert_eq!(super::json_type_name(&serde_json::json!(true)), "boolean");
        assert_eq!(super::json_type_name(&serde_json::json!(42)), "number");
        assert_eq!(super::json_type_name(&serde_json::json!("hi")), "string");
        assert_eq!(super::json_type_name(&serde_json::json!([])), "array");
        assert_eq!(super::json_type_name(&serde_json::json!({})), "object");
    }
}
