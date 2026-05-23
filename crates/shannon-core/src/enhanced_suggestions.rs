//! # Enhanced Context Suggestions
//!
//! Context-aware suggestion engine that analyzes project state, recent edits,
//! file types, and conversation history to provide intelligent tool and file
//! suggestions. Inspired by Claude Code's unifiedSuggestions and fileSuggestions.
//!
//! ## Architecture
//!
//! - [`SuggestionTrigger`]: What kind of action triggered the suggestion request
//! - [`ContextualSuggestion`]: A single suggestion with metadata and confidence
//! - [`ContextSuggestionEngine`]: The engine that produces suggestions based on context
//!
//! ## Example
//!
//! ```
//! use shannon_core::enhanced_suggestions::{
//!     ContextSuggestionEngine, SuggestionTrigger, SuggestionContext,
//! };
//!
//! let engine = ContextSuggestionEngine::new();
//! let suggestions = engine.suggest_for_edit(
//!     "src/main.rs",
//!     &["src/lib.rs".to_string(), "src/config.rs".to_string()],
//! );
//! assert!(!suggestions.is_empty());
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors produced by the enhanced suggestion engine.
#[derive(Error, Debug)]
pub enum SuggestionError {
    #[error("No matching patterns found for trigger: {0}")]
    NoPatterns(String),

    #[error("Invalid file extension: {0}")]
    InvalidExtension(String),

    #[error("Context analysis failed: {0}")]
    AnalysisFailed(String),
}

// ---------------------------------------------------------------------------
// SuggestionTrigger
// ---------------------------------------------------------------------------

/// The type of action that triggered a suggestion request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SuggestionTrigger {
    /// A file was edited (or is about to be edited).
    FileEdit,
    /// A new file is being created.
    FileCreate,
    /// A tool was invoked (or is about to be invoked).
    ToolUse,
    /// A shell command was run.
    CommandRun,
    /// A new conversation just started.
    ConversationStart,
}

impl std::fmt::Display for SuggestionTrigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FileEdit => write!(f, "file_edit"),
            Self::FileCreate => write!(f, "file_create"),
            Self::ToolUse => write!(f, "tool_use"),
            Self::CommandRun => write!(f, "command_run"),
            Self::ConversationStart => write!(f, "conversation_start"),
        }
    }
}

// ---------------------------------------------------------------------------
// ContextualSuggestion
// ---------------------------------------------------------------------------

/// A single context-aware suggestion with associated metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextualSuggestion {
    /// Unique identifier.
    pub id: String,
    /// What triggered this suggestion.
    pub trigger: SuggestionTrigger,
    /// The suggested tool name (if applicable).
    pub suggested_tool: Option<String>,
    /// Suggested files to operate on.
    pub suggested_files: Vec<String>,
    /// Human-readable reason for this suggestion.
    pub reason: String,
    /// Priority in 0..=100. Higher = more important.
    pub priority: u8,
    /// Confidence in 0.0..=1.0.
    pub confidence: f64,
}

// ---------------------------------------------------------------------------
// SuggestionContext
// ---------------------------------------------------------------------------

/// External context passed into the suggestion engine.
#[derive(Debug, Clone, Default)]
pub struct SuggestionContext {
    /// Files recently edited in the session.
    pub recently_edited_files: Vec<String>,
    /// Files recently created in the session.
    pub recently_created_files: Vec<String>,
    /// Tools recently used.
    pub recently_used_tools: Vec<String>,
    /// Commands recently run.
    pub recently_run_commands: Vec<String>,
    /// The current working directory.
    pub working_directory: Option<String>,
    /// Open file tabs.
    pub open_files: Vec<String>,
}

// ---------------------------------------------------------------------------
// File-extension -> tool mappings
// ---------------------------------------------------------------------------

/// Maps a file extension to tools that are commonly useful for that type.
fn extension_tool_map() -> HashMap<&'static str, Vec<&'static str>> {
    let mut m = HashMap::new();
    m.insert("rs", vec!["cargo", "clippy", "rustfmt"]);
    m.insert("ts", vec!["tsc", "eslint", "prettier"]);
    m.insert("tsx", vec!["tsc", "eslint", "prettier"]);
    m.insert("js", vec!["eslint", "prettier", "node"]);
    m.insert("jsx", vec!["eslint", "prettier"]);
    m.insert("py", vec!["pytest", "ruff", "mypy", "black"]);
    m.insert("go", vec!["go test", "go vet", "gofmt"]);
    m.insert("java", vec!["javac", "junit"]);
    m.insert("rb", vec!["rspec", "rubocop"]);
    m.insert("md", vec!["markdownlint"]);
    m.insert("yaml", vec!["yamllint"]);
    m.insert("yml", vec!["yamllint"]);
    m.insert("json", vec!["jq", "jsonlint"]);
    m.insert("toml", vec!["cargo"]);
    m.insert("css", vec!["stylelint", "prettier"]);
    m.insert("scss", vec!["stylelint", "prettier"]);
    m.insert("html", vec!["htmlhint", "prettier"]);
    m.insert("sql", vec!["sqlfluff"]);
    m.insert("sh", vec!["shellcheck", "shfmt"]);
    m.insert("bash", vec!["shellcheck", "shfmt"]);
    m
}

/// Returns the file extension (without dot) or `None`.
fn file_extension(path: &str) -> Option<&str> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let dot = name.rfind('.')?;
    Some(&name[dot + 1..])
}

// ---------------------------------------------------------------------------
// ContextSuggestionEngine
// ---------------------------------------------------------------------------

/// The main engine for producing context-aware suggestions.
#[derive(Debug, Clone)]
pub struct ContextSuggestionEngine {
    /// Maximum suggestions to return from any single method.
    max_suggestions: usize,
}

impl Default for ContextSuggestionEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextSuggestionEngine {
    /// Create a new engine with default settings.
    pub fn new() -> Self {
        Self { max_suggestions: 5 }
    }

    /// Create with a custom limit.
    pub fn with_max_suggestions(max: usize) -> Self {
        Self {
            max_suggestions: max,
        }
    }

    // ---- Edit suggestions ------------------------------------------------

    /// Suggest follow-up actions after editing (or about to edit) a file.
    pub fn suggest_for_edit(
        &self,
        edited_file: &str,
        recently_edited: &[String],
    ) -> Vec<ContextualSuggestion> {
        let mut suggestions = Vec::new();

        // 1. Suggest running tests if a source file was edited
        let ext = file_extension(edited_file);
        if let Some(ext) = ext {
            if is_source_extension(ext) {
                suggestions.push(ContextualSuggestion {
                    id: uid(),
                    trigger: SuggestionTrigger::FileEdit,
                    suggested_tool: Some("test".to_string()),
                    suggested_files: vec![edited_file.to_string()],
                    reason: format!("Source file .{ext} was edited; running tests is recommended."),
                    priority: 80,
                    confidence: 0.85,
                });
            }

            // 2. Suggest linting/formatting based on extension
            if let Some(tools) = extension_tool_map().get(ext) {
                for tool in tools {
                    suggestions.push(ContextualSuggestion {
                        id: uid(),
                        trigger: SuggestionTrigger::FileEdit,
                        suggested_tool: Some(tool.to_string()),
                        suggested_files: vec![edited_file.to_string()],
                        reason: format!("Run {tool} for .{ext} files."),
                        priority: 60,
                        confidence: 0.70,
                    });
                }
            }
        }

        // 3. Suggest editing related files (co-occurring edits)
        let related: Vec<String> = recently_edited
            .iter()
            .filter(|f| **f != edited_file && files_related(edited_file, f))
            .take(3)
            .cloned()
            .collect();
        if !related.is_empty() {
            suggestions.push(ContextualSuggestion {
                id: uid(),
                trigger: SuggestionTrigger::FileEdit,
                suggested_tool: Some("edit".to_string()),
                suggested_files: related.clone(),
                reason: format!(
                    "These files were recently edited alongside {}: {}",
                    edited_file,
                    related.join(", ")
                ),
                priority: 70,
                confidence: 0.75,
            });
        }

        // 4. Suggest checking companion test file
        if let Some(test_file) = companion_test_file(edited_file) {
            suggestions.push(ContextualSuggestion {
                id: uid(),
                trigger: SuggestionTrigger::FileEdit,
                suggested_tool: Some("read".to_string()),
                suggested_files: vec![test_file.clone()],
                reason: format!("Companion test file: {test_file}"),
                priority: 65,
                confidence: 0.80,
            });
        }

        self.truncate(suggestions)
    }

    // ---- Creation suggestions --------------------------------------------

    /// Suggest actions after creating a new file.
    pub fn suggest_for_creation(
        &self,
        created_file: &str,
        recently_created: &[String],
    ) -> Vec<ContextualSuggestion> {
        let mut suggestions = Vec::new();

        let ext = file_extension(created_file);

        // 1. Suggest creating a companion test file
        if let Some(test_path) = companion_test_file(created_file) {
            let already_exists = recently_created.contains(&test_path);
            if !already_exists {
                suggestions.push(ContextualSuggestion {
                    id: uid(),
                    trigger: SuggestionTrigger::FileCreate,
                    suggested_tool: Some("write".to_string()),
                    suggested_files: vec![test_path.clone()],
                    reason: format!("Create tests for the new file: {test_path}"),
                    priority: 85,
                    confidence: 0.90,
                });
            }
        }

        // 2. If Rust, suggest adding module declaration
        if ext == Some("rs") {
            suggestions.push(ContextualSuggestion {
                id: uid(),
                trigger: SuggestionTrigger::FileCreate,
                suggested_tool: Some("edit".to_string()),
                suggested_files: vec!["mod.rs".to_string(), "lib.rs".to_string()],
                reason: "Add module declaration to the parent mod.rs or lib.rs.".to_string(),
                priority: 75,
                confidence: 0.80,
            });
        }

        // 3. Suggest creating a companion types/models file
        if let Some(types_path) = companion_types_file(created_file) {
            suggestions.push(ContextualSuggestion {
                id: uid(),
                trigger: SuggestionTrigger::FileCreate,
                suggested_tool: Some("write".to_string()),
                suggested_files: vec![types_path.clone()],
                reason: format!("Consider creating a types file: {types_path}"),
                priority: 50,
                confidence: 0.55,
            });
        }

        // 4. Suggest linting the new file
        if let Some(ext) = ext {
            if let Some(tools) = extension_tool_map().get(ext) {
                let tool = tools.first().unwrap_or(&"lint");
                suggestions.push(ContextualSuggestion {
                    id: uid(),
                    trigger: SuggestionTrigger::FileCreate,
                    suggested_tool: Some(tool.to_string()),
                    suggested_files: vec![created_file.to_string()],
                    reason: format!("Lint the new .{ext} file with {tool}."),
                    priority: 55,
                    confidence: 0.60,
                });
            }
        }

        // 5. Suggest adding documentation
        if ext == Some("rs") || ext == Some("py") || ext == Some("ts") {
            suggestions.push(ContextualSuggestion {
                id: uid(),
                trigger: SuggestionTrigger::FileCreate,
                suggested_tool: Some("edit".to_string()),
                suggested_files: vec![created_file.to_string()],
                reason: "Add module-level documentation (doc comments).".to_string(),
                priority: 45,
                confidence: 0.65,
            });
        }

        self.truncate(suggestions)
    }

    // ---- Tool-use suggestions --------------------------------------------

    /// Suggest tools based on the file types involved and recent activity.
    pub fn suggest_for_tool(
        &self,
        tool_name: &str,
        context: &SuggestionContext,
    ) -> Vec<ContextualSuggestion> {
        let mut suggestions = Vec::new();

        match tool_name {
            "read" => {
                // After reading, suggest edit or search
                suggestions.push(ContextualSuggestion {
                    id: uid(),
                    trigger: SuggestionTrigger::ToolUse,
                    suggested_tool: Some("edit".to_string()),
                    suggested_files: context.open_files.clone(),
                    reason: "You just read a file; you may want to edit it.".to_string(),
                    priority: 75,
                    confidence: 0.80,
                });
                suggestions.push(ContextualSuggestion {
                    id: uid(),
                    trigger: SuggestionTrigger::ToolUse,
                    suggested_tool: Some("grep".to_string()),
                    suggested_files: vec![],
                    reason: "Search for related symbols across the codebase.".to_string(),
                    priority: 55,
                    confidence: 0.60,
                });
            }
            "edit" => {
                suggestions.push(ContextualSuggestion {
                    id: uid(),
                    trigger: SuggestionTrigger::ToolUse,
                    suggested_tool: Some("test".to_string()),
                    suggested_files: vec![],
                    reason: "Run tests after editing to verify changes.".to_string(),
                    priority: 85,
                    confidence: 0.85,
                });
            }
            "bash" => {
                // If a test command was run, suggest next steps
                let had_test = context.recently_run_commands.iter().any(|c| {
                    c.contains("test") || c.contains("pytest") || c.contains("cargo test")
                });
                if had_test {
                    suggestions.push(ContextualSuggestion {
                        id: uid(),
                        trigger: SuggestionTrigger::ToolUse,
                        suggested_tool: Some("edit".to_string()),
                        suggested_files: vec![],
                        reason: "Tests ran; you may need to fix failing tests.".to_string(),
                        priority: 80,
                        confidence: 0.70,
                    });
                }
            }
            "write" => {
                suggestions.push(ContextualSuggestion {
                    id: uid(),
                    trigger: SuggestionTrigger::ToolUse,
                    suggested_tool: Some("read".to_string()),
                    suggested_files: vec![],
                    reason: "Verify the written file by reading it back.".to_string(),
                    priority: 50,
                    confidence: 0.55,
                });
            }
            "glob" => {
                suggestions.push(ContextualSuggestion {
                    id: uid(),
                    trigger: SuggestionTrigger::ToolUse,
                    suggested_tool: Some("read".to_string()),
                    suggested_files: vec![],
                    reason: "Read one of the discovered files.".to_string(),
                    priority: 70,
                    confidence: 0.75,
                });
            }
            _ => {}
        }

        // If tools have been used heavily, suggest a summary
        if context.recently_used_tools.len() > 3 {
            suggestions.push(ContextualSuggestion {
                id: uid(),
                trigger: SuggestionTrigger::ToolUse,
                suggested_tool: None,
                suggested_files: vec![],
                reason: "Many tools used; consider summarizing the changes.".to_string(),
                priority: 30,
                confidence: 0.50,
            });
        }

        self.truncate(suggestions)
    }

    // ---- Command suggestions ---------------------------------------------

    /// Suggest follow-up actions after running a command.
    pub fn suggest_for_command(
        &self,
        command: &str,
        context: &SuggestionContext,
    ) -> Vec<ContextualSuggestion> {
        let mut suggestions = Vec::new();

        let cmd_lower = command.to_lowercase();

        // Test failures
        if cmd_lower.contains("fail") || cmd_lower.contains("error") {
            suggestions.push(ContextualSuggestion {
                id: uid(),
                trigger: SuggestionTrigger::CommandRun,
                suggested_tool: Some("read".to_string()),
                suggested_files: context.recently_edited_files.clone(),
                reason: "Command reported failures; inspect recently edited files.".to_string(),
                priority: 90,
                confidence: 0.85,
            });
        }

        // Build success
        if cmd_lower.contains("cargo build")
            || cmd_lower.contains("npm run build")
            || cmd_lower.contains("compile")
        {
            suggestions.push(ContextualSuggestion {
                id: uid(),
                trigger: SuggestionTrigger::CommandRun,
                suggested_tool: Some("bash".to_string()),
                suggested_files: vec![],
                reason: "Build succeeded; run tests next.".to_string(),
                priority: 80,
                confidence: 0.80,
            });
        }

        // Git operations
        if cmd_lower.starts_with("git") {
            if cmd_lower.contains("commit") {
                suggestions.push(ContextualSuggestion {
                    id: uid(),
                    trigger: SuggestionTrigger::CommandRun,
                    suggested_tool: Some("bash".to_string()),
                    suggested_files: vec![],
                    reason: "After committing, consider pushing to the remote.".to_string(),
                    priority: 65,
                    confidence: 0.70,
                });
            }
            if cmd_lower.contains("checkout") || cmd_lower.contains("switch") {
                suggestions.push(ContextualSuggestion {
                    id: uid(),
                    trigger: SuggestionTrigger::CommandRun,
                    suggested_tool: Some("bash".to_string()),
                    suggested_files: vec![],
                    reason: "Branch switched; check for merge conflicts.".to_string(),
                    priority: 70,
                    confidence: 0.65,
                });
            }
        }

        // Install/dependency changes
        if cmd_lower.contains("install")
            || cmd_lower.contains("add ")
            || cmd_lower.contains("cargo add")
        {
            suggestions.push(ContextualSuggestion {
                id: uid(),
                trigger: SuggestionTrigger::CommandRun,
                suggested_tool: Some("bash".to_string()),
                suggested_files: vec![],
                reason: "Dependencies changed; rebuild to verify.".to_string(),
                priority: 75,
                confidence: 0.75,
            });
        }

        self.truncate(suggestions)
    }

    // ---- Conversation start suggestions ----------------------------------

    /// Suggest actions when a new conversation begins.
    pub fn suggest_for_conversation_start(
        &self,
        context: &SuggestionContext,
    ) -> Vec<ContextualSuggestion> {
        let mut suggestions = Vec::new();

        // If there are uncommitted changes, suggest git status
        suggestions.push(ContextualSuggestion {
            id: uid(),
            trigger: SuggestionTrigger::ConversationStart,
            suggested_tool: Some("bash".to_string()),
            suggested_files: vec![],
            reason: "Check git status to see uncommitted changes.".to_string(),
            priority: 60,
            confidence: 0.70,
        });

        // Suggest reading CLAUDE.md if it exists
        suggestions.push(ContextualSuggestion {
            id: uid(),
            trigger: SuggestionTrigger::ConversationStart,
            suggested_tool: Some("read".to_string()),
            suggested_files: vec!["CLAUDE.md".to_string()],
            reason: "Read CLAUDE.md for project-specific instructions.".to_string(),
            priority: 70,
            confidence: 0.65,
        });

        // If recently edited files exist, suggest continuing work
        if !context.recently_edited_files.is_empty() {
            let files = context
                .recently_edited_files
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>();
            suggestions.push(ContextualSuggestion {
                id: uid(),
                trigger: SuggestionTrigger::ConversationStart,
                suggested_tool: Some("read".to_string()),
                suggested_files: files.clone(),
                reason: format!("Resume work on recently edited files: {}", files.join(", ")),
                priority: 75,
                confidence: 0.75,
            });
        }

        self.truncate(suggestions)
    }

    // ---- Helpers --------------------------------------------------------

    fn truncate(&self, mut suggestions: Vec<ContextualSuggestion>) -> Vec<ContextualSuggestion> {
        suggestions.sort_by(|a, b| {
            b.priority.cmp(&a.priority).then(
                b.confidence
                    .partial_cmp(&a.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
        });
        suggestions.truncate(self.max_suggestions);
        suggestions
    }
}

// ---------------------------------------------------------------------------
// Free helper functions
// ---------------------------------------------------------------------------

/// Quick UUID-like unique ID for suggestions.
fn uid() -> String {
    uuid::Uuid::new_v4().to_string()[..8].to_string()
}

/// Heuristic: is this a source-code extension?
fn is_source_extension(ext: &str) -> bool {
    matches!(
        ext,
        "rs" | "ts"
            | "tsx"
            | "js"
            | "jsx"
            | "py"
            | "go"
            | "java"
            | "rb"
            | "c"
            | "cpp"
            | "h"
            | "hpp"
            | "zig"
            | "swift"
            | "kt"
    )
}

/// Simple heuristic for file relatedness (same directory or same extension).
fn files_related(a: &str, b: &str) -> bool {
    let dir_a = a.rfind('/').map(|i| &a[..i]);
    let dir_b = b.rfind('/').map(|i| &b[..i]);
    if dir_a == dir_b {
        return true;
    }
    file_extension(a) == file_extension(b)
}

/// Given a source file, guess the companion test file path.
fn companion_test_file(path: &str) -> Option<String> {
    let ext = file_extension(path)?;
    let test_ext = match ext {
        "rs" => Some("rs"),
        "py" => Some("py"),
        "ts" | "tsx" => Some("test.ts"),
        "js" | "jsx" => Some("test.js"),
        "go" => Some("go"),
        "java" => Some("java"),
        "rb" => Some("rb"),
        _ => return None,
    };

    // Common patterns: src/foo.rs -> tests/foo.rs or src/foo_test.rs
    let stem = path.strip_suffix(&format!(".{ext}")).unwrap_or(path);
    let name = stem.rsplit('/').next().unwrap_or(stem);

    let candidates = match ext {
        "rs" => vec![
            format!("tests/{}.rs", name),
            format!("{}{}", stem, "_test.rs"),
        ],
        "py" => vec![format!("test_{}", path), format!("tests/test_{}.py", name)],
        "ts" | "tsx" => vec![
            format!("{}.test.{}", stem, ext),
            format!("{}.spec.{}", stem, ext),
            format!("__tests__/{}.{}", name, test_ext.unwrap_or(ext)),
        ],
        "js" | "jsx" => vec![
            format!("{}.test.{}", stem, ext),
            format!("{}.spec.{}", stem, ext),
        ],
        "go" => vec![format!("{}_test.go", stem)],
        "java" => vec![format!("{}Test.java", stem)],
        "rb" => vec![format!("{}_test.rb", stem), format!("{}_spec.rb", stem)],
        _ => vec![],
    };

    Some(candidates.into_iter().next().unwrap_or_default())
}

/// Suggest a companion types file path.
fn companion_types_file(path: &str) -> Option<String> {
    let dir = path.rfind('/')?;
    let dir = &path[..dir];
    Some(format!("{dir}/types"))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> ContextSuggestionEngine {
        ContextSuggestionEngine::new()
    }

    // -- Trigger display ---------------------------------------------------

    #[test]
    fn trigger_display_file_edit() {
        assert_eq!(SuggestionTrigger::FileEdit.to_string(), "file_edit");
    }

    #[test]
    fn trigger_display_file_create() {
        assert_eq!(SuggestionTrigger::FileCreate.to_string(), "file_create");
    }

    #[test]
    fn trigger_display_tool_use() {
        assert_eq!(SuggestionTrigger::ToolUse.to_string(), "tool_use");
    }

    #[test]
    fn trigger_display_command_run() {
        assert_eq!(SuggestionTrigger::CommandRun.to_string(), "command_run");
    }

    #[test]
    fn trigger_display_conversation_start() {
        assert_eq!(
            SuggestionTrigger::ConversationStart.to_string(),
            "conversation_start"
        );
    }

    // -- File extension ----------------------------------------------------

    #[test]
    fn file_extension_simple() {
        assert_eq!(file_extension("main.rs"), Some("rs"));
    }

    #[test]
    fn file_extension_with_path() {
        assert_eq!(file_extension("src/foo/bar.ts"), Some("ts"));
    }

    #[test]
    fn file_extension_no_ext() {
        assert_eq!(file_extension("Makefile"), None);
    }

    #[test]
    fn file_extension_dotfile() {
        assert_eq!(file_extension(".gitignore"), Some("gitignore"));
    }

    // -- is_source_extension -----------------------------------------------

    #[test]
    fn is_source_ext_rs() {
        assert!(is_source_extension("rs"));
    }

    #[test]
    fn is_source_ext_ts() {
        assert!(is_source_extension("ts"));
    }

    #[test]
    fn is_source_ext_txt() {
        assert!(!is_source_extension("txt"));
    }

    // -- files_related -----------------------------------------------------

    #[test]
    fn related_same_dir() {
        assert!(files_related("src/main.rs", "src/lib.rs"));
    }

    #[test]
    fn related_same_ext() {
        assert!(files_related("src/main.rs", "lib/main.rs"));
    }

    #[test]
    fn not_related() {
        assert!(!files_related("src/main.rs", "docs/readme.md"));
    }

    // -- companion_test_file -----------------------------------------------

    #[test]
    fn companion_test_rust() {
        let result = companion_test_file("src/lib.rs");
        assert!(result.is_some());
        assert!(result.unwrap().contains("lib"));
    }

    #[test]
    fn companion_test_python() {
        let result = companion_test_file("app/models.py");
        assert!(result.is_some());
    }

    #[test]
    fn companion_test_none_for_md() {
        assert!(companion_test_file("README.md").is_none());
    }

    // -- suggest_for_edit --------------------------------------------------

    #[test]
    fn edit_suggests_tests_for_source() {
        let suggestions = engine().suggest_for_edit("src/main.rs", &[]);
        assert!(
            suggestions
                .iter()
                .any(|s| s.suggested_tool.as_deref() == Some("test"))
        );
    }

    #[test]
    fn edit_suggests_related_files() {
        let recent = vec!["src/lib.rs".to_string(), "src/config.rs".to_string()];
        let suggestions = engine().suggest_for_edit("src/main.rs", &recent);
        assert!(suggestions.iter().any(|s| !s.suggested_files.is_empty()));
    }

    #[test]
    fn edit_no_suggestions_for_txt() {
        let suggestions = engine().suggest_for_edit("notes.txt", &[]);
        // .txt has no tool mapping, but related files may still match
        // At minimum we should get a companion test suggestion (which will be None for .txt)
        assert!(suggestions.is_empty() || suggestions.len() < 3);
    }

    // -- suggest_for_creation ----------------------------------------------

    #[test]
    fn creation_suggests_test_file() {
        let suggestions = engine().suggest_for_creation("src/utils.rs", &[]);
        assert!(suggestions.iter().any(|s| {
            s.suggested_files
                .iter()
                .any(|f| f.contains("test") || f.contains("spec"))
        }));
    }

    #[test]
    fn creation_suggests_module_decl_for_rust() {
        let suggestions = engine().suggest_for_creation("src/parser.rs", &[]);
        assert!(suggestions.iter().any(|s| {
            s.suggested_files
                .iter()
                .any(|f| f == "mod.rs" || f == "lib.rs")
        }));
    }

    // -- suggest_for_tool --------------------------------------------------

    #[test]
    fn tool_suggests_edit_after_read() {
        let ctx = SuggestionContext::default();
        let suggestions = engine().suggest_for_tool("read", &ctx);
        assert!(
            suggestions
                .iter()
                .any(|s| s.suggested_tool.as_deref() == Some("edit"))
        );
    }

    #[test]
    fn tool_suggests_test_after_edit() {
        let ctx = SuggestionContext::default();
        let suggestions = engine().suggest_for_tool("edit", &ctx);
        assert!(
            suggestions
                .iter()
                .any(|s| s.suggested_tool.as_deref() == Some("test"))
        );
    }

    #[test]
    fn tool_no_suggestions_for_unknown() {
        let ctx = SuggestionContext::default();
        let suggestions = engine().suggest_for_tool("unknown_tool_xyz", &ctx);
        assert!(suggestions.is_empty());
    }

    // -- suggest_for_command -----------------------------------------------

    #[test]
    fn command_suggests_after_build() {
        let ctx = SuggestionContext::default();
        let suggestions = engine().suggest_for_command("cargo build", &ctx);
        assert!(suggestions.iter().any(|s| s.reason.contains("test")));
    }

    #[test]
    fn command_suggests_after_failure() {
        let ctx = SuggestionContext::default();
        let suggestions = engine().suggest_for_command("cargo test (failed 2)", &ctx);
        assert!(!suggestions.is_empty());
    }

    #[test]
    fn command_suggests_after_git_commit() {
        let ctx = SuggestionContext::default();
        let suggestions = engine().suggest_for_command("git commit -m 'feat: add parser'", &ctx);
        assert!(suggestions.iter().any(|s| s.reason.contains("push")));
    }

    // -- suggest_for_conversation_start ------------------------------------

    #[test]
    fn conversation_start_suggests_claude_md() {
        let ctx = SuggestionContext::default();
        let suggestions = engine().suggest_for_conversation_start(&ctx);
        assert!(
            suggestions
                .iter()
                .any(|s| { s.suggested_files.iter().any(|f| f == "CLAUDE.md") })
        );
    }

    #[test]
    fn conversation_start_resumes_work() {
        let ctx = SuggestionContext {
            recently_edited_files: vec!["src/main.rs".to_string()],
            ..Default::default()
        };
        let suggestions = engine().suggest_for_conversation_start(&ctx);
        assert!(suggestions.iter().any(|s| s.reason.contains("Resume")));
    }

    // -- Truncation --------------------------------------------------------

    #[test]
    fn max_suggestions_respected() {
        let engine = ContextSuggestionEngine::with_max_suggestions(2);
        let s = engine.suggest_for_edit(
            "src/main.rs",
            &["src/lib.rs".to_string(), "src/config.rs".to_string()],
        );
        assert!(s.len() <= 2);
    }

    // -- Confidence & priority bounds --------------------------------------

    #[test]
    fn confidence_within_bounds() {
        let suggestions = engine().suggest_for_edit("src/main.rs", &[]);
        for s in &suggestions {
            assert!(
                (0.0..=1.0).contains(&s.confidence),
                "Confidence {} out of bounds",
                s.confidence
            );
        }
    }

    #[test]
    fn priority_within_bounds() {
        let suggestions = engine().suggest_for_creation("src/foo.rs", &[]);
        for s in &suggestions {
            assert!(s.priority <= 100, "Priority {} out of bounds", s.priority);
        }
    }

    // -- Serialization round-trip ------------------------------------------

    #[test]
    fn suggestion_serialization_roundtrip() {
        let s = ContextualSuggestion {
            id: "abc123".to_string(),
            trigger: SuggestionTrigger::FileEdit,
            suggested_tool: Some("edit".to_string()),
            suggested_files: vec!["foo.rs".to_string()],
            reason: "test".to_string(),
            priority: 80,
            confidence: 0.9,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: ContextualSuggestion = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    // -- Error type --------------------------------------------------------

    #[test]
    fn error_display() {
        let err = SuggestionError::NoPatterns("file_edit".to_string());
        assert!(err.to_string().contains("file_edit"));
    }
}
