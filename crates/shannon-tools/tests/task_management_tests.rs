//! Task management integration tests
//!
//! Tests the TaskTool and todo module tools through the public Tool trait
//! interface using serde_json input values, matching the patterns from
//! tool_tests.rs.

use shannon_tools::{
    TaskTool, Tool,
    task::TaskStatus,
    todo::{
        TaskCreateTool, TaskListTool, TaskUpdateTool, TaskGetTool, TaskStore,
        TodoItem, TodoStatus,
    },
};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

// ============================================================================
// TaskTool tests (task.rs - single-tool CRUD via "operation" dispatch)
// ============================================================================

#[tokio::test]
async fn test_task_create_with_subject_and_description() {
    let tool = TaskTool::new();
    let input = serde_json::json!({
        "operation": "Create",
        "subject": "Implement auth",
        "description": "Add JWT authentication to the API"
    });

    let result = tool.execute(input).await.unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains("Created task"));

    let task = result.metadata.get("task").unwrap();
    assert_eq!(task["subject"], "Implement auth");
    assert_eq!(task["description"], "Add JWT authentication to the API");
    assert_eq!(task["status"], "pending");
    assert!(task["owner"].is_null());
}

#[tokio::test]
async fn test_task_create_with_metadata_and_active_form() {
    let tool = TaskTool::new();
    let input = serde_json::json!({
        "operation": "Create",
        "subject": "Deploy service",
        "description": "Deploy the microservice to staging",
        "active_form": "Deploying service to staging",
        "metadata": {
            "priority": "high",
            "team": "backend"
        }
    });

    let result = tool.execute(input).await.unwrap();
    assert!(!result.is_error);

    let task = result.metadata.get("task").unwrap();
    assert_eq!(task["active_form"], "Deploying service to staging");
    let metadata = task["metadata"].as_object().unwrap();
    assert_eq!(metadata["priority"], "high");
    assert_eq!(metadata["team"], "backend");
}

#[tokio::test]
async fn test_task_lifecycle_pending_to_completed() {
    let tool = TaskTool::new();

    // Create task (starts as Pending)
    let create_input = serde_json::json!({
        "operation": "Create",
        "subject": "Write tests",
        "description": "Add comprehensive test suite"
    });
    let create_result = tool.execute(create_input).await.unwrap();
    let task_id = create_result.metadata["task"]["id"].as_str().unwrap().to_string();
    assert_eq!(create_result.metadata["task"]["status"], "pending");

    // Update to InProgress
    let update_input = serde_json::json!({
        "operation": "Update",
        "task_id": task_id,
        "status": "inprogress"
    });
    let update_result = tool.execute(update_input).await.unwrap();
    assert_eq!(update_result.metadata["task"]["status"], "inprogress");

    // Update to Completed
    let complete_input = serde_json::json!({
        "operation": "Update",
        "task_id": task_id,
        "status": "completed"
    });
    let complete_result = tool.execute(complete_input).await.unwrap();
    assert_eq!(complete_result.metadata["task"]["status"], "completed");
}

#[tokio::test]
async fn test_task_deletion_via_status() {
    let tool = TaskTool::new();

    // Create a task
    let create_input = serde_json::json!({
        "operation": "Create",
        "subject": "To delete",
        "description": "This task will be deleted"
    });
    let create_result = tool.execute(create_input).await.unwrap();
    let task_id = create_result.metadata["task"]["id"].as_str().unwrap().to_string();

    // Mark as Deleted
    let delete_input = serde_json::json!({
        "operation": "Update",
        "task_id": task_id,
        "status": "deleted"
    });
    let delete_result = tool.execute(delete_input).await.unwrap();
    assert_eq!(delete_result.metadata["task"]["status"], "deleted");
}

#[tokio::test]
async fn test_task_dependency_tracking() {
    let tool = TaskTool::new();

    // Create two tasks
    let create1 = serde_json::json!({
        "operation": "Create",
        "subject": "Design API",
        "description": "Design the REST API"
    });
    let result1 = tool.execute(create1).await.unwrap();
    let task1_id = result1.metadata["task"]["id"].as_str().unwrap().to_string();

    let create2 = serde_json::json!({
        "operation": "Create",
        "subject": "Implement API",
        "description": "Implement the REST API endpoints"
    });
    let result2 = tool.execute(create2).await.unwrap();
    let task2_id = result2.metadata["task"]["id"].as_str().unwrap().to_string();

    // Add blocks/blocked_by relationships
    let update_input = serde_json::json!({
        "operation": "Update",
        "task_id": task1_id,
        "add_blocks": [task2_id]
    });
    let update_result = tool.execute(update_input).await.unwrap();
    let blocks = update_result.metadata["task"]["blocks"].as_array().unwrap();
    assert!(blocks.iter().any(|b| b.as_str() == Some(&task2_id)));

    // Add blocked_by on task2
    let update_input2 = serde_json::json!({
        "operation": "Update",
        "task_id": task2_id,
        "add_blocked_by": [task1_id]
    });
    let update_result2 = tool.execute(update_input2).await.unwrap();
    let blocked_by = update_result2.metadata["task"]["blocked_by"].as_array().unwrap();
    assert!(blocked_by.iter().any(|b| b.as_str() == Some(&task1_id)));
}

#[tokio::test]
async fn test_task_metadata_merging_via_update() {
    let tool = TaskTool::new();

    // Create with initial metadata
    let create = serde_json::json!({
        "operation": "Create",
        "subject": "Feature X",
        "description": "Build feature X",
        "metadata": {"priority": "medium"}
    });
    let result = tool.execute(create).await.unwrap();
    assert_eq!(result.metadata["task"]["metadata"]["priority"], "medium");
}

#[tokio::test]
async fn test_task_owner_assignment() {
    let tool = TaskTool::new();

    let create = serde_json::json!({
        "operation": "Create",
        "subject": "Refactor module",
        "description": "Refactor the auth module"
    });
    let result = tool.execute(create).await.unwrap();
    let task_id = result.metadata["task"]["id"].as_str().unwrap().to_string();
    assert!(result.metadata["task"]["owner"].is_null());

    // Assign owner
    let update = serde_json::json!({
        "operation": "Update",
        "task_id": task_id,
        "owner": "agent-alpha"
    });
    let update_result = tool.execute(update).await.unwrap();
    assert_eq!(update_result.metadata["task"]["owner"], "agent-alpha");
}

#[tokio::test]
async fn test_multiple_tasks_listing_and_ordering() {
    let tool = TaskTool::new();

    // Create several tasks with slight delay to ensure ordering
    let subjects = ["Task A", "Task B", "Task C"];
    for subject in &subjects {
        let input = serde_json::json!({
            "operation": "Create",
            "subject": *subject,
            "description": format!("Description for {}", *subject)
        });
        tool.execute(input).await.unwrap();
    }

    // List all tasks
    let list_input = serde_json::json!({
        "operation": "List"
    });
    let list_result = tool.execute(list_input).await.unwrap();
    assert!(!list_result.is_error);
    assert_eq!(list_result.metadata["count"], 3);

    let tasks = list_result.metadata["tasks"].as_array().unwrap();
    // Tasks should be sorted by ID (ascending)
    let ids: Vec<&str> = tasks.iter().map(|t| t["id"].as_str().unwrap()).collect();
    assert!(ids.windows(2).all(|w| w[0] <= w[1]), "Tasks should be sorted by ID");
}

#[tokio::test]
async fn test_task_get_found_and_not_found() {
    let tool = TaskTool::new();

    // Create a task
    let create = serde_json::json!({
        "operation": "Create",
        "subject": "Findable task",
        "description": "This task can be retrieved"
    });
    let result = tool.execute(create).await.unwrap();
    let task_id = result.metadata["task"]["id"].as_str().unwrap().to_string();

    // Get existing task
    let get_input = serde_json::json!({
        "operation": "Get",
        "task_id": task_id
    });
    let get_result = tool.execute(get_input).await.unwrap();
    assert!(!get_result.is_error);
    assert!(get_result.content.contains("found"));

    // Get non-existent task
    let missing_input = serde_json::json!({
        "operation": "Get",
        "task_id": "99999"
    });
    let missing_result = tool.execute(missing_input).await.unwrap();
    assert!(missing_result.is_error);
    assert!(missing_result.content.contains("not found"));
}

#[tokio::test]
async fn test_task_update_nonexistent_returns_error() {
    let tool = TaskTool::new();
    let input = serde_json::json!({
        "operation": "Update",
        "task_id": "nonexistent",
        "status": "completed"
    });
    let result = tool.execute(input).await;
    assert!(result.is_err(), "Updating nonexistent task should return Err");
}

#[tokio::test]
async fn test_task_unknown_operation_rejected() {
    let tool = TaskTool::new();
    let input = serde_json::json!({
        "operation": "Delete"
    });
    let result = tool.execute(input).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Unknown operation"));
}

// ============================================================================
// TaskCreateTool / TaskListTool / TaskUpdateTool / TaskGetTool (todo.rs)
// Tests using shared store to verify cross-tool data sharing.
// ============================================================================

#[tokio::test]
async fn test_todo_task_create_and_list_with_shared_store() {
    let store: TaskStore = Arc::new(RwLock::new(HashMap::new()));
    let create_tool = TaskCreateTool::with_store(store.clone());
    let list_tool = TaskListTool::with_store(store.clone());

    // Create two tasks
    let input1 = serde_json::json!({
        "subject": "Setup CI",
        "description": "Configure the CI pipeline"
    });
    create_tool.execute(input1).await.unwrap();

    let input2 = serde_json::json!({
        "subject": "Add tests",
        "description": "Write unit tests"
    });
    create_tool.execute(input2).await.unwrap();

    // List all
    let list_input = serde_json::json!({});
    let list_result = list_tool.execute(list_input).await.unwrap();
    assert_eq!(list_result.metadata["count"], 2);
}

#[tokio::test]
async fn test_todo_task_update_lifecycle_with_shared_store() {
    let store: TaskStore = Arc::new(RwLock::new(HashMap::new()));
    let create_tool = TaskCreateTool::with_store(store.clone());
    let update_tool = TaskUpdateTool::with_store(store.clone());
    let get_tool = TaskGetTool::with_store(store.clone());

    // Create
    let create_input = serde_json::json!({
        "subject": "Build feature",
        "description": "Build the notification feature"
    });
    let create_result = create_tool.execute(create_input).await.unwrap();
    let task_id = create_result.metadata["task_id"].as_str().unwrap().to_string();

    // Update to in_progress
    let update_input = serde_json::json!({
        "task_id": task_id,
        "status": "in_progress"
    });
    update_tool.execute(update_input).await.unwrap();

    // Verify via get
    let get_input = serde_json::json!({"task_id": task_id});
    let get_result = get_tool.execute(get_input).await.unwrap();
    assert!(!get_result.is_error);
    let task = get_result.metadata["task"].as_object().unwrap();
    assert_eq!(task["status"], "in_progress");
}

#[tokio::test]
async fn test_todo_task_list_with_status_filter() {
    let store: TaskStore = Arc::new(RwLock::new(HashMap::new()));
    let create_tool = TaskCreateTool::with_store(store.clone());
    let update_tool = TaskUpdateTool::with_store(store.clone());
    let list_tool = TaskListTool::with_store(store.clone());

    // Create two tasks
    let c1 = serde_json::json!({"subject": "Task 1", "description": "First task"});
    let r1 = create_tool.execute(c1).await.unwrap();
    let id1 = r1.metadata["task_id"].as_str().unwrap().to_string();

    let c2 = serde_json::json!({"subject": "Task 2", "description": "Second task"});
    create_tool.execute(c2).await.unwrap();

    // Complete first task
    update_tool.execute(serde_json::json!({
        "task_id": id1, "status": "completed"
    })).await.unwrap();

    // Filter for pending only
    let list_result = list_tool.execute(serde_json::json!({
        "status_filter": "pending"
    })).await.unwrap();
    assert_eq!(list_result.metadata["count"], 1);

    // Filter for completed only
    let list_result = list_tool.execute(serde_json::json!({
        "status_filter": "completed"
    })).await.unwrap();
    assert_eq!(list_result.metadata["count"], 1);
}

// ============================================================================
// TaskStatus / TodoStatus serialization tests
// ============================================================================

#[test]
fn test_task_status_serialization_roundtrip() {
    let statuses = vec![
        TaskStatus::Pending,
        TaskStatus::InProgress,
        TaskStatus::Completed,
        TaskStatus::Deleted,
    ];
    for status in statuses {
        let json = serde_json::to_string(&status).unwrap();
        let parsed: TaskStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, parsed);
    }
}

#[test]
fn test_task_status_lowercase_serialization() {
    assert_eq!(serde_json::to_string(&TaskStatus::Pending).unwrap(), "\"pending\"");
    assert_eq!(serde_json::to_string(&TaskStatus::InProgress).unwrap(), "\"inprogress\"");
    assert_eq!(serde_json::to_string(&TaskStatus::Completed).unwrap(), "\"completed\"");
    assert_eq!(serde_json::to_string(&TaskStatus::Deleted).unwrap(), "\"deleted\"");
}

#[test]
fn test_todo_status_serialization_roundtrip() {
    let statuses = vec![
        TodoStatus::Pending,
        TodoStatus::InProgress,
        TodoStatus::Completed,
    ];
    for status in statuses {
        let json = serde_json::to_string(&status).unwrap();
        let parsed: TodoStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, parsed);
    }
}

#[test]
fn test_todo_item_new_has_valid_uuid() {
    let item = TodoItem::new("Test task".to_string());
    assert!(uuid::Uuid::parse_str(&item.task_id).is_ok());
    assert_eq!(item.status, TodoStatus::Pending);
    assert!(item.blocked_by.is_empty());
    assert!(item.active_form.is_none());
    assert!(item.metadata.is_none());
}

// ============================================================================
// Tool trait metadata tests
// ============================================================================

#[test]
fn test_task_tool_name() {
    let tool = TaskTool::new();
    assert_eq!(tool.name(), "Task");
}

#[test]
fn test_task_tool_description() {
    let tool = TaskTool::new();
    assert!(!tool.description().is_empty());
}

#[test]
fn test_task_tool_input_schema() {
    let tool = TaskTool::new();
    let schema = tool.input_schema();
    assert_eq!(schema["type"], "object");
    let required = schema["required"].as_array().unwrap();
    assert!(required.contains(&serde_json::json!("operation")));
}

#[test]
fn test_task_create_tool_schema() {
    let tool = TaskCreateTool::new();
    let schema = tool.input_schema();
    let required = schema["required"].as_array().unwrap();
    assert!(required.contains(&serde_json::json!("subject")));
    assert!(required.contains(&serde_json::json!("description")));
}

#[test]
fn test_task_list_tool_is_read_only() {
    let tool = TaskListTool::new();
    assert!(tool.is_read_only());
}

#[test]
fn test_task_get_tool_is_read_only() {
    let tool = TaskGetTool::new();
    assert!(tool.is_read_only());
}
