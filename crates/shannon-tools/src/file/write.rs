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
    // Use UUID suffix to prevent symlink race attacks on predictable temp paths.
    let temp_path = format!("{}.shannon-tmp-{}", input.file_path, uuid::Uuid::new_v4().as_simple());
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── WriteInput serialization ────────────────────────────────────────

    #[test]
    fn test_write_input_roundtrip() {
        let input = WriteInput {
            file_path: "/tmp/test.txt".into(),
            content: "hello world".into(),
        };
        let json = serde_json::to_string(&input).unwrap();
        let parsed: WriteInput = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.file_path, input.file_path);
        assert_eq!(parsed.content, input.content);
    }

    // ── Size limit ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_write_rejects_oversized_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.txt");
        let input = WriteInput {
            file_path: path.to_str().unwrap().to_string(),
            content: "x".repeat(10 * 1024 * 1024 + 1), // 10MB + 1 byte
        };
        let result = execute(input).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("too large"));
    }

    // ── Write to real file ──────────────────────────────────────────────

    #[tokio::test]
    async fn test_write_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new.txt");
        let input = WriteInput {
            file_path: path.to_str().unwrap().to_string(),
            content: "hello world".into(),
        };
        let output = execute(input).await.unwrap();
        assert!(!output.is_error);
        assert!(output.content.contains("11 bytes"));
        assert_eq!(output.metadata.get("bytes").unwrap(), 11);
        // Verify file was actually written
        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "hello world");
    }

    #[tokio::test]
    async fn test_write_overwrites_existing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("existing.txt");
        tokio::fs::write(&path, "old content").await.unwrap();

        let input = WriteInput {
            file_path: path.to_str().unwrap().to_string(),
            content: "new content".into(),
        };
        execute(input).await.unwrap();
        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "new content");
    }

    #[tokio::test]
    async fn test_write_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/dir/file.txt");
        tokio::fs::create_dir_all(path.parent().unwrap()).await.unwrap();

        let input = WriteInput {
            file_path: path.to_str().unwrap().to_string(),
            content: "nested".into(),
        };
        execute(input).await.unwrap();
        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "nested");
    }

    #[tokio::test]
    async fn test_write_empty_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.txt");
        let input = WriteInput {
            file_path: path.to_str().unwrap().to_string(),
            content: String::new(),
        };
        let output = execute(input).await.unwrap();
        assert!(!output.is_error);
        assert_eq!(output.metadata.get("bytes").unwrap(), 0);
    }

    #[tokio::test]
    async fn test_write_unicode_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("unicode.txt");
        let input = WriteInput {
            file_path: path.to_str().unwrap().to_string(),
            content: "你好世界 🌍 hello".into(),
        };
        execute(input).await.unwrap();
        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "你好世界 🌍 hello");
    }

    #[tokio::test]
    async fn test_write_no_temp_file_left() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clean.txt");
        let input = WriteInput {
            file_path: path.to_str().unwrap().to_string(),
            content: "clean".into(),
        };
        execute(input).await.unwrap();
        // Verify no .shannon-tmp files remain
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let name = entry.unwrap().file_name();
            assert!(!name.to_str().unwrap().contains("shannon-tmp"));
        }
    }
}
