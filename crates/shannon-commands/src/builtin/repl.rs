//! REPL-specific commands
//!
//! These commands interact directly with the REPL UI and state.
//! They are registered in the CommandRegistry for discovery and completion,
//! but their execution is handled by the REPL itself.

use crate::command::{Command, CommandBase, CommandSource, LocalCommand, CommandAvailability};

/// /clear command - Clear chat history
pub fn clear_command() -> Command {
    Command::Local(LocalCommand {
        base: CommandBase {
            name: "clear".to_string(),
            aliases: vec!["cls".to_string()],
            description: "Clear chat history".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: None,
            when_to_use: Some("Use to clear the chat history when the screen gets cluttered".to_string()),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        supports_non_interactive: false,
    })
}

/// /quit command - Exit Shannon
pub fn quit_command() -> Command {
    Command::Local(LocalCommand {
        base: CommandBase {
            name: "quit".to_string(),
            aliases: vec!["exit".to_string(), "q".to_string()],
            description: "Exit Shannon REPL".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: None,
            when_to_use: Some("Use to exit the Shannon REPL".to_string()),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        supports_non_interactive: false,
    })
}

/// /model command - Show or set the AI model
pub fn model_command() -> Command {
    Command::Local(LocalCommand {
        base: CommandBase {
            name: "model".to_string(),
            aliases: vec!["models".to_string()],
            description: "Show or set the AI model".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[model name]".to_string()),
            when_to_use: Some("Use to change the AI model or see the current model".to_string()),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        supports_non_interactive: false,
    })
}

/// /init command - Initialize project configuration
pub fn init_command() -> Command {
    Command::Local(LocalCommand {
        base: CommandBase {
            name: "init".to_string(),
            aliases: vec!["initialize".to_string()],
            description: "Initialize project configuration (CLAUDE.md, detect git)".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: None,
            when_to_use: Some("Use when starting work in a new project directory".to_string()),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        supports_non_interactive: false,
    })
}

/// /sessions command - List saved sessions
pub fn sessions_command() -> Command {
    Command::Local(LocalCommand {
        base: CommandBase {
            name: "sessions".to_string(),
            aliases: vec!["list-sessions".to_string()],
            description: "List saved sessions".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[--all] [--search <query>]".to_string()),
            when_to_use: Some("Use to see previously saved sessions that can be resumed".to_string()),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        supports_non_interactive: false,
    })
}

/// /resume command - Resume a saved session
pub fn resume_command() -> Command {
    Command::Local(LocalCommand {
        base: CommandBase {
            name: "resume".to_string(),
            aliases: vec!["restore".to_string()],
            description: "Resume a saved session by UUID or number".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("<number-or-uuid>".to_string()),
            when_to_use: Some("Use to continue a previous session".to_string()),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        supports_non_interactive: false,
    })
}

/// /history command - Show session stats or export
pub fn history_command() -> Command {
    Command::Local(LocalCommand {
        base: CommandBase {
            name: "history".to_string(),
            aliases: vec!["stats".to_string()],
            description: "Show current session stats or export to file".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[--export <path>]".to_string()),
            when_to_use: Some("Use to see session statistics or export the conversation".to_string()),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        supports_non_interactive: false,
    })
}

/// /worktree command - Manage git worktrees
pub fn worktree_command() -> Command {
    Command::Local(LocalCommand {
        base: CommandBase {
            name: "worktree".to_string(),
            aliases: vec![],
            description: "Manage git worktrees (enter, exit, status)".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[enter <name>|exit [--keep|--remove]|status]".to_string()),
            when_to_use: Some("Use to work in isolated git branches using worktrees".to_string()),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        supports_non_interactive: false,
    })
}

/// /branch command - Create a branch from an existing session
pub fn branch_command() -> Command {
    Command::Local(LocalCommand {
        base: CommandBase {
            name: "branch".to_string(),
            aliases: vec!["fork".to_string()],
            description: "Create a branch from an existing session".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("<session-id-or-number> [message-index]".to_string()),
            when_to_use: Some(
                "Use to fork a conversation from a specific point, creating a new session".to_string(),
            ),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        supports_non_interactive: false,
    })
}

/// Get all REPL-specific commands
pub fn all_commands() -> Vec<Command> {
    vec![
        clear_command(),
        quit_command(),
        model_command(),
        init_command(),
        sessions_command(),
        resume_command(),
        history_command(),
        worktree_command(),
        branch_command(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clear_command() {
        let cmd = clear_command();
        assert_eq!(cmd.name(), "clear");
        assert!(cmd.aliases().contains(&"cls".to_string()));
    }

    #[test]
    fn test_quit_command_aliases() {
        let cmd = quit_command();
        assert_eq!(cmd.name(), "quit");
        assert!(cmd.aliases().contains(&"exit".to_string()));
        assert!(cmd.aliases().contains(&"q".to_string()));
    }

    #[test]
    fn test_model_command() {
        let cmd = model_command();
        assert_eq!(cmd.name(), "model");
        assert!(cmd.argument_hint().is_some());
    }

    #[test]
    fn test_all_commands_count() {
        let cmds = all_commands();
        assert_eq!(cmds.len(), 9);
    }
}
