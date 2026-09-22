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
use shannon_tool_interface::FileSystemProvider;
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

/// Maximum number of results returned. Very broad patterns (`**/*`) can
/// match tens of thousands of files; without a cap the tool result floods
/// the model's context. When the cap truncates, the output says so and
/// `metadata` carries `truncated` + `total_matches`.
const MAX_RESULTS: usize = 100;

/// Match options where `*` does NOT match directory separators, matching the
/// conventional glob semantics that Claude Code users expect.
const MATCH_OPTS: glob::MatchOptions = glob::MatchOptions {
    require_literal_separator: true,
    case_sensitive: true,
    require_literal_leading_dot: false,
};

/// Whether a glob pattern addresses outside its base directory. Absolute
/// patterns can never match (matching runs on base-relative paths) and `..`
/// segments only ever produce empty results; both previously surfaced as a
/// silent "No files found".
fn pattern_is_escaping(pattern: &str) -> bool {
    Path::new(pattern).is_absolute() || pattern.split(['/', '\\']).any(|seg| seg == "..")
}

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
fn build_result(fs: &dyn FileSystemProvider, path: &Path) -> Option<GlobResult> {
    let meta = fs.metadata_blocking(path).ok()?;
    let modified = meta.modified.and_then(format_modified_time);
    Some(GlobResult {
        path: path.display().to_string(),
        size: meta.len,
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
    execute_with(input, crate::defaults::fs()).await
}

/// Provider-injected entry point (§4.11): metadata and canonicalization flow
/// through the injected filesystem world.
///
/// §P2-14: takes the provider as an owned `Arc` so the blocking walk can run
/// on tokio's blocking pool (`spawn_blocking` requires `'static`).
pub async fn execute_with(
    input: GlobInput,
    fs: std::sync::Arc<dyn FileSystemProvider>,
) -> Result<ToolOutput, ToolError> {
    let base_path = input.path.clone().unwrap_or_else(|| ".".to_string());
    let base = PathBuf::from(&base_path);
    let excludes = input.exclude_pattern.clone().unwrap_or_default();
    let pattern = input.pattern.clone();

    // A pattern that addresses outside its base directory can never match:
    // matching runs against paths RELATIVE to the base, so absolute or
    // `..`-containing patterns silently returned "No files found" — which
    // reads as "wrong pattern" and invites the model to keep guessing escape
    // depths (dogfood l2 2026-08-23: ../../../../../../ tried at two wrong
    // depths, then the task gave up without producing its answer). Report
    // the confinement so the model can switch to a relative pattern.
    // (Pure string logic — stays on the async side.)
    if pattern_is_escaping(&pattern) {
        return Ok(ToolOutput {
            content: format!(
                "Glob pattern '{pattern}' cannot match here: patterns are \
                 applied to paths relative to the search directory, and this \
                 one addresses outside it. Use a relative pattern (e.g. \
                 'src/**/*.rs') or set the 'path' parameter to a \
                 subdirectory."
            ),
            is_error: true,
            metadata: HashMap::new(),
        });
    }

    // Compile the glob pattern once (pure).
    let glob_pattern = glob::Pattern::new(&pattern)
        .map_err(|e| ToolError::InvalidInput(format!("Invalid glob pattern '{pattern}': {e}")))?;

    // Review §P2-14: canonicalization, existence probes, the .gitignore-aware
    // walk and the per-file metadata reads are all synchronous IO — on a
    // remote world (SSH/Docker) every one of those calls even spins a helper
    // thread. Run the whole blocking section on tokio's blocking pool instead
    // of parking the async worker; the join re-raises both the JoinError and
    // the inner ToolError.
    //
    // `Some(output)` is an advisory early-return (path traversal blocked /
    // directory not found); `None` means the walk produced `results`.
    let job = move || -> Result<(Option<ToolOutput>, Vec<GlobResult>), ToolError> {
        // Prevent path traversal (e.g. "../../etc") by checking components.
        // Confinement compares against the *world's* canonical root: on a
        // remote target both the path and its resolution live on the other
        // machine, and the local cwd is meaningless there.
        for component in base.components() {
            if matches!(component, std::path::Component::ParentDir) {
                // Allow if path resolves within the world's canonical base
                // after canonicalization (symlinks resolved remotely via
                // SFTP).
                if let Ok(canonical) = fs.canonicalize_blocking(&base) {
                    if let Ok(base_canonical) = fs.canonicalize_blocking(Path::new(".")) {
                        if !canonical.starts_with(&base_canonical) {
                            return Ok((
                                Some(ToolOutput {
                                    content: format!(
                                        "Path traversal blocked: '{base_path}' resolves outside project"
                                    ),
                                    is_error: true,
                                    metadata: HashMap::new(),
                                }),
                                Vec::new(),
                            ));
                        }
                    }
                }
                break;
            }
        }

        // If the base directory does not exist, return early with empty
        // results. (Provider-checked so the probe hits the active world's
        // disk.)
        if !fs.exists_blocking(&base) {
            return Ok((
                Some(ToolOutput {
                    content: format!("Directory not found: {base_path}"),
                    is_error: true,
                    metadata: HashMap::new(),
                }),
                Vec::new(),
            ));
        }

        // .gitignore-aware traversal through the injected filesystem world,
        // so matching runs against the same machine the files live on.
        //
        // §P3-13: two hardenings over the previous implementation —
        //   1. results are canonicalized and must stay inside the (canonicalized)
        //      search base, so entries reached through symlinks pointing outside
        //      the workspace are dropped rather than reported;
        //   2. the walk stops as soon as MAX_RESULTS matches are collected
        //      instead of collecting the entire tree first (a giant directory no
        //      longer means a giant traversal).
        let mut results: Vec<GlobResult> = Vec::new();
        let base_canonical = fs.canonicalize_blocking(&base).ok();

        fs.walk_blocking(&base, &mut |entry| {
            // §P3-13 quota guard: once capped, stop the walk. Local worlds
            // honor `false` as a full stop; provider_walk treats it as
            // "don't descend" — the guard keeps the cap on both semantics.
            if results.len() >= MAX_RESULTS {
                return false;
            }
            if !entry.is_dir {
                let path = entry.path.as_path();

                // Match the path *relative* to the base directory. This
                // ensures `*.rs` only matches files in the root, not
                // `src/mod.rs`.
                let rel = match path.strip_prefix(&base) {
                    Ok(r) => r,
                    Err(_) => return true,
                };

                if !glob_pattern.matches_path_with(rel, MATCH_OPTS) {
                    return true;
                }

                // Apply user-supplied exclude patterns.
                if matches_any_exclude(rel, &excludes) {
                    return true;
                }

                // Sandbox check: a path that canonicalizes outside the search
                // base (symlink escape) is dropped, never reported.
                if let Some(base_canonical) = &base_canonical {
                    match fs.canonicalize_blocking(path) {
                        Ok(canonical) if !canonical.starts_with(base_canonical) => {
                            return true;
                        }
                        // Canonicalization failure is provider-specific (e.g.
                        // unsupported on a remote world) — walk-level
                        // containment is the best we have, so keep the entry.
                        Ok(_) | Err(_) => {}
                    }
                }

                if let Some(result) = build_result(fs.as_ref(), path) {
                    results.push(result);
                }
                if results.len() >= MAX_RESULTS {
                    return false; // prune the rest of the walk
                }
            }
            true
        })
        .map_err(|e| ToolError::ExecutionFailed(format!("glob walk failed: {e}")))?;
        Ok((None, results))
    };

    let (early, mut results) = tokio::task::spawn_blocking(job)
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("glob blocking worker failed: {e}")))??;
    if let Some(output) = early {
        return Ok(output);
    }

    sort_results(&mut results);
    // §P3-13: the walk stops at the cap, so the total is unknown past it —
    // `total_matches` mirrors the grep tool's semantics (the collected count,
    // with `truncated: true` flagging that the tree may hold more).
    let total_matches = results.len();
    let truncated = total_matches >= MAX_RESULTS;
    let count = results.len();

    // Build the output content summary.
    let content = if count == 0 {
        format!("No files found matching pattern: {pattern}")
    } else {
        let file_list: Vec<String> = results.iter().map(|r| r.path.clone()).collect();
        let mut content = format!(
            "Found {} files matching pattern: {}\n{}",
            count,
            pattern,
            file_list.join("\n")
        );
        if truncated {
            content.push_str(&format!(
                "\n\n(result cap of {MAX_RESULTS} reached — the tree may hold more matches; \
                 narrow the pattern or set `path` to a subdirectory to see the rest)"
            ));
        }
        content
    };

    // Build structured output.
    let mut metadata = HashMap::new();
    metadata.insert("files".to_string(), json!(results));
    metadata.insert("count".to_string(), json!(count));
    metadata.insert("pattern".to_string(), json!(pattern));
    if truncated {
        metadata.insert("truncated".to_string(), json!(true));
        metadata.insert("total_matches".to_string(), json!(total_matches));
    }

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
#[allow(clippy::unwrap_used)]
mod tests {
    /// Remote-semantics test: the base directory lives only in the injected
    /// world (fake fs), so traversal, existence probe and metadata all come
    /// from the provider — never from the local disk.
    #[tokio::test]
    async fn glob_traverses_through_injected_world() {
        use shannon_tool_interface::{DirEntryInfo, FileMeta, FileSystemProvider};
        use std::io;
        use std::path::{Path, PathBuf};

        struct RemoteFakeFs;

        impl RemoteFakeFs {
            fn entries(root: &Path) -> Vec<DirEntryInfo> {
                vec![
                    DirEntryInfo {
                        path: root.to_path_buf(),
                        len: 0,
                        is_dir: true,
                    },
                    DirEntryInfo {
                        path: root.join("lib.rs"),
                        len: 32,
                        is_dir: false,
                    },
                    DirEntryInfo {
                        path: root.join("notes.txt"),
                        len: 8,
                        is_dir: false,
                    },
                ]
            }
        }

        #[async_trait::async_trait]
        impl FileSystemProvider for RemoteFakeFs {
            async fn read_text(&self, _p: &Path) -> io::Result<String> {
                unimplemented!()
            }
            async fn read_bytes(&self, _p: &Path) -> io::Result<Vec<u8>> {
                unimplemented!()
            }
            async fn metadata(&self, p: &Path) -> io::Result<FileMeta> {
                Ok(self.metadata_blocking(p).unwrap())
            }
            async fn create_dir_all(&self, _p: &Path) -> io::Result<()> {
                unimplemented!()
            }
            async fn write_bytes(&self, _p: &Path, _c: &[u8]) -> io::Result<()> {
                unimplemented!()
            }
            async fn rename(&self, _f: &Path, _t: &Path) -> io::Result<()> {
                unimplemented!()
            }
            async fn canonicalize(&self, p: &Path) -> io::Result<PathBuf> {
                Ok(p.to_path_buf())
            }
            fn read_text_blocking(&self, _p: &Path) -> io::Result<String> {
                unimplemented!()
            }
            fn write_bytes_blocking(&self, _p: &Path, _c: &[u8]) -> io::Result<()> {
                unimplemented!()
            }
            fn create_dir_all_blocking(&self, _p: &Path) -> io::Result<()> {
                unimplemented!()
            }
            fn rename_blocking(&self, _from: &Path, _to: &Path) -> io::Result<()> {
                unimplemented!()
            }
            fn remove_file_blocking(&self, _p: &Path) -> io::Result<()> {
                unimplemented!()
            }
            fn canonicalize_blocking(&self, p: &Path) -> io::Result<PathBuf> {
                Ok(p.to_path_buf())
            }
            fn metadata_blocking(&self, p: &Path) -> io::Result<FileMeta> {
                Ok(FileMeta {
                    len: if p.extension().is_some_and(|e| e == "rs") {
                        32
                    } else {
                        8
                    },
                    is_dir: p.extension().is_none(),
                    modified: None,
                })
            }
            fn read_prefix_blocking(&self, _p: &Path, _m: usize) -> io::Result<Vec<u8>> {
                unimplemented!()
            }
            fn list_dir_blocking(&self, _p: &Path) -> io::Result<Vec<DirEntryInfo>> {
                Ok(Vec::new())
            }
            fn exists_blocking(&self, _p: &Path) -> bool {
                true
            }
            fn walk_blocking(
                &self,
                root: &Path,
                cb: &mut dyn FnMut(&DirEntryInfo) -> bool,
            ) -> io::Result<()> {
                for entry in Self::entries(root) {
                    cb(&entry);
                }
                Ok(())
            }
        }

        let output = execute_with(
            GlobInput {
                pattern: "*.rs".into(),
                path: Some("/remote-host/proj".into()),
                exclude_pattern: None,
            },
            std::sync::Arc::new(RemoteFakeFs) as std::sync::Arc<dyn FileSystemProvider>,
        )
        .await
        .unwrap();

        assert!(!output.is_error);
        assert!(
            output.content.contains("/remote-host/proj/lib.rs"),
            "results must come from the injected world, got: {}",
            output.content
        );
        assert!(!output.content.contains("notes.txt"));
    }

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
    async fn test_escaping_pattern_reports_confinement_not_silence() {
        let tmp = TempDir::new().unwrap();
        let root = setup_test_tree(&tmp);

        // `..` prefix: previously a silent "No files found", which the model
        // read as "wrong pattern" and kept guessing escape depths (dogfood
        // l2 2026-08-23). Must surface as a confinement error instead.
        let input = GlobInput {
            pattern: "../../**/*.rs".to_string(),
            path: Some(root.display().to_string()),
            exclude_pattern: None,
        };
        let output = execute(input).await.unwrap();
        assert!(output.is_error, "escape pattern must be an error");
        assert!(
            output.content.contains("relative pattern"),
            "error must point at the recovery: {}",
            output.content
        );

        // Absolute pattern: matching is base-relative, so it can never match
        // either — same confinement error, not silence.
        let input = GlobInput {
            pattern: format!("{}/*.rs", root.display()),
            path: None,
            exclude_pattern: None,
        };
        let output = execute(input).await.unwrap();
        assert!(output.is_error, "absolute pattern must be an error");

        // Sanity: ordinary relative patterns are unaffected.
        let input = GlobInput {
            pattern: "*.rs".to_string(),
            path: Some(root.display().to_string()),
            exclude_pattern: None,
        };
        let output = execute(input).await.unwrap();
        assert!(!output.is_error);
        assert_eq!(extract_paths(&output).len(), 2);
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
    async fn test_results_capped_at_100_with_truncation_metadata() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".git")).unwrap();
        // Well past the cap.
        for i in 0..130 {
            fs::write(root.join(format!("gen_{i:03}.rs")), "").unwrap();
        }

        let input = GlobInput {
            pattern: "*.rs".to_string(),
            path: Some(root.display().to_string()),
            exclude_pattern: None,
        };

        let output = execute(input).await.unwrap();
        assert!(!output.is_error);

        // Hard cap on returned files…
        let files = output.metadata["files"].as_array().unwrap();
        assert_eq!(files.len(), 100);
        assert_eq!(output.metadata["count"], 100);
        // …truncation surfaced in metadata (§P3-13: the walk stops at the
        // cap, so the exact total is unknown — grep-style collected count)…
        assert_eq!(output.metadata["truncated"], true);
        assert_eq!(output.metadata["total_matches"], 100);
        // …and in the model-facing content.
        assert!(
            output.content.contains("result cap of 100"),
            "content must say the list was truncated, got: {}",
            output.content
        );
        assert!(output.content.contains("narrow the pattern"));
    }

    /// §P3-13: an entry that canonicalizes outside the search base (symlink
    /// escape) must be dropped, never reported.
    #[tokio::test]
    async fn glob_drops_entries_canonicalizing_outside_base() {
        use shannon_tool_interface::{DirEntryInfo, FileMeta, FileSystemProvider};
        use std::io;
        use std::path::{Path, PathBuf};

        struct EscapeFs;

        /// /remote/proj/evil.rs is a symlink to /etc/outside.rs — its
        /// canonical form lives outside the search base.
        fn canonical_of(p: &Path) -> PathBuf {
            if p.ends_with("evil.rs") {
                PathBuf::from("/etc/outside.rs")
            } else {
                p.to_path_buf()
            }
        }

        fn entry(root: &Path, name: &str, is_dir: bool) -> DirEntryInfo {
            DirEntryInfo {
                path: root.join(name),
                len: 16,
                is_dir,
            }
        }

        #[async_trait::async_trait]
        impl FileSystemProvider for EscapeFs {
            async fn read_text(&self, _p: &Path) -> io::Result<String> {
                unimplemented!()
            }
            async fn read_bytes(&self, _p: &Path) -> io::Result<Vec<u8>> {
                unimplemented!()
            }
            async fn metadata(&self, p: &Path) -> io::Result<FileMeta> {
                Ok(self.metadata_blocking(p).unwrap())
            }
            async fn create_dir_all(&self, _p: &Path) -> io::Result<()> {
                unimplemented!()
            }
            async fn write_bytes(&self, _p: &Path, _c: &[u8]) -> io::Result<()> {
                unimplemented!()
            }
            async fn rename(&self, _f: &Path, _t: &Path) -> io::Result<()> {
                unimplemented!()
            }
            async fn canonicalize(&self, p: &Path) -> io::Result<PathBuf> {
                Ok(canonical_of(p))
            }
            fn read_text_blocking(&self, _p: &Path) -> io::Result<String> {
                unimplemented!()
            }
            fn write_bytes_blocking(&self, _p: &Path, _c: &[u8]) -> io::Result<()> {
                unimplemented!()
            }
            fn create_dir_all_blocking(&self, _p: &Path) -> io::Result<()> {
                unimplemented!()
            }
            fn rename_blocking(&self, _from: &Path, _to: &Path) -> io::Result<()> {
                unimplemented!()
            }
            fn remove_file_blocking(&self, _p: &Path) -> io::Result<()> {
                unimplemented!()
            }
            fn canonicalize_blocking(&self, p: &Path) -> io::Result<PathBuf> {
                Ok(canonical_of(p))
            }
            fn metadata_blocking(&self, _p: &Path) -> io::Result<FileMeta> {
                Ok(FileMeta {
                    len: 16,
                    is_dir: false,
                    modified: None,
                })
            }
            fn read_prefix_blocking(&self, _p: &Path, _m: usize) -> io::Result<Vec<u8>> {
                unimplemented!()
            }
            fn list_dir_blocking(&self, _p: &Path) -> io::Result<Vec<DirEntryInfo>> {
                Ok(Vec::new())
            }
            fn exists_blocking(&self, _p: &Path) -> bool {
                true
            }
            fn walk_blocking(
                &self,
                root: &Path,
                cb: &mut dyn FnMut(&DirEntryInfo) -> bool,
            ) -> io::Result<()> {
                for e in [
                    entry(root, "", true),
                    entry(root, "lib.rs", false),
                    // The symlink escape: walked (walkers may not resolve
                    // symlinks), but canonicalizes outside the base.
                    entry(root, "evil.rs", false),
                ] {
                    cb(&e);
                }
                Ok(())
            }
        }

        let output = execute_with(
            GlobInput {
                pattern: "*.rs".into(),
                path: Some("/remote/proj".into()),
                exclude_pattern: None,
            },
            std::sync::Arc::new(EscapeFs) as std::sync::Arc<dyn FileSystemProvider>,
        )
        .await
        .unwrap();

        assert!(!output.is_error);
        let paths = extract_paths(&output);
        assert!(
            paths.iter().any(|p| p.ends_with("lib.rs")),
            "in-base entry kept: {paths:?}"
        );
        assert!(
            !paths.iter().any(|p| p.ends_with("evil.rs")),
            "symlink-escaped entry must be dropped: {paths:?}"
        );
    }

    /// §P3-13: the walk must stop once the result cap is reached instead of
    /// traversing the entire tree (the old collect-all-then-truncate).
    #[tokio::test]
    async fn glob_stops_walking_once_cap_reached() {
        use shannon_tool_interface::{DirEntryInfo, FileMeta, FileSystemProvider};
        use std::io;
        use std::path::{Path, PathBuf};
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct CountingFs {
            visited: Arc<AtomicUsize>,
        }

        impl CountingFs {
            fn entries(root: &Path) -> Vec<DirEntryInfo> {
                (0..130)
                    .map(|i| DirEntryInfo {
                        path: root.join(format!("gen_{i:03}.rs")),
                        len: 1,
                        is_dir: false,
                    })
                    .collect()
            }
        }

        #[async_trait::async_trait]
        impl FileSystemProvider for CountingFs {
            async fn read_text(&self, _p: &Path) -> io::Result<String> {
                unimplemented!()
            }
            async fn read_bytes(&self, _p: &Path) -> io::Result<Vec<u8>> {
                unimplemented!()
            }
            async fn metadata(&self, p: &Path) -> io::Result<FileMeta> {
                Ok(self.metadata_blocking(p).unwrap())
            }
            async fn create_dir_all(&self, _p: &Path) -> io::Result<()> {
                unimplemented!()
            }
            async fn write_bytes(&self, _p: &Path, _c: &[u8]) -> io::Result<()> {
                unimplemented!()
            }
            async fn rename(&self, _f: &Path, _t: &Path) -> io::Result<()> {
                unimplemented!()
            }
            async fn canonicalize(&self, p: &Path) -> io::Result<PathBuf> {
                Ok(p.to_path_buf())
            }
            fn read_text_blocking(&self, _p: &Path) -> io::Result<String> {
                unimplemented!()
            }
            fn write_bytes_blocking(&self, _p: &Path, _c: &[u8]) -> io::Result<()> {
                unimplemented!()
            }
            fn create_dir_all_blocking(&self, _p: &Path) -> io::Result<()> {
                unimplemented!()
            }
            fn rename_blocking(&self, _from: &Path, _to: &Path) -> io::Result<()> {
                unimplemented!()
            }
            fn remove_file_blocking(&self, _p: &Path) -> io::Result<()> {
                unimplemented!()
            }
            fn canonicalize_blocking(&self, p: &Path) -> io::Result<PathBuf> {
                Ok(p.to_path_buf())
            }
            fn metadata_blocking(&self, _p: &Path) -> io::Result<FileMeta> {
                Ok(FileMeta {
                    len: 1,
                    is_dir: false,
                    modified: None,
                })
            }
            fn read_prefix_blocking(&self, _p: &Path, _m: usize) -> io::Result<Vec<u8>> {
                unimplemented!()
            }
            fn list_dir_blocking(&self, _p: &Path) -> io::Result<Vec<DirEntryInfo>> {
                Ok(Vec::new())
            }
            fn exists_blocking(&self, _p: &Path) -> bool {
                true
            }
            fn walk_blocking(
                &self,
                root: &Path,
                cb: &mut dyn FnMut(&DirEntryInfo) -> bool,
            ) -> io::Result<()> {
                // Honor the callback's false like a real walker prunes.
                for e in Self::entries(root) {
                    self.visited.fetch_add(1, Ordering::SeqCst);
                    if !cb(&e) {
                        break;
                    }
                }
                Ok(())
            }
        }

        let tmp = TempDir::new().unwrap();
        let visited = Arc::new(AtomicUsize::new(0));
        let output = execute_with(
            GlobInput {
                pattern: "*.rs".into(),
                path: Some(tmp.path().display().to_string()),
                exclude_pattern: None,
            },
            std::sync::Arc::new(CountingFs {
                visited: visited.clone(),
            }) as std::sync::Arc<dyn FileSystemProvider>,
        )
        .await
        .unwrap();

        assert_eq!(output.metadata["count"], 100);
        assert_eq!(output.metadata["truncated"], true);
        let visited_count = visited.load(Ordering::SeqCst);
        assert!(
            visited_count < 130,
            "walk must stop at the cap instead of visiting all 130 entries (visited {visited_count})"
        );
        assert_eq!(visited_count, 100, "stop exactly at the cap");
    }

    #[tokio::test]
    async fn test_results_under_cap_have_no_truncation_markers() {
        let tmp = TempDir::new().unwrap();
        let root = setup_test_tree(&tmp);

        let input = GlobInput {
            pattern: "**/*.rs".to_string(),
            path: Some(root.display().to_string()),
            exclude_pattern: None,
        };
        let output = execute(input).await.unwrap();
        assert!(!output.metadata.contains_key("truncated"));
        assert!(!output.metadata.contains_key("total_matches"));
        assert!(!output.content.contains("narrow the pattern"));
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

    /// An escaping pattern (`..`) deterministically returns the same
    /// error-shaped output with no "count" key — pins the contract the
    /// determinism proptest relies on (proptest-regressions seeds).
    #[tokio::test]
    async fn glob_escaping_pattern_error_contract_is_stable() {
        let tmp = TempDir::new().unwrap();
        let input = GlobInput {
            pattern: "..".to_string(),
            path: Some(tmp.path().display().to_string()),
            exclude_pattern: None,
        };
        let out = execute(input.clone())
            .await
            .expect("error-shaped Ok output");
        assert!(out.is_error);
        assert!(out.content.contains("cannot match here"));
        assert!(!out.metadata.contains_key("count"));

        let again = execute(input).await.expect("second call");
        assert_eq!(out.is_error, again.is_error);
        assert_eq!(out.content, again.content);
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
                    assert_eq!(o1.content, o2.content);
                    // Error-shaped outputs (traversal blocked, escaping pattern,
                    // missing directory) carry no "count" metadata by contract —
                    // only compare it when both calls succeeded.
                    if !o1.is_error {
                        assert_eq!(
                            o1.metadata.get("count"),
                            o2.metadata.get("count"),
                            "count differs between identical calls"
                        );
                    }
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
