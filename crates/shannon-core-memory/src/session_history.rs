//! Session history management

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// Session history entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEntry {
    pub id: Uuid,
    pub title: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub message_count: usize,
    pub last_activity: chrono::DateTime<chrono::Utc>,
}

/// Session history manager
pub struct SessionHistoryManager {
    storage_path: PathBuf,
    sessions: Vec<SessionEntry>,
}

impl SessionHistoryManager {
    pub fn new(storage_path: PathBuf) -> Self {
        Self {
            storage_path,
            sessions: Vec::new(),
        }
    }

    /// Add a session to history
    pub fn add_session(&mut self, entry: SessionEntry) -> Result<(), SessionHistoryError> {
        self.sessions.push(entry);
        self.save()?;
        Ok(())
    }

    /// Update session activity
    pub fn update_activity(&mut self, session_id: &Uuid) -> Result<(), SessionHistoryError> {
        if let Some(session) = self.sessions.iter_mut().find(|s| s.id == *session_id) {
            session.last_activity = chrono::Utc::now();
            self.save()?;
        }
        Ok(())
    }

    /// Get recent sessions
    pub fn recent_sessions(&self, limit: usize) -> Vec<SessionEntry> {
        let mut sessions = self.sessions.clone();
        sessions.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));
        sessions.into_iter().take(limit).collect()
    }

    /// Get session by ID
    pub fn get_session(&self, id: &Uuid) -> Option<&SessionEntry> {
        self.sessions.iter().find(|s| s.id == *id)
    }

    /// Save to disk
    fn save(&self) -> Result<(), SessionHistoryError> {
        if let Some(parent) = self.storage_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| SessionHistoryError::StorageError(e.to_string()))?;
        }

        let json = serde_json::to_string_pretty(&self.sessions)
            .map_err(|e| SessionHistoryError::SerializationError(e.to_string()))?;

        std::fs::write(&self.storage_path, json)
            .map_err(|e| SessionHistoryError::StorageError(e.to_string()))?;

        Ok(())
    }

    /// Load from disk
    pub fn load(&mut self) -> Result<(), SessionHistoryError> {
        if !self.storage_path.exists() {
            return Ok(());
        }

        let json = std::fs::read_to_string(&self.storage_path)
            .map_err(|e| SessionHistoryError::StorageError(e.to_string()))?;

        self.sessions = serde_json::from_str(&json)
            .map_err(|e| SessionHistoryError::SerializationError(e.to_string()))?;

        Ok(())
    }
}

/// Session history errors
#[derive(Debug, thiserror::Error)]
pub enum SessionHistoryError {
    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Session not found: {0}")]
    NotFound(Uuid),
}
