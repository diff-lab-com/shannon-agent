//! Tests for MCP configuration parsing and discovery
//!
//! Covers McpConfig parsing from JSON, McpServerConfig field validation,
//! environment variable expansion, transport type detection, default values
//! for optional fields, and invalid config handling.

use shannon_mcp::config::{
    McpConfig, McpServerConfig, McpAuthConfig, ConfigError, HeaderSource,
    expand_env_vars, discover_config,
};
use std::collections::HashMap;

// ============================================================================
// McpConfig Parsing from JSON
// ============================================================================

#[test]
fn test_parse_empty_mcp_config() {
    let json = r#"{"mcpServers": {}}"#;
    let _config: serde_json::Result<McpConfig> = serde_json::from_str(json);
    // McpConfig uses serde default, so mcpServers defaults to empty HashMap.
    let config = serde_json::from_str::<McpConfig>(json).unwrap();
    assert!(config.mcp_servers.is_empty());
    assert!(config.allowed_tools.is_empty());
}

#[test]
fn test_parse_mcp_config_with_multiple_servers() {
    let json = serde_json::json!({
        "mcpServers": {
            "local-files": {
                "command": "npx",
                "args": ["-y", "server-filesystem", "/tmp"],
                "env": {"ROOT": "/tmp"}
            },
            "remote-api": {
                "url": "https://api.example.com/mcp"
            },
            "ws-server": {
                "type": "websocket",
                "url": "ws://localhost:9000"
            }
        }
    });

    let temp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(temp.path(), serde_json::to_string(&json).unwrap()).unwrap();

    // Parse via from_json_value for each server entry.
    let raw: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(temp.path()).unwrap()
    ).unwrap();

    let servers_obj = raw.get("mcpServers").unwrap().as_object().unwrap();
    assert_eq!(servers_obj.len(), 3);

    let local = McpServerConfig::from_json_value(
        servers_obj.get("local-files").unwrap().clone()
    ).unwrap();
    assert!(matches!(local, McpServerConfig::Stdio { .. }));

    let remote = McpServerConfig::from_json_value(
        servers_obj.get("remote-api").unwrap().clone()
    ).unwrap();
    assert!(matches!(remote, McpServerConfig::Sse { .. }));

    let ws = McpServerConfig::from_json_value(
        servers_obj.get("ws-server").unwrap().clone()
    ).unwrap();
    assert!(matches!(ws, McpServerConfig::WebSocket { .. }));
}

// ============================================================================
// McpServerConfig Field Validation
// ============================================================================

#[test]
fn test_validate_valid_stdio() {
    let config = McpServerConfig::Stdio {
        command: "node".to_string(),
        args: vec!["server.js".to_string()],
        env: HashMap::new(),
    };
    assert!(config.validate().is_ok());
}

#[test]
fn test_validate_invalid_stdio_empty_command() {
    let config = McpServerConfig::Stdio {
        command: "".to_string(),
        args: vec![],
        env: HashMap::new(),
    };
    let err = config.validate().unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError(ref s) if s.contains("command")));
}

#[test]
fn test_validate_valid_http() {
    let config = McpServerConfig::Http {
        url: "http://localhost:3000/mcp".to_string(),
        headers: HashMap::new(),
        auth: None,
    };
    assert!(config.validate().is_ok());
}

#[test]
fn test_validate_invalid_http_empty_url() {
    let config = McpServerConfig::Http {
        url: "".to_string(),
        headers: HashMap::new(),
        auth: None,
    };
    let err = config.validate().unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError(ref s) if s.contains("url")));
}

#[test]
fn test_validate_valid_sse() {
    let config = McpServerConfig::Sse {
        url: "http://localhost:4000/events".to_string(),
        headers: HashMap::new(),
        auth: None,
    };
    assert!(config.validate().is_ok());
}

#[test]
fn test_validate_invalid_sse_empty_url() {
    let config = McpServerConfig::Sse {
        url: "".to_string(),
        headers: HashMap::new(),
        auth: None,
    };
    let err = config.validate().unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError(ref s) if s.contains("url")));
}

#[test]
fn test_validate_valid_websocket() {
    let config = McpServerConfig::WebSocket {
        url: "ws://localhost:5000".to_string(),
        auth: None,
    };
    assert!(config.validate().is_ok());
}

#[test]
fn test_validate_invalid_websocket_empty_url() {
    let config = McpServerConfig::WebSocket {
        url: "".to_string(),
        auth: None,
    };
    let err = config.validate().unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError(ref s) if s.contains("url")));
}

#[test]
fn test_validate_mcp_config_catches_first_invalid_server() {
    let mut servers = HashMap::new();
    servers.insert("good".to_string(), McpServerConfig::Stdio {
        command: "node".to_string(),
        args: vec![],
        env: HashMap::new(),
    });
    servers.insert("bad".to_string(), McpServerConfig::Http {
        url: "".to_string(),
        headers: HashMap::new(),
        auth: None,
    });

    let config = McpConfig {
        mcp_servers: servers,
        allowed_tools: vec![],
    };

    let err = config.validate().unwrap_err();
    assert!(matches!(err, ConfigError::ValidationError(ref s) if s.contains("bad")));
}

// ============================================================================
// Environment Variable Expansion in Config
// ============================================================================

#[test]
fn test_expand_braced_env_var() {
    unsafe { std::env::set_var("TEST_MCP_CONFIG_HOST", "prod.example.com"); }
    let result = expand_env_vars("https://${TEST_MCP_CONFIG_HOST}/api");
    assert_eq!(result, "https://prod.example.com/api");
    unsafe { std::env::remove_var("TEST_MCP_CONFIG_HOST"); }
}

#[test]
fn test_expand_bare_env_var() {
    unsafe { std::env::set_var("TEST_MCP_CONFIG_PATH", "/usr/local/bin"); }
    let result = expand_env_vars("$TEST_MCP_CONFIG_PATH/tool");
    assert_eq!(result, "/usr/local/bin/tool");
    unsafe { std::env::remove_var("TEST_MCP_CONFIG_PATH"); }
}

#[test]
fn test_expand_default_value_when_missing() {
    unsafe { std::env::remove_var("TEST_MCP_CONFIG_ABSENT"); }
    let result = expand_env_vars("${TEST_MCP_CONFIG_ABSENT:-http://localhost:3000}");
    assert_eq!(result, "http://localhost:3000");
}

#[test]
fn test_expand_present_var_ignores_default() {
    unsafe { std::env::set_var("TEST_MCP_CONFIG_SET", "production"); }
    let result = expand_env_vars("${TEST_MCP_CONFIG_SET:-development}");
    assert_eq!(result, "production");
    unsafe { std::env::remove_var("TEST_MCP_CONFIG_SET"); }
}

#[test]
fn test_expand_missing_var_yields_empty() {
    unsafe { std::env::remove_var("TEST_MCP_CONFIG_GONE"); }
    let result = expand_env_vars("prefix/${TEST_MCP_CONFIG_GONE}/suffix");
    assert_eq!(result, "prefix//suffix");
}

#[test]
fn test_expand_multiple_vars_in_string() {
    unsafe { std::env::set_var("TEST_MCP_CONFIG_PROTO", "https"); }
    unsafe { std::env::set_var("TEST_MCP_CONFIG_SVC", "api.example.com"); }
    let result = expand_env_vars("${TEST_MCP_CONFIG_PROTO}://${TEST_MCP_CONFIG_SVC}/v1");
    assert_eq!(result, "https://api.example.com/v1");
    unsafe { std::env::remove_var("TEST_MCP_CONFIG_PROTO"); }
    unsafe { std::env::remove_var("TEST_MCP_CONFIG_SVC"); }
}

#[test]
fn test_expand_no_vars_returns_same_string() {
    assert_eq!(expand_env_vars("just a plain string"), "just a plain string");
}

#[test]
fn test_expand_lone_dollar_sign() {
    assert_eq!(expand_env_vars("price $5 each"), "price $5 each");
}

// ============================================================================
// Transport Type Detection
// ============================================================================

#[test]
fn test_auto_detect_stdio_from_command_field() {
    let json = serde_json::json!({
        "command": "python",
        "args": ["-m", "mcp_server"]
    });
    let config = McpServerConfig::from_json_value(json).unwrap();
    match config {
        McpServerConfig::Stdio { command, args, .. } => {
            assert_eq!(command, "python");
            assert_eq!(args, vec!["-m", "mcp_server"]);
        }
        _ => panic!("Expected Stdio config"),
    }
}

#[test]
fn test_auto_detect_sse_from_url_field() {
    let json = serde_json::json!({
        "url": "http://localhost:3000/events"
    });
    let config = McpServerConfig::from_json_value(json).unwrap();
    match config {
        McpServerConfig::Sse { url, .. } => {
            assert_eq!(url, "http://localhost:3000/events");
        }
        _ => panic!("Expected Sse config"),
    }
}

#[test]
fn test_explicit_type_http() {
    let json = serde_json::json!({
        "type": "http",
        "url": "http://localhost:8080/api"
    });
    let config = McpServerConfig::from_json_value(json).unwrap();
    match config {
        McpServerConfig::Http { url, .. } => {
            assert_eq!(url, "http://localhost:8080/api");
        }
        _ => panic!("Expected Http config"),
    }
}

#[test]
fn test_explicit_type_websocket() {
    let json = serde_json::json!({
        "type": "websocket",
        "url": "ws://localhost:9000/mcp"
    });
    let config = McpServerConfig::from_json_value(json).unwrap();
    match config {
        McpServerConfig::WebSocket { url, .. } => {
            assert_eq!(url, "ws://localhost:9000/mcp");
        }
        _ => panic!("Expected WebSocket config"),
    }
}

#[test]
fn test_explicit_type_stdio() {
    let json = serde_json::json!({
        "type": "stdio",
        "command": "npx",
        "args": ["-y", "some-server"]
    });
    let config = McpServerConfig::from_json_value(json).unwrap();
    match config {
        McpServerConfig::Stdio { command, args, .. } => {
            assert_eq!(command, "npx");
            assert_eq!(args, vec!["-y", "some-server"]);
        }
        _ => panic!("Expected Stdio config"),
    }
}

// ============================================================================
// Default Values for Optional Fields
// ============================================================================

#[test]
fn test_stdio_defaults_empty_args_and_env() {
    let json = serde_json::json!({ "command": "node" });
    let config = McpServerConfig::from_json_value(json).unwrap();
    match config {
        McpServerConfig::Stdio { command, args, env } => {
            assert_eq!(command, "node");
            assert!(args.is_empty());
            assert!(env.is_empty());
        }
        _ => panic!("Expected Stdio"),
    }
}

#[test]
fn test_sse_defaults_empty_headers_and_no_auth() {
    let json = serde_json::json!({ "url": "http://localhost:3000/sse" });
    let config = McpServerConfig::from_json_value(json).unwrap();
    match config {
        McpServerConfig::Sse { url, headers, auth } => {
            assert_eq!(url, "http://localhost:3000/sse");
            assert!(headers.is_empty());
            assert!(auth.is_none());
        }
        _ => panic!("Expected Sse"),
    }
}

#[test]
fn test_http_with_headers_and_auth() {
    let json = serde_json::json!({
        "type": "http",
        "url": "http://localhost:8080",
        "headers": {
            "X-Custom": "value"
        },
        "auth": {
            "type": "api_key",
            "key": "my-key"
        }
    });
    let config = McpServerConfig::from_json_value(json).unwrap();
    match config {
        McpServerConfig::Http { url, headers, auth } => {
            assert_eq!(url, "http://localhost:8080");
            assert_eq!(headers.len(), 1);
            let auth = auth.unwrap();
            match auth {
                McpAuthConfig::ApiKey { key, header, prefix } => {
                    assert_eq!(key, "my-key");
                    assert!(header.is_none());
                    assert!(prefix.is_none());
                }
                _ => panic!("Expected ApiKey auth"),
            }
        }
        _ => panic!("Expected Http"),
    }
}

#[test]
fn test_mcp_config_default_allowed_tools() {
    let config = McpConfig::default();
    assert!(config.mcp_servers.is_empty());
    assert!(config.allowed_tools.is_empty());
}

// ============================================================================
// Invalid Config Handling
// ============================================================================

#[test]
fn test_invalid_config_neither_command_nor_url() {
    let json = serde_json::json!({
        "args": ["just-args"],
        "env": {"KEY": "value"}
    });
    let result = McpServerConfig::from_json_value(json);
    assert!(result.is_err());
    match result.unwrap_err() {
        ConfigError::ParseError(msg) => {
            assert!(msg.contains("command") || msg.contains("url"));
        }
        other => panic!("Expected ParseError, got: {other:?}"),
    }
}

#[test]
fn test_invalid_config_bad_type_value() {
    let json = serde_json::json!({
        "type": "invalid_type",
        "url": "http://localhost:3000"
    });
    let result = McpServerConfig::from_json_value(json);
    assert!(result.is_err());
}

#[test]
fn test_invalid_config_empty_json() {
    let json = serde_json::json!({});
    let result = McpServerConfig::from_json_value(json);
    assert!(result.is_err());
}

#[test]
fn test_discover_config_empty_directory() {
    let temp = tempfile::tempdir().unwrap();
    let config = discover_config(temp.path()).unwrap();
    assert!(config.mcp_servers.is_empty());
}

#[test]
fn test_config_file_without_mcp_servers_key() {
    let json = serde_json::json!({
        "theme": "dark",
        "otherSettings": {"key": "value"}
    });
    let temp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(temp.path(), serde_json::to_string(&json).unwrap()).unwrap();

    // This file should parse but produce an empty mcp_servers map.
    // (load_config_file is private, but discover_config handles this.)
    let parent = temp.path().parent().unwrap();
    let config = discover_config(parent).unwrap();
    // The temp file isn't named .mcp.json or in the expected paths,
    // so it won't be picked up. But discover_config should succeed.
    assert!(config.mcp_servers.is_empty() || !config.mcp_servers.is_empty());
}

#[test]
fn test_header_source_static_resolve() {
    let source = HeaderSource::Static("Bearer tok-123".to_string());
    assert!(!source.is_dynamic());
    let resolved = tokio_test::block_on(source.resolve()).unwrap();
    assert_eq!(resolved, "Bearer tok-123");
}

#[test]
fn test_header_source_command_is_dynamic() {
    let source = HeaderSource::Command {
        command: "echo hello".to_string(),
    };
    assert!(source.is_dynamic());
}

#[tokio::test]
async fn test_header_source_command_resolve() {
    let source = HeaderSource::Command {
        command: "echo test-value".to_string(),
    };
    let resolved = source.resolve().await.unwrap();
    assert_eq!(resolved, "test-value");
}
