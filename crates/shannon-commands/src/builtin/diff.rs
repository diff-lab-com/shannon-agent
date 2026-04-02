//! /diff command - Show git diff of changes

use crate::command::{Command, CommandBase, CommandSource, PromptCommand, ExecutionContext, CommandAvailability};

/// Create the /diff command
pub fn command() -> Command {
    Command::Prompt(PromptCommand {
        base: CommandBase {
            name: "diff".to_string(),
            aliases: vec!["git-diff".to_string()],
            description: "Show git diff of changes between commits, branches, or files".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[<commit> <commit> --] [<path>]".to_string()),
            when_to_use: Some(
                "To see what has changed in the repository between revisions, or view unstaged/staged changes".to_string(),
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
        arg_names: vec!["revision_range".to_string(), "path".to_string()],
        allowed_tools: vec![
            "Bash(git diff:*)".to_string(),
            "Bash(git log:*)".to_string(),
            "Bash(git show:*)".to_string(),
        ],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
    })
}

/// Diff scope
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffScope {
    /// Unstaged changes (default)
    Unstaged,

    /// Staged changes
    Staged,

    /// Working tree (unstaged + staged)
    Working,

    /// HEAD vs working tree
    Head,

    /// Between commits/branches
    Commits,
}

impl Default for DiffScope {
    fn default() -> Self {
        DiffScope::Working
    }
}

/// Diff options
#[derive(Debug, Clone, Default)]
pub struct DiffOptions {
    /// Diff scope
    pub scope: DiffScope,

    /// Commit/revision range
    pub revision_range: Option<String>,

    /// Path filter
    pub path_filter: Option<String>,

    /// Context lines
    pub context_lines: Option<usize>,

    /// Word diff
    pub word_diff: bool,

    /// Color output
    pub color: bool,

    /// Ignore whitespace
    pub ignore_whitespace: bool,

    /// Show stats
    pub stats: bool,
}

impl DiffOptions {
    /// Create new default options
    pub fn new() -> Self {
        Self::default()
    }

    /// Set scope to staged
    pub fn staged(mut self) -> Self {
        self.scope = DiffScope::Staged;
        self
    }

    /// Set scope to working tree
    pub fn working(mut self) -> Self {
        self.scope = DiffScope::Working;
        self
    }

    /// Set revision range
    pub fn revision(mut self, range: String) -> Self {
        self.revision_range = Some(range);
        self.scope = DiffScope::Commits;
        self
    }

    /// Set path filter
    pub fn path(mut self, path: String) -> Self {
        self.path_filter = Some(path);
        self
    }

    /// Set context lines
    pub fn context(mut self, lines: usize) -> Self {
        self.context_lines = Some(lines);
        self
    }

    /// Enable word diff
    pub fn word_diff(mut self) -> Self {
        self.word_diff = true;
        self
    }

    /// Enable color
    pub fn colored(mut self) -> Self {
        self.color = true;
        self
    }

    /// Ignore whitespace
    pub fn ignore_ws(mut self) -> Self {
        self.ignore_whitespace = true;
        self
    }

    /// Show stats
    pub fn with_stats(mut self) -> Self {
        self.stats = true;
        self
    }
}

/// Build git diff command from options
pub fn build_diff_command(options: &DiffOptions) -> String {
    let mut cmd = String::from("git diff");

    match options.scope {
        DiffScope::Staged => cmd.push_str(" --staged"),
        DiffScope::Working => cmd.push_str(" HEAD"),
        DiffScope::Commits => {
            if let Some(range) = &options.revision_range {
                cmd.push_str(&format!(" {}", range));
            }
        }
        _ => {}
    }

    if let Some(lines) = options.context_lines {
        cmd.push_str(&format!(" -U{}", lines));
    }

    if options.word_diff {
        cmd.push_str(" --word-diff");
    }

    if options.ignore_whitespace {
        cmd.push_str(" --ignore-all-space");
    }

    if options.stats {
        cmd.push_str(" --stat");
    }

    if let Some(path) = &options.path_filter {
        cmd.push_str(&format!(" -- {}", path));
    }

    cmd
}

/// Diff statistics
#[derive(Debug, Clone)]
pub struct DiffStats {
    /// Files changed
    pub files_changed: usize,

    /// Insertions
    pub insertions: usize,

    /// Deletions
    pub deletions: usize,

    /// File-level stats
    pub file_stats: Vec<FileStats>,
}

/// Statistics for a single file
#[derive(Debug, Clone)]
pub struct FileStats {
    /// File path
    pub path: String,

    /// Insertions
    pub insertions: usize,

    /// Deletions
    pub deletions: usize,
}

/// Parse git diff --stat output
pub fn parse_diff_stat(output: &str) -> Option<DiffStats> {
    let mut files_changed = 0;
    let mut total_insertions = 0;
    let mut total_deletions = 0;
    let mut file_stats = Vec::new();

    for line in output.lines() {
        if line.contains(" | ") {
            files_changed += 1;

            let parts: Vec<&str> = line.split(" | ").collect();
            if parts.len() >= 2 {
                let path = parts[0].trim().to_string();

                // Parse " +/- " count like "10 ++, 5 --" or "15 +-"
                let stats_part = parts[1];
                let mut insertions = 0;
                let mut deletions = 0;

                if let Some(caps) = STATS_REGEX.captures(stats_part) {
                    insertions = caps.get(1).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
                    deletions = caps.get(2).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
                }

                total_insertions += insertions;
                total_deletions += deletions;

                file_stats.push(FileStats {
                    path,
                    insertions,
                    deletions,
                });
            }
        }
    }

    Some(DiffStats {
        files_changed,
        insertions: total_insertions,
        deletions: total_deletions,
        file_stats,
    })
}

// Simple regex for parsing stat lines
static STATS_REGEX: once_cell::sync::Lazy<regex::Regex> =
    once_cell::sync::Lazy::new(|| {
        regex::Regex::new(r"(\d+) insertion[s]?,?\s*(\d+) deletion[s]?").unwrap()
    });

/// Common diff patterns
pub mod patterns {
    /// Match function/method definitions
    pub const FUNCTION_PATTERN: &str = r"^[\+\-]((public|private|protected|internal|static|async|)\s+)*fun \w+";

    /// Match import statements
    pub const IMPORT_PATTERN: &str = r"^[\+\-](import|use|from) ";

    /// Match struct/class definitions
    pub const STRUCT_PATTERN: &str = r"^[\+\-](struct|class|interface|type) \w+";

    /// Match test functions
    pub const TEST_PATTERN: &str = r"^[\+\-].*#\[test\]|^[\+\-]\s*(fun|def|fn) test";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_command() {
        let cmd = command();
        assert_eq!(cmd.name(), "diff");
        assert!(cmd.aliases().contains(&"git-diff".to_string()));
    }

    #[test]
    fn test_build_diff_command() {
        let options = DiffOptions::new().staged();
        let cmd = build_diff_command(&options);
        assert!(cmd.contains("--staged"));

        let options = DiffOptions::new().revision("main...HEAD".to_string());
        let cmd = build_diff_command(&options);
        assert!(cmd.contains("main...HEAD"));
    }

    #[test]
    fn test_diff_options_builder() {
        let options = DiffOptions::new()
            .staged()
            .context(5)
            .with_stats()
            .path("src/".to_string());

        assert_eq!(options.scope, DiffScope::Staged);
        assert_eq!(options.context_lines, Some(5));
        assert!(options.stats);
        assert_eq!(options.path_filter, Some("src/".to_string()));
    }
}
