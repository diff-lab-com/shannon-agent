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
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, LazyLock, RwLock};
use tokio::sync::RwLock as AsyncRwLock;

/// Per-worktree session manifest (F28). The manager's in-memory
/// `active_sessions` map dies with the manager, so agent worktrees created
/// through a throwaway manager (e.g. the `/team add` path) record themselves
/// on disk instead; the startup sweep uses these manifests to find and
/// remove worktrees whose session no longer exists.
const SESSION_MANIFEST_FILE: &str = ".shannon-worktree-session.json";

/// On-disk record of a created worktree session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeSessionManifest {
    /// Session id (matches the worktree directory name)
    pub session_id: String,
    /// Branch the worktree checks out
    pub branch: String,
    /// Associated agent, if this is an agent worktree
    #[serde(default)]
    pub agent: Option<String>,
    /// RFC 3339 creation timestamp
    pub created_at: String,
}

/// Classification of uncommitted work in a worktree (F30).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirtKind {
    /// `git status --porcelain` output is empty.
    Clean,
    /// Only untracked (`??`) entries — build artifacts and the like; safe
    /// to discard because no tracked file was touched.
    UntrackedOnly,
    /// Modified / staged / deleted tracked files — real work that must be
    /// preserved unless explicitly discarded.
    TrackedChanges,
}

/// Outcome of [`WorktreeManager::sweep_orphaned_sessions`] (F28).
#[derive(Debug, Clone, Default)]
pub struct OrphanSweepReport {
    /// Orphaned worktree directories that were removed.
    pub removed: Vec<PathBuf>,
    /// Worktrees that were kept, with the reason (e.g. tracked changes).
    pub failed: Vec<(PathBuf, String)>,
}

/// Write the per-worktree session manifest (F28).
fn write_session_manifest(
    worktree_path: &Path,
    manifest: &WorktreeSessionManifest,
) -> Result<(), AgentError> {
    let json = serde_json::to_string_pretty(manifest).map_err(AgentError::Serialization)?;
    std::fs::write(worktree_path.join(SESSION_MANIFEST_FILE), json).map_err(AgentError::Io)
}

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
            path: worktree_path.clone(),
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

        // F28: record the session on disk so a later manager (whose
        // in-memory registry starts empty) can discover and clean it up.
        if let Err(e) = write_session_manifest(
            &worktree_path,
            &WorktreeSessionManifest {
                session_id: session.id.clone(),
                branch: session.branch_name.clone(),
                agent: None,
                created_at: session.created_at.to_rfc3339(),
            },
        ) {
            tracing::warn!(
                path = %worktree_path.display(),
                error = %e,
                "Failed to write worktree session manifest"
            );
        }

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
        let uid = uuid::Uuid::new_v4();
        let session_id = format!("agent-{agent_name}-{uid}");
        // F28: the branch must carry a unique suffix too. A plain
        // `agent-work/<name>` branch collides on the second `/team add` of
        // the same agent name (`git worktree add -b` fails), which is also
        // what turned the first session into an orphan.
        let short_uid = uid.simple().to_string()[..8].to_string();
        let branch_name = format!("agent-work/{agent_name}-{short_uid}");

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

        // Refresh the on-disk manifest with the agent association.
        if let Err(e) = write_session_manifest(
            &session.path,
            &WorktreeSessionManifest {
                session_id: session.id.clone(),
                branch: session.branch_name.clone(),
                agent: Some(agent_name.to_string()),
                created_at: session.created_at.to_rfc3339(),
            },
        ) {
            tracing::warn!(
                path = %session.path.display(),
                error = %e,
                "Failed to update worktree session manifest with agent"
            );
        }

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

        // Check for uncommitted changes (F30: untracked-only dirt — build
        // artifacts and the like — does not block removal; only *tracked*
        // modifications do).
        let dirt = self.session_dirt_kind(&session.path).await?;
        if !discard_changes {
            match dirt {
                DirtKind::Clean => {}
                DirtKind::UntrackedOnly => {
                    tracing::info!(
                        session_id = %session_id,
                        "Session has only untracked files; treating as removable"
                    );
                }
                DirtKind::TrackedChanges => {
                    return Err(AgentError::Worktree(
                        "Session has uncommitted tracked changes. Use discard_changes=true to force exit."
                            .to_string(),
                    ));
                }
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
                // git worktree remove refuses dirty worktrees: force when we
                // are intentionally discarding untracked-only content or the
                // user explicitly discarded changes.
                let force = discard_changes || dirt == DirtKind::UntrackedOnly;
                self.remove_worktree_opts(&session.path, force).await?;

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

    /// Clean up all active sessions (F30: failures are logged and counted
    /// instead of silently swallowed).
    pub async fn cleanup_all(&self) -> Result<(), AgentError> {
        let session_ids: Vec<_> = self.active_sessions.read().await.keys().cloned().collect();

        let mut failed: Vec<(String, AgentError)> = Vec::new();
        for session_id in session_ids {
            if let Err(e) = self
                .exit_session(&session_id, ExitAction::RemoveWorktree, false)
                .await
            {
                failed.push((session_id, e));
            }
        }

        if failed.is_empty() {
            tracing::debug!("All worktree sessions cleaned up");
        } else {
            for (session_id, error) in &failed {
                tracing::warn!(
                    session_id = %session_id,
                    error = %error,
                    "Worktree session not cleaned up (kept on disk)"
                );
            }
            tracing::warn!(
                count = failed.len(),
                "worktree cleanup: {} session(s) preserved due to tracked changes; \
                 remove manually or exit with discard_changes=true",
                failed.len()
            );
        }

        Ok(())
    }

    /// F28: remove agent worktree sessions this manager does not have
    /// registered. Agent sessions record a manifest on disk at creation;
    /// any `agent-*` directory under the base dir without a live session
    /// here is a leftover from a dropped manager (e.g. a previous process)
    /// and is removed — untracked-only content is force-removed, worktrees
    /// with tracked changes are kept and reported.
    pub async fn sweep_orphaned_sessions(&self) -> Result<OrphanSweepReport, AgentError> {
        let live: HashSet<String> = self.active_sessions.read().await.keys().cloned().collect();
        let mut report = OrphanSweepReport::default();

        let entries = match std::fs::read_dir(&self.config.base_dir) {
            Ok(entries) => entries,
            Err(_) => return Ok(report), // no base dir → nothing to sweep
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !dir_name.starts_with("agent-") || live.contains(dir_name) {
                continue;
            }

            let branch = std::fs::read_to_string(path.join(SESSION_MANIFEST_FILE))
                .ok()
                .and_then(|s| serde_json::from_str::<WorktreeSessionManifest>(&s).ok())
                .map(|m| m.branch);

            match self.session_dirt_kind(&path).await {
                Ok(DirtKind::TrackedChanges) => {
                    report
                        .failed
                        .push((path.clone(), "has tracked changes".to_string()));
                    continue;
                }
                Ok(DirtKind::Clean | DirtKind::UntrackedOnly) => {}
                Err(e) => {
                    report.failed.push((path.clone(), e.to_string()));
                    continue;
                }
            }

            match self.remove_worktree_opts(&path, true).await {
                Ok(()) => {
                    if let Some(branch) = branch {
                        // remove_branch logs its own failures and is
                        // deliberately non-fatal (the worktree is gone).
                        let _ = self.remove_branch(&branch).await;
                    }
                    tracing::info!(
                        path = %path.display(),
                        "Removed orphaned agent worktree"
                    );
                    report.removed.push(path);
                }
                Err(e) => report.failed.push((path, e.to_string())),
            }
        }

        Ok(report)
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

    /// Classify the uncommitted work in a worktree (F30).
    ///
    /// `git status --porcelain` reports untracked files as `??` entries and
    /// tracked modifications with a status letter. Only tracked changes
    /// represent work that must be preserved; untracked-only dirt (build
    /// artifacts, logs) is safe to discard.
    async fn session_dirt_kind(&self, path: &Path) -> Result<DirtKind, AgentError> {
        let output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(path)
            .output()
            .map_err(|e| AgentError::Worktree(format!("Failed to execute git: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let entries: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
        if entries.is_empty() {
            return Ok(DirtKind::Clean);
        }
        if entries.iter().all(|l| l.starts_with("??")) {
            return Ok(DirtKind::UntrackedOnly);
        }
        Ok(DirtKind::TrackedChanges)
    }

    /// Remove a worktree, optionally forcing through untracked/modified
    /// content that plain `git worktree remove` refuses to delete.
    async fn remove_worktree_opts(&self, path: &Path, force: bool) -> Result<(), AgentError> {
        let path_str = path.to_str().ok_or_else(|| {
            AgentError::Worktree(format!(
                "Worktree path is not valid UTF-8: {}",
                path.display()
            ))
        })?;
        let mut cmd = Command::new("git");
        cmd.args(["worktree", "remove"]);
        if force {
            cmd.arg("--force");
        }
        let output = cmd
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
#[allow(clippy::unwrap_used)]
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

    // ── F28 / F30: real-git-repo worktree lifecycle tests ──────────────

    /// Create a throwaway git repo with one commit (RAII: TempDir removes
    /// everything on drop).
    fn init_git_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let run = |args: &[&str]| {
            let out = Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .output()
                .expect("git should be runnable");
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        run(&["init"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        std::fs::write(dir.path().join("README.md"), "seed\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-m", "init"]);
        dir
    }

    async fn manager_for(repo: &tempfile::TempDir) -> WorktreeManager {
        let config = WorktreeConfig {
            base_dir: repo.path().join(".claude").join("worktrees"),
            repository_path: repo.path().to_path_buf(),
            ..Default::default()
        };
        WorktreeManager::new(config)
            .await
            .expect("worktree manager should build in a git repo")
    }

    /// F28 regression: branches carry a unique suffix, so adding two agent
    /// sessions with the same name must both succeed with distinct branches
    /// (the old `agent-work/<name>` branch collided on the second add).
    #[tokio::test]
    async fn create_agent_session_same_name_twice_distinct_branches() {
        let repo = init_git_repo();
        let manager = manager_for(&repo).await;
        let s1 = manager.create_agent_session("alice", None).await.unwrap();
        let s2 = manager.create_agent_session("alice", None).await.unwrap();

        assert_ne!(s1.branch_name, s2.branch_name, "branches must differ");
        assert_ne!(s1.path, s2.path, "worktree dirs must differ");
        assert!(s1.branch_name.starts_with("agent-work/alice-"));
    }

    /// F28 regression: a worktree created through a throwaway manager (the
    /// `/team add` path) must be discoverable and removable by a fresh
    /// manager's orphan sweep, and live sessions must be left alone.
    #[tokio::test]
    async fn sweep_removes_orphaned_agent_worktrees_but_spares_live_sessions() {
        let repo = init_git_repo();

        // Throwaway manager: dropped immediately, its in-memory session
        // registry is gone — exactly the leak the /team add path has.
        let session = {
            let manager = manager_for(&repo).await;
            manager
                .create_agent_session("bob", None)
                .await
                .expect("agent session should be created")
        };
        assert!(session.path.exists());

        let fresh = manager_for(&repo).await;
        let report = fresh.sweep_orphaned_sessions().await.unwrap();
        assert_eq!(report.removed.len(), 1, "orphan must be removed");
        assert!(
            report.removed[0] == session.path,
            "removed path should match the orphan"
        );
        assert!(!session.path.exists(), "orphan worktree dir must be gone");

        // A session live in THIS manager is not an orphan.
        let live = fresh.create_agent_session("carol", None).await.unwrap();
        let report = fresh.sweep_orphaned_sessions().await.unwrap();
        assert!(report.removed.is_empty(), "live session must be spared");
        assert!(live.path.exists());
    }

    /// F30 regression: a worktree containing only untracked files must be
    /// removed by cleanup_all (build artifacts don't strand the worktree).
    #[tokio::test]
    async fn cleanup_all_removes_worktree_with_only_untracked_files() {
        let repo = init_git_repo();
        let manager = manager_for(&repo).await;
        let session = manager.create_session(None, None, None).await.unwrap();

        std::fs::write(session.path.join("build-artifact.log"), "junk\n").unwrap();

        manager.cleanup_all().await.unwrap();

        assert!(
            !session.path.exists(),
            "untracked-only worktree must be removed"
        );
        assert_eq!(manager.session_count().await, 0);
    }

    /// F30: tracked modifications still preserve the worktree (and the
    /// failure is reported, not swallowed).
    #[tokio::test]
    async fn cleanup_all_preserves_worktree_with_tracked_changes() {
        let repo = init_git_repo();
        let manager = manager_for(&repo).await;
        let session = manager.create_session(None, None, None).await.unwrap();

        // Modify a tracked file inside the worktree.
        std::fs::write(session.path.join("README.md"), "real work\n").unwrap();

        manager.cleanup_all().await.unwrap();

        assert!(
            session.path.exists(),
            "tracked changes must preserve the worktree"
        );
        assert_eq!(
            manager.session_count().await,
            1,
            "session stays registered for a later discard_changes exit"
        );
    }
}
