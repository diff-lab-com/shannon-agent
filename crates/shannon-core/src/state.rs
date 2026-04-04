//! # State Management
//!
//! Persistent state and session management for Shannon.
//!
//! Sessions can be persisted to disk in `~/.shannon/sessions/` (or a configurable
//! directory) as human-readable JSON files. Each session is stored as a single
//! `.json` file named after its UUID.

use crate::api::Message;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;

/// Default sessions directory relative to home: `~/.shannon/sessions/`
const DEFAULT_SESSIONS_DIR: &str = ".shannon/sessions";

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during state operations
#[derive(Error, Debug)]
pub enum StateError {
    #[error("Session not found: {0}")]
    SessionNotFound(Uuid),

    #[error("State serialization error: {0}")]
    SerializationError(String),

    #[error("State deserialization error: {0}")]
    DeserializationError(String),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

// ============================================================================
// In-Memory Session Types (existing)
// ============================================================================

/// State for a single session (in-memory)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    pub session_id: Uuid,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub metadata: SessionMetadata,
    pub data: serde_json::Value,
}

/// Metadata about a session (in-memory)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub user_id: Option<String>,
    pub query_count: u64,
    pub total_tokens_used: u64,
    pub model: String,
}

/// Global application state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalState {
    pub version: String,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub total_sessions: u64,
    pub total_queries: u64,
}

// ============================================================================
// Persistent Session Types
// ============================================================================

/// Metadata for a persisted session.
///
/// Stored alongside messages in the session JSON file and used to populate
/// `SessionInfo` listings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPersistMetadata {
    /// The model used for this session (e.g. "claude-3-5-sonnet-20241022").
    pub model: String,
    /// ISO 8601 creation timestamp.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// ISO 8601 last-modified timestamp.
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// Total input tokens consumed across all turns.
    pub total_input_tokens: u64,
    /// Total output tokens consumed across all turns.
    pub total_output_tokens: u64,
    /// Number of conversation turns (user messages).
    pub turn_count: usize,
    /// Optional title / summary provided by the caller.
    pub title: Option<String>,
}

impl Default for SessionPersistMetadata {
    fn default() -> Self {
        let now = chrono::Utc::now();
        Self {
            model: String::new(),
            created_at: now,
            updated_at: now,
            total_input_tokens: 0,
            total_output_tokens: 0,
            turn_count: 0,
            title: None,
        }
    }
}

/// Full persisted session data written to disk.
///
/// Contains the conversation messages together with metadata and aggregate
/// statistics. Serialized as a single human-readable JSON file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    /// Unique session identifier.
    pub session_id: Uuid,
    /// Session metadata (model, timestamps, token counts).
    pub metadata: SessionPersistMetadata,
    /// Ordered list of conversation messages.
    pub messages: Vec<Message>,
}

impl SessionData {
    /// Create a new empty session data container.
    pub fn new(session_id: Uuid, model: String) -> Self {
        Self {
            session_id,
            metadata: SessionPersistMetadata {
                model,
                ..Default::default()
            },
            messages: Vec::new(),
        }
    }

    /// Convenience: return the first user message text as a title preview.
    pub fn first_user_message_preview(&self, max_len: usize) -> Option<String> {
        self.messages
            .iter()
            .find(|m| m.role == "user")
            .and_then(|m| match &m.content {
                crate::api::MessageContent::Text(t) => Some(t.clone()),
                crate::api::MessageContent::Blocks(blocks) => blocks
                    .iter()
                    .find_map(|b| match b {
                        crate::api::ContentBlock::Text { text } => Some(text.clone()),
                        _ => None,
                    }),
            })
            .map(|t| {
                if t.len() > max_len {
                    format!("{}...", &t[..max_len.saturating_sub(3)])
                } else {
                    t
                }
            })
    }
}

/// Lightweight summary used when listing sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: Uuid,
    pub title: Option<String>,
    /// Preview of the first user message (fallback when no title is set).
    pub preview: Option<String>,
    pub model: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub turn_count: usize,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
}

// ============================================================================
// State Manager
// ============================================================================

/// State manager for handling persistent state.
///
/// Maintains an in-memory `DashMap` of active sessions **and** supports
/// persisting sessions to disk as JSON files under a configurable directory
/// (defaulting to `~/.shannon/sessions/`).
pub struct StateManager {
    sessions: Arc<DashMap<Uuid, SessionState>>,
    global: Arc<DashMap<String, serde_json::Value>>,
    /// Directory where persisted session JSON files are stored.
    sessions_dir: PathBuf,
}

impl StateManager {
    /// Create a new state manager with the default sessions directory
    /// (`~/.shannon/sessions/`).
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
            global: Arc::new(DashMap::new()),
            sessions_dir: default_sessions_dir(),
        }
    }

    /// Create a new state manager with a custom sessions directory.
    ///
    /// The directory is created if it does not already exist.
    pub fn with_sessions_dir(dir: PathBuf) -> Result<Self, StateError> {
        fs::create_dir_all(&dir)?;
        Ok(Self {
            sessions: Arc::new(DashMap::new()),
            global: Arc::new(DashMap::new()),
            sessions_dir: dir,
        })
    }

    /// Return the configured sessions directory path.
    pub fn sessions_dir(&self) -> &Path {
        &self.sessions_dir
    }

    // ----------------------------------------------------------------
    // In-memory operations (existing API, unchanged)
    // ----------------------------------------------------------------

    /// Create a new in-memory session.
    pub fn create_session(
        &self,
        user_id: Option<String>,
        model: String,
    ) -> Result<SessionState, StateError> {
        let session_id = Uuid::new_v4();
        let now = chrono::Utc::now();

        let session = SessionState {
            session_id,
            created_at: now,
            updated_at: now,
            metadata: SessionMetadata {
                user_id,
                query_count: 0,
                total_tokens_used: 0,
                model,
            },
            data: serde_json::json!({}),
        };

        self.sessions.insert(session_id, session.clone());
        Ok(session)
    }

    /// Get a session by ID (in-memory only).
    pub fn get_session(&self, session_id: Uuid) -> Result<SessionState, StateError> {
        self.sessions
            .get(&session_id)
            .map(|v| v.clone())
            .ok_or(StateError::SessionNotFound(session_id))
    }

    /// Update a session (in-memory only).
    pub fn update_session(
        &self,
        session_id: Uuid,
        mut updater: impl FnMut(&mut SessionState),
    ) -> Result<(), StateError> {
        let mut session = self.get_session(session_id)?;

        updater(&mut session);
        session.updated_at = chrono::Utc::now();

        self.sessions.insert(session_id, session);
        Ok(())
    }

    /// Delete a session (in-memory only).
    pub fn delete_session(&self, session_id: Uuid) -> Result<(), StateError> {
        self.sessions
            .remove(&session_id)
            .ok_or(StateError::SessionNotFound(session_id))?;
        Ok(())
    }

    /// Get global state value.
    pub fn get_global(&self, key: &str) -> Option<serde_json::Value> {
        self.global.get(key).map(|v| v.clone())
    }

    /// Set global state value.
    pub fn set_global(&self, key: String, value: serde_json::Value) {
        self.global.insert(key, value);
    }

    /// Increment session query count.
    pub fn increment_query_count(&self, session_id: Uuid) -> Result<(), StateError> {
        self.update_session(session_id, |session| {
            session.metadata.query_count += 1;
        })
    }

    /// Add tokens used to session.
    pub fn add_tokens_used(&self, session_id: Uuid, tokens: u64) -> Result<(), StateError> {
        self.update_session(session_id, |session| {
            session.metadata.total_tokens_used += tokens;
        })
    }

    /// Get all active sessions (in-memory).
    pub fn list_sessions(&self) -> Vec<SessionState> {
        self.sessions.iter().map(|v| v.clone()).collect()
    }

    /// Get session count (in-memory).
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    /// Serialize all in-memory sessions to JSON.
    pub fn serialize_sessions(&self) -> Result<String, StateError> {
        let sessions: Vec<(Uuid, SessionState)> = self
            .sessions
            .iter()
            .map(|entry| (*entry.key(), entry.value().clone()))
            .collect();
        serde_json::to_string(&sessions)
            .map_err(|e| StateError::SerializationError(e.to_string()))
    }

    /// Deserialize sessions from JSON into memory.
    pub fn deserialize_sessions(&self, data: &str) -> Result<(), StateError> {
        let sessions: Vec<(Uuid, SessionState)> =
            serde_json::from_str(data).map_err(|e| StateError::DeserializationError(e.to_string()))?;

        for (id, session) in sessions {
            self.sessions.insert(id, session);
        }

        Ok(())
    }

    // ----------------------------------------------------------------
    // Persistent session operations
    // ----------------------------------------------------------------

    /// Save a session to disk.
    ///
    /// Writes the given messages and metadata as a JSON file named
    /// `{session_id}.json` inside the configured sessions directory.
    /// The file is written with pretty-printing so that it is
    /// human-readable and diff-friendly.
    pub fn save_session(
        &self,
        session_id: &Uuid,
        messages: &[Message],
        metadata: &SessionPersistMetadata,
    ) -> Result<(), StateError> {
        fs::create_dir_all(&self.sessions_dir)?;

        let mut metadata = metadata.clone();
        metadata.updated_at = chrono::Utc::now();
        metadata.turn_count = messages.iter().filter(|m| m.role == "user").count();

        let session_data = SessionData {
            session_id: *session_id,
            metadata,
            messages: messages.to_vec(),
        };

        let path = self.session_file_path(session_id);
        let json = serde_json::to_string_pretty(&session_data)
            .map_err(|e| StateError::SerializationError(e.to_string()))?;

        // Atomic-ish write: write to temp file then rename.
        let tmp_path = path.with_extension("json.tmp");
        fs::write(&tmp_path, json)?;
        fs::rename(&tmp_path, &path)?;

        Ok(())
    }

    /// Load a session from disk.
    ///
    /// Returns `Ok(None)` when no file exists for the given `session_id`.
    pub fn load_session(&self, session_id: &Uuid) -> Result<Option<SessionData>, StateError> {
        let path = self.session_file_path(session_id);

        if !path.exists() {
            return Ok(None);
        }

        let contents = fs::read_to_string(&path)?;
        let session_data: SessionData =
            serde_json::from_str(&contents).map_err(|e| StateError::DeserializationError(e.to_string()))?;

        Ok(Some(session_data))
    }

    /// List all persisted sessions.
    ///
    /// Scans the sessions directory for `.json` files and reads each one to
    /// extract lightweight `SessionInfo` metadata. Sessions whose files
    /// cannot be parsed are silently skipped.
    pub fn list_persisted_sessions(&self) -> Result<Vec<SessionInfo>, StateError> {
        if !self.sessions_dir.exists() {
            return Ok(Vec::new());
        }

        let mut infos = Vec::new();

        let entries = fs::read_dir(&self.sessions_dir)?;
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue, // skip unreadable entries
            };

            let path = entry.path();
            // Only consider .json files (skip .json.tmp etc.)
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }

            let contents = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue, // skip unreadable files
            };

            let data: SessionData = match serde_json::from_str(&contents) {
                Ok(d) => d,
                Err(_) => continue, // skip malformed files
            };

            infos.push(SessionInfo {
                session_id: data.session_id,
                title: data.metadata.title.clone(),
                preview: data.first_user_message_preview(80),
                model: data.metadata.model,
                created_at: data.metadata.created_at,
                updated_at: data.metadata.updated_at,
                turn_count: data.metadata.turn_count,
                total_input_tokens: data.metadata.total_input_tokens,
                total_output_tokens: data.metadata.total_output_tokens,
            });
        }

        // Return sorted by most-recently-updated first.
        infos.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

        Ok(infos)
    }

    /// Delete a persisted session from disk.
    ///
    /// Returns `Ok(false)` when no file exists for the given `session_id`.
    pub fn delete_persisted_session(&self, session_id: &Uuid) -> Result<bool, StateError> {
        let path = self.session_file_path(session_id);

        if !path.exists() {
            return Ok(false);
        }

        fs::remove_file(&path)?;
        Ok(true)
    }

    // ----------------------------------------------------------------
    // Private helpers
    // ----------------------------------------------------------------

    /// Build the file path for a given session UUID.
    fn session_file_path(&self, session_id: &Uuid) -> PathBuf {
        self.sessions_dir.join(format!("{}.json", session_id))
    }
}

impl Default for StateManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Free helpers
// ============================================================================

/// Return the default sessions directory path (`$HOME/.shannon/sessions`).
///
/// Falls back to `/tmp/.shannon/sessions` when `$HOME` is not set (e.g. in
/// some CI environments).
fn default_sessions_dir() -> PathBuf {
    match std::env::var("HOME") {
        Ok(home) => PathBuf::from(home).join(DEFAULT_SESSIONS_DIR),
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

    // -- helper to build a test manager with a temp directory --

    fn temp_sessions_dir() -> PathBuf {
        let dir = std::env::temp_dir()
            .join("shannon-test-sessions")
            .join(Uuid::new_v4().to_string());
        // We create it inside tests; caller is responsible for cleanup.
        let _ = fs::create_dir_all(&dir);
        dir
    }

    fn test_manager() -> StateManager {
        let dir = temp_sessions_dir();
        StateManager::with_sessions_dir(dir).unwrap()
    }

    fn make_messages() -> Vec<Message> {
        vec![
            Message {
                role: "user".to_string(),
                content: MessageContent::Text("Hello, Claude!".to_string()),
            },
            Message {
                role: "assistant".to_string(),
                content: MessageContent::Text("Hello! How can I help you today?".to_string()),
            },
            Message {
                role: "user".to_string(),
                content: MessageContent::Text("Tell me about Rust.".to_string()),
            },
        ]
    }

    fn make_metadata(model: &str) -> SessionPersistMetadata {
        SessionPersistMetadata {
            model: model.to_string(),
            ..Default::default()
        }
    }

    // -- existing in-memory tests (unchanged) --

    #[test]
    fn test_session_creation() {
        let manager = StateManager::new();
        let session = manager
            .create_session(Some("user123".to_string()), "claude-3-5-sonnet".to_string())
            .unwrap();

        assert_eq!(session.metadata.user_id, Some("user123".to_string()));
        assert_eq!(session.metadata.query_count, 0);
    }

    #[test]
    fn test_session_update() {
        let manager = StateManager::new();
        let session = manager
            .create_session(None, "claude-3-5-sonnet".to_string())
            .unwrap();

        manager
            .update_session(session.session_id, |s| {
                s.metadata.query_count = 5;
            })
            .unwrap();

        let updated = manager.get_session(session.session_id).unwrap();
        assert_eq!(updated.metadata.query_count, 5);
    }

    #[test]
    fn test_increment_query_count() {
        let manager = StateManager::new();
        let session = manager
            .create_session(None, "claude-3-5-sonnet".to_string())
            .unwrap();

        manager.increment_query_count(session.session_id).unwrap();
        let updated = manager.get_session(session.session_id).unwrap();
        assert_eq!(updated.metadata.query_count, 1);
    }

    #[test]
    fn test_global_state() {
        let manager = StateManager::new();
        manager.set_global("test_key".to_string(), serde_json::json!("test_value"));
        assert_eq!(
            manager.get_global("test_key"),
            Some(serde_json::json!("test_value"))
        );
    }

    // -- persistent session tests --

    #[test]
    fn test_save_and_load_session() {
        let manager = test_manager();
        let session_id = Uuid::new_v4();
        let messages = make_messages();
        let metadata = make_metadata("claude-3-5-sonnet-20241022");

        // Save
        manager.save_session(&session_id, &messages, &metadata).unwrap();

        // Verify file exists
        let path = manager.session_file_path(&session_id);
        assert!(path.exists(), "session file should exist after save");

        // Load
        let loaded = manager.load_session(&session_id).unwrap().expect("session should load");
        assert_eq!(loaded.session_id, session_id);
        assert_eq!(loaded.messages.len(), 3);
        assert_eq!(loaded.metadata.model, "claude-3-5-sonnet-20241022");
        assert_eq!(loaded.metadata.turn_count, 2); // two user messages
    }

    #[test]
    fn test_load_nonexistent_session_returns_none() {
        let manager = test_manager();
        let result = manager.load_session(&Uuid::new_v4()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_save_overwrites_existing() {
        let manager = test_manager();
        let session_id = Uuid::new_v4();

        // Save with one message
        let msgs1 = vec![Message {
            role: "user".to_string(),
            content: MessageContent::Text("First".to_string()),
        }];
        manager
            .save_session(&session_id, &msgs1, &make_metadata("model-a"))
            .unwrap();

        // Save again with two messages
        let msgs2 = vec![
            Message {
                role: "user".to_string(),
                content: MessageContent::Text("First".to_string()),
            },
            Message {
                role: "assistant".to_string(),
                content: MessageContent::Text("Second".to_string()),
            },
        ];
        manager
            .save_session(&session_id, &msgs2, &make_metadata("model-b"))
            .unwrap();

        let loaded = manager.load_session(&session_id).unwrap().unwrap();
        assert_eq!(loaded.messages.len(), 2);
        assert_eq!(loaded.metadata.model, "model-b");
    }

    #[test]
    fn test_list_persisted_sessions() {
        let manager = test_manager();

        // Save two sessions
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        manager
            .save_session(&id1, &make_messages(), &make_metadata("model-1"))
            .unwrap();
        manager
            .save_session(&id2, &make_messages(), &make_metadata("model-2"))
            .unwrap();

        let list = manager.list_persisted_sessions().unwrap();
        assert_eq!(list.len(), 2);

        // Both IDs should be present (order is newest-first).
        let ids: Vec<Uuid> = list.iter().map(|i| i.session_id).collect();
        assert!(ids.contains(&id1));
        assert!(ids.contains(&id2));
    }

    #[test]
    fn test_list_persisted_sessions_empty_dir() {
        let manager = test_manager();
        let list = manager.list_persisted_sessions().unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn test_list_persisted_sessions_skips_bad_files() {
        let manager = test_manager();

        // Write a valid session
        let id = Uuid::new_v4();
        manager
            .save_session(&id, &make_messages(), &make_metadata("model"))
            .unwrap();

        // Write a garbage file that should be silently skipped
        let garbage_path = manager.sessions_dir.join("garbage.json");
        fs::write(&garbage_path, "not valid json {{{{").unwrap();

        // Write a .json.tmp file that should also be skipped
        let tmp_path = manager.sessions_dir.join("should-not-appear.json.tmp");
        fs::write(&tmp_path, "{}").unwrap();

        let list = manager.list_persisted_sessions().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].session_id, id);
    }

    #[test]
    fn test_delete_persisted_session() {
        let manager = test_manager();
        let session_id = Uuid::new_v4();

        manager
            .save_session(&session_id, &make_messages(), &make_metadata("model"))
            .unwrap();
        assert!(manager.session_file_path(&session_id).exists());

        let deleted = manager.delete_persisted_session(&session_id).unwrap();
        assert!(deleted);
        assert!(!manager.session_file_path(&session_id).exists());
    }

    #[test]
    fn test_delete_nonexistent_session_returns_false() {
        let manager = test_manager();
        let deleted = manager
            .delete_persisted_session(&Uuid::new_v4())
            .unwrap();
        assert!(!deleted);
    }

    #[test]
    fn test_session_file_is_human_readable_json() {
        let manager = test_manager();
        let session_id = Uuid::new_v4();

        manager
            .save_session(&session_id, &make_messages(), &make_metadata("model"))
            .unwrap();

        let contents = fs::read_to_string(manager.session_file_path(&session_id)).unwrap();

        // Must be valid JSON
        let _: serde_json::Value = serde_json::from_str(&contents).unwrap();

        // Must contain session_id
        assert!(contents.contains(&session_id.to_string()));

        // Pretty-printed means it contains newlines
        assert!(contents.contains('\n'));
    }

    #[test]
    fn test_session_metadata_updated_at_is_set_on_save() {
        let manager = test_manager();
        let session_id = Uuid::new_v4();
        let messages = make_messages();

        let mut metadata = make_metadata("model");
        let before = metadata.updated_at;

        // Small sleep to ensure a different timestamp
        std::thread::sleep(std::time::Duration::from_millis(10));

        manager.save_session(&session_id, &messages, &metadata).unwrap();

        let loaded = manager.load_session(&session_id).unwrap().unwrap();
        assert!(loaded.metadata.updated_at >= before);
    }

    #[test]
    fn test_session_turn_count_computed_on_save() {
        let manager = test_manager();
        let session_id = Uuid::new_v4();

        let mut metadata = make_metadata("model");
        metadata.turn_count = 0; // should be overwritten

        manager
            .save_session(&session_id, &make_messages(), &metadata)
            .unwrap();

        let loaded = manager.load_session(&session_id).unwrap().unwrap();
        assert_eq!(loaded.metadata.turn_count, 2); // two user messages in make_messages
    }

    #[test]
    fn test_session_info_preview_from_first_user_message() {
        let session_data = SessionData::new(Uuid::new_v4(), "model".into());
        // SessionData with no messages has no preview.
        assert!(session_data.first_user_message_preview(80).is_none());

        // With messages.
        let data = SessionData {
            messages: make_messages(),
            ..SessionData::new(Uuid::new_v4(), "model".into())
        };
        let preview = data.first_user_message_preview(80).unwrap();
        assert_eq!(preview, "Hello, Claude!");

        // Truncation
        let long = Message {
            role: "user".into(),
            content: MessageContent::Text("A".repeat(200)),
        };
        let data2 = SessionData {
            messages: vec![long],
            ..SessionData::new(Uuid::new_v4(), "model".into())
        };
        let truncated = data2.first_user_message_preview(20).unwrap();
        assert!(truncated.ends_with("..."));
        assert!(truncated.len() <= 20);
    }

    #[test]
    fn test_with_sessions_dir_creates_directory() {
        let dir = std::env::temp_dir()
            .join("shannon-test-create")
            .join(Uuid::new_v4().to_string());
        assert!(!dir.exists());

        StateManager::with_sessions_dir(dir.clone()).unwrap();
        assert!(dir.exists());

        // Cleanup
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_default_sessions_dir_uses_home() {
        // Just verify the function doesn't panic.
        let dir = default_sessions_dir();
        assert!(dir.to_string_lossy().contains(".shannon"));
        assert!(dir.to_string_lossy().contains("sessions"));
    }
}
