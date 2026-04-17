//! Structured Output tool
//!
//! Allows the AI to return structured JSON output in a requested format.
//! Validates input against a provided JSON schema and returns the structured data.
//! Based on Claude Code's SyntheticOutputTool.

use crate::{Tool, ToolError, ToolResult, ToolOutput};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

pub const STRUCTURED_OUTPUT_TOOL_NAME: &str = "StructuredOutput";

/// Input for the structured output tool.
/// Uses `#[serde(flatten)]` to accept any valid JSON object.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StructuredOutputInput {
    /// The structured output data (any valid JSON object).
    /// Flattened so that all top-level keys are treated as the structured fields.
    #[serde(flatten)]
    pub data: serde_json::Map<String, Value>,
}

/// Output returned after successfully providing structured data.
#[derive(Debug, Clone, Serialize)]
pub struct StructuredOutputOutput {
    pub message: String,
    pub structured_output: serde_json::Map<String, Value>,
}

/// Structured output tool implementation.
///
/// This tool allows the AI to return structured JSON output in a requested format.
/// It accepts any JSON object and stores the structured data in metadata for
/// downstream consumption.
pub struct StructuredOutputTool;

impl StructuredOutputTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for StructuredOutputTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for StructuredOutputTool {
    fn name(&self) -> &str {
        STRUCTURED_OUTPUT_TOOL_NAME
    }

    fn description(&self) -> &str {
        "Return structured output in the requested format. Call this tool exactly once at the end of your response to provide the structured output."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "data": {
                    "type": "object",
                    "description": "The structured output data matching the requested schema"
                }
            },
            "required": ["data"]
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let parsed: StructuredOutputInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid structured output input: {e}")))?;

        let field_count = parsed.data.len();

        let mut metadata = HashMap::new();
        metadata.insert(
            "structured_output".to_string(),
            json!(parsed.data),
        );
        metadata.insert(
            "field_count".to_string(),
            json!(field_count),
        );

        Ok(ToolOutput {
            content: format!(
                "Structured output provided successfully ({field_count} fields)"
            ),
            is_error: false,
            metadata,
        })
    }
    fn is_read_only(&self) -> bool {        true    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_accepts_any_object() {
        let tool = StructuredOutputTool::new();
        let input = json!({
            "name": "test",
            "value": 42,
            "active": true
        });

        let result = tool.execute(input).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(!output.is_error);
        assert!(output.content.contains("3 fields"));
        assert_eq!(
            output.metadata.get("structured_output").unwrap()["name"],
            "test"
        );
        assert_eq!(
            output.metadata.get("field_count").unwrap(),
            3
        );
    }

    #[tokio::test]
    async fn test_rejects_non_object() {
        let tool = StructuredOutputTool::new();
        // A string is not an object, so flatten will fail
        let input = json!("just a string");

        let result = tool.execute(input).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ToolError::InvalidInput(msg) => {
                assert!(msg.contains("Invalid structured output input"));
            }
            other => panic!("Expected InvalidInput, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_empty_object() {
        let tool = StructuredOutputTool::new();
        let input = json!({});

        let result = tool.execute(input).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(!output.is_error);
        assert!(output.content.contains("0 fields"));
        assert_eq!(
            output.metadata.get("field_count").unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn test_nested_structures() {
        let tool = StructuredOutputTool::new();
        let input = json!({
            "user": {
                "name": "Alice",
                "tags": ["admin", "dev"]
            },
            "config": {
                "settings": {
                    "theme": "dark",
                    "notifications": true
                }
            }
        });

        let result = tool.execute(input).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(!output.is_error);
        assert!(output.content.contains("2 fields"));

        let structured = output.metadata.get("structured_output").unwrap();
        assert_eq!(structured["user"]["name"], "Alice");
        assert_eq!(structured["config"]["settings"]["theme"], "dark");
    }
}
