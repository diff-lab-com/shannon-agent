//! Team coordination integration tests.
//!
//! Tests cover:
//! - Team creation and config persistence
//! - Task creation with dependencies
//! - Self-claim workflow via FilePersistence
//! - Message passing via inbox delivery
//! - TaskBoard lifecycle
//! - Teammate and protocol message types

use shannon_agents::*;
use std::collections::HashMap;
use tempfile::TempDir;
use uuid::Uuid;

fn temp_persistence() -> (TempDir, FilePersistence) {
    let dir = TempDir::new().unwrap();
    let persistence = FilePersistence::with_base_dir(dir.path().to_path_buf());
    (dir, persistence)
}

fn now_ts() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn make_team_config(name: &str) -> TeamConfigFile {
    TeamConfigFile {
        name: name.to_string(),
        description: format!("Test team {name}"),
        members: HashMap::new(),
        created_at: now_ts(),
        assignment_index: 0,
    }
}

fn make_task(subject: &str) -> TaskFile {
    TaskFile {
        id: Uuid::new_v4().to_string(),
        subject: subject.to_string(),
        description: format!("Description for {subject}"),
        status: "pending".to_string(),
        priority: "normal".to_string(),
        owner: None,
        blocked_by: vec![],
        blocks: vec![],
        active_form: None,
        required_capabilities: vec![],
        metadata: serde_json::Value::Null,
        created_at: now_ts(),
        updated_at: now_ts(),
    }
}

fn make_inbox_message(from: &str, content: &str) -> InboxMessage {
    InboxMessage {
        id: Uuid::new_v4().to_string(),
        from: from.to_string(),
        content: content.to_string(),
        timestamp: now_ts(),
        read: false,
    }
}

// =========================================================================
// 1. Team Creation & Persistence
// =========================================================================

mod team_creation {
    use super::*;

    #[test]
    fn save_and_load_team() {
        let (_dir, persistence) = temp_persistence();
        let team_name = format!("team-{}", Uuid::new_v4().as_simple());
        let config = make_team_config(&team_name);

        persistence.save_team(&config).unwrap();
        let loaded = persistence.load_team(&team_name).unwrap();
        assert_eq!(loaded.name, team_name);
    }

    #[test]
    fn list_teams_empty() {
        let (_dir, persistence) = temp_persistence();
        let teams = persistence.list_teams().unwrap();
        assert!(teams.is_empty());
    }

    #[test]
    fn list_teams_after_creation() {
        let (_dir, persistence) = temp_persistence();
        for i in 0..3 {
            let name = format!("team-{i}-{}", Uuid::new_v4().as_simple());
            let config = make_team_config(&name);
            persistence.save_team(&config).unwrap();
        }
        let teams = persistence.list_teams().unwrap();
        assert_eq!(teams.len(), 3);
    }

    #[test]
    fn team_with_members() {
        let (_dir, persistence) = temp_persistence();
        let team_name = format!("team-{}", Uuid::new_v4().as_simple());
        let mut config = make_team_config(&team_name);
        config
            .members
            .insert("agent-1".to_string(), TeammateConfig::default());
        persistence.save_team(&config).unwrap();

        let loaded = persistence.load_team(&team_name).unwrap();
        assert_eq!(loaded.members.len(), 1);
        assert!(loaded.members.contains_key("agent-1"));
    }
}

// =========================================================================
// 2. Task Creation with Dependencies
// =========================================================================

mod task_creation {
    use super::*;

    #[test]
    fn save_and_load_task() {
        let (_dir, persistence) = temp_persistence();
        let team_name = format!("team-{}", Uuid::new_v4().as_simple());
        persistence
            .save_team(&make_team_config(&team_name))
            .unwrap();

        let task = make_task("Setup database");
        persistence.save_task(&team_name, &task).unwrap();

        let loaded = persistence.load_task(&team_name, &task.id).unwrap();
        assert_eq!(loaded.subject, "Setup database");
    }

    #[test]
    fn task_with_dependencies() {
        let (_dir, persistence) = temp_persistence();
        let team_name = format!("team-{}", Uuid::new_v4().as_simple());
        persistence
            .save_team(&make_team_config(&team_name))
            .unwrap();

        let task1 = make_task("Setup database");
        persistence.save_task(&team_name, &task1).unwrap();

        let mut task2 = make_task("Write API endpoints");
        task2.blocked_by = vec![task1.id.clone()];
        persistence.save_task(&team_name, &task2).unwrap();

        let loaded = persistence.load_task(&team_name, &task2.id).unwrap();
        assert_eq!(loaded.blocked_by.len(), 1);
        assert_eq!(loaded.blocked_by[0], task1.id);
    }

    #[test]
    fn find_claimable_prefers_unblocked() {
        let (_dir, persistence) = temp_persistence();
        let team_name = format!("team-{}", Uuid::new_v4().as_simple());
        persistence
            .save_team(&make_team_config(&team_name))
            .unwrap();

        // Blocked task
        let mut blocked = make_task("Blocked task");
        blocked.blocked_by = vec!["nonexistent-task-id".to_string()];
        persistence.save_task(&team_name, &blocked).unwrap();

        // Unblocked task
        let unblocked = make_task("Ready task");
        persistence.save_task(&team_name, &unblocked).unwrap();

        let found = persistence
            .find_claimable_task(&team_name)
            .unwrap()
            .unwrap();
        assert_eq!(found.subject, "Ready task");
    }
}

// =========================================================================
// 3. Self-Claim Workflow
// =========================================================================

mod self_claim {
    use super::*;

    #[test]
    fn claim_task_sets_owner() {
        let (_dir, persistence) = temp_persistence();
        let team_name = format!("team-{}", Uuid::new_v4().as_simple());
        persistence
            .save_team(&make_team_config(&team_name))
            .unwrap();

        let task = make_task("Do something");
        persistence.save_task(&team_name, &task).unwrap();

        let claimed = persistence
            .claim_task(&team_name, &task.id, "agent-1")
            .unwrap();
        assert_eq!(claimed.owner.as_deref(), Some("agent-1"));
        assert_eq!(claimed.status, "in_progress");
    }

    #[test]
    fn claimed_task_not_claimable() {
        let (_dir, persistence) = temp_persistence();
        let team_name = format!("team-{}", Uuid::new_v4().as_simple());
        persistence
            .save_team(&make_team_config(&team_name))
            .unwrap();

        let task = make_task("Only one can claim");
        persistence.save_task(&team_name, &task).unwrap();

        persistence
            .claim_task(&team_name, &task.id, "agent-1")
            .unwrap();

        // After claim, find_claimable should return None
        let found = persistence.find_claimable_task(&team_name).unwrap();
        assert!(found.is_none());
    }

    #[test]
    fn complete_claimed_task() {
        let (_dir, persistence) = temp_persistence();
        let team_name = format!("team-{}", Uuid::new_v4().as_simple());
        persistence
            .save_team(&make_team_config(&team_name))
            .unwrap();

        let task = make_task("Complete me");
        persistence.save_task(&team_name, &task).unwrap();

        persistence
            .claim_task(&team_name, &task.id, "agent-1")
            .unwrap();

        // Mark completed
        let mut completed = persistence.load_task(&team_name, &task.id).unwrap();
        completed.status = "completed".to_string();
        persistence.save_task(&team_name, &completed).unwrap();

        let loaded = persistence.load_task(&team_name, &task.id).unwrap();
        assert_eq!(loaded.status, "completed");

        // No tasks should be claimable
        let found = persistence.find_claimable_task(&team_name).unwrap();
        assert!(found.is_none());
    }
}

// =========================================================================
// 4. Message Passing via Inbox
// =========================================================================

mod message_passing {
    use super::*;

    #[test]
    fn deliver_and_read_message() {
        let (_dir, persistence) = temp_persistence();
        let team_name = format!("team-{}", Uuid::new_v4().as_simple());
        persistence
            .save_team(&make_team_config(&team_name))
            .unwrap();

        let msg = make_inbox_message("agent-1", "Task done, your turn");
        persistence
            .deliver_message(&team_name, "agent-2", &msg)
            .unwrap();

        let inbox = persistence.read_inbox(&team_name, "agent-2").unwrap();
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0].from, "agent-1");
        assert_eq!(inbox[0].content, "Task done, your turn");
    }

    #[test]
    fn read_inbox_is_consumed() {
        let (_dir, persistence) = temp_persistence();
        let team_name = format!("team-{}", Uuid::new_v4().as_simple());
        persistence
            .save_team(&make_team_config(&team_name))
            .unwrap();

        let msg = make_inbox_message("lead", "Start task 1");
        persistence
            .deliver_message(&team_name, "worker", &msg)
            .unwrap();

        let inbox = persistence.read_inbox(&team_name, "worker").unwrap();
        assert_eq!(inbox.len(), 1);

        let inbox2 = persistence.read_inbox(&team_name, "worker").unwrap();
        assert!(inbox2.is_empty());
    }

    #[test]
    fn peek_inbox_not_consumed() {
        let (_dir, persistence) = temp_persistence();
        let team_name = format!("team-{}", Uuid::new_v4().as_simple());
        persistence
            .save_team(&make_team_config(&team_name))
            .unwrap();

        let msg = make_inbox_message("lead", "Check this");
        persistence
            .deliver_message(&team_name, "worker", &msg)
            .unwrap();

        let inbox1 = persistence.peek_inbox(&team_name, "worker").unwrap();
        assert_eq!(inbox1.len(), 1);

        let inbox2 = persistence.peek_inbox(&team_name, "worker").unwrap();
        assert_eq!(inbox2.len(), 1);
    }

    #[test]
    fn multiple_messages_in_inbox() {
        let (_dir, persistence) = temp_persistence();
        let team_name = format!("team-{}", Uuid::new_v4().as_simple());
        persistence
            .save_team(&make_team_config(&team_name))
            .unwrap();

        for i in 0..3 {
            let msg = make_inbox_message("lead", &format!("Message {i}"));
            persistence
                .deliver_message(&team_name, "worker", &msg)
                .unwrap();
        }

        let inbox = persistence.read_inbox(&team_name, "worker").unwrap();
        assert_eq!(inbox.len(), 3);
    }
}

// =========================================================================
// 5. TaskBoard Lifecycle (async)
// =========================================================================

mod task_board {
    use super::*;

    #[tokio::test]
    async fn empty_board_summary() {
        let board = TaskBoard::new();
        let summary = board.summary().await;
        assert_eq!(summary.total_tasks, 0);
    }

    #[tokio::test]
    async fn add_and_assign_task() {
        let board = TaskBoard::new();
        let task = AgentTask::new(
            "Test task".to_string(),
            "A test".to_string(),
            TaskPriority::Medium,
        );

        board.add_task(task.clone()).await.unwrap();
        board
            .assign_task(task.id, "agent-1".to_string())
            .await
            .unwrap();

        let loaded = board.get_task(task.id).await.unwrap();
        assert_eq!(loaded.owner.as_deref(), Some("agent-1"));
        assert_eq!(loaded.status, TaskStatus::InProgress);
    }

    #[tokio::test]
    async fn complete_task() {
        let board = TaskBoard::new();
        let task = AgentTask::new(
            "Lifecycle task".to_string(),
            "Full lifecycle".to_string(),
            TaskPriority::Medium,
        );

        board.add_task(task.clone()).await.unwrap();
        board
            .assign_task(task.id, "agent-1".to_string())
            .await
            .unwrap();
        board.complete_task(task.id).await.unwrap();

        let summary = board.summary().await;
        assert_eq!(summary.total_tasks, 1);
        assert_eq!(summary.completed_tasks, 1);
    }

    #[tokio::test]
    async fn add_dependency_blocks_task() {
        let board = TaskBoard::new();
        let blocker = AgentTask::new(
            "Blocker".to_string(),
            "Must finish first".to_string(),
            TaskPriority::Medium,
        );
        let blocked = AgentTask::new(
            "Blocked".to_string(),
            "Waits for blocker".to_string(),
            TaskPriority::Medium,
        );

        board.add_task(blocker.clone()).await.unwrap();
        board.add_task(blocked.clone()).await.unwrap();

        // Before dependency, both are ready
        assert_eq!(board.list_ready_tasks().await.len(), 2);

        // Add dependency: blocked depends on blocker
        board.add_dependency(blocked.id, blocker.id).await.unwrap();

        // Now only blocker is ready
        let ready = board.list_ready_tasks().await;
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, blocker.id);

        // get_next_task returns the unblocked one
        let next = board.get_next_task("agent-1").await.unwrap();
        assert_eq!(next.id, blocker.id);
    }
}

// =========================================================================
// 6. Message & Protocol Types
// =========================================================================

mod message_types {
    use super::*;

    #[test]
    fn agent_message_text() {
        let msg = AgentMessage::new_text("alice".into(), "bob".into(), "hello".into());
        assert_eq!(msg.from, "alice");
        assert_eq!(msg.to, "bob");
    }

    #[test]
    fn protocol_shutdown_request() {
        let msg = ProtocolMessage::ShutdownRequest {
            reason: "All tasks done".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("ShutdownRequest"));
        assert!(json.contains("All tasks done"));
    }

    #[test]
    fn protocol_plan_approval() {
        let msg = ProtocolMessage::PlanApprovalRequest {
            request_id: Uuid::new_v4(),
            plan: "Step 1: Setup\nStep 2: Test".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("PlanApprovalRequest"));
    }

    #[test]
    fn protocol_serialization_roundtrip() {
        let msgs = vec![
            ProtocolMessage::ShutdownRequest {
                reason: "done".into(),
            },
            ProtocolMessage::StatusRequest,
            ProtocolMessage::TaskAssign {
                task_id: Uuid::new_v4(),
                description: "Do X".into(),
                priority: Some("high".into()),
            },
        ];

        for msg in &msgs {
            let json = serde_json::to_string(msg).unwrap();
            let deserialized: ProtocolMessage = serde_json::from_str(&json).unwrap();
            let json2 = serde_json::to_string(&deserialized).unwrap();
            assert_eq!(json, json2);
        }
    }
}
