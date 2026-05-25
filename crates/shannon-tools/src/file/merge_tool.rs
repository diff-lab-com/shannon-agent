//! MergeResolve tool — resolves conflict markers in files.
//!
//! After a three-way merge produces conflict markers (`<<<<<<<`, `=======`, `>>>>>>>`),
//! this tool lets the LLM or user resolve each conflict by choosing "ours" or "theirs"
//! for each conflicted region.

use crate::file::merge::{parse_conflict_markers, resolve_conflicts};
use crate::{ToolError, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

// ---------------------------------------------------------------------------
// Input types
// ---------------------------------------------------------------------------

/// Input for resolving merge conflicts.
#[derive(Debug, Clone, Deserialize)]
pub struct MergeResolveInput {
    /// Path to the file containing conflict markers.
    pub file_path: String,
    /// Resolution strategy for each conflict in order.
    /// Each entry must be `"ours"` or `"theirs"`.
    pub resolutions: Vec<String>,
}

// ---------------------------------------------------------------------------
// MergeResolveTool
// ---------------------------------------------------------------------------

/// Tool for resolving merge conflicts in files.
///
/// Reads a file with conflict markers, applies the specified resolutions,
/// and writes the resolved content back.
pub struct MergeResolveTool {
    description: String,
}

impl Default for MergeResolveTool {
    fn default() -> Self {
        Self::new()
    }
}

impl MergeResolveTool {
    pub fn new() -> Self {
        Self {
            description: "Resolve merge conflict markers in a file by choosing 'ours' or 'theirs' for each conflict".to_string(),
        }
    }
}

#[async_trait]
impl crate::Tool for MergeResolveTool {
    fn name(&self) -> &str {
        "MergeResolve"
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
                    "description": "Path to the file containing conflict markers"
                },
                "resolutions": {
                    "type": "array",
                    "description": "Resolution for each conflict in order: 'ours' or 'theirs'",
                    "items": {
                        "type": "string",
                        "enum": ["ours", "theirs"]
                    }
                }
            },
            "required": ["file_path", "resolutions"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        use tokio::fs;

        let resolve_input: MergeResolveInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid merge-resolve input: {e}")))?;

        // Validate resolutions
        for (i, r) in resolve_input.resolutions.iter().enumerate() {
            if r != "ours" && r != "theirs" {
                return Err(ToolError::InvalidInput(format!(
                    "Invalid resolution at index {i}: '{r}' — must be 'ours' or 'theirs'"
                )));
            }
        }

        // Check file exists
        let metadata = fs::metadata(&resolve_input.file_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ToolError::InvalidInput(format!("File not found: {}", resolve_input.file_path))
            } else {
                ToolError::ExecutionFailed(format!("Failed to access file: {e}"))
            }
        })?;

        if metadata.is_dir() {
            return Err(ToolError::InvalidInput(format!(
                "Path is a directory, not a file: {}",
                resolve_input.file_path
            )));
        }

        // Read the file
        let content = fs::read_to_string(&resolve_input.file_path)
            .await
            .map_err(|e| {
                ToolError::ExecutionFailed(format!(
                    "Failed to read file '{}': {}",
                    resolve_input.file_path, e
                ))
            })?;

        // Parse conflicts for reporting
        let conflicts = parse_conflict_markers(&content);
        if conflicts.is_empty() {
            return Ok(ToolOutput::success(
                "No conflict markers found in file. Nothing to resolve.".to_string(),
            ));
        }

        if conflicts.len() != resolve_input.resolutions.len() {
            return Err(ToolError::InvalidInput(format!(
                "Found {} conflict(s) but {} resolution(s) provided. Each conflict needs exactly one resolution ('ours' or 'theirs').",
                conflicts.len(),
                resolve_input.resolutions.len()
            )));
        }

        // Resolve conflicts
        let resolved = resolve_conflicts(&content, &resolve_input.resolutions)
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to resolve conflicts: {e}")))?;

        // Write the resolved file atomically
        let temp_path = format!(
            "{}.shannon-merge-{}",
            resolve_input.file_path,
            uuid::Uuid::new_v4().as_simple()
        );
        fs::write(&temp_path, &resolved).await.map_err(|e| {
            ToolError::ExecutionFailed(format!(
                "Failed to write file '{}': {}",
                resolve_input.file_path, e
            ))
        })?;
        fs::rename(&temp_path, &resolve_input.file_path)
            .await
            .map_err(|e| {
                let _ = std::fs::remove_file(&temp_path);
                ToolError::ExecutionFailed(format!(
                    "Failed to rename temp file for '{}': {}",
                    resolve_input.file_path, e
                ))
            })?;

        // Build summary
        let resolution_summary: Vec<String> = resolve_input
            .resolutions
            .iter()
            .enumerate()
            .map(|(i, r)| format!("  Conflict {}: {}", i + 1, r))
            .collect();

        let message = format!(
            "Resolved {} conflict(s) in {}:\n{}",
            conflicts.len(),
            resolve_input.file_path,
            resolution_summary.join("\n")
        );

        Ok(ToolOutput {
            content: message,
            is_error: false,
            metadata: {
                let mut map = std::collections::HashMap::new();
                map.insert("file_path".to_string(), json!(resolve_input.file_path));
                map.insert("conflicts_resolved".to_string(), json!(conflicts.len()));
                map.insert("resolutions".to_string(), json!(resolve_input.resolutions));
                map
            },
        })
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Tool;
    use std::io::Write;

    /// Helper: write a temp file and return its path.
    fn write_temp_file(content: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("shannon_merge_resolve_tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("test_{}.txt", uuid::Uuid::new_v4()));
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    fn cleanup_temp_file(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.parent().unwrap());
    }

    // ── Tool trait methods ─────────────────────────────────────────────

    #[test]
    fn test_tool_name() {
        let tool = MergeResolveTool::new();
        assert_eq!(tool.name(), "MergeResolve");
    }

    #[test]
    fn test_tool_description() {
        let tool = MergeResolveTool::new();
        assert!(tool.description().contains("conflict"));
    }

    #[test]
    fn test_tool_schema_has_required_fields() {
        let tool = MergeResolveTool::new();
        let schema = tool.input_schema();
        assert!(schema["properties"]["file_path"].is_object());
        assert!(schema["properties"]["resolutions"].is_object());
        assert!(schema["required"].is_array());
    }

    #[test]
    fn test_tool_not_read_only() {
        let tool = MergeResolveTool::new();
        assert!(!tool.is_read_only());
        assert!(!tool.is_concurrency_safe());
    }

    #[test]
    fn test_tool_default() {
        let tool = MergeResolveTool::default();
        assert_eq!(tool.name(), "MergeResolve");
    }

    // ── Async integration tests ────────────────────────────────────────

    #[tokio::test]
    async fn test_resolve_ours_async() {
        let content = "\
line1
<<<<<<< ours
OURS
=======
THEIRS
>>>>>>> theirs
line3
";
        let path = write_temp_file(content);
        let tool = MergeResolveTool::new();
        let input = serde_json::json!({
            "file_path": path.to_string_lossy().to_string(),
            "resolutions": ["ours"]
        });
        let result = tool.execute(input).await;
        assert!(result.is_ok(), "Expected success, got {result:?}");
        let output = result.unwrap();
        assert!(!output.is_error);
        assert!(output.content.contains("Resolved 1 conflict"));

        // Verify file content
        let resolved = tokio::fs::read_to_string(&path).await.unwrap();
        assert!(resolved.contains("OURS"));
        assert!(!resolved.contains("THEIRS"));
        assert!(!resolved.contains("<<<<<<<"));
        cleanup_temp_file(&path);
    }

    #[tokio::test]
    async fn test_resolve_theirs_async() {
        let content = "\
line1
<<<<<<< ours
OURS
=======
THEIRS
>>>>>>> theirs
line3
";
        let path = write_temp_file(content);
        let tool = MergeResolveTool::new();
        let input = serde_json::json!({
            "file_path": path.to_string_lossy().to_string(),
            "resolutions": ["theirs"]
        });
        let result = tool.execute(input).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(!output.is_error);

        let resolved = tokio::fs::read_to_string(&path).await.unwrap();
        assert!(resolved.contains("THEIRS"));
        assert!(!resolved.contains("OURS"));
        assert!(!resolved.contains("<<<<<<<"));
        cleanup_temp_file(&path);
    }

    #[tokio::test]
    async fn test_resolve_no_conflicts() {
        let content = "clean file\nno conflicts\n";
        let path = write_temp_file(content);
        let tool = MergeResolveTool::new();
        let input = serde_json::json!({
            "file_path": path.to_string_lossy().to_string(),
            "resolutions": []
        });
        let result = tool.execute(input).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.content.contains("No conflict markers"));
        cleanup_temp_file(&path);
    }

    #[tokio::test]
    async fn test_resolve_wrong_count() {
        let content = "\
<<<<<<< ours
a
=======
b
>>>>>>> theirs
";
        let path = write_temp_file(content);
        let tool = MergeResolveTool::new();
        let input = serde_json::json!({
            "file_path": path.to_string_lossy().to_string(),
            "resolutions": ["ours", "theirs"]
        });
        let result = tool.execute(input).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("1 conflict(s)") && err.contains("2 resolution(s)"));
        cleanup_temp_file(&path);
    }

    #[tokio::test]
    async fn test_resolve_file_not_found() {
        let tool = MergeResolveTool::new();
        let input = serde_json::json!({
            "file_path": "/nonexistent/file.txt",
            "resolutions": ["ours"]
        });
        let result = tool.execute(input).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("File not found"));
    }

    #[tokio::test]
    async fn test_resolve_invalid_resolution_value() {
        let tool = MergeResolveTool::new();
        let input = serde_json::json!({
            "file_path": "/tmp/test.txt",
            "resolutions": ["invalid"]
        });
        let result = tool.execute(input).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid resolution")
        );
    }

    #[tokio::test]
    async fn test_resolve_metadata() {
        let content = "\
<<<<<<< ours
a
=======
b
>>>>>>> theirs
";
        let path = write_temp_file(content);
        let tool = MergeResolveTool::new();
        let input = serde_json::json!({
            "file_path": path.to_string_lossy().to_string(),
            "resolutions": ["theirs"]
        });
        let result = tool.execute(input).await.unwrap();
        assert_eq!(result.metadata["conflicts_resolved"], serde_json::json!(1));
        assert_eq!(
            result.metadata["resolutions"],
            serde_json::json!(["theirs"])
        );
        cleanup_temp_file(&path);
    }

    // ── Send/Sync ──────────────────────────────────────────────────────

    #[test]
    fn test_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MergeResolveTool>();
    }
}
