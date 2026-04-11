//! Integration tests for shannon-commands
//!
//! Tests cross-module flows: parsing → registry lookup, serialization,
//! and built-in command registration.

use shannon_commands::{
    CommandParser, ParsedCommand, CommandRegistry, CommandSource, CommandAvailability,
    Command, CommandBase, PromptCommand, LocalCommand,
    LocalJSXCommand,
};

// ── Helpers ──────────────────────────────────────────────────────

fn make_prompt_command(name: &str, desc: &str) -> Command {
    Command::Prompt(PromptCommand {
        base: CommandBase {
            name: name.to_string(),
            aliases: vec![],
            description: desc.to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: None,
            when_to_use: None,
            version: None,
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        progress_message: format!("{} in progress...", name),
        content_length: 0,
        arg_names: vec![],
        allowed_tools: vec![],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: Default::default(),
        agent: None,
        paths: vec![],
        prompt_template: None,
    })
}

fn make_local_command(name: &str, desc: &str) -> Command {
    Command::Local(LocalCommand {
        base: CommandBase {
            name: name.to_string(),
            aliases: vec![format!("{}-alt", name)],
            description: desc.to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: None,
            when_to_use: None,
            version: None,
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        supports_non_interactive: true,
    })
}

fn make_jsx_command(name: &str, desc: &str) -> Command {
    Command::LocalJSX(LocalJSXCommand {
        base: CommandBase {
            name: name.to_string(),
            aliases: vec![],
            description: desc.to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: None,
            when_to_use: None,
            version: None,
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        progress_message: format!("{} running...", name),
    })
}

// ── Parser Tests ──────────────────────────────────────────────────

#[test]
fn test_parse_simple_command() {
    let parser = CommandParser::new();
    let result = parser.parse("/commit").unwrap();
    assert_eq!(result.name, "commit");
    assert_eq!(result.args, "");
    assert_eq!(result.raw, "/commit");
}

#[test]
fn test_parse_command_with_args() {
    let parser = CommandParser::new();
    let result = parser.parse("/review-pr 123").unwrap();
    assert_eq!(result.name, "review-pr");
    assert_eq!(result.args_trimmed(), "123");
}

#[test]
fn test_parse_command_with_quoted_args() {
    let parser = CommandParser::new();
    let result = parser.parse("/commit -m \"fix bug\"").unwrap();
    assert_eq!(result.name, "commit");
    assert!(result.args_trimmed().contains("fix bug"));
}

#[test]
fn test_parse_without_prefix_fails() {
    let parser = CommandParser::new();
    let result = parser.parse("commit");
    assert!(result.is_err());
}

#[test]
fn test_parse_empty_fails() {
    let parser = CommandParser::new();
    let result = parser.parse("");
    assert!(result.is_err());
}

#[test]
fn test_parse_only_prefix_fails() {
    let parser = CommandParser::new();
    let result = parser.parse("/");
    assert!(result.is_err());
}

#[test]
fn test_parse_with_custom_prefix() {
    let parser = CommandParser::with_prefix("!".to_string());
    let result = parser.parse("!commit").unwrap();
    assert_eq!(result.name, "commit");
}

#[test]
fn test_args_split() {
    let parser = CommandParser::new();
    let result = parser.parse("/tool search --type file").unwrap();
    let split = result.args_split();
    assert_eq!(split, vec!["search", "--type", "file"]);
}

#[test]
fn test_parsed_command_flags_empty() {
    let cmd = ParsedCommand::new("test".to_string(), "arg1 arg2".to_string(), "/test arg1 arg2".to_string());
    assert!(!cmd.has_flag("verbose"));
    assert_eq!(cmd.flag_value("verbose"), None);
}

// ── Registry Integration Tests ────────────────────────────────────

#[tokio::test]
async fn test_register_prompt_command_and_retrieve() {
    let registry = CommandRegistry::new();
    let cmd = make_prompt_command("generate", "Generate code");
    registry.register(cmd).await.unwrap();

    let retrieved = registry.get("generate").await.unwrap();
    assert_eq!(retrieved.name(), "generate");
    assert_eq!(retrieved.description(), "Generate code");
}

#[tokio::test]
async fn test_register_local_command() {
    let registry = CommandRegistry::new();
    let cmd = make_local_command("clear", "Clear screen");
    registry.register(cmd).await.unwrap();

    let retrieved = registry.get("clear").await.unwrap();
    assert_eq!(retrieved.name(), "clear");
}

#[tokio::test]
async fn test_register_jsx_command() {
    let registry = CommandRegistry::new();
    let cmd = make_jsx_command("dashboard", "Open dashboard");
    registry.register(cmd).await.unwrap();

    let retrieved = registry.get("dashboard").await.unwrap();
    assert_eq!(retrieved.name(), "dashboard");
}

#[tokio::test]
async fn test_alias_resolution() {
    let registry = CommandRegistry::new();
    let cmd = make_local_command("status", "Show status");
    registry.register(cmd).await.unwrap();

    // Lookup by alias
    let retrieved = registry.get("status-alt").await.unwrap();
    assert_eq!(retrieved.name(), "status");
}

#[tokio::test]
async fn test_list_names_includes_all_registered() {
    let registry = CommandRegistry::new();
    registry.register(make_prompt_command("a", "Cmd A")).await.unwrap();
    registry.register(make_local_command("b", "Cmd B")).await.unwrap();
    registry.register(make_jsx_command("c", "Cmd C")).await.unwrap();

    let names = registry.list_names().await;
    assert_eq!(names.len(), 3);
    assert!(names.contains(&"a".to_string()));
    assert!(names.contains(&"b".to_string()));
    assert!(names.contains(&"c".to_string()));
}

#[tokio::test]
async fn test_search_finds_matching_commands() {
    let registry = CommandRegistry::new();
    registry.register(make_prompt_command("commit", "Commit changes")).await.unwrap();
    registry.register(make_prompt_command("compact", "Compact context")).await.unwrap();
    registry.register(make_prompt_command("deploy", "Deploy application")).await.unwrap();

    let results = registry.search("comp").await;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name(), "compact");
}

#[tokio::test]
async fn test_search_returns_empty_for_no_match() {
    let registry = CommandRegistry::new();
    registry.register(make_prompt_command("commit", "Commit changes")).await.unwrap();

    let results = registry.search("nonexistent").await;
    assert!(results.is_empty());
}

#[tokio::test]
async fn test_duplicate_registration_overwrites() {
    let registry = CommandRegistry::new();
    registry.register(make_prompt_command("test", "Original")).await.unwrap();
    registry.register(make_prompt_command("test", "Updated")).await.unwrap();

    let retrieved = registry.get("test").await.unwrap();
    assert_eq!(retrieved.description(), "Updated");
}

// ── Serialization Tests ──────────────────────────────────────────

#[test]
fn test_command_source_serialization() {
    for source in [
        CommandSource::Builtin,
        CommandSource::Mcp,
        CommandSource::Plugin,
        CommandSource::Skills,
        CommandSource::Bundled,
        CommandSource::CommandsDeprecated,
        CommandSource::Managed,
    ] {
        let json = serde_json::to_string(&source).unwrap();
        let parsed: CommandSource = serde_json::from_str(&json).unwrap();
        assert_eq!(source, parsed);
    }
}

#[test]
fn test_command_availability_serialization() {
    for avail in [CommandAvailability::ClaudeAI, CommandAvailability::Console, CommandAvailability::All] {
        let json = serde_json::to_string(&avail).unwrap();
        let parsed: CommandAvailability = serde_json::from_str(&json).unwrap();
        assert_eq!(avail, parsed);
    }
}

// ── Parser → Registry Integration ───────────────────────────────

#[tokio::test]
async fn test_parse_then_lookup() {
    let registry = CommandRegistry::new();
    registry.register(make_prompt_command("review", "Review code")).await.unwrap();

    let parser = CommandParser::new();
    let parsed = parser.parse("/review 42").unwrap();

    // Use parsed name to look up in registry
    let cmd = registry.get(&parsed.name).await.unwrap();
    assert_eq!(cmd.name(), "review");
}

#[tokio::test]
async fn test_parse_unknown_command_not_in_registry() {
    let registry = CommandRegistry::new();

    let parser = CommandParser::new();
    let parsed = parser.parse("/unknown arg1").unwrap();

    // Parse succeeds but lookup fails
    let result = registry.get(&parsed.name).await;
    assert!(result.is_err());
}

// ── Full Registry via create_registry() ──────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn test_create_registry_includes_builtins() {
    let registry = shannon_commands::create_registry();
    let names = registry.list_names().await;

    // Should have built-in commands
    assert!(!names.is_empty(), "Built-in registry should not be empty");

    // Should contain common commands
    assert!(names.contains(&"commit".to_string()), "Should have /commit command");
    assert!(names.contains(&"help".to_string()), "Should have /help command");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_create_registry_commands_are_accessible() {
    let registry = shannon_commands::create_registry();

    let commit = registry.get("commit").await;
    assert!(commit.is_ok(), "Should be able to get /commit");

    let cmd = commit.unwrap();
    assert_eq!(cmd.name(), "commit");
    assert!(!cmd.description().is_empty());
}
