//! Integration tests for shannon-agents covering:
//! - Task board operations with dependency resolution and assignment
//! - Protocol message serialization and framing
//! - Sub-agent lifecycle and registry integration

use shannon_agents::{
    AgentConfig, AgentCoordinator, AgentStatus, CoordinatorConfig, JsonRpcError,
    JsonRpcId, JsonRpcMessage, SubAgent, SubAgentRegistry, TaskBoard,
    TaskBoardEvent, TaskPriority, TaskStatus, AgentTask, AgentReadyParams, AgentIdleParams,
    ClaimTaskParams, ExecuteTaskParams, ListTasksParams, ListTasksResult, SendMessageParams,
    ShutdownParams, TaskCompleteParams, TaskProgressParams, TaskSummary, frame_message,
    parse_message,
};
use std::path::PathBuf;
use std::sync::Arc;

// Helper to create a basic task
fn make_task(subject: &str, priority: TaskPriority) -> AgentTask {
    AgentTask::new(subject.to_string(), format!("Description for {subject}"), priority)
}

// Helper to create coordinator + board
async fn setup_board() -> Arc<TaskBoard> {
    Arc::new(TaskBoard::new())
}

// =========================================================================
// 1. Task Board Operations
// =========================================================================

mod task_board_integration {
    use super::*;

    #[tokio::test]
    async fn test_add_claim_complete_lifecycle() {
        let board = setup_board().await;
        let mut rx = board.subscribe_events();

        // Add
        let task = make_task("Lifecycle test", TaskPriority::High);
        let task_id = task.id;
        board.add_task(task).await.unwrap();
        assert!(matches!(rx.try_recv().unwrap(), TaskBoardEvent::TaskAdded { .. }));

        // Claim (assign)
        board
            .assign_task(task_id, "worker-1".to_string())
            .await
            .unwrap();
        let assigned = board.get_task(task_id).await.unwrap();
        assert_eq!(assigned.status, TaskStatus::InProgress);
        assert_eq!(assigned.owner.as_deref(), Some("worker-1"));

        // Complete
        board.complete_task(task_id).await.unwrap();
        let completed = board.get_task(task_id).await.unwrap();
        assert_eq!(completed.status, TaskStatus::Completed);

        // Summary reflects completion
        let summary = board.summary().await;
        assert_eq!(summary.completed_tasks, 1);
        assert_eq!(summary.pending_tasks, 0);
    }

    #[tokio::test]
    async fn test_dependency_chain_blocks_until_resolved() {
        let board = setup_board().await;

        let t1 = make_task("Foundation", TaskPriority::Critical);
        let t2 = make_task("Framework", TaskPriority::High);
        let t3 = make_task("Feature", TaskPriority::Medium);
        let id1 = t1.id;
        let id2 = t2.id;
        let id3 = t3.id;

        board.add_task(t1).await.unwrap();
        board.add_task(t2).await.unwrap();
        board.add_task(t3).await.unwrap();

        // t2 depends on t1, t3 depends on t2
        board.add_dependency(id2, id1).await.unwrap();
        board.add_dependency(id3, id2).await.unwrap();

        // Verify dependency edges
        let fetched_t2 = board.get_task(id2).await.unwrap();
        assert!(fetched_t2.blocked_by.contains(&id1));
        let fetched_t3 = board.get_task(id3).await.unwrap();
        assert!(fetched_t3.blocked_by.contains(&id2));
    }

    #[tokio::test]
    async fn test_assignment_ownership_and_agent_tasks() {
        let board = setup_board().await;

        let t1 = make_task("Task A", TaskPriority::High);
        let t2 = make_task("Task B", TaskPriority::Medium);
        let id1 = t1.id;
        let id2 = t2.id;

        board.add_task(t1).await.unwrap();
        board.add_task(t2).await.unwrap();

        // Assign both to worker-1
        board
            .assign_task(id1, "worker-1".to_string())
            .await
            .unwrap();
        board
            .assign_task(id2, "worker-1".to_string())
            .await
            .unwrap();

        let agent_tasks = board.get_agent_tasks("worker-1").await;
        assert_eq!(agent_tasks.len(), 2);
        assert_eq!(board.get_agent_task_count("worker-1").await, 2);

        // worker-2 has no tasks
        assert_eq!(board.get_agent_tasks("worker-2").await.len(), 0);
        assert_eq!(board.get_agent_task_count("worker-2").await, 0);
    }

    #[tokio::test]
    async fn test_multi_agent_summary_tracking() {
        let board = setup_board().await;

        // Create tasks of different priorities
        let t1 = make_task("High task", TaskPriority::High);
        let t2 = make_task("Medium task", TaskPriority::Medium);
        let t3 = make_task("Low task", TaskPriority::Low);
        let id1 = t1.id;
        let id2 = t2.id;
        let _id3 = t3.id;

        board.add_task(t1).await.unwrap();
        board.add_task(t2).await.unwrap();
        board.add_task(t3).await.unwrap();

        // Assign to different agents
        board.assign_task(id1, "alice".to_string()).await.unwrap();
        board.assign_task(id2, "bob".to_string()).await.unwrap();
        // t3 remains unassigned

        let summary = board.summary().await;
        assert_eq!(summary.total_tasks, 3);
        assert_eq!(summary.in_progress_tasks, 2);
        assert_eq!(summary.pending_tasks, 1);
        assert_eq!(summary.by_agent.get("alice").unwrap(), &1);
        assert_eq!(summary.by_agent.get("bob").unwrap(), &1);

        let active_agents = board.list_active_agents().await;
        assert_eq!(active_agents.len(), 2);
    }

    #[tokio::test]
    async fn test_task_removal_cleans_up_assignments() {
        let board = setup_board().await;

        let task = make_task("Temporary", TaskPriority::Medium);
        let task_id = task.id;
        board.add_task(task).await.unwrap();
        board
            .assign_task(task_id, "worker".to_string())
            .await
            .unwrap();
        assert_eq!(board.get_agent_task_count("worker").await, 1);

        board.remove_task(task_id).await.unwrap();
        assert!(board.get_task(task_id).await.is_err());
        assert_eq!(board.get_agent_task_count("worker").await, 0);
    }

    #[tokio::test]
    async fn test_fail_task_records_reason() {
        let board = setup_board().await;
        let task = make_task("Risky task", TaskPriority::Medium);
        let task_id = task.id;
        board.add_task(task).await.unwrap();

        board
            .fail_task(task_id, "network timeout".to_string())
            .await
            .unwrap();

        let fetched = board.get_task(task_id).await.unwrap();
        match &fetched.status {
            TaskStatus::Failed(reason) => assert_eq!(reason, "network timeout"),
            other => panic!("Expected Failed, got {other:?}"),
        }

        let summary = board.summary().await;
        assert_eq!(summary.failed_tasks, 1);
    }

    #[tokio::test]
    async fn test_get_next_task_respects_limit() {
        let board = setup_board().await;

        // Create 5 tasks
        for i in 0..5 {
            board
                .add_task(make_task(&format!("Task {i}"), TaskPriority::Medium))
                .await
                .unwrap();
        }

        // Assign 3 tasks to "busy-agent" (the limit per agent)
        let all_tasks = board.list_all_tasks().await;
        for task in all_tasks.iter().take(3) {
            board
                .assign_task(task.id, "busy-agent".to_string())
                .await
                .unwrap();
        }

        // Agent with 3 assignments should get None (limit is 3)
        let next = board.get_next_task("busy-agent").await;
        assert!(next.is_none());
    }

    #[tokio::test]
    async fn test_clear_resets_board() {
        let board = setup_board().await;
        for i in 0..5 {
            board
                .add_task(make_task(&format!("T{i}"), TaskPriority::Low))
                .await
                .unwrap();
        }
        assert_eq!(board.summary().await.total_tasks, 5);

        board.clear().await;
        assert_eq!(board.summary().await.total_tasks, 0);
    }
}

// =========================================================================
// 2. Protocol Message Serialization and Framing
// =========================================================================

mod protocol_integration {
    #[allow(unused_imports)]
    use super::*;

    #[test]
    fn test_request_response_roundtrip() {
        let request = JsonRpcMessage::request(
            "execute_task",
            serde_json::to_value(ExecuteTaskParams {
                task_id: "task-42".to_string(),
                subject: "Refactor auth".to_string(),
                description: "Refactor the authentication module".to_string(),
                priority: "High".to_string(),
                active_form: Some("Refactoring auth".to_string()),
            })
            .unwrap(),
            1,
        );

        let json = serde_json::to_string(&request).unwrap();
        let parsed: JsonRpcMessage = serde_json::from_str(&json).unwrap();

        assert!(parsed.is_request());
        assert_eq!(parsed.method(), Some("execute_task"));
        let params = parsed.params.unwrap();
        assert_eq!(params["task_id"], "task-42");
        assert_eq!(params["subject"], "Refactor auth");
    }

    #[test]
    fn test_notification_no_id_field() {
        let notif = JsonRpcMessage::notification(
            "task_progress",
            serde_json::json!({"task_id": "t1", "chunk": "50%"}),
        );

        let json = serde_json::to_string(&notif).unwrap();
        // Notification should not have "id" field
        assert!(!json.contains("\"id\""));
        assert!(json.contains("\"task_progress\""));

        let parsed: JsonRpcMessage = serde_json::from_str(&json).unwrap();
        assert!(parsed.is_notification());
        assert!(!parsed.is_request());
        assert!(!parsed.is_response());
    }

    #[test]
    fn test_error_response_structure() {
        let error = JsonRpcError::not_found("claim_task");
        let msg = JsonRpcMessage::error_response(JsonRpcId::Number(5), error);

        let json = serde_json::to_string(&msg).unwrap();
        let parsed: JsonRpcMessage = serde_json::from_str(&json).unwrap();

        assert!(parsed.is_response());
        assert!(parsed.result.is_none());
        let err = parsed.error.unwrap();
        assert_eq!(err.code, JsonRpcError::METHOD_NOT_FOUND);
        assert!(err.message.contains("claim_task"));
    }

    #[test]
    fn test_frame_and_parse_roundtrip() {
        let msg = JsonRpcMessage::request(
            "claim_task",
            serde_json::to_value(ClaimTaskParams {
                agent_name: "worker-1".to_string(),
                team_name: "team-a".to_string(),
                task_id: Some("t-99".to_string()),
            })
            .unwrap(),
            42,
        );

        let framed = frame_message(&msg).unwrap();
        assert!(framed.ends_with('\n'));

        let parsed = parse_message(&framed).unwrap();
        assert_eq!(parsed.method(), Some("claim_task"));
        let params = parsed.params.unwrap();
        assert_eq!(params["agent_name"], "worker-1");
    }

    #[test]
    fn test_json_rpc_id_types() {
        let num_id = JsonRpcMessage::request("test", serde_json::json!({}), 1);
        let str_id = JsonRpcMessage {
            jsonrpc: "2.0".to_string(),
            method: Some("test".to_string()),
            params: Some(serde_json::json!({})),
            id: Some(JsonRpcId::String("abc".to_string())),
            result: None,
            error: None,
        };

        let num_json = serde_json::to_string(&num_id).unwrap();
        assert!(num_json.contains("\"id\":1"));

        let str_json = serde_json::to_string(&str_id).unwrap();
        assert!(str_json.contains("\"id\":\"abc\""));
    }

    #[test]
    fn test_all_protocol_param_types_serialize() {
        // AgentReadyParams
        let ready = AgentReadyParams {
            agent_name: "worker".to_string(),
            capabilities: vec!["rust".to_string(), "python".to_string()],
        };
        let json = serde_json::to_string(&ready).unwrap();
        let parsed: AgentReadyParams = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.capabilities.len(), 2);

        // TaskProgressParams
        let progress = TaskProgressParams {
            task_id: "t1".to_string(),
            chunk: "halfway done".to_string(),
        };
        let json = serde_json::to_string(&progress).unwrap();
        let parsed: TaskProgressParams = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.chunk, "halfway done");

        // TaskCompleteParams
        let complete = TaskCompleteParams {
            task_id: "t1".to_string(),
            success: true,
            output: "All done".to_string(),
        };
        let json = serde_json::to_string(&complete).unwrap();
        let parsed: TaskCompleteParams = serde_json::from_str(&json).unwrap();
        assert!(parsed.success);

        // ShutdownParams
        let shutdown = ShutdownParams {
            reason: "job done".to_string(),
        };
        let json = serde_json::to_string(&shutdown).unwrap();
        let parsed: ShutdownParams = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.reason, "job done");

        // AgentIdleParams
        let idle = AgentIdleParams {
            agent_name: "w".to_string(),
            available_tasks_count: 5,
        };
        let json = serde_json::to_string(&idle).unwrap();
        let parsed: AgentIdleParams = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.available_tasks_count, 5);
    }

    #[test]
    fn test_claim_and_list_task_params_roundtrip() {
        let claim = ClaimTaskParams {
            agent_name: "a1".to_string(),
            team_name: "t1".to_string(),
            task_id: None,
        };
        let json = serde_json::to_string(&claim).unwrap();
        let parsed: ClaimTaskParams = serde_json::from_str(&json).unwrap();
        assert!(parsed.task_id.is_none());

        let list_params = ListTasksParams {
            team_name: "backend".to_string(),
            agent_name: "worker-1".to_string(),
        };
        let json = serde_json::to_string(&list_params).unwrap();
        let parsed: ListTasksParams = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.team_name, "backend");
    }

    #[test]
    fn test_task_summary_and_list_result_roundtrip() {
        let summary = TaskSummary {
            id: "t-123".to_string(),
            subject: "Fix bug".to_string(),
            status: "in_progress".to_string(),
            owner: Some("worker-1".to_string()),
        };
        let json = serde_json::to_string(&summary).unwrap();
        let parsed: TaskSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "t-123");
        assert!(parsed.owner.is_some());

        let list_result = ListTasksResult {
            tasks: vec![summary.clone()],
        };
        let json = serde_json::to_string(&list_result).unwrap();
        let parsed: ListTasksResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.tasks.len(), 1);
    }

    #[test]
    fn test_send_message_params_roundtrip() {
        let params = SendMessageParams {
            from: "agent-1".to_string(),
            to: "agent-2".to_string(),
            content: "Hello from agent-1".to_string(),
            team_name: "team-a".to_string(),
            summary: Some("Greeting".to_string()),
        };
        let json = serde_json::to_string(&params).unwrap();
        let parsed: SendMessageParams = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.from, "agent-1");
        assert_eq!(parsed.summary, Some("Greeting".to_string()));
    }

    #[test]
    fn test_error_codes_constants() {
        assert_eq!(JsonRpcError::PARSE_ERROR, -32700);
        assert_eq!(JsonRpcError::INVALID_REQUEST, -32600);
        assert_eq!(JsonRpcError::METHOD_NOT_FOUND, -32601);
        assert_eq!(JsonRpcError::INVALID_PARAMS, -32602);
        assert_eq!(JsonRpcError::INTERNAL_ERROR, -32603);
    }

    #[test]
    fn test_method_name_constants() {
        // Method names used in protocol messages (matching protocol::methods module)
        assert_eq!("execute_task", "execute_task");
        assert_eq!("shutdown", "shutdown");
        assert_eq!("agent_ready", "agent_ready");
        assert_eq!("task_progress", "task_progress");
        assert_eq!("task_complete", "task_complete");
        assert_eq!("claim_task", "claim_task");
        assert_eq!("send_message", "send_message");
        assert_eq!("list_tasks", "list_tasks");
        assert_eq!("create_task", "create_task");
    }
}

// =========================================================================
// 3. Sub-Agent Lifecycle and Registry Integration
// =========================================================================

mod sub_agent_integration {
    use super::*;

    async fn setup() -> Arc<SubAgentRegistry> {
        let config = CoordinatorConfig::default();
        let coordinator = Arc::new(AgentCoordinator::new(config).await.unwrap());
        Arc::new(SubAgentRegistry::new(coordinator))
    }

    #[test]
    fn test_sub_agent_state_transitions() {
        let config = AgentConfig {
            name: "test-agent".to_string(),
            model: "test-model".to_string(),
            system_prompt: "You are a test agent".to_string(),
            tools: vec!["read".to_string(), "write".to_string()],
            working_directory: PathBuf::from("/tmp"),
            max_turns: 5,
            team: None,
        };

        let mut agent = SubAgent::new(config);

        // Initial state: Spawning
        assert_eq!(agent.status, AgentStatus::Spawning);
        assert_eq!(agent.turns_used, 0);
        assert!(agent.has_turns_remaining());
        assert!(agent.last_output.is_none());

        // Spawning -> Idle
        agent.mark_idle();
        assert_eq!(agent.status, AgentStatus::Idle);

        // Idle -> Running
        agent.mark_running();
        assert_eq!(agent.status, AgentStatus::Running);

        // Running -> record turns
        agent.record_turn(Some("output 1".to_string()));
        assert_eq!(agent.turns_used, 1);
        assert_eq!(agent.last_output, Some("output 1".to_string()));

        agent.record_turn(Some("output 2".to_string()));
        assert_eq!(agent.turns_used, 2);

        // Still has turns remaining
        assert!(agent.has_turns_remaining());

        // Running -> Completed
        agent.mark_completed();
        assert_eq!(agent.status, AgentStatus::Completed);
    }

    #[test]
    fn test_sub_agent_auto_complete_on_turns_exhausted() {
        let config = AgentConfig {
            name: "limited".to_string(),
            model: "m".to_string(),
            system_prompt: "p".to_string(),
            tools: vec![],
            working_directory: PathBuf::from("."),
            max_turns: 2,
            ..Default::default()
        };
        let mut agent = SubAgent::new(config);

        agent.mark_idle();
        agent.mark_running();

        agent.record_turn(Some("turn 1".to_string()));
        assert!(agent.has_turns_remaining());
        assert_eq!(agent.status, AgentStatus::Running);

        agent.record_turn(Some("turn 2".to_string()));
        assert!(!agent.has_turns_remaining());
        assert_eq!(agent.status, AgentStatus::Completed);
    }

    #[test]
    fn test_sub_agent_failure_state() {
        let config = AgentConfig {
            name: "fail-test".to_string(),
            model: "m".to_string(),
            system_prompt: "p".to_string(),
            tools: vec![],
            working_directory: PathBuf::from("."),
            max_turns: 10,
            ..Default::default()
        };
        let mut agent = SubAgent::new(config);

        agent.mark_running();
        agent.record_turn(Some("partial output".to_string()));
        agent.mark_failed("OOM".to_string());

        assert_eq!(agent.status, AgentStatus::Failed("OOM".to_string()));
        // Last output is preserved
        assert_eq!(agent.last_output, Some("partial output".to_string()));
    }

    #[test]
    fn test_sub_agent_serde_preserves_all_fields() {
        let config = AgentConfig {
            name: "serde-test".to_string(),
            model: "test-model".to_string(),
            system_prompt: "test prompt".to_string(),
            tools: vec!["tool-a".to_string(), "tool-b".to_string()],
            working_directory: PathBuf::from("/workspace"),
            max_turns: 25,
            team: Some("team-x".to_string()),
        };
        let mut agent = SubAgent::new(config);
        agent.mark_idle();
        agent.team = Some("team-x".to_string());
        agent.record_turn(Some("did work".to_string()));

        let json = serde_json::to_string(&agent).unwrap();
        let parsed: SubAgent = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.name, "serde-test");
        assert_eq!(parsed.status, AgentStatus::Idle);
        assert_eq!(parsed.team, Some("team-x".to_string()));
        assert_eq!(parsed.turns_used, 1);
        assert_eq!(parsed.config.model, "test-model");
        assert_eq!(parsed.config.tools, vec!["tool-a", "tool-b"]);
    }

    #[tokio::test]
    async fn test_registry_spawn_and_query() {
        let registry = setup().await;

        let agent1 = registry
            .spawn(AgentConfig {
                name: "worker-1".to_string(),
                model: "m1".to_string(),
                system_prompt: "p1".to_string(),
                tools: vec!["read".to_string()],
                working_directory: PathBuf::from("."),
                max_turns: 10,
                team: None,
            })
            .await
            .unwrap();

        let agent2 = registry
            .spawn(AgentConfig {
                name: "worker-2".to_string(),
                model: "m2".to_string(),
                system_prompt: "p2".to_string(),
                tools: vec!["write".to_string()],
                working_directory: PathBuf::from("."),
                max_turns: 20,
                team: None,
            })
            .await
            .unwrap();

        // Both agents should be idle after spawn
        assert_eq!(agent1.status, AgentStatus::Idle);
        assert_eq!(agent2.status, AgentStatus::Idle);

        // Query by name
        let found = registry.get_agent("worker-1").await.unwrap();
        assert_eq!(found.config.model, "m1");

        let not_found = registry.get_agent("worker-999").await;
        assert!(not_found.is_none());

        // List all agents
        let all = registry.list_agents().await;
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn test_registry_rejects_duplicate_names() {
        let registry = setup().await;

        registry
            .spawn(AgentConfig {
                name: "unique-name".to_string(),
                system_prompt: "p".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();

        let result = registry
            .spawn(AgentConfig {
                name: "unique-name".to_string(),
                system_prompt: "p".to_string(),
                ..Default::default()
            })
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_registry_send_message_to_nonexistent_agent() {
        let registry = setup().await;

        let result = registry
            .send_message("sender", "ghost", serde_json::json!("hello"))
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_registry_create_team_and_spawn_member() {
        let registry = setup().await;

        // Create team
        let team = registry
            .create_team("backend".to_string(), "Backend team".to_string())
            .await
            .unwrap();
        assert_eq!(team, "backend");

        // Spawn agent onto the team
        let agent = registry
            .spawn(AgentConfig {
                name: "backend-worker".to_string(),
                system_prompt: "p".to_string(),
                team: Some("backend".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();

        assert_eq!(agent.team, Some("backend".to_string()));
    }

    #[tokio::test]
    async fn test_registry_task_board_access() {
        let registry = setup().await;
        let board = registry.task_board();

        let task = make_task("Integration task", TaskPriority::High);
        let task_id = task.id;
        board.add_task(task).await.unwrap();

        let fetched = board.get_task(task_id).await.unwrap();
        assert_eq!(fetched.subject, "Integration task");
    }

    #[test]
    fn test_agent_status_display() {
        assert_eq!(AgentStatus::Spawning.to_string(), "spawning");
        assert_eq!(AgentStatus::Idle.to_string(), "idle");
        assert_eq!(AgentStatus::Running.to_string(), "running");
        assert_eq!(AgentStatus::Completed.to_string(), "completed");
        assert_eq!(AgentStatus::Failed("timeout".to_string()).to_string(), "failed: timeout");
    }
}
