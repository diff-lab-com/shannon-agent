//! # Tool Orchestration Optimization
//!
//! Multi-turn tool call deduplication and parallel batching analysis.
//!
//! When the LLM issues multiple tool calls in a single turn, this module
//! analyzes the batch to find optimization opportunities:
//!
//! - **Deduplication**: Identical calls (same tool + same input) are reduced
//!   to a single execution, with the result shared across callers.
//! - **Parallel grouping**: Read-only tools (Read, Glob, Grep) targeting
//!   different files can execute concurrently. Write tools targeting
//!   different files are likewise parallelizable.
//! - **Sequential ordering**: A read-after-write dependency on the same file
//!   forces those two calls into sequential execution order.
//!
//! ## Architecture
//!
//! - [`ToolOrchestrationTracker`]: Call-history cache with TTL expiry, used to
//!   detect duplicate calls across turns.
//! - [`ToolCallOptimizer`]: Stateless analyzer that inspects a batch of
//!   [`PendingToolCall`] values and returns an [`OptimizedCallPlan`].
//!
//! ## Usage
//!
//! ```rust,ignore
//! use shannon_core::tool_orchestration::{
//!     ToolOrchestrationTracker, ToolCallOptimizer, PendingToolCall,
//! };
//!
//! let mut tracker = ToolOrchestrationTracker::new();
//!
//! // Record a call result for future dedup
//! tracker.record("Read", r#"{"file_path":"/tmp/a.rs"}"#, 0xABCD);
//!
//! // Check whether a repeat call is cached
//! assert!(tracker.is_cached("Read", r#"{"file_path":"/tmp/a.rs"}"#));
//!
//! // Optimize a batch
//! let calls = vec![
//!     PendingToolCall::new("Read",  serde_json::json!({"file_path": "/tmp/a.rs"})),
//!     PendingToolCall::new("Read",  serde_json::json!({"file_path": "/tmp/b.rs"})),
//!     PendingToolCall::new("Edit",  serde_json::json!({"file_path": "/tmp/a.rs", "old_string": "x", "new_string": "y"})),
//! ];
//! let plan = ToolCallOptimizer::optimize(&calls);
//! ```

use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Pending tool call
// ---------------------------------------------------------------------------

/// A pending tool call awaiting execution, ready for optimization analysis.
#[derive(Debug, Clone)]
pub struct PendingToolCall {
    /// Tool name (e.g. "Read", "Edit", "Bash").
    pub tool_name: String,
    /// Tool input as a JSON value.
    pub input: Value,
    /// Pre-computed hash of the serialized input for fast comparison.
    pub input_hash: u64,
}

impl PendingToolCall {
    /// Create a new pending tool call, computing the input hash automatically.
    pub fn new(tool_name: &str, input: Value) -> Self {
        let input_hash = hash_value(&input);
        Self {
            tool_name: tool_name.to_string(),
            input,
            input_hash,
        }
    }
}

// ---------------------------------------------------------------------------
// Optimized call plan
// ---------------------------------------------------------------------------

/// Result of analyzing a batch of pending tool calls for optimization.
#[derive(Debug, Clone)]
pub struct OptimizedCallPlan {
    /// Indices of calls that can be deduplicated (use cached results).
    pub deduplicated: Vec<usize>,
    /// Groups of call indices that can execute in parallel.
    pub parallel_groups: Vec<Vec<usize>>,
    /// Call indices that must execute sequentially (in order).
    pub sequential: Vec<usize>,
}

// ---------------------------------------------------------------------------
// Tracker
// ---------------------------------------------------------------------------

/// Tracks tool call history across turns to enable dedup and optimization.
///
/// Stores a mapping from `(tool_name, input_hash)` to `(result_hash, timestamp)`
/// and automatically prunes entries older than the configured TTL.
pub struct ToolOrchestrationTracker {
    /// Recent tool calls: `(tool_name, input_hash)` -> `(result_hash, timestamp)`.
    call_cache: HashMap<(String, u64), (u64, Instant)>,
    /// Cache TTL — entries older than this are considered expired.
    cache_ttl: Duration,
}

impl ToolOrchestrationTracker {
    /// Create a new tracker with the default TTL (5 minutes).
    pub fn new() -> Self {
        Self {
            call_cache: HashMap::new(),
            cache_ttl: Duration::from_secs(300),
        }
    }

    /// Create a tracker with a custom TTL.
    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            call_cache: HashMap::new(),
            cache_ttl: ttl,
        }
    }

    /// Check whether a tool call result is still cached and not expired.
    pub fn is_cached(&self, tool_name: &str, input: &str) -> bool {
        let input_hash = hash_str(input);
        let key = (tool_name.to_string(), input_hash);
        if let Some((_, ts)) = self.call_cache.get(&key) {
            ts.elapsed() < self.cache_ttl
        } else {
            false
        }
    }

    /// Record a tool call result for future dedup.
    pub fn record(&mut self, tool_name: &str, input: &str, result_hash: u64) {
        let input_hash = hash_str(input);
        let key = (tool_name.to_string(), input_hash);
        self.call_cache.insert(key, (result_hash, Instant::now()));
    }

    /// Prune expired entries from the cache.
    pub fn prune(&mut self) {
        let ttl = self.cache_ttl;
        self.call_cache.retain(|_, (_, ts)| ts.elapsed() < ttl);
    }

    /// Return the number of cached entries (including potentially expired ones).
    pub fn len(&self) -> usize {
        self.call_cache.len()
    }

    /// Return whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.call_cache.is_empty()
    }
}

impl Default for ToolOrchestrationTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Optimizer
// ---------------------------------------------------------------------------

/// Stateless analyzer that categorizes pending tool calls into an optimized
/// execution plan.
pub struct ToolCallOptimizer;

impl ToolCallOptimizer {
    /// Analyze a list of pending tool calls and produce an optimized plan.
    ///
    /// ## Algorithm
    ///
    /// 1. **Dedup**: Exact duplicates (same tool + same input hash) are
    ///    marked in `deduplicated`. Only the first occurrence is kept for
    ///    execution.
    /// 2. **Parallel groups**:
    ///    - All read-only calls form a single parallel group.
    ///    - Write calls targeting different files form additional parallel
    ///      groups.
    /// 3. **Sequential**: A read call that depends on a prior write call to
    ///    the same file (read-after-write) forces both into the `sequential`
    ///    list.
    pub fn optimize(calls: &[PendingToolCall]) -> OptimizedCallPlan {
        let mut deduplicated = Vec::new();
        let mut seen: HashSet<(String, u64)> = HashSet::new();

        // Indices that survive dedup, mapped to their original position.
        let mut surviving: Vec<usize> = Vec::new();

        // Step 1: deduplicate exact duplicates.
        for (i, call) in calls.iter().enumerate() {
            let key = (call.tool_name.clone(), call.input_hash);
            if seen.contains(&key) {
                deduplicated.push(i);
            } else {
                seen.insert(key);
                surviving.push(i);
            }
        }

        // Step 2: categorize surviving calls.
        let mut read_only_indices: Vec<usize> = Vec::new();
        let mut write_by_file: HashMap<String, Vec<usize>> = HashMap::new();
        let mut file_written_by: HashMap<String, usize> = HashMap::new(); // file -> first write index

        for &idx in &surviving {
            let call = &calls[idx];
            if Self::is_read_only(&call.tool_name) {
                read_only_indices.push(idx);
            } else {
                // Write tool — group by target file
                if let Some(file) = Self::extract_file_path(&call.tool_name, &call.input) {
                    file_written_by.insert(file.clone(), idx);
                    write_by_file.entry(file).or_default().push(idx);
                } else {
                    // No discernible file target — treat as sequential
                    write_by_file
                        .entry(format!("__unknown_{idx}"))
                        .or_default()
                        .push(idx);
                }
            }
        }

        // Step 3: detect read-after-write dependencies.
        let mut read_after_write: HashSet<usize> = HashSet::new();
        for &read_idx in &read_only_indices {
            let call = &calls[read_idx];
            if let Some(file) = Self::extract_file_path(&call.tool_name, &call.input) {
                if file_written_by.contains_key(&file) {
                    read_after_write.insert(read_idx);
                }
            }
        }

        // Step 4: build the plan.
        let mut parallel_groups: Vec<Vec<usize>> = Vec::new();
        let mut sequential: Vec<usize> = Vec::new();

        // Read-only calls without write dependencies -> parallel.
        let independent_reads: Vec<usize> = read_only_indices
            .into_iter()
            .filter(|idx| !read_after_write.contains(idx))
            .collect();
        if !independent_reads.is_empty() {
            parallel_groups.push(independent_reads);
        }

        // Read-after-write pairs -> sequential.
        // For each file with both writes and dependent reads, order sequentially.
        for &read_idx in &read_after_write {
            sequential.push(read_idx);
        }

        // Write calls grouped by file.
        // Different files can be parallel; same file must be sequential.
        let mut multi_write_sequential: Vec<usize> = Vec::new();
        for (_file, mut indices) in write_by_file {
            if indices.len() > 1 {
                // Multiple writes to same file — must be sequential.
                indices.sort();
                multi_write_sequential.extend(indices);
            }
        }
        sequential.extend(multi_write_sequential);

        // Collect write calls that are not already in sequential into parallel
        // groups (one per file, single-writer files can run in parallel).
        let sequential_set: HashSet<usize> = sequential.iter().copied().collect();
        let mut parallel_writes: Vec<usize> = Vec::new();
        for &idx in &surviving {
            if sequential_set.contains(&idx) || deduplicated.contains(&idx) {
                continue;
            }
            let call = &calls[idx];
            if !Self::is_read_only(&call.tool_name) {
                parallel_writes.push(idx);
            }
        }
        if !parallel_writes.is_empty() {
            parallel_groups.push(parallel_writes);
        }

        // Deduplicate sequential list and sort for deterministic output.
        let mut seq_dedup: Vec<usize> = sequential
            .into_iter()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        seq_dedup.sort();
        sequential = seq_dedup;

        OptimizedCallPlan {
            deduplicated,
            parallel_groups,
            sequential,
        }
    }

    /// Check if a tool is read-only (does not modify filesystem state).
    fn is_read_only(tool_name: &str) -> bool {
        matches!(
            tool_name,
            "Read"
                | "read"
                | "Glob"
                | "glob"
                | "Grep"
                | "grep"
                | "LS"
                | "ls"
                | "ListFiles"
                | "list_files"
        )
    }

    /// Extract the target file path from tool input for dependency analysis.
    ///
    /// - Read/Edit/Write tools: `input["file_path"]`
    /// - Glob/Grep tools: `input["path"]`
    /// - Bash and others: `None`
    fn extract_file_path(tool_name: &str, input: &Value) -> Option<String> {
        let obj = input.as_object()?;
        match tool_name {
            "Read" | "read" | "Edit" | "edit" | "Write" | "write" => obj
                .get("file_path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            "Glob" | "glob" | "Grep" | "grep" => obj
                .get("path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Hashing helpers
// ---------------------------------------------------------------------------

/// Hash a `serde_json::Value` to a `u64` using `DefaultHasher`.
fn hash_value(value: &Value) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value_hash(value, &mut hasher);
    hasher.finish()
}

/// Hash a string to a `u64` using `DefaultHasher`.
fn hash_str(s: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

/// Recursively hash a JSON value in a deterministic order.
fn value_hash(value: &Value, hasher: &mut impl Hasher) {
    // Discriminant byte ensures different types don't collide.
    match value {
        Value::Null => 0u8.hash(hasher),
        Value::Bool(b) => {
            1u8.hash(hasher);
            b.hash(hasher);
        }
        Value::Number(n) => {
            2u8.hash(hasher);
            n.to_string().hash(hasher);
        }
        Value::String(s) => {
            3u8.hash(hasher);
            s.hash(hasher);
        }
        Value::Array(arr) => {
            4u8.hash(hasher);
            arr.len().hash(hasher);
            for item in arr {
                value_hash(item, hasher);
            }
        }
        Value::Object(map) => {
            5u8.hash(hasher);
            map.len().hash(hasher);
            // Sort keys for deterministic hashing.
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for key in keys {
                key.hash(hasher);
                value_hash(&map[key], hasher);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use serde_json::json;

    // -- Tracker tests --

    #[test]
    fn test_tracker_records_and_checks_cache() {
        let mut tracker = ToolOrchestrationTracker::new();

        let input = r#"{"file_path":"/tmp/a.rs"}"#;
        assert!(
            !tracker.is_cached("Read", input),
            "should not be cached initially"
        );

        tracker.record("Read", input, 0xABCD);
        assert!(
            tracker.is_cached("Read", input),
            "should be cached after recording"
        );
    }

    #[test]
    fn test_tracker_cache_expires() {
        let mut tracker = ToolOrchestrationTracker::with_ttl(Duration::from_millis(50));

        let input = r#"{"file_path":"/tmp/a.rs"}"#;
        tracker.record("Read", input, 0xABCD);
        assert!(
            tracker.is_cached("Read", input),
            "should be cached immediately"
        );

        // Wait for TTL to expire
        std::thread::sleep(Duration::from_millis(80));
        assert!(
            !tracker.is_cached("Read", input),
            "should be expired after TTL"
        );
    }

    #[test]
    fn test_tracker_prune_removes_expired() {
        let mut tracker = ToolOrchestrationTracker::with_ttl(Duration::from_millis(50));

        tracker.record("Read", r#"{"file_path":"/tmp/a.rs"}"#, 1);
        std::thread::sleep(Duration::from_millis(80));
        tracker.record("Read", r#"{"file_path":"/tmp/b.rs"}"#, 2);

        assert_eq!(tracker.len(), 2);
        tracker.prune();
        assert_eq!(tracker.len(), 1, "only the second entry should survive");
        assert!(tracker.is_cached("Read", r#"{"file_path":"/tmp/b.rs"}"#));
    }

    #[test]
    fn test_tracker_default() {
        let tracker = ToolOrchestrationTracker::default();
        assert!(tracker.is_empty());
    }

    #[test]
    fn test_tracker_different_inputs_not_deduplicated() {
        let mut tracker = ToolOrchestrationTracker::new();
        tracker.record("Read", r#"{"file_path":"/tmp/a.rs"}"#, 1);

        assert!(
            !tracker.is_cached("Read", r#"{"file_path":"/tmp/b.rs"}"#),
            "different input should not be cached"
        );
    }

    // -- Optimizer tests --

    #[test]
    fn test_optimizer_deduplicates_identical_calls() {
        let calls = vec![
            PendingToolCall::new("Read", json!({"file_path": "/tmp/a.rs"})),
            PendingToolCall::new("Read", json!({"file_path": "/tmp/a.rs"})),
            PendingToolCall::new("Read", json!({"file_path": "/tmp/b.rs"})),
        ];

        let plan = ToolCallOptimizer::optimize(&calls);

        assert_eq!(
            plan.deduplicated,
            vec![1],
            "second identical call should be deduplicated"
        );
    }

    #[test]
    fn test_optimizer_groups_read_only_parallel() {
        let calls = vec![
            PendingToolCall::new("Read", json!({"file_path": "/tmp/a.rs"})),
            PendingToolCall::new("Read", json!({"file_path": "/tmp/b.rs"})),
            PendingToolCall::new("Grep", json!({"path": "/tmp/project", "pattern": "TODO"})),
        ];

        let plan = ToolCallOptimizer::optimize(&calls);

        assert!(plan.deduplicated.is_empty(), "no duplicates");
        assert!(
            plan.sequential.is_empty(),
            "read-only calls have no sequential deps"
        );

        // All three should be in a single parallel group.
        assert_eq!(
            plan.parallel_groups.len(),
            1,
            "one parallel group for reads"
        );
        let group = &plan.parallel_groups[0];
        assert_eq!(group.len(), 3);
        assert!(group.contains(&0));
        assert!(group.contains(&1));
        assert!(group.contains(&2));
    }

    #[test]
    fn test_optimizer_sequential_for_write_then_read() {
        let calls = vec![
            PendingToolCall::new(
                "Edit",
                json!({"file_path": "/tmp/a.rs", "old_string": "x", "new_string": "y"}),
            ),
            PendingToolCall::new("Read", json!({"file_path": "/tmp/a.rs"})),
        ];

        let plan = ToolCallOptimizer::optimize(&calls);

        assert!(plan.deduplicated.is_empty());
        // Read of /tmp/a.rs depends on Edit of /tmp/a.rs -> sequential.
        assert!(
            plan.sequential.contains(&1),
            "read-after-write should be sequential"
        );
    }

    #[test]
    fn test_optimizer_parallel_for_different_files() {
        let calls = vec![
            PendingToolCall::new(
                "Edit",
                json!({"file_path": "/tmp/a.rs", "old_string": "x", "new_string": "y"}),
            ),
            PendingToolCall::new(
                "Edit",
                json!({"file_path": "/tmp/b.rs", "old_string": "x", "new_string": "y"}),
            ),
        ];

        let plan = ToolCallOptimizer::optimize(&calls);

        assert!(plan.deduplicated.is_empty());
        // Writes to different files can be parallel.
        assert_eq!(
            plan.parallel_groups.len(),
            1,
            "one parallel group for writes to different files"
        );
        let group = &plan.parallel_groups[0];
        assert!(group.contains(&0));
        assert!(group.contains(&1));
    }

    #[test]
    fn test_optimizer_multiple_writes_same_file_sequential() {
        let calls = vec![
            PendingToolCall::new(
                "Edit",
                json!({"file_path": "/tmp/a.rs", "old_string": "x", "new_string": "y"}),
            ),
            PendingToolCall::new(
                "Edit",
                json!({"file_path": "/tmp/a.rs", "old_string": "y", "new_string": "z"}),
            ),
        ];

        let plan = ToolCallOptimizer::optimize(&calls);

        // Two writes to the same file -> sequential.
        assert!(plan.sequential.contains(&0));
        assert!(plan.sequential.contains(&1));
    }

    // -- extract_file_path tests --

    #[test]
    fn test_extract_file_path_read() {
        let input = json!({"file_path": "/tmp/test.rs"});
        let path = ToolCallOptimizer::extract_file_path("Read", &input);
        assert_eq!(path, Some("/tmp/test.rs".to_string()));
    }

    #[test]
    fn test_extract_file_path_edit() {
        let input = json!({"file_path": "/tmp/test.rs", "old_string": "x", "new_string": "y"});
        let path = ToolCallOptimizer::extract_file_path("Edit", &input);
        assert_eq!(path, Some("/tmp/test.rs".to_string()));
    }

    #[test]
    fn test_extract_file_path_bash_none() {
        let input = json!({"command": "ls -la"});
        let path = ToolCallOptimizer::extract_file_path("Bash", &input);
        assert!(path.is_none(), "Bash should not extract a file path");
    }

    #[test]
    fn test_extract_file_path_grep() {
        let input = json!({"path": "/tmp/project", "pattern": "TODO"});
        let path = ToolCallOptimizer::extract_file_path("Grep", &input);
        assert_eq!(path, Some("/tmp/project".to_string()));
    }

    #[test]
    fn test_extract_file_path_glob() {
        let input = json!({"path": "/tmp/project", "pattern": "**/*.rs"});
        let path = ToolCallOptimizer::extract_file_path("Glob", &input);
        assert_eq!(path, Some("/tmp/project".to_string()));
    }

    // -- is_read_only tests --

    #[test]
    fn test_is_read_only() {
        assert!(ToolCallOptimizer::is_read_only("Read"));
        assert!(ToolCallOptimizer::is_read_only("read"));
        assert!(ToolCallOptimizer::is_read_only("Glob"));
        assert!(ToolCallOptimizer::is_read_only("Grep"));
        assert!(!ToolCallOptimizer::is_read_only("Edit"));
        assert!(!ToolCallOptimizer::is_read_only("Write"));
        assert!(!ToolCallOptimizer::is_read_only("Bash"));
    }

    // -- Hashing tests --

    #[test]
    fn test_hash_value_deterministic() {
        let v1 = json!({"file_path": "/tmp/a.rs", "offset": 10});
        let v2 = json!({"offset": 10, "file_path": "/tmp/a.rs"});
        assert_eq!(
            hash_value(&v1),
            hash_value(&v2),
            "key order should not affect hash"
        );
    }

    #[test]
    fn test_hash_value_different_inputs() {
        let v1 = json!({"file_path": "/tmp/a.rs"});
        let v2 = json!({"file_path": "/tmp/b.rs"});
        assert_ne!(hash_value(&v1), hash_value(&v2));
    }

    // -- PendingToolCall tests --

    #[test]
    fn test_pending_tool_call_new() {
        let call = PendingToolCall::new("Read", json!({"file_path": "/tmp/a.rs"}));
        assert_eq!(call.tool_name, "Read");
        assert_eq!(call.input["file_path"], "/tmp/a.rs");
        assert_ne!(call.input_hash, 0);
    }

    // -- Integration-style tests --

    #[test]
    fn test_optimizer_mixed_batch() {
        let calls = vec![
            // 0: Read a.rs
            PendingToolCall::new("Read", json!({"file_path": "/tmp/a.rs"})),
            // 1: Edit a.rs
            PendingToolCall::new(
                "Edit",
                json!({"file_path": "/tmp/a.rs", "old_string": "x", "new_string": "y"}),
            ),
            // 2: Read a.rs (duplicate of 0)
            PendingToolCall::new("Read", json!({"file_path": "/tmp/a.rs"})),
            // 3: Read b.rs
            PendingToolCall::new("Read", json!({"file_path": "/tmp/b.rs"})),
            // 4: Edit c.rs
            PendingToolCall::new(
                "Edit",
                json!({"file_path": "/tmp/c.rs", "old_string": "a", "new_string": "b"}),
            ),
        ];

        let plan = ToolCallOptimizer::optimize(&calls);

        // Call 2 is a duplicate of call 0.
        assert!(plan.deduplicated.contains(&2));

        // Call 3 (Read b.rs) has no write dependency -> parallel.
        let all_parallel: HashSet<usize> = plan.parallel_groups.iter().flatten().copied().collect();
        assert!(
            all_parallel.contains(&3),
            "independent read should be parallel"
        );
    }

    #[test]
    fn test_optimizer_empty_batch() {
        let calls: Vec<PendingToolCall> = vec![];
        let plan = ToolCallOptimizer::optimize(&calls);

        assert!(plan.deduplicated.is_empty());
        assert!(plan.parallel_groups.is_empty());
        assert!(plan.sequential.is_empty());
    }

    #[test]
    fn test_optimizer_single_call() {
        let calls = vec![PendingToolCall::new(
            "Read",
            json!({"file_path": "/tmp/a.rs"}),
        )];
        let plan = ToolCallOptimizer::optimize(&calls);

        assert!(plan.deduplicated.is_empty());
        assert_eq!(plan.parallel_groups.len(), 1);
        assert!(plan.parallel_groups[0].contains(&0));
        assert!(plan.sequential.is_empty());
    }
}
