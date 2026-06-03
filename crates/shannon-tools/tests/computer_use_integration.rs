//! Integration tests for the computer use feature.

use shannon_core::tools::ToolRegistry;
use shannon_tools::register_default_tools;
use shannon_tools::{ComputerUseTool, Tool};

#[test]
fn computer_tool_registered_by_default() {
    let mut registry = ToolRegistry::new();
    register_default_tools(&mut registry).unwrap();

    let info = registry.list_tools_info();
    let computer = info.iter().find(|t| t.name == "computer");
    assert!(computer.is_some(), "computer tool should be registered");
    assert!(computer.unwrap().description.contains("screenshot"));
}

#[test]
fn computer_tool_schema_has_all_actions() {
    let tool = ComputerUseTool::new();
    let schema = tool.input_schema();

    let actions = schema
        .pointer("/properties/action/enum")
        .unwrap()
        .as_array()
        .unwrap();

    let action_names: Vec<&str> = actions.iter().map(|v| v.as_str().unwrap()).collect();

    assert!(action_names.contains(&"screenshot"));
    assert!(action_names.contains(&"click"));
    assert!(action_names.contains(&"type"));
    assert!(action_names.contains(&"scroll"));
    assert!(action_names.contains(&"key_press"));
    assert!(action_names.contains(&"wait"));
    assert!(action_names.contains(&"mouse_move"));
    assert!(action_names.contains(&"left_click_drag"));
}

#[tokio::test]
async fn computer_tool_screenshot_without_feature() {
    let tool = ComputerUseTool::new();
    let result = tool
        .execute(serde_json::json!({"action": "screenshot"}))
        .await
        .unwrap();

    #[cfg(not(feature = "computer-use"))]
    {
        assert!(result.is_error);
        assert!(result.content.contains("computer-use"));
    }
}

#[tokio::test]
async fn computer_tool_wait_always_works() {
    let tool = ComputerUseTool::new();
    let start = std::time::Instant::now();
    let result = tool
        .execute(serde_json::json!({"action": "wait", "duration": 0.05}))
        .await
        .unwrap();
    assert!(!result.is_error);
    assert!(start.elapsed() >= std::time::Duration::from_millis(40));
}

#[test]
fn no_duplicate_tool_names_after_registration() {
    let mut registry = ToolRegistry::new();
    register_default_tools(&mut registry).unwrap();

    let tools = registry.list_tools_info();
    let mut names = std::collections::HashSet::new();
    for t in &tools {
        assert!(names.insert(t.name.clone()), "Duplicate tool: {}", t.name);
    }
}
