//! # BriefTool
//!
//! Generates concise task summaries from conversation history or content.
//!
//! Provides text-based summarization without LLM calls, extracting key information
//! such as actions taken, files modified, errors encountered, and decisions made.

use crate::{Tool, ToolError, ToolResult, ToolOutput};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

/// Format for the brief output
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum BriefFormat {
    Plain,
    #[default]
    Markdown,
}


/// A single message in a conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BriefMessage {
    /// Message role ("user" or "assistant")
    pub role: String,
    /// Message content
    pub content: String,
}

/// Input for the BriefTool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BriefInput {
    /// Content to summarize (if no messages provided)
    pub content: Option<String>,
    /// Conversation messages to summarize
    pub messages: Option<Vec<BriefMessage>>,
    /// Max summary length in characters (default: 500)
    pub max_length: Option<usize>,
    /// Output format (default: Markdown)
    pub format: Option<BriefFormat>,
}

/// Extracted summary sections
struct BriefSummary {
    actions: Vec<String>,
    results: Vec<String>,
    issues: Vec<String>,
    next_steps: Vec<String>,
}

/// BriefTool generates concise task summaries from content or conversation history.
pub struct BriefTool {
    description: String,
}

impl BriefTool {
    pub fn new() -> Self {
        Self {
            description: "Generate a concise task summary from conversation history or content".to_string(),
        }
    }

    /// Extract summary from plain content
    fn summarize_content(&self, content: &str) -> BriefSummary {
        let mut summary = BriefSummary {
            actions: Vec::new(),
            results: Vec::new(),
            issues: Vec::new(),
            next_steps: Vec::new(),
        };

        let lines: Vec<&str> = content.lines().collect();

        for line in &lines {
            let trimmed = line.trim();

            // Detect action patterns (verbs at start of line, imperative sentences)
            if self.is_action_line(trimmed) {
                summary.actions.push(self.clean_line(trimmed));
            }

            // Detect error/warning patterns
            if self.is_error_line(trimmed) {
                summary.issues.push(self.clean_line(trimmed));
            }

            // Detect result/success patterns
            if self.is_result_line(trimmed) {
                summary.results.push(self.clean_line(trimmed));
            }

            // Detect next step / TODO patterns
            if self.is_next_step_line(trimmed) {
                summary.next_steps.push(self.clean_line(trimmed));
            }
        }

        // If no structured sections found, create a general summary
        if summary.actions.is_empty()
            && summary.results.is_empty()
            && summary.issues.is_empty()
            && summary.next_steps.is_empty()
        {
            let sentences: Vec<&str> = content
                .split(['.', '!', '?'])
                .filter(|s| !s.trim().is_empty())
                .collect();

            // Take the most informative sentences (first few and last few)
            let key_count = std::cmp::min(sentences.len(), 5);
            if key_count > 0 {
                summary.results = sentences[..key_count]
                    .iter()
                    .map(|s| s.trim().to_string())
                    .collect();
            }
        }

        summary
    }

    /// Extract summary from conversation messages
    fn summarize_messages(&self, messages: &[BriefMessage]) -> BriefSummary {
        let mut summary = BriefSummary {
            actions: Vec::new(),
            results: Vec::new(),
            issues: Vec::new(),
            next_steps: Vec::new(),
        };

        for msg in messages {
            // Extract from assistant messages primarily
            if msg.role == "assistant" {
                let content_summary = self.summarize_content(&msg.content);
                summary.actions.extend(content_summary.actions);
                summary.results.extend(content_summary.results);
                summary.issues.extend(content_summary.issues);
                summary.next_steps.extend(content_summary.next_steps);
            } else {
                // From user messages, extract key requests as actions
                let trimmed = msg.content.trim();
                if trimmed.len() > 10 && trimmed.len() < 200 {
                    summary.actions.push(format!("Requested: {trimmed}"));
                }
            }
        }

        // Deduplicate while preserving order
        summary.actions = self.dedup(&summary.actions);
        summary.results = self.dedup(&summary.results);
        summary.issues = self.dedup(&summary.issues);
        summary.next_steps = self.dedup(&summary.next_steps);

        summary
    }

    /// Format the summary into a string
    fn format_summary(&self, summary: &BriefSummary, format: &BriefFormat, max_length: usize) -> String {
        let mut sections = Vec::new();

        if !summary.actions.is_empty() {
            let items: Vec<String> = summary
                .actions
                .iter()
                .map(|a| format!("  - {a}"))
                .collect();
            sections.push(("Actions", items.join("\n")));
        }

        if !summary.results.is_empty() {
            let items: Vec<String> = summary
                .results
                .iter()
                .map(|r| format!("  - {r}"))
                .collect();
            sections.push(("Results", items.join("\n")));
        }

        if !summary.issues.is_empty() {
            let items: Vec<String> = summary
                .issues
                .iter()
                .map(|i| format!("  - {i}"))
                .collect();
            sections.push(("Issues", items.join("\n")));
        }

        if !summary.next_steps.is_empty() {
            let items: Vec<String> = summary
                .next_steps
                .iter()
                .map(|n| format!("  - {n}"))
                .collect();
            sections.push(("Next Steps", items.join("\n")));
        }

        let output = match format {
            BriefFormat::Markdown => {
                let mut parts = Vec::new();
                for (title, body) in &sections {
                    parts.push(format!("### {title}\n{body}"));
                }
                parts.join("\n\n")
            }
            BriefFormat::Plain => {
                let mut parts = Vec::new();
                for (title, body) in &sections {
                    parts.push(format!("{title}:\n{body}"));
                }
                parts.join("\n\n")
            }
        };

        self.truncate(&output, max_length)
    }

    /// Truncate text to max_length while preserving structure
    fn truncate(&self, text: &str, max_length: usize) -> String {
        if text.len() <= max_length {
            return text.to_string();
        }

        // Try to truncate at a section boundary
        let mut length = 0;
        let mut last_boundary = 0;
        let mut in_section = false;

        for (i, c) in text.char_indices() {
            length += 1;
            if c == '\n' && (text.chars().nth(i + 1) == Some('\n')) {
                last_boundary = i;
                in_section = true;
            }
            if length >= max_length {
                if in_section && last_boundary > 0 {
                    let mut truncated = text[..last_boundary].to_string();
                    truncated.push_str("\n\n... (truncated)");
                    return truncated;
                }
                // Hard truncate at word boundary
                let end = text[..i].rfind(' ').unwrap_or(i);
                let mut truncated = text[..end].to_string();
                truncated.push_str("...");
                return truncated;
            }
        }

        text.to_string()
    }

    /// Check if a line represents an action
    fn is_action_line(&self, line: &str) -> bool {
        let action_prefixes = [
            "created", "updated", "modified", "deleted", "added", "removed",
            "implemented", "refactored", "fixed", "changed", "installed",
            "configured", "deployed", "built", "ran", "executed", "wrote",
            "committed", "merged", "moved", "renamed", "generated", "set up",
        ];
        let lower = line.to_lowercase();
        action_prefixes.iter().any(|prefix| lower.starts_with(prefix))
            && line.len() > 10
    }

    /// Check if a line represents an error or warning
    fn is_error_line(&self, line: &str) -> bool {
        let lower = line.to_lowercase();
        lower.contains("error:")
            || lower.contains("failed")
            || lower.contains("warning:")
            || lower.starts_with("error")
            || lower.contains("exception")
            || lower.contains("panic")
            || lower.contains("not found")
            || lower.contains("denied")
    }

    /// Check if a line represents a result or success
    fn is_result_line(&self, line: &str) -> bool {
        let lower = line.to_lowercase();
        lower.contains("success")
            || lower.contains("completed")
            || lower.contains("passed")
            || lower.contains("verified")
            || lower.contains("confirmed")
            || lower.starts_with("result:")
    }

    /// Check if a line represents a next step
    fn is_next_step_line(&self, line: &str) -> bool {
        let lower = line.to_lowercase();
        lower.contains("todo:")
            || lower.contains("next:")
            || lower.contains("remaining:")
            || lower.contains("pending:")
            || lower.starts_with("- [ ]")
            || lower.starts_with("- [x]")
            || lower.contains("need to")
            || lower.contains("should ")
    }

    /// Clean a line for summary output
    fn clean_line(&self, line: &str) -> String {
        // Remove leading markdown bullets/list markers
        let trimmed = line.trim();
        let cleaned = trimmed
            .strip_prefix("- [ ] ")
            .or_else(|| trimmed.strip_prefix("- [x] "))
            .or_else(|| trimmed.strip_prefix("- "))
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("TODO: "))
            .or_else(|| trimmed.strip_prefix("TODO:"))
            .or_else(|| trimmed.strip_prefix("NEXT: "))
            .or_else(|| trimmed.strip_prefix("NEXT:"))
            .unwrap_or(trimmed);
        // Truncate very long lines
        if cleaned.len() > 200 {
            format!("{}...", &cleaned[..197])
        } else {
            cleaned.to_string()
        }
    }

    /// Deduplicate a list while preserving order
    fn dedup(&self, items: &[String]) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();
        for item in items {
            if seen.insert(item.clone()) {
                result.push(item.clone());
            }
        }
        result
    }

    /// Core summarization logic
    fn generate_brief(&self, input: BriefInput) -> Result<String, ToolError> {
        let max_length = input.max_length.unwrap_or(500);
        let format = input.format.unwrap_or_default();

        // Determine content source
        let text = match (input.content, input.messages) {
            (Some(content), _) if !content.trim().is_empty() => {
                let summary = self.summarize_content(&content);
                self.format_summary(&summary, &format, max_length)
            }
            (_, Some(messages)) if !messages.is_empty() => {
                let summary = self.summarize_messages(&messages);
                self.format_summary(&summary, &format, max_length)
            }
            _ => {
                return Err(ToolError::InvalidInput(
                    "Either 'content' or 'messages' must be provided".to_string(),
                ));
            }
        };

        Ok(text)
    }
}

impl Default for BriefTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for BriefTool {
    fn name(&self) -> &str {
        "Brief"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "Content to summarize (use if no messages provided)"
                },
                "messages": {
                    "type": "array",
                    "description": "Conversation messages to summarize",
                    "items": {
                        "type": "object",
                        "properties": {
                            "role": {
                                "type": "string",
                                "description": "Message role ('user' or 'assistant')",
                                "enum": ["user", "assistant"]
                            },
                            "content": {
                                "type": "string",
                                "description": "Message content"
                            }
                        },
                        "required": ["role", "content"]
                    }
                },
                "max_length": {
                    "type": "integer",
                    "description": "Maximum summary length in characters (default: 500)",
                    "minimum": 50,
                    "maximum": 5000
                },
                "format": {
                    "type": "string",
                    "description": "Output format",
                    "enum": ["plain", "markdown"],
                    "default": "markdown"
                }
            }
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let brief_input: BriefInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid brief input: {e}")))?;

        let summary = self.generate_brief(brief_input)?;

        Ok(ToolOutput {
            content: summary,
            is_error: false,
            metadata: HashMap::new(),
        })
    }
    fn is_read_only(&self) -> bool {        true    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tool() -> BriefTool {
        BriefTool::new()
    }

    #[tokio::test]
    async fn test_summarize_plain_content() {
        let tool = make_tool();
        let input = BriefInput {
            content: Some("Created auth module. Updated database schema. Fixed login bug.".to_string()),
            messages: None,
            max_length: Some(500),
            format: Some(BriefFormat::Markdown),
        };

        let result = tool.generate_brief(input).unwrap();
        assert!(result.contains("Created auth module"));
        assert!(result.contains("Updated database schema"));
    }

    #[tokio::test]
    async fn test_summarize_conversation_messages() {
        let tool = make_tool();
        let input = BriefInput {
            content: None,
            messages: Some(vec![
                BriefMessage {
                    role: "user".to_string(),
                    content: "Please fix the login bug".to_string(),
                },
                BriefMessage {
                    role: "assistant".to_string(),
                    content: "Fixed the authentication flow. Updated session handling. Error: timeout on redirect was resolved.".to_string(),
                },
            ]),
            max_length: Some(500),
            format: Some(BriefFormat::Markdown),
        };

        let result = tool.generate_brief(input).unwrap();
        assert!(!result.is_empty());
        assert!(result.contains("### Actions") || result.contains("Actions:"));
    }

    #[tokio::test]
    async fn test_max_length_truncation() {
        let tool = make_tool();
        let long_content = "Created the authentication module with JWT token support. \
            Updated the database schema to include user sessions table. \
            Modified the API routes for login and logout endpoints. \
            Fixed the session expiration bug that caused premature logouts. \
            Added rate limiting to the authentication endpoint. \
            Implemented password reset functionality with email verification. \
            Refactored the user model to separate concerns. \
            Added comprehensive unit tests for all auth flows.";

        let input = BriefInput {
            content: Some(long_content.to_string()),
            messages: None,
            max_length: Some(100),
            format: Some(BriefFormat::Plain),
        };

        let result = tool.generate_brief(input).unwrap();
        assert!(result.len() <= 110, "Summary should be truncated: {} chars", result.len());
    }

    #[tokio::test]
    async fn test_empty_input_returns_error() {
        let tool = make_tool();
        let input = BriefInput {
            content: None,
            messages: None,
            max_length: None,
            format: None,
        };

        let result = tool.generate_brief(input);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Either 'content' or 'messages'"));
    }

    #[tokio::test]
    async fn test_empty_content_returns_error() {
        let tool = make_tool();
        let input = BriefInput {
            content: Some("   ".to_string()),
            messages: None,
            max_length: None,
            format: None,
        };

        let result = tool.generate_brief(input);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_empty_messages_returns_error() {
        let tool = make_tool();
        let input = BriefInput {
            content: None,
            messages: Some(vec![]),
            max_length: None,
            format: None,
        };

        let result = tool.generate_brief(input);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_plain_format() {
        let tool = make_tool();
        let input = BriefInput {
            content: Some("Created auth module. Error: connection timeout. Completed tests successfully.".to_string()),
            messages: None,
            max_length: Some(500),
            format: Some(BriefFormat::Plain),
        };

        let result = tool.generate_brief(input).unwrap();
        // Plain format should not contain markdown headers
        assert!(!result.contains("###"));
        assert!(result.contains("Actions:") || result.contains("Issues:") || result.contains("Results:"));
    }

    #[tokio::test]
    async fn test_markdown_format() {
        let tool = make_tool();
        let input = BriefInput {
            content: Some("Created auth module. Fixed login bug. Tests passed successfully.".to_string()),
            messages: None,
            max_length: Some(500),
            format: Some(BriefFormat::Markdown),
        };

        let result = tool.generate_brief(input).unwrap();
        // Markdown format should contain headers
        assert!(result.contains("### Actions") || result.contains("### Results") || result.contains("### Issues"));
    }

    #[tokio::test]
    async fn test_long_content_handling() {
        let tool = make_tool();
        let mut lines = Vec::new();
        for i in 0..100 {
            lines.push(format!("Created module number {i} with extensive functionality and many features"));
        }
        let long_content = lines.join(". ");

        let input = BriefInput {
            content: Some(long_content),
            messages: None,
            max_length: Some(200),
            format: Some(BriefFormat::Plain),
        };

        let result = tool.generate_brief(input).unwrap();
        assert!(result.len() <= 210, "Long content should be truncated: {} chars", result.len());
    }

    #[tokio::test]
    async fn test_error_detection() {
        let tool = make_tool();
        let input = BriefInput {
            content: Some(
                "Error: database connection failed. Warning: deprecated API usage. \
                 Panic: null pointer in handler. Authentication denied for user."
                    .to_string(),
            ),
            messages: None,
            max_length: Some(500),
            format: Some(BriefFormat::Markdown),
        };

        let result = tool.generate_brief(input).unwrap();
        assert!(result.contains("### Issues"));
    }

    #[tokio::test]
    async fn test_default_values() {
        let tool = make_tool();
        let input = BriefInput {
            content: Some("Created module. Fixed bug.".to_string()),
            messages: None,
            max_length: None, // should default to 500
            format: None,     // should default to Markdown
        };

        let result = tool.generate_brief(input).unwrap();
        assert!(result.len() <= 510);
    }

    #[test]
    fn test_brief_format_default() {
        assert_eq!(BriefFormat::default(), BriefFormat::Markdown);
    }

    #[test]
    fn test_brief_format_serialization() {
        let md = BriefFormat::Markdown;
        let serialized = serde_json::to_string(&md).unwrap();
        assert_eq!(serialized, "\"markdown\"");

        let plain = BriefFormat::Plain;
        let serialized = serde_json::to_string(&plain).unwrap();
        assert_eq!(serialized, "\"plain\"");
    }

    #[test]
    fn test_tool_name_and_description() {
        let tool = make_tool();
        assert_eq!(tool.name(), "Brief");
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn test_input_schema_validity() {
        let tool = make_tool();
        let schema = tool.input_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"].is_object());
        assert!(schema["properties"]["content"].is_object());
        assert!(schema["properties"]["messages"].is_object());
        assert!(schema["properties"]["max_length"].is_object());
        assert!(schema["properties"]["format"].is_object());
    }

    #[tokio::test]
    async fn test_tool_execute_interface() {
        let tool = make_tool();
        let input = json!({
            "content": "Created the module. Fixed the bug. Tests passed.",
            "max_length": 500,
            "format": "markdown"
        });

        let result = tool.execute(input).await.unwrap();
        assert!(!result.is_error);
        assert!(!result.content.is_empty());
    }

    #[tokio::test]
    async fn test_tool_execute_with_messages() {
        let tool = make_tool();
        let input = json!({
            "messages": [
                {"role": "user", "content": "Implement auth"},
                {"role": "assistant", "content": "Created auth module. Updated routes. Error: missing config was fixed."}
            ]
        });

        let result = tool.execute(input).await.unwrap();
        assert!(!result.is_error);
        assert!(!result.content.is_empty());
    }

    #[tokio::test]
    async fn test_tool_execute_invalid_input() {
        let tool = make_tool();
        let input = json!({
            "invalid_field": "value"
        });

        let result = tool.execute(input).await;
        assert!(result.is_err());
    }
}
