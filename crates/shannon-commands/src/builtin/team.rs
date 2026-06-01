//! /team command — Team management and coordination
//!
//! Provides a REPL command for creating and managing agent teams.
//! The command generates a prompt that instructs the AI to use the
//! TeamCreate, TeamTaskCreate, Agent, SendMessage, and related tools.

use crate::command::{
    Command, CommandAvailability, CommandBase, CommandSource, ExecutionContext, PromptCommand,
};

const TEAM_PROMPT: &str = r##"
Manage an agent team for parallel work.

Arguments: {args}

Parse the arguments to determine the action:

- **create <name> [description]**: Create a new team.
  Use `TeamCreate({ team_name: "<name>", description: "<description>" })`.

- **list**: Show current teams, members, and task status.
  Use `TeamTaskList()` to show all tasks and their status.

- **spawn <name> [type]**: Spawn a teammate into the current team.
  Use `Agent({ prompt: "You are a teammate. Check TaskList, claim an available task, execute it, mark it completed. Repeat.", team_name: "<current-team>", name: "<name>", subagent_type: "<type>" })`.

- **message <name> <text>**: Send a message to a teammate.
  Use `SendMessage({ to: "<name>", message: "<text>" })`.

- **shutdown**: Gracefully shut down all teammates and clean up.
  Send `SendMessage({ to: "*", message: { "type": "shutdown_request", "reason": "Team work complete" } })` then `TeamDelete()`.

If no subcommand is given, default to showing team status (equivalent to /team list).
"##;

/// Create the /team command
pub fn command() -> Command {
    Command::Prompt(Box::new(PromptCommand {
        base: CommandBase {
            name: "team".to_string(),
            aliases: vec!["teams".to_string()],
            description: "Manage agent teams: create, spawn teammates, assign tasks, coordinate"
                .to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[create|list|spawn|message|shutdown] [args]".to_string()),
            when_to_use: Some(
                "To create or manage agent teams for parallel task execution".to_string(),
            ),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        progress_message: "Managing team...".to_string(),
        content_length: 500,
        arg_names: vec!["subcommand".to_string(), "args".to_string()],
        allowed_tools: vec![
            "TeamCreate".to_string(),
            "TeamDelete".to_string(),
            "TeamTaskCreate".to_string(),
            "TeamTaskUpdate".to_string(),
            "TeamTaskList".to_string(),
            "SendMessage".to_string(),
            "Agent".to_string(),
            "TaskList".to_string(),
            "TaskCreate".to_string(),
            "TaskUpdate".to_string(),
        ],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
        prompt_template: Some(TEAM_PROMPT.to_string()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_team_command() {
        let cmd = command();
        assert_eq!(cmd.name(), "team");
        assert!(cmd.aliases().contains(&"teams".to_string()));
    }

    #[test]
    fn test_team_command_allowed_tools() {
        let cmd = command();
        let prompt = match cmd {
            Command::Prompt(p) => p,
            _ => panic!("Expected PromptCommand"),
        };
        assert!(prompt.allowed_tools.contains(&"TeamCreate".to_string()));
        assert!(prompt.allowed_tools.contains(&"SendMessage".to_string()));
        assert!(prompt.allowed_tools.contains(&"Agent".to_string()));
    }
}
