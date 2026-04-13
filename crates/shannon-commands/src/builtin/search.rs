//! /search command - Search command history with regex support

use crate::command::{Command, CommandBase, CommandSource, PromptCommand, ExecutionContext, CommandAvailability};
use std::time::{SystemTime, UNIX_EPOCH};

/// Search prompt template
const SEARCH_PROMPT: &str = r##"
Search through command history for matching entries.

Arguments: {args}
- The first non-flag tokens form the search pattern
- Flags: --count N (max results, default 20), --regex (use regex matching), --case-sensitive (exact case), --no-timestamps

Display matching history entries in reverse chronological order (most recent first).
If no pattern is given, show the last 20 commands.
"##;

/// Create the /search command
pub fn command() -> Command {
    Command::Prompt(PromptCommand {
        base: CommandBase {
            name: "search".to_string(),
            aliases: vec!["?".to_string(), "history-search".to_string(), "hist".to_string()],
            description: "Search command history with regex patterns and filtering".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[pattern] [--count N] [--regex] [--case-sensitive]".to_string()),
            when_to_use: Some(
                "To find previously run commands matching a pattern or search through your command history".to_string(),
            ),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: true,
            is_sensitive: false,
            user_facing_name: None,
        },
        progress_message: "".to_string(),
        content_length: 1000,
        arg_names: vec!["pattern".to_string(), "options".to_string()],
        allowed_tools: vec![],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
        prompt_template: Some(SEARCH_PROMPT.to_string()),
    })
}

/// Search options parsed from command arguments
#[derive(Debug, Clone)]
pub struct SearchOptions {
    /// Search pattern
    pub pattern: String,

    /// Maximum number of results
    pub count: usize,

    /// Use regex matching
    pub regex: bool,

    /// Case-sensitive search
    pub case_sensitive: bool,

    /// Show timestamps (if available)
    pub show_timestamps: bool,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            pattern: String::new(),
            count: 20,
            regex: false,
            case_sensitive: false,
            show_timestamps: true,
        }
    }
}

/// Parse search arguments into options
pub fn parse_search_args(args: &str) -> Result<SearchOptions, String> {
    let mut options = SearchOptions::default();
    let mut pattern_parts = Vec::new();

    for token in args.split_whitespace() {
        match token {
            t if t.starts_with("--count=") || t.starts_with("-c=") => {
                let count_str = t.split('=').nth(1).ok_or("Missing --count value")?;
                options.count = count_str
                    .parse::<usize>()
                    .map_err(|_| format!("Invalid count value: {count_str}"))?;
            }
            "--regex" | "-r" => {
                options.regex = true;
            }
            "--case-sensitive" | "-i" => {
                options.case_sensitive = true;
            }
            "--no-timestamps" => {
                options.show_timestamps = false;
            }
            t if t.starts_with('-') => {
                return Err(format!("Unknown option: {t}"));
            }
            t => {
                pattern_parts.push(t.to_string());
            }
        }
    }

    options.pattern = pattern_parts.join(" ");

    if options.pattern.is_empty() {
        return Err("No search pattern provided".to_string());
    }

    Ok(options)
}

/// Search history entries with given options
pub fn search_history(entries: &[String], options: &SearchOptions) -> Vec<HistoryMatch> {
    let mut matches = Vec::new();

    for (idx, entry) in entries.iter().enumerate() {
        let is_match = if options.regex {
            match regex::Regex::new(&options.pattern) {
                Ok(re) => {
                    if options.case_sensitive {
                        re.is_match(entry)
                    } else {
                        re.is_match(&entry.to_lowercase())
                    }
                }
                Err(_) => {
                    // Fallback to substring match on invalid regex
                    if options.case_sensitive {
                        entry.contains(&options.pattern)
                    } else {
                        entry.to_lowercase().contains(&options.pattern.to_lowercase())
                    }
                }
            }
        } else {
            let search_entry = if options.case_sensitive {
                entry.clone()
            } else {
                entry.to_lowercase()
            };
            let search_pattern = if options.case_sensitive {
                options.pattern.clone()
            } else {
                options.pattern.to_lowercase()
            };
            search_entry.contains(&search_pattern)
        };

        if is_match {
            matches.push(HistoryMatch {
                index: idx,
                command: entry.clone(),
                timestamp: None, // TODO: Add timestamp support when available
            });
        }
    }

    // Reverse to show most recent first, then limit
    matches.reverse();
    matches.truncate(options.count);
    matches
}

/// A history match result
#[derive(Debug, Clone)]
pub struct HistoryMatch {
    /// Index in the history
    pub index: usize,

    /// The command string
    pub command: String,

    /// Optional timestamp
    pub timestamp: Option<u64>,
}

/// Format search results for display
pub fn format_results(matches: &[HistoryMatch], options: &SearchOptions) -> String {
    if matches.is_empty() {
        return format!("No matches found for pattern: {}", options.pattern);
    }

    let mut output = format!(
        "Found {} match{} for pattern: '{}'\n",
        matches.len(),
        if matches.len() == 1 { "" } else { "es" },
        options.pattern
    );

    if options.show_timestamps {
        output.push_str("\n[timestamp] Command\n");
        output.push_str(&str::repeat("-", 80));
        output.push('\n');
    }

    for (i, match_) in matches.iter().enumerate() {
        if options.show_timestamps {
            let timestamp = match_.timestamp.unwrap_or_else(|| {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
            });
            output.push_str(&format!(
                "[{}] {}\n",
                format_timestamp(timestamp),
                match_.command
            ));
        } else {
            output.push_str(&format!("{}: {}\n", i + 1, match_.command));
        }
    }

    output
}

/// Format a Unix timestamp as a readable date/time
fn format_timestamp(secs: u64) -> String {
    use chrono::{DateTime, Local, Utc};

    let dt = DateTime::<Utc>::from_timestamp(secs as i64, 0)
        .unwrap_or_default();
    let local: DateTime<Local> = dt.into();

    local.format("%Y-%m-%d %H:%M:%S").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_command() {
        let cmd = command();
        assert_eq!(cmd.name(), "search");
        assert!(cmd.aliases().contains(&"history-search".to_string()));
    }

    #[test]
    fn test_parse_search_args_basic() {
        let args = "test pattern";
        let options = parse_search_args(args).unwrap();
        assert_eq!(options.pattern, "test pattern");
        assert_eq!(options.count, 20);
        assert!(!options.regex);
        assert!(!options.case_sensitive);
    }

    #[test]
    fn test_parse_search_args_with_options() {
        let args = "git --count=5 --regex --case-sensitive";
        let options = parse_search_args(args).unwrap();
        assert_eq!(options.pattern, "git");
        assert_eq!(options.count, 5);
        assert!(options.regex);
        assert!(options.case_sensitive);
    }

    #[test]
    fn test_parse_search_args_empty_pattern() {
        let args = "--count=10";
        let result = parse_search_args(args);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_search_args_invalid_count() {
        let args = "test --count=abc";
        let result = parse_search_args(args);
        assert!(result.is_err());
    }

    #[test]
    fn test_search_history_basic() {
        let entries = vec![
            "git status".to_string(),
            "git commit -m 'fix'".to_string(),
            "cargo build".to_string(),
            "git log".to_string(),
        ];

        let options = SearchOptions {
            pattern: "git".to_string(),
            ..Default::default()
        };

        let results = search_history(&entries, &options);
        assert_eq!(results.len(), 3);
        // Results are reversed (most recent first)
        assert_eq!(results[0].command, "git log");
        assert_eq!(results[1].command, "git commit -m 'fix'");
        assert_eq!(results[2].command, "git status");
    }

    #[test]
    fn test_search_history_case_insensitive() {
        let entries = vec![
            "GIT STATUS".to_string(),
            "git commit".to_string(),
            "Cargo Build".to_string(),
        ];

        let options = SearchOptions {
            pattern: "git".to_string(),
            case_sensitive: false,
            ..Default::default()
        };

        let results = search_history(&entries, &options);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_search_history_case_sensitive() {
        let entries = vec![
            "GIT STATUS".to_string(),
            "git commit".to_string(),
        ];

        let options = SearchOptions {
            pattern: "git".to_string(),
            case_sensitive: true,
            ..Default::default()
        };

        let results = search_history(&entries, &options);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].command, "git commit");
    }

    #[test]
    fn test_search_history_count_limit() {
        let entries = vec![
            "git 1".to_string(),
            "git 2".to_string(),
            "git 3".to_string(),
            "git 4".to_string(),
            "git 5".to_string(),
        ];

        let options = SearchOptions {
            pattern: "git".to_string(),
            count: 3,
            ..Default::default()
        };

        let results = search_history(&entries, &options);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_search_history_no_matches() {
        let entries = vec![
            "git status".to_string(),
            "cargo build".to_string(),
        ];

        let options = SearchOptions {
            pattern: "npm".to_string(),
            ..Default::default()
        };

        let results = search_history(&entries, &options);
        assert!(results.is_empty());
    }

    #[test]
    fn test_format_results_empty() {
        let options = SearchOptions {
            pattern: "nothing".to_string(),
            ..Default::default()
        };
        let output = format_results(&[], &options);
        assert!(output.contains("No matches found"));
    }

    #[test]
    fn test_format_results_with_matches() {
        let matches = vec![
            HistoryMatch {
                index: 0,
                command: "git status".to_string(),
                timestamp: Some(1_600_000_000),
            },
            HistoryMatch {
                index: 1,
                command: "cargo build".to_string(),
                timestamp: Some(1_600_000_100),
            },
        ];

        let options = SearchOptions {
            pattern: "test".to_string(),
            ..Default::default()
        };

        let output = format_results(&matches, &options);
        assert!(output.contains("Found 2 matches"));
        assert!(output.contains("git status"));
        assert!(output.contains("cargo build"));
    }

    #[test]
    fn test_format_results_no_timestamps() {
        let matches = vec![
            HistoryMatch {
                index: 0,
                command: "git status".to_string(),
                timestamp: None,
            },
        ];

        let options = SearchOptions {
            pattern: "test".to_string(),
            show_timestamps: false,
            ..Default::default()
        };

        let output = format_results(&matches, &options);
        assert!(output.contains("1: git status"));
        assert!(!output.contains("[timestamp]"));
    }
}
