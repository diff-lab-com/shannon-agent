//! ReplCommand trait - unified interface for REPL commands
//!
//! This trait provides a consistent interface for commands that can be
//! executed from the REPL, regardless of their internal implementation.

use crate::command::{CommandContext, CommandResult};
use async_trait::async_trait;

/// A command that can be executed from the REPL
///
/// This trait provides a unified interface for commands, making it easier
/// to add new commands and maintain backward compatibility.
#[async_trait]
pub trait ReplCommand: Send + Sync {
    /// Get the command name (e.g., "commit", "help")
    fn name(&self) -> &str;

    /// Get the command description
    fn description(&self) -> &str;

    /// Get command aliases (alternative names)
    fn aliases(&self) -> Vec<&str> {
        Vec::new()
    }

    /// Execute the command with given arguments and context
    async fn execute(
        &self,
        args: &str,
        ctx: &CommandContext,
    ) -> CommandResult<String>;

    /// Get detailed help text for this command
    fn help(&self) -> Option<&str> {
        None
    }

    /// Check if the command is available in the current environment
    fn is_available(&self) -> bool {
        true
    }
}

/// Adapter to wrap existing Command types as ReplCommand
pub struct CommandAdapter {
    /// The underlying command
    pub(crate) inner: crate::Command,
}

impl CommandAdapter {
    /// Create a new adapter from a Command
    pub fn new(command: crate::Command) -> Self {
        Self { inner: command }
    }

    /// Get the inner command
    pub fn inner(&self) -> &crate::Command {
        &self.inner
    }
}

#[async_trait]
impl ReplCommand for CommandAdapter {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn aliases(&self) -> Vec<&str> {
        self.inner
            .aliases()
            .iter()
            .map(|s| s.as_str())
            .collect()
    }

    async fn execute(
        &self,
        _args: &str,
        _ctx: &CommandContext,
    ) -> CommandResult<String> {
        // For now, return a placeholder
        // In a full implementation, this would delegate to the Command's executor
        Ok(format!("Command '{}' executed (placeholder)", self.name()))
    }

    fn help(&self) -> Option<&str> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{CommandBase, PromptCommand, CommandSource, CommandAvailability};

    fn create_test_command(name: &str, description: &str) -> crate::Command {
        crate::Command::Prompt(Box::new(PromptCommand {
            base: CommandBase {
                name: name.to_string(),
                aliases: vec!["test_alias".to_string()],
                description: description.to_string(),
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
            progress_message: "Testing...".to_string(),
            content_length: 0,
            arg_names: vec![],
            allowed_tools: vec![],
            model: None,
            hooks: std::collections::HashMap::new(),
            context: crate::command::ExecutionContext::Inline,
            agent: None,
            paths: vec![],
            prompt_template: None,
        }))
    }

    #[tokio::test]
    async fn test_command_adapter_name() {
        let cmd = create_test_command("test", "A test command");
        let adapter = CommandAdapter::new(cmd);
        assert_eq!(adapter.name(), "test");
    }

    #[tokio::test]
    async fn test_command_adapter_description() {
        let cmd = create_test_command("test", "A test command");
        let adapter = CommandAdapter::new(cmd);
        assert_eq!(adapter.description(), "A test command");
    }

    #[tokio::test]
    async fn test_command_adapter_aliases() {
        let cmd = create_test_command("test", "A test command");
        let adapter = CommandAdapter::new(cmd);
        assert_eq!(adapter.aliases(), vec!["test_alias"]);
    }

    #[tokio::test]
    async fn test_command_adapter_execute() {
        let cmd = create_test_command("test", "A test command");
        let adapter = CommandAdapter::new(cmd);
        let ctx = CommandContext::default();
        let result = adapter.execute("", &ctx).await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("test"));
    }
}
