//! Git worktree isolation for parallel agent development
//!
//! Provides:
//! - `WorktreeManager`: Session-based worktree management for multi-agent coordination
//! - `EnterWorktreeTool` / `ExitWorktreeTool`: Tool trait implementations for the query engine

use crate::error::AgentError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use shannon_core::tools::{Tool, ToolError, ToolOutput, ToolResult};
use shannon_types::recover_lock;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, LazyLock, RwLock};
use tokio::sync::RwLock as AsyncRwLock;

/// Configuration for worktree manager
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeConfig {
    /// Base directory for worktrees
    pub base_dir: PathBuf,
    /// Repository to create worktrees from
    pub repository_path: PathBuf,
    /// Prefix for worktree directories
    pub worktree_prefix: String,
    /// Auto-cleanup on drop
    pub auto_cleanup: bool,
    /// Keep worktree directory after cleanup
    pub keep_directory: bool,
}

impl Default for WorktreeConfig {
    fn default() -> Self {
        Self {
            base_dir: PathBuf::from(".claude/worktrees"),
            repository_path: PathBuf::from("."),
            worktree_prefix: "worktree-".to_string(),
            auto_cleanup: true,
            keep_directory: false,
        }
    }
}

/// Action to take when exiting a worktree session
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExitAction {
    /// Keep the worktree and branch
    Keep,
    /// Remove the worktree but keep the branch
    RemoveWorktree,
    /// Remove both worktree and branch
    RemoveBoth,
}

/// Status of a worktree session
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorktreeStatus {
    /// Session is active
    Active,
    /// Session is being cleaned up
    Cleaning,
    /// Session has been stopped
    Stopped,
    /// Session encountered an error
    Error,
}

/// An active worktree session
#[derive(Debug, Clone)]
pub struct WorktreeSession {
    /// Unique session identifier
    pub id: String,
    /// Path to the worktree directory
    pub path: PathBuf,
    /// Branch name for this worktree
    pub branch_name: String,
    /// Original branch when creating worktree
    pub original_branch: String,
    /// Session status
    pub status: WorktreeStatus,
    /// Session creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Associated agent (if any)
    pub agent: Option<String>,
    /// Session metadata
    pub metadata: HashMap<String, String>,
}

/// Manager for git worktree isolation
pub struct WorktreeManager {
    config: WorktreeConfig,
    active_sessions: AsyncRwLock<HashMap<String, WorktreeSession>>,
}

impl WorktreeManager {
    /// Create a new worktree manager
    pub async fn new(config: WorktreeConfig) -> Result<Self, AgentError> {
        // Ensure base directory exists
        tokio::fs::create_dir_all(&config.base_dir)
            .await
            .map_err(|e| AgentError::Worktree(format!("Failed to create base directory: {e}")))?;

        // Verify we're in a git repository
        let output = Command::new("git")
            .args(["rev-parse", "--is-inside-work-tree"])
            .current_dir(&config.repository_path)
            .output()
            .map_err(|e| AgentError::Worktree(format!("Failed to execute git: {e}")))?;

        if !output.status.success() {
            return Err(AgentError::Worktree("Not in a git repository".to_string()));
        }

        Ok(Self {
            config,
            active_sessions: AsyncRwLock::new(HashMap::new()),
        })
    }

    /// Create a new worktree session
    pub async fn create_session(
        &self,
        name: Option<String>,
        branch_name: Option<String>,
        starting_point: Option<String>,
    ) -> Result<WorktreeSession, AgentError> {
        let session_id = name
            .unwrap_or_else(|| format!("{}{}", self.config.worktree_prefix, uuid::Uuid::new_v4()));

        let branch = branch_name.unwrap_or_else(|| format!("worktree/{session_id}"));

        let worktree_path = self.config.base_dir.join(&session_id);

        // Get current branch
        let original_branch = self
            .get_current_branch()
            .await
            .unwrap_or_else(|_| "HEAD".to_string());

        // Build git worktree add command
        let mut cmd = Command::new("git");
        cmd.args(["worktree", "add", "-b", &branch]);

        if let Some(ref start) = starting_point {
            cmd.arg(start);
        }

        let worktree_str = worktree_path.to_str().ok_or_else(|| {
            AgentError::Worktree(format!(
                "Worktree path is not valid UTF-8: {}",
                worktree_path.display()
            ))
        })?;
        cmd.arg(worktree_str);

        let output = cmd
            .current_dir(&self.config.repository_path)
            .output()
            .map_err(|e| AgentError::Worktree(format!("Failed to execute git: {e}")))?;

        if !output.status.success() {
            return Err(AgentError::Worktree(format!(
                "Failed to create worktree: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        let session = WorktreeSession {
            id: session_id,
            path: worktree_path,
            branch_name: branch,
            original_branch,
            status: WorktreeStatus::Active,
            created_at: chrono::Utc::now(),
            agent: None,
            metadata: HashMap::new(),
        };

        self.active_sessions
            .write()
            .await
            .insert(session.id.clone(), session.clone());

        tracing::info!(
            session_id = %session.id,
            path = %session.path.display(),
            branch = %session.branch_name,
            "Worktree session created"
        );

        Ok(session)
    }

    /// Create a worktree session for a specific agent
    pub async fn create_agent_session(
        &self,
        agent_name: &str,
        task_id: Option<uuid::Uuid>,
    ) -> Result<WorktreeSession, AgentError> {
        let session_id = format!("agent-{}-{}", agent_name, uuid::Uuid::new_v4());
        let branch_name = format!("agent-work/{agent_name}");

        let mut session = self
            .create_session(Some(session_id.clone()), Some(branch_name), None)
            .await?;

        session.agent = Some(agent_name.to_string());

        if let Some(task_id) = task_id {
            session
                .metadata
                .insert("task_id".to_string(), task_id.to_string());
        }

        self.active_sessions
            .write()
            .await
            .insert(session_id.clone(), session.clone());

        Ok(session)
    }

    /// Get an active session by ID
    pub async fn get_session(&self, session_id: &str) -> Option<WorktreeSession> {
        self.active_sessions.read().await.get(session_id).cloned()
    }

    /// Get session by agent name
    pub async fn get_agent_session(&self, agent_name: &str) -> Option<WorktreeSession> {
        let sessions = self.active_sessions.read().await;

        for session in sessions.values() {
            if session.agent.as_deref() == Some(agent_name) {
                return Some(session.clone());
            }
        }

        None
    }

    /// List all active sessions
    pub async fn list_sessions(&self) -> Vec<WorktreeSession> {
        self.active_sessions
            .read()
            .await
            .values()
            .cloned()
            .collect()
    }

    /// Update session metadata
    pub async fn update_session_metadata(
        &self,
        session_id: &str,
        key: String,
        value: String,
    ) -> Result<(), AgentError> {
        let mut sessions = self.active_sessions.write().await;

        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| AgentError::Worktree(format!("Session '{session_id}' not found")))?;

        session.metadata.insert(key, value);

        Ok(())
    }

    /// Exit a worktree session
    pub async fn exit_session(
        &self,
        session_id: &str,
        action: ExitAction,
        discard_changes: bool,
    ) -> Result<(), AgentError> {
        let mut sessions = self.active_sessions.write().await;

        let session = sessions
            .get(session_id)
            .ok_or_else(|| AgentError::Worktree(format!("Session '{session_id}' not found")))?
            .clone();

        // Check for uncommitted changes
        if !discard_changes {
            let has_changes = self.session_has_changes(&session.path).await?;
            if has_changes {
                return Err(AgentError::Worktree(
                    "Session has uncommitted changes. Use discard_changes=true to force exit."
                        .to_string(),
                ));
            }
        }

        let session = sessions
            .remove(session_id)
            .ok_or_else(|| AgentError::Worktree(format!("Session '{session_id}' not found")))?;

        match action {
            ExitAction::Keep => {
                tracing::debug!(session_id = %session_id, "Keeping worktree session");
            }
            ExitAction::RemoveWorktree | ExitAction::RemoveBoth => {
                self.remove_worktree(&session.path).await?;

                if action == ExitAction::RemoveBoth {
                    self.remove_branch(&session.branch_name).await?;
                }
            }
        }

        tracing::debug!(session_id = %session_id, "Exited worktree session");

        Ok(())
    }

    /// Remove a worktree session
    pub async fn remove_session(&self, session_id: &str) -> Result<(), AgentError> {
        self.exit_session(session_id, ExitAction::RemoveBoth, false)
            .await
    }

    /// Clean up all active sessions
    pub async fn cleanup_all(&self) -> Result<(), AgentError> {
        let session_ids: Vec<_> = self.active_sessions.read().await.keys().cloned().collect();

        for session_id in session_ids {
            let _ = self
                .exit_session(&session_id, ExitAction::RemoveWorktree, false)
                .await;
        }

        tracing::debug!("All worktree sessions cleaned up");

        Ok(())
    }

    /// Get the current git branch
    async fn get_current_branch(&self) -> Result<String, AgentError> {
        let output = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(&self.config.repository_path)
            .output()
            .map_err(|e| AgentError::Worktree(format!("Failed to execute git: {e}")))?;

        if !output.status.success() {
            return Err(AgentError::Worktree(
                "Failed to get current branch".to_string(),
            ));
        }

        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(branch)
    }

    /// Check if a worktree has uncommitted changes
    async fn session_has_changes(&self, path: &Path) -> Result<bool, AgentError> {
        let output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(path)
            .output()
            .map_err(|e| AgentError::Worktree(format!("Failed to execute git: {e}")))?;

        Ok(!output.stdout.is_empty())
    }

    /// Remove a worktree
    async fn remove_worktree(&self, path: &Path) -> Result<(), AgentError> {
        let path_str = path.to_str().ok_or_else(|| {
            AgentError::Worktree(format!(
                "Worktree path is not valid UTF-8: {}",
                path.display()
            ))
        })?;
        let output = Command::new("git")
            .args(["worktree", "remove"])
            .arg(path_str)
            .current_dir(&self.config.repository_path)
            .output()
            .map_err(|e| AgentError::Worktree(format!("Failed to execute git: {e}")))?;

        if !output.status.success() {
            return Err(AgentError::Worktree(format!(
                "Failed to remove worktree: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        Ok(())
    }

    /// Remove a branch
    async fn remove_branch(&self, branch_name: &str) -> Result<(), AgentError> {
        let output = Command::new("git")
            .args(["branch", "-D", branch_name])
            .current_dir(&self.config.repository_path)
            .output()
            .map_err(|e| AgentError::Worktree(format!("Failed to execute git: {e}")))?;

        if !output.status.success() {
            tracing::warn!(
                branch = %branch_name,
                error = %String::from_utf8_lossy(&output.stderr),
                "Failed to delete branch"
            );
        }

        Ok(())
    }

    /// Get count of active sessions
    pub async fn session_count(&self) -> usize {
        self.active_sessions.read().await.len()
    }
}

// ---------------------------------------------------------------------------
// Tool trait implementations for the query engine
// ---------------------------------------------------------------------------

/// Input for the enter_worktree tool.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EnterWorktreeToolInput {
    /// Optional worktree name. Auto-generated if omitted.
    pub name: Option<String>,
}

/// Input for the exit_worktree tool.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExitWorktreeToolInput {
    /// "keep" to leave the worktree on disk, "remove" to delete it.
    pub action: String,
    /// Required when action is "remove" and there are uncommitted changes.
    pub discard_changes: Option<bool>,
}

/// Global state tracking the currently active worktree session (process-wide).
static ACTIVE_WORKTREE: LazyLock<Arc<RwLock<Option<WorktreeSession>>>> =
    LazyLock::new(|| Arc::new(RwLock::new(None)));

/// Get a snapshot of the currently active worktree session (if any).
pub fn get_active_worktree() -> Option<WorktreeSession> {
    recover_lock(ACTIVE_WORKTREE.read()).clone()
}

/// Validate that a worktree name contains only safe characters.
fn validate_name(name: &str) -> Result<(), ToolError> {
    if name.is_empty() {
        return Err(ToolError::ExecutionFailed(
            "Worktree name must not be empty".into(),
        ));
    }
    if name.len() > 64 {
        return Err(ToolError::ExecutionFailed(
            "Worktree name must be at most 64 characters".into(),
        ));
    }
    // Block path traversal: reject names that are "." or ".." or composed only of dots
    if name.chars().all(|c| c == '.') {
        return Err(ToolError::ExecutionFailed(
            "Worktree name must not be '.' or '..'".into(),
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '.' || c == '_' || c == '-')
    {
        return Err(ToolError::ExecutionFailed(format!(
            "Worktree name '{name}' contains invalid characters. Use only letters, digits, dots, underscores, and dashes."
        )));
    }
    Ok(())
}

/// Walk upward from `start` to find a directory containing `.git/`.
fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut current = Some(start.to_path_buf());
    while let Some(path) = current {
        if path.join(".git").exists() {
            return Some(path);
        }
        current = path.parent().map(|p| p.to_path_buf());
    }
    None
}

/// Generate a random worktree name.
fn generate_random_name() -> String {
    format!(
        "wt-{}",
        uuid::Uuid::new_v4()
            .to_string()
            .split('-')
            .next()
            .unwrap_or("0")
    )
}

// ---- EnterWorktreeTool ---------------------------------------------------

/// Tool that creates a git worktree and switches the session into it.
pub struct EnterWorktreeTool {
    session: Arc<RwLock<Option<WorktreeSession>>>,
}

impl Default for EnterWorktreeTool {
    fn default() -> Self {
        Self::new()
    }
}

impl EnterWorktreeTool {
    /// Create a tool that uses the shared (process-wide) session state.
    pub fn new() -> Self {
        Self {
            session: Arc::clone(&ACTIVE_WORKTREE),
        }
    }

    /// Create a tool with its own isolated session state (for testing).
    #[cfg(test)]
    pub fn new_isolated() -> (Self, Arc<RwLock<Option<WorktreeSession>>>) {
        let session = Arc::new(RwLock::new(None));
        let tool = Self {
            session: Arc::clone(&session),
        };
        (tool, session)
    }
}

#[async_trait]
impl Tool for EnterWorktreeTool {
    fn name(&self) -> &str {
        "enter_worktree"
    }

    fn description(&self) -> &str {
        "Create an isolated git worktree for safe experimentation"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Worktree name (auto-generated if omitted)"
                }
            }
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let parsed: EnterWorktreeToolInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid enter_worktree input: {e}")))?;

        // Prevent double-entry.
        {
            let guard = self
                .session
                .read()
                .map_err(|e| ToolError::ExecutionFailed(format!("Lock error: {e}")))?;
            if guard.is_some() {
                return Err(ToolError::ExecutionFailed(
                    "Already inside a worktree session".into(),
                ));
            }
        }

        let cwd = std::env::current_dir()
            .map_err(|e| ToolError::ExecutionFailed(format!("Cannot determine cwd: {e}")))?;
        let git_root = find_git_root(&cwd)
            .ok_or_else(|| ToolError::ExecutionFailed("Not in a git repository".into()))?;

        // Resolve / validate name.
        let name = match &parsed.name {
            Some(n) => {
                validate_name(n)?;
                n.clone()
            }
            None => generate_random_name(),
        };

        let worktree_path = git_root.join(".claude").join("worktrees").join(&name);
        let branch = format!("worktree/{name}");

        // Create the worktree.
        let worktree_str = worktree_path.to_str().ok_or_else(|| {
            ToolError::ExecutionFailed(format!(
                "Worktree path is not valid UTF-8: {}",
                worktree_path.display()
            ))
        })?;
        let output = Command::new("git")
            .args(["worktree", "add", "-b", &branch])
            .arg(worktree_str)
            .current_dir(&git_root)
            .output()
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to run git: {e}")))?;

        if !output.status.success() {
            return Err(ToolError::ExecutionFailed(format!(
                "Failed to create worktree: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        let session = WorktreeSession {
            id: name.clone(),
            path: worktree_path.clone(),
            branch_name: branch.clone(),
            original_branch: String::new(),
            status: WorktreeStatus::Active,
            created_at: chrono::Utc::now(),
            agent: None,
            metadata: HashMap::new(),
        };

        {
            let mut guard = self
                .session
                .write()
                .map_err(|e| ToolError::ExecutionFailed(format!("Lock error: {e}")))?;
            *guard = Some(session);
        }

        tracing::info!(
            name = %name,
            path = %worktree_path.display(),
            "Entered worktree"
        );

        Ok(ToolOutput {
            content: format!(
                "Created worktree '{}' at {}.",
                name,
                worktree_path.display()
            ),
            is_error: false,
            metadata: {
                let mut m = HashMap::new();
                m.insert(
                    "worktree_path".into(),
                    json!(worktree_path.to_string_lossy()),
                );
                m.insert("branch".into(), json!(branch));
                m
            },
        })
    }
}

// ---- ExitWorktreeTool ----------------------------------------------------

/// Tool that exits the current worktree session, optionally removing it.
pub struct ExitWorktreeTool {
    session: Arc<RwLock<Option<WorktreeSession>>>,
}

impl Default for ExitWorktreeTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ExitWorktreeTool {
    /// Create a tool that uses the shared (process-wide) session state.
    pub fn new() -> Self {
        Self {
            session: Arc::clone(&ACTIVE_WORKTREE),
        }
    }

    /// Create a tool with its own isolated session state (for testing).
    #[cfg(test)]
    pub fn new_isolated() -> (Self, Arc<RwLock<Option<WorktreeSession>>>) {
        let session = Arc::new(RwLock::new(None));
        let tool = Self {
            session: Arc::clone(&session),
        };
        (tool, session)
    }
}

#[async_trait]
impl Tool for ExitWorktreeTool {
    fn name(&self) -> &str {
        "exit_worktree"
    }

    fn description(&self) -> &str {
        "Exit and optionally remove a git worktree"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["keep", "remove"],
                    "description": "Whether to keep or remove the worktree"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let parsed: ExitWorktreeToolInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid exit_worktree input: {e}")))?;

        let session = {
            let guard = self
                .session
                .read()
                .map_err(|e| ToolError::ExecutionFailed(format!("Lock error: {e}")))?;
            guard
                .as_ref()
                .cloned()
                .ok_or_else(|| ToolError::ExecutionFailed("No active worktree session".into()))?
        };

        let action = parsed.action.as_str();
        let action_lower = action.to_lowercase();

        match action_lower.as_str() {
            "keep" => {
                // Clear session state.
                {
                    let mut guard = self
                        .session
                        .write()
                        .map_err(|e| ToolError::ExecutionFailed(format!("Lock error: {e}")))?;
                    *guard = None;
                }

                tracing::info!(
                    name = %session.id,
                    "Exited worktree (kept)"
                );

                Ok(ToolOutput {
                    content: format!(
                        "Exited worktree. Worktree preserved at {} on branch {}.",
                        session.path.display(),
                        session.branch_name
                    ),
                    is_error: false,
                    metadata: {
                        let mut m = HashMap::new();
                        m.insert("action".into(), json!("keep"));
                        m.insert(
                            "worktree_path".into(),
                            json!(session.path.to_string_lossy()),
                        );
                        m
                    },
                })
            }

            "remove" => {
                // Check for uncommitted changes unless discard_changes is set.
                if !parsed.discard_changes.unwrap_or(false) {
                    let has_changes = has_uncommitted_changes(&session.path)?;
                    if has_changes {
                        return Err(ToolError::ExecutionFailed(
                            "Worktree has uncommitted changes. Set discard_changes: true to force removal.".into(),
                        ));
                    }
                }

                // Remove the worktree via git.
                let output = Command::new("git")
                    .args(["worktree", "remove", "--force"])
                    .arg(session.path.to_str().unwrap_or("."))
                    .output()
                    .map_err(|e| ToolError::ExecutionFailed(format!("Failed to run git: {e}")))?;

                if !output.status.success() {
                    return Err(ToolError::ExecutionFailed(format!(
                        "Failed to remove worktree: {}",
                        String::from_utf8_lossy(&output.stderr)
                    )));
                }

                // Delete the associated branch to prevent orphaned branches.
                let branch_output = Command::new("git")
                    .args(["branch", "-D"])
                    .arg(&session.branch_name)
                    .output();
                match branch_output {
                    Ok(out) if out.status.success() => {
                        tracing::info!(
                            branch = %session.branch_name,
                            "Deleted worktree branch"
                        );
                    }
                    Ok(out) => {
                        // Non-fatal: worktree is already removed, just log the failure.
                        tracing::warn!(
                            branch = %session.branch_name,
                            stderr = %String::from_utf8_lossy(&out.stderr).trim(),
                            "Failed to delete worktree branch (non-fatal)"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            branch = %session.branch_name,
                            error = %e,
                            "Failed to run git branch -D (non-fatal)"
                        );
                    }
                }

                // Clear session state.
                {
                    let mut guard = self
                        .session
                        .write()
                        .map_err(|e| ToolError::ExecutionFailed(format!("Lock error: {e}")))?;
                    *guard = None;
                }

                tracing::info!(
                    name = %session.id,
                    "Exited and removed worktree"
                );

                Ok(ToolOutput {
                    content: format!("Exited and removed worktree at {}.", session.path.display()),
                    is_error: false,
                    metadata: {
                        let mut m = HashMap::new();
                        m.insert("action".into(), json!("remove"));
                        m.insert(
                            "worktree_path".into(),
                            json!(session.path.to_string_lossy()),
                        );
                        m
                    },
                })
            }

            other => Err(ToolError::InvalidInput(format!(
                "Invalid action '{other}'. Expected 'keep' or 'remove'."
            ))),
        }
    }
}

/// Check whether a worktree path has uncommitted changes.
fn has_uncommitted_changes(path: &Path) -> Result<bool, ToolError> {
    let path_str = path.to_str().ok_or_else(|| {
        ToolError::ExecutionFailed(format!("Path is not valid UTF-8: {}", path.display()))
    })?;
    let output = Command::new("git")
        .args(["-C", path_str, "status", "--porcelain"])
        .output()
        .map_err(|e| ToolError::ExecutionFailed(format!("Failed to run git: {e}")))?;

    Ok(!output.stdout.is_empty())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_name_accepts_valid() {
        assert!(validate_name("my-worktree").is_ok());
        assert!(validate_name("worktree_123").is_ok());
        assert!(validate_name("a.b").is_ok());
        assert!(validate_name("ABC").is_ok());
    }

    #[test]
    fn test_validate_name_rejects_invalid() {
        assert!(validate_name("has spaces").is_err());
        assert!(validate_name("has/slash").is_err());
        assert!(validate_name("").is_err());
        // Path traversal attempts
        assert!(validate_name("..").is_err());
        assert!(validate_name(".").is_err());
        assert!(validate_name("...").is_err());
    }

    #[test]
    fn test_validate_name_rejects_too_long() {
        let long_name = "a".repeat(65);
        assert!(validate_name(&long_name).is_err());
        assert!(validate_name(&"a".repeat(64)).is_ok());
    }

    #[test]
    fn test_generate_random_name_format() {
        let name = generate_random_name();
        assert!(name.starts_with("wt-"));
        // UUID segment is 8 hex chars
        assert_eq!(name.len(), "wt-".len() + 8);
    }

    #[test]
    fn test_enter_worktree_input_optional_name() {
        let input = EnterWorktreeToolInput { name: None };
        assert!(input.name.is_none());

        let input = EnterWorktreeToolInput {
            name: Some("test-wt".into()),
        };
        assert_eq!(input.name.as_deref(), Some("test-wt"));
    }

    #[test]
    fn test_exit_worktree_input_parsing() {
        let input = ExitWorktreeToolInput {
            action: "keep".into(),
            discard_changes: None,
        };
        assert_eq!(input.action, "keep");

        let input = ExitWorktreeToolInput {
            action: "remove".into(),
            discard_changes: Some(true),
        };
        assert_eq!(input.action, "remove");
        assert_eq!(input.discard_changes, Some(true));
    }

    #[test]
    fn test_enter_worktree_tool_schema() {
        let tool = EnterWorktreeTool::new();
        assert_eq!(tool.name(), "enter_worktree");
        assert!(!tool.description().is_empty());

        let schema = tool.input_schema();
        assert_eq!(schema["type"], "object");
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("name"));
    }

    #[test]
    fn test_exit_worktree_tool_schema() {
        let tool = ExitWorktreeTool::new();
        assert_eq!(tool.name(), "exit_worktree");
        assert!(!tool.description().is_empty());

        let schema = tool.input_schema();
        assert_eq!(schema["type"], "object");
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("action")));

        let props = schema["properties"].as_object().unwrap();
        let action = &props["action"];
        let enum_vals = action["enum"].as_array().unwrap();
        assert!(enum_vals.contains(&json!("keep")));
        assert!(enum_vals.contains(&json!("remove")));
    }

    #[test]
    fn test_enter_worktree_tool_execute_invalid_json() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (tool, _session) = EnterWorktreeTool::new_isolated();
        let result = rt.block_on(tool.execute(json!({"name": 123})));
        assert!(result.is_err());
    }

    #[test]
    fn test_exit_worktree_tool_execute_no_session() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (tool, _session) = ExitWorktreeTool::new_isolated();

        // The isolated session starts as None, so this should error.

        let result = rt.block_on(tool.execute(json!({"action": "keep"})));
        assert!(
            result.is_err(),
            "Expected error when no active session, got: {result:?}"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("No active worktree session"),
            "Error message: {err}"
        );
    }

    #[test]
    fn test_exit_worktree_tool_execute_invalid_action() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (tool, session) = ExitWorktreeTool::new_isolated();

        // Set up a fake session so we get past the "no session" check.
        {
            let mut guard = session.write().unwrap();
            *guard = Some(WorktreeSession {
                id: "test_invalid_action".into(),
                path: PathBuf::from("/tmp/nonexistent-worktree-invalid"),
                branch_name: "worktree/test-invalid".into(),
                original_branch: String::new(),
                status: WorktreeStatus::Active,
                created_at: chrono::Utc::now(),
                agent: None,
                metadata: HashMap::new(),
            });
        }

        let result = rt.block_on(tool.execute(json!({"action": "invalid"})));
        assert!(
            result.is_err(),
            "Expected error for invalid action, got: {result:?}"
        );
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid action"), "Error message: {err}");
    }

    #[test]
    fn test_find_git_root_finds_repo() {
        // This test runs inside the actual git repo.
        let cwd = std::env::current_dir().unwrap();
        let root = find_git_root(&cwd);
        assert!(root.is_some());
        // The root should contain a .git directory.
        assert!(root.unwrap().join(".git").exists());
    }

    #[test]
    fn test_find_git_root_no_repo() {
        // /tmp is very unlikely to be inside a git repo.
        let result = find_git_root(Path::new("/tmp"));
        // May or may not find one depending on system, so just ensure no panic.
        let _ = result;
    }
}
