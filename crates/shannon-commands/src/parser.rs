//! Command parser - parses command strings into structured commands

use crate::command::CommandError;
use nom::{
    branch::alt,
    bytes::complete::{tag, take_while1},
    character::complete::{space1},
    combinator::{map, opt, rest},
    sequence::{preceded, tuple},
    IResult,
};
use std::collections::HashMap;

/// Parsed command with name and arguments
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCommand {
    /// Command name (without the / prefix)
    pub name: String,

    /// Arguments passed to the command
    pub args: String,

    /// Raw original input
    pub raw: String,

    /// Parsed flags/options
    pub flags: HashMap<String, Option<String>>,
}

impl ParsedCommand {
    /// Create a new parsed command
    pub fn new(name: String, args: String, raw: String) -> Self {
        Self {
            name,
            args,
            raw,
            flags: HashMap::new(),
        }
    }

    /// Check if a flag is present
    pub fn has_flag(&self, flag: &str) -> bool {
        self.flags.contains_key(flag)
    }

    /// Get flag value if present
    pub fn flag_value(&self, flag: &str) -> Option<&String> {
        self.flags.get(flag).and_then(|v| v.as_ref())
    }

    /// Get args as trimmed string
    pub fn args_trimmed(&self) -> &str {
        self.args.trim()
    }

    /// Split args by whitespace
    pub fn args_split(&self) -> Vec<&str> {
        self.args
            .split_whitespace()
            .collect()
    }
}

/// Command parser
pub struct CommandParser {
    /// Command prefix (default: "/")
    prefix: String,
}

impl CommandParser {
    /// Create a new command parser with default prefix
    pub fn new() -> Self {
        Self {
            prefix: "/".to_string(),
        }
    }

    /// Create a new command parser with custom prefix
    pub fn with_prefix(prefix: String) -> Self {
        Self { prefix }
    }

    /// Parse a command string
    pub fn parse(&self, input: &str) -> Result<ParsedCommand, CommandError> {
        let trimmed = input.trim();

        // Check for command prefix
        if !trimmed.starts_with(&self.prefix) {
            return Err(CommandError::ParseError(
                "Command must start with /".to_string(),
            ));
        }

        // Remove prefix and parse
        let rest = &trimmed[self.prefix.len()..];

        // Parse command name and args
        match parse_command(rest) {
            Ok((_, (name, args))) => Ok(ParsedCommand {
                name: name.to_string(),
                args: args.to_string(),
                raw: trimmed.to_string(),
                flags: parse_flags(args),
            }),
            Err(_) => Err(CommandError::ParseError(
                "Failed to parse command".to_string(),
            )),
        }
    }

    /// Parse multiple commands from input (handles chaining)
    pub fn parse_multiple(&self, input: &str) -> Result<Vec<ParsedCommand>, CommandError> {
        let mut results = vec![];
        let mut remaining = input.trim();

        while !remaining.is_empty() {
            match self.parse(remaining) {
                Ok(cmd) => {
                    let consumed_len = cmd.raw.len();
                    results.push(cmd);
                    remaining = remaining[consumed_len..].trim();
                }
                Err(_) if results.is_empty() => return Err(CommandError::ParseError(
                    "No valid commands found".to_string(),
                )),
                Err(_) => break,
            }
        }

        if results.is_empty() {
            Err(CommandError::ParseError("No commands to execute".to_string()))
        } else {
            Ok(results)
        }
    }

    /// Check if input looks like a command
    pub fn is_command(&self, input: &str) -> bool {
        input.trim().starts_with(&self.prefix)
    }
}

impl Default for CommandParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse command name and arguments
fn parse_command(input: &str) -> IResult<&str, (&str, &str)> {
    // Command name: alphanumeric, hyphen, underscore
    let name = take_while1(|c: char| c.is_alphanumeric() || c == '-' || c == '_');

    // Optional space followed by arguments (defaults to empty string)
    let args = opt(preceded(
        space1,
        alt((rest, map(tag(""), |_| ""))),
    ));

    map(tuple((name, args)), |(n, a)| (n, a.unwrap_or("")))(input)
}

/// Parse flags from arguments (e.g., --flag, -f, --key=value)
fn parse_flags(input: &str) -> HashMap<String, Option<String>> {
    let mut flags = HashMap::new();
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '-' {
            // Check for --flag or -f
            if let Some(&next) = chars.peek() {
                if next == '-' {
                    // Long flag: --flag or --key=value
                    chars.next(); // consume second dash
                    let flag_name = take_flag_name(&mut chars);
                    let value = if let Some(&'=') = chars.peek() {
                        chars.next(); // consume =
                        Some(take_flag_value(&mut chars))
                    } else {
                        None
                    };
                    flags.insert(flag_name, value);
                } else {
                    // Short flag: -f or -abc (multiple flags)
                    let short_flags = take_short_flags(&mut chars);
                    for f in short_flags.chars() {
                        flags.insert(f.to_string(), None);
                    }
                }
            }
        }
    }

    flags
}

/// Take flag name until space or equals
fn take_flag_name(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut result = String::new();
    while let Some(&c) = chars.peek() {
        if c == ' ' || c == '=' {
            break;
        }
        result.push(c);
        chars.next();
    }
    result
}

/// Take short flags (e.g., -abc becomes a, b, c)
fn take_short_flags(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut result = String::new();
    while let Some(&c) = chars.peek() {
        if c.is_alphabetic() {
            result.push(c);
            chars.next();
        } else {
            break;
        }
    }
    result
}

/// Take flag value until space or end
fn take_flag_value(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut result = String::new();
    while let Some(&c) = chars.peek() {
        if c == ' ' {
            break;
        }
        result.push(c);
        chars.next();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Basic parsing ──────────────────────────────────────────────

    #[test]
    fn test_parse_simple_command() {
        let parser = CommandParser::new();
        let result = parser.parse("/commit").unwrap();
        assert_eq!(result.name, "commit");
        assert_eq!(result.args, "");
    }

    #[test]
    fn test_parse_command_with_args() {
        let parser = CommandParser::new();
        let result = parser.parse("/commit fix the bug").unwrap();
        assert_eq!(result.name, "commit");
        assert_eq!(result.args, "fix the bug");
    }

    #[test]
    fn test_parse_command_with_flags() {
        let parser = CommandParser::new();
        let result = parser.parse("/commit --amend --message=fix").unwrap();
        assert_eq!(result.name, "commit");
        assert!(result.has_flag("amend"));
        assert_eq!(result.flag_value("message"), Some(&"fix".to_string()));
    }

    #[test]
    fn test_parse_short_flags() {
        let parser = CommandParser::new();
        let result = parser.parse("/test -abc").unwrap();
        assert!(result.has_flag("a"));
        assert!(result.has_flag("b"));
        assert!(result.has_flag("c"));
    }

    #[test]
    fn test_parse_invalid_no_slash() {
        let parser = CommandParser::new();
        assert!(parser.parse("commit").is_err());
    }

    #[test]
    fn test_parse_just_slash() {
        let parser = CommandParser::new();
        assert!(parser.parse("/").is_err());
    }

    #[test]
    fn test_parse_only_whitespace_after_slash() {
        let parser = CommandParser::new();
        assert!(parser.parse("/  ").is_err());
    }

    // ── Edge cases ──────────────────────────────────────────────────

    #[test]
    fn test_parse_hyphenated_command() {
        let parser = CommandParser::new();
        let result = parser.parse("/review-pr 123").unwrap();
        assert_eq!(result.name, "review-pr");
        assert_eq!(result.args_trimmed(), "123");
    }

    #[test]
    fn test_parse_command_with_numbers_in_name() {
        let parser = CommandParser::new();
        let result = parser.parse("/issue123 fix").unwrap();
        assert_eq!(result.name, "issue123");
    }

    #[test]
    fn test_parse_unicode_args() {
        let parser = CommandParser::new();
        let result = parser.parse("/commit 修复中文bug").unwrap();
        assert_eq!(result.name, "commit");
        assert!(result.args.contains("修复中文bug"));
    }

    #[test]
    fn test_parse_extra_spaces_in_args() {
        let parser = CommandParser::new();
        let result = parser.parse("/commit   fix   the   bug").unwrap();
        assert_eq!(result.name, "commit");
        // nom space1 consumes exactly one space, rest captures everything after
        assert!(result.args.contains("fix"));
    }

    #[test]
    fn test_parse_long_command_name() {
        let long_name = "a".repeat(200);
        let input = format!("/{long_name}");
        let parser = CommandParser::new();
        let result = parser.parse(&input).unwrap();
        assert_eq!(result.name, long_name);
    }

    // ── Flags ────────────────────────────────────────────────────────

    #[test]
    fn test_parse_flag_with_url_value() {
        let parser = CommandParser::new();
        let result = parser.parse("/set --url=https://example.com").unwrap();
        assert_eq!(result.flag_value("url"), Some(&"https://example.com".to_string()));
    }

    #[test]
    fn test_parse_flag_with_dot_value() {
        let parser = CommandParser::new();
        let result = parser.parse("/search --query=file.ts").unwrap();
        assert_eq!(result.flag_value("query"), Some(&"file.ts".to_string()));
    }

    #[test]
    fn test_parse_multiple_long_flags() {
        let parser = CommandParser::new();
        let result = parser
            .parse("/run --model=gpt-4 --temp=0.7 --max-tokens=4096")
            .unwrap();
        assert_eq!(result.flag_value("model"), Some(&"gpt-4".to_string()));
        assert_eq!(result.flag_value("temp"), Some(&"0.7".to_string()));
        assert_eq!(result.flag_value("max-tokens"), Some(&"4096".to_string()));
    }

    #[test]
    fn test_parse_flag_without_value() {
        let parser = CommandParser::new();
        let result = parser.parse("/commit --amend").unwrap();
        assert!(result.has_flag("amend"));
        assert_eq!(result.flag_value("amend"), None);
    }

    #[test]
    fn test_parse_flag_dry_run() {
        let parser = CommandParser::new();
        let result = parser.parse("/test --dry-run").unwrap();
        assert!(result.has_flag("dry-run"));
        assert_eq!(result.flag_value("dry-run"), None);
    }

    #[test]
    fn test_parse_no_flags() {
        let parser = CommandParser::new();
        let result = parser.parse("/commit fix bug").unwrap();
        assert!(result.flags.is_empty());
        assert!(!result.has_flag("anything"));
    }

    #[test]
    fn test_parse_multiple_mixed_flags() {
        let parser = CommandParser::new();
        let result = parser.parse("/test --amend -v --verbose").unwrap();
        assert!(result.has_flag("amend"));
        assert!(result.has_flag("v"));
        assert!(result.has_flag("verbose"));
    }

    #[test]
    fn test_parse_short_flags_duplicate() {
        let parser = CommandParser::new();
        let result = parser.parse("/test -vva").unwrap();
        assert!(result.has_flag("v"));
        assert!(result.has_flag("a"));
    }

    // ── Custom prefix ───────────────────────────────────────────────

    #[test]
    fn test_parse_custom_prefix_exclamation() {
        let parser = CommandParser::with_prefix("!".to_string());
        let result = parser.parse("!commit fix").unwrap();
        assert_eq!(result.name, "commit");
        // Default prefix should not work
        assert!(parser.parse("/commit").is_err());
    }

    #[test]
    fn test_parse_custom_prefix_double_dash() {
        let parser = CommandParser::with_prefix("--".to_string());
        let result = parser.parse("--commit").unwrap();
        assert_eq!(result.name, "commit");
    }

    #[test]
    fn test_parse_custom_prefix_only_fails() {
        let parser = CommandParser::with_prefix("!".to_string());
        assert!(parser.parse("!").is_err());
    }

    #[test]
    fn test_parse_custom_prefix_wrong_prefix_fails() {
        let parser = CommandParser::with_prefix("!".to_string());
        assert!(parser.parse("/commit").is_err());
    }

    // ── ParsedCommand helpers ──────────────────────────────────────

    #[test]
    fn test_args_trimmed() {
        let parser = CommandParser::new();
        let result = parser.parse("/commit  fix the bug  ").unwrap();
        assert_eq!(result.args_trimmed(), "fix the bug");
    }

    #[test]
    fn test_args_split() {
        let parser = CommandParser::new();
        let result = parser.parse("/tool search --type file").unwrap();
        let split = result.args_split();
        assert_eq!(split, vec!["search", "--type", "file"]);
    }

    #[test]
    fn test_raw_preserved() {
        let parser = CommandParser::new();
        let raw = "/commit --amend -v fix bug";
        let result = parser.parse(raw).unwrap();
        assert_eq!(result.raw, raw);
    }

    #[test]
    fn test_parsed_command_new() {
        let cmd = ParsedCommand::new(
            "test".to_string(),
            "arg1 arg2".to_string(),
            "/test arg1 arg2".to_string(),
        );
        assert_eq!(cmd.name, "test");
        assert_eq!(cmd.args, "arg1 arg2");
        assert_eq!(cmd.raw, "/test arg1 arg2");
        assert!(cmd.flags.is_empty());
    }

    // ── is_command ────────────────────────────────────────────────

    #[test]
    fn test_is_command_true() {
        let parser = CommandParser::new();
        assert!(parser.is_command("/help"));
        }

    #[test]
    fn test_is_command_false() {
        let parser = CommandParser::new();
        assert!(!parser.is_command("help"));
    }

    #[test]
    fn test_is_command_with_leading_space() {
        let parser = CommandParser::new();
        assert!(parser.is_command("  /help"));
    }

    // ── parse_multiple ────────────────────────────────────────────

    #[test]
    fn test_parse_multiple_single_command() {
        let parser = CommandParser::new();
        let result = parser.parse_multiple("/help").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "help");
    }

    #[test]
    fn test_parse_multiple_empty_input() {
        let parser = CommandParser::new();
        assert!(parser.parse_multiple("").is_err());
    }

    #[test]
    fn test_parse_multiple_non_command_input() {
        let parser = CommandParser::new();
        assert!(parser.parse_multiple("not a command").is_err());
    }
}
