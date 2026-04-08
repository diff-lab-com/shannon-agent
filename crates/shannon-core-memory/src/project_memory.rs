//! Project memory management

use crate::memory::{Memory, MemoryStore, MemoryError};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// Project-specific memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemory {
    pub project_id: Uuid,
    pub project_path: PathBuf,
    pub memories: Vec<Memory>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Project memory manager
pub struct ProjectMemoryManager {
    store: MemoryStore,
    base_path: PathBuf,
}

impl ProjectMemoryManager {
    pub fn new(base_path: PathBuf) -> Self {
        let store = MemoryStore::new()
            .with_storage_path(base_path.join("memories"));

        Self { store, base_path }
    }

    /// Get memory for a specific project
    pub fn get_project_memory(&self, project_id: &Uuid) -> Result<Option<ProjectMemory>, MemoryError> {
        let key = format!("project:{}", project_id);
        if let Some(memory) = self.store.get(&key) {
            let project_memory: ProjectMemory = serde_json::from_value(memory.value.clone())
                .map_err(|e| MemoryError::SerializationError(e.to_string()))?;
            Ok(Some(project_memory))
        } else {
            Ok(None)
        }
    }

    /// Save project memory
    pub fn save_project_memory(&mut self, project_memory: ProjectMemory) -> Result<(), MemoryError> {
        let key = format!("project:{}", project_memory.project_id);
        let value = serde_json::to_value(project_memory)
            .map_err(|e| MemoryError::SerializationError(e.to_string()))?;

        let memory = Memory::new(key, value)
            .with_tags(vec!["project".to_string()]);

        self.store.add(memory)
    }

    /// Get all project memories
    pub fn list_projects(&self) -> Result<Vec<ProjectMemory>, MemoryError> {
        let results = self.store.search(crate::memory::MemorySearchOptions {
            tags: Some(vec!["project".to_string()]),
            ..Default::default()
        });

        results.into_iter().map(|m| {
            serde_json::from_value(m.value.clone())
                .map_err(|e| MemoryError::SerializationError(e.to_string()))
        }).collect()
    }
}
