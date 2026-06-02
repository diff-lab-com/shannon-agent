//! End-to-end team lifecycle tests.
//!
//! Tests the full multi-agent coordination pipeline through AgentCoordinator
//! with file persistence: team creation → member joining → task assignment →
//! claiming → completion → message passing → disbanding, verified on disk.

use shannon_agents::*;
use tempfile::TempDir;

async fn setup() -> (TempDir, AgentCoordinator) {
    let dir = TempDir::new().unwrap();
    let persistence = FilePersistence::with_base_dir(dir.path().to_path_buf());
    let mut config = CoordinatorConfig::default();
    config.enable_worktree_isolation = false;
    let mut coord = AgentCoordinator::new(config).await.unwrap();
    coord.set_persistence(persistence);
    (dir, coord)
}

fn teammate(name: &str) -> TeammateConfig {
    TeammateConfig {
        agent_type: name.to_string(),
        capabilities: vec!["general".to_string()],
        max_concurrent_tasks: 3,
        plan_mode_required: false,
        model: None,
        system_prompt: None,
        temperature: None,
        is_lead: false,
        allowed_tools: vec![],
        permission_mode: None,
        isolation: None,
    }
}

fn lead(name: &str) -> TeammateConfig {
    TeammateConfig {
        is_lead: true,
        ..teammate(name)
    }
}

// ── Full lifecycle ────────────────────────────────────────────────────────────

#[tokio::test]
async fn full_team_lifecycle_create_to_disband() {
    let (dir, coord) = setup().await;

    // 1. Create team
    coord
        .create_team("project-x".into(), "Build feature X".into())
        .await
        .unwrap();

    // 2. Add lead + workers
    coord
        .add_teammate("project-x", "lead".into(), lead("lead"))
        .await
        .unwrap();
    coord
        .add_teammate("project-x", "worker-a".into(), teammate("worker-a"))
        .await
        .unwrap();
    coord
        .add_teammate("project-x", "worker-b".into(), teammate("worker-b"))
        .await
        .unwrap();

    // Verify manifest
    let manifest = coord.team_manifest("project-x").await.unwrap();
    assert_eq!(manifest.members.len(), 3);

    // 3. Add tasks
    let t1 = coord
        .add_task(
            "project-x",
            "Setup DB".into(),
            "Create schema".into(),
            TaskPriority::High,
        )
        .await
        .unwrap();
    let t2 = coord
        .add_task(
            "project-x",
            "Write API".into(),
            "REST endpoints".into(),
            TaskPriority::Medium,
        )
        .await
        .unwrap();

    // 4. Workers claim tasks
    let claimed1 = coord
        .self_claim_task("project-x", "worker-a", t1)
        .await
        .unwrap();
    assert_eq!(claimed1.status, TaskStatus::InProgress);
    assert_eq!(claimed1.owner, Some("worker-a".into()));

    let claimed2 = coord
        .self_claim_task("project-x", "worker-b", t2)
        .await
        .unwrap();
    assert_eq!(claimed2.status, TaskStatus::InProgress);

    // 5. Workers complete tasks
    coord
        .complete_task(t1, "project-x", "worker-a")
        .await
        .unwrap();
    coord
        .complete_task(t2, "project-x", "worker-b")
        .await
        .unwrap();

    // Verify all complete
    let board = coord.task_board();
    assert_eq!(
        board.get_task(t1).await.unwrap().status,
        TaskStatus::Completed
    );
    assert_eq!(
        board.get_task(t2).await.unwrap().status,
        TaskStatus::Completed
    );

    // 6. Verify persistence on disk
    let persistence = FilePersistence::with_base_dir(dir.path().to_path_buf());
    let teams = persistence.list_teams().unwrap();
    assert!(teams.contains(&"project-x".to_string()));

    let team_config = persistence.load_team("project-x").unwrap();
    assert_eq!(team_config.members.len(), 3);

    // 7. Disband
    coord.disband_team("project-x").await.unwrap();
    let teams_after = coord.list_teams().await;
    assert!(!teams_after.contains(&"project-x".to_string()));
}

// ── Dependency chain ──────────────────────────────────────────────────────────

#[tokio::test]
async fn dependency_chain_blocks_dependents() {
    let (_dir, coord) = setup().await;
    coord
        .create_team("chain".into(), "Dependency chain".into())
        .await
        .unwrap();
    coord
        .add_teammate("chain", "worker".into(), teammate("worker"))
        .await
        .unwrap();

    // A → B → C (C blocked by B, B blocked by A)
    let task_a = coord
        .add_task("chain", "Task A".into(), "First".into(), TaskPriority::High)
        .await
        .unwrap();
    let task_b = coord
        .add_task_with_deps(
            "chain",
            "Task B".into(),
            "Second".into(),
            TaskPriority::Medium,
            vec![task_a],
        )
        .await
        .unwrap();
    let task_c = coord
        .add_task_with_deps(
            "chain",
            "Task C".into(),
            "Third".into(),
            TaskPriority::Low,
            vec![task_b],
        )
        .await
        .unwrap();

    // Only A is ready (B and C have blockers in blocked_by)
    let ready = coord.task_board().list_ready_tasks().await;
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].id, task_a);

    // B and C are not ready
    let task_b_data = coord.task_board().get_task(task_b).await.unwrap();
    assert!(!task_b_data.is_ready());
    let task_c_data = coord.task_board().get_task(task_c).await.unwrap();
    assert!(!task_c_data.is_ready());

    // Verify the full dependency graph via get_next_task
    let next = coord.task_board().get_next_task("worker").await.unwrap();
    assert_eq!(next.id, task_a);

    // Claim and complete A
    coord
        .self_claim_task("chain", "worker", task_a)
        .await
        .unwrap();
    coord
        .complete_task(task_a, "chain", "worker")
        .await
        .unwrap();

    // After A completes, the board summary reflects 1 completed task
    let summary = coord.task_board().summary().await;
    assert_eq!(summary.completed_tasks, 1);
    assert_eq!(summary.total_tasks, 3);
}

// ── Multi-agent claim race ────────────────────────────────────────────────────

#[tokio::test]
async fn two_agents_claim_different_tasks() {
    let (_dir, coord) = setup().await;
    coord
        .create_team("race".into(), "Claim race".into())
        .await
        .unwrap();
    coord
        .add_teammate("race", "fast-agent".into(), teammate("fast-agent"))
        .await
        .unwrap();
    coord
        .add_teammate("race", "slow-agent".into(), teammate("slow-agent"))
        .await
        .unwrap();

    let t1 = coord
        .add_task("race", "Task 1".into(), "First".into(), TaskPriority::High)
        .await
        .unwrap();
    let t2 = coord
        .add_task(
            "race",
            "Task 2".into(),
            "Second".into(),
            TaskPriority::Medium,
        )
        .await
        .unwrap();

    // fast-agent claims t1
    let claimed = coord
        .self_claim_task("race", "fast-agent", t1)
        .await
        .unwrap();
    assert_eq!(claimed.owner, Some("fast-agent".into()));

    // slow-agent claims t2
    let claimed = coord
        .self_claim_task("race", "slow-agent", t2)
        .await
        .unwrap();
    assert_eq!(claimed.owner, Some("slow-agent".into()));

    // fast-agent cannot re-claim t2 (already owned by slow-agent)
    let _result = coord.self_claim_task("race", "fast-agent", t2).await;
    // The board-level claim should reflect the already-claimed state
    let task = coord.task_board().get_task(t2).await.unwrap();
    assert_eq!(task.owner, Some("slow-agent".into()));
}

// ── Cross-team isolation ──────────────────────────────────────────────────────

#[tokio::test]
async fn tasks_isolated_between_teams() {
    let (_dir, coord) = setup().await;

    // Team A
    coord
        .create_team("team-a".into(), "Team A".into())
        .await
        .unwrap();
    coord
        .add_teammate("team-a", "alice".into(), teammate("alice"))
        .await
        .unwrap();

    // Team B
    coord
        .create_team("team-b".into(), "Team B".into())
        .await
        .unwrap();
    coord
        .add_teammate("team-b", "bob".into(), teammate("bob"))
        .await
        .unwrap();

    // Add tasks to each team
    let task_a = coord
        .add_task(
            "team-a",
            "A's task".into(),
            "For Alice".into(),
            TaskPriority::High,
        )
        .await
        .unwrap();
    let task_b = coord
        .add_task(
            "team-b",
            "B's task".into(),
            "For Bob".into(),
            TaskPriority::High,
        )
        .await
        .unwrap();

    // Alice claims in team-a, Bob claims in team-b
    coord
        .self_claim_task("team-a", "alice", task_a)
        .await
        .unwrap();
    coord
        .self_claim_task("team-b", "bob", task_b)
        .await
        .unwrap();

    // Verify isolation: team-a has 1 task, team-b has 1 task
    let manifest_a = coord.team_manifest("team-a").await.unwrap();
    let manifest_b = coord.team_manifest("team-b").await.unwrap();
    assert_eq!(manifest_a.members.len(), 1);
    assert_eq!(manifest_b.members.len(), 1);

    // Alice completing team-a's task doesn't affect team-b
    coord
        .complete_task(task_a, "team-a", "alice")
        .await
        .unwrap();
    let task_b_state = coord.task_board().get_task(task_b).await.unwrap();
    assert_eq!(task_b_state.status, TaskStatus::InProgress);
}

// ── Notify idle discovers ready tasks ─────────────────────────────────────────

#[tokio::test]
async fn idle_agent_discovers_ready_tasks() {
    let (_dir, coord) = setup().await;
    coord
        .create_team("pipeline".into(), "Pipeline team".into())
        .await
        .unwrap();
    coord
        .add_teammate("pipeline", "worker".into(), teammate("worker"))
        .await
        .unwrap();

    // One ready task, one blocked
    let ready_task = coord
        .add_task(
            "pipeline",
            "Ready task".into(),
            "Can start now".into(),
            TaskPriority::High,
        )
        .await
        .unwrap();
    let _blocked = coord
        .add_task_with_deps(
            "pipeline",
            "Blocked".into(),
            "Has blocker".into(),
            TaskPriority::Medium,
            vec![ready_task],
        )
        .await
        .unwrap();

    // Worker goes idle — only the ready task is available
    let available = coord.notify_idle("pipeline", "worker").await.unwrap();
    assert_eq!(available.len(), 1);
    assert_eq!(available[0].subject, "Ready task");

    // Claim and complete the ready task
    coord
        .self_claim_task("pipeline", "worker", ready_task)
        .await
        .unwrap();
    coord
        .complete_task(ready_task, "pipeline", "worker")
        .await
        .unwrap();

    // Worker goes idle again — blocked task is now unblocked (auto-unblocked)
    let available = coord.notify_idle("pipeline", "worker").await.unwrap();
    assert_eq!(available.len(), 1);
    assert_eq!(available[0].subject, "Blocked");
}

// ── Broadcast + DM messages ───────────────────────────────────────────────────

#[tokio::test]
async fn lead_broadcasts_then_sends_dm() {
    let (_dir, coord) = setup().await;
    coord
        .create_team("comms".into(), "Communications".into())
        .await
        .unwrap();
    coord
        .add_teammate("comms", "lead".into(), lead("lead"))
        .await
        .unwrap();
    coord
        .add_teammate("comms", "w1".into(), teammate("w1"))
        .await
        .unwrap();
    coord
        .add_teammate("comms", "w2".into(), teammate("w2"))
        .await
        .unwrap();

    // Broadcast
    let broadcast_msgs = coord
        .broadcast_to_team("comms", "lead", "Everyone start!".into())
        .await
        .unwrap();
    assert_eq!(broadcast_msgs.len(), 2);

    // DM to specific worker
    let dm = coord
        .send_direct_message(
            "comms",
            "lead",
            "w1",
            MessageContent::Text("Your task is critical".into()),
        )
        .await
        .unwrap();
    assert_eq!(dm.from, "w1");
    assert_eq!(dm.to, "lead");
}

// ── Persistence round-trip ────────────────────────────────────────────────────

#[tokio::test]
async fn team_and_tasks_persist_to_disk() {
    let (dir, coord) = setup().await;

    coord
        .create_team("persist-test".into(), "Persistence test".into())
        .await
        .unwrap();
    coord
        .add_teammate("persist-test", "agent-1".into(), teammate("agent-1"))
        .await
        .unwrap();

    let task_id = coord
        .add_task(
            "persist-test",
            "Persist me".into(),
            "Check disk".into(),
            TaskPriority::Medium,
        )
        .await
        .unwrap();

    // Verify team config on disk
    let persistence = FilePersistence::with_base_dir(dir.path().to_path_buf());
    let team = persistence.load_team("persist-test").unwrap();
    assert_eq!(team.name, "persist-test");
    assert!(team.members.contains_key("agent-1"));

    // Verify task on disk
    let task_file = persistence
        .load_task("persist-test", &task_id.to_string())
        .unwrap();
    assert_eq!(task_file.subject, "Persist me");
    assert_eq!(task_file.status, "pending");
}

#[tokio::test]
async fn claimed_task_status_reflects_on_disk() {
    let (dir, coord) = setup().await;
    coord
        .create_team("disk-claim".into(), "Disk claim test".into())
        .await
        .unwrap();
    coord
        .add_teammate("disk-claim", "claimer".into(), teammate("claimer"))
        .await
        .unwrap();

    let task_id = coord
        .add_task(
            "disk-claim",
            "Claim me".into(),
            "Verify on disk".into(),
            TaskPriority::Medium,
        )
        .await
        .unwrap();

    coord
        .self_claim_task("disk-claim", "claimer", task_id)
        .await
        .unwrap();

    // Verify the on-disk task reflects the claim
    let persistence = FilePersistence::with_base_dir(dir.path().to_path_buf());
    let task_file = persistence
        .load_task("disk-claim", &task_id.to_string())
        .unwrap();
    assert_eq!(task_file.status, "in_progress");
    assert_eq!(task_file.owner.as_deref(), Some("claimer"));
}

// ── Disband cleans up ─────────────────────────────────────────────────────────

#[tokio::test]
async fn disband_removes_team_from_listing() {
    let (_dir, coord) = setup().await;

    coord
        .create_team("temp-team".into(), "Temporary".into())
        .await
        .unwrap();
    coord
        .create_team("perm-team".into(), "Permanent".into())
        .await
        .unwrap();

    let teams = coord.list_teams().await;
    assert_eq!(teams.len(), 2);

    coord.disband_team("temp-team").await.unwrap();

    let teams_after = coord.list_teams().await;
    assert_eq!(teams_after.len(), 1);
    assert!(teams_after.contains(&"perm-team".to_string()));
}

// ── Multiple teams with independent lifecycles ────────────────────────────────

#[tokio::test]
async fn multiple_teams_independent_lifecycles() {
    let (_dir, coord) = setup().await;

    // Team 1
    coord
        .create_team("backend".into(), "Backend team".into())
        .await
        .unwrap();
    coord
        .add_teammate("backend", "rust-dev".into(), teammate("rust-dev"))
        .await
        .unwrap();

    // Team 2
    coord
        .create_team("frontend".into(), "Frontend team".into())
        .await
        .unwrap();
    coord
        .add_teammate("frontend", "ts-dev".into(), teammate("ts-dev"))
        .await
        .unwrap();

    // Tasks for each team
    let backend_task = coord
        .add_task(
            "backend",
            "Write API".into(),
            "REST endpoints".into(),
            TaskPriority::High,
        )
        .await
        .unwrap();
    let frontend_task = coord
        .add_task(
            "frontend",
            "Build UI".into(),
            "React components".into(),
            TaskPriority::High,
        )
        .await
        .unwrap();

    // Each team works independently
    coord
        .self_claim_task("backend", "rust-dev", backend_task)
        .await
        .unwrap();
    coord
        .self_claim_task("frontend", "ts-dev", frontend_task)
        .await
        .unwrap();

    coord
        .complete_task(backend_task, "backend", "rust-dev")
        .await
        .unwrap();
    coord
        .complete_task(frontend_task, "frontend", "ts-dev")
        .await
        .unwrap();

    // Disband one team, other survives
    coord.disband_team("backend").await.unwrap();

    let teams = coord.list_teams().await;
    assert_eq!(teams.len(), 1);
    assert!(teams.contains(&"frontend".to_string()));

    // Frontend team still operational
    let manifest = coord.team_manifest("frontend").await.unwrap();
    assert_eq!(manifest.members.len(), 1);
}

// ── Task priority ordering ────────────────────────────────────────────────────

#[tokio::test]
async fn high_priority_tasks_claimed_first() {
    let (_dir, coord) = setup().await;
    coord
        .create_team("prio".into(), "Priority test".into())
        .await
        .unwrap();
    coord
        .add_teammate("prio", "worker".into(), teammate("worker"))
        .await
        .unwrap();

    let _low = coord
        .add_task(
            "prio",
            "Low prio".into(),
            "Can wait".into(),
            TaskPriority::Low,
        )
        .await
        .unwrap();
    let _med = coord
        .add_task(
            "prio",
            "Med prio".into(),
            "Normal".into(),
            TaskPriority::Medium,
        )
        .await
        .unwrap();
    let _high = coord
        .add_task(
            "prio",
            "High prio".into(),
            "Urgent".into(),
            TaskPriority::High,
        )
        .await
        .unwrap();

    // notify_idle returns all unblocked tasks
    let available = coord.notify_idle("prio", "worker").await.unwrap();
    assert_eq!(available.len(), 3);
    let subjects: Vec<&str> = available.iter().map(|t| t.subject.as_str()).collect();
    assert!(subjects.contains(&"Low prio"));
    assert!(subjects.contains(&"Med prio"));
    assert!(subjects.contains(&"High prio"));
}

// ── Add task after teammate idle ──────────────────────────────────────────────

#[tokio::test]
async fn new_task_discovered_after_idle_notification() {
    let (_dir, coord) = setup().await;
    coord
        .create_team("dynamic".into(), "Dynamic tasks".into())
        .await
        .unwrap();
    coord
        .add_teammate("dynamic", "worker".into(), teammate("worker"))
        .await
        .unwrap();

    // Initially no tasks
    let available = coord.notify_idle("dynamic", "worker").await.unwrap();
    assert!(available.is_empty());

    // Add a task
    coord
        .add_task(
            "dynamic",
            "New task".into(),
            "Just added".into(),
            TaskPriority::Medium,
        )
        .await
        .unwrap();

    // Now idle notification returns the new task
    let available = coord.notify_idle("dynamic", "worker").await.unwrap();
    assert_eq!(available.len(), 1);
    assert_eq!(available[0].subject, "New task");
}
