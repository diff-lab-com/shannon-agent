//! Memory storage and retrieval

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;

/// Memory entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: Uuid,
    pub key: String,
    pub value: serde_json::Value,
    pub tags: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl Memory {
    pub fn new(key: String, value: serde_json::Value) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: Uuid::new_v4(),
            key,
            value,
            tags: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    pub fn add_tag(&mut self, tag: String) {
        if !self.tags.contains(&tag) {
            self.tags.push(tag);
        }
    }
}

/// Memory search options
#[derive(Debug, Clone, Default)]
pub struct MemorySearchOptions {
    pub tags: Option<Vec<String>>,
    pub key_prefix: Option<String>,
    pub limit: Option<usize>,
    pub newer_than: Option<chrono::DateTime<chrono::Utc>>,
    pub older_than: Option<chrono::DateTime<chrono::Utc>>,
}

/// Memory store
pub struct MemoryStore {
    memories: HashMap<Uuid, Memory>,
    key_index: HashMap<String, Uuid>,
    tag_index: HashMap<String, Vec<Uuid>>,
    storage_path: Option<PathBuf>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self {
            memories: HashMap::new(),
            key_index: HashMap::new(),
            tag_index: HashMap::new(),
            storage_path: None,
        }
    }

    pub fn with_storage_path(mut self, path: PathBuf) -> Self {
        self.storage_path = Some(path);
        self
    }

    /// Add a memory to the store
    pub fn add(&mut self, memory: Memory) -> Result<(), MemoryError> {
        // Remove old memory with same key if exists
        if let Some(old_id) = self.key_index.get(&memory.key) {
            self.remove_by_id(*old_id);
        }

        // Update indexes
        self.key_index.insert(memory.key.clone(), memory.id);
        for tag in &memory.tags {
            self.tag_index.entry(tag.clone()).or_default().push(memory.id);
        }

        // Store memory
        self.memories.insert(memory.id, memory.clone());

        // Persist if storage path is set
        if let Some(path) = &self.storage_path {
            self.persist(&memory)?;
        }

        Ok(())
    }

    /// Get memory by key
    pub fn get(&self, key: &str) -> Option<&Memory> {
        self.key_index.get(key).and_then(|id| self.memories.get(id))
    }

    /// Get memory by ID
    pub fn get_by_id(&self, id: &Uuid) -> Option<&Memory> {
        self.memories.get(id)
    }

    /// Search memories
    pub fn search(&self, options: MemorySearchOptions) -> Vec<&Memory> {
        let mut results: Vec<&Memory> = self.memories.values().collect();

        // Filter by tags
        if let Some(tags) = &options.tags {
            results = results.into_iter().filter(|m| {
                tags.iter().all(|tag| m.tags.contains(tag))
            }).collect();
        }

        // Filter by key prefix
        if let Some(prefix) = &options.key_prefix {
            results = results.into_iter().filter(|m| m.key.starts_with(prefix)).collect();
        }

        // Filter by time range
        if let Some(newer) = options.newer_than {
            results = results.into_iter().filter(|m| m.created_at > newer).collect();
        }
        if let Some(older) = options.older_than {
            results = results.into_iter().filter(|m| m.created_at < older).collect();
        }

        // Limit results
        if let Some(limit) = options.limit {
            results.truncate(limit);
        }

        results
    }

    /// Remove memory by key
    pub fn remove(&mut self, key: &str) -> Option<Memory> {
        if let Some(id) = self.key_index.remove(key) {
            self.remove_by_id(id)
        } else {
            None
        }
    }

    /// Remove memory by ID
    fn remove_by_id(&mut self, id: Uuid) -> Option<Memory> {
        if let Some(memory) = self.memories.remove(&id) {
            // Remove from tag index
            for tag in &memory.tags {
                if let Some(tag_memories) = self.tag_index.get_mut(tag) {
                    tag_memories.retain(|x| *x != id);
                }
            }
            Some(memory)
        } else {
            None
        }
    }

    /// Update memory value
    pub fn update(&mut self, key: &str, value: serde_json::Value) -> Result<(), MemoryError> {
        if let Some(memory) = self.get_mut(key) {
            memory.value = value;
            memory.updated_at = chrono::Utc::now();
            Ok(())
        } else {
            Err(MemoryError::NotFound(key.to_string()))
        }
    }

    fn get_mut(&mut self, key: &str) -> Option<&mut Memory> {
        if let Some(id) = self.key_index.get(key) {
            self.memories.get_mut(id)
        } else {
            None
        }
    }

    /// Persist memory to disk
    fn persist(&self, memory: &Memory) -> Result<(), MemoryError> {
        if let Some(path) = &self.storage_path {
            // Create directory if it doesn't exist
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| MemoryError::StorageError(e.to_string()))?;
            }

            // Serialize and write memory
            let json = serde_json::to_string_pretty(memory)
                .map_err(|e| MemoryError::SerializationError(e.to_string()))?;

            let file_path = path.join(format!("{}.json", memory.id));
            std::fs::write(file_path, json)
                .map_err(|e| MemoryError::StorageError(e.to_string()))?;
        }
        Ok(())
    }

    /// Load memories from disk
    pub fn load_from_disk(&mut self) -> Result<(), MemoryError> {
        if let Some(path) = &self.storage_path {
            if !path.exists() {
                return Ok(());
            }

            let entries = std::fs::read_dir(path)
                .map_err(|e| MemoryError::StorageError(e.to_string()))?;

            for entry in entries {
                let entry = entry.map_err(|e| MemoryError::StorageError(e.to_string()))?;
                let path = entry.path();

                if path.extension().and_then(|s| s.to_str()) != Some("json") {
                    continue;
                }

                let json = std::fs::read_to_string(&path)
                    .map_err(|e| MemoryError::StorageError(e.to_string()))?;

                let memory: Memory = serde_json::from_str(&json)
                    .map_err(|e| MemoryError::SerializationError(e.to_string()))?;

                // Add to store without persisting again
                self.key_index.insert(memory.key.clone(), memory.id);
                for tag in &memory.tags {
                    self.tag_index.entry(tag.clone()).or_default().push(memory.id);
                }
                self.memories.insert(memory.id, memory);
            }
        }

        Ok(())
    }

    /// Get all memories
    pub fn all(&self) -> Vec<&Memory> {
        self.memories.values().collect()
    }

    /// Get memory count
    pub fn count(&self) -> usize {
        self.memories.len()
    }

    /// Clear all memories
    pub fn clear(&mut self) {
        self.memories.clear();
        self.key_index.clear();
        self.tag_index.clear();
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Memory errors
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("Memory not found: {0}")]
    NotFound(String),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Memory already exists: {0}")]
    AlreadyExists(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_creation() {
        let memory = Memory::new("test_key".to_string(), serde_json::json!("test_value"));
        assert_eq!(memory.key, "test_key");
        assert!(memory.tags.is_empty());
    }

    #[test]
    fn test_memory_with_tags() {
        let memory = Memory::new("test_key".to_string(), serde_json::json!("test_value"))
            .with_tags(vec!["tag1".to_string(), "tag2".to_string()]);
        assert_eq!(memory.tags.len(), 2);
    }

    #[test]
    fn test_memory_store_add_get() {
        let mut store = MemoryStore::new();
        let memory = Memory::new("test_key".to_string(), serde_json::json!("test_value"));
        store.add(memory).unwrap();

        let retrieved = store.get("test_key");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().key, "test_key");
    }

    #[test]
    fn test_memory_store_search_by_tags() {
        let mut store = MemoryStore::new();

        let m1 = Memory::new("key1".to_string(), serde_json::json!(1))
            .with_tags(vec!["tag1".to_string()]);
        let m2 = Memory::new("key2".to_string(), serde_json::json!(2))
            .with_tags(vec!["tag2".to_string()]);
        let m3 = Memory::new("key3".to_string(), serde_json::json!(3))
            .with_tags(vec!["tag1".to_string(), "tag2".to_string()]);

        store.add(m1).unwrap();
        store.add(m2).unwrap();
        store.add(m3).unwrap();

        let results = store.search(MemorySearchOptions {
            tags: Some(vec!["tag1".to_string()]),
            ..Default::default()
        });

        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_memory_store_remove() {
        let mut store = MemoryStore::new();
        let memory = Memory::new("test_key".to_string(), serde_json::json!("test_value"));
        store.add(memory).unwrap();

        let removed = store.remove("test_key");
        assert!(removed.is_some());

        let retrieved = store.get("test_key");
        assert!(retrieved.is_none());
    }

    #[test]
    fn test_memory_store_update() {
        let mut store = MemoryStore::new();
        let memory = Memory::new("test_key".to_string(), serde_json::json!("old_value"));
        store.add(memory).unwrap();

        store.update("test_key", serde_json::json!("new_value")).unwrap();

        let retrieved = store.get("test_key");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().value, serde_json::json!("new_value"));
    }
}
