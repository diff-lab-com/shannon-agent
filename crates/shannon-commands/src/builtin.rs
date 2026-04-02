//! Built-in commands
//!
//! Core commands inspired by Claude Code:
//! - /commit: Create git commits
//! - /review-pr: Review pull requests
//! - /pdf: Process PDF documents
//! - /help: Show command help
//! - /status: Show git status
//! - /diff: Show git diff

use crate::command::{Command, CommandBase, CommandSource, PromptCommand};
use crate::registry::CommandRegistry;

mod commit;
mod review_pr;
mod pdf;
mod help;
mod status;
mod diff;

/// Register all built-in commands
pub fn register_all(registry: &mut CommandRegistry) {
    // Use a simple registration approach since CommandRegistry uses async
    // In a real implementation, this would be called from an async context
    let commands = all_commands();

    // Note: This is a synchronous function that would need to be called from async
    // The actual registration happens in create_registry()
}

/// Get all built-in commands
pub fn all_commands() -> Vec<Command> {
    vec![
        commit::command(),
        review_pr::command(),
        pdf::command(),
        help::command(),
        status::command(),
        diff::command(),
    ]
}

/// Create individual commands for direct access
pub mod commands {
    pub use super::commit::command as commit_command;
    pub use super::review_pr::command as review_pr_command;
    pub use super::pdf::command as pdf_command;
    pub use super::help::command as help_command;
    pub use super::status::command as status_command;
    pub use super::diff::command as diff_command;
}
