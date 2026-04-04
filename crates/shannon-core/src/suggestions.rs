//! # Prompt Suggestions
//!
//! Context-aware prompt suggestion engine that generates relevant next-step
//! suggestions based on the current state of the project and conversation.
//!
//! ## Architecture
//!
//! Suggestions are produced by matching the current [`SuggestionContext`] against
//! a set of [`SuggestionRule`]s using regex patterns. Each matching rule yields
//! a [`Suggestion`] whose template is expanded with any regex capture groups.
//!
//! ## Example
//!
//! ```
//! use shannon_core::suggestions::{SuggestionEngine, SuggestionContext};
//! use shannon_core::tools::ToolInfo;
//! use serde_json::{Value, json};
//!
//! let engine = SuggestionEngine::new();
//! let context = SuggestionContext::AfterFileRead {
//!     file_path: "src/main.rs".to_string(),
//! };
//!
//! let suggestions = engine.suggest(&context, &[]);
//! assert!(!suggestions.is_empty());
//! ```

use crate::tools::ToolInfo;
use regex::Regex;
use serde::{Deserialize, Serialize};

/// Maximum number of suggestions returned by [`SuggestionEngine::suggest`].
const MAX_SUGGESTIONS: usize = 5;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// A single prompt suggestion ready for display.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Suggestion {
    /// Unique identifier for this suggestion.
    pub id: String,
    /// The suggested prompt text the user can submit.
    pub text: String,
    /// Human-readable explanation of why this is suggested.
    pub description: String,
    /// Broad category of the suggestion.
    pub category: SuggestionCategory,
    /// Relevance score in [0.0, 1.0]. Higher = more relevant.
    pub priority: f64,
    /// The context that triggered this suggestion.
    pub context: SuggestionContext,
}

/// Classification of a suggestion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SuggestionCategory {
    /// Logical next step in the workflow.
    NextAction,
    /// Code quality improvement.
    Improvement,
    /// Test-related action.
    Testing,
    /// Documentation generation or update.
    Documentation,
    /// Debugging / diagnosis.
    Debugging,
    /// Code exploration / understanding.
    Exploration,
}

impl std::fmt::Display for SuggestionCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NextAction => write!(f, "next_action"),
            Self::Improvement => write!(f, "improvement"),
            Self::Testing => write!(f, "testing"),
            Self::Documentation => write!(f, "documentation"),
            Self::Debugging => write!(f, "debugging"),
            Self::Exploration => write!(f, "exploration"),
        }
    }
}

/// Describes the situation in which the engine was asked for suggestions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SuggestionContext {
    /// A file was just read.
    AfterFileRead { file_path: String },
    /// A file was just edited.
    AfterFileEdit { file_path: String },
    /// An error was encountered.
    AfterError { error_message: String },
    /// Tests just ran.
    AfterTest { test_results: String },
    /// A new project session started.
    ProjectStart { project_path: String },
    /// The user has been idle (no recent actions).
    Idle,
}

impl SuggestionContext {
    /// Return a short string representation used as the haystack for rule matching.
    fn match_text(&self) -> String {
        match self {
            Self::AfterFileRead { file_path } => {
                format!("after_file_read:{}", file_path)
            }
            Self::AfterFileEdit { file_path } => {
                format!("after_file_edit:{}", file_path)
            }
            Self::AfterError { error_message } => {
                format!("after_error:{}", error_message)
            }
            Self::AfterTest { test_results } => {
                format!("after_test:{}", test_results)
            }
            Self::ProjectStart { project_path } => {
                format!("project_start:{}", project_path)
            }
            Self::Idle => "idle".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Rules
// ---------------------------------------------------------------------------

/// A single rule that can produce suggestions when its pattern matches.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestionRule {
    /// Unique identifier.
    pub id: String,
    /// Regex pattern matched against [`SuggestionContext::match_text`].
    pub pattern: String,
    /// Category for suggestions produced by this rule.
    pub category: SuggestionCategory,
    /// Suggestion text template. Capture groups are available as `{1}`, `{2}`, etc.
    pub template: String,
    /// Description template (same capture substitution applies).
    pub description: String,
    /// Base priority [0.0, 1.0].
    pub priority: f64,
}

impl SuggestionRule {
    /// Convenience constructor.
    pub fn new(
        id: impl Into<String>,
        pattern: impl Into<String>,
        category: SuggestionCategory,
        template: impl Into<String>,
        description: impl Into<String>,
        priority: f64,
    ) -> Self {
        Self {
            id: id.into(),
            pattern: pattern.into(),
            category,
            template: template.into(),
            description: description.into(),
            priority: priority.clamp(0.0, 1.0),
        }
    }
}

// ---------------------------------------------------------------------------
// SuggestionEngine
// ---------------------------------------------------------------------------

/// The core engine that matches rules against contexts to produce suggestions.
#[derive(Debug, Clone)]
pub struct SuggestionEngine {
    rules: Vec<SuggestionRule>,
}

impl Default for SuggestionEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl SuggestionEngine {
    /// Create a new engine pre-loaded with built-in rules.
    pub fn new() -> Self {
        Self {
            rules: builtin_rules(),
        }
    }

    /// Create an engine with only the given custom rules (no built-ins).
    pub fn with_custom_rules(rules: Vec<SuggestionRule>) -> Self {
        Self { rules }
    }

    /// Add a rule to the engine. If a rule with the same id already exists it is
    /// replaced.
    pub fn add_rule(&mut self, rule: SuggestionRule) {
        if let Some(pos) = self.rules.iter().position(|r| r.id == rule.id) {
            self.rules[pos] = rule;
        } else {
            self.rules.push(rule);
        }
    }

    /// Remove a rule by id. Returns `true` if a rule was removed.
    pub fn remove_rule(&mut self, id: &str) -> bool {
        if let Some(pos) = self.rules.iter().position(|r| r.id == id) {
            self.rules.remove(pos);
            true
        } else {
            false
        }
    }

    /// Return a reference to the current rules.
    pub fn rules(&self) -> &[SuggestionRule] {
        &self.rules
    }

    /// Generate suggestions for the given context, filtered by available tools.
    ///
    /// The returned vector is sorted by descending priority and capped at
    /// [`MAX_SUGGESTIONS`] entries.
    pub fn suggest(
        &self,
        context: &SuggestionContext,
        available_tools: &[ToolInfo],
    ) -> Vec<Suggestion> {
        let haystack = context.match_text();
        let tool_names: std::collections::HashSet<&str> =
            available_tools.iter().map(|t| t.name.as_str()).collect();

        let mut suggestions: Vec<Suggestion> = Vec::new();

        for rule in &self.rules {
            // Skip rules whose template references a tool that is not available.
            if !self.rule_tools_available(&rule.template, &tool_names) {
                continue;
            }

            let re = match Regex::new(&rule.pattern) {
                Ok(r) => r,
                Err(_) => continue, // skip malformed patterns silently
            };

            if let Some(caps) = re.captures(&haystack) {
                let text = expand_template(&rule.template, &caps);
                let description = expand_template(&rule.description, &caps);

                suggestions.push(Suggestion {
                    id: rule.id.clone(),
                    text,
                    description,
                    category: rule.category,
                    priority: rule.priority,
                    context: context.clone(),
                });
            }
        }

        // Sort by descending priority, then by id for stability.
        suggestions.sort_by(|a, b| {
            b.priority
                .partial_cmp(&a.priority)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.id.cmp(&b.id))
        });

        suggestions.truncate(MAX_SUGGESTIONS);
        suggestions
    }

    /// Check whether a template's `{tool:...}` placeholders are satisfied by the
    /// available tool set. If a template does not reference any tool it always
    /// passes.
    fn rule_tools_available(
        &self,
        template: &str,
        tool_names: &std::collections::HashSet<&str>,
    ) -> bool {
        for segment in template.split('{') {
            if let Some(rest) = segment.strip_prefix("tool:") {
                // rest is something like "Bash} to check status"
                // Take only the text up to the first '}'
                let tool_ref = rest.split('}').next().unwrap_or(rest);
                if !tool_names.contains(tool_ref) {
                    return false;
                }
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// Template expansion
// ---------------------------------------------------------------------------

/// Replace `{N}` placeholders with the Nth regex capture group (1-indexed).
/// Also replaces `{tool:X}` placeholders with just `X`.
fn expand_template(template: &str, caps: &regex::Captures) -> String {
    let mut result = template.to_string();

    // Replace {tool:X} with just X
    let mut offset = 0;
    while let Some(pos) = result[offset..].find("{tool:") {
        let abs_pos = pos + offset;
        let after_prefix = abs_pos + 6; // len of "{tool:"
        if let Some(end) = result[after_prefix..].find('}') {
            let abs_end = after_prefix + end;
            let tool_name: String = result[after_prefix..abs_end].to_string();
            result.replace_range(abs_pos..=abs_end, &tool_name);
            offset = abs_pos + tool_name.len();
        } else {
            break;
        }
    }

    // Replace {N} capture groups
    for i in 0..caps.len() {
        if i == 0 {
            continue; // skip the full match
        }
        let placeholder = format!("{{{}}}", i);
        if let Some(m) = caps.get(i) {
            result = result.replace(&placeholder, m.as_str());
        } else {
            result = result.replace(&placeholder, "");
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Built-in rules
// ---------------------------------------------------------------------------

/// Return the default set of suggestion rules.
fn builtin_rules() -> Vec<SuggestionRule> {
    vec![
        // 1. After reading a Rust source file -> suggest checking compilation
        SuggestionRule::new(
            "rust_check_compile",
            r"after_file_read:.*\.rs$",
            SuggestionCategory::Testing,
            "Check if the project compiles: cargo check",
            "Verify the project compiles after reading this Rust file",
            0.7,
        ),
        // 2. After reading a Rust source file -> suggest running tests
        SuggestionRule::new(
            "rust_run_tests",
            r"after_file_read:.*\.rs$",
            SuggestionCategory::Testing,
            "Run the test suite: cargo test",
            "Run tests to make sure everything passes",
            0.6,
        ),
        // 3. After reading a test file -> suggest running those tests
        SuggestionRule::new(
            "test_file_run",
            r"after_file_read:.*test",
            SuggestionCategory::Testing,
            "Run the tests in this file: cargo test",
            "Execute the tests in the file you just read",
            0.8,
        ),
        // 4. After editing a file -> suggest running related tests
        SuggestionRule::new(
            "edit_run_tests",
            r"after_file_edit:.*",
            SuggestionCategory::Testing,
            "Run tests to verify the change: cargo test",
            "Run tests to confirm your edit did not break anything",
            0.8,
        ),
        // 5. After editing a file -> suggest checking compilation
        SuggestionRule::new(
            "edit_check_compile",
            r"after_file_edit:.*\.rs$",
            SuggestionCategory::NextAction,
            "Check compilation: cargo check",
            "Verify the edited Rust file compiles",
            0.9,
        ),
        // 6. After a Rust compilation error -> suggest cargo fix
        SuggestionRule::new(
            "error_cargo_fix",
            r"after_error:(?i).*cannot find|no method|expected.*found|mismatched types|E0",
            SuggestionCategory::Debugging,
            "Try cargo fix to auto-resolve: cargo fix",
            "Automatically fix common compilation errors",
            0.85,
        ),
        // 7. After test failure -> suggest investigating the failure
        SuggestionRule::new(
            "test_failure_debug",
            r"after_test:(?i).*fail|FAILED|error",
            SuggestionCategory::Debugging,
            "Show me the failing test output with details",
            "Investigate which tests failed and why",
            0.9,
        ),
        // 8. After test failure -> suggest running a single test
        SuggestionRule::new(
            "test_failure_single",
            r"after_test:(?i).*fail|FAILED|error",
            SuggestionCategory::Testing,
            "Run only the failing test for faster iteration",
            "Isolate the failing test for quicker debug cycles",
            0.7,
        ),
        // 9. Project start -> suggest exploring structure
        SuggestionRule::new(
            "project_explore",
            r"project_start:.*",
            SuggestionCategory::Exploration,
            "Show me the project structure",
            "Get an overview of the codebase layout",
            0.9,
        ),
        // 10. Project start -> suggest reading CLAUDE.md
        SuggestionRule::new(
            "project_claude_md",
            r"project_start:.*",
            SuggestionCategory::Documentation,
            "Read CLAUDE.md for project instructions",
            "Load project-specific context and conventions",
            0.85,
        ),
        // 11. After reading a file -> suggest related files
        SuggestionRule::new(
            "file_read_related",
            r"after_file_read:(.*)",
            SuggestionCategory::Exploration,
            "Show me files related to {1}",
            "Find files that import or are imported by this file",
            0.5,
        ),
        // 12. After successful test -> suggest committing
        SuggestionRule::new(
            "test_success_commit",
            r"after_test:(?i).*passed|ok|test result: ok",
            SuggestionCategory::NextAction,
            "Commit these changes",
            "All tests pass -- a good time to commit",
            0.6,
        ),
        // 13. Idle -> suggest reviewing git status
        SuggestionRule::new(
            "idle_git_status",
            r"^idle$",
            SuggestionCategory::NextAction,
            "Show git status",
            "Check what files have changed since the last commit",
            0.4,
        ),
        // 14. After an error -> suggest searching for similar issues
        SuggestionRule::new(
            "error_search_issues",
            r"(?i)after_error:.*",
            SuggestionCategory::Debugging,
            "Search for similar errors in the codebase",
            "Look for patterns or prior fixes for this kind of error",
            0.6,
        ),
        // 15. After editing a Cargo.toml -> suggest cargo check
        SuggestionRule::new(
            "edit_cargo_toml",
            r"after_file_edit:.*Cargo\.toml$",
            SuggestionCategory::NextAction,
            "Run cargo check to validate dependency changes",
            "Ensure the dependency changes compile correctly",
            0.9,
        ),
    ]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolInfo;
    use serde_json::json;

    /// Helper: build a minimal `ToolInfo`.
    fn tool_info(name: &str) -> ToolInfo {
        ToolInfo {
            name: name.to_string(),
            description: String::new(),
            category: String::new(),
            requires_auth: false,
            input_schema: json!({}),
        }
    }

    // -----------------------------------------------------------------------
    // 1. Built-in rules loaded correctly
    // -----------------------------------------------------------------------
    #[test]
    fn builtin_rules_loaded() {
        let engine = SuggestionEngine::new();
        assert!(
            engine.rules().len() >= 10,
            "Expected at least 10 built-in rules, got {}",
            engine.rules().len()
        );
        // Spot-check a few known rule ids.
        let ids: Vec<&str> = engine.rules().iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"rust_check_compile"));
        assert!(ids.contains(&"test_file_run"));
        assert!(ids.contains(&"project_explore"));
        assert!(ids.contains(&"idle_git_status"));
    }

    // -----------------------------------------------------------------------
    // 2. Context matching works - AfterFileRead with .rs
    // -----------------------------------------------------------------------
    #[test]
    fn context_match_after_file_read_rs() {
        let engine = SuggestionEngine::new();
        let ctx = SuggestionContext::AfterFileRead {
            file_path: "src/main.rs".to_string(),
        };
        let suggestions = engine.suggest(&ctx, &[]);
        assert!(
            !suggestions.is_empty(),
            "Expected suggestions after reading a .rs file"
        );
        // Should include compilation check
        assert!(suggestions.iter().any(|s| s.id == "rust_check_compile"));
        assert!(suggestions.iter().any(|s| s.id == "rust_run_tests"));
    }

    // -----------------------------------------------------------------------
    // 3. Context matching works - AfterFileRead with test file
    // -----------------------------------------------------------------------
    #[test]
    fn context_match_after_file_read_test() {
        let engine = SuggestionEngine::new();
        let ctx = SuggestionContext::AfterFileRead {
            file_path: "src/test_helpers.rs".to_string(),
        };
        let suggestions = engine.suggest(&ctx, &[]);
        assert!(suggestions.iter().any(|s| s.id == "test_file_run"));
    }

    // -----------------------------------------------------------------------
    // 4. Context matching works - AfterFileEdit
    // -----------------------------------------------------------------------
    #[test]
    fn context_match_after_file_edit() {
        let engine = SuggestionEngine::new();
        let ctx = SuggestionContext::AfterFileEdit {
            file_path: "src/lib.rs".to_string(),
        };
        let suggestions = engine.suggest(&ctx, &[]);
        assert!(suggestions.iter().any(|s| s.id == "edit_run_tests"));
        assert!(suggestions.iter().any(|s| s.id == "edit_check_compile"));
    }

    // -----------------------------------------------------------------------
    // 5. Context matching works - AfterError
    // -----------------------------------------------------------------------
    #[test]
    fn context_match_after_error() {
        let engine = SuggestionEngine::new();
        let ctx = SuggestionContext::AfterError {
            error_message: "cannot find value `foo` in this scope E0425".to_string(),
        };
        let suggestions = engine.suggest(&ctx, &[]);
        assert!(suggestions.iter().any(|s| s.id == "error_cargo_fix"));
        assert!(suggestions.iter().any(|s| s.id == "error_search_issues"));
    }

    // -----------------------------------------------------------------------
    // 6. Context matching works - AfterTest (failure)
    // -----------------------------------------------------------------------
    #[test]
    fn context_match_after_test_failure() {
        let engine = SuggestionEngine::new();
        let ctx = SuggestionContext::AfterTest {
            test_results: "test foo ... FAILED".to_string(),
        };
        let suggestions = engine.suggest(&ctx, &[]);
        assert!(suggestions.iter().any(|s| s.id == "test_failure_debug"));
    }

    // -----------------------------------------------------------------------
    // 7. Context matching works - AfterTest (success)
    // -----------------------------------------------------------------------
    #[test]
    fn context_match_after_test_success() {
        let engine = SuggestionEngine::new();
        let ctx = SuggestionContext::AfterTest {
            test_results: "test result: ok. 42 passed".to_string(),
        };
        let suggestions = engine.suggest(&ctx, &[]);
        assert!(suggestions.iter().any(|s| s.id == "test_success_commit"));
    }

    // -----------------------------------------------------------------------
    // 8. Context matching works - ProjectStart
    // -----------------------------------------------------------------------
    #[test]
    fn context_match_project_start() {
        let engine = SuggestionEngine::new();
        let ctx = SuggestionContext::ProjectStart {
            project_path: "/tmp/my-project".to_string(),
        };
        let suggestions = engine.suggest(&ctx, &[]);
        assert!(suggestions.iter().any(|s| s.id == "project_explore"));
        assert!(suggestions.iter().any(|s| s.id == "project_claude_md"));
    }

    // -----------------------------------------------------------------------
    // 9. Context matching works - Idle
    // -----------------------------------------------------------------------
    #[test]
    fn context_match_idle() {
        let engine = SuggestionEngine::new();
        let ctx = SuggestionContext::Idle;
        let suggestions = engine.suggest(&ctx, &[]);
        assert!(suggestions.iter().any(|s| s.id == "idle_git_status"));
    }

    // -----------------------------------------------------------------------
    // 10. Priority sorting works
    // -----------------------------------------------------------------------
    #[test]
    fn suggestions_sorted_by_priority() {
        let engine = SuggestionEngine::new();
        let ctx = SuggestionContext::ProjectStart {
            project_path: "/tmp/proj".to_string(),
        };
        let suggestions = engine.suggest(&ctx, &[]);
        for window in suggestions.windows(2) {
            assert!(
                window[0].priority >= window[1].priority,
                "Suggestions not sorted: {} (p={}) >= {} (p={})",
                window[0].id,
                window[0].priority,
                window[1].id,
                window[1].priority
            );
        }
    }

    // -----------------------------------------------------------------------
    // 11. Top N limiting
    // -----------------------------------------------------------------------
    #[test]
    fn suggestions_capped_at_max() {
        let engine = SuggestionEngine::new();
        let ctx = SuggestionContext::AfterFileRead {
            file_path: "src/main.rs".to_string(),
        };
        let suggestions = engine.suggest(&ctx, &[]);
        assert!(
            suggestions.len() <= MAX_SUGGESTIONS,
            "Expected at most {} suggestions, got {}",
            MAX_SUGGESTIONS,
            suggestions.len()
        );
    }

    // -----------------------------------------------------------------------
    // 12. Custom rules work
    // -----------------------------------------------------------------------
    #[test]
    fn custom_rules() {
        let custom = vec![SuggestionRule::new(
            "custom_hello",
            r"^idle$",
            SuggestionCategory::Exploration,
            "Say hello to the world",
            "A friendly greeting suggestion",
            1.0,
        )];
        let engine = SuggestionEngine::with_custom_rules(custom);
        let suggestions = engine.suggest(&SuggestionContext::Idle, &[]);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].id, "custom_hello");
        assert_eq!(suggestions[0].text, "Say hello to the world");
    }

    // -----------------------------------------------------------------------
    // 13. Tool availability filtering
    // -----------------------------------------------------------------------
    #[test]
    fn tool_availability_filtering() {
        let engine = SuggestionEngine::with_custom_rules(vec![
            SuggestionRule::new(
                "needs_bash",
                r"^idle$",
                SuggestionCategory::NextAction,
                "Run {tool:Bash} to check status",
                "Needs the Bash tool",
                0.9,
            ),
            SuggestionRule::new(
                "no_tool_needed",
                r"^idle$",
                SuggestionCategory::NextAction,
                "Review your changes",
                "Does not need any tool",
                0.5,
            ),
        ]);

        let suggestions = engine.suggest(&SuggestionContext::Idle, &[]);
        // With no tools available, the Bash-dependent rule should be filtered out.
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].id, "no_tool_needed");

        // Now provide the Bash tool.
        let tools = vec![tool_info("Bash")];
        let suggestions = engine.suggest(&SuggestionContext::Idle, &tools);
        assert_eq!(suggestions.len(), 2);
    }

    // -----------------------------------------------------------------------
    // 14. Rule removal
    // -----------------------------------------------------------------------
    #[test]
    fn rule_removal() {
        let mut engine = SuggestionEngine::new();
        let initial_count = engine.rules().len();
        assert!(engine.remove_rule("rust_check_compile"));
        assert_eq!(engine.rules().len(), initial_count - 1);
        // Removing a non-existent rule returns false.
        assert!(!engine.remove_rule("does_not_exist"));
        assert_eq!(engine.rules().len(), initial_count - 1);
    }

    // -----------------------------------------------------------------------
    // 15. No matching rules returns empty
    // -----------------------------------------------------------------------
    #[test]
    fn no_matching_rules() {
        let engine = SuggestionEngine::with_custom_rules(vec![]);
        let ctx = SuggestionContext::AfterFileRead {
            file_path: "src/main.rs".to_string(),
        };
        let suggestions = engine.suggest(&ctx, &[]);
        assert!(suggestions.is_empty());
    }

    // -----------------------------------------------------------------------
    // 16. Template capture group expansion
    // -----------------------------------------------------------------------
    #[test]
    fn template_capture_expansion() {
        let engine = SuggestionEngine::with_custom_rules(vec![SuggestionRule::new(
            "related_files",
            r"after_file_read:(.*)",
            SuggestionCategory::Exploration,
            "Show files related to {1}",
            "Explore files related to {1}",
            0.5,
        )]);
        let ctx = SuggestionContext::AfterFileRead {
            file_path: "src/lib.rs".to_string(),
        };
        let suggestions = engine.suggest(&ctx, &[]);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(
            suggestions[0].text,
            "Show files related to src/lib.rs"
        );
        assert_eq!(
            suggestions[0].description,
            "Explore files related to src/lib.rs"
        );
    }

    // -----------------------------------------------------------------------
    // 17. Add rule replaces existing
    // -----------------------------------------------------------------------
    #[test]
    fn add_rule_replaces_existing() {
        let mut engine = SuggestionEngine::new();
        let count_before = engine.rules().len();
        engine.add_rule(SuggestionRule::new(
            "rust_check_compile",
            r"after_file_read:.*\.rs$",
            SuggestionCategory::NextAction,
            "New template",
            "New description",
            1.0,
        ));
        // Count should be the same -- existing rule replaced.
        assert_eq!(engine.rules().len(), count_before);
        let rule = engine.rules().iter().find(|r| r.id == "rust_check_compile").unwrap();
        assert_eq!(rule.template, "New template");
        assert_eq!(rule.priority, 1.0);
    }

    // -----------------------------------------------------------------------
    // 18. Malformed regex patterns are skipped
    // -----------------------------------------------------------------------
    #[test]
    fn malformed_regex_skipped() {
        let engine = SuggestionEngine::with_custom_rules(vec![SuggestionRule::new(
            "bad_regex",
            r"(?P<unclosed",
            SuggestionCategory::NextAction,
            "Should not appear",
            "Malformed regex",
            1.0,
        )]);
        let suggestions = engine.suggest(&SuggestionContext::Idle, &[]);
        assert!(suggestions.is_empty());
    }

    // -----------------------------------------------------------------------
    // 19. Each context variant produces suggestions with the right context
    // -----------------------------------------------------------------------
    #[test]
    fn suggestion_carries_context() {
        let engine = SuggestionEngine::new();
        let ctx = SuggestionContext::AfterError {
            error_message: "boom".to_string(),
        };
        let suggestions = engine.suggest(&ctx, &[]);
        for s in &suggestions {
            assert_eq!(s.context, ctx);
        }
    }

    // -----------------------------------------------------------------------
    // 20. Cargo.toml edit triggers specific rule
    // -----------------------------------------------------------------------
    #[test]
    fn edit_cargo_toml_rule() {
        let engine = SuggestionEngine::new();
        let ctx = SuggestionContext::AfterFileEdit {
            file_path: "Cargo.toml".to_string(),
        };
        let suggestions = engine.suggest(&ctx, &[]);
        assert!(suggestions.iter().any(|s| s.id == "edit_cargo_toml"));
    }
}
