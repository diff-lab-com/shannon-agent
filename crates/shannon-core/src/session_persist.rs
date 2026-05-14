//! # Multi-Session Persistence
//!
//! Provides session state persistence for save/restore, auto-save, and
//! session resume. Each session is stored as a single JSON file under a
//! configurable storage directory (defaulting to `~/.shannon/sessions/`).

use crate::api::Message;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use thiserror::Error;
use uuid::Uuid;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during session persistence operations.
#[derive(Error, Debug)]
pub enum SessionPersistError {
    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Storage error: {0}")]
    StorageError(String),
}

// ============================================================================
// Data Types
// ============================================================================

/// Persistent session state for save/restore.
///
/// Contains the full conversation history along with metadata that
/// describes the session context. Serialized as a single human-readable
/// JSON file per session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSessionState {
    /// Unique session identifier (UUID string).
    pub session_id: String,
    /// ISO 8601 creation timestamp.
    pub created_at: DateTime<Utc>,
    /// ISO 8601 last-modified timestamp.
    pub updated_at: DateTime<Utc>,
    /// Ordered conversation history.
    pub messages: Vec<Message>,
    /// Last used model name.
    pub model: String,
    /// Working directory at the time of session creation.
    pub working_dir: PathBuf,
    /// Additional session metadata.
    pub metadata: PersistMetadata,
}

/// Additional metadata attached to a persisted session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub struct PersistMetadata {
    /// User-given name for the session.
    pub name: Option<String>,
    /// Number of tool invocations in this session.
    pub tool_use_count: u32,
    /// Total tokens consumed (input + output).
    pub total_tokens: u64,
    /// User-assigned tags for categorisation.
    pub tags: Vec<String>,
}


/// Lightweight summary used when listing sessions without loading full
/// conversation history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub message_count: usize,
    pub model: String,
    pub working_dir: PathBuf,
    pub total_tokens: u64,
}

/// Current session file format version. Increment when the schema changes.
const SESSION_FORMAT_VERSION: u32 = 1;

/// On-disk envelope: wraps the session state together with a content hash
/// used for dirty-checking during auto-save.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionFile {
    /// Format version for forward/backward compatibility.
    /// Old files without this field deserialize as 0.
    #[serde(default)]
    version: u32,
    /// The session payload.
    session: PersistedSessionState,
    /// SHA-256-sized hex hash of the serialised session at save time, used to
    /// detect whether the session has changed since the last save.
    content_hash: String,
}

// ============================================================================
// Session Persistence Manager
// ============================================================================

/// Manages session persistence to disk.
///
/// Stores one JSON file per session in the configured `storage_dir`.
/// The default storage location is `~/.shannon/sessions/`.
pub struct SessionPersistManager {
    /// Directory where session JSON files are stored.
    storage_dir: PathBuf,
}

impl SessionPersistManager {
    /// Default storage directory relative to `$HOME`.
    const DEFAULT_DIR: &'static str = ".shannon/sessions";

    /// Create a new manager backed by the default sessions directory.
    ///
    /// The directory is created if it does not exist.
    pub fn new() -> Result<Self, SessionPersistError> {
        let dir = default_storage_dir();
        fs::create_dir_all(&dir)?;
        Ok(Self { storage_dir: dir })
    }

    /// Create a manager backed by a custom storage directory.
    ///
    /// The directory is created if it does not exist.
    pub fn with_dir(storage_dir: PathBuf) -> Result<Self, SessionPersistError> {
        fs::create_dir_all(&storage_dir)?;
        Ok(Self { storage_dir })
    }

    // ----------------------------------------------------------------
    // Core CRUD
    // ----------------------------------------------------------------

    /// Save a session to disk atomically (write temp, then rename).
    pub fn save_session(&self, state: &PersistedSessionState) -> Result<(), SessionPersistError> {
        fs::create_dir_all(&self.storage_dir)?;

        let content_hash = compute_hash(state);
        let file = SessionFile {
            version: SESSION_FORMAT_VERSION,
            session: state.clone(),
            content_hash,
        };

        let json = serde_json::to_string_pretty(&file)?;

        // Atomic write: temp file then rename.
        let path = self.session_path(&state.session_id);
        let tmp_path = path.with_extension("json.tmp");
        fs::write(&tmp_path, json)?;
        fs::rename(&tmp_path, &path)?;

        Ok(())
    }

    /// Load a session from disk by its ID.
    pub fn load_session(&self, id: &str) -> Result<PersistedSessionState, SessionPersistError> {
        let path = self.session_path(id);
        if !path.exists() {
            return Err(SessionPersistError::SessionNotFound(id.to_string()));
        }

        let contents = fs::read_to_string(&path)?;
        let mut file: SessionFile = serde_json::from_str(&contents)?;

        // Migrate from older formats
        if file.version < SESSION_FORMAT_VERSION {
            migrate_session(&mut file);
        }

        Ok(file.session)
    }

    /// List all saved sessions as lightweight summaries.
    ///
    /// Sessions whose files cannot be parsed are silently skipped.
    pub fn list_sessions(&self) -> Result<Vec<SessionSummary>, SessionPersistError> {
        if !self.storage_dir.exists() {
            return Ok(Vec::new());
        }

        let mut summaries = Vec::new();

        let entries = fs::read_dir(&self.storage_dir)?;
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }

            let contents = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let file: SessionFile = match serde_json::from_str(&contents) {
                Ok(f) => f,
                Err(_) => continue,
            };

            let s = &file.session;
            summaries.push(SessionSummary {
                session_id: s.session_id.clone(),
                name: s.metadata.name.clone(),
                created_at: s.created_at,
                updated_at: s.updated_at,
                message_count: s.messages.len(),
                model: s.model.clone(),
                working_dir: s.working_dir.clone(),
                total_tokens: s.metadata.total_tokens,
            });
        }

        // Sort by most-recently-updated first.
        summaries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

        Ok(summaries)
    }

    /// Delete a session file from disk.
    pub fn delete_session(&self, id: &str) -> Result<(), SessionPersistError> {
        let path = self.session_path(id);
        if !path.exists() {
            return Err(SessionPersistError::SessionNotFound(id.to_string()));
        }
        fs::remove_file(&path)?;
        Ok(())
    }

    /// Return the file path for a given session ID.
    pub fn session_path(&self, id: &str) -> PathBuf {
        self.storage_dir.join(format!("{id}.json"))
    }

    // ----------------------------------------------------------------
    // Auto-Save
    // ----------------------------------------------------------------

    /// Save the session only if it has changed since the last save.
    ///
    /// Returns `Ok(true)` when a save was performed, `Ok(false)` when the
    /// session was unchanged.
    pub fn auto_save(&self, state: &PersistedSessionState) -> Result<bool, SessionPersistError> {
        let path = self.session_path(&state.session_id);

        if path.exists() {
            let contents = fs::read_to_string(&path)?;
            if let Ok(file) = serde_json::from_str::<SessionFile>(&contents) {
                let current_hash = compute_hash(state);
                if file.content_hash == current_hash {
                    return Ok(false);
                }
            }
        }

        self.save_session(state)?;
        Ok(true)
    }

    // ----------------------------------------------------------------
    // Session Resume
    // ----------------------------------------------------------------

    /// Determine whether the most recent session should be resumed.
    ///
    /// Returns `Some(session_id)` when a session exists that was last updated
    /// within `timeout` of the current time. Returns `None` otherwise.
    pub fn should_resume_last(
        &self,
        timeout: chrono::Duration,
    ) -> Option<String> {
        let summaries = self.list_sessions().ok()?;

        let latest = summaries.first()?;
        let now = Utc::now();
        let age = now - latest.updated_at;

        if age <= timeout {
            Some(latest.session_id.clone())
        } else {
            None
        }
    }

    /// Return the current (or generate a new) session ID.
    ///
    /// This always generates a fresh UUID. Callers that wish to continue an
    /// existing session should use [`Self::load_session`] with the previous ID.
    pub fn generate_session_id() -> String {
        Uuid::new_v4().to_string()
    }
}

impl Default for SessionPersistManager {
    fn default() -> Self {
        match Self::with_dir(default_storage_dir()) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("failed to create SessionPersistManager with default dir: {e}");
                let fallback_dir = std::env::temp_dir().join(".shannon").join("sessions");
                match Self::with_dir(fallback_dir.clone()) {
                    Ok(s) => s,
                    Err(fallback_err) => {
                        tracing::error!("SessionPersistManager: temp fallback also failed: {fallback_err}. Using non-persisting instance.");
                        Self { storage_dir: fallback_dir }
                    }
                }
            }
        }
    }
}

// ============================================================================
// Private helpers
// ============================================================================

/// Return the default storage directory (`$HOME/.shannon/sessions`).
///
/// Falls back to `/tmp/.shannon/sessions` when `$HOME` is not set.
fn default_storage_dir() -> PathBuf {
    match std::env::var("HOME") {
        Ok(home) => PathBuf::from(home).join(SessionPersistManager::DEFAULT_DIR),
        Err(_) => std::env::temp_dir().join(".shannon").join("sessions"),
    }
}

/// Compute a fast, stable hash of the session state for dirty-checking.
///
/// Uses a simple XOR-fold of the bytes in the canonical JSON representation.
/// This is not cryptographically secure but is sufficient for detecting
/// whether the session data has changed between saves.
/// Apply format migrations to bring an older session up to the current version.
/// Add version-specific migration steps as the format evolves.
fn migrate_session(file: &mut SessionFile) {
    // Version 0 → 1: initial versioned format, no structural changes needed.
    // Future migrations go here, e.g.:
    // if file.version < 2 { migrate_v1_to_v2(file); }
    file.version = SESSION_FORMAT_VERSION;
}

fn compute_hash(state: &PersistedSessionState) -> String {
    let json = serde_json::to_string(state).unwrap_or_default();

    // Use SHA-256 for collision-resistant content hashing.
    // Previously used FNV-1a 64-bit which risks collisions at scale.
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(json.as_bytes());
    let result = hasher.finalize();
    hex::encode(result)
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
            .join("shannon-test-session-persist")
            .join(Uuid::new_v4().to_string());
        let _ = fs::create_dir_all(&dir);
        dir
    }

    fn manager() -> SessionPersistManager {
        SessionPersistManager::with_dir(temp_dir()).unwrap()
    }

    fn make_messages() -> Vec<Message> {
        vec![
            Message {
                role: "user".into(),
                content: MessageContent::Text("Hello, Claude!".into()),
            },
            Message {
                role: "assistant".into(),
                content: MessageContent::Text("Hello! How can I help you today?".into()),
            },
            Message {
                role: "user".into(),
                content: MessageContent::Text("Tell me about Rust.".into()),
            },
        ]
    }

    fn make_state(id: &str) -> PersistedSessionState {
        let now = Utc::now();
        PersistedSessionState {
            session_id: id.to_string(),
            created_at: now,
            updated_at: now,
            messages: make_messages(),
            model: "claude-3-5-sonnet-20241022".to_string(),
            working_dir: PathBuf::from("/tmp/project"),
            metadata: PersistMetadata {
                name: Some("Test session".to_string()),
                tool_use_count: 5,
                total_tokens: 1024,
                tags: vec!["test".to_string()],
            },
        }
    }

    // -- save and load round-trip --

    #[test]
    fn test_save_and_load_round_trip() {
        let mgr = manager();
        let id = &Uuid::new_v4().to_string();
        let state = make_state(id);

        mgr.save_session(&state).unwrap();

        let loaded = mgr.load_session(id).unwrap();
        assert_eq!(loaded.session_id, state.session_id);
        assert_eq!(loaded.messages.len(), state.messages.len());
        assert_eq!(loaded.model, state.model);
        assert_eq!(loaded.working_dir, state.working_dir);
        assert_eq!(loaded.metadata.name, state.metadata.name);
        assert_eq!(loaded.metadata.tool_use_count, state.metadata.tool_use_count);
        assert_eq!(loaded.metadata.total_tokens, state.metadata.total_tokens);
        assert_eq!(loaded.metadata.tags, state.metadata.tags);
    }

    #[test]
    fn test_load_nonexistent_session() {
        let mgr = manager();
        let result = mgr.load_session("does-not-exist");
        assert!(result.is_err());
        match result.unwrap_err() {
            SessionPersistError::SessionNotFound(id) => {
                assert_eq!(id, "does-not-exist");
            }
            other => panic!("expected SessionNotFound, got {other}"),
        }
    }

    // -- list_sessions --

    #[test]
    fn test_list_sessions_returns_all_saved() {
        let mgr = manager();
        let id1 = &Uuid::new_v4().to_string();
        let id2 = &Uuid::new_v4().to_string();

        mgr.save_session(&make_state(id1)).unwrap();
        // Small sleep to ensure different updated_at ordering.
        std::thread::sleep(std::time::Duration::from_millis(10));
        mgr.save_session(&make_state(id2)).unwrap();

        let list = mgr.list_sessions().unwrap();
        assert_eq!(list.len(), 2);

        let ids: Vec<&str> = list.iter().map(|s| s.session_id.as_str()).collect();
        assert!(ids.contains(&id1.as_str()));
        assert!(ids.contains(&id2.as_str()));
    }

    #[test]
    fn test_list_sessions_empty_dir() {
        let mgr = manager();
        let list = mgr.list_sessions().unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn test_list_sessions_sorted_by_updated_at_desc() {
        let mgr = manager();
        let id1 = &Uuid::new_v4().to_string();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let id2 = &Uuid::new_v4().to_string();

        mgr.save_session(&make_state(id1)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        mgr.save_session(&make_state(id2)).unwrap();

        let list = mgr.list_sessions().unwrap();
        assert_eq!(list[0].session_id, *id2);
        assert_eq!(list[1].session_id, *id1);
    }

    #[test]
    fn test_list_sessions_skips_malformed_files() {
        let mgr = manager();

        // Write a valid session.
        let id = &Uuid::new_v4().to_string();
        mgr.save_session(&make_state(id)).unwrap();

        // Write a garbage file.
        let garbage_path = mgr.storage_dir.join("garbage.json");
        fs::write(&garbage_path, "not valid json {{{{").unwrap();

        let list = mgr.list_sessions().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].session_id, *id);
    }

    // -- delete_session --

    #[test]
    fn test_delete_removes_file() {
        let mgr = manager();
        let id = &Uuid::new_v4().to_string();
        mgr.save_session(&make_state(id)).unwrap();

        assert!(mgr.session_path(id).exists());
        mgr.delete_session(id).unwrap();
        assert!(!mgr.session_path(id).exists());
    }

    #[test]
    fn test_delete_nonexistent_session() {
        let mgr = manager();
        let result = mgr.delete_session("no-such-session");
        assert!(result.is_err());
        match result.unwrap_err() {
            SessionPersistError::SessionNotFound(id) => {
                assert_eq!(id, "no-such-session");
            }
            other => panic!("expected SessionNotFound, got {other}"),
        }
    }

    // -- atomic write (temp file + rename) --

    #[test]
    fn test_atomic_write_no_temp_file_remaining() {
        let mgr = manager();
        let id = &Uuid::new_v4().to_string();
        mgr.save_session(&make_state(id)).unwrap();

        // The .json.tmp file must not be left behind.
        let tmp_path = mgr.session_path(id).with_extension("json.tmp");
        assert!(!tmp_path.exists());

        // The final .json file must exist.
        assert!(mgr.session_path(id).exists());
    }

    #[test]
    fn test_save_overwrites_existing_session() {
        let mgr = manager();
        let id = &Uuid::new_v4().to_string();

        let mut state = make_state(id);
        state.messages = vec![Message {
            role: "user".into(),
            content: MessageContent::Text("First save".into()),
        }];
        mgr.save_session(&state).unwrap();

        state.messages.push(Message {
            role: "assistant".into(),
            content: MessageContent::Text("Second save".into()),
        });
        mgr.save_session(&state).unwrap();

        let loaded = mgr.load_session(id).unwrap();
        assert_eq!(loaded.messages.len(), 2);
    }

    // -- session summary extraction --

    #[test]
    fn test_session_summary_fields() {
        let mgr = manager();
        let id = &Uuid::new_v4().to_string();
        let state = make_state(id);
        mgr.save_session(&state).unwrap();

        let summaries = mgr.list_sessions().unwrap();
        let summary = summaries.into_iter().find(|s| s.session_id == *id).unwrap();

        assert_eq!(summary.session_id, *id);
        assert_eq!(summary.name, Some("Test session".to_string()));
        assert_eq!(summary.message_count, 3);
        assert_eq!(summary.model, "claude-3-5-sonnet-20241022");
        assert_eq!(summary.working_dir, PathBuf::from("/tmp/project"));
        assert_eq!(summary.total_tokens, 1024);
    }

    // -- auto-save detection (changed vs unchanged) --

    #[test]
    fn test_auto_save_unchanged_returns_false() {
        let mgr = manager();
        let id = &Uuid::new_v4().to_string();
        let state = make_state(id);

        // Initial save.
        mgr.save_session(&state).unwrap();

        // Auto-save the same state -- should detect no change.
        let did_save = mgr.auto_save(&state).unwrap();
        assert!(!did_save, "auto_save should return false when unchanged");
    }

    #[test]
    fn test_auto_save_changed_returns_true() {
        let mgr = manager();
        let id = &Uuid::new_v4().to_string();
        let mut state = make_state(id);

        // Initial save.
        mgr.save_session(&state).unwrap();

        // Modify the session.
        state.messages.push(Message {
            role: "user".into(),
            content: MessageContent::Text("New message".into()),
        });

        let did_save = mgr.auto_save(&state).unwrap();
        assert!(did_save, "auto_save should return true when changed");

        // Verify the new state was persisted.
        let loaded = mgr.load_session(id).unwrap();
        assert_eq!(loaded.messages.len(), 4);
    }

    #[test]
    fn test_auto_save_new_session_returns_true() {
        let mgr = manager();
        let id = &Uuid::new_v4().to_string();
        let state = make_state(id);

        // Auto-save a session that has never been saved.
        let did_save = mgr.auto_save(&state).unwrap();
        assert!(did_save, "auto_save should return true for a new session");

        // And the file should exist now.
        assert!(mgr.session_path(id).exists());
    }

    // -- should_resume_last --

    #[test]
    fn test_should_resume_recent_session() {
        let mgr = manager();
        let id = &Uuid::new_v4().to_string();
        let state = make_state(id);
        mgr.save_session(&state).unwrap();

        // Session was just saved -- should be within any reasonable timeout.
        let timeout = chrono::Duration::hours(1);
        let result = mgr.should_resume_last(timeout);
        assert_eq!(result, Some(id.clone()));
    }

    #[test]
    fn test_should_resume_stale_session_returns_none() {
        let mgr = manager();
        let id = &Uuid::new_v4().to_string();

        // Create a session with an updated_at far in the past.
        let mut state = make_state(id);
        state.updated_at = Utc::now() - chrono::Duration::hours(24);
        mgr.save_session(&state).unwrap();

        let timeout = chrono::Duration::hours(1);
        let result = mgr.should_resume_last(timeout);
        assert!(result.is_none(), "stale session should not be resumed");
    }

    #[test]
    fn test_should_resume_empty_dir_returns_none() {
        let mgr = manager();
        let timeout = chrono::Duration::hours(1);
        let result = mgr.should_resume_last(timeout);
        assert!(result.is_none(), "empty storage should return None");
    }

    #[test]
    fn test_should_resume_picks_most_recent() {
        let mgr = manager();

        // Old session.
        let old_id = &Uuid::new_v4().to_string();
        let mut old_state = make_state(old_id);
        old_state.updated_at = Utc::now() - chrono::Duration::hours(2);
        mgr.save_session(&old_state).unwrap();

        // Recent session.
        let recent_id = &Uuid::new_v4().to_string();
        let recent_state = make_state(recent_id);
        mgr.save_session(&recent_state).unwrap();

        let timeout = chrono::Duration::hours(3);
        let result = mgr.should_resume_last(timeout);
        assert_eq!(result, Some(recent_id.clone()));
    }

    // -- generate_session_id --

    #[test]
    fn test_generate_session_id_is_valid_uuid() {
        let id = SessionPersistManager::generate_session_id();
        assert!(Uuid::parse_str(&id).is_ok(), "generated ID must be a valid UUID");
    }

    #[test]
    fn test_generate_session_id_is_unique() {
        let id1 = SessionPersistManager::generate_session_id();
        let id2 = SessionPersistManager::generate_session_id();
        assert_ne!(id1, id2);
    }

    // -- session_path --

    #[test]
    fn test_session_path() {
        let mgr = manager();
        let path = mgr.session_path("abc-123");
        assert_eq!(path, mgr.storage_dir.join("abc-123.json"));
    }

    // -- human-readable JSON --

    #[test]
    fn test_saved_file_is_pretty_json() {
        let mgr = manager();
        let id = &Uuid::new_v4().to_string();
        mgr.save_session(&make_state(id)).unwrap();

        let contents = fs::read_to_string(mgr.session_path(id)).unwrap();
        assert!(contents.contains('\n'), "file should be pretty-printed");
        assert!(contents.contains("session_id"), "file should contain session_id");
        assert!(contents.contains("messages"), "file should contain messages");
    }

    // -- new and with_dir --

    #[test]
    fn test_with_dir_creates_directory() {
        let dir = std::env::temp_dir()
            .join("shannon-test-persist-create")
            .join(Uuid::new_v4().to_string());
        assert!(!dir.exists());

        SessionPersistManager::with_dir(dir.clone()).unwrap();
        assert!(dir.exists());
    }

    #[test]
    fn test_new_uses_default_dir() {
        let mgr = SessionPersistManager::new().unwrap();
        let dir_str = mgr.storage_dir.to_string_lossy();
        assert!(dir_str.contains(".shannon"));
        assert!(dir_str.contains("sessions"));
    }

    // -- content_hash stability --

    #[test]
    fn test_content_hash_deterministic() {
        let state = make_state(&Uuid::new_v4().to_string());
        let h1 = compute_hash(&state);
        let h2 = compute_hash(&state);
        assert_eq!(h1, h2, "hash must be deterministic for the same input");
    }

    #[test]
    fn test_content_hash_differs_on_change() {
        let mut state = make_state(&Uuid::new_v4().to_string());
        let h1 = compute_hash(&state);
        state.messages.push(Message {
            role: "user".into(),
            content: MessageContent::Text("extra".into()),
        });
        let h2 = compute_hash(&state);
        assert_ne!(h1, h2, "hash must change when session data changes");
    }
}
