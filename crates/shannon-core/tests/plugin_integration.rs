//! Integration tests for the Plugin system.
//!
//! Tests:
//! - PluginManager construction and configuration
//! - PluginManifest creation and field access
//! - Plugin state transitions
//! - PluginTool registration via register_plugin_tools
//! - ToolDefinition / CommandDefinition / HookDefinition structures

use shannon_core::plugins::{
    PluginManager, PluginManifest, PluginState, Plugin,
    ToolDefinition, CommandDefinition, HookDefinition,
};
use shannon_core::tools::ToolRegistry;
use shannon_core::plugin_tool::register_plugin_tools;
use std::path::PathBuf;

// ── PluginManager Construction ──────────────────────────────────────────

#[test]
fn test_plugin_manager_new() {
    let pm = PluginManager::new();
    // Default configuration should have plugin dirs
    assert!(!pm.plugin_dirs().is_empty());
    // No plugins loaded initially
    assert!(pm.list_plugins().is_empty());
}

#[test]
fn test_plugin_manager_with_custom_config() {
    let dirs = vec![PathBuf::from("/tmp/test-plugins")];
    let state_file = PathBuf::from("/tmp/test-plugin-state.json");
    let pm = PluginManager::with_config(
        dirs.clone(),
        state_file.clone(),
        "0.1.0",
    );
    assert_eq!(pm.plugin_dirs(), dirs.as_slice());
    assert_eq!(pm.state_file_path(), state_file.as_path());
    assert_eq!(pm.current_version(), "0.1.0");
}

#[test]
fn test_plugin_manager_add_dir() {
    let mut pm = PluginManager::new();
    let initial_count = pm.plugin_dirs().len();
    pm.add_plugin_dir(PathBuf::from("/custom/plugins"));
    assert_eq!(pm.plugin_dirs().len(), initial_count + 1);
}

#[test]
fn test_plugin_manager_list_empty() {
    let pm = PluginManager::new();
    assert!(pm.list_plugins().is_empty());
}

#[test]
fn test_plugin_manager_get_nonexistent() {
    let pm = PluginManager::new();
    assert!(pm.get_plugin("nonexistent").is_none());
}

// ── PluginManifest ─────────────────────────────────────────────────────

#[test]
fn test_plugin_manifest_construction() {
    let manifest = PluginManifest {
        name: "test-plugin".to_string(),
        version: "1.0.0".to_string(),
        description: Some("A test plugin".to_string()),
        author: Some("Test Author".to_string()),
        min_version: Some("0.1.0".to_string()),
        tools: vec![],
        hooks: vec![],
        commands: vec![],
        settings_schema: None,
    };

    assert_eq!(manifest.name, "test-plugin");
    assert_eq!(manifest.version, "1.0.0");
    assert_eq!(manifest.description.as_deref(), Some("A test plugin"));
    assert!(manifest.tools.is_empty());
    assert!(manifest.hooks.is_empty());
    assert!(manifest.commands.is_empty());
}

#[test]
fn test_plugin_manifest_with_tools() {
    let tool_def = ToolDefinition {
        name: "my_tool".to_string(),
        description: "Does something".to_string(),
        input_schema: serde_json::json!({"type": "object"}),
        command: "echo".to_string(),
        is_read_only: true,
    };

    let manifest = PluginManifest {
        name: "tool-plugin".to_string(),
        version: "0.1.0".to_string(),
        description: None,
        author: None,
        min_version: None,
        tools: vec![tool_def],
        hooks: vec![],
        commands: vec![],
        settings_schema: None,
    };

    assert_eq!(manifest.tools.len(), 1);
    assert_eq!(manifest.tools[0].name, "my_tool");
    assert!(manifest.tools[0].is_read_only);
}

#[test]
fn test_plugin_manifest_with_commands() {
    let cmd_def = CommandDefinition {
        name: "review".to_string(),
        description: "Review code".to_string(),
        prompt_template: "Review this code: {args}".to_string(),
    };

    let manifest = PluginManifest {
        name: "cmd-plugin".to_string(),
        version: "0.1.0".to_string(),
        description: None,
        author: None,
        min_version: None,
        tools: vec![],
        hooks: vec![],
        commands: vec![cmd_def],
        settings_schema: None,
    };

    assert_eq!(manifest.commands.len(), 1);
    assert_eq!(manifest.commands[0].name, "review");
    assert!(manifest.commands[0].prompt_template.contains("{args}"));
}

#[test]
fn test_plugin_manifest_with_hooks() {
    let hook_def = HookDefinition {
        event: "tool_execution".to_string(),
        matcher: "*".to_string(),
        command: "validator.sh".to_string(),
        timeout_secs: 30,
        blocking: true,
    };

    let manifest = PluginManifest {
        name: "hook-plugin".to_string(),
        version: "0.1.0".to_string(),
        description: None,
        author: None,
        min_version: None,
        tools: vec![],
        hooks: vec![hook_def],
        commands: vec![],
        settings_schema: None,
    };

    assert_eq!(manifest.hooks.len(), 1);
    assert_eq!(manifest.hooks[0].event, "tool_execution");
    assert!(manifest.hooks[0].blocking);
    assert_eq!(manifest.hooks[0].timeout_secs, 30);
}

// ── Plugin State ───────────────────────────────────────────────────────

#[test]
fn test_plugin_state_variants() {
    let loaded = PluginState::Loaded;
    let active = PluginState::Active;
    let disabled = PluginState::Disabled;
    let failed = PluginState::Failed("error msg".to_string());

    // Verify variants exist and can be compared
    assert!(matches!(loaded, PluginState::Loaded));
    assert!(matches!(active, PluginState::Active));
    assert!(matches!(disabled, PluginState::Disabled));
    assert!(matches!(failed, PluginState::Failed(_)));
}

#[test]
fn test_plugin_construction() {
    let plugin = Plugin {
        manifest: PluginManifest {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            author: None,
            min_version: None,
            tools: vec![],
            hooks: vec![],
            commands: vec![],
            settings_schema: None,
        },
        state: PluginState::Active,
        path: PathBuf::from("/plugins/test"),
        settings: serde_json::json!({}),
    };

    assert_eq!(plugin.manifest.name, "test");
    assert!(matches!(plugin.state, PluginState::Active));
}

// ── register_plugin_tools Integration ───────────────────────────────────

#[test]
fn test_register_plugin_tools_empty_manager() {
    let pm = PluginManager::new();
    let mut registry = ToolRegistry::new();

    // Should not panic with empty plugin manager
    register_plugin_tools(&pm, &mut registry);

    // No tools should be registered
    assert!(registry.list_tools_info().is_empty());
}

#[test]
fn test_register_plugin_tools_with_temp_plugin() {
    // Use a temp directory with a plugin manifest to test real discovery
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let plugin_dir = temp_dir.path().join("test-plugin");
    std::fs::create_dir_all(&plugin_dir).expect("create plugin dir");

    // Write a minimal plugin.json
    let manifest = serde_json::json!({
        "name": "test-plugin",
        "version": "1.0.0",
        "description": "Test",
        "tools": [],
        "hooks": [],
        "commands": []
    });
    std::fs::write(
        plugin_dir.join("plugin.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    ).expect("write plugin.json");

    let state_file = temp_dir.path().join("plugin-state.json");
    let mut pm = PluginManager::with_config(
        vec![temp_dir.path().to_path_buf()],
        state_file,
        "0.1.0",
    );
    let mut registry = ToolRegistry::new();

    // Discover and load from temp dir (no tools in this manifest)
    let rt = tokio::runtime::Runtime::new().expect("create runtime");
    let _ = rt.block_on(pm.discover_and_load_all());

    register_plugin_tools(&pm, &mut registry);
    // Plugin has no tools, so registry should be empty
    assert!(registry.list_tools_info().is_empty());
}

// ── ToolDefinition Properties ───────────────────────────────────────────

#[test]
fn test_tool_definition_schema() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "path": {"type": "string", "description": "File path"},
            "content": {"type": "string", "description": "File content"}
        },
        "required": ["path", "content"]
    });

    let tool_def = ToolDefinition {
        name: "write_file".to_string(),
        description: "Write file to disk".to_string(),
        input_schema: schema.clone(),
        command: "write-file.sh".to_string(),
        is_read_only: false,
    };

    assert_eq!(tool_def.input_schema["type"], "object");
    assert_eq!(tool_def.input_schema["required"], serde_json::json!(["path", "content"]));
    assert!(!tool_def.is_read_only);
}

#[test]
fn test_hook_definition_defaults() {
    let hook = HookDefinition {
        event: "pre_tool".to_string(),
        matcher: "bash".to_string(),
        command: "check-bash.sh".to_string(),
        timeout_secs: 30,
        blocking: true,
    };

    assert_eq!(hook.timeout_secs, 30);
    assert!(hook.blocking);
}

#[test]
fn test_command_definition_template() {
    let cmd = CommandDefinition {
        name: "commit".to_string(),
        description: "Create a git commit".to_string(),
        prompt_template: "Create a commit with message: {args}".to_string(),
    };

    let rendered = cmd.prompt_template.replace("{args}", "fix: update config");
    assert_eq!(rendered, "Create a commit with message: fix: update config");
}
