//! # Auto-Memory User Preference Detection and Storage
//!
//! Automatically detects user preferences from conversations (corrections, style
//! choices, workflow preferences) and persists them for cross-session recall.
//!
//! ## Architecture
//!
//! - [`PreferenceMemoryManager`]: Top-level manager that loads, detects, stores,
//!   and formats user preferences.
//! - [`PreferenceEntry`]: A single preference rule with metadata.
//! - [`PreferenceCategory`]: Classification of preference kinds.
//! - [`PreferencePriority`]: Importance level for prompt injection ordering.
//!
//! Preferences are stored in a human-readable Markdown file (`preferences.md`)
//! under the configured storage directory.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::RwLock;
use thiserror::Error;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during preference memory operations.
#[derive(Error, Debug)]
pub enum PreferenceMemoryError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to parse preferences file: {0}")]
    ParseError(String),
}

// ============================================================================
// Types
// ============================================================================

/// A single preference entry detected from conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreferenceEntry {
    /// The preference rule (e.g., "Use TypeScript strict mode").
    pub rule: String,
    /// Why this preference exists (context from conversation).
    pub context: String,
    /// Category of preference.
    pub category: PreferenceCategory,
    /// When this was first observed.
    pub created_at: DateTime<Utc>,
    /// How many times this preference has been reinforced.
    pub reinforcement_count: usize,
    /// Priority level for prompt injection ordering.
    pub priority: PreferencePriority,
}

/// Classification of preference kinds.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum PreferenceCategory {
    /// Coding style preferences (naming, formatting).
    Style,
    /// Tool usage preferences (always use TDD, never mock DB).
    Workflow,
    /// Communication preferences (terse responses, no summaries).
    Communication,
    /// Project-specific conventions (use Redis, avoid library X).
    Project,
    /// General preferences (language, framework choices).
    General,
}

impl std::fmt::Display for PreferenceCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PreferenceCategory::Style => write!(f, "Style"),
            PreferenceCategory::Workflow => write!(f, "Workflow"),
            PreferenceCategory::Communication => write!(f, "Communication"),
            PreferenceCategory::Project => write!(f, "Project"),
            PreferenceCategory::General => write!(f, "General"),
        }
    }
}

impl PreferenceCategory {
    /// Parse from the heading text used in the markdown file.
    fn from_heading(s: &str) -> Option<Self> {
        match s.trim() {
            "Style" => Some(PreferenceCategory::Style),
            "Workflow" => Some(PreferenceCategory::Workflow),
            "Communication" => Some(PreferenceCategory::Communication),
            "Project" => Some(PreferenceCategory::Project),
            "General" => Some(PreferenceCategory::General),
            _ => None,
        }
    }
}

/// Importance level for prompt injection ordering.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum PreferencePriority {
    Low,
    Normal,
    High,
}

impl std::fmt::Display for PreferencePriority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PreferencePriority::Low => write!(f, "LOW"),
            PreferencePriority::Normal => write!(f, "NORMAL"),
            PreferencePriority::High => write!(f, "HIGH"),
        }
    }
}

impl PreferencePriority {
    /// Parse from the bracket notation used in the markdown file.
    fn from_bracket(s: &str) -> Option<Self> {
        match s.trim().to_uppercase().as_str() {
            "LOW" => Some(PreferencePriority::Low),
            "NORMAL" => Some(PreferencePriority::Normal),
            "HIGH" => Some(PreferencePriority::High),
            _ => None,
        }
    }
}

// ============================================================================
// Preference Memory Manager
// ============================================================================

/// Manages user preference memory -- auto-detected from conversations and
/// persisted for cross-session recall.
///
/// Thread-safe via `RwLock` so it can be shared across async tasks.
pub struct PreferenceMemoryManager {
    /// Directory for preference files.
    storage_dir: PathBuf,
    /// Loaded preferences for current session.
    preferences: RwLock<Vec<PreferenceEntry>>,
}

// Filenames used for persistence.
const PREFERENCES_FILE: &str = "preferences.md";
const FILE_HEADER: &str = "# Shannon Code - User Preferences\n";

impl PreferenceMemoryManager {
    /// Create a new manager backed by the given storage directory.
    ///
    /// The directory is created if it does not exist. Existing preferences are
    /// loaded from disk automatically.
    pub fn new(storage_dir: PathBuf) -> Self {
        let manager = Self {
            storage_dir,
            preferences: RwLock::new(Vec::new()),
        };
        // Best-effort load; ignore errors (file may not exist yet).
        let _ = manager.load_preferences();
        manager
    }

    /// Load preferences from the `preferences.md` file.
    ///
    /// Returns the loaded entries. On success the in-memory list is replaced.
    pub fn load_preferences(&self) -> Result<Vec<PreferenceEntry>, PreferenceMemoryError> {
        let path = self.storage_dir.join(PREFERENCES_FILE);
        if !path.exists() {
            return Ok(Vec::new());
        }

        let contents = fs::read_to_string(&path)?;
        let entries = parse_preferences_markdown(&contents)?;

        let mut prefs = self
            .preferences
            .write()
            .map_err(|e| PreferenceMemoryError::Io(std::io::Error::other(e.to_string())))?;
        prefs.clear();
        prefs.extend(entries.clone());

        Ok(entries)
    }

    /// Persist current preferences to disk as markdown.
    pub fn save_preferences(&self) -> Result<(), PreferenceMemoryError> {
        let prefs = self
            .preferences
            .read()
            .map_err(|e| PreferenceMemoryError::Io(std::io::Error::other(e.to_string())))?;

        fs::create_dir_all(&self.storage_dir)?;

        let markdown = format_preferences_markdown(&prefs);
        let path = self.storage_dir.join(PREFERENCES_FILE);
        fs::write(&path, markdown)?;

        Ok(())
    }

    /// Heuristic detection of preferences from a single conversation turn.
    ///
    /// Examines the user message for correction patterns, explicit preference
    /// statements, confirmations, and style directives. The assistant message
    /// is used as context for confirmation patterns.
    ///
    /// Returns `Some(PreferenceEntry)` when a preference is detected, or
    /// `None` otherwise.
    pub fn detect_from_message(
        &self,
        user_msg: &str,
        assistant_msg: &str,
    ) -> Option<PreferenceEntry> {
        let user_lower = user_msg.to_lowercase();

        // Try each detection strategy in priority order.
        if let Some(entry) = detect_correction(&user_lower, user_msg) {
            return Some(entry);
        }

        if let Some(entry) = detect_explicit_preference(&user_lower, user_msg) {
            return Some(entry);
        }

        if let Some(entry) = detect_style_directive(&user_lower, user_msg) {
            return Some(entry);
        }

        if let Some(entry) = detect_confirmation(&user_lower, user_msg, assistant_msg) {
            return Some(entry);
        }

        None
    }

    /// Add a new preference, merging with an existing similar one if found.
    ///
    /// Two preferences are considered similar when their rules share enough
    /// keywords (word-level Jaccard similarity > 0.5). When merging, the
    /// reinforcement count is incremented and the priority is promoted to
    /// the higher of the two.
    pub fn add_preference(&self, entry: PreferenceEntry) -> Result<(), PreferenceMemoryError> {
        let mut prefs = self
            .preferences
            .write()
            .map_err(|e| PreferenceMemoryError::Io(std::io::Error::other(e.to_string())))?;

        // Look for a similar existing preference to merge with.
        let similar_idx = prefs.iter().position(|existing| {
            rule_similarity(&existing.rule, &entry.rule) > 0.5
        });

        if let Some(idx) = similar_idx {
            let existing = &mut prefs[idx];
            existing.reinforcement_count += 1;
            if entry.priority > existing.priority {
                existing.priority = entry.priority;
            }
            // Preserve the more specific context.
            if entry.context.len() > existing.context.len() {
                existing.context = entry.context;
            }
        } else {
            prefs.push(entry);
        }

        drop(prefs);
        self.save_preferences()?;
        Ok(())
    }

    /// Format all preferences for injection into an LLM system prompt.
    ///
    /// Returns an empty string when no preferences exist. High-priority entries
    /// are listed first within each category.
    pub fn get_preferences_for_prompt(&self) -> String {
        let prefs = match self.preferences.read() {
            Ok(p) => p,
            Err(_) => return String::new(),
        };

        if prefs.is_empty() {
            return String::new();
        }

        let mut lines: Vec<String> = Vec::new();
        lines.push("## User Preferences".to_string());
        lines.push(String::new());

        for cat in &[
            PreferenceCategory::Style,
            PreferenceCategory::Workflow,
            PreferenceCategory::Communication,
            PreferenceCategory::Project,
            PreferenceCategory::General,
        ] {
            let mut entries: Vec<&PreferenceEntry> = prefs
                .iter()
                .filter(|e| e.category == *cat)
                .collect();

            if entries.is_empty() {
                continue;
            }

            // Sort: High first, then Normal, then Low.
            entries.sort_by(|a, b| b.priority.cmp(&a.priority));

            lines.push(format!("### {cat}"));
            for entry in entries {
                lines.push(format!("- {}", entry.rule));
            }
            lines.push(String::new());
        }

        lines.join("\n")
    }

    /// Increment the reinforcement count for any preference whose rule
    /// contains the given substring.
    pub fn reinforce_preference(
        &self,
        rule_substring: &str,
    ) -> Result<(), PreferenceMemoryError> {
        let mut prefs = self
            .preferences
            .write()
            .map_err(|e| PreferenceMemoryError::Io(std::io::Error::other(e.to_string())))?;

        let lower_sub = rule_substring.to_lowercase();
        let mut changed = false;

        for entry in prefs.iter_mut() {
            if entry.rule.to_lowercase().contains(&lower_sub) {
                entry.reinforcement_count += 1;
                changed = true;
            }
        }

        drop(prefs);

        if changed {
            self.save_preferences()?;
        }

        Ok(())
    }
}

// ============================================================================
// Detection Heuristics
// ============================================================================

/// Patterns that signal the user is correcting the assistant's behavior.
const CORRECTION_PATTERNS: &[&str] = &[
    "no, don't",
    "no don't",
    "stop doing",
    "actually use",
    "instead of",
    "not like that",
    "don't do that",
    "i said",
    "that's wrong",
    "thats wrong",
    "no, use",
    "no use",
    "i don't want",
    "i dont want",
];

/// Patterns for explicit preference statements.
const PREFERENCE_PATTERNS: &[&str] = &[
    "i prefer",
    "always use",
    "never use",
    "never do",
    "make sure to",
    "make sure you",
    "please always",
    "please never",
    "i always",
    "i never",
    "from now on",
    "going forward",
    "by default",
    "i like it when",
    "i want you to",
    "i expect",
];

/// Patterns for style/formatting directives.
const STYLE_PATTERNS: &[&str] = &[
    "use snake_case",
    "use camelcase",
    "use pascalcase",
    "use kebab-case",
    "keep functions short",
    "no comments needed",
    "no comments",
    "keep it concise",
    "use tabs",
    "use spaces",
    "single quotes",
    "double quotes",
    "max line length",
    "no semicolons",
    "use semicolons",
    "use trailing commas",
    "no trailing commas",
    "indent with",
    "use 2 spaces",
    "use 4 spaces",
];

/// Patterns for positive confirmations (used together with the assistant message).
const CONFIRMATION_PATTERNS: &[&str] = &[
    "yes exactly",
    "that's right",
    "thats right",
    "perfect",
    "exactly what",
    "that's exactly",
    "thats exactly",
    "just like that",
    "this is what i want",
    "this is what i meant",
    "good, now",
    "great, now",
];

/// Detect a correction preference from the user message.
fn detect_correction(user_lower: &str, user_original: &str) -> Option<PreferenceEntry> {
    for pattern in CORRECTION_PATTERNS {
        if user_lower.contains(pattern) {
            let rule = extract_rule_from_correction(user_original);
            return Some(PreferenceEntry {
                rule,
                context: user_original.to_string(),
                category: PreferenceCategory::Workflow,
                created_at: Utc::now(),
                reinforcement_count: 0,
                priority: PreferencePriority::High,
            });
        }
    }
    None
}

/// Detect an explicit preference statement.
fn detect_explicit_preference(user_lower: &str, user_original: &str) -> Option<PreferenceEntry> {
    for pattern in PREFERENCE_PATTERNS {
        if user_lower.contains(pattern) {
            let rule = extract_rule_from_preference(user_original);
            let category = classify_preference_category(user_lower);
            let priority = determine_priority(user_lower);
            return Some(PreferenceEntry {
                rule,
                context: user_original.to_string(),
                category,
                created_at: Utc::now(),
                reinforcement_count: 0,
                priority,
            });
        }
    }
    None
}

/// Detect a style/formatting directive.
fn detect_style_directive(user_lower: &str, user_original: &str) -> Option<PreferenceEntry> {
    for pattern in STYLE_PATTERNS {
        if user_lower.contains(pattern) {
            return Some(PreferenceEntry {
                rule: capitalize_first(user_original.trim()),
                context: user_original.to_string(),
                category: PreferenceCategory::Style,
                created_at: Utc::now(),
                reinforcement_count: 0,
                priority: PreferencePriority::Normal,
            });
        }
    }
    None
}

/// Detect a positive confirmation of the assistant's behavior.
///
/// This only triggers when the user confirms something specific the assistant
/// did, converting it into a preference for future sessions.
fn detect_confirmation(
    user_lower: &str,
    user_original: &str,
    assistant_msg: &str,
) -> Option<PreferenceEntry> {
    // Only trigger confirmations if the assistant message is non-trivial
    // (contains a code block or a specific action description).
    if !assistant_msg.contains("```") && assistant_msg.len() < 50 {
        return None;
    }

    for pattern in CONFIRMATION_PATTERNS {
        if user_lower.contains(pattern) {
            let rule = extract_rule_from_confirmation(user_original, assistant_msg);
            return Some(PreferenceEntry {
                rule,
                context: format!("User confirmed: {}", user_original.trim()),
                category: PreferenceCategory::General,
                created_at: Utc::now(),
                reinforcement_count: 0,
                priority: PreferencePriority::Low,
            });
        }
    }
    None
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Extract a concise rule from a correction message.
fn extract_rule_from_correction(msg: &str) -> String {
    let trimmed = msg.trim();
    // Remove leading "no, " / "no " style prefixes and capitalize.
    let cleaned = trimmed
        .trim_start_matches("no, ")
        .trim_start_matches("no ")
        .trim_start_matches("stop ")
        .trim();

    // If the message is long, truncate to the first sentence.
    let rule = first_sentence(cleaned);
    capitalize_first(&rule)
}

/// Extract a concise rule from a preference statement.
fn extract_rule_from_preference(msg: &str) -> String {
    let trimmed = msg.trim();
    let rule = first_sentence(trimmed);
    capitalize_first(&rule)
}

/// Extract a concise rule from a confirmation, incorporating context from the
/// assistant message.
fn extract_rule_from_confirmation(user_msg: &str, assistant_msg: &str) -> String {
    // Take the first line of the assistant message as the behavior context.
    let assistant_summary = assistant_msg
        .lines()
        .next()
        .unwrap_or("")
        .trim();

    // If the assistant line is too long, truncate.
    let summary = if assistant_summary.len() > 100 {
        format!("{}...", &assistant_summary[..97])
    } else {
        assistant_summary.to_string()
    };

    format!(
        "Confirmed approach: {}",
        if summary.is_empty() {
            user_msg.trim()
        } else {
            &summary
        }
    )
}

/// Get the first sentence from text (delimited by `.`, `!`, `?`, or newline).
fn first_sentence(text: &str) -> String {
    for (i, c) in text.char_indices() {
        if c == '.' || c == '!' || c == '?' || c == '\n' {
            return text[..i].trim().to_string();
        }
    }
    text.trim().to_string()
}

/// Capitalize the first letter of a string.
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Classify a preference into a category based on keyword content.
fn classify_preference_category(lower: &str) -> PreferenceCategory {
    if lower.contains("test") || lower.contains("build") || lower.contains("deploy") || lower.contains("commit") {
        PreferenceCategory::Workflow
    } else if lower.contains("response") || lower.contains("explain") || lower.contains("summar") || lower.contains("concise") {
        PreferenceCategory::Communication
    } else if lower.contains("project") || lower.contains("our ") || lower.contains("we ") {
        PreferenceCategory::Project
    } else {
        PreferenceCategory::General
    }
}

/// Determine priority from the strength of the preference signal.
fn determine_priority(lower: &str) -> PreferencePriority {
    if lower.contains("always") || lower.contains("never") || lower.contains("must") {
        PreferencePriority::High
    } else if lower.contains("prefer") || lower.contains("by default") || lower.contains("please") {
        PreferencePriority::Normal
    } else {
        PreferencePriority::Low
    }
}

/// Compute word-level Jaccard similarity between two rules.
fn rule_similarity(a: &str, b: &str) -> f64 {
    let a_lower = a.to_lowercase();
    let b_lower = b.to_lowercase();
    let words_a: HashSet<&str> = a_lower.split_whitespace().collect();
    let words_b: HashSet<&str> = b_lower.split_whitespace().collect();

    if words_a.is_empty() && words_b.is_empty() {
        return 1.0;
    }

    let intersection = words_a.intersection(&words_b).count();
    let union = words_a.union(&words_b).count();

    if union == 0 {
        return 1.0;
    }

    intersection as f64 / union as f64
}

// ============================================================================
// Markdown Serialization / Deserialization
// ============================================================================

/// Format preferences as a human-readable Markdown document.
fn format_preferences_markdown(entries: &[PreferenceEntry]) -> String {
    let mut out = String::from(FILE_HEADER);

    for cat in &[
        PreferenceCategory::Style,
        PreferenceCategory::Workflow,
        PreferenceCategory::Communication,
        PreferenceCategory::Project,
        PreferenceCategory::General,
    ] {
        let entries_for_cat: Vec<&PreferenceEntry> =
            entries.iter().filter(|e| e.category == *cat).collect();

        if entries_for_cat.is_empty() {
            continue;
        }

        out.push_str(&format!("\n## {cat}\n"));

        for entry in &entries_for_cat {
            let reinforcement = if entry.reinforcement_count > 0 {
                format!(" (reinforced {} times)", entry.reinforcement_count)
            } else {
                String::new()
            };
            out.push_str(&format!(
                "- [{}] {}\n  Context: {}{}\n",
                entry.priority, entry.rule, entry.context, reinforcement
            ));
        }
    }

    out
}

/// Parse preferences from the Markdown format produced by
/// [`format_preferences_markdown`].
fn parse_preferences_markdown(
    contents: &str,
) -> Result<Vec<PreferenceEntry>, PreferenceMemoryError> {
    let mut entries = Vec::new();
    let mut current_category: Option<PreferenceCategory> = None;

    for line in contents.lines() {
        let trimmed = line.trim();

        // Category heading: "## Style"
        if trimmed.starts_with("## ") {
            let heading = trimmed.trim_start_matches("## ").trim();
            current_category = PreferenceCategory::from_heading(heading);
            continue;
        }

        // Skip header and blank lines.
        if trimmed.starts_with("# ") || trimmed.is_empty() {
            continue;
        }

        // Preference line: "- [HIGH] Use TypeScript strict mode"
        if trimmed.starts_with("- [") {
            let cat = current_category.unwrap_or(PreferenceCategory::General);

            // Extract priority from brackets.
            let close_bracket = match trimmed.find(']') {
                Some(idx) => idx,
                None => continue,
            };
            let priority_str = &trimmed[3..close_bracket];
            let priority = PreferencePriority::from_bracket(priority_str)
                .unwrap_or(PreferencePriority::Normal);

            // Rule is everything after "] ".
            let rest = &trimmed[close_bracket + 1..].trim_start();
            let rule = rest.to_string();

            entries.push(PreferenceEntry {
                rule,
                context: String::new(),
                category: cat,
                created_at: Utc::now(),
                reinforcement_count: 0,
                priority,
            });
            continue;
        }

        // Context line: "  Context: ..."
        if trimmed.starts_with("Context:") {
            if let Some(last) = entries.last_mut() {
                let ctx = trimmed.trim_start_matches("Context:").trim();
                // Strip trailing reinforcement note if present.
                let ctx = match ctx.find("(reinforced") {
                    Some(pos) => ctx[..pos].trim().to_string(),
                    None => ctx.to_string(),
                };
                last.context = ctx;

                // Parse reinforcement count from the same line.
                if let Some(paren_start) = trimmed.find("(reinforced") {
                    let count_str = &trimmed[paren_start..];
                    if let Some(count) = parse_reinforcement_count(count_str) {
                        last.reinforcement_count = count;
                    }
                }
            }
        }
    }

    Ok(entries)
}

/// Parse reinforcement count from a string like "(reinforced 3 times)".
fn parse_reinforcement_count(s: &str) -> Option<usize> {
    // Find the number between "reinforced " and " times".
    let start = s.find("reinforced ")? + "reinforced ".len();
    let rest = &s[start..];
    let end = rest.find(' ')?;
    rest[..end].parse().ok()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to create a temp dir and manager for testing.
    fn test_manager() -> (PreferenceMemoryManager, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let manager = PreferenceMemoryManager::new(dir.path().to_path_buf());
        (manager, dir)
    }

    // -----------------------------------------------------------------------
    // Detection tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_detect_correction_no_dont() {
        let mgr = PreferenceMemoryManager::new(std::env::temp_dir());
        let result = mgr.detect_from_message(
            "No, don't use var, use const instead",
            "Here is the code using var...",
        );
        assert!(result.is_some());
        let entry = result.unwrap();
        assert!(entry.rule.contains("const") || entry.rule.contains("Don't use var"));
        assert_eq!(entry.category, PreferenceCategory::Workflow);
        assert_eq!(entry.priority, PreferencePriority::High);
    }

    #[test]
    fn test_detect_correction_stop_doing() {
        let mgr = PreferenceMemoryManager::new(std::env::temp_dir());
        let result = mgr.detect_from_message(
            "Stop doing that, use tabs instead",
            "Here is the indented code...",
        );
        assert!(result.is_some());
        let entry = result.unwrap();
        assert_eq!(entry.priority, PreferencePriority::High);
    }

    #[test]
    fn test_detect_explicit_preference_always() {
        let mgr = PreferenceMemoryManager::new(std::env::temp_dir());
        let result = mgr.detect_from_message(
            "Always use TypeScript strict mode",
            "",
        );
        assert!(result.is_some());
        let entry = result.unwrap();
        assert!(entry.rule.contains("TypeScript strict mode"));
        assert_eq!(entry.priority, PreferencePriority::High);
    }

    #[test]
    fn test_detect_explicit_preference_never() {
        let mgr = PreferenceMemoryManager::new(std::env::temp_dir().to_path_buf());
        let result = mgr.detect_from_message(
            "Never use any in TypeScript",
            "",
        );
        assert!(result.is_some());
        let entry = result.unwrap();
        assert!(entry.rule.contains("any"));
        assert_eq!(entry.priority, PreferencePriority::High);
    }

    #[test]
    fn test_detect_explicit_preference_prefer() {
        let mgr = PreferenceMemoryManager::new(std::env::temp_dir());
        let result = mgr.detect_from_message(
            "I prefer 2 space indentation",
            "",
        );
        assert!(result.is_some());
        let entry = result.unwrap();
        assert_eq!(entry.priority, PreferencePriority::Normal);
    }

    #[test]
    fn test_detect_style_directive() {
        let mgr = PreferenceMemoryManager::new(std::env::temp_dir());
        let result = mgr.detect_from_message(
            "Use snake_case for all variables",
            "",
        );
        assert!(result.is_some());
        let entry = result.unwrap();
        assert_eq!(entry.category, PreferenceCategory::Style);
        assert!(entry.rule.contains("snake_case"));
    }

    #[test]
    fn test_detect_style_no_comments() {
        let mgr = PreferenceMemoryManager::new(std::env::temp_dir());
        let result = mgr.detect_from_message(
            "No comments needed for this code",
            "",
        );
        assert!(result.is_some());
        let entry = result.unwrap();
        assert_eq!(entry.category, PreferenceCategory::Style);
    }

    #[test]
    fn test_detect_confirmation_with_code() {
        let mgr = PreferenceMemoryManager::new(std::env::temp_dir());
        let result = mgr.detect_from_message(
            "Yes exactly, that's what I wanted",
            "Here is the refactored code:\n```rust\nfn main() {}\n```",
        );
        assert!(result.is_some());
        let entry = result.unwrap();
        assert_eq!(entry.priority, PreferencePriority::Low);
    }

    #[test]
    fn test_detect_confirmation_without_code_ignored() {
        let mgr = PreferenceMemoryManager::new(std::env::temp_dir());
        let result = mgr.detect_from_message(
            "Perfect",
            "OK",
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_detect_no_preference() {
        let mgr = PreferenceMemoryManager::new(std::env::temp_dir());
        let result = mgr.detect_from_message(
            "What is the weather today?",
            "The weather is sunny.",
        );
        assert!(result.is_none());
    }

    // -----------------------------------------------------------------------
    // Loading / saving roundtrip
    // -----------------------------------------------------------------------

    #[test]
    fn test_save_and_load_roundtrip() {
        let (mgr, _dir) = test_manager();

        let entry = PreferenceEntry {
            rule: "Use TypeScript strict mode".to_string(),
            context: "User corrected non-strict code".to_string(),
            category: PreferenceCategory::Style,
            created_at: Utc::now(),
            reinforcement_count: 3,
            priority: PreferencePriority::High,
        };

        mgr.add_preference(entry).unwrap();

        // Create a fresh manager from the same dir to verify persistence.
        let mgr2 = PreferenceMemoryManager::new(_dir.path().to_path_buf());
        let loaded = mgr2.load_preferences().unwrap();

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].rule, "Use TypeScript strict mode");
        assert_eq!(loaded[0].context, "User corrected non-strict code");
        assert_eq!(loaded[0].category, PreferenceCategory::Style);
        assert_eq!(loaded[0].reinforcement_count, 3);
        assert_eq!(loaded[0].priority, PreferencePriority::High);
    }

    #[test]
    fn test_load_empty_preferences() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = PreferenceMemoryManager::new(dir.path().to_path_buf());
        let loaded = mgr.load_preferences().unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn test_load_nonexistent_dir() {
        let dir = std::env::temp_dir().join("shannon_pref_test_nonexistent");
        let mgr = PreferenceMemoryManager::new(dir.join("sub"));
        let loaded = mgr.load_preferences().unwrap();
        assert!(loaded.is_empty());
    }

    // -----------------------------------------------------------------------
    // Merge on similar preference
    // -----------------------------------------------------------------------

    #[test]
    fn test_merge_similar_preferences() {
        let (mgr, _dir) = test_manager();

        let entry1 = PreferenceEntry {
            rule: "Use TypeScript strict mode".to_string(),
            context: "First time".to_string(),
            category: PreferenceCategory::Style,
            created_at: Utc::now(),
            reinforcement_count: 0,
            priority: PreferencePriority::Normal,
        };

        let entry2 = PreferenceEntry {
            rule: "Use TypeScript strict mode always".to_string(),
            context: "Reinforced after correction".to_string(),
            category: PreferenceCategory::Style,
            created_at: Utc::now(),
            reinforcement_count: 0,
            priority: PreferencePriority::High,
        };

        mgr.add_preference(entry1).unwrap();
        mgr.add_preference(entry2).unwrap();

        let prefs = mgr.preferences.read().unwrap();
        // Should merge into a single entry.
        assert_eq!(prefs.len(), 1);
        assert_eq!(prefs[0].reinforcement_count, 1);
        assert_eq!(prefs[0].priority, PreferencePriority::High);
        // Context should be updated to the longer one.
        assert_eq!(prefs[0].context, "Reinforced after correction");
    }

    #[test]
    fn test_no_merge_dissimilar_preferences() {
        let (mgr, _dir) = test_manager();

        let entry1 = PreferenceEntry {
            rule: "Use tabs for indentation".to_string(),
            context: "User preference".to_string(),
            category: PreferenceCategory::Style,
            created_at: Utc::now(),
            reinforcement_count: 0,
            priority: PreferencePriority::Normal,
        };

        let entry2 = PreferenceEntry {
            rule: "Always run tests after changes".to_string(),
            context: "Workflow requirement".to_string(),
            category: PreferenceCategory::Workflow,
            created_at: Utc::now(),
            reinforcement_count: 0,
            priority: PreferencePriority::Normal,
        };

        mgr.add_preference(entry1).unwrap();
        mgr.add_preference(entry2).unwrap();

        let prefs = mgr.preferences.read().unwrap();
        assert_eq!(prefs.len(), 2);
    }

    // -----------------------------------------------------------------------
    // Reinforcement counting
    // -----------------------------------------------------------------------

    #[test]
    fn test_reinforce_existing_preference() {
        let (mgr, _dir) = test_manager();

        let entry = PreferenceEntry {
            rule: "Use TypeScript strict mode".to_string(),
            context: "Initial".to_string(),
            category: PreferenceCategory::Style,
            created_at: Utc::now(),
            reinforcement_count: 0,
            priority: PreferencePriority::Normal,
        };

        mgr.add_preference(entry).unwrap();
        mgr.reinforce_preference("typescript strict").unwrap();

        let prefs = mgr.preferences.read().unwrap();
        assert_eq!(prefs[0].reinforcement_count, 1);

        // Reinforce again.
        drop(prefs);
        mgr.reinforce_preference("strict mode").unwrap();

        let prefs = mgr.preferences.read().unwrap();
        assert_eq!(prefs[0].reinforcement_count, 2);
    }

    #[test]
    fn test_reinforce_no_match_is_noop() {
        let (mgr, _dir) = test_manager();

        let entry = PreferenceEntry {
            rule: "Use TypeScript strict mode".to_string(),
            context: "Initial".to_string(),
            category: PreferenceCategory::Style,
            created_at: Utc::now(),
            reinforcement_count: 0,
            priority: PreferencePriority::Normal,
        };

        mgr.add_preference(entry).unwrap();

        // Write the file timestamp before reinforcement attempt.
        let path = _dir.path().join(PREFERENCES_FILE);
        let metadata_before = fs::metadata(&path).unwrap().modified().unwrap();

        // Small sleep to ensure timestamp would change if file were written.
        std::thread::sleep(std::time::Duration::from_millis(10));

        mgr.reinforce_preference("something completely unrelated").unwrap();

        let metadata_after = fs::metadata(&path).unwrap().modified().unwrap();

        // File should NOT have been rewritten (no matching preference).
        assert_eq!(metadata_before, metadata_after);
    }

    // -----------------------------------------------------------------------
    // Format for prompt injection
    // -----------------------------------------------------------------------

    #[test]
    fn test_format_for_prompt_with_entries() {
        let (mgr, _dir) = test_manager();

        let entries = vec![
            PreferenceEntry {
                rule: "Use TypeScript strict mode".to_string(),
                context: "ctx".to_string(),
                category: PreferenceCategory::Style,
                created_at: Utc::now(),
                reinforcement_count: 0,
                priority: PreferencePriority::High,
            },
            PreferenceEntry {
                rule: "Always run tests after changes".to_string(),
                context: "ctx".to_string(),
                category: PreferenceCategory::Workflow,
                created_at: Utc::now(),
                reinforcement_count: 0,
                priority: PreferencePriority::Normal,
            },
            PreferenceEntry {
                rule: "Keep responses concise".to_string(),
                context: "ctx".to_string(),
                category: PreferenceCategory::Communication,
                created_at: Utc::now(),
                reinforcement_count: 0,
                priority: PreferencePriority::Normal,
            },
        ];

        for entry in entries {
            mgr.add_preference(entry).unwrap();
        }

        let prompt = mgr.get_preferences_for_prompt();
        assert!(prompt.contains("## User Preferences"));
        assert!(prompt.contains("### Style"));
        assert!(prompt.contains("### Workflow"));
        assert!(prompt.contains("### Communication"));
        assert!(prompt.contains("- Use TypeScript strict mode"));
        assert!(prompt.contains("- Always run tests after changes"));
        assert!(prompt.contains("- Keep responses concise"));
    }

    #[test]
    fn test_format_for_prompt_empty() {
        let (mgr, _dir) = test_manager();
        let prompt = mgr.get_preferences_for_prompt();
        assert!(prompt.is_empty());
    }

    #[test]
    fn test_format_priority_ordering() {
        let (mgr, _dir) = test_manager();

        let entries = vec![
            PreferenceEntry {
                rule: "Low priority rule".to_string(),
                context: "ctx".to_string(),
                category: PreferenceCategory::General,
                created_at: Utc::now(),
                reinforcement_count: 0,
                priority: PreferencePriority::Low,
            },
            PreferenceEntry {
                rule: "High priority rule".to_string(),
                context: "ctx".to_string(),
                category: PreferenceCategory::General,
                created_at: Utc::now(),
                reinforcement_count: 0,
                priority: PreferencePriority::High,
            },
            PreferenceEntry {
                rule: "Normal priority rule".to_string(),
                context: "ctx".to_string(),
                category: PreferenceCategory::General,
                created_at: Utc::now(),
                reinforcement_count: 0,
                priority: PreferencePriority::Normal,
            },
        ];

        for entry in entries {
            mgr.add_preference(entry).unwrap();
        }

        let prompt = mgr.get_preferences_for_prompt();
        // High should come before Normal, Normal before Low.
        let high_pos = prompt.find("High priority rule").unwrap();
        let normal_pos = prompt.find("Normal priority rule").unwrap();
        let low_pos = prompt.find("Low priority rule").unwrap();
        assert!(high_pos < normal_pos);
        assert!(normal_pos < low_pos);
    }

    // -----------------------------------------------------------------------
    // Markdown parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_markdown_with_reinforcement() {
        let markdown = r#"# Shannon Code - User Preferences

## Style
- [HIGH] Use TypeScript strict mode
  Context: User corrected non-strict code (reinforced 3 times)

## Workflow
- [NORMAL] Always run tests after changes
  Context: User requested after first commit
"#;
        let entries = parse_preferences_markdown(markdown).unwrap();
        assert_eq!(entries.len(), 2);

        assert_eq!(entries[0].rule, "Use TypeScript strict mode");
        assert_eq!(entries[0].category, PreferenceCategory::Style);
        assert_eq!(entries[0].priority, PreferencePriority::High);
        assert_eq!(entries[0].reinforcement_count, 3);
        assert_eq!(entries[0].context, "User corrected non-strict code");

        assert_eq!(entries[1].rule, "Always run tests after changes");
        assert_eq!(entries[1].category, PreferenceCategory::Workflow);
        assert_eq!(entries[1].priority, PreferencePriority::Normal);
        assert_eq!(entries[1].reinforcement_count, 0);
    }

    #[test]
    fn test_parse_empty_markdown() {
        let markdown = "# Shannon Code - User Preferences\n";
        let entries = parse_preferences_markdown(markdown).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_parse_markdown_without_context() {
        let markdown = "# Shannon Code - User Preferences\n\n## Style\n- [NORMAL] Use tabs\n";
        let entries = parse_preferences_markdown(markdown).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].rule, "Use tabs");
        assert_eq!(entries[0].context, "");
    }

    // -----------------------------------------------------------------------
    // Category classification
    // -----------------------------------------------------------------------

    #[test]
    fn test_classify_workflow() {
        let cat = classify_preference_category("always run tests before commit");
        assert_eq!(cat, PreferenceCategory::Workflow);
    }

    #[test]
    fn test_classify_communication() {
        let cat = classify_preference_category("keep responses concise");
        assert_eq!(cat, PreferenceCategory::Communication);
    }

    #[test]
    fn test_classify_project() {
        let cat = classify_preference_category("in our project we use redis");
        assert_eq!(cat, PreferenceCategory::Project);
    }

    #[test]
    fn test_classify_general() {
        let cat = classify_preference_category("i prefer dark mode");
        assert_eq!(cat, PreferenceCategory::General);
    }

    // -----------------------------------------------------------------------
    // Rule similarity
    // -----------------------------------------------------------------------

    #[test]
    fn test_rule_similarity_identical() {
        let sim = rule_similarity("Use TypeScript strict mode", "Use TypeScript strict mode");
        assert!((sim - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_rule_similarity_similar() {
        let sim = rule_similarity(
            "Use TypeScript strict mode",
            "Use TypeScript strict mode always",
        );
        assert!(sim > 0.5);
    }

    #[test]
    fn test_rule_similarity_dissimilar() {
        let sim = rule_similarity(
            "Use TypeScript strict mode",
            "Always run tests after changes",
        );
        assert!(sim < 0.3);
    }
}
