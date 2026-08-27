//! # State Management
//!
//! In-memory session registry for Shannon.
//!
//! Durable session persistence moved to the L0 event log in §4.6: the
//! authoritative record for every session is
//! `<sessions-dir>/<uuid>/events.jsonl`, and everything else (message
//! history, token totals, listings, branches) is **derived** from it via
//! `shannon_core::session_log`. This type keeps only the process-lifetime
//! registry (live sessions and global key/values) plus the resolved
//! sessions directory — the storage location shared with the L0 writer and
//! with [`shannon_core::session_log::SessionStore`], which owns all disk
//! I/O. Legacy single-file `sessions/<uuid>.json` snapshots are gone (DP4,
//! breaking change: no migration path).

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
// State Manager
// ============================================================================

/// State manager for the process-lifetime session registry.
///
/// Maintains an in-memory `DashMap` of active sessions plus global
/// key/values, and resolves the shared sessions directory (defaulting to
/// `~/.shannon/sessions/`) that the L0 log writer and
/// `shannon_core::session_log::SessionStore` operate on.
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
        serde_json::to_string(&sessions).map_err(|e| StateError::SerializationError(e.to_string()))
    }

    /// Deserialize sessions from JSON into memory.
    pub fn deserialize_sessions(&self, data: &str) -> Result<(), StateError> {
        let sessions: Vec<(Uuid, SessionState)> = serde_json::from_str(data)
            .map_err(|e| StateError::DeserializationError(e.to_string()))?;

        for (id, session) in sessions {
            self.sessions.insert(id, session);
        }

        Ok(())
    }
}

impl Default for StateManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Storage location note
// ============================================================================

/// The sessions directory resolved here is shared verbatim with the L0
/// writer: the engine tees events into
/// `sessions_dir()/<uuid>/events.jsonl`, and
/// `shannon_core::session_log::SessionStore` derives everything else from it.
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
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    // -- existing in-memory tests --

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

    // -- Persistence behavior moved to shannon_core::session_log::session_store.
    // The L0 cutover removed single-file snapshots; restore/listing/branch
    // coverage lives beside the store implementation (breaking change DP4).

    #[tokio::test]
    async fn test_concurrent_session_insert_and_read() {
        let manager = Arc::new(StateManager::new());
        let num_threads = 10;
        let inserts_per_thread = 100;

        let mut handles = Vec::new();

        // Spawn multiple threads that each create sessions
        for _ in 0..num_threads {
            let manager_clone = manager.clone();
            let handle = tokio::spawn(async move {
                for i in 0..inserts_per_thread {
                    let _ = manager_clone
                        .create_session(Some(format!("user_{i}")), "test-model".to_string());
                }
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify all sessions were inserted
        assert_eq!(manager.session_count(), num_threads * inserts_per_thread);
    }

    #[tokio::test]
    async fn test_concurrent_global_state_access() {
        let manager = Arc::new(StateManager::new());
        let num_threads = 20;
        let operations_per_thread = 50;

        let mut handles = Vec::new();

        // Each thread performs a mix of set and get operations
        for i in 0..num_threads {
            let manager_ref = manager.clone();
            let handle = tokio::spawn(async move {
                for j in 0..operations_per_thread {
                    let key = format!("key_{i}_{j}");
                    let value = serde_json::json!(j);

                    // Set
                    manager_ref.set_global(key.clone(), value);

                    // Get (should return what was just set)
                    let retrieved = manager_ref.get_global(&key);
                    assert!(retrieved.is_some());
                    assert_eq!(retrieved.unwrap().as_i64(), Some(j));
                }
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify final state - manager is an Arc, so we need to get the inner DashMap length
        let final_len = manager.global.len();
        assert_eq!(
            final_len,
            num_threads as usize * operations_per_thread as usize
        );
    }

    #[tokio::test]
    async fn test_concurrent_session_update_and_delete() {
        let manager = Arc::new(StateManager::new());
        let num_sessions = 10;

        // Create initial sessions
        let mut session_ids = Vec::new();
        for _ in 0..num_sessions {
            let session = manager
                .create_session(None, "test-model".to_string())
                .unwrap();
            session_ids.push(session.session_id);
        }

        let session_ids_for_update = session_ids.clone();
        let session_ids_for_delete = session_ids.clone();
        let manager_for_update = manager.clone();
        let manager_for_delete = manager.clone();

        // Concurrently update and delete sessions
        let update_handle = tokio::spawn(async move {
            for session_id in session_ids_for_update.iter() {
                for _ in 0..10 {
                    let _ = manager_for_update.increment_query_count(*session_id);
                    let _ = manager_for_update.add_tokens_used(*session_id, 100);
                }
            }
        });

        let delete_handle = tokio::spawn(async move {
            for (i, session_id) in session_ids_for_delete.iter().enumerate() {
                if i % 2 == 0 {
                    // Delete every other session
                    let _ = manager_for_delete.delete_session(*session_id);
                }
            }
        });

        update_handle.await.unwrap();
        delete_handle.await.unwrap();

        // Verify remaining sessions
        let remaining_count = manager.session_count();
        assert_eq!(remaining_count, num_sessions / 2);

        // Verify that remaining sessions have the correct counts
        for session_id in &session_ids {
            if let Ok(session) = manager.get_session(*session_id) {
                assert_eq!(session.metadata.query_count, 10);
                assert_eq!(session.metadata.total_tokens_used, 1000);
            }
        }
    }

    #[tokio::test]
    async fn test_concurrent_tool_registry_style_operations() {
        // Simulate tool registry style operations with DashMap
        use dashmap::DashMap;

        let tool_registry: Arc<DashMap<String, serde_json::Value>> = Arc::new(DashMap::new());
        let num_threads = 15;
        let tools_per_thread = 20;

        let mut handles = Vec::new();

        // Each thread registers tools
        for i in 0..num_threads {
            let registry_ref = tool_registry.clone();
            let handle = tokio::spawn(async move {
                for j in 0..tools_per_thread {
                    let tool_name = format!("tool_{i}_{j}");
                    let tool_def = serde_json::json!({
                        "name": tool_name,
                        "description": "Test tool"
                    });

                    // Insert
                    registry_ref.insert(tool_name.clone(), tool_def.clone());

                    // Read back
                    if let Some(retrieved) = registry_ref.get(&tool_name) {
                        assert_eq!(retrieved["name"], tool_name);
                    }

                    // Remove every other tool
                    if j % 2 == 0 {
                        registry_ref.remove(&tool_name);
                    }
                }
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify final count (should be half because we removed every other)
        let final_count = tool_registry.len();
        assert_eq!(final_count, num_threads * tools_per_thread / 2);
    }

    #[tokio::test]
    async fn test_concurrent_session_state_serialization() {
        let manager = Arc::new(StateManager::new());
        let num_threads = 5;

        // Create sessions
        let mut session_ids = Vec::new();
        for _ in 0..num_threads {
            let session = manager
                .create_session(
                    Some(format!("user_{}", session_ids.len())),
                    "test-model".to_string(),
                )
                .unwrap();
            session_ids.push(session.session_id);
        }

        // Concurrently serialize sessions - clone session_ids to move into closures
        let handles: Vec<_> = session_ids
            .iter()
            .map(|session_id| {
                let manager_clone = manager.clone();
                let id = *session_id;
                tokio::spawn(async move {
                    // Simultaneous reads during serialization
                    let session1 = manager_clone.get_session(id).unwrap();
                    let serialized = manager_clone.serialize_sessions().unwrap();
                    let session2 = manager_clone.get_session(id).unwrap();

                    assert_eq!(session1.session_id, session2.session_id);
                    assert!(serialized.contains(&id.to_string()));
                })
            })
            .collect();

        for handle in handles {
            handle.await.unwrap();
        }
    }
}
