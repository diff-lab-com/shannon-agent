//! Git worktree isolation for parallel agent development

use crate::error::AgentError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio::sync::RwLock;

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
    active_sessions: RwLock<HashMap<String, WorktreeSession>>,
}

impl WorktreeManager {
    /// Create a new worktree manager
    pub async fn new(config: WorktreeConfig) -> Result<Self, AgentError> {
        // Ensure base directory exists
        tokio::fs::create_dir_all(&config.base_dir).await
            .map_err(|e| AgentError::Worktree(format!("Failed to create base directory: {}", e)))?;

        // Verify we're in a git repository
        let output = Command::new("git")
            .args(["rev-parse", "--is-inside-work-tree"])
            .current_dir(&config.repository_path)
            .output()
            .map_err(|e| AgentError::Worktree(format!("Failed to execute git: {}", e)))?;

        if !output.status.success() {
            return Err(AgentError::Worktree(
                "Not in a git repository".to_string()
            ));
        }

        Ok(Self {
            config,
            active_sessions: RwLock::new(HashMap::new()),
        })
    }

    /// Create a new worktree session
    pub async fn create_session(
        &self,
        name: Option<String>,
        branch_name: Option<String>,
        starting_point: Option<String>,
    ) -> Result<WorktreeSession, AgentError> {
        let session_id = name.unwrap_or_else(|| {
            format!("{}{}", self.config.worktree_prefix, uuid::Uuid::new_v4())
        });

        let branch = branch_name.unwrap_or_else(|| {
            format!("worktree/{}", session_id)
        });

        let worktree_path = self.config.base_dir.join(&session_id);

        // Get current branch
        let original_branch = self.get_current_branch().await
            .unwrap_or_else(|_| "HEAD".to_string());

        // Build git worktree add command
        let mut cmd = Command::new("git");
        cmd.args(["worktree", "add", "-b", &branch]);

        if let Some(ref start) = starting_point {
            cmd.arg(start);
        }

        cmd.arg(worktree_path.to_str().unwrap());

        let output = cmd
            .current_dir(&self.config.repository_path)
            .output()
            .map_err(|e| AgentError::Worktree(format!("Failed to execute git: {}", e)))?;

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

        self.active_sessions.write().await.insert(session.id.clone(), session.clone());

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
        let branch_name = format!("agent-work/{}", agent_name);

        let mut session = self.create_session(
            Some(session_id.clone()),
            Some(branch_name),
            None,
        ).await?;

        session.agent = Some(agent_name.to_string());

        if let Some(task_id) = task_id {
            session.metadata.insert("task_id".to_string(), task_id.to_string());
        }

        self.active_sessions.write().await.insert(session_id.clone(), session.clone());

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
        self.active_sessions.read().await.values().cloned().collect()
    }

    /// Update session metadata
    pub async fn update_session_metadata(
        &self,
        session_id: &str,
        key: String,
        value: String,
    ) -> Result<(), AgentError> {
        let mut sessions = self.active_sessions.write().await;

        let session = sessions.get_mut(session_id)
            .ok_or_else(|| AgentError::Worktree(format!("Session '{}' not found", session_id)))?;

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

        let session = sessions.get(session_id)
            .ok_or_else(|| AgentError::Worktree(format!("Session '{}' not found", session_id)))?
            .clone();

        // Check for uncommitted changes
        if !discard_changes {
            let has_changes = self.session_has_changes(&session.path).await?;
            if has_changes {
                return Err(AgentError::Worktree(
                    "Session has uncommitted changes. Use discard_changes=true to force exit.".to_string()
                ));
            }
        }

        let session = sessions.remove(session_id)
            .ok_or_else(|| AgentError::Worktree(format!("Session '{}' not found", session_id)))?;

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
        self.exit_session(session_id, ExitAction::RemoveBoth, false).await
    }

    /// Clean up all active sessions
    pub async fn cleanup_all(&self) -> Result<(), AgentError> {
        let session_ids: Vec<_> = self.active_sessions.read().await.keys().cloned().collect();

        for session_id in session_ids {
            let _ = self.exit_session(&session_id, ExitAction::RemoveWorktree, false).await;
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
            .map_err(|e| AgentError::Worktree(format!("Failed to execute git: {}", e)))?;

        if !output.status.success() {
            return Err(AgentError::Worktree(
                "Failed to get current branch".to_string()
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
            .map_err(|e| AgentError::Worktree(format!("Failed to execute git: {}", e)))?;

        Ok(!output.stdout.is_empty())
    }

    /// Remove a worktree
    async fn remove_worktree(&self, path: &Path) -> Result<(), AgentError> {
        let output = Command::new("git")
            .args(["worktree", "remove"])
            .arg(path.to_str().unwrap())
            .current_dir(&self.config.repository_path)
            .output()
            .map_err(|e| AgentError::Worktree(format!("Failed to execute git: {}", e)))?;

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
            .map_err(|e| AgentError::Worktree(format!("Failed to execute git: {}", e)))?;

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
