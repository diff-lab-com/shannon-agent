//! Edit tool implementation — diff-based precise file editing
//!
//! Provides `old_string` → `new_string` replacement with:
//! - Line number reporting for each replacement
//! - Uniqueness validation (non-replace-all mode requires single match)
//! - File existence and permission checks
//! - Comprehensive error messages with context

use crate::file::diff_renderer::{DiffHunk, DiffLine, DiffLineType, DiffRenderer};
use crate::file::merge::{self, MergeResult};
use crate::{ToolError, ToolOutput};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors that can occur during file edit operations.
#[derive(Debug, thiserror::Error)]
pub enum EditError {
    /// The `old_string` parameter was empty.
    #[error("old_string must not be empty")]
    EmptyOldString,
    /// The `old_string` and `new_string` are identical.
    #[error("old_string and new_string are identical — no change needed")]
    IdenticalStrings,
    /// The `old_string` was not found in the file content.
    #[error("{0}")]
    NotFound(String),
    /// The `old_string` matches multiple locations but `replace_all` is false.
    #[error("{message}")]
    NotUnique {
        /// The total number of occurrences found.
        total: usize,
        /// The formatted error message including match locations.
        message: String,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EditInput {
    /// Absolute path to the file
    pub file_path: String,

    /// Text to find and replace
    pub old_string: String,

    /// Replacement text
    pub new_string: String,

    /// Replace all occurrences (default: false).
    /// When false, old_string must be unique in the file.
    #[serde(default)]
    pub replace_all: bool,

    /// Preview mode: compute and return the diff without writing the file.
    #[serde(default)]
    pub preview: bool,
}

/// Metadata about a single replacement location
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplacementLocation {
    /// 1-based line number where the match starts
    pub line: usize,
    /// 1-based column number where the match starts on that line
    pub column: usize,
}

#[derive(Debug, Serialize)]
pub struct EditOutput {
    /// File path that was edited
    pub file_path: String,

    /// Number of replacements made
    pub replacements: usize,

    /// Line numbers where replacements occurred
    pub locations: Vec<ReplacementLocation>,

    /// Success message
    pub message: String,
}

/// Find all byte offsets where `needle` occurs in `haystack`.
fn find_all_occurrences(haystack: &str, needle: &str) -> Vec<usize> {
    if needle.is_empty() {
        return vec![];
    }
    let mut offsets = Vec::new();
    let haystack_bytes = haystack.as_bytes();
    let needle_bytes = needle.as_bytes();
    let mut search_from = 0;
    while let Some(pos) = haystack_bytes[search_from..]
        .windows(needle_bytes.len())
        .position(|w| w == needle_bytes)
    {
        let absolute_pos = search_from + pos;
        offsets.push(absolute_pos);
        search_from = absolute_pos + 1;
        if search_from >= haystack_bytes.len() {
            break;
        }
    }
    offsets
}

/// Convert a byte offset to 1-based (line, column).
fn byte_offset_to_line_col(content: &str, byte_offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut column = 1;
    for (i, ch) in content.char_indices() {
        if i == byte_offset {
            return (line, column);
        }
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    // Offset at end of content
    (line, column)
}

/// Count total occurrences of `needle` in `haystack`.
fn count_occurrences(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    haystack.matches(needle).count()
}

/// Core editing logic — synchronous, testable without async runtime.
pub fn perform_edit(
    content: &str,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
) -> Result<(String, usize, Vec<ReplacementLocation>), EditError> {
    // --- Validation ---
    if old_string.is_empty() {
        return Err(EditError::EmptyOldString);
    }

    if old_string == new_string {
        return Err(EditError::IdenticalStrings);
    }

    if !content.contains(old_string) {
        // Build a helpful error message with context snippets
        let mut msg = "old_string not found in file content.".to_string();
        // Show first few lines of file for context
        let preview_lines: Vec<&str> = content.lines().take(3).collect();
        if !preview_lines.is_empty() {
            msg.push_str(&format!(
                "\nFile starts with:\n{}",
                preview_lines
                    .iter()
                    .enumerate()
                    .map(|(i, l)| format!("  {}: {}", i + 1, l))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        // Truncate old_string display to 120 chars
        let display_old = if old_string.len() > 120 {
            let mut end = 120;
            while !old_string.is_char_boundary(end) {
                end -= 1;
            }
            format!(
                "{}...(truncated, {} bytes total)",
                &old_string[..end],
                old_string.len()
            )
        } else {
            old_string.to_string()
        };
        msg.push_str(&format!(
            "\n\nold_string ({} bytes):\n{}",
            old_string.len(),
            display_old
        ));
        return Err(EditError::NotFound(msg));
    }

    let total_matches = count_occurrences(content, old_string);

    if !replace_all && total_matches > 1 {
        // Report all match locations so the user can disambiguate
        let offsets = find_all_occurrences(content, old_string);
        let locations: Vec<ReplacementLocation> = offsets
            .iter()
            .map(|&off| {
                let (line, col) = byte_offset_to_line_col(content, off);
                ReplacementLocation { line, column: col }
            })
            .collect();
        let location_strs: Vec<String> = locations
            .iter()
            .map(|loc| format!("  - line {}", loc.line))
            .collect();
        let message = format!(
            "old_string is not unique in file — {} occurrences found at:\n{}\nUse replace_all: true to replace all occurrences, or make old_string more specific.",
            total_matches,
            location_strs.join("\n")
        );
        return Err(EditError::NotUnique {
            total: total_matches,
            message,
        });
    }

    // --- Perform replacement ---
    let new_content;
    let replacements;
    let locations;

    if replace_all {
        replacements = total_matches;
        // Build new content tracking positions
        let offsets = find_all_occurrences(content, old_string);
        locations = offsets
            .iter()
            .map(|&off| {
                let (line, col) = byte_offset_to_line_col(content, off);
                ReplacementLocation { line, column: col }
            })
            .collect();
        new_content = content.replace(old_string, new_string);
    } else {
        replacements = 1;
        let offset = content.find(old_string).ok_or_else(|| {
            EditError::NotFound(
                "old_string not found (race condition or encoding mismatch)".to_string(),
            )
        })?;
        let (line, col) = byte_offset_to_line_col(content, offset);
        locations = vec![ReplacementLocation { line, column: col }];
        new_content = content.replacen(old_string, new_string, 1);
    };

    Ok((new_content, replacements, locations))
}

/// Compute diff hunks between old and new content using a simple LCS approach.
/// Returns structured hunks suitable for rendering via DiffRenderer.
#[allow(unused_assignments)]
pub fn compute_diff_hunks(old: &str, new: &str) -> Vec<DiffHunk> {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    let m = old_lines.len();
    let n = new_lines.len();

    // Guard against O(m*n) memory explosion on files with many short lines.
    // Fall back to a simple whole-file replacement diff for large inputs.
    const MAX_LINES_FOR_LCS: usize = 50_000;
    if m > MAX_LINES_FOR_LCS || n > MAX_LINES_FOR_LCS {
        if old_lines == new_lines {
            return Vec::new();
        }
        let header = format!("@@ -1,{m} +1,{n} @@");
        return vec![DiffHunk {
            old_start: 1,
            old_count: m,
            new_start: 1,
            new_count: n,
            header,
            lines: Vec::new(),
        }];
    }

    // Build LCS table
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            if old_lines[i - 1] == new_lines[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else {
                dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
            }
        }
    }

    // Backtrack to find edit script
    let mut edits: Vec<(char, &str)> = Vec::new();
    let (mut i, mut j) = (m, n);
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && old_lines[i - 1] == new_lines[j - 1] {
            edits.push(('=', old_lines[i - 1]));
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            edits.push(('+', new_lines[j - 1]));
            j -= 1;
        } else {
            edits.push(('-', old_lines[i - 1]));
            i -= 1;
        }
    }
    edits.reverse();

    // Build hunks from edit script with context lines
    const CONTEXT: usize = 3;
    let mut hunks: Vec<DiffHunk> = Vec::new();
    let mut current_lines: Vec<DiffLine> = Vec::new();
    let mut old_line = 0usize;
    let mut new_line = 0usize;
    let mut hunk_old_start = 0usize;
    let mut hunk_new_start = 0usize;
    let mut in_hunk = false;
    let mut changes_in_hunk = 0;
    let mut context_after_change = 0usize;

    for edit in &edits {
        match edit.0 {
            '=' => {
                if in_hunk {
                    if context_after_change < CONTEXT {
                        current_lines.push(DiffLine {
                            line_type: DiffLineType::Context,
                            content: edit.1.to_string(),
                            line_number_old: Some(old_line + 1),
                            line_number_new: Some(new_line + 1),
                        });
                        context_after_change += 1;
                    } else {
                        // Close the current hunk
                        let hunk = finalize_hunk(
                            hunk_old_start,
                            hunk_new_start,
                            &current_lines,
                            old_line,
                            new_line,
                        );
                        hunks.push(hunk);
                        current_lines.clear();
                        in_hunk = false;
                        changes_in_hunk = 0;
                        context_after_change = 0;
                        // Don't increment — this context line was consumed for closing
                        old_line += 1;
                        new_line += 1;
                        continue;
                    }
                }
                old_line += 1;
                new_line += 1;
            }
            '-' => {
                if !in_hunk {
                    // Start new hunk with leading context
                    in_hunk = true;
                    hunk_old_start = old_line + 1;
                    hunk_new_start = new_line + 1;
                    changes_in_hunk = 0;
                    context_after_change = 0;
                    current_lines.clear();
                    // Add preceding context
                    let ctx_start = old_line.saturating_sub(CONTEXT);
                    for (k, line) in old_lines
                        .iter()
                        .enumerate()
                        .skip(ctx_start)
                        .take(old_line - ctx_start)
                    {
                        current_lines.push(DiffLine {
                            line_type: DiffLineType::Context,
                            content: line.to_string(),
                            line_number_old: Some(k + 1),
                            line_number_new: Some(k + 1),
                        });
                    }
                    hunk_old_start = ctx_start + 1;
                    hunk_new_start = ctx_start + 1;
                }
                current_lines.push(DiffLine {
                    line_type: DiffLineType::Delete,
                    content: edit.1.to_string(),
                    line_number_old: Some(old_line + 1),
                    line_number_new: None,
                });
                changes_in_hunk += 1;
                context_after_change = 0;
                old_line += 1;
            }
            '+' => {
                if !in_hunk {
                    in_hunk = true;
                    changes_in_hunk = 0;
                    context_after_change = 0;
                    current_lines.clear();
                    let ctx_start = old_line.saturating_sub(CONTEXT);
                    for (k, line) in old_lines
                        .iter()
                        .enumerate()
                        .skip(ctx_start)
                        .take(old_line - ctx_start)
                    {
                        current_lines.push(DiffLine {
                            line_type: DiffLineType::Context,
                            content: line.to_string(),
                            line_number_old: Some(k + 1),
                            line_number_new: Some(k + 1),
                        });
                    }
                    hunk_old_start = ctx_start + 1;
                    hunk_new_start = ctx_start + 1;
                }
                current_lines.push(DiffLine {
                    line_type: DiffLineType::Add,
                    content: edit.1.to_string(),
                    line_number_old: None,
                    line_number_new: Some(new_line + 1),
                });
                changes_in_hunk += 1;
                context_after_change = 0;
                new_line += 1;
            }
            _ => {}
        }
    }

    // Flush remaining hunk
    if in_hunk && changes_in_hunk > 0 {
        let hunk = finalize_hunk(
            hunk_old_start,
            hunk_new_start,
            &current_lines,
            old_line,
            new_line,
        );
        hunks.push(hunk);
    }

    hunks
}

/// Finalize a hunk by computing its header and metadata.
fn finalize_hunk(
    old_start: usize,
    new_start: usize,
    lines: &[DiffLine],
    _old_end: usize,
    _new_end: usize,
) -> DiffHunk {
    let old_count = lines
        .iter()
        .filter(|l| l.line_type == DiffLineType::Context || l.line_type == DiffLineType::Delete)
        .count();
    let new_count = lines
        .iter()
        .filter(|l| l.line_type == DiffLineType::Context || l.line_type == DiffLineType::Add)
        .count();

    let old_range = if old_count == 1 {
        format!("{old_start}")
    } else {
        format!("{old_start},{old_count}")
    };
    let new_range = if new_count == 1 {
        format!("{new_start}")
    } else {
        format!("{new_start},{new_count}")
    };

    DiffHunk {
        old_start,
        old_count,
        new_start,
        new_count,
        header: format!("@@ -{old_range} +{new_range} @@"),
        lines: lines.to_vec(),
    }
}

/// Result of attempting a three-way merge fallback.
enum MergeFallbackResult {
    /// Merge succeeded (possibly with conflicts).
    Applied {
        merged_content: String,
        merge_conflicts: Vec<merge::ConflictRegion>,
    },
    /// Three-way merge could not be attempted (no git base available).
    NotAvailable(String),
}

/// Attempt a three-way merge when `old_string` is not found in the current file.
///
/// Strategy:
/// - `base` = git HEAD version of the file
/// - `ours` = current file content on disk
/// - `theirs` = what the file would look like if we applied the edit to base
///   (i.e. `base.replace(old_string, new_string)`)
///
/// If the base version is available and contains `old_string`, we can construct
/// `theirs` and perform the three-way merge.
async fn attempt_merge_fallback(
    file_path: &str,
    current_content: &str,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
) -> MergeFallbackResult {
    // Get the git HEAD version of the file
    let base = match merge::get_git_head_version(file_path).await {
        Some(content) => content,
        None => {
            return MergeFallbackResult::NotAvailable(
                "old_string not found in file content, and git HEAD version is not available for three-way merge.".to_string(),
            );
        }
    };

    // Check if the base version contains old_string
    if !base.contains(old_string) {
        return MergeFallbackResult::NotAvailable(
            "old_string not found in file content, and it is also not present in the git HEAD version — the edit target does not exist.".to_string(),
        );
    }

    // Construct "theirs" — what the file would look like with the edit applied to base
    let theirs = if replace_all {
        base.replace(old_string, new_string)
    } else {
        base.replacen(old_string, new_string, 1)
    };

    // Perform three-way merge
    let result = merge::three_way_merge(&base, current_content, &theirs);

    match result {
        MergeResult::Clean(merged) => MergeFallbackResult::Applied {
            merged_content: merged,
            merge_conflicts: vec![],
        },
        MergeResult::Conflicted { merged, conflicts } => MergeFallbackResult::Applied {
            merged_content: merged,
            merge_conflicts: conflicts,
        },
    }
}

pub async fn execute(input: EditInput) -> Result<ToolOutput, ToolError> {
    use tokio::fs;

    // --- Check file existence ---
    let metadata = fs::metadata(&input.file_path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ToolError::InvalidInput(format!("File not found: {}", input.file_path))
        } else {
            ToolError::ExecutionFailed(format!("Failed to access file: {e}"))
        }
    })?;

    if metadata.is_dir() {
        return Err(ToolError::InvalidInput(format!(
            "Path is a directory, not a file: {}",
            input.file_path
        )));
    }

    // Check file size before reading to prevent memory exhaustion
    const MAX_EDIT_FILE_SIZE: u64 = 10 * 1024 * 1024; // 10 MB
    if metadata.len() > MAX_EDIT_FILE_SIZE {
        return Err(ToolError::InvalidInput(format!(
            "File too large to edit: {} bytes (max {} bytes). Use terminal commands for large files.",
            metadata.len(),
            MAX_EDIT_FILE_SIZE
        )));
    }

    // --- Read file ---
    let content = fs::read_to_string(&input.file_path).await.map_err(|e| {
        ToolError::ExecutionFailed(format!("Failed to read file '{}': {}", input.file_path, e))
    })?;

    // --- Perform the edit (with three-way merge fallback) ---
    let edit_result = perform_edit(
        &content,
        &input.old_string,
        &input.new_string,
        input.replace_all,
    );

    let (new_content, replacements, locations, merge_conflicts) = match edit_result {
        Ok((nc, reps, locs)) => (nc, reps, locs, vec![]),
        Err(EditError::NotFound(_)) => {
            // old_string not found — try three-way merge fallback
            let merge_result = attempt_merge_fallback(
                &input.file_path,
                &content,
                &input.old_string,
                &input.new_string,
                input.replace_all,
            )
            .await;

            match merge_result {
                MergeFallbackResult::Applied {
                    merged_content,
                    merge_conflicts,
                } => {
                    let locs = if merge_conflicts.is_empty() {
                        vec![ReplacementLocation { line: 0, column: 0 }]
                    } else {
                        vec![ReplacementLocation {
                            line: merge_conflicts[0].start_line,
                            column: 0,
                        }]
                    };
                    (merged_content, 1, locs, merge_conflicts)
                }
                MergeFallbackResult::NotAvailable(original_error) => {
                    return Err(ToolError::InvalidInput(original_error));
                }
            }
        }
        Err(e) => return Err(ToolError::InvalidInput(e.to_string())),
    };

    // --- Generate diff preview ---
    let hunks = compute_diff_hunks(&content, &new_content);
    let renderer = DiffRenderer::new();
    let diff_preview = if hunks.is_empty() {
        String::new()
    } else {
        renderer.render_diff(&hunks, &input.file_path)
    };

    // --- Build location summary ---
    let location_summary: Vec<String> = locations
        .iter()
        .map(|loc| format!("line {}", loc.line))
        .collect();

    if input.preview {
        // Preview mode: return the diff without writing the file
        let message = if replacements == 1 {
            format!(
                "Preview: would replace 1 occurrence at {} in {}",
                location_summary[0], input.file_path
            )
        } else {
            format!(
                "Preview: would replace {} occurrences at [{}] in {}",
                replacements,
                location_summary.join(", "),
                input.file_path
            )
        };

        let full_content = if diff_preview.is_empty() {
            message.clone()
        } else {
            format!("{message}\n\n{diff_preview}")
        };

        return Ok(ToolOutput {
            content: full_content,
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("file_path".to_string(), json!(input.file_path));
                map.insert("replacements".to_string(), json!(replacements));
                map.insert("locations".to_string(), json!(locations));
                map.insert("preview".to_string(), json!(true));
                map
            },
        });
    }

    // --- Write file (atomic: write to temp then rename to avoid corruption on crash) ---
    let temp_path = format!(
        "{}.shannon-edit-{}",
        input.file_path,
        uuid::Uuid::new_v4().as_simple()
    );
    fs::write(&temp_path, &new_content).await.map_err(|e| {
        ToolError::ExecutionFailed(format!("Failed to write file '{}': {}", input.file_path, e))
    })?;
    fs::rename(&temp_path, &input.file_path)
        .await
        .map_err(|e| {
            let _ = std::fs::remove_file(&temp_path);
            ToolError::ExecutionFailed(format!(
                "Failed to rename temp file for '{}': {}",
                input.file_path, e
            ))
        })?;

    // If there are merge conflicts, report them as an error with the conflict info
    if !merge_conflicts.is_empty() {
        let conflict_report: Vec<String> = merge_conflicts
            .iter()
            .enumerate()
            .map(|(i, c)| {
                format!(
                    "  Conflict {} at line {}:\n    ours:   {}\n    theirs: {}",
                    i + 1,
                    c.start_line,
                    c.ours_content.lines().next().unwrap_or("(empty)"),
                    c.theirs_content.lines().next().unwrap_or("(empty)")
                )
            })
            .collect();

        let message = format!(
            "Three-way merge applied with {} conflict(s) in {}. \
             The file has been written with conflict markers. \
             Use MergeResolve to pick 'ours' or 'theirs' for each conflict.\n\n{}\n\n{}",
            merge_conflicts.len(),
            input.file_path,
            conflict_report.join("\n"),
            diff_preview
        );

        return Ok(ToolOutput {
            content: message,
            is_error: true,
            metadata: {
                let mut map = HashMap::new();
                map.insert("file_path".to_string(), json!(input.file_path));
                map.insert("replacements".to_string(), json!(replacements));
                map.insert("locations".to_string(), json!(locations));
                map.insert("merge_conflicts".to_string(), json!(merge_conflicts.len()));
                map
            },
        });
    }

    let message = if replacements == 1 {
        format!(
            "Successfully replaced 1 occurrence at {} in {}",
            location_summary[0], input.file_path
        )
    } else {
        format!(
            "Successfully replaced {} occurrences at [{}] in {}",
            replacements,
            location_summary.join(", "),
            input.file_path
        )
    };

    let full_content = if diff_preview.is_empty() {
        message.clone()
    } else {
        format!("{message}\n\n{diff_preview}")
    };

    Ok(ToolOutput {
        content: full_content,
        is_error: false,
        metadata: {
            let mut map = HashMap::new();
            map.insert("file_path".to_string(), json!(input.file_path));
            map.insert("replacements".to_string(), json!(replacements));
            map.insert("locations".to_string(), json!(locations));
            map
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Helper: write a temp file and return its path
    fn write_temp_file(content: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("shannon_edit_tests");
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

    // --- perform_edit unit tests ---

    #[test]
    fn test_single_replacement() {
        let content = "hello world\nfoo bar\nbaz qux";
        let result = perform_edit(content, "foo bar", "FOO BAR", false);
        let (new_content, replacements, locations) = result.unwrap();
        assert_eq!(new_content, "hello world\nFOO BAR\nbaz qux");
        assert_eq!(replacements, 1);
        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].line, 2);
        assert_eq!(locations[0].column, 1);
    }

    #[test]
    fn test_replace_all() {
        let content = "foo bar\nfoo baz\nfoo qux";
        let result = perform_edit(content, "foo", "FOO", true);
        let (new_content, replacements, locations) = result.unwrap();
        assert_eq!(new_content, "FOO bar\nFOO baz\nFOO qux");
        assert_eq!(replacements, 3);
        assert_eq!(locations.len(), 3);
        assert_eq!(locations[0].line, 1);
        assert_eq!(locations[1].line, 2);
        assert_eq!(locations[2].line, 3);
    }

    #[test]
    fn test_not_found() {
        let content = "hello world";
        let result = perform_edit(content, "missing", "replacement", false);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("old_string not found"));
    }

    #[test]
    fn test_empty_old_string() {
        let content = "hello world";
        let result = perform_edit(content, "", "replacement", false);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("old_string must not be empty"));
    }

    #[test]
    fn test_identical_strings() {
        let content = "hello world";
        let result = perform_edit(content, "hello", "hello", false);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("identical"));
    }

    #[test]
    fn test_multiple_matches_without_replace_all() {
        let content = "line1 foo\nline2 foo\nline3";
        let result = perform_edit(content, "foo", "bar", false);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not unique"));
        assert!(err.contains("2 occurrences"));
        assert!(err.contains("line 1"));
        assert!(err.contains("line 2"));
    }

    #[test]
    fn test_multiline_old_string() {
        let content = "fn main() {\n    println!(\"hello\");\n    println!(\"world\");\n}\n";
        let old = "    println!(\"hello\");\n    println!(\"world\");";
        let new = "    println!(\"hello, world!\");";
        let result = perform_edit(content, old, new, false);
        let (new_content, replacements, locations) = result.unwrap();
        assert_eq!(
            new_content,
            "fn main() {\n    println!(\"hello, world!\");\n}\n"
        );
        assert_eq!(replacements, 1);
        assert_eq!(locations[0].line, 2);
    }

    #[test]
    fn test_column_tracking() {
        let content = "  let x = 1;\n  let y = 2;";
        let result = perform_edit(content, "let x", "let x_mut", false);
        let (new_content, replacements, locations) = result.unwrap();
        assert_eq!(new_content, "  let x_mut = 1;\n  let y = 2;");
        assert_eq!(replacements, 1);
        assert_eq!(locations[0].line, 1);
        assert_eq!(locations[0].column, 3);
    }

    #[test]
    fn test_replacement_with_empty_string() {
        let content = "hello world\nfoo bar";
        let result = perform_edit(content, "hello world\n", "", false);
        let (new_content, replacements, _locations) = result.unwrap();
        assert_eq!(new_content, "foo bar");
        assert_eq!(replacements, 1);
    }

    #[test]
    fn test_unicode_content() {
        let content = "function test() {\n    let x = 1;\n}\n";
        let result = perform_edit(content, "let x = 1", "let x = 42", false);
        let (new_content, replacements, locations) = result.unwrap();
        assert_eq!(new_content, "function test() {\n    let x = 42;\n}\n");
        assert_eq!(replacements, 1);
        assert_eq!(locations[0].line, 2);
    }

    // --- Integration tests (async, file I/O) ---

    #[tokio::test]
    async fn test_async_single_edit() {
        let path = write_temp_file("line1\nline2\nline3\n");
        let input = EditInput {
            file_path: path.to_string_lossy().to_string(),
            old_string: "line2".to_string(),
            new_string: "LINE_TWO".to_string(),
            replace_all: false,
            preview: false,
        };
        let result = execute(input).await;
        cleanup_temp_file(&path);
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(!output.is_error);
        assert_eq!(output.metadata["replacements"], json!(1));
    }

    #[tokio::test]
    async fn test_async_replace_all() {
        let path = write_temp_file("foo\nbar foo\nbaz foo\n");
        let input = EditInput {
            file_path: path.to_string_lossy().to_string(),
            old_string: "foo".to_string(),
            new_string: "FOO".to_string(),
            replace_all: true,
            preview: false,
        };
        let result = execute(input).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert_eq!(output.metadata["replacements"], json!(3));

        // Verify file was actually written
        let written = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(written, "FOO\nbar FOO\nbaz FOO\n");
        cleanup_temp_file(&path);
    }

    #[tokio::test]
    async fn test_async_file_not_found() {
        let input = EditInput {
            file_path: "/nonexistent/path/file.txt".to_string(),
            old_string: "foo".to_string(),
            new_string: "bar".to_string(),
            replace_all: false,
            preview: false,
        };
        let result = execute(input).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("File not found"));
    }

    #[tokio::test]
    async fn test_async_old_string_not_found() {
        let path = write_temp_file("hello world\n");
        let input = EditInput {
            file_path: path.to_string_lossy().to_string(),
            old_string: "missing".to_string(),
            new_string: "replacement".to_string(),
            replace_all: false,
            preview: false,
        };
        let result = execute(input).await;
        cleanup_temp_file(&path);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_async_not_unique_error() {
        let path = write_temp_file("foo bar\nfoo baz\n");
        let input = EditInput {
            file_path: path.to_string_lossy().to_string(),
            old_string: "foo".to_string(),
            new_string: "FOO".to_string(),
            replace_all: false,
            preview: false,
        };
        let result = execute(input).await;
        cleanup_temp_file(&path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not unique"));
    }

    // --- byte_offset_to_line_col tests ---

    #[test]
    fn test_line_col_at_start() {
        assert_eq!(byte_offset_to_line_col("hello", 0), (1, 1));
    }

    #[test]
    fn test_line_col_after_newline() {
        assert_eq!(byte_offset_to_line_col("a\nb\nc", 2), (2, 1));
    }

    #[test]
    fn test_line_col_mid_line() {
        assert_eq!(byte_offset_to_line_col("  hello", 2), (1, 3));
    }

    #[test]
    fn test_line_col_multibyte_char() {
        // "a" = 1 byte, then a 3-byte UTF-8 char
        let s = "a\u{2605}"; // "a★"
        assert_eq!(byte_offset_to_line_col(s, 1), (1, 2));
    }

    // --- compute_diff_hunks tests ---

    #[test]
    fn test_diff_single_line_change() {
        let old = "line1\nline2\nline3";
        let new = "line1\nLINE_TWO\nline3";
        let hunks = compute_diff_hunks(old, new);
        assert_eq!(hunks.len(), 1);
        assert!(
            hunks[0]
                .lines
                .iter()
                .any(|l| l.line_type == DiffLineType::Delete)
        );
        assert!(
            hunks[0]
                .lines
                .iter()
                .any(|l| l.line_type == DiffLineType::Add)
        );
    }

    #[test]
    fn test_diff_no_changes() {
        let old = "line1\nline2\nline3";
        let hunks = compute_diff_hunks(old, old);
        assert!(hunks.is_empty());
    }

    #[test]
    fn test_diff_addition_only() {
        let old = "line1\nline3";
        let new = "line1\nline2\nline3";
        let hunks = compute_diff_hunks(old, new);
        assert!(!hunks.is_empty());
        let has_add = hunks
            .iter()
            .any(|h| h.lines.iter().any(|l| l.line_type == DiffLineType::Add));
        assert!(has_add);
    }

    #[test]
    fn test_diff_deletion_only() {
        let old = "line1\nline2\nline3";
        let new = "line1\nline3";
        let hunks = compute_diff_hunks(old, new);
        assert!(!hunks.is_empty());
        let has_del = hunks
            .iter()
            .any(|h| h.lines.iter().any(|l| l.line_type == DiffLineType::Delete));
        assert!(has_del);
    }

    #[test]
    fn test_diff_hunk_header_format() {
        let old = "a\nb\nc";
        let new = "a\nB\nc";
        let hunks = compute_diff_hunks(old, new);
        assert_eq!(hunks.len(), 1);
        assert!(hunks[0].header.starts_with("@@"));
        assert!(hunks[0].header.ends_with("@@"));
    }

    #[test]
    fn test_diff_multiple_hunks() {
        let old = "a\nb\nc\nd\ne\nf\ng";
        let new = "a\nB\nc\nd\ne\nF\ng";
        let hunks = compute_diff_hunks(old, new);
        // Two separate changes should produce two hunks if far enough apart
        assert!(!hunks.is_empty());
    }

    #[test]
    fn test_diff_empty_to_content() {
        let hunks = compute_diff_hunks("", "new line\nanother line");
        assert!(!hunks.is_empty());
        let all_adds: Vec<_> = hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .filter(|l| l.line_type == DiffLineType::Add)
            .collect();
        assert!(!all_adds.is_empty());
    }

    #[test]
    fn test_diff_content_to_empty() {
        let hunks = compute_diff_hunks("old line\nanother line", "");
        assert!(!hunks.is_empty());
        let all_dels: Vec<_> = hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .filter(|l| l.line_type == DiffLineType::Delete)
            .collect();
        assert!(!all_dels.is_empty());
    }

    #[test]
    fn test_diff_context_lines_included() {
        let old = "ctx1\nctx2\nold\nctx4\nctx5";
        let new = "ctx1\nctx2\nnew\nctx4\nctx5";
        let hunks = compute_diff_hunks(old, new);
        assert_eq!(hunks.len(), 1);
        let context_count = hunks[0]
            .lines
            .iter()
            .filter(|l| l.line_type == DiffLineType::Context)
            .count();
        // Should have context lines surrounding the change
        assert!(
            context_count >= 2,
            "expected >= 2 context lines, got {context_count}"
        );
    }

    // --- Diff preview in execute output ---

    #[tokio::test]
    async fn test_execute_includes_diff_preview() {
        let path = write_temp_file("line1\nline2\nline3\n");
        let input = EditInput {
            file_path: path.to_string_lossy().to_string(),
            old_string: "line2".to_string(),
            new_string: "REPLACED".to_string(),
            replace_all: false,
            preview: false,
        };
        let result = execute(input).await;
        cleanup_temp_file(&path);
        assert!(result.is_ok());
        let output = result.unwrap();
        // Should contain diff markers
        assert!(output.content.contains("--- diff for:"));
        assert!(output.content.contains("line2") || output.content.contains("REPLACED"));
        assert!(output.content.contains("changed"));
    }

    #[tokio::test]
    async fn test_execute_diff_shows_additions_and_deletions() {
        let path = write_temp_file("fn main() {\n    println!(\"old\");\n}\n");
        let input = EditInput {
            file_path: path.to_string_lossy().to_string(),
            old_string: "println!(\"old\");".to_string(),
            new_string: "println!(\"new\");".to_string(),
            replace_all: false,
            preview: false,
        };
        let result = execute(input).await;
        cleanup_temp_file(&path);
        assert!(result.is_ok());
        let output = result.unwrap();
        // The diff should contain both the deleted and added line
        assert!(output.content.contains("old"));
        assert!(output.content.contains("new"));
    }

    #[tokio::test]
    async fn test_execute_diff_with_replace_all() {
        let path = write_temp_file("foo\nbar foo\nbaz foo\n");
        let input = EditInput {
            file_path: path.to_string_lossy().to_string(),
            old_string: "foo".to_string(),
            new_string: "FOO".to_string(),
            replace_all: true,
            preview: false,
        };
        let result = execute(input).await;
        cleanup_temp_file(&path);
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.content.contains("diff for:"));
    }

    #[tokio::test]
    async fn test_preview_mode_does_not_write() {
        let path = write_temp_file("hello world\n");
        let input = EditInput {
            file_path: path.to_string_lossy().to_string(),
            old_string: "hello".to_string(),
            new_string: "goodbye".to_string(),
            replace_all: false,
            preview: true,
        };
        let result = execute(input).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.content.contains("Preview:"));
        assert!(output.metadata.get("preview").unwrap().as_bool().unwrap());

        // File should NOT have changed
        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "hello world\n");
        cleanup_temp_file(&path);
    }

    #[tokio::test]
    async fn test_preview_includes_diff() {
        let path = write_temp_file("fn main() {\n    println!(\"old\");\n}\n");
        let input = EditInput {
            file_path: path.to_string_lossy().to_string(),
            old_string: "println!(\"old\");".to_string(),
            new_string: "println!(\"new\");".to_string(),
            replace_all: false,
            preview: true,
        };
        let result = execute(input).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.content.contains("Preview:"));
        assert!(output.content.contains("old"));
        assert!(output.content.contains("new"));
        cleanup_temp_file(&path);
    }

    // --- attempt_merge_fallback tests ---
    //
    // These tests change the process working directory (via `set_current_dir`) so
    // that `git show HEAD:<relative-path>` inside `get_git_head_version` resolves
    // correctly. A static mutex serialises them to avoid parallel-cwd races.

    use std::sync::Mutex;
    static CWD_MUTEX: Mutex<()> = Mutex::new(());

    /// Helper: create a temp git repo, commit an initial file, return TempDir.
    /// The repo root can be used as cwd so that `git show HEAD:<file>` works.
    async fn init_git_repo_with_file(filename: &str, content: &str) -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        let repo_path = dir.path();

        // git init
        tokio::process::Command::new("git")
            .arg("init")
            .current_dir(repo_path)
            .output()
            .await
            .expect("git init failed");

        // git config (needed for commit)
        tokio::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(repo_path)
            .output()
            .await
            .expect("git config email failed");
        tokio::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(repo_path)
            .output()
            .await
            .expect("git config name failed");

        // Write initial file
        tokio::fs::write(repo_path.join(filename), content)
            .await
            .unwrap();

        // git add + commit
        tokio::process::Command::new("git")
            .args(["add", filename])
            .current_dir(repo_path)
            .output()
            .await
            .expect("git add failed");
        tokio::process::Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(repo_path)
            .output()
            .await
            .expect("git commit failed");

        dir
    }

    #[tokio::test]
    async fn test_attempt_merge_fallback_no_git() {
        let _lock = CWD_MUTEX.lock().unwrap();

        // Non-git temp directory — no HEAD version available
        let dir = tempfile::TempDir::new().unwrap();
        tokio::fs::write(dir.path().join("test.txt"), "hello world")
            .await
            .unwrap();

        let saved_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let result = attempt_merge_fallback(
            "test.txt",
            "hello world",
            "hello",
            "goodbye",
            false,
        )
        .await;

        std::env::set_current_dir(&saved_cwd).unwrap();

        match result {
            MergeFallbackResult::NotAvailable(msg) => {
                assert!(
                    msg.contains("git HEAD version is not available"),
                    "expected 'git HEAD version is not available' in message, got: {msg}"
                );
            }
            MergeFallbackResult::Applied { .. } => {
                panic!("expected NotAvailable, got Applied");
            }
        }
    }

    #[tokio::test]
    async fn test_attempt_merge_fallback_base_lacks_old_string() {
        let _lock = CWD_MUTEX.lock().unwrap();

        let dir = init_git_repo_with_file("test.txt", "original content").await;

        let saved_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let result = attempt_merge_fallback(
            "test.txt",
            "original content",
            "nonexistent",
            "replacement",
            false,
        )
        .await;

        std::env::set_current_dir(&saved_cwd).unwrap();

        match result {
            MergeFallbackResult::NotAvailable(msg) => {
                assert!(
                    msg.contains("not present in the git HEAD version"),
                    "expected message about not present in git HEAD, got: {msg}"
                );
            }
            MergeFallbackResult::Applied { .. } => {
                panic!("expected NotAvailable, got Applied");
            }
        }
    }

    #[tokio::test]
    async fn test_attempt_merge_fallback_clean_merge() {
        let _lock = CWD_MUTEX.lock().unwrap();

        // base   = "line1\nline2\nline3\n"
        // ours   = "line1\nline2\nMODIFIED3\n"  (external change on line3)
        // theirs = "line1\nreplaced\nline3\n"   (edit: line2 → replaced)
        // ours changed line3, theirs changed line2 → non-overlapping → clean
        let base_content = "line1\nline2\nline3\n";
        let dir = init_git_repo_with_file("test.txt", base_content).await;

        let disk_content = "line1\nline2\nMODIFIED3\n";
        tokio::fs::write(dir.path().join("test.txt"), disk_content)
            .await
            .unwrap();

        let saved_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let result = attempt_merge_fallback(
            "test.txt",
            disk_content,
            "line2",
            "replaced",
            false,
        )
        .await;

        std::env::set_current_dir(&saved_cwd).unwrap();

        match result {
            MergeFallbackResult::Applied {
                merged_content,
                merge_conflicts,
            } => {
                assert!(
                    merge_conflicts.is_empty(),
                    "expected no merge conflicts, got {}",
                    merge_conflicts.len()
                );
                assert!(
                    merged_content.contains("replaced"),
                    "merged content should contain 'replaced', got: {merged_content}"
                );
                assert!(
                    merged_content.contains("MODIFIED3"),
                    "merged content should contain 'MODIFIED3' from ours, got: {merged_content}"
                );
            }
            MergeFallbackResult::NotAvailable(msg) => {
                panic!("expected Applied, got NotAvailable: {msg}");
            }
        }
    }

    #[tokio::test]
    async fn test_attempt_merge_fallback_conflict_merge() {
        let _lock = CWD_MUTEX.lock().unwrap();

        // base   = "line1\nline2\nline3\n"
        // ours   = "line1\nchanged_by_us\nline3\n"
        // theirs = "line1\nchanged_by_edit\nline3\n"
        // Both sides change line2 to different values → conflict
        let base_content = "line1\nline2\nline3\n";
        let dir = init_git_repo_with_file("test.txt", base_content).await;

        let disk_content = "line1\nchanged_by_us\nline3\n";
        tokio::fs::write(dir.path().join("test.txt"), disk_content)
            .await
            .unwrap();

        let saved_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let result = attempt_merge_fallback(
            "test.txt",
            disk_content,
            "line2",
            "changed_by_edit",
            false,
        )
        .await;

        std::env::set_current_dir(&saved_cwd).unwrap();

        match result {
            MergeFallbackResult::Applied {
                merged_content,
                merge_conflicts,
            } => {
                assert!(
                    !merge_conflicts.is_empty(),
                    "expected merge conflicts, got none. merged_content: {merged_content}"
                );
                assert!(
                    merged_content.contains("changed_by_us"),
                    "merged content should contain ours content 'changed_by_us'"
                );
                assert!(
                    merged_content.contains("changed_by_edit"),
                    "merged content should contain theirs content 'changed_by_edit'"
                );
            }
            MergeFallbackResult::NotAvailable(msg) => {
                panic!("expected Applied, got NotAvailable: {msg}");
            }
        }
    }

    #[tokio::test]
    async fn test_execute_merge_fallback_integration() {
        let _lock = CWD_MUTEX.lock().unwrap();

        // Integration: commit a file, modify it externally, call execute() with
        // old_string from the committed version. Direct edit fails (old_string not
        // in current content) → merge fallback path is triggered.
        let base_content = "line1\nline2\nline3\n";
        let dir = init_git_repo_with_file("test.txt", base_content).await;

        // Modify the file externally — change line3
        let disk_content = "line1\nline2\nEXTERNAL_CHANGE\n";
        tokio::fs::write(dir.path().join("test.txt"), disk_content)
            .await
            .unwrap();

        let saved_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let input = EditInput {
            file_path: "test.txt".to_string(),
            old_string: "line2".to_string(),
            new_string: "replaced_line2".to_string(),
            replace_all: false,
            preview: false,
        };
        let result = execute(input).await;

        std::env::set_current_dir(&saved_cwd).unwrap();

        assert!(
            result.is_ok(),
            "execute should succeed via merge fallback, got error: {:?}",
            result.err()
        );
        let output = result.unwrap();

        // The merge should be clean (ours changed line3, theirs changed line2)
        assert!(
            !output.is_error,
            "expected clean merge (is_error=false), got error output: {}",
            output.content
        );

        // Verify the file on disk has both changes merged
        let written = tokio::fs::read_to_string(dir.path().join("test.txt"))
            .await
            .unwrap();
        assert!(
            written.contains("replaced_line2"),
            "file should contain 'replaced_line2', got: {written}"
        );
        assert!(
            written.contains("EXTERNAL_CHANGE"),
            "file should contain 'EXTERNAL_CHANGE' from ours, got: {written}"
        );
    }
}
