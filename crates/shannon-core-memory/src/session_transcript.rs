//! Session transcript storage

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;
use shannon_types::Message;

/// Session transcript
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transcript {
    pub session_id: Uuid,
    pub messages: Vec<Message>,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub ended_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Transcript store
pub struct TranscriptStore {
    storage_path: PathBuf,
}

impl TranscriptStore {
    pub fn new(storage_path: PathBuf) -> Self {
        Self { storage_path }
    }

    /// Save a transcript
    pub fn save(&self, transcript: &Transcript) -> Result<(), TranscriptError> {
        let session_dir = self.storage_path.join(transcript.session_id.to_string());
        std::fs::create_dir_all(&session_dir)
            .map_err(|e| TranscriptError::StorageError(e.to_string()))?;

        let json = serde_json::to_string_pretty(transcript)
            .map_err(|e| TranscriptError::SerializationError(e.to_string()))?;

        let file_path = session_dir.join("transcript.json");
        std::fs::write(file_path, json)
            .map_err(|e| TranscriptError::StorageError(e.to_string()))?;

        Ok(())
    }

    /// Load a transcript
    pub fn load(&self, session_id: &Uuid) -> Result<Option<Transcript>, TranscriptError> {
        let file_path = self.storage_path.join(session_id.to_string()).join("transcript.json");

        if !file_path.exists() {
            return Ok(None);
        }

        let json = std::fs::read_to_string(&file_path)
            .map_err(|e| TranscriptError::StorageError(e.to_string()))?;

        let transcript: Transcript = serde_json::from_str(&json)
            .map_err(|e| TranscriptError::SerializationError(e.to_string()))?;

        Ok(Some(transcript))
    }

    /// List all available transcripts
    pub fn list(&self) -> Result<Vec<Uuid>, TranscriptError> {
        let mut transcripts = Vec::new();

        if !self.storage_path.exists() {
            return Ok(transcripts);
        }

        for entry in std::fs::read_dir(&self.storage_path)
            .map_err(|e| TranscriptError::StorageError(e.to_string()))?
        {
            let entry = entry.map_err(|e| TranscriptError::StorageError(e.to_string()))?;
            if let Ok(session_id) = entry.file_name().to_string_lossy().parse::<Uuid>() {
                transcripts.push(session_id);
            }
        }

        Ok(transcripts)
    }

    /// Delete a transcript
    pub fn delete(&self, session_id: &Uuid) -> Result<(), TranscriptError> {
        let session_dir = self.storage_path.join(session_id.to_string());
        std::fs::remove_dir_all(session_dir)
            .map_err(|e| TranscriptError::StorageError(e.to_string()))?;

        Ok(())
    }
}

/// Transcript errors
#[derive(Debug, thiserror::Error)]
pub enum TranscriptError {
    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Transcript not found: {0}")]
    NotFound(Uuid),
}
