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
use std::sync::{Arc, Mutex};

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

/// Best-effort pre-modify snapshot for file-level `/undo` (W6-2).
///
/// No-op when `history` is `None` (the default — tools without an attached
/// manager behave identically to pre-W6-2). A missing or unreadable file
/// (e.g. new-file creation) is skipped: undo of creation is a command-layer
/// concern, not the snapshot layer's. Any error is swallowed so snapshotting
/// can never block the actual write/edit — the snapshot is strictly best-effort.
fn snapshot_for_undo(history: &Option<Arc<Mutex<history::FileHistoryManager>>>, file_path: &str) {
    let Some(history) = history else {
        return;
    };
    // Bound memory before reading: the history manager's storage-quota check
    // only runs *after* content is in memory, so pre-filter oversized files
    // here. 10 MB covers typical source files; large data/minified files are
    // poor undo targets anyway. A benign TOCTOU exists between this stat and
    // the read — the cap is a best-effort guard, not a hard guarantee.
    const MAX_SNAPSHOT_BYTES: u64 = 10 * 1024 * 1024;
    let Ok(meta) = std::fs::metadata(file_path) else {
        return;
    };
    if meta.len() > MAX_SNAPSHOT_BYTES {
        return;
    }
    // Only existing, readable text files carry restorable pre-modify state.
    let Ok(old_content) = std::fs::read_to_string(file_path) else {
        return;
    };
    if let Ok(mut mgr) = history.lock() {
        let _ = mgr.record_snapshot(
            Path::new(file_path),
            &old_content,
            history::FileOperation::Edit,
        );
    }
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
    /// Optional shared file-history manager for file-level `/undo` (W6-2).
    /// `None` (default) records no snapshots — identical to pre-W6-2 behavior.
    history: Option<Arc<Mutex<history::FileHistoryManager>>>,
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
            history: None,
        }
    }

    /// Create a WriteTool with a custom sandbox configuration.
    pub fn with_sandbox(sandbox: PathSandbox) -> Self {
        Self {
            description: "Write content to a file, overwriting if it exists".to_string(),
            sandbox,
            history: None,
        }
    }

    /// Attach a shared file-history manager so each write snapshots the
    /// pre-modify content (enables file-level `/undo`). Unset = no snapshots.
    pub fn with_history(mut self, history: Arc<Mutex<history::FileHistoryManager>>) -> Self {
        self.history = Some(history);
        self
    }

    /// Like [`with_history`](Self::with_history) but accepts `None`, letting the
    /// registration layer pass through a disabled config unchanged.
    pub fn with_history_opt(
        mut self,
        history: Option<Arc<Mutex<history::FileHistoryManager>>>,
    ) -> Self {
        self.history = history;
        self
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

        snapshot_for_undo(&self.history, &input.file_path);
        write::execute(input).await
    }
}

/// Edit tool implementation
pub struct EditTool {
    description: String,
    sandbox: PathSandbox,
    /// Optional shared file-history manager for file-level `/undo` (W6-2).
    history: Option<Arc<Mutex<history::FileHistoryManager>>>,
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
            history: None,
        }
    }

    /// Create an EditTool with a custom sandbox configuration.
    pub fn with_sandbox(sandbox: PathSandbox) -> Self {
        Self {
            description: "Perform exact string replacements in files".to_string(),
            sandbox,
            history: None,
        }
    }

    /// Attach a shared file-history manager so each edit snapshots the
    /// pre-modify content (enables file-level `/undo`). Unset = no snapshots.
    pub fn with_history(mut self, history: Arc<Mutex<history::FileHistoryManager>>) -> Self {
        self.history = Some(history);
        self
    }

    /// Like [`with_history`](Self::with_history) but accepts `None`, letting the
    /// registration layer pass through a disabled config unchanged.
    pub fn with_history_opt(
        mut self,
        history: Option<Arc<Mutex<history::FileHistoryManager>>>,
    ) -> Self {
        self.history = history;
        self
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

        snapshot_for_undo(&self.history, &input.file_path);
        edit::execute(input).await
    }
}

/// Atomic multi-file edit tool
pub struct MultiEditTool {
    description: String,
    sandbox: PathSandbox,
    /// Optional shared file-history manager for file-level `/undo` (W6-2).
    history: Option<Arc<Mutex<history::FileHistoryManager>>>,
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
            history: None,
        }
    }

    pub fn with_sandbox(sandbox: PathSandbox) -> Self {
        Self {
            description:
                "Apply multiple file edits atomically — all edits succeed or none are applied"
                    .to_string(),
            sandbox,
            history: None,
        }
    }

    /// Attach a shared file-history manager so each edited file is snapshotted
    /// before the atomic apply (enables file-level `/undo`). Unset = no snapshots.
    pub fn with_history(mut self, history: Arc<Mutex<history::FileHistoryManager>>) -> Self {
        self.history = Some(history);
        self
    }

    /// Like [`with_history`](Self::with_history) but accepts `None`, letting the
    /// registration layer pass through a disabled config unchanged.
    pub fn with_history_opt(
        mut self,
        history: Option<Arc<Mutex<history::FileHistoryManager>>>,
    ) -> Self {
        self.history = history;
        self
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

        // Snapshot each distinct file once before the atomic apply, so the
        // pre-modify state of every touched file is recoverable via `/undo` (W6-2).
        let mut snapshotted: Vec<String> = Vec::new();
        for op in &multi_input.edits {
            if !snapshotted.contains(&op.file_path) {
                snapshotted.push(op.file_path.clone());
                snapshot_for_undo(&self.history, &op.file_path);
            }
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
#[allow(clippy::unwrap_used)]
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
            "Write should succeed for new file: {result:?}"
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
            "Write should succeed for nested new file: {result:?}"
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

    // ── W6-2 file-history snapshot wiring ─────────────────────────────

    /// Build a fresh history manager backed by a throwaway temp dir.
    fn history_manager() -> Arc<Mutex<history::FileHistoryManager>> {
        Arc::new(Mutex::new(history::FileHistoryManager::new_temp().unwrap()))
    }

    /// Sandbox scoped to a single temp dir (matches the existing write tests).
    fn sandbox_for(dir: &Path) -> PathSandbox {
        PathSandbox::with_config(crate::file::sandbox::SandboxConfig {
            allowed_roots: vec![dir.to_path_buf()],
            denied_patterns: vec![],
            strict_mode: true,
        })
    }

    #[tokio::test]
    async fn write_tool_snapshots_pre_modify_content() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "old content").unwrap();

        let history = history_manager();
        let tool = WriteTool::with_sandbox(sandbox_for(dir.path())).with_history(history.clone());

        let input = serde_json::json!({
            "file_path": path.to_string_lossy(),
            "content": "new content"
        });
        tool.execute(input).await.unwrap();

        let mut mgr = history.lock().unwrap();
        let h = mgr.get_history(&path).unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(h.snapshots[0].content, "old content");
    }

    #[tokio::test]
    async fn edit_tool_snapshots_pre_modify_content() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("b.txt");
        std::fs::write(&path, "foo bar baz").unwrap();

        let history = history_manager();
        let tool = EditTool::with_sandbox(sandbox_for(dir.path())).with_history(history.clone());

        let input = serde_json::json!({
            "file_path": path.to_string_lossy(),
            "old_string": "bar",
            "new_string": "qux"
        });
        tool.execute(input).await.unwrap();

        let mut mgr = history.lock().unwrap();
        let h = mgr.get_history(&path).unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(h.snapshots[0].content, "foo bar baz");
    }

    #[tokio::test]
    async fn multiedit_tool_snapshots_each_distinct_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path_a = dir.path().join("a.txt");
        let path_b = dir.path().join("b.txt");
        std::fs::write(&path_a, "alpha-1 alpha-2").unwrap();
        std::fs::write(&path_b, "beta").unwrap();

        let history = history_manager();
        let tool =
            MultiEditTool::with_sandbox(sandbox_for(dir.path())).with_history(history.clone());

        // Two edits target a.txt, one targets b.txt → snapshot each file once.
        let input = serde_json::json!({
            "edits": [
                { "file_path": path_a.to_string_lossy(), "old_string": "alpha-1", "new_string": "ALPHA1" },
                { "file_path": path_a.to_string_lossy(), "old_string": "alpha-2", "new_string": "ALPHA2" },
                { "file_path": path_b.to_string_lossy(), "old_string": "beta", "new_string": "BETA" }
            ]
        });
        tool.execute(input).await.unwrap();

        let mut mgr = history.lock().unwrap();
        let ha = mgr.get_history(&path_a).unwrap();
        assert_eq!(ha.len(), 1, "a.txt snapshotted once despite two edits");
        assert_eq!(ha.snapshots[0].content, "alpha-1 alpha-2");
        let hb = mgr.get_history(&path_b).unwrap();
        assert_eq!(hb.len(), 1);
        assert_eq!(hb.snapshots[0].content, "beta");
    }

    #[tokio::test]
    async fn write_tool_without_history_records_nothing() {
        // No with_history() → manager absent; write still succeeds, no panic.
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("c.txt");
        std::fs::write(&path, "orig").unwrap();

        let tool = WriteTool::with_sandbox(sandbox_for(dir.path()));
        let input = serde_json::json!({
            "file_path": path.to_string_lossy(),
            "content": "changed"
        });
        tool.execute(input).await.unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "changed");
    }

    #[tokio::test]
    async fn write_tool_skips_snapshot_for_oversized_file() {
        // Memory bound: pre-modify content over the 10 MB snapshot cap is not
        // read into memory or recorded.
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("big.txt");
        std::fs::write(&path, "x".repeat(10 * 1024 * 1024 + 1)).unwrap();

        let history = history_manager();
        let tool = WriteTool::with_sandbox(sandbox_for(dir.path())).with_history(history.clone());

        let input = serde_json::json!({
            "file_path": path.to_string_lossy(),
            "content": "shrunk"
        });
        tool.execute(input).await.unwrap();

        let mut mgr = history.lock().unwrap();
        assert!(
            mgr.get_history(&path).is_err(),
            "oversized pre-modify content must not be snapshotted"
        );
    }
}
