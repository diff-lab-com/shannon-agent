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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_new_defaults() {
        let task = AgentTask::new("Fix bug".to_string(), "Fix the auth bug".to_string(), TaskPriority::High);
        assert_eq!(task.subject, "Fix bug");
        assert_eq!(task.description, "Fix the auth bug");
        assert_eq!(task.status, TaskStatus::Pending);
        assert_eq!(task.priority, TaskPriority::High);
        assert!(task.owner.is_none());
        assert!(task.blocked_by.is_empty());
        assert!(task.blocks.is_empty());
        assert!(task.active_form.is_none());
        assert!(task.required_capabilities.is_empty());
        assert!(task.metadata.is_null());
    }

    #[test]
    fn task_is_ready_when_pending_no_blockers() {
        let task = AgentTask::new("x".into(), "x".into(), TaskPriority::Medium);
        assert!(task.is_ready());
    }

    #[test]
    fn task_not_ready_when_blocked() {
        let mut task = AgentTask::new("x".into(), "x".into(), TaskPriority::Medium);
        task.blocked_by.push(Uuid::new_v4());
        assert!(!task.is_ready());
    }

    #[test]
    fn task_not_ready_when_in_progress() {
        let mut task = AgentTask::new("x".into(), "x".into(), TaskPriority::Medium);
        task.status = TaskStatus::InProgress;
        assert!(!task.is_ready());
    }

    #[test]
    fn task_is_blocking() {
        let mut task = AgentTask::new("x".into(), "x".into(), TaskPriority::Medium);
        assert!(!task.is_blocking());
        task.blocks.push(Uuid::new_v4());
        assert!(task.is_blocking());
    }

    #[test]
    fn task_add_dependency_no_duplicates() {
        let mut task = AgentTask::new("x".into(), "x".into(), TaskPriority::Medium);
        let dep_id = Uuid::new_v4();
        task.add_dependency(dep_id);
        task.add_dependency(dep_id);
        assert_eq!(task.blocked_by.len(), 1);
    }

    #[test]
    fn task_mark_completed() {
        let mut task = AgentTask::new("x".into(), "x".into(), TaskPriority::Medium);
        task.mark_completed();
        assert_eq!(task.status, TaskStatus::Completed);
    }

    #[test]
    fn task_mark_failed() {
        let mut task = AgentTask::new("x".into(), "x".into(), TaskPriority::Medium);
        task.mark_failed("timeout".to_string());
        match &task.status {
            TaskStatus::Failed(reason) => assert_eq!(reason, "timeout"),
            other => panic!("Expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn task_assign_to() {
        let mut task = AgentTask::new("x".into(), "x".into(), TaskPriority::Medium);
        task.assign_to("worker-1".to_string());
        assert_eq!(task.owner, Some("worker-1".to_string()));
        assert_eq!(task.status, TaskStatus::InProgress);
    }

    #[test]
    fn task_priority_ordering() {
        assert!(TaskPriority::Critical > TaskPriority::High);
        assert!(TaskPriority::High > TaskPriority::Medium);
        assert!(TaskPriority::Medium > TaskPriority::Low);
    }

    #[test]
    fn task_status_serde() {
        let statuses = vec![
            TaskStatus::Pending,
            TaskStatus::InProgress,
            TaskStatus::Completed,
            TaskStatus::Failed("err".to_string()),
            TaskStatus::Blocked,
            TaskStatus::Cancelled,
        ];
        let json = serde_json::to_string(&statuses).unwrap();
        let de: Vec<TaskStatus> = serde_json::from_str(&json).unwrap();
        assert_eq!(de, statuses);
    }

    #[test]
    fn task_status_inprogress_serializes_pascal() {
        let json = serde_json::to_string(&TaskStatus::InProgress).unwrap();
        assert!(json.contains("InProgress"), "Expected 'InProgress', got: {json}");
    }

    #[test]
    fn task_priority_serde() {
        let json = serde_json::to_string(&TaskPriority::Critical).unwrap();
        assert!(json.contains("Critical"));
        let de: TaskPriority = serde_json::from_str("\"Low\"").unwrap();
        assert_eq!(de, TaskPriority::Low);
    }

    #[test]
    fn dependency_type_serde() {
        let deps = vec![DependencyType::MustComplete, DependencyType::MustStart, DependencyType::ShouldComplete];
        let json = serde_json::to_string(&deps).unwrap();
        let de: Vec<DependencyType> = serde_json::from_str(&json).unwrap();
        assert_eq!(de, deps);
    }

    #[test]
    fn agent_task_roundtrip() {
        let task = AgentTask::new("Test".into(), "Test task".into(), TaskPriority::High);
        let json = serde_json::to_string(&task).unwrap();
        let de: AgentTask = serde_json::from_str(&json).unwrap();
        assert_eq!(de.subject, "Test");
        assert_eq!(de.priority, TaskPriority::High);
    }

    #[test]
    fn task_dependency_roundtrip() {
        let dep = TaskDependency {
            task_id: Uuid::new_v4(),
            depends_on: Uuid::new_v4(),
            dependency_type: DependencyType::MustComplete,
        };
        let json = serde_json::to_string(&dep).unwrap();
        let de: TaskDependency = serde_json::from_str(&json).unwrap();
        assert_eq!(de.dependency_type, DependencyType::MustComplete);
    }

    #[test]
    fn send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<AgentTask>();
        assert_send_sync::<TaskStatus>();
        assert_send_sync::<TaskPriority>();
        assert_send_sync::<DependencyType>();
        assert_send_sync::<TaskDependency>();
    }
}
