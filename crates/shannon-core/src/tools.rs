//! # Tool System
//!
//! Dynamic tool registration, execution, and result handling.
//!
//! This module re-exports the core tool trait and types from `shannon_tool_interface`
//! and provides the `ToolRegistry` for managing available tools.

pub use shannon_tool_interface::{
    BoxedProgressSender, ProgressSender, Tool, ToolError, ToolInfo, ToolOutput, ToolResult,
};

use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// A cached tool result with creation timestamp for TTL expiry.
#[derive(Clone)]
struct CachedToolResult {
    output: ToolOutput,
    created_at: std::time::Instant,
}

/// Default cache TTL in seconds (5 minutes).
const DEFAULT_CACHE_TTL_SECS: u64 = 300;

// ---------------------------------------------------------------------------
// Glob-based tool filter
// ---------------------------------------------------------------------------

/// A single compiled allow/deny pattern for tool names.
struct ToolPattern {
    /// Compiled regex derived from a glob pattern.
    regex: Regex,
    /// `true` means this is an exclude (deny) pattern.
    is_exclude: bool,
}

/// A set of compiled allow/deny patterns for tool name filtering.
///
/// Supports glob-style patterns where `*` matches any characters and `?`
/// matches a single character. Patterns prefixed with `!` are deny rules
/// and take precedence over allow rules.
///
/// # Matching logic
///
/// A tool name is **allowed** if:
/// 1. No allow patterns exist (empty filter = allow all), OR
/// 2. It matches at least one allow pattern.
///
/// A tool name is **denied** if:
/// 1. It matches any deny (exclude) pattern, regardless of allow matches.
struct ToolFilter {
    patterns: Vec<ToolPattern>,
}

impl ToolFilter {
    /// Build a filter from a list of glob-style patterns.
    ///
    /// Each pattern may optionally be prefixed with `!` to indicate a deny rule.
    /// Glob syntax: `*` = any chars, `?` = single char.
    fn from_patterns(patterns: Vec<String>) -> Self {
        let compiled: Vec<ToolPattern> = patterns
            .into_iter()
            .filter_map(|p| {
                let (is_exclude, pat) = if let Some(rest) = p.strip_prefix('!') {
                    (true, rest.to_string())
                } else {
                    (false, p)
                };
                glob_to_regex(&pat)
                    .ok()
                    .map(|regex| ToolPattern { regex, is_exclude })
            })
            .collect();
        Self { patterns: compiled }
    }

    /// Returns `true` if the tool name passes the filter.
    fn is_allowed(&self, name: &str) -> bool {
        let has_includes = self.patterns.iter().any(|p| !p.is_exclude);

        // Deny patterns take absolute precedence.
        if self
            .patterns
            .iter()
            .any(|p| p.is_exclude && p.regex.is_match(name))
        {
            return false;
        }

        if !has_includes {
            // No include patterns → everything not denied is allowed.
            return true;
        }

        // Must match at least one include pattern.
        self.patterns
            .iter()
            .any(|p| !p.is_exclude && p.regex.is_match(name))
    }
}

/// Convert a glob pattern to a compiled regex.
///
/// - `*` → `.*`
/// - `?` → `.`
/// - All other regex meta-characters are escaped.
/// - The pattern is anchored (`^...$`).
fn glob_to_regex(pattern: &str) -> Result<Regex, regex::Error> {
    let mut regex_str = String::with_capacity(pattern.len() * 2);
    regex_str.push('^');
    for ch in pattern.chars() {
        match ch {
            '*' => regex_str.push_str(".*"),
            '?' => regex_str.push('.'),
            // Escape regex meta-characters
            '.' | '^' | '$' | '+' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '\\' => {
                regex_str.push('\\');
                regex_str.push(ch);
            }
            _ => regex_str.push(ch),
        }
    }
    regex_str.push('$');
    Regex::new(&regex_str)
}

// ---------------------------------------------------------------------------
// ToolRegistry
// ---------------------------------------------------------------------------

/// Default threshold above which MCP server tools are deferred (not sent to LLM schema).
pub const DEFER_THRESHOLD: usize = 50;

/// Registry for managing available tools
pub struct ToolRegistry {
    tools: std::sync::RwLock<HashMap<String, std::sync::Arc<dyn Tool>>>,
    /// Tool names that are deferred — registered and executable, but excluded from
    /// the JSON schema sent to the LLM.  Discovered on-demand via `ToolSearch`.
    deferred: std::sync::RwLock<HashSet<String>>,
    /// Optional glob-based allow/deny filter for tool access.
    tool_filter: Option<ToolFilter>,
    /// Cache for read-only tool results: (tool_name, input_hash) -> cached output.
    /// Wrapped in Mutex for interior mutability so `execute` can stay `&self`.
    result_cache: std::sync::Mutex<HashMap<(String, u64), CachedToolResult>>,
    /// Insertion-order tracking for FIFO cache eviction.
    cache_order: std::sync::Mutex<std::collections::VecDeque<(String, u64)>>,
    /// Maximum number of cached entries (FIFO eviction when exceeded)
    max_cache_entries: usize,
    /// Cache TTL in seconds. Entries older than this are considered stale.
    cache_ttl_secs: u64,
    /// Schema cache: (version, JSON schema Value). Invalidated on register/unregister.
    schema_cache: std::sync::RwLock<Option<(u64, Value)>>,
    /// Tool definitions cache: (version, Vec<ToolDefinition>). Invalidated on register/unregister.
    defs_cache: std::sync::RwLock<Option<(u64, Vec<crate::api::ToolDefinition>)>>,
    /// Version counter for cache invalidation.
    version: std::sync::atomic::AtomicU64,
    /// Concurrent TTL-based cache for streaming tool results.
    /// Used by `execute_streaming` to avoid re-executing identical read-only calls.
    streaming_cache: Option<std::sync::Arc<crate::tool_cache::ToolResultCache>>,
}

impl ToolRegistry {
    /// Helper to recover from a poisoned lock by extracting the inner value.
    /// This prevents panics when another thread panicked while holding the lock.
    fn recover_lock<T>(lock_result: std::sync::LockResult<T>) -> T {
        shannon_types::recover_lock(lock_result)
    }

    /// Create a new empty tool registry
    pub fn new() -> Self {
        Self {
            tools: std::sync::RwLock::new(HashMap::new()),
            deferred: std::sync::RwLock::new(HashSet::new()),
            tool_filter: None,
            result_cache: std::sync::Mutex::new(HashMap::new()),
            cache_order: std::sync::Mutex::new(std::collections::VecDeque::new()),
            max_cache_entries: 256,
            cache_ttl_secs: DEFAULT_CACHE_TTL_SECS,
            schema_cache: std::sync::RwLock::new(None),
            defs_cache: std::sync::RwLock::new(None),
            version: std::sync::atomic::AtomicU64::new(0),
            streaming_cache: None,
        }
    }

    /// Set the cache TTL in seconds.
    pub fn set_cache_ttl(&mut self, ttl_secs: u64) {
        self.cache_ttl_secs = ttl_secs;
    }

    /// Set the max cache entries.
    pub fn set_max_cache_entries(&mut self, max: usize) {
        self.max_cache_entries = max;
    }

    /// Set the streaming cache for `execute_streaming` result caching.
    ///
    /// When set, `execute_streaming` will check the cache before executing
    /// read-only tools and store successful results in the cache.
    pub fn set_streaming_cache(&mut self, cache: std::sync::Arc<crate::tool_cache::ToolResultCache>) {
        self.streaming_cache = Some(cache);
    }

    /// Get the streaming cache, if configured.
    pub fn streaming_cache(&self) -> Option<&std::sync::Arc<crate::tool_cache::ToolResultCache>> {
        self.streaming_cache.as_ref()
    }

    /// Restrict the registry to only allow specific tools.
    ///
    /// Supports glob patterns: `*` matches any characters, `?` matches one.
    /// Prefix a pattern with `!` to exclude matching tools.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // Only allow MCP tools and bash, deny internal MCP tools
    /// registry.set_allowed_tools(Some(vec![
    ///     "mcp__*".into(),
    ///     "Bash".into(),
    ///     "!mcp__internal__*".into(),
    /// ]));
    /// ```
    ///
    /// Pass `None` to remove all restrictions (allow everything).
    pub fn set_allowed_tools(&mut self, allowed: Option<Vec<String>>) {
        self.tool_filter = allowed.map(ToolFilter::from_patterns);
    }

    /// Check if a tool name is allowed by the current filter.
    fn is_allowed(&self, name: &str) -> bool {
        match &self.tool_filter {
            Some(filter) => filter.is_allowed(name),
            None => true,
        }
    }

    /// Bump the version counter to invalidate schema/defs caches.
    fn invalidate_cache(&self) {
        self.version
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// Register a new tool
    pub fn register(&self, tool: Box<dyn Tool>) -> ToolResult<()> {
        let name = tool.name().to_string();
        let mut tools = Self::recover_lock(self.tools.write());
        if tools.contains_key(&name) {
            return Err(ToolError::RegistryError(format!(
                "Tool {name} already registered"
            )));
        }
        tools.insert(name, std::sync::Arc::from(tool));
        self.invalidate_cache();
        Ok(())
    }

    /// Unregister a tool by name.
    pub fn unregister(&self, name: &str) -> ToolResult<()> {
        Self::recover_lock(self.tools.write())
            .remove(name)
            .ok_or_else(|| ToolError::NotFound(name.to_string()))?;
        // Also remove from deferred set if present
        Self::recover_lock(self.deferred.write()).remove(name);
        self.invalidate_cache();
        Ok(())
    }

    /// Register a tool and mark it as deferred (excluded from LLM schema).
    ///
    /// Deferred tools are still executable via `get()` / `execute()` but their
    /// schemas are not sent to the model.  The model discovers them on-demand
    /// through the `ToolSearch` meta-tool.
    pub fn register_deferred(&self, tool: Box<dyn Tool>) -> ToolResult<()> {
        let name = tool.name().to_string();
        let mut tools = Self::recover_lock(self.tools.write());
        if tools.contains_key(&name) {
            return Err(ToolError::RegistryError(format!(
                "Tool {name} already registered"
            )));
        }
        Self::recover_lock(self.deferred.write()).insert(name.clone());
        tools.insert(name, std::sync::Arc::from(tool));
        self.invalidate_cache();
        Ok(())
    }

    /// Register multiple tools, automatically deferring if the batch exceeds
    /// [`DEFER_THRESHOLD`].  Returns the number of tools marked as deferred.
    pub fn register_batch(&self, batch: Vec<Box<dyn Tool>>) -> ToolResult<usize> {
        if batch.is_empty() {
            return Ok(0);
        }
        let defer = batch.len() > DEFER_THRESHOLD;
        let mut deferred_count = 0;
        for tool in batch {
            let name = tool.name().to_string();
            let mut tools = Self::recover_lock(self.tools.write());
            if tools.contains_key(&name) {
                continue; // skip duplicates silently in batch mode
            }
            if defer {
                Self::recover_lock(self.deferred.write()).insert(name.clone());
                deferred_count += 1;
            }
            tools.insert(name, std::sync::Arc::from(tool));
        }
        self.invalidate_cache();
        Ok(deferred_count)
    }

    /// Search deferred tools by keyword (name or description substring match).
    ///
    /// Returns full `ToolInfo` including input_schema so the LLM can use the
    /// tool after discovery.
    pub fn search_deferred(&self, query: &str) -> Vec<ToolInfo> {
        let deferred = Self::recover_lock(self.deferred.read());
        if deferred.is_empty() {
            return vec![];
        }
        let query_lower = query.to_lowercase();
        let tools = Self::recover_lock(self.tools.read());
        deferred
            .iter()
            .filter_map(|name| {
                let tool = tools.get(name)?;
                let tool_name = tool.name();
                if !self.is_allowed(tool_name) {
                    return None;
                }
                let name_match = tool_name.to_lowercase().contains(&query_lower);
                let desc_match = tool.description().to_lowercase().contains(&query_lower);
                if name_match || desc_match {
                    Some(ToolInfo {
                        name: tool_name.to_string(),
                        description: tool.description().to_string(),
                        category: tool.category().to_string(),
                        requires_auth: tool.requires_auth(),
                        input_schema: tool.input_schema(),
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    /// Returns the number of deferred tools.
    pub fn deferred_count(&self) -> usize {
        Self::recover_lock(self.deferred.read()).len()
    }

    /// Get a tool by name (respects the allowed_tools filter).
    ///
    /// Returns an `Arc` clone so the caller does not need to hold the internal
    /// read lock — the tool can be used (and `.await`ed) without blocking
    /// concurrent registrations.
    pub fn get(&self, name: &str) -> Option<std::sync::Arc<dyn Tool>> {
        if self.is_allowed(name) {
            Self::recover_lock(self.tools.read()).get(name).cloned()
        } else {
            None
        }
    }

    /// List all registered tool names (respects the allowed_tools filter)
    pub fn list(&self) -> Vec<String> {
        Self::recover_lock(self.tools.read())
            .keys()
            .filter(|name| self.is_allowed(name))
            .cloned()
            .collect()
    }

    /// List all registered tools with their metadata (name, description, category, auth, schema).
    pub fn list_tools_info(&self) -> Vec<ToolInfo> {
        Self::recover_lock(self.tools.read())
            .values()
            .filter(|t| self.is_allowed(t.name()))
            .map(|t| ToolInfo {
                name: t.name().to_string(),
                description: t.description().to_string(),
                category: t.category().to_string(),
                requires_auth: t.requires_auth(),
                input_schema: t.input_schema(),
            })
            .collect()
    }

    /// Execute a tool by name (respects the allowed_tools filter).
    ///
    /// Results from read-only tools are cached in memory with a TTL.
    /// A subsequent call with the same tool name and identical input JSON
    /// will return the cached result without re-executing the tool,
    /// as long as the entry has not expired.
    pub async fn execute(&self, name: &str, input: Value) -> ToolResult<ToolOutput> {
        let tool = self
            .get(name)
            .ok_or_else(|| ToolError::NotFound(name.to_string()))?;

        let is_read_only = tool.is_read_only();

        // Compute hash before moving input into the tool
        let input_hash = if is_read_only {
            Some(Self::hash_input(&input))
        } else {
            None
        };

        // Check cache for read-only tools (with TTL expiry)
        if let Some(hash) = input_hash {
            let cache_key = (name.to_string(), hash);
            if let Some(cached) = Self::recover_lock(self.result_cache.lock())
                .get(&cache_key)
                .cloned()
            {
                let elapsed = cached.created_at.elapsed().as_secs();
                if elapsed < self.cache_ttl_secs {
                    return Ok(cached.output);
                }
                // Entry expired — remove from both cache and order tracking
                Self::recover_lock(self.result_cache.lock()).remove(&cache_key);
                Self::recover_lock(self.cache_order.lock()).retain(|k| k != &cache_key);
            }
        }

        let result = tool.execute(input).await;

        // Cache successful results from read-only tools
        if let Some(hash) = input_hash {
            if let Ok(ref output) = result {
                if !output.is_error {
                    let cache_key = (name.to_string(), hash);
                    let mut cache = Self::recover_lock(self.result_cache.lock());

                    // Evict oldest entries if cache is full (FIFO)
                    if cache.len() >= self.max_cache_entries {
                        let mut order = Self::recover_lock(self.cache_order.lock());
                        if let Some(old_key) = order.pop_front() {
                            cache.remove(&old_key);
                        }
                    }

                    cache.insert(
                        cache_key.clone(),
                        CachedToolResult {
                            output: output.clone(),
                            created_at: std::time::Instant::now(),
                        },
                    );
                    Self::recover_lock(self.cache_order.lock()).push_back(cache_key);
                }
            }
        }

        result
    }

    /// Hash tool input for cache key generation.
    fn hash_input(input: &Value) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        input.to_string().hash(&mut hasher);
        hasher.finish()
    }

    /// Clear the tool result cache.
    pub fn clear_cache(&self) {
        Self::recover_lock(self.result_cache.lock()).clear();
        Self::recover_lock(self.cache_order.lock()).clear();
    }

    /// Get the number of cached results (including potentially expired ones).
    pub fn cache_size(&self) -> usize {
        Self::recover_lock(self.result_cache.lock()).len()
    }

    /// Invalidate cache entries that were created before the given instant.
    /// Useful for clearing stale entries after file changes.
    pub fn invalidate_older_than(&self, cutoff: std::time::Instant) {
        let mut cache = Self::recover_lock(self.result_cache.lock());
        cache.retain(|_, entry| entry.created_at > cutoff);
    }

    /// Evict all expired entries from the cache.
    pub fn evict_expired(&self) {
        let ttl = self.cache_ttl_secs;
        let mut cache = Self::recover_lock(self.result_cache.lock());
        cache.retain(|_, entry| entry.created_at.elapsed().as_secs() < ttl);
    }

    /// Execute a tool by name using streaming progress.
    ///
    /// Falls back to non-streaming `execute()` for tools that don't override
    /// `execute_streaming`. The progress sender receives incremental output
    /// lines during execution.
    ///
    /// When a streaming cache is configured, successful results from read-only
    /// tools are cached and reused on subsequent calls with identical inputs.
    pub async fn execute_streaming(
        &self,
        name: &str,
        input: Value,
        progress: shannon_tool_interface::BoxedProgressSender,
    ) -> ToolResult<ToolOutput> {
        let tool = self
            .get(name)
            .ok_or_else(|| ToolError::NotFound(name.to_string()))?;

        let is_read_only = tool.is_read_only();

        // Check streaming cache for read-only tools
        if let Some(ref cache) = self.streaming_cache {
            if is_read_only {
                if let Some(cached) = cache.get(name, &input) {
                    tracing::debug!("Streaming cache hit for tool '{}'", name);
                    return Ok(cached);
                }
            }
        }

        let result = tool.execute_streaming(input.clone(), progress).await;

        // Cache successful results from read-only tools
        if let Some(ref cache) = self.streaming_cache {
            if is_read_only {
                if let Ok(ref output) = result {
                    if !output.is_error {
                        cache.put(name, &input, output, true);
                    }
                }
            }
        }

        result
    }

    /// Check whether a registered tool is read-only.
    ///
    /// Returns `false` for unknown tools (conservative default — assume side effects).
    pub fn is_tool_read_only(&self, name: &str) -> bool {
        self.get(name).map(|t| t.is_read_only()).unwrap_or(false)
    }

    /// Check whether a registered tool is safe to run concurrently.
    ///
    /// Returns `false` for unknown tools.
    pub fn is_tool_concurrency_safe(&self, name: &str) -> bool {
        self.get(name)
            .map(|t| t.is_concurrency_safe())
            .unwrap_or(false)
    }

    /// Check whether a registered tool may perform destructive operations.
    ///
    /// Destructive tools always require user confirmation regardless of approval mode.
    /// Returns `false` for unknown tools (non-MCP tools default to non-destructive).
    pub fn is_tool_destructive(&self, name: &str) -> bool {
        self.get(name).map(|t| t.is_destructive()).unwrap_or(false)
    }

    /// Return the names of all registered tools flagged as destructive.
    pub fn destructive_tool_names(&self) -> Vec<String> {
        Self::recover_lock(self.tools.read())
            .values()
            .filter(|t| t.is_destructive())
            .map(|t| t.name().to_string())
            .collect()
    }

    /// Partition a list of approved tool calls into execution batches.
    /// Invalidate streaming cache entries for the given file paths.
    ///
    /// Called when source files change so stale cached read results are not
    /// returned on subsequent tool calls.
    pub fn invalidate_cache_paths(&self, paths: &[String]) {
        if let Some(ref cache) = self.streaming_cache {
            for path in paths {
                cache.invalidate_path(path);
            }
        }
        // Also invalidate the basic cache
        let now = std::time::Instant::now();
        self.invalidate_older_than(now);
    }

    /// Invalidate all streaming cache entries (e.g., on branch switch).
    pub fn invalidate_cache_all(&self) {
        if let Some(ref cache) = self.streaming_cache {
            cache.invalidate_all();
        }
        self.clear_cache();
    }

    ///
    /// Walks through the tools in order and groups consecutive read-only /
    /// concurrency-safe tools into parallel batches (capped at `max_parallel`).
    /// Write tools each get their own serial batch.
    ///
    /// Returns an ordered list of batches that must be executed sequentially,
    /// with each batch's tools safe to run in parallel.
    pub fn partition_tool_calls(
        &self,
        tools: Vec<(String, String, Value)>, // (tool_use_id, tool_name, input)
        max_parallel: usize,
    ) -> Vec<ToolBatch> {
        let mut batches: Vec<ToolBatch> = Vec::new();
        let mut current_parallel: Vec<(String, String, Value)> = Vec::new();

        let flush_parallel = |batches: &mut Vec<ToolBatch>,
                              buf: &mut Vec<(String, String, Value)>| {
            if !buf.is_empty() {
                batches.push(ToolBatch::Parallel(std::mem::take(buf)));
            }
        };

        for tool_call in tools {
            let is_safe = self.is_tool_concurrency_safe(&tool_call.1);

            if is_safe {
                current_parallel.push(tool_call);
                if current_parallel.len() >= max_parallel {
                    flush_parallel(&mut batches, &mut current_parallel);
                }
            } else {
                // Write tool: flush any accumulated parallel batch first
                flush_parallel(&mut batches, &mut current_parallel);
                batches.push(ToolBatch::Serial(tool_call));
            }
        }

        flush_parallel(&mut batches, &mut current_parallel);
        batches
    }
}

/// A batch of tool calls produced by [`ToolRegistry::partition_tool_calls`].
#[derive(Debug)]
pub enum ToolBatch {
    /// Tools safe to execute concurrently (read-only / concurrency-safe).
    Parallel(Vec<(String, String, Value)>),
    /// A single tool that must run alone (has side effects).
    Serial((String, String, Value)),
}

impl ToolRegistry {
    /// Get all tools as JSON schema for Claude API (respects the allowed_tools filter
    /// and excludes deferred tools). Results are cached and invalidated on register/unregister.
    pub fn to_json_schema(&self) -> Value {
        let ver = self.version.load(std::sync::atomic::Ordering::Relaxed);
        {
            let cache = Self::recover_lock(self.schema_cache.read());
            if let Some((cached_ver, ref schema)) = *cache {
                if cached_ver == ver {
                    return schema.clone();
                }
            }
        }
        // Cache miss — rebuild
        let deferred = Self::recover_lock(self.deferred.read());
        let tools: Vec<Value> = Self::recover_lock(self.tools.read())
            .values()
            .filter(|t| self.is_allowed(t.name()) && !deferred.contains(t.name()))
            .map(|tool| {
                serde_json::json!({
                    "name": tool.name(),
                    "description": tool.description(),
                    "input_schema": tool.input_schema(),
                })
            })
            .collect();
        let schema = serde_json::json!(tools);
        *Self::recover_lock(self.schema_cache.write()) = Some((ver, schema.clone()));
        schema
    }

    /// Get all tools as ToolDefinition for Claude API (respects the allowed_tools filter
    /// and excludes deferred tools). Results are cached and invalidated on register/unregister.
    pub fn to_tool_definitions(&self) -> Vec<crate::api::ToolDefinition> {
        let ver = self.version.load(std::sync::atomic::Ordering::Relaxed);
        {
            let cache = Self::recover_lock(self.defs_cache.read());
            if let Some((cached_ver, ref defs)) = *cache {
                if cached_ver == ver {
                    return defs.clone();
                }
            }
        }
        // Cache miss — rebuild
        let deferred = Self::recover_lock(self.deferred.read());
        let defs: Vec<crate::api::ToolDefinition> = Self::recover_lock(self.tools.read())
            .values()
            .filter(|t| self.is_allowed(t.name()) && !deferred.contains(t.name()))
            .map(|tool| crate::api::ToolDefinition {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                input_schema: tool.input_schema(),
                strict: Some(false),
            })
            .collect();
        *Self::recover_lock(self.defs_cache.write()) = Some((ver, defs.clone()));
        defs
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use serde_json::json;

    struct DummyTool {
        name: String,
    }

    #[async_trait]
    impl Tool for DummyTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn description(&self) -> &str {
            "A dummy tool for testing"
        }

        fn input_schema(&self) -> Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "input": {"type": "string"}
                }
            })
        }

        async fn execute(&self, _input: Value) -> ToolResult<ToolOutput> {
            Ok(ToolOutput::success("Executed".to_string()))
        }
    }

    #[tokio::test]
    async fn test_tool_registration() {
        let registry = ToolRegistry::new();
        let tool = Box::new(DummyTool {
            name: "test_tool".to_string(),
        });

        registry.register(tool).unwrap();
        assert_eq!(registry.list(), vec!["test_tool".to_string()]);
    }

    #[tokio::test]
    async fn test_tool_execution() {
        let registry = ToolRegistry::new();
        let tool = Box::new(DummyTool {
            name: "test_tool".to_string(),
        });

        registry.register(tool).unwrap();
        let result = registry
            .execute("test_tool", serde_json::json!({"input": "test"}))
            .await
            .unwrap();
        assert_eq!(result.content, "Executed");
    }

    // ── Tool Registry Integration Tests ───────────────────────────────────

    struct AsyncTool {
        name: String,
        delay_ms: u64,
    }

    #[async_trait]
    impl Tool for AsyncTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn description(&self) -> &str {
            "An async tool for testing"
        }

        fn input_schema(&self) -> Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "input": {"type": "string"}
                }
            })
        }

        async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
            // Simulate async work
            tokio::time::sleep(tokio::time::Duration::from_millis(self.delay_ms)).await;
            Ok(ToolOutput::success(format!(
                "Processed: {}",
                input["input"].as_str().unwrap_or("")
            )))
        }

        fn requires_auth(&self) -> bool {
            true
        }

        fn category(&self) -> &str {
            "test"
        }
    }

    #[tokio::test]
    async fn test_concurrent_tool_execution() {
        let registry = ToolRegistry::new();

        // Register multiple tools
        for i in 0..5 {
            let tool = Box::new(AsyncTool {
                name: format!("async_tool_{i}"),
                delay_ms: 10,
            });
            registry.register(tool).unwrap();
        }

        let registry = std::sync::Arc::new(registry);
        let mut handles = Vec::new();

        // Execute tools concurrently
        for i in 0..5 {
            let registry_clone = registry.clone();
            let handle = tokio::spawn(async move {
                let tool_name = format!("async_tool_{i}");
                let input = serde_json::json!({"input": format!("request_{}", i)});
                registry_clone.execute(&tool_name, input).await
            });
            handles.push(handle);
        }

        // Wait for all executions
        let results = futures::future::join_all(handles).await;

        // All should succeed
        for result in results {
            assert!(result.is_ok());
            let output = result.unwrap().unwrap();
            assert!(output.content.contains("Processed:"));
        }
    }

    #[tokio::test]
    async fn test_tool_execution_with_permission_checks() {
        use crate::permissions::{PermissionPrompt, RiskLevel};

        let registry = ToolRegistry::new();
        let tool = Box::new(AsyncTool {
            name: "secure_tool".to_string(),
            delay_ms: 0,
        });

        registry.register(tool).unwrap();

        // Check tool info includes auth requirement
        let tools_info = registry.list_tools_info();
        let secure_tool_info = tools_info.iter().find(|t| t.name == "secure_tool").unwrap();
        assert!(secure_tool_info.requires_auth);
        assert_eq!(secure_tool_info.category, "test");

        // Execute the tool
        let result = registry
            .execute("secure_tool", serde_json::json!({"input": "test"}))
            .await;

        assert!(result.is_ok());

        // Verify permission prompt that would be generated
        let prompt = PermissionPrompt::new(
            "secure_tool".to_string(),
            serde_json::json!({"input": "test"}),
            RiskLevel::Low,
            "Execute secure_tool".to_string(),
        );
        assert_eq!(prompt.tool_name, "secure_tool");
        assert_eq!(prompt.risk_level, RiskLevel::Low);
    }

    #[tokio::test]
    async fn test_tool_registry_with_multiple_tools() {
        let registry = ToolRegistry::new();

        // Register multiple tools with different characteristics
        let tools = vec![
            Box::new(AsyncTool {
                name: "read_tool".to_string(),
                delay_ms: 0,
            }) as Box<dyn Tool>,
            Box::new(AsyncTool {
                name: "write_tool".to_string(),
                delay_ms: 0,
            }) as Box<dyn Tool>,
            Box::new(AsyncTool {
                name: "network_tool".to_string(),
                delay_ms: 0,
            }) as Box<dyn Tool>,
        ];

        for tool in tools {
            registry.register(tool).unwrap();
        }

        // List all tools
        let tool_names = registry.list();
        assert_eq!(tool_names.len(), 3);
        assert!(tool_names.contains(&"read_tool".to_string()));
        assert!(tool_names.contains(&"write_tool".to_string()));
        assert!(tool_names.contains(&"network_tool".to_string()));

        // Get all tool info
        let tools_info = registry.list_tools_info();
        assert_eq!(tools_info.len(), 3);

        // Convert to JSON schema
        let json_schema = registry.to_json_schema();
        assert!(json_schema.is_array());
        assert_eq!(json_schema.as_array().unwrap().len(), 3);

        // Convert to tool definitions
        let tool_defs = registry.to_tool_definitions();
        assert_eq!(tool_defs.len(), 3);
    }

    #[tokio::test]
    async fn test_tool_unregister() {
        let registry = ToolRegistry::new();

        let tool = Box::new(DummyTool {
            name: "temp_tool".to_string(),
        });

        registry.register(tool).unwrap();
        assert!(registry.list().contains(&"temp_tool".to_string()));

        // Unregister
        registry.unregister("temp_tool").unwrap();
        assert!(!registry.list().contains(&"temp_tool".to_string()));

        // Unregistering non-existent tool should fail
        let result = registry.unregister("nonexistent");
        assert!(matches!(result, Err(ToolError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_duplicate_tool_registration_fails() {
        let registry = ToolRegistry::new();

        let tool1 = Box::new(DummyTool {
            name: "dup_tool".to_string(),
        });
        let tool2 = Box::new(DummyTool {
            name: "dup_tool".to_string(),
        });

        registry.register(tool1).unwrap();

        let result = registry.register(tool2);
        assert!(matches!(result, Err(ToolError::RegistryError(_))));
    }

    #[tokio::test]
    async fn test_execute_nonexistent_tool() {
        let registry = ToolRegistry::new();

        let result = registry.execute("nonexistent", serde_json::json!({})).await;

        assert!(matches!(result, Err(ToolError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_tool_metadata() {
        let registry = ToolRegistry::new();

        let tool = Box::new(DummyTool {
            name: "metadata_tool".to_string(),
        });

        registry.register(tool).unwrap();

        // Get tool info
        let tools_info = registry.list_tools_info();
        let info = tools_info
            .iter()
            .find(|t| t.name == "metadata_tool")
            .unwrap();

        assert_eq!(info.name, "metadata_tool");
        assert_eq!(info.description, "A dummy tool for testing");
        assert_eq!(info.category, "general");
        assert!(!info.requires_auth);
        assert!(info.input_schema.is_object());
    }

    #[tokio::test]
    async fn test_concurrent_tool_registration() {
        let registry = std::sync::Arc::new(std::sync::Mutex::new(ToolRegistry::new()));
        let num_threads = 10;

        let mut handles = Vec::new();

        // Each thread registers a unique tool
        for i in 0..num_threads {
            let registry_clone = registry.clone();
            let handle = tokio::spawn(async move {
                let tool = Box::new(DummyTool {
                    name: format!("concurrent_tool_{i}"),
                });
                registry_clone.lock().unwrap().register(tool)
            });
            handles.push(handle);
        }

        // Wait for all registrations
        let results = futures::future::join_all(handles).await;

        // All should succeed
        for result in results {
            assert!(result.is_ok());
        }

        // Verify all tools were registered
        let tool_names = registry.lock().unwrap().list();
        assert_eq!(tool_names.len(), num_threads);
    }

    #[tokio::test]
    async fn test_tool_output_with_metadata() {
        let registry = ToolRegistry::new();

        struct MetadataTool {
            name: String,
        }

        #[async_trait]
        impl Tool for MetadataTool {
            fn name(&self) -> &str {
                &self.name
            }

            fn description(&self) -> &str {
                "Tool with metadata"
            }

            fn input_schema(&self) -> Value {
                serde_json::json!({"type": "object"})
            }

            async fn execute(&self, _input: Value) -> ToolResult<ToolOutput> {
                Ok(ToolOutput::success("Success".to_string())
                    .with_metadata("execution_time_ms".to_string(), json!(100))
                    .with_metadata("timestamp".to_string(), json!("2024-01-01T00:00:00Z")))
            }
        }

        let tool = Box::new(MetadataTool {
            name: "metadata_tool".to_string(),
        });

        registry.register(tool).unwrap();

        let result = registry
            .execute("metadata_tool", serde_json::json!({}))
            .await
            .unwrap();

        assert_eq!(result.content, "Success");
        assert!(!result.is_error);
        assert_eq!(result.metadata.get("execution_time_ms"), Some(&json!(100)));
        assert_eq!(
            result.metadata.get("timestamp"),
            Some(&json!("2024-01-01T00:00:00Z"))
        );
    }

    // ── Glob-based allow/deny filter tests ────────────────────────────────

    #[tokio::test]
    async fn test_glob_filter_wildcard() {
        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(DummyTool {
                name: "mcp__server__tool1".into(),
            }))
            .unwrap();
        registry
            .register(Box::new(DummyTool {
                name: "mcp__server__tool2".into(),
            }))
            .unwrap();
        registry
            .register(Box::new(DummyTool {
                name: "Bash".into(),
            }))
            .unwrap();

        registry.set_allowed_tools(Some(vec!["mcp__*".into()]));
        let names = registry.list();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"mcp__server__tool1".to_string()));
        assert!(names.contains(&"mcp__server__tool2".to_string()));
    }

    #[tokio::test]
    async fn test_glob_filter_exact_name() {
        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(DummyTool {
                name: "Bash".into(),
            }))
            .unwrap();
        registry
            .register(Box::new(DummyTool {
                name: "Read".into(),
            }))
            .unwrap();

        registry.set_allowed_tools(Some(vec!["Bash".into()]));
        assert!(registry.get("Bash").is_some());
        assert!(registry.get("Read").is_none());
    }

    #[tokio::test]
    async fn test_glob_filter_exclude() {
        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(DummyTool {
                name: "mcp__public__tool".into(),
            }))
            .unwrap();
        registry
            .register(Box::new(DummyTool {
                name: "mcp__internal__secret".into(),
            }))
            .unwrap();

        registry.set_allowed_tools(Some(vec!["mcp__*".into(), "!mcp__internal__*".into()]));
        let names = registry.list();
        assert_eq!(names.len(), 1);
        assert!(names.contains(&"mcp__public__tool".to_string()));
    }

    #[tokio::test]
    async fn test_glob_filter_question_mark() {
        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(DummyTool {
                name: "tool_a".into(),
            }))
            .unwrap();
        registry
            .register(Box::new(DummyTool {
                name: "tool_b".into(),
            }))
            .unwrap();
        registry
            .register(Box::new(DummyTool {
                name: "tool_ab".into(),
            }))
            .unwrap();

        // ? matches exactly one character
        registry.set_allowed_tools(Some(vec!["tool_?".into()]));
        let names = registry.list();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"tool_a".to_string()));
        assert!(names.contains(&"tool_b".to_string()));
    }

    #[tokio::test]
    async fn test_glob_filter_none_allows_all() {
        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(DummyTool { name: "a".into() }))
            .unwrap();
        registry
            .register(Box::new(DummyTool { name: "b".into() }))
            .unwrap();

        registry.set_allowed_tools(None);
        assert_eq!(registry.list().len(), 2);
    }

    #[tokio::test]
    async fn test_glob_filter_only_excludes() {
        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(DummyTool {
                name: "mcp__secret".into(),
            }))
            .unwrap();
        registry
            .register(Box::new(DummyTool {
                name: "Bash".into(),
            }))
            .unwrap();

        // Only exclude patterns → everything not denied is allowed
        registry.set_allowed_tools(Some(vec!["!mcp__*".into()]));
        let names = registry.list();
        assert_eq!(names.len(), 1);
        assert!(names.contains(&"Bash".to_string()));
    }

    // ── Deferred loading tests ──────────────────────────────────────────

    #[test]
    fn test_register_deferred_excludes_from_schema() {
        let registry = ToolRegistry::new();
        registry
            .register(Box::new(DummyTool {
                name: "VisibleTool".into(),
            }))
            .unwrap();
        registry
            .register_deferred(Box::new(DummyTool {
                name: "HiddenTool".into(),
            }))
            .unwrap();

        // Both should be in list() and get()
        assert!(registry.get("VisibleTool").is_some());
        assert!(registry.get("HiddenTool").is_some());
        assert_eq!(registry.list().len(), 2);

        // Only VisibleTool should appear in schema
        let schema = registry.to_json_schema();
        let tools = schema.as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "VisibleTool");

        // Only VisibleTool in tool definitions
        let defs = registry.to_tool_definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "VisibleTool");
    }

    #[test]
    fn test_deferred_count() {
        let registry = ToolRegistry::new();
        assert_eq!(registry.deferred_count(), 0);
        registry
            .register_deferred(Box::new(DummyTool { name: "D1".into() }))
            .unwrap();
        registry
            .register_deferred(Box::new(DummyTool { name: "D2".into() }))
            .unwrap();
        assert_eq!(registry.deferred_count(), 2);
    }

    #[test]
    fn test_search_deferred() {
        let registry = ToolRegistry::new();
        registry
            .register(Box::new(DummyTool {
                name: "NormalTool".into(),
            }))
            .unwrap();
        registry
            .register_deferred(Box::new(DummyTool {
                name: "mcp__db__query_users".into(),
            }))
            .unwrap();
        registry
            .register_deferred(Box::new(DummyTool {
                name: "mcp__db__query_orders".into(),
            }))
            .unwrap();

        let results = registry.search_deferred("query");
        assert_eq!(results.len(), 2);

        let results = registry.search_deferred("users");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "mcp__db__query_users");
    }

    #[test]
    fn test_register_batch_auto_defers() {
        let registry = ToolRegistry::new();

        // Create 51 tools — should auto-defer
        let batch: Vec<Box<dyn Tool>> = (0..51)
            .map(|i| {
                Box::new(DummyTool {
                    name: format!("tool_{i}"),
                }) as Box<dyn Tool>
            })
            .collect();

        let deferred = registry.register_batch(batch).unwrap();
        assert_eq!(deferred, 51);
        assert_eq!(registry.deferred_count(), 51);

        // Schema should be empty (all deferred)
        let schema = registry.to_json_schema();
        assert_eq!(schema.as_array().unwrap().len(), 0);

        // But tools are still executable
        assert!(registry.get("tool_0").is_some());
        assert!(registry.get("tool_50").is_some());
    }

    #[test]
    fn test_register_batch_small_no_defer() {
        let registry = ToolRegistry::new();

        let batch: Vec<Box<dyn Tool>> = (0..10)
            .map(|i| {
                Box::new(DummyTool {
                    name: format!("small_{i}"),
                }) as Box<dyn Tool>
            })
            .collect();

        let deferred = registry.register_batch(batch).unwrap();
        assert_eq!(deferred, 0);
        assert_eq!(registry.deferred_count(), 0);

        // All should appear in schema
        let schema = registry.to_json_schema();
        assert_eq!(schema.as_array().unwrap().len(), 10);
    }

    #[test]
    fn test_unregister_removes_deferred() {
        let registry = ToolRegistry::new();
        registry
            .register_deferred(Box::new(DummyTool {
                name: "DeferredTool".into(),
            }))
            .unwrap();
        assert_eq!(registry.deferred_count(), 1);

        registry.unregister("DeferredTool").unwrap();
        assert_eq!(registry.deferred_count(), 0);
        assert!(registry.get("DeferredTool").is_none());
    }

    // ── execute_streaming tests ─────────────────────────────────────────

    /// A tool that sends progress lines when streaming.
    struct StreamingEchoTool;

    #[async_trait]
    impl Tool for StreamingEchoTool {
        fn name(&self) -> &str {
            "stream_echo"
        }
        fn description(&self) -> &str {
            "streams lines"
        }
        fn input_schema(&self) -> Value {
            json!({"type": "object", "properties": {"msg": {"type": "string"}}})
        }
        async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
            let msg = input.get("msg").and_then(|v| v.as_str()).unwrap_or("");
            Ok(ToolOutput::success(msg.to_string()))
        }
        async fn execute_streaming(
            &self,
            input: Value,
            progress: shannon_tool_interface::BoxedProgressSender,
        ) -> ToolResult<ToolOutput> {
            let msg = input.get("msg").and_then(|v| v.as_str()).unwrap_or("");
            for line in msg.lines() {
                progress.send(line);
            }
            Ok(ToolOutput::success(msg.to_string()))
        }
    }

    #[tokio::test]
    async fn test_registry_execute_streaming_delegates() {
        let registry = ToolRegistry::new();
        registry.register(Box::new(StreamingEchoTool)).unwrap();

        struct Collector {
            lines: std::sync::Mutex<Vec<String>>,
        }
        impl shannon_tool_interface::ProgressSender for Collector {
            fn send(&self, line: &str) {
                self.lines.lock().unwrap().push(line.to_string());
            }
        }
        let sender = std::sync::Arc::new(Collector {
            lines: std::sync::Mutex::new(Vec::new()),
        });

        let result = registry
            .execute_streaming(
                "stream_echo",
                json!({"msg": "line1\nline2\nline3"}),
                sender.clone(),
            )
            .await
            .unwrap();

        assert_eq!(result.content, "line1\nline2\nline3");
        assert!(!result.is_error);
        assert_eq!(sender.lines.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn test_registry_execute_streaming_unknown_tool() {
        let registry = ToolRegistry::new();
        struct NopSender;
        impl shannon_tool_interface::ProgressSender for NopSender {
            fn send(&self, _: &str) {}
        }

        let result = registry
            .execute_streaming("nonexistent", json!({}), std::sync::Arc::new(NopSender))
            .await;
        assert!(matches!(result, Err(ToolError::NotFound(_))));
    }
}
