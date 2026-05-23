//! Types shared across the process pool module.

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

// ---------------------------------------------------------------------------
// Tool permission helpers
// ---------------------------------------------------------------------------

/// Simple glob pattern matching supporting `*` (any chars) and `?` (single char).
pub(crate) fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    glob_match_impl(&p, &t, 0, 0)
}

fn glob_match_impl(p: &[char], t: &[char], pi: usize, ti: usize) -> bool {
    if pi == p.len() {
        return ti == t.len();
    }
    if p[pi] == '*' {
        // '*' matches zero or more characters
        return glob_match_impl(p, t, pi + 1, ti) // match zero chars
            || (ti < t.len() && glob_match_impl(p, t, pi, ti + 1)); // consume one char
    }
    if ti < t.len() && (p[pi] == '?' || p[pi] == t[ti]) {
        return glob_match_impl(p, t, pi + 1, ti + 1);
    }
    false
}

/// Check whether a tool name is permitted by the given allow/deny patterns.
///
/// Pattern syntax:
/// - `mcp__fetch__*`  — allow all tools from the `fetch` server
/// - `!mcp__internal__*` — deny all tools from the `internal` server
/// - Empty patterns list → everything is allowed (default)
pub(crate) fn is_tool_allowed_by_patterns(tool_name: &str, patterns: &[String]) -> bool {
    if patterns.is_empty() {
        return true; // No restrictions configured → allow all
    }

    let mut has_allow_patterns = false;
    let mut denied = false;
    let mut explicitly_allowed = false;

    for pattern in patterns {
        if let Some(deny_pattern) = pattern.strip_prefix('!') {
            // Deny rule — check first
            if glob_match(deny_pattern, tool_name) {
                denied = true;
            }
        } else {
            has_allow_patterns = true;
            if glob_match(pattern, tool_name) {
                explicitly_allowed = true;
            }
        }
    }

    if denied {
        return false;
    }

    if has_allow_patterns {
        return explicitly_allowed;
    }

    // Only deny patterns were specified and this tool wasn't denied → allow
    true
}

// ---------------------------------------------------------------------------
// Tool result chunking store
// ---------------------------------------------------------------------------

/// Stores oversized tool results so they can be retrieved in chunks later.
///
/// When a tool result is compressed or truncated, the full content is stored
/// here with a unique chunk ID. The LLM can then request the full result
/// or the next chunk if needed.
pub(crate) struct ToolResultStore {
    /// Full results keyed by chunk ID.
    results: DashMap<String, StoredResult>,
    /// Maximum age for stored results (auto-evicted).
    max_age: Duration,
}

/// A stored tool result with metadata.
struct StoredResult {
    /// The full content.
    full_content: String,
    /// Tool name that produced this result.
    tool_name: String,
    /// When this result was stored.
    stored_at: Instant,
}

impl ToolResultStore {
    pub(crate) fn new() -> Self {
        Self {
            results: DashMap::new(),
            max_age: Duration::from_secs(300), // 5 minutes
        }
    }

    /// Store a result and return its chunk ID.
    pub(crate) fn store(&self, tool_name: &str, full_content: String) -> String {
        // Periodically evict expired entries to prevent unbounded memory growth
        if !self.results.is_empty() && self.results.len() % 8 == 0 {
            self.evict_expired();
        }
        let id = format!("chunk_{}", uuid::Uuid::new_v4().as_simple());
        self.results.insert(
            id.clone(),
            StoredResult {
                full_content,
                tool_name: tool_name.to_string(),
                stored_at: Instant::now(),
            },
        );
        id
    }

    /// Get the full content for a chunk ID.
    pub(crate) fn get_full(&self, chunk_id: &str) -> Option<(String, String)> {
        self.results
            .get(chunk_id)
            .map(|r| (r.tool_name.clone(), r.full_content.clone()))
    }

    /// Get a specific chunk (offset, length) of a stored result.
    pub(crate) fn get_chunk(
        &self,
        chunk_id: &str,
        offset: usize,
        max_chars: usize,
    ) -> Option<ChunkResult> {
        self.results.get(chunk_id).map(|r| {
            let content = &r.full_content;
            let total_len = content.len();
            if offset >= total_len {
                return ChunkResult {
                    content: String::new(),
                    offset,
                    total_len,
                    has_more: false,
                    tool_name: r.tool_name.clone(),
                };
            }
            // Find safe char boundary
            let mut end = (offset + max_chars).min(total_len);
            while !content.is_char_boundary(end) && end > offset {
                end -= 1;
            }
            let has_more = end < total_len;
            ChunkResult {
                content: content[offset..end].to_string(),
                offset: end,
                total_len,
                has_more,
                tool_name: r.tool_name.clone(),
            }
        })
    }

    /// Evict expired results.
    pub(crate) fn evict_expired(&self) {
        self.results
            .retain(|_, v| v.stored_at.elapsed() < self.max_age);
    }
}

/// Result of retrieving a chunk from the store.
pub struct ChunkResult {
    /// Content of this chunk.
    pub content: String,
    /// Byte offset for the next chunk request.
    pub offset: usize,
    /// Total byte length of the full stored result.
    pub total_len: usize,
    /// Whether more content remains after this chunk.
    pub has_more: bool,
    /// Tool name that produced this result.
    pub tool_name: String,
}

/// Maximum length for MCP tool descriptions (in characters).
///
/// Some servers dump 15-60KB into `tool.description`, wasting ~15K tokens per turn.
/// Claude Code caps at 2,048 chars; we match that.
pub(crate) const MAX_TOOL_DESCRIPTION_CHARS: usize = 2048;

/// Maximum length for MCP tool results (in characters).
///
/// Some MCP tools return 100KB+ responses. Sending all of that to the LLM wastes
/// tokens and degrades response quality. Claude Code truncates at ~25K chars.
pub(crate) const MAX_TOOL_RESULT_CHARS: usize = 25_000;

/// Default timeout for establishing a new MCP server connection (initialize handshake).
pub(crate) const DEFAULT_CONNECTION_TIMEOUT_SECS: u64 = 30;

/// Default timeout for regular JSON-RPC requests (tools/list, ping, etc.).
pub(crate) const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 60;

/// Default timeout for tool call requests (tools/call).
///
/// Tool calls can be long-running (e.g. file search, code analysis).
/// Claude Code uses a very generous timeout (~27.8h); we use 10 minutes
/// which covers virtually all realistic tool executions.
pub(crate) const DEFAULT_TOOL_TIMEOUT_SECS: u64 = 600;

/// Compress a tool result string to fit within [`MAX_TOOL_RESULT_CHARS`].
///
/// Uses format-aware strategies instead of simple truncation:
/// - **JSON arrays**: show first N items + item count summary
/// - **JSON objects**: show all keys with truncated values
/// - **Stack traces / line-based text**: show first/last lines + line count
/// - **Long text**: paragraph-aware truncation
///
/// For content that is already within budget, returns it unchanged.
pub(crate) fn truncate_tool_result(content: &str, budget: usize) -> String {
    if content.len() <= budget {
        return content.to_string();
    }

    let original_len = content.len();
    let trimmed = content.trim();

    // Strategy 1: Try JSON-aware compression for JSON content.
    if trimmed.starts_with('[') || trimmed.starts_with('{') {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
            let compressed = compress_json(&value, budget);
            let pct = ((original_len - compressed.len()) as f64 / original_len as f64) * 100.0;
            return format!(
                "{}\n\n[compressed: showed ~{} of ~{} chars ({:.0}% omitted)]",
                compressed,
                compressed.len(),
                original_len,
                pct,
            );
        }
    }

    // Strategy 2: Line-based compression for structured text (stack traces, logs).
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() > 20 {
        let head_budget = budget / 2;
        let tail_budget = budget / 2;

        let mut head_lines = Vec::new();
        let mut head_len = 0;
        for line in &lines {
            if head_len + line.len() + 1 > head_budget {
                break;
            }
            head_lines.push(*line);
            head_len += line.len() + 1;
        }

        let mut tail_lines = Vec::new();
        let mut tail_len = 0;
        for line in lines.iter().rev() {
            if tail_len + line.len() + 1 > tail_budget {
                break;
            }
            tail_lines.push(*line);
            tail_len += line.len() + 1;
        }
        tail_lines.reverse();

        let omitted_lines = lines.len() - head_lines.len() - tail_lines.len();
        let head_text = head_lines.join("\n");
        let tail_text = tail_lines.join("\n");
        let pct = ((original_len - head_text.len() - tail_text.len()) as f64 / original_len as f64)
            * 100.0;

        return format!(
            "{}\n\n... [{} lines omitted] ...\n\n{}\n\n[compressed: showed ~{} of ~{} chars ({:.0}% omitted)]",
            head_text,
            omitted_lines,
            tail_text,
            head_text.len() + tail_text.len(),
            original_len,
            pct,
        );
    }

    // Strategy 3: Paragraph-aware truncation for prose text.
    let mut end = budget;
    while !content.is_char_boundary(end) && end > 0 {
        end -= 1;
    }
    // Try to cut at a paragraph boundary (double newline).
    let truncated = &content[..end];
    let cut = truncated
        .rfind("\n\n")
        .or_else(|| truncated.rfind('\n'))
        .unwrap_or(end);
    let cut = if content.is_char_boundary(cut) {
        cut
    } else {
        end
    };
    let pct = ((original_len - cut) as f64 / original_len as f64) * 100.0;
    format!(
        "{}\n\n[compressed: showed ~{} of ~{} chars ({:.0}% omitted)]",
        &content[..cut],
        cut,
        original_len,
        pct,
    )
}

/// Format-aware JSON compression.
///
/// - Arrays: show first N items + summary of remaining count.
/// - Objects: show all keys with truncated values.
/// - Primitives: pass through.
pub(crate) fn compress_json(value: &serde_json::Value, budget: usize) -> String {
    match value {
        serde_json::Value::Array(items) => {
            if items.is_empty() {
                return "[]".to_string();
            }
            // Determine how many items fit in budget.
            let mut result = String::from("[\n");
            let mut shown = 0;
            for item in items {
                let item_str = format!("  {},\n", serde_json::to_string(item).unwrap_or_default());
                if result.len() + item_str.len() + 50 > budget {
                    break;
                }
                result.push_str(&item_str);
                shown += 1;
            }
            let remaining = items.len() - shown;
            if remaining > 0 {
                result.push_str(&format!("  // ... {remaining} more items\n"));
            }
            result.push(']');
            result
        }
        serde_json::Value::Object(map) => {
            if map.is_empty() {
                return "{}".to_string();
            }
            let mut result = String::from("{\n");
            let value_budget = 200; // max chars per value
            let total_keys = map.len();
            for (i, (key, val)) in map.into_iter().enumerate() {
                let val_str = serde_json::to_string(val).unwrap_or_default();
                let display_val = if val_str.len() > value_budget {
                    let mut v_end = value_budget;
                    while !val_str.is_char_boundary(v_end) && v_end > 0 {
                        v_end -= 1;
                    }
                    format!("{}…", &val_str[..v_end])
                } else {
                    val_str
                };
                let line = format!("  \"{key}\": {display_val},\n");
                if result.len() + line.len() + 30 > budget {
                    let remaining = total_keys - i;
                    result.push_str(&format!("  // ... {remaining} more keys\n"));
                    break;
                }
                result.push_str(&line);
            }
            result.push('}');
            result
        }
        _ => {
            let s = serde_json::to_string(value).unwrap_or_default();
            if s.len() <= budget {
                s
            } else {
                let mut end = budget;
                while !s.is_char_boundary(end) && end > 0 {
                    end -= 1;
                }
                format!("{}...", &s[..end])
            }
        }
    }
}

/// Normalize multi-element error content into a single coherent error message.
///
/// MCP servers can return errors with multiple content blocks of different types
/// (text, images, embedded resources). This function extracts all blocks into a
/// single string, summarizing non-text content.
pub(crate) fn normalize_error_content(content_array: &[serde_json::Value]) -> String {
    content_array
        .iter()
        .map(|block| match block.get("type").and_then(|t| t.as_str()) {
            Some("text") => block
                .get("text")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string(),
            Some("image") => {
                let mime = block
                    .get("mimeType")
                    .and_then(|m| m.as_str())
                    .unwrap_or("image/unknown");
                format!("[{mime} image]")
            }
            Some("resource") => {
                let uri = block
                    .get("resource")
                    .and_then(|r| r.get("uri"))
                    .and_then(|u| u.as_str())
                    .unwrap_or("unknown");
                let text = block
                    .get("resource")
                    .and_then(|r| r.get("text"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                if text.is_empty() {
                    format!("[resource: {uri}]")
                } else {
                    format!("[resource: {uri}]\n{text}")
                }
            }
            other => {
                let text = block.get("text").and_then(|t| t.as_str()).unwrap_or("");
                if text.is_empty() {
                    format!("[{} block]", other.unwrap_or("unknown"))
                } else {
                    text.to_string()
                }
            }
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// Server state
// ---------------------------------------------------------------------------

/// Lifecycle state of an MCP server process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerState {
    /// Process is being started and initialized.
    Starting,
    /// Process is healthy and accepting requests.
    Healthy,
    /// Process failed health check or crashed. Contains error message.
    Unhealthy(String),
    /// Process has been shut down.
    Stopped,
}

/// Runtime status of an MCP server process, including health metrics.
#[derive(Debug, Clone, Serialize)]
pub struct ServerStatus {
    /// Server name.
    pub name: String,
    /// Current lifecycle state.
    pub state: ServerState,
    /// Time since the server was last started (None if not running).
    pub uptime: Option<Duration>,
    /// Total number of tool call requests sent.
    pub request_count: u64,
    /// Total number of failed requests.
    pub error_count: u64,
    /// Number of restart attempts since initial start.
    pub restart_count: u64,
    /// Time since last successful health check (None if never checked).
    pub last_health_check: Option<Duration>,
    /// Total bytes of tool result content across all calls (approximate token usage / 4).
    pub total_result_bytes: u64,
    /// Configured budget in bytes for this server (None = unlimited).
    pub budget_bytes: Option<u64>,
}

// ---------------------------------------------------------------------------
// Pending request tracking
// ---------------------------------------------------------------------------

/// A pending JSON-RPC request waiting for a response.
pub(crate) struct PendingRequest {
    /// Oneshot channel to deliver the response.
    pub(crate) tx: oneshot::Sender<Value>,
    /// When this request was created (for timeout tracking).
    #[allow(dead_code)]
    pub(crate) created_at: Instant,
    /// Optional progress token sent in `_meta.progressToken`.
    pub(crate) progress_token: Option<Value>,
    /// Optional callback invoked on `notifications/progress` for this request.
    pub(crate) on_progress: Option<Arc<dyn Fn(f64, Option<f64>) + Send + Sync>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_exact_match() {
        assert!(glob_match("hello", "hello"));
        assert!(!glob_match("hello", "world"));
    }

    #[test]
    fn glob_star_matches_zero_or_more() {
        assert!(glob_match("mcp__*", "mcp__fetch"));
        assert!(glob_match("mcp__*", "mcp__"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("prefix*", "prefix_suffix"));
        assert!(glob_match("*suffix", "some_suffix"));
    }

    #[test]
    fn glob_question_mark_single_char() {
        assert!(glob_match("mcp_?", "mcp_X"));
        assert!(!glob_match("mcp_?", "mcp_XY"));
        assert!(!glob_match("mcp_?", "mcp_"));
    }

    #[test]
    fn glob_combined_patterns() {
        assert!(glob_match("mcp__fetch__*", "mcp__fetch__fetch_url"));
        assert!(glob_match("?cp__*", "mcp__fetch"));
    }

    #[test]
    fn glob_no_match() {
        assert!(!glob_match("abc", "def"));
        assert!(!glob_match("mcp__internal__*", "mcp__fetch__tool"));
    }

    #[test]
    fn allow_all_when_empty_patterns() {
        assert!(is_tool_allowed_by_patterns("any_tool", &[]));
    }

    #[test]
    fn allow_pattern_matches() {
        let patterns = vec!["mcp__fetch__*".to_string()];
        assert!(is_tool_allowed_by_patterns(
            "mcp__fetch__fetch_url",
            &patterns
        ));
        assert!(!is_tool_allowed_by_patterns(
            "mcp__internal__secret",
            &patterns
        ));
    }

    #[test]
    fn deny_pattern_overrides_allow() {
        let patterns = vec!["mcp__*".to_string(), "!mcp__internal__*".to_string()];
        assert!(is_tool_allowed_by_patterns("mcp__fetch__tool", &patterns));
        assert!(!is_tool_allowed_by_patterns(
            "mcp__internal__secret",
            &patterns
        ));
    }

    #[test]
    fn deny_only_patterns_allow_non_denied() {
        let patterns = vec!["!mcp__internal__*".to_string()];
        assert!(is_tool_allowed_by_patterns("mcp__fetch__tool", &patterns));
        assert!(!is_tool_allowed_by_patterns(
            "mcp__internal__secret",
            &patterns
        ));
    }

    #[test]
    fn multiple_allow_patterns() {
        let patterns = vec!["mcp__fetch__*".to_string(), "mcp__tavily__*".to_string()];
        assert!(is_tool_allowed_by_patterns("mcp__fetch__fetch", &patterns));
        assert!(is_tool_allowed_by_patterns(
            "mcp__tavily__search",
            &patterns
        ));
        assert!(!is_tool_allowed_by_patterns(
            "mcp__internal__tool",
            &patterns
        ));
    }

    #[test]
    fn truncate_within_budget_unchanged() {
        let content = "short content";
        assert_eq!(truncate_tool_result(content, 100), content);
    }

    #[test]
    fn truncate_exact_budget_unchanged() {
        let content = "x".repeat(50);
        assert_eq!(truncate_tool_result(&content, 50), content);
    }

    #[test]
    fn truncate_json_array() {
        let items: Vec<String> = (0..200).map(|i| format!("item_{i}")).collect();
        let content = serde_json::to_string(&items).unwrap();
        let result = truncate_tool_result(&content, 200);
        assert!(result.contains("compressed"));
        assert!(result.len() < content.len());
    }

    #[test]
    fn truncate_json_object() {
        let mut map = serde_json::Map::new();
        for i in 0..50 {
            map.insert(
                format!("key_{i}"),
                serde_json::Value::String("x".repeat(100)),
            );
        }
        let content = serde_json::to_string(&map).unwrap();
        let result = truncate_tool_result(&content, 300);
        assert!(result.contains("compressed"));
    }

    #[test]
    fn truncate_line_based_text() {
        let lines: Vec<String> = (0..100)
            .map(|i| format!("line {i}: some content here"))
            .collect();
        let content = lines.join("\n");
        let result = truncate_tool_result(&content, 200);
        assert!(result.contains("lines omitted"));
    }

    #[test]
    fn truncate_prose_text() {
        let content = "short paragraph\n\n".repeat(50);
        let result = truncate_tool_result(&content, 100);
        assert!(result.contains("compressed"));
    }

    #[test]
    fn compress_empty_array() {
        assert_eq!(compress_json(&serde_json::json!([]), 100), "[]");
    }

    #[test]
    fn compress_empty_object() {
        assert_eq!(compress_json(&serde_json::json!({}), 100), "{}");
    }

    #[test]
    fn compress_array_fits_items() {
        let arr = serde_json::json!([1, 2, 3]);
        let result = compress_json(&arr, 500);
        assert!(result.contains('1'));
        assert!(result.contains('3'));
    }

    #[test]
    fn compress_array_truncates_large() {
        let items: Vec<i32> = (0..1000).collect();
        let arr = serde_json::json!(items);
        let result = compress_json(&arr, 50);
        assert!(result.contains("more items"));
    }

    #[test]
    fn compress_object_truncates_values() {
        let obj = serde_json::json!({"key": "a".repeat(500)});
        let result = compress_json(&obj, 300);
        assert!(result.contains("key"));
    }

    #[test]
    fn compress_primitive_within_budget() {
        assert_eq!(compress_json(&serde_json::json!("hello"), 100), "\"hello\"");
    }

    #[test]
    fn compress_primitive_over_budget() {
        let result = compress_json(&serde_json::json!("x".repeat(200)), 50);
        assert!(result.len() <= 53);
    }

    #[test]
    fn normalize_text_blocks() {
        let blocks = vec![
            serde_json::json!({"type": "text", "text": "Error 1"}),
            serde_json::json!({"type": "text", "text": "Error 2"}),
        ];
        let result = normalize_error_content(&blocks);
        assert_eq!(result, "Error 1\nError 2");
    }

    #[test]
    fn normalize_image_block() {
        let blocks = vec![serde_json::json!({"type": "image", "mimeType": "image/png"})];
        let result = normalize_error_content(&blocks);
        assert!(result.contains("image/png"));
    }

    #[test]
    fn normalize_resource_block_with_text() {
        let blocks = vec![serde_json::json!({
            "type": "resource",
            "resource": {"uri": "file:///foo", "text": "content"}
        })];
        let result = normalize_error_content(&blocks);
        assert!(result.contains("file:///foo"));
        assert!(result.contains("content"));
    }

    #[test]
    fn normalize_resource_block_without_text() {
        let blocks = vec![serde_json::json!({
            "type": "resource",
            "resource": {"uri": "file:///bar"}
        })];
        let result = normalize_error_content(&blocks);
        assert!(result.contains("file:///bar"));
        assert!(!result.contains("\n"));
    }

    #[test]
    fn normalize_unknown_block_with_text() {
        let blocks = vec![serde_json::json!({"type": "custom", "text": "custom content"})];
        let result = normalize_error_content(&blocks);
        assert_eq!(result, "custom content");
    }

    #[test]
    fn normalize_empty_blocks() {
        assert_eq!(normalize_error_content(&[]), "");
    }

    #[test]
    fn normalize_filters_empty_text() {
        let blocks = vec![
            serde_json::json!({"type": "text", "text": ""}),
            serde_json::json!({"type": "text", "text": "visible"}),
        ];
        let result = normalize_error_content(&blocks);
        assert_eq!(result, "visible");
    }

    #[test]
    fn store_and_retrieve_full() {
        let store = ToolResultStore::new();
        let id = store.store("test_tool", "full content here".to_string());
        let (tool_name, content) = store.get_full(&id).unwrap();
        assert_eq!(tool_name, "test_tool");
        assert_eq!(content, "full content here");
    }

    #[test]
    fn store_missing_id_returns_none() {
        let store = ToolResultStore::new();
        assert!(store.get_full("nonexistent").is_none());
    }

    #[test]
    fn store_chunk_retrieval() {
        let store = ToolResultStore::new();
        let content = "0123456789".repeat(10);
        let id = store.store("tool", content);
        let chunk = store.get_chunk(&id, 0, 20).unwrap();
        assert_eq!(chunk.content.len(), 20);
        assert!(chunk.has_more);
        assert_eq!(chunk.tool_name, "tool");
    }

    #[test]
    fn store_chunk_beyond_end() {
        let store = ToolResultStore::new();
        let id = store.store("tool", "short".to_string());
        let chunk = store.get_chunk(&id, 100, 10).unwrap();
        assert!(chunk.content.is_empty());
        assert!(!chunk.has_more);
    }

    #[test]
    fn server_state_serialization() {
        let states = vec![
            ServerState::Starting,
            ServerState::Healthy,
            ServerState::Unhealthy("timeout".to_string()),
            ServerState::Stopped,
        ];
        let json = serde_json::to_string(&states).unwrap();
        let de: Vec<ServerState> = serde_json::from_str(&json).unwrap();
        assert_eq!(de, states);
    }

    #[test]
    fn server_state_equality() {
        assert_eq!(ServerState::Starting, ServerState::Starting);
        assert_ne!(ServerState::Healthy, ServerState::Stopped);
        assert_eq!(
            ServerState::Unhealthy("err".to_string()),
            ServerState::Unhealthy("err".to_string())
        );
    }

    #[test]
    fn constants_reasonable_values() {
        assert_eq!(MAX_TOOL_DESCRIPTION_CHARS, 2048);
        assert_eq!(MAX_TOOL_RESULT_CHARS, 25_000);
        assert_eq!(DEFAULT_CONNECTION_TIMEOUT_SECS, 30);
        assert_eq!(DEFAULT_REQUEST_TIMEOUT_SECS, 60);
        assert_eq!(DEFAULT_TOOL_TIMEOUT_SECS, 600);
        assert!(DEFAULT_TOOL_TIMEOUT_SECS > DEFAULT_REQUEST_TIMEOUT_SECS);
    }

    #[test]
    fn server_status_serialization() {
        let status = ServerStatus {
            name: "test_server".to_string(),
            state: ServerState::Healthy,
            uptime: Some(Duration::from_secs(60)),
            request_count: 42,
            error_count: 2,
            restart_count: 0,
            last_health_check: Some(Duration::from_secs(5)),
            total_result_bytes: 1024,
            budget_bytes: Some(1_000_000),
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("test_server"));
        assert!(json.contains("Healthy"));
        assert!(json.contains("42"));
    }

    #[test]
    fn send_sync_types() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ServerState>();
        assert_send_sync::<ServerStatus>();
        assert_send_sync::<ToolResultStore>();
        assert_send_sync::<ChunkResult>();
    }
}
