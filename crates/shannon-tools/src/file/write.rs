//! Write tool implementation

use super::super::ToolError;
use serde::{Deserialize, Serialize};

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

pub async fn execute(input: WriteInput) -> Result<serde_json::Value, ToolError> {
    use tokio::fs;

    fs::write(&input.file_path, &input.content)
        .await
        .map_err(|e| ToolError::FileError(format!("Failed to write file: {}", e)))?;

    let bytes = input.content.len();

    let output = WriteOutput {
        file_path: input.file_path,
        bytes,
        message: format!("Successfully wrote {} bytes to file", bytes),
    };

    serde_json::to_value(output).map_err(ToolError::from)
}
