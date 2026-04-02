//! File operation tools
//!
//! Provides implementations for:
//! - Read: Read file contents
//! - Write: Create/overwrite files
//! - Edit: Make targeted edits to files
//! - Glob: Pattern-based file search

use crate::{Tool, ToolError, ToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub mod read;
pub mod write;
pub mod edit;
pub mod glob;

/// File operation types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "operation")]
pub enum FileOperation {
    Read(read::ReadInput),
    Write(write::WriteInput),
    Edit(edit::EditInput),
    Glob(glob::GlobInput),
}

/// Base file tool trait
#[async_trait]
pub trait FileTool: Tool {
    /// Validate file path exists and is accessible
    async fn validate_path(&self, path: &str) -> Result<(), ToolError> {
        use tokio::fs;

        fs::metadata(path)
            .await
            .map_err(|e| ToolError::FileError(format!("Path validation failed for {}: {}", path, e)))?;

        Ok(())
    }

    /// Check if path is within allowed directory bounds
    fn is_path_allowed(&self, path: &str) -> bool {
        // TODO: Implement path sandboxing rules
        // For now, allow all paths - this should be restricted in production
        true
    }
}

/// Read tool implementation
pub struct ReadTool {
    description: String,
}

impl ReadTool {
    pub fn new() -> Self {
        Self {
            description: "Read file contents from the local filesystem".to_string(),
        }
    }
}

#[async_trait]
impl Tool for ReadTool {
    async fn execute(&self, input: serde_json::Value) -> ToolResult<serde_json::Value> {
        let read_input: read::ReadInput = serde_json::from_value(input)?;
        read::execute(read_input).await
    }

    fn name(&self) -> &str {
        "Read"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn validate_input(&self, input: &serde_json::Value) -> Result<(), ToolError> {
        // Validate required fields
        if !input.is_object() {
            return Err(ToolError::FileError("Input must be an object".to_string()));
        }

        if input.get("file_path").is_none() {
            return Err(ToolError::FileError("Missing required field: file_path".to_string()));
        }

        Ok(())
    }
}

/// Write tool implementation
pub struct WriteTool {
    description: String,
}

impl WriteTool {
    pub fn new() -> Self {
        Self {
            description: "Write content to a file, overwriting if it exists".to_string(),
        }
    }
}

#[async_trait]
impl Tool for WriteTool {
    async fn execute(&self, input: serde_json::Value) -> ToolResult<serde_json::Value> {
        let write_input: write::WriteInput = serde_json::from_value(input)?;
        write::execute(write_input).await
    }

    fn name(&self) -> &str {
        "Write"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn validate_input(&self, input: &serde_json::Value) -> Result<(), ToolError> {
        if !input.is_object() {
            return Err(ToolError::FileError("Input must be an object".to_string()));
        }

        if input.get("file_path").is_none() {
            return Err(ToolError::FileError("Missing required field: file_path".to_string()));
        }

        if input.get("content").is_none() {
            return Err(ToolError::FileError("Missing required field: content".to_string()));
        }

        Ok(())
    }
}

/// Edit tool implementation
pub struct EditTool {
    description: String,
}

impl EditTool {
    pub fn new() -> Self {
        Self {
            description: "Perform exact string replacements in files".to_string(),
        }
    }
}

#[async_trait]
impl Tool for EditTool {
    async fn execute(&self, input: serde_json::Value) -> ToolResult<serde_json::Value> {
        let edit_input: edit::EditInput = serde_json::from_value(input)?;
        edit::execute(edit_input).await
    }

    fn name(&self) -> &str {
        "Edit"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn validate_input(&self, input: &serde_json::Value) -> Result<(), ToolError> {
        if !input.is_object() {
            return Err(ToolError::FileError("Input must be an object".to_string()));
        }

        if input.get("file_path").is_none() {
            return Err(ToolError::FileError("Missing required field: file_path".to_string()));
        }

        if input.get("old_string").is_none() {
            return Err(ToolError::FileError("Missing required field: old_string".to_string()));
        }

        if input.get("new_string").is_none() {
            return Err(ToolError::FileError("Missing required field: new_string".to_string()));
        }

        Ok(())
    }
}

/// Glob tool implementation
pub struct GlobTool {
    description: String,
}

impl GlobTool {
    pub fn new() -> Self {
        Self {
            description: "Fast file pattern matching tool that works with any codebase size".to_string(),
        }
    }
}

#[async_trait]
impl Tool for GlobTool {
    async fn execute(&self, input: serde_json::Value) -> ToolResult<serde_json::Value> {
        let glob_input: glob::GlobInput = serde_json::from_value(input)?;
        glob::execute(glob_input).await
    }

    fn name(&self) -> &str {
        "Glob"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn validate_input(&self, input: &serde_json::Value) -> Result<(), ToolError> {
        if !input.is_object() {
            return Err(ToolError::FileError("Input must be an object".to_string()));
        }

        if input.get("pattern").is_none() {
            return Err(ToolError::FileError("Missing required field: pattern".to_string()));
        }

        Ok(())
    }
}
