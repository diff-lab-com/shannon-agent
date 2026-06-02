//! Integration tests for agent definition registry and config persistence.

use shannon_agents::AgentDefinitionRegistry;

// ============================================================================
// Agent Definition Registry Tests
// ============================================================================

mod agent_definition_tests {
    use super::*;

    #[test]
    fn builtin_definitions_load() {
        let registry = AgentDefinitionRegistry::load_from_dirs();
        assert!(
            !registry.all().is_empty(),
            "Should have built-in definitions"
        );
    }

    #[test]
    fn builtin_explorer_exists() {
        let registry = AgentDefinitionRegistry::load_from_dirs();
        let explorer = registry.get("explorer");
        assert!(explorer.is_some(), "explorer agent should exist");
    }

    #[test]
    fn builtin_planner_exists() {
        let registry = AgentDefinitionRegistry::load_from_dirs();
        let planner = registry.get("planner");
        assert!(planner.is_some(), "planner agent should exist");
    }

    #[test]
    fn builtin_has_system_prompts() {
        let registry = AgentDefinitionRegistry::load_from_dirs();
        for (name, def) in registry.all() {
            if let Some(ref prompt) = def.system_prompt {
                assert!(!prompt.is_empty(), "Agent '{name}' has empty system prompt");
            }
        }
    }

    #[test]
    fn unknown_agent_returns_none() {
        let registry = AgentDefinitionRegistry::load_from_dirs();
        assert!(registry.get("nonexistent-agent-type").is_none());
    }

    #[test]
    fn list_names_includes_builtins() {
        let registry = AgentDefinitionRegistry::load_from_dirs();
        let names = registry.list_names();
        assert!(
            names.contains(&"explorer".to_string()),
            "Should include explorer"
        );
    }

    #[test]
    fn load_from_custom_toml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("custom-researcher.toml"),
            r#"
name = "custom-researcher"
description = "Custom research agent"
system_prompt = "You are a specialized researcher."
allowed_tools = ["Grep", "Glob", "Read"]
"#,
        )
        .unwrap();

        let mut registry = AgentDefinitionRegistry::load_from_dirs();
        registry.load_from_dir(dir.path());
        let def = registry.get("custom-researcher");
        assert!(def.is_some());
        let def = def.unwrap();
        assert_eq!(
            def.system_prompt.as_deref(),
            Some("You are a specialized researcher.")
        );
        assert_eq!(def.allowed_tools.len(), 3);
    }

    #[test]
    fn custom_overrides_builtin() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("explorer.toml"),
            r#"
name = "explorer"
description = "Overridden explorer"
system_prompt = "Custom prompt override"
"#,
        )
        .unwrap();

        let mut registry = AgentDefinitionRegistry::load_from_dirs();
        registry.load_from_dir(dir.path());

        let def = registry.get("explorer").unwrap();
        assert_eq!(def.system_prompt.as_deref(), Some("Custom prompt override"));
    }

    #[test]
    fn invalid_toml_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("bad.toml"), "not valid toml [[[[[").unwrap();

        let mut registry = AgentDefinitionRegistry::load_from_dirs();
        registry.load_from_dir(dir.path());
        assert!(registry.get("bad").is_none());
    }

    #[test]
    fn nonexistent_dir_is_ok() {
        let mut registry = AgentDefinitionRegistry::load_from_dirs();
        registry.load_from_dir(std::path::Path::new("/no/such/directory"));
    }

    #[test]
    fn load_from_markdown_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("code-reviewer.md"),
            r#"---
model: claude-opus
---
You are a code reviewer. Focus on bugs and security issues.
"#,
        )
        .unwrap();

        let mut registry = AgentDefinitionRegistry::load_from_dirs();
        registry.load_markdown_from_dir(dir.path());

        let def = registry.get("code-reviewer");
        assert!(def.is_some());
        let def = def.unwrap();
        assert_eq!(def.model.as_deref(), Some("claude-opus"));
        assert!(
            def.system_prompt
                .as_deref()
                .unwrap()
                .contains("code reviewer")
        );
    }

    #[test]
    fn markdown_without_frontmatter() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("simple-agent.md"),
            "You are a simple agent with no frontmatter.",
        )
        .unwrap();

        let mut registry = AgentDefinitionRegistry::load_from_dirs();
        registry.load_markdown_from_dir(dir.path());

        let def = registry.get("simple-agent");
        assert!(def.is_some());
        let def = def.unwrap();
        assert!(
            def.system_prompt
                .as_deref()
                .unwrap()
                .contains("simple agent")
        );
        assert!(def.model.is_none());
    }

    #[test]
    fn toml_full_field_coverage() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("full-agent.toml"),
            r#"
name = "full-agent"
description = "Agent with all fields"
system_prompt = "You are a full agent."
model = "claude-sonnet"
capabilities = ["rust", "testing"]
allowed_tools = ["Bash", "Read", "Write"]
max_concurrent_tasks = 5
plan_mode_required = true
temperature = 0.7
"#,
        )
        .unwrap();

        let mut registry = AgentDefinitionRegistry::load_from_dirs();
        registry.load_from_dir(dir.path());

        let def = registry.get("full-agent").unwrap();
        assert_eq!(def.model.as_deref(), Some("claude-sonnet"));
        assert_eq!(def.capabilities, vec!["rust", "testing"]);
        assert_eq!(def.allowed_tools, vec!["Bash", "Read", "Write"]);
        assert_eq!(def.max_concurrent_tasks, 5);
        assert!(def.plan_mode_required);
        assert_eq!(def.temperature, Some(0.7));
    }

    #[test]
    fn local_toml_overrides_global() {
        let global_dir = tempfile::tempdir().unwrap();
        std::fs::write(
            global_dir.path().join("shared.toml"),
            r#"
name = "shared"
description = "Global version"
system_prompt = "Global prompt"
model = "haiku"
"#,
        )
        .unwrap();

        let local_dir = tempfile::tempdir().unwrap();
        std::fs::write(
            local_dir.path().join("shared.toml"),
            r#"
name = "shared"
description = "Local version"
system_prompt = "Local prompt"
model = "opus"
"#,
        )
        .unwrap();

        let mut registry = AgentDefinitionRegistry::load_from_dirs();
        registry.load_from_dir(global_dir.path());
        registry.load_from_dir(local_dir.path());

        let def = registry.get("shared").unwrap();
        assert_eq!(def.system_prompt.as_deref(), Some("Local prompt"));
        assert_eq!(def.model.as_deref(), Some("opus"));
    }
}
