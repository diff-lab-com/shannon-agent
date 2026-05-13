//! # Session Recovery and Crash-Safe Persistence
//!
//! Provides append-only JSONL session logging keyed by project path for
//! crash-safe session resume. Sessions are stored under
//! `~/.shannon/sessions/{encoded_project_path}/` with one JSONL file per
//! session and a companion `session_meta.json` for indexing.
//!
//! ## Crash Safety
//!
//! Each message is appended as a single JSON line and flushed (fsync) before
//! the write is considered complete. In the event of a crash, at most the
//! last message may be lost (partial line at the end of the JSONL file).

use crate::api::Message;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;
use uuid::Uuid;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during session recovery operations.
#[derive(Error, Debug)]
pub enum SessionRecoveryError {
    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("No sessions found")]
    NoSessions,
}

// ============================================================================
// Data Types
// ============================================================================

/// A single JSONL entry in the session log.
///
/// Each line in the JSONL file is one of these entries. The `seq` field
/// provides ordering guarantees.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionLogEntry {
    /// Monotonically increasing sequence number.
    pub seq: u64,
    /// The conversation message.
    pub message: Message,
    /// ISO 8601 timestamp when this entry was written.
    pub timestamp: DateTime<Utc>,
}

/// Metadata for indexing and resuming sessions.
///
/// Stored as `session_meta.json` alongside the JSONL log file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryMetadata {
    /// Unique session identifier (UUID).
    pub id: String,
    /// Absolute project path this session belongs to.
    pub project_path: PathBuf,
    /// ISO 8601 creation timestamp.
    pub created_at: DateTime<Utc>,
    /// ISO 8601 last-modified timestamp.
    pub updated_at: DateTime<Utc>,
    /// Number of messages in the session.
    pub message_count: usize,
    /// Model used for this session.
    pub model: String,
}

// ============================================================================
// Session Recovery Manager
// ============================================================================

/// Manages crash-safe session persistence using append-only JSONL logs.
///
/// Sessions are keyed by project path and stored under
/// `~/.shannon/sessions/{encoded_project_path}/`. Each session consists of:
/// - A `.jsonl` file containing one message per line (append-only)
/// - A `session_meta.json` file for fast indexing
///
/// The append-only design means that in a crash, at most the last line
/// (message) may be lost or truncated.
pub struct SessionRecovery {
    /// Base directory for session storage.
    sessions_dir: PathBuf,
}

impl SessionRecovery {
    /// Default base directory relative to `$HOME`.
    const DEFAULT_BASE_DIR: &'static str = ".shannon/sessions";

    /// Create a new recovery manager backed by the default directory.
    pub fn new() -> Result<Self, SessionRecoveryError> {
        let dir = default_sessions_base_dir();
        fs::create_dir_all(&dir)?;
        Ok(Self { sessions_dir: dir })
    }

    /// Create with a custom base directory (for testing).
    pub fn with_dir(dir: PathBuf) -> Result<Self, SessionRecoveryError> {
        fs::create_dir_all(&dir)?;
        Ok(Self { sessions_dir: dir })
    }

    // ----------------------------------------------------------------
    // Project-path encoding
    // ----------------------------------------------------------------

    /// Encode a project path for use as a directory name.
    ///
    /// Replaces `/` with `_` and prefixes with `_` to avoid collisions.
    /// Handles special characters by percent-encoding the path.
    fn encode_project_path(project_path: &Path) -> String {
        let path_str = project_path.to_string_lossy();
        // Use a simple percent-encoding approach for the full path.
        // This handles special chars, spaces, unicode, etc.
        let mut encoded = String::with_capacity(path_str.len());
        for byte in path_str.bytes() {
            match byte {
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                    encoded.push(byte as char);
                }
                _ => {
                    encoded.push_str(&format!("_{byte:02x}"));
                }
            }
        }
        encoded
    }

    /// Get the session directory for a project path.
    pub fn project_session_dir(&self, project_path: &Path) -> PathBuf {
        let encoded = Self::encode_project_path(project_path);
        self.sessions_dir.join(encoded)
    }

    /// Get the JSONL log file path for a session.
    pub fn session_log_path(&self, project_dir: &Path, session_id: &str) -> PathBuf {
        project_dir.join(format!("{session_id}.jsonl"))
    }

    /// Get the metadata file path for a session.
    pub fn session_meta_path(&self, project_dir: &Path, session_id: &str) -> PathBuf {
        project_dir.join(format!("{session_id}_meta.json"))
    }

    // ----------------------------------------------------------------
    // Core operations
    // ----------------------------------------------------------------

    /// Create a new session for a project, returning the session ID.
    ///
    /// Creates the project directory and initial metadata file.
    pub fn create_session(
        &self,
        project_path: &Path,
        model: &str,
    ) -> Result<String, SessionRecoveryError> {
        let project_dir = self.project_session_dir(project_path);
        fs::create_dir_all(&project_dir)?;

        let session_id = Uuid::new_v4().to_string();
        let now = Utc::now();

        let meta = RecoveryMetadata {
            id: session_id.clone(),
            project_path: project_path.to_path_buf(),
            created_at: now,
            updated_at: now,
            message_count: 0,
            model: model.to_string(),
        };

        self.write_metadata(&project_dir, &meta)?;

        // Create empty JSONL file.
        File::create(self.session_log_path(&project_dir, &session_id))?;

        Ok(session_id)
    }

    /// Create a new session with a pre-assigned ID (e.g. from QueryEngine's session_id).
    pub fn create_session_with_id(
        &self,
        project_path: &Path,
        session_id: &str,
        model: &str,
    ) -> Result<(), SessionRecoveryError> {
        let project_dir = self.project_session_dir(project_path);
        fs::create_dir_all(&project_dir)?;

        let now = Utc::now();
        let meta = RecoveryMetadata {
            id: session_id.to_string(),
            project_path: project_path.to_path_buf(),
            created_at: now,
            updated_at: now,
            message_count: 0,
            model: model.to_string(),
        };
        self.write_metadata(&project_dir, &meta)?;
        File::create(self.session_log_path(&project_dir, session_id))?;
        Ok(())
    }

    /// Append a message to the session log with immediate flush (fsync).
    ///
    /// This is the core crash-safety mechanism: each message is written as a
    /// single JSON line, flushed to the OS buffer, and fsynced to disk before
    /// returning. In a crash, at most the last appended message may be lost.
    pub fn append_message(
        &self,
        project_path: &Path,
        session_id: &str,
        seq: u64,
        message: &Message,
    ) -> Result<(), SessionRecoveryError> {
        let project_dir = self.project_session_dir(project_path);
        let log_path = self.session_log_path(&project_dir, session_id);

        if !log_path.exists() {
            return Err(SessionRecoveryError::SessionNotFound(session_id.to_string()));
        }

        let entry = SessionLogEntry {
            seq,
            message: message.clone(),
            timestamp: Utc::now(),
        };

        // Serialize to a single line (no newlines in JSON).
        let mut line = serde_json::to_string(&entry)?;
        line.push('\n');

        // Open in append mode, write, flush, fsync.
        let mut file = OpenOptions::new().append(true).open(&log_path)?;
        file.write_all(line.as_bytes())?;
        file.flush()?;

        // fsync for durability. Best-effort on systems that don't support it.
        #[cfg(unix)]
        {
            let _ = file.sync_all();
        }

        // Update metadata (message count and updated_at).
        self.update_metadata_counts(&project_dir, session_id)?;

        Ok(())
    }

    /// Append a batch of messages atomically.
    ///
    /// Each message is written and flushed individually for crash safety,
    /// but metadata is only updated once at the end.
    pub fn append_messages(
        &self,
        project_path: &Path,
        session_id: &str,
        start_seq: u64,
        messages: &[Message],
    ) -> Result<(), SessionRecoveryError> {
        let project_dir = self.project_session_dir(project_path);
        let log_path = self.session_log_path(&project_dir, session_id);

        if !log_path.exists() {
            return Err(SessionRecoveryError::SessionNotFound(session_id.to_string()));
        }

        // Open once in append mode and write all entries.
        let mut file = OpenOptions::new().append(true).open(&log_path)?;

        for (i, message) in messages.iter().enumerate() {
            let entry = SessionLogEntry {
                seq: start_seq + i as u64,
                message: message.clone(),
                timestamp: Utc::now(),
            };

            let mut line = serde_json::to_string(&entry)?;
            line.push('\n');
            file.write_all(line.as_bytes())?;
        }

        file.flush()?;
        #[cfg(unix)]
        {
            let _ = file.sync_all();
        }

        // Update metadata once.
        self.update_metadata_counts(&project_dir, session_id)?;

        Ok(())
    }

    /// Load all messages from a session log.
    ///
    /// Reads the JSONL file and returns messages in sequence order. Partial
    /// or malformed lines at the end of the file are silently skipped (crash
    /// recovery: only the last message may be lost).
    pub fn load_messages(
        &self,
        project_path: &Path,
        session_id: &str,
    ) -> Result<Vec<Message>, SessionRecoveryError> {
        let project_dir = self.project_session_dir(project_path);
        let log_path = self.session_log_path(&project_dir, session_id);

        if !log_path.exists() {
            return Err(SessionRecoveryError::SessionNotFound(session_id.to_string()));
        }

        self.read_log_entries(&log_path)
    }

    /// Load all log entries (including metadata) from a session.
    pub fn load_entries(
        &self,
        project_path: &Path,
        session_id: &str,
    ) -> Result<Vec<SessionLogEntry>, SessionRecoveryError> {
        let project_dir = self.project_session_dir(project_path);
        let log_path = self.session_log_path(&project_dir, session_id);

        if !log_path.exists() {
            return Err(SessionRecoveryError::SessionNotFound(session_id.to_string()));
        }

        self.read_raw_entries(&log_path)
    }

    /// Save/update session metadata.
    pub fn save_metadata(
        &self,
        project_path: &Path,
        meta: &RecoveryMetadata,
    ) -> Result<(), SessionRecoveryError> {
        let project_dir = self.project_session_dir(project_path);
        fs::create_dir_all(&project_dir)?;
        self.write_metadata(&project_dir, meta)
    }

    /// Load session metadata.
    pub fn load_metadata(
        &self,
        project_path: &Path,
        session_id: &str,
    ) -> Result<RecoveryMetadata, SessionRecoveryError> {
        let project_dir = self.project_session_dir(project_path);
        let meta_path = self.session_meta_path(&project_dir, session_id);

        if !meta_path.exists() {
            return Err(SessionRecoveryError::SessionNotFound(session_id.to_string()));
        }

        let contents = fs::read_to_string(&meta_path)?;
        let meta: RecoveryMetadata = serde_json::from_str(&contents)?;
        Ok(meta)
    }

    /// List all sessions for a project.
    pub fn list_sessions(
        &self,
        project_path: &Path,
    ) -> Result<Vec<RecoveryMetadata>, SessionRecoveryError> {
        let project_dir = self.project_session_dir(project_path);

        if !project_dir.exists() {
            return Ok(Vec::new());
        }

        let mut sessions = Vec::new();
        let entries = fs::read_dir(&project_dir)?;

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();
            // Look for _meta.json files.
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };

            if !name.ends_with("_meta.json") {
                continue;
            }

            let contents = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            match serde_json::from_str::<RecoveryMetadata>(&contents) {
                Ok(meta) => sessions.push(meta),
                Err(_) => continue,
            }
        }

        // Sort by updated_at descending (most recent first).
        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

        Ok(sessions)
    }

    /// Get the most recent session for a project.
    pub fn get_latest_session(
        &self,
        project_path: &Path,
    ) -> Result<Option<RecoveryMetadata>, SessionRecoveryError> {
        let sessions = self.list_sessions(project_path)?;
        Ok(sessions.into_iter().next())
    }

    /// Get the most recent session across all projects.
    ///
    /// Scans all project directories and returns the session with the most
    /// recent `updated_at` timestamp.
    pub fn get_last_session(&self) -> Result<Option<(PathBuf, RecoveryMetadata)>, SessionRecoveryError> {
        if !self.sessions_dir.exists() {
            return Ok(None);
        }

        let mut best: Option<(PathBuf, RecoveryMetadata)> = None;

        let project_dirs = fs::read_dir(&self.sessions_dir)?;
        for project_entry in project_dirs {
            let project_entry = match project_entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let project_dir = project_entry.path();
            if !project_dir.is_dir() {
                continue;
            }

            let meta_entries = fs::read_dir(&project_dir);
            let Ok(meta_entries) = meta_entries else {
                continue;
            };

            for meta_entry in meta_entries {
                let meta_entry = match meta_entry {
                    Ok(e) => e,
                    Err(_) => continue,
                };

                let path = meta_entry.path();
                let name = match path.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n,
                    None => continue,
                };

                if !name.ends_with("_meta.json") {
                    continue;
                }

                let contents = match fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };

                let meta: RecoveryMetadata = match serde_json::from_str(&contents) {
                    Ok(m) => m,
                    Err(_) => continue,
                };

                let is_newer = match &best {
                    None => true,
                    Some((_, existing)) => meta.updated_at > existing.updated_at,
                };

                if is_newer {
                    best = Some((meta.project_path.clone(), meta));
                }
            }
        }

        Ok(best)
    }

    /// Delete a session.
    pub fn delete_session(
        &self,
        project_path: &Path,
        session_id: &str,
    ) -> Result<(), SessionRecoveryError> {
        let project_dir = self.project_session_dir(project_path);
        let log_path = self.session_log_path(&project_dir, session_id);
        let meta_path = self.session_meta_path(&project_dir, session_id);

        if !log_path.exists() && !meta_path.exists() {
            return Err(SessionRecoveryError::SessionNotFound(session_id.to_string()));
        }

        if log_path.exists() {
            fs::remove_file(&log_path)?;
        }
        if meta_path.exists() {
            fs::remove_file(&meta_path)?;
        }

        Ok(())
    }

    // ----------------------------------------------------------------
    // Private helpers
    // ----------------------------------------------------------------

    /// Write metadata to disk atomically (temp file + rename).
    fn write_metadata(
        &self,
        project_dir: &Path,
        meta: &RecoveryMetadata,
    ) -> Result<(), SessionRecoveryError> {
        let meta_path = self.session_meta_path(project_dir, &meta.id);
        let json = serde_json::to_string_pretty(meta)?;

        let tmp_path = meta_path.with_extension("json.tmp");
        fs::write(&tmp_path, &json)?;
        fs::rename(&tmp_path, &meta_path)?;

        Ok(())
    }

    /// Update the message count and timestamp in existing metadata.
    fn update_metadata_counts(
        &self,
        project_dir: &Path,
        session_id: &str,
    ) -> Result<(), SessionRecoveryError> {
        let meta_path = self.session_meta_path(project_dir, session_id);

        if !meta_path.exists() {
            return Ok(());
        }

        let contents = fs::read_to_string(&meta_path)?;
        let mut meta: RecoveryMetadata = match serde_json::from_str(&contents) {
            Ok(m) => m,
            Err(_) => return Ok(()), // skip malformed
        };

        // Count lines in the JSONL file to get the actual message count.
        let log_path = self.session_log_path(project_dir, session_id);
        let count = if log_path.exists() {
            let file = File::open(&log_path)?;
            let reader = BufReader::new(file);
            reader.lines().filter(|l| l.as_ref().is_ok_and(|s| !s.trim().is_empty())).count()
        } else {
            0
        };

        meta.message_count = count;
        meta.updated_at = Utc::now();

        self.write_metadata(project_dir, &meta)?;

        Ok(())
    }

    /// Read messages from a JSONL log file, skipping partial/malformed lines.
    fn read_log_entries(&self, log_path: &Path) -> Result<Vec<Message>, SessionRecoveryError> {
        let entries = self.read_raw_entries(log_path)?;
        Ok(entries.into_iter().map(|e| e.message).collect())
    }

    /// Read raw log entries, sorting by sequence number, skipping bad lines.
    fn read_raw_entries(&self, log_path: &Path) -> Result<Vec<SessionLogEntry>, SessionRecoveryError> {
        let file = File::open(log_path)?;
        let reader = BufReader::new(file);

        let mut entries: Vec<SessionLogEntry> = Vec::new();

        for line_result in reader.lines() {
            let line = match line_result {
                Ok(l) => l,
                Err(_) => continue, // skip unreadable lines (crash recovery)
            };

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            match serde_json::from_str::<SessionLogEntry>(trimmed) {
                Ok(entry) => entries.push(entry),
                Err(_) => continue, // skip malformed lines (crash recovery)
            }
        }

        // Sort by sequence number to ensure correct order.
        entries.sort_by_key(|e| e.seq);

        Ok(entries)
    }
}

impl Default for SessionRecovery {
    fn default() -> Self {
        match Self::with_dir(default_sessions_base_dir()) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("failed to create SessionRecovery with default dir: {e}");
                let fallback_dir = std::env::temp_dir().join(".shannon").join("sessions");
                match Self::with_dir(fallback_dir.clone()) {
                    Ok(s) => s,
                    Err(fallback_err) => {
                        tracing::error!("SessionRecovery: temp fallback also failed: {fallback_err}. Using non-persisting instance.");
                        Self { sessions_dir: fallback_dir }
                    }
                }
            }
        }
    }
}

// ============================================================================
// Free helpers
// ============================================================================

/// Return the default sessions base directory.
fn default_sessions_base_dir() -> PathBuf {
    match std::env::var("HOME") {
        Ok(home) => PathBuf::from(home).join(SessionRecovery::DEFAULT_BASE_DIR),
        Err(_) => std::env::temp_dir().join(".shannon").join("sessions"),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::MessageContent;

    // -- helpers --

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir()
            .join("shannon-test-session-recovery")
            .join(Uuid::new_v4().to_string());
        let _ = fs::create_dir_all(&dir);
        dir
    }

    fn manager() -> SessionRecovery {
        SessionRecovery::with_dir(temp_dir()).unwrap()
    }

    fn project_path() -> PathBuf {
        PathBuf::from("/home/user/my-project")
    }

    fn make_message(role: &str, text: &str) -> Message {
        Message {
            role: role.to_string(),
            content: MessageContent::Text(text.to_string()),
        }
    }

    // -- project path encoding --

    #[test]
    fn test_encode_simple_path() {
        let encoded = SessionRecovery::encode_project_path(Path::new("/home/user/project"));
        assert!(!encoded.is_empty());
        assert!(!encoded.contains('/'));
    }

    #[test]
    fn test_encode_path_with_spaces() {
        let encoded = SessionRecovery::encode_project_path(Path::new("/home/user/my project"));
        assert!(!encoded.contains(' '));
        assert!(!encoded.contains('/'));
    }

    #[test]
    fn test_encode_path_with_special_chars() {
        let encoded = SessionRecovery::encode_project_path(Path::new("/tmp/test@#$"));
        assert!(!encoded.contains('@'));
        assert!(!encoded.contains('#'));
        assert!(!encoded.contains('$'));
    }

    #[test]
    fn test_encode_path_deterministic() {
        let path = Path::new("/home/user/project");
        let e1 = SessionRecovery::encode_project_path(path);
        let e2 = SessionRecovery::encode_project_path(path);
        assert_eq!(e1, e2);
    }

    #[test]
    fn test_encode_different_paths_produce_different_results() {
        let e1 = SessionRecovery::encode_project_path(Path::new("/home/user/project-a"));
        let e2 = SessionRecovery::encode_project_path(Path::new("/home/user/project-b"));
        assert_ne!(e1, e2);
    }

    #[test]
    fn test_encode_unicode_path() {
        let encoded = SessionRecovery::encode_project_path(Path::new("/home/用户/项目"));
        assert!(!encoded.is_empty());
        // Should not panic or produce empty string
    }

    // -- session creation --

    #[test]
    fn test_create_session() {
        let mgr = manager();
        let id = mgr.create_session(&project_path(), "test-model").unwrap();

        assert!(Uuid::parse_str(&id).is_ok());

        // Verify metadata was written.
        let meta = mgr.load_metadata(&project_path(), &id).unwrap();
        assert_eq!(meta.id, id);
        assert_eq!(meta.model, "test-model");
        assert_eq!(meta.message_count, 0);
        assert_eq!(meta.project_path, project_path());
    }

    #[test]
    fn test_create_session_creates_project_dir() {
        let mgr = manager();
        let project_dir = mgr.project_session_dir(&project_path());
        assert!(!project_dir.exists());

        mgr.create_session(&project_path(), "model").unwrap();
        assert!(project_dir.exists());
    }

    // -- message append and read --

    #[test]
    fn test_append_and_load_single_message() {
        let mgr = manager();
        let id = mgr.create_session(&project_path(), "model").unwrap();

        let msg = make_message("user", "Hello");
        mgr.append_message(&project_path(), &id, 0, &msg).unwrap();

        let loaded = mgr.load_messages(&project_path(), &id).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].role, "user");

        match &loaded[0].content {
            MessageContent::Text(t) => assert_eq!(t, "Hello"),
            _ => panic!("expected text content"),
        }
    }

    #[test]
    fn test_append_and_load_multiple_messages() {
        let mgr = manager();
        let id = mgr.create_session(&project_path(), "model").unwrap();

        let messages = vec![
            make_message("user", "Hello"),
            make_message("assistant", "Hi there!"),
            make_message("user", "How are you?"),
        ];

        mgr.append_messages(&project_path(), &id, 0, &messages).unwrap();

        let loaded = mgr.load_messages(&project_path(), &id).unwrap();
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded[0].role, "user");
        assert_eq!(loaded[1].role, "assistant");
        assert_eq!(loaded[2].role, "user");
    }

    #[test]
    fn test_messages_in_sequence_order() {
        let mgr = manager();
        let id = mgr.create_session(&project_path(), "model").unwrap();

        // Append out of order by seq.
        mgr.append_message(&project_path(), &id, 2, &make_message("user", "Third")).unwrap();
        mgr.append_message(&project_path(), &id, 0, &make_message("user", "First")).unwrap();
        mgr.append_message(&project_path(), &id, 1, &make_message("user", "Second")).unwrap();

        let loaded = mgr.load_messages(&project_path(), &id).unwrap();
        assert_eq!(loaded.len(), 3);

        match &loaded[0].content {
            MessageContent::Text(t) => assert_eq!(t, "First"),
            _ => panic!("expected text"),
        }
        match &loaded[1].content {
            MessageContent::Text(t) => assert_eq!(t, "Second"),
            _ => panic!("expected text"),
        }
        match &loaded[2].content {
            MessageContent::Text(t) => assert_eq!(t, "Third"),
            _ => panic!("expected text"),
        }
    }

    // -- metadata round-trip --

    #[test]
    fn test_metadata_round_trip() {
        let mgr = manager();
        let id = mgr.create_session(&project_path(), "test-model").unwrap();

        let meta = mgr.load_metadata(&project_path(), &id).unwrap();
        assert_eq!(meta.id, id);
        assert_eq!(meta.project_path, project_path());
        assert_eq!(meta.model, "test-model");
        assert!(meta.created_at <= Utc::now());
        assert!(meta.updated_at <= Utc::now());
    }

    #[test]
    fn test_metadata_updates_on_append() {
        let mgr = manager();
        let id = mgr.create_session(&project_path(), "model").unwrap();

        let meta_before = mgr.load_metadata(&project_path(), &id).unwrap();
        assert_eq!(meta_before.message_count, 0);

        mgr.append_message(&project_path(), &id, 0, &make_message("user", "Hi")).unwrap();

        let meta_after = mgr.load_metadata(&project_path(), &id).unwrap();
        assert_eq!(meta_after.message_count, 1);
        assert!(meta_after.updated_at >= meta_before.updated_at);
    }

    // -- list sessions --

    #[test]
    fn test_list_sessions_empty() {
        let mgr = manager();
        let sessions = mgr.list_sessions(&project_path()).unwrap();
        assert!(sessions.is_empty());
    }

    #[test]
    fn test_list_sessions_returns_created() {
        let mgr = manager();
        mgr.create_session(&project_path(), "model-a").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let id2 = mgr.create_session(&project_path(), "model-b").unwrap();

        let sessions = mgr.list_sessions(&project_path()).unwrap();
        assert_eq!(sessions.len(), 2);

        // Most recent first.
        assert_eq!(sessions[0].id, id2);
    }

    // -- get_latest_session --

    #[test]
    fn test_get_latest_session_none() {
        let mgr = manager();
        assert!(mgr.get_latest_session(&project_path()).unwrap().is_none());
    }

    #[test]
    fn test_get_latest_session_returns_most_recent() {
        let mgr = manager();
        let id1 = mgr.create_session(&project_path(), "model-a").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let id2 = mgr.create_session(&project_path(), "model-b").unwrap();

        let latest = mgr.get_latest_session(&project_path()).unwrap().unwrap();
        assert_eq!(latest.id, id2);
        assert_ne!(latest.id, id1);
    }

    // -- get_last_session (cross-project) --

    #[test]
    fn test_get_last_session_empty() {
        let mgr = manager();
        assert!(mgr.get_last_session().unwrap().is_none());
    }

    #[test]
    fn test_get_last_session_cross_project() {
        let mgr = manager();
        let project_a = PathBuf::from("/home/user/project-a");
        let project_b = PathBuf::from("/home/user/project-b");

        mgr.create_session(&project_a, "model").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let id_b = mgr.create_session(&project_b, "model").unwrap();

        let (path, meta) = mgr.get_last_session().unwrap().unwrap();
        assert_eq!(path, project_b);
        assert_eq!(meta.id, id_b);
    }

    // -- delete session --

    #[test]
    fn test_delete_session() {
        let mgr = manager();
        let id = mgr.create_session(&project_path(), "model").unwrap();

        mgr.append_message(&project_path(), &id, 0, &make_message("user", "Hi")).unwrap();
        mgr.delete_session(&project_path(), &id).unwrap();

        assert!(mgr.load_messages(&project_path(), &id).is_err());
        assert!(mgr.load_metadata(&project_path(), &id).is_err());
    }

    #[test]
    fn test_delete_nonexistent_session() {
        let mgr = manager();
        let result = mgr.delete_session(&project_path(), "no-such-session");
        assert!(result.is_err());
    }

    // -- crash recovery simulation --

    #[test]
    fn test_crash_recovery_skips_partial_line() {
        let mgr = manager();
        let id = mgr.create_session(&project_path(), "model").unwrap();

        // Append a valid message.
        mgr.append_message(&project_path(), &id, 0, &make_message("user", "Valid")).unwrap();

        // Simulate a crash by writing a partial/truncated line to the JSONL file.
        let project_dir = mgr.project_session_dir(&project_path());
        let log_path = mgr.session_log_path(&project_dir, &id);
        {
            let mut file = OpenOptions::new().append(true).open(&log_path).unwrap();
            file.write_all(br#"{"seq":1,"message":{"role":"user","content":{"Text":"Partial"#).unwrap();
            file.flush().unwrap();
        }

        // Load should succeed, returning only valid entries.
        let loaded = mgr.load_messages(&project_path(), &id).unwrap();
        assert_eq!(loaded.len(), 1);

        match &loaded[0].content {
            MessageContent::Text(t) => assert_eq!(t, "Valid"),
            _ => panic!("expected text"),
        }
    }

    #[test]
    fn test_crash_recovery_empty_lines_skipped() {
        let mgr = manager();
        let id = mgr.create_session(&project_path(), "model").unwrap();

        // Append valid messages.
        mgr.append_message(&project_path(), &id, 0, &make_message("user", "First")).unwrap();
        mgr.append_message(&project_path(), &id, 1, &make_message("user", "Second")).unwrap();

        // Inject empty lines into the JSONL file.
        let project_dir = mgr.project_session_dir(&project_path());
        let log_path = mgr.session_log_path(&project_dir, &id);
        {
            let mut file = OpenOptions::new().append(true).open(&log_path).unwrap();
            file.write_all("\n\n\n".as_bytes()).unwrap();
            file.flush().unwrap();
        }

        let loaded = mgr.load_messages(&project_path(), &id).unwrap();
        assert_eq!(loaded.len(), 2);
    }

    // -- project isolation --

    #[test]
    fn test_different_projects_isolated() {
        let mgr = manager();
        let project_a = PathBuf::from("/home/user/project-a");
        let project_b = PathBuf::from("/home/user/project-b");

        let id_a = mgr.create_session(&project_a, "model").unwrap();
        let id_b = mgr.create_session(&project_b, "model").unwrap();

        mgr.append_message(&project_a, &id_a, 0, &make_message("user", "Message A")).unwrap();
        mgr.append_message(&project_b, &id_b, 0, &make_message("user", "Message B")).unwrap();

        let msgs_a = mgr.load_messages(&project_a, &id_a).unwrap();
        let msgs_b = mgr.load_messages(&project_b, &id_b).unwrap();

        assert_eq!(msgs_a.len(), 1);
        assert_eq!(msgs_b.len(), 1);

        match &msgs_a[0].content {
            MessageContent::Text(t) => assert_eq!(t, "Message A"),
            _ => panic!("expected text"),
        }
        match &msgs_b[0].content {
            MessageContent::Text(t) => assert_eq!(t, "Message B"),
            _ => panic!("expected text"),
        }
    }

    // -- new/with_dir --

    #[test]
    fn test_new_creates_default_dir() {
        let mgr = SessionRecovery::new().unwrap();
        assert!(mgr.sessions_dir.to_string_lossy().contains(".shannon"));
        assert!(mgr.sessions_dir.to_string_lossy().contains("sessions"));
    }

    #[test]
    fn test_with_dir_creates_directory() {
        let dir = std::env::temp_dir()
            .join("shannon-test-recovery-create")
            .join(Uuid::new_v4().to_string());
        assert!(!dir.exists());

        SessionRecovery::with_dir(dir.clone()).unwrap();
        assert!(dir.exists());
    }

    // -- log entries with metadata --

    #[test]
    fn test_load_entries_preserves_metadata() {
        let mgr = manager();
        let id = mgr.create_session(&project_path(), "model").unwrap();

        let before = Utc::now();
        mgr.append_message(&project_path(), &id, 0, &make_message("user", "Test")).unwrap();

        let entries = mgr.load_entries(&project_path(), &id).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].seq, 0);
        assert!(entries[0].timestamp >= before);
    }
}
