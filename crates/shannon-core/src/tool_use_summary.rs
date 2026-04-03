//! Tool Use Summary
//!
//! Generates human-readable summaries of completed tool batches.
//! Produces git-commit-style short labels like "Fixed NPE in UserService".

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Information about a completed tool call
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUseInfo {
    pub name: String,
    pub input: Value,
    pub output: Value,
}

/// Summary generation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUseSummary {
    pub label: String,
    pub tools_processed: usize,
}

pub struct ToolUseSummaryGenerator;

impl ToolUseSummaryGenerator {
    pub fn new() -> Self {
        Self
    }

    /// Generate a short summary label for a batch of tool calls
    pub fn generate(&self, tools: &[ToolUseInfo]) -> Option<ToolUseSummary> {
        if tools.is_empty() {
            return None;
        }

        let _actions: Vec<String> = tools
            .iter()
            .map(|tool| {
                let input_summary = summarize_value(&tool.input, 100);
                format!("{}({})", tool.name, input_summary)
            })
            .collect();

        // Generate a git-commit-style label
        let label = self.generate_label(tools);

        Some(ToolUseSummary {
            label,
            tools_processed: tools.len(),
        })
    }

    fn generate_label(&self, tools: &[ToolUseInfo]) -> String {
        let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();

        // Pattern matching for common operations
        match tool_names.as_slice() {
            [name] if name.contains("Read") || name.contains("Glob") || name.contains("Grep") => {
                let target = extract_target(&tools[0].input);
                format!("Searched in {}", target)
            }
            [name] if name.contains("Write") || name.contains("Edit") => {
                let target = extract_target(&tools[0].input);
                format!("Modified {}", target)
            }
            [name] if name.contains("Bash") => {
                let cmd = tools[0]
                    .input
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let truncated = cmd.chars().take(50).collect::<String>();
                format!("Ran: {}", truncated)
            }
            _ => {
                let primary = tool_names.first().unwrap_or(&"Tool");
                format!("{} ({} calls)", primary, tools.len())
            }
        }
    }
}

/// Extract a file path target from tool input.
/// Shows the full path from the input for clarity.
fn extract_target(input: &Value) -> String {
    input
        .get("file_path")
        .or_else(|| input.get("path"))
        .or_else(|| input.get("pattern"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "files".to_string())
}

/// Summarize a JSON value to a short string
fn summarize_value(value: &Value, max_len: usize) -> String {
    let s = serde_json::to_string(value).unwrap_or_else(|_| "[complex]".to_string());
    if s.len() <= max_len {
        s
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_tool(name: &str, input: Value) -> ToolUseInfo {
        ToolUseInfo {
            name: name.to_string(),
            input,
            output: json!({}),
        }
    }

    #[test]
    fn test_empty_tools() {
        let generator = ToolUseSummaryGenerator::new();
        assert!(generator.generate(&[]).is_none());
    }

    #[test]
    fn test_read_tool() {
        let generator = ToolUseSummaryGenerator::new();
        let tools = vec![make_tool("Read", json!({"file_path": "src/main.rs"}))];
        let summary = generator.generate(&tools).unwrap();
        assert_eq!(summary.label, "Searched in src/main.rs");
        assert_eq!(summary.tools_processed, 1);
    }

    #[test]
    fn test_edit_tool() {
        let generator = ToolUseSummaryGenerator::new();
        let tools = vec![make_tool(
            "Edit",
            json!({"file_path": "src/lib.rs", "old_string": "foo", "new_string": "bar"}),
        )];
        let summary = generator.generate(&tools).unwrap();
        assert_eq!(summary.label, "Modified src/lib.rs");
    }

    #[test]
    fn test_bash_tool() {
        let generator = ToolUseSummaryGenerator::new();
        let tools = vec![make_tool("Bash", json!({"command": "cargo test --workspace"}))];
        let summary = generator.generate(&tools).unwrap();
        assert!(summary.label.starts_with("Ran:"));
    }

    #[test]
    fn test_multiple_tools() {
        let generator = ToolUseSummaryGenerator::new();
        let tools = vec![
            make_tool("Read", json!({"file_path": "src/main.rs"})),
            make_tool("Edit", json!({"file_path": "src/main.rs"})),
            make_tool("Bash", json!({"command": "cargo check"})),
        ];
        let summary = generator.generate(&tools).unwrap();
        assert!(summary.label.contains("Read"));
        assert_eq!(summary.tools_processed, 3);
    }

    #[test]
    fn test_nested_path() {
        let generator = ToolUseSummaryGenerator::new();
        let tools = vec![make_tool(
            "Read",
            json!({"file_path": "crates/shannon-core/src/lib.rs"}),
        )];
        let summary = generator.generate(&tools).unwrap();
        assert!(summary.label.contains("crates/shannon-core/src/lib.rs"));
    }
}
