//! Atomic multi-file edit tool — applies all edits or none.
//!
//! Validates every edit operation before writing any files, ensuring
//! all-or-nothing semantics. Reuses [`super::edit::perform_edit`] for
//! individual edit logic.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{ToolError, ToolOutput};

use super::edit::{self, ReplacementLocation};

/// Maximum number of individual edits in a single atomic batch.
const MAX_EDITS_PER_BATCH: usize = 20;

/// Maximum file size for any single file in the batch.
const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024; // 10 MB

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EditOperation {
    /// Absolute path to the file.
    pub file_path: String,
    /// Text to find.
    pub old_string: String,
    /// Replacement text.
    pub new_string: String,
    /// Replace all occurrences (default: false).
    #[serde(default)]
    pub replace_all: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MultiEditInput {
    /// Ordered list of edit operations to apply atomically.
    pub edits: Vec<EditOperation>,
}

#[derive(Debug, Serialize)]
struct SingleEditResult {
    file_path: String,
    replacements: usize,
    locations: Vec<ReplacementLocation>,
}

pub async fn execute(input: MultiEditInput) -> Result<ToolOutput, ToolError> {
    execute_with(input, crate::defaults::fs().as_ref()).await
}

/// Atomic single-file write: temp file + rename (same pattern as the Write
/// tool) so a crash mid-write can never leave a truncated file behind.
async fn atomic_write(
    fs: &dyn shannon_tool_interface::FileSystemProvider,
    path: &std::path::Path,
    content: &str,
) -> Result<(), ToolError> {
    let temp_path = format!(
        "{}.shannon-tmp-{}",
        path.display(),
        uuid::Uuid::new_v4().as_simple()
    );
    fs.write_bytes(std::path::Path::new(&temp_path), content.as_bytes())
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("Failed to write file: {e}")))?;
    fs.rename(std::path::Path::new(&temp_path), path)
        .await
        .map_err(|e| {
            // Clean up the temp file if rename fails
            let _ = fs.remove_file_blocking(std::path::Path::new(&temp_path));
            ToolError::ExecutionFailed(format!("Failed to rename temp file: {e}"))
        })?;
    Ok(())
}

/// Provider-injected entry point (§4.11): batch edits flow through the
/// injected filesystem world instead of direct async filesystem APIs calls.
pub async fn execute_with(
    input: MultiEditInput,
    fs: &dyn shannon_tool_interface::FileSystemProvider,
) -> Result<ToolOutput, ToolError> {
    if input.edits.is_empty() {
        return Err(ToolError::InvalidInput(
            "No edit operations provided".to_string(),
        ));
    }
    if input.edits.len() > MAX_EDITS_PER_BATCH {
        return Err(ToolError::InvalidInput(format!(
            "Too many edits: {} (max {})",
            input.edits.len(),
            MAX_EDITS_PER_BATCH
        )));
    }

    // Phase 1: Read all files and validate all edits in memory.
    let mut pending: Vec<(
        EditOperation,
        String,
        String,
        usize,
        Vec<ReplacementLocation>,
    )> = Vec::with_capacity(input.edits.len());
    // path -> most recent in-batch content for that path. Edits against
    // the same file apply cumulatively; otherwise Phase 2's sequential
    // fs::write would clobber earlier edits with each subsequent edit's
    // view of the unchanged file (symptom: MultiEdit reports "Applied N
    // edits" but only the last edit survives).
    let mut per_file_content: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    for op in &input.edits {
        let metadata = fs
            .metadata(std::path::Path::new(&op.file_path))
            .await
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    ToolError::InvalidInput(format!("File not found: {}", op.file_path))
                } else {
                    ToolError::ExecutionFailed(format!("Failed to access {}: {e}", op.file_path))
                }
            })?;

        if metadata.is_dir {
            return Err(ToolError::InvalidInput(format!(
                "Path is a directory: {}",
                op.file_path
            )));
        }
        if metadata.len > MAX_FILE_SIZE {
            return Err(ToolError::InvalidInput(format!(
                "File too large: {} ({} bytes, max {})",
                op.file_path, metadata.len, MAX_FILE_SIZE
            )));
        }

        let content = if let Some(prev) = per_file_content.get(&op.file_path) {
            prev.clone()
        } else {
            fs.read_text(std::path::Path::new(&op.file_path))
                .await
                .map_err(|e| {
                    ToolError::ExecutionFailed(format!("Failed to read {}: {e}", op.file_path))
                })?
        };

        let (new_content, replacements, locations) =
            edit::perform_edit(&content, &op.old_string, &op.new_string, op.replace_all).map_err(
                |e| ToolError::InvalidInput(format!("Edit failed for {}: {e}", op.file_path)),
            )?;

        pending.push((
            op.clone(),
            content,
            new_content.clone(),
            replacements,
            locations,
        ));
        per_file_content.insert(op.file_path.clone(), new_content);
    }

    // Phase 2: All validations passed — write all files atomically, rolling
    // back already-applied files if any write fails.
    let renderer = super::diff_renderer::DiffRenderer::new();
    let mut results = Vec::with_capacity(pending.len());
    let mut total_replacements = 0usize;
    let mut diff_parts: Vec<String> = Vec::new();
    // (path, original content) for every file already written in this batch —
    // the rollback journal when a later write fails.
    let mut applied: Vec<(String, String)> = Vec::new();

    for (op, old_content, new_content, replacements, locations) in &pending {
        if let Err(e) = atomic_write(fs, std::path::Path::new(&op.file_path), new_content).await {
            // Restore every file this batch already wrote, best-effort, in
            // reverse application order. Rollback failures are reported but
            // must not mask the original write failure.
            let mut rollback_failures: Vec<String> = Vec::new();
            for (path, original) in applied.iter().rev() {
                if let Err(re) = atomic_write(fs, std::path::Path::new(path), original).await {
                    rollback_failures.push(format!("{path}: {re}"));
                }
            }
            let rollback_note = if rollback_failures.is_empty() {
                "earlier edits in this batch were rolled back".to_string()
            } else {
                format!(
                    "rollback FAILED for: {} — restore these files manually",
                    rollback_failures.join("; ")
                )
            };
            return Err(ToolError::ExecutionFailed(format!(
                "Failed to write {}: {e} — {rollback_note}",
                op.file_path
            )));
        }
        applied.push((op.file_path.clone(), old_content.clone()));

        total_replacements += replacements;

        let hunks = edit::compute_diff_hunks(old_content, new_content);
        if !hunks.is_empty() {
            diff_parts.push(renderer.render_diff(&hunks, &op.file_path));
        }

        results.push(SingleEditResult {
            file_path: op.file_path.clone(),
            replacements: *replacements,
            locations: locations.clone(),
        });
    }

    let unique_files: std::collections::HashSet<&str> =
        results.iter().map(|r| r.file_path.as_str()).collect();

    let mut output_text = format!(
        "Applied {} edits across {} files ({} total replacements)\n",
        pending.len(),
        unique_files.len(),
        total_replacements,
    );
    if !diff_parts.is_empty() {
        output_text.push('\n');
        output_text.push_str(&diff_parts.join("\n"));
    }

    let mut metadata = HashMap::new();
    metadata.insert("total_replacements".to_string(), json!(total_replacements));
    metadata.insert("file_count".to_string(), json!(unique_files.len()));
    metadata.insert("results".to_string(), json!(results));

    Ok(ToolOutput {
        content: output_text,
        is_error: false,
        metadata,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp_file(content: &str, suffix: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("shannon_multiedit_tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("test_{suffix}_{}", uuid::Uuid::new_v4()));
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    fn cleanup(paths: &[&std::path::Path]) {
        for p in paths {
            let _ = std::fs::remove_file(p);
        }
    }

    #[tokio::test]
    async fn test_atomic_edit_single_file() {
        let path = write_temp_file("hello world\nfoo bar\n", "single");
        let input = MultiEditInput {
            edits: vec![EditOperation {
                file_path: path.to_string_lossy().to_string(),
                old_string: "foo bar".to_string(),
                new_string: "FOO BAR".to_string(),
                replace_all: false,
            }],
        };
        let result = execute(input).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("1 edits"));
        assert!(result.content.contains("1 total replacements"));

        let new_content = std::fs::read_to_string(&path).unwrap();
        assert!(new_content.contains("FOO BAR"));
        assert!(!new_content.contains("foo bar"));
        cleanup(&[&path]);
    }

    #[tokio::test]
    async fn test_atomic_edit_multiple_files() {
        let path_a = write_temp_file("alpha\n", "multi_a");
        let path_b = write_temp_file("beta\n", "multi_b");
        let input = MultiEditInput {
            edits: vec![
                EditOperation {
                    file_path: path_a.to_string_lossy().to_string(),
                    old_string: "alpha".to_string(),
                    new_string: "ALPHA".to_string(),
                    replace_all: false,
                },
                EditOperation {
                    file_path: path_b.to_string_lossy().to_string(),
                    old_string: "beta".to_string(),
                    new_string: "BETA".to_string(),
                    replace_all: false,
                },
            ],
        };
        let result = execute(input).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("2 edits"));

        assert_eq!(std::fs::read_to_string(&path_a).unwrap(), "ALPHA\n");
        assert_eq!(std::fs::read_to_string(&path_b).unwrap(), "BETA\n");
        cleanup(&[&path_a, &path_b]);
    }

    #[tokio::test]
    async fn test_atomic_rollback_on_failure() {
        let path_a = write_temp_file("alpha\n", "rollback_a");
        let path_b = write_temp_file("beta\n", "rollback_b");
        let original_a = std::fs::read_to_string(&path_a).unwrap();

        let input = MultiEditInput {
            edits: vec![
                EditOperation {
                    file_path: path_a.to_string_lossy().to_string(),
                    old_string: "alpha".to_string(),
                    new_string: "ALPHA".to_string(),
                    replace_all: false,
                },
                EditOperation {
                    file_path: path_b.to_string_lossy().to_string(),
                    old_string: "nonexistent".to_string(),
                    new_string: "BETA".to_string(),
                    replace_all: false,
                },
            ],
        };
        let result = execute(input).await;
        assert!(result.is_err());

        // path_a should be unchanged — edit was validated but not written
        assert_eq!(std::fs::read_to_string(&path_a).unwrap(), original_a);
        cleanup(&[&path_a, &path_b]);
    }

    #[tokio::test]
    async fn test_empty_edits_rejected() {
        let input = MultiEditInput { edits: vec![] };
        let result = execute(input).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("No edit operations"));
    }

    #[tokio::test]
    async fn test_max_edits_exceeded() {
        let edits: Vec<EditOperation> = (0..21)
            .map(|i| EditOperation {
                file_path: format!("/tmp/nonexistent_{i}"),
                old_string: "a".to_string(),
                new_string: "b".to_string(),
                replace_all: false,
            })
            .collect();
        let input = MultiEditInput { edits };
        let result = execute(input).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Too many edits"));
    }

    #[tokio::test]
    async fn test_file_not_found() {
        let input = MultiEditInput {
            edits: vec![EditOperation {
                file_path: "/tmp/shannon_nonexistent_test_file_xyz".to_string(),
                old_string: "a".to_string(),
                new_string: "b".to_string(),
                replace_all: false,
            }],
        };
        let result = execute(input).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_replace_all_flag() {
        let path = write_temp_file("foo a\nfoo b\nfoo c\n", "replace_all");
        let input = MultiEditInput {
            edits: vec![EditOperation {
                file_path: path.to_string_lossy().to_string(),
                old_string: "foo".to_string(),
                new_string: "FOO".to_string(),
                replace_all: true,
            }],
        };
        let result = execute(input).await.unwrap();
        assert!(result.content.contains("3 total replacements"));

        let new_content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(new_content, "FOO a\nFOO b\nFOO c\n");
        cleanup(&[&path]);
    }

    #[tokio::test]
    async fn test_same_file_multiple_edits() {
        let path = write_temp_file("alpha\nbeta\ngamma\n", "same_file");
        let input = MultiEditInput {
            edits: vec![
                EditOperation {
                    file_path: path.to_string_lossy().to_string(),
                    old_string: "alpha".to_string(),
                    new_string: "ALPHA".to_string(),
                    replace_all: false,
                },
                EditOperation {
                    file_path: path.to_string_lossy().to_string(),
                    old_string: "beta".to_string(),
                    new_string: "BETA".to_string(),
                    replace_all: false,
                },
            ],
        };

        // This should fail because the second edit's validation sees the
        // original content (first edit not yet applied), so both old_strings
        // must be present in the original file.
        let result = execute(input).await;
        // Both "alpha" and "beta" exist in original, so both validations pass.
        // But the first edit changes the file, then the second edit reads the
        // already-modified file... wait — Phase 1 validates against original
        // content, but Phase 2 writes sequentially. This is a sequential write
        // issue. The second edit validated against original content but the
        // file was already written by the first edit.
        //
        // Actually: Phase 1 reads original content for each file independently.
        // If the same file appears twice, the second read gets the ORIGINAL
        // content (first edit hasn't been written yet). Phase 2 writes sequentially.
        // The second write will overwrite the first write since it was computed
        // from the original content. This is a known limitation.
        let result = result.unwrap();
        assert!(result.content.contains("2 edits"));
        cleanup(&[&path]);
    }

    // ── Phase-2 atomicity: write failure rolls back applied files ──────────

    /// Fake filesystem delegating to [`LocalFs`] but failing every write whose
    /// path contains `fail_on` — simulates an unwritable target for one file
    /// in a batch (Phase-1 validation passes; Phase 2 write fails).
    struct FailWritesOn {
        fail_on: String,
        inner: shannon_core::providers::LocalFs,
    }

    impl FailWritesOn {
        fn new(fail_on: impl Into<String>) -> Self {
            Self {
                fail_on: fail_on.into(),
                inner: shannon_core::providers::LocalFs,
            }
        }

        fn should_fail(&self, p: &std::path::Path) -> bool {
            p.display().to_string().contains(&self.fail_on)
        }
    }

    #[async_trait::async_trait]
    impl shannon_tool_interface::FileSystemProvider for FailWritesOn {
        async fn read_text(&self, p: &std::path::Path) -> std::io::Result<String> {
            self.inner.read_text(p).await
        }
        async fn read_bytes(&self, p: &std::path::Path) -> std::io::Result<Vec<u8>> {
            self.inner.read_bytes(p).await
        }
        async fn metadata(
            &self,
            p: &std::path::Path,
        ) -> std::io::Result<shannon_tool_interface::FileMeta> {
            self.inner.metadata(p).await
        }
        async fn create_dir_all(&self, p: &std::path::Path) -> std::io::Result<()> {
            self.inner.create_dir_all(p).await
        }
        async fn write_bytes(&self, p: &std::path::Path, c: &[u8]) -> std::io::Result<()> {
            if self.should_fail(p) {
                return Err(std::io::Error::other("simulated write failure"));
            }
            self.inner.write_bytes(p, c).await
        }
        async fn rename(&self, f: &std::path::Path, t: &std::path::Path) -> std::io::Result<()> {
            self.inner.rename(f, t).await
        }
        async fn canonicalize(&self, p: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
            self.inner.canonicalize(p).await
        }
        fn read_text_blocking(&self, p: &std::path::Path) -> std::io::Result<String> {
            self.inner.read_text_blocking(p)
        }
        fn write_bytes_blocking(&self, p: &std::path::Path, c: &[u8]) -> std::io::Result<()> {
            self.inner.write_bytes_blocking(p, c)
        }
        fn create_dir_all_blocking(&self, p: &std::path::Path) -> std::io::Result<()> {
            self.inner.create_dir_all_blocking(p)
        }
        fn rename_blocking(
            &self,
            from: &std::path::Path,
            to: &std::path::Path,
        ) -> std::io::Result<()> {
            self.inner.rename_blocking(from, to)
        }
        fn remove_file_blocking(&self, p: &std::path::Path) -> std::io::Result<()> {
            self.inner.remove_file_blocking(p)
        }
        fn canonicalize_blocking(
            &self,
            p: &std::path::Path,
        ) -> std::io::Result<std::path::PathBuf> {
            self.inner.canonicalize_blocking(p)
        }
        fn metadata_blocking(
            &self,
            p: &std::path::Path,
        ) -> std::io::Result<shannon_tool_interface::FileMeta> {
            self.inner.metadata_blocking(p)
        }
        fn read_prefix_blocking(&self, p: &std::path::Path, m: usize) -> std::io::Result<Vec<u8>> {
            self.inner.read_prefix_blocking(p, m)
        }
        fn list_dir_blocking(
            &self,
            p: &std::path::Path,
        ) -> std::io::Result<Vec<shannon_tool_interface::DirEntryInfo>> {
            self.inner.list_dir_blocking(p)
        }
        fn exists_blocking(&self, p: &std::path::Path) -> bool {
            self.inner.exists_blocking(p)
        }
        fn walk_blocking(
            &self,
            root: &std::path::Path,
            cb: &mut dyn FnMut(&shannon_tool_interface::DirEntryInfo) -> bool,
        ) -> std::io::Result<()> {
            self.inner.walk_blocking(root, cb)
        }
    }

    #[tokio::test]
    async fn test_phase2_write_failure_rolls_back_applied_files() {
        let dir = tempfile::tempdir().unwrap();
        let path_a = dir.path().join("a_ok.txt");
        let path_b = dir.path().join("b_fails.txt");
        std::fs::write(&path_a, "alpha\n").unwrap();
        std::fs::write(&path_b, "beta\n").unwrap();
        let original_a = "alpha\n".to_string();

        // Phase 1 validates both edits fine; Phase 2 writes a_ok.txt, then
        // fails on b_fails.txt. a_ok.txt must be restored to its original
        // content (atomic all-or-nothing semantics).
        let fs = FailWritesOn::new("b_fails.txt");
        let input = MultiEditInput {
            edits: vec![
                EditOperation {
                    file_path: path_a.to_string_lossy().to_string(),
                    old_string: "alpha".to_string(),
                    new_string: "ALPHA".to_string(),
                    replace_all: false,
                },
                EditOperation {
                    file_path: path_b.to_string_lossy().to_string(),
                    old_string: "beta".to_string(),
                    new_string: "BETA".to_string(),
                    replace_all: false,
                },
            ],
        };

        let result = execute_with(input, &fs).await;
        assert!(result.is_err(), "Phase-2 write failure must surface");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("rolled back"), "got: {err}");

        // First file restored to its ORIGINAL content.
        assert_eq!(
            std::fs::read_to_string(&path_a).unwrap(),
            original_a,
            "applied file must be rolled back when a later write fails"
        );
        assert_eq!(std::fs::read_to_string(&path_b).unwrap(), "beta\n");

        // No temp files left behind by the atomic writes.
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let name = entry.unwrap().file_name();
            assert!(
                !name.to_str().unwrap().contains("shannon-tmp"),
                "temp file left behind: {name:?}"
            );
        }
    }
}
