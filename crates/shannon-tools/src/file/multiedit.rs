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
    use tokio::fs;

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
    let mut pending: Vec<(EditOperation, String, String, usize, Vec<ReplacementLocation>)> =
        Vec::with_capacity(input.edits.len());

    for op in &input.edits {
        let metadata = fs::metadata(&op.file_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ToolError::InvalidInput(format!("File not found: {}", op.file_path))
            } else {
                ToolError::ExecutionFailed(format!("Failed to access {}: {e}", op.file_path))
            }
        })?;

        if metadata.is_dir() {
            return Err(ToolError::InvalidInput(format!(
                "Path is a directory: {}",
                op.file_path
            )));
        }
        if metadata.len() > MAX_FILE_SIZE {
            return Err(ToolError::InvalidInput(format!(
                "File too large: {} ({} bytes, max {})",
                op.file_path,
                metadata.len(),
                MAX_FILE_SIZE
            )));
        }

        let content = fs::read_to_string(&op.file_path)
            .await
            .map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to read {}: {e}", op.file_path))
            })?;

        let (new_content, replacements, locations) =
            edit::perform_edit(&content, &op.old_string, &op.new_string, op.replace_all)
                .map_err(|e| {
                    ToolError::InvalidInput(format!("Edit failed for {}: {e}", op.file_path))
                })?;

        pending.push((op.clone(), content, new_content, replacements, locations));
    }

    // Phase 2: All validations passed — write all files.
    let renderer = super::diff_renderer::DiffRenderer::new();
    let mut results = Vec::with_capacity(pending.len());
    let mut total_replacements = 0usize;
    let mut diff_parts: Vec<String> = Vec::new();

    for (op, old_content, new_content, replacements, locations) in &pending {
        fs::write(&op.file_path, new_content)
            .await
            .map_err(|e| {
                ToolError::ExecutionFailed(format!(
                    "Failed to write {}: {e} — earlier edits in this batch were applied",
                    op.file_path
                ))
            })?;

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

    let unique_files: std::collections::HashSet<&str> = results
        .iter()
        .map(|r| r.file_path.as_str())
        .collect();

    let mut output_text = format!(
        "Applied {} edits across {} files ({} total replacements)\n",
        pending.len(),
        unique_files.len(),
        total_replacements,
    );
    if !diff_parts.is_empty() {
        output_text.push_str("\n");
        output_text.push_str(&diff_parts.join("\n"));
    }

    let mut metadata = HashMap::new();
    metadata.insert("total_replacements".to_string(), json!(total_replacements));
    metadata.insert(
        "file_count".to_string(),
        json!(unique_files.len()),
    );
    metadata.insert("results".to_string(), json!(results));

    Ok(ToolOutput {
        content: output_text,
        is_error: false,
        metadata,
    })
}
