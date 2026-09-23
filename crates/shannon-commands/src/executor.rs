//! Command executor — the REPL's shared handle on the [`CommandRegistry`].
//!
//! Review §P3-22: this type used to also expose an `execute()` path that was
//! a stub — it returned placeholder strings ("Local command: x") and reduced
//! sensitive-command confirmation to a log line. Nothing in the workspace
//! called it:
//!
//! - Real slash-command dispatch lives in the REPL command handlers
//!   (`shannon-ui::repl::commands::handle_other_command`), which read the
//!   same [`CommandRegistry`](crate::registry::CommandRegistry) directly.
//! - Real shell execution lives in `shannon-tools::system::BashTool`, gated
//!   by the engine's permission/approval flow (`is_destructive` tools go
//!   through the approval-request sink); re-implementing it here would have
//!   duplicated that machinery and bypassed the approval channel.
//! - Real prompt-template expansion already exists as
//!   [`CommandExecutor::get_prompt`].
//!
//! The misleading stub was therefore removed instead of half-implemented.
//! What remains is the live half: a concurrent, shareable wrapper around the
//! registry ([`SharedExecutor`]) plus the prompt-expansion helper.

use crate::command::{Command, CommandContext, CommandError, CommandResult};
use crate::registry::CommandRegistry;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Command executor — shared access to the command registry plus prompt
/// expansion for prompt-type commands.
#[derive(Debug)]
pub struct CommandExecutor {
    /// Command registry
    registry: CommandRegistry,
}

impl CommandExecutor {
    /// Create a new command executor over the given registry.
    pub fn new(registry: CommandRegistry) -> Self {
        Self { registry }
    }

    /// Get the command registry
    pub fn registry(&self) -> &CommandRegistry {
        &self.registry
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
                    Ok(format!(
                        "Execute the /{command_name} command with args: '{args}'"
                    ))
                }
            }
            _ => Err(CommandError::ExecutionError(
                "Command is not a prompt command".to_string(),
            )),
        }
    }
}

/// Shared executor state for concurrent access
///
/// Wraps `CommandExecutor` in `Arc<RwLock<>>` for safe concurrent use
/// from multiple tasks (e.g., REPL input + background commands).
#[derive(Debug, Clone)]
pub struct SharedExecutor {
    inner: Arc<RwLock<CommandExecutor>>,
}

impl SharedExecutor {
    /// Create a new shared executor
    pub fn new(executor: CommandExecutor) -> Self {
        Self {
            inner: Arc::new(RwLock::new(executor)),
        }
    }

    /// Get the registry
    pub async fn registry(&self) -> CommandRegistry {
        let executor = self.inner.read().await;
        executor.registry().clone()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::command::{CommandBase, CommandSource, PromptCommand};
    use std::collections::HashMap;

    fn base_command(name: &str) -> CommandBase {
        CommandBase {
            name: name.to_string(),
            aliases: vec![],
            description: format!("{name} test command"),
            has_user_specified_description: false,
            availability: vec![],
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
        }
    }

    fn prompt_command(name: &str, template: Option<&str>) -> Command {
        Command::Prompt(Box::new(PromptCommand {
            base: base_command(name),
            progress_message: String::new(),
            content_length: 0,
            arg_names: vec![],
            allowed_tools: vec![],
            model: None,
            hooks: HashMap::new(),
            context: crate::command::ExecutionContext::Inline,
            agent: None,
            paths: vec![],
            prompt_template: template.map(str::to_string),
        }))
    }

    #[tokio::test]
    async fn registry_lookup_nonexistent_is_not_found() {
        let executor = CommandExecutor::new(CommandRegistry::new());
        let result = executor.registry().get("nonexistent").await;
        assert!(matches!(result, Err(CommandError::NotFound(_))));
    }

    #[tokio::test]
    async fn get_prompt_expands_args_placeholder() {
        let registry = CommandRegistry::new();
        registry
            .register(prompt_command("greet", Some("Say hello to {args}!")))
            .await
            .unwrap();
        let executor = CommandExecutor::new(registry);
        let context = CommandContext::default();

        let prompt = executor
            .get_prompt("greet", "world", &context)
            .await
            .unwrap();
        assert_eq!(prompt, "Say hello to world!");
    }

    #[tokio::test]
    async fn get_prompt_falls_back_without_template() {
        let registry = CommandRegistry::new();
        registry
            .register(prompt_command("plain", None))
            .await
            .unwrap();
        let executor = CommandExecutor::new(registry);
        let context = CommandContext::default();

        let prompt = executor
            .get_prompt("plain", "some args", &context)
            .await
            .unwrap();
        assert!(prompt.contains("/plain"), "got: {prompt}");
        assert!(prompt.contains("some args"), "got: {prompt}");
    }

    #[tokio::test]
    async fn get_prompt_rejects_non_prompt_command() {
        let registry = CommandRegistry::new();
        registry
            .register(Command::Local(crate::command::LocalCommand {
                base: base_command("localcmd"),
                supports_non_interactive: false,
            }))
            .await
            .unwrap();
        let executor = CommandExecutor::new(registry);
        let context = CommandContext::default();

        let result = executor.get_prompt("localcmd", "", &context).await;
        assert!(matches!(result, Err(CommandError::ExecutionError(_))));
    }

    // `register_sync` takes a blocking write lock — needs the multi-thread
    // flavor.
    #[tokio::test(flavor = "multi_thread")]
    async fn shared_executor_exposes_the_registry() {
        let mut registry = CommandRegistry::new();
        registry.register_sync(prompt_command("shared", Some("v: {args}")));
        let shared = SharedExecutor::new(CommandExecutor::new(registry));

        let handle = shared.registry().await;
        let command = handle.get("shared").await.unwrap();
        assert_eq!(command.name(), "shared");
    }
}
