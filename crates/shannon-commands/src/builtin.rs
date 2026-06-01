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

mod batch;
mod check;
mod commit;
mod config;
mod cost;
mod credentials;
mod debug;
mod diff;
mod doctor;
mod effort;
mod export;
mod help;
mod issue;
mod lsp;
mod mcp;
mod memory;
mod monitor;
mod pdf;
mod plan;
mod preset;
mod review;
mod review_pr;
mod search;
mod security_review;
mod status;
mod team;
mod theme;
// Plugin management commands — scaffolded for future use
mod context;
mod image;
mod outline;
mod plugin;
mod repl;
mod repomap;
mod session;

/// Register all built-in commands
pub fn register_all(registry: &CommandRegistry) {
    for command in all_commands() {
        registry.register_sync(command);
    }
}

/// Get all built-in commands
pub fn all_commands() -> Vec<Command> {
    let mut cmds = vec![
        batch::command(),
        commit::command(),
        review_pr::command(),
        review::command(),
        security_review::command(),
        theme::command(),
        effort::command(),
        memory::command(),
        plan::command(),
        monitor::command(),
        check::command(),
        pdf::command(),
        issue::command(),
        help::command(),
        status::command(),
        cost::command(),
        diff::command(),
        search::command(),
        export::command(),
        config::command(),
        credentials::command(),
        debug::command(),
        doctor::command(),
        mcp::command(),
        lsp::command(),
        plugin::command(),
        image::command(),
        repomap::command(),
        outline::command(),
        context::command(),
        preset::command(),
        session::command(),
        team::command(),
    ];
    cmds.extend(repl::all_commands());
    cmds
}

/// Create individual commands for direct access
pub mod commands {
    pub use super::commit::command as commit_command;
    pub use super::diff::command as diff_command;
    pub use super::help::command as help_command;
    pub use super::pdf::command as pdf_command;
    pub use super::review_pr::command as review_pr_command;
    pub use super::status::command as status_command;
}

/// Re-export help utilities for REPL integration
pub mod help_utils {
    pub use super::help::{
        CommandHelpEntry, HelpCategory, all_help_entries, generate_help, get_command_help,
    };
}

/// Re-export PDF types for external consumers
pub mod pdf_types {
    pub use super::pdf::{
        ImageFormat, PdfContent, PdfImage, PdfMetadata, PdfOptions, PdfPage, PdfTable,
        get_pdf_prompt,
    };
}

/// Re-export export utilities for REPL integration
pub mod export_utils {
    pub use super::export::{
        ExportFormat, ExportMessage, ExportOptions, ExportSession, SessionMetadata, export_to_json,
        export_to_markdown, generate_filename, parse_export_args, write_export,
    };
}

/// Re-export credential utilities for REPL integration
pub mod credential_utils {
    pub use super::credentials::{
        CredentialAction, format_credential_count, format_credential_delete, format_credential_get,
        format_credential_store, format_credentials_list, parse_credential_action,
    };
}

/// Re-export git status utilities for REPL integration
pub mod status_utils {
    pub use super::status::{
        AheadBehind, GitStatusInfo, StatusFile, format_status, parse_git_status,
    };
}

/// Re-export diff analysis utilities for REPL integration
pub mod diff_utils {
    pub use super::diff::{
        CategorizedChange, ChangeCategory, DiffAnalysis, DiffAnalyzer, DiffOptions, DiffScope,
        DiffStats, FileStats, build_diff_command, parse_diff_stat, run_diff_analysis,
    };
}

/// Re-export search utilities for REPL integration
pub mod search_utils {
    pub use super::search::{
        HistoryMatch, SearchOptions, format_results, parse_search_args, search_history,
    };
}

/// Re-export debug utilities for REPL integration
pub mod debug_utils {
    pub use super::debug::{
        DebugSubcommand, LogLevel, format_debug_help, format_log_response, format_profile_response,
        format_system_info, format_trace_response, parse_debug_subcommand, parse_log_level,
    };
}

/// Re-export config utilities for REPL integration
pub mod config_utils {
    pub use super::config::{
        ConfigAction, ConfigKey, format_config_get, format_config_list, format_config_reset,
        format_config_set, known_config_keys, parse_config_action,
    };
}

/// Re-export PR review utilities for prompt generation and output formatting
pub mod review_utils {
    pub use super::review_pr::{
        Assessment, IssueSeverity, ReviewCategory, ReviewIssue, ReviewResult, get_review_prompt,
        run_pr_analysis,
    };
}

/// Re-export doctor utilities for REPL integration
pub mod doctor_utils {
    pub use super::doctor::{
        CheckResult, CheckStatus, check_config_files, check_rust_toolchain, format_doctor_report,
        run_all_checks,
    };
}

/// Re-export image utilities for REPL integration
pub mod image_utils {
    pub use super::image::detect_media_type;
}

/// Re-export preset utilities for REPL integration
#[allow(unused_imports)]
pub mod preset_utils {
    pub use super::preset::{
        ConversationPreset, builtin_presets, format_preset_detail, format_preset_list,
        merge_presets,
    };
}

/// Re-export MCP command utilities for REPL integration
#[allow(unused_imports)]
pub mod mcp_utils {
    pub use super::mcp::{
        AnnotationInfo, McpSubcommand, ServerStateDisplay, ServerStateInfo, ServerStatusInfo,
        ToolInfo, format_annotations, format_duration, format_mcp_help, format_server_list,
        format_server_status, format_tool_list, parse_mcp_subcommand,
    };
}

/// Re-export LSP command utilities for REPL integration
#[allow(unused_imports)]
pub mod lsp_utils {
    pub use super::lsp::{
        LspServerState, LspServerStatus, LspSubcommand, format_lsp_help, format_server_list,
        get_default_server_command, is_valid_language, parse_lsp_subcommand,
    };
}

/// Re-export plugin command utilities for REPL integration
#[allow(unused_imports)]
pub mod plugin_utils {
    pub use super::plugin::{
        PluginDisplayInfo, PluginInfoDisplay, PluginSubcommand, create_index, create_registry,
        default_plugins_dir, enable_disable, execute_plugin_subcommand, format_plugin_help,
        format_plugin_info, format_plugin_list, format_ranked_search_results,
        format_search_results, get_info, install_from_source, list_installed,
        parse_plugin_subcommand, search_index, uninstall, update_plugins,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_commands_returns_nonempty() {
        let cmds = all_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn all_commands_no_duplicate_names() {
        let cmds = all_commands();
        let names: Vec<&str> = cmds.iter().map(|c| c.name()).collect();
        let unique: std::collections::HashSet<&str> = names.iter().copied().collect();
        assert_eq!(names.len(), unique.len());
    }

    #[test]
    fn all_commands_includes_key_commands() {
        let cmds = all_commands();
        let names: std::collections::HashSet<&str> = cmds.iter().map(|c| c.name()).collect();
        assert!(names.contains("commit"));
        assert!(names.contains("help"));
        assert!(names.contains("status"));
        assert!(names.contains("diff"));
        assert!(names.contains("config"));
        assert!(names.contains("pdf"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn register_all_populates_registry() {
        let registry = CommandRegistry::new();
        register_all(&registry);
        let count = registry.count().await;
        assert_eq!(count, all_commands().len());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn register_all_key_commands_lookup() {
        let registry = CommandRegistry::new();
        register_all(&registry);
        assert!(registry.contains("commit").await);
        assert!(registry.contains("help").await);
        assert!(registry.contains("status").await);
        assert!(registry.contains("clear").await);
    }
}
