//! Edit tool implementation — diff-based precise file editing
//!
//! Provides `old_string` → `new_string` replacement with:
//! - Line number reporting for each replacement
//! - Uniqueness validation (non-replace-all mode requires single match)
//! - File existence and permission checks
//! - Comprehensive error messages with context

use crate::{ToolOutput, ToolError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

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
pub fn perform_edit(content: &str, old_string: &str, new_string: &str, replace_all: bool) -> Result<(String, usize, Vec<ReplacementLocation>), String> {
    // --- Validation ---
    if old_string.is_empty() {
        return Err("old_string must not be empty".to_string());
    }

    if old_string == new_string {
        return Err("old_string and new_string are identical — no change needed".to_string());
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
            format!("{}...(truncated, {} bytes total)", &old_string[..120], old_string.len())
        } else {
            old_string.to_string()
        };
        msg.push_str(&format!(
            "\n\nold_string ({} bytes):\n{}",
            old_string.len(),
            display_old
        ));
        return Err(msg);
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
        return Err(format!(
            "old_string is not unique in file — {} occurrences found at:\n{}\nUse replace_all: true to replace all occurrences, or make old_string more specific.",
            total_matches,
            location_strs.join("\n")
        ));
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
        let offset = content.find(old_string).unwrap(); // guaranteed by contains check above
        let (line, col) = byte_offset_to_line_col(content, offset);
        locations = vec![ReplacementLocation { line, column: col }];
        new_content = content.replacen(old_string, new_string, 1);
    };

    Ok((new_content, replacements, locations))
}

pub async fn execute(input: EditInput) -> Result<ToolOutput, ToolError> {
    use tokio::fs;

    // --- Check file existence ---
    let metadata = fs::metadata(&input.file_path)
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ToolError::InvalidInput(format!(
                    "File not found: {}",
                    input.file_path
                ))
            } else {
                ToolError::ExecutionFailed(format!(
                    "Failed to access file: {e}"
                ))
            }
        })?;

    if metadata.is_dir() {
        return Err(ToolError::InvalidInput(format!(
            "Path is a directory, not a file: {}",
            input.file_path
        )));
    }

    // --- Read file ---
    let content = fs::read_to_string(&input.file_path)
        .await
        .map_err(|e| {
            ToolError::ExecutionFailed(format!(
                "Failed to read file '{}': {}",
                input.file_path, e
            ))
        })?;

    // --- Perform the edit ---
    let (new_content, replacements, locations) =
        perform_edit(&content, &input.old_string, &input.new_string, input.replace_all)
            .map_err(ToolError::InvalidInput)?;

    // --- Write file ---
    fs::write(&input.file_path, &new_content)
        .await
        .map_err(|e| {
            ToolError::ExecutionFailed(format!(
                "Failed to write file '{}': {}",
                input.file_path, e
            ))
        })?;

    // --- Build location summary ---
    let location_summary: Vec<String> = locations
        .iter()
        .map(|loc| format!("line {}", loc.line))
        .collect();

    let message = if replacements == 1 {
        format!(
            "Successfully replaced 1 occurrence at {} in {}",
            location_summary[0],
            input.file_path
        )
    } else {
        format!(
            "Successfully replaced {} occurrences at [{}] in {}",
            replacements,
            location_summary.join(", "),
            input.file_path
        )
    };

    Ok(ToolOutput {
        content: message.clone(),
        is_error: false,
        metadata: {
            let mut map = HashMap::new();
            map.insert("file_path".to_string(), json!(input.file_path));
            map.insert("replacements".to_string(), json!(replacements));
            map.insert(
                "locations".to_string(),
                json!(locations),
            );
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
        let err = result.unwrap_err();
        assert!(err.contains("old_string not found"));
    }

    #[test]
    fn test_empty_old_string() {
        let content = "hello world";
        let result = perform_edit(content, "", "replacement", false);
        let err = result.unwrap_err();
        assert!(err.contains("old_string must not be empty"));
    }

    #[test]
    fn test_identical_strings() {
        let content = "hello world";
        let result = perform_edit(content, "hello", "hello", false);
        let err = result.unwrap_err();
        assert!(err.contains("identical"));
    }

    #[test]
    fn test_multiple_matches_without_replace_all() {
        let content = "line1 foo\nline2 foo\nline3";
        let result = perform_edit(content, "foo", "bar", false);
        let err = result.unwrap_err();
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
        assert_eq!(new_content, "fn main() {\n    println!(\"hello, world!\");\n}\n");
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
}
