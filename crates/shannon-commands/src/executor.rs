//! Command executor - executes commands with context

use crate::command::{Command, CommandContext, CommandError, CommandResult, ExecutionResult};
use crate::parser::ParsedCommand;
use crate::registry::CommandRegistry;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Command executor - handles command execution with context
#[derive(Debug)]
pub struct CommandExecutor {
    /// Command registry
    registry: CommandRegistry,

    /// Execution options
    options: ExecutorOptions,
}

/// Options for command execution
#[derive(Debug, Clone)]
pub struct ExecutorOptions {
    /// Allow model invocation
    pub allow_model_invocation: bool,

    /// Require confirmation for sensitive commands
    pub require_confirmation: bool,

    /// Timeout in seconds
    pub timeout_seconds: Option<u64>,

    /// Whether to stream output
    pub stream_output: bool,
}

impl Default for ExecutorOptions {
    fn default() -> Self {
        Self {
            allow_model_invocation: true,
            require_confirmation: true,
            timeout_seconds: Some(300),
            stream_output: false,
        }
    }
}

impl CommandExecutor {
    /// Create a new command executor
    pub fn new(registry: CommandRegistry) -> Self {
        Self {
            registry,
            options: Default::default(),
        }
    }

    /// Create with custom options
    pub fn with_options(registry: CommandRegistry, options: ExecutorOptions) -> Self {
        Self { registry, options }
    }

    /// Get the command registry
    pub fn registry(&self) -> &CommandRegistry {
        &self.registry
    }

    /// Execute a parsed command
    pub async fn execute(
        &self,
        parsed: &ParsedCommand,
        _context: &CommandContext,
    ) -> CommandResult<ExecutionResult> {
        // Get command from registry
        let command = self.registry.get(&parsed.name).await?;

        // Check if enabled
        if !command.is_enabled() {
            return Err(CommandError::NotFound(format!(
                "Command '{}' is disabled",
                parsed.name
            )));
        }

        // Check model invocation permission
        if command.base().disable_model_invocation && !self.options.allow_model_invocation {
            return Err(CommandError::PermissionDenied(
                "Model invocation is disabled for this command".to_string(),
            ));
        }

        // Check for sensitive command confirmation
        if command.base().is_sensitive && self.options.require_confirmation {
            // In a real implementation, this would prompt for confirmation
            // For now, we'll just note it
            tracing::warn!("Executing sensitive command: {}", parsed.name);
        }

        // Execute based on command type
        match &*command {
            Command::Prompt(_) => {
                // For prompt commands, we need to generate the prompt
                // This is handled by the QueryEngine in the main system
                Ok(ExecutionResult::Text {
                    value: format!("Prompt command: {}", parsed.name),
                })
            }
            Command::Local(_) => {
                // Local command execution
                Ok(ExecutionResult::Text {
                    value: format!("Local command: {}", parsed.name),
                })
            }
            Command::LocalJSX(_) => {
                // Local JSX command with UI
                Ok(ExecutionResult::Text {
                    value: format!("UI command: {}", parsed.name),
                })
            }
        }
    }

    /// Execute a command string
    pub async fn execute_string(
        &self,
        input: &str,
        context: &CommandContext,
    ) -> CommandResult<Vec<ExecutionResult>> {
        let parser = crate::parser::CommandParser::new();
        let parsed_commands = parser.parse_multiple(input)?;

        let mut results = vec![];
        for parsed in parsed_commands {
            let result = self.execute(&parsed, context).await?;
            results.push(result);
        }

        Ok(results)
    }

    /// Get prompt for a prompt command
    pub async fn get_prompt(
        &self,
        command_name: &str,
        args: &str,
        _context: &CommandContext,
    ) -> CommandResult<String> {
        let command = self.registry.get(command_name).await?;

        match &*command {
            Command::Prompt(cmd) => {
                if let Some(ref template) = cmd.prompt_template {
                    Ok(template.replace("{args}", args))
                } else {
                    Ok(format!("Execute the /{command_name} command with args: '{args}'"))
                }
            }
            _ => Err(CommandError::ExecutionError(
                "Command is not a prompt command".to_string(),
            )),
        }
    }

    /// Set execution options
    pub fn set_options(&mut self, options: ExecutorOptions) {
        self.options = options;
    }
}

/// Shared executor state for concurrent access
///
/// Wraps `CommandExecutor` in `Arc<RwLock<>>` for safe concurrent use
/// from multiple tasks (e.g., REPL input + background commands).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct SharedExecutor {
    inner: Arc<RwLock<CommandExecutor>>,
}

#[allow(dead_code)]
impl SharedExecutor {
    /// Create a new shared executor
    pub fn new(executor: CommandExecutor) -> Self {
        Self {
            inner: Arc::new(RwLock::new(executor)),
        }
    }

    /// Execute a command
    pub async fn execute(
        &self,
        parsed: &ParsedCommand,
        _context: &CommandContext,
    ) -> CommandResult<ExecutionResult> {
        let executor = self.inner.read().await;
        executor.execute(parsed, _context).await
    }

    /// Execute a command string
    pub async fn execute_string(
        &self,
        input: &str,
        _context: &CommandContext,
    ) -> CommandResult<Vec<ExecutionResult>> {
        let executor = self.inner.read().await;
        executor.execute_string(input, _context).await
    }

    /// Get the registry
    pub async fn registry(&self) -> CommandRegistry {
        let executor = self.inner.read().await;
        executor.registry().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    

    fn create_test_registry() -> CommandRegistry {
        
        // Commands would be registered here
        CommandRegistry::new()
    }

    #[tokio::test]
    async fn test_execute_nonexistent() {
        let executor = CommandExecutor::new(create_test_registry());
        let parsed = ParsedCommand::new("nonexistent".to_string(), "".to_string(), "/nonexistent".to_string());
        let context = CommandContext::default();

        let result = executor.execute(&parsed, &context).await;
        assert!(matches!(result, Err(CommandError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_execute_string() {
        let executor = CommandExecutor::new(create_test_registry());
        let context = CommandContext::default();

        // Empty input should error
        let result = executor.execute_string("", &context).await;
        assert!(result.is_err());
    }
}
