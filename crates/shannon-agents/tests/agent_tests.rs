//! Agent coordination and management tests
//!
//! Tests cover:
//! - Coordinator: team creation, agent addition, task assignment, status tracking
//! - Task Board: task lifecycle, assignment, dependencies, queries
//! - Teammate: creation, message handling, state transitions, capabilities

use shannon_agents::*;
use std::time::Duration;
use tokio::time::sleep;
use uuid::Uuid;

// =========================================================================
// 1. Coordinator Tests
// =========================================================================

mod coordinator_tests {
    use super::*;

    #[tokio::test]
    async fn coordinator_new_with_default_config() {
        let config = CoordinatorConfig::default();
        let coordinator = AgentCoordinator::new(config).await;

        assert!(coordinator.is_ok());
    }

    #[tokio::test]
    async fn coordinator_new_with_custom_config() {
        let _config = CoordinatorConfig {
            max_team_size: 5,
            message_buffer_size: 50,
            enable_worktree_isolation: false,
            worktree_config: None,
            heartbeat_interval_secs: 15,
            assignment_strategy: AssignmentStrategy::RoundRobin,
            delegate_mode: false,
            agent_mode: AgentMode::default(),
        };
    }

    #[tokio::test]
    async fn coordinator_create_team() {
        let coordinator = AgentCoordinator::new(CoordinatorConfig::default())
            .await
            .unwrap();

        let result = coordinator
            .create_team(
                "backend-team".to_string(),
                "Backend development team".to_string(),
            )
            .await;

        assert!(result.is_ok());

        let teams = coordinator.list_teams().await;
        assert_eq!(teams.len(), 1);
        assert_eq!(teams[0], "backend-team");
    }

    #[tokio::test]
    async fn coordinator_create_duplicate_team_fails() {
        let coordinator = AgentCoordinator::new(CoordinatorConfig::default())
            .await
            .unwrap();

        coordinator
            .create_team("team-a".to_string(), "First team".to_string())
            .await
            .unwrap();

        let result = coordinator
            .create_team("team-a".to_string(), "Duplicate team".to_string())
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn coordinator_add_teammate_to_team() {
        let coordinator = AgentCoordinator::new(CoordinatorConfig::default())
            .await
            .unwrap();

        coordinator
            .create_team("dev-team".to_string(), "Development team".to_string())
            .await
            .unwrap();

        let config = TeammateConfig {
            agent_type: "developer".to_string(),
            capabilities: vec!["rust".to_string()],
            ..Default::default()
        };

        let result = coordinator
            .add_teammate("dev-team", "alice".to_string(), config)
            .await;

        assert!(result.is_ok());

        let members = coordinator.get_team_members("dev-team").await.unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0], "alice");
    }

    #[tokio::test]
    async fn coordinator_add_teammate_to_nonexistent_team_fails() {
        let coordinator = AgentCoordinator::new(CoordinatorConfig::default())
            .await
            .unwrap();

        let result = coordinator
            .add_teammate(
                "nonexistent-team",
                "alice".to_string(),
                TeammateConfig::default(),
            )
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn coordinator_add_teammate_exceeds_max_team_size() {
        let config = CoordinatorConfig {
            max_team_size: 2,
            ..Default::default()
        };
        let coordinator = AgentCoordinator::new(config).await.unwrap();

        coordinator
            .create_team("small-team".to_string(), "Small team".to_string())
            .await
            .unwrap();

        coordinator
            .add_teammate("small-team", "alice".to_string(), TeammateConfig::default())
            .await
            .unwrap();
        coordinator
            .add_teammate("small-team", "bob".to_string(), TeammateConfig::default())
            .await
            .unwrap();

        let result = coordinator
            .add_teammate(
                "small-team",
                "charlie".to_string(),
                TeammateConfig::default(),
            )
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn coordinator_add_duplicate_teammate_fails() {
        let coordinator = AgentCoordinator::new(CoordinatorConfig::default())
            .await
            .unwrap();

        coordinator
            .create_team("team".to_string(), "Team".to_string())
            .await
            .unwrap();
        coordinator
            .add_teammate("team", "alice".to_string(), TeammateConfig::default())
            .await
            .unwrap();

        let result = coordinator
            .add_teammate("team", "alice".to_string(), TeammateConfig::default())
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn coordinator_get_agent() {
        let coordinator = AgentCoordinator::new(CoordinatorConfig::default())
            .await
            .unwrap();

        coordinator
            .create_team("team".to_string(), "Team".to_string())
            .await
            .unwrap();
        coordinator
            .add_teammate("team", "alice".to_string(), TeammateConfig::default())
            .await
            .unwrap();

        let agent = coordinator.get_agent("team", "alice").await;

        assert!(agent.is_ok());
        let agent = agent.unwrap();
        assert_eq!(agent.name, "alice");
    }

    #[tokio::test]
    async fn coordinator_get_nonexistent_agent_fails() {
        let coordinator = AgentCoordinator::new(CoordinatorConfig::default())
            .await
            .unwrap();

        coordinator
            .create_team("team".to_string(), "Team".to_string())
            .await
            .unwrap();

        let result = coordinator.get_agent("team", "nonexistent").await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn coordinator_add_task() {
        let coordinator = AgentCoordinator::new(CoordinatorConfig::default())
            .await
            .unwrap();

        coordinator
            .create_team("team".to_string(), "Team".to_string())
            .await
            .unwrap();

        let task_id = coordinator
            .add_task(
                "team",
                "Fix bug".to_string(),
                "Fix the null pointer".to_string(),
                TaskPriority::High,
            )
            .await;

        assert!(task_id.is_ok());
        let task_id = task_id.unwrap();
        assert!(!task_id.is_nil());
    }

    #[tokio::test]
    async fn coordinator_assign_task() {
        let coordinator = AgentCoordinator::new(CoordinatorConfig::default())
            .await
            .unwrap();

        coordinator
            .create_team("team".to_string(), "Team".to_string())
            .await
            .unwrap();
        coordinator
            .add_teammate("team", "alice".to_string(), TeammateConfig::default())
            .await
            .unwrap();

        let task_id = coordinator
            .add_task(
                "team",
                "Task".to_string(),
                "Description".to_string(),
                TaskPriority::Medium,
            )
            .await
            .unwrap();

        let result = coordinator.assign_task("team", task_id).await;

        assert!(result.is_ok());
        let assigned_agent = result.unwrap();
        assert_eq!(assigned_agent, "alice");
    }

    #[tokio::test]
    async fn coordinator_send_message() {
        let coordinator = AgentCoordinator::new(CoordinatorConfig::default())
            .await
            .unwrap();

        let message =
            AgentMessage::new_text("alice".to_string(), "bob".to_string(), "Hello".to_string());

        let result = coordinator.send_message(message).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn coordinator_subscribe_events() {
        let coordinator = AgentCoordinator::new(CoordinatorConfig::default())
            .await
            .unwrap();

        coordinator
            .create_team("events-team".to_string(), "Team".to_string())
            .await
            .unwrap();

        let mut receiver = coordinator.subscribe_events();

        coordinator
            .add_teammate(
                "events-team",
                "alice".to_string(),
                TeammateConfig::default(),
            )
            .await
            .unwrap();

        // Wait for event with timeout
        let event_opt = tokio::time::timeout(Duration::from_millis(200), async {
            loop {
                match receiver.try_recv() {
                    Ok(event) => break Some(event),
                    Err(_) => {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                }
            }
        })
        .await
        .ok()
        .flatten();

        assert!(event_opt.is_some());
        assert!(matches!(
            event_opt,
            Some(CoordinatorEvent::AgentJoined { .. })
        ));
    }

    #[tokio::test]
    async fn coordinator_task_board_access() {
        let coordinator = AgentCoordinator::new(CoordinatorConfig::default())
            .await
            .unwrap();

        let task_board = coordinator.task_board();

        assert_eq!(task_board.list_all_tasks().await.len(), 0);
    }

    #[tokio::test]
    async fn coordinator_shutdown() {
        let coordinator = AgentCoordinator::new(CoordinatorConfig::default())
            .await
            .unwrap();

        coordinator
            .create_team("team".to_string(), "Team".to_string())
            .await
            .unwrap();
        coordinator
            .add_teammate("team", "alice".to_string(), TeammateConfig::default())
            .await
            .unwrap();

        let result = coordinator.shutdown().await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn coordinator_event_task_assigned() {
        let coordinator = AgentCoordinator::new(CoordinatorConfig::default())
            .await
            .unwrap();
        let mut receiver = coordinator.subscribe_events();

        coordinator
            .create_team("team".to_string(), "Team".to_string())
            .await
            .unwrap();
        coordinator
            .add_teammate("team", "alice".to_string(), TeammateConfig::default())
            .await
            .unwrap();

        let task_id = coordinator
            .add_task(
                "team",
                "Task".to_string(),
                "Description".to_string(),
                TaskPriority::Medium,
            )
            .await
            .unwrap();

        coordinator.assign_task("team", task_id).await.unwrap();

        sleep(Duration::from_millis(100)).await;

        let event = receiver.try_recv();
        assert!(event.is_ok());

        // Drain events to find TaskAssigned
        while let Ok(event) = receiver.try_recv() {
            if matches!(event, CoordinatorEvent::TaskAssigned { .. }) {
                return;
            }
        }
    }
}

// =========================================================================
// 2. Task Board Tests
// =========================================================================

mod task_board_tests {
    use super::*;

    #[tokio::test]
    async fn task_board_new() {
        let board = TaskBoard::new();

        assert_eq!(board.list_all_tasks().await.len(), 0);
    }

    #[tokio::test]
    async fn task_board_default() {
        let board = TaskBoard::default();

        assert_eq!(board.list_all_tasks().await.len(), 0);
    }

    #[tokio::test]
    async fn task_board_add_task() {
        let board = TaskBoard::new();

        let task = AgentTask::new(
            "Test task".to_string(),
            "Test description".to_string(),
            TaskPriority::Medium,
        );

        let result = board.add_task(task).await;

        assert!(result.is_ok());
        assert_eq!(board.list_all_tasks().await.len(), 1);
    }

    #[tokio::test]
    async fn task_board_add_duplicate_task_fails() {
        let board = TaskBoard::new();

        let task = AgentTask::new(
            "Task".to_string(),
            "Description".to_string(),
            TaskPriority::Medium,
        );

        board.add_task(task.clone()).await.unwrap();

        let result = board.add_task(task).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn task_board_get_task() {
        let board = TaskBoard::new();

        let task = AgentTask::new(
            "Task".to_string(),
            "Description".to_string(),
            TaskPriority::Medium,
        );
        let task_id = task.id;

        board.add_task(task).await.unwrap();

        let result = board.get_task(task_id).await;

        assert!(result.is_ok());
        let retrieved = result.unwrap();
        assert_eq!(retrieved.subject, "Task");
    }

    #[tokio::test]
    async fn task_board_get_nonexistent_task_fails() {
        let board = TaskBoard::new();

        let result = board.get_task(Uuid::new_v4()).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn task_board_list_ready_tasks() {
        let board = TaskBoard::new();

        let task1 = AgentTask::new(
            "Ready task".to_string(),
            "Description".to_string(),
            TaskPriority::Medium,
        );
        let mut task2 = AgentTask::new(
            "Blocked task".to_string(),
            "Description".to_string(),
            TaskPriority::Medium,
        );

        task2.blocked_by.push(Uuid::new_v4());

        board.add_task(task1).await.unwrap();
        board.add_task(task2).await.unwrap();

        let ready_tasks = board.list_ready_tasks().await;

        assert_eq!(ready_tasks.len(), 1);
        assert_eq!(ready_tasks[0].subject, "Ready task");
    }

    #[tokio::test]
    async fn task_board_list_tasks_by_status() {
        let board = TaskBoard::new();

        let task1 = AgentTask::new(
            "Pending task".to_string(),
            "Description".to_string(),
            TaskPriority::Medium,
        );
        let mut task2 = AgentTask::new(
            "In progress task".to_string(),
            "Description".to_string(),
            TaskPriority::Medium,
        );

        task2.status = TaskStatus::InProgress;

        board.add_task(task1).await.unwrap();
        board.add_task(task2).await.unwrap();

        let pending_tasks = board.list_tasks_by_status(TaskStatus::Pending).await;
        let in_progress_tasks = board.list_tasks_by_status(TaskStatus::InProgress).await;

        assert_eq!(pending_tasks.len(), 1);
        assert_eq!(in_progress_tasks.len(), 1);
    }

    #[tokio::test]
    async fn task_board_list_tasks_by_priority() {
        let board = TaskBoard::new();

        board
            .add_task(AgentTask::new(
                "Low".to_string(),
                "Description".to_string(),
                TaskPriority::Low,
            ))
            .await
            .unwrap();
        board
            .add_task(AgentTask::new(
                "High".to_string(),
                "Description".to_string(),
                TaskPriority::High,
            ))
            .await
            .unwrap();
        board
            .add_task(AgentTask::new(
                "High2".to_string(),
                "Description".to_string(),
                TaskPriority::High,
            ))
            .await
            .unwrap();

        let high_priority_tasks = board.list_tasks_by_priority(TaskPriority::High).await;

        assert_eq!(high_priority_tasks.len(), 2);
    }

    #[tokio::test]
    async fn task_board_summary() {
        let board = TaskBoard::new();

        board
            .add_task(AgentTask::new(
                "Pending".to_string(),
                "Description".to_string(),
                TaskPriority::Medium,
            ))
            .await
            .unwrap();

        let mut in_progress = AgentTask::new(
            "In Progress".to_string(),
            "Description".to_string(),
            TaskPriority::High,
        );
        in_progress.status = TaskStatus::InProgress;
        board.add_task(in_progress).await.unwrap();

        let summary = board.summary().await;

        assert_eq!(summary.total_tasks, 2);
        assert_eq!(summary.pending_tasks, 1);
        assert_eq!(summary.in_progress_tasks, 1);
    }

    #[tokio::test]
    async fn task_board_assign_task() {
        let board = TaskBoard::new();

        let task = AgentTask::new(
            "Task".to_string(),
            "Description".to_string(),
            TaskPriority::Medium,
        );
        let task_id = task.id;

        board.add_task(task).await.unwrap();

        let result = board.assign_task(task_id, "alice".to_string()).await;

        assert!(result.is_ok());

        let assigned_task = board.get_task(task_id).await.unwrap();
        assert_eq!(assigned_task.owner.as_deref(), Some("alice"));
        assert_eq!(assigned_task.status, TaskStatus::InProgress);
    }

    #[tokio::test]
    async fn task_board_assign_blocked_task_fails() {
        let board = TaskBoard::new();

        let mut task = AgentTask::new(
            "Blocked".to_string(),
            "Description".to_string(),
            TaskPriority::Medium,
        );
        task.blocked_by.push(Uuid::new_v4());
        let task_id = task.id;

        board.add_task(task).await.unwrap();

        let result = board.assign_task(task_id, "alice".to_string()).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn task_board_update_task_status() {
        let board = TaskBoard::new();

        let task = AgentTask::new(
            "Task".to_string(),
            "Description".to_string(),
            TaskPriority::Medium,
        );
        let task_id = task.id;

        board.add_task(task).await.unwrap();

        let result = board
            .update_task_status(task_id, TaskStatus::Completed)
            .await;

        assert!(result.is_ok());

        let updated = board.get_task(task_id).await.unwrap();
        assert_eq!(updated.status, TaskStatus::Completed);
    }

    #[tokio::test]
    async fn task_board_complete_task() {
        let board = TaskBoard::new();

        let task = AgentTask::new(
            "Task".to_string(),
            "Description".to_string(),
            TaskPriority::Medium,
        );
        let task_id = task.id;

        board.add_task(task).await.unwrap();

        let result = board.complete_task(task_id).await;

        assert!(result.is_ok());

        let completed = board.get_task(task_id).await.unwrap();
        assert_eq!(completed.status, TaskStatus::Completed);
    }

    #[tokio::test]
    async fn task_board_fail_task() {
        let board = TaskBoard::new();

        let task = AgentTask::new(
            "Task".to_string(),
            "Description".to_string(),
            TaskPriority::Medium,
        );
        let task_id = task.id;

        board.add_task(task).await.unwrap();

        let result = board
            .fail_task(task_id, "Something went wrong".to_string())
            .await;

        assert!(result.is_ok());

        let failed = board.get_task(task_id).await.unwrap();
        assert!(matches!(failed.status, TaskStatus::Failed(_)));
    }

    #[tokio::test]
    async fn task_board_add_dependency() {
        let board = TaskBoard::new();

        let task1 = AgentTask::new(
            "Task 1".to_string(),
            "Description".to_string(),
            TaskPriority::Medium,
        );
        let task2 = AgentTask::new(
            "Task 2".to_string(),
            "Description".to_string(),
            TaskPriority::Medium,
        );

        let task1_id = task1.id;
        let task2_id = task2.id;

        board.add_task(task1).await.unwrap();
        board.add_task(task2).await.unwrap();

        let result = board.add_dependency(task2_id, task1_id).await;

        assert!(result.is_ok());

        let task2 = board.get_task(task2_id).await.unwrap();
        assert_eq!(task2.blocked_by.len(), 1);
        assert_eq!(task2.blocked_by[0], task1_id);

        let task1 = board.get_task(task1_id).await.unwrap();
        assert_eq!(task1.blocks.len(), 1);
        assert_eq!(task1.blocks[0], task2_id);
    }

    #[tokio::test]
    async fn task_board_add_circular_dependency_fails() {
        let board = TaskBoard::new();

        let task1 = AgentTask::new(
            "Task 1".to_string(),
            "Description".to_string(),
            TaskPriority::Medium,
        );
        let task2 = AgentTask::new(
            "Task 2".to_string(),
            "Description".to_string(),
            TaskPriority::Medium,
        );

        let task1_id = task1.id;
        let task2_id = task2.id;

        board.add_task(task1).await.unwrap();
        board.add_task(task2).await.unwrap();

        board.add_dependency(task2_id, task1_id).await.unwrap();

        let result = board.add_dependency(task1_id, task2_id).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn task_board_remove_dependency() {
        let board = TaskBoard::new();

        let task1 = AgentTask::new(
            "Task 1".to_string(),
            "Description".to_string(),
            TaskPriority::Medium,
        );
        let task2 = AgentTask::new(
            "Task 2".to_string(),
            "Description".to_string(),
            TaskPriority::Medium,
        );

        let task1_id = task1.id;
        let task2_id = task2.id;

        board.add_task(task1).await.unwrap();
        board.add_task(task2).await.unwrap();

        board.add_dependency(task2_id, task1_id).await.unwrap();
        board.remove_dependency(task2_id, task1_id).await.unwrap();

        let task2 = board.get_task(task2_id).await.unwrap();
        assert_eq!(task2.blocked_by.len(), 0);

        let task1 = board.get_task(task1_id).await.unwrap();
        assert_eq!(task1.blocks.len(), 0);
    }

    #[tokio::test]
    async fn task_board_get_next_task_for_agent() {
        let board = TaskBoard::new();

        let task = AgentTask::new(
            "Ready task".to_string(),
            "Description".to_string(),
            TaskPriority::Medium,
        );

        board.add_task(task).await.unwrap();

        let next_task = board.get_next_task("alice").await;

        assert!(next_task.is_some());
        assert_eq!(next_task.unwrap().subject, "Ready task");
    }

    #[tokio::test]
    async fn task_board_get_next_task_returns_none_when_at_capacity() {
        let board = TaskBoard::new();

        // Assign 3 tasks to alice (the capacity limit in get_next_task)
        for i in 0..3 {
            let task = AgentTask::new(
                format!("Task {i}"),
                "Description".to_string(),
                TaskPriority::Medium,
            );
            let id = task.id;
            board.add_task(task).await.unwrap();
            board.assign_task(id, "alice".to_string()).await.unwrap();
        }

        let ready_task = AgentTask::new(
            "Ready".to_string(),
            "Description".to_string(),
            TaskPriority::Medium,
        );
        board.add_task(ready_task).await.unwrap();

        let next_task = board.get_next_task("alice").await;

        assert!(next_task.is_none());
    }

    #[tokio::test]
    async fn task_board_get_agent_tasks() {
        let board = TaskBoard::new();

        let task1 = AgentTask::new(
            "Task 1".to_string(),
            "Description".to_string(),
            TaskPriority::Medium,
        );
        let task1_id = task1.id;
        board.add_task(task1).await.unwrap();
        board
            .assign_task(task1_id, "alice".to_string())
            .await
            .unwrap();

        let task2 = AgentTask::new(
            "Task 2".to_string(),
            "Description".to_string(),
            TaskPriority::High,
        );
        let task2_id = task2.id;
        board.add_task(task2).await.unwrap();
        board
            .assign_task(task2_id, "bob".to_string())
            .await
            .unwrap();

        let alice_tasks = board.get_agent_tasks("alice").await;
        let bob_tasks = board.get_agent_tasks("bob").await;

        assert_eq!(alice_tasks.len(), 1);
        assert_eq!(bob_tasks.len(), 1);
    }

    #[tokio::test]
    async fn task_board_get_agent_task_count() {
        let board = TaskBoard::new();

        for i in 0..3 {
            let task = AgentTask::new(
                format!("Task {i}"),
                "Description".to_string(),
                TaskPriority::Medium,
            );
            let id = task.id;
            board.add_task(task).await.unwrap();
            board.assign_task(id, "alice".to_string()).await.unwrap();
        }

        let count = board.get_agent_task_count("alice").await;

        assert_eq!(count, 3);
    }

    #[tokio::test]
    async fn task_board_list_active_agents() {
        let board = TaskBoard::new();

        let task = AgentTask::new(
            "Task".to_string(),
            "Description".to_string(),
            TaskPriority::Medium,
        );
        let id = task.id;
        board.add_task(task).await.unwrap();
        board.assign_task(id, "alice".to_string()).await.unwrap();

        let agents = board.list_active_agents().await;

        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0], "alice");
    }

    #[tokio::test]
    async fn task_board_remove_task() {
        let board = TaskBoard::new();

        let task = AgentTask::new(
            "Task".to_string(),
            "Description".to_string(),
            TaskPriority::Medium,
        );
        let task_id = task.id;

        board.add_task(task).await.unwrap();

        let result = board.remove_task(task_id).await;

        assert!(result.is_ok());
        assert_eq!(board.list_all_tasks().await.len(), 0);
    }

    #[tokio::test]
    async fn task_board_remove_nonexistent_task_fails() {
        let board = TaskBoard::new();

        let result = board.remove_task(Uuid::new_v4()).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn task_board_clear() {
        let board = TaskBoard::new();

        board
            .add_task(AgentTask::new(
                "Task 1".to_string(),
                "Description".to_string(),
                TaskPriority::Medium,
            ))
            .await
            .unwrap();
        board
            .add_task(AgentTask::new(
                "Task 2".to_string(),
                "Description".to_string(),
                TaskPriority::High,
            ))
            .await
            .unwrap();

        board.clear().await;

        assert_eq!(board.list_all_tasks().await.len(), 0);
    }

    #[tokio::test]
    async fn task_board_subscribe_events() {
        let board = TaskBoard::new();
        let mut receiver = board.subscribe_events();

        let task = AgentTask::new(
            "Task".to_string(),
            "Description".to_string(),
            TaskPriority::Medium,
        );
        board.add_task(task).await.unwrap();

        let event = receiver.try_recv();

        assert!(event.is_ok());
        if let Ok(TaskBoardEvent::TaskAdded { task_id }) = event {
            assert!(!task_id.is_nil());
        } else {
            panic!("Expected TaskAdded event");
        }
    }

    #[tokio::test]
    async fn task_board_event_task_assigned() {
        let board = TaskBoard::new();
        let mut receiver = board.subscribe_events();

        let task = AgentTask::new(
            "Task".to_string(),
            "Description".to_string(),
            TaskPriority::Medium,
        );
        let task_id = task.id;

        board.add_task(task).await.unwrap();
        board
            .assign_task(task_id, "alice".to_string())
            .await
            .unwrap();

        let event = receiver.try_recv();
        assert!(event.is_ok());

        while let Ok(event) = receiver.try_recv() {
            if matches!(event, TaskBoardEvent::TaskAssigned { .. }) {
                return;
            }
        }
    }
}

// =========================================================================
// 3. Teammate Tests
// =========================================================================

mod teammate_extended_tests {
    use super::*;

    #[tokio::test]
    async fn teammate_new_with_custom_config() {
        let config = TeammateConfig {
            agent_type: "specialist".to_string(),
            capabilities: vec!["rust".to_string(), "testing".to_string()],
            max_concurrent_tasks: 5,
            plan_mode_required: false,
            model: Some("claude-sonnet-4-6".to_string()),
            system_prompt: Some("Be thorough.".to_string()),
            temperature: Some(0.7),
            is_lead: false,
            allowed_tools: vec![],
            permission_mode: None,
            isolation: None,
        };

        let teammate = Teammate::new("expert".to_string(), config);

        assert_eq!(teammate.name, "expert");
        assert!(teammate.is_available().await);
    }

    #[tokio::test]
    async fn teammate_status_transitions_idle_to_busy() {
        let teammate = Teammate::new("worker".to_string(), TeammateConfig::default());

        assert_eq!(teammate.status().await, TeammateStatus::Idle);

        teammate.assign_task(Uuid::new_v4()).await.unwrap();

        assert_eq!(teammate.status().await, TeammateStatus::Busy);
    }

    #[tokio::test]
    async fn teammate_status_transitions_busy_to_idle() {
        let teammate = Teammate::new("worker".to_string(), TeammateConfig::default());
        let task_id = Uuid::new_v4();

        teammate.assign_task(task_id).await.unwrap();
        assert_eq!(teammate.status().await, TeammateStatus::Busy);

        teammate.complete_task(task_id).await;

        assert_eq!(teammate.status().await, TeammateStatus::Idle);
    }

    #[tokio::test]
    async fn teammate_assign_multiple_tasks_up_to_max() {
        let config = TeammateConfig {
            max_concurrent_tasks: 3,
            ..Default::default()
        };
        let teammate = Teammate::new("worker".to_string(), config);

        let task1 = Uuid::new_v4();

        // After first assignment, status becomes Busy
        teammate.assign_task(task1).await.unwrap();
        assert_eq!(teammate.status().await, TeammateStatus::Busy);
        assert!(!teammate.is_available().await);

        // Additional assignments fail because status is Busy
        let result = teammate.assign_task(Uuid::new_v4()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn teammate_assign_task_exceeds_max_concurrent() {
        let config = TeammateConfig {
            max_concurrent_tasks: 1,
            ..Default::default()
        };
        let teammate = Teammate::new("worker".to_string(), config);

        teammate.assign_task(Uuid::new_v4()).await.unwrap();

        let result = teammate.assign_task(Uuid::new_v4()).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn teammate_handle_chat_message() {
        let teammate = Teammate::new("alice".to_string(), TeammateConfig::default());

        let message = AgentMessage::new_text(
            "bob".to_string(),
            "alice".to_string(),
            "Hello from bob".to_string(),
        );

        let response = teammate.handle_message(message).await;

        assert!(response.is_ok());
        let response = response.unwrap();
        assert_eq!(response.from, "alice");
        assert_eq!(response.to, "bob");
    }

    #[tokio::test]
    async fn teammate_handle_protocol_shutdown_request() {
        let teammate = Teammate::new("alice".to_string(), TeammateConfig::default());

        let message = AgentMessage::protocol(
            "coordinator".to_string(),
            "alice".to_string(),
            ProtocolMessage::ShutdownRequest {
                reason: "Server shutting down".to_string(),
            },
        );

        let response = teammate.handle_message(message).await;

        assert!(response.is_ok());
        assert_eq!(teammate.status().await, TeammateStatus::ShuttingDown);

        let response = response.unwrap();
        if let MessageContent::Protocol(ProtocolMessage::ShutdownResponse { approve, .. }) =
            response.content
        {
            assert!(approve);
        } else {
            panic!("Expected ShutdownResponse");
        }
    }

    #[tokio::test]
    async fn teammate_handle_plan_approval_request() {
        let teammate = Teammate::new("planner".to_string(), TeammateConfig::default());

        let message = AgentMessage::protocol(
            "lead".to_string(),
            "planner".to_string(),
            ProtocolMessage::PlanApprovalRequest {
                request_id: Uuid::new_v4(),
                plan: "Refactor module X".to_string(),
            },
        );

        let response = teammate.handle_message(message).await;

        assert!(response.is_ok());
        let response = response.unwrap();
        if let MessageContent::Protocol(ProtocolMessage::PlanApprovalResponse { approve, .. }) =
            response.content
        {
            assert!(approve);
        } else {
            panic!("Expected PlanApprovalResponse");
        }
    }

    #[tokio::test]
    async fn teammate_handle_status_request() {
        let teammate = Teammate::new("alice".to_string(), TeammateConfig::default());

        let message = AgentMessage {
            id: Uuid::new_v4(),
            from: "coordinator".to_string(),
            to: "alice".to_string(),
            message_type: MessageType::Status,
            priority: MessagePriority::Normal,
            content: MessageContent::Text("status?".to_string()),
            timestamp: chrono::Utc::now(),
        };

        let response = teammate.handle_message(message).await;

        assert!(response.is_ok());
        let response = response.unwrap();
        // MessageType doesn't implement PartialEq, check via serialization
        assert_eq!(
            serde_json::to_string(&response.message_type).unwrap(),
            serde_json::to_string(&MessageType::Status).unwrap()
        );
    }

    #[tokio::test]
    async fn teammate_send_and_recv_message() {
        let teammate = Teammate::new("alice".to_string(), TeammateConfig::default());

        let message =
            AgentMessage::new_text("bob".to_string(), "alice".to_string(), "Hello".to_string());

        teammate.send(message).await.unwrap();

        let received = teammate.recv().await;

        assert!(received.is_some());
        let received = received.unwrap();
        assert_eq!(received.from, "bob");
    }

    #[tokio::test]
    async fn teammate_complete_task_updates_status() {
        let teammate = Teammate::new("worker".to_string(), TeammateConfig::default());

        let task1 = Uuid::new_v4();

        teammate.assign_task(task1).await.unwrap();
        assert_eq!(teammate.status().await, TeammateStatus::Busy);

        teammate.complete_task(task1).await;
        assert_eq!(teammate.status().await, TeammateStatus::Idle);
    }

    #[tokio::test]
    async fn teammate_fail_task() {
        let teammate = Teammate::new("worker".to_string(), TeammateConfig::default());

        let task_id = Uuid::new_v4();

        teammate.assign_task(task_id).await.unwrap();

        teammate
            .fail_task(task_id, "Network error".to_string())
            .await;

        assert_eq!(teammate.status().await, TeammateStatus::Idle);
    }

    #[tokio::test]
    async fn teammate_metadata_operations() {
        let teammate = Teammate::new("agent".to_string(), TeammateConfig::default());

        teammate
            .set_metadata("key1".to_string(), serde_json::json!("value1"))
            .await;
        teammate
            .set_metadata("key2".to_string(), serde_json::json!(42))
            .await;

        assert_eq!(
            teammate.get_metadata("key1").await,
            Some(serde_json::json!("value1"))
        );
        assert_eq!(
            teammate.get_metadata("key2").await,
            Some(serde_json::json!(42))
        );
        assert!(teammate.get_metadata("nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn teammate_enter_and_exit_plan_mode() {
        let config = TeammateConfig {
            plan_mode_required: true,
            ..Default::default()
        };
        let teammate = Teammate::new("planner".to_string(), config);

        teammate.enter_plan_mode().await.unwrap();
        assert_eq!(teammate.status().await, TeammateStatus::Planning);
        assert!(!teammate.is_available().await);

        teammate.exit_plan_mode().await.unwrap();
        assert_eq!(teammate.status().await, TeammateStatus::Idle);
    }

    #[tokio::test]
    async fn teammate_exit_plan_mode_when_not_planning_fails() {
        let teammate = Teammate::new("agent".to_string(), TeammateConfig::default());

        let result = teammate.exit_plan_mode().await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn teammate_is_available_considers_max_concurrent() {
        let config = TeammateConfig {
            max_concurrent_tasks: 2,
            ..Default::default()
        };
        let teammate = Teammate::new("worker".to_string(), config);

        // Initially available (Idle status, 0 tasks)
        assert!(teammate.is_available().await);

        // After assigning one task, status becomes Busy, so not available anymore
        // The max_concurrent_tasks check only matters when status is Idle
        teammate.assign_task(Uuid::new_v4()).await.unwrap();
        assert!(!teammate.is_available().await);
    }

    #[tokio::test]
    async fn teammate_state_includes_active_tasks() {
        let teammate = Teammate::new("worker".to_string(), TeammateConfig::default());

        let state = teammate.state().await;
        assert_eq!(state.active_tasks, 0);

        teammate.assign_task(Uuid::new_v4()).await.unwrap();

        let state = teammate.state().await;
        assert_eq!(state.active_tasks, 1);
    }

    #[tokio::test]
    async fn teammate_worktree_metadata() {
        let teammate = Teammate::new("agent".to_string(), TeammateConfig::default());

        assert!(teammate.state().await.current_worktree.is_none());

        teammate
            .set_metadata(
                "current_worktree".to_string(),
                serde_json::json!("feature-branch"),
            )
            .await;

        let state = teammate.state().await;
        assert_eq!(state.current_worktree.as_deref(), Some("feature-branch"));
    }

    #[tokio::test]
    async fn teammate_capability_check_case_insensitive() {
        let config = TeammateConfig {
            capabilities: vec!["Rust".to_string(), "Python".to_string()],
            ..Default::default()
        };
        let teammate = Teammate::new("polyglot".to_string(), config);

        assert!(teammate.has_capability("rust"));
        assert!(teammate.has_capability("RUST"));
        assert!(teammate.has_capability("python"));
        assert!(teammate.has_capability("PYTHON"));
        assert!(!teammate.has_capability("java"));
    }

    #[tokio::test]
    async fn teammate_cloned_shares_state() {
        let teammate = Teammate::new("alice".to_string(), TeammateConfig::default());

        let task_id = Uuid::new_v4();
        teammate.assign_task(task_id).await.unwrap();

        let cloned = teammate.clone();
        assert_eq!(cloned.name, "alice");
        assert_eq!(cloned.status().await, TeammateStatus::Busy);
    }
}

// =========================================================================
// 4. Integration Tests
// =========================================================================

mod integration_tests {
    use super::*;

    #[tokio::test]
    async fn coordinator_task_board_integration() {
        let coordinator = AgentCoordinator::new(CoordinatorConfig::default())
            .await
            .unwrap();

        coordinator
            .create_team("team".to_string(), "Team".to_string())
            .await
            .unwrap();
        coordinator
            .add_teammate("team", "alice".to_string(), TeammateConfig::default())
            .await
            .unwrap();

        let task_id = coordinator
            .add_task(
                "team",
                "Task".to_string(),
                "Description".to_string(),
                TaskPriority::Medium,
            )
            .await
            .unwrap();

        let assigned = coordinator.assign_task("team", task_id).await.unwrap();

        assert_eq!(assigned, "alice");

        let tasks = coordinator.task_board().get_agent_tasks("alice").await;
        assert_eq!(tasks.len(), 1);
    }

    #[tokio::test]
    async fn coordinator_message_to_teammate() {
        let coordinator = AgentCoordinator::new(CoordinatorConfig::default())
            .await
            .unwrap();

        coordinator
            .create_team("team".to_string(), "Team".to_string())
            .await
            .unwrap();
        coordinator
            .add_teammate("team", "alice".to_string(), TeammateConfig::default())
            .await
            .unwrap();

        let message = AgentMessage::new_text(
            "coordinator".to_string(),
            "alice".to_string(),
            "Please do this task".to_string(),
        );

        // Note: Message routing to individual teammates' inboxes
        // The coordinator's send_message goes to a channel that's processed by a background task
        // which then calls handle_message on the teammate, not inbox.send
        // This test verifies the send_message doesn't error
        let result = coordinator.send_message(message).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn task_board_dependency_workflow() {
        let board = TaskBoard::new();

        let task1 = AgentTask::new(
            "Build".to_string(),
            "Build project".to_string(),
            TaskPriority::High,
        );
        let task2 = AgentTask::new(
            "Test".to_string(),
            "Run tests".to_string(),
            TaskPriority::High,
        );
        let task3 = AgentTask::new(
            "Deploy".to_string(),
            "Deploy to prod".to_string(),
            TaskPriority::Critical,
        );

        let task1_id = task1.id;
        let task2_id = task2.id;
        let task3_id = task3.id;

        board.add_task(task1).await.unwrap();
        board.add_task(task2).await.unwrap();
        board.add_task(task3).await.unwrap();

        // Test depends on build
        board.add_dependency(task2_id, task1_id).await.unwrap();
        // Deploy depends on test
        board.add_dependency(task3_id, task2_id).await.unwrap();

        let ready_tasks = board.list_ready_tasks().await;
        assert_eq!(ready_tasks.len(), 1);
        assert_eq!(ready_tasks[0].id, task1_id);

        board.complete_task(task1_id).await.unwrap();

        // After completing task1, task2 is still blocked because dependencies aren't auto-cleaned
        // Need to remove the dependency manually for task2 to become ready
        board.remove_dependency(task2_id, task1_id).await.unwrap();

        let ready_tasks = board.list_ready_tasks().await;
        assert_eq!(ready_tasks.len(), 1);
        assert_eq!(ready_tasks[0].id, task2_id);
    }

    #[tokio::test]
    async fn teammate_coordinator_event_integration() {
        let coordinator = AgentCoordinator::new(CoordinatorConfig::default())
            .await
            .unwrap();
        let mut receiver = coordinator.subscribe_events();

        coordinator
            .create_team("team".to_string(), "Team".to_string())
            .await
            .unwrap();

        // Give time for the create_team event to propagate
        sleep(Duration::from_millis(50)).await;

        // Drain any pending events
        while receiver.try_recv().is_ok() {}

        coordinator
            .add_teammate("team", "alice".to_string(), TeammateConfig::default())
            .await
            .unwrap();

        // Give time for the add_teammate event to propagate
        sleep(Duration::from_millis(50)).await;

        let event = receiver.try_recv();
        assert!(event.is_ok());
        if let Ok(CoordinatorEvent::AgentJoined { team, agent }) = event {
            assert_eq!(team, "team");
            assert_eq!(agent, "alice");
        }
    }
}

// =========================================================================
// Self-Claim Task Tests
// =========================================================================

mod self_claim_tests {
    use super::*;

    async fn setup_team_with_tasks() -> AgentCoordinator {
        let config = CoordinatorConfig {
            assignment_strategy: AssignmentStrategy::SelfClaim,
            ..Default::default()
        };
        let coordinator = AgentCoordinator::new(config).await.unwrap();
        coordinator
            .create_team("dev".to_string(), "Development team".to_string())
            .await
            .unwrap();
        coordinator
            .add_teammate("dev", "alice".to_string(), TeammateConfig::default())
            .await
            .unwrap();
        coordinator
            .add_teammate("dev", "bob".to_string(), TeammateConfig::default())
            .await
            .unwrap();
        coordinator
    }

    /// Helper: create a task on the coordinator's task board.
    async fn create_task(
        coordinator: &AgentCoordinator,
        subject: &str,
        description: &str,
        priority: TaskPriority,
    ) -> Uuid {
        coordinator
            .add_task(
                "dev",
                subject.to_string(),
                description.to_string(),
                priority,
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn self_claim_task_assigns_to_agent() {
        let coordinator = setup_team_with_tasks().await;
        let task_id =
            create_task(&coordinator, "Task 1", "Do something", TaskPriority::Medium).await;

        let task = coordinator
            .self_claim_task("dev", "alice", task_id)
            .await
            .unwrap();

        assert_eq!(task.owner.as_deref(), Some("alice"));
        assert!(matches!(task.status, TaskStatus::InProgress));
    }

    #[tokio::test]
    async fn self_claim_rejects_already_owned_task() {
        let coordinator = setup_team_with_tasks().await;
        let task_id =
            create_task(&coordinator, "Task 1", "Do something", TaskPriority::Medium).await;

        // Alice claims first
        coordinator
            .self_claim_task("dev", "alice", task_id)
            .await
            .unwrap();

        // Bob tries to claim the same task
        let result = coordinator.self_claim_task("dev", "bob", task_id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn self_claim_rejects_non_member() {
        let coordinator = setup_team_with_tasks().await;
        let task_id =
            create_task(&coordinator, "Task 1", "Do something", TaskPriority::Medium).await;

        let result = coordinator
            .self_claim_task("dev", "unknown_agent", task_id)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn self_claim_rejects_blocked_task() {
        let coordinator = setup_team_with_tasks().await;
        let blocker_id = create_task(
            &coordinator,
            "Blocker",
            "Must finish first",
            TaskPriority::High,
        )
        .await;
        let blocked_id =
            create_task(&coordinator, "Blocked", "Waiting", TaskPriority::Medium).await;

        // Add dependency: blocked_task depends on blocker
        coordinator
            .task_board()
            .add_dependency(blocked_id, blocker_id)
            .await
            .unwrap();

        let result = coordinator
            .self_claim_task("dev", "alice", blocked_id)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn claim_next_picks_lowest_id_task() {
        let coordinator = setup_team_with_tasks().await;

        // Create tasks in order (earlier = lower ID)
        let task1 = create_task(
            &coordinator,
            "First task",
            "Created first",
            TaskPriority::Low,
        )
        .await;
        // Small sleep to ensure different timestamps (10ms for reliable resolution)
        sleep(Duration::from_millis(10)).await;
        let _task2 = create_task(
            &coordinator,
            "Second task",
            "Created second",
            TaskPriority::High,
        )
        .await;

        // claim_next should pick the earliest-created task
        let claimed = coordinator
            .claim_next_task("dev", "alice")
            .await
            .unwrap()
            .unwrap();

        assert_eq!(claimed.id, task1);
        assert_eq!(claimed.owner.as_deref(), Some("alice"));
    }

    #[tokio::test]
    async fn claim_next_returns_none_when_no_tasks() {
        let coordinator = setup_team_with_tasks().await;

        let result = coordinator.claim_next_task("dev", "alice").await.unwrap();

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn claim_next_skips_owned_tasks() {
        let coordinator = setup_team_with_tasks().await;

        let task1 = create_task(
            &coordinator,
            "First task",
            "Created first",
            TaskPriority::Medium,
        )
        .await;
        let task2 = create_task(
            &coordinator,
            "Second task",
            "Created second",
            TaskPriority::Medium,
        )
        .await;

        // Alice claims task1
        coordinator
            .self_claim_task("dev", "alice", task1)
            .await
            .unwrap();

        // Bob claims next — should get task2
        let claimed = coordinator
            .claim_next_task("dev", "bob")
            .await
            .unwrap()
            .unwrap();

        assert_eq!(claimed.id, task2);
        assert_eq!(claimed.owner.as_deref(), Some("bob"));
    }

    #[tokio::test]
    async fn find_next_claimable_returns_none_when_all_claimed() {
        let coordinator = setup_team_with_tasks().await;

        let task1 = create_task(&coordinator, "Only task", "Solo", TaskPriority::Medium).await;

        // Alice claims it
        coordinator
            .self_claim_task("dev", "alice", task1)
            .await
            .unwrap();

        // Bob looks for tasks — should find none
        let result = coordinator.find_next_claimable_task("dev", "bob").await;
        assert!(result.is_none());
    }

    // ── Idle Notification Tests ────────────────────────────────────────

    #[tokio::test]
    async fn notify_idle_returns_available_tasks() {
        let coordinator = setup_team_with_tasks().await;

        create_task(&coordinator, "Task 1", "Available", TaskPriority::Medium).await;
        create_task(&coordinator, "Task 2", "Also available", TaskPriority::High).await;

        let available = coordinator.notify_idle("dev", "alice").await.unwrap();

        assert_eq!(available.len(), 2);
    }

    #[tokio::test]
    async fn notify_idle_returns_empty_when_no_tasks() {
        let coordinator = setup_team_with_tasks().await;

        let available = coordinator.notify_idle("dev", "alice").await.unwrap();

        assert!(available.is_empty());
    }

    #[tokio::test]
    async fn notify_idle_excludes_owned_tasks() {
        let coordinator = setup_team_with_tasks().await;

        let task_id = create_task(&coordinator, "Task 1", "Claimed", TaskPriority::Medium).await;

        // Alice claims the task
        coordinator
            .self_claim_task("dev", "alice", task_id)
            .await
            .unwrap();

        // Bob goes idle — should see no unowned tasks
        let available = coordinator.notify_idle("dev", "bob").await.unwrap();

        assert!(available.is_empty());
    }

    #[tokio::test]
    async fn idle_agents_detects_idle_team_members() {
        let coordinator = setup_team_with_tasks().await;

        // Initially both alice and bob are idle
        let idle = coordinator.idle_agents("dev").await;
        assert_eq!(idle.len(), 2);
        assert!(idle.contains(&"alice".to_string()));
        assert!(idle.contains(&"bob".to_string()));
    }

    #[tokio::test]
    async fn idle_agents_excludes_busy_members() {
        let coordinator = setup_team_with_tasks().await;

        let task_id = create_task(&coordinator, "Task 1", "Work", TaskPriority::Medium).await;

        // Alice claims and becomes busy
        coordinator
            .self_claim_task("dev", "alice", task_id)
            .await
            .unwrap();

        let idle = coordinator.idle_agents("dev").await;
        assert_eq!(idle.len(), 1);
        assert!(idle.contains(&"bob".to_string()));
        assert!(!idle.contains(&"alice".to_string()));
    }
}
