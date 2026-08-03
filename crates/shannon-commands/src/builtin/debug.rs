//! /debug command - Developer tools for debugging, logging, and profiling

use std::str::FromStr;
use std::sync::Mutex;

use crate::command::{
    Command, CommandAvailability, CommandBase, CommandSource, ExecutionContext, PromptCommand,
};

/// Debug prompt template
///
/// Instructs the AI to handle the subcommands defined in [`DebugSubcommand`]
/// using the formatting helpers in this module.
const DEBUG_PROMPT: &str = r##"
Developer debugging tools.

Arguments: {args}

Subcommands:
- **log [level]** — Set log verbosity. Levels: trace, debug, info (default), warn, error
- **profile [start|stop]** — Begin or end performance profiling
- **trace [on|off]** — Toggle execution tracing
- **info** — Show system diagnostics (OS, arch, working dir, git status)
- **help** — Show this help

If no subcommand is given or the argument is unrecognized, show the help.

For `info`, run shell commands to gather:
1. OS and architecture (`uname -a` or equivalent)
2. Working directory
3. Git status (if in a repo)
4. Environment info (Rust version if available)

For `log`, report the requested level. For `profile`, acknowledge start/stop.
For `trace`, acknowledge the toggle state.
"##;

/// Create the /debug command
pub fn command() -> Command {
    Command::Prompt(Box::new(PromptCommand {
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
        prompt_template: Some(DEBUG_PROMPT.to_string()),
    }))
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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

impl FromStr for LogLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_log_level(s).ok_or_else(|| {
            format!("Unknown log level: '{s}'. Expected: trace, debug, info, warn, error")
        })
    }
}

/// Map this command's [`LogLevel`] to `shannon_core::internal_logging::InternalLogLevel`.
///
/// The two enums intentionally have different surfaces:
/// `LogLevel::Trace` has no direct match in `InternalLogLevel` (which only
/// goes down to Debug), so we collapse Trace→Debug and Info stays at Info.
/// This is intentional: the runtime `tracing_subscriber` honours Trace via
/// `RUST_LOG` while `InternalLogger` only filters at Debug+.
pub fn to_internal_log_level(level: LogLevel) -> shannon_core::internal_logging::InternalLogLevel {
    use shannon_core::internal_logging::InternalLogLevel;
    match level {
        LogLevel::Trace | LogLevel::Debug => InternalLogLevel::Debug,
        LogLevel::Info => InternalLogLevel::Info,
        LogLevel::Warn => InternalLogLevel::Warn,
        LogLevel::Error => InternalLogLevel::Error,
    }
}

/// Filter an `InternalLogger` to entries at or above the given threshold.
pub fn filter_internal_entries_below(
    entries: &[shannon_core::internal_logging::InternalLogEntry],
    threshold: LogLevel,
) -> Vec<shannon_core::internal_logging::InternalLogEntry> {
    let min_internal = to_internal_log_level(threshold);
    let min_rank = rank(min_internal);
    entries
        .iter()
        .filter(|e| rank(level_from_string(&e.level)) >= min_rank)
        .cloned()
        .collect()
}

fn level_from_string(s: &str) -> shannon_core::internal_logging::InternalLogLevel {
    use shannon_core::internal_logging::InternalLogLevel;
    match s {
        "DEBUG" => InternalLogLevel::Debug,
        "INFO" => InternalLogLevel::Info,
        "WARN" => InternalLogLevel::Warn,
        "ERROR" => InternalLogLevel::Error,
        _ => InternalLogLevel::Info,
    }
}

fn rank(level: shannon_core::internal_logging::InternalLogLevel) -> u8 {
    use shannon_core::internal_logging::InternalLogLevel;
    match level {
        InternalLogLevel::Debug => 0,
        InternalLogLevel::Info => 1,
        InternalLogLevel::Warn => 2,
        InternalLogLevel::Error => 3,
    }
}

/// Process-wide runtime log-level override.
///
/// P1-1 step 5 requires `/debug log <level>` to flip an in-process flag,
/// without forcing a restart. This state lives behind a Mutex so the
/// REPL/setter can mutate while the logger/pollers read concurrently.
///
/// Callers should use the [`current_log_level`] accessor (or the
/// [`set_runtime_log_level`] setter) rather than touching the static
/// directly.
static RUNTIME_LOG_LEVEL: Mutex<Option<LogLevel>> = Mutex::new(None);

/// Override the process-global log level at runtime.
///
/// Returns the previous level (or `None` if none was set). On platforms
/// using `tracing_subscriber::EnvFilter`, the `RUST_LOG` env var supplies
/// the actual filter — this helper only records the intent so
/// downstream consumers can read it back via [`current_log_level`].
pub fn set_runtime_log_level(level: Option<LogLevel>) -> Option<LogLevel> {
    let mut guard = RUNTIME_LOG_LEVEL.lock().expect("log-level mutex poisoned");
    let prev = *guard;
    *guard = level;
    prev
}

/// Return the current runtime log level override.
///
/// Resolution order:
///   1. Explicit override set via [`set_runtime_log_level`].
///   2. The `SHANNON_LOG_LEVEL` env var (parsed case-insensitively).
///   3. `RUST_LOG` (normal tracing convention) — used as a hint.
///   4. Default [`LogLevel::Info`].
pub fn current_log_level() -> LogLevel {
    if let Ok(guard) = RUNTIME_LOG_LEVEL.lock() {
        if let Some(level) = *guard {
            return level;
        }
    }

    if let Ok(s) = std::env::var("SHANNON_LOG_LEVEL") {
        if let Some(level) = parse_log_level(&s) {
            return level;
        }
    }
    if let Ok(s) = std::env::var("RUST_LOG") {
        // RUST_LOG is a directive; parse its loosest level spec.
        let first = s.split(',').next().unwrap_or("").trim();
        if first.contains("trace") {
            return LogLevel::Trace;
        }
        if first.contains("debug") {
            return LogLevel::Debug;
        }
        if first.contains("warn") || first.contains("warning") {
            return LogLevel::Warn;
        }
        if first.contains("error") {
            return LogLevel::Error;
        }
    }
    LogLevel::Info
}

/// Pretty-print the active log level for `/debug log` output.
pub fn format_runtime_log_status() -> String {
    let level = current_log_level();
    let source = if std::env::var("SHANNON_LOG_LEVEL").is_ok() {
        "env:SHANNON_LOG_LEVEL"
    } else if std::env::var("RUST_LOG").is_ok() {
        "env:RUST_LOG"
    } else if RUNTIME_LOG_LEVEL
        .lock()
        .map(|g| g.is_some())
        .unwrap_or(false)
    {
        "runtime override"
    } else {
        "default"
    };
    format!("Log level: {level} (source: {source})")
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
        Some(lvl) => {
            set_runtime_log_level(Some(lvl));
            format_runtime_log_status()
        }
        None => {
            // No level argument — show current state.
            format_runtime_log_status()
        }
    }
}

/// Format profile response
pub fn format_profile_response(action: &str) -> String {
    match action {
        "start" => {
            "Profiling started. Use '/debug profile stop' to end and view results.".to_string()
        }
        "stop" => {
            let mut output = "Profiling Results:\n\n".to_string();
            output.push_str("  Duration: N/A (profiling not instrumented yet)\n");
            output.push_str("  Memory: N/A\n");
            output.push_str("  Tool calls: N/A\n");
            output.push_str("\nNote: Full profiling requires runtime instrumentation.");
            output
        }
        _ => format!("Unknown profile action: '{action}'. Use 'start' or 'stop'."),
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
    output.push_str("  Rust edition: 2024\n");

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
        let prev = set_runtime_log_level(None);
        let valid = format_log_response(Some(LogLevel::Debug));
        assert!(valid.contains("debug"), "{valid}");

        // `format_log_response(None)` now reports current runtime state
        // rather than printing an "Invalid level" message — this is the
        // P1-1 behavior so /debug log with no argument is a status query.
        let status = format_log_response(None);
        assert!(status.starts_with("Log level:"), "{status}");
        set_runtime_log_level(prev);
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

    // ── P1-1 Connector Tests ────────────────────────────────────────

    #[test]
    fn log_level_ord_orders_severity_descending() {
        // We rank with smaller value = more verbose for the rank() helper,
        // but the public LogLevel ordering should treat Trace as the most
        // verbose (smallest) and Error as the most aggressive filter.
        assert!(LogLevel::Trace < LogLevel::Debug);
        assert!(LogLevel::Debug < LogLevel::Info);
        assert!(LogLevel::Info < LogLevel::Warn);
        assert!(LogLevel::Warn < LogLevel::Error);
    }

    #[test]
    fn log_level_fromstr_rejects_unknown() {
        let err = LogLevel::from_str("verbose").unwrap_err();
        assert!(err.contains("Unknown log level"));
        assert!(err.contains("verbose"));

        // Aliases are accepted via FromStr -> parse
        for (raw, expected) in [
            ("trace", LogLevel::Trace),
            ("DEBUG", LogLevel::Debug),
            ("Info", LogLevel::Info),
            ("warn", LogLevel::Warn),
            ("warning", LogLevel::Warn),
            ("error", LogLevel::Error),
        ] {
            assert_eq!(LogLevel::from_str(raw).unwrap(), expected, "input={raw}");
        }
    }

    #[test]
    fn to_internal_log_level_collapses_trace_into_debug() {
        use shannon_core::internal_logging::InternalLogLevel;
        assert_eq!(
            to_internal_log_level(LogLevel::Trace),
            InternalLogLevel::Debug
        );
        assert_eq!(
            to_internal_log_level(LogLevel::Debug),
            InternalLogLevel::Debug
        );
        assert_eq!(
            to_internal_log_level(LogLevel::Info),
            InternalLogLevel::Info
        );
        assert_eq!(
            to_internal_log_level(LogLevel::Warn),
            InternalLogLevel::Warn
        );
        assert_eq!(
            to_internal_log_level(LogLevel::Error),
            InternalLogLevel::Error
        );
    }

    #[test]
    fn filter_internal_entries_below_drops_lower_levels() {
        use shannon_core::internal_logging::InternalLogEntry;
        let entries = vec![
            InternalLogEntry::new(
                shannon_core::internal_logging::InternalLogLevel::Debug,
                "comp",
                "d1",
            ),
            InternalLogEntry::new(
                shannon_core::internal_logging::InternalLogLevel::Info,
                "comp",
                "i1",
            ),
            InternalLogEntry::new(
                shannon_core::internal_logging::InternalLogLevel::Warn,
                "comp",
                "w1",
            ),
            InternalLogEntry::new(
                shannon_core::internal_logging::InternalLogLevel::Error,
                "comp",
                "e1",
            ),
        ];
        let warn_or_higher = filter_internal_entries_below(&entries, LogLevel::Warn);
        assert_eq!(warn_or_higher.len(), 2, "Warn+Error survive Warn threshold");
        for e in &warn_or_higher {
            assert!(matches!(e.level.as_str(), "WARN" | "ERROR"));
        }
        let error_only = filter_internal_entries_below(&entries, LogLevel::Error);
        assert_eq!(error_only.len(), 1);
        assert_eq!(error_only[0].level, "ERROR");
    }

    #[test]
    fn runtime_log_level_override_round_trips() {
        // Snapshot before asserting: this avoids cross-test state leak.
        let initial = current_log_level();
        set_runtime_log_level(Some(LogLevel::Error));
        assert_eq!(current_log_level(), LogLevel::Error);
        let prev2 = set_runtime_log_level(Some(LogLevel::Trace));
        assert_eq!(current_log_level(), LogLevel::Trace);
        assert_eq!(prev2, Some(LogLevel::Error));
        // Restore so we don't change behaviour for downstream tests.
        set_runtime_log_level(Some(initial));
        assert_eq!(current_log_level(), initial);
    }

    #[test]
    fn format_log_response_records_runtime_override() {
        let prev = set_runtime_log_level(None);
        let resp = format_log_response(Some(LogLevel::Debug));
        assert!(resp.contains("debug"), "{resp}");
        assert!(resp.contains("Log level"), "{resp}");
        set_runtime_log_level(prev);
    }

    #[test]
    fn format_log_response_without_args_reports_current_state() {
        let prev = set_runtime_log_level(Some(LogLevel::Warn));
        let resp = format_log_response(None);
        assert!(resp.contains("warn"), "{resp}");
        set_runtime_log_level(prev);
    }

    #[test]
    fn format_runtime_log_status_includes_source_label() {
        let prev = set_runtime_log_level(None);
        // No envs set (test runner doesn't force them), nothing in override:
        let s = format_runtime_log_status();
        assert!(s.starts_with("Log level:"));
        assert!(s.contains("source:"));
        set_runtime_log_level(prev);
    }
}
