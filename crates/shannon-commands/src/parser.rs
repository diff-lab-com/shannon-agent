//! Command parser - parses command strings into structured commands

use crate::command::CommandError;
use nom::{
    branch::alt,
    bytes::complete::{tag, take_till, take_while1},
    character::complete::{char, space0, space1},
    combinator::{map, opt, rest},
    sequence::{pair, preceded, tuple},
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
    fn test_parse_invalid() {
        let parser = CommandParser::new();
        assert!(parser.parse("commit").is_err()); // No slash
        assert!(parser.parse("/").is_err()); // Just slash
    }

    #[test]
    fn test_is_command() {
        let parser = CommandParser::new();
        assert!(parser.is_command("/commit"));
        assert!(parser.is_command("/commit with args"));
        assert!(!parser.is_command("commit"));
        assert!(!parser.is_command(""));
    }
}
