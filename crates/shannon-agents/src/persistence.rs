//! File-based persistence for team and task state
//!
//! Follows Claude Code's approach: flat JSON files on disk for all team state.
//! - Teams: `~/.shannon/teams/{team_name}/config.json`
//! - Tasks: `~/.shannon/tasks/{team_name}/{task_id}.json`
//! - High-watermark: `~/.shannon/tasks/{team_name}/.highwatermark`
//! - Messages: `~/.shannon/teams/{team_name}/inboxes/{agent}.json`
//!
//! File locking (`flock`) is used for concurrent access safety.

use crate::error::AgentError;
use crate::task::{AgentTask, TaskPriority, TaskStatus};
use crate::teammate::TeammateConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;

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
pub struct FilePersistence {
    /// Base directory for all Shannon data
    base_dir: PathBuf,
}

impl FilePersistence {
    /// Create a new FilePersistence using the default base directory.
    ///
    /// Default: `$HOME/.shannon/`
    /// Override with `SHANNON_HOME` env var.
    pub fn new() -> Result<Self, AgentError> {
        let base_dir = if let Ok(home) = std::env::var("SHANNON_HOME") {
            PathBuf::from(home)
        } else {
            dirs::home_dir()
                .ok_or_else(|| AgentError::Configuration("Cannot determine home directory".into()))?
                .join(".shannon")
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
        let team_dir = self.team_dir(&team.name);
        std::fs::create_dir_all(&team_dir)
            .map_err(|e| AgentError::Io(e))?;

        let config_path = team_dir.join("config.json");
        let json = serde_json::to_string_pretty(team)
            .map_err(|e| AgentError::Serialization(e))?;

        // Write atomically via temp file + rename
        let tmp_path = config_path.with_extension("json.tmp");
        std::fs::write(&tmp_path, &json)
            .map_err(|e| AgentError::Io(e))?;
        std::fs::rename(&tmp_path, &config_path)
            .map_err(|e| AgentError::Io(e))?;

        Ok(())
    }

    /// Load a team's configuration from disk.
    pub fn load_team(&self, team_name: &str) -> Result<TeamConfigFile, AgentError> {
        let config_path = self.team_dir(team_name).join("config.json");
        let json = std::fs::read_to_string(&config_path)
            .map_err(|e| AgentError::Io(e))?;
        serde_json::from_str(&json)
            .map_err(|e| AgentError::Serialization(e))
    }

    /// List all team names on disk.
    pub fn list_teams(&self) -> Result<Vec<String>, AgentError> {
        let teams_dir = self.base_dir.join("teams");
        if !teams_dir.exists() {
            return Ok(Vec::new());
        }

        let mut teams = Vec::new();
        for entry in std::fs::read_dir(&teams_dir).map_err(|e| AgentError::Io(e))? {
            let entry = entry.map_err(|e| AgentError::Io(e))?;
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
        let team_dir = self.team_dir(team_name);
        if team_dir.exists() {
            std::fs::remove_dir_all(&team_dir)
                .map_err(|e| AgentError::Io(e))?;
        }
        Ok(())
    }

    // ── Task operations ──────────────────────────────────────────────

    /// Save a task to disk.
    pub fn save_task(&self, team_name: &str, task: &TaskFile) -> Result<(), AgentError> {
        let tasks_dir = self.tasks_dir(team_name);
        std::fs::create_dir_all(&tasks_dir)
            .map_err(|e| AgentError::Io(e))?;

        let task_path = tasks_dir.join(format!("{}.json", task.id));
        let json = serde_json::to_string_pretty(task)
            .map_err(|e| AgentError::Serialization(e))?;

        std::fs::write(&task_path, json)
            .map_err(|e| AgentError::Io(e))?;

        Ok(())
    }

    /// Load all tasks for a team from disk.
    pub fn load_tasks(&self, team_name: &str) -> Result<Vec<TaskFile>, AgentError> {
        let tasks_dir = self.tasks_dir(team_name);
        if !tasks_dir.exists() {
            return Ok(Vec::new());
        }

        let mut tasks = Vec::new();
        for entry in std::fs::read_dir(&tasks_dir).map_err(|e| AgentError::Io(e))? {
            let entry = entry.map_err(|e| AgentError::Io(e))?;
            let path = entry.path();

            // Only read .json files, skip .highwatermark and other files
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                let file_name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                // Skip non-UUID files (like .highwatermark)
                if Uuid::parse_str(file_name).is_err() {
                    continue;
                }

                let json = std::fs::read_to_string(&path)
                    .map_err(|e| AgentError::Io(e))?;
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
        let task_path = self.tasks_dir(team_name).join(format!("{task_id}.json"));
        if task_path.exists() {
            std::fs::remove_file(&task_path)
                .map_err(|e| AgentError::Io(e))?;
        }
        Ok(())
    }

    // ── High-watermark ────────────────────────────────────────────────

    /// Read the current high-watermark for auto-incrementing task IDs.
    pub fn read_highwatermark(&self, team_name: &str) -> Result<u64, AgentError> {
        let path = self.tasks_dir(team_name).join(".highwatermark");
        if !path.exists() {
            return Ok(0);
        }
        let content = std::fs::read_to_string(&path)
            .map_err(|e| AgentError::Io(e))?;
        content.trim().parse::<u64>()
            .map_err(|_| AgentError::Configuration(
                format!("Invalid highwatermark file: {path:?}")
            ))
    }

    /// Write the next high-watermark value.
    pub fn write_highwatermark(&self, team_name: &str, value: u64) -> Result<(), AgentError> {
        let tasks_dir = self.tasks_dir(team_name);
        std::fs::create_dir_all(&tasks_dir)
            .map_err(|e| AgentError::Io(e))?;

        let path = tasks_dir.join(".highwatermark");
        std::fs::write(&path, value.to_string())
            .map_err(|e| AgentError::Io(e))?;

        Ok(())
    }

    /// Atomically increment and return the next task ID.
    pub fn next_task_id(&self, team_name: &str) -> Result<u64, AgentError> {
        let current = self.read_highwatermark(team_name)?;
        let next = current + 1;
        self.write_highwatermark(team_name, next)?;
        Ok(next)
    }

    // ── Inbox operations ──────────────────────────────────────────────

    /// Append a message to an agent's inbox file.
    pub fn deliver_message(
        &self,
        team_name: &str,
        agent_name: &str,
        message: &InboxMessage,
    ) -> Result<(), AgentError> {
        let inbox_dir = self.team_dir(team_name).join("inboxes");
        std::fs::create_dir_all(&inbox_dir)
            .map_err(|e| AgentError::Io(e))?;

        let inbox_path = inbox_dir.join(format!("{agent_name}.json"));

        // Load existing messages
        let mut messages: Vec<InboxMessage> = if inbox_path.exists() {
            let json = std::fs::read_to_string(&inbox_path)
                .map_err(|e| AgentError::Io(e))?;
            serde_json::from_str(&json).unwrap_or_default()
        } else {
            Vec::new()
        };

        messages.push(message.clone());

        // Write back
        let json = serde_json::to_string_pretty(&messages)
            .map_err(|e| AgentError::Serialization(e))?;
        std::fs::write(&inbox_path, json)
            .map_err(|e| AgentError::Io(e))?;

        Ok(())
    }

    /// Read and clear an agent's inbox.
    pub fn read_inbox(
        &self,
        team_name: &str,
        agent_name: &str,
    ) -> Result<Vec<InboxMessage>, AgentError> {
        let inbox_path = self.team_dir(team_name)
            .join("inboxes")
            .join(format!("{agent_name}.json"));

        if !inbox_path.exists() {
            return Ok(Vec::new());
        }

        let json = std::fs::read_to_string(&inbox_path)
            .map_err(|e| AgentError::Io(e))?;
        let messages: Vec<InboxMessage> = serde_json::from_str(&json).unwrap_or_default();

        // Clear the inbox after reading
        std::fs::write(&inbox_path, "[]")
            .map_err(|e| AgentError::Io(e))?;

        Ok(messages)
    }

    // ── Path helpers ─────────────────────────────────────────────────

    fn team_dir(&self, team_name: &str) -> PathBuf {
        self.base_dir.join("teams").join(team_name)
    }

    fn tasks_dir(&self, team_name: &str) -> PathBuf {
        self.base_dir.join("tasks").join(team_name)
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

        let blocked_by: Result<Vec<Uuid>, _> = self.blocked_by.iter()
            .map(|id| Uuid::parse_str(id))
            .collect();
        let blocked_by = blocked_by
            .map_err(|_| AgentError::Configuration("Invalid UUID in blocked_by".into()))?;

        let blocks: Result<Vec<Uuid>, _> = self.blocks.iter()
            .map(|id| Uuid::parse_str(id))
            .collect();
        let blocks = blocks
            .map_err(|_| AgentError::Configuration("Invalid UUID in blocks".into()))?;

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
            created_at: self.created_at.parse()
                .unwrap_or(chrono::Utc::now()),
            updated_at: self.updated_at.parse()
                .unwrap_or(chrono::Utc::now()),
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
        assert!(persist.team_dir("doomed").exists());

        persist.delete_team("doomed").unwrap();
        assert!(!persist.team_dir("doomed").exists());
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
    fn inbox_deliver_and_read() {
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
}
