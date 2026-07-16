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

    /// When the cache was last refreshed
    last_refreshed: Option<chrono::DateTime<chrono::Utc>>,
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
            last_refreshed: None,
        }
    }

    /// Get the index URL
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Fetch and refresh the index from the remote URL.
    pub async fn refresh(&mut self) -> PluginResult<()> {
        let response = reqwest::get(&self.url)
            .await
            .map_err(|e| PluginError::IndexRefreshFailed(e.to_string()))?;
        let json = response
            .text()
            .await
            .map_err(|e| PluginError::IndexRefreshFailed(e.to_string()))?;
        self.load_from_json(&json)?;
        self.last_refreshed = Some(chrono::Utc::now());
        Ok(())
    }

    /// When the index was last refreshed from the remote URL.
    pub fn last_refreshed(&self) -> Option<&chrono::DateTime<chrono::Utc>> {
        self.last_refreshed.as_ref()
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
                let keyword_matches = entry
                    .keywords
                    .iter()
                    .any(|k| k.to_lowercase().contains(&query_lower));

                name_matches || desc_matches || author_matches || keyword_matches
            })
            .collect()
    }

    /// Search with relevance scoring.
    ///
    /// Returns entries sorted by descending score. Scoring weights:
    /// - Name match: 3x (exact=30, substring=15, word=6)
    /// - Keyword match: 2x (exact=10, substring=4)
    /// - Description match: 1x
    /// - Author match: 0.5x
    pub fn search_ranked(&self, query: &str) -> Vec<(f64, &IndexEntry)> {
        let query_lower = query.to_lowercase();

        let mut scored: Vec<(f64, &IndexEntry)> = self
            .cache
            .values()
            .filter_map(|entry| {
                let name_lower = entry.name.to_lowercase();
                let desc_lower = entry.description.to_lowercase();
                let author_lower = entry.author.to_lowercase();

                // Name scoring (3x weight)
                let name_score = if name_lower == query_lower {
                    3.0 * 10.0
                } else if name_lower.contains(&query_lower) {
                    3.0 * 5.0
                } else if query_lower
                    .split_whitespace()
                    .all(|w| name_lower.contains(w))
                {
                    3.0 * 2.0
                } else {
                    0.0
                };

                // Keyword scoring (2x weight)
                let keyword_score: f64 = entry
                    .keywords
                    .iter()
                    .map(|k| {
                        let k_lower = k.to_lowercase();
                        if k_lower == query_lower {
                            2.0 * 5.0
                        } else if k_lower.contains(&query_lower) {
                            2.0 * 2.0
                        } else {
                            0.0
                        }
                    })
                    .sum();

                // Description scoring (1x weight)
                let desc_score = if desc_lower.contains(&query_lower) {
                    1.0
                } else {
                    0.0
                };

                // Author scoring (0.5x weight)
                let author_score = if author_lower.contains(&query_lower) {
                    0.5
                } else {
                    0.0
                };

                let total = name_score + keyword_score + desc_score + author_score;

                if total > 0.0 {
                    Some((total, entry))
                } else {
                    None
                }
            })
            .collect();

        // Sort by score descending, then by name for stability
        scored.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.1.name.cmp(&b.1.name))
        });

        scored
    }

    /// Get an entry by name
    pub fn get(&self, name: &str) -> Option<&IndexEntry> {
        self.cache.get(name)
    }

    /// Get detailed info about a specific plugin.
    pub fn info(&self, name: &str) -> Option<&IndexEntry> {
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
#[allow(clippy::unwrap_used)]
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

    #[test]
    fn test_search_ranked_exact_name_match_highest() {
        let mut index = PluginIndex::new("https://example.com/index.json".to_string());
        index.load_from_json(SAMPLE_INDEX).unwrap();

        let results = index.search_ranked("example-plugin");
        assert_eq!(results.len(), 1); // only exact name match

        // Exact name match should be first
        assert_eq!(results[0].1.name, "example-plugin");
        let exact_score = results[0].0;
        assert!(exact_score >= 30.0); // 3x * 10 for exact name
    }

    #[test]
    fn test_search_ranked_keyword_match() {
        let mut index = PluginIndex::new("https://example.com/index.json".to_string());
        index.load_from_json(SAMPLE_INDEX).unwrap();

        let results = index.search_ranked("demo");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.name, "example-plugin");
        // "demo" is a keyword, so score should include keyword weight
        assert!(results[0].0 > 0.0);
    }

    #[test]
    fn test_search_ranked_author_match() {
        let mut index = PluginIndex::new("https://example.com/index.json".to_string());
        index.load_from_json(SAMPLE_INDEX).unwrap();

        let results = index.search_ranked("Shannon");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.name, "example-plugin");
    }

    #[test]
    fn test_search_ranked_no_results() {
        let mut index = PluginIndex::new("https://example.com/index.json".to_string());
        index.load_from_json(SAMPLE_INDEX).unwrap();

        let results = index.search_ranked("nonexistent-xyz");
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_ranked_sorted_by_score() {
        let mut index = PluginIndex::new("https://example.com/index.json".to_string());
        let custom_json = r#"
[
    {
        "name": "test-tool",
        "description": "A test tool plugin",
        "author": "Author",
        "repository": "https://github.com/test/test-tool",
        "latest_version": "1.0.0",
        "plugin_type": "tool",
        "downloads": 100,
        "keywords": ["test", "tool"]
    },
    {
        "name": "test-helper",
        "description": "A test helper plugin",
        "author": "Author",
        "repository": "https://github.com/test/test-helper",
        "latest_version": "0.1.0",
        "plugin_type": "command",
        "downloads": 50,
        "keywords": ["test"]
    }
]
"#;
        index.load_from_json(custom_json).unwrap();

        let results = index.search_ranked("test-tool");
        assert!(!results.is_empty());
        // Exact match should be first
        assert_eq!(results[0].1.name, "test-tool");
    }

    #[test]
    fn test_info() {
        let mut index = PluginIndex::new("https://example.com/index.json".to_string());
        index.load_from_json(SAMPLE_INDEX).unwrap();

        let entry = index.info("example-plugin").unwrap();
        assert_eq!(entry.name, "example-plugin");
        assert_eq!(entry.author, "Shannon Team");
        assert!(index.info("nonexistent").is_none());
    }

    #[test]
    fn test_last_refreshed_initially_none() {
        let index = PluginIndex::new("https://example.com/index.json".to_string());
        assert!(index.last_refreshed().is_none());
    }
}
