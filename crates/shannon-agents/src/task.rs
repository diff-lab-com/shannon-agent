//! Task management for multi-agent coordination

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Priority levels for agent tasks
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TaskPriority {
    Low = 0,
    Medium = 1,
    High = 2,
    Critical = 3,
}

/// Status of a task in the agent system
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    /// Task is pending assignment
    Pending,
    /// Task is currently being worked on
    InProgress,
    /// Task has been completed
    Completed,
    /// Task failed and needs attention
    Failed(String),
    /// Task is blocked by dependencies
    Blocked,
    /// Task was cancelled
    Cancelled,
}

/// A dependency relationship between tasks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDependency {
    /// The task ID that depends on another
    pub task_id: Uuid,
    /// The task ID that this task depends on
    pub depends_on: Uuid,
    /// Type of dependency relationship
    pub dependency_type: DependencyType,
}

/// Types of task dependencies
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DependencyType {
    /// Must complete before this task can start
    MustComplete,
    /// Must start before this task can start
    MustStart,
    /// Should complete (soft dependency)
    ShouldComplete,
}

/// A task that can be assigned to an agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTask {
    /// Unique task identifier
    pub id: Uuid,
    /// Brief action title in imperative form
    pub subject: String,
    /// Detailed description of what needs to be done
    pub description: String,
    /// Current status
    pub status: TaskStatus,
    /// Task priority
    pub priority: TaskPriority,
    /// Agent currently assigned to this task (if any)
    pub owner: Option<String>,
    /// Tasks that must complete before this one can start
    pub blocked_by: Vec<Uuid>,
    /// Tasks that are waiting on this one to complete
    pub blocks: Vec<Uuid>,
    /// Present continuous form shown in progress spinner
    pub active_form: Option<String>,
    /// Capabilities required to perform this task
    pub required_capabilities: Vec<String>,
    /// Additional metadata
    pub metadata: serde_json::Value,
    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Last update timestamp
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl AgentTask {
    /// Create a new task
    pub fn new(subject: String, description: String, priority: TaskPriority) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: Uuid::new_v4(),
            subject,
            description,
            status: TaskStatus::Pending,
            priority,
            owner: None,
            blocked_by: Vec::new(),
            blocks: Vec::new(),
            active_form: None,
            required_capabilities: Vec::new(),
            metadata: serde_json::Value::Null,
            created_at: now,
            updated_at: now,
        }
    }

    /// Check if this task is ready to be started (no pending blockers)
    pub fn is_ready(&self) -> bool {
        self.blocked_by.is_empty() && self.status == TaskStatus::Pending
    }

    /// Check if this task is blocking any other tasks
    pub fn is_blocking(&self) -> bool {
        !self.blocks.is_empty()
    }

    /// Add a dependency on another task
    pub fn add_dependency(&mut self, depends_on: Uuid) {
        if !self.blocked_by.contains(&depends_on) {
            self.blocked_by.push(depends_on);
        }
    }

    /// Mark task as completed
    pub fn mark_completed(&mut self) {
        self.status = TaskStatus::Completed;
        self.updated_at = chrono::Utc::now();
    }

    /// Mark task as failed with reason
    pub fn mark_failed(&mut self, reason: String) {
        self.status = TaskStatus::Failed(reason);
        self.updated_at = chrono::Utc::now();
    }

    /// Assign task to an agent
    pub fn assign_to(&mut self, agent_name: String) {
        self.owner = Some(agent_name);
        self.status = TaskStatus::InProgress;
        self.updated_at = chrono::Utc::now();
    }
}
