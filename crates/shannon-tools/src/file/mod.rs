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

use crate::{Tool, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};

pub mod diff_renderer;
pub mod edit;
pub mod glob;
pub mod history;
pub mod merge;
pub mod merge_tool;
pub mod multiedit;
pub mod read;
pub mod sandbox;
pub mod sandbox_adapter;
pub mod write;

// Re-export sandbox types for external use
pub use sandbox::{PathSandbox, SandboxConfig, SandboxError};

// Re-export merge resolve tool for external use
pub use merge_tool::MergeResolveTool;

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
/// to tool errors that the tool trait expects. Returns the canonical path
/// to avoid TOCTOU issues when the tool later accesses the filesystem.
async fn validate_path(sandbox: &PathSandbox, path: &str) -> ToolResult<PathBuf> {
    sandbox
        .validate(Path::new(path))
        .await
        .map_err(|e| ToolError::InvalidInput(format!("Path sandbox: {e}")))
}

/// Validate a path for writing (allows non-existent target files).
async fn validate_write_path(sandbox: &PathSandbox, path: &str) -> ToolResult<PathBuf> {
    sandbox
        .validate_for_write(Path::new(path))
        .await
        .map_err(|e| ToolError::InvalidInput(format!("Path sandbox: {e}")))
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

        let canonical = validate_path(&self.sandbox, &read_input.file_path).await?;
        let mut input = read_input;
        input.file_path = canonical.to_string_lossy().to_string();

        read::execute(input).await
    }
    fn is_read_only(&self) -> bool {
        true
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

        let canonical = validate_write_path(&self.sandbox, &write_input.file_path).await?;
        let mut input = write_input;
        input.file_path = canonical.to_string_lossy().to_string();

        write::execute(input).await
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

        let canonical = validate_path(&self.sandbox, &edit_input.file_path).await?;
        let mut input = edit_input;
        input.file_path = canonical.to_string_lossy().to_string();

        edit::execute(input).await
    }
}

/// Atomic multi-file edit tool
pub struct MultiEditTool {
    description: String,
    sandbox: PathSandbox,
}

impl Default for MultiEditTool {
    fn default() -> Self {
        Self::new()
    }
}

impl MultiEditTool {
    pub fn new() -> Self {
        Self {
            description:
                "Apply multiple file edits atomically — all edits succeed or none are applied"
                    .to_string(),
            sandbox: PathSandbox::new(),
        }
    }

    pub fn with_sandbox(sandbox: PathSandbox) -> Self {
        Self {
            description:
                "Apply multiple file edits atomically — all edits succeed or none are applied"
                    .to_string(),
            sandbox,
        }
    }
}

#[async_trait]
impl Tool for MultiEditTool {
    fn name(&self) -> &str {
        "MultiEdit"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "edits": {
                    "type": "array",
                    "description": "List of edit operations to apply atomically",
                    "items": {
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
                    }
                }
            },
            "required": ["edits"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let mut multi_input: multiedit::MultiEditInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid multi-edit input: {e}")))?;

        for op in &mut multi_input.edits {
            let canonical = validate_path(&self.sandbox, &op.file_path).await?;
            op.file_path = canonical.to_string_lossy().to_string();
        }

        multiedit::execute(multi_input).await
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
            description: "Fast file pattern matching tool that works with any codebase size"
                .to_string(),
            sandbox: PathSandbox::new(),
        }
    }

    /// Create a GlobTool with a custom sandbox configuration.
    pub fn with_sandbox(sandbox: PathSandbox) -> Self {
        Self {
            description: "Fast file pattern matching tool that works with any codebase size"
                .to_string(),
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
    fn is_read_only(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── FileOperation serde ─────────────────────────────────────

    #[test]
    fn file_operation_read_tag() {
        let json = serde_json::json!({
            "operation": "Read",
            "file_path": "/tmp/test.txt"
        });
        let op: FileOperation = serde_json::from_value(json).unwrap();
        match op {
            FileOperation::Read(r) => assert_eq!(r.file_path, "/tmp/test.txt"),
            _ => panic!("Expected Read variant"),
        }
    }

    #[test]
    fn file_operation_write_tag() {
        let json = serde_json::json!({
            "operation": "Write",
            "file_path": "/tmp/out.txt",
            "content": "hello"
        });
        let op: FileOperation = serde_json::from_value(json).unwrap();
        match op {
            FileOperation::Write(w) => {
                assert_eq!(w.file_path, "/tmp/out.txt");
                assert_eq!(w.content, "hello");
            }
            _ => panic!("Expected Write variant"),
        }
    }

    #[test]
    fn file_operation_edit_tag() {
        let json = serde_json::json!({
            "operation": "Edit",
            "file_path": "/tmp/test.txt",
            "old_string": "foo",
            "new_string": "bar"
        });
        let op: FileOperation = serde_json::from_value(json).unwrap();
        match op {
            FileOperation::Edit(e) => {
                assert_eq!(e.file_path, "/tmp/test.txt");
                assert_eq!(e.old_string, "foo");
                assert_eq!(e.new_string, "bar");
            }
            _ => panic!("Expected Edit variant"),
        }
    }

    #[test]
    fn file_operation_glob_tag() {
        let json = serde_json::json!({
            "operation": "Glob",
            "pattern": "**/*.rs"
        });
        let op: FileOperation = serde_json::from_value(json).unwrap();
        match op {
            FileOperation::Glob(g) => assert_eq!(g.pattern, "**/*.rs"),
            _ => panic!("Expected Glob variant"),
        }
    }

    #[test]
    fn file_operation_unknown_tag_fails() {
        let json = serde_json::json!({
            "operation": "Delete",
            "file_path": "/tmp/test.txt"
        });
        assert!(serde_json::from_value::<FileOperation>(json).is_err());
    }

    // ── Tool name/description/schema ────────────────────────────

    #[test]
    fn read_tool_name_and_schema() {
        let tool = ReadTool::new();
        assert_eq!(tool.name(), "Read");
        assert!(tool.description().contains("Read"));
        let schema = tool.input_schema();
        assert!(schema["properties"]["file_path"].is_object());
        assert!(schema["properties"]["offset"].is_object());
        assert!(schema["properties"]["limit"].is_object());
    }

    #[test]
    fn read_tool_is_read_only() {
        let tool = ReadTool::new();
        assert!(tool.is_read_only());
    }

    #[test]
    fn read_tool_default() {
        let tool = ReadTool::default();
        assert_eq!(tool.name(), "Read");
    }

    #[test]
    fn write_tool_name_and_schema() {
        let tool = WriteTool::new();
        assert_eq!(tool.name(), "Write");
        let schema = tool.input_schema();
        assert!(schema["properties"]["file_path"].is_object());
        assert!(schema["properties"]["content"].is_object());
    }

    #[test]
    fn write_tool_default() {
        let tool = WriteTool::default();
        assert_eq!(tool.name(), "Write");
    }

    #[test]
    fn edit_tool_name_and_schema() {
        let tool = EditTool::new();
        assert_eq!(tool.name(), "Edit");
        let schema = tool.input_schema();
        assert!(schema["properties"]["old_string"].is_object());
        assert!(schema["properties"]["new_string"].is_object());
        assert!(schema["properties"]["replace_all"].is_object());
    }

    #[test]
    fn edit_tool_default() {
        let tool = EditTool::default();
        assert_eq!(tool.name(), "Edit");
    }

    #[test]
    fn multiedit_tool_name_and_schema() {
        let tool = MultiEditTool::new();
        assert_eq!(tool.name(), "MultiEdit");
        let schema = tool.input_schema();
        assert!(schema["properties"]["edits"].is_object());
    }

    #[test]
    fn multiedit_tool_default() {
        let tool = MultiEditTool::default();
        assert_eq!(tool.name(), "MultiEdit");
    }

    #[test]
    fn glob_tool_name_and_schema() {
        let tool = GlobTool::new();
        assert_eq!(tool.name(), "Glob");
        let schema = tool.input_schema();
        assert!(schema["properties"]["pattern"].is_object());
    }

    #[test]
    fn glob_tool_is_read_only() {
        let tool = GlobTool::new();
        assert!(tool.is_read_only());
    }

    #[test]
    fn glob_tool_default() {
        let tool = GlobTool::default();
        assert_eq!(tool.name(), "Glob");
    }

    // ── with_sandbox constructors ───────────────────────────────

    #[test]
    fn read_tool_with_sandbox() {
        let sandbox = PathSandbox::new();
        let tool = ReadTool::with_sandbox(sandbox);
        assert_eq!(tool.name(), "Read");
    }

    #[test]
    fn write_tool_with_sandbox() {
        let sandbox = PathSandbox::new();
        let tool = WriteTool::with_sandbox(sandbox);
        assert_eq!(tool.name(), "Write");
    }

    #[test]
    fn edit_tool_with_sandbox() {
        let sandbox = PathSandbox::new();
        let tool = EditTool::with_sandbox(sandbox);
        assert_eq!(tool.name(), "Edit");
    }

    #[test]
    fn glob_tool_with_sandbox() {
        let sandbox = PathSandbox::new();
        let tool = GlobTool::with_sandbox(sandbox);
        assert_eq!(tool.name(), "Glob");
    }

    #[test]
    fn send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ReadTool>();
        assert_send_sync::<WriteTool>();
        assert_send_sync::<EditTool>();
        assert_send_sync::<MultiEditTool>();
        assert_send_sync::<GlobTool>();
    }

    // ── Write tool: new file creation via sandbox ────────────────────

    #[tokio::test]
    async fn test_write_tool_creates_new_file_in_sandbox() {
        let dir = tempfile::TempDir::new().unwrap();
        let sandbox = PathSandbox::with_config(crate::file::sandbox::SandboxConfig {
            allowed_roots: vec![dir.path().to_path_buf()],
            denied_patterns: vec![],
            strict_mode: true,
        });
        let tool = WriteTool::with_sandbox(sandbox);

        let new_path = dir.path().join("new_file.txt");
        assert!(!new_path.exists(), "File should not exist before write");

        let input = serde_json::json!({
            "file_path": new_path.to_string_lossy(),
            "content": "hello world"
        });
        let result = tool.execute(input).await;
        assert!(
            result.is_ok(),
            "Write should succeed for new file: {:?}",
            result
        );

        let content = std::fs::read_to_string(&new_path).unwrap();
        assert_eq!(content, "hello world");
    }

    #[tokio::test]
    async fn test_write_tool_creates_nested_new_file_in_sandbox() {
        let dir = tempfile::TempDir::new().unwrap();
        // Pre-create the nested directory
        std::fs::create_dir_all(dir.path().join("src")).unwrap();

        let sandbox = PathSandbox::with_config(crate::file::sandbox::SandboxConfig {
            allowed_roots: vec![dir.path().to_path_buf()],
            denied_patterns: vec![],
            strict_mode: true,
        });
        let tool = WriteTool::with_sandbox(sandbox);

        let new_path = dir.path().join("src/lib.rs");
        assert!(!new_path.exists());

        let input = serde_json::json!({
            "file_path": new_path.to_string_lossy(),
            "content": "pub fn add(a: i32, b: i32) -> i32 { a + b }"
        });
        let result = tool.execute(input).await;
        assert!(
            result.is_ok(),
            "Write should succeed for nested new file: {:?}",
            result
        );

        let content = std::fs::read_to_string(&new_path).unwrap();
        assert!(content.contains("pub fn add"));
    }

    #[tokio::test]
    async fn test_write_tool_rejects_path_outside_sandbox() {
        let dir = tempfile::TempDir::new().unwrap();
        let sandbox = PathSandbox::with_config(crate::file::sandbox::SandboxConfig {
            allowed_roots: vec![dir.path().to_path_buf()],
            denied_patterns: vec![],
            strict_mode: true,
        });
        let tool = WriteTool::with_sandbox(sandbox);

        let outside = std::env::temp_dir().join("outside_sandbox_test.txt");
        let input = serde_json::json!({
            "file_path": outside.to_string_lossy(),
            "content": "should not be written"
        });
        let result = tool.execute(input).await;
        assert!(result.is_err(), "Write should reject path outside sandbox");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("sandbox") || err.contains("allowed"),
            "Error should mention sandbox or allowed roots: {err}"
        );
    }

    #[tokio::test]
    async fn test_write_tool_overwrites_existing_file_in_sandbox() {
        let dir = tempfile::TempDir::new().unwrap();
        let existing = dir.path().join("existing.txt");
        std::fs::write(&existing, "old content").unwrap();

        let sandbox = PathSandbox::with_config(crate::file::sandbox::SandboxConfig {
            allowed_roots: vec![dir.path().to_path_buf()],
            denied_patterns: vec![],
            strict_mode: true,
        });
        let tool = WriteTool::with_sandbox(sandbox);

        let input = serde_json::json!({
            "file_path": existing.to_string_lossy(),
            "content": "new content"
        });
        let result = tool.execute(input).await;
        assert!(
            result.is_ok(),
            "Write should succeed overwriting existing file"
        );

        let content = std::fs::read_to_string(&existing).unwrap();
        assert_eq!(content, "new content");
    }
}
