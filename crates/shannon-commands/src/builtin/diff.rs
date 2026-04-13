//! /diff command - Show git diff of changes

use crate::command::{Command, CommandBase, CommandSource, PromptCommand, ExecutionContext, CommandAvailability};

/// Prompt template for the /diff command.
///
/// Instructs the AI to run git diff and provide a structured analysis
/// using the same categories defined in [`ChangeCategory`].
const DIFF_PROMPT: &str = r##"
Analyze the git diff for this repository and provide a structured summary.

Steps:
1. Run `git diff {args}` — if args is empty, use `git diff HEAD` to show all uncommitted changes.
   For staged changes, the user would pass `--staged`.
   For a commit range, the user passes e.g. `main...HEAD`.
2. Read the diff output and categorize every changed line into these categories:
   - **function**: function or method definitions added/removed/modified
   - **import**: import, use, or include statements changed
   - **type**: struct, class, enum, or type definition changes
   - **test**: test functions or test-related code changed
   - **docs**: documentation comments changed
   - **config**: configuration file changes (Cargo.toml, package.json, YAML, etc.)
   - **other**: anything not in the above categories
3. Provide a summary including:
   - Total files changed, insertions, and deletions
   - Category breakdown (e.g., "function: 12, import: 3, test: 5")
   - Whether test changes are present (important for merge decisions)
   - Any potential risks or notable patterns

Format the output as a clear, concise summary with bullet points.
If the diff is large, focus on the most significant changes first.
"##;

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
        prompt_template: Some(DIFF_PROMPT.to_string()),
    })
}

/// Diff scope
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(Default)]
pub enum DiffScope {
    /// Unstaged changes (default)
    Unstaged,

    /// Staged changes
    Staged,

    /// Working tree (unstaged + staged)
    #[default]
    Working,

    /// HEAD vs working tree
    Head,

    /// Between commits/branches
    Commits,
}


/// Diff options
#[allow(dead_code)]
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

#[allow(dead_code)]
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
#[allow(dead_code)]
pub fn build_diff_command(options: &DiffOptions) -> String {
    let mut cmd = String::from("git diff");

    match options.scope {
        DiffScope::Staged => cmd.push_str(" --staged"),
        DiffScope::Working => cmd.push_str(" HEAD"),
        DiffScope::Commits => {
            if let Some(range) = &options.revision_range {
                cmd.push_str(&format!(" {range}"));
            }
        }
        _ => {}
    }

    if let Some(lines) = options.context_lines {
        cmd.push_str(&format!(" -U{lines}"));
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
        cmd.push_str(&format!(" -- {path}"));
    }

    cmd
}

/// Diff statistics
#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
static STATS_REGEX: once_cell::sync::Lazy<regex::Regex> =
    once_cell::sync::Lazy::new(|| {
        regex::Regex::new(r"(\d+) insertion[s]?,?\s*(\d+) deletion[s]?").unwrap()
    });

/// Common diff patterns
pub mod patterns {
    /// Match function/method definitions (Rust, Kotlin, Python, TypeScript)
    pub const FUNCTION_PATTERN: &str =
        r"^[\+\-]\s*((pub\s+)?(async\s+)?fn\s+\w+|(public|private|protected|static|async)\s+fun\s+\w+|def\s+\w+)";

    /// Match import statements
    pub const IMPORT_PATTERN: &str =
        r"^[\+\-]\s*(use\s+|import\s+|from\s+\S+\s+import|#include\s+)";

    /// Match struct/class/interface/enum definitions
    pub const STRUCT_PATTERN: &str =
        r"^[\+\-]\s*(pub\s+)?(struct|class|interface|enum|type)\s+\w+";

    /// Match test functions (Rust #[test], Python def test_, JS/TS test()/it())
    pub const TEST_PATTERN: &str =
        r"^[\+\-].*#\[test\]|^[\+\-].*#\[tokio::test\]|^[\+\-]\s*(fn|def|fun)\s+test_|^[\+\-].*\b(it|test|describe)\s*\(";

    /// Match doc comments (Rust ///, JS/Javadoc **, JS //@, Python """)
    pub const DOC_COMMENT_PATTERN: &str =
        r"^[\+\-]\s*///|^[\+\-]\s*\*\*|^[\+\-]\s*//\s*@";

    /// Match configuration changes (Cargo.toml, package.json, YAML, etc.)
    pub const CONFIG_PATTERN: &str =
        r"^[\+\-].*(dependencies|version|features|\[package\]|\[dependencies\])";
}

/// Category of a code change identified by the diff analyzer.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeCategory {
    /// Function or method definition changed
    Function,
    /// Import or include statement changed
    Import,
    /// Struct, class, enum, or type definition changed
    TypeDefinition,
    /// Test function changed
    Test,
    /// Documentation comment changed
    Documentation,
    /// Configuration file changed
    Configuration,
    /// Unclassified change
    Other,
}

#[allow(dead_code)]
impl ChangeCategory {
    /// Human-readable label for this category.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Import => "import",
            Self::TypeDefinition => "type",
            Self::Test => "test",
            Self::Documentation => "docs",
            Self::Configuration => "config",
            Self::Other => "other",
        }
    }
}

/// A single diff line that has been categorized.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct CategorizedChange {
    /// The category of this change.
    pub category: ChangeCategory,
    /// The original diff line (including +/- prefix).
    pub line: String,
    /// Line number in the diff output (1-based).
    pub line_number: usize,
}

/// Summary of categorized changes across an entire diff.
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct DiffAnalysis {
    /// Changes grouped by category.
    pub by_category: std::collections::HashMap<ChangeCategory, Vec<CategorizedChange>>,
}

#[allow(dead_code)]
impl DiffAnalysis {
    /// Count changes in a specific category.
    pub fn count(&self, category: ChangeCategory) -> usize {
        self.by_category.get(&category).map_or(0, |v| v.len())
    }

    /// Total number of categorized changes.
    pub fn total(&self) -> usize {
        self.by_category.values().map(|v| v.len()).sum()
    }

    /// Whether any test-related changes were detected.
    pub fn has_test_changes(&self) -> bool {
        self.count(ChangeCategory::Test) > 0
    }

    /// Render a brief summary of the analysis.
    pub fn summary(&self) -> String {
        if self.total() == 0 {
            return "No categorized changes.".to_string();
        }

        let mut parts = Vec::new();
        for cat in &[
            ChangeCategory::Function,
            ChangeCategory::TypeDefinition,
            ChangeCategory::Import,
            ChangeCategory::Test,
            ChangeCategory::Documentation,
            ChangeCategory::Configuration,
            ChangeCategory::Other,
        ] {
            let count = self.count(*cat);
            if count > 0 {
                parts.push(format!("{}: {}", cat.label(), count));
            }
        }
        parts.join(", ")
    }
}

/// Analyzes diff output to categorize changed lines.
#[allow(dead_code)]
pub struct DiffAnalyzer {
    function_re: regex::Regex,
    import_re: regex::Regex,
    struct_re: regex::Regex,
    test_re: regex::Regex,
    doc_re: regex::Regex,
    config_re: regex::Regex,
}

#[allow(dead_code)]
impl DiffAnalyzer {
    /// Create a new analyzer with compiled regex patterns.
    pub fn new() -> Self {
        Self {
            function_re: regex::Regex::new(patterns::FUNCTION_PATTERN).unwrap(),
            import_re: regex::Regex::new(patterns::IMPORT_PATTERN).unwrap(),
            struct_re: regex::Regex::new(patterns::STRUCT_PATTERN).unwrap(),
            test_re: regex::Regex::new(patterns::TEST_PATTERN).unwrap(),
            doc_re: regex::Regex::new(patterns::DOC_COMMENT_PATTERN).unwrap(),
            config_re: regex::Regex::new(patterns::CONFIG_PATTERN).unwrap(),
        }
    }

    /// Categorize a single diff line.
    pub fn categorize_line(&self, line: &str) -> ChangeCategory {
        // Order matters: test before function since test fns are also fns
        if self.test_re.is_match(line) {
            return ChangeCategory::Test;
        }
        if self.struct_re.is_match(line) {
            return ChangeCategory::TypeDefinition;
        }
        if self.import_re.is_match(line) {
            return ChangeCategory::Import;
        }
        if self.function_re.is_match(line) {
            return ChangeCategory::Function;
        }
        if self.doc_re.is_match(line) {
            return ChangeCategory::Documentation;
        }
        if self.config_re.is_match(line) {
            return ChangeCategory::Configuration;
        }
        ChangeCategory::Other
    }

    /// Analyze a full diff output, categorizing all changed lines.
    pub fn analyze(&self, diff_output: &str) -> DiffAnalysis {
        let mut analysis = DiffAnalysis::default();

        for (i, line) in diff_output.lines().enumerate() {
            // Only categorize addition/deletion lines (skip context, headers, etc.)
            if !line.starts_with('+') && !line.starts_with('-') {
                continue;
            }
            // Skip +++ / --- file headers
            if line.starts_with("+++") || line.starts_with("---") {
                continue;
            }

            let category = self.categorize_line(line);
            let change = CategorizedChange {
                category,
                line: line.to_string(),
                line_number: i + 1,
            };
            analysis
                .by_category
                .entry(category)
                .or_default()
                .push(change);
        }

        analysis
    }
}

impl Default for DiffAnalyzer {
    fn default() -> Self {
        Self::new()
    }
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

    // ── ChangeCategory Tests ──────────────────────────────────────────

    #[test]
    fn test_change_category_labels() {
        assert_eq!(ChangeCategory::Function.label(), "function");
        assert_eq!(ChangeCategory::Import.label(), "import");
        assert_eq!(ChangeCategory::TypeDefinition.label(), "type");
        assert_eq!(ChangeCategory::Test.label(), "test");
        assert_eq!(ChangeCategory::Documentation.label(), "docs");
        assert_eq!(ChangeCategory::Configuration.label(), "config");
        assert_eq!(ChangeCategory::Other.label(), "other");
    }

    // ── DiffAnalyzer Tests ────────────────────────────────────────────

    #[test]
    fn test_analyzer_categorizes_rust_fn() {
        let analyzer = DiffAnalyzer::new();
        assert_eq!(analyzer.categorize_line("+pub fn hello() {}"), ChangeCategory::Function);
        assert_eq!(analyzer.categorize_line("+async fn do_work() {}"), ChangeCategory::Function);
    }

    #[test]
    fn test_analyzer_categorizes_test_before_fn() {
        let analyzer = DiffAnalyzer::new();
        // Test pattern should take priority over function pattern
        assert_eq!(analyzer.categorize_line("+    fn test_something() {"), ChangeCategory::Test);
    }

    #[test]
    fn test_analyzer_categorizes_import() {
        let analyzer = DiffAnalyzer::new();
        assert_eq!(analyzer.categorize_line("+use std::collections::HashMap;"), ChangeCategory::Import);
        assert_eq!(analyzer.categorize_line("+import React from 'react';"), ChangeCategory::Import);
    }

    #[test]
    fn test_analyzer_categorizes_struct() {
        let analyzer = DiffAnalyzer::new();
        assert_eq!(analyzer.categorize_line("+pub struct MyStruct {"), ChangeCategory::TypeDefinition);
        assert_eq!(analyzer.categorize_line("+enum Color {"), ChangeCategory::TypeDefinition);
    }

    #[test]
    fn test_analyzer_categorizes_doc_comments() {
        let analyzer = DiffAnalyzer::new();
        assert_eq!(analyzer.categorize_line("+/// This is a doc comment"), ChangeCategory::Documentation);
    }

    #[test]
    fn test_analyzer_categorizes_config() {
        let analyzer = DiffAnalyzer::new();
        assert_eq!(analyzer.categorize_line("+version = \"1.0\""), ChangeCategory::Configuration);
    }

    #[test]
    fn test_analyzer_categorizes_other() {
        let analyzer = DiffAnalyzer::new();
        assert_eq!(analyzer.categorize_line("+    let x = 42;"), ChangeCategory::Other);
    }

    #[test]
    fn test_analyzer_skips_file_headers() {
        let analyzer = DiffAnalyzer::new();
        // +++ and --- are file headers, not categorizable changes
        assert_eq!(analyzer.categorize_line("+++ b/src/main.rs"), ChangeCategory::Other);
        assert_eq!(analyzer.categorize_line("--- a/src/main.rs"), ChangeCategory::Other);
    }

    // ── DiffAnalysis Tests ────────────────────────────────────────────

    #[test]
    fn test_diff_analysis_empty() {
        let analysis = DiffAnalysis::default();
        assert_eq!(analysis.total(), 0);
        assert_eq!(analysis.count(ChangeCategory::Function), 0);
        assert!(!analysis.has_test_changes());
        assert_eq!(analysis.summary(), "No categorized changes.");
    }

    #[test]
    fn test_diff_analysis_full() {
        let analyzer = DiffAnalyzer::new();
        let diff = "\
diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,5 @@
 use std::io;
+use std::collections::HashMap;
+/// Doc comment
+pub struct Config {
+    field: String,
+}
+pub fn main() {}
+    fn test_main() {}
+    let x = 42;
";

        let analysis = analyzer.analyze(diff);
        assert!(analysis.count(ChangeCategory::Import) >= 1, "should detect import");
        assert!(analysis.count(ChangeCategory::Documentation) >= 1, "should detect doc");
        assert!(analysis.count(ChangeCategory::TypeDefinition) >= 1, "should detect struct");
        assert!(analysis.count(ChangeCategory::Function) >= 1, "should detect fn");
        assert!(analysis.has_test_changes(), "should detect test fn");
        assert!(analysis.total() >= 5, "should categorize at least 5 lines");

        let summary = analysis.summary();
        assert!(summary.contains("import"), "summary should mention import");
        assert!(summary.contains("function"), "summary should mention function");
        assert!(summary.contains("test"), "summary should mention test");
    }
}
