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

mod command;
mod context;
mod executor;
mod parser;
mod registry;
mod repl_command;

mod builtin;

pub use command::ExecutionResult;
pub use command::{
    Command, CommandAvailability, CommandBase, CommandResult, CommandSource, Executable,
    ExecutionContext, LocalCommand, LocalJSXCommand, PluginExecutable, PromptCommand,
};
pub use context::{CommandContext, ToolUseContext};
pub use executor::CommandExecutor;
pub use executor::SharedExecutor;
pub use parser::{CommandParser, ParsedCommand};
pub use registry::CommandRegistry;
pub use repl_command::{CommandAdapter, ReplCommand};

/// Re-export built-in commands
pub mod builtin_commands {
    pub use crate::builtin::commands::*;
    pub use crate::builtin::{all_commands, register_all};
}

/// Re-export help utilities for REPL integration
pub mod help_utils {
    pub use crate::builtin::help_utils::*;
}

/// Re-export credential utilities for REPL integration
pub mod credential_utils {
    pub use crate::builtin::credential_utils::*;
}

/// Re-export export utilities for REPL integration
pub mod export_utils {
    pub use crate::builtin::export_utils::*;
}

/// Re-export git status utilities for REPL integration
pub mod status_utils {
    pub use crate::builtin::status_utils::*;
}

/// Re-export diff analysis utilities for REPL integration
pub mod diff_utils {
    pub use crate::builtin::diff_utils::*;
}

/// Re-export image utilities for REPL integration
pub mod image_utils {
    pub use crate::builtin::image_utils::*;
}

/// Re-export search utilities for REPL integration
pub mod search_utils {
    pub use crate::builtin::search_utils::*;
}

/// Re-export PDF types and utilities for external consumers
pub mod pdf_utils {
    pub use crate::builtin::pdf_types::*;
}

/// Re-export debug utilities for REPL integration
pub mod debug_utils {
    pub use crate::builtin::debug_utils::*;
}

/// Re-export config utilities for REPL integration
pub mod config_utils {
    pub use crate::builtin::config_utils::*;
}

/// Re-export PR review utilities for prompt generation and output formatting
pub mod review_utils {
    pub use crate::builtin::review_utils::*;
}

pub mod doctor_utils {
    pub use crate::builtin::doctor_utils::*;
}

/// Re-export preset utilities for REPL integration and config loading
pub mod preset_utils {
    pub use crate::builtin::preset_utils::*;
}

/// Create a new command registry with all built-in commands registered
pub fn create_registry() -> CommandRegistry {
    let registry = CommandRegistry::new();
    builtin::register_all(&registry);
    registry
}
