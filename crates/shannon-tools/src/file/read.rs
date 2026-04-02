//! Read tool implementation

use crate::{ToolOutput, ToolError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

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

pub async fn execute(input: ReadInput) -> Result<ToolOutput, ToolError> {
    use tokio::fs;

    let content = fs::read_to_string(&input.file_path)
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read file: {}", e)))?;

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

    Ok(ToolOutput {
        content: selected_lines.clone(),
        is_error: false,
        metadata: {
            let mut map = HashMap::new();
            map.insert("lines".to_string(), json!(end - start));
            map.insert("file_path".to_string(), json!(input.file_path));
            map
        },
    })
}
