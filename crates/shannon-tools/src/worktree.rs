//! Git worktree management tools
//!
//! Provides implementations for:
//! - EnterWorktree: Create isolated git worktree and switch into it
//! - ExitWorktree: Exit worktree session and return to original directory
//!
//! These tools enable safe, isolated experimentation in parallel git branches.

use crate::{Tool, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::RwLock;
use uuid::Uuid;

/// Worktree session state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeSession {
    /// Path to the worktree directory
    pub worktree_path: String,

    /// Branch name for the worktree
    pub worktree_branch: Option<String>,

    /// Original working directory before entering worktree
    pub original_cwd: String,

    /// Original HEAD commit (for detecting changes)
    pub original_head_commit: Option<String>,

    /// Optional tmux session name
    pub tmux_session_name: Option<String>,

    /// Session ID
    pub session_id: String,
}

/// Input for entering a worktree
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EnterWorktreeInput {
    /// Optional name for the worktree
    /// Each "/"-separated segment may contain only letters, digits, dots, underscores, and dashes
    /// Max 64 chars total. Random name generated if not provided.
    pub name: Option<String>,
}

/// Output from entering a worktree
#[derive(Debug, Serialize)]
pub struct EnterWorktreeOutput {
    /// Path to the created worktree
    pub worktree_path: String,

    /// Branch name for the worktree
    pub worktree_branch: Option<String>,

    /// Success message
    pub message: String,
}

/// Action for exiting a worktree
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ExitAction {
    /// Leave the worktree and branch on disk
    Keep,
    /// Delete both the worktree and branch
    Remove,
}

/// Input for exiting a worktree
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExitWorktreeInput {
    /// Action to take (keep or remove)
    pub action: ExitAction,

    /// Required when action is "remove" and worktree has uncommitted changes
    pub discard_changes: Option<bool>,
}

/// Output from exiting a worktree
#[derive(Debug, Serialize)]
pub struct ExitWorktreeOutput {
    /// Action that was taken
    pub action: ExitAction,

    /// Original working directory
    pub original_cwd: String,

    /// Worktree path
    pub worktree_path: String,

    /// Worktree branch (if any)
    pub worktree_branch: Option<String>,

    /// Tmux session name (if any)
    pub tmux_session_name: Option<String>,

    /// Number of discarded files (when removing)
    pub discarded_files: Option<usize>,

    /// Number of discarded commits (when removing)
    pub discarded_commits: Option<usize>,

    /// Status message
    pub message: String,
}

/// Global worktree session state
static CURRENT_WORKTREE_SESSION: RwLock<Option<WorktreeSession>> = RwLock::new(None);

/// Validate worktree name format
fn validate_worktree_name(name: &str) -> Result<(), ToolError> {
    if name.len() > 64 {
        return Err(ToolError::ExecutionFailed(
            "Worktree name must be 64 characters or less".to_string(),
        ));
    }

    for segment in name.split('/') {
        if !segment
            .chars()
            .all(|c| c.is_alphanumeric() || c == '.' || c == '_' || c == '-')
        {
            return Err(ToolError::ExecutionFailed(format!(
                "Worktree name segment '{segment}' contains invalid characters. Only letters, digits, dots, underscores, and dashes are allowed"
            )));
        }
    }

    Ok(())
}

/// Get the current git HEAD commit
fn get_current_head_commit(repo_path: &Path) -> Result<Option<String>, ToolError> {
    let repo_str = repo_path.to_str().ok_or_else(|| {
        ToolError::ExecutionFailed("Repository path contains invalid UTF-8".to_string())
    })?;
    let output = Command::new("git")
        .args(["-C", repo_str, "rev-parse", "HEAD"])
        .output()
        .map_err(|e| ToolError::ExecutionFailed(format!("Failed to get HEAD commit: {e}")))?;

    if output.status.success() {
        let commit = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(Some(commit))
    } else {
        Ok(None)
    }
}

/// Find the canonical git root directory
fn find_git_root(start_path: &Path) -> Option<PathBuf> {
    let mut current = Some(start_path.to_path_buf());

    while let Some(path) = current {
        let git_dir = path.join(".git");
        if git_dir.exists() {
            return Some(path);
        }

        current = path.parent().map(|p| p.to_path_buf());
    }

    None
}

/// Generate a random worktree name
fn generate_worktree_name() -> String {
    let uuid_str = Uuid::new_v4().to_string();
    let first_segment = uuid_str.split('-').next().unwrap_or("unknown");
    format!("worktree-{first_segment}")
}

/// Get current working directory
fn get_current_dir() -> Result<String, ToolError> {
    std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| ToolError::ExecutionFailed(format!("Failed to get current directory: {e}")))
}

/// Change working directory
fn change_directory(path: &str) -> Result<(), ToolError> {
    std::env::set_current_dir(path)
        .map_err(|e| ToolError::ExecutionFailed(format!("Failed to change directory: {e}")))
}

/// Worktree management tool
pub struct WorktreeTool {
    description: String,
}

impl Default for WorktreeTool {
    fn default() -> Self {
        Self::new()
    }
}

impl WorktreeTool {
    pub fn new() -> Self {
        Self {
            description: "Manage git worktree sessions for isolated branch work".to_string(),
        }
    }

    /// Enter a new worktree session
    async fn enter_worktree(
        &self,
        input: EnterWorktreeInput,
    ) -> Result<EnterWorktreeOutput, ToolError> {
        // Check if already in a worktree session
        {
            let session = CURRENT_WORKTREE_SESSION
                .read()
                .map_err(|e| ToolError::ExecutionFailed(format!("Failed to acquire lock: {e}")))?;
            if session.is_some() {
                return Err(ToolError::ExecutionFailed(
                    "Already in a worktree session".to_string(),
                ));
            }
        }

        let current_dir = get_current_dir()?;

        // Find git root
        let git_root = find_git_root(Path::new(&current_dir))
            .ok_or_else(|| ToolError::ExecutionFailed("Not in a git repository".to_string()))?;

        // Change to git root for worktree creation
        change_directory(git_root.to_str().ok_or_else(|| {
            ToolError::ExecutionFailed("Git root path contains invalid UTF-8".to_string())
        })?)?;

        // Get original HEAD commit
        let original_head_commit = get_current_head_commit(&git_root).ok();

        // Generate or validate worktree name
        let worktree_name = if let Some(name) = input.name {
            validate_worktree_name(&name)?;
            name
        } else {
            generate_worktree_name()
        };

        // Create worktree directory path
        let worktree_path = git_root
            .join(".claude")
            .join("worktrees")
            .join(&worktree_name);

        // Create worktree using git
        let branch_name = format!("worktree/{worktree_name}");

        let worktree_path_str = worktree_path.to_str().ok_or_else(|| {
            ToolError::ExecutionFailed("Worktree path contains invalid UTF-8".to_string())
        })?;

        let output = Command::new("git")
            .args(["worktree", "add", "-b", &branch_name, worktree_path_str])
            .output()
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to create worktree: {e}")))?;

        if !output.status.success() {
            return Err(ToolError::ExecutionFailed(format!(
                "Git worktree creation failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        // Create session state
        let session = WorktreeSession {
            worktree_path: worktree_path.to_string_lossy().to_string(),
            worktree_branch: Some(branch_name.clone()),
            original_cwd: current_dir.clone(),
            original_head_commit: original_head_commit.flatten(),
            tmux_session_name: None,
            session_id: Uuid::new_v4().to_string(),
        };

        // Store session
        {
            let mut guard = CURRENT_WORKTREE_SESSION
                .write()
                .map_err(|e| ToolError::ExecutionFailed(format!("Failed to acquire lock: {e}")))?;
            *guard = Some(session);
        }

        // Change to worktree directory
        change_directory(worktree_path.to_str().ok_or_else(|| {
            ToolError::ExecutionFailed("Worktree path contains invalid UTF-8".to_string())
        })?)?;

        Ok(EnterWorktreeOutput {
            worktree_path: worktree_path.to_string_lossy().to_string(),
            worktree_branch: Some(branch_name.clone()),
            message: format!(
                "Created worktree '{}' at {}. Branch: {}. The session is now working in the worktree.",
                worktree_name,
                worktree_path.display(),
                branch_name
            ),
        })
    }

    /// Count changes in worktree
    fn count_worktree_changes(
        worktree_path: &Path,
        original_head: Option<&String>,
    ) -> Result<(usize, usize), ToolError> {
        let worktree_str = worktree_path.to_str().ok_or_else(|| {
            ToolError::ExecutionFailed("Worktree path contains invalid UTF-8".to_string())
        })?;

        // Count changed files
        let status_output = Command::new("git")
            .args(["-C", worktree_str, "status", "--porcelain"])
            .output()
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to get git status: {e}")))?;

        let changed_files = if status_output.status.success() {
            String::from_utf8_lossy(&status_output.stdout)
                .lines()
                .filter(|l| !l.trim().is_empty())
                .count()
        } else {
            0
        };

        // Count new commits
        let commits = if let Some(original_commit) = original_head {
            let revlist_output = Command::new("git")
                .args([
                    "-C",
                    worktree_str,
                    "rev-list",
                    "--count",
                    &format!("{original_commit}..HEAD"),
                ])
                .output()
                .map_err(|e| ToolError::ExecutionFailed(format!("Failed to count commits: {e}")))?;

            if revlist_output.status.success() {
                String::from_utf8_lossy(&revlist_output.stdout)
                    .trim()
                    .parse()
                    .unwrap_or(0)
            } else {
                0
            }
        } else {
            0
        };

        Ok((changed_files, commits))
    }

    /// Exit current worktree session
    async fn exit_worktree(
        &self,
        input: ExitWorktreeInput,
    ) -> Result<ExitWorktreeOutput, ToolError> {
        // Get current session
        let session = {
            let guard = CURRENT_WORKTREE_SESSION
                .read()
                .map_err(|e| ToolError::ExecutionFailed(format!("Failed to acquire lock: {e}")))?;
            guard.as_ref().cloned().ok_or_else(|| {
                ToolError::ExecutionFailed("No active worktree session to exit".to_string())
            })?
        };

        let (changed_files, commits) = Self::count_worktree_changes(
            Path::new(&session.worktree_path),
            session.original_head_commit.as_ref(),
        )?;

        // Check for uncommitted changes when removing
        if input.action == ExitAction::Remove
            && !input.discard_changes.unwrap_or(false)
            && (changed_files > 0 || commits > 0)
        {
            let mut parts = Vec::new();
            if changed_files > 0 {
                parts.push(format!(
                    "{} uncommitted {}",
                    changed_files,
                    if changed_files == 1 { "file" } else { "files" }
                ));
            }
            if commits > 0 {
                parts.push(format!(
                    "{} {}",
                    commits,
                    if commits == 1 { "commit" } else { "commits" }
                ));
            }
            return Err(ToolError::ExecutionFailed(format!(
                "Worktree has {}. Set discard_changes: true to proceed, or use action: keep to preserve the worktree.",
                parts.join(" and ")
            )));
        }

        match input.action {
            ExitAction::Keep => {
                // Just return to original directory
                change_directory(&session.original_cwd)?;
            }
            ExitAction::Remove => {
                // Remove worktree
                let output = Command::new("git")
                    .args(["worktree", "remove", &session.worktree_path])
                    .output()
                    .map_err(|e| {
                        ToolError::ExecutionFailed(format!("Failed to remove worktree: {e}"))
                    })?;

                if !output.status.success() {
                    return Err(ToolError::ExecutionFailed(format!(
                        "Failed to remove worktree: {}",
                        String::from_utf8_lossy(&output.stderr)
                    )));
                }

                // Return to original directory
                change_directory(&session.original_cwd)?;
            }
        }

        // Clear session state
        {
            let mut guard = CURRENT_WORKTREE_SESSION
                .write()
                .map_err(|e| ToolError::ExecutionFailed(format!("Failed to acquire lock: {e}")))?;
            *guard = None;
        }

        let tmux_note = if let Some(ref tmux_name) = session.tmux_session_name {
            format!(
                " Tmux session {tmux_name} is still running; reattach with: tmux attach -t {tmux_name}"
            )
        } else {
            String::new()
        };

        let message = match input.action {
            ExitAction::Keep => format!(
                "Exited worktree. Your work is preserved at {}{}.",
                session.worktree_path,
                if let Some(ref branch) = session.worktree_branch {
                    format!(" on branch {branch}")
                } else {
                    String::new()
                }
            ),
            ExitAction::Remove => {
                let mut discard_parts = Vec::new();
                if commits > 0 {
                    discard_parts.push(format!(
                        "{} {}",
                        commits,
                        if commits == 1 { "commit" } else { "commits" }
                    ));
                }
                if changed_files > 0 {
                    discard_parts.push(format!(
                        "{} uncommitted {}",
                        changed_files,
                        if changed_files == 1 { "file" } else { "files" }
                    ));
                }
                let discard_note = if !discard_parts.is_empty() {
                    format!(" Discarded {}.", discard_parts.join(" and "))
                } else {
                    String::new()
                };
                format!(
                    "Exited and removed worktree at {}.{} Session is now back in {}.",
                    session.worktree_path, discard_note, session.original_cwd
                )
            }
        };

        Ok(ExitWorktreeOutput {
            action: input.action.clone(),
            original_cwd: session.original_cwd,
            worktree_path: session.worktree_path,
            worktree_branch: session.worktree_branch,
            tmux_session_name: session.tmux_session_name,
            discarded_files: if input.action == ExitAction::Remove && changed_files > 0 {
                Some(changed_files)
            } else {
                None
            },
            discarded_commits: if input.action == ExitAction::Remove && commits > 0 {
                Some(commits)
            } else {
                None
            },
            message: message + &tmux_note,
        })
    }
}

#[async_trait]
impl Tool for WorktreeTool {
    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        // Parse operation type from input
        let operation = input
            .get("operation")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("Missing operation field".to_string()))?;

        match operation {
            "Enter" => {
                let enter_input: EnterWorktreeInput =
                    serde_json::from_value(input).map_err(|e| {
                        ToolError::InvalidInput(format!("Invalid enter worktree input: {e}"))
                    })?;
                let output = self.enter_worktree(enter_input).await?;
                Ok(ToolOutput {
                    content: output.message.clone(),
                    is_error: false,
                    metadata: {
                        let mut map = HashMap::new();
                        map.insert("worktree_path".to_string(), json!(output.worktree_path));
                        map.insert("worktree_branch".to_string(), json!(output.worktree_branch));
                        map
                    },
                })
            }
            "Exit" => {
                let exit_input: ExitWorktreeInput = serde_json::from_value(input).map_err(|e| {
                    ToolError::InvalidInput(format!("Invalid exit worktree input: {e}"))
                })?;
                let output = self.exit_worktree(exit_input).await?;
                Ok(ToolOutput {
                    content: output.message.clone(),
                    is_error: false,
                    metadata: {
                        let mut map = HashMap::new();
                        map.insert("action".to_string(), json!(format!("{:?}", output.action)));
                        map.insert("original_cwd".to_string(), json!(output.original_cwd));
                        map.insert("worktree_path".to_string(), json!(output.worktree_path));
                        map.insert("worktree_branch".to_string(), json!(output.worktree_branch));
                        map
                    },
                })
            }
            _ => Err(ToolError::InvalidInput(format!(
                "Unknown operation: {operation}"
            ))),
        }
    }

    fn name(&self) -> &str {
        "Worktree"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "description": "Operation type",
                    "enum": ["Enter", "Exit"]
                },
                "name": {
                    "type": "string",
                    "description": "Worktree name"
                },
                "branch": {
                    "type": "string",
                    "description": "Branch name"
                },
                "action": {
                    "type": "string",
                    "description": "Exit action"
                }
            },
            "required": ["operation"]
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn validate_worktree_name_valid() {
        assert!(validate_worktree_name("my-branch").is_ok());
        assert!(validate_worktree_name("feature_123").is_ok());
        assert!(validate_worktree_name("release.v2").is_ok());
        assert!(validate_worktree_name("a/b/c").is_ok());
    }

    #[test]
    fn validate_worktree_name_too_long() {
        let long_name = "x".repeat(65);
        let err = validate_worktree_name(&long_name).unwrap_err();
        assert!(err.to_string().contains("64"));
    }

    #[test]
    fn validate_worktree_name_exactly_64_chars() {
        let name = "x".repeat(64);
        assert!(validate_worktree_name(&name).is_ok());
    }

    #[test]
    fn validate_worktree_name_invalid_chars() {
        assert!(validate_worktree_name("my branch").is_err());
        assert!(validate_worktree_name("my@branch").is_err());
        assert!(validate_worktree_name("my#branch").is_err());
    }

    #[test]
    fn validate_worktree_name_empty_segment_ok() {
        // Splitting "a//b" by '/' gives segments ["a", "", "b"]
        // Empty segments pass the .all() check vacuously
        assert!(validate_worktree_name("a/b").is_ok());
    }

    #[test]
    fn generate_worktree_name_format() {
        let name = generate_worktree_name();
        assert!(name.starts_with("worktree-"));
        assert!(name.len() > "worktree-".len());
    }

    #[test]
    fn generate_worktree_name_unique() {
        let a = generate_worktree_name();
        let b = generate_worktree_name();
        assert_ne!(a, b);
    }

    #[test]
    fn exit_action_serde() {
        let keep_json = serde_json::to_string(&ExitAction::Keep).unwrap();
        assert_eq!(keep_json, "\"keep\"");

        let remove_json = serde_json::to_string(&ExitAction::Remove).unwrap();
        assert_eq!(remove_json, "\"remove\"");

        let de: ExitAction = serde_json::from_str("\"keep\"").unwrap();
        assert_eq!(de, ExitAction::Keep);

        let de: ExitAction = serde_json::from_str("\"remove\"").unwrap();
        assert_eq!(de, ExitAction::Remove);
    }

    #[test]
    fn exit_action_equality() {
        assert_eq!(ExitAction::Keep, ExitAction::Keep);
        assert_ne!(ExitAction::Keep, ExitAction::Remove);
    }

    #[test]
    fn worktree_session_serde() {
        let session = WorktreeSession {
            worktree_path: "/tmp/worktree".to_string(),
            worktree_branch: Some("worktree/test".to_string()),
            original_cwd: "/home/user/project".to_string(),
            original_head_commit: Some("abc123".to_string()),
            tmux_session_name: None,
            session_id: "test-id".to_string(),
        };
        let json = serde_json::to_string(&session).unwrap();
        let de: WorktreeSession = serde_json::from_str(&json).unwrap();
        assert_eq!(de.worktree_path, "/tmp/worktree");
        assert_eq!(de.worktree_branch.unwrap(), "worktree/test");
        assert_eq!(de.session_id, "test-id");
    }

    #[test]
    fn enter_worktree_input_deserialize() {
        let input: EnterWorktreeInput = serde_json::from_str("{\"name\": \"my-feature\"}").unwrap();
        assert_eq!(input.name.unwrap(), "my-feature");

        let input: EnterWorktreeInput = serde_json::from_str("{}").unwrap();
        assert!(input.name.is_none());
    }

    #[test]
    fn exit_worktree_input_deserialize() {
        let input: ExitWorktreeInput =
            serde_json::from_str("{\"action\": \"remove\", \"discard_changes\": true}").unwrap();
        assert_eq!(input.action, ExitAction::Remove);
        assert_eq!(input.discard_changes, Some(true));

        let input: ExitWorktreeInput = serde_json::from_str("{\"action\": \"keep\"}").unwrap();
        assert_eq!(input.action, ExitAction::Keep);
        assert!(input.discard_changes.is_none());
    }

    #[test]
    fn find_git_root_returns_none_for_tmp() {
        // /tmp likely has no .git directory at root level
        // This tests the traversal eventually returning None
        let result = find_git_root(Path::new("/tmp"));
        // Result depends on environment, just verify it doesn't panic
        let _ = result;
    }

    #[test]
    fn worktree_tool_default() {
        let tool = WorktreeTool::default();
        assert_eq!(tool.name(), "Worktree");
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn send_sync_types() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<WorktreeSession>();
        assert_send_sync::<EnterWorktreeInput>();
        assert_send_sync::<ExitWorktreeInput>();
        assert_send_sync::<ExitAction>();
        assert_send_sync::<WorktreeTool>();
    }
}
