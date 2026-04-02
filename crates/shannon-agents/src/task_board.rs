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

        let _ = self.event_sender.send(TaskBoardEvent::TaskAdded { task_id });

        tracing::debug!(task_id = %task_id, subject = %subject, "Task added to board");

        Ok(())
    }

    /// Get a task by ID
    pub async fn get_task(&self, task_id: Uuid) -> Result<AgentTask, AgentError> {
        let tasks = self.tasks.read().await;

        tasks.get(&task_id)
            .cloned()
            .ok_or_else(|| AgentError::Task(TaskError::TaskNotFound(task_id)))
    }

    /// List all pending tasks that are ready to be started
    pub async fn list_ready_tasks(&self) -> Vec<AgentTask> {
        let tasks = self.tasks.read().await;

        tasks.values()
            .filter(|task| task.is_ready())
            .cloned()
            .collect()
    }

    /// List tasks by status
    pub async fn list_tasks_by_status(&self, status: TaskStatus) -> Vec<AgentTask> {
        let tasks = self.tasks.read().await;

        tasks.values()
            .filter(|task| task.status == status)
            .cloned()
            .collect()
    }

    /// List tasks by priority
    pub async fn list_tasks_by_priority(&self, priority: TaskPriority) -> Vec<AgentTask> {
        let tasks = self.tasks.read().await;

        let mut result: Vec<_> = tasks.values()
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
            *summary.by_agent.entry(assignment.agent.clone()).or_insert(0) += 1;
        }

        summary
    }

    /// Assign a task to an agent
    pub async fn assign_task(&self, task_id: Uuid, agent: String) -> Result<(), AgentError> {
        let agent_name = agent.clone();

        let mut tasks = self.tasks.write().await;

        let task = tasks.get_mut(&task_id)
            .ok_or_else(|| AgentError::Task(TaskError::TaskNotFound(task_id)))?;

        if !task.is_ready() {
            return Err(AgentError::Task(TaskError::TaskBlocked(task_id)));
        }

        task.assign_to(agent.clone());

        let assignment = TaskAssignment {
            task: task.clone(),
            agent,
            assigned_at: chrono::Utc::now(),
        };

        self.assignments.write().await.insert(task_id, assignment);

        let _ = self.event_sender.send(TaskBoardEvent::TaskAssigned {
            task_id,
            agent: agent_name.clone(),
        });

        tracing::debug!(task_id = %task_id, agent = %agent_name, "Task assigned");

        Ok(())
    }

    /// Update task status
    pub async fn update_task_status(&self, task_id: Uuid, status: TaskStatus) -> Result<(), AgentError> {
        let status_display = format!("{:?}", status);

        let mut tasks = self.tasks.write().await;

        let task = tasks.get_mut(&task_id)
            .ok_or_else(|| AgentError::Task(TaskError::TaskNotFound(task_id)))?;

        task.status = status.clone();
        task.updated_at = chrono::Utc::now();

        let _ = self.event_sender.send(TaskBoardEvent::TaskStatusChanged {
            task_id,
            status,
        });

        tracing::debug!(task_id = %task_id, status = %status_display, "Task status updated");

        Ok(())
    }

    /// Mark a task as completed
    pub async fn complete_task(&self, task_id: Uuid) -> Result<(), AgentError> {
        let agent = self.assignments.read().await
            .get(&task_id)
            .map(|a| a.agent.clone());

        self.update_task_status(task_id, TaskStatus::Completed).await?;

        if let Some(agent) = agent {
            let _ = self.event_sender.send(TaskBoardEvent::TaskCompleted {
                task_id,
                agent,
            });
        }

        Ok(())
    }

    /// Mark a task as failed
    pub async fn fail_task(&self, task_id: Uuid, reason: String) -> Result<(), AgentError> {
        let mut tasks = self.tasks.write().await;

        let task = tasks.get_mut(&task_id)
            .ok_or_else(|| AgentError::Task(TaskError::TaskNotFound(task_id)))?;

        task.mark_failed(reason.clone());

        let _ = self.event_sender.send(TaskBoardEvent::TaskFailed {
            task_id,
            reason,
        });

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
            deps.entry(task_id).or_insert_with(HashSet::new).insert(depends_on);
        }

        {
            let mut reverse_deps = self.reverse_dependencies.write().await;
            reverse_deps.entry(depends_on).or_insert_with(HashSet::new).insert(task_id);
        }

        let mut tasks = self.tasks.write().await;

        let task = tasks.get_mut(&task_id)
            .ok_or_else(|| AgentError::Task(TaskError::TaskNotFound(task_id)))?;

        task.add_dependency(depends_on);

        let dep_task = tasks.get_mut(&depends_on)
            .ok_or_else(|| AgentError::Task(TaskError::TaskNotFound(depends_on)))?;

        if !dep_task.blocks.contains(&task_id) {
            dep_task.blocks.push(task_id);
        }

        let _ = self.event_sender.send(TaskBoardEvent::DependencyAdded {
            task_id,
            depends_on,
        });

        tracing::debug!(
            task_id = %task_id,
            depends_on = %depends_on,
            "Dependency added"
        );

        Ok(())
    }

    /// Remove a dependency between tasks
    pub async fn remove_dependency(&self, task_id: Uuid, depends_on: Uuid) -> Result<(), AgentError> {
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

        let task = tasks.get_mut(&task_id)
            .ok_or_else(|| AgentError::Task(TaskError::TaskNotFound(task_id)))?;

        task.blocked_by.retain(|id| *id != depends_on);

        let dep_task = tasks.get_mut(&depends_on)
            .ok_or_else(|| AgentError::Task(TaskError::TaskNotFound(depends_on)))?;

        dep_task.blocks.retain(|id| *id != task_id);

        tracing::debug!(
            task_id = %task_id,
            depends_on = %depends_on,
            "Dependency removed"
        );

        Ok(())
    }

    /// Check for circular dependencies
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
        let assigned_count = assignments.values()
            .filter(|a| a.agent == agent)
            .count();

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

        assignments.values()
            .filter(|a| a.agent == agent)
            .map(|a| a.task.clone())
            .collect()
    }

    /// Get count of tasks assigned to an agent
    pub async fn get_agent_task_count(&self, agent: &str) -> usize {
        let assignments = self.assignments.read().await;
        assignments.values()
            .filter(|a| a.agent == agent)
            .count()
    }

    /// Get all agents with assigned tasks
    pub async fn list_active_agents(&self) -> Vec<String> {
        let assignments = self.assignments.read().await;

        let mut agents: Vec<_> = assignments.values()
            .map(|a| a.agent.clone())
            .collect();

        agents.sort();
        agents.dedup();

        agents
    }

    /// Remove a task from the board
    pub async fn remove_task(&self, task_id: Uuid) -> Result<(), AgentError> {
        let mut tasks = self.tasks.write().await;
        let mut assignments = self.assignments.write().await;

        tasks.remove(&task_id)
            .ok_or_else(|| AgentError::Task(TaskError::TaskNotFound(task_id)))?;

        assignments.remove(&task_id);

        {
            let mut deps = self.dependencies.write().await;
            deps.remove(&task_id);
        }

        {
            let mut reverse_deps = self.reverse_dependencies.write().await;
            reverse_deps.remove(&task_id);
        }

        let _ = self.event_sender.send(TaskBoardEvent::TaskRemoved { task_id });

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
