//! /profile command — Permission profile management
//!
//! Provides a REPL command for listing, showing, and switching permission profiles.
//! Built-in profiles: strict, balanced, permissive.
//! Custom profiles loaded from `.shannon/profiles/*.toml` and `.claude/profiles/*.toml`.

use crate::command::{
    Command, CommandAvailability, CommandBase, CommandSource, ExecutionContext, PromptCommand,
};

const PROFILE_PROMPT: &str = r##"
Manage permission profiles for tool access control.

Arguments: {args}

Parse the arguments to determine the action:

- **list**: Show all available profiles (built-in + custom).
  List the three built-in profiles (strict, balanced, permissive) and any custom profiles from `.shannon/profiles/*.toml`.

- **show [name]**: Display the rules for a profile.
  If no name given, show the current active profile.

- **set <name>**: Switch to a named profile.
  Valid built-in names: `strict`, `balanced`, `permissive`.
  Custom profiles can also be referenced by name.
  This changes which tools are auto-approved, need confirmation, or are denied.

- **create <name>**: Create a new custom profile.
  Generate a template TOML file at `.shannon/profiles/{name}.toml` with sensible defaults.

If no subcommand is given, default to showing the current profile (equivalent to /profile show).
"##;

/// Create the /profile command
pub fn command() -> Command {
    Command::Prompt(Box::new(PromptCommand {
        base: CommandBase {
            name: "profile".to_string(),
            aliases: vec!["profiles".to_string(), "perm-profile".to_string()],
            description: "Manage permission profiles: list, show, set, create".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[list|show|set|create] [name]".to_string()),
            when_to_use: Some(
                "To view or switch permission profiles that control tool access".to_string(),
            ),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        progress_message: "Loading profiles...".to_string(),
        content_length: 600,
        arg_names: vec!["subcommand".to_string(), "name".to_string()],
        allowed_tools: vec![
            "Read".to_string(),
            "Write".to_string(),
            "Glob".to_string(),
            "Grep".to_string(),
        ],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
        prompt_template: Some(PROFILE_PROMPT.to_string()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_command() {
        let cmd = command();
        assert_eq!(cmd.name(), "profile");
        assert!(cmd.aliases().contains(&"profiles".to_string()));
    }

    #[test]
    fn test_profile_command_allowed_tools() {
        let cmd = command();
        let prompt = match cmd {
            Command::Prompt(p) => p,
            _ => panic!("Expected PromptCommand"),
        };
        assert!(prompt.allowed_tools.contains(&"Read".to_string()));
        assert!(prompt.allowed_tools.contains(&"Write".to_string()));
    }
}
