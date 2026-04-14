//! Command registry for command registration and lookup

use crate::command::{Command, CommandError, CommandResult};
use crate::context::CommandContext;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Command registry - central registry for all commands
#[derive(Debug)]
pub struct CommandRegistry {
    /// Map of command name to command
    commands: Arc<RwLock<HashMap<String, Arc<Command>>>>,

    /// Map of aliases to command names
    aliases: Arc<RwLock<HashMap<String, String>>>,
}

impl CommandRegistry {
    /// Create a new empty command registry
    pub fn new() -> Self {
        Self {
            commands: Arc::new(RwLock::new(HashMap::new())),
            aliases: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a command (blocking — uses block_in_place, safe inside tokio)
    ///
    /// Use this from sync contexts or from within a tokio runtime.
    /// Uses `tokio::task::block_in_place` to avoid deadlocking.
    pub fn register_sync(&self, command: Command) {
        let name = command.name().to_string();
        let cmd_aliases = command.aliases().to_vec();

        let commands = Arc::clone(&self.commands);
        let aliases = Arc::clone(&self.aliases);

        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                commands
                    .write()
                    .await
                    .insert(name.clone(), Arc::new(command.clone()));

                let mut aliases_map = aliases.write().await;
                for alias in cmd_aliases {
                    aliases_map.insert(alias, name.clone());
                }
            });
        });
    }

    /// Register a command
    pub async fn register(&self, command: Command) -> CommandResult<()> {
        let name = command.name().to_string();
        let aliases = command.aliases().to_vec();

        // Store the command
        self.commands
            .write()
            .await
            .insert(name.clone(), Arc::new(command.clone()));

        // Register aliases
        let mut aliases_map = self.aliases.write().await;
        for alias in aliases {
            aliases_map.insert(alias, name.clone());
        }

        Ok(())
    }

    /// Register multiple commands
    pub async fn register_all(&self, commands: Vec<Command>) -> CommandResult<()> {
        for command in commands {
            self.register(command).await?;
        }
        Ok(())
    }

    /// Get a command by name
    pub async fn get(&self, name: &str) -> CommandResult<Arc<Command>> {
        // Check direct name first
        let commands = self.commands.read().await;
        if let Some(cmd) = commands.get(name) {
            return Ok(Arc::clone(cmd));
        }
        drop(commands);

        // Check aliases
        let aliases = self.aliases.read().await;
        if let Some(actual_name) = aliases.get(name).cloned() {
            drop(aliases);
            let commands = self.commands.read().await;
            if let Some(cmd) = commands.get(&actual_name) {
                return Ok(Arc::clone(cmd));
            }
        }

        Err(CommandError::NotFound(name.to_string()))
    }

    /// List all registered command names
    pub async fn list_names(&self) -> Vec<String> {
        self.commands
            .read()
            .await
            .keys()
            .cloned()
            .collect()
    }

    /// List all enabled commands
    pub async fn list_enabled(&self) -> Vec<Arc<Command>> {
        self.commands
            .read()
            .await
            .values()
            .filter(|cmd| cmd.is_enabled())
            .cloned()
            .collect()
    }

    /// List visible (non-hidden) commands
    pub async fn list_visible(&self) -> Vec<Arc<Command>> {
        self.commands
            .read()
            .await
            .values()
            .filter(|cmd| cmd.is_enabled() && !cmd.is_hidden())
            .cloned()
            .collect()
    }

    /// Remove a command by name
    pub async fn unregister(&self, name: &str) -> CommandResult<()> {
        let commands = self.commands.read().await;
        let _cmd = commands.get(name).cloned();
        drop(commands);

        let commands = self.commands.read().await;
        if commands.get(name).is_some() {
            // Remove aliases
            let mut aliases = self.aliases.write().await;
            aliases.retain(|_, v| v != name);

            // Remove command
            self.commands.write().await.remove(name);
            Ok(())
        } else {
            Err(CommandError::NotFound(name.to_string()))
        }
    }

    /// Check if a command exists
    pub async fn contains(&self, name: &str) -> bool {
        let commands = self.commands.read().await;
        commands.contains_key(name) || self.aliases.read().await.contains_key(name)
    }

    /// Get command count
    pub async fn count(&self) -> usize {
        self.commands.read().await.len()
    }

    /// Clear all commands
    pub async fn clear(&self) {
        self.commands.write().await.clear();
        self.aliases.write().await.clear();
    }

    /// Find commands matching a pattern
    pub async fn search(&self, pattern: &str) -> Vec<Arc<Command>> {
        let commands = self.commands.read().await;
        let pattern_lower = pattern.to_lowercase();

        commands
            .values()
            .filter(|cmd| {
                let name_matches = cmd.name().to_lowercase().contains(&pattern_lower);
                let desc_matches = cmd
                    .description()
                    .to_lowercase()
                    .contains(&pattern_lower);
                let alias_matches = cmd
                    .aliases()
                    .iter()
                    .any(|a| a.to_lowercase().contains(&pattern_lower));

                name_matches || desc_matches || alias_matches
            })
            .cloned()
            .collect()
    }

    /// Dispatch a command by name with arguments and context
    ///
    /// This provides a convenient way to look up and execute a command.
    /// Returns an error if the command is not found or if execution fails.
    pub async fn dispatch(
        &self,
        name: &str,
        args: &str,
        _context: &CommandContext,
    ) -> CommandResult<String> {
        let _command = self.get(name).await?;
        // For now, return a simple success message
        // In a full implementation, this would execute the command
        Ok(format!(
            "Command '{name}' dispatched with args: '{args}'"
        ))
    }

    /// Generate help text for all commands or a specific command
    ///
    /// If `command_name` is Some, returns help for that specific command.
    /// If None, returns a summary of all available commands.
    pub async fn get_help(&self, command_name: Option<&str>) -> String {
        if let Some(name) = command_name {
            // Get help for a specific command
            match self.get(name).await {
                Ok(command) => {
                    let mut help = format!("## /{}", command.name());

                    if let Some(hint) = command.argument_hint() {
                        help.push_str(&format!(" `{hint}`"));
                    }

                    help.push_str(&format!("\n\n{}\n", command.description()));

                    if !command.aliases().is_empty() {
                        help.push_str(&format!(
                            "\n**Aliases:** {}\n",
                            command.aliases().join(", ")
                        ));
                    }

                    if let Some(when) = command.base().when_to_use.as_ref() {
                        help.push_str(&format!("\n**When to use:** {when}\n"));
                    }

                    help
                }
                Err(_) => format!("No help found for command: {name}"),
            }
        } else {
            // Generate summary help for all commands
            let mut output = String::from("# Available Commands\n\n");

            let commands = self.list_visible().await;
            let mut sorted = commands;
            sorted.sort_by(|a, b| a.name().cmp(b.name()));

            for command in sorted {
                let aliases = if command.aliases().is_empty() {
                    String::new()
                } else {
                    format!(" ({})", command.aliases().join(", "))
                };
                let hint = command
                    .argument_hint()
                    .map(|h| format!(" {h}"))
                    .unwrap_or_default();

                output.push_str(&format!(
                    "- **/{}{}**{} — {}\n",
                    command.name(),
                    aliases,
                    hint,
                    command.description()
                ));
            }

            output.push_str("\nUse `/help <command>` for detailed information about a specific command.\n");
            output
        }
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Clone the registry (creates new instances sharing the same storage)
impl Clone for CommandRegistry {
    fn clone(&self) -> Self {
        Self {
            commands: Arc::clone(&self.commands),
            aliases: Arc::clone(&self.aliases),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{CommandBase, PromptCommand};

    fn create_test_command(name: &str, description: &str) -> Command {
        Command::Prompt(Box::new(PromptCommand {
            base: CommandBase {
                name: name.to_string(),
                aliases: vec![],
                description: description.to_string(),
                has_user_specified_description: false,
                availability: vec![],
                source: crate::command::CommandSource::Builtin,
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
    async fn test_register_and_get() {
        let registry = CommandRegistry::new();
        let cmd = create_test_command("test", "A test command");

        registry.register(cmd).await.unwrap();

        let retrieved = registry.get("test").await.unwrap();
        assert_eq!(retrieved.name(), "test");
        assert_eq!(retrieved.description(), "A test command");
    }

    #[tokio::test]
    async fn test_not_found() {
        let registry = CommandRegistry::new();
        let result = registry.get("nonexistent").await;
        assert!(matches!(result, Err(CommandError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_list_commands() {
        let registry = CommandRegistry::new();
        registry
            .register(create_test_command("cmd1", "Command 1"))
            .await
            .unwrap();
        registry
            .register(create_test_command("cmd2", "Command 2"))
            .await
            .unwrap();

        let names = registry.list_names().await;
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"cmd1".to_string()));
        assert!(names.contains(&"cmd2".to_string()));
    }

    #[tokio::test]
    async fn test_search() {
        let registry = CommandRegistry::new();
        registry
            .register(create_test_command("commit", "Commit changes"))
            .await
            .unwrap();
        registry
            .register(create_test_command("status", "Show status"))
            .await
            .unwrap();

        let results = registry.search("commit").await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name(), "commit");
    }
}
