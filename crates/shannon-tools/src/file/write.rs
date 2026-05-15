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

    const MAX_WRITE_SIZE: usize = 10 * 1024 * 1024; // 10 MB
    if input.content.len() > MAX_WRITE_SIZE {
        return Err(ToolError::InvalidInput(format!(
            "Content too large: {} bytes (max {} bytes)",
            input.content.len(),
            MAX_WRITE_SIZE
        )));
    }

    let bytes = input.content.len();

    // Atomic write: write to temp file then rename to avoid partial writes on crash.
    let temp_path = format!("{}.shannon-tmp", input.file_path);
    fs::write(&temp_path, &input.content)
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("Failed to write file: {e}")))?;
    fs::rename(&temp_path, &input.file_path)
        .await
        .map_err(|e| {
            // Clean up temp file if rename fails
            let _ = std::fs::remove_file(&temp_path);
            ToolError::ExecutionFailed(format!("Failed to rename temp file: {e}"))
        })?;

    Ok(ToolOutput {
        content: format!("Successfully wrote {bytes} bytes to file"),
        is_error: false,
        metadata: {
            let mut map = HashMap::new();
            map.insert("file_path".to_string(), json!(input.file_path));
            map.insert("bytes".to_string(), json!(bytes));
            map
        },
    })
}
