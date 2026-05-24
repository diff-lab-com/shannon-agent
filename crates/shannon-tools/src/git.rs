//! Git operation tools
//!
//! Provides implementations for:
//! - GitBranch: List, create, switch, delete branches
//! - GitDiff: Enhanced diff with staged/unstaged/commit range options
//! - GitLog: View commit history with formatting options
//! - GitStash: Stash and unstash changes
//! - GitSafety: Safety checks before destructive operations

use crate::{Tool, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::process::Command;

// ---------------------------------------------------------------------------
// Helper: run a git command and capture output
// ---------------------------------------------------------------------------

/// Validate a user-provided git argument (branch name, etc.) to prevent injection.
fn validate_git_arg(arg: &str) -> Result<&str, ToolError> {
    if arg.is_empty() {
        return Err(ToolError::InvalidInput(
            "Argument must not be empty".to_string(),
        ));
    }
    if arg.starts_with('-') {
        return Err(ToolError::InvalidInput(format!(
            "Invalid argument: '{arg}' must not start with '-'"
        )));
    }
    if arg.contains('\0') {
        return Err(ToolError::InvalidInput(
            "Argument must not contain null bytes".to_string(),
        ));
    }
    if arg.contains("../") || arg.contains("..\\") {
        return Err(ToolError::InvalidInput(
            "Path traversal not allowed".to_string(),
        ));
    }
    if arg.contains([';', '&', '|', '$', '`', '(', ')', '{', '}']) {
        return Err(ToolError::InvalidInput(
            "Shell metacharacters not allowed".to_string(),
        ));
    }
    if arg.len() > 256 {
        return Err(ToolError::InvalidInput(
            "Argument too long (max 256 chars)".to_string(),
        ));
    }
    Ok(arg)
}

/// Run a git command in a given working directory and return stdout, stderr, exit status.
fn run_git(args: &[&str], cwd: Option<&str>) -> Result<(String, String, bool), ToolError> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let output = cmd
        .output()
        .map_err(|e| ToolError::ExecutionFailed(format!("Failed to execute git: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    Ok((stdout, stderr, output.status.success()))
}

/// Find the git root starting from the current directory (or a given path).
fn find_git_root(start: Option<&str>) -> Result<String, ToolError> {
    let start_path = match start {
        Some(s) => std::path::PathBuf::from(s),
        None => std::env::current_dir()
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to determine cwd: {e}")))?,
    };

    let mut current = Some(start_path.as_path());
    while let Some(path) = current {
        if path.join(".git").exists() {
            return Ok(path.to_string_lossy().to_string());
        }
        current = path.parent();
    }
    Err(ToolError::ExecutionFailed(
        "Not a git repository (or any parent up to the root)".to_string(),
    ))
}

/// Get the current branch name.
fn current_branch(cwd: Option<&str>) -> Result<String, ToolError> {
    let (stdout, _, success) = run_git(&["rev-parse", "--abbrev-ref", "HEAD"], cwd)?;
    if !success {
        return Err(ToolError::ExecutionFailed(
            "Failed to determine current branch".to_string(),
        ));
    }
    Ok(stdout.trim().to_string())
}

/// Check whether the working directory has uncommitted changes.
fn is_working_dir_dirty(cwd: Option<&str>) -> Result<bool, ToolError> {
    let (stdout, _, success) = run_git(&["status", "--porcelain"], cwd)?;
    if !success {
        return Err(ToolError::ExecutionFailed(
            "Failed to check working directory status".to_string(),
        ));
    }
    Ok(!stdout.trim().is_empty())
}

// ---------------------------------------------------------------------------
// GitBranchTool
// ---------------------------------------------------------------------------

/// Branch action type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum BranchAction {
    List,
    Create,
    Switch,
    Delete,
}

/// Input for GitBranchTool.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GitBranchInput {
    /// Action to perform: list, create, switch, delete
    pub action: BranchAction,

    /// Branch name (required for create, switch, delete)
    pub name: Option<String>,

    /// Force delete a branch (only applies to delete action)
    pub force: Option<bool>,

    /// Set as the new current branch when creating (create -b equivalent)
    pub checkout: Option<bool>,
}

/// Git branch management tool.
pub struct GitBranchTool {
    description: String,
}

impl Default for GitBranchTool {
    fn default() -> Self {
        Self::new()
    }
}

impl GitBranchTool {
    pub fn new() -> Self {
        Self {
            description: "List, create, switch, and delete git branches".to_string(),
        }
    }

    fn list_branches(&self, cwd: Option<&str>) -> Result<ToolOutput, ToolError> {
        let (stdout, stderr, success) =
            run_git(&["branch", "-a", "--color=never", "-v", "--no-abbrev"], cwd)?;
        if !success {
            return Ok(ToolOutput {
                content: format!("Failed to list branches: {stderr}"),
                is_error: true,
                metadata: HashMap::new(),
            });
        }

        // Also get current branch
        let current = current_branch(cwd).unwrap_or_else(|_| "unknown".to_string());

        Ok(ToolOutput {
            content: stdout,
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("current_branch".to_string(), json!(current));
                map
            },
        })
    }

    fn create_branch(
        &self,
        input: &GitBranchInput,
        cwd: Option<&str>,
    ) -> Result<ToolOutput, ToolError> {
        let name = input.name.as_deref().ok_or_else(|| {
            ToolError::InvalidInput("Branch name is required for create action".to_string())
        })?;

        validate_git_arg(name)?;

        let checkout = input.checkout.unwrap_or(false);

        let args: Vec<&str> = if checkout {
            vec!["checkout", "-b", name]
        } else {
            vec!["branch", name]
        };

        let (stdout, stderr, success) = run_git(&args, cwd)?;
        if !success {
            return Ok(ToolOutput {
                content: format!("Failed to create branch '{name}': {stderr}"),
                is_error: true,
                metadata: HashMap::new(),
            });
        }

        let msg = if checkout {
            format!("Created and switched to branch '{name}'.")
        } else {
            format!("Created branch '{name}'.")
        };

        Ok(ToolOutput {
            content: format!("{}\n{}", msg, stdout.trim()),
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("branch".to_string(), json!(name));
                map.insert("checkout".to_string(), json!(checkout));
                map
            },
        })
    }

    fn switch_branch(
        &self,
        input: &GitBranchInput,
        cwd: Option<&str>,
    ) -> Result<ToolOutput, ToolError> {
        let name = input.name.as_deref().ok_or_else(|| {
            ToolError::InvalidInput("Branch name is required for switch action".to_string())
        })?;

        validate_git_arg(name)?;

        // Safety check: warn if working directory is dirty
        if is_working_dir_dirty(cwd)? {
            tracing::warn!(
                branch = %name,
                "Switching branches with uncommitted changes — may cause conflicts"
            );
        }

        let (stdout, stderr, success) = run_git(&["checkout", name], cwd)?;
        if !success {
            return Ok(ToolOutput {
                content: format!("Failed to switch to branch '{}': {}", name, stderr.trim()),
                is_error: true,
                metadata: HashMap::new(),
            });
        }

        Ok(ToolOutput {
            content: format!("Switched to branch '{}'.\n{}", name, stdout.trim()),
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("branch".to_string(), json!(name));
                map
            },
        })
    }

    fn delete_branch(
        &self,
        input: &GitBranchInput,
        cwd: Option<&str>,
    ) -> Result<ToolOutput, ToolError> {
        let name = input.name.as_deref().ok_or_else(|| {
            ToolError::InvalidInput("Branch name is required for delete action".to_string())
        })?;

        validate_git_arg(name)?;

        // Safety: refuse to delete the current branch
        let current = current_branch(cwd)?;
        if current == name {
            return Err(ToolError::ExecutionFailed(format!(
                "Cannot delete the current branch '{name}'. Switch to another branch first."
            )));
        }

        let force = input.force.unwrap_or(false);

        // Safety: warn on force delete
        if force {
            return Ok(ToolOutput {
                content: format!(
                    "[SAFETY WARNING] Force-deleting branch '{name}' will discard all unmerged commits. \
                     This cannot be undone. If you are sure, use the Bash tool with: git branch -D {name}\n"
                ),
                is_error: false,
                metadata: {
                    let mut map = HashMap::new();
                    map.insert("force_delete_warning".to_string(), json!(true));
                    map.insert("target_branch".to_string(), json!(name));
                    map
                },
            });
        }

        let (stdout, stderr, success) = run_git(&["branch", "-d", name], cwd)?;
        if !success {
            return Ok(ToolOutput {
                content: format!(
                    "Failed to delete branch '{}': {}\n\
                     Hint: If the branch is fully merged and you want to force delete, set force: true.",
                    name,
                    stderr.trim()
                ),
                is_error: true,
                metadata: HashMap::new(),
            });
        }

        Ok(ToolOutput {
            content: format!("Deleted branch '{}'.\n{}", name, stdout.trim()),
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("branch".to_string(), json!(name));
                map.insert("force".to_string(), json!(force));
                map
            },
        })
    }
}

#[async_trait]
impl Tool for GitBranchTool {
    fn name(&self) -> &str {
        "GitBranch"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "description": "Branch operation to perform",
                    "enum": ["list", "create", "switch", "delete"]
                },
                "name": {
                    "type": "string",
                    "description": "Branch name (required for create, switch, delete)"
                },
                "force": {
                    "type": "boolean",
                    "description": "Force delete (delete action only, warns instead of executing)"
                },
                "checkout": {
                    "type": "boolean",
                    "description": "Switch to the new branch after creating (create action only)"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let branch_input: GitBranchInput = serde_json::from_value(input.clone())
            .map_err(|e| ToolError::InvalidInput(format!("Invalid git branch input: {e}")))?;

        // We must be in a git repo for any action
        if let Err(e) = find_git_root(None) {
            return Err(ToolError::ExecutionFailed(e.to_string()));
        }

        match branch_input.action {
            BranchAction::List => self.list_branches(None),
            BranchAction::Create => self.create_branch(&branch_input, None),
            BranchAction::Switch => self.switch_branch(&branch_input, None),
            BranchAction::Delete => self.delete_branch(&branch_input, None),
        }
    }

    fn category(&self) -> &str {
        "git"
    }
}

// ---------------------------------------------------------------------------
// GitDiffTool
// ---------------------------------------------------------------------------

/// Input for GitDiffTool.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GitDiffInput {
    /// Show staged changes only
    pub staged: Option<bool>,

    /// Commit range, e.g. "abc123..def456"
    pub commit_range: Option<String>,

    /// Specific file path to diff
    pub file: Option<String>,

    /// Number of context lines around each change (default: 3)
    pub context_lines: Option<u32>,

    /// Ignore whitespace changes
    pub ignore_whitespace: Option<bool>,

    /// Show stats instead of full diff
    pub stat: Option<bool>,
}

/// Enhanced git diff tool.
pub struct GitDiffTool {
    description: String,
}

impl Default for GitDiffTool {
    fn default() -> Self {
        Self::new()
    }
}

impl GitDiffTool {
    pub fn new() -> Self {
        Self {
            description: "Show git diffs with support for staged, unstaged, and commit range diffs"
                .to_string(),
        }
    }

    fn build_diff_args(&self, input: &GitDiffInput) -> Result<Vec<String>, ToolError> {
        let mut args = Vec::new();

        if input.staged.unwrap_or(false) {
            args.push("--cached".to_string());
        }

        if let Some(ref range) = input.commit_range {
            validate_git_arg(range)?;
            args.push(range.clone());
        }

        if let Some(ref file) = input.file {
            validate_git_arg(file)?;
            args.push("--".to_string());
            args.push(file.clone());
        }

        if let Some(context) = input.context_lines {
            args.push(format!("-U{context}"));
        }

        if input.ignore_whitespace.unwrap_or(false) {
            args.push("-w".to_string());
        }

        if input.stat.unwrap_or(false) {
            args.push("--stat".to_string());
        }

        args.push("--color=never".to_string());

        Ok(args)
    }
}

#[async_trait]
impl Tool for GitDiffTool {
    fn name(&self) -> &str {
        "GitDiff"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "staged": {
                    "type": "boolean",
                    "description": "Show staged changes only (default: false)"
                },
                "commit_range": {
                    "type": "string",
                    "description": "Commit range to diff, e.g. 'abc123..def456'"
                },
                "file": {
                    "type": "string",
                    "description": "Specific file path to diff"
                },
                "context_lines": {
                    "type": "integer",
                    "description": "Number of context lines around changes (default: 3)"
                },
                "ignore_whitespace": {
                    "type": "boolean",
                    "description": "Ignore whitespace changes (default: false)"
                },
                "stat": {
                    "type": "boolean",
                    "description": "Show diffstat summary instead of full diff (default: false)"
                }
            }
        })
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let diff_input: GitDiffInput = serde_json::from_value(input.clone())
            .map_err(|e| ToolError::InvalidInput(format!("Invalid git diff input: {e}")))?;

        // Verify we are in a git repo
        if let Err(e) = find_git_root(None) {
            return Err(ToolError::ExecutionFailed(e.to_string()));
        }

        let args = self.build_diff_args(&diff_input)?;
        let mut full_args = vec!["diff"];
        for arg in &args {
            full_args.push(arg.as_str());
        }

        let (stdout, stderr, success) = run_git(&full_args, None)?;

        if !success && !stderr.is_empty() {
            return Ok(ToolOutput {
                content: format!("Diff failed: {}", stderr.trim()),
                is_error: true,
                metadata: HashMap::new(),
            });
        }

        let has_changes = !stdout.trim().is_empty();
        let description = if diff_input.staged.unwrap_or(false) {
            "staged changes"
        } else if diff_input.commit_range.is_some() {
            "commit range diff"
        } else {
            "unstaged changes"
        };

        Ok(ToolOutput {
            content: if has_changes {
                stdout
            } else {
                format!("No {description} found.")
            },
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("has_changes".to_string(), json!(has_changes));
                map.insert("diff_type".to_string(), json!(description));
                if let Some(ref file) = diff_input.file {
                    map.insert("file".to_string(), json!(file));
                }
                if let Some(ref range) = diff_input.commit_range {
                    map.insert("commit_range".to_string(), json!(range));
                }
                map
            },
        })
    }

    fn category(&self) -> &str {
        "git"
    }
    fn is_read_only(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// GitLogTool
// ---------------------------------------------------------------------------

/// Input for GitLogTool.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GitLogInput {
    /// Number of commits to show (default: 10, max: 100)
    pub count: Option<usize>,

    /// Filter by author name or email
    pub author: Option<String>,

    /// Filter commits since a date (e.g. "2 weeks ago", "2024-01-01")
    pub since: Option<String>,

    /// Compact one-line format per commit
    pub oneline: Option<bool>,

    /// Specific file to show history for
    pub file: Option<String>,

    /// Show diffs alongside each commit
    pub patch: Option<bool>,

    /// Branch to show log for
    pub branch: Option<String>,
}

/// Git log viewer tool.
pub struct GitLogTool {
    description: String,
}

impl Default for GitLogTool {
    fn default() -> Self {
        Self::new()
    }
}

impl GitLogTool {
    pub fn new() -> Self {
        Self {
            description: "View git commit history with filtering and formatting options"
                .to_string(),
        }
    }

    fn build_log_args(&self, input: &GitLogInput) -> Result<Vec<String>, ToolError> {
        let mut args = Vec::new();

        let count = input.count.unwrap_or(10).min(100);
        args.push(format!("-{count}"));

        if input.oneline.unwrap_or(false) {
            args.push("--oneline".to_string());
        } else {
            // Default: readable format
            args.push("--format=%h %ad %an - %s (%ar)".to_string());
            args.push("--date=short".to_string());
        }

        if let Some(ref author) = input.author {
            validate_git_arg(author)?;
            args.push(format!("--author={author}"));
        }

        if let Some(ref since) = input.since {
            validate_git_arg(since)?;
            args.push(format!("--since={since}"));
        }

        if input.patch.unwrap_or(false) {
            args.push("-p".to_string());
        }

        if let Some(ref branch) = input.branch {
            validate_git_arg(branch)?;
            args.push(branch.clone());
        }

        if let Some(ref file) = input.file {
            validate_git_arg(file)?;
            args.push("--".to_string());
            args.push(file.clone());
        }

        Ok(args)
    }
}

#[async_trait]
impl Tool for GitLogTool {
    fn name(&self) -> &str {
        "GitLog"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "count": {
                    "type": "integer",
                    "description": "Number of commits to show (default: 10, max: 100)"
                },
                "author": {
                    "type": "string",
                    "description": "Filter by author name or email"
                },
                "since": {
                    "type": "string",
                    "description": "Filter commits since a date (e.g. '2 weeks ago', '2024-01-01')"
                },
                "oneline": {
                    "type": "boolean",
                    "description": "Show compact one-line format per commit (default: false)"
                },
                "file": {
                    "type": "string",
                    "description": "Show history for a specific file only"
                },
                "patch": {
                    "type": "boolean",
                    "description": "Include diffs for each commit (default: false)"
                },
                "branch": {
                    "type": "string",
                    "description": "Show log for a specific branch"
                }
            }
        })
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let log_input: GitLogInput = serde_json::from_value(input.clone())
            .map_err(|e| ToolError::InvalidInput(format!("Invalid git log input: {e}")))?;

        // Verify we are in a git repo
        if let Err(e) = find_git_root(None) {
            return Err(ToolError::ExecutionFailed(e.to_string()));
        }

        let args = self.build_log_args(&log_input)?;
        let mut full_args = vec!["log"];
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        full_args.extend(&arg_refs);

        let (stdout, stderr, success) = run_git(&full_args, None)?;

        if !success {
            return Ok(ToolOutput {
                content: format!("Log failed: {}", stderr.trim()),
                is_error: true,
                metadata: HashMap::new(),
            });
        }

        let count = log_input.count.unwrap_or(10).min(100);
        let line_count = stdout.lines().filter(|l| !l.trim().is_empty()).count();

        Ok(ToolOutput {
            content: if stdout.trim().is_empty() {
                "No commits found matching the given filters.".to_string()
            } else {
                stdout
            },
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("commit_count".to_string(), json!(line_count));
                map.insert("requested_count".to_string(), json!(count));
                if let Some(ref author) = log_input.author {
                    map.insert("author_filter".to_string(), json!(author));
                }
                if let Some(ref file) = log_input.file {
                    map.insert("file_filter".to_string(), json!(file));
                }
                map
            },
        })
    }

    fn category(&self) -> &str {
        "git"
    }
    fn is_read_only(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// GitStashTool
// ---------------------------------------------------------------------------

/// Stash action type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum StashAction {
    List,
    Push,
    Pop,
    Drop,
    Apply,
}

/// Input for GitStashTool.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GitStashInput {
    /// Action to perform: list, push, pop, drop, apply
    pub action: StashAction,

    /// Optional stash message (for push action)
    pub message: Option<String>,

    /// Stash index to pop/drop/apply (default: 0, the latest stash)
    pub index: Option<usize>,

    /// Include untracked files in the stash (push action only)
    pub include_untracked: Option<bool>,
}

/// Git stash management tool.
pub struct GitStashTool {
    description: String,
}

impl Default for GitStashTool {
    fn default() -> Self {
        Self::new()
    }
}

impl GitStashTool {
    pub fn new() -> Self {
        Self {
            description: "Manage git stashes: list, push, pop, drop, and apply stashed changes"
                .to_string(),
        }
    }

    fn list_stashes(&self) -> Result<ToolOutput, ToolError> {
        let (stdout, stderr, success) = run_git(&["stash", "list"], None)?;
        if !success {
            return Ok(ToolOutput {
                content: format!("Failed to list stashes: {}", stderr.trim()),
                is_error: true,
                metadata: HashMap::new(),
            });
        }

        if stdout.trim().is_empty() {
            return Ok(ToolOutput {
                content: "No stashes found.".to_string(),
                is_error: false,
                metadata: {
                    let mut map = HashMap::new();
                    map.insert("stash_count".to_string(), json!(0));
                    map
                },
            });
        }

        let count = stdout.lines().filter(|l| !l.trim().is_empty()).count();

        Ok(ToolOutput {
            content: stdout,
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("stash_count".to_string(), json!(count));
                map
            },
        })
    }

    fn push_stash(&self, input: &GitStashInput) -> Result<ToolOutput, ToolError> {
        if !is_working_dir_dirty(None)? {
            return Ok(ToolOutput {
                content: "Nothing to stash: working directory is clean.".to_string(),
                is_error: false,
                metadata: HashMap::new(),
            });
        }

        let mut full_args: Vec<String> = vec!["stash".to_string(), "push".to_string()];
        if input.include_untracked.unwrap_or(false) {
            full_args.push("--include-untracked".to_string());
        }
        if let Some(ref msg) = input.message {
            full_args.push("-m".to_string());
            full_args.push(msg.clone());
        }

        let arg_refs: Vec<&str> = full_args.iter().map(|s| s.as_str()).collect();
        let (stdout, stderr, success) = run_git(&arg_refs, None)?;

        if !success {
            return Ok(ToolOutput {
                content: format!("Failed to stash changes: {}", stderr.trim()),
                is_error: true,
                metadata: HashMap::new(),
            });
        }

        Ok(ToolOutput {
            content: format!("Changes stashed successfully.\n{}", stdout.trim()),
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert(
                    "message".to_string(),
                    json!(input.message.clone().unwrap_or_default()),
                );
                map.insert(
                    "include_untracked".to_string(),
                    json!(input.include_untracked.unwrap_or(false)),
                );
                map
            },
        })
    }

    fn pop_stash(&self, index: usize) -> Result<ToolOutput, ToolError> {
        let index_str = format!("stash@{{{index}}}");
        let args = &["stash", "pop", &index_str];

        let (stdout, stderr, success) = run_git(args, None)?;

        if !success {
            return Ok(ToolOutput {
                content: format!(
                    "Failed to pop stash {}:\n{}\n\nHint: You may need to resolve conflicts first, \
                     or try 'git stash apply' to keep the stash after applying.",
                    index,
                    stderr.trim()
                ),
                is_error: true,
                metadata: HashMap::new(),
            });
        }

        Ok(ToolOutput {
            content: format!("Applied and removed stash {}.\n{}", index, stdout.trim()),
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("stash_index".to_string(), json!(index));
                map
            },
        })
    }

    fn drop_stash(&self, index: usize) -> Result<ToolOutput, ToolError> {
        let index_str = format!("stash@{{{index}}}");
        let args = &["stash", "drop", &index_str];

        let (stdout, stderr, success) = run_git(args, None)?;

        if !success {
            return Ok(ToolOutput {
                content: format!("Failed to drop stash {}: {}", index, stderr.trim()),
                is_error: true,
                metadata: HashMap::new(),
            });
        }

        Ok(ToolOutput {
            content: format!(
                "[SAFETY WARNING] Dropped stash {}. The stashed changes have been discarded.\n{}",
                index,
                stdout.trim()
            ),
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("stash_index".to_string(), json!(index));
                map.insert("drop_warning".to_string(), json!(true));
                map
            },
        })
    }

    fn apply_stash(&self, index: usize) -> Result<ToolOutput, ToolError> {
        let index_str = format!("stash@{{{index}}}");
        let args = &["stash", "apply", &index_str];

        let (stdout, stderr, success) = run_git(args, None)?;

        if !success {
            return Ok(ToolOutput {
                content: format!("Failed to apply stash {}:\n{}", index, stderr.trim()),
                is_error: true,
                metadata: HashMap::new(),
            });
        }

        Ok(ToolOutput {
            content: format!("Applied stash {} (stash kept).\n{}", index, stdout.trim()),
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("stash_index".to_string(), json!(index));
                map
            },
        })
    }
}

#[async_trait]
impl Tool for GitStashTool {
    fn name(&self) -> &str {
        "GitStash"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "description": "Stash operation to perform",
                    "enum": ["list", "push", "pop", "drop", "apply"]
                },
                "message": {
                    "type": "string",
                    "description": "Stash message (push action only)"
                },
                "index": {
                    "type": "integer",
                    "description": "Stash index to pop/drop/apply (default: 0)"
                },
                "include_untracked": {
                    "type": "boolean",
                    "description": "Include untracked files in the stash (push action only, default: false)"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let stash_input: GitStashInput = serde_json::from_value(input.clone())
            .map_err(|e| ToolError::InvalidInput(format!("Invalid git stash input: {e}")))?;

        // Verify we are in a git repo
        if let Err(e) = find_git_root(None) {
            return Err(ToolError::ExecutionFailed(e.to_string()));
        }

        let index = stash_input.index.unwrap_or(0);

        match stash_input.action {
            StashAction::List => self.list_stashes(),
            StashAction::Push => self.push_stash(&stash_input),
            StashAction::Pop => self.pop_stash(index),
            StashAction::Drop => self.drop_stash(index),
            StashAction::Apply => self.apply_stash(index),
        }
    }

    fn category(&self) -> &str {
        "git"
    }
}

// ---------------------------------------------------------------------------
// GitSafetyTool
// ---------------------------------------------------------------------------

/// Input for GitSafetyTool.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GitSafetyInput {
    /// The git command to check (e.g., "push --force origin main")
    pub command: String,
}

/// Safety analysis result.
#[derive(Debug, Clone, Serialize)]
pub struct SafetyCheckResult {
    /// Whether the command is allowed
    pub allowed: bool,

    /// Risk level: "safe", "warning", "blocked"
    pub risk: String,

    /// Human-readable explanation
    pub message: String,
}

/// Git safety check tool.
pub struct GitSafetyTool {
    description: String,
}

impl Default for GitSafetyTool {
    fn default() -> Self {
        Self::new()
    }
}

impl GitSafetyTool {
    pub fn new() -> Self {
        Self {
            description: "Check if a git command is safe before executing it".to_string(),
        }
    }

    /// Analyze a git command for safety.
    fn check_command(&self, command: &str) -> SafetyCheckResult {
        let lower = command.to_lowercase();

        // ---- BLOCKED operations ----

        // git push --force to main/master
        if (lower.contains("push") && lower.contains("--force"))
            && (lower.contains("main") || lower.contains("master"))
        {
            return SafetyCheckResult {
                allowed: false,
                risk: "blocked".to_string(),
                message: "BLOCKED: Force-pushing to main/master is not allowed. \
                          This can rewrite public history and break other collaborators' work."
                    .to_string(),
            };
        }

        // git push -f (short form) to main/master
        if (lower.contains("push") && lower.contains(" -f"))
            && (lower.contains("main") || lower.contains("master"))
        {
            return SafetyCheckResult {
                allowed: false,
                risk: "blocked".to_string(),
                message: "BLOCKED: Force-pushing to main/master is not allowed. \
                          This can rewrite public history and break other collaborators' work."
                    .to_string(),
            };
        }

        // git reset --hard
        if lower.contains("reset") && lower.contains("--hard") {
            return SafetyCheckResult {
                allowed: false,
                risk: "blocked".to_string(),
                message: "WARNING: 'git reset --hard' will discard all uncommitted changes. \
                          Ensure you have committed or stashed everything important first."
                    .to_string(),
            };
        }

        // git clean (especially with -fd or -fdx)
        if lower.contains("clean") && (lower.contains("-f") || lower.contains("--force")) {
            let has_dx = lower.contains("-d") || lower.contains("fdx") || lower.contains("-x");
            let msg = if has_dx {
                "WARNING: 'git clean -fd' (or -fdx) will permanently delete untracked files and directories. \
                 This cannot be undone. Double-check the list of files to be deleted with 'git clean -n' first."
            } else {
                "WARNING: 'git clean -f' will permanently delete untracked files. \
                 This cannot be undone. Double-check with 'git clean -n' first."
            };
            return SafetyCheckResult {
                allowed: false,
                risk: "blocked".to_string(),
                message: msg.to_string(),
            };
        }

        // git checkout -- . (discard all changes)
        if lower.contains("checkout") && lower.contains("-- .") {
            return SafetyCheckResult {
                allowed: false,
                risk: "blocked".to_string(),
                message: "WARNING: 'git checkout -- .' will discard all uncommitted changes in the working directory. \
                          Consider stashing instead."
                    .to_string(),
            };
        }

        // git restore --staged . (unstage all)
        if lower.contains("restore") && lower.contains("--staged") && lower.contains(".") {
            return SafetyCheckResult {
                allowed: true,
                risk: "warning".to_string(),
                message: "CAUTION: 'git restore --staged .' will unstage all changes. \
                          The changes remain in your working directory but are no longer staged for commit."
                    .to_string(),
            };
        }

        // git branch -D (force delete)
        if lower.contains("branch") && lower.contains("-d") && !lower.contains("-d ") {
            // Just -d is safe; -D is force
        }
        if lower.contains("branch") && (lower.contains(" -d") || lower.contains(" -D")) {
            let force = lower.contains(" -d");
            if !force {
                return SafetyCheckResult {
                    allowed: true,
                    risk: "safe".to_string(),
                    message: "Safe: 'git branch -d' only deletes fully merged branches."
                        .to_string(),
                };
            }
        }

        // git rebase on public branches
        if lower.contains("rebase")
            && (lower.contains("main") || lower.contains("master") || lower.contains("origin/"))
        {
            return SafetyCheckResult {
                allowed: true,
                risk: "warning".to_string(),
                message: "CAUTION: Rebasing on a shared branch can rewrite history for other collaborators. \
                          Consider merging instead, or ensure you are on a feature branch."
                    .to_string(),
            };
        }

        // git push --force to non-main branches
        if lower.contains("push") && (lower.contains("--force") || lower.contains(" -f")) {
            return SafetyCheckResult {
                allowed: true,
                risk: "warning".to_string(),
                message: "CAUTION: Force-pushing rewrites history. Ensure no one else is working on this branch, \
                          and communicate the force-push to collaborators."
                    .to_string(),
            };
        }

        // Default: safe
        SafetyCheckResult {
            allowed: true,
            risk: "safe".to_string(),
            message: "Command appears safe to execute.".to_string(),
        }
    }
}

#[async_trait]
impl Tool for GitSafetyTool {
    fn name(&self) -> &str {
        "GitSafety"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The git command to safety-check (e.g. 'push --force origin main')"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let safety_input: GitSafetyInput = serde_json::from_value(input.clone())
            .map_err(|e| ToolError::InvalidInput(format!("Invalid git safety input: {e}")))?;

        if safety_input.command.trim().is_empty() {
            return Err(ToolError::InvalidInput(
                "Command must not be empty".to_string(),
            ));
        }

        let result = self.check_command(&safety_input.command);

        Ok(ToolOutput {
            content: format!(
                "[{}] {}\nCommand: git {}",
                result.risk.to_uppercase(),
                result.message,
                safety_input.command
            ),
            is_error: !result.allowed,
            metadata: {
                let mut map = HashMap::new();
                map.insert("allowed".to_string(), json!(result.allowed));
                map.insert("risk".to_string(), json!(result.risk));
                map
            },
        })
    }

    fn category(&self) -> &str {
        "git"
    }
    fn is_read_only(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// AutoCommitTool
// ---------------------------------------------------------------------------

/// Input for AutoCommitTool.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AutoCommitInput {
    /// Commit message. If omitted, one is generated from the diff.
    pub message: Option<String>,
    /// If true, show what would be committed without committing.
    #[serde(default)]
    pub dry_run: bool,
    /// If true, stage all tracked changes (`git add -u`). Default: true.
    #[serde(default = "default_true")]
    pub add_all: bool,
    /// Specific files to stage (relative paths). Mutually exclusive with add_all=true
    /// — if both are set, specific files take priority.
    #[serde(default)]
    pub files: Vec<String>,
    /// Optional co-author trailer, e.g. "Claude <noreply@anthropic.com>"
    #[serde(default)]
    pub co_author: Option<String>,
}

fn default_true() -> bool {
    true
}

/// Auto-commit tool. Stages and commits all current changes with a generated
/// or provided commit message. Inspired by Aider's auto-commit behavior.
///
/// # Safety
///
/// - Never force-pushes.
/// - Skips commit if working directory is clean.
/// - Checks for GitSafety violations before staging.
pub struct AutoCommitTool {
    description: String,
}

impl Default for AutoCommitTool {
    fn default() -> Self {
        Self::new()
    }
}

impl AutoCommitTool {
    pub fn new() -> Self {
        Self {
            description:
                "Smart git commit tool. Stages changes (all or specific files), generates \
                semantic commit messages from diffs, and creates commits. Supports dry-run preview \
                and co-author trailers. Shows git status and diff stats before committing."
                    .to_string(),
        }
    }

    /// Generate a commit message from the staged diff stats.
    fn generate_message(cwd: Option<&str>) -> Result<String, ToolError> {
        // Get short stat from diff
        let (stat, _, success) = run_git(&["diff", "--stat", "--cached"], cwd)?;
        if !success {
            // Fallback to unstaged diff if no cached changes yet
            let (stat2, _, _) = run_git(&["diff", "--stat"], cwd)?;
            return Ok(Self::message_from_stat(&stat2));
        }
        Ok(Self::message_from_stat(&stat))
    }

    fn message_from_stat(stat: &str) -> String {
        let file_count = stat
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count()
            .saturating_sub(1); // last line is summary

        let summary_line = stat.lines().last().unwrap_or("").trim();

        if file_count == 0 {
            return "chore: update files".to_string();
        }

        // Extract file names for a descriptive message
        let files: Vec<&str> = stat
            .lines()
            .take(file_count)
            .filter_map(|l| l.split_whitespace().next())
            .collect();

        // Detect semantic commit type from file paths
        let commit_type = Self::detect_commit_type(&files);

        if files.len() <= 3 {
            let file_list = files.join(", ");
            format!("{commit_type}: update {file_list}")
        } else {
            format!("{commit_type}: update {file_count} files ({summary_line})")
        }
    }

    /// Detect semantic commit type from the list of changed file paths.
    fn detect_commit_type(files: &[&str]) -> &'static str {
        let all_test = files.iter().all(|f| {
            f.contains("/tests/")
                || f.contains("/test/")
                || f.starts_with("test")
                || f.contains("_test.")
                || f.contains(".test.")
        });
        if all_test && !files.is_empty() {
            return "test";
        }

        let all_docs = files.iter().all(|f| {
            f.ends_with(".md")
                || f.ends_with(".txt")
                || f.ends_with(".rst")
                || f.starts_with("docs/")
        });
        if all_docs && !files.is_empty() {
            return "docs";
        }

        let any_style = files.iter().any(|f| {
            f.ends_with(".css")
                || f.ends_with(".scss")
                || f.ends_with(".less")
                || f.contains("lint")
                || f.contains("fmt")
                || f.contains("clippy")
        });
        if any_style {
            return "style";
        }

        let any_ci = files.iter().any(|f| {
            f.starts_with(".github/")
                || f.contains("ci")
                || f.ends_with("Dockerfile")
                || f.ends_with(".yml")
                || f.ends_with(".yaml")
                || f.ends_with(".toml")
        });
        if any_ci {
            return "ci";
        }

        let any_feat = files.iter().any(|f| {
            f.contains("/src/")
                || f.contains("/lib/")
                || f.contains("/commands/")
                || f.contains("/tools/")
        });
        if any_feat {
            return "feat";
        }

        "chore"
    }
}

#[async_trait]
impl Tool for AutoCommitTool {
    fn name(&self) -> &str {
        "auto_commit"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "message": {
                    "type": "string",
                    "description": "Commit message. Auto-generated from diff if omitted."
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "Show what would be committed without committing.",
                    "default": false
                },
                "add_all": {
                    "type": "boolean",
                    "description": "Stage all tracked changes before committing.",
                    "default": true
                },
                "files": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Specific files to stage (relative paths). Overrides add_all if non-empty."
                },
                "co_author": {
                    "type": "string",
                    "description": "Optional co-author trailer, e.g. 'Claude <noreply@anthropic.com>'"
                }
            }
        })
    }

    async fn execute(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let parsed: AutoCommitInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid auto_commit input: {e}")))?;

        let cwd: Option<&str> = None;

        // Check if we're in a git repo
        let _git_root = find_git_root(cwd)?;

        // Gather current status
        let (status_out, _, status_ok) = run_git(&["status", "--porcelain"], cwd)?;
        if !status_ok {
            return Ok(ToolOutput {
                content: "Failed to check git status.".to_string(),
                is_error: true,
                metadata: HashMap::new(),
            });
        }
        if status_out.trim().is_empty() {
            return Ok(ToolOutput {
                content: "Nothing to commit — working directory is clean.".to_string(),
                is_error: false,
                metadata: HashMap::new(),
            });
        }

        // Safety check: get current branch
        let branch = current_branch(cwd)?;

        // Stage changes: specific files or all tracked
        if !parsed.files.is_empty() {
            // Stage specific files
            let file_args: Vec<&str> = parsed.files.iter().map(|s| s.as_str()).collect();
            let mut add_args = vec!["add"];
            add_args.extend(&file_args);
            let (_, stderr, success) = run_git(&add_args, cwd)?;
            if !success {
                return Ok(ToolOutput {
                    content: format!("Failed to stage files: {stderr}"),
                    is_error: true,
                    metadata: HashMap::new(),
                });
            }
        } else if parsed.add_all {
            let (_, stderr, success) = run_git(&["add", "-u"], cwd)?;
            if !success {
                return Ok(ToolOutput {
                    content: format!("Failed to stage changes: {stderr}"),
                    is_error: true,
                    metadata: HashMap::new(),
                });
            }
        }

        // Get staged diff stats for commit context
        let (stat, _, _) = run_git(&["diff", "--stat", "--cached"], cwd)?;

        // Dry run: show what would be committed
        if parsed.dry_run {
            let message = parsed.message.clone().unwrap_or_else(|| {
                Self::generate_message(cwd).unwrap_or_else(|_| "chore: update files".to_string())
            });
            let co_author_line = parsed
                .co_author
                .as_deref()
                .map(|c| format!("\nCo-Authored-By: {c}"))
                .unwrap_or_default();
            return Ok(ToolOutput {
                content: format!(
                    "[dry-run] Would commit on branch '{branch}':\n{stat}\nMessage: {message}{co_author_line}"
                ),
                is_error: false,
                metadata: HashMap::new(),
            });
        }

        // Generate or use provided commit message
        let message = match parsed.message {
            Some(msg) => msg,
            None => Self::generate_message(cwd)?,
        };

        // Build commit message with optional co-author trailer
        let full_message = match &parsed.co_author {
            Some(co) => format!("{message}\n\nCo-Authored-By: {co}"),
            None => message.clone(),
        };

        // Commit
        let (_, stderr, success) = run_git(&["commit", "-m", &full_message], cwd)?;
        if !success {
            return Ok(ToolOutput {
                content: format!("Commit failed: {stderr}"),
                is_error: true,
                metadata: HashMap::new(),
            });
        }

        // Get the short hash of the new commit
        let (hash, _, _) = run_git(&["rev-parse", "--short", "HEAD"], cwd)?;
        let hash = hash.trim();

        Ok(ToolOutput {
            content: format!("Committed on branch '{branch}': {hash} {message}"),
            is_error: false,
            metadata: HashMap::new(),
        })
    }

    fn category(&self) -> &str {
        "git"
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- GitBranchTool tests ----

    #[test]
    fn test_git_branch_tool_name() {
        let tool = GitBranchTool::new();
        assert_eq!(tool.name(), "GitBranch");
    }

    #[test]
    fn test_git_branch_tool_description() {
        let tool = GitBranchTool::new();
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn test_git_branch_tool_schema() {
        let tool = GitBranchTool::new();
        let schema = tool.input_schema();
        assert_eq!(schema["type"], "object");
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("action"));
        assert!(props.contains_key("name"));
        assert!(props.contains_key("force"));
        assert!(props.contains_key("checkout"));
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("action")));
    }

    #[test]
    fn test_git_branch_tool_category() {
        let tool = GitBranchTool::new();
        assert_eq!(tool.category(), "git");
    }

    #[test]
    fn test_git_branch_input_parsing() {
        let input = json!({"action": "create", "name": "feature/test"});
        let parsed: GitBranchInput = serde_json::from_value(input).unwrap();
        assert_eq!(parsed.action, BranchAction::Create);
        assert_eq!(parsed.name.as_deref(), Some("feature/test"));
    }

    #[test]
    fn test_git_branch_input_list() {
        let input = json!({"action": "list"});
        let parsed: GitBranchInput = serde_json::from_value(input).unwrap();
        assert_eq!(parsed.action, BranchAction::List);
        assert!(parsed.name.is_none());
    }

    #[tokio::test]
    async fn test_git_branch_not_a_repo() {
        // Change to a temp directory that is not a git repo
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let tool = GitBranchTool::new();
        let result = tool.execute(json!({"action": "list"})).await;

        // Should fail because not a git repo
        assert!(result.is_err());
    }

    // ---- GitDiffTool tests ----

    #[test]
    fn test_git_diff_tool_name() {
        let tool = GitDiffTool::new();
        assert_eq!(tool.name(), "GitDiff");
    }

    #[test]
    fn test_git_diff_tool_description() {
        let tool = GitDiffTool::new();
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn test_git_diff_tool_schema() {
        let tool = GitDiffTool::new();
        let schema = tool.input_schema();
        assert_eq!(schema["type"], "object");
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("staged"));
        assert!(props.contains_key("commit_range"));
        assert!(props.contains_key("file"));
        assert!(props.contains_key("context_lines"));
        assert!(props.contains_key("ignore_whitespace"));
        assert!(props.contains_key("stat"));
        // No required fields for diff
        assert!(schema.get("required").is_none());
    }

    #[test]
    fn test_git_diff_input_parsing() {
        let input = json!({"staged": true, "file": "src/main.rs"});
        let parsed: GitDiffInput = serde_json::from_value(input).unwrap();
        assert_eq!(parsed.staged, Some(true));
        assert_eq!(parsed.file.as_deref(), Some("src/main.rs"));
    }

    #[test]
    fn test_git_diff_build_args_staged() {
        let tool = GitDiffTool::new();
        let input = GitDiffInput {
            staged: Some(true),
            commit_range: None,
            file: None,
            context_lines: None,
            ignore_whitespace: None,
            stat: None,
        };
        let args = tool.build_diff_args(&input).unwrap();
        assert!(args.contains(&"--cached".to_string()));
    }

    #[test]
    fn test_git_diff_build_args_commit_range() {
        let tool = GitDiffTool::new();
        let input = GitDiffInput {
            staged: None,
            commit_range: Some("abc123..def456".to_string()),
            file: None,
            context_lines: None,
            ignore_whitespace: None,
            stat: None,
        };
        let args = tool.build_diff_args(&input).unwrap();
        assert!(args.contains(&"abc123..def456".to_string()));
    }

    #[test]
    fn test_git_diff_build_args_file() {
        let tool = GitDiffTool::new();
        let input = GitDiffInput {
            staged: None,
            commit_range: None,
            file: Some("src/lib.rs".to_string()),
            context_lines: Some(5),
            ignore_whitespace: Some(true),
            stat: None,
        };
        let args = tool.build_diff_args(&input).unwrap();
        assert!(args.contains(&"--".to_string()));
        assert!(args.contains(&"src/lib.rs".to_string()));
        assert!(args.contains(&"-U5".to_string()));
        assert!(args.contains(&"-w".to_string()));
    }

    // ---- GitLogTool tests ----

    #[test]
    fn test_git_log_tool_name() {
        let tool = GitLogTool::new();
        assert_eq!(tool.name(), "GitLog");
    }

    #[test]
    fn test_git_log_tool_description() {
        let tool = GitLogTool::new();
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn test_git_log_tool_schema() {
        let tool = GitLogTool::new();
        let schema = tool.input_schema();
        assert_eq!(schema["type"], "object");
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("count"));
        assert!(props.contains_key("author"));
        assert!(props.contains_key("since"));
        assert!(props.contains_key("oneline"));
        assert!(props.contains_key("file"));
        assert!(props.contains_key("patch"));
        assert!(props.contains_key("branch"));
    }

    #[test]
    fn test_git_log_input_parsing() {
        let input = json!({"count": 5, "author": "Alice", "oneline": true});
        let parsed: GitLogInput = serde_json::from_value(input).unwrap();
        assert_eq!(parsed.count, Some(5));
        assert_eq!(parsed.author.as_deref(), Some("Alice"));
        assert_eq!(parsed.oneline, Some(true));
    }

    #[test]
    fn test_git_log_build_args_default() {
        let tool = GitLogTool::new();
        let input = GitLogInput {
            count: None,
            author: None,
            since: None,
            oneline: None,
            file: None,
            patch: None,
            branch: None,
        };
        let args = tool.build_log_args(&input).unwrap();
        assert!(args.contains(&"-10".to_string()));
        // Should contain the format string
        assert!(args.iter().any(|a| a.contains("%h %ad")));
    }

    #[test]
    fn test_git_log_build_args_oneline() {
        let tool = GitLogTool::new();
        let input = GitLogInput {
            count: Some(3),
            author: None,
            since: None,
            oneline: Some(true),
            file: None,
            patch: None,
            branch: None,
        };
        let args = tool.build_log_args(&input).unwrap();
        assert!(args.contains(&"-3".to_string()));
        assert!(args.contains(&"--oneline".to_string()));
    }

    #[test]
    fn test_git_log_build_args_with_filters() {
        let tool = GitLogTool::new();
        let input = GitLogInput {
            count: Some(20),
            author: Some("bob@example.com".to_string()),
            since: Some("2 weeks ago".to_string()),
            oneline: None,
            file: Some("src/lib.rs".to_string()),
            patch: None,
            branch: Some("feature".to_string()),
        };
        let args = tool.build_log_args(&input).unwrap();
        assert!(args.contains(&"-20".to_string()));
        assert!(args.iter().any(|a| a.contains("bob@example.com")));
        assert!(args.iter().any(|a| a.contains("2 weeks ago")));
        assert!(args.contains(&"--".to_string()));
        assert!(args.contains(&"src/lib.rs".to_string()));
        assert!(args.contains(&"feature".to_string()));
    }

    // ---- GitStashTool tests ----

    #[test]
    fn test_git_stash_tool_name() {
        let tool = GitStashTool::new();
        assert_eq!(tool.name(), "GitStash");
    }

    #[test]
    fn test_git_stash_tool_description() {
        let tool = GitStashTool::new();
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn test_git_stash_tool_schema() {
        let tool = GitStashTool::new();
        let schema = tool.input_schema();
        assert_eq!(schema["type"], "object");
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("action"));
        assert!(props.contains_key("message"));
        assert!(props.contains_key("index"));
        assert!(props.contains_key("include_untracked"));
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("action")));
    }

    #[test]
    fn test_git_stash_input_parsing() {
        let input = json!({"action": "push", "message": "WIP: feature x"});
        let parsed: GitStashInput = serde_json::from_value(input).unwrap();
        assert_eq!(parsed.action, StashAction::Push);
        assert_eq!(parsed.message.as_deref(), Some("WIP: feature x"));
    }

    #[test]
    fn test_git_stash_input_default_index() {
        let input = json!({"action": "pop"});
        let parsed: GitStashInput = serde_json::from_value(input).unwrap();
        assert_eq!(parsed.action, StashAction::Pop);
        assert!(parsed.index.is_none());
    }

    // ---- GitSafetyTool tests ----

    #[test]
    fn test_git_safety_tool_name() {
        let tool = GitSafetyTool::new();
        assert_eq!(tool.name(), "GitSafety");
    }

    #[test]
    fn test_git_safety_tool_description() {
        let tool = GitSafetyTool::new();
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn test_git_safety_tool_schema() {
        let tool = GitSafetyTool::new();
        let schema = tool.input_schema();
        assert_eq!(schema["type"], "object");
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("command")));
    }

    #[test]
    fn test_git_safety_block_force_push_main() {
        let tool = GitSafetyTool::new();
        let result = tool.check_command("push --force origin main");
        assert!(!result.allowed);
        assert_eq!(result.risk, "blocked");
    }

    #[test]
    fn test_git_safety_block_force_push_master() {
        let tool = GitSafetyTool::new();
        let result = tool.check_command("push -f origin master");
        assert!(!result.allowed);
        assert_eq!(result.risk, "blocked");
    }

    #[test]
    fn test_git_safety_block_reset_hard() {
        let tool = GitSafetyTool::new();
        let result = tool.check_command("reset --hard HEAD");
        assert!(!result.allowed);
        assert_eq!(result.risk, "blocked");
    }

    #[test]
    fn test_git_safety_block_clean_force() {
        let tool = GitSafetyTool::new();
        let result = tool.check_command("clean -fd");
        assert!(!result.allowed);
        assert_eq!(result.risk, "blocked");
    }

    #[test]
    fn test_git_safety_block_checkout_discard() {
        let tool = GitSafetyTool::new();
        let result = tool.check_command("checkout -- .");
        assert!(!result.allowed);
        assert_eq!(result.risk, "blocked");
    }

    #[test]
    fn test_git_safety_warn_rebase_public() {
        let tool = GitSafetyTool::new();
        let result = tool.check_command("rebase origin/main");
        assert!(result.allowed);
        assert_eq!(result.risk, "warning");
    }

    #[test]
    fn test_git_safety_warn_force_push_feature() {
        let tool = GitSafetyTool::new();
        let result = tool.check_command("push --force origin feature-branch");
        assert!(result.allowed);
        assert_eq!(result.risk, "warning");
    }

    #[test]
    fn test_git_safety_safe_log() {
        let tool = GitSafetyTool::new();
        let result = tool.check_command("log --oneline -10");
        assert!(result.allowed);
        assert_eq!(result.risk, "safe");
    }

    #[test]
    fn test_git_safety_safe_status() {
        let tool = GitSafetyTool::new();
        let result = tool.check_command("status");
        assert!(result.allowed);
        assert_eq!(result.risk, "safe");
    }

    #[test]
    fn test_git_safety_safe_branch_delete() {
        let tool = GitSafetyTool::new();
        let result = tool.check_command("branch -d old-feature");
        assert!(result.allowed);
        assert_eq!(result.risk, "safe");
    }

    #[test]
    fn test_git_safety_warn_restore_staged() {
        let tool = GitSafetyTool::new();
        let result = tool.check_command("restore --staged .");
        assert!(result.allowed);
        assert_eq!(result.risk, "warning");
    }

    #[tokio::test]
    async fn test_git_safety_execute_blocked() {
        let tool = GitSafetyTool::new();
        let result = tool
            .execute(json!({"command": "push --force origin main"}))
            .await
            .unwrap();
        assert!(result.is_error);
        assert!(result.content.contains("BLOCKED"));
    }

    #[tokio::test]
    async fn test_git_safety_execute_safe() {
        let tool = GitSafetyTool::new();
        let result = tool.execute(json!({"command": "status"})).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("SAFE"));
    }

    #[tokio::test]
    async fn test_git_safety_empty_command() {
        let tool = GitSafetyTool::new();
        let result = tool.execute(json!({"command": ""})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_git_safety_invalid_input() {
        let tool = GitSafetyTool::new();
        let result = tool.execute(json!({})).await;
        assert!(result.is_err());
    }

    // ---- Helper function tests ----

    #[test]
    fn test_find_git_root_not_a_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let result = find_git_root(Some(tmp.path().to_str().unwrap()));
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Not a git repository")
        );
    }

    #[test]
    fn test_branch_action_deserialization() {
        let input = json!({"action": "list"});
        let parsed: GitBranchInput = serde_json::from_value(input).unwrap();
        assert_eq!(parsed.action, BranchAction::List);

        let input = json!({"action": "create"});
        let parsed: GitBranchInput = serde_json::from_value(input).unwrap();
        assert_eq!(parsed.action, BranchAction::Create);

        let input = json!({"action": "switch"});
        let parsed: GitBranchInput = serde_json::from_value(input).unwrap();
        assert_eq!(parsed.action, BranchAction::Switch);

        let input = json!({"action": "delete"});
        let parsed: GitBranchInput = serde_json::from_value(input).unwrap();
        assert_eq!(parsed.action, BranchAction::Delete);
    }

    #[test]
    fn test_stash_action_deserialization() {
        let input = json!({"action": "list"});
        let parsed: GitStashInput = serde_json::from_value(input).unwrap();
        assert_eq!(parsed.action, StashAction::List);

        let input = json!({"action": "push"});
        let parsed: GitStashInput = serde_json::from_value(input).unwrap();
        assert_eq!(parsed.action, StashAction::Push);

        let input = json!({"action": "pop"});
        let parsed: GitStashInput = serde_json::from_value(input).unwrap();
        assert_eq!(parsed.action, StashAction::Pop);

        let input = json!({"action": "drop"});
        let parsed: GitStashInput = serde_json::from_value(input).unwrap();
        assert_eq!(parsed.action, StashAction::Drop);

        let input = json!({"action": "apply"});
        let parsed: GitStashInput = serde_json::from_value(input).unwrap();
        assert_eq!(parsed.action, StashAction::Apply);
    }

    #[test]
    fn test_auto_commit_input_deserialization() {
        let input = json!({"message": "feat: add new feature"});
        let parsed: AutoCommitInput = serde_json::from_value(input).unwrap();
        assert_eq!(parsed.message, Some("feat: add new feature".to_string()));
        assert!(!parsed.dry_run);

        let input = json!({"message": "wip", "dry_run": true, "add_all": false});
        let parsed: AutoCommitInput = serde_json::from_value(input).unwrap();
        assert!(parsed.dry_run);
        assert!(!parsed.add_all);
    }

    // =========================================================================
    // EDGE CASE TESTS
    // =========================================================================

    // ---- validate_git_arg edge cases ----

    #[test]
    fn test_validate_git_arg_rejects_empty_string() {
        let result = validate_git_arg("");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("must not be empty"),
            "expected empty message, got: {err}"
        );
    }

    #[test]
    fn test_validate_git_arg_rejects_dash_prefix() {
        let result = validate_git_arg("--flag");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("must not start with"));
    }

    #[test]
    fn test_validate_git_arg_rejects_single_dash() {
        let result = validate_git_arg("-a");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_git_arg_rejects_null_byte() {
        let result = validate_git_arg("branch\0name");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("null bytes"));
    }

    #[test]
    fn test_validate_git_arg_rejects_path_traversal_dotdot_slash() {
        let result = validate_git_arg("../etc/passwd");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Path traversal"));
    }

    #[test]
    fn test_validate_git_arg_rejects_path_traversal_backslash() {
        let result = validate_git_arg("..\\windows\\system32");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Path traversal"));
    }

    #[test]
    fn test_validate_git_arg_rejects_semicolon() {
        let result = validate_git_arg("foo;rm -rf /");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("metacharacters"));
    }

    #[test]
    fn test_validate_git_arg_rejects_ampersand() {
        let result = validate_git_arg("foo&&bar");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_git_arg_rejects_pipe() {
        let result = validate_git_arg("foo|bar");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_git_arg_rejects_dollar() {
        let result = validate_git_arg("foo$BAR");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_git_arg_rejects_backtick() {
        let result = validate_git_arg("foo`cmd`");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_git_arg_rejects_parentheses() {
        assert!(validate_git_arg("foo(bar)").is_err());
        assert!(validate_git_arg("foo{baz}").is_err());
    }

    #[test]
    fn test_validate_git_arg_rejects_overlong_name() {
        let long_name = "x".repeat(257);
        let result = validate_git_arg(&long_name);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too long"));
    }

    #[test]
    fn test_validate_git_arg_accepts_max_length() {
        // Exactly 256 chars should be accepted
        let name = "a".repeat(256);
        let result = validate_git_arg(&name);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_git_arg_accepts_valid_branch_names() {
        assert!(validate_git_arg("main").is_ok());
        assert!(validate_git_arg("feature/my-feature").is_ok());
        assert!(validate_git_arg("fix_123").is_ok());
        assert!(validate_git_arg("release/v2.0").is_ok());
    }

    #[test]
    fn test_validate_git_arg_accepts_unicode() {
        assert!(validate_git_arg("feature/日本語").is_ok());
        assert!(validate_git_arg("branch-ñoño").is_ok());
    }

    #[test]
    fn test_validate_git_arg_accepts_commit_range() {
        // A commit range like abc123..def456 contains dots but not "../"
        assert!(validate_git_arg("abc123..def456").is_ok());
    }

    // ---- find_git_root edge cases ----

    #[test]
    fn test_find_git_root_finds_repo_in_parent() {
        let tmp = tempfile::tempdir().unwrap();
        let git_dir = tmp.path();
        Command::new("git")
            .args(["init"])
            .current_dir(git_dir)
            .output()
            .expect("git init failed");

        let subdir = git_dir.join("a").join("b");
        std::fs::create_dir_all(&subdir).unwrap();

        let root = find_git_root(subdir.to_str()).unwrap();
        assert_eq!(root, git_dir.to_str().unwrap());
    }

    #[test]
    fn test_find_git_root_at_exact_git_dir() {
        let tmp = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init"])
            .current_dir(tmp.path())
            .output()
            .expect("git init failed");

        let root = find_git_root(tmp.path().to_str()).unwrap();
        assert_eq!(root, tmp.path().to_str().unwrap());
    }

    #[test]
    fn test_find_git_root_nonexistent_path() {
        let result = find_git_root(Some("/nonexistent/path/that/does/not/exist"));
        assert!(result.is_err());
    }

    // ---- current_branch edge cases ----

    #[test]
    fn test_current_branch_in_fresh_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();

        Command::new("git")
            .args(["init"])
            .current_dir(cwd)
            .output()
            .expect("git init failed");

        let result = current_branch(Some(cwd.to_str().unwrap()));
        // Fresh repo without commits may return error or a branch name depending on git version
        assert!(result.is_ok() || result.unwrap_err().to_string().contains("Failed to determine"));
    }

    #[test]
    fn test_current_branch_with_commit() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();

        Command::new("git").args(["init"]).current_dir(cwd).output().expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        std::fs::write(cwd.join("README.md"), "hello").unwrap();
        Command::new("git").args(["add", "."]).current_dir(cwd).output().expect("git add");
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(cwd)
            .output()
            .expect("git commit");

        let branch = current_branch(Some(cwd.to_str().unwrap())).unwrap();
        assert!(!branch.is_empty());
        assert_ne!(branch, "HEAD");
    }

    // ---- is_working_dir_dirty edge cases ----

    #[test]
    fn test_is_working_dir_dirty_clean_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();

        Command::new("git").args(["init"]).current_dir(cwd).output().expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        std::fs::write(cwd.join("file.txt"), "content").unwrap();
        Command::new("git").args(["add", "."]).current_dir(cwd).output().expect("git add");
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(cwd)
            .output()
            .expect("git commit");

        assert!(!is_working_dir_dirty(Some(cwd.to_str().unwrap())).unwrap());
    }

    #[test]
    fn test_is_working_dir_dirty_with_untracked() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();

        Command::new("git").args(["init"]).current_dir(cwd).output().expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        std::fs::write(cwd.join("file.txt"), "content").unwrap();
        Command::new("git").args(["add", "."]).current_dir(cwd).output().expect("git add");
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(cwd)
            .output()
            .expect("git commit");

        std::fs::write(cwd.join("new_file.txt"), "new").unwrap();
        assert!(is_working_dir_dirty(Some(cwd.to_str().unwrap())).unwrap());
    }

    #[test]
    fn test_is_working_dir_dirty_with_modified() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();

        Command::new("git").args(["init"]).current_dir(cwd).output().expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        std::fs::write(cwd.join("file.txt"), "content").unwrap();
        Command::new("git").args(["add", "."]).current_dir(cwd).output().expect("git add");
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(cwd)
            .output()
            .expect("git commit");

        std::fs::write(cwd.join("file.txt"), "modified").unwrap();
        assert!(is_working_dir_dirty(Some(cwd.to_str().unwrap())).unwrap());
    }

    // ---- run_git edge cases ----

    #[test]
    fn test_run_git_invalid_subcommand() {
        let (stdout, stderr, success) =
            run_git(&["nonexistent-command-xyz"], None).unwrap();
        assert!(!success);
        assert!(!stderr.is_empty() || !stdout.is_empty());
    }

    #[test]
    fn test_run_git_in_nonexistent_dir() {
        let result = run_git(&["status"], Some("/nonexistent/dir/path"));
        // Command::new may succeed (git handles the error) or fail (OS error)
        if let Ok((_, _, success)) = result {
            assert!(!success);
        }
        // If Err, that's also acceptable (OS-level failure)
    }

    // ---- GitBranchTool edge cases ----

    #[tokio::test]
    async fn test_git_branch_create_without_name() {
        // Set up a git repo so find_git_root succeeds, then create without name
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();
        Command::new("git").args(["init"]).current_dir(cwd).output().expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        std::fs::write(cwd.join("file.txt"), "content").unwrap();
        Command::new("git").args(["add", "."]).current_dir(cwd).output().expect("git add");
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(cwd)
            .output()
            .expect("git commit");

        std::env::set_current_dir(cwd).unwrap();

        let tool = GitBranchTool::new();
        let result = tool.execute(json!({"action": "create"})).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Branch name is required"));
    }

    #[tokio::test]
    async fn test_git_branch_switch_without_name() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();
        Command::new("git").args(["init"]).current_dir(cwd).output().expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        std::fs::write(cwd.join("file.txt"), "content").unwrap();
        Command::new("git").args(["add", "."]).current_dir(cwd).output().expect("git add");
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(cwd)
            .output()
            .expect("git commit");

        std::env::set_current_dir(cwd).unwrap();

        let tool = GitBranchTool::new();
        let result = tool.execute(json!({"action": "switch"})).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Branch name is required"));
    }

    #[tokio::test]
    async fn test_git_branch_delete_without_name() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();
        Command::new("git").args(["init"]).current_dir(cwd).output().expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        std::fs::write(cwd.join("file.txt"), "content").unwrap();
        Command::new("git").args(["add", "."]).current_dir(cwd).output().expect("git add");
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(cwd)
            .output()
            .expect("git commit");

        std::env::set_current_dir(cwd).unwrap();

        let tool = GitBranchTool::new();
        let result = tool.execute(json!({"action": "delete"})).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Branch name is required"));
    }

    #[tokio::test]
    async fn test_git_branch_create_with_injection_attempt() {
        let tool = GitBranchTool::new();
        let result = tool
            .execute(json!({"action": "create", "name": "foo;rm -rf /"}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_git_branch_create_with_dash_prefix() {
        let tool = GitBranchTool::new();
        let result = tool
            .execute(json!({"action": "create", "name": "--flag"}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_git_branch_delete_current_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();

        Command::new("git").args(["init"]).current_dir(cwd).output().expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        std::fs::write(cwd.join("file.txt"), "content").unwrap();
        Command::new("git").args(["add", "."]).current_dir(cwd).output().expect("git add");
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(cwd)
            .output()
            .expect("git commit");

        let branch = current_branch(Some(cwd.to_str().unwrap())).unwrap();
        std::env::set_current_dir(cwd).unwrap();

        let tool = GitBranchTool::new();
        let result = tool
            .execute(json!({"action": "delete", "name": branch}))
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Cannot delete the current branch"));
    }

    #[tokio::test]
    async fn test_git_branch_list_in_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();

        Command::new("git").args(["init"]).current_dir(cwd).output().expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        std::fs::write(cwd.join("file.txt"), "content").unwrap();
        Command::new("git").args(["add", "."]).current_dir(cwd).output().expect("git add");
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(cwd)
            .output()
            .expect("git commit");

        std::env::set_current_dir(cwd).unwrap();

        let tool = GitBranchTool::new();
        let result = tool.execute(json!({"action": "list"})).await.unwrap();
        assert!(!result.is_error);
        assert!(result.metadata.contains_key("current_branch"));
    }

    #[tokio::test]
    async fn test_git_branch_create_and_switch_in_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();

        Command::new("git").args(["init"]).current_dir(cwd).output().expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        std::fs::write(cwd.join("file.txt"), "content").unwrap();
        Command::new("git").args(["add", "."]).current_dir(cwd).output().expect("git add");
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(cwd)
            .output()
            .expect("git commit");

        std::env::set_current_dir(cwd).unwrap();

        let tool = GitBranchTool::new();
        let result = tool
            .execute(json!({"action": "create", "name": "feature/test-branch"}))
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("Created branch"));
        assert_eq!(result.metadata["branch"], json!("feature/test-branch"));
        assert_eq!(result.metadata["checkout"], json!(false));

        let result = tool
            .execute(json!({
                "action": "create",
                "name": "feature/checkout-branch",
                "checkout": true
            }))
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("Created and switched"));
        assert_eq!(result.metadata["checkout"], json!(true));
    }

    #[tokio::test]
    async fn test_git_branch_delete_nonexistent_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();

        Command::new("git").args(["init"]).current_dir(cwd).output().expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        std::fs::write(cwd.join("file.txt"), "content").unwrap();
        Command::new("git").args(["add", "."]).current_dir(cwd).output().expect("git add");
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(cwd)
            .output()
            .expect("git commit");

        std::env::set_current_dir(cwd).unwrap();

        let tool = GitBranchTool::new();
        let result = tool
            .execute(json!({"action": "delete", "name": "nonexistent-branch"}))
            .await
            .unwrap();
        assert!(result.is_error);
        assert!(result.content.contains("Failed to delete"));
    }

    #[tokio::test]
    async fn test_git_branch_force_delete_returns_warning() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();

        Command::new("git").args(["init"]).current_dir(cwd).output().expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        std::fs::write(cwd.join("file.txt"), "content").unwrap();
        Command::new("git").args(["add", "."]).current_dir(cwd).output().expect("git add");
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(cwd)
            .output()
            .expect("git commit");

        Command::new("git")
            .args(["branch", "to-force-delete"])
            .current_dir(cwd)
            .output()
            .expect("git branch");

        std::env::set_current_dir(cwd).unwrap();

        let tool = GitBranchTool::new();
        let result = tool
            .execute(json!({"action": "delete", "name": "to-force-delete", "force": true}))
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("SAFETY WARNING"));
        assert_eq!(result.metadata["force_delete_warning"], json!(true));
    }

    // ---- GitDiffTool edge cases ----

    #[test]
    fn test_git_diff_build_args_stat_mode() {
        let tool = GitDiffTool::new();
        let input = GitDiffInput {
            staged: None,
            commit_range: None,
            file: None,
            context_lines: None,
            ignore_whitespace: None,
            stat: Some(true),
        };
        let args = tool.build_diff_args(&input).unwrap();
        assert!(args.contains(&"--stat".to_string()));
    }

    #[test]
    fn test_git_diff_build_args_all_options() {
        let tool = GitDiffTool::new();
        let input = GitDiffInput {
            staged: Some(true),
            commit_range: Some("abc..def".to_string()),
            file: Some("src/main.rs".to_string()),
            context_lines: Some(10),
            ignore_whitespace: Some(true),
            stat: Some(true),
        };
        let args = tool.build_diff_args(&input).unwrap();
        assert!(args.contains(&"--cached".to_string()));
        assert!(args.contains(&"abc..def".to_string()));
        assert!(args.contains(&"--".to_string()));
        assert!(args.contains(&"src/main.rs".to_string()));
        assert!(args.contains(&"-U10".to_string()));
        assert!(args.contains(&"-w".to_string()));
        assert!(args.contains(&"--stat".to_string()));
        assert!(args.contains(&"--color=never".to_string()));
    }

    #[test]
    fn test_git_diff_build_args_empty_input() {
        let tool = GitDiffTool::new();
        let input = GitDiffInput {
            staged: None,
            commit_range: None,
            file: None,
            context_lines: None,
            ignore_whitespace: None,
            stat: None,
        };
        let args = tool.build_diff_args(&input).unwrap();
        assert!(args.contains(&"--color=never".to_string()));
    }

    #[test]
    fn test_git_diff_build_args_rejects_injection_in_commit_range() {
        let tool = GitDiffTool::new();
        let input = GitDiffInput {
            staged: None,
            commit_range: Some("abc;rm -rf /".to_string()),
            file: None,
            context_lines: None,
            ignore_whitespace: None,
            stat: None,
        };
        let result = tool.build_diff_args(&input);
        assert!(result.is_err());
    }

    #[test]
    fn test_git_diff_build_args_rejects_injection_in_file() {
        let tool = GitDiffTool::new();
        let input = GitDiffInput {
            staged: None,
            commit_range: None,
            file: Some("$(evil)".to_string()),
            context_lines: None,
            ignore_whitespace: None,
            stat: None,
        };
        let result = tool.build_diff_args(&input);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_git_diff_not_in_repo() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let tool = GitDiffTool::new();
        let result = tool.execute(json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_git_diff_no_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();

        Command::new("git").args(["init"]).current_dir(cwd).output().expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        std::fs::write(cwd.join("file.txt"), "content").unwrap();
        Command::new("git").args(["add", "."]).current_dir(cwd).output().expect("git add");
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(cwd)
            .output()
            .expect("git commit");

        std::env::set_current_dir(cwd).unwrap();

        let tool = GitDiffTool::new();
        let result = tool.execute(json!({})).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("No unstaged changes found"));
        assert_eq!(result.metadata["has_changes"], json!(false));
    }

    #[tokio::test]
    async fn test_git_diff_with_unstaged_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();

        Command::new("git").args(["init"]).current_dir(cwd).output().expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        std::fs::write(cwd.join("file.txt"), "content").unwrap();
        Command::new("git").args(["add", "."]).current_dir(cwd).output().expect("git add");
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(cwd)
            .output()
            .expect("git commit");

        std::fs::write(cwd.join("file.txt"), "modified content").unwrap();

        std::env::set_current_dir(cwd).unwrap();

        let tool = GitDiffTool::new();
        let result = tool.execute(json!({})).await.unwrap();
        assert!(!result.is_error);
        assert_eq!(result.metadata["has_changes"], json!(true));
        assert!(result.content.contains("file.txt"));
    }

    // ---- GitLogTool edge cases ----

    #[test]
    fn test_git_log_build_args_count_capped_at_100() {
        let tool = GitLogTool::new();
        let input = GitLogInput {
            count: Some(500),
            author: None,
            since: None,
            oneline: None,
            file: None,
            patch: None,
            branch: None,
        };
        let args = tool.build_log_args(&input).unwrap();
        assert!(args.contains(&"-100".to_string()));
        assert!(!args.contains(&"-500".to_string()));
    }

    #[test]
    fn test_git_log_build_args_with_patch() {
        let tool = GitLogTool::new();
        let input = GitLogInput {
            count: Some(5),
            author: None,
            since: None,
            oneline: None,
            file: None,
            patch: Some(true),
            branch: None,
        };
        let args = tool.build_log_args(&input).unwrap();
        assert!(args.contains(&"-p".to_string()));
    }

    #[test]
    fn test_git_log_build_args_rejects_injection_in_author() {
        let tool = GitLogTool::new();
        let input = GitLogInput {
            count: None,
            author: Some("foo;rm -rf /".to_string()),
            since: None,
            oneline: None,
            file: None,
            patch: None,
            branch: None,
        };
        let result = tool.build_log_args(&input);
        assert!(result.is_err());
    }

    #[test]
    fn test_git_log_build_args_rejects_injection_in_branch() {
        let tool = GitLogTool::new();
        let input = GitLogInput {
            count: None,
            author: None,
            since: None,
            oneline: None,
            file: None,
            patch: None,
            branch: Some("$(evil)".to_string()),
        };
        let result = tool.build_log_args(&input);
        assert!(result.is_err());
    }

    #[test]
    fn test_git_log_build_args_rejects_injection_in_file() {
        let tool = GitLogTool::new();
        let input = GitLogInput {
            count: None,
            author: None,
            since: None,
            oneline: None,
            file: Some("file`cmd`".to_string()),
            patch: None,
            branch: None,
        };
        let result = tool.build_log_args(&input);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_git_log_not_in_repo() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let tool = GitLogTool::new();
        let result = tool.execute(json!({"count": 5})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_git_log_empty_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();

        Command::new("git").args(["init"]).current_dir(cwd).output().expect("git init");

        std::env::set_current_dir(cwd).unwrap();

        let tool = GitLogTool::new();
        let result = tool.execute(json!({"count": 5})).await.unwrap();
        // Empty repo: git log fails, so the tool returns is_error=true with "Log failed" message
        assert!(result.is_error || result.content.contains("No commits found"));
    }

    #[tokio::test]
    async fn test_git_log_single_commit() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();

        Command::new("git").args(["init"]).current_dir(cwd).output().expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        std::fs::write(cwd.join("file.txt"), "content").unwrap();
        Command::new("git").args(["add", "."]).current_dir(cwd).output().expect("git add");
        Command::new("git")
            .args(["commit", "-m", "initial commit"])
            .current_dir(cwd)
            .output()
            .expect("git commit");

        std::env::set_current_dir(cwd).unwrap();

        let tool = GitLogTool::new();
        let result = tool
            .execute(json!({"count": 1, "oneline": true}))
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("initial commit"));
    }

    #[tokio::test]
    async fn test_git_log_unicode_commit_message() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();

        Command::new("git").args(["init"]).current_dir(cwd).output().expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        Command::new("git")
            .args(["config", "user.name", "Tëst Üser"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        std::fs::write(cwd.join("file.txt"), "content").unwrap();
        Command::new("git").args(["add", "."]).current_dir(cwd).output().expect("git add");
        Command::new("git")
            .args(["commit", "-m", "日本語コミットメッセージ"])
            .current_dir(cwd)
            .output()
            .expect("git commit");

        std::env::set_current_dir(cwd).unwrap();

        let tool = GitLogTool::new();
        let result = tool
            .execute(json!({"count": 1, "oneline": true}))
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("日本語"));
    }

    #[tokio::test]
    async fn test_git_log_with_author_filter() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();

        Command::new("git").args(["init"]).current_dir(cwd).output().expect("git init");
        Command::new("git")
            .args(["config", "user.email", "unique@author.com"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        Command::new("git")
            .args(["config", "user.name", "Unique Author"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        std::fs::write(cwd.join("file.txt"), "content").unwrap();
        Command::new("git").args(["add", "."]).current_dir(cwd).output().expect("git add");
        Command::new("git")
            .args(["commit", "-m", "commit by unique"])
            .current_dir(cwd)
            .output()
            .expect("git commit");

        std::env::set_current_dir(cwd).unwrap();

        let tool = GitLogTool::new();
        let result = tool
            .execute(json!({"count": 10, "author": "unique@author.com"}))
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("commit by unique"));
    }

    // ---- GitStashTool edge cases ----

    #[tokio::test]
    async fn test_git_stash_not_in_repo() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let tool = GitStashTool::new();
        let result = tool.execute(json!({"action": "list"})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_git_stash_list_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();

        Command::new("git").args(["init"]).current_dir(cwd).output().expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        std::fs::write(cwd.join("file.txt"), "content").unwrap();
        Command::new("git").args(["add", "."]).current_dir(cwd).output().expect("git add");
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(cwd)
            .output()
            .expect("git commit");

        std::env::set_current_dir(cwd).unwrap();

        let tool = GitStashTool::new();
        let result = tool.execute(json!({"action": "list"})).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("No stashes found"));
        assert_eq!(result.metadata["stash_count"], json!(0));
    }

    #[tokio::test]
    async fn test_git_stash_push_clean_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();

        Command::new("git").args(["init"]).current_dir(cwd).output().expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        std::fs::write(cwd.join("file.txt"), "content").unwrap();
        Command::new("git").args(["add", "."]).current_dir(cwd).output().expect("git add");
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(cwd)
            .output()
            .expect("git commit");

        std::env::set_current_dir(cwd).unwrap();

        let tool = GitStashTool::new();
        let result = tool.execute(json!({"action": "push"})).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("Nothing to stash"));
    }

    #[tokio::test]
    async fn test_git_stash_push_and_list() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();

        Command::new("git").args(["init"]).current_dir(cwd).output().expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        std::fs::write(cwd.join("file.txt"), "content").unwrap();
        Command::new("git").args(["add", "."]).current_dir(cwd).output().expect("git add");
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(cwd)
            .output()
            .expect("git commit");

        std::fs::write(cwd.join("file.txt"), "modified").unwrap();

        std::env::set_current_dir(cwd).unwrap();

        let tool = GitStashTool::new();
        let result = tool
            .execute(json!({"action": "push", "message": "test stash"}))
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("stashed successfully"));

        let result = tool.execute(json!({"action": "list"})).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("test stash"));
        assert_eq!(result.metadata["stash_count"], json!(1));
    }

    #[tokio::test]
    async fn test_git_stash_pop_nonexistent_index() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();

        Command::new("git").args(["init"]).current_dir(cwd).output().expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        std::fs::write(cwd.join("file.txt"), "content").unwrap();
        Command::new("git").args(["add", "."]).current_dir(cwd).output().expect("git add");
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(cwd)
            .output()
            .expect("git commit");

        std::env::set_current_dir(cwd).unwrap();

        let tool = GitStashTool::new();
        let result = tool.execute(json!({"action": "pop", "index": 99})).await.unwrap();
        assert!(result.is_error);
        assert!(result.content.contains("Failed to pop stash"));
    }

    #[tokio::test]
    async fn test_git_stash_drop_nonexistent_index() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();

        Command::new("git").args(["init"]).current_dir(cwd).output().expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        std::fs::write(cwd.join("file.txt"), "content").unwrap();
        Command::new("git").args(["add", "."]).current_dir(cwd).output().expect("git add");
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(cwd)
            .output()
            .expect("git commit");

        std::env::set_current_dir(cwd).unwrap();

        let tool = GitStashTool::new();
        let result = tool.execute(json!({"action": "drop", "index": 5})).await.unwrap();
        assert!(result.is_error);
    }

    #[tokio::test]
    async fn test_git_stash_apply_nonexistent_index() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();

        Command::new("git").args(["init"]).current_dir(cwd).output().expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        std::fs::write(cwd.join("file.txt"), "content").unwrap();
        Command::new("git").args(["add", "."]).current_dir(cwd).output().expect("git add");
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(cwd)
            .output()
            .expect("git commit");

        std::env::set_current_dir(cwd).unwrap();

        let tool = GitStashTool::new();
        let result = tool.execute(json!({"action": "apply", "index": 42})).await.unwrap();
        assert!(result.is_error);
    }

    #[tokio::test]
    async fn test_git_stash_push_with_untracked() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();

        Command::new("git").args(["init"]).current_dir(cwd).output().expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        std::fs::write(cwd.join("file.txt"), "content").unwrap();
        Command::new("git").args(["add", "."]).current_dir(cwd).output().expect("git add");
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(cwd)
            .output()
            .expect("git commit");

        std::fs::write(cwd.join("untracked.txt"), "new").unwrap();

        std::env::set_current_dir(cwd).unwrap();

        let tool = GitStashTool::new();
        let result = tool
            .execute(json!({"action": "push", "include_untracked": true}))
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("stashed successfully"));
    }

    // ---- GitSafetyTool edge cases ----

    #[test]
    fn test_git_safety_block_clean_force_dx() {
        let tool = GitSafetyTool::new();
        let result = tool.check_command("clean -fdx");
        assert!(!result.allowed);
        assert_eq!(result.risk, "blocked");
        assert!(result.message.contains("-fdx"));
    }

    #[test]
    fn test_git_safety_block_clean_force_no_flags() {
        let tool = GitSafetyTool::new();
        let result = tool.check_command("clean --force");
        assert!(!result.allowed);
    }

    #[test]
    fn test_git_safety_safe_checkout_branch() {
        let tool = GitSafetyTool::new();
        let result = tool.check_command("checkout feature-branch");
        assert!(result.allowed);
        assert_eq!(result.risk, "safe");
    }

    #[test]
    fn test_git_safety_case_insensitive() {
        let tool = GitSafetyTool::new();
        let result = tool.check_command("PUSH --FORCE ORIGIN MAIN");
        assert!(!result.allowed);
        assert_eq!(result.risk, "blocked");
    }

    #[test]
    fn test_git_safety_safe_diff() {
        let tool = GitSafetyTool::new();
        let result = tool.check_command("diff HEAD");
        assert!(result.allowed);
        assert_eq!(result.risk, "safe");
    }

    #[test]
    fn test_git_safety_safe_add() {
        let tool = GitSafetyTool::new();
        let result = tool.check_command("add src/main.rs");
        assert!(result.allowed);
        assert_eq!(result.risk, "safe");
    }

    #[test]
    fn test_git_safety_warn_force_push_non_main() {
        let tool = GitSafetyTool::new();
        let result = tool.check_command("push --force origin my-feature");
        assert!(result.allowed);
        assert_eq!(result.risk, "warning");
    }

    #[test]
    fn test_git_safety_empty_string() {
        let tool = GitSafetyTool::new();
        let result = tool.check_command("");
        assert!(result.allowed);
        assert_eq!(result.risk, "safe");
    }

    #[test]
    fn test_git_safety_whitespace_command() {
        let tool = GitSafetyTool::new();
        let result = tool.check_command("   ");
        assert!(result.allowed);
    }

    #[test]
    fn test_git_safety_block_reset_hard_head_n() {
        let tool = GitSafetyTool::new();
        let result = tool.check_command("reset --hard HEAD~3");
        assert!(!result.allowed);
        assert_eq!(result.risk, "blocked");
    }

    #[tokio::test]
    async fn test_git_safety_execute_whitespace_command() {
        let tool = GitSafetyTool::new();
        let result = tool.execute(json!({"command": "   "})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_git_safety_execute_with_metadata() {
        let tool = GitSafetyTool::new();
        let result = tool
            .execute(json!({"command": "log --oneline"}))
            .await
            .unwrap();
        assert!(!result.is_error);
        assert_eq!(result.metadata["allowed"], json!(true));
        assert_eq!(result.metadata["risk"], json!("safe"));
    }

    // ---- AutoCommitTool edge cases ----

    #[test]
    fn test_auto_commit_tool_name() {
        let tool = AutoCommitTool::new();
        assert_eq!(tool.name(), "auto_commit");
    }

    #[test]
    fn test_auto_commit_tool_schema() {
        let tool = AutoCommitTool::new();
        let schema = tool.input_schema();
        assert_eq!(schema["type"], "object");
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("message"));
        assert!(props.contains_key("dry_run"));
        assert!(props.contains_key("add_all"));
        assert!(props.contains_key("files"));
        assert!(props.contains_key("co_author"));
    }

    #[test]
    fn test_auto_commit_input_defaults() {
        let input = json!({});
        let parsed: AutoCommitInput = serde_json::from_value(input).unwrap();
        assert!(parsed.message.is_none());
        assert!(!parsed.dry_run);
        assert!(parsed.add_all);
        assert!(parsed.files.is_empty());
        assert!(parsed.co_author.is_none());
    }

    #[test]
    fn test_detect_commit_type_test_files() {
        assert_eq!(
            AutoCommitTool::detect_commit_type(&["tests/unit_test.rs"]),
            "test"
        );
        assert_eq!(
            AutoCommitTool::detect_commit_type(&["test/helper.js"]),
            "test"
        );
        assert_eq!(
            AutoCommitTool::detect_commit_type(&["foo.test.ts"]),
            "test"
        );
        assert_eq!(
            AutoCommitTool::detect_commit_type(&["bar_test.py"]),
            "test"
        );
    }

    #[test]
    fn test_detect_commit_type_docs() {
        assert_eq!(AutoCommitTool::detect_commit_type(&["README.md"]), "docs");
        assert_eq!(
            AutoCommitTool::detect_commit_type(&["docs/guide.txt"]),
            "docs"
        );
    }

    #[test]
    fn test_detect_commit_type_style() {
        assert_eq!(
            AutoCommitTool::detect_commit_type(&["styles/main.css"]),
            "style"
        );
        assert_eq!(
            AutoCommitTool::detect_commit_type(&["theme.scss"]),
            "style"
        );
    }

    #[test]
    fn test_detect_commit_type_ci() {
        assert_eq!(
            AutoCommitTool::detect_commit_type(&[".github/workflows/ci.yml"]),
            "ci"
        );
        assert_eq!(AutoCommitTool::detect_commit_type(&["Cargo.toml"]), "ci");
        assert_eq!(AutoCommitTool::detect_commit_type(&["Dockerfile"]), "ci");
    }

    #[test]
    fn test_detect_commit_type_feat() {
        assert_eq!(
            AutoCommitTool::detect_commit_type(&["my-crate/src/main.rs"]),
            "feat"
        );
        assert_eq!(
            AutoCommitTool::detect_commit_type(&["my-crate/lib/index.ts"]),
            "feat"
        );
    }

    #[test]
    fn test_detect_commit_type_chore_default() {
        assert_eq!(AutoCommitTool::detect_commit_type(&["random.bin"]), "chore");
    }

    #[test]
    fn test_detect_commit_type_empty() {
        assert_eq!(AutoCommitTool::detect_commit_type(&[]), "chore");
    }

    #[test]
    fn test_message_from_stat_empty() {
        let msg = AutoCommitTool::message_from_stat("");
        assert_eq!(msg, "chore: update files");
    }

    #[test]
    fn test_message_from_stat_single_file() {
        let stat = " crates/shannon-core/src/main.rs | 5 +++--\n 1 file changed, 3 insertions(+), 2 deletions(-)";
        let msg = AutoCommitTool::message_from_stat(stat);
        assert!(msg.starts_with("feat:"));
        assert!(msg.contains("crates/shannon-core/src/main.rs"));
    }

    #[test]
    fn test_message_from_stat_many_files() {
        let stat = " src/main.rs  | 5 +++--\n lib/mod.rs  | 10 +++++-----\n tests/a.rs  | 2 +-\
                     \n tests/b.rs  | 2 +-\
                     \n 4 files changed, 10 insertions(+), 10 deletions(-)";
        let msg = AutoCommitTool::message_from_stat(stat);
        assert!(msg.contains("4 files"));
    }

    #[tokio::test]
    async fn test_auto_commit_not_in_repo() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let tool = AutoCommitTool::new();
        let result = tool.execute(json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_auto_commit_clean_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();

        Command::new("git").args(["init"]).current_dir(cwd).output().expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        std::fs::write(cwd.join("file.txt"), "content").unwrap();
        Command::new("git").args(["add", "."]).current_dir(cwd).output().expect("git add");
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(cwd)
            .output()
            .expect("git commit");

        std::env::set_current_dir(cwd).unwrap();

        let tool = AutoCommitTool::new();
        let result = tool.execute(json!({})).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("Nothing to commit"));
    }

    #[tokio::test]
    async fn test_auto_commit_dry_run() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();

        Command::new("git").args(["init"]).current_dir(cwd).output().expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        std::fs::write(cwd.join("file.txt"), "content").unwrap();
        Command::new("git").args(["add", "."]).current_dir(cwd).output().expect("git add");
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(cwd)
            .output()
            .expect("git commit");

        std::fs::write(cwd.join("file.txt"), "modified").unwrap();

        std::env::set_current_dir(cwd).unwrap();

        let tool = AutoCommitTool::new();
        let result = tool
            .execute(json!({"dry_run": true, "message": "test msg"}))
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("dry-run"));
        assert!(result.content.contains("test msg"));

        // File should still be dirty (not actually committed)
        let (status, _, _) =
            run_git(&["status", "--porcelain"], Some(cwd.to_str().unwrap())).unwrap();
        assert!(!status.trim().is_empty());
    }

    #[tokio::test]
    async fn test_auto_commit_with_co_author() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();

        Command::new("git").args(["init"]).current_dir(cwd).output().expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        std::fs::write(cwd.join("file.txt"), "content").unwrap();
        Command::new("git").args(["add", "."]).current_dir(cwd).output().expect("git add");
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(cwd)
            .output()
            .expect("git commit");

        std::fs::write(cwd.join("file.txt"), "modified again").unwrap();

        std::env::set_current_dir(cwd).unwrap();

        let tool = AutoCommitTool::new();
        let result = tool
            .execute(json!({
                "message": "add co-author",
                "co_author": "Claude <noreply@anthropic.com>"
            }))
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("Committed"));

        let (log, _, _) =
            run_git(&["log", "-1", "--format=%B"], Some(cwd.to_str().unwrap())).unwrap();
        assert!(log.contains("Co-Authored-By: Claude <noreply@anthropic.com>"));
    }

    #[tokio::test]
    async fn test_auto_commit_specific_files() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();

        Command::new("git").args(["init"]).current_dir(cwd).output().expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        std::fs::write(cwd.join("a.txt"), "a").unwrap();
        std::fs::write(cwd.join("b.txt"), "b").unwrap();
        Command::new("git").args(["add", "."]).current_dir(cwd).output().expect("git add");
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(cwd)
            .output()
            .expect("git commit");

        std::fs::write(cwd.join("a.txt"), "a modified").unwrap();
        std::fs::write(cwd.join("b.txt"), "b modified").unwrap();

        std::env::set_current_dir(cwd).unwrap();

        let tool = AutoCommitTool::new();
        let result = tool
            .execute(json!({
                "message": "only a.txt",
                "files": ["a.txt"],
                "add_all": true
            }))
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("Committed"));

        let (status, _, _) =
            run_git(&["status", "--porcelain"], Some(cwd.to_str().unwrap())).unwrap();
        assert!(status.contains("b.txt"));
    }

    // ---- Deserialization edge cases ----

    #[test]
    fn test_branch_action_invalid_value() {
        let input = json!({"action": "invalid_action"});
        let result = serde_json::from_value::<GitBranchInput>(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_stash_action_invalid_value() {
        let input = json!({"action": "invalid_stash"});
        let result = serde_json::from_value::<GitStashInput>(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_branch_action_case_sensitivity() {
        let input = json!({"action": "List"});
        let result = serde_json::from_value::<GitBranchInput>(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_stash_action_case_sensitivity() {
        let input = json!({"action": "PUSH"});
        let result = serde_json::from_value::<GitStashInput>(input);
        assert!(result.is_err());
    }

    // ---- Unicode edge cases in tool execution ----

    #[tokio::test]
    async fn test_git_branch_create_unicode_name() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();

        Command::new("git").args(["init"]).current_dir(cwd).output().expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(cwd)
            .output()
            .expect("git config");
        std::fs::write(cwd.join("file.txt"), "content").unwrap();
        Command::new("git").args(["add", "."]).current_dir(cwd).output().expect("git add");
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(cwd)
            .output()
            .expect("git commit");

        std::env::set_current_dir(cwd).unwrap();

        let tool = GitBranchTool::new();
        let result = tool
            .execute(json!({"action": "create", "name": "feature/日本語"}))
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("日本語"));
    }

    // ---- Category and read-only trait tests ----

    #[test]
    fn test_git_diff_tool_category() {
        let tool = GitDiffTool::new();
        assert_eq!(tool.category(), "git");
    }

    #[test]
    fn test_git_diff_tool_is_read_only() {
        let tool = GitDiffTool::new();
        assert!(tool.is_read_only());
    }

    #[test]
    fn test_git_log_tool_category() {
        let tool = GitLogTool::new();
        assert_eq!(tool.category(), "git");
    }

    #[test]
    fn test_git_log_tool_is_read_only() {
        let tool = GitLogTool::new();
        assert!(tool.is_read_only());
    }

    #[test]
    fn test_git_safety_tool_category() {
        let tool = GitSafetyTool::new();
        assert_eq!(tool.category(), "git");
    }

    #[test]
    fn test_git_safety_tool_is_read_only() {
        let tool = GitSafetyTool::new();
        assert!(tool.is_read_only());
    }

    #[test]
    fn test_git_stash_tool_category() {
        let tool = GitStashTool::new();
        assert_eq!(tool.category(), "git");
    }

    #[test]
    fn test_git_branch_tool_not_read_only() {
        let tool = GitBranchTool::new();
        assert!(!tool.is_read_only());
    }

    #[test]
    fn test_auto_commit_tool_category() {
        let tool = AutoCommitTool::new();
        assert_eq!(tool.category(), "git");
    }
}
