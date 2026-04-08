//! State management foundation types

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// State errors
#[derive(thiserror::Error, Debug)]
pub enum StateError {
    #[error("Session not found: {0}")]
    SessionNotFound(Uuid),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Metadata for a persisted session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPersistMetadata {
    pub session_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub file_path: PathBuf,
    pub file_size: u64,
}

/// In-memory session state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    pub session_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub metadata: SessionMetadata,
    pub data: serde_json::Value,
}

/// Session metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub user_id: Option<String>,
    pub query_count: u64,
    pub total_tokens_used: u64,
    pub model: String,
}

/// Session data for persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub session_id: Uuid,
    pub messages: Vec<serde_json::Value>,
    pub metadata: SessionMetadata,
}

/// Session info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub metadata: SessionMetadata,
}

/// State manager (placeholder - to be implemented)
pub struct StateManager {
    pub sessions_dir: PathBuf,
}

impl StateManager {
    pub fn new(sessions_dir: PathBuf) -> Self {
        Self { sessions_dir }
    }

    pub fn sessions_dir(&self) -> &PathBuf {
        &self.sessions_dir
    }
}
