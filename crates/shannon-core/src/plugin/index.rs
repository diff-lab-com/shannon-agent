//! Remote plugin index

use crate::plugin::{PluginError, PluginResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Remote plugin index
#[derive(Debug, Clone)]
pub struct PluginIndex {
    /// Index URL
    url: String,

    /// Cached entries
    cache: HashMap<String, IndexEntry>,
}

/// Entry in the plugin index
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    /// Plugin name
    pub name: String,

    /// Short description
    pub description: String,

    /// Author
    pub author: String,

    /// Repository URL
    pub repository: String,

    /// Latest version
    pub latest_version: String,

    /// Plugin type
    pub plugin_type: String,

    /// Download count
    pub downloads: u64,

    /// Keywords for search
    #[serde(default)]
    pub keywords: Vec<String>,
}

impl PluginIndex {
    /// Create a new plugin index
    pub fn new(url: String) -> Self {
        Self {
            url,
            cache: HashMap::new(),
        }
    }

    /// Get the index URL
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Search the index for plugins matching a query
    pub fn search(&self, query: &str) -> Vec<&IndexEntry> {
        let query_lower = query.to_lowercase();

        self.cache
            .values()
            .filter(|entry| {
                let name_matches = entry.name.to_lowercase().contains(&query_lower);
                let desc_matches = entry.description.to_lowercase().contains(&query_lower);
                let author_matches = entry.author.to_lowercase().contains(&query_lower);
                let keyword_matches = entry.keywords.iter().any(|k| {
                    k.to_lowercase().contains(&query_lower)
                });

                name_matches || desc_matches || author_matches || keyword_matches
            })
            .collect()
    }

    /// Get an entry by name
    pub fn get(&self, name: &str) -> Option<&IndexEntry> {
        self.cache.get(name)
    }

    /// Load index entries from JSON
    pub fn load_from_json(&mut self, json: &str) -> PluginResult<()> {
        let entries: Vec<IndexEntry> = serde_json::from_str(json)
            .map_err(|e| PluginError::IndexRefreshFailed(e.to_string()))?;

        self.cache.clear();
        for entry in entries {
            self.cache.insert(entry.name.clone(), entry);
        }

        Ok(())
    }

    /// Get all cached entries
    pub fn all_entries(&self) -> Vec<&IndexEntry> {
        self.cache.values().collect()
    }

    /// Get the number of entries in the cache
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Check if the cache is empty
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_INDEX: &str = r#"
[
    {
        "name": "example-plugin",
        "description": "An example plugin",
        "author": "Shannon Team",
        "repository": "https://github.com/shannon-code/example-plugin",
        "latest_version": "1.0.0",
        "plugin_type": "tool",
        "downloads": 1000,
        "keywords": ["example", "demo"]
    },
    {
        "name": "another-plugin",
        "description": "Another plugin for testing",
        "author": "Test Author",
        "repository": "https://github.com/test/another-plugin",
        "latest_version": "0.5.0",
        "plugin_type": "command",
        "downloads": 500,
        "keywords": ["test"]
    }
]
"#;

    #[test]
    fn test_load_index() {
        let mut index = PluginIndex::new("https://example.com/index.json".to_string());
        index.load_from_json(SAMPLE_INDEX).unwrap();

        assert_eq!(index.len(), 2);
        assert!(index.get("example-plugin").is_some());
        assert!(index.get("another-plugin").is_some());
        assert!(index.get("nonexistent").is_none());
    }

    #[test]
    fn test_search() {
        let mut index = PluginIndex::new("https://example.com/index.json".to_string());
        index.load_from_json(SAMPLE_INDEX).unwrap();

        let results = index.search("example");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "example-plugin");

        let results = index.search("Shannon");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "example-plugin");
    }

    #[test]
    fn test_get_entry() {
        let mut index = PluginIndex::new("https://example.com/index.json".to_string());
        index.load_from_json(SAMPLE_INDEX).unwrap();

        let entry = index.get("example-plugin").unwrap();
        assert_eq!(entry.name, "example-plugin");
        assert_eq!(entry.latest_version, "1.0.0");
        assert_eq!(entry.plugin_type, "tool");
    }
}
