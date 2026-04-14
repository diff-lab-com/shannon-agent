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
pub mod pdf_types {
    pub use super::pdf::{
        PdfContent, PdfPage, PdfTable, PdfImage, PdfMetadata, PdfOptions, ImageFormat,
        get_pdf_prompt,
    };
}

/// Re-export export utilities for REPL integration
pub mod export_utils {
    pub use super::export::{
        ExportFormat, ExportMessage, ExportSession, SessionMetadata,
        ExportOptions, parse_export_args, generate_filename,
        export_to_markdown, export_to_json, write_export,
    };
}

/// Re-export credential utilities for REPL integration
pub mod credential_utils {
    pub use super::credentials::{
        CredentialAction, parse_credential_action,
        format_credentials_list, format_credential_store,
        format_credential_get, format_credential_delete,
        format_credential_count,
    };
}

/// Re-export git status utilities for REPL integration
pub mod status_utils {
    pub use super::status::{
        GitStatusInfo, StatusFile, AheadBehind,
        parse_git_status, format_status,
    };
}

/// Re-export diff analysis utilities for REPL integration
pub mod diff_utils {
    pub use super::diff::{
        DiffScope, DiffOptions, build_diff_command,
        DiffStats, FileStats, parse_diff_stat,
        ChangeCategory, CategorizedChange,
        DiffAnalysis, DiffAnalyzer,
    };
}

/// Re-export search utilities for REPL integration
pub mod search_utils {
    pub use super::search::{
        SearchOptions, HistoryMatch,
        parse_search_args, search_history, format_results,
    };
}

/// Re-export debug utilities for REPL integration
pub mod debug_utils {
    pub use super::debug::{
        DebugSubcommand, LogLevel,
        parse_debug_subcommand, parse_log_level,
        format_debug_help, format_log_response,
        format_profile_response, format_trace_response,
        format_system_info,
    };
}

/// Re-export config utilities for REPL integration
pub mod config_utils {
    pub use super::config::{
        ConfigAction, ConfigKey,
        parse_config_action, known_config_keys,
        format_config_list, format_config_get,
        format_config_set, format_config_reset,
    };
}

/// Re-export PR review utilities for prompt generation and output formatting
pub mod review_utils {
    pub use super::review_pr::{
        get_review_prompt, ReviewCategory, ReviewIssue, IssueSeverity,
        ReviewResult, Assessment,
    };
}

/// Re-export doctor utilities for REPL integration
pub mod doctor_utils {
    pub use super::doctor::{
        CheckStatus, CheckResult,
        run_all_checks,
        check_config_files, check_rust_toolchain,
        format_doctor_report,
    };
}
