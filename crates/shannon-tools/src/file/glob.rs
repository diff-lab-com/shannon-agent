//! Glob tool implementation
//!
//! Provides pattern-based file search using the `glob` crate with:
//! - Recursive pattern matching (`**/*.rs`)
//! - Path restriction to a base directory
//! - Exclude patterns (e.g., `!target/**`)
//! - .gitignore-aware traversal via the `ignore` crate
//! - Results sorted by modification time (most recent first)

use crate::{ToolError, ToolOutput};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Input / Output types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GlobInput {
    /// Glob pattern to match files (e.g., `*.rs`, `src/**/*.py`)
    pub pattern: String,

    /// Optional directory to search in (defaults to current directory)
    pub path: Option<String>,

    /// Optional exclusion patterns (e.g., `["!target/**", "!**/test/**"]`)
    #[serde(default)]
    pub exclude_pattern: Option<Vec<String>>,
}

/// A single glob result with metadata.
#[derive(Debug, Clone, Serialize)]
pub struct GlobResult {
    /// Absolute or relative file path
    pub path: String,
    /// File size in bytes
    pub size: u64,
    /// Last-modified timestamp in ISO 8601 format, if available
    pub modified: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GlobOutput {
    /// Matching files with metadata
    pub files: Vec<GlobResult>,

    /// Number of matches found
    pub count: usize,

    /// Pattern that was searched
    pub pattern: String,
}

// ---------------------------------------------------------------------------
// Core implementation
// ---------------------------------------------------------------------------

/// Match options where `*` does NOT match directory separators, matching the
/// conventional glob semantics that Claude Code users expect.
const MATCH_OPTS: glob::MatchOptions = glob::MatchOptions {
    require_literal_separator: true,
    case_sensitive: true,
    require_literal_leading_dot: false,
};

/// Check whether `candidate` (a relative path) matches any of the given
/// exclude patterns.
fn matches_any_exclude(candidate: &Path, excludes: &[String]) -> bool {
    for exc in excludes {
        // Support the `!pattern` prefix convention -- strip it.
        let pat = exc.strip_prefix('!').unwrap_or(exc.as_str());
        if let Ok(glob_pat) = glob::Pattern::new(pat) {
            if glob_pat.matches_path_with(candidate, MATCH_OPTS) {
                return true;
            }
        }
    }
    false
}

/// Convert a `std::time::SystemTime` to an ISO 8601 string, or `None`.
fn format_modified_time(time: std::time::SystemTime) -> Option<String> {
    let duration = time.duration_since(std::time::UNIX_EPOCH).ok()?;
    let chrono_dt =
        chrono::DateTime::from_timestamp(duration.as_secs() as i64, duration.subsec_nanos())?;
    Some(chrono_dt.to_rfc3339())
}

/// Build a `GlobResult` from a file path, returning `None` on I/O errors.
fn build_result(path: &Path) -> Option<GlobResult> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok().and_then(format_modified_time);
    Some(GlobResult {
        path: path.display().to_string(),
        size: meta.len(),
        modified,
    })
}

/// Sort results by modification time descending (most recent first), with an
/// alphabetical-path fallback.
fn sort_results(results: &mut [GlobResult]) {
    results.sort_by(|a, b| match (&a.modified, &b.modified) {
        (Some(ma), Some(mb)) => mb.cmp(ma).then_with(|| a.path.cmp(&b.path)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.path.cmp(&b.path),
    });
}

/// Execute a glob search using the `ignore` crate for .gitignore-aware traversal
/// and the `glob` crate for pattern matching.
pub async fn execute(input: GlobInput) -> Result<ToolOutput, ToolError> {
    let base_path = input.path.as_deref().unwrap_or(".");
    let base = PathBuf::from(base_path);

    // Prevent path traversal (e.g. "../../etc") by checking components
    for component in base.components() {
        if matches!(component, std::path::Component::ParentDir) {
            // Allow if path resolves within cwd after canonicalization
            if let Ok(canonical) = std::fs::canonicalize(&base) {
                let cwd = std::env::current_dir().unwrap_or_default();
                if !canonical.starts_with(&cwd) {
                    return Ok(ToolOutput {
                        content: format!(
                            "Path traversal blocked: '{base_path}' resolves outside project"
                        ),
                        is_error: true,
                        metadata: HashMap::new(),
                    });
                }
            }
            break;
        }
    }

    // If the base directory does not exist, return early with empty results.
    if !base.exists() {
        return Ok(ToolOutput {
            content: format!("Directory not found: {base_path}"),
            is_error: true,
            metadata: HashMap::new(),
        });
    }

    let excludes = input.exclude_pattern.as_deref().unwrap_or(&[]);
    let pattern = &input.pattern;

    // Compile the glob pattern once.
    let glob_pattern = glob::Pattern::new(pattern)
        .map_err(|e| ToolError::InvalidInput(format!("Invalid glob pattern '{pattern}': {e}")))?;

    // Use `ignore::WalkBuilder` for .gitignore-aware traversal.
    let mut builder = ignore::WalkBuilder::new(&base);
    builder.hidden(true); // skip hidden files
    builder.git_ignore(true); // respect .gitignore
    builder.git_global(true); // respect global gitignore
    builder.git_exclude(true); // respect .git/info/exclude

    let mut results: Vec<GlobResult> = Vec::new();

    for entry in builder.build() {
        match entry {
            Ok(dir_entry) => {
                if let Some(file_type) = dir_entry.file_type() {
                    if !file_type.is_file() {
                        continue;
                    }
                }

                let path = dir_entry.path();

                // Match the path *relative* to the base directory. This ensures
                // `*.rs` only matches files in the root, not `src/mod.rs`.
                let rel = match path.strip_prefix(&base) {
                    Ok(r) => r,
                    Err(_) => continue,
                };

                if !glob_pattern.matches_path_with(rel, MATCH_OPTS) {
                    continue;
                }

                // Apply user-supplied exclude patterns.
                if matches_any_exclude(rel, excludes) {
                    continue;
                }

                if let Some(result) = build_result(path) {
                    results.push(result);
                }
            }
            Err(err) => {
                tracing::debug!("glob walk error: {}", err);
            }
        }
    }

    sort_results(&mut results);
    let count = results.len();

    // Build the output content summary.
    let content = if count == 0 {
        format!("No files found matching pattern: {pattern}")
    } else {
        let file_list: Vec<String> = results.iter().map(|r| r.path.clone()).collect();
        format!(
            "Found {} files matching pattern: {}\n{}",
            count,
            pattern,
            file_list.join("\n")
        )
    };

    // Build structured output.
    let mut metadata = HashMap::new();
    metadata.insert("files".to_string(), json!(results));
    metadata.insert("count".to_string(), json!(count));
    metadata.insert("pattern".to_string(), json!(pattern));

    Ok(ToolOutput {
        content,
        is_error: false,
        metadata,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Helper: create a small directory tree for testing.
    ///
    /// ```text
    /// tmp/
    ///   .git/           (empty directory so `ignore` respects .gitignore)
    ///   .gitignore      (contains "target/")
    ///   a.rs
    ///   b.rs
    ///   src/
    ///     mod.rs
    ///     lib.rs
    ///   target/
    ///     build.rs
    /// ```
    fn setup_test_tree(tmp: &TempDir) -> PathBuf {
        let root = tmp.path();

        // Create a minimal .git directory so `ignore` crate picks up .gitignore
        fs::create_dir_all(root.join(".git")).unwrap();

        fs::write(root.join("a.rs"), "// a").unwrap();
        fs::write(root.join("b.rs"), "// b").unwrap();

        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("mod.rs"), "// mod").unwrap();
        fs::write(src.join("lib.rs"), "// lib").unwrap();

        let target = root.join("target");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("build.rs"), "// build").unwrap();

        // Write a .gitignore so the `ignore` crate skips `target/`
        fs::write(root.join(".gitignore"), "target/\n").unwrap();

        root.to_path_buf()
    }

    /// Extract file paths from a ToolOutput metadata.
    fn extract_paths(output: &ToolOutput) -> Vec<String> {
        output.metadata["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["path"].as_str().unwrap().to_string())
            .collect()
    }

    #[tokio::test]
    async fn test_basic_pattern_matching() {
        let tmp = TempDir::new().unwrap();
        let root = setup_test_tree(&tmp);

        let input = GlobInput {
            pattern: "*.rs".to_string(),
            path: Some(root.display().to_string()),
            exclude_pattern: None,
        };

        let output = execute(input).await.unwrap();
        assert!(!output.is_error);

        let files = extract_paths(&output);

        assert!(files.iter().any(|f| f.ends_with("a.rs")));
        assert!(files.iter().any(|f| f.ends_with("b.rs")));
        // `*.rs` should NOT match files inside `src/` because `*` does not
        // cross directory boundaries.
        assert!(!files.iter().any(|f| f.contains("src")));
    }

    #[tokio::test]
    async fn test_recursive_pattern() {
        let tmp = TempDir::new().unwrap();
        let root = setup_test_tree(&tmp);

        let input = GlobInput {
            pattern: "**/*.rs".to_string(),
            path: Some(root.display().to_string()),
            exclude_pattern: None,
        };

        let output = execute(input).await.unwrap();
        assert!(!output.is_error);

        let files = extract_paths(&output);

        assert!(files.iter().any(|f| f.ends_with("a.rs")));
        assert!(files.iter().any(|f| f.ends_with("b.rs")));
        assert!(files.iter().any(|f| f.ends_with("mod.rs")));
        assert!(files.iter().any(|f| f.ends_with("lib.rs")));
        // `target/` should be excluded by .gitignore
        assert!(!files.iter().any(|f| f.contains("target")));
    }

    #[tokio::test]
    async fn test_gitignore_awareness() {
        let tmp = TempDir::new().unwrap();
        let root = setup_test_tree(&tmp);

        let input = GlobInput {
            pattern: "**/*.rs".to_string(),
            path: Some(root.display().to_string()),
            exclude_pattern: None,
        };

        let output = execute(input).await.unwrap();
        let files = extract_paths(&output);

        // Even a broad pattern should not match files inside `target/`
        assert!(!files.iter().any(|f| f.contains("target")));
    }

    #[tokio::test]
    async fn test_exclude_pattern() {
        let tmp = TempDir::new().unwrap();
        let root = setup_test_tree(&tmp);

        let input = GlobInput {
            pattern: "**/*.rs".to_string(),
            path: Some(root.display().to_string()),
            exclude_pattern: Some(vec!["!src/**".to_string()]),
        };

        let output = execute(input).await.unwrap();
        let files = extract_paths(&output);

        assert!(files.iter().any(|f| f.ends_with("a.rs")));
        assert!(files.iter().any(|f| f.ends_with("b.rs")));
        assert!(!files.iter().any(|f| f.contains("src")));
    }

    #[tokio::test]
    async fn test_exclude_pattern_with_bang_prefix() {
        let tmp = TempDir::new().unwrap();
        let root = setup_test_tree(&tmp);

        let input = GlobInput {
            pattern: "**/*.rs".to_string(),
            path: Some(root.display().to_string()),
            exclude_pattern: Some(vec!["!src/**".to_string(), "!a.rs".to_string()]),
        };

        let output = execute(input).await.unwrap();
        let files = extract_paths(&output);

        assert!(!files.iter().any(|f| f.ends_with("a.rs")));
        assert!(!files.iter().any(|f| f.contains("src")));
        assert!(files.iter().any(|f| f.ends_with("b.rs")));
    }

    #[tokio::test]
    async fn test_nonexistent_path() {
        let input = GlobInput {
            pattern: "*.rs".to_string(),
            path: Some("/nonexistent/path/that/does/not/exist".to_string()),
            exclude_pattern: None,
        };

        let output = execute(input).await.unwrap();
        assert!(output.is_error);
        assert!(output.content.contains("not found"));
    }

    #[tokio::test]
    async fn test_empty_results() {
        let tmp = TempDir::new().unwrap();
        let root = setup_test_tree(&tmp);

        let input = GlobInput {
            pattern: "*.xyz".to_string(),
            path: Some(root.display().to_string()),
            exclude_pattern: None,
        };

        let output = execute(input).await.unwrap();
        assert!(!output.is_error);
        let count = output.metadata["count"].as_u64().unwrap();
        assert_eq!(count, 0);
        assert!(output.content.contains("No files found"));
    }

    #[tokio::test]
    async fn test_path_restriction() {
        let tmp = TempDir::new().unwrap();
        let root = setup_test_tree(&tmp);
        let src_dir = root.join("src");

        let input = GlobInput {
            pattern: "*.rs".to_string(),
            path: Some(src_dir.display().to_string()),
            exclude_pattern: None,
        };

        let output = execute(input).await.unwrap();
        assert!(!output.is_error);

        let files = extract_paths(&output);

        // Helper: check that the last path component of `f` equals `name`.
        let filename_is =
            |f: &str, name: &str| -> bool { Path::new(f).file_name().is_some_and(|n| n == name) };

        assert!(files.iter().any(|f| filename_is(f, "mod.rs")));
        assert!(files.iter().any(|f| filename_is(f, "lib.rs")));
        // Root-level files should not appear when restricted to src/
        assert!(!files.iter().any(|f| filename_is(f, "a.rs")));
        assert!(!files.iter().any(|f| filename_is(f, "b.rs")));
    }

    #[tokio::test]
    async fn test_file_size_metadata() {
        let tmp = TempDir::new().unwrap();
        let root = setup_test_tree(&tmp);

        let input = GlobInput {
            pattern: "a.rs".to_string(),
            path: Some(root.display().to_string()),
            exclude_pattern: None,
        };

        let output = execute(input).await.unwrap();
        let files = output.metadata["files"].as_array().unwrap();

        assert_eq!(files.len(), 1);
        let size = files[0]["size"].as_u64().unwrap();
        assert!(size > 0);
    }

    #[tokio::test]
    async fn test_modified_timestamp() {
        let tmp = TempDir::new().unwrap();
        let root = setup_test_tree(&tmp);

        let input = GlobInput {
            pattern: "*.rs".to_string(),
            path: Some(root.display().to_string()),
            exclude_pattern: None,
        };

        let output = execute(input).await.unwrap();
        let files = output.metadata["files"].as_array().unwrap();

        assert!(!files.is_empty());
        // Every result should have a modified timestamp
        for f in files {
            assert!(f["modified"].is_string());
            assert!(!f["modified"].as_str().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn test_sorting_by_modification_time() {
        let tmp = TempDir::new().unwrap();
        let root = setup_test_tree(&tmp);

        // Touch a.rs after b.rs so it has a newer mtime
        std::thread::sleep(std::time::Duration::from_millis(10));
        fs::write(root.join("a.rs"), "// a modified").unwrap();

        let input = GlobInput {
            pattern: "*.rs".to_string(),
            path: Some(root.display().to_string()),
            exclude_pattern: None,
        };

        let output = execute(input).await.unwrap();
        let files = output.metadata["files"].as_array().unwrap();

        // a.rs was touched last, so it should appear first
        assert!(files[0]["path"].as_str().unwrap().ends_with("a.rs"));
    }

    #[tokio::test]
    async fn test_exclude_pattern_without_bang_prefix() {
        let tmp = TempDir::new().unwrap();
        let root = setup_test_tree(&tmp);

        let input = GlobInput {
            pattern: "**/*.rs".to_string(),
            path: Some(root.display().to_string()),
            exclude_pattern: Some(vec!["src/**".to_string()]),
        };

        let output = execute(input).await.unwrap();
        let files = extract_paths(&output);

        assert!(!files.iter().any(|f| f.contains("src")));
        assert!(files.iter().any(|f| f.ends_with("a.rs")));
        assert!(files.iter().any(|f| f.ends_with("b.rs")));
    }

    #[tokio::test]
    async fn test_deserialization_with_optional_fields() {
        // Verify JSON deserialization works when exclude_pattern is omitted
        let json_str = r#"{"pattern": "*.rs", "path": "/tmp"}"#;
        let input: GlobInput = serde_json::from_str(json_str).unwrap();
        assert_eq!(input.pattern, "*.rs");
        assert_eq!(input.path, Some("/tmp".to_string()));
        assert!(input.exclude_pattern.is_none());

        // Verify with all fields present
        let json_str2 = r#"{"pattern": "*.rs", "path": "/tmp", "exclude_pattern": ["!target/**"]}"#;
        let input2: GlobInput = serde_json::from_str(json_str2).unwrap();
        assert_eq!(input2.exclude_pattern.as_deref().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_recursive_pattern_with_path_restriction() {
        let tmp = TempDir::new().unwrap();
        let root = setup_test_tree(&tmp);
        let src_dir = root.join("src");

        let input = GlobInput {
            pattern: "**/*.rs".to_string(),
            path: Some(src_dir.display().to_string()),
            exclude_pattern: None,
        };

        let output = execute(input).await.unwrap();
        assert!(!output.is_error);

        let files = extract_paths(&output);

        // Both src files should appear
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.ends_with("mod.rs")));
        assert!(files.iter().any(|f| f.ends_with("lib.rs")));
    }

    // ======================================================================
    // Property-based tests (proptest)
    // ======================================================================

    proptest::proptest! {
        /// Any valid glob pattern compiles without panic.
        /// We restrict to common safe characters to avoid invalid patterns.
        #[test]
        fn proptest_glob_pattern_no_panic(pattern in "[a-zA-Z0-9_./*?]{0,30}") {
            // Should not panic -- errors are returned as Ok ToolOutput with is_error
            let tmp = TempDir::new().unwrap();
            let root = tmp.path();
            fs::create_dir_all(root.join(".git")).unwrap();
            fs::write(root.join("a.rs"), "").unwrap();

            let rt = tokio::runtime::Runtime::new().unwrap();
            let input = GlobInput {
                pattern: pattern.clone(),
                path: Some(root.display().to_string()),
                exclude_pattern: None,
            };
            // Must complete without panic
            let _ = rt.block_on(execute(input));
        }

        /// Glob matching is deterministic: same pattern on the same directory
        /// always produces the same result (both success or both error).
        #[test]
        fn proptest_glob_deterministic(pattern in "[a-zA-Z0-9_.*]{0,20}") {
            let tmp = TempDir::new().unwrap();
            let root = tmp.path();
            fs::create_dir_all(root.join(".git")).unwrap();
            fs::write(root.join("a.rs"), "").unwrap();
            fs::write(root.join("b.rs"), "").unwrap();

            let rt = tokio::runtime::Runtime::new().unwrap();
            let input1 = GlobInput {
                pattern: pattern.clone(),
                path: Some(root.display().to_string()),
                exclude_pattern: None,
            };
            let input2 = GlobInput {
                pattern: pattern.clone(),
                path: Some(root.display().to_string()),
                exclude_pattern: None,
            };

            let out1 = rt.block_on(execute(input1));
            let out2 = rt.block_on(execute(input2));

            // Both calls must produce the same kind of result
            match (out1, out2) {
                (Ok(o1), Ok(o2)) => {
                    assert_eq!(o1.is_error, o2.is_error);
                    assert_eq!(o1.metadata["count"], o2.metadata["count"]);
                }
                (Err(e1), Err(e2)) => {
                    assert_eq!(e1.to_string(), e2.to_string());
                }
                _ => panic!("determinism violation: one call succeeded, the other failed"),
            }
        }

        /// GlobInput deserialization roundtrips through JSON.
        #[test]
        fn proptest_glob_input_roundtrip(
            pattern in ".{1,30}",
            path in proptest::option::of(".{0,50}"),
        ) {
            let input = GlobInput {
                pattern: pattern.clone(),
                path: path.clone(),
                exclude_pattern: None,
            };
            let json = serde_json::to_string(&input).unwrap();
            let parsed: GlobInput = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed.pattern, pattern);
            assert_eq!(parsed.path, path);
        }
    }
}
