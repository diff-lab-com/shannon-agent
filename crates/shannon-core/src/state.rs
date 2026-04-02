//! # State Management
//!
//! Persistent state and session management for Shannon.

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;

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
}

/// State for a single session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    pub session_id: Uuid,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub metadata: SessionMetadata,
    pub data: serde_json::Value,
}

/// Metadata about a session
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

/// State manager for handling persistent state
pub struct StateManager {
    sessions: Arc<DashMap<Uuid, SessionState>>,
    global: Arc<DashMap<String, serde_json::Value>>,
}

impl StateManager {
    /// Create a new state manager
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
            global: Arc::new(DashMap::new()),
        }
    }

    /// Create a new session
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

    /// Get a session by ID
    pub fn get_session(&self, session_id: Uuid) -> Result<SessionState, StateError> {
        self.sessions
            .get(&session_id)
            .map(|v| v.clone())
            .ok_or(StateError::SessionNotFound(session_id))
    }

    /// Update a session
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

    /// Delete a session
    pub fn delete_session(&self, session_id: Uuid) -> Result<(), StateError> {
        self.sessions
            .remove(&session_id)
            .ok_or(StateError::SessionNotFound(session_id))?;
        Ok(())
    }

    /// Get global state value
    pub fn get_global(&self, key: &str) -> Option<serde_json::Value> {
        self.global.get(key).map(|v| v.clone())
    }

    /// Set global state value
    pub fn set_global(&self, key: String, value: serde_json::Value) {
        self.global.insert(key, value);
    }

    /// Increment session query count
    pub fn increment_query_count(&self, session_id: Uuid) -> Result<(), StateError> {
        self.update_session(session_id, |session| {
            session.metadata.query_count += 1;
        })
    }

    /// Add tokens used to session
    pub fn add_tokens_used(&self, session_id: Uuid, tokens: u64) -> Result<(), StateError> {
        self.update_session(session_id, |session| {
            session.metadata.total_tokens_used += tokens;
        })
    }

    /// Get all active sessions
    pub fn list_sessions(&self) -> Vec<SessionState> {
        self.sessions.iter().map(|v| v.clone()).collect()
    }

    /// Get session count
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    /// Serialize all sessions to JSON
    pub fn serialize_sessions(&self) -> Result<String, StateError> {
        let sessions: Vec<(Uuid, SessionState)> = self
            .sessions
            .iter()
            .map(|entry| (*entry.key(), entry.value().clone()))
            .collect();
        serde_json::to_string(&sessions)
            .map_err(|e| StateError::SerializationError(e.to_string()))
    }

    /// Deserialize sessions from JSON
    pub fn deserialize_sessions(&self, data: &str) -> Result<(), StateError> {
        let sessions: Vec<(Uuid, SessionState)> =
            serde_json::from_str(data).map_err(|e| StateError::DeserializationError(e.to_string()))?;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
