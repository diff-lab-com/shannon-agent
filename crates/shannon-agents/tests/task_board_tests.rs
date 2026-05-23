//! Comprehensive tests for TaskBoard — the core multi-agent task coordination structure.

use shannon_agents::{
    AgentError, AgentTask, TaskBoard, TaskBoardEvent, TaskError, TaskPriority, TaskStatus,
};
use uuid::Uuid;

fn make_task(subject: &str, priority: TaskPriority) -> AgentTask {
    AgentTask::new(subject.to_string(), format!("Do {subject}"), priority)
}

// ── Lifecycle: add, get, remove ──

#[tokio::test]
async fn test_add_and_get_task() {
    let board = TaskBoard::new();
    let task = make_task("Implement auth", TaskPriority::High);
    let id = task.id;

    board.add_task(task).await.unwrap();
    let fetched = board.get_task(id).await.unwrap();
    assert_eq!(fetched.subject, "Implement auth");
    assert_eq!(fetched.status, TaskStatus::Pending);
    assert_eq!(fetched.priority, TaskPriority::High);
}

#[tokio::test]
async fn test_add_duplicate_task_fails() {
    let board = TaskBoard::new();
    let task = make_task("Duplicate task", TaskPriority::Medium);
    let id = task.id;

    board.add_task(task).await.unwrap();

    let mut dup = make_task("Other", TaskPriority::Low);
    dup.id = id;
    let result = board.add_task(dup).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_get_nonexistent_task() {
    let board = TaskBoard::new();
    let result = board.get_task(Uuid::new_v4()).await;
    assert!(matches!(
        result,
        Err(AgentError::Task(TaskError::TaskNotFound(_)))
    ));
}

#[tokio::test]
async fn test_remove_task() {
    let board = TaskBoard::new();
    let task = make_task("To remove", TaskPriority::Low);
    let id = task.id;

    board.add_task(task).await.unwrap();
    board.remove_task(id).await.unwrap();

    let result = board.get_task(id).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_remove_nonexistent_task() {
    let board = TaskBoard::new();
    let result = board.remove_task(Uuid::new_v4()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_clear_board() {
    let board = TaskBoard::new();

    for i in 0..5 {
        board
            .add_task(make_task(&format!("Task {i}"), TaskPriority::Medium))
            .await
            .unwrap();
    }

    assert_eq!(board.list_all_tasks().await.len(), 5);
    board.clear().await;
    assert_eq!(board.list_all_tasks().await.len(), 0);
}

// ── Listing and filtering ──

#[tokio::test]
async fn test_list_all_tasks() {
    let board = TaskBoard::new();
    board
        .add_task(make_task("A", TaskPriority::High))
        .await
        .unwrap();
    board
        .add_task(make_task("B", TaskPriority::Low))
        .await
        .unwrap();
    board
        .add_task(make_task("C", TaskPriority::Medium))
        .await
        .unwrap();

    let all = board.list_all_tasks().await;
    assert_eq!(all.len(), 3);
}

#[tokio::test]
async fn test_list_ready_tasks() {
    let board = TaskBoard::new();
    let ready = make_task("Ready", TaskPriority::High);
    let ready_id = ready.id;

    let mut blocked = make_task("Blocked", TaskPriority::Medium);
    blocked.status = TaskStatus::Blocked;

    board.add_task(ready).await.unwrap();
    board.add_task(blocked).await.unwrap();

    let ready_tasks = board.list_ready_tasks().await;
    assert_eq!(ready_tasks.len(), 1);
    assert_eq!(ready_tasks[0].id, ready_id);
}

#[tokio::test]
async fn test_list_tasks_by_status() {
    let board = TaskBoard::new();

    let mut in_progress = make_task("WIP", TaskPriority::High);
    in_progress.status = TaskStatus::InProgress;

    let pending = make_task("Pending", TaskPriority::Medium);

    board.add_task(pending).await.unwrap();
    board.add_task(in_progress).await.unwrap();

    let pending_tasks = board.list_tasks_by_status(TaskStatus::Pending).await;
    assert_eq!(pending_tasks.len(), 1);

    let wip_tasks = board.list_tasks_by_status(TaskStatus::InProgress).await;
    assert_eq!(wip_tasks.len(), 1);
}

#[tokio::test]
async fn test_list_tasks_by_priority() {
    let board = TaskBoard::new();
    board
        .add_task(make_task("H1", TaskPriority::High))
        .await
        .unwrap();
    board
        .add_task(make_task("M1", TaskPriority::Medium))
        .await
        .unwrap();
    board
        .add_task(make_task("H2", TaskPriority::High))
        .await
        .unwrap();

    let high = board.list_tasks_by_priority(TaskPriority::High).await;
    assert_eq!(high.len(), 2);

    let medium = board.list_tasks_by_priority(TaskPriority::Medium).await;
    assert_eq!(medium.len(), 1);

    let low = board.list_tasks_by_priority(TaskPriority::Low).await;
    assert!(low.is_empty());
}

// ── Summary ──

#[tokio::test]
async fn test_summary_counts() {
    let board = TaskBoard::new();

    board
        .add_task(make_task("P1", TaskPriority::High))
        .await
        .unwrap();
    board
        .add_task(make_task("P2", TaskPriority::Medium))
        .await
        .unwrap();

    let mut wip = make_task("WIP", TaskPriority::Low);
    wip.status = TaskStatus::InProgress;
    board.add_task(wip).await.unwrap();

    let summary = board.summary().await;
    assert_eq!(summary.total_tasks, 3);
    assert_eq!(summary.pending_tasks, 2);
    assert_eq!(summary.in_progress_tasks, 1);
    assert_eq!(summary.completed_tasks, 0);
}

#[tokio::test]
async fn test_summary_priority_breakdown() {
    let board = TaskBoard::new();
    board
        .add_task(make_task("H", TaskPriority::High))
        .await
        .unwrap();
    board
        .add_task(make_task("H2", TaskPriority::High))
        .await
        .unwrap();
    board
        .add_task(make_task("M", TaskPriority::Medium))
        .await
        .unwrap();

    let summary = board.summary().await;
    assert_eq!(*summary.by_priority.get("High").unwrap_or(&0), 2);
    assert_eq!(*summary.by_priority.get("Medium").unwrap_or(&0), 1);
}

#[tokio::test]
async fn test_summary_agent_breakdown() {
    let board = TaskBoard::new();
    let t1 = make_task("T1", TaskPriority::High);
    let t2 = make_task("T2", TaskPriority::Medium);
    let id1 = t1.id;
    let id2 = t2.id;

    board.add_task(t1).await.unwrap();
    board.add_task(t2).await.unwrap();

    board.assign_task(id1, "agent-a".to_string()).await.unwrap();
    board.assign_task(id2, "agent-b".to_string()).await.unwrap();

    let summary = board.summary().await;
    assert_eq!(*summary.by_agent.get("agent-a").unwrap_or(&0), 1);
    assert_eq!(*summary.by_agent.get("agent-b").unwrap_or(&0), 1);
}

// ── Assignment ──

#[tokio::test]
async fn test_assign_task() {
    let board = TaskBoard::new();
    let task = make_task("Work", TaskPriority::High);
    let id = task.id;

    board.add_task(task).await.unwrap();
    board.assign_task(id, "agent-1".to_string()).await.unwrap();

    let fetched = board.get_task(id).await.unwrap();
    assert_eq!(fetched.owner.as_deref(), Some("agent-1"));
    assert_eq!(fetched.status, TaskStatus::InProgress);
}

#[tokio::test]
async fn test_assign_nonexistent_task_fails() {
    let board = TaskBoard::new();
    let result = board.assign_task(Uuid::new_v4(), "agent".to_string()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_assign_already_assigned_task_fails() {
    let board = TaskBoard::new();
    let task = make_task("Owned", TaskPriority::High);
    let id = task.id;

    board.add_task(task).await.unwrap();
    board.assign_task(id, "agent-1".to_string()).await.unwrap();

    let result = board.assign_task(id, "agent-2".to_string()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_assign_blocked_task_fails() {
    let board = TaskBoard::new();
    let mut task = make_task("Blocked", TaskPriority::Medium);
    task.status = TaskStatus::Blocked;
    let id = task.id;

    board.add_task(task).await.unwrap();
    let result = board.assign_task(id, "agent".to_string()).await;
    assert!(result.is_err());
}

// ── Status updates ──

#[tokio::test]
async fn test_complete_task() {
    let board = TaskBoard::new();
    let task = make_task("To complete", TaskPriority::High);
    let id = task.id;

    board.add_task(task).await.unwrap();
    board.assign_task(id, "agent-1".to_string()).await.unwrap();
    board.complete_task(id).await.unwrap();

    let fetched = board.get_task(id).await.unwrap();
    assert_eq!(fetched.status, TaskStatus::Completed);
}

#[tokio::test]
async fn test_fail_task() {
    let board = TaskBoard::new();
    let task = make_task("To fail", TaskPriority::Medium);
    let id = task.id;

    board.add_task(task).await.unwrap();
    board.fail_task(id, "timeout".to_string()).await.unwrap();

    let fetched = board.get_task(id).await.unwrap();
    assert!(matches!(fetched.status, TaskStatus::Failed(ref r) if r == "timeout"));
}

#[tokio::test]
async fn test_complete_nonexistent_task_fails() {
    let board = TaskBoard::new();
    let result = board.complete_task(Uuid::new_v4()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_fail_nonexistent_task_fails() {
    let board = TaskBoard::new();
    let result = board.fail_task(Uuid::new_v4(), "reason".to_string()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_update_task_status() {
    let board = TaskBoard::new();
    let task = make_task("Status test", TaskPriority::Low);
    let id = task.id;

    board.add_task(task).await.unwrap();
    board
        .update_task_status(id, TaskStatus::Cancelled)
        .await
        .unwrap();

    let fetched = board.get_task(id).await.unwrap();
    assert_eq!(fetched.status, TaskStatus::Cancelled);
}

// ── Dependencies ──

#[tokio::test]
async fn test_add_dependency() {
    let board = TaskBoard::new();
    let t1 = make_task("Parent", TaskPriority::High);
    let t2 = make_task("Child", TaskPriority::Medium);
    let id1 = t1.id;
    let id2 = t2.id;

    board.add_task(t1).await.unwrap();
    board.add_task(t2).await.unwrap();

    board.add_dependency(id2, id1).await.unwrap();

    let child = board.get_task(id2).await.unwrap();
    assert!(child.blocked_by.contains(&id1));

    let parent = board.get_task(id1).await.unwrap();
    assert!(parent.blocks.contains(&id2));
}

#[tokio::test]
async fn test_circular_dependency_rejected() {
    let board = TaskBoard::new();
    let t1 = make_task("A", TaskPriority::High);
    let t2 = make_task("B", TaskPriority::Medium);
    let t3 = make_task("C", TaskPriority::Low);
    let id1 = t1.id;
    let id2 = t2.id;
    let id3 = t3.id;

    board.add_task(t1).await.unwrap();
    board.add_task(t2).await.unwrap();
    board.add_task(t3).await.unwrap();

    board.add_dependency(id2, id1).await.unwrap();
    board.add_dependency(id3, id2).await.unwrap();

    let result = board.add_dependency(id1, id3).await;
    assert!(matches!(
        result,
        Err(AgentError::Task(TaskError::CircularDependency(_)))
    ));
}

#[tokio::test]
async fn test_self_dependency_rejected() {
    let board = TaskBoard::new();
    let task = make_task("Self", TaskPriority::Medium);
    let id = task.id;

    board.add_task(task).await.unwrap();
    let result = board.add_dependency(id, id).await;
    assert!(matches!(
        result,
        Err(AgentError::Task(TaskError::CircularDependency(_)))
    ));
}

#[tokio::test]
async fn test_remove_dependency() {
    let board = TaskBoard::new();
    let t1 = make_task("A", TaskPriority::High);
    let t2 = make_task("B", TaskPriority::Medium);
    let id1 = t1.id;
    let id2 = t2.id;

    board.add_task(t1).await.unwrap();
    board.add_task(t2).await.unwrap();
    board.add_dependency(id2, id1).await.unwrap();

    board.remove_dependency(id2, id1).await.unwrap();

    let child = board.get_task(id2).await.unwrap();
    assert!(!child.blocked_by.contains(&id1));
}

#[tokio::test]
async fn test_blocked_task_not_ready() {
    let board = TaskBoard::new();
    let t1 = make_task("Blocker", TaskPriority::High);
    let t2 = make_task("Blocked", TaskPriority::Medium);
    let id1 = t1.id;
    let id2 = t2.id;

    board.add_task(t1).await.unwrap();
    board.add_task(t2).await.unwrap();
    board.add_dependency(id2, id1).await.unwrap();

    let ready = board.list_ready_tasks().await;
    assert!(ready.iter().all(|t| t.id != id2));
    assert!(ready.iter().any(|t| t.id == id1));
}

// ── Agent queries ──

#[tokio::test]
async fn test_get_agent_tasks() {
    let board = TaskBoard::new();
    let t1 = make_task("A1", TaskPriority::High);
    let t2 = make_task("A2", TaskPriority::Medium);
    let t3 = make_task("B1", TaskPriority::Low);
    let id1 = t1.id;
    let id2 = t2.id;
    let id3 = t3.id;

    board.add_task(t1).await.unwrap();
    board.add_task(t2).await.unwrap();
    board.add_task(t3).await.unwrap();

    board.assign_task(id1, "agent-a".to_string()).await.unwrap();
    board.assign_task(id2, "agent-a".to_string()).await.unwrap();
    board.assign_task(id3, "agent-b".to_string()).await.unwrap();

    let a_tasks = board.get_agent_tasks("agent-a").await;
    assert_eq!(a_tasks.len(), 2);

    let b_tasks = board.get_agent_tasks("agent-b").await;
    assert_eq!(b_tasks.len(), 1);

    let c_tasks = board.get_agent_tasks("agent-c").await;
    assert!(c_tasks.is_empty());
}

#[tokio::test]
async fn test_get_agent_task_count() {
    let board = TaskBoard::new();
    let t1 = make_task("X", TaskPriority::High);
    let t2 = make_task("Y", TaskPriority::Medium);
    let id1 = t1.id;
    let id2 = t2.id;

    board.add_task(t1).await.unwrap();
    board.add_task(t2).await.unwrap();
    board.assign_task(id1, "worker".to_string()).await.unwrap();
    board.assign_task(id2, "worker".to_string()).await.unwrap();

    assert_eq!(board.get_agent_task_count("worker").await, 2);
    assert_eq!(board.get_agent_task_count("nobody").await, 0);
}

#[tokio::test]
async fn test_list_active_agents() {
    let board = TaskBoard::new();
    let t1 = make_task("T1", TaskPriority::High);
    let t2 = make_task("T2", TaskPriority::Medium);
    let id1 = t1.id;
    let id2 = t2.id;

    board.add_task(t1).await.unwrap();
    board.add_task(t2).await.unwrap();
    board.assign_task(id1, "agent-x".to_string()).await.unwrap();
    board.assign_task(id2, "agent-y".to_string()).await.unwrap();

    let agents = board.list_active_agents().await;
    assert_eq!(agents.len(), 2);
    assert!(agents.contains(&"agent-x".to_string()));
    assert!(agents.contains(&"agent-y".to_string()));
}

#[tokio::test]
async fn test_list_active_agents_empty() {
    let board = TaskBoard::new();
    let agents = board.list_active_agents().await;
    assert!(agents.is_empty());
}

// ── get_next_task ──

#[tokio::test]
async fn test_get_next_task_returns_ready_task() {
    let board = TaskBoard::new();
    let low = make_task("Low", TaskPriority::Low);
    let high = make_task("High", TaskPriority::High);

    board.add_task(low).await.unwrap();
    board.add_task(high).await.unwrap();

    let next = board.get_next_task("agent").await;
    assert!(next.is_some());
}

#[tokio::test]
async fn test_get_next_task_none_when_no_ready() {
    let board = TaskBoard::new();
    let task = make_task("Only one", TaskPriority::High);
    let id = task.id;

    board.add_task(task).await.unwrap();
    board.assign_task(id, "agent".to_string()).await.unwrap();

    let next = board.get_next_task("agent").await;
    assert!(next.is_none());
}

#[tokio::test]
async fn test_get_next_task_agent_limit() {
    let board = TaskBoard::new();

    for i in 0..3 {
        let task = make_task(&format!("Task {i}"), TaskPriority::High);
        let id = task.id;
        board.add_task(task).await.unwrap();
        board
            .assign_task(id, "busy-agent".to_string())
            .await
            .unwrap();
    }

    board
        .add_task(make_task("Extra", TaskPriority::High))
        .await
        .unwrap();

    let next = board.get_next_task("busy-agent").await;
    assert!(next.is_none());
}

#[tokio::test]
async fn test_get_next_task_other_agent_can_claim() {
    let board = TaskBoard::new();

    for i in 0..3 {
        let task = make_task(&format!("Task {i}"), TaskPriority::High);
        let id = task.id;
        board.add_task(task).await.unwrap();
        board.assign_task(id, "agent-a".to_string()).await.unwrap();
    }

    board
        .add_task(make_task("Extra", TaskPriority::High))
        .await
        .unwrap();

    let next = board.get_next_task("agent-b").await;
    assert!(next.is_some());
}

// ── Events ──

#[tokio::test]
async fn test_task_added_event() {
    let board = TaskBoard::new();
    let mut rx = board.subscribe_events();

    let task = make_task("Event test", TaskPriority::High);
    let id = task.id;
    board.add_task(task).await.unwrap();

    let event = rx.try_recv().unwrap();
    assert!(matches!(event, TaskBoardEvent::TaskAdded { task_id } if task_id == id));
}

#[tokio::test]
async fn test_task_assigned_event() {
    let board = TaskBoard::new();
    let mut rx = board.subscribe_events();

    let task = make_task("Assign event", TaskPriority::High);
    let id = task.id;
    board.add_task(task).await.unwrap();
    let _ = rx.try_recv(); // consume TaskAdded

    board.assign_task(id, "worker".to_string()).await.unwrap();

    let event = rx.try_recv().unwrap();
    assert!(
        matches!(event, TaskBoardEvent::TaskAssigned { task_id, agent } if task_id == id && agent == "worker")
    );
}

#[tokio::test]
async fn test_task_completed_event() {
    let board = TaskBoard::new();
    let mut rx = board.subscribe_events();

    let task = make_task("Complete event", TaskPriority::High);
    let id = task.id;
    board.add_task(task).await.unwrap();
    let _ = rx.try_recv(); // TaskAdded
    board.assign_task(id, "worker".to_string()).await.unwrap();
    let _ = rx.try_recv(); // TaskAssigned
    let _ = rx.try_recv(); // TaskStatusChanged (assign)

    board.complete_task(id).await.unwrap();

    let status_event = rx.try_recv().unwrap();
    assert!(matches!(
        status_event,
        TaskBoardEvent::TaskStatusChanged { .. }
    ));

    let complete_event = rx.try_recv().unwrap();
    assert!(
        matches!(complete_event, TaskBoardEvent::TaskCompleted { task_id, agent } if task_id == id && agent == "worker")
    );
}

#[tokio::test]
async fn test_task_failed_event() {
    let board = TaskBoard::new();
    let mut rx = board.subscribe_events();

    let task = make_task("Fail event", TaskPriority::Medium);
    let id = task.id;
    board.add_task(task).await.unwrap();
    let _ = rx.try_recv(); // TaskAdded

    board.fail_task(id, "crashed".to_string()).await.unwrap();

    let event = rx.try_recv().unwrap();
    assert!(
        matches!(event, TaskBoardEvent::TaskFailed { task_id, reason } if task_id == id && reason == "crashed")
    );
}

#[tokio::test]
async fn test_dependency_added_event() {
    let board = TaskBoard::new();
    let mut rx = board.subscribe_events();

    let t1 = make_task("A", TaskPriority::High);
    let t2 = make_task("B", TaskPriority::Medium);
    let id1 = t1.id;
    let id2 = t2.id;

    board.add_task(t1).await.unwrap();
    board.add_task(t2).await.unwrap();
    let _ = rx.try_recv(); // TaskAdded
    let _ = rx.try_recv(); // TaskAdded

    board.add_dependency(id2, id1).await.unwrap();

    let event = rx.try_recv().unwrap();
    assert!(
        matches!(event, TaskBoardEvent::DependencyAdded { task_id, depends_on } if task_id == id2 && depends_on == id1)
    );
}

#[tokio::test]
async fn test_task_removed_event() {
    let board = TaskBoard::new();
    let mut rx = board.subscribe_events();

    let task = make_task("Remove event", TaskPriority::Low);
    let id = task.id;
    board.add_task(task).await.unwrap();
    let _ = rx.try_recv(); // TaskAdded

    board.remove_task(id).await.unwrap();

    let event = rx.try_recv().unwrap();
    assert!(matches!(event, TaskBoardEvent::TaskRemoved { task_id } if task_id == id));
}

// ── Edge cases ──

#[tokio::test]
async fn test_empty_board_summary() {
    let board = TaskBoard::new();
    let summary = board.summary().await;
    assert_eq!(summary.total_tasks, 0);
    assert_eq!(summary.pending_tasks, 0);
    assert_eq!(summary.in_progress_tasks, 0);
}

#[tokio::test]
async fn test_remove_task_cleans_up_assignment() {
    let board = TaskBoard::new();
    let task = make_task("Assigned", TaskPriority::High);
    let id = task.id;

    board.add_task(task).await.unwrap();
    board.assign_task(id, "agent".to_string()).await.unwrap();
    board.remove_task(id).await.unwrap();

    let agents = board.list_active_agents().await;
    assert!(agents.is_empty());
}

#[tokio::test]
async fn test_multiple_priorities_summary() {
    let board = TaskBoard::new();
    board
        .add_task(make_task("Critical", TaskPriority::Critical))
        .await
        .unwrap();
    board
        .add_task(make_task("High", TaskPriority::High))
        .await
        .unwrap();
    board
        .add_task(make_task("Medium", TaskPriority::Medium))
        .await
        .unwrap();
    board
        .add_task(make_task("Low", TaskPriority::Low))
        .await
        .unwrap();

    let summary = board.summary().await;
    assert_eq!(summary.total_tasks, 4);
    assert_eq!(*summary.by_priority.get("Critical").unwrap_or(&0), 1);
    assert_eq!(*summary.by_priority.get("High").unwrap_or(&0), 1);
    assert_eq!(*summary.by_priority.get("Medium").unwrap_or(&0), 1);
    assert_eq!(*summary.by_priority.get("Low").unwrap_or(&0), 1);
}

#[tokio::test]
async fn test_assign_complete_then_assign_new() {
    let board = TaskBoard::new();

    let t1 = make_task("First task", TaskPriority::High);
    let t2 = make_task("Second task", TaskPriority::Medium);
    let id1 = t1.id;
    let id2 = t2.id;

    board.add_task(t1).await.unwrap();
    board.add_task(t2).await.unwrap();

    board.assign_task(id1, "agent-1".to_string()).await.unwrap();
    board.complete_task(id1).await.unwrap();

    board.assign_task(id2, "agent-1".to_string()).await.unwrap();

    let agent_tasks = board.get_agent_tasks("agent-1").await;
    // get_agent_tasks returns all tasks ever assigned (including completed)
    assert_eq!(agent_tasks.len(), 2);
    let ids: Vec<_> = agent_tasks.iter().map(|t| t.id).collect();
    assert!(ids.contains(&id1));
    assert!(ids.contains(&id2));
}
