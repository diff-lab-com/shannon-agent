//! File-based persistence for team and task state
//!
//! Follows Claude Code's approach: flat JSON files on disk for all team state.
//! - Teams: `{base}/teams/{team_name}/config.json`
//! - Tasks: `{base}/tasks/{team_name}/{task_id}.json`
//! - High-watermark: `{base}/tasks/{team_name}/.highwatermark`
//! - Messages: `{base}/teams/{team_name}/inboxes/{agent}.jsonl`
//!
//! Dual-path support: prefers `.claude/` (Claude Code compat) with
//! `.shannon/` fallback.
//!
//! File locking (`flock` via `fs2`) is used for concurrent access safety.

use crate::error::AgentError;
use crate::task::{AgentTask, TaskPriority, TaskStatus};
use crate::teammate::TeammateConfig;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Reject path components that could enable directory traversal.
fn sanitize_path_component(name: &str, label: &str) -> Result<(), AgentError> {
    if name.is_empty() || name == "." || name == ".." || name.chars().all(|c| c == '.') {
        return Err(AgentError::Configuration(format!(
            "{label} must not be empty or all-dots"
        )));
    }
    if name.contains('/') || name.contains('\\') {
        return Err(AgentError::Configuration(format!(
            "{label} must not contain path separators"
        )));
    }
    if name.contains("..") {
        return Err(AgentError::Configuration(format!(
            "{label} must not contain '..'"
        )));
    }
    Ok(())
}

// ── Persisted data structures ────────────────────────────────────────

/// Persisted team configuration, written to `config.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamConfigFile {
    /// Team name
    pub name: String,
    /// Team description
    pub description: String,
    /// Team members with their configs
    pub members: HashMap<String, TeammateConfig>,
    /// Creation timestamp
    pub created_at: String,
    /// Current round-robin assignment index
    pub assignment_index: usize,
}

/// Persisted task file, one per task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskFile {
    /// Unique task identifier
    pub id: String,
    /// Task subject
    pub subject: String,
    /// Task description
    pub description: String,
    /// Current status
    pub status: String,
    /// Priority level
    pub priority: String,
    /// Assigned agent (if any)
    pub owner: Option<String>,
    /// Task IDs this is blocked by
    pub blocked_by: Vec<String>,
    /// Task IDs this blocks
    pub blocks: Vec<String>,
    /// Present continuous form
    pub active_form: Option<String>,
    /// Required capabilities
    pub required_capabilities: Vec<String>,
    /// Additional metadata
    pub metadata: serde_json::Value,
    /// Creation timestamp
    pub created_at: String,
    /// Last update timestamp
    pub updated_at: String,
}

/// Message delivered to an agent's inbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxMessage {
    /// Message ID
    pub id: String,
    /// Sender
    pub from: String,
    /// Message content
    pub content: String,
    /// Timestamp
    pub timestamp: String,
    /// Whether the message has been read
    pub read: bool,
}

// ── Persistence manager ──────────────────────────────────────────────

/// Manages file-based persistence of team and task state.
///
/// Uses a dual-path strategy:
/// 1. Prefer `$HOME/.claude/` for Claude Code compatibility
/// 2. Fall back to `$HOME/.shannon/` if `.claude/` is unavailable
///
/// Override the base directory with `SHANNON_HOME` env var.
pub struct FilePersistence {
    /// Resolved base directory for all Shannon data
    base_dir: PathBuf,
}

impl FilePersistence {
    /// Create a new FilePersistence using the dual-path resolution strategy.
    ///
    /// Resolution order:
    /// 1. `SHANNON_HOME` env var (explicit override)
    /// 2. `$HOME/.claude/` (Claude Code compat)
    /// 3. `$HOME/.shannon/` (Shannon fallback)
    pub fn new() -> Result<Self, AgentError> {
        let home = dirs::home_dir()
            .ok_or_else(|| AgentError::Configuration("Cannot determine home directory".into()))?;

        let base_dir = if let Ok(home_var) = std::env::var("SHANNON_HOME") {
            PathBuf::from(home_var)
        } else {
            let claude_dir = home.join(".claude");
            if claude_dir.exists() {
                claude_dir
            } else {
                home.join(".shannon")
            }
        };

        Ok(Self { base_dir })
    }

    /// Create a FilePersistence with a specific base directory (for testing).
    pub fn with_base_dir(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    /// Get the base directory path.
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    // ── Team operations ──────────────────────────────────────────────

    /// Save a team's configuration to disk.
    pub fn save_team(&self, team: &TeamConfigFile) -> Result<(), AgentError> {
        let team_dir = self.team_dir(&team.name)?;
        std::fs::create_dir_all(&team_dir).map_err(AgentError::Io)?;

        let config_path = team_dir.join("config.json");
        let json = serde_json::to_string_pretty(team).map_err(AgentError::Serialization)?;

        // Write atomically via temp file + rename
        let tmp_path = config_path.with_extension("json.tmp");
        std::fs::write(&tmp_path, &json).map_err(AgentError::Io)?;
        std::fs::rename(&tmp_path, &config_path).map_err(AgentError::Io)?;

        Ok(())
    }

    /// Load a team's configuration from disk.
    pub fn load_team(&self, team_name: &str) -> Result<TeamConfigFile, AgentError> {
        let config_path = self.team_dir(team_name)?.join("config.json");
        let json = std::fs::read_to_string(&config_path).map_err(AgentError::Io)?;
        serde_json::from_str(&json).map_err(AgentError::Serialization)
    }

    /// List all team names on disk.
    pub fn list_teams(&self) -> Result<Vec<String>, AgentError> {
        let teams_dir = self.base_dir.join("teams");
        if !teams_dir.exists() {
            return Ok(Vec::new());
        }

        let mut teams = Vec::new();
        for entry in std::fs::read_dir(&teams_dir).map_err(AgentError::Io)? {
            let entry = entry.map_err(AgentError::Io)?;
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                let config_path = entry.path().join("config.json");
                if config_path.exists() {
                    if let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) {
                        teams.push(name);
                    }
                }
            }
        }

        Ok(teams)
    }

    /// Delete a team directory from disk.
    pub fn delete_team(&self, team_name: &str) -> Result<(), AgentError> {
        let team_dir = self.team_dir(team_name)?;
        if team_dir.exists() {
            std::fs::remove_dir_all(&team_dir).map_err(AgentError::Io)?;
        }
        Ok(())
    }

    // ── Task operations (with file locking) ──────────────────────────

    /// Save a task to disk with file locking for concurrency safety.
    pub fn save_task(&self, team_name: &str, task: &TaskFile) -> Result<(), AgentError> {
        let tasks_dir = self.tasks_dir(team_name)?;
        std::fs::create_dir_all(&tasks_dir).map_err(AgentError::Io)?;

        let task_path = tasks_dir.join(format!("{}.json", task.id));
        let lock_path = tasks_dir.join(format!("{}.lock", task.id));

        // Acquire exclusive lock
        let lock_file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(AgentError::Io)?;
        FileExt::lock_exclusive(&lock_file).map_err(|e| {
            AgentError::Io(std::io::Error::other(format!(
                "Failed to acquire exclusive lock on {lock_path:?}: {e}"
            )))
        })?;

        let result = (|| -> Result<(), AgentError> {
            let json = serde_json::to_string_pretty(task).map_err(AgentError::Serialization)?;
            std::fs::write(&task_path, json).map_err(AgentError::Io)?;
            Ok(())
        })();

        // Release lock (unlock on drop)
        if let Err(e) = FileExt::unlock(&lock_file) {
            tracing::warn!("Failed to unlock file: {e}");
        }

        result
    }

    /// Load a single task from disk with shared (read) lock.
    pub fn load_task(&self, team_name: &str, task_id: &str) -> Result<TaskFile, AgentError> {
        let tasks_dir = self.tasks_dir(team_name)?;
        let task_path = tasks_dir.join(format!("{task_id}.json"));
        let lock_path = tasks_dir.join(format!("{task_id}.lock"));

        // Acquire shared lock for reading
        let lock_file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(AgentError::Io)?;
        FileExt::lock_shared(&lock_file).map_err(|e| {
            AgentError::Io(std::io::Error::other(format!(
                "Failed to acquire shared lock on {lock_path:?}: {e}"
            )))
        })?;

        let result = std::fs::read_to_string(&task_path)
            .map_err(AgentError::Io)
            .and_then(|json| {
                serde_json::from_str::<TaskFile>(&json).map_err(AgentError::Serialization)
            });

        if let Err(e) = FileExt::unlock(&lock_file) {
            tracing::warn!("Failed to unlock file: {e}");
        }
        result
    }

    /// Load all tasks for a team from disk.
    pub fn load_tasks(&self, team_name: &str) -> Result<Vec<TaskFile>, AgentError> {
        let tasks_dir = self.tasks_dir(team_name)?;
        if !tasks_dir.exists() {
            return Ok(Vec::new());
        }

        let mut tasks = Vec::new();
        for entry in std::fs::read_dir(&tasks_dir).map_err(AgentError::Io)? {
            let entry = entry.map_err(AgentError::Io)?;
            let path = entry.path();

            // Only read .json files, skip .highwatermark, .lock, .tmp files
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                let file_name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                // Skip non-UUID files (like .highwatermark)
                if Uuid::parse_str(file_name).is_err() {
                    continue;
                }

                let json = std::fs::read_to_string(&path).map_err(AgentError::Io)?;
                if let Ok(task) = serde_json::from_str::<TaskFile>(&json) {
                    tasks.push(task);
                }
            }
        }

        // Sort by creation time
        tasks.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        Ok(tasks)
    }

    /// Delete a task file from disk.
    pub fn delete_task(&self, team_name: &str, task_id: &str) -> Result<(), AgentError> {
        let tasks_dir = self.tasks_dir(team_name)?;
        let task_path = tasks_dir.join(format!("{task_id}.json"));
        let lock_path = tasks_dir.join(format!("{task_id}.lock"));

        if task_path.exists() {
            // Acquire exclusive lock before deleting
            let lock_file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(&lock_path)
                .map_err(AgentError::Io)?;
            FileExt::lock_exclusive(&lock_file).map_err(|e| {
                AgentError::Io(std::io::Error::other(format!(
                    "Failed to acquire lock for delete: {e}"
                )))
            })?;

            let result = std::fs::remove_file(&task_path).map_err(AgentError::Io);
            if let Err(e) = FileExt::unlock(&lock_file) {
                tracing::warn!("Failed to unlock file: {e}");
            }
            let _ = std::fs::remove_file(&lock_path);
            result?;
        }
        Ok(())
    }

    // ── High-watermark ────────────────────────────────────────────────

    /// Read the current high-watermark for auto-incrementing task IDs.
    pub fn read_highwatermark(&self, team_name: &str) -> Result<u64, AgentError> {
        let path = self.tasks_dir(team_name)?.join(".highwatermark");
        if !path.exists() {
            return Ok(0);
        }
        let content = std::fs::read_to_string(&path).map_err(AgentError::Io)?;
        content
            .trim()
            .parse::<u64>()
            .map_err(|_| AgentError::Configuration(format!("Invalid highwatermark file: {path:?}")))
    }

    /// Write the next high-watermark value.
    pub fn write_highwatermark(&self, team_name: &str, value: u64) -> Result<(), AgentError> {
        let tasks_dir = self.tasks_dir(team_name)?;
        std::fs::create_dir_all(&tasks_dir).map_err(AgentError::Io)?;

        let path = tasks_dir.join(".highwatermark");

        // Use exclusive lock on the highwatermark file itself
        let lock_path = tasks_dir.join(".highwatermark.lock");
        let lock_file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(AgentError::Io)?;
        FileExt::lock_exclusive(&lock_file).map_err(|e| {
            AgentError::Io(std::io::Error::other(format!(
                "Failed to lock highwatermark: {e}"
            )))
        })?;

        let result = std::fs::write(&path, value.to_string()).map_err(AgentError::Io);
        if let Err(e) = FileExt::unlock(&lock_file) {
            tracing::warn!("Failed to unlock file: {e}");
        }
        result
    }

    /// Atomically increment and return the next task ID.
    ///
    /// Uses file locking to ensure that concurrent agents get unique IDs.
    pub fn next_task_id(&self, team_name: &str) -> Result<u64, AgentError> {
        let tasks_dir = self.tasks_dir(team_name)?;
        std::fs::create_dir_all(&tasks_dir).map_err(AgentError::Io)?;

        let hw_path = tasks_dir.join(".highwatermark");
        let lock_path = tasks_dir.join(".highwatermark.lock");

        // Acquire exclusive lock for the read-modify-write
        let lock_file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(AgentError::Io)?;
        FileExt::lock_exclusive(&lock_file).map_err(|e| {
            AgentError::Io(std::io::Error::other(format!(
                "Failed to lock highwatermark for increment: {e}"
            )))
        })?;

        let current = if hw_path.exists() {
            std::fs::read_to_string(&hw_path)
                .map_err(AgentError::Io)?
                .trim()
                .parse::<u64>()
                .map_err(|_| {
                    AgentError::Configuration(format!("Invalid highwatermark file: {hw_path:?}"))
                })?
        } else {
            0
        };

        let next = current + 1;
        let write_result = std::fs::write(&hw_path, next.to_string()).map_err(AgentError::Io);

        if let Err(e) = FileExt::unlock(&lock_file) {
            tracing::warn!("Failed to unlock file: {e}");
        }
        write_result?;
        Ok(next)
    }

    // ── Inbox operations (JSONL) ──────────────────────────────────────

    /// Append a message to an agent's inbox file (JSONL format).
    ///
    /// Each line is a self-contained JSON object. This allows atomic appends
    /// without reading the full file.
    pub fn deliver_message(
        &self,
        team_name: &str,
        agent_name: &str,
        message: &InboxMessage,
    ) -> Result<(), AgentError> {
        let inbox_dir = self.team_dir(team_name)?.join("inboxes");
        std::fs::create_dir_all(&inbox_dir).map_err(AgentError::Io)?;

        let inbox_path = inbox_dir.join(format!("{agent_name}.jsonl"));
        let lock_path = inbox_dir.join(format!("{agent_name}.jsonl.lock"));

        // Acquire exclusive lock
        let lock_file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(AgentError::Io)?;
        FileExt::lock_exclusive(&lock_file).map_err(|e| {
            AgentError::Io(std::io::Error::other(format!(
                "Failed to lock inbox for {agent_name}: {e}"
            )))
        })?;

        let result = (|| -> Result<(), AgentError> {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&inbox_path)
                .map_err(AgentError::Io)?;

            let line = serde_json::to_string(message).map_err(AgentError::Serialization)?;
            writeln!(file, "{line}").map_err(AgentError::Io)?;
            Ok(())
        })();

        if let Err(e) = FileExt::unlock(&lock_file) {
            tracing::warn!("Failed to unlock file: {e}");
        }
        result
    }

    /// Read all messages from an agent's inbox (JSONL format).
    ///
    /// Returns all messages and marks them as read by clearing the file.
    /// Uses file locking for concurrency safety.
    pub fn read_inbox(
        &self,
        team_name: &str,
        agent_name: &str,
    ) -> Result<Vec<InboxMessage>, AgentError> {
        let inbox_dir = self.team_dir(team_name)?.join("inboxes");
        let inbox_path = inbox_dir.join(format!("{agent_name}.jsonl"));
        let lock_path = inbox_dir.join(format!("{agent_name}.jsonl.lock"));

        if !inbox_path.exists() {
            return Ok(Vec::new());
        }

        // Acquire exclusive lock (read + clear is a write operation)
        let lock_file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(AgentError::Io)?;
        FileExt::lock_exclusive(&lock_file).map_err(|e| {
            AgentError::Io(std::io::Error::other(format!(
                "Failed to lock inbox for read: {e}"
            )))
        })?;

        let result = (|| -> Result<Vec<InboxMessage>, AgentError> {
            let file = std::fs::File::open(&inbox_path).map_err(AgentError::Io)?;
            let reader = std::io::BufReader::new(file);

            let mut messages = Vec::new();
            for line in reader.lines() {
                let line = line.map_err(AgentError::Io)?;
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(msg) = serde_json::from_str::<InboxMessage>(trimmed) {
                    messages.push(msg);
                }
            }

            // Clear the inbox after reading
            std::fs::write(&inbox_path, "").map_err(AgentError::Io)?;

            Ok(messages)
        })();

        if let Err(e) = FileExt::unlock(&lock_file) {
            tracing::warn!("Failed to unlock file: {e}");
        }
        result
    }

    /// Read inbox messages without clearing the file (non-destructive).
    pub fn peek_inbox(
        &self,
        team_name: &str,
        agent_name: &str,
    ) -> Result<Vec<InboxMessage>, AgentError> {
        let inbox_dir = self.team_dir(team_name)?.join("inboxes");
        let inbox_path = inbox_dir.join(format!("{agent_name}.jsonl"));

        if !inbox_path.exists() {
            return Ok(Vec::new());
        }

        let file = std::fs::File::open(&inbox_path).map_err(AgentError::Io)?;
        let _ = FileExt::lock_shared(&file);
        let reader = std::io::BufReader::new(&file);

        let mut messages = Vec::new();
        for line in reader.lines() {
            let line = line.map_err(AgentError::Io)?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(msg) = serde_json::from_str::<InboxMessage>(trimmed) {
                messages.push(msg);
            }
        }
        if let Err(e) = FileExt::unlock(&file) {
            tracing::warn!("Failed to unlock file: {e}");
        }

        Ok(messages)
    }

    // ── Claim conflict resolution via locking ─────────────────────────

    /// Attempt to atomically claim a task by ID.
    ///
    /// Returns `Ok(claimed_task)` if the claim succeeded (task was Pending,
    /// no owner). Returns `Err` if the task was already claimed or doesn't
    /// exist. Uses exclusive file locking to prevent race conditions between
    /// two agents trying to claim the same task simultaneously.
    pub fn claim_task(
        &self,
        team_name: &str,
        task_id: &str,
        agent_name: &str,
    ) -> Result<TaskFile, AgentError> {
        let tasks_dir = self.tasks_dir(team_name)?;
        let task_path = tasks_dir.join(format!("{task_id}.json"));
        let lock_path = tasks_dir.join(format!("{task_id}.lock"));

        if !task_path.exists() {
            return Err(AgentError::Task(crate::error::TaskError::TaskNotFound(
                Uuid::parse_str(task_id).unwrap_or(Uuid::nil()),
            )));
        }

        // Acquire exclusive lock
        let lock_file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(AgentError::Io)?;
        FileExt::lock_exclusive(&lock_file).map_err(|e| {
            AgentError::Io(std::io::Error::other(format!(
                "Failed to acquire claim lock for task {task_id}: {e}"
            )))
        })?;

        let result = (|| -> Result<TaskFile, AgentError> {
            // Read current task state
            let json = std::fs::read_to_string(&task_path).map_err(AgentError::Io)?;
            let mut task: TaskFile =
                serde_json::from_str(&json).map_err(AgentError::Serialization)?;

            // Verify claimable: Pending + no owner
            if task.status != "pending" {
                return Err(AgentError::Task(crate::error::TaskError::InvalidTaskState(
                    Uuid::parse_str(task_id).unwrap_or(Uuid::nil()),
                )));
            }
            if task.owner.is_some() {
                return Err(AgentError::Communication(format!(
                    "Task {} already claimed by {:?}",
                    task_id, task.owner
                )));
            }

            // Update task: set owner and status
            task.owner = Some(agent_name.to_string());
            task.status = "in_progress".to_string();
            task.updated_at = chrono::Utc::now().to_rfc3339();

            // Write back atomically
            let updated_json =
                serde_json::to_string_pretty(&task).map_err(AgentError::Serialization)?;
            let tmp_path = task_path.with_extension("json.tmp");
            std::fs::write(&tmp_path, &updated_json).map_err(AgentError::Io)?;
            std::fs::rename(&tmp_path, &task_path).map_err(AgentError::Io)?;

            Ok(task)
        })();

        if let Err(e) = FileExt::unlock(&lock_file) {
            tracing::warn!("Failed to unlock file: {e}");
        }
        result
    }

    /// Find the next claimable task (lowest-ID, unblocked, unowned).
    ///
    /// Scans task files on disk and returns the first Pending task with no
    /// owner and no unresolved blockers.
    pub fn find_claimable_task(&self, team_name: &str) -> Result<Option<TaskFile>, AgentError> {
        let tasks = self.load_tasks(team_name)?;

        // Filter to Pending + no owner + no blockers, sort by created_at
        let mut claimable: Vec<_> = tasks
            .into_iter()
            .filter(|t| t.status == "pending" && t.owner.is_none() && t.blocked_by.is_empty())
            .collect();
        claimable.sort_by(|a, b| a.created_at.cmp(&b.created_at));

        Ok(claimable.into_iter().next())
    }

    // ── Load on startup ───────────────────────────────────────────────

    /// Load all persisted teams and their members into memory.
    ///
    /// Returns a list of (team_name, TeamConfigFile) pairs ready for
    /// reconstruction into in-memory team structures.
    pub fn load_all_teams(&self) -> Result<Vec<(String, TeamConfigFile)>, AgentError> {
        let team_names = self.list_teams()?;
        let mut result = Vec::new();

        for team_name in &team_names {
            match self.load_team(team_name) {
                Ok(config) => result.push((team_name.clone(), config)),
                Err(e) => {
                    tracing::warn!(
                        team = %team_name,
                        error = %e,
                        "Failed to load persisted team, skipping"
                    );
                }
            }
        }

        Ok(result)
    }

    // ── Path helpers ─────────────────────────────────────────────────

    fn team_dir(&self, team_name: &str) -> Result<PathBuf, AgentError> {
        sanitize_path_component(team_name, "team_name")?;
        Ok(self.base_dir.join("teams").join(team_name))
    }

    fn tasks_dir(&self, team_name: &str) -> Result<PathBuf, AgentError> {
        sanitize_path_component(team_name, "team_name")?;
        Ok(self.base_dir.join("tasks").join(team_name))
    }
}

// ── Conversion helpers ───────────────────────────────────────────────

impl TaskFile {
    /// Convert from an in-memory AgentTask to a persistable TaskFile.
    pub fn from_agent_task(task: &AgentTask) -> Self {
        Self {
            id: task.id.to_string(),
            subject: task.subject.clone(),
            description: task.description.clone(),
            status: match &task.status {
                TaskStatus::Pending => "pending".into(),
                TaskStatus::InProgress => "in_progress".into(),
                TaskStatus::Completed => "completed".into(),
                TaskStatus::Failed(reason) => format!("failed:{reason}"),
                TaskStatus::Blocked => "blocked".into(),
                TaskStatus::Cancelled => "cancelled".into(),
            },
            priority: match task.priority {
                TaskPriority::Low => "low".into(),
                TaskPriority::Medium => "medium".into(),
                TaskPriority::High => "high".into(),
                TaskPriority::Critical => "critical".into(),
            },
            owner: task.owner.clone(),
            blocked_by: task.blocked_by.iter().map(|id| id.to_string()).collect(),
            blocks: task.blocks.iter().map(|id| id.to_string()).collect(),
            active_form: task.active_form.clone(),
            required_capabilities: task.required_capabilities.clone(),
            metadata: task.metadata.clone(),
            created_at: task.created_at.to_rfc3339(),
            updated_at: task.updated_at.to_rfc3339(),
        }
    }

    /// Convert back to an in-memory AgentTask.
    pub fn to_agent_task(&self) -> Result<AgentTask, AgentError> {
        let id = Uuid::parse_str(&self.id)
            .map_err(|_| AgentError::Configuration(format!("Invalid task UUID: {}", self.id)))?;

        let status = if self.status == "pending" {
            TaskStatus::Pending
        } else if self.status == "in_progress" {
            TaskStatus::InProgress
        } else if self.status == "completed" {
            TaskStatus::Completed
        } else if self.status == "blocked" {
            TaskStatus::Blocked
        } else if self.status == "cancelled" {
            TaskStatus::Cancelled
        } else if self.status.starts_with("failed:") {
            TaskStatus::Failed(self.status[7..].to_string())
        } else {
            TaskStatus::Pending
        };

        let priority = match self.priority.as_str() {
            "low" => TaskPriority::Low,
            "medium" => TaskPriority::Medium,
            "high" => TaskPriority::High,
            "critical" => TaskPriority::Critical,
            _ => TaskPriority::Medium,
        };

        let blocked_by: Result<Vec<Uuid>, _> = self
            .blocked_by
            .iter()
            .map(|id| Uuid::parse_str(id))
            .collect();
        let blocked_by = blocked_by
            .map_err(|_| AgentError::Configuration("Invalid UUID in blocked_by".into()))?;

        let blocks: Result<Vec<Uuid>, _> =
            self.blocks.iter().map(|id| Uuid::parse_str(id)).collect();
        let blocks =
            blocks.map_err(|_| AgentError::Configuration("Invalid UUID in blocks".into()))?;

        Ok(AgentTask {
            id,
            subject: self.subject.clone(),
            description: self.description.clone(),
            status,
            priority,
            owner: self.owner.clone(),
            blocked_by,
            blocks,
            active_form: self.active_form.clone(),
            required_capabilities: self.required_capabilities.clone(),
            metadata: self.metadata.clone(),
            created_at: self.created_at.parse().unwrap_or(chrono::Utc::now()),
            updated_at: self.updated_at.parse().unwrap_or(chrono::Utc::now()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir() -> PathBuf {
        tempfile::tempdir().unwrap().path().to_path_buf()
    }

    #[test]
    fn save_and_load_team() {
        let dir = tmp_dir();
        let persist = FilePersistence::with_base_dir(dir.clone());

        let mut members = HashMap::new();
        members.insert("agent-1".into(), TeammateConfig::default());

        let team = TeamConfigFile {
            name: "test-team".into(),
            description: "A test team".into(),
            members,
            created_at: "2026-01-01T00:00:00Z".into(),
            assignment_index: 0,
        };

        persist.save_team(&team).unwrap();
        let loaded = persist.load_team("test-team").unwrap();

        assert_eq!(loaded.name, "test-team");
        assert_eq!(loaded.description, "A test team");
        assert_eq!(loaded.members.len(), 1);
    }

    #[test]
    fn list_teams() {
        let dir = tmp_dir();
        let persist = FilePersistence::with_base_dir(dir.clone());

        // Save two teams
        for name in &["team-a", "team-b"] {
            let team = TeamConfigFile {
                name: name.to_string(),
                description: format!("Team {name}"),
                members: HashMap::new(),
                created_at: "2026-01-01T00:00:00Z".into(),
                assignment_index: 0,
            };
            persist.save_team(&team).unwrap();
        }

        let mut teams = persist.list_teams().unwrap();
        teams.sort();
        assert_eq!(teams, vec!["team-a", "team-b"]);
    }

    #[test]
    fn delete_team() {
        let dir = tmp_dir();
        let persist = FilePersistence::with_base_dir(dir.clone());

        let team = TeamConfigFile {
            name: "doomed".into(),
            description: "Will be deleted".into(),
            members: HashMap::new(),
            created_at: "2026-01-01T00:00:00Z".into(),
            assignment_index: 0,
        };
        persist.save_team(&team).unwrap();
        assert!(persist.team_dir("doomed").unwrap().exists());

        persist.delete_team("doomed").unwrap();
        assert!(!persist.team_dir("doomed").unwrap().exists());
    }

    #[test]
    fn save_and_load_tasks() {
        let dir = tmp_dir();
        let persist = FilePersistence::with_base_dir(dir.clone());

        let task = TaskFile {
            id: Uuid::new_v4().to_string(),
            subject: "Implement auth".into(),
            description: "Add JWT auth middleware".into(),
            status: "pending".into(),
            priority: "high".into(),
            owner: None,
            blocked_by: Vec::new(),
            blocks: Vec::new(),
            active_form: Some("Implementing auth".into()),
            required_capabilities: vec!["rust".into()],
            metadata: serde_json::json!({"sprint": 3}),
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        };

        persist.save_task("my-team", &task).unwrap();
        let tasks = persist.load_tasks("my-team").unwrap();

        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].subject, "Implement auth");
        assert_eq!(tasks[0].priority, "high");
    }

    #[test]
    fn highwatermark_increment() {
        let dir = tmp_dir();
        let persist = FilePersistence::with_base_dir(dir.clone());

        assert_eq!(persist.read_highwatermark("team").unwrap(), 0);

        let id1 = persist.next_task_id("team").unwrap();
        let id2 = persist.next_task_id("team").unwrap();
        let id3 = persist.next_task_id("team").unwrap();

        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(id3, 3);
        assert_eq!(persist.read_highwatermark("team").unwrap(), 3);
    }

    #[test]
    fn inbox_deliver_and_read_jsonl() {
        let dir = tmp_dir();
        let persist = FilePersistence::with_base_dir(dir.clone());

        let msg1 = InboxMessage {
            id: "msg-1".into(),
            from: "lead".into(),
            content: "Start task 1".into(),
            timestamp: "2026-01-01T00:00:00Z".into(),
            read: false,
        };
        let msg2 = InboxMessage {
            id: "msg-2".into(),
            from: "lead".into(),
            content: "Start task 2".into(),
            timestamp: "2026-01-01T00:00:01Z".into(),
            read: false,
        };

        persist.deliver_message("team", "agent-1", &msg1).unwrap();
        persist.deliver_message("team", "agent-1", &msg2).unwrap();

        // Verify the file is JSONL (one JSON object per line)
        let inbox_path = persist
            .team_dir("team")
            .unwrap()
            .join("inboxes")
            .join("agent-1.jsonl");
        let raw_content = std::fs::read_to_string(&inbox_path).unwrap();
        let lines: Vec<&str> = raw_content.trim().lines().collect();
        assert_eq!(lines.len(), 2);

        let messages = persist.read_inbox("team", "agent-1").unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, "Start task 1");
        assert_eq!(messages[1].content, "Start task 2");

        // Inbox should be cleared after reading
        let empty = persist.read_inbox("team", "agent-1").unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn task_file_roundtrip() {
        let original = AgentTask::new(
            "Fix bug".into(),
            "Fix the auth bug".into(),
            TaskPriority::High,
        );

        let file = TaskFile::from_agent_task(&original);
        let restored = file.to_agent_task().unwrap();

        assert_eq!(restored.id, original.id);
        assert_eq!(restored.subject, "Fix bug");
        assert_eq!(restored.priority, TaskPriority::High);
        assert_eq!(restored.status, TaskStatus::Pending);
    }

    #[test]
    fn task_file_failed_status_roundtrip() {
        let mut original = AgentTask::new(
            "Deploy".into(),
            "Deploy to prod".into(),
            TaskPriority::Critical,
        );
        original.mark_failed("OOM killed".into());

        let file = TaskFile::from_agent_task(&original);
        assert_eq!(file.status, "failed:OOM killed");

        let restored = file.to_agent_task().unwrap();
        assert!(matches!(restored.status, TaskStatus::Failed(ref r) if r == "OOM killed"));
    }

    #[test]
    fn claim_task_success() {
        let dir = tmp_dir();
        let persist = FilePersistence::with_base_dir(dir.clone());

        let task_id = Uuid::new_v4().to_string();
        let task = TaskFile {
            id: task_id.clone(),
            subject: "Do thing".into(),
            description: "Do the thing".into(),
            status: "pending".into(),
            priority: "high".into(),
            owner: None,
            blocked_by: Vec::new(),
            blocks: Vec::new(),
            active_form: None,
            required_capabilities: Vec::new(),
            metadata: serde_json::Value::Null,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        };

        persist.save_task("team", &task).unwrap();
        let claimed = persist.claim_task("team", &task_id, "agent-1").unwrap();

        assert_eq!(claimed.owner.as_deref(), Some("agent-1"));
        assert_eq!(claimed.status, "in_progress");

        // Verify persisted state
        let loaded = persist.load_task("team", &task_id).unwrap();
        assert_eq!(loaded.owner.as_deref(), Some("agent-1"));
        assert_eq!(loaded.status, "in_progress");
    }

    #[test]
    fn claim_task_conflict_resolution() {
        let dir = tmp_dir();
        let persist = FilePersistence::with_base_dir(dir.clone());

        let task_id = Uuid::new_v4().to_string();
        let task = TaskFile {
            id: task_id.clone(),
            subject: "Race condition".into(),
            description: "Two agents try to claim".into(),
            status: "pending".into(),
            priority: "high".into(),
            owner: None,
            blocked_by: Vec::new(),
            blocks: Vec::new(),
            active_form: None,
            required_capabilities: Vec::new(),
            metadata: serde_json::Value::Null,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        };

        persist.save_task("team", &task).unwrap();

        // First agent claims successfully
        let claimed = persist.claim_task("team", &task_id, "agent-1").unwrap();
        assert_eq!(claimed.owner.as_deref(), Some("agent-1"));

        // Second agent's claim fails because task is now in_progress
        let result = persist.claim_task("team", &task_id, "agent-2");
        assert!(result.is_err());
    }

    #[test]
    fn find_claimable_task_returns_earliest() {
        let dir = tmp_dir();
        let persist = FilePersistence::with_base_dir(dir.clone());

        // Create tasks in chronological order
        let t1 = TaskFile {
            id: Uuid::new_v4().to_string(),
            subject: "First task".into(),
            description: "Created first".into(),
            status: "pending".into(),
            priority: "medium".into(),
            owner: None,
            blocked_by: Vec::new(),
            blocks: Vec::new(),
            active_form: None,
            required_capabilities: Vec::new(),
            metadata: serde_json::Value::Null,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        };
        let t2 = TaskFile {
            id: Uuid::new_v4().to_string(),
            subject: "Second task".into(),
            description: "Created second".into(),
            status: "pending".into(),
            priority: "high".into(),
            owner: None,
            blocked_by: Vec::new(),
            blocks: Vec::new(),
            active_form: None,
            required_capabilities: Vec::new(),
            metadata: serde_json::Value::Null,
            created_at: "2026-01-02T00:00:00Z".into(),
            updated_at: "2026-01-02T00:00:00Z".into(),
        };

        persist.save_task("team", &t2).unwrap();
        persist.save_task("team", &t1).unwrap();

        // Should return earliest created task
        let claimable = persist.find_claimable_task("team").unwrap().unwrap();
        assert_eq!(claimable.subject, "First task");
    }

    #[test]
    fn find_claimable_task_skips_blocked_and_owned() {
        let dir = tmp_dir();
        let persist = FilePersistence::with_base_dir(dir.clone());

        let blocker = Uuid::new_v4().to_string();
        let blocked = TaskFile {
            id: Uuid::new_v4().to_string(),
            subject: "Blocked task".into(),
            description: "Has a blocker".into(),
            status: "pending".into(),
            priority: "high".into(),
            owner: None,
            blocked_by: vec![blocker],
            blocks: Vec::new(),
            active_form: None,
            required_capabilities: Vec::new(),
            metadata: serde_json::Value::Null,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        };
        let owned = TaskFile {
            id: Uuid::new_v4().to_string(),
            subject: "Owned task".into(),
            description: "Already claimed".into(),
            status: "pending".into(),
            priority: "high".into(),
            owner: Some("agent-0".into()),
            blocked_by: Vec::new(),
            blocks: Vec::new(),
            active_form: None,
            required_capabilities: Vec::new(),
            metadata: serde_json::Value::Null,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        };

        persist.save_task("team", &blocked).unwrap();
        persist.save_task("team", &owned).unwrap();

        // No claimable tasks
        let result = persist.find_claimable_task("team").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn load_all_teams() {
        let dir = tmp_dir();
        let persist = FilePersistence::with_base_dir(dir.clone());

        for name in &["alpha", "beta"] {
            let team = TeamConfigFile {
                name: name.to_string(),
                description: format!("Team {name}"),
                members: HashMap::new(),
                created_at: "2026-01-01T00:00:00Z".into(),
                assignment_index: 0,
            };
            persist.save_team(&team).unwrap();
        }

        let mut all = persist.load_all_teams().unwrap();
        all.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].0, "alpha");
        assert_eq!(all[1].0, "beta");
    }

    #[test]
    fn delete_task_with_locking() {
        let dir = tmp_dir();
        let persist = FilePersistence::with_base_dir(dir.clone());

        let task_id = Uuid::new_v4().to_string();
        let task = TaskFile {
            id: task_id.clone(),
            subject: "To delete".into(),
            description: "Will be removed".into(),
            status: "pending".into(),
            priority: "low".into(),
            owner: None,
            blocked_by: Vec::new(),
            blocks: Vec::new(),
            active_form: None,
            required_capabilities: Vec::new(),
            metadata: serde_json::Value::Null,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        };

        persist.save_task("team", &task).unwrap();
        assert!(persist.load_task("team", &task_id).is_ok());

        persist.delete_task("team", &task_id).unwrap();
        assert!(persist.load_task("team", &task_id).is_err());
    }

    #[test]
    fn load_single_task_with_lock() {
        let dir = tmp_dir();
        let persist = FilePersistence::with_base_dir(dir.clone());

        let task_id = Uuid::new_v4().to_string();
        let task = TaskFile {
            id: task_id.clone(),
            subject: "Single task".into(),
            description: "Load this one".into(),
            status: "pending".into(),
            priority: "medium".into(),
            owner: Some("agent-x".into()),
            blocked_by: Vec::new(),
            blocks: Vec::new(),
            active_form: None,
            required_capabilities: Vec::new(),
            metadata: serde_json::Value::Null,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        };

        persist.save_task("team", &task).unwrap();
        let loaded = persist.load_task("team", &task_id).unwrap();
        assert_eq!(loaded.subject, "Single task");
        assert_eq!(loaded.owner.as_deref(), Some("agent-x"));
    }
}
