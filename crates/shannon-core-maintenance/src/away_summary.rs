//! Away summary service

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// Away summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwaySummary {
    pub id: Uuid,
    pub start_time: chrono::DateTime<chrono::Utc>,
    pub end_time: chrono::DateTime<chrono::Utc>,
    pub activities_count: usize,
    pub summary: String,
    pub key_points: Vec<String>,
}

impl AwaySummary {
    pub fn new(start_time: chrono::DateTime<chrono::Utc>) -> Self {
        Self {
            id: Uuid::new_v4(),
            start_time,
            end_time: start_time,
            activities_count: 0,
            summary: String::new(),
            key_points: Vec::new(),
        }
    }

    pub fn with_end_time(mut self, end_time: chrono::DateTime<chrono::Utc>) -> Self {
        self.end_time = end_time;
        self
    }

    pub fn with_activities_count(mut self, count: usize) -> Self {
        self.activities_count = count;
        self
    }

    pub fn with_summary(mut self, summary: String) -> Self {
        self.summary = summary;
        self
    }

    pub fn with_key_points(mut self, key_points: Vec<String>) -> Self {
        self.key_points = key_points;
        self
    }
}

/// Away summary service
pub struct AwaySummaryService {
    storage_path: PathBuf,
}

impl AwaySummaryService {
    pub fn new(storage_path: PathBuf) -> Self {
        Self { storage_path }
    }

    /// Generate an away summary
    pub async fn generate_summary(&self, activities: Vec<String>) -> Result<AwaySummary, SummaryError> {
        let start_time = chrono::Utc::now() - chrono::Duration::hours(1);
        let end_time = chrono::Utc::now();

        // TODO: Implement actual summary generation with AI
        let summary = format!("Session summary: {} activities", activities.len());
        let activities_count = activities.len();
        let key_points = activities;

        Ok(AwaySummary {
            id: Uuid::new_v4(),
            start_time,
            end_time,
            activities_count,
            summary,
            key_points,
        })
    }

    /// Save a summary
    pub async fn save_summary(&self, summary: &AwaySummary) -> Result<(), SummaryError> {
        std::fs::create_dir_all(&self.storage_path)
            .map_err(|e| SummaryError::StorageError(e.to_string()))?;

        let file_path = self.storage_path.join(format!("{}.json", summary.id));
        let json = serde_json::to_string_pretty(summary)
            .map_err(|e| SummaryError::SerializationError(e.to_string()))?;

        std::fs::write(file_path, json)
            .map_err(|e| SummaryError::StorageError(e.to_string()))?;

        Ok(())
    }

    /// Load a summary
    pub async fn load_summary(&self, id: &Uuid) -> Result<Option<AwaySummary>, SummaryError> {
        let file_path = self.storage_path.join(format!("{}.json", id));

        if !file_path.exists() {
            return Ok(None);
        }

        let json = std::fs::read_to_string(&file_path)
            .map_err(|e| SummaryError::StorageError(e.to_string()))?;

        let summary: AwaySummary = serde_json::from_str(&json)
            .map_err(|e| SummaryError::SerializationError(e.to_string()))?;

        Ok(Some(summary))
    }

    /// List all summaries
    pub async fn list_summaries(&self) -> Result<Vec<AwaySummary>, SummaryError> {
        let mut summaries = Vec::new();

        if !self.storage_path.exists() {
            return Ok(summaries);
        }

        for entry in std::fs::read_dir(&self.storage_path)
            .map_err(|e| SummaryError::StorageError(e.to_string()))?
        {
            let entry = entry.map_err(|e| SummaryError::StorageError(e.to_string()))?;
            let file_path = entry.path();

            if file_path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }

            let json = std::fs::read_to_string(&file_path)
                .map_err(|e| SummaryError::StorageError(e.to_string()))?;

            if let Ok(summary) = serde_json::from_str(&json) {
                summaries.push(summary);
            }
        }

        Ok(summaries)
    }
}

/// Summary errors
#[derive(Debug, thiserror::Error)]
pub enum SummaryError {
    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Generation failed: {0}")]
    GenerationFailed(String),
}
