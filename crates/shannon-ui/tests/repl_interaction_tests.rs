//! REPL interaction tests — command parsing, input handling, and command-vs-query routing.
//!
//! Run with: cargo test --package shannon-ui --test repl_interaction_tests -- --test-threads=1

use shannon_commands::CommandParser;

// ── Command Parsing ─────────────────────────────────────────────────

#[test]
fn test_command_parsing_help() {
    let parser = CommandParser::new();
    let result = parser.parse("/help").unwrap();
    assert_eq!(result.name, "help");
    assert_eq!(result.args, "");
}

#[test]
fn test_command_parsing_config() {
    let parser = CommandParser::new();
    let result = parser.parse("/config").unwrap();
    assert_eq!(result.name, "config");
    assert_eq!(result.args, "");
}

#[test]
fn test_command_parsing_quit() {
    let parser = CommandParser::new();
    let result = parser.parse("/quit").unwrap();
    assert_eq!(result.name, "quit");
    assert_eq!(result.args, "");
}

#[test]
fn test_command_parsing_unknown() {
    let parser = CommandParser::new();
    // "unknown" is not a registered command name, but parsing still succeeds.
    // The *routing* layer would report it as unknown. Here we verify the parser
    // can handle arbitrary command names.
    let result = parser.parse("/unknown-xyz").unwrap();
    assert_eq!(result.name, "unknown-xyz");
}

// ── Empty / Whitespace Input ────────────────────────────────────────

#[test]
fn test_empty_input_ignored() {
    let parser = CommandParser::new();
    // An empty string does not start with "/", so it is not a command.
    assert!(!parser.is_command(""));
    // Parsing empty input should fail (no slash prefix).
    assert!(parser.parse("").is_err());
}

#[test]
fn test_whitespace_only_input() {
    let parser = CommandParser::new();
    // Whitespace-only input should not be treated as a command.
    assert!(!parser.is_command("   "));
    assert!(!parser.is_command("\t"));
    assert!(!parser.is_command("  \n  "));
    // Parsing whitespace-only input fails (no slash prefix after trim).
    assert!(parser.parse("   ").is_err());
}

// ── Command vs Query Distinction ────────────────────────────────────

#[test]
fn test_command_vs_query_distinction() {
    let parser = CommandParser::new();

    // Strings starting with "/" are commands.
    assert!(parser.is_command("/help"));
    assert!(parser.is_command("/config set model gpt-4"));
    assert!(parser.is_command("  /quit")); // leading whitespace is trimmed

    // Strings without "/" prefix are queries, not commands.
    assert!(!parser.is_command("help me with this code"));
    assert!(!parser.is_command("what is a closure?"));
    assert!(!parser.is_command("  plain text"));
}

// ── Special Command Routing ─────────────────────────────────────────

#[test]
fn test_special_command_routing() {
    let parser = CommandParser::new();

    // Verify different slash-commands parse to distinct names.
    let help = parser.parse("/help").unwrap();
    let config = parser.parse("/config set theme dark").unwrap();
    let commit = parser.parse("/commit fix the bug").unwrap();
    let review = parser.parse("/review-pr 123").unwrap();
    let doctor = parser.parse("/doctor").unwrap();

    assert_ne!(help.name, config.name);
    assert_ne!(config.name, commit.name);
    assert_ne!(commit.name, review.name);
    assert_ne!(review.name, doctor.name);

    // Each parses correctly.
    assert_eq!(help.name, "help");
    assert_eq!(config.name, "config");
    assert!(config.args.contains("set theme dark"));
    assert_eq!(commit.name, "commit");
    assert_eq!(review.name, "review-pr");
    assert_eq!(doctor.name, "doctor");
}

// ── Input Trim Handling ─────────────────────────────────────────────

#[test]
fn test_input_trim_handling() {
    let parser = CommandParser::new();

    // Leading/trailing whitespace is trimmed before parsing.
    let result = parser.parse("  /help  ").unwrap();
    assert_eq!(result.name, "help");

    // The raw field preserves the trimmed input.
    assert_eq!(result.raw, "/help");

    // Arguments also get trimmed via args_trimmed().
    let with_args = parser.parse("  /commit   fix the bug  ").unwrap();
    assert_eq!(with_args.name, "commit");
    assert_eq!(with_args.args_trimmed(), "fix the bug");
}
