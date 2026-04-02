//! Edit tool implementation

use crate::{ToolOutput, ToolError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EditInput {
    /// Absolute path to the file
    pub file_path: String,

    /// Text to replace
    pub old_string: String,

    /// Replacement text
    pub new_string: String,

    /// Replace all occurrences (default: false)
    #[serde(default)]
    pub replace_all: bool,
}

#[derive(Debug, Serialize)]
pub struct EditOutput {
    /// File path that was edited
    pub file_path: String,

    /// Number of replacements made
    pub replacements: usize,

    /// Success message
    pub message: String,
}

pub async fn execute(input: EditInput) -> Result<ToolOutput, ToolError> {
    use tokio::fs;

    let content = fs::read_to_string(&input.file_path)
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read file: {}", e)))?;

    let (new_content, replacements) = if input.replace_all {
        let count = content.matches(&input.old_string).count();
        (
            content.replace(&input.old_string, &input.new_string),
            count,
        )
    } else {
        match content.replacen(&input.old_string, &input.new_string, 1) {
            modified if modified != content => (modified, 1),
            _ => return Err(ToolError::InvalidInput(
                "Old string not found in file or old_string is not unique".to_string()
            ))
        }
    };

    fs::write(&input.file_path, new_content)
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("Failed to write file: {}", e)))?;

    Ok(ToolOutput {
        content: format!("Successfully made {} replacement(s)", replacements),
        is_error: false,
        metadata: {
            let mut map = HashMap::new();
            map.insert("file_path".to_string(), json!(input.file_path));
            map.insert("replacements".to_string(), json!(replacements));
            map
        },
    })
}
