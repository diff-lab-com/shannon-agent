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
}
