// shannon-commands
//
// Command system for Shannon Code, inspired by Claude Code's command architecture.
//
// ## Architecture
//
// The command system consists of:
// - **Registry**: Central command registration and lookup
// - **Parser**: Argument parsing and validation
// - **Executor**: Command execution with context
// - **Built-in commands**: Core commands (commit, review-pr, pdf, etc.)
//
// ## Command Types
//
// - **PromptCommand**: Commands that generate prompts for AI processing
// - **LocalCommand**: Commands executed locally without AI
// - **LocalJSXCommand**: Commands with rich UI (TUI components)

mod registry;
mod parser;
mod executor;
mod command;
mod context;

mod builtin;

pub use command::{
    Command, CommandBase, PromptCommand, LocalCommand, LocalJSXCommand,
    CommandResult, CommandAvailability, CommandSource,
};
pub use registry::CommandRegistry;
pub use parser::{CommandParser, ParsedCommand};
pub use command::ExecutionResult;
pub use executor::CommandExecutor;
pub use context::{CommandContext, ToolUseContext};

/// Re-export built-in commands
pub mod builtin_commands {
    pub use crate::builtin::commands::*;
    pub use crate::builtin::{register_all, all_commands};
}

/// Create a new command registry with all built-in commands registered
pub fn create_registry() -> CommandRegistry {
    let registry = CommandRegistry::new();
    builtin::register_all(&registry);
    registry
}
