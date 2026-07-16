//! Hook-triggered routines loaded from `.shannon/routines.toml` and `.claude/routines.toml`.
//!
//! Routines are shell commands that execute automatically when a matching hook event fires.
//! They complement the interval-based `ScheduledRoutine` system with event-driven automation.
//!
//! ## File Format
//!
//! ```toml
//! [[routine]]
//! name = "post-save-lint"
//! trigger = "PostToolUse"
//! matcher = "Edit|Write"
//! command = "cargo clippy --fix --allow-dirty"
//!
//! [[routine]]
//! name = "file-change-test"
//! trigger = "FileChanged"
//! pattern = "*.rs"
//! command = "cargo check"
//!
//! [[routine]]
//! name = "session-start-pull"
//! trigger = "SessionStart"
//! command = "git pull --rebase"
//! ```
//!
//! ## Discovery
//!
//! Files are loaded from (later overrides earlier by name):
//! 1. `~/.shannon/routines.toml` (user-global)
//! 2. `~/.claude/routines.toml` (user-global, compatible)
//! 3. `.shannon/routines.toml` (project-local)
//! 4. `.claude/routines.toml` (project-local)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::HookEventType;

/// A single routine definition loaded from TOML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggeredRoutineDef {
    /// Human-readable name (must be unique within a config file).
    pub name: String,
    /// Hook event type that triggers this routine.
    pub trigger: String,
    /// Optional matcher against the event subject (tool name, file path, etc.).
    /// Supports pipe-separated: `"Edit|Write"`, wildcard: `"*"`, exact match.
    #[serde(default)]
    pub matcher: Option<String>,
    /// Optional glob pattern for file paths (used with FileChanged events).
    #[serde(default)]
    pub pattern: Option<String>,
    /// Shell command to execute when triggered.
    pub command: String,
    /// Whether this routine is enabled (default: true).
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Timeout in seconds (default: 60).
    #[serde(default = "default_timeout")]
    pub timeout: u64,
    /// Whether to run in the background (non-blocking, default: true).
    #[serde(default = "default_background")]
    pub background: bool,
    /// Optional description of what this routine does.
    #[serde(default)]
    pub description: Option<String>,
}

fn default_enabled() -> bool {
    true
}

fn default_timeout() -> u64 {
    60
}

fn default_background() -> bool {
    true
}

/// Registry of triggered routines loaded from TOML files.
#[derive(Debug, Clone, Default)]
pub struct TriggeredRoutineRegistry {
    routines: HashMap<String, TriggeredRoutineDef>,
}

/// TOML file structure for routines config.
#[derive(Debug, Deserialize, Default)]
struct RoutinesFile {
    #[serde(default)]
    routine: Vec<TriggeredRoutineDef>,
}

impl TriggeredRoutineRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load triggered routines from standard directories.
    ///
    /// Loading order (later overrides earlier by name):
    /// 1. `~/.shannon/routines.toml` (user-global)
    /// 2. `~/.claude/routines.toml` (user-global)
    /// 3. `.shannon/routines.toml` (project-local)
    /// 4. `.claude/routines.toml` (project-local)
    pub fn load_from_dirs() -> Self {
        let mut registry = Self::new();
        let mut search_paths = Vec::new();

        if let Some(home) = dirs::home_dir() {
            search_paths.push(home.join(".shannon").join("routines.toml"));
            search_paths.push(home.join(".claude").join("routines.toml"));
        }

        search_paths.push(PathBuf::from(".shannon").join("routines.toml"));
        search_paths.push(PathBuf::from(".claude").join("routines.toml"));

        for path in &search_paths {
            if path.exists() {
                registry.load_from_file(path);
            }
        }

        registry
    }

    /// Load routines from a single TOML file.
    pub fn load_from_file(&mut self, path: &Path) {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                tracing::debug!(path = %path.display(), error = %e, "Failed to read routines file");
                return;
            }
        };

        match toml::from_str::<RoutinesFile>(&content) {
            Ok(file) => {
                for def in file.routine {
                    tracing::debug!(name = %def.name, path = %path.display(), "Loaded triggered routine");
                    self.routines.insert(def.name.clone(), def);
                }
            }
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "Failed to parse routines file");
            }
        }
    }

    /// Find all routines that match the given hook event.
    pub fn matching_routines(
        &self,
        event_type: &HookEventType,
        subject: &str,
    ) -> Vec<&TriggeredRoutineDef> {
        let event_str = event_type.to_string();
        self.routines
            .values()
            .filter(|r| {
                if !r.enabled {
                    return false;
                }
                // Check trigger matches event type
                if r.trigger != event_str {
                    return false;
                }
                // Check matcher against subject
                if let Some(ref matcher) = r.matcher {
                    if !matches_subject(matcher, subject) {
                        return false;
                    }
                }
                true
            })
            .collect()
    }

    /// Find routines matching a hook event, also checking file pattern against a path.
    pub fn matching_routines_with_path(
        &self,
        event_type: &HookEventType,
        subject: &str,
        file_path: Option<&str>,
    ) -> Vec<&TriggeredRoutineDef> {
        self.matching_routines(event_type, subject)
            .into_iter()
            .filter(|r| {
                // If routine has a file pattern, check it against the file path
                if let (Some(pattern), Some(path)) = (&r.pattern, file_path) {
                    return matches_file_pattern(pattern, path);
                }
                true
            })
            .collect()
    }

    /// Get a routine by name.
    pub fn get(&self, name: &str) -> Option<&TriggeredRoutineDef> {
        self.routines.get(name)
    }

    /// List all routine names.
    pub fn list_names(&self) -> Vec<String> {
        self.routines.keys().cloned().collect()
    }

    /// Get all loaded routines.
    pub fn all(&self) -> &HashMap<String, TriggeredRoutineDef> {
        &self.routines
    }

    /// Check if any routines are loaded.
    pub fn is_empty(&self) -> bool {
        self.routines.is_empty()
    }

    /// Get a summary of loaded routines.
    pub fn summary(&self) -> String {
        if self.routines.is_empty() {
            return "No triggered routines loaded.".to_string();
        }

        let mut lines = Vec::new();
        lines.push(format!(
            "Loaded {} triggered routine(s):",
            self.routines.len()
        ));
        for (name, def) in &self.routines {
            let status = if def.enabled { "enabled" } else { "disabled" };
            let desc = def.description.as_deref().unwrap_or_else(|| &def.command);
            lines.push(format!(
                "  - {name} [{status}]: {trigger} → {desc}",
                trigger = def.trigger,
            ));
        }
        lines.join("\n")
    }

    /// Execute a routine's command.
    ///
    /// Returns the exit code and combined stdout/stderr output.
    pub async fn execute(&self, name: &str) -> Result<RoutineExecResult, TriggeredRoutineError> {
        let def = self
            .routines
            .get(name)
            .ok_or_else(|| TriggeredRoutineError::NotFound(name.to_string()))?;

        if !def.enabled {
            return Err(TriggeredRoutineError::Disabled(name.to_string()));
        }

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(def.timeout),
            tokio::process::Command::new("sh")
                .arg("-c")
                .arg(&def.command)
                .output(),
        )
        .await
        .map_err(|_| TriggeredRoutineError::Timeout {
            name: name.to_string(),
            timeout: def.timeout,
        })?
        .map_err(|e| TriggeredRoutineError::Execution {
            name: name.to_string(),
            error: e.to_string(),
        })?;

        Ok(RoutineExecResult {
            name: name.to_string(),
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }

    /// Execute all matching routines for a hook event (non-blocking).
    pub async fn execute_matching(
        &self,
        event_type: &HookEventType,
        subject: &str,
        file_path: Option<&str>,
    ) -> Vec<RoutineExecResult> {
        let matching = self.matching_routines_with_path(event_type, subject, file_path);
        let mut results = Vec::new();

        for def in matching {
            match self.execute(&def.name).await {
                Ok(result) => {
                    if !result.success() {
                        tracing::warn!(
                            name = %result.name,
                            exit_code = result.exit_code,
                            "Triggered routine failed"
                        );
                    }
                    results.push(result);
                }
                Err(e) => {
                    tracing::warn!(name = %def.name, error = %e, "Triggered routine error");
                }
            }
        }

        results
    }
}

/// Result of executing a triggered routine.
#[derive(Debug, Clone)]
pub struct RoutineExecResult {
    pub name: String,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl RoutineExecResult {
    /// Check if the routine succeeded (exit code 0).
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }
}

/// Check if a matcher pattern matches a subject string.
///
/// Supports:
/// - `"*"` wildcard: matches everything
/// - Pipe-separated: `"Edit|Write"` matches either
/// - Exact string match
fn matches_subject(matcher: &str, subject: &str) -> bool {
    if matcher == "*" {
        return true;
    }
    if matcher == subject {
        return true;
    }
    if matcher.contains('|') {
        return matcher.split('|').any(|part| {
            let part = part.trim();
            part == "*" || part == subject
        });
    }
    false
}

/// Check if a glob pattern matches a file path.
fn matches_file_pattern(pattern: &str, path: &str) -> bool {
    if let Ok(glob) = globset::Glob::new(pattern) {
        let matcher = glob.compile_matcher();
        return matcher.is_match(path);
    }
    // Fallback: simple extension match
    if pattern.starts_with("*.") {
        let ext = &pattern[1..]; // ".rs"
        return path.ends_with(ext);
    }
    path.contains(pattern)
}

/// Errors for triggered routines.
#[derive(Debug, thiserror::Error)]
pub enum TriggeredRoutineError {
    #[error("Routine not found: {0}")]
    NotFound(String),
    #[error("Routine disabled: {0}")]
    Disabled(String),
    #[error("Routine '{name}' timed out after {timeout}s")]
    Timeout { name: String, timeout: u64 },
    #[error("Routine '{name}' execution failed: {error}")]
    Execution { name: String, error: String },
    #[error("IO error reading {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("Parse error in {0}: {1}")]
    Parse(PathBuf, String),
    #[error("Validation error: {0}")]
    Validation(String),
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_routine() {
        let toml = r#"
[[routine]]
name = "post-save-lint"
trigger = "PostToolUse"
matcher = "Edit|Write"
command = "cargo clippy --fix"
description = "Lint after edits"
"#;
        let file: RoutinesFile = toml::from_str(toml).unwrap();
        assert_eq!(file.routine.len(), 1);
        let r = &file.routine[0];
        assert_eq!(r.name, "post-save-lint");
        assert_eq!(r.trigger, "PostToolUse");
        assert_eq!(r.matcher.as_deref(), Some("Edit|Write"));
        assert!(r.enabled);
        assert_eq!(r.timeout, 60);
    }

    #[test]
    fn parse_minimal_routine() {
        let toml = r#"
[[routine]]
name = "quick-check"
trigger = "FileChanged"
command = "cargo check"
"#;
        let file: RoutinesFile = toml::from_str(toml).unwrap();
        let r = &file.routine[0];
        assert_eq!(r.name, "quick-check");
        assert!(r.matcher.is_none());
        assert!(r.pattern.is_none());
        assert!(r.enabled);
        assert!(r.background);
    }

    #[test]
    fn parse_disabled_routine() {
        let toml = r#"
[[routine]]
name = "slow-test"
trigger = "UserPromptSubmit"
command = "cargo test"
enabled = false
timeout = 120
"#;
        let file: RoutinesFile = toml::from_str(toml).unwrap();
        let r = &file.routine[0];
        assert!(!r.enabled);
        assert_eq!(r.timeout, 120);
    }

    #[test]
    fn parse_file_pattern() {
        let toml = r#"
[[routine]]
name = "rust-check"
trigger = "FileChanged"
pattern = "*.rs"
command = "cargo check"
"#;
        let file: RoutinesFile = toml::from_str(toml).unwrap();
        assert_eq!(file.routine[0].pattern.as_deref(), Some("*.rs"));
    }

    #[test]
    fn parse_multiple_routines() {
        let toml = r#"
[[routine]]
name = "lint"
trigger = "PostToolUse"
matcher = "Edit"
command = "cargo clippy"

[[routine]]
name = "test"
trigger = "Stop"
command = "cargo test"
"#;
        let file: RoutinesFile = toml::from_str(toml).unwrap();
        assert_eq!(file.routine.len(), 2);
    }

    #[test]
    fn load_from_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("routines.toml");
        std::fs::write(
            &path,
            r#"
[[routine]]
name = "lint"
trigger = "PostToolUse"
command = "cargo clippy"
"#,
        )
        .unwrap();

        let mut registry = TriggeredRoutineRegistry::new();
        registry.load_from_file(&path);
        assert_eq!(registry.all().len(), 1);
        assert!(registry.get("lint").is_some());
    }

    #[test]
    fn local_overrides_global() {
        let global = tempfile::tempdir().unwrap();
        let local = tempfile::tempdir().unwrap();

        std::fs::write(
            global.path().join("routines.toml"),
            r#"
[[routine]]
name = "check"
trigger = "FileChanged"
command = "cargo check"
"#,
        )
        .unwrap();

        std::fs::write(
            local.path().join("routines.toml"),
            r#"
[[routine]]
name = "check"
trigger = "FileChanged"
command = "cargo check --all-targets"
"#,
        )
        .unwrap();

        let mut registry = TriggeredRoutineRegistry::new();
        registry.load_from_file(&global.path().join("routines.toml"));
        registry.load_from_file(&local.path().join("routines.toml"));

        let def = registry.get("check").unwrap();
        assert_eq!(def.command, "cargo check --all-targets");
    }

    #[test]
    fn matching_routines_by_event() {
        let mut registry = TriggeredRoutineRegistry::new();
        registry.routines.insert(
            "lint".to_string(),
            TriggeredRoutineDef {
                name: "lint".to_string(),
                trigger: "PostToolUse".to_string(),
                matcher: Some("Edit|Write".to_string()),
                pattern: None,
                command: "cargo clippy".to_string(),
                enabled: true,
                timeout: 60,
                background: true,
                description: None,
            },
        );
        registry.routines.insert(
            "test".to_string(),
            TriggeredRoutineDef {
                name: "test".to_string(),
                trigger: "Stop".to_string(),
                matcher: None,
                pattern: None,
                command: "cargo test".to_string(),
                enabled: true,
                timeout: 60,
                background: true,
                description: None,
            },
        );

        let matching = registry.matching_routines(&HookEventType::PostToolUse, "Edit");
        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0].name, "lint");

        let matching = registry.matching_routines(&HookEventType::Stop, "");
        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0].name, "test");

        let matching = registry.matching_routines(&HookEventType::PostToolUse, "Read");
        assert!(matching.is_empty());
    }

    #[test]
    fn matching_with_file_pattern() {
        let mut registry = TriggeredRoutineRegistry::new();
        registry.routines.insert(
            "rust-check".to_string(),
            TriggeredRoutineDef {
                name: "rust-check".to_string(),
                trigger: "FileChanged".to_string(),
                matcher: None,
                pattern: Some("*.rs".to_string()),
                command: "cargo check".to_string(),
                enabled: true,
                timeout: 60,
                background: true,
                description: None,
            },
        );

        let matching = registry.matching_routines_with_path(
            &HookEventType::FileChanged,
            "",
            Some("src/main.rs"),
        );
        assert_eq!(matching.len(), 1);

        let matching = registry.matching_routines_with_path(
            &HookEventType::FileChanged,
            "",
            Some("src/style.css"),
        );
        assert!(matching.is_empty());
    }

    #[test]
    fn disabled_routines_dont_match() {
        let mut registry = TriggeredRoutineRegistry::new();
        registry.routines.insert(
            "disabled".to_string(),
            TriggeredRoutineDef {
                name: "disabled".to_string(),
                trigger: "PostToolUse".to_string(),
                matcher: None,
                pattern: None,
                command: "echo".to_string(),
                enabled: false,
                timeout: 60,
                background: true,
                description: None,
            },
        );

        let matching = registry.matching_routines(&HookEventType::PostToolUse, "Edit");
        assert!(matching.is_empty());
    }

    #[test]
    fn matches_subject_patterns() {
        assert!(matches_subject("*", "anything"));
        assert!(matches_subject("Edit", "Edit"));
        assert!(!matches_subject("Edit", "Write"));
        assert!(matches_subject("Edit|Write", "Edit"));
        assert!(matches_subject("Edit|Write", "Write"));
        assert!(!matches_subject("Edit|Write", "Bash"));
    }

    #[test]
    fn matches_file_patterns() {
        assert!(matches_file_pattern("*.rs", "src/main.rs"));
        assert!(!matches_file_pattern("*.rs", "src/style.css"));
        assert!(matches_file_pattern("*.toml", "Cargo.toml"));
        assert!(matches_file_pattern("src/**", "src/main.rs"));
    }

    #[test]
    fn nonexistent_file_is_ok() {
        let mut registry = TriggeredRoutineRegistry::new();
        registry.load_from_file(Path::new("/no/such/file.toml"));
        assert!(registry.is_empty());
    }

    #[test]
    fn invalid_toml_is_handled() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.toml");
        std::fs::write(&path, "not valid toml [[[[").unwrap();

        let mut registry = TriggeredRoutineRegistry::new();
        registry.load_from_file(&path);
        assert!(registry.is_empty());
    }

    #[test]
    fn summary_format() {
        let mut registry = TriggeredRoutineRegistry::new();
        let empty = registry.summary();
        assert!(empty.contains("No triggered"));

        registry.routines.insert(
            "lint".to_string(),
            TriggeredRoutineDef {
                name: "lint".to_string(),
                trigger: "PostToolUse".to_string(),
                matcher: Some("Edit".to_string()),
                pattern: None,
                command: "cargo clippy".to_string(),
                enabled: true,
                timeout: 60,
                background: true,
                description: Some("Run clippy after edits".to_string()),
            },
        );

        let summary = registry.summary();
        assert!(summary.contains("1 triggered routine"));
        assert!(summary.contains("lint"));
        assert!(summary.contains("enabled"));
    }

    #[test]
    fn exec_result_success() {
        let result = RoutineExecResult {
            name: "test".to_string(),
            exit_code: 0,
            stdout: "ok".to_string(),
            stderr: String::new(),
        };
        assert!(result.success());

        let failed = RoutineExecResult {
            name: "test".to_string(),
            exit_code: 1,
            stdout: String::new(),
            stderr: "error".to_string(),
        };
        assert!(!failed.success());
    }

    // ── Error path tests ──────────────────────────────────────────────────

    #[tokio::test]
    async fn execute_not_found() {
        let registry = TriggeredRoutineRegistry::new();
        let result = registry.execute("nonexistent").await;
        assert!(matches!(result, Err(TriggeredRoutineError::NotFound(_))));
    }

    #[tokio::test]
    async fn execute_disabled() {
        let mut registry = TriggeredRoutineRegistry::new();
        registry.routines.insert(
            "disabled".to_string(),
            TriggeredRoutineDef {
                name: "disabled".to_string(),
                trigger: "Stop".to_string(),
                matcher: None,
                pattern: None,
                command: "echo hello".to_string(),
                enabled: false,
                timeout: 10,
                background: true,
                description: None,
            },
        );
        let result = registry.execute("disabled").await;
        assert!(matches!(result, Err(TriggeredRoutineError::Disabled(_))));
    }

    #[tokio::test]
    async fn execute_success_command() {
        let mut registry = TriggeredRoutineRegistry::new();
        registry.routines.insert(
            "echo-test".to_string(),
            TriggeredRoutineDef {
                name: "echo-test".to_string(),
                trigger: "Stop".to_string(),
                matcher: None,
                pattern: None,
                command: "echo hello-world".to_string(),
                enabled: true,
                timeout: 10,
                background: true,
                description: None,
            },
        );
        let result = registry.execute("echo-test").await.unwrap();
        assert!(result.success());
        assert!(result.stdout.contains("hello-world"));
    }

    #[tokio::test]
    async fn execute_failing_command() {
        let mut registry = TriggeredRoutineRegistry::new();
        registry.routines.insert(
            "fail-test".to_string(),
            TriggeredRoutineDef {
                name: "fail-test".to_string(),
                trigger: "Stop".to_string(),
                matcher: None,
                pattern: None,
                command: "exit 42".to_string(),
                enabled: true,
                timeout: 10,
                background: true,
                description: None,
            },
        );
        let result = registry.execute("fail-test").await.unwrap();
        assert!(!result.success());
        assert_eq!(result.exit_code, 42);
    }

    #[tokio::test]
    async fn execute_timeout() {
        let mut registry = TriggeredRoutineRegistry::new();
        registry.routines.insert(
            "slow".to_string(),
            TriggeredRoutineDef {
                name: "slow".to_string(),
                trigger: "Stop".to_string(),
                matcher: None,
                pattern: None,
                command: "sleep 60".to_string(),
                enabled: true,
                timeout: 1,
                background: true,
                description: None,
            },
        );
        let result = registry.execute("slow").await;
        assert!(matches!(result, Err(TriggeredRoutineError::Timeout { .. })));
    }

    #[tokio::test]
    async fn execute_matching_filters_by_event() {
        let mut registry = TriggeredRoutineRegistry::new();
        registry.routines.insert(
            "lint".to_string(),
            TriggeredRoutineDef {
                name: "lint".to_string(),
                trigger: "PostToolUse".to_string(),
                matcher: Some("Edit".to_string()),
                pattern: None,
                command: "echo lint-ran".to_string(),
                enabled: true,
                timeout: 10,
                background: true,
                description: None,
            },
        );
        // Should not match Stop event
        let results = registry
            .execute_matching(&HookEventType::Stop, "", None)
            .await;
        assert!(results.is_empty());

        // Should match PostToolUse with Edit
        let results = registry
            .execute_matching(&HookEventType::PostToolUse, "Edit", None)
            .await;
        assert_eq!(results.len(), 1);
        assert!(results[0].success());
    }

    // ── Edge cases ────────────────────────────────────────────────────────

    #[test]
    fn wildcard_matcher_matches_all() {
        let mut registry = TriggeredRoutineRegistry::new();
        registry.routines.insert(
            "all".to_string(),
            TriggeredRoutineDef {
                name: "all".to_string(),
                trigger: "PostToolUse".to_string(),
                matcher: Some("*".to_string()),
                pattern: None,
                command: "echo".to_string(),
                enabled: true,
                timeout: 10,
                background: true,
                description: None,
            },
        );
        // Matches any subject
        assert_eq!(
            registry
                .matching_routines(&HookEventType::PostToolUse, "Bash")
                .len(),
            1
        );
        assert_eq!(
            registry
                .matching_routines(&HookEventType::PostToolUse, "Edit")
                .len(),
            1
        );
        // Doesn't match different event
        assert!(
            registry
                .matching_routines(&HookEventType::Stop, "Bash")
                .is_empty()
        );
    }

    #[test]
    fn multiple_routines_same_trigger() {
        let mut registry = TriggeredRoutineRegistry::new();
        for name in ["lint", "format", "test"] {
            registry.routines.insert(
                name.to_string(),
                TriggeredRoutineDef {
                    name: name.to_string(),
                    trigger: "PostToolUse".to_string(),
                    matcher: Some("Edit".to_string()),
                    pattern: None,
                    command: format!("echo {name}"),
                    enabled: true,
                    timeout: 10,
                    background: true,
                    description: None,
                },
            );
        }
        let matching = registry.matching_routines(&HookEventType::PostToolUse, "Edit");
        assert_eq!(matching.len(), 3);
    }

    #[test]
    fn serialization_roundtrip() {
        let def = TriggeredRoutineDef {
            name: "test".to_string(),
            trigger: "FileChanged".to_string(),
            matcher: Some("*.rs".to_string()),
            pattern: Some("src/**".to_string()),
            command: "cargo check".to_string(),
            enabled: true,
            timeout: 120,
            background: false,
            description: Some("Check on file change".to_string()),
        };
        let json = serde_json::to_string(&def).unwrap();
        let back: TriggeredRoutineDef = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, def.name);
        assert_eq!(back.trigger, def.trigger);
        assert_eq!(back.command, def.command);
        assert_eq!(back.timeout, 120);
        assert!(!back.background);
    }

    #[test]
    fn error_display_messages() {
        let err = TriggeredRoutineError::NotFound("test".to_string());
        assert!(err.to_string().contains("test"));

        let err = TriggeredRoutineError::Disabled("my-routine".to_string());
        assert!(err.to_string().contains("my-routine"));

        let err = TriggeredRoutineError::Timeout {
            name: "slow".to_string(),
            timeout: 30,
        };
        assert!(err.to_string().contains("30"));
        assert!(err.to_string().contains("slow"));

        let err = TriggeredRoutineError::Execution {
            name: "bad".to_string(),
            error: "permission denied".to_string(),
        };
        assert!(err.to_string().contains("permission denied"));
    }

    #[test]
    fn list_names_returns_all() {
        let mut registry = TriggeredRoutineRegistry::new();
        registry.routines.insert(
            "a".to_string(),
            TriggeredRoutineDef {
                name: "a".to_string(),
                trigger: "Stop".to_string(),
                matcher: None,
                pattern: None,
                command: "echo a".to_string(),
                enabled: true,
                timeout: 10,
                background: true,
                description: None,
            },
        );
        registry.routines.insert(
            "b".to_string(),
            TriggeredRoutineDef {
                name: "b".to_string(),
                trigger: "Stop".to_string(),
                matcher: None,
                pattern: None,
                command: "echo b".to_string(),
                enabled: true,
                timeout: 10,
                background: true,
                description: None,
            },
        );
        let mut names = registry.list_names();
        names.sort();
        assert_eq!(names, vec!["a", "b"]);
    }
}
