//! ReplCommand trait - unified interface for REPL commands
//!
//! This trait provides a consistent interface for commands that can be
//! executed from the REPL, regardless of their internal implementation.

use crate::command::{Command, CommandContext, CommandError, CommandResult};
use async_trait::async_trait;
use std::path::Path;
use std::time::Duration;

/// Default timeout for shell command execution (seconds)
const LOCAL_COMMAND_TIMEOUT_SECS: u64 = 30;

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
        args: &str,
        ctx: &CommandContext,
    ) -> CommandResult<String> {
        match &self.inner {
            Command::Prompt(cmd) => {
                // Prompt commands render their template and return it for AI processing
                if let Some(ref template) = cmd.prompt_template {
                    Ok(template.replace("{args}", args))
                } else {
                    Ok(format!(
                        "Execute the /{} command with args: '{}'",
                        self.name(),
                        args
                    ))
                }
            }
            Command::Local(_) => {
                // Local commands may carry shell args to execute;
                // if args are provided, run them as a subprocess.
                let trimmed = args.trim();
                if trimmed.is_empty() {
                    // No shell args — the REPL handles the local command itself
                    Ok(format!("Local command '{}' acknowledged", self.name()))
                } else {
                    execute_shell_command(trimmed, &ctx.cwd, LOCAL_COMMAND_TIMEOUT_SECS).await
                }
            }
            Command::LocalJSX(_) => {
                let trimmed = args.trim();
                if trimmed.is_empty() {
                    Ok(format!("UI command '{}' acknowledged", self.name()))
                } else {
                    execute_shell_command(trimmed, &ctx.cwd, LOCAL_COMMAND_TIMEOUT_SECS).await
                }
            }
        }
    }

    fn help(&self) -> Option<&str> {
        None
    }
}

/// Execute a shell command via the system shell with timeout and output capture.
///
/// Uses `sh -c` on Unix to support pipelines, redirects, and other shell
/// features.  Stdout and stderr are captured and returned on success.  On a
/// non-zero exit code the captured stderr is surfaced as an `ExecutionError`.
async fn execute_shell_command(
    command: &str,
    cwd: &Path,
    timeout_secs: u64,
) -> CommandResult<String> {
    let output = tokio::time::timeout(
        Duration::from_secs(timeout_secs),
        tokio::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(cwd)
            .output(),
    )
    .await
    .map_err(|_| {
        CommandError::ExecutionError(format!(
            "Command timed out after {timeout_secs}s: {command}"
        ))
    })?
    .map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            CommandError::NotFound(format!(
                "Shell not found when executing: {command}"
            ))
        } else {
            CommandError::ExecutionError(format!(
                "Failed to execute command '{command}': {e}"
            ))
        }
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.success() {
        let stdout_trimmed = stdout.trim();
        if stdout_trimmed.is_empty() && stderr.trim().is_empty() {
            Ok("Command completed successfully (no output)".to_string())
        } else if stderr.trim().is_empty() {
            Ok(stdout_trimmed.to_string())
        } else {
            Ok(format!("{stdout_trimmed}\n{stderr}", stderr = stderr.trim()))
        }
    } else {
        let code = output
            .status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        Err(CommandError::ExecutionError(format!(
            "Command exited with code {code}: {stderr}",
            stderr = stderr.trim()
        )))
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

    #[tokio::test]
    async fn test_prompt_command_with_template() {
        let mut cmd = create_test_command("greet", "A greeting command");
        if let crate::Command::Prompt(ref mut p) = cmd {
            p.prompt_template = Some("Hello, {args}!".to_string());
        }
        let adapter = CommandAdapter::new(cmd);
        let ctx = CommandContext::default();
        let result = adapter.execute("World", &ctx).await;
        assert_eq!(result.unwrap(), "Hello, World!");
    }

    #[tokio::test]
    async fn test_prompt_command_without_template() {
        let cmd = create_test_command("foo", "Foo command");
        let adapter = CommandAdapter::new(cmd);
        let ctx = CommandContext::default();
        let result = adapter.execute("bar baz", &ctx).await;
        let output = result.unwrap();
        assert!(output.contains("foo"));
        assert!(output.contains("bar baz"));
    }

    #[tokio::test]
    async fn test_local_command_no_args() {
        let cmd = crate::Command::Local(crate::command::LocalCommand {
            base: CommandBase {
                name: "clear".to_string(),
                aliases: vec![],
                description: "Clear screen".to_string(),
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
            supports_non_interactive: false,
        });
        let adapter = CommandAdapter::new(cmd);
        let ctx = CommandContext::default();
        let result = adapter.execute("", &ctx).await;
        let output = result.unwrap();
        assert!(output.contains("clear"));
        assert!(output.contains("acknowledged"));
    }

    #[tokio::test]
    async fn test_shell_command_success() {
        let result =
            execute_shell_command("echo hello", std::path::Path::new("."), 5).await;
        assert_eq!(result.unwrap(), "hello");
    }

    #[tokio::test]
    async fn test_shell_command_failure() {
        let result =
            execute_shell_command("false", std::path::Path::new("."), 5).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("exited with code"));
    }

    #[tokio::test]
    async fn test_shell_command_timeout() {
        // Sleep longer than the timeout to trigger a timeout error
        let result =
            execute_shell_command("sleep 10", std::path::Path::new("."), 1).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("timed out"));
    }

    #[tokio::test]
    async fn test_shell_command_stderr_captured() {
        let result =
            execute_shell_command("echo err >&2", std::path::Path::new("."), 5).await;
        // stderr-only output should still be captured
        assert!(result.is_ok());
        assert!(result.unwrap().contains("err"));
    }

    #[tokio::test]
    async fn test_shell_command_no_output() {
        let result =
            execute_shell_command("true", std::path::Path::new("."), 5).await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("no output"));
    }
}
