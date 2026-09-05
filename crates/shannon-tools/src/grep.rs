//! Grep tool implementation - content search across files
//!
//! Provides ripgrep-like search capabilities using the `regex` and `ignore` crates.
//! Supports pattern matching, include/exclude globs, context lines, and multiple output modes.

use crate::file::sandbox::PathSandbox;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use shannon_core::tools::ToolError;
use shannon_core::{Tool, ToolOutput, ToolResult};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Maximum number of results returned by default
const DEFAULT_MAX_RESULTS: usize = 1000;
/// Maximum allowed results to prevent resource exhaustion
const MAX_ALLOWED_RESULTS: usize = 10000;
/// Maximum context lines per side
const MAX_CONTEXT_LINES: usize = 100;

/// Number of bytes to check for binary detection
const BINARY_CHECK_BYTES: usize = 8192;

/// Output format for search results
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum GrepOutputMode {
    /// Show matching lines with file paths and line numbers (default)
    #[default]
    Content,
    /// Show only file paths containing matches
    Files,
    /// Show match count per file
    Count,
}

/// Input parameters for the grep tool
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GrepInput {
    /// Regex pattern to search for
    pub pattern: String,

    /// Directory or file to search in (default: current directory)
    pub path: Option<String>,

    /// Glob pattern to include files (e.g., "*.rs")
    pub include: Option<String>,

    /// Glob pattern to exclude files (e.g., "target/**")
    pub exclude: Option<String>,

    /// Case insensitive search
    pub case_insensitive: Option<bool>,

    /// Show line numbers (default: true)
    pub line_number: Option<bool>,

    /// Context lines before match
    pub context_before: Option<usize>,

    /// Context lines after match
    pub context_after: Option<usize>,

    /// Maximum number of matches to return
    pub max_results: Option<usize>,

    /// Output mode: content, files, or count
    pub output_mode: Option<GrepOutputMode>,
}

/// A single line match within a file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepLineMatch {
    /// 1-based line number of the match
    pub line_number: usize,
    /// The matching line content
    pub line: String,
    /// Lines before the match (for context)
    pub context_before: Vec<String>,
    /// Lines after the match (for context)
    pub context_after: Vec<String>,
}

/// All matches found within a single file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepFileMatch {
    /// File path (relative to search root)
    pub file: String,
    /// Individual line matches
    pub matches: Vec<GrepLineMatch>,
    /// Total match count in this file
    pub match_count: usize,
}

/// GrepTool - search file contents using regex patterns
pub struct GrepTool {
    sandbox: PathSandbox,
    /// Filesystem world backing binary sniffing and line reads (§4.11).
    fs: std::sync::Arc<dyn shannon_tool_interface::FileSystemProvider>,
}

impl GrepTool {
    pub fn new() -> Self {
        Self {
            // Default sandbox: blocks system paths but allows any project dir.
            // with_sandbox() is used when a specific project dir is known.
            sandbox: PathSandbox::with_config(crate::file::sandbox::SandboxConfig {
                allowed_roots: vec![],
                denied_patterns: crate::file::sandbox::SandboxConfig::default_denied_patterns(),
                strict_mode: false,
            }),
            fs: crate::defaults::fs(),
        }
    }

    /// Create a GrepTool with a custom sandbox configuration.
    pub fn with_sandbox(sandbox: PathSandbox) -> Self {
        Self {
            sandbox,
            fs: crate::defaults::fs(),
        }
    }

    /// Inject a filesystem world override (sandbox/remote assemblies).
    pub fn with_fs(
        mut self,
        fs: std::sync::Arc<dyn shannon_tool_interface::FileSystemProvider>,
    ) -> Self {
        self.fs = fs;
        self
    }

    /// Check if a file appears to be binary by looking for null bytes.
    /// Reads the sniff buffer through the injected filesystem world.
    fn is_binary(&self, path: &Path) -> bool {
        match self.fs.read_prefix_blocking(path, BINARY_CHECK_BYTES) {
            Ok(buf) => buf.contains(&0),
            Err(_) => true, // Treat unreadable files as binary
        }
    }

    /// Read lines from a file, returning a vector of (line_number, line_content)
    fn read_file_lines(&self, path: &Path) -> std::io::Result<Vec<(usize, String)>> {
        // Skip files that are too large to avoid OOM on huge log/data files
        const MAX_FILE_SIZE: u64 = 50 * 1024 * 1024; // 50 MB
        if let Ok(meta) = self.fs.metadata_blocking(path) {
            if meta.len > MAX_FILE_SIZE {
                return Ok(Vec::new());
            }
        }
        let content = self.fs.read_text_blocking(path)?;
        Ok(content
            .lines()
            .enumerate()
            .map(|(i, line)| (i + 1, line.to_string()))
            .collect())
    }

    /// Search a single file for pattern matches
    fn search_file(
        &self,
        path: &Path,
        regex: &regex::Regex,
        _show_line_numbers: bool,
        context_before: usize,
        context_after: usize,
    ) -> Option<GrepFileMatch> {
        if self.is_binary(path) {
            return None;
        }

        let lines = match self.read_file_lines(path) {
            Ok(lines) => lines,
            Err(_) => return None,
        };

        let mut matches = Vec::new();
        let total_lines = lines.len();

        for (idx, (line_num, line)) in lines.iter().enumerate() {
            if regex.is_match(line) {
                // Collect context before
                let ctx_before_start = idx.saturating_sub(context_before);
                let context_before_lines: Vec<String> = lines[ctx_before_start..idx]
                    .iter()
                    .map(|(_, l)| l.clone())
                    .collect();

                // Collect context after
                let ctx_after_end = (idx + 1 + context_after).min(total_lines);
                let context_after_lines: Vec<String> = lines[idx + 1..ctx_after_end]
                    .iter()
                    .map(|(_, l)| l.clone())
                    .collect();

                matches.push(GrepLineMatch {
                    line_number: *line_num,
                    line: line.clone(),
                    context_before: context_before_lines,
                    context_after: context_after_lines,
                });
            }
        }

        if matches.is_empty() {
            None
        } else {
            let match_count = matches.len();
            // A3: echo the path in the command sandbox's view (e.g.
            // `/workspace/src/x.rs`) so the model can feed it straight into
            // a sandboxed Bash command. Identity when output aliasing is off.
            let display_path = self.sandbox.alias_display_path(path);
            Some(GrepFileMatch {
                file: display_path,
                matches,
                match_count,
            })
        }
    }

    /// Format results in "content" mode (default)
    fn format_content_output(results: &[GrepFileMatch], show_line_numbers: bool) -> String {
        let mut output = String::new();
        for file_match in results {
            if !output.is_empty() {
                output.push('\n');
            }
            // Always show file path header
            output.push_str(&file_match.file);
            output.push('\n');
            for line_match in &file_match.matches {
                // Context before lines
                for ctx_line in &line_match.context_before {
                    output.push('-');
                    if show_line_numbers {
                        // We don't have line numbers for context lines in this simplified impl
                        output.push_str("  ");
                    }
                    output.push_str(ctx_line);
                    output.push('\n');
                }
                // The matching line
                output.push(':');
                if show_line_numbers {
                    output.push_str(&format!("{}:", line_match.line_number));
                }
                output.push_str(&line_match.line);
                output.push('\n');
                // Context after lines
                for ctx_line in &line_match.context_after {
                    output.push('-');
                    if show_line_numbers {
                        output.push_str("  ");
                    }
                    output.push_str(ctx_line);
                    output.push('\n');
                }
            }
        }
        output.trim_end().to_string()
    }

    /// Format results in "files" mode
    fn format_files_output(results: &[GrepFileMatch]) -> String {
        results
            .iter()
            .map(|r| r.file.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Format results in "count" mode
    fn format_count_output(results: &[GrepFileMatch]) -> String {
        results
            .iter()
            .map(|r| format!("{}:{}", r.file, r.match_count))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "Grep"
    }

    fn description(&self) -> &str {
        "A powerful search tool built on ripgrep. Searches file contents using regex patterns. \
        Supports include/exclude globs, context lines, case-insensitive matching, and multiple output modes."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "The regular expression pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "Directory or file to search in. Defaults to current directory."
                },
                "include": {
                    "type": "string",
                    "description": "Glob pattern to include files (e.g., \"*.rs\")"
                },
                "exclude": {
                    "type": "string",
                    "description": "Glob pattern to exclude files (e.g., \"target/**\")"
                },
                "case_insensitive": {
                    "type": "boolean",
                    "description": "Perform case-insensitive search"
                },
                "line_number": {
                    "type": "boolean",
                    "description": "Show line numbers (default: true)"
                },
                "context_before": {
                    "type": "integer",
                    "description": "Number of context lines before each match"
                },
                "context_after": {
                    "type": "integer",
                    "description": "Number of context lines after each match"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of matches to return (default: 1000)"
                },
                "output_mode": {
                    "type": "string",
                    "enum": ["content", "files", "count"],
                    "description": "Output mode: content (matching lines), files (filenames only), count (match counts per file)"
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        // Parse input
        let grep_input: GrepInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid grep input: {e}")))?;

        if grep_input.pattern.is_empty() {
            return Err(ToolError::InvalidInput(
                "Pattern cannot be empty".to_string(),
            ));
        }

        // Compile regex
        let case_insensitive = grep_input.case_insensitive.unwrap_or(false);
        let regex = regex::RegexBuilder::new(&grep_input.pattern)
            .case_insensitive(case_insensitive)
            .build()
            .map_err(|e| {
                ToolError::InvalidInput(format!(
                    "Invalid regex pattern '{}': {}",
                    grep_input.pattern, e
                ))
            })?;

        // Determine search path
        let search_path = grep_input.path.as_deref().unwrap_or(".");
        let search_root = PathBuf::from(search_path);

        // Validate search path through sandbox
        let canonical_root = self
            .sandbox
            .validate(&search_root)
            .await
            .map_err(|e| ToolError::InvalidInput(format!("Path sandbox: {e}")))?;

        // Existence is provider-checked: on a remote world the search root
        // lives on the target, so `Path::exists` would probe the wrong disk.
        // When the raw spelling is missing but the sandbox resolved it (bind
        // alias addressing like `/workspace/src`), walk the canonical host
        // root — the companion of the output aliasing in `search_file`.
        let search_root = if self.fs.exists_blocking(&search_root) {
            search_root
        } else if self.fs.exists_blocking(&canonical_root) {
            canonical_root
        } else {
            return Err(ToolError::ExecutionFailed(format!(
                "Path does not exist: {search_path}"
            )));
        };

        // Traversal, gitignore handling and content reads all follow the
        // injected filesystem world (local by default, SSH/Docker under a
        // remote target). Include/exclude filtering stays in the callback.
        let mut all_matches: Vec<GrepFileMatch> = Vec::new();
        let mut total_matches: usize = 0;
        let mut quota_reached = false;

        let show_line_numbers = grep_input.line_number.unwrap_or(true);
        let context_before = grep_input
            .context_before
            .unwrap_or(0)
            .min(MAX_CONTEXT_LINES);
        let context_after = grep_input.context_after.unwrap_or(0).min(MAX_CONTEXT_LINES);
        let max_results = grep_input
            .max_results
            .unwrap_or(DEFAULT_MAX_RESULTS)
            .min(MAX_ALLOWED_RESULTS);
        let output_mode = grep_input.output_mode.unwrap_or_default();

        self.fs
            .walk_blocking(&search_root, &mut |entry| {
                if quota_reached {
                    return false;
                }
                let path = &entry.path;

                // Skip directories
                if entry.is_dir {
                    return true;
                }

                // Skip files that don't match include pattern (for simple extension matching)
                if let Some(include) = &grep_input.include {
                    if !path_matches_glob(path, include) {
                        return true;
                    }
                }

                // Skip files that match exclude pattern
                if let Some(exclude) = &grep_input.exclude {
                    if path_matches_glob(path, exclude) {
                        return true;
                    }
                }

                if let Some(mut file_match) = self.search_file(
                    path,
                    &regex,
                    show_line_numbers,
                    context_before,
                    context_after,
                ) {
                    // Truncate matches if we'd exceed max_results
                    let remaining = max_results - total_matches;
                    if file_match.matches.len() > remaining {
                        file_match.matches.truncate(remaining);
                        file_match.match_count = file_match.matches.len();
                    }
                    total_matches += file_match.match_count;
                    all_matches.push(file_match);
                }
                if total_matches >= max_results {
                    quota_reached = true;
                    return false; // prune the rest of the walk
                }
                true
            })
            .map_err(|e| ToolError::ExecutionFailed(format!("walk failed: {e}")))?;

        // Format output based on mode
        let content = match output_mode {
            GrepOutputMode::Files => Self::format_files_output(&all_matches),
            GrepOutputMode::Count => Self::format_count_output(&all_matches),
            GrepOutputMode::Content => Self::format_content_output(&all_matches, show_line_numbers),
        };

        let total_files = all_matches.len();
        let is_error = false;

        Ok(ToolOutput {
            content,
            is_error,
            metadata: {
                let mut map = HashMap::new();
                map.insert("total_files".to_string(), json!(total_files));
                map.insert("total_matches".to_string(), json!(total_matches));
                map.insert(
                    "output_mode".to_string(),
                    json!(format!("{output_mode:?}").to_lowercase()),
                );
                map.insert("truncated".to_string(), json!(total_matches >= max_results));
                map
            },
        })
    }

    fn category(&self) -> &str {
        "search"
    }
    fn is_read_only(&self) -> bool {
        true
    }
}

impl Default for GrepTool {
    fn default() -> Self {
        Self::new()
    }
}

/// Simple glob matching for file paths (extension-based and wildcard patterns)
fn path_matches_glob(path: &Path, pattern: &str) -> bool {
    let path_str = path.to_string_lossy();

    // Handle simple extension patterns like "*.rs"
    if pattern.starts_with("*.") && !pattern.contains('/') {
        let ext = &pattern[2..]; // "rs" (skip "*.")
        return path
            .extension()
            .map(|e| e.to_string_lossy() == ext)
            .unwrap_or(false);
    }

    // Handle ** patterns like "src/**" or "target/**/*.log"
    if pattern.contains("**") {
        // Extract the directory prefix (e.g., "src" from "src/**")
        let dir_part = pattern.split("**").next().unwrap_or("");
        let dir_part = dir_part.trim_end_matches('/').trim_end_matches('\\');

        if !dir_part.is_empty() {
            // Check if the path is under this directory
            let sep_str = std::path::MAIN_SEPARATOR.to_string();
            let dir_with_sep = format!("{}{}", dir_part, '/');
            let dir_with_sep_native = format!("{dir_part}{sep_str}");
            return path_str.contains(&dir_with_sep) || path_str.contains(&dir_with_sep_native);
        }

        // "**" alone matches everything
        return true;
    }

    // Check if the pattern has a directory component
    if pattern.contains('/') || pattern.contains('\\') {
        let sep_str = std::path::MAIN_SEPARATOR.to_string();
        // Check if the path ends with the pattern (for "src/file.rs" style patterns)
        return path_str.ends_with(pattern) || path_str.ends_with(&pattern.replace('/', &sep_str));
    }

    // Fallback: check if filename matches pattern
    if let Some(file_name) = path.file_name() {
        let name = file_name.to_string_lossy();
        if let Some(suffix) = pattern.strip_prefix('*') {
            return name.ends_with(suffix);
        }
        return name == pattern;
    }

    false
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    /// Remote-semantics test: paths that do NOT exist on the local disk are
    /// searched through the injected world (fake fs), proving traversal and
    /// content reads follow the provider rather than the local disk.
    #[tokio::test]
    async fn grep_traverses_through_injected_world() {
        use shannon_tool_interface::{DirEntryInfo, FileMeta, FileSystemProvider};
        use std::io;
        use std::path::{Path, PathBuf};
        use std::sync::Arc;

        struct RemoteFakeFs;

        #[async_trait]
        impl FileSystemProvider for RemoteFakeFs {
            async fn read_text(&self, _path: &Path) -> io::Result<String> {
                unimplemented!()
            }
            async fn read_bytes(&self, _p: &Path) -> io::Result<Vec<u8>> {
                unimplemented!()
            }
            async fn metadata(&self, _p: &Path) -> io::Result<FileMeta> {
                unimplemented!()
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
                Ok("alpha needle beta\nnothing here\nthird needle line".to_string())
            }
            fn write_bytes_blocking(&self, _p: &Path, _c: &[u8]) -> io::Result<()> {
                unimplemented!()
            }
            fn create_dir_all_blocking(&self, _p: &Path) -> io::Result<()> {
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
                    len: 64,
                    is_dir: false,
                    modified: None,
                })
            }
            fn read_prefix_blocking(&self, _p: &Path, _m: usize) -> io::Result<Vec<u8>> {
                Ok(b"alpha needle beta\nnothing here\nthird needle line".to_vec())
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
                // Serve two non-local files under the remote root.
                cb(&DirEntryInfo {
                    path: root.to_path_buf(),
                    len: 0,
                    is_dir: true,
                });
                cb(&DirEntryInfo {
                    path: root.join("a.rs"),
                    len: 64,
                    is_dir: false,
                });
                cb(&DirEntryInfo {
                    path: root.join("b.rs"),
                    len: 64,
                    is_dir: false,
                });
                Ok(())
            }
        }

        let fs: Arc<dyn FileSystemProvider> = Arc::new(RemoteFakeFs);
        // Wire the SAME world into the sandbox's TOCTOU canonicalization,
        // exactly as register_all_tools does for remote assemblies.
        let sandbox =
            crate::file::sandbox::PathSandbox::with_config(crate::file::sandbox::SandboxConfig {
                allowed_roots: vec![PathBuf::from("/remote-host/proj")],
                denied_patterns: crate::file::sandbox::SandboxConfig::default_denied_patterns(),
                strict_mode: true,
            })
            .with_fs_provider(fs.clone());
        let tool = GrepTool::with_sandbox(sandbox).with_fs(fs);

        let output = Tool::execute(
            &tool,
            serde_json::json!({ "pattern": "needle", "path": "/remote-host/proj" }),
        )
        .await
        .unwrap();

        assert!(!output.is_error);
        assert!(
            output.content.contains("/remote-host/proj/a.rs"),
            "matches must come from the injected world, got: {}",
            output.content
        );
        assert!(output.content.contains("/remote-host/proj/b.rs"));
        assert_eq!(
            output.metadata.get("total_matches"),
            Some(&serde_json::json!(4))
        );
    }

    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // ── A3: sandbox-visible output paths (docs/eval-findings-2026-09-glm.md) ──

    /// Sandbox wired like the project registration: project root + temp root,
    /// output aliasing enabled.
    fn alias_output_sandbox(root: &Path) -> PathSandbox {
        PathSandbox::with_config(crate::file::sandbox::SandboxConfig {
            allowed_roots: crate::file::sandbox::SandboxConfig::command_aligned_roots(root),
            denied_patterns: crate::file::sandbox::SandboxConfig::default_denied_patterns(),
            strict_mode: true,
        })
        .with_bind_alias_output(true)
    }

    fn alias_grep_fixture() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/a.rs"), "needle here\n").unwrap();
        dir
    }

    #[tokio::test]
    async fn grep_alias_sandbox_echoes_workspace_paths() {
        let dir = alias_grep_fixture();
        let tool = GrepTool::with_sandbox(alias_output_sandbox(dir.path()));
        let host_str = dir.path().to_string_lossy().to_string();

        let output = Tool::execute(
            &tool,
            json!({ "pattern": "needle", "path": dir.path().to_string_lossy() }),
        )
        .await
        .unwrap();

        assert!(
            output.content.contains("/workspace/src/a.rs"),
            "output must show sandbox-visible paths, got: {}",
            output.content
        );
        assert!(
            !output.content.contains(&host_str),
            "output must not leak the host path, got: {}",
            output.content
        );
    }

    #[tokio::test]
    async fn grep_accepts_alias_search_root() {
        if std::path::Path::new("/workspace").exists() {
            return; // host really has /workspace — alias addressing is ambiguous
        }
        let dir = alias_grep_fixture();
        let tool = GrepTool::with_sandbox(alias_output_sandbox(dir.path()));

        let output = Tool::execute(
            &tool,
            json!({ "pattern": "needle", "path": "/workspace" }),
        )
        .await
        .unwrap();

        assert!(
            output.content.contains("/workspace/src/a.rs"),
            "grep must walk the alias-resolved root, got: {}",
            output.content
        );
    }

    /// Helper to create a temp directory with test files
    fn setup_test_files() -> TempDir {
        let dir = TempDir::new().unwrap();
        let base = dir.path();

        // Create a Rust source file
        fs::write(
            base.join("main.rs"),
            "fn main() {\n    println!(\"Hello, world!\");\n    let x = 42;\n    println!(\"x = {}\", x);\n}\n",
        )
        .unwrap();

        // Create a JavaScript file
        fs::write(
            base.join("app.js"),
            "const app = require('express')();\napp.get('/', (req, res) => {\n  res.send('Hello');\n});\napp.listen(3000);\n",
        )
        .unwrap();

        // Create a subdirectory with more files
        fs::create_dir_all(base.join("src")).unwrap();
        fs::write(
            base.join("src/lib.rs"),
            "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n\npub fn multiply(a: i32, b: i32) -> i32 {\n    a * b\n}\n",
        )
        .unwrap();

        fs::write(
            base.join("src/utils.rs"),
            "pub fn greet(name: &str) -> String {\n    format!(\"Hello, {}!\", name)\n}\n",
        )
        .unwrap();

        dir
    }

    fn create_grep_input(pattern: &str, path: Option<&str>) -> Value {
        let mut input = json!({
            "pattern": pattern,
        });
        if let Some(p) = path {
            input["path"] = json!(p);
        }
        input
    }

    // === Basic regex search ===

    #[tokio::test]
    async fn test_basic_regex_search() {
        let dir = setup_test_files();
        let tool = GrepTool::new();
        let input = create_grep_input("println", Some(dir.path().to_str().unwrap()));

        let result = tool.execute(input).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("println"));
        assert!(
            result
                .metadata
                .get("total_matches")
                .unwrap()
                .as_u64()
                .unwrap()
                >= 2
        );
    }

    #[tokio::test]
    async fn test_search_finds_correct_files() {
        let dir = setup_test_files();
        let tool = GrepTool::new();
        let input = create_grep_input("fn main", Some(dir.path().to_str().unwrap()));

        let result = tool.execute(input).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("main.rs"));
    }

    // === Case insensitive search ===

    #[tokio::test]
    async fn test_case_sensitive_search() {
        let dir = setup_test_files();
        let tool = GrepTool::new();
        let mut input = create_grep_input("Hello", Some(dir.path().to_str().unwrap()));
        input["case_insensitive"] = json!(false);

        let result = tool.execute(input).await.unwrap();
        assert!(!result.is_error);
        // "Hello" appears as "Hello, world!" and "Hello" in JS file
        assert!(
            result
                .metadata
                .get("total_matches")
                .unwrap()
                .as_u64()
                .unwrap()
                >= 2
        );
    }

    #[tokio::test]
    async fn test_case_insensitive_search() {
        let dir = setup_test_files();
        let tool = GrepTool::new();
        let mut input = create_grep_input("hello", Some(dir.path().to_str().unwrap()));
        input["case_insensitive"] = json!(true);

        let result = tool.execute(input).await.unwrap();
        assert!(!result.is_error);
        // With case insensitive, should find "Hello, world!", "Hello", and "Hello, {name}!"
        assert!(
            result
                .metadata
                .get("total_matches")
                .unwrap()
                .as_u64()
                .unwrap()
                >= 3
        );
    }

    // === Include/exclude patterns ===

    #[tokio::test]
    async fn test_include_pattern() {
        let dir = setup_test_files();
        let tool = GrepTool::new();
        let mut input = create_grep_input("fn", Some(dir.path().to_str().unwrap()));
        input["include"] = json!("*.rs");

        let result = tool.execute(input).await.unwrap();
        assert!(!result.is_error);
        // Should only match .rs files, not .js
        let content = &result.content;
        assert!(!content.contains("app.js"));
        assert!(
            result
                .metadata
                .get("total_matches")
                .unwrap()
                .as_u64()
                .unwrap()
                >= 4
        );
    }

    #[tokio::test]
    async fn test_include_pattern_js_only() {
        let dir = setup_test_files();
        let tool = GrepTool::new();
        let mut input = create_grep_input("app", Some(dir.path().to_str().unwrap()));
        input["include"] = json!("*.js");

        let result = tool.execute(input).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("app.js"));
        // Should not find "fn add" from Rust files
        assert!(!result.content.contains("lib.rs"));
    }

    #[tokio::test]
    async fn test_exclude_pattern() {
        let dir = setup_test_files();
        let tool = GrepTool::new();
        let mut input = create_grep_input("fn", Some(dir.path().to_str().unwrap()));
        input["exclude"] = json!("src/**");

        let result = tool.execute(input).await.unwrap();
        assert!(!result.is_error);
        // Should find "fn main" in main.rs but not in src/
        assert!(result.content.contains("main.rs"));
        assert!(!result.content.contains("src"));
    }

    // === Context lines ===

    #[tokio::test]
    async fn test_context_before() {
        let dir = setup_test_files();
        let tool = GrepTool::new();
        let mut input = create_grep_input("let x", Some(dir.path().to_str().unwrap()));
        input["context_before"] = json!(1);

        let result = tool.execute(input).await.unwrap();
        assert!(!result.is_error);
        // Should show the line before "let x" which is the opening brace
        assert!(result.content.contains("println!")); // line before let x
    }

    #[tokio::test]
    async fn test_context_after() {
        let dir = setup_test_files();
        let tool = GrepTool::new();
        let mut input = create_grep_input("let x", Some(dir.path().to_str().unwrap()));
        input["context_after"] = json!(1);

        let result = tool.execute(input).await.unwrap();
        assert!(!result.is_error);
        // Should show the line after "let x" which is println with x
        assert!(result.content.contains("println!(\"x = {}\", x)"));
    }

    #[tokio::test]
    async fn test_context_both_sides() {
        let dir = setup_test_files();
        let tool = GrepTool::new();
        let mut input = create_grep_input("add", Some(dir.path().join("src").to_str().unwrap()));
        input["context_before"] = json!(1);
        input["context_after"] = json!(1);

        let result = tool.execute(input).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("lib.rs"));
    }

    // === Output modes ===

    #[tokio::test]
    async fn test_output_mode_content() {
        let dir = setup_test_files();
        let tool = GrepTool::new();
        let mut input = create_grep_input("fn", Some(dir.path().to_str().unwrap()));
        input["output_mode"] = json!("content");

        let result = tool.execute(input).await.unwrap();
        assert!(!result.is_error);
        // Content mode should show line numbers by default
        assert!(result.content.contains(':')); // colon before line number
        assert_eq!(
            result
                .metadata
                .get("output_mode")
                .unwrap()
                .as_str()
                .unwrap(),
            "content"
        );
    }

    #[tokio::test]
    async fn test_output_mode_files() {
        let dir = setup_test_files();
        let tool = GrepTool::new();
        let mut input = create_grep_input("fn", Some(dir.path().to_str().unwrap()));
        input["output_mode"] = json!("files");

        let result = tool.execute(input).await.unwrap();
        assert!(!result.is_error);
        // Files mode should only contain file paths, no line content
        assert!(result.content.contains("main.rs"));
        assert!(result.content.contains("lib.rs"));
        // Should not contain code lines
        assert!(!result.content.contains("fn main"));
        assert_eq!(
            result
                .metadata
                .get("output_mode")
                .unwrap()
                .as_str()
                .unwrap(),
            "files"
        );
    }

    #[tokio::test]
    async fn test_output_mode_count() {
        let dir = setup_test_files();
        let tool = GrepTool::new();
        let mut input = create_grep_input("fn", Some(dir.path().to_str().unwrap()));
        input["output_mode"] = json!("count");

        let result = tool.execute(input).await.unwrap();
        assert!(!result.is_error);
        // Count mode should show file:count format
        assert!(result.content.contains(':'));
        assert!(result.content.contains("main.rs"));
        assert_eq!(
            result
                .metadata
                .get("output_mode")
                .unwrap()
                .as_str()
                .unwrap(),
            "count"
        );
    }

    // === Binary file skipping ===

    #[tokio::test]
    async fn test_binary_file_skipping() {
        let dir = TempDir::new().unwrap();
        // Create a binary file with null bytes
        let binary_content: Vec<u8> = vec![0x89, 0x50, 0x4E, 0x47, 0x00, 0xFF, 0xD8, 0xFF];
        fs::write(dir.path().join("binary.png"), &binary_content).unwrap();

        // Create a text file with "hello"
        fs::write(dir.path().join("text.txt"), "hello world\n").unwrap();

        let tool = GrepTool::new();
        let input = create_grep_input("hello", Some(dir.path().to_str().unwrap()));

        let result = tool.execute(input).await.unwrap();
        assert!(!result.is_error);
        // Should find in text.txt but skip binary.png
        assert!(result.content.contains("text.txt"));
        assert!(!result.content.contains("binary.png"));
    }

    // === Max results limit ===

    #[tokio::test]
    async fn test_max_results_limit() {
        let dir = TempDir::new().unwrap();
        // Create many files with matching content
        for i in 0..20 {
            fs::write(
                dir.path().join(format!("file_{i}.txt")),
                "match line here\nanother line\n",
            )
            .unwrap();
        }

        let tool = GrepTool::new();
        let mut input = create_grep_input("match", Some(dir.path().to_str().unwrap()));
        input["max_results"] = json!(5);

        let result = tool.execute(input).await.unwrap();
        assert!(!result.is_error);
        // Should be limited to 5 matches
        assert_eq!(
            result
                .metadata
                .get("total_matches")
                .unwrap()
                .as_u64()
                .unwrap(),
            5
        );
        assert!(result.metadata.get("truncated").unwrap().as_bool().unwrap());
    }

    // === Empty results ===

    #[tokio::test]
    async fn test_empty_results() {
        let dir = setup_test_files();
        let tool = GrepTool::new();
        let input = create_grep_input(
            "ZZZZNONEXISTENT_PATTERN",
            Some(dir.path().to_str().unwrap()),
        );

        let result = tool.execute(input).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.is_empty());
        assert_eq!(
            result
                .metadata
                .get("total_matches")
                .unwrap()
                .as_u64()
                .unwrap(),
            0
        );
        assert_eq!(
            result
                .metadata
                .get("total_files")
                .unwrap()
                .as_u64()
                .unwrap(),
            0
        );
    }

    // === Invalid regex handling ===

    #[tokio::test]
    async fn test_invalid_regex() {
        let tool = GrepTool::new();
        let input = create_grep_input("(unclosed parenthesis", Some("."));

        let result = tool.execute(input).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Invalid regex pattern"));
    }

    #[tokio::test]
    async fn test_empty_pattern() {
        let tool = GrepTool::new();
        let input = create_grep_input("", Some("."));

        let result = tool.execute(input).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Pattern cannot be empty")
        );
    }

    // === Nonexistent path ===

    #[tokio::test]
    async fn test_nonexistent_path() {
        let tool = GrepTool::new();
        let input = create_grep_input("test", Some("/nonexistent/path/that/does/not/exist"));

        let result = tool.execute(input).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        // Sandbox validation fails first (cannot canonicalize nonexistent path),
        // or the explicit exists-check fires if sandbox is permissive.
        assert!(
            err_msg.contains("Path does not exist")
                || err_msg.contains("Cannot resolve path")
                || err_msg.contains("Path sandbox"),
            "Unexpected error: {err_msg}"
        );
    }

    // === Line number toggle ===

    #[tokio::test]
    async fn test_line_numbers_enabled() {
        let dir = setup_test_files();
        let tool = GrepTool::new();
        let mut input = create_grep_input("fn main", Some(dir.path().to_str().unwrap()));
        input["line_number"] = json!(true);

        let result = tool.execute(input).await.unwrap();
        assert!(!result.is_error);
        // With line numbers, format is "file\n:1:fn main() {"
        assert!(result.content.contains("1:fn main"));
    }

    #[tokio::test]
    async fn test_line_numbers_disabled() {
        let dir = setup_test_files();
        let tool = GrepTool::new();
        let mut input = create_grep_input("fn main", Some(dir.path().to_str().unwrap()));
        input["line_number"] = json!(false);

        let result = tool.execute(input).await.unwrap();
        assert!(!result.is_error);
        // Without line numbers, format is "file\n:fn main() {"
        assert!(result.content.contains(":fn main"));
        // Should not contain a colon followed by a digit
        let lines: Vec<&str> = result.content.lines().collect();
        let has_line_num = lines.iter().any(|l| {
            l.starts_with(':')
                && l.chars()
                    .nth(1)
                    .map(|c| c.is_ascii_digit())
                    .unwrap_or(false)
        });
        assert!(!has_line_num);
    }

    // === Path matching helper ===

    #[test]
    fn test_path_matches_glob_extension() {
        assert!(path_matches_glob(Path::new("file.rs"), "*.rs"));
        assert!(path_matches_glob(Path::new("/path/to/file.rs"), "*.rs"));
        assert!(!path_matches_glob(Path::new("file.js"), "*.rs"));
        assert!(!path_matches_glob(Path::new("file.rs.bak"), "*.rs"));
    }

    #[test]
    fn test_path_matches_glob_exact() {
        assert!(path_matches_glob(Path::new("main.rs"), "main.rs"));
        assert!(!path_matches_glob(Path::new("lib.rs"), "main.rs"));
    }

    #[test]
    fn test_path_matches_glob_wildcard() {
        assert!(path_matches_glob(Path::new("test_file.rs"), "*.rs"));
        assert!(path_matches_glob(Path::new("something.txt"), "*.txt"));
    }

    // === Default values ===

    #[test]
    fn test_grep_output_mode_default() {
        let mode: GrepOutputMode = Default::default();
        assert_eq!(mode, GrepOutputMode::Content);
    }

    #[test]
    fn test_grep_tool_default() {
        let tool = GrepTool::new();
        assert_eq!(tool.name(), "Grep");
    }

    // === Single file search ===

    #[tokio::test]
    async fn test_search_single_file() {
        let dir = setup_test_files();
        let tool = GrepTool::new();
        let input = create_grep_input("require", Some(dir.path().join("app.js").to_str().unwrap()));

        let result = tool.execute(input).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("require"));
        assert_eq!(
            result
                .metadata
                .get("total_files")
                .unwrap()
                .as_u64()
                .unwrap(),
            1
        );
    }

    // ======================================================================
    // Property-based tests (proptest)
    // ======================================================================

    proptest::proptest! {
        /// Any valid regex pattern compiles without panic for simple patterns.
        #[test]
        fn proptest_regex_compiles(pattern in "[a-zA-Z0-9_. ]{1,30}") {
            let dir = TempDir::new().unwrap();
            fs::write(dir.path().join("test.txt"), "hello world\n").unwrap();

            let rt = tokio::runtime::Runtime::new().unwrap();
            let tool = GrepTool::new();
            let input = json!({
                "pattern": pattern,
                "path": dir.path().to_str().unwrap(),
            });

            // Must not panic; errors are returned as Err
            let _ = rt.block_on(tool.execute(input));
        }

        /// Search results are deterministic: same pattern on the same files
        /// produces the same total_matches count.
        #[test]
        fn proptest_search_deterministic(pattern in "[a-zA-Z]{1,10}") {
            let dir = TempDir::new().unwrap();
            fs::write(dir.path().join("test.txt"), "hello world foo bar\n").unwrap();

            let rt = tokio::runtime::Runtime::new().unwrap();
            let tool = GrepTool::new();

            let input1 = json!({
                "pattern": pattern,
                "path": dir.path().to_str().unwrap(),
            });
            let input2 = json!({
                "pattern": pattern,
                "path": dir.path().to_str().unwrap(),
            });

            let r1 = rt.block_on(tool.execute(input1)).unwrap();
            let r2 = rt.block_on(tool.execute(input2)).unwrap();
            assert_eq!(
                r1.metadata["total_matches"],
                r2.metadata["total_matches"]
            );
        }

        /// GrepOutputMode roundtrips through JSON.
        #[test]
        fn proptest_output_mode_roundtrip(mode_str in "content|files|count") {
            let json_str = format!("\"{mode_str}\"");
            let parsed: GrepOutputMode = serde_json::from_str(&json_str).unwrap();
            let serialized = serde_json::to_string(&parsed).unwrap();
            let reparsed: GrepOutputMode = serde_json::from_str(&serialized).unwrap();
            assert_eq!(parsed, reparsed);
        }

        /// GrepInput deserialization roundtrips through JSON for basic fields.
        #[test]
        fn proptest_grep_input_roundtrip(pattern in ".{1,30}") {
            let input = GrepInput {
                pattern: pattern.clone(),
                path: None,
                include: None,
                exclude: None,
                case_insensitive: None,
                line_number: None,
                context_before: None,
                context_after: None,
                max_results: None,
                output_mode: None,
            };
            let json = serde_json::to_string(&input).unwrap();
            let parsed: GrepInput = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed.pattern, pattern);
        }

        /// path_matches_glob is deterministic for any path/pattern pair.
        #[test]
        fn proptest_path_matches_glob_deterministic(
            path_str in ".{0,50}",
            pattern in "[a-zA-Z0-9_.*]{0,20}",
        ) {
            let path = Path::new(&path_str);
            let r1 = path_matches_glob(path, &pattern);
            let r2 = path_matches_glob(path, &pattern);
            assert_eq!(r1, r2);
        }
    }
}
