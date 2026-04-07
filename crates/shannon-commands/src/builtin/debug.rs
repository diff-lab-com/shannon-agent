//! /debug command - Developer tools for debugging, logging, and profiling

use crate::command::{Command, CommandBase, CommandSource, PromptCommand, ExecutionContext, CommandAvailability};

/// Create the /debug command
pub fn command() -> Command {
    Command::Prompt(PromptCommand {
        base: CommandBase {
            name: "debug".to_string(),
            aliases: vec!["dbg".to_string(), "dev".to_string()],
            description: "Developer tools: debug, log, and profile commands".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[log|profile|trace|info] [args]".to_string()),
            when_to_use: Some(
                "Use to toggle debug logging, profile performance, or trace execution".to_string(),
            ),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        progress_message: "".to_string(),
        content_length: 2000,
        arg_names: vec!["subcommand".to_string(), "args".to_string()],
        allowed_tools: vec![],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
    })
}

/// Debug subcommands
#[derive(Debug, Clone, PartialEq)]
pub enum DebugSubcommand {
    /// Toggle or configure logging
    Log,
    /// Performance profiling
    Profile,
    /// Execution tracing
    Trace,
    /// Show system info and diagnostics
    Info,
    /// Show help
    Help,
}

/// Log level configuration
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Trace => write!(f, "trace"),
            LogLevel::Debug => write!(f, "debug"),
            LogLevel::Info => write!(f, "info"),
            LogLevel::Warn => write!(f, "warn"),
            LogLevel::Error => write!(f, "error"),
        }
    }
}

/// Parse debug subcommand from argument
pub fn parse_debug_subcommand(arg: &str) -> DebugSubcommand {
    match arg.to_lowercase().as_str() {
        "log" | "logging" => DebugSubcommand::Log,
        "profile" | "perf" | "prof" => DebugSubcommand::Profile,
        "trace" | "tracing" => DebugSubcommand::Trace,
        "info" | "diagnostics" | "diag" => DebugSubcommand::Info,
        "help" | "?" => DebugSubcommand::Help,
        _ => DebugSubcommand::Help,
    }
}

/// Parse log level from string
pub fn parse_log_level(s: &str) -> Option<LogLevel> {
    match s.to_lowercase().as_str() {
        "trace" => Some(LogLevel::Trace),
        "debug" => Some(LogLevel::Debug),
        "info" => Some(LogLevel::Info),
        "warn" | "warning" => Some(LogLevel::Warn),
        "error" => Some(LogLevel::Error),
        _ => None,
    }
}

/// Format debug help output
pub fn format_debug_help() -> String {
    let mut output = String::from("Developer Debug Commands:\n\n");

    output.push_str("  /debug log [level]     - Set log level (trace|debug|info|warn|error)\n");
    output.push_str("  /debug profile start   - Start performance profiling\n");
    output.push_str("  /debug profile stop    - Stop profiling and show results\n");
    output.push_str("  /debug trace [on|off]  - Toggle execution tracing\n");
    output.push_str("  /debug info            - Show system diagnostics\n");
    output.push_str("\nLog Levels:\n");
    output.push_str("  trace - All messages including internals\n");
    output.push_str("  debug - Debug messages and above\n");
    output.push_str("  info  - Informational messages (default)\n");
    output.push_str("  warn  - Warnings and errors only\n");
    output.push_str("  error - Critical errors only\n");

    output
}

/// Format log level response
pub fn format_log_response(level: Option<LogLevel>) -> String {
    match level {
        Some(lvl) => format!("Log level set to: {}", lvl),
        None => "Invalid log level. Use: trace, debug, info, warn, error".to_string(),
    }
}

/// Format profile response
pub fn format_profile_response(action: &str) -> String {
    match action {
        "start" => "Profiling started. Use '/debug profile stop' to end and view results.".to_string(),
        "stop" => {
            let mut output = "Profiling Results:\n\n".to_string();
            output.push_str("  Duration: N/A (profiling not instrumented yet)\n");
            output.push_str("  Memory: N/A\n");
            output.push_str("  Tool calls: N/A\n");
            output.push_str("\nNote: Full profiling requires runtime instrumentation.");
            output
        }
        _ => format!("Unknown profile action: '{}'. Use 'start' or 'stop'.", action),
    }
}

/// Format trace response
pub fn format_trace_response(enabled: bool) -> String {
    if enabled {
        "Execution tracing enabled. Operations will be logged to trace output.".to_string()
    } else {
        "Execution tracing disabled.".to_string()
    }
}

/// Format system info diagnostics
pub fn format_system_info() -> String {
    let mut output = String::from("System Diagnostics:\n\n");

    output.push_str(&format!("  OS: {}\n", std::env::consts::OS));
    output.push_str(&format!("  Arch: {}\n", std::env::consts::ARCH));
    output.push_str(&format!("  Rust edition: 2024\n"));

    // Current directory
    if let Ok(cwd) = std::env::current_dir() {
        output.push_str(&format!("  Working dir: {}\n", cwd.display()));
    }

    // Git status
    output.push_str("\n  Git: ");
    if std::path::Path::new(".git").exists() {
        output.push_str("repository detected\n");
    } else {
        output.push_str("not a git repository\n");
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_debug_subcommand() {
        assert_eq!(parse_debug_subcommand("log"), DebugSubcommand::Log);
        assert_eq!(parse_debug_subcommand("logging"), DebugSubcommand::Log);
        assert_eq!(parse_debug_subcommand("profile"), DebugSubcommand::Profile);
        assert_eq!(parse_debug_subcommand("perf"), DebugSubcommand::Profile);
        assert_eq!(parse_debug_subcommand("trace"), DebugSubcommand::Trace);
        assert_eq!(parse_debug_subcommand("info"), DebugSubcommand::Info);
        assert_eq!(parse_debug_subcommand("unknown"), DebugSubcommand::Help);
    }

    #[test]
    fn test_parse_log_level() {
        assert_eq!(parse_log_level("trace"), Some(LogLevel::Trace));
        assert_eq!(parse_log_level("debug"), Some(LogLevel::Debug));
        assert_eq!(parse_log_level("info"), Some(LogLevel::Info));
        assert_eq!(parse_log_level("warn"), Some(LogLevel::Warn));
        assert_eq!(parse_log_level("error"), Some(LogLevel::Error));
        assert_eq!(parse_log_level("warning"), Some(LogLevel::Warn));
        assert_eq!(parse_log_level("invalid"), None);
    }

    #[test]
    fn test_log_level_display() {
        assert_eq!(LogLevel::Trace.to_string(), "trace");
        assert_eq!(LogLevel::Debug.to_string(), "debug");
        assert_eq!(LogLevel::Info.to_string(), "info");
        assert_eq!(LogLevel::Warn.to_string(), "warn");
        assert_eq!(LogLevel::Error.to_string(), "error");
    }

    #[test]
    fn test_format_debug_help() {
        let help = format_debug_help();
        assert!(help.contains("/debug log"));
        assert!(help.contains("/debug profile"));
        assert!(help.contains("/debug trace"));
        assert!(help.contains("/debug info"));
    }

    #[test]
    fn test_format_log_response() {
        let valid = format_log_response(Some(LogLevel::Debug));
        assert!(valid.contains("debug"));

        let invalid = format_log_response(None);
        assert!(invalid.contains("Invalid"));
    }

    #[test]
    fn test_format_trace_response() {
        let on = format_trace_response(true);
        assert!(on.contains("enabled"));

        let off = format_trace_response(false);
        assert!(off.contains("disabled"));
    }

    #[test]
    fn test_format_system_info() {
        let info = format_system_info();
        assert!(info.contains("OS:"));
        assert!(info.contains("Arch:"));
    }
}
