//! E2E tests for the /routine command.
//!
//! Tests command properties, registry integration, and template rendering.

use shannon_commands::Command;

/// Get the /routine command from the registered commands.
fn routine_command() -> Command {
    shannon_commands::builtin_commands::all_commands()
        .into_iter()
        .find(|c| c.name() == "routine")
        .expect("/routine should be registered")
}

// ============================================================================
// Command Property Tests
// ============================================================================

#[test]
fn test_routine_command_name() {
    let cmd = routine_command();
    assert_eq!(cmd.name(), "routine");
}

#[test]
fn test_routine_command_aliases() {
    let cmd = routine_command();
    assert!(cmd.aliases().contains(&"routines".to_string()));
}

#[test]
fn test_routine_command_description() {
    let cmd = routine_command();
    let desc = cmd.description();
    assert!(!desc.is_empty());
    assert!(
        desc.contains("routine"),
        "Description should mention routines"
    );
}

#[test]
fn test_routine_command_allowed_tools() {
    let cmd = routine_command();
    let prompt = match cmd {
        Command::Prompt(p) => p,
        _ => panic!("Expected PromptCommand"),
    };
    assert!(prompt.allowed_tools.contains(&"Read".to_string()));
    assert!(prompt.allowed_tools.contains(&"Bash".to_string()));
    assert!(prompt.allowed_tools.contains(&"Glob".to_string()));
    assert!(prompt.allowed_tools.contains(&"Grep".to_string()));
    assert!(prompt.allowed_tools.contains(&"Write".to_string()));
}

#[test]
fn test_routine_command_prompt_template() {
    let cmd = routine_command();
    let prompt = match cmd {
        Command::Prompt(p) => p,
        _ => panic!("Expected PromptCommand"),
    };
    let template = prompt
        .prompt_template
        .as_deref()
        .expect("Should have template");
    assert!(template.contains("list"));
    assert!(template.contains("show"));
    assert!(template.contains("run"));
    assert!(template.contains("create"));
    assert!(template.contains("{args}"));
}

#[test]
fn test_routine_command_argument_hint() {
    let cmd = routine_command();
    let prompt = match cmd {
        Command::Prompt(p) => p,
        _ => panic!("Expected PromptCommand"),
    };
    assert!(prompt.base.argument_hint.is_some());
    let hint = prompt.base.argument_hint.unwrap();
    assert!(hint.contains("list") || hint.contains("show") || hint.contains("run"));
}

// ============================================================================
// Registry Integration Tests
// ============================================================================

#[tokio::test]
async fn test_routine_registered_in_all_commands() {
    let commands = shannon_commands::builtin_commands::all_commands();
    let routine_names: Vec<&str> = commands
        .iter()
        .filter(|c| c.name() == "routine" || c.aliases().contains(&"routines".to_string()))
        .map(|c| c.name())
        .collect();
    assert!(
        !routine_names.is_empty(),
        "/routine should be in all_commands()"
    );
}

#[tokio::test]
async fn test_routine_alias_in_all_commands() {
    let commands = shannon_commands::builtin_commands::all_commands();
    let has_alias = commands
        .iter()
        .any(|c| c.aliases().contains(&"routines".to_string()));
    assert!(has_alias, "Some command should have 'routines' alias");
}

#[tokio::test]
async fn test_routine_command_in_registry() {
    let registry = shannon_commands::CommandRegistry::new();
    registry.register(routine_command()).await.unwrap();
    let retrieved = registry.get("routine").await.unwrap();
    assert_eq!(retrieved.name(), "routine");
}

#[tokio::test]
async fn test_routine_alias_lookup() {
    let registry = shannon_commands::CommandRegistry::new();
    registry.register(routine_command()).await.unwrap();
    let retrieved = registry.get("routines").await.unwrap();
    assert_eq!(retrieved.name(), "routine");
}

// ============================================================================
// Template Rendering Tests
// ============================================================================

#[test]
fn test_routine_template_renders_list() {
    let cmd = routine_command();
    let prompt = match cmd {
        Command::Prompt(p) => p,
        _ => panic!("Expected PromptCommand"),
    };
    let rendered = prompt
        .prompt_template
        .as_deref()
        .unwrap()
        .replace("{args}", "list");
    assert!(rendered.contains("list"));
}

#[test]
fn test_routine_template_renders_show() {
    let cmd = routine_command();
    let prompt = match cmd {
        Command::Prompt(p) => p,
        _ => panic!("Expected PromptCommand"),
    };
    let rendered = prompt
        .prompt_template
        .as_deref()
        .unwrap()
        .replace("{args}", "show post-edit-lint");
    assert!(rendered.contains("post-edit-lint"));
}

#[test]
fn test_routine_template_renders_run() {
    let cmd = routine_command();
    let prompt = match cmd {
        Command::Prompt(p) => p,
        _ => panic!("Expected PromptCommand"),
    };
    let rendered = prompt
        .prompt_template
        .as_deref()
        .unwrap()
        .replace("{args}", "run post-edit-lint");
    assert!(rendered.contains("run"));
}

#[test]
fn test_routine_template_renders_create() {
    let cmd = routine_command();
    let prompt = match cmd {
        Command::Prompt(p) => p,
        _ => panic!("Expected PromptCommand"),
    };
    let rendered = prompt
        .prompt_template
        .as_deref()
        .unwrap()
        .replace("{args}", "create my-routine");
    assert!(rendered.contains("create"));
}

#[test]
fn test_routine_template_empty_args() {
    let cmd = routine_command();
    let prompt = match cmd {
        Command::Prompt(p) => p,
        _ => panic!("Expected PromptCommand"),
    };
    let rendered = prompt
        .prompt_template
        .as_deref()
        .unwrap()
        .replace("{args}", "");
    assert!(!rendered.contains("{args}"));
}
