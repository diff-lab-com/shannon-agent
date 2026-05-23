//! Task board for managing agent task assignments

use crate::error::{AgentError, TaskError};
use crate::task::{AgentTask, TaskPriority, TaskStatus};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use uuid::Uuid;

/// Assignment of a task to an agent
#[derive(Debug, Clone)]
pub struct TaskAssignment {
    /// Task being assigned
    pub task: AgentTask,
    /// Agent assigned to the task
    pub agent: String,
    /// Assignment timestamp
    pub assigned_at: chrono::DateTime<chrono::Utc>,
}

/// Events emitted by the task board
#[derive(Debug, Clone)]
pub enum TaskBoardEvent {
    /// Task added to the board
    TaskAdded { task_id: Uuid },
    /// Task assigned to an agent
    TaskAssigned { task_id: Uuid, agent: String },
    /// Task status changed
    TaskStatusChanged { task_id: Uuid, status: TaskStatus },
    /// Task completed
    TaskCompleted { task_id: Uuid, agent: String },
    /// Task failed
    TaskFailed { task_id: Uuid, reason: String },
    /// Task removed
    TaskRemoved { task_id: Uuid },
    /// Dependency added
    DependencyAdded { task_id: Uuid, depends_on: Uuid },
}

/// Summary of task board state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskBoardSummary {
    /// Total number of tasks
    pub total_tasks: usize,
    /// Number of pending tasks
    pub pending_tasks: usize,
    /// Number of in-progress tasks
    pub in_progress_tasks: usize,
    /// Number of completed tasks
    pub completed_tasks: usize,
    /// Number of failed tasks
    pub failed_tasks: usize,
    /// Number of blocked tasks
    pub blocked_tasks: usize,
    /// Tasks by priority
    pub by_priority: HashMap<String, usize>,
    /// Tasks by agent
    pub by_agent: HashMap<String, usize>,
}

/// Task board for coordinating agent tasks
pub struct TaskBoard {
    /// All tasks indexed by ID
    tasks: Arc<RwLock<HashMap<Uuid, AgentTask>>>,
    /// Task assignments indexed by task ID
    assignments: Arc<RwLock<HashMap<Uuid, TaskAssignment>>>,
    /// Dependency graph
    dependencies: Arc<RwLock<HashMap<Uuid, HashSet<Uuid>>>>,
    /// Reverse dependency graph (which tasks depend on this)
    reverse_dependencies: Arc<RwLock<HashMap<Uuid, HashSet<Uuid>>>>,
    /// Event broadcaster
    event_sender: broadcast::Sender<TaskBoardEvent>,
}

impl TaskBoard {
    /// Create a new task board
    pub fn new() -> Self {
        let (event_sender, _) = broadcast::channel(100);

        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            assignments: Arc::new(RwLock::new(HashMap::new())),
            dependencies: Arc::new(RwLock::new(HashMap::new())),
            reverse_dependencies: Arc::new(RwLock::new(HashMap::new())),
            event_sender,
        }
    }

    /// Subscribe to task board events
    pub fn subscribe_events(&self) -> broadcast::Receiver<TaskBoardEvent> {
        self.event_sender.subscribe()
    }

    /// Add a new task to the board
    pub async fn add_task(&self, task: AgentTask) -> Result<(), AgentError> {
        let task_id = task.id;
        let subject = task.subject.clone();

        let mut tasks = self.tasks.write().await;

        if tasks.contains_key(&task.id) {
            return Err(AgentError::Task(TaskError::TaskNotFound(task.id)));
        }

        tasks.insert(task.id, task);

        if let Err(e) = self
            .event_sender
            .send(TaskBoardEvent::TaskAdded { task_id })
        {
            tracing::debug!("Failed to send TaskAdded event: {e}");
        }

        tracing::debug!(task_id = %task_id, subject = %subject, "Task added to board");

        Ok(())
    }

    /// Get a task by ID
    pub async fn get_task(&self, task_id: Uuid) -> Result<AgentTask, AgentError> {
        let tasks = self.tasks.read().await;

        tasks
            .get(&task_id)
            .cloned()
            .ok_or(AgentError::Task(TaskError::TaskNotFound(task_id)))
    }

    /// List all pending tasks that are ready to be started
    pub async fn list_ready_tasks(&self) -> Vec<AgentTask> {
        let tasks = self.tasks.read().await;

        tasks
            .values()
            .filter(|task| task.is_ready())
            .cloned()
            .collect()
    }

    /// List tasks by status
    pub async fn list_tasks_by_status(&self, status: TaskStatus) -> Vec<AgentTask> {
        let tasks = self.tasks.read().await;

        tasks
            .values()
            .filter(|task| task.status == status)
            .cloned()
            .collect()
    }

    /// List tasks by priority
    pub async fn list_tasks_by_priority(&self, priority: TaskPriority) -> Vec<AgentTask> {
        let tasks = self.tasks.read().await;

        let mut result: Vec<_> = tasks
            .values()
            .filter(|task| task.priority == priority)
            .cloned()
            .collect();

        // Sort by creation time
        result.sort_by(|a, b| a.created_at.cmp(&b.created_at));

        result
    }

    /// List all tasks
    pub async fn list_all_tasks(&self) -> Vec<AgentTask> {
        let tasks = self.tasks.read().await;
        tasks.values().cloned().collect()
    }

    /// Get board summary
    pub async fn summary(&self) -> TaskBoardSummary {
        let tasks = self.tasks.read().await;
        let assignments = self.assignments.read().await;

        let mut summary = TaskBoardSummary {
            total_tasks: tasks.len(),
            pending_tasks: 0,
            in_progress_tasks: 0,
            completed_tasks: 0,
            failed_tasks: 0,
            blocked_tasks: 0,
            by_priority: HashMap::new(),
            by_agent: HashMap::new(),
        };

        for task in tasks.values() {
            match task.status {
                TaskStatus::Pending => summary.pending_tasks += 1,
                TaskStatus::InProgress => summary.in_progress_tasks += 1,
                TaskStatus::Completed => summary.completed_tasks += 1,
                TaskStatus::Failed(_) => summary.failed_tasks += 1,
                TaskStatus::Blocked => summary.blocked_tasks += 1,
                TaskStatus::Cancelled => {}
            }

            let priority_key = format!("{:?}", task.priority);
            *summary.by_priority.entry(priority_key).or_insert(0) += 1;
        }

        for assignment in assignments.values() {
            *summary
                .by_agent
                .entry(assignment.agent.clone())
                .or_insert(0) += 1;
        }

        summary
    }

    /// Assign a task to an agent
    pub async fn assign_task(&self, task_id: Uuid, agent: String) -> Result<(), AgentError> {
        let agent_name = agent.clone();

        let mut tasks = self.tasks.write().await;

        let task = tasks
            .get_mut(&task_id)
            .ok_or(AgentError::Task(TaskError::TaskNotFound(task_id)))?;

        if !task.is_ready() {
            return Err(AgentError::Task(TaskError::TaskBlocked(task_id)));
        }

        if task.owner.is_some() {
            return Err(AgentError::Task(TaskError::InvalidTaskState(task_id)));
        }

        task.assign_to(agent.clone());

        let assignment = TaskAssignment {
            task: task.clone(),
            agent,
            assigned_at: chrono::Utc::now(),
        };

        self.assignments.write().await.insert(task_id, assignment);

        if let Err(e) = self.event_sender.send(TaskBoardEvent::TaskAssigned {
            task_id,
            agent: agent_name.clone(),
        }) {
            tracing::debug!("Failed to send TaskAssigned event: {e}");
        }

        tracing::debug!(task_id = %task_id, agent = %agent_name, "Task assigned");

        Ok(())
    }

    /// Update task status
    pub async fn update_task_status(
        &self,
        task_id: Uuid,
        status: TaskStatus,
    ) -> Result<(), AgentError> {
        let status_display = format!("{status:?}");

        let mut tasks = self.tasks.write().await;

        let task = tasks
            .get_mut(&task_id)
            .ok_or(AgentError::Task(TaskError::TaskNotFound(task_id)))?;

        task.status = status.clone();
        task.updated_at = chrono::Utc::now();

        if let Err(e) = self
            .event_sender
            .send(TaskBoardEvent::TaskStatusChanged { task_id, status })
        {
            tracing::debug!("Failed to send TaskStatusChanged event: {e}");
        }

        tracing::debug!(task_id = %task_id, status = %status_display, "Task status updated");

        Ok(())
    }

    /// Mark a task as completed
    pub async fn complete_task(&self, task_id: Uuid) -> Result<(), AgentError> {
        let agent = self
            .assignments
            .read()
            .await
            .get(&task_id)
            .map(|a| a.agent.clone());

        self.update_task_status(task_id, TaskStatus::Completed)
            .await?;

        if let Some(agent) = agent {
            if let Err(e) = self
                .event_sender
                .send(TaskBoardEvent::TaskCompleted { task_id, agent })
            {
                tracing::debug!("Failed to send TaskCompleted event: {e}");
            }
        }

        Ok(())
    }

    /// Mark a task as failed
    pub async fn fail_task(&self, task_id: Uuid, reason: String) -> Result<(), AgentError> {
        let mut tasks = self.tasks.write().await;

        let task = tasks
            .get_mut(&task_id)
            .ok_or(AgentError::Task(TaskError::TaskNotFound(task_id)))?;

        task.mark_failed(reason.clone());

        if let Err(e) = self
            .event_sender
            .send(TaskBoardEvent::TaskFailed { task_id, reason })
        {
            tracing::debug!("Failed to send TaskFailed event: {e}");
        }

        Ok(())
    }

    /// Add a dependency between tasks
    pub async fn add_dependency(&self, task_id: Uuid, depends_on: Uuid) -> Result<(), AgentError> {
        {
            let tasks = self.tasks.read().await;

            // Check for circular dependency
            if self.has_circular_dependency(&tasks, task_id, depends_on) {
                return Err(AgentError::Task(TaskError::CircularDependency(task_id)));
            }
        }

        {
            let mut deps = self.dependencies.write().await;
            deps.entry(task_id)
                .or_insert_with(HashSet::new)
                .insert(depends_on);
        }

        {
            let mut reverse_deps = self.reverse_dependencies.write().await;
            reverse_deps
                .entry(depends_on)
                .or_insert_with(HashSet::new)
                .insert(task_id);
        }

        let mut tasks = self.tasks.write().await;

        let task = tasks
            .get_mut(&task_id)
            .ok_or(AgentError::Task(TaskError::TaskNotFound(task_id)))?;

        task.add_dependency(depends_on);

        let dep_task = tasks
            .get_mut(&depends_on)
            .ok_or(AgentError::Task(TaskError::TaskNotFound(depends_on)))?;

        if !dep_task.blocks.contains(&task_id) {
            dep_task.blocks.push(task_id);
        }

        if let Err(e) = self.event_sender.send(TaskBoardEvent::DependencyAdded {
            task_id,
            depends_on,
        }) {
            tracing::debug!("Failed to send DependencyAdded event: {e}");
        }

        tracing::debug!(
            task_id = %task_id,
            depends_on = %depends_on,
            "Dependency added"
        );

        Ok(())
    }

    /// Remove a dependency between tasks
    pub async fn remove_dependency(
        &self,
        task_id: Uuid,
        depends_on: Uuid,
    ) -> Result<(), AgentError> {
        {
            let mut deps = self.dependencies.write().await;
            if let Some(deps_set) = deps.get_mut(&task_id) {
                deps_set.remove(&depends_on);
            }
        }

        {
            let mut reverse_deps = self.reverse_dependencies.write().await;
            if let Some(reverse_set) = reverse_deps.get_mut(&depends_on) {
                reverse_set.remove(&task_id);
            }
        }

        let mut tasks = self.tasks.write().await;

        let task = tasks
            .get_mut(&task_id)
            .ok_or(AgentError::Task(TaskError::TaskNotFound(task_id)))?;

        task.blocked_by.retain(|id| *id != depends_on);

        let dep_task = tasks
            .get_mut(&depends_on)
            .ok_or(AgentError::Task(TaskError::TaskNotFound(depends_on)))?;

        dep_task.blocks.retain(|id| *id != task_id);

        tracing::debug!(
            task_id = %task_id,
            depends_on = %depends_on,
            "Dependency removed"
        );

        Ok(())
    }

    /// Check for circular dependencies
    #[allow(clippy::only_used_in_recursion)]
    fn has_circular_dependency(
        &self,
        tasks: &HashMap<Uuid, AgentTask>,
        start: Uuid,
        current: Uuid,
    ) -> bool {
        if start == current {
            return true;
        }

        let task = match tasks.get(&current) {
            Some(t) => t,
            None => return false,
        };

        for dep_id in &task.blocked_by {
            if self.has_circular_dependency(tasks, start, *dep_id) {
                return true;
            }
        }

        false
    }

    /// Get next available task for an agent
    pub async fn get_next_task(&self, agent: &str) -> Option<AgentTask> {
        let ready_tasks = self.list_ready_tasks().await;

        // Get tasks already assigned to this agent
        let assignments = self.assignments.read().await;
        let assigned_count = assignments.values().filter(|a| a.agent == agent).count();

        // Return first ready task if under limit
        if assigned_count < 3 {
            ready_tasks.into_iter().next()
        } else {
            None
        }
    }

    /// Get all tasks assigned to a specific agent
    pub async fn get_agent_tasks(&self, agent: &str) -> Vec<AgentTask> {
        let assignments = self.assignments.read().await;

        assignments
            .values()
            .filter(|a| a.agent == agent)
            .map(|a| a.task.clone())
            .collect()
    }

    /// Get count of tasks assigned to an agent
    pub async fn get_agent_task_count(&self, agent: &str) -> usize {
        let assignments = self.assignments.read().await;
        assignments.values().filter(|a| a.agent == agent).count()
    }

    /// Get all agents with assigned tasks
    pub async fn list_active_agents(&self) -> Vec<String> {
        let assignments = self.assignments.read().await;

        let mut agents: Vec<_> = assignments.values().map(|a| a.agent.clone()).collect();

        agents.sort();
        agents.dedup();

        agents
    }

    /// Remove a task from the board
    pub async fn remove_task(&self, task_id: Uuid) -> Result<(), AgentError> {
        let mut tasks = self.tasks.write().await;
        let mut assignments = self.assignments.write().await;

        tasks
            .remove(&task_id)
            .ok_or(AgentError::Task(TaskError::TaskNotFound(task_id)))?;

        assignments.remove(&task_id);

        {
            let mut deps = self.dependencies.write().await;
            deps.remove(&task_id);
        }

        {
            let mut reverse_deps = self.reverse_dependencies.write().await;
            reverse_deps.remove(&task_id);
        }

        if let Err(e) = self
            .event_sender
            .send(TaskBoardEvent::TaskRemoved { task_id })
        {
            tracing::debug!("Failed to send TaskRemoved event: {e}");
        }

        tracing::debug!(task_id = %task_id, "Task removed from board");

        Ok(())
    }

    /// Clear all tasks from the board
    pub async fn clear(&self) {
        self.tasks.write().await.clear();
        self.assignments.write().await.clear();
        self.dependencies.write().await.clear();
        self.reverse_dependencies.write().await.clear();

        tracing::debug!("Task board cleared");
    }
}

impl Default for TaskBoard {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn task_board_new_is_empty() {
        let board = TaskBoard::new();
        let tasks = board.tasks.read().await;
        assert!(tasks.is_empty());
    }

    #[tokio::test]
    async fn task_board_add_task() {
        let board = TaskBoard::new();
        let task = AgentTask::new(
            "Test task".into(),
            "Do something".into(),
            TaskPriority::Medium,
        );
        let task_id = task.id;
        board.add_task(task).await.unwrap();
        let tasks = board.tasks.read().await;
        assert!(tasks.contains_key(&task_id));
    }

    #[tokio::test]
    async fn task_board_add_duplicate_fails() {
        let board = TaskBoard::new();
        let task = AgentTask::new("T1".into(), "D1".into(), TaskPriority::Medium);
        board.add_task(task.clone()).await.unwrap();
        let result = board.add_task(task).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn task_board_fail_task() {
        let board = TaskBoard::new();
        let task = AgentTask::new("T1".into(), "D1".into(), TaskPriority::Medium);
        let task_id = task.id;
        board.add_task(task).await.unwrap();
        board
            .fail_task(task_id, "timeout".to_string())
            .await
            .unwrap();
        let tasks = board.tasks.read().await;
        match &tasks.get(&task_id).unwrap().status {
            TaskStatus::Failed(reason) => assert_eq!(reason, "timeout"),
            other => panic!("Expected Failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn task_board_remove_task() {
        let board = TaskBoard::new();
        let task = AgentTask::new("T1".into(), "D1".into(), TaskPriority::Medium);
        let task_id = task.id;
        board.add_task(task).await.unwrap();
        board.remove_task(task_id).await.unwrap();
        let tasks = board.tasks.read().await;
        assert!(!tasks.contains_key(&task_id));
    }

    #[tokio::test]
    async fn task_board_get_task() {
        let board = TaskBoard::new();
        let task = AgentTask::new("T1".into(), "D1".into(), TaskPriority::Medium);
        let task_id = task.id;
        board.add_task(task).await.unwrap();
        let retrieved = board.get_task(task_id).await.unwrap();
        assert_eq!(retrieved.subject, "T1");
        assert!(board.get_task(Uuid::new_v4()).await.is_err());
    }

    #[tokio::test]
    async fn task_board_events() {
        let board = TaskBoard::new();
        let mut rx = board.subscribe_events();
        let task = AgentTask::new("T1".into(), "D1".into(), TaskPriority::Medium);
        let task_id = task.id;
        board.add_task(task).await.unwrap();
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, TaskBoardEvent::TaskAdded { task_id: tid } if tid == task_id));
    }

    #[tokio::test]
    async fn task_board_summary() {
        let board = TaskBoard::new();
        let t1 = AgentTask::new("T1".into(), "D1".into(), TaskPriority::High);
        let t2 = AgentTask::new("T2".into(), "D2".into(), TaskPriority::Medium);
        board.add_task(t1).await.unwrap();
        board.add_task(t2).await.unwrap();
        let summary = board.summary().await;
        assert_eq!(summary.total_tasks, 2);
        assert_eq!(summary.pending_tasks, 2);
    }

    #[test]
    fn task_board_summary_serde() {
        let summary = TaskBoardSummary {
            total_tasks: 5,
            pending_tasks: 2,
            in_progress_tasks: 1,
            completed_tasks: 1,
            failed_tasks: 1,
            blocked_tasks: 0,
            by_priority: HashMap::from([("High".to_string(), 2), ("Medium".to_string(), 3)]),
            by_agent: HashMap::from([("worker-1".to_string(), 3)]),
        };
        let json = serde_json::to_string(&summary).unwrap();
        let de: TaskBoardSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(de.total_tasks, 5);
        assert_eq!(de.by_agent["worker-1"], 3);
    }

    #[test]
    fn task_board_default() {
        let board = TaskBoard::default();
        let _ = board;
    }

    #[tokio::test]
    async fn task_board_clear() {
        let board = TaskBoard::new();
        let task = AgentTask::new("T1".into(), "D1".into(), TaskPriority::Medium);
        board.add_task(task).await.unwrap();
        board.clear().await;
        let tasks = board.tasks.read().await;
        assert!(tasks.is_empty());
    }

    // ── Assignment tests ─────────────────────────────────────────────────

    #[tokio::test]
    async fn task_board_assign_task() {
        let board = TaskBoard::new();
        let task = AgentTask::new("T1".into(), "D1".into(), TaskPriority::Medium);
        let task_id = task.id;
        board.add_task(task).await.unwrap();
        board
            .assign_task(task_id, "worker-1".to_string())
            .await
            .unwrap();
        let agent_tasks = board.get_agent_tasks("worker-1").await;
        assert_eq!(agent_tasks.len(), 1);
        assert_eq!(agent_tasks[0].subject, "T1");
    }

    #[tokio::test]
    async fn task_board_assign_nonexistent_fails() {
        let board = TaskBoard::new();
        let result = board
            .assign_task(Uuid::new_v4(), "worker-1".to_string())
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn task_board_get_agent_task_count() {
        let board = TaskBoard::new();
        let t1 = AgentTask::new("T1".into(), "D1".into(), TaskPriority::Medium);
        let t2 = AgentTask::new("T2".into(), "D2".into(), TaskPriority::Medium);
        let id1 = t1.id;
        let id2 = t2.id;
        board.add_task(t1).await.unwrap();
        board.add_task(t2).await.unwrap();
        board
            .assign_task(id1, "worker-1".to_string())
            .await
            .unwrap();
        board
            .assign_task(id2, "worker-1".to_string())
            .await
            .unwrap();
        assert_eq!(board.get_agent_task_count("worker-1").await, 2);
        assert_eq!(board.get_agent_task_count("worker-2").await, 0);
    }

    #[tokio::test]
    async fn task_board_list_active_agents() {
        let board = TaskBoard::new();
        let t1 = AgentTask::new("T1".into(), "D1".into(), TaskPriority::Medium);
        let t2 = AgentTask::new("T2".into(), "D2".into(), TaskPriority::Medium);
        let id1 = t1.id;
        let id2 = t2.id;
        board.add_task(t1).await.unwrap();
        board.add_task(t2).await.unwrap();
        board
            .assign_task(id1, "worker-1".to_string())
            .await
            .unwrap();
        board
            .assign_task(id2, "worker-2".to_string())
            .await
            .unwrap();
        let agents = board.list_active_agents().await;
        assert_eq!(agents.len(), 2);
    }

    // ── Status update tests ──────────────────────────────────────────────

    #[tokio::test]
    async fn task_board_update_status() {
        let board = TaskBoard::new();
        let task = AgentTask::new("T1".into(), "D1".into(), TaskPriority::Medium);
        let task_id = task.id;
        board.add_task(task).await.unwrap();
        board
            .update_task_status(task_id, TaskStatus::InProgress)
            .await
            .unwrap();
        let retrieved = board.get_task(task_id).await.unwrap();
        assert_eq!(retrieved.status, TaskStatus::InProgress);
    }

    #[tokio::test]
    async fn task_board_complete_task() {
        let board = TaskBoard::new();
        let task = AgentTask::new("T1".into(), "D1".into(), TaskPriority::Medium);
        let task_id = task.id;
        board.add_task(task).await.unwrap();
        board
            .assign_task(task_id, "worker-1".to_string())
            .await
            .unwrap();
        board.complete_task(task_id).await.unwrap();
        let retrieved = board.get_task(task_id).await.unwrap();
        assert_eq!(retrieved.status, TaskStatus::Completed);
    }

    // ── Dependency tests ─────────────────────────────────────────────────

    #[tokio::test]
    async fn task_board_add_dependency() {
        let board = TaskBoard::new();
        let t1 = AgentTask::new("T1".into(), "First".into(), TaskPriority::High);
        let t2 = AgentTask::new("T2".into(), "Second".into(), TaskPriority::Medium);
        board.add_task(t1.clone()).await.unwrap();
        board.add_task(t2.clone()).await.unwrap();
        board.add_dependency(t2.id, t1.id).await.unwrap();
        let deps = board.dependencies.read().await;
        assert!(deps.get(&t2.id).unwrap().contains(&t1.id));
    }

    #[tokio::test]
    async fn task_board_circular_dependency_rejected() {
        let board = TaskBoard::new();
        let t1 = AgentTask::new("T1".into(), "First".into(), TaskPriority::High);
        let t2 = AgentTask::new("T2".into(), "Second".into(), TaskPriority::Medium);
        board.add_task(t1.clone()).await.unwrap();
        board.add_task(t2.clone()).await.unwrap();
        board.add_dependency(t2.id, t1.id).await.unwrap();
        let result = board.add_dependency(t1.id, t2.id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn task_board_remove_dependency() {
        let board = TaskBoard::new();
        let t1 = AgentTask::new("T1".into(), "First".into(), TaskPriority::High);
        let t2 = AgentTask::new("T2".into(), "Second".into(), TaskPriority::Medium);
        board.add_task(t1.clone()).await.unwrap();
        board.add_task(t2.clone()).await.unwrap();
        board.add_dependency(t2.id, t1.id).await.unwrap();
        board.remove_dependency(t2.id, t1.id).await.unwrap();
        let deps = board.dependencies.read().await;
        assert!(deps.get(&t2.id).is_none_or(|s| !s.contains(&t1.id)));
    }

    // ── Priority ordering tests ──────────────────────────────────────────

    #[tokio::test]
    async fn task_board_list_by_priority() {
        let board = TaskBoard::new();
        let t1 = AgentTask::new("Low".into(), "D1".into(), TaskPriority::Low);
        let t2 = AgentTask::new("High".into(), "D2".into(), TaskPriority::High);
        let t3 = AgentTask::new("Medium".into(), "D3".into(), TaskPriority::Medium);
        board.add_task(t1).await.unwrap();
        board.add_task(t2).await.unwrap();
        board.add_task(t3).await.unwrap();
        let high = board.list_tasks_by_priority(TaskPriority::High).await;
        assert_eq!(high.len(), 1);
        assert_eq!(high[0].subject, "High");
    }

    #[tokio::test]
    async fn task_board_list_by_status() {
        let board = TaskBoard::new();
        let t1 = AgentTask::new("T1".into(), "D1".into(), TaskPriority::Medium);
        let t2 = AgentTask::new("T2".into(), "D2".into(), TaskPriority::Medium);
        board.add_task(t1.clone()).await.unwrap();
        board.add_task(t2).await.unwrap();
        board
            .update_task_status(t1.id, TaskStatus::InProgress)
            .await
            .unwrap();
        let pending = board.list_tasks_by_status(TaskStatus::Pending).await;
        let in_progress = board.list_tasks_by_status(TaskStatus::InProgress).await;
        assert_eq!(pending.len(), 1);
        assert_eq!(in_progress.len(), 1);
    }

    // ── Event emission tests ─────────────────────────────────────────────

    #[tokio::test]
    async fn task_board_assign_event() {
        let board = TaskBoard::new();
        let mut rx = board.subscribe_events();
        let task = AgentTask::new("T1".into(), "D1".into(), TaskPriority::Medium);
        let task_id = task.id;
        board.add_task(task).await.unwrap();
        let _ = rx.try_recv(); // consume TaskAdded
        board
            .assign_task(task_id, "worker-1".to_string())
            .await
            .unwrap();
        let event = rx.try_recv().unwrap();
        assert!(
            matches!(event, TaskBoardEvent::TaskAssigned { task_id: tid, agent } if tid == task_id && agent == "worker-1")
        );
    }

    #[tokio::test]
    async fn task_board_complete_event() {
        let board = TaskBoard::new();
        let mut rx = board.subscribe_events();
        let task = AgentTask::new("T1".into(), "D1".into(), TaskPriority::Medium);
        let task_id = task.id;
        board.add_task(task).await.unwrap();
        let _ = rx.try_recv(); // TaskAdded
        board
            .assign_task(task_id, "worker-1".to_string())
            .await
            .unwrap();
        let _ = rx.try_recv(); // TaskAssigned
        board.complete_task(task_id).await.unwrap();
        let _ = rx.try_recv(); // TaskStatusChanged from update_task_status
        let event = rx.try_recv().unwrap();
        assert!(
            matches!(event, TaskBoardEvent::TaskCompleted { task_id: tid, agent } if tid == task_id && agent == "worker-1")
        );
    }

    #[tokio::test]
    async fn task_board_get_next_task_returns_ready_task() {
        let board = TaskBoard::new();
        let t1 = AgentTask::new("Task 1".into(), "D1".into(), TaskPriority::High);
        let t2 = AgentTask::new("Task 2".into(), "D2".into(), TaskPriority::Low);
        board.add_task(t1).await.unwrap();
        board.add_task(t2).await.unwrap();
        let next = board.get_next_task("worker-1").await;
        assert!(next.is_some());
        let task = next.unwrap();
        assert!(task.subject == "Task 1" || task.subject == "Task 2");
    }
}
