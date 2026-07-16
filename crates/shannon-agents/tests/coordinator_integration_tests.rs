//! Coordinator-level integration tests for Agent Teams.
//!
//! Tests the full AgentCoordinator API: team creation, task lifecycle,
//! message passing (DM + broadcast), self-claim workflow, and team manifest.

use shannon_agents::*;
use tempfile::TempDir;

async fn temp_coordinator() -> (TempDir, AgentCoordinator) {
    let dir = TempDir::new().unwrap();
    let persistence = FilePersistence::with_base_dir(dir.path().to_path_buf());
    let config = CoordinatorConfig {
        enable_worktree_isolation: false,
        ..Default::default()
    };
    let mut coord = AgentCoordinator::new(config).await.unwrap();
    coord.set_persistence(persistence);
    (dir, coord)
}

fn make_teammate_config(name: &str) -> TeammateConfig {
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

#[tokio::test]
async fn create_team_and_list() {
    let (_dir, coord) = temp_coordinator().await;
    coord
        .create_team("team-alpha".to_string(), "Test team alpha".to_string())
        .await
        .unwrap();

    let teams = coord.list_teams().await;
    assert!(teams.contains(&"team-alpha".to_string()));
}

#[tokio::test]
async fn add_teammate_to_team() {
    let (_dir, coord) = temp_coordinator().await;
    coord
        .create_team("team-beta".to_string(), "Test team beta".to_string())
        .await
        .unwrap();

    coord
        .add_teammate(
            "team-beta",
            "worker-1".to_string(),
            make_teammate_config("worker-1"),
        )
        .await
        .unwrap();

    let members = coord.get_team_members("team-beta").await.unwrap();
    assert!(members.contains(&"worker-1".to_string()));
}

#[tokio::test]
async fn team_manifest_lists_members() {
    let (_dir, coord) = temp_coordinator().await;
    coord
        .create_team("team-gamma".to_string(), "Test team gamma".to_string())
        .await
        .unwrap();

    coord
        .add_teammate(
            "team-gamma",
            "lead".to_string(),
            TeammateConfig {
                agent_type: "lead".to_string(),
                capabilities: vec!["general".to_string()],
                max_concurrent_tasks: 3,
                plan_mode_required: false,
                model: None,
                system_prompt: None,
                temperature: None,
                is_lead: true,
                allowed_tools: vec![],
                permission_mode: None,
                isolation: None,
            },
        )
        .await
        .unwrap();

    coord
        .add_teammate(
            "team-gamma",
            "helper".to_string(),
            make_teammate_config("helper"),
        )
        .await
        .unwrap();

    let manifest = coord.team_manifest("team-gamma").await.unwrap();
    assert_eq!(manifest.members.len(), 2);
}

#[tokio::test]
async fn add_task_and_query_via_board() {
    let (_dir, coord) = temp_coordinator().await;
    coord
        .create_team("team-delta".to_string(), "Test team delta".to_string())
        .await
        .unwrap();

    let task_id = coord
        .add_task(
            "team-delta",
            "Implement feature X".to_string(),
            "Write the code for feature X".to_string(),
            TaskPriority::High,
        )
        .await
        .unwrap();

    let task = coord.task_board().get_task(task_id).await.unwrap();
    assert_eq!(task.subject, "Implement feature X");
    assert_eq!(task.priority, TaskPriority::High);
    assert_eq!(task.status, TaskStatus::Pending);
}

#[tokio::test]
async fn add_task_with_dependencies() {
    let (_dir, coord) = temp_coordinator().await;
    coord
        .create_team("team-eps".to_string(), "Test team epsilon".to_string())
        .await
        .unwrap();

    let task_a = coord
        .add_task(
            "team-eps",
            "Task A".to_string(),
            "First task".to_string(),
            TaskPriority::Medium,
        )
        .await
        .unwrap();

    let task_b = coord
        .add_task_with_deps(
            "team-eps",
            "Task B".to_string(),
            "Depends on A".to_string(),
            TaskPriority::Medium,
            vec![task_a],
        )
        .await
        .unwrap();

    let task = coord.task_board().get_task(task_b).await.unwrap();
    assert!(task.blocked_by.contains(&task_a));
}

#[tokio::test]
async fn self_claim_task_workflow() {
    let (_dir, coord) = temp_coordinator().await;
    coord
        .create_team("team-zeta".to_string(), "Test team zeta".to_string())
        .await
        .unwrap();

    coord
        .add_teammate(
            "team-zeta",
            "worker-1".to_string(),
            make_teammate_config("worker-1"),
        )
        .await
        .unwrap();

    let task_id = coord
        .add_task(
            "team-zeta",
            "Do work".to_string(),
            "Implement something".to_string(),
            TaskPriority::Medium,
        )
        .await
        .unwrap();

    let claimed = coord
        .self_claim_task("team-zeta", "worker-1", task_id)
        .await
        .unwrap();

    assert_eq!(claimed.status, TaskStatus::InProgress);
    assert_eq!(claimed.owner, Some("worker-1".to_string()));
}

#[tokio::test]
async fn find_next_claimable_prefers_earliest() {
    let (_dir, coord) = temp_coordinator().await;
    coord
        .create_team("team-eta".to_string(), "Test team eta".to_string())
        .await
        .unwrap();

    coord
        .add_teammate(
            "team-eta",
            "worker".to_string(),
            make_teammate_config("worker"),
        )
        .await
        .unwrap();

    let task_a = coord
        .add_task(
            "team-eta",
            "First".to_string(),
            "First task".to_string(),
            TaskPriority::Medium,
        )
        .await
        .unwrap();

    let _task_b = coord
        .add_task(
            "team-eta",
            "Second".to_string(),
            "Second task".to_string(),
            TaskPriority::Medium,
        )
        .await
        .unwrap();

    let result = coord.find_next_claimable_task("team-eta", "worker").await;
    assert!(result.is_some());
    let (id, _) = result.unwrap();
    assert_eq!(id, task_a);
}

#[tokio::test]
async fn complete_task_updates_status() {
    let (_dir, coord) = temp_coordinator().await;
    coord
        .create_team("team-theta".to_string(), "Test team theta".to_string())
        .await
        .unwrap();

    coord
        .add_teammate(
            "team-theta",
            "worker-1".to_string(),
            make_teammate_config("worker-1"),
        )
        .await
        .unwrap();

    let task_id = coord
        .add_task(
            "team-theta",
            "Finish it".to_string(),
            "Complete this task".to_string(),
            TaskPriority::Medium,
        )
        .await
        .unwrap();

    // Claim first, then complete
    coord
        .self_claim_task("team-theta", "worker-1", task_id)
        .await
        .unwrap();

    coord
        .complete_task(task_id, "team-theta", "worker-1")
        .await
        .unwrap();

    let task = coord.task_board().get_task(task_id).await.unwrap();
    assert_eq!(task.status, TaskStatus::Completed);
}

#[tokio::test]
async fn send_direct_message() {
    let (_dir, coord) = temp_coordinator().await;
    coord
        .create_team("team-iota".to_string(), "Test team iota".to_string())
        .await
        .unwrap();

    coord
        .add_teammate(
            "team-iota",
            "alice".to_string(),
            make_teammate_config("alice"),
        )
        .await
        .unwrap();
    coord
        .add_teammate("team-iota", "bob".to_string(), make_teammate_config("bob"))
        .await
        .unwrap();

    let msg = coord
        .send_direct_message(
            "team-iota",
            "alice",
            "bob",
            MessageContent::Text("Hello Bob!".to_string()),
        )
        .await
        .unwrap();

    // send_direct_message returns the recipient's response
    assert_eq!(msg.from, "bob");
    assert_eq!(msg.to, "alice");
}

#[tokio::test]
async fn broadcast_to_team() {
    let (_dir, coord) = temp_coordinator().await;
    coord
        .create_team("team-kappa".to_string(), "Test team kappa".to_string())
        .await
        .unwrap();

    coord
        .add_teammate("team-kappa", "m1".to_string(), make_teammate_config("m1"))
        .await
        .unwrap();
    coord
        .add_teammate("team-kappa", "m2".to_string(), make_teammate_config("m2"))
        .await
        .unwrap();

    let messages = coord
        .broadcast_to_team("team-kappa", "lead", "Everyone start working!".to_string())
        .await
        .unwrap();

    assert_eq!(messages.len(), 2);
}

#[tokio::test]
async fn disband_team_removes_it() {
    let (_dir, coord) = temp_coordinator().await;
    coord
        .create_team("team-lambda".to_string(), "Temporary team".to_string())
        .await
        .unwrap();

    coord.disband_team("team-lambda").await.unwrap();
    let teams = coord.list_teams().await;
    assert!(!teams.contains(&"team-lambda".to_string()));
}

#[tokio::test]
async fn notify_idle_returns_available_tasks() {
    let (_dir, coord) = temp_coordinator().await;
    coord
        .create_team("team-mu".to_string(), "Test team mu".to_string())
        .await
        .unwrap();

    coord
        .add_teammate(
            "team-mu",
            "idle-worker".to_string(),
            make_teammate_config("idle-worker"),
        )
        .await
        .unwrap();

    coord
        .add_task(
            "team-mu",
            "Available task".to_string(),
            "Needs doing".to_string(),
            TaskPriority::Medium,
        )
        .await
        .unwrap();

    let available = coord.notify_idle("team-mu", "idle-worker").await.unwrap();

    assert!(!available.is_empty());
    assert_eq!(available[0].subject, "Available task");
}
