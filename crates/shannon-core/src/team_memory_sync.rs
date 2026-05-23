//! # Team Memory Sync with Secret Scanner
//!
//! Bidirectional memory synchronization between local and team directories,
//! with built-in secret scanning to prevent accidental leakage of API keys,
//! tokens, and other sensitive credentials.
//!
//! ## Architecture
//!
//! - [`SecretScanner`]: Pattern-based scanner for common credential formats
//! - [`TeamMemorySync`]: Bidirectional sync engine with secret-gated uploads
//! - [`TeamMemoryGuard`]: Content gate that blocks or redacts detected secrets
//!
//! ## Secret Rules
//!
//! Curated high-confidence patterns sourced from gitleaks, covering:
//! - Anthropic API keys (`sk-ant-api03-...`)
//! - AWS access tokens (`AKIA[0-9A-Z]{16}`)
//! - GitHub PATs (`gh[pousr]_...`)
//! - Google API keys (`AIza[0-9A-Za-z\-_]{35}`)
//! - OpenAI API keys (`sk-[A-Za-z0-9]{48}`)
//! - Slack tokens (`xox[baprs]-...`)
//! - Stripe live keys (`sk_live_...`)

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use thiserror::Error;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during team memory sync operations.
#[derive(Error, Debug)]
pub enum TeamMemorySyncError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Secret detected in content: {0} matches found")]
    SecretsDetected(usize),

    #[error("Secret blocked from upload: {rule_id} in {source_desc}")]
    SecretBlocked {
        rule_id: String,
        source_desc: String,
    },

    #[error("Invalid path: {0}")]
    InvalidPath(String),

    #[error("Sync disabled")]
    SyncDisabled,

    #[error("Regex compilation failed for rule '{id}': {err}")]
    RegexCompile { id: String, err: String },

    #[error("Sync conflict: {local:?} vs {team:?}")]
    Conflict { local: PathBuf, team: PathBuf },

    #[error("File too large: {path} ({size} bytes, max {max} bytes)")]
    FileTooLarge { path: PathBuf, size: u64, max: u64 },
}

// ============================================================================
// Secret Scanner Types
// ============================================================================

/// A single secret-detection rule with a compiled regex and metadata.
#[derive(Debug, Clone)]
pub struct SecretRule {
    /// Unique identifier for this rule (e.g. `"anthropic-api-key"`).
    pub id: String,
    /// Human-readable description of what this rule detects.
    pub description: String,
    /// The raw regex pattern string.
    pub pattern: String,
    /// Whether the regex should be compiled case-sensitively.
    pub is_case_sensitive: bool,
    /// Compiled regex, populated by [`SecretRule::compiled`].
    compiled: Option<Regex>,
}

impl SecretRule {
    /// Create a new rule. Compiles the regex immediately.
    pub fn new(
        id: impl Into<String>,
        description: impl Into<String>,
        pattern: impl Into<String>,
        is_case_sensitive: bool,
    ) -> Result<Self, TeamMemorySyncError> {
        let id = id.into();
        let pattern = pattern.into();
        let compiled = if is_case_sensitive {
            Regex::new(&pattern)
        } else {
            Regex::new(&format!("(?i){pattern}"))
        }
        .map_err(|err| TeamMemorySyncError::RegexCompile {
            id: id.clone(),
            err: err.to_string(),
        })?;
        Ok(Self {
            id,
            description: description.into(),
            pattern,
            is_case_sensitive,
            compiled: Some(compiled),
        })
    }

    /// Convenience constructor that defers regex compilation.
    pub fn raw(
        id: impl Into<String>,
        description: impl Into<String>,
        pattern: impl Into<String>,
        is_case_sensitive: bool,
    ) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
            pattern: pattern.into(),
            is_case_sensitive,
            compiled: None,
        }
    }

    /// Return a reference to the compiled regex, compiling lazily if needed.
    pub fn compiled(&mut self) -> Result<&Regex, TeamMemorySyncError> {
        if self.compiled.is_none() {
            let re = if self.is_case_sensitive {
                Regex::new(&self.pattern)
            } else {
                Regex::new(&format!("(?i){}", self.pattern))
            }
            .map_err(|err| TeamMemorySyncError::RegexCompile {
                id: self.id.clone(),
                err: err.to_string(),
            })?;
            self.compiled = Some(re);
        }
        Ok(self
            .compiled
            .as_ref()
            .expect("compiled must be set after successful compilation above"))
    }
}

/// A single match produced by the secret scanner.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecretMatch {
    /// ID of the rule that matched.
    pub rule_id: String,
    /// Human-readable label (rule description).
    pub label: String,
    /// The full line of text that contained the match.
    pub matched_line: String,
    /// 1-based line number where the match was found.
    pub line_number: usize,
}

impl std::fmt::Display for SecretMatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}] {} (line {})",
            self.rule_id, self.label, self.line_number
        )
    }
}

// ============================================================================
// Secret Scanner
// ============================================================================

/// Pattern-based secret scanner that checks content against a set of rules.
///
/// Rules are compiled once at construction time and reused across scans.
/// Default rules cover common cloud provider and service credentials.
#[derive(Debug, Clone)]
pub struct SecretScanner {
    rules: Vec<SecretRule>,
}

impl SecretScanner {
    /// Create a scanner with the default curated rules.
    pub fn new() -> Self {
        Self {
            rules: Self::default_rules(),
        }
    }

    /// Create a scanner with custom rules.
    pub fn with_rules(rules: Vec<SecretRule>) -> Self {
        Self { rules }
    }

    /// Return the curated set of default secret-detection rules.
    ///
    /// These are high-confidence patterns derived from gitleaks:
    pub fn default_rules() -> Vec<SecretRule> {
        let raw_rules = vec![
            (
                "anthropic-api-key",
                "Anthropic API key",
                r"sk-ant-api03-[A-Za-z0-9\-_]{90,}",
                true,
            ),
            (
                "aws-access-token",
                "AWS access token",
                r"AKIA[0-9A-Z]{16}",
                true,
            ),
            (
                "aws-secret-key",
                "AWS secret key",
                r"(?:^|[^A-Za-z0-9])aws(?:[_\-]{0,1}[A-Za-z0-9]{0,20})?secret(?:[_\-]{0,1}[A-Za-z0-9]{0,20})?[[:space:]]*[:=][[:space:]]*[A-Za-z0-9/+=]{40}",
                false,
            ),
            (
                "github-pat",
                "GitHub personal access token",
                r"gh[pousr]_[A-Za-z0-9_]{36,}",
                true,
            ),
            (
                "google-api-key",
                "Google API key",
                r"AIza[0-9A-Za-z\-_]{35}",
                true,
            ),
            (
                "openai-api-key",
                "OpenAI API key",
                r"sk-[A-Za-z0-9]{48}",
                true,
            ),
            ("slack-token", "Slack token", r"xox[baprs]-[0-9]{10,}", true),
            (
                "stripe-key",
                "Stripe live key",
                r"sk_live_[0-9a-z]{24,}",
                false,
            ),
        ];

        raw_rules
            .into_iter()
            .filter_map(|(id, desc, pattern, case_sensitive)| {
                SecretRule::new(id, desc, pattern, case_sensitive).ok()
            })
            .collect()
    }

    /// Scan a string for secrets, returning all matches.
    pub fn scan(&self, content: &str) -> Vec<SecretMatch> {
        let mut matches = Vec::new();
        for rule in &self.rules {
            if let Some(ref compiled) = rule.compiled {
                for (line_idx, line) in content.lines().enumerate() {
                    if compiled.is_match(line) {
                        matches.push(SecretMatch {
                            rule_id: rule.id.clone(),
                            label: rule.description.clone(),
                            matched_line: line.to_string(),
                            line_number: line_idx + 1,
                        });
                    }
                }
            }
        }
        matches
    }

    /// Scan a file on disk for secrets.
    pub fn scan_file(&self, path: &Path) -> Result<Vec<SecretMatch>, TeamMemorySyncError> {
        let content = fs::read_to_string(path)?;
        Ok(self.scan(&content))
    }

    /// Return the number of loaded rules.
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// Return the IDs of all loaded rules.
    pub fn rule_ids(&self) -> Vec<&str> {
        self.rules.iter().map(|r| r.id.as_str()).collect()
    }

    /// Check whether content contains any secrets.
    pub fn has_secrets(&self, content: &str) -> bool {
        !self.scan(content).is_empty()
    }
}

impl Default for SecretScanner {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Team Memory Config
// ============================================================================

/// Configuration for [`TeamMemorySync`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMemoryConfig {
    /// Whether team memory sync is enabled.
    pub enabled: bool,
    /// Local memory directory (e.g. `~/.shannon/memories/`).
    pub local_memory_dir: PathBuf,
    /// Team-shared memory directory.
    pub team_memory_dir: PathBuf,
    /// Sync interval in seconds.
    pub sync_interval_secs: u64,
    /// Whether secret scanning is enforced on uploads.
    pub secret_scanning_enabled: bool,
}

impl Default for TeamMemoryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            local_memory_dir: PathBuf::from(".shannon/memories"),
            team_memory_dir: PathBuf::from(".shannon/team-memories"),
            sync_interval_secs: 300,
            secret_scanning_enabled: true,
        }
    }
}

// ============================================================================
// Sync Result
// ============================================================================

/// Result of a sync operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    /// Files uploaded (local -> team).
    pub uploaded: Vec<PathBuf>,
    /// Files downloaded (team -> local).
    pub downloaded: Vec<PathBuf>,
    /// Secret matches that blocked uploads.
    pub secrets_blocked: Vec<SecretMatch>,
    /// Non-fatal error messages.
    pub errors: Vec<String>,
    /// Wall-clock duration of the sync in milliseconds.
    pub duration_ms: u64,
}

impl SyncResult {
    /// Returns `true` if the sync completed without any issues.
    pub fn is_clean(&self) -> bool {
        self.secrets_blocked.is_empty() && self.errors.is_empty()
    }

    /// Returns the total number of files transferred (uploaded + downloaded).
    pub fn total_transferred(&self) -> usize {
        self.uploaded.len() + self.downloaded.len()
    }
}

// ============================================================================
// Team Memory Sync
// ============================================================================

/// Bidirectional sync engine for local and team memory directories.
///
/// Uploads are gated through the secret scanner. Downloads from the team
/// directory are copied verbatim (the team directory is considered trusted).
pub struct TeamMemorySync {
    local_dir: PathBuf,
    team_dir: PathBuf,
    secret_scanner: SecretScanner,
    enabled: bool,
    sync_interval: Duration,
    secret_scanning_enabled: bool,
    /// Maximum file size allowed for sync (default 1 MB).
    max_file_size: u64,
}

impl TeamMemorySync {
    /// Create a new sync engine from the given configuration.
    ///
    /// Creates the local and team directories if they do not exist.
    pub fn new(config: TeamMemoryConfig) -> Result<Self, TeamMemorySyncError> {
        if config.enabled {
            fs::create_dir_all(&config.local_memory_dir)?;
            fs::create_dir_all(&config.team_memory_dir)?;
        }

        Ok(Self {
            local_dir: config.local_memory_dir,
            team_dir: config.team_memory_dir,
            secret_scanner: if config.secret_scanning_enabled {
                SecretScanner::new()
            } else {
                SecretScanner::with_rules(vec![])
            },
            enabled: config.enabled,
            sync_interval: Duration::from_secs(config.sync_interval_secs),
            secret_scanning_enabled: config.secret_scanning_enabled,
            max_file_size: 1_048_576, // 1 MB
        })
    }

    /// Returns `true` if sync is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Perform a bidirectional sync: upload new/changed local memories
    /// to the team directory, and download new/changed team memories
    /// to the local directory.
    pub fn sync(&self) -> Result<SyncResult, TeamMemorySyncError> {
        if !self.enabled {
            return Err(TeamMemorySyncError::SyncDisabled);
        }

        let start = Instant::now();
        let mut result = SyncResult {
            uploaded: Vec::new(),
            downloaded: Vec::new(),
            secrets_blocked: Vec::new(),
            errors: Vec::new(),
            duration_ms: 0,
        };

        // Upload: local -> team
        self.sync_direction(
            &self.local_dir,
            &self.team_dir,
            &mut result.uploaded,
            &mut result.secrets_blocked,
            &mut result.errors,
        );

        // Download: team -> local
        self.sync_direction_trusted(
            &self.team_dir,
            &self.local_dir,
            &mut result.downloaded,
            &mut result.errors,
        );

        result.duration_ms = start.elapsed().as_millis() as u64;
        Ok(result)
    }

    /// Upload a single memory file, scanning for secrets first.
    pub fn upload_memory(&self, path: &Path) -> Result<(), TeamMemorySyncError> {
        if !self.enabled {
            return Err(TeamMemorySyncError::SyncDisabled);
        }

        if !path.starts_with(&self.local_dir) {
            return Err(TeamMemorySyncError::InvalidPath(format!(
                "Path {:?} is not inside the local memory directory {:?}",
                path, self.local_dir
            )));
        }

        let metadata = fs::metadata(path)?;
        if metadata.len() > self.max_file_size {
            return Err(TeamMemorySyncError::FileTooLarge {
                path: path.to_path_buf(),
                size: metadata.len(),
                max: self.max_file_size,
            });
        }

        if self.secret_scanning_enabled {
            let matches = self.secret_scanner.scan_file(path)?;
            if !matches.is_empty() {
                return Err(TeamMemorySyncError::SecretsDetected(matches.len()));
            }
        }

        let relative = path.strip_prefix(&self.local_dir).map_err(|_| {
            TeamMemorySyncError::InvalidPath(format!("Cannot strip prefix from {path:?}"))
        })?;
        let dest = self.team_dir.join(relative);

        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(path, &dest)?;

        Ok(())
    }

    /// Download a single memory file from the team directory.
    pub fn download_memory(&self, path: &Path) -> Result<(), TeamMemorySyncError> {
        if !self.enabled {
            return Err(TeamMemorySyncError::SyncDisabled);
        }

        if !path.starts_with(&self.team_dir) {
            return Err(TeamMemorySyncError::InvalidPath(format!(
                "Path {:?} is not inside the team memory directory {:?}",
                path, self.team_dir
            )));
        }

        let relative = path.strip_prefix(&self.team_dir).map_err(|_| {
            TeamMemorySyncError::InvalidPath(format!("Cannot strip prefix from {path:?}"))
        })?;
        let dest = self.local_dir.join(relative);

        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(path, &dest)?;

        Ok(())
    }

    /// Scan content for secrets using the internal scanner.
    pub fn scan_for_secrets(&self, content: &str) -> Vec<SecretMatch> {
        self.secret_scanner.scan(content)
    }

    /// List all memory file names in the team directory.
    pub fn list_team_memories(&self) -> Result<Vec<String>, TeamMemorySyncError> {
        if !self.team_dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries = Vec::new();
        self.walk_memory_files(&self.team_dir, &mut entries)?;
        Ok(entries)
    }

    /// List all memory file names in the local directory.
    pub fn list_local_memories(&self) -> Result<Vec<String>, TeamMemorySyncError> {
        if !self.local_dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries = Vec::new();
        self.walk_memory_files(&self.local_dir, &mut entries)?;
        Ok(entries)
    }

    /// Check whether a path is inside the team memory directory.
    pub fn is_team_path(&self, path: &Path) -> bool {
        path.starts_with(&self.team_dir)
    }

    /// Check whether a path is inside the local memory directory.
    pub fn is_local_path(&self, path: &Path) -> bool {
        path.starts_with(&self.local_dir)
    }

    /// Return the configured sync interval.
    pub fn sync_interval(&self) -> Duration {
        self.sync_interval
    }

    /// Return the maximum allowed file size.
    pub fn max_file_size(&self) -> u64 {
        self.max_file_size
    }

    // -- Private helpers ---------------------------------------------------

    /// Sync files from `src_dir` to `dst_dir` with secret scanning.
    /// Files that contain secrets are reported in `blocked_secrets`.
    fn sync_direction(
        &self,
        src_dir: &Path,
        dst_dir: &Path,
        uploaded: &mut Vec<PathBuf>,
        blocked_secrets: &mut Vec<SecretMatch>,
        errors: &mut Vec<String>,
    ) {
        let src_files = match self.collect_memory_files(src_dir) {
            Ok(f) => f,
            Err(e) => {
                errors.push(format!("Failed to list source files: {e}"));
                return;
            }
        };

        for src_path in &src_files {
            let relative = match src_path.strip_prefix(src_dir) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let dst_path = dst_dir.join(relative);

            // Skip if destination is newer or equal
            if let (Ok(src_meta), Ok(dst_meta)) = (fs::metadata(src_path), fs::metadata(&dst_path))
            {
                if let (Ok(src_time), Ok(dst_time)) = (src_meta.modified(), dst_meta.modified()) {
                    if src_time <= dst_time {
                        continue;
                    }
                }
            }

            // Secret scanning for uploads
            if self.secret_scanning_enabled {
                match self.secret_scanner.scan_file(src_path) {
                    Ok(matches) if matches.is_empty() => {}
                    Ok(matches) => {
                        blocked_secrets.extend(matches);
                        continue;
                    }
                    Err(e) => {
                        errors.push(format!("Failed to scan {src_path:?} for secrets: {e}"));
                        continue;
                    }
                }
            }

            // Copy
            if let Some(parent) = dst_path.parent() {
                if let Err(e) = fs::create_dir_all(parent) {
                    errors.push(format!("Failed to create directory: {e}"));
                    continue;
                }
            }
            match fs::copy(src_path, &dst_path) {
                Ok(_) => uploaded.push(relative.to_path_buf()),
                Err(e) => {
                    errors.push(format!("Failed to copy {src_path:?}: {e}"));
                }
            }
        }
    }

    /// Sync files from `src_dir` to `dst_dir` without secret scanning
    /// (trusted direction, e.g. team -> local).
    fn sync_direction_trusted(
        &self,
        src_dir: &Path,
        dst_dir: &Path,
        downloaded: &mut Vec<PathBuf>,
        errors: &mut Vec<String>,
    ) {
        let src_files = match self.collect_memory_files(src_dir) {
            Ok(f) => f,
            Err(e) => {
                errors.push(format!("Failed to list source files: {e}"));
                return;
            }
        };

        for src_path in &src_files {
            let relative = match src_path.strip_prefix(src_dir) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let dst_path = dst_dir.join(relative);

            // Skip if destination is newer or equal
            if let (Ok(src_meta), Ok(dst_meta)) = (fs::metadata(src_path), fs::metadata(&dst_path))
            {
                if let (Ok(src_time), Ok(dst_time)) = (src_meta.modified(), dst_meta.modified()) {
                    if src_time <= dst_time {
                        continue;
                    }
                }
            }

            if let Some(parent) = dst_path.parent() {
                if let Err(e) = fs::create_dir_all(parent) {
                    errors.push(format!("Failed to create directory: {e}"));
                    continue;
                }
            }
            match fs::copy(src_path, &dst_path) {
                Ok(_) => downloaded.push(relative.to_path_buf()),
                Err(e) => {
                    errors.push(format!("Failed to copy {src_path:?}: {e}"));
                }
            }
        }
    }

    /// Collect all regular files recursively under `dir`.
    fn collect_memory_files(&self, dir: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
        let mut files = Vec::new();
        Self::walk_files_recursive(dir, &mut files)?;
        Ok(files)
    }

    /// Recursively walk `dir` collecting regular files.
    fn walk_files_recursive(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), std::io::Error> {
        if !dir.is_dir() {
            return Ok(());
        }
        let entries = fs::read_dir(dir)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                // Skip hidden directories
                if let Some(name) = path.file_name() {
                    if name.to_string_lossy().starts_with('.') {
                        continue;
                    }
                }
                Self::walk_files_recursive(&path, files)?;
            } else if path.is_file() {
                files.push(path);
            }
        }
        Ok(())
    }

    /// Walk memory files and collect relative path strings.
    fn walk_memory_files(
        &self,
        dir: &Path,
        entries: &mut Vec<String>,
    ) -> Result<(), TeamMemorySyncError> {
        let files = self.collect_memory_files(dir)?;
        for file in files {
            if let Ok(relative) = file.strip_prefix(dir) {
                entries.push(relative.to_string_lossy().to_string());
            }
        }
        Ok(())
    }
}

// ============================================================================
// Team Memory Guard
// ============================================================================

/// A content guard that blocks or redacts detected secrets.
///
/// Used as a safety net before any content is persisted or transmitted.
pub struct TeamMemoryGuard {
    scanner: SecretScanner,
    blocked_categories: HashSet<String>,
}

impl TeamMemoryGuard {
    /// Create a guard with the default scanner and all default rule categories blocked.
    pub fn new() -> Self {
        let scanner = SecretScanner::new();
        let blocked_categories: HashSet<String> =
            scanner.rule_ids().iter().map(|s| s.to_string()).collect();
        Self {
            scanner,
            blocked_categories,
        }
    }

    /// Create a guard with a custom scanner and explicit blocked categories.
    pub fn with_blocked_categories(
        scanner: SecretScanner,
        blocked_categories: Vec<String>,
    ) -> Self {
        Self {
            scanner,
            blocked_categories: blocked_categories.into_iter().collect(),
        }
    }

    /// Check content for blocked secrets. Returns all matches from blocked rules.
    ///
    /// If the result is non-empty, the content should not be persisted or shared.
    pub fn check_content(
        &self,
        content: &str,
        _source: &str,
    ) -> Result<Vec<SecretMatch>, TeamMemorySyncError> {
        let all_matches = self.scanner.scan(content);
        let blocked: Vec<SecretMatch> = all_matches
            .into_iter()
            .filter(|m| self.blocked_categories.contains(&m.rule_id))
            .collect();

        if blocked.is_empty() {
            Ok(Vec::new())
        } else {
            Err(TeamMemorySyncError::SecretsDetected(blocked.len()))
        }
    }

    /// Check content and return all blocked matches (without returning an error).
    ///
    /// Useful for reporting purposes.
    pub fn find_blocked(&self, content: &str) -> Vec<SecretMatch> {
        let all_matches = self.scanner.scan(content);
        all_matches
            .into_iter()
            .filter(|m| self.blocked_categories.contains(&m.rule_id))
            .collect()
    }

    /// Check whether a specific category is blocked.
    pub fn is_blocked(&self, category: &str) -> bool {
        self.blocked_categories.contains(category)
    }

    /// Add a category to the blocked set.
    pub fn block_category(&mut self, category: impl Into<String>) {
        self.blocked_categories.insert(category.into());
    }

    /// Remove a category from the blocked set.
    pub fn unblock_category(&mut self, category: &str) {
        self.blocked_categories.remove(category);
    }

    /// Sanitize content by redacting detected secrets.
    ///
    /// Each matched secret line has the matched portion replaced with
    /// `[REDACTED:<rule_id>]`.
    pub fn sanitize_content(&self, content: &str) -> String {
        let matches = self.find_blocked(content);
        let mut lines: Vec<String> = content.lines().map(String::from).collect();

        for m in &matches {
            if m.line_number > 0 && m.line_number <= lines.len() {
                let idx = m.line_number - 1;
                let line = &lines[idx];
                // Find and redact the matched portion
                if let Some(compiled) = self
                    .scanner
                    .rules
                    .iter()
                    .find(|r| r.id == m.rule_id)
                    .and_then(|r| r.compiled.as_ref())
                {
                    let redacted = compiled.replace(line, format!("[REDACTED:{}]", m.rule_id));
                    lines[idx] = redacted.to_string();
                }
            }
        }

        lines.join("\n")
    }

    /// Return the IDs of all blocked categories.
    pub fn blocked_categories(&self) -> Vec<&str> {
        let mut cats: Vec<&str> = self.blocked_categories.iter().map(|s| s.as_str()).collect();
        cats.sort();
        cats
    }

    /// Return the total number of blocked categories.
    pub fn blocked_count(&self) -> usize {
        self.blocked_categories.len()
    }
}

impl Default for TeamMemoryGuard {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // -- SecretRule tests --------------------------------------------------

    #[test]
    fn test_secret_rule_new_valid() {
        let rule = SecretRule::new("test-rule", "Test", r"\bTEST_\w+\b", true).unwrap();
        assert_eq!(rule.id, "test-rule");
        assert!(rule.compiled.is_some());
    }

    #[test]
    fn test_secret_rule_new_invalid_regex() {
        let result = SecretRule::new("bad", "Bad", r"(?P<unclosed", true);
        assert!(result.is_err());
        match result.unwrap_err() {
            TeamMemorySyncError::RegexCompile { id, .. } => assert_eq!(id, "bad"),
            other => panic!("Expected RegexCompile error, got: {other:?}"),
        }
    }

    #[test]
    fn test_secret_rule_raw_deferred_compile() {
        let mut rule = SecretRule::raw("lazy", "Lazy", r"\blazy\b", false);
        assert!(rule.compiled.is_none());
        let compiled = rule.compiled().unwrap();
        assert!(compiled.is_match("lazy"));
        assert!(compiled.is_match("LAZY"));
    }

    #[test]
    fn test_secret_rule_case_sensitivity() {
        let mut sensitive = SecretRule::raw("sensitive", "S", r"ABC", true);
        assert!(sensitive.compiled().unwrap().is_match("ABC"));
        assert!(!sensitive.compiled().unwrap().is_match("abc"));

        let mut insensitive = SecretRule::raw("insensitive", "I", r"ABC", false);
        assert!(insensitive.compiled().unwrap().is_match("ABC"));
        assert!(insensitive.compiled().unwrap().is_match("abc"));
    }

    // -- SecretScanner tests -----------------------------------------------

    #[test]
    fn test_scanner_default_rules_loaded() {
        let scanner = SecretScanner::new();
        assert!(scanner.rule_count() >= 8);
    }

    #[test]
    fn test_scanner_rule_ids() {
        let scanner = SecretScanner::new();
        let ids = scanner.rule_ids();
        assert!(ids.contains(&"anthropic-api-key"));
        assert!(ids.contains(&"aws-access-token"));
        assert!(ids.contains(&"github-pat"));
        assert!(ids.contains(&"openai-api-key"));
    }

    #[test]
    fn test_scanner_detects_anthropic_key() {
        let scanner = SecretScanner::new();
        let content = format!("ANTHROPIC_API_KEY=sk-ant-api03-{}", "A".repeat(95));
        let matches = scanner.scan(&content);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].rule_id, "anthropic-api-key");
        assert_eq!(matches[0].line_number, 1);
    }

    #[test]
    fn test_scanner_detects_aws_access_token() {
        let scanner = SecretScanner::new();
        let content = "AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE";
        let matches = scanner.scan(content);
        assert!(matches.iter().any(|m| m.rule_id == "aws-access-token"));
    }

    #[test]
    fn test_scanner_detects_github_pat() {
        let scanner = SecretScanner::new();
        let content = "GITHUB_TOKEN=ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij";
        let matches = scanner.scan(content);
        assert!(matches.iter().any(|m| m.rule_id == "github-pat"));
    }

    #[test]
    fn test_scanner_detects_google_api_key() {
        let scanner = SecretScanner::new();
        let content = "GOOGLE_API_KEY=AIzaSyA1234567890abcdefghijklmnopqrstuv";
        let matches = scanner.scan(content);
        assert!(matches.iter().any(|m| m.rule_id == "google-api-key"));
    }

    #[test]
    fn test_scanner_detects_openai_key() {
        let scanner = SecretScanner::new();
        let content =
            "OPENAI_API_KEY=sk-ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz12345678";
        let matches = scanner.scan(content);
        assert!(matches.iter().any(|m| m.rule_id == "openai-api-key"));
    }

    #[test]
    fn test_scanner_detects_slack_token() {
        let scanner = SecretScanner::new();
        let content = "SLACK_TOKEN=xoxb-1234567890";
        let matches = scanner.scan(content);
        assert!(matches.iter().any(|m| m.rule_id == "slack-token"));
    }

    #[test]
    fn test_scanner_detects_stripe_key() {
        let scanner = SecretScanner::new();
        let content = concat!("STRIPE_KEY=sk_live_", "000000000000000000000000");
        let matches = scanner.scan(content);
        assert!(matches.iter().any(|m| m.rule_id == "stripe-key"));
    }

    #[test]
    fn test_scanner_clean_content() {
        let scanner = SecretScanner::new();
        let content = "This is a normal file with no secrets.\nJust regular text here.\n";
        let matches = scanner.scan(content);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_scanner_multiple_secrets() {
        let scanner = SecretScanner::new();
        let content = format!(
            "KEY1=sk-ant-api03-{}\nKEY2=ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij\n",
            "A".repeat(90)
        );
        let matches = scanner.scan(&content);
        assert!(matches.len() >= 2);
    }

    #[test]
    fn test_scanner_line_numbers() {
        let scanner = SecretScanner::new();
        let content = format!(
            "line one\nline two\nsk-ant-api03-{}\nline four",
            "A".repeat(95)
        );
        let matches = scanner.scan(&content);
        assert_eq!(matches[0].line_number, 3);
    }

    #[test]
    fn test_scanner_has_secrets() {
        let scanner = SecretScanner::new();
        assert!(scanner.has_secrets(&format!("KEY=sk-ant-api03-{}", "A".repeat(95))));
        assert!(!scanner.has_secrets("Hello world"));
    }

    #[test]
    fn test_scanner_with_empty_rules() {
        let scanner = SecretScanner::with_rules(vec![]);
        assert_eq!(scanner.rule_count(), 0);
        assert!(scanner.scan("anything").is_empty());
    }

    #[test]
    fn test_scanner_scan_file() {
        let tmp = TempDir::new().unwrap();
        let file_path = tmp.path().join("test.md");
        fs::write(
            &file_path,
            "GITHUB_TOKEN=ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij\n",
        )
        .unwrap();

        let scanner = SecretScanner::new();
        let matches = scanner.scan_file(&file_path).unwrap();
        assert!(matches.iter().any(|m| m.rule_id == "github-pat"));
    }

    // -- TeamMemorySync tests ----------------------------------------------

    #[test]
    fn test_sync_disabled_returns_error() {
        let config = TeamMemoryConfig {
            enabled: false,
            ..Default::default()
        };
        let sync = TeamMemorySync::new(config).unwrap();
        assert!(matches!(
            sync.sync(),
            Err(TeamMemorySyncError::SyncDisabled)
        ));
    }

    #[test]
    fn test_sync_enabled_creates_dirs() {
        let tmp = TempDir::new().unwrap();
        let local = tmp.path().join("local");
        let team = tmp.path().join("team");
        let config = TeamMemoryConfig {
            enabled: true,
            local_memory_dir: local.clone(),
            team_memory_dir: team.clone(),
            ..Default::default()
        };
        let _sync = TeamMemorySync::new(config).unwrap();
        assert!(local.is_dir());
        assert!(team.is_dir());
    }

    #[test]
    fn test_sync_bidirectional() {
        let tmp = TempDir::new().unwrap();
        let local = tmp.path().join("local");
        let team = tmp.path().join("team");
        fs::create_dir_all(&local).unwrap();
        fs::create_dir_all(&team).unwrap();

        // Write a local memory
        fs::write(local.join("feature.md"), "This is a local memory\n").unwrap();

        // Write a team memory
        fs::write(team.join("shared.md"), "This is a team memory\n").unwrap();

        let config = TeamMemoryConfig {
            enabled: true,
            local_memory_dir: local.clone(),
            team_memory_dir: team.clone(),
            secret_scanning_enabled: true,
            ..Default::default()
        };
        let sync = TeamMemorySync::new(config).unwrap();
        let result = sync.sync().unwrap();

        // Both should have been synced
        assert!(
            result
                .uploaded
                .iter()
                .any(|p| p.to_string_lossy() == "feature.md")
        );
        assert!(
            result
                .downloaded
                .iter()
                .any(|p| p.to_string_lossy() == "shared.md")
        );
        assert!(result.is_clean());

        // Verify files exist in both dirs
        assert!(local.join("shared.md").exists());
        assert!(team.join("feature.md").exists());
    }

    #[test]
    fn test_upload_blocks_secrets() {
        let tmp = TempDir::new().unwrap();
        let local = tmp.path().join("local");
        let team = tmp.path().join("team");
        fs::create_dir_all(&local).unwrap();
        fs::create_dir_all(&team).unwrap();

        // Write a file with a secret
        let secret_content = format!("API_KEY=sk-ant-api03-{}\n", "A".repeat(90));
        fs::write(local.join("secret.md"), &secret_content).unwrap();

        let config = TeamMemoryConfig {
            enabled: true,
            local_memory_dir: local.clone(),
            team_memory_dir: team.clone(),
            secret_scanning_enabled: true,
            ..Default::default()
        };
        let sync = TeamMemorySync::new(config).unwrap();
        let result = sync.sync().unwrap();

        assert!(!result.secrets_blocked.is_empty());
        assert!(!team.join("secret.md").exists());
    }

    #[test]
    fn test_upload_memory_blocks_secrets() {
        let tmp = TempDir::new().unwrap();
        let local = tmp.path().join("local");
        let team = tmp.path().join("team");
        fs::create_dir_all(&local).unwrap();
        fs::create_dir_all(&team).unwrap();

        let secret_content = format!("API_KEY=sk-ant-api03-{}\n", "A".repeat(90));
        fs::write(local.join("bad.md"), &secret_content).unwrap();

        let config = TeamMemoryConfig {
            enabled: true,
            local_memory_dir: local.clone(),
            team_memory_dir: team.clone(),
            secret_scanning_enabled: true,
            ..Default::default()
        };
        let sync = TeamMemorySync::new(config).unwrap();
        let result = sync.upload_memory(&local.join("bad.md"));
        assert!(matches!(
            result,
            Err(TeamMemorySyncError::SecretsDetected(_))
        ));
    }

    #[test]
    fn test_upload_memory_invalid_path() {
        let tmp = TempDir::new().unwrap();
        let local = tmp.path().join("local");
        let team = tmp.path().join("team");
        fs::create_dir_all(&local).unwrap();
        fs::create_dir_all(&team).unwrap();

        let config = TeamMemoryConfig {
            enabled: true,
            local_memory_dir: local.clone(),
            team_memory_dir: team.clone(),
            ..Default::default()
        };
        let sync = TeamMemorySync::new(config).unwrap();

        let outside = tmp.path().join("outside.md");
        let result = sync.upload_memory(&outside);
        assert!(matches!(result, Err(TeamMemorySyncError::InvalidPath(_))));
    }

    #[test]
    fn test_download_memory() {
        let tmp = TempDir::new().unwrap();
        let local = tmp.path().join("local");
        let team = tmp.path().join("team");
        fs::create_dir_all(&local).unwrap();
        fs::create_dir_all(&team).unwrap();

        fs::write(team.join("team_file.md"), "team content\n").unwrap();

        let config = TeamMemoryConfig {
            enabled: true,
            local_memory_dir: local.clone(),
            team_memory_dir: team.clone(),
            ..Default::default()
        };
        let sync = TeamMemorySync::new(config).unwrap();
        sync.download_memory(&team.join("team_file.md")).unwrap();

        assert!(local.join("team_file.md").exists());
    }

    #[test]
    fn test_list_team_memories() {
        let tmp = TempDir::new().unwrap();
        let team = tmp.path().join("team");
        fs::create_dir_all(team.join("subdir")).unwrap();
        fs::write(team.join("a.md"), "a\n").unwrap();
        fs::write(team.join("subdir/b.md"), "b\n").unwrap();

        let config = TeamMemoryConfig {
            enabled: true,
            team_memory_dir: team.clone(),
            ..Default::default()
        };
        let sync = TeamMemorySync::new(config).unwrap();
        let memories = sync.list_team_memories().unwrap();

        assert!(memories.iter().any(|m| m == "a.md"));
        assert!(memories.iter().any(|m| m.contains("b.md")));
    }

    #[test]
    fn test_is_team_path() {
        let _config = TeamMemoryConfig {
            enabled: true,
            team_memory_dir: PathBuf::from("/tmp/team"),
            ..Default::default()
        };
        // We won't create dirs here; just test the path logic.
        // Use a non-enabled config so it doesn't try to create dirs.
        let config = TeamMemoryConfig {
            enabled: false,
            team_memory_dir: PathBuf::from("/tmp/team"),
            local_memory_dir: PathBuf::from("/tmp/local"),
            ..Default::default()
        };
        let sync = TeamMemorySync::new(config).unwrap();
        assert!(sync.is_team_path(Path::new("/tmp/team/file.md")));
        assert!(!sync.is_team_path(Path::new("/tmp/other/file.md")));
        assert!(sync.is_local_path(Path::new("/tmp/local/file.md")));
        assert!(!sync.is_local_path(Path::new("/tmp/team/file.md")));
    }

    #[test]
    fn test_scan_for_secrets() {
        let _config = TeamMemoryConfig::default();
        let sync = TeamMemorySync::new(TeamMemoryConfig::default()).unwrap();
        let content = format!("KEY=sk-ant-api03-{}", "A".repeat(95));
        let matches = sync.scan_for_secrets(&content);
        assert!(!matches.is_empty());
    }

    #[test]
    fn test_sync_skips_older_files() {
        let tmp = TempDir::new().unwrap();
        let local = tmp.path().join("local");
        let team = tmp.path().join("team");
        fs::create_dir_all(&local).unwrap();
        fs::create_dir_all(&team).unwrap();

        // Write a team file, then a local file with same name but same content time
        fs::write(team.join("existing.md"), "v1\n").unwrap();
        fs::write(local.join("existing.md"), "v1\n").unwrap();

        // Force identical timestamps — rapid sequential writes may differ by nanoseconds
        let _ = std::process::Command::new("touch")
            .arg("-r")
            .arg(team.join("existing.md"))
            .arg(local.join("existing.md"))
            .status();

        let config = TeamMemoryConfig {
            enabled: true,
            local_memory_dir: local.clone(),
            team_memory_dir: team.clone(),
            ..Default::default()
        };
        let sync = TeamMemorySync::new(config).unwrap();
        let result = sync.sync().unwrap();

        // Neither direction should transfer since timestamps are equal
        assert!(result.uploaded.is_empty());
        assert!(result.downloaded.is_empty());
    }

    // -- TeamMemoryGuard tests ---------------------------------------------

    #[test]
    fn test_guard_blocks_secrets() {
        let guard = TeamMemoryGuard::new();
        let content = format!("API_KEY=sk-ant-api03-{}\n", "A".repeat(95));
        let result = guard.check_content(&content, "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_guard_allows_clean_content() {
        let guard = TeamMemoryGuard::new();
        let content = "This is safe content with no secrets.\n";
        let result = guard.check_content(content, "test");
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_guard_find_blocked() {
        let guard = TeamMemoryGuard::new();
        let content = format!("KEY=sk-ant-api03-{}\nSAFE=line\n", "A".repeat(90));
        let blocked = guard.find_blocked(&content);
        assert_eq!(blocked.len(), 1);
        assert_eq!(blocked[0].rule_id, "anthropic-api-key");
    }

    #[test]
    fn test_guard_is_blocked() {
        let guard = TeamMemoryGuard::new();
        assert!(guard.is_blocked("anthropic-api-key"));
        assert!(guard.is_blocked("aws-access-token"));
        assert!(!guard.is_blocked("nonexistent-rule"));
    }

    #[test]
    fn test_guard_block_unblock_category() {
        let mut guard = TeamMemoryGuard::new();
        guard.unblock_category("anthropic-api-key");
        assert!(!guard.is_blocked("anthropic-api-key"));
        guard.block_category("anthropic-api-key");
        assert!(guard.is_blocked("anthropic-api-key"));
    }

    #[test]
    fn test_guard_sanitize_content() {
        let guard = TeamMemoryGuard::new();
        let content = format!("KEY=sk-ant-api03-{}\nSAFE=line\n", "A".repeat(90));
        let sanitized = guard.sanitize_content(&content);
        assert!(sanitized.contains("[REDACTED:anthropic-api-key]"));
        assert!(sanitized.contains("SAFE=line"));
    }

    #[test]
    fn test_guard_sanitize_preserves_line_structure() {
        let guard = TeamMemoryGuard::new();
        let content = "line1\nline2\nline3\n";
        let sanitized = guard.sanitize_content(content);
        let line_count = sanitized.lines().count();
        assert_eq!(line_count, 3);
    }

    #[test]
    fn test_guard_blocked_categories() {
        let guard = TeamMemoryGuard::new();
        let cats = guard.blocked_categories();
        assert!(!cats.is_empty());
        assert!(cats.contains(&"anthropic-api-key"));
    }

    #[test]
    fn test_guard_blocked_count() {
        let guard = TeamMemoryGuard::new();
        assert!(guard.blocked_count() >= 8);
    }

    #[test]
    fn test_guard_with_custom_categories() {
        let scanner = SecretScanner::new();
        let guard = TeamMemoryGuard::with_blocked_categories(
            scanner,
            vec!["anthropic-api-key".to_string()],
        );
        assert!(guard.is_blocked("anthropic-api-key"));
        assert!(!guard.is_blocked("aws-access-token"));
    }

    #[test]
    fn test_guard_custom_allows_other_secrets() {
        let scanner = SecretScanner::new();
        let guard = TeamMemoryGuard::with_blocked_categories(
            scanner,
            vec!["anthropic-api-key".to_string()],
        );
        // AWS key should not be blocked
        let content = "AWS_KEY=AKIAIOSFODNN7EXAMPLE\n";
        let blocked = guard.find_blocked(content);
        assert!(blocked.iter().all(|m| m.rule_id != "aws-access-token"));
    }

    // -- SyncResult tests --------------------------------------------------

    #[test]
    fn test_sync_result_is_clean() {
        let clean = SyncResult {
            uploaded: vec![PathBuf::from("a.md")],
            downloaded: vec![PathBuf::from("b.md")],
            secrets_blocked: vec![],
            errors: vec![],
            duration_ms: 10,
        };
        assert!(clean.is_clean());
        assert_eq!(clean.total_transferred(), 2);

        let dirty = SyncResult {
            uploaded: vec![],
            downloaded: vec![],
            secrets_blocked: vec![SecretMatch {
                rule_id: "test".into(),
                label: "Test".into(),
                matched_line: "x".into(),
                line_number: 1,
            }],
            errors: vec![],
            duration_ms: 5,
        };
        assert!(!dirty.is_clean());
        assert_eq!(dirty.total_transferred(), 0);
    }

    // -- TeamMemoryConfig tests --------------------------------------------

    #[test]
    fn test_config_defaults() {
        let config = TeamMemoryConfig::default();
        assert!(!config.enabled);
        assert!(config.secret_scanning_enabled);
        assert_eq!(config.sync_interval_secs, 300);
    }

    // -- Integration test: round-trip sync ----------------------------------

    #[test]
    fn test_round_trip_sync() {
        let tmp = TempDir::new().unwrap();
        let local = tmp.path().join("local");
        let team = tmp.path().join("team");
        fs::create_dir_all(&local).unwrap();
        fs::create_dir_all(&team).unwrap();

        // Step 1: Write local memories
        fs::write(local.join("note1.md"), "Important note 1\n").unwrap();
        fs::write(local.join("note2.md"), "Important note 2\n").unwrap();

        // Step 2: Sync local -> team
        let config = TeamMemoryConfig {
            enabled: true,
            local_memory_dir: local.clone(),
            team_memory_dir: team.clone(),
            secret_scanning_enabled: true,
            ..Default::default()
        };
        let sync = TeamMemorySync::new(config).unwrap();
        let result = sync.sync().unwrap();
        assert_eq!(result.uploaded.len(), 2);
        assert!(result.is_clean());

        // Step 3: Verify team has both files
        assert!(team.join("note1.md").exists());
        assert!(team.join("note2.md").exists());

        // Step 4: Add a team memory
        fs::write(team.join("shared.md"), "Shared team memory\n").unwrap();

        // Step 5: Sync again (team -> local for the new file)
        let result2 = sync.sync().unwrap();
        assert!(
            result2
                .downloaded
                .iter()
                .any(|p| p.to_string_lossy() == "shared.md")
        );
        assert!(local.join("shared.md").exists());
    }

    #[test]
    fn test_sync_skips_hidden_dirs() {
        let tmp = TempDir::new().unwrap();
        let local = tmp.path().join("local");
        let team = tmp.path().join("team");
        fs::create_dir_all(local.join(".hidden")).unwrap();
        fs::create_dir_all(&team).unwrap();

        // Write file in hidden dir
        fs::write(local.join(".hidden/secret.md"), "hidden file\n").unwrap();

        let config = TeamMemoryConfig {
            enabled: true,
            local_memory_dir: local.clone(),
            team_memory_dir: team.clone(),
            secret_scanning_enabled: true,
            ..Default::default()
        };
        let sync = TeamMemorySync::new(config).unwrap();
        let result = sync.sync().unwrap();
        assert!(result.uploaded.is_empty());
    }
}
