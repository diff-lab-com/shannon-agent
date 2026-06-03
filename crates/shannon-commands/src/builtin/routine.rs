//! /routine command — Triggered routine management
//!
//! Provides a REPL command for listing, showing, and running triggered routines.
//! Routines are loaded from `.shannon/routines.toml` and `.claude/routines.toml`.

use crate::command::{
    Command, CommandAvailability, CommandBase, CommandSource, ExecutionContext, PromptCommand,
};

const ROUTINE_PROMPT: &str = r##"
Manage triggered routines for automated task execution.

Arguments: {args}

Parse the arguments to determine the action:

- **list**: Show all loaded triggered routines.
  Read `.shannon/routines.toml` and display routines with their trigger events and commands.

- **show [name]**: Display details for a specific routine.
  Show the trigger, matcher, file pattern, command, and enabled status.

- **run <name>**: Manually execute a named routine.
  Run the routine's command and report the result.

- **create <name>**: Create a new routine template.
  Generate a TOML entry in `.shannon/routines.toml` with sensible defaults.

If no subcommand is given, default to listing all routines (equivalent to /routine list).
"##;

/// Create the /routine command
pub fn command() -> Command {
    Command::Prompt(Box::new(PromptCommand {
        base: CommandBase {
            name: "routine".to_string(),
            aliases: vec!["routines".to_string()],
            description: "Manage triggered routines: list, show, run, create".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[list|show|run|create] [name]".to_string()),
            when_to_use: Some(
                "To view or manage automated routines triggered by hook events".to_string(),
            ),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        progress_message: "Loading routines...".to_string(),
        content_length: 600,
        arg_names: vec!["subcommand".to_string(), "name".to_string()],
        allowed_tools: vec![
            "Read".to_string(),
            "Write".to_string(),
            "Glob".to_string(),
            "Grep".to_string(),
            "Bash".to_string(),
        ],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
        prompt_template: Some(ROUTINE_PROMPT.to_string()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_routine_command() {
        let cmd = command();
        assert_eq!(cmd.name(), "routine");
        assert!(cmd.aliases().contains(&"routines".to_string()));
    }

    #[test]
    fn test_routine_command_allowed_tools() {
        let cmd = command();
        let prompt = match cmd {
            Command::Prompt(p) => p,
            _ => panic!("Expected PromptCommand"),
        };
        assert!(prompt.allowed_tools.contains(&"Read".to_string()));
        assert!(prompt.allowed_tools.contains(&"Bash".to_string()));
    }
}
