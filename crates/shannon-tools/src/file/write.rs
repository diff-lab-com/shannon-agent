//! Write tool implementation

use crate::{ToolOutput, ToolError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WriteInput {
    /// Absolute path to the file
    pub file_path: String,

    /// Content to write
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct WriteOutput {
    /// File path that was written
    pub file_path: String,

    /// Number of bytes written
    pub bytes: usize,

    /// Success message
    pub message: String,
}

pub async fn execute(input: WriteInput) -> Result<ToolOutput, ToolError> {
    use tokio::fs;

    fs::write(&input.file_path, &input.content)
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("Failed to write file: {}", e)))?;

    let bytes = input.content.len();

    Ok(ToolOutput {
        content: format!("Successfully wrote {} bytes to file", bytes),
        is_error: false,
        metadata: {
            let mut map = HashMap::new();
            map.insert("file_path".to_string(), json!(input.file_path));
            map.insert("bytes".to_string(), json!(bytes));
            map
        },
    })
}
