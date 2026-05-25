//! # Tool Result Cache
//!
//! TTL-based cache for read-only tool results. When the LLM makes repeated
//! read-only tool calls (Read, Glob, Grep) with identical inputs, cached
//! results are returned to reduce context window pressure and latency.
//!
//! ## Architecture
//!
//! - [`ToolResultCache`]: Thread-safe concurrent cache backed by `DashMap`
//! - [`ToolCacheConfig`]: Cache settings (max entries, TTL, enabled toggle)
//!
//! ## Invalidation
//!
//! - **TTL**: Entries expire after a configurable duration (default: 5 minutes)
//! - **Path-based**: When source files change, entries referencing those paths
//!   are invalidated via [`ToolResultCache::invalidate_path`]
//! - **Full clear**: On branch switch or major context changes via
//!   [`ToolResultCache::invalidate_all`]

use dashmap::DashMap;
use serde_json::Value;
use std::time::{Duration, Instant};

use crate::tools::ToolOutput;

// ---------------------------------------------------------------------------
// Cache entry
// ---------------------------------------------------------------------------

/// A cached tool result entry with metadata for TTL and path-based invalidation.
struct CacheEntry {
    /// The cached tool output.
    output: ToolOutput,
    /// When this entry was cached.
    cached_at: Instant,
    /// File paths referenced by this tool call (for invalidation).
    /// For Read tool: `input["file_path"]`
    /// For Glob tool: `input["path"]`
    /// For Grep tool: `input["path"]`
    file_paths: Vec<String>,
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the tool result cache.
#[derive(Debug, Clone)]
pub struct ToolCacheConfig {
    /// Maximum number of entries in the cache. Default: 200.
    pub max_entries: usize,
    /// TTL for cached entries. Default: 5 minutes.
    pub ttl: Duration,
    /// Whether caching is enabled. Default: true.
    pub enabled: bool,
}

impl Default for ToolCacheConfig {
    fn default() -> Self {
        Self {
            max_entries: 200,
            ttl: Duration::from_secs(300),
            enabled: true,
        }
    }
}

impl ToolCacheConfig {
    /// Create a disabled config (all operations are no-ops).
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Self::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Cache
// ---------------------------------------------------------------------------

/// Thread-safe tool result cache for read-only tools.
///
/// Uses `DashMap` for lock-free concurrent access. All operations are O(1)
/// amortized except for path-based invalidation which is O(n).
pub struct ToolResultCache {
    entries: DashMap<String, CacheEntry>,
    config: ToolCacheConfig,
}

impl ToolResultCache {
    /// Create a new cache with the given configuration.
    pub fn new(config: ToolCacheConfig) -> Self {
        Self {
            entries: DashMap::with_capacity(config.max_entries),
            config,
        }
    }

    /// Create a cache with default configuration.
    pub fn with_default() -> Self {
        Self::new(ToolCacheConfig::default())
    }

    /// Generate a deterministic cache key from tool name and input.
    ///
    /// Format: `{tool_name}:{serde_json_sorted_string}`
    fn cache_key(tool_name: &str, input: &Value) -> String {
        // Use sorted JSON keys for deterministic serialization
        let sorted = Self::sort_json_keys(input);
        format!("{tool_name}:{sorted}")
    }

    /// Sort JSON object keys recursively for deterministic serialization.
    fn sort_json_keys(value: &Value) -> String {
        match value {
            Value::Object(map) => {
                let mut pairs: Vec<(String, Value)> =
                    map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                pairs.sort_by(|a, b| a.0.cmp(&b.0));
                let inner: Vec<String> = pairs
                    .iter()
                    .map(|(k, v)| format!("{}:{}", k, Self::sort_json_keys(v)))
                    .collect();
                format!("{{{}}}", inner.join(","))
            }
            Value::Array(arr) => {
                let items: Vec<String> = arr.iter().map(Self::sort_json_keys).collect();
                format!("[{}]", items.join(","))
            }
            Value::String(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
            _ => value.to_string(),
        }
    }

    /// Look up a cached result. Returns `None` if not found or expired.
    pub fn get(&self, tool_name: &str, input: &Value) -> Option<ToolOutput> {
        if !self.config.enabled {
            return None;
        }

        let key = Self::cache_key(tool_name, input);
        if let Some(entry) = self.entries.get(&key) {
            if entry.cached_at.elapsed() < self.config.ttl {
                return Some(entry.output.clone());
            }
            // Entry expired — remove it
            drop(entry);
            self.entries.remove(&key);
        }
        None
    }

    /// Store a tool result in the cache.
    ///
    /// Only caches if the tool is read-only and the result is not an error.
    pub fn put(&self, tool_name: &str, input: &Value, output: &ToolOutput, is_read_only: bool) {
        if !self.config.enabled {
            return;
        }

        // Only cache read-only, successful results
        if !is_read_only || output.is_error {
            return;
        }

        let key = Self::cache_key(tool_name, input);

        // If cache is full, evict expired entries first, then oldest
        if self.entries.len() >= self.config.max_entries {
            self.evict_expired();
            if self.entries.len() >= self.config.max_entries {
                // Remove the oldest entry
                let mut oldest_key: Option<String> = None;
                let mut oldest_time = Instant::now();
                for entry in self.entries.iter() {
                    if entry.cached_at < oldest_time {
                        oldest_time = entry.cached_at;
                        oldest_key = Some(entry.key().clone());
                    }
                }
                if let Some(key) = oldest_key {
                    self.entries.remove(&key);
                }
            }
        }

        let file_paths = Self::extract_file_paths(tool_name, input);
        self.entries.insert(
            key,
            CacheEntry {
                output: output.clone(),
                cached_at: Instant::now(),
                file_paths,
            },
        );
    }

    /// Extract file paths from tool input for invalidation tracking.
    ///
    /// - Read tool: looks at `input["file_path"]`
    /// - Glob tool: looks at `input["path"]`
    /// - Grep tool: looks at `input["path"]`
    fn extract_file_paths(tool_name: &str, input: &Value) -> Vec<String> {
        let mut paths = Vec::new();
        if let Some(obj) = input.as_object() {
            match tool_name {
                "Read" | "read" => {
                    if let Some(v) = obj.get("file_path").and_then(|v| v.as_str()) {
                        paths.push(v.to_string());
                    }
                }
                "Glob" | "glob" | "Grep" | "grep" => {
                    if let Some(v) = obj.get("path").and_then(|v| v.as_str()) {
                        paths.push(v.to_string());
                    }
                }
                _ => {
                    // For other tools, try common path fields
                    for key in &["file_path", "path", "filePath"] {
                        if let Some(v) = obj.get(*key).and_then(|v| v.as_str()) {
                            paths.push(v.to_string());
                        }
                    }
                }
            }
        }
        paths
    }

    /// Invalidate all cache entries that reference the given file path.
    ///
    /// This is called when a source file changes so stale cached results
    /// are not returned.
    pub fn invalidate_path(&self, file_path: &str) {
        self.entries
            .retain(|_, entry| !entry.file_paths.iter().any(|p| p == file_path));
    }

    /// Invalidate all cache entries (e.g., on branch switch).
    pub fn invalidate_all(&self) {
        self.entries.clear();
    }

    /// Remove expired entries. Called periodically or when cache is full.
    pub fn evict_expired(&self) {
        let ttl = self.config.ttl;
        self.entries
            .retain(|_, entry| entry.cached_at.elapsed() < ttl);
    }

    /// Return the number of cached entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Return whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Return the cache configuration.
    pub fn config(&self) -> &ToolCacheConfig {
        &self.config
    }

    /// Check whether caching is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_output(content: &str, is_error: bool) -> ToolOutput {
        ToolOutput {
            content: content.to_string(),
            is_error,
            metadata: Default::default(),
        }
    }

    // -- Test helper: create a cache with a short TTL for expiration tests --

    /// A cache entry with a manually set `cached_at` for testing TTL expiration.
    /// We insert directly into the DashMap to control the timestamp.
    fn insert_with_age(
        cache: &ToolResultCache,
        tool_name: &str,
        input: &Value,
        output: &ToolOutput,
        age: Duration,
    ) {
        let key = ToolResultCache::cache_key(tool_name, input);
        let file_paths = ToolResultCache::extract_file_paths(tool_name, input);
        cache.entries.insert(
            key,
            CacheEntry {
                output: output.clone(),
                cached_at: Instant::now() - age,
                file_paths,
            },
        );
    }

    // 1. Cache miss on empty cache
    #[test]
    fn test_cache_miss() {
        let cache = ToolResultCache::with_default();
        let result = cache.get("Read", &json!({"file_path": "/tmp/test.rs"}));
        assert!(result.is_none());
    }

    // 2. Store and retrieve
    #[test]
    fn test_cache_put_and_get() {
        let cache = ToolResultCache::with_default();
        let input = json!({"file_path": "/tmp/test.rs"});
        let output = make_output("fn main() {}", false);

        cache.put("Read", &input, &output, true);

        let result = cache.get("Read", &input);
        assert!(result.is_some());
        let cached = result.unwrap();
        assert_eq!(cached.content, "fn main() {}");
        assert!(!cached.is_error);
    }

    // 3. Error results are not cached
    #[test]
    fn test_cache_does_not_store_errors() {
        let cache = ToolResultCache::with_default();
        let input = json!({"file_path": "/tmp/missing.rs"});
        let output = make_output("File not found", true);

        cache.put("Read", &input, &output, true);

        let result = cache.get("Read", &input);
        assert!(result.is_none());
    }

    // 4. Non-read-only results are not cached
    #[test]
    fn test_cache_does_not_store_non_readonly() {
        let cache = ToolResultCache::with_default();
        let input = json!({"file_path": "/tmp/test.rs", "content": "hello"});
        let output = make_output("Written successfully", false);

        cache.put("Write", &input, &output, false);

        let result = cache.get("Write", &input);
        assert!(result.is_none());
    }

    // 5. TTL expiration
    #[test]
    fn test_cache_ttl_expiration() {
        let mut config = ToolCacheConfig::default();
        config.ttl = Duration::from_secs(300);
        let cache = ToolResultCache::new(config);

        let input = json!({"file_path": "/tmp/test.rs"});
        let output = make_output("fn main() {}", false);

        // Insert with an age of 600 seconds (beyond the 300s TTL)
        insert_with_age(&cache, "Read", &input, &output, Duration::from_secs(600));

        let result = cache.get("Read", &input);
        assert!(result.is_none(), "Expired entry should not be returned");
    }

    // 6. Path-based invalidation
    #[test]
    fn test_cache_invalidate_path() {
        let cache = ToolResultCache::with_default();

        let input1 = json!({"file_path": "/tmp/test.rs"});
        let output1 = make_output("file 1", false);
        cache.put("Read", &input1, &output1, true);

        let input2 = json!({"file_path": "/tmp/other.rs"});
        let output2 = make_output("file 2", false);
        cache.put("Read", &input2, &output2, true);

        assert_eq!(cache.len(), 2);

        // Invalidate entries referencing /tmp/test.rs
        cache.invalidate_path("/tmp/test.rs");

        assert!(
            cache.get("Read", &input1).is_none(),
            "Invalidated entry should be gone"
        );
        assert!(
            cache.get("Read", &input2).is_some(),
            "Non-matching entry should remain"
        );
    }

    // 7. Invalidate all
    #[test]
    fn test_cache_invalidate_all() {
        let cache = ToolResultCache::with_default();

        for i in 0..5 {
            let input = json!({"file_path": format!("/tmp/file{i}.rs")});
            let output = make_output(&format!("content {i}"), false);
            cache.put("Read", &input, &output, true);
        }

        assert_eq!(cache.len(), 5);

        cache.invalidate_all();

        assert!(cache.is_empty());
    }

    // 8. Evict expired entries
    #[test]
    fn test_cache_evict_expired() {
        let cache = ToolResultCache::with_default();

        // Insert a fresh entry
        let fresh_input = json!({"file_path": "/tmp/fresh.rs"});
        let fresh_output = make_output("fresh", false);
        cache.put("Read", &fresh_input, &fresh_output, true);

        // Insert an expired entry directly
        let old_input = json!({"file_path": "/tmp/old.rs"});
        let old_output = make_output("old", false);
        insert_with_age(
            &cache,
            "Read",
            &old_input,
            &old_output,
            Duration::from_secs(600),
        );

        assert_eq!(cache.len(), 2);

        cache.evict_expired();

        assert_eq!(cache.len(), 1);
        assert!(cache.get("Read", &fresh_input).is_some());
        assert!(cache.get("Read", &old_input).is_none());
    }

    // 9. Max entries eviction (oldest removed when full)
    #[test]
    fn test_cache_max_entries() {
        let mut config = ToolCacheConfig::default();
        config.max_entries = 3;
        let cache = ToolResultCache::new(config);

        // Fill cache to max
        for i in 0..3 {
            let input = json!({"file_path": format!("/tmp/file{i}.rs")});
            let output = make_output(&format!("content {i}"), false);
            cache.put("Read", &input, &output, true);
        }

        assert_eq!(cache.len(), 3);

        // Add one more — should trigger eviction of oldest
        let new_input = json!({"file_path": "/tmp/new.rs"});
        let new_output = make_output("new", false);
        cache.put("Read", &new_input, &new_output, true);

        assert!(
            cache.get("Read", &new_input).is_some(),
            "New entry should be cached"
        );

        // The total should still be <= max_entries (oldest was evicted)
        assert!(
            cache.len() <= 3,
            "Cache should not exceed max_entries, got {}",
            cache.len()
        );
    }

    // 10. Deterministic cache keys
    #[test]
    fn test_cache_key_deterministic() {
        let input1 = json!({"file_path": "/tmp/test.rs", "offset": 10});
        let input2 = json!({"offset": 10, "file_path": "/tmp/test.rs"});

        let key1 = ToolResultCache::cache_key("Read", &input1);
        let key2 = ToolResultCache::cache_key("Read", &input2);

        assert_eq!(
            key1, key2,
            "Same logical input with different key order should produce same cache key"
        );
    }

    // 11. Disabled cache
    #[test]
    fn test_cache_disabled() {
        let cache = ToolResultCache::new(ToolCacheConfig::disabled());

        let input = json!({"file_path": "/tmp/test.rs"});
        let output = make_output("content", false);

        // put should be a no-op
        cache.put("Read", &input, &output, true);
        assert!(cache.is_empty());

        // get should return None
        assert!(cache.get("Read", &input).is_none());
    }

    // -- Additional coverage tests --

    #[test]
    fn test_cache_different_tools_same_input() {
        let cache = ToolResultCache::with_default();
        let input = json!({"path": "/tmp"});

        let read_output = make_output("read result", false);
        let glob_output = make_output("glob result", false);

        cache.put("Read", &input, &read_output, true);
        cache.put("Glob", &input, &glob_output, true);

        let r = cache.get("Read", &input).unwrap();
        assert_eq!(r.content, "read result");

        let g = cache.get("Glob", &input).unwrap();
        assert_eq!(g.content, "glob result");
    }

    #[test]
    fn test_cache_different_inputs_same_tool() {
        let cache = ToolResultCache::with_default();

        let input1 = json!({"file_path": "/tmp/a.rs"});
        let output1 = make_output("a", false);
        cache.put("Read", &input1, &output1, true);

        let input2 = json!({"file_path": "/tmp/b.rs"});
        let output2 = make_output("b", false);
        cache.put("Read", &input2, &output2, true);

        assert_eq!(cache.get("Read", &input1).unwrap().content, "a");
        assert_eq!(cache.get("Read", &input2).unwrap().content, "b");
    }

    #[test]
    fn test_cache_overwrite_on_put() {
        let cache = ToolResultCache::with_default();
        let input = json!({"file_path": "/tmp/test.rs"});

        let output1 = make_output("version 1", false);
        cache.put("Read", &input, &output1, true);

        let output2 = make_output("version 2", false);
        cache.put("Read", &input, &output2, true);

        let result = cache.get("Read", &input).unwrap();
        assert_eq!(result.content, "version 2");
    }

    #[test]
    fn test_extract_file_paths_read() {
        let input = json!({"file_path": "/tmp/test.rs"});
        let paths = ToolResultCache::extract_file_paths("Read", &input);
        assert_eq!(paths, vec!["/tmp/test.rs"]);
    }

    #[test]
    fn test_extract_file_paths_glob() {
        let input = json!({"path": "/tmp/project"});
        let paths = ToolResultCache::extract_file_paths("Glob", &input);
        assert_eq!(paths, vec!["/tmp/project"]);
    }

    #[test]
    fn test_extract_file_paths_grep() {
        let input = json!({"path": "/tmp/project", "pattern": "TODO"});
        let paths = ToolResultCache::extract_file_paths("Grep", &input);
        assert_eq!(paths, vec!["/tmp/project"]);
    }

    #[test]
    fn test_extract_file_paths_unknown_tool() {
        let input = json!({"file_path": "/tmp/test.rs", "path": "/tmp/other"});
        let paths = ToolResultCache::extract_file_paths("Custom", &input);
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&"/tmp/test.rs".to_string()));
        assert!(paths.contains(&"/tmp/other".to_string()));
    }

    #[test]
    fn test_extract_file_paths_no_path() {
        let input = json!({"pattern": "TODO"});
        let paths = ToolResultCache::extract_file_paths("Grep", &input);
        assert!(paths.is_empty());
    }

    #[test]
    fn test_invalidate_path_with_glob_entry() {
        let cache = ToolResultCache::with_default();
        let input = json!({"path": "/tmp/project"});
        let output = make_output("found files", false);
        cache.put("Glob", &input, &output, true);

        cache.invalidate_path("/tmp/project");
        assert!(cache.get("Glob", &input).is_none());
    }

    #[test]
    fn test_invalidate_path_does_not_affect_unrelated() {
        let cache = ToolResultCache::with_default();

        let input1 = json!({"file_path": "/tmp/a.rs"});
        cache.put("Read", &input1, &make_output("a", false), true);

        let input2 = json!({"file_path": "/tmp/b.rs"});
        cache.put("Read", &input2, &make_output("b", false), true);

        cache.invalidate_path("/tmp/c.rs");

        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn test_config_disabled() {
        let config = ToolCacheConfig::disabled();
        assert!(!config.enabled);
        assert_eq!(config.max_entries, 200);
    }

    #[test]
    fn test_is_enabled() {
        let enabled = ToolResultCache::with_default();
        assert!(enabled.is_enabled());

        let disabled = ToolResultCache::new(ToolCacheConfig::disabled());
        assert!(!disabled.is_enabled());
    }

    #[test]
    fn test_sort_json_keys_nested() {
        let input = json!({"z": 1, "a": {"c": 3, "b": 2}});
        let sorted = ToolResultCache::sort_json_keys(&input);
        assert!(sorted.starts_with("{a:"));
        assert!(sorted.contains("{b:2,c:3}"));
    }

    #[test]
    fn test_sort_json_keys_array() {
        let input = json!([3, 1, 2]);
        let sorted = ToolResultCache::sort_json_keys(&input);
        assert_eq!(sorted, "[3,1,2]");
    }

    #[test]
    fn test_sort_json_keys_string_escaping() {
        let input = json!({"key": "value with \"quotes\""});
        let sorted = ToolResultCache::sort_json_keys(&input);
        assert!(sorted.contains(r#"value with \"quotes\""#));
    }

    #[test]
    fn test_cache_concurrent_access() {
        use std::sync::Arc;
        use std::thread;

        let cache = Arc::new(ToolResultCache::with_default());
        let mut handles = Vec::new();

        for i in 0..4 {
            let c = cache.clone();
            handles.push(thread::spawn(move || {
                let input = json!({"file_path": format!("/tmp/thread_{i}.rs")});
                let output = make_output(&format!("content {i}"), false);
                c.put("Read", &input, &output, true);
                let result = c.get("Read", &input);
                assert!(result.is_some());
                assert_eq!(result.unwrap().content, format!("content {i}"));
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(cache.len(), 4);
    }

    #[test]
    fn test_cache_len_and_is_empty() {
        let cache = ToolResultCache::with_default();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);

        let input = json!({"file_path": "/tmp/test.rs"});
        cache.put("Read", &input, &make_output("ok", false), true);

        assert!(!cache.is_empty());
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_config_accessor() {
        let cache = ToolResultCache::with_default();
        let config = cache.config();
        assert!(config.enabled);
        assert_eq!(config.max_entries, 200);
        assert_eq!(config.ttl, Duration::from_secs(300));
    }

    #[test]
    fn test_cache_with_custom_config() {
        let mut config = ToolCacheConfig::default();
        config.max_entries = 10;
        config.ttl = Duration::from_secs(60);
        let cache = ToolResultCache::new(config);

        let input = json!({"file_path": "/tmp/test.rs"});
        let output = make_output("content", false);
        cache.put("Read", &input, &output, true);

        let result = cache.get("Read", &input);
        assert!(result.is_some());

        // Expired entry after 60s TTL
        insert_with_age(
            &cache,
            "Read",
            &json!({"file_path": "/tmp/old.rs"}),
            &make_output("old", false),
            Duration::from_secs(120),
        );
        assert!(
            cache
                .get("Read", &json!({"file_path": "/tmp/old.rs"}))
                .is_none()
        );
    }

    #[test]
    fn test_evict_expired_keeps_fresh() {
        let cache = ToolResultCache::with_default();

        // Fresh entry
        let fresh_input = json!({"file_path": "/tmp/fresh.rs"});
        cache.put("Read", &fresh_input, &make_output("fresh", false), true);

        // Expired entry
        insert_with_age(
            &cache,
            "Read",
            &json!({"file_path": "/tmp/old.rs"}),
            &make_output("old", false),
            Duration::from_secs(600),
        );

        assert_eq!(cache.len(), 2);
        cache.evict_expired();
        assert_eq!(cache.len(), 1);
        assert!(cache.get("Read", &fresh_input).is_some());
    }

    #[test]
    fn test_put_then_invalidate_all_then_get() {
        let cache = ToolResultCache::with_default();
        let input = json!({"file_path": "/tmp/test.rs"});
        cache.put("Read", &input, &make_output("content", false), true);
        assert!(cache.get("Read", &input).is_some());

        cache.invalidate_all();
        assert!(cache.get("Read", &input).is_none());
    }

    #[test]
    fn test_max_entries_eviction_removes_oldest() {
        let mut config = ToolCacheConfig::default();
        config.max_entries = 2;
        let cache = ToolResultCache::new(config);

        // Insert first (oldest)
        let input1 = json!({"file_path": "/tmp/oldest.rs"});
        cache.put("Read", &input1, &make_output("oldest", false), true);

        // Insert second
        let input2 = json!({"file_path": "/tmp/middle.rs"});
        cache.put("Read", &input2, &make_output("middle", false), true);

        assert_eq!(cache.len(), 2);

        // Insert third — should evict oldest
        let input3 = json!({"file_path": "/tmp/newest.rs"});
        cache.put("Read", &input3, &make_output("newest", false), true);

        assert!(cache.len() <= 2);
        assert!(
            cache.get("Read", &input3).is_some(),
            "newest should survive"
        );
    }
}
