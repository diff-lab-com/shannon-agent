//! Internal logging system

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// Internal log level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum InternalLogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

/// Internal log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternalLogEntry {
    pub id: Uuid,
    pub level: InternalLogLevel,
    pub target: String,
    pub message: String,
    pub metadata: serde_json::Value,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl InternalLogEntry {
    pub fn new(level: InternalLogLevel, target: String, message: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            level,
            target,
            message,
            metadata: serde_json::Value::Object(serde_json::Map::new()),
            timestamp: chrono::Utc::now(),
        }
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }
}

/// Internal log
pub type InternalLog = Vec<InternalLogEntry>;

/// Internal logger
pub struct InternalLogger {
    log: InternalLog,
    storage_path: Option<PathBuf>,
    max_entries: usize,
}

impl InternalLogger {
    pub fn new() -> Self {
        Self {
            log: Vec::new(),
            storage_path: None,
            max_entries: 10000,
        }
    }

    pub fn with_storage(mut self, path: PathBuf) -> Self {
        self.storage_path = Some(path);
        self
    }

    pub fn with_max_entries(mut self, max: usize) -> Self {
        self.max_entries = max;
        self
    }

    /// Log a message
    pub fn log(&mut self, entry: InternalLogEntry) {
        self.log.push(entry);

        // Maintain max entries limit
        if self.log.len() > self.max_entries {
            self.log.remove(0);
        }
    }

    /// Get all log entries
    pub fn get_entries(&self) -> &[InternalLogEntry] {
        &self.log
    }

    /// Get entries by level
    pub fn get_entries_by_level(&self, level: InternalLogLevel) -> Vec<&InternalLogEntry> {
        self.log.iter().filter(|e| e.level == level).collect()
    }

    /// Get entries by target
    pub fn get_entries_by_target(&self, target: &str) -> Vec<&InternalLogEntry> {
        self.log
            .iter()
            .filter(|e| e.target.contains(target))
            .collect()
    }

    /// Clear all entries
    pub fn clear(&mut self) {
        self.log.clear();
    }

    /// Save to disk
    pub async fn save(&self) -> Result<(), LogError> {
        if let Some(path) = &self.storage_path {
            std::fs::create_dir_all(path)
                .map_err(|e| LogError::StorageError(e.to_string()))?;

            let file_path = path.join(format!("internal_log_{}.json", chrono::Utc::now().timestamp()));
            let json = serde_json::to_string_pretty(&self.log)
                .map_err(|e| LogError::SerializationError(e.to_string()))?;

            std::fs::write(file_path, json)
                .map_err(|e| LogError::StorageError(e.to_string()))?;
        }

        Ok(())
    }

    /// Load from disk
    pub async fn load(&mut self, path: &PathBuf) -> Result<(), LogError> {
        if !path.exists() {
            return Ok(());
        }

        let json = std::fs::read_to_string(path)
            .map_err(|e| LogError::StorageError(e.to_string()))?;

        self.log = serde_json::from_str(&json)
            .map_err(|e| LogError::SerializationError(e.to_string()))?;

        Ok(())
    }
}

impl Default for InternalLogger {
    fn default() -> Self {
        Self::new()
    }
}

/// Log errors
#[derive(Debug, thiserror::Error)]
pub enum LogError {
    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),
}
