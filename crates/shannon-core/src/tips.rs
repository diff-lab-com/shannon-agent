//! # Tips
//!
//! Context-aware usage tips for Shannon Code.
//!
//! The [`TipManager`] maintains a pool of [`Tip`] entries, each carrying a
//! [`TipCondition`] that determines when the tip is eligible.  Tips that have
//! already been shown are tracked so they are not repeated within the same
//! session (and optionally persisted across sessions).
//!
//! ## Example
//!
//! ```
//! use shannon_core::tips::{TipManager, TipContext};
//!
//! let mut mgr = TipManager::new();
//! let tip = mgr.get_tip(&TipContext::AfterCommand {
//!     command: "read src/main.rs".into(),
//! });
//! assert!(tip.is_some());
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

// ============================================================================
// Core types
// ============================================================================

/// Classification of a tip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TipCategory {
    /// General productivity advice.
    Productivity,
    /// Keyboard shortcut hints.
    Keyboard,
    /// Built-in command reference.
    Commands,
    /// Tool-related suggestions.
    Tools,
    /// Workflow and process guidance.
    Workflow,
}

impl std::fmt::Display for TipCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Productivity => write!(f, "productivity"),
            Self::Keyboard => write!(f, "keyboard"),
            Self::Commands => write!(f, "commands"),
            Self::Tools => write!(f, "tools"),
            Self::Workflow => write!(f, "workflow"),
        }
    }
}

/// Condition that must be met for a tip to be eligible.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TipCondition {
    /// Always eligible.
    Always,
    /// Eligible after a specific command is used.
    AfterCommand { command: String },
    /// Eligible after an error matching `error_pattern` occurs.
    AfterError { error_pattern: String },
    /// Eligible once the session count reaches `min_sessions`.
    SessionCount { min_sessions: usize },
    /// Eligible once the file count reaches `min_files`.
    FileCount { min_files: usize },
}

/// A single usage tip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tip {
    /// Unique identifier.
    pub id: String,
    /// The tip message displayed to the user.
    pub message: String,
    /// Category for this tip.
    pub category: TipCategory,
    /// Condition that must be satisfied for this tip to be shown.
    pub condition: TipCondition,
    /// Priority in [0.0, 1.0]. Higher = shown first.
    pub priority: f64,
}

/// The context in which a tip is being requested.
#[derive(Debug, Clone)]
pub enum TipContext {
    /// No specific context -- generic tip request.
    Generic,
    /// A command was just executed.
    AfterCommand { command: String },
    /// An error just occurred.
    AfterError { error_message: String },
    /// Current session metadata.
    SessionInfo {
        session_count: u32,
        file_count: usize,
    },
}

// ============================================================================
// TipManager
// ============================================================================

/// Manages a pool of tips and tracks which have been shown.
pub struct TipManager {
    tips: Vec<Tip>,
    shown_tips: HashSet<String>,
    session_count: u32,
    storage_path: Option<PathBuf>,
}

impl TipManager {
    /// Create a new `TipManager` with built-in tips registered.
    pub fn new() -> Self {
        let mut mgr = Self {
            tips: Vec::new(),
            shown_tips: HashSet::new(),
            session_count: 1,
            storage_path: None,
        };
        mgr.register_builtin_tips();
        mgr
    }

    /// Create a `TipManager` with no built-in tips.
    pub fn empty() -> Self {
        Self {
            tips: Vec::new(),
            shown_tips: HashSet::new(),
            session_count: 1,
            storage_path: None,
        }
    }

    /// Set the file path used to persist the set of shown tip IDs.
    pub fn set_storage_path(&mut self, path: impl Into<PathBuf>) {
        self.storage_path = Some(path.into());
    }

    /// Add a custom tip to the pool.
    pub fn add_tip(&mut self, tip: Tip) {
        self.tips.push(tip);
    }

    /// Record that a tip has been shown (prevents future repeats).
    pub fn mark_shown(&mut self, id: &str) {
        self.shown_tips.insert(id.to_string());
    }

    /// Return the set of tip IDs that have already been shown.
    pub fn shown_ids(&self) -> &HashSet<String> {
        &self.shown_tips
    }

    /// Return the total number of registered tips.
    pub fn tip_count(&self) -> usize {
        self.tips.len()
    }

    /// Get the best eligible tip for the given context, or `None` if no
    /// unseen tip matches.
    pub fn get_tip(&mut self, context: &TipContext) -> Option<Tip> {
        let mut candidates: Vec<&Tip> = self
            .tips
            .iter()
            .filter(|t| {
                !self.shown_tips.contains(&t.id) && self.condition_matches(t, context)
            })
            .collect();

        if candidates.is_empty() {
            return None;
        }

        // Sort by priority descending (highest first).
        candidates.sort_by(|a, b| b.priority.partial_cmp(&a.priority).unwrap_or(std::cmp::Ordering::Equal));

        let tip = candidates.into_iter().next().expect("candidates checked non-empty above").clone();
        self.mark_shown(&tip.id);
        Some(tip)
    }

    /// Check whether a tip's condition is satisfied by the context.
    fn condition_matches(&self, tip: &Tip, context: &TipContext) -> bool {
        match &tip.condition {
            TipCondition::Always => true,
            TipCondition::AfterCommand { command } => match context {
                TipContext::AfterCommand { command: ctx_cmd } => {
                    ctx_cmd.contains(command.as_str())
                        || command.contains(ctx_cmd.as_str())
                }
                _ => false,
            },
            TipCondition::AfterError { error_pattern } => match context {
                TipContext::AfterError { error_message } => {
                    error_message.contains(error_pattern.as_str())
                        || error_pattern.contains(error_message.as_str())
                }
                _ => false,
            },
            TipCondition::SessionCount { min_sessions } => {
                let current = match context {
                    TipContext::SessionInfo { session_count, .. } => *session_count,
                    _ => self.session_count,
                };
                (current as usize) >= *min_sessions
            }
            TipCondition::FileCount { min_files } => {
                let current = match context {
                    TipContext::SessionInfo { file_count, .. } => *file_count,
                    _ => 0,
                };
                current >= *min_files
            }
        }
    }

    /// Set the current session count (used for `SessionCount` conditions).
    pub fn set_session_count(&mut self, count: u32) {
        self.session_count = count;
    }

    /// Load previously shown tip IDs from the storage file.
    ///
    /// If the file does not exist or cannot be parsed this is a no-op.
    pub fn load_shown(&mut self) -> Result<(), TipError> {
        if let Some(ref path) = self.storage_path {
            if path.exists() {
                let data = fs::read_to_string(path)?;
                let ids: HashSet<String> = serde_json::from_str(&data)?;
                self.shown_tips = ids;
            }
        }
        Ok(())
    }

    /// Persist the set of shown tip IDs to the storage file.
    pub fn save_shown(&self) -> Result<(), TipError> {
        if let Some(ref path) = self.storage_path {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let data = serde_json::to_string_pretty(&self.shown_tips)?;
            fs::write(path, data)?;
        }
        Ok(())
    }

    // -- Built-in tips ------------------------------------------------------

    fn register_builtin_tips(&mut self) {
        let builtin = [
            Tip {
                id: "welcome".into(),
                message: "Welcome to Shannon Code! Type /help to see available commands.".into(),
                category: TipCategory::Productivity,
                condition: TipCondition::SessionCount { min_sessions: 1 },
                priority: 1.0,
            },
            Tip {
                id: "read_file".into(),
                message: "Use 'read <path>' to view any file in your project.".into(),
                category: TipCategory::Commands,
                condition: TipCondition::Always,
                priority: 0.8,
            },
            Tip {
                id: "edit_file".into(),
                message: "Use 'edit <path>' to make targeted edits to existing files.".into(),
                category: TipCategory::Commands,
                condition: TipCondition::Always,
                priority: 0.8,
            },
            Tip {
                id: "write_file".into(),
                message: "Use 'write <path>' to create new files or fully replace existing ones.".into(),
                category: TipCategory::Commands,
                condition: TipCondition::Always,
                priority: 0.8,
            },
            Tip {
                id: "search".into(),
                message: "Use 'search <pattern>' to find text across your project files.".into(),
                category: TipCategory::Tools,
                condition: TipCondition::Always,
                priority: 0.7,
            },
            Tip {
                id: "ls_tip".into(),
                message: "Use 'ls' to list the contents of any directory.".into(),
                category: TipCategory::Commands,
                condition: TipCondition::AfterCommand { command: "cd".into() },
                priority: 0.6,
            },
            Tip {
                id: "git_tip".into(),
                message: "You can run git commands directly -- try 'git status' or 'git diff'.".into(),
                category: TipCategory::Workflow,
                condition: TipCondition::AfterCommand { command: "git".into() },
                priority: 0.6,
            },
            Tip {
                id: "todo_tip".into(),
                message: "Use /todo to manage a task list. Good for multi-step work.".into(),
                category: TipCategory::Productivity,
                condition: TipCondition::SessionCount { min_sessions: 2 },
                priority: 0.5,
            },
            Tip {
                id: "compact_tip".into(),
                message: "When context gets long, use /compact to summarise and free up space.".into(),
                category: TipCategory::Productivity,
                condition: TipCondition::SessionCount { min_sessions: 3 },
                priority: 0.5,
            },
            Tip {
                id: "model_tip".into(),
                message: "Use /model to switch between Claude models on the fly.".into(),
                category: TipCategory::Commands,
                condition: TipCondition::SessionCount { min_sessions: 2 },
                priority: 0.4,
            },
            Tip {
                id: "error_tip".into(),
                message: "When you hit an error, paste it directly -- Claude can diagnose and fix it.".into(),
                category: TipCategory::Workflow,
                condition: TipCondition::AfterError {
                    error_pattern: "error".into(),
                },
                priority: 0.7,
            },
            Tip {
                id: "multi_file_tip".into(),
                message: "Claude can edit multiple files at once. Describe the full change and it will handle the rest.".into(),
                category: TipCategory::Productivity,
                condition: TipCondition::FileCount { min_files: 5 },
                priority: 0.6,
            },
            Tip {
                id: "permission_tip".into(),
                message: "Shannon asks for permission before running risky commands. Use /allowed-tools to pre-approve tools.".into(),
                category: TipCategory::Workflow,
                condition: TipCondition::SessionCount { min_sessions: 1 },
                priority: 0.3,
            },
        ];

        for tip in builtin {
            self.tips.push(tip);
        }
    }
}

impl Default for TipManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Error type
// ============================================================================

/// Errors that can occur during tip operations.
#[derive(Debug, thiserror::Error)]
pub enum TipError {
    /// An I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// JSON serialization / deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- TipCategory display -------------------------------------------------

    #[test]
    fn test_tip_category_display() {
        assert_eq!(TipCategory::Productivity.to_string(), "productivity");
        assert_eq!(TipCategory::Keyboard.to_string(), "keyboard");
        assert_eq!(TipCategory::Commands.to_string(), "commands");
        assert_eq!(TipCategory::Tools.to_string(), "tools");
        assert_eq!(TipCategory::Workflow.to_string(), "workflow");
    }

    // -- TipManager construction ---------------------------------------------

    #[test]
    fn test_new_has_builtin_tips() {
        let mgr = TipManager::new();
        assert!(mgr.tip_count() >= 12);
    }

    #[test]
    fn test_empty_has_no_tips() {
        let mgr = TipManager::empty();
        assert_eq!(mgr.tip_count(), 0);
    }

    #[test]
    fn test_default_is_same_as_new() {
        let mgr = TipManager::default();
        assert!(mgr.tip_count() >= 12);
    }

    // -- Adding tips --------------------------------------------------------

    #[test]
    fn test_add_tip() {
        let mut mgr = TipManager::empty();
        mgr.add_tip(Tip {
            id: "custom".into(),
            message: "custom tip".into(),
            category: TipCategory::Tools,
            condition: TipCondition::Always,
            priority: 0.5,
        });
        assert_eq!(mgr.tip_count(), 1);
    }

    // -- get_tip with Generic context ----------------------------------------

    #[test]
    fn test_get_tip_generic_returns_always_tip() {
        let mut mgr = TipManager::empty();
        mgr.add_tip(Tip {
            id: "always-tip".into(),
            message: "always".into(),
            category: TipCategory::Productivity,
            condition: TipCondition::Always,
            priority: 1.0,
        });

        let tip = mgr.get_tip(&TipContext::Generic);
        assert!(tip.is_some());
        assert_eq!(tip.unwrap().id, "always-tip");
    }

    #[test]
    fn test_get_tip_generic_skips_conditional() {
        let mut mgr = TipManager::empty();
        mgr.add_tip(Tip {
            id: "cmd-tip".into(),
            message: "cmd".into(),
            category: TipCategory::Commands,
            condition: TipCondition::AfterCommand {
                command: "git".into(),
            },
            priority: 1.0,
        });

        let tip = mgr.get_tip(&TipContext::Generic);
        assert!(tip.is_none());
    }

    // -- get_tip with AfterCommand context -----------------------------------

    #[test]
    fn test_get_tip_after_command_matches() {
        let mut mgr = TipManager::empty();
        mgr.add_tip(Tip {
            id: "git-tip".into(),
            message: "git tip".into(),
            category: TipCategory::Workflow,
            condition: TipCondition::AfterCommand {
                command: "git".into(),
            },
            priority: 1.0,
        });

        let tip = mgr.get_tip(&TipContext::AfterCommand {
            command: "git status".into(),
        });
        assert!(tip.is_some());
        assert_eq!(tip.unwrap().id, "git-tip");
    }

    #[test]
    fn test_get_tip_after_command_no_match() {
        let mut mgr = TipManager::empty();
        mgr.add_tip(Tip {
            id: "npm-tip".into(),
            message: "npm".into(),
            category: TipCategory::Tools,
            condition: TipCondition::AfterCommand {
                command: "npm".into(),
            },
            priority: 1.0,
        });

        let tip = mgr.get_tip(&TipContext::AfterCommand {
            command: "git status".into(),
        });
        assert!(tip.is_none());
    }

    // -- get_tip with AfterError context -------------------------------------

    #[test]
    fn test_get_tip_after_error_matches() {
        let mut mgr = TipManager::empty();
        mgr.add_tip(Tip {
            id: "err-tip".into(),
            message: "error help".into(),
            category: TipCategory::Workflow,
            condition: TipCondition::AfterError {
                error_pattern: "compile".into(),
            },
            priority: 1.0,
        });

        let tip = mgr.get_tip(&TipContext::AfterError {
            error_message: "compile error: missing semicolon".into(),
        });
        assert!(tip.is_some());
    }

    // -- get_tip with SessionInfo context ------------------------------------

    #[test]
    fn test_get_tip_session_count_eligible() {
        let mut mgr = TipManager::empty();
        mgr.set_session_count(5);
        mgr.add_tip(Tip {
            id: "session-tip".into(),
            message: "veteran tip".into(),
            category: TipCategory::Productivity,
            condition: TipCondition::SessionCount {
                min_sessions: 3,
            },
            priority: 1.0,
        });

        let tip = mgr.get_tip(&TipContext::SessionInfo {
            session_count: 5,
            file_count: 0,
        });
        assert!(tip.is_some());
    }

    #[test]
    fn test_get_tip_session_count_not_eligible() {
        let mut mgr = TipManager::empty();
        mgr.add_tip(Tip {
            id: "session-tip".into(),
            message: "veteran".into(),
            category: TipCategory::Productivity,
            condition: TipCondition::SessionCount {
                min_sessions: 10,
            },
            priority: 1.0,
        });

        let tip = mgr.get_tip(&TipContext::SessionInfo {
            session_count: 2,
            file_count: 0,
        });
        assert!(tip.is_none());
    }

    // -- get_tip with FileCount context --------------------------------------

    #[test]
    fn test_get_tip_file_count_eligible() {
        let mut mgr = TipManager::empty();
        mgr.add_tip(Tip {
            id: "file-tip".into(),
            message: "big project".into(),
            category: TipCategory::Productivity,
            condition: TipCondition::FileCount { min_files: 10 },
            priority: 1.0,
        });

        let tip = mgr.get_tip(&TipContext::SessionInfo {
            session_count: 1,
            file_count: 15,
        });
        assert!(tip.is_some());
    }

    #[test]
    fn test_get_tip_file_count_not_eligible() {
        let mut mgr = TipManager::empty();
        mgr.add_tip(Tip {
            id: "file-tip".into(),
            message: "big".into(),
            category: TipCategory::Productivity,
            condition: TipCondition::FileCount { min_files: 100 },
            priority: 1.0,
        });

        let tip = mgr.get_tip(&TipContext::SessionInfo {
            session_count: 1,
            file_count: 15,
        });
        assert!(tip.is_none());
    }

    // -- mark_shown / dedup -------------------------------------------------

    #[test]
    fn test_mark_shown_prevents_repeat() {
        let mut mgr = TipManager::empty();
        mgr.add_tip(Tip {
            id: "once".into(),
            message: "only once".into(),
            category: TipCategory::Productivity,
            condition: TipCondition::Always,
            priority: 1.0,
        });

        let first = mgr.get_tip(&TipContext::Generic);
        assert!(first.is_some());
        let second = mgr.get_tip(&TipContext::Generic);
        assert!(second.is_none());
    }

    #[test]
    fn test_shown_ids_reflects_marks() {
        let mut mgr = TipManager::empty();
        assert!(mgr.shown_ids().is_empty());
        mgr.mark_shown("a");
        mgr.mark_shown("b");
        assert_eq!(mgr.shown_ids().len(), 2);
        assert!(mgr.shown_ids().contains("a"));
    }

    // -- priority ordering --------------------------------------------------

    #[test]
    fn test_higher_priority_returned_first() {
        let mut mgr = TipManager::empty();
        mgr.add_tip(Tip {
            id: "low".into(),
            message: "low".into(),
            category: TipCategory::Productivity,
            condition: TipCondition::Always,
            priority: 0.2,
        });
        mgr.add_tip(Tip {
            id: "high".into(),
            message: "high".into(),
            category: TipCategory::Productivity,
            condition: TipCondition::Always,
            priority: 0.9,
        });

        let tip = mgr.get_tip(&TipContext::Generic);
        assert_eq!(tip.unwrap().id, "high");
    }

    // -- persistence --------------------------------------------------------

    #[test]
    fn test_save_and_load_shown() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shown.json");

        let mut mgr = TipManager::empty();
        mgr.set_storage_path(&path);
        mgr.add_tip(Tip {
            id: "persist".into(),
            message: "persists".into(),
            category: TipCategory::Productivity,
            condition: TipCondition::Always,
            priority: 1.0,
        });

        // Show a tip, save, create new manager, load.
        mgr.get_tip(&TipContext::Generic).unwrap();
        mgr.save_shown().unwrap();
        assert_eq!(mgr.shown_ids().len(), 1);

        let mut mgr2 = TipManager::empty();
        mgr2.set_storage_path(&path);
        mgr2.load_shown().unwrap();
        assert_eq!(mgr2.shown_ids().len(), 1);
        assert!(mgr2.shown_ids().contains("persist"));
    }

    #[test]
    fn test_load_shown_missing_file_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");

        let mut mgr = TipManager::empty();
        mgr.set_storage_path(&path);
        assert!(mgr.load_shown().is_ok());
        assert!(mgr.shown_ids().is_empty());
    }

    // -- Tip serialization --------------------------------------------------

    #[test]
    fn test_tip_serialization_roundtrip() {
        let tip = Tip {
            id: "ser".into(),
            message: "roundtrip".into(),
            category: TipCategory::Tools,
            condition: TipCondition::AfterCommand {
                command: "cargo".into(),
            },
            priority: 0.75,
        };
        let json = serde_json::to_string(&tip).unwrap();
        let back: Tip = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, tip.id);
        assert_eq!(back.category, tip.category);
        assert_eq!(back.priority, tip.priority);
    }

    // -- set_session_count --------------------------------------------------

    #[test]
    fn test_set_session_count_used_as_fallback() {
        let mut mgr = TipManager::empty();
        mgr.set_session_count(10);
        mgr.add_tip(Tip {
            id: "sc".into(),
            message: "session".into(),
            category: TipCategory::Productivity,
            condition: TipCondition::SessionCount {
                min_sessions: 5,
            },
            priority: 1.0,
        });

        // Generic context falls back to mgr.session_count.
        let tip = mgr.get_tip(&TipContext::Generic);
        assert!(tip.is_some());
    }
}
