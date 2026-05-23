//! PooledMcpToolAdapter — routes tool calls through the persistent process pool.

use async_trait::async_trait;
use serde_json::Value;
use shannon_tool_interface::{Tool, ToolError, ToolOutput, ToolResult};
use std::sync::Arc;

use super::McpProcessPool;
use super::types::*;

// ---------------------------------------------------------------------------
// Pooled MCP Tool Adapter
// ---------------------------------------------------------------------------

/// A tool adapter that routes calls through the persistent process pool.
///
/// Unlike `McpToolAdapter` (which spawns a fresh process per call),
/// this adapter uses the pool's persistent connections for zero-overhead
/// tool execution after initial startup.
pub struct PooledMcpToolAdapter {
    /// Shared reference to the process pool.
    pool: Arc<McpProcessPool>,
    /// Server name in the pool.
    server_name: String,
    /// Tool name on the MCP server side.
    remote_tool_name: String,
    /// Human-readable description.
    description: String,
    /// JSON Schema for tool input.
    input_schema: Value,
    /// Tool name in the registry (e.g., "mcp__fetch__fetch").
    pub(crate) tool_name: String,
    /// Behavioral hints from the MCP server (readOnly, destructive, etc.).
    annotations: Option<crate::ToolAnnotations>,
    /// Per-tool output limit in chars (from `_meta.maxResultSizeChars`).
    /// Overrides the pool's global `max_output_chars` when set.
    max_output_chars: Option<usize>,
    /// Per-tool timeout in seconds (from `_meta.timeoutSeconds`).
    /// Overrides the handle's default `tool_timeout` when set.
    tool_timeout_secs: Option<u64>,
}

impl PooledMcpToolAdapter {
    /// Create a new pooled tool adapter.
    pub fn new(
        pool: Arc<McpProcessPool>,
        server_name: String,
        remote_tool_name: String,
        description: String,
        input_schema: Value,
        annotations: Option<crate::ToolAnnotations>,
    ) -> Self {
        Self::with_output_limit(
            pool,
            server_name,
            remote_tool_name,
            description,
            input_schema,
            annotations,
            None,
            None,
        )
    }

    /// Create a pooled tool adapter with explicit per-tool overrides.
    ///
    /// `max_output_chars` overrides the pool's global limit (from `_meta.maxResultSizeChars`).
    /// `tool_timeout_secs` overrides the handle's default timeout (from `_meta.timeoutSeconds`).
    #[allow(clippy::too_many_arguments)]
    pub fn with_output_limit(
        pool: Arc<McpProcessPool>,
        server_name: String,
        remote_tool_name: String,
        description: String,
        input_schema: Value,
        annotations: Option<crate::ToolAnnotations>,
        max_output_chars: Option<usize>,
        tool_timeout_secs: Option<u64>,
    ) -> Self {
        let tool_name = format!("mcp__{server_name}__{remote_tool_name}");
        // Truncate oversized descriptions to avoid wasting context tokens.
        let description = if description.len() > MAX_TOOL_DESCRIPTION_CHARS {
            let mut end = MAX_TOOL_DESCRIPTION_CHARS;
            while !description.is_char_boundary(end) && end > 0 {
                end -= 1;
            }
            format!("{}…", &description[..end])
        } else {
            description
        };
        Self {
            pool,
            server_name,
            remote_tool_name,
            description,
            input_schema,
            tool_name,
            annotations,
            max_output_chars,
            tool_timeout_secs,
        }
    }

    /// Internal helper: calls the tool via the pool, using progress reporting
    /// when a progress callback is registered on the pool.
    async fn call_tool_inner(&self, input: Value) -> ToolResult<ToolOutput> {
        // Use per-tool limit if set, otherwise use pool's global default.
        let max_chars = self.max_output_chars.unwrap_or(self.pool.max_output_chars);

        let fut = async {
            let progress_cb = self.pool.progress_callback.lock().await;
            if let Some(ref cb) = *progress_cb {
                let tool_name = self.tool_name.clone();
                let cb = cb.clone();
                drop(progress_cb);

                let on_progress = Arc::new(move |progress: f64, total: Option<f64>| {
                    cb(&tool_name, progress, total);
                });

                self.pool
                    .call_tool_with_progress_and_limit(
                        &self.server_name,
                        &self.remote_tool_name,
                        input,
                        on_progress,
                        max_chars,
                    )
                    .await
            } else {
                drop(progress_cb);
                self.pool
                    .call_tool_with_limit(
                        &self.server_name,
                        &self.remote_tool_name,
                        input,
                        max_chars,
                    )
                    .await
            }
        };

        // Apply per-tool timeout if specified (from _meta.timeoutSeconds).
        if let Some(secs) = self.tool_timeout_secs {
            tokio::time::timeout(std::time::Duration::from_secs(secs), fut)
                .await
                .map_err(|_| {
                    ToolError::ExecutionFailed(format!(
                        "MCP tool '{}' timed out after {secs}s (per-tool timeout)",
                        self.tool_name
                    ))
                })?
        } else {
            fut.await
        }
    }

    /// Produce a deterministic, sorted JSON string for cache key stability.
    fn sorted_args(input: &Value) -> String {
        match input {
            Value::Object(map) => {
                let mut pairs: Vec<(String, String)> = map
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_string()))
                    .collect();
                pairs.sort_by(|a, b| a.0.cmp(&b.0));
                pairs
                    .into_iter()
                    .map(|(k, v)| format!("{k}:{v}"))
                    .collect::<Vec<_>>()
                    .join(",")
            }
            other => other.to_string(),
        }
    }
}

impl std::fmt::Debug for PooledMcpToolAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PooledMcpToolAdapter")
            .field("server_name", &self.server_name)
            .field("tool_name", &self.tool_name)
            .finish()
    }
}

#[async_trait]
impl Tool for PooledMcpToolAdapter {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        // When deferred mode is enabled, return a minimal stub to save context.
        // The real schema is available via pool.get_deferred_schema() / McpToolSearchTool.
        if self.pool.is_defer_tool_schemas() {
            serde_json::json!({
                "type": "object",
                "description": format!(
                    "Use the mcp__tool_search tool with tool_name=\"{}\" to get the full parameter schema before calling this tool.",
                    self.tool_name
                )
            })
        } else {
            self.input_schema.clone()
        }
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        // Check tool permission against allowlist patterns.
        if !self.pool.is_tool_allowed(&self.tool_name).await {
            return Err(ToolError::ExecutionFailed(format!(
                "Tool '{}' is not in the allowed tools list",
                self.tool_name
            )));
        }

        // Check cache for read-only tools.
        if self.is_read_only() {
            let cache_key = format!(
                "{}:{}:{}",
                self.server_name,
                self.remote_tool_name,
                Self::sorted_args(&input)
            );
            if let Some(cached) = self.pool.get_cached(&cache_key).await {
                tracing::debug!(
                    server = %self.server_name,
                    tool = %self.remote_tool_name,
                    "Returning cached tool result"
                );
                return Ok(ToolOutput::success(cached));
            }

            let result = self.call_tool_inner(input.clone()).await?;

            // Store in cache on success.
            if !result.is_error {
                self.pool
                    .put_cached(&cache_key, result.content.clone())
                    .await;
            }

            return Ok(result);
        }

        self.call_tool_inner(input).await
    }

    fn requires_auth(&self) -> bool {
        false
    }

    fn category(&self) -> &str {
        "mcp"
    }

    fn is_read_only(&self) -> bool {
        self.annotations.as_ref().is_some_and(|a| a.read_only_hint)
    }

    fn is_concurrency_safe(&self) -> bool {
        // Idempotent or read-only tools are safe to run concurrently.
        self.annotations
            .as_ref()
            .is_some_and(|a| a.read_only_hint || a.idempotent_hint)
    }

    fn is_destructive(&self) -> bool {
        self.annotations
            .as_ref()
            .is_some_and(|a| a.destructive_hint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_adapter(annotations: Option<crate::ToolAnnotations>) -> PooledMcpToolAdapter {
        let pool = Arc::new(McpProcessPool::new());
        PooledMcpToolAdapter::new(
            pool,
            "test-server".to_string(),
            "my_tool".to_string(),
            "A test tool".to_string(),
            serde_json::json!({"type": "object"}),
            annotations,
        )
    }

    #[test]
    fn test_tool_name_format() {
        let adapter = make_adapter(None);
        assert_eq!(adapter.name(), "mcp__test-server__my_tool");
    }

    #[test]
    fn test_description() {
        let adapter = make_adapter(None);
        assert_eq!(adapter.description(), "A test tool");
    }

    #[test]
    fn test_category() {
        let adapter = make_adapter(None);
        assert_eq!(adapter.category(), "mcp");
    }

    #[test]
    fn test_requires_auth_is_false() {
        let adapter = make_adapter(None);
        assert!(!adapter.requires_auth());
    }

    #[test]
    fn test_is_read_only_with_annotations() {
        let read_only = make_adapter(Some(crate::ToolAnnotations {
            read_only_hint: true,
            destructive_hint: false,
            idempotent_hint: false,
            open_world_hint: false,
        }));
        assert!(read_only.is_read_only());

        let writable = make_adapter(Some(crate::ToolAnnotations {
            read_only_hint: false,
            destructive_hint: false,
            idempotent_hint: false,
            open_world_hint: false,
        }));
        assert!(!writable.is_read_only());
    }

    #[test]
    fn test_is_read_only_without_annotations() {
        let adapter = make_adapter(None);
        assert!(!adapter.is_read_only());
    }

    #[test]
    fn test_is_destructive() {
        let destructive = make_adapter(Some(crate::ToolAnnotations {
            read_only_hint: false,
            destructive_hint: true,
            idempotent_hint: false,
            open_world_hint: false,
        }));
        assert!(destructive.is_destructive());

        let safe = make_adapter(Some(crate::ToolAnnotations {
            read_only_hint: false,
            destructive_hint: false,
            idempotent_hint: false,
            open_world_hint: false,
        }));
        assert!(!safe.is_destructive());
    }

    #[test]
    fn test_is_concurrency_safe_read_only() {
        let read_only = make_adapter(Some(crate::ToolAnnotations {
            read_only_hint: true,
            destructive_hint: false,
            idempotent_hint: false,
            open_world_hint: false,
        }));
        assert!(read_only.is_concurrency_safe());
    }

    #[test]
    fn test_is_concurrency_safe_idempotent() {
        let idempotent = make_adapter(Some(crate::ToolAnnotations {
            read_only_hint: false,
            destructive_hint: false,
            idempotent_hint: true,
            open_world_hint: false,
        }));
        assert!(idempotent.is_concurrency_safe());
    }

    #[test]
    fn test_is_concurrency_safe_neither() {
        let neither = make_adapter(Some(crate::ToolAnnotations {
            read_only_hint: false,
            destructive_hint: false,
            idempotent_hint: false,
            open_world_hint: false,
        }));
        assert!(!neither.is_concurrency_safe());
    }

    #[test]
    fn test_input_schema_normal() {
        let adapter = make_adapter(None);
        let schema = adapter.input_schema();
        assert_eq!(schema["type"], "object");
    }

    #[test]
    fn test_sorted_args_object() {
        let input = serde_json::json!({"z": 1, "a": 2, "m": 3});
        let sorted = PooledMcpToolAdapter::sorted_args(&input);
        assert!(sorted.starts_with("a:"));
        let z_pos = sorted.find("z:").unwrap();
        let a_pos = sorted.find("a:").unwrap();
        assert!(a_pos < z_pos);
    }

    #[test]
    fn test_sorted_args_non_object() {
        let input = serde_json::json!("hello");
        let sorted = PooledMcpToolAdapter::sorted_args(&input);
        assert_eq!(sorted, "\"hello\"");
    }

    #[test]
    fn test_description_truncation() {
        let pool = Arc::new(McpProcessPool::new());
        let long_desc = "x".repeat(3000);
        let adapter = PooledMcpToolAdapter::new(
            pool,
            "srv".to_string(),
            "tool".to_string(),
            long_desc.clone(),
            serde_json::json!({"type": "object"}),
            None,
        );
        assert!(adapter.description().len() < long_desc.len());
        assert!(adapter.description().ends_with('…'));
    }

    #[test]
    fn test_debug_format() {
        let adapter = make_adapter(None);
        let debug = format!("{adapter:?}");
        assert!(debug.contains("test-server"));
        assert!(debug.contains("mcp__test-server__my_tool"));
    }

    #[test]
    fn test_with_output_limit() {
        let pool = Arc::new(McpProcessPool::new());
        let adapter = PooledMcpToolAdapter::with_output_limit(
            pool,
            "srv".to_string(),
            "tool".to_string(),
            "desc".to_string(),
            serde_json::json!({"type": "object"}),
            None,
            Some(5000),
            Some(60),
        );
        assert_eq!(adapter.name(), "mcp__srv__tool");
    }
}
