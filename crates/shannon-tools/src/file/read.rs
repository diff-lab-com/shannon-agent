//! Read tool implementation

use super::super::ToolError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReadInput {
    /// Absolute path to the file
    pub file_path: String,

    /// Optional line offset for reading specific ranges
    pub offset: Option<usize>,

    /// Optional line limit
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct ReadOutput {
    /// File contents as string
    pub content: String,

    /// Number of lines read
    pub lines: usize,

    /// File path
    pub file_path: String,
}

pub async fn execute(input: ReadInput) -> Result<serde_json::Value, ToolError> {
    use tokio::fs;

    let content = fs::read_to_string(&input.file_path)
        .await
        .map_err(|e| ToolError::FileError(format!("Failed to read file: {}", e)))?;

    let lines: Vec<&str> = content.lines().collect();

    let (start, end) = match (input.offset, input.limit) {
        (Some(offset), Some(limit)) => {
            let start = offset.min(lines.len());
            let end = (offset + limit).min(lines.len());
            (start, end)
        }
        (Some(offset), None) => (offset.min(lines.len()), lines.len()),
        (None, Some(limit)) => (0, limit.min(lines.len())),
        (None, None) => (0, lines.len()),
    };

    let selected_lines = lines[start..end].join("\n");

    let output = ReadOutput {
        content: selected_lines,
        lines: end - start,
        file_path: input.file_path,
    };

    serde_json::to_value(output).map_err(ToolError::from)
}
