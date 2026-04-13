//! File operation tools
//!
//! Provides implementations for:
//! - Read: Read file contents
//! - Write: Create/overwrite files
//! - Edit: Make targeted edits to files
//! - Glob: Pattern-based file search
//!
//! All file operations are gated through a path sandbox that enforces
//! security boundaries: path traversal prevention, symlink resolution,
//! denied system paths, and home directory boundary checks.

use crate::{Tool, ToolError, ToolResult, ToolOutput};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;

pub mod read;
pub mod write;
pub mod edit;
pub mod glob;
pub mod sandbox;
pub mod history;
pub mod diff_renderer;
pub mod sandbox_adapter;

// Re-export sandbox types for external use
pub use sandbox::{PathSandbox, SandboxConfig, SandboxError};

/// File operation types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "operation")]
pub enum FileOperation {
    Read(read::ReadInput),
    Write(write::WriteInput),
    Edit(edit::EditInput),
    Glob(glob::GlobInput),
}

/// Validate a file path against the sandbox, converting sandbox errors
/// to tool errors that the tool trait expects.
async fn validate_path(sandbox: &PathSandbox, path: &str) -> ToolResult<()> {
    sandbox
        .validate(Path::new(path))
        .await
        .map_err(|e| ToolError::InvalidInput(format!("Path sandbox: {e}")))?;
    Ok(())
}

/// Read tool implementation
pub struct ReadTool {
    description: String,
    sandbox: PathSandbox,
}

impl Default for ReadTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ReadTool {
    pub fn new() -> Self {
        Self {
            description: "Read file contents from the local filesystem".to_string(),
            sandbox: PathSandbox::new(),
        }
    }

    /// Create a ReadTool with a custom sandbox configuration.
    pub fn with_sandbox(sandbox: PathSandbox) -> Self {
        Self {
            description: "Read file contents from the local filesystem".to_string(),
            sandbox,
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
            .map_err(|e| ToolError::InvalidInput(format!("Invalid read input: {e}")))?;

        validate_path(&self.sandbox, &read_input.file_path).await?;

        read::execute(read_input).await
    }
}

/// Write tool implementation
pub struct WriteTool {
    description: String,
    sandbox: PathSandbox,
}

impl Default for WriteTool {
    fn default() -> Self {
        Self::new()
    }
}

impl WriteTool {
    pub fn new() -> Self {
        Self {
            description: "Write content to a file, overwriting if it exists".to_string(),
            sandbox: PathSandbox::new(),
        }
    }

    /// Create a WriteTool with a custom sandbox configuration.
    pub fn with_sandbox(sandbox: PathSandbox) -> Self {
        Self {
            description: "Write content to a file, overwriting if it exists".to_string(),
            sandbox,
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
            .map_err(|e| ToolError::InvalidInput(format!("Invalid write input: {e}")))?;

        validate_path(&self.sandbox, &write_input.file_path).await?;

        write::execute(write_input).await
    }
}

/// Edit tool implementation
pub struct EditTool {
    description: String,
    sandbox: PathSandbox,
}

impl Default for EditTool {
    fn default() -> Self {
        Self::new()
    }
}

impl EditTool {
    pub fn new() -> Self {
        Self {
            description: "Perform exact string replacements in files".to_string(),
            sandbox: PathSandbox::new(),
        }
    }

    /// Create an EditTool with a custom sandbox configuration.
    pub fn with_sandbox(sandbox: PathSandbox) -> Self {
        Self {
            description: "Perform exact string replacements in files".to_string(),
            sandbox,
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
            .map_err(|e| ToolError::InvalidInput(format!("Invalid edit input: {e}")))?;

        validate_path(&self.sandbox, &edit_input.file_path).await?;

        edit::execute(edit_input).await
    }
}

/// Glob tool implementation
pub struct GlobTool {
    description: String,
    sandbox: PathSandbox,
}

impl Default for GlobTool {
    fn default() -> Self {
        Self::new()
    }
}

impl GlobTool {
    pub fn new() -> Self {
        Self {
            description: "Fast file pattern matching tool that works with any codebase size".to_string(),
            sandbox: PathSandbox::new(),
        }
    }

    /// Create a GlobTool with a custom sandbox configuration.
    pub fn with_sandbox(sandbox: PathSandbox) -> Self {
        Self {
            description: "Fast file pattern matching tool that works with any codebase size".to_string(),
            sandbox,
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
            .map_err(|e| ToolError::InvalidInput(format!("Invalid glob input: {e}")))?;

        // Validate the base path (if provided) through the sandbox
        if let Some(ref base_path) = glob_input.path {
            validate_path(&self.sandbox, base_path).await?;
        }

        glob::execute(glob_input).await
    }
}
