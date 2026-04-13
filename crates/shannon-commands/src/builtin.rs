//! Built-in commands
//!
//! Core commands inspired by Claude Code:
//! - /commit: Create git commits
//! - /review-pr: Review pull requests
//! - /pdf: Process PDF documents
//! - /help: Show command help
//! - /status: Show git status
//! - /diff: Show git diff
//! - /search: Search command history
//! - /export: Export session data
//! - /config: Manage configuration
//! - /debug: Developer tools

use crate::command::Command;
use crate::registry::CommandRegistry;

mod commit;
mod review_pr;
mod pdf;
mod help;
mod status;
mod diff;
mod search;
mod export;
mod config;
mod credentials;
mod debug;
mod doctor;
mod repl;

/// Register all built-in commands
pub fn register_all(registry: &CommandRegistry) {
    for command in all_commands() {
        registry.register_sync(command);
    }
}

/// Get all built-in commands
pub fn all_commands() -> Vec<Command> {
    let mut cmds = vec![
        commit::command(),
        review_pr::command(),
        pdf::command(),
        help::command(),
        status::command(),
        diff::command(),
        search::command(),
        export::command(),
        config::command(),
        credentials::command(),
        debug::command(),
        doctor::command(),
    ];
    cmds.extend(repl::all_commands());
    cmds
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

/// Re-export help utilities for REPL integration
pub mod help_utils {
    pub use super::help::{generate_help, get_command_help, all_help_entries, HelpCategory, CommandHelpEntry};
}

/// Re-export PDF types for external consumers
#[allow(unused_imports)]
pub mod pdf_types {
    pub use super::pdf::{
        PdfContent, PdfPage, PdfTable, PdfImage, PdfMetadata, PdfOptions, ImageFormat,
    };
}
