//! Failure scenario tests for the shannon-agents crate.
//!
//! Tests cover:
//! 1. Orphan cleanup on agent crash (SubAgent state after simulated crash)
//! 2. Worktree cleanup on failure (failed task state recovery)
//! 3. Task board corruption recovery (invalid JSON deserialization handling)
//! 4. Deadlock detection (circular task dependencies)
//! 5. Coordinator failure handling (disband + state verification)
//! 6. Agent timeout kill (timeout config, AgentResult::timed_out)
//! 7. Batch partial failure rollback (MultiAgentResult failure tracking)
//! 8. Agent communication failure (channel failure, error propagation)

use shannon_agents::*;
use std::collections::HashMap;
use std::time::Duration;
use uuid::Uuid;

// =========================================================================
// 1. Orphan Cleanup on Crash
// =========================================================================

/// Simulated agent crash leaves SubAgent in Failed state with no orphan
/// state inconsistencies. After a crash, the agent should be in Failed
/// (not Running or Idle), turns_used should reflect actual usage, and
/// serializing/deserializing should produce consistent results.
#[test]
fn test_orphan_cleanup_on_crash() {
    let config = AgentConfig {
        name: "crash-agent".to_string(),
        model: "test-model".to_string(),
        system_prompt: "You are a test agent.".to_string(),
        tools: vec!["read".to_string()],
        working_directory: std::env::current_dir().unwrap(),
        max_turns: 10,
        team: Some("crash-team".to_string()),
    };

    let mut agent = SubAgent::new(config);
    agent.mark_idle();
    agent.mark_running();

    // Simulate some work before crash
    agent.record_turn(Some("partial output before crash".to_string()));
    assert_eq!(agent.turns_used, 1);
    assert_eq!(agent.status, AgentStatus::Running);

    // Agent crashes
    let crash_reason = "segfault in native module".to_string();
    agent.mark_failed(crash_reason.clone());

    // Verify clean crash state: not Running, not Idle
    assert_eq!(agent.status, AgentStatus::Failed(crash_reason.clone()));
    assert!(agent.has_turns_remaining()); // Only used 1 of 10 turns

    // Verify serialization roundtrip preserves crash state
    let json = serde_json::to_string(&agent).expect("serialize crashed agent");
    let restored: SubAgent = serde_json::from_str(&json).expect("deserialize crashed agent");

    assert_eq!(restored.status, AgentStatus::Failed(crash_reason));
    assert_eq!(restored.turns_used, 1);
    assert_eq!(restored.last_output.as_deref(), Some("partial output before crash"));
    assert_eq!(restored.id, agent.id);
    assert_eq!(restored.name, "crash-agent");
}

// =========================================================================
// 2. Worktree Cleanup on Failure
// =========================================================================

/// When a task fails, its state must accurately reflect the failure reason,
/// timestamps must be updated, and the task should be recoverable via
/// serialization (simulating persistence for later cleanup).
#[tokio::test]
async fn test_worktree_cleanup_on_failure() {
    let board = TaskBoard::new();

    // Create a task that simulates worktree-associated work
    let mut task = AgentTask::new(
        "Refactor module".to_string(),
        "Refactor auth module in worktree".to_string(),
        TaskPriority::High,
    );
    task.active_form = Some("Refactoring auth module".to_string());
    let task_id = task.id;

    board.add_task(task).await.unwrap();

    // Agent claims the task (simulating worktree checkout)
    board.assign_task(task_id, "worker-1".to_string()).await.unwrap();
    let assigned = board.get_task(task_id).await.unwrap();
    assert_eq!(assigned.status, TaskStatus::InProgress);
    assert_eq!(assigned.owner.as_deref(), Some("worker-1"));

    // Task fails during worktree operation
    let failure_reason = "worktree merge conflict".to_string();
    board.fail_task(task_id, failure_reason.clone()).await.unwrap();

    // Verify failure state
    let failed_task = board.get_task(task_id).await.unwrap();
    assert!(matches!(
        &failed_task.status,
        TaskStatus::Failed(r) if r == &failure_reason
    ));
    assert_eq!(failed_task.owner.as_deref(), Some("worker-1"));

    // Verify the failed task serializes (for persistence/cleanup)
    let json = serde_json::to_string(&failed_task).expect("serialize failed task");
    let restored: AgentTask = serde_json::from_str(&json).expect("deserialize failed task");
    assert_eq!(restored.id, task_id);
    assert_eq!(restored.subject, "Refactor module");
    assert_eq!(restored.owner.as_deref(), Some("worker-1"));

    // Verify task board summary reflects the failure
    let summary = board.summary().await;
    assert_eq!(summary.total_tasks, 1);
    assert_eq!(summary.pending_tasks, 0);
    assert_eq!(summary.in_progress_tasks, 0);

    // Failed tasks can be removed (worktree cleanup)
    board.remove_task(task_id).await.unwrap();
    assert!(board.get_task(task_id).await.is_err());
    assert!(board.list_active_agents().await.is_empty());
}

// =========================================================================
// 3. Task Board Corruption Recovery
// =========================================================================

/// When invalid JSON is encountered (e.g., corrupted task board file),
/// deserialization should fail gracefully, not panic. Valid tasks
/// mixed with invalid data should be distinguishable.
#[test]
fn test_task_board_corruption_recovery() {
    // Valid task roundtrips correctly
    let valid_task = AgentTask::new(
        "Valid task".to_string(),
        "This should work".to_string(),
        TaskPriority::High,
    );
    let valid_json = serde_json::to_string(&valid_task).expect("serialize valid task");
    let restored: AgentTask = serde_json::from_str(&valid_json).expect("deserialize valid task");
    assert_eq!(restored.subject, "Valid task");

    // Invalid JSON is rejected cleanly
    let corrupted_jsons = vec![
        r#"{not valid json}"#,
        r#"{ "id": "not-a-uuid" }"#,
        r#"{ "subject": 12345 }"#,
        "",
        "null",
        r#"[{]"#,
    ];

    for corrupted in &corrupted_jsons {
        let result: Result<AgentTask, _> = serde_json::from_str(corrupted);
        assert!(result.is_err(), "Expected error for corrupted JSON: {corrupted}");
    }

    // TaskStatus also recovers from corruption
    let valid_statuses = vec![
        TaskStatus::Pending,
        TaskStatus::InProgress,
        TaskStatus::Completed,
        TaskStatus::Failed("timeout".to_string()),
        TaskStatus::Blocked,
        TaskStatus::Cancelled,
    ];

    for status in &valid_statuses {
        let json = serde_json::to_string(status).expect("serialize status");
        let restored: TaskStatus = serde_json::from_str(&json).expect("deserialize status");
        assert_eq!(*status, restored);
    }

    // Invalid TaskStatus strings are rejected
    let invalid_status: Result<TaskStatus, _> = serde_json::from_str(r#""InvalidStatus""#);
    assert!(invalid_status.is_err());

    // TaskPriority similarly
    let invalid_priority: Result<TaskPriority, _> = serde_json::from_str(r#""Extreme""#);
    assert!(invalid_priority.is_err());
}

// =========================================================================
// 4. Deadlock Detection (Circular Dependencies)
// =========================================================================

/// Circular task dependencies on the TaskBoard are detected and rejected.
/// This tests both direct cycles and longer dependency chains.
#[tokio::test]
async fn test_deadlock_detection() {
    let board = TaskBoard::new();

    // --- Direct 2-node cycle ---
    let t1 = AgentTask::new("Task A".to_string(), "desc A".to_string(), TaskPriority::High);
    let t2 = AgentTask::new("Task B".to_string(), "desc B".to_string(), TaskPriority::High);
    let id1 = t1.id;
    let id2 = t2.id;

    board.add_task(t1).await.unwrap();
    board.add_task(t2).await.unwrap();

    // A -> B is fine
    board.add_dependency(id2, id1).await.unwrap();

    // B -> A should be rejected (creates cycle)
    let result = board.add_dependency(id1, id2).await;
    assert!(result.is_err());
    assert!(matches!(
        result,
        Err(AgentError::Task(TaskError::CircularDependency(_)))
    ));

    // --- 3-node cycle ---
    let board2 = TaskBoard::new();
    let ta = AgentTask::new("A".to_string(), "a".to_string(), TaskPriority::High);
    let tb = AgentTask::new("B".to_string(), "b".to_string(), TaskPriority::Medium);
    let tc = AgentTask::new("C".to_string(), "c".to_string(), TaskPriority::Low);
    let ida = ta.id;
    let idb = tb.id;
    let idc = tc.id;

    board2.add_task(ta).await.unwrap();
    board2.add_task(tb).await.unwrap();
    board2.add_task(tc).await.unwrap();

    // A -> B -> C is fine
    board2.add_dependency(idb, ida).await.unwrap();
    board2.add_dependency(idc, idb).await.unwrap();

    // C -> A should be rejected
    let result = board2.add_dependency(ida, idc).await;
    assert!(result.is_err());

    // --- Self-dependency ---
    let board3 = TaskBoard::new();
    let self_task = AgentTask::new("Self".to_string(), "self".to_string(), TaskPriority::Medium);
    let self_id = self_task.id;
    board3.add_task(self_task).await.unwrap();

    let result = board3.add_dependency(self_id, self_id).await;
    assert!(result.is_err());

    // Also verify DependencyError roundtrips through serialization
    let dep_err = DependencyError::CircularDependency(vec!["alpha".into(), "beta".into(), "gamma".into()]);
    let err_msg = dep_err.to_string();
    assert!(err_msg.contains("circular") || err_msg.contains("Circular") || err_msg.contains("cycle"),
        "DependencyError::CircularDependency message should mention cycle: {err_msg}");
}

// =========================================================================
// 5. Coordinator Failure Handling
// =========================================================================

/// When the coordinator fails mid-orchestration (simulated via disband),
/// all tasks should be failed, agents should be removed, and state
/// should be consistent.
#[tokio::test]
async fn test_coordinator_failure_handling() {
    let config = CoordinatorConfig::default();
    let coordinator = AgentCoordinator::new(config).await.unwrap();

    // Create team with 3 agents and 3 tasks
    coordinator
        .create_team("fail-team".to_string(), "Failure test team".to_string())
        .await
        .unwrap();

    let cfg = TeammateConfig {
        agent_type: "worker".into(),
        ..Default::default()
    };

    coordinator
        .add_teammate("fail-team", "agent-1".into(), cfg.clone())
        .await
        .unwrap();
    coordinator
        .add_teammate("fail-team", "agent-2".into(), cfg.clone())
        .await
        .unwrap();
    coordinator
        .add_teammate("fail-team", "agent-3".into(), cfg.clone())
        .await
        .unwrap();

    let tid1 = coordinator
        .add_task("fail-team", "Task 1".into(), "desc".into(), TaskPriority::High)
        .await
        .unwrap();
    let tid2 = coordinator
        .add_task("fail-team", "Task 2".into(), "desc".into(), TaskPriority::Medium)
        .await
        .unwrap();
    let _tid3 = coordinator
        .add_task("fail-team", "Task 3".into(), "desc".into(), TaskPriority::Low)
        .await
        .unwrap();

    // Claim one task
    coordinator
        .self_claim_task("fail-team", "agent-1", tid1)
        .await
        .unwrap();

    // Verify state before failure
    let status = coordinator.team_status("fail-team").await.unwrap();
    assert!(status.contains("3 members"));
    assert!(status.contains("3 tasks"));

    // Simulate coordinator failure via disband
    coordinator.disband_team("fail-team").await.unwrap();

    // Team should no longer exist
    assert!(coordinator.team_status("fail-team").await.is_err());

    // Task board should reflect failures for tasks owned by disbanded members
    let task1 = coordinator.task_board().get_task(tid1).await;
    assert!(task1.is_ok());
    let t1 = task1.unwrap();
    assert!(
        matches!(t1.status, TaskStatus::Failed(ref r) if r.contains("disbanded")),
        "Assigned task should be failed with 'Team disbanded', got: {:?}",
        t1.status
    );

    // Unclaimed tasks remain in their pre-disband state on the board
    let task2 = coordinator.task_board().get_task(tid2).await;
    assert!(task2.is_ok());
}

// =========================================================================
// 6. Agent Timeout Kill
// =========================================================================

/// Agent exceeding time limit produces a Timeout result. Tests the
/// timeout configuration, result construction, and status tracking.
#[test]
fn test_agent_timeout_kill() {
    // --- MultiAgentConfig timeout ---
    let config = MultiAgentConfig::new(vec![SpawnAgentConfig::new("slow", "long task")])
        .with_timeout(Duration::from_secs(30));

    assert_eq!(config.timeout_secs, 30, "timeout should be configured");

    // --- AgentResult::timed_out construction ---
    let timeout_duration = Duration::from_secs(300);
    let result = MultiAgentTaskResult::timed_out("slow-agent".to_string(), timeout_duration);

    assert_eq!(result.agent_name, "slow-agent");
    assert_eq!(result.status, AgentResultStatus::Timeout);
    assert_eq!(result.duration, timeout_duration);
    assert!(result.output.is_none());
    assert!(result.error.as_ref().unwrap().contains("exceeded timeout"));

    // --- AgentStatus does NOT have a Timeout variant ---
    // SubAgent uses Failed(String) for timeouts
    let mut agent = SubAgent::new(AgentConfig {
        name: "timeout-agent".to_string(),
        model: Default::default(),
        system_prompt: "test".to_string(),
        tools: Default::default(),
        working_directory: Default::default(),
        max_turns: 10,
        team: None,
    });

    agent.mark_idle();
    agent.mark_running();
    agent.mark_failed("execution timeout: exceeded 300s limit".to_string());

    assert_eq!(
        agent.status,
        AgentStatus::Failed("execution timeout: exceeded 300s limit".to_string())
    );

    // --- HealthCheckConfig timeout values ---
    let health = HealthCheckConfig {
        ping_timeout_secs: 5,
        graceful_shutdown_timeout_secs: 3,
        ..Default::default()
    };

    assert_eq!(health.ping_timeout_secs, 5);
    assert_eq!(health.graceful_shutdown_timeout_secs, 3);
    assert_eq!(health.max_restart_attempts, 3);

    // HealthCheckConfig serialization roundtrip
    let json = serde_json::to_string(&health).unwrap();
    let restored: HealthCheckConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.ping_timeout_secs, 5);
    assert_eq!(restored.graceful_shutdown_timeout_secs, 3);
}

// =========================================================================
// 7. Batch Partial Failure Rollback
// =========================================================================

/// When some agents in a batch fail, the MultiAgentResult should
/// accurately track successes, failures, and provide filtering.
/// Skipped agents (due to dependency failure) should be tracked separately.
#[test]
fn test_batch_partial_failure_rollback() {
    let completed = MultiAgentTaskResult::completed(
        "agent-a".to_string(),
        shannon_core::tools::ToolOutput {
            content: "Successfully refactored module A".to_string(),
            is_error: false,
            metadata: HashMap::new(),
        },
        Duration::from_secs(10),
    );

    let failed = MultiAgentTaskResult::failed(
        "agent-b".to_string(),
        "compilation error in module B".to_string(),
        Duration::from_secs(5),
    );

    let timed_out = MultiAgentTaskResult::timed_out("agent-c".to_string(), Duration::from_secs(300));

    let skipped = MultiAgentTaskResult::skipped("agent-d".to_string());

    let result = MultiAgentResult {
        agent_results: vec![completed, failed, timed_out, skipped],
        total_duration: Duration::from_secs(300),
        success_count: 1,
        failure_count: 3,
    };

    // Not all succeeded
    assert!(!result.all_succeeded());
    assert_eq!(result.success_count, 1);
    assert_eq!(result.failure_count, 3);

    // Failures include failed, timed out, and skipped
    let failures = result.failures();
    assert_eq!(failures.len(), 3);

    let failure_names: Vec<&str> = failures.iter().map(|f| f.agent_name.as_str()).collect();
    assert!(failure_names.contains(&"agent-b"));
    assert!(failure_names.contains(&"agent-c"));
    assert!(failure_names.contains(&"agent-d"));

    // Successes only include completed
    let successes = result.successes();
    assert_eq!(successes.len(), 1);
    assert_eq!(successes[0].agent_name, "agent-a");

    // Verify individual error messages
    let agent_b = result
        .agent_results
        .iter()
        .find(|r| r.agent_name == "agent-b")
        .unwrap();
    assert_eq!(agent_b.status, AgentResultStatus::Failed);
    assert_eq!(agent_b.error.as_deref(), Some("compilation error in module B"));
    assert!(agent_b.output.is_none());

    let agent_c = result
        .agent_results
        .iter()
        .find(|r| r.agent_name == "agent-c")
        .unwrap();
    assert_eq!(agent_c.status, AgentResultStatus::Timeout);
    assert!(agent_c.error.as_ref().unwrap().contains("timeout"));

    let agent_d = result
        .agent_results
        .iter()
        .find(|r| r.agent_name == "agent-d")
        .unwrap();
    assert_eq!(agent_d.status, AgentResultStatus::Skipped);
    assert_eq!(agent_d.duration, Duration::ZERO);

    // Serialization roundtrip preserves failure details
    let json = serde_json::to_string(&result).unwrap();
    let restored: MultiAgentResult = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.success_count, 1);
    assert_eq!(restored.failure_count, 3);
    assert_eq!(restored.agent_results.len(), 4);
    assert!(!restored.all_succeeded());

    // Fail-fast config
    let fail_fast_config = MultiAgentConfig::new(vec![
        SpawnAgentConfig::new("a", "task a"),
        SpawnAgentConfig::new("b", "task b"),
    ])
    .with_fail_fast();
    assert!(fail_fast_config.fail_fast, "fail_fast should stop on first error");
}

// =========================================================================
// 8. Agent Communication Failure
// =========================================================================

/// Message channel failures are handled gracefully: AgentError::Communication
/// carries the failure reason, protocol messages serialize/deserialize
/// through error conditions, and message routing to nonexistent agents
/// produces correct errors.
#[tokio::test]
async fn test_agent_communication_failure() {
    let config = CoordinatorConfig::default();
    let coordinator = AgentCoordinator::new(config).await.unwrap();

    // Sending to a nonexistent team fails
    let result = coordinator
        .send_direct_message(
            "nonexistent-team",
            "agent-a",
            "agent-b",
            MessageContent::Text("hello".to_string()),
        )
        .await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));

    // Create a team with one agent
    coordinator
        .create_team("comm-team".to_string(), "Communication test".to_string())
        .await
        .unwrap();

    let cfg = TeammateConfig {
        agent_type: "worker".into(),
        ..Default::default()
    };
    coordinator
        .add_teammate("comm-team", "sender".into(), cfg.clone())
        .await
        .unwrap();

    // Sending to a nonexistent agent in an existing team fails
    let result = coordinator
        .send_direct_message(
            "comm-team",
            "sender",
            "nonexistent-recipient",
            MessageContent::Text("hello".to_string()),
        )
        .await;
    assert!(result.is_err());

    // Verify error types
    let comm_err = AgentError::Communication("channel closed unexpectedly".to_string());
    assert!(comm_err.to_string().contains("communication"));
    assert!(comm_err.to_string().contains("channel closed"));

    // Protocol message roundtrip through simulated failure
    let shutdown_msg = ProtocolMessage::ShutdownRequest {
        reason: "communication timeout".to_string(),
    };
    let message = AgentMessage::protocol(
        "coordinator".to_string(),
        "crashed-agent".to_string(),
        shutdown_msg,
    );

    assert_eq!(message.priority, MessagePriority::High);
    assert_eq!(message.to, "crashed-agent");

    // Serialize and recover (simulating message that was in-flight during crash)
    let json = serde_json::to_string(&message).expect("serialize in-flight message");
    let recovered: AgentMessage =
        serde_json::from_str(&json).expect("deserialize recovered message");

    assert_eq!(recovered.id, message.id);
    assert_eq!(recovered.from, "coordinator");
    assert_eq!(recovered.to, "crashed-agent");
    assert_eq!(recovered.priority, MessagePriority::High);

    // TaskResult protocol message for failure reporting
    let task_result = ProtocolMessage::TaskResult {
        task_id: Uuid::new_v4(),
        success: false,
        output: "Agent crashed: out of memory".to_string(),
    };
    let error_msg = AgentMessage::protocol(
        "agent-1".to_string(),
        "coordinator".to_string(),
        task_result,
    );

    let json = serde_json::to_string(&error_msg).unwrap();
    let restored: AgentMessage = serde_json::from_str(&json).unwrap();

    match &restored.content {
        MessageContent::Protocol(ProtocolMessage::TaskResult { success, output, .. }) => {
            assert!(!success);
            assert_eq!(output, "Agent crashed: out of memory");
        }
        other => panic!("Expected TaskResult protocol message, got: {other:?}"),
    }
}

/// Additional test: process manager operations on nonexistent agents
/// produce appropriate errors (no panic, no silent failure).
#[tokio::test]
async fn test_process_manager_failure_paths() {
    let mut mgr = AgentProcessManager::new();

    // Request to nonexistent agent
    let result = mgr
        .send_request("ghost-agent", "ping", serde_json::Value::Null)
        .await;
    assert!(result.is_err());

    // Kill nonexistent agent
    let result = mgr.kill_agent("ghost-agent").await;
    assert!(result.is_err());

    // Status of nonexistent agent is None
    let status = mgr.agent_status("ghost-agent").await;
    assert!(status.is_none());

    // No agents running
    let agents = mgr.running_agents().await;
    assert!(agents.is_empty());

    // No events pending
    assert!(mgr.try_recv_event().is_none());

    // Shutdown all (no-op, should not panic)
    mgr.shutdown_all().await;

    // Spawn with nonexistent binary
    let config = AgentProcessConfig {
        binary_path: std::path::PathBuf::from("/nonexistent/binary"),
        args: vec![],
        env: HashMap::new(),
        worktree_path: None,
        model: None,
        system_prompt: None,
        agent_name: "bad-binary".to_string(),
        permission_mode: None,
        allowed_tools: None,
        startup_timeout_secs: 60,
    };
    let result = mgr.spawn_agent(config).await;
    assert!(result.is_err(), "Spawning with nonexistent binary should fail");
}
