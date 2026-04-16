//! Integration tests for MCP tool adapter.
//!
//! Tests:
//! - McpToolAdapter construction and property access
//! - Input schema validation
//! - Tool trait method signatures
//! - Registration into ToolRegistry

use serde_json::json;
use shannon_core::tools::ToolRegistry;
use shannon_core::Tool;

/// Test: McpToolAdapter can be constructed with valid parameters.
#[test]
fn test_mcp_adapter_construction() {
    let adapter = shannon_core::mcp_tool_adapter::McpToolAdapter::new(
        "test-server".to_string(),
        Some("node".to_string()),
        vec!["server.js".to_string()],
        std::collections::HashMap::new(),
        "Test MCP tool".to_string(),
        json!({"type": "object", "properties": {"query": {"type": "string"}}}),
    );

    assert_eq!(adapter.name(), "mcp__test-server");
    assert_eq!(adapter.description(), "Test MCP tool");
    assert_eq!(
        adapter.input_schema()["type"],
        json!("object")
    );
}

/// Test: McpToolAdapter with environment variables.
#[test]
fn test_mcp_adapter_with_env() {
    let mut env = std::collections::HashMap::new();
    env.insert("API_KEY".to_string(), "test-key-123".to_string());
    env.insert("DEBUG".to_string(), "true".to_string());

    let adapter = shannon_core::mcp_tool_adapter::McpToolAdapter::new(
        "env-server".to_string(),
        Some("python".to_string()),
        vec!["-m".to_string(), "mcp_server".to_string()],
        env,
        "Server with env".to_string(),
        json!({"type": "object"}),
    );

    assert_eq!(adapter.name(), "mcp__env-server");
}

/// Test: McpToolAdapter can be registered in ToolRegistry.
#[test]
fn test_mcp_adapter_registration_in_tool_registry() {
    let mut registry = ToolRegistry::new();

    let adapter = shannon_core::mcp_tool_adapter::McpToolAdapter::new(
        "my-server".to_string(),
        Some("echo".to_string()),
        vec![],
        std::collections::HashMap::new(),
        "Echo server".to_string(),
        json!({"type": "object", "properties": {"msg": {"type": "string"}}, "required": ["msg"]}),
    );

    let result = registry.register(Box::new(adapter));
    assert!(result.is_ok());

    // Verify the tool is registered
    let tools = registry.list_tools_info();
    assert!(tools.iter().any(|t| t.name == "mcp__my-server"), "Tool should be registered as 'mcp_my-server'");
}

/// Test: Multiple MCP adapters can be registered.
#[test]
fn test_multiple_mcp_adapters_registration() {
    let mut registry = ToolRegistry::new();

    for i in 0..3 {
        let adapter = shannon_core::mcp_tool_adapter::McpToolAdapter::new(
            format!("server-{i}"),
            Some("test-cmd".to_string()),
            vec![],
            std::collections::HashMap::new(),
            format!("Server {i}"),
            json!({"type": "object"}),
        );
        let result = registry.register(Box::new(adapter));
        assert!(result.is_ok(), "Registration of server-{i} should succeed");
    }

    let tools = registry.list_tools_info();
    assert!(tools.iter().any(|t| t.name == "mcp__server-0"));
    assert!(tools.iter().any(|t| t.name == "mcp__server-1"));
    assert!(tools.iter().any(|t| t.name == "mcp__server-2"));
}

/// Test: McpToolAdapter input_schema returns valid JSON Schema.
#[test]
fn test_mcp_adapter_input_schema_structure() {
    let schema = json!({
        "type": "object",
        "properties": {
            "tool_name": {
                "type": "string",
                "description": "Name of the tool to call"
            },
            "arguments": {
                "type": "object",
                "description": "Arguments to pass"
            }
        },
        "required": ["tool_name"]
    });

    let adapter = shannon_core::mcp_tool_adapter::McpToolAdapter::new(
        "schema-test".to_string(),
        Some("cmd".to_string()),
        vec![],
        std::collections::HashMap::new(),
        "Schema test".to_string(),
        schema.clone(),
    );

    let returned_schema = adapter.input_schema();
    assert_eq!(returned_schema["type"], json!("object"));
    assert_eq!(returned_schema["required"], json!(["tool_name"]));
    assert!(returned_schema["properties"]["tool_name"].is_object());
}

/// Test: McpToolAdapter with complex command-line arguments.
#[test]
fn test_mcp_adapter_with_complex_args() {
    let adapter = shannon_core::mcp_tool_adapter::McpToolAdapter::new(
        "complex".to_string(),
        Some("npx".to_string()),
        vec![
            "-y".to_string(),
            "@modelcontextprotocol/server-filesystem".to_string(),
            "/tmp".to_string(),
        ],
        std::collections::HashMap::new(),
        "Filesystem server".to_string(),
        json!({"type": "object"}),
    );

    assert_eq!(adapter.name(), "mcp__complex");
    assert_eq!(adapter.description(), "Filesystem server");
}

/// Test: McpToolAdapter with no command (None).
#[test]
fn test_mcp_adapter_no_command() {
    let adapter = shannon_core::mcp_tool_adapter::McpToolAdapter::new(
        "no-cmd".to_string(),
        None,
        vec![],
        std::collections::HashMap::new(),
        "No command server".to_string(),
        json!({"type": "object"}),
    );

    assert_eq!(adapter.name(), "mcp__no-cmd");
    assert_eq!(adapter.description(), "No command server");
}

/// Test: McpToolAdapter name follows mcp_{server_name} convention.
#[test]
fn test_mcp_adapter_naming_convention() {
    let adapter = shannon_core::mcp_tool_adapter::McpToolAdapter::new(
        "my-filesystem-server".to_string(),
        Some("node".to_string()),
        vec![],
        std::collections::HashMap::new(),
        "FS server".to_string(),
        json!({"type": "object"}),
    );

    assert!(adapter.name().starts_with("mcp__"));
    assert_eq!(adapter.name(), "mcp__my-filesystem-server");
}

/// Test: McpToolAdapter with empty args and empty env.
#[test]
fn test_mcp_adapter_minimal() {
    let adapter = shannon_core::mcp_tool_adapter::McpToolAdapter::new(
        "minimal".to_string(),
        Some("echo".to_string()),
        vec![],
        std::collections::HashMap::new(),
        "Minimal server".to_string(),
        json!({"type": "object"}),
    );

    assert_eq!(adapter.name(), "mcp__minimal");
    let schema = adapter.input_schema();
    assert_eq!(schema["type"], json!("object"));
}
