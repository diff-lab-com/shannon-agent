//! File operation tools
//!
//! Provides implementations for:
//! - Read: Read file contents
//! - Write: Create/overwrite files
//! - Edit: Make targeted edits to files
//! - Glob: Pattern-based file search

use crate::{Tool, ToolError, ToolResult, ToolOutput};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

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
    fn name(&self) -> &str {
        "Read"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute path to the file"
                },
                "offset": {
                    "type": "integer",
                    "description": "Optional line offset for reading specific ranges"
                },
                "limit": {
                    "type": "integer",
                    "description": "Optional line limit"
                }
            },
            "required": ["file_path"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let read_input: read::ReadInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid read input: {}", e)))?;

        read::execute(read_input).await
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
    fn name(&self) -> &str {
        "Write"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute path to the file"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write"
                }
            },
            "required": ["file_path", "content"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let write_input: write::WriteInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid write input: {}", e)))?;
        write::execute(write_input).await
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
    fn name(&self) -> &str {
        "Edit"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute path to the file"
                },
                "old_string": {
                    "type": "string",
                    "description": "Text to replace"
                },
                "new_string": {
                    "type": "string",
                    "description": "Replacement text"
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace all occurrences (default: false)"
                }
            },
            "required": ["file_path", "old_string", "new_string"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let edit_input: edit::EditInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid edit input: {}", e)))?;
        edit::execute(edit_input).await
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
    fn name(&self) -> &str {
        "Glob"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "File pattern to match (e.g., *.rs, src/**/*.py)"
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let glob_input: glob::GlobInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid glob input: {}", e)))?;
        glob::execute(glob_input).await
    }
}
