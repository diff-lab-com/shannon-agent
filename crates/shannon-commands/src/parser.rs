//! Command parser - parses command strings into structured commands

use crate::command::CommandError;
use nom::{
    IResult,
    branch::alt,
    bytes::complete::{tag, take_while1},
    character::complete::space1,
    combinator::{map, opt, rest},
    sequence::{preceded, tuple},
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
        self.args.split_whitespace().collect()
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

    /// Parse multiple commands from input (handles chaining).
    ///
    /// Commands are separated by a newline, `;`, or `&&` — shell-style.
    /// Separators inside single- or double-quoted spans are literal text
    /// (e.g. `/commit -m "fix; refactor" ; /test` yields two commands). Each
    /// segment is parsed independently; the first segment failing to parse is
    /// a hard error, while a later non-command segment stops the chain with
    /// the commands parsed so far.
    pub fn parse_multiple(&self, input: &str) -> Result<Vec<ParsedCommand>, CommandError> {
        let mut results = vec![];

        for segment in split_command_chain(input) {
            match self.parse(&segment) {
                Ok(cmd) => results.push(cmd),
                // A trailing fragment that isn't a command (e.g. text after
                // the last separator) ends the chain without an error.
                Err(_) if results.is_empty() => {
                    return Err(CommandError::ParseError(
                        "No valid commands found".to_string(),
                    ));
                }
                Err(_) => break,
            }
        }

        if results.is_empty() {
            Err(CommandError::ParseError(
                "No commands to execute".to_string(),
            ))
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
    let args = opt(preceded(space1, alt((rest, map(tag(""), |_| "")))));

    map(tuple((name, args)), |(n, a)| (n, a.unwrap_or("")))(input)
}

/// Split chained input into command segments on newlines, `;`, and `&&`.
///
/// Separators inside single- or double-quoted spans are literal text, so
/// quoted argument values (`-m "fix; refactor"`) never split a command.
fn split_command_chain(input: &str) -> Vec<String> {
    let mut segments: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();
    let mut quote: Option<char> = None;

    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            current.push(c);
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => {
                quote = Some(c);
                current.push(c);
            }
            '\n' | ';' => {
                segments.push(std::mem::take(&mut current));
            }
            '&' if chars.peek() == Some(&'&') => {
                chars.next(); // consume the second '&'
                segments.push(std::mem::take(&mut current));
            }
            _ => current.push(c),
        }
    }
    segments.push(current);

    segments
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Tokenize an args string on whitespace, keeping quoted spans as single
/// tokens with their quotes stripped (`--text "hello -world"` → two tokens:
/// `--text`, `hello -world`).
fn split_args_tokens(input: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;

    for c in input.chars() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else {
                current.push(c);
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            c if c.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Whether a token itself looks like a flag (`--flag`, `-f`, `-abc`).
fn looks_like_flag(token: &str) -> bool {
    if let Some(long) = token.strip_prefix("--") {
        return !long.is_empty();
    }
    // A bare `-` is not a flag; a single letter (`-v`) or cluster (`-abc`) is.
    token
        .strip_prefix('-')
        .is_some_and(|short| short.chars().next().is_some_and(|c| c.is_alphabetic()))
}

/// Parse flags from the **leading** flag section of the args.
///
/// Recognized forms: `--flag`, `--key=value`, `--flag value`, and short
/// clusters (`-abc` → a, b, c). Parsing stops at the first token that is not
/// a flag — everything after it is body text. This keeps hyphenated words in
/// the message body (`/cmd a -b "c -d" --flag x rest -here`) from being
/// swallowed as flags: only tokens *before* the first positional word count.
fn parse_flags(input: &str) -> HashMap<String, Option<String>> {
    let mut flags = HashMap::new();
    let tokens = split_args_tokens(input);

    let mut i = 0;
    while i < tokens.len() {
        let token = &tokens[i];

        // Convention: a bare `--` explicitly ends the flag section.
        if token == "--" {
            break;
        }

        if let Some(long) = token.strip_prefix("--") {
            if long.is_empty() {
                break;
            }
            match long.split_once('=') {
                Some((name, value)) => {
                    flags.insert(name.to_string(), Some(value.to_string()));
                    i += 1;
                }
                None if i + 1 < tokens.len() && !looks_like_flag(&tokens[i + 1]) => {
                    // `--flag value` — the next token is this flag's value.
                    flags.insert(long.to_string(), Some(tokens[i + 1].clone()));
                    i += 2;
                }
                None => {
                    flags.insert(long.to_string(), None);
                    i += 1;
                }
            }
            continue;
        }

        if token.len() > 1 && token.starts_with('-') {
            let cluster = &token[1..];
            if !cluster.chars().all(|c| c.is_alphabetic()) {
                // `-1`, `->`, `--`-adjacent junk, … — not a flag cluster; the
                // flag section ends here and the token is body text.
                break;
            }
            for c in cluster.chars() {
                flags.insert(c.to_string(), None);
            }
            i += 1;
            continue;
        }

        // First non-flag token: everything from here on is body text.
        break;
    }

    flags
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
        assert_eq!(
            result.flag_value("url"),
            Some(&"https://example.com".to_string())
        );
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

    // ── parse_multiple chaining (§P3-17) ──────────────────────────────

    #[test]
    fn test_parse_multiple_chains_on_semicolon() {
        let parser = CommandParser::new();
        let result = parser.parse_multiple("/help; /test").unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "help");
        assert_eq!(result[1].name, "test");
    }

    #[test]
    fn test_parse_multiple_chains_on_newline_and_double_ampersand() {
        let parser = CommandParser::new();
        let result = parser.parse_multiple("/help\n/test").unwrap();
        assert_eq!(result.len(), 2);

        let result = parser.parse_multiple("/a one && /b two").unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].args_trimmed(), "one");
        assert_eq!(result[1].args_trimmed(), "two");
    }

    #[test]
    fn test_parse_multiple_chain_keeps_quoted_separator_literal() {
        let parser = CommandParser::new();
        // A `;` inside a quoted value is literal text, not a separator.
        let result = parser
            .parse_multiple(r#"/commit -m "fix; refactor" ; /test"#)
            .unwrap();
        assert_eq!(result.len(), 2);
        assert!(result[0].args.contains("fix; refactor"));
        assert_eq!(result[1].name, "test");
    }

    #[test]
    fn test_parse_multiple_stops_at_non_command_segment() {
        let parser = CommandParser::new();
        // Text after the last separator isn't a command: keep what parsed.
        let result = parser.parse_multiple("/help; plain text here").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "help");
    }

    // ── parse_flags leading-only semantics (§P3-18) ───────────────────

    /// Adversarial case from the review: hyphenated words in the *body*
    /// must not be swallowed as flags.
    #[test]
    fn test_parse_flags_body_hyphen_words_are_not_flags() {
        let parser = CommandParser::new();
        let result = parser
            .parse(r#"/cmd a -b "c -d" --flag x rest -here"#)
            .unwrap();
        assert!(
            result.flags.is_empty(),
            "body flags must not be parsed, got {:?}",
            result.flags
        );
        // Body is untouched.
        assert!(result.args.contains("-b"));
        assert!(result.args.contains("c -d"));
        assert!(result.args.contains("--flag x"));
        assert!(result.args.contains("-here"));
    }

    #[test]
    fn test_parse_flags_leading_flags_then_body_stops() {
        let parser = CommandParser::new();
        let result = parser.parse("/cmd --verbose hello -world").unwrap();
        assert!(result.has_flag("verbose"));
        assert!(!result.has_flag("world"), "-world is body text, not a flag");
    }

    #[test]
    fn test_parse_flags_leading_space_separated_value() {
        let parser = CommandParser::new();
        let result = parser.parse(r#"/msg --text "hello -world" tail"#).unwrap();
        assert_eq!(result.flag_value("text"), Some(&"hello -world".to_string()));
    }

    #[test]
    fn test_parse_flags_negative_number_is_value_not_flag() {
        let parser = CommandParser::new();
        let result = parser.parse("/run --limit -5").unwrap();
        assert_eq!(result.flag_value("limit"), Some(&"-5".to_string()));
        assert!(!result.has_flag("5"));
    }

    #[test]
    fn test_parse_flags_bare_double_dash_ends_flag_section() {
        let parser = CommandParser::new();
        let result = parser.parse("/run -- --not-a-flag").unwrap();
        assert!(result.flags.is_empty());
        assert!(result.args.contains("--not-a-flag"));
    }

    #[test]
    fn test_parse_flags_quoted_equals_value() {
        let parser = CommandParser::new();
        let result = parser.parse(r#"/commit --message="fix the; bug""#).unwrap();
        assert_eq!(
            result.flag_value("message"),
            Some(&"fix the; bug".to_string())
        );
    }
}
