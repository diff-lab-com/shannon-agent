//! Tests for the process pool module.

use super::*;
use super::types::{
    glob_match, is_tool_allowed_by_patterns, normalize_error_content,
    truncate_tool_result, MAX_TOOL_DESCRIPTION_CHARS, MAX_TOOL_RESULT_CHARS,
    PendingRequest, ServerState,
};
use super::handle::McpServerHandle;
use super::adapter::PooledMcpToolAdapter;
use dashmap::DashMap;
use serde_json::Value;
use shannon_tool_interface::{Tool, ToolOutput};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::collections::HashMap;
use tokio::sync::{Mutex, RwLock};

    #[test]
    fn test_pooled_tool_adapter_name() {
        let pool = Arc::new(McpProcessPool::new());
        let adapter = PooledMcpToolAdapter::new(
            pool,
            "test-server".to_string(),
            "fetch".to_string(),
            "Fetch a URL".to_string(),
            serde_json::json!({"type": "object"}),
            None,
        );
        assert_eq!(adapter.name(), "mcp__test-server__fetch");
    }

    #[test]
    fn test_pooled_tool_adapter_description() {
        let pool = Arc::new(McpProcessPool::new());
        let adapter = PooledMcpToolAdapter::new(
            pool,
            "srv".to_string(),
            "search".to_string(),
            "Search the web".to_string(),
            serde_json::json!({"type": "object"}),
            None,
        );
        assert_eq!(adapter.description(), "Search the web");
    }

    #[test]
    fn test_deferred_tool_schema_off_by_default() {
        let pool = McpProcessPool::new();
        assert!(!pool.is_defer_tool_schemas());
    }

    #[test]
    fn test_deferred_tool_schema_returns_minimal_when_enabled() {
        let pool = Arc::new(McpProcessPool::new());
        pool.set_defer_tool_schemas(true);

        let full_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "url": {"type": "string", "description": "The URL to fetch"},
                "method": {"type": "string", "enum": ["GET", "POST"]}
            },
            "required": ["url"]
        });

        let adapter = PooledMcpToolAdapter::new(
            pool.clone(),
            "fetch".to_string(),
            "fetch".to_string(),
            "Fetch a URL".to_string(),
            full_schema.clone(),
            None,
        );

        // Store the deferred schema.
        pool.store_deferred_schema(adapter.name(), full_schema.clone());

        // input_schema() should return minimal stub.
        let schema = adapter.input_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema.get("properties").is_none());

        // Verify the real schema is retrievable.
        let real = pool.get_deferred_schema(adapter.name()).unwrap();
        assert_eq!(real["properties"]["url"]["type"], "string");
    }

    #[test]
    fn test_deferred_tool_schema_returns_full_when_disabled() {
        let pool = Arc::new(McpProcessPool::new());
        // Deferred mode is OFF by default.

        let full_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"}
            }
        });

        let adapter = PooledMcpToolAdapter::new(
            pool,
            "search".to_string(),
            "search".to_string(),
            "Search".to_string(),
            full_schema.clone(),
            None,
        );

        // input_schema() should return the full schema.
        let schema = adapter.input_schema();
        assert_eq!(schema["properties"]["query"]["type"], "string");
    }

    #[test]
    fn test_deferred_schema_store_and_retrieve() {
        let pool = McpProcessPool::new();
        pool.store_deferred_schema("mcp__test__tool", serde_json::json!({"type": "object"}));
        assert!(pool.get_deferred_schema("mcp__test__tool").is_some());
        assert!(pool.get_deferred_schema("mcp__nonexistent").is_none());
        assert!(pool.deferred_schema_tool_names().contains(&"mcp__test__tool".to_string()));
    }

    #[test]
    fn test_tool_description_truncation() {
        let long_desc = "x".repeat(5000);
        let pool = Arc::new(McpProcessPool::new());
        let adapter = PooledMcpToolAdapter::new(
            pool,
            "srv".to_string(),
            "tool".to_string(),
            long_desc,
            serde_json::json!({"type": "object"}),
            None,
        );
        let desc = adapter.description();
        assert!(desc.chars().count() <= MAX_TOOL_DESCRIPTION_CHARS + 1, "description should be truncated to ~2048 chars");
        assert!(desc.ends_with('…'));
    }

    #[test]
    fn test_tool_description_short_not_truncated() {
        let short_desc = "A short description".to_string();
        let pool = Arc::new(McpProcessPool::new());
        let adapter = PooledMcpToolAdapter::new(
            pool,
            "srv".to_string(),
            "tool".to_string(),
            short_desc.clone(),
            serde_json::json!({"type": "object"}),
            None,
        );
        assert_eq!(adapter.description(), short_desc);
    }

    #[test]
    fn test_tool_result_not_truncated_under_limit() {
        let content = "x".repeat(100);
        let result = truncate_tool_result(&content, MAX_TOOL_RESULT_CHARS);
        assert_eq!(result, content);
    }

    #[test]
    fn test_tool_result_truncated_plain_text() {
        let content = "line\n".repeat(10_000); // 50,000 chars
        let result = truncate_tool_result(&content, MAX_TOOL_RESULT_CHARS);
        assert!(result.len() <= MAX_TOOL_RESULT_CHARS + 200); // +200 for notice
        assert!(result.contains("[compressed:") || result.contains("[...truncated:"));
        assert!(result.contains("50"));
        assert!(result.contains("chars"));
        // Should cut at a newline boundary
        assert!(result.lines().count() < 10_000);
    }

    #[test]
    fn test_tool_result_truncated_json() {
        let items: Vec<String> = (0..5000).map(|i| format!(r#"{{"id": {i}, "data": "item {i}"}}"#)).collect();
        let content = format!("[{}]", items.join(",\n"));
        let result = truncate_tool_result(&content, MAX_TOOL_RESULT_CHARS);
        assert!(result.len() <= MAX_TOOL_RESULT_CHARS + 200);
        assert!(result.contains("[compressed:") || result.contains("[...truncated:"));
    }

    #[test]
    fn test_tool_result_truncation_preserves_unicode() {
        // String with multi-byte chars
        let content = "日本語テスト\n".repeat(10_000);
        let result = truncate_tool_result(&content, MAX_TOOL_RESULT_CHARS);
        // Should not panic on char boundary
        assert!(result.contains("[compressed:") || result.contains("[...truncated:"));
    }

    #[test]
    fn test_normalize_error_content_text_only() {
        let blocks = vec![
            serde_json::json!({"type": "text", "text": "Error: file not found"}),
        ];
        assert_eq!(normalize_error_content(&blocks), "Error: file not found");
    }

    #[test]
    fn test_normalize_error_content_multi_text() {
        let blocks = vec![
            serde_json::json!({"type": "text", "text": "Error: connection failed"}),
            serde_json::json!({"type": "text", "text": "Retry after 30 seconds"}),
        ];
        assert_eq!(
            normalize_error_content(&blocks),
            "Error: connection failed\nRetry after 30 seconds"
        );
    }

    #[test]
    fn test_normalize_error_content_with_image() {
        let blocks = vec![
            serde_json::json!({"type": "text", "text": "Screenshot of error:"}),
            serde_json::json!({"type": "image", "mimeType": "image/png", "data": "..."}),
        ];
        let result = normalize_error_content(&blocks);
        assert!(result.contains("Screenshot of error:"));
        assert!(result.contains("[image/png image]"));
    }

    #[test]
    fn test_normalize_error_content_with_resource() {
        let blocks = vec![
            serde_json::json!({"type": "text", "text": "Server returned:"}),
            serde_json::json!({
                "type": "resource",
                "resource": {
                    "uri": "file:///var/log/error.log",
                    "mimeType": "text/plain",
                    "text": "Stack trace here"
                }
            }),
        ];
        let result = normalize_error_content(&blocks);
        assert!(result.contains("[resource: file:///var/log/error.log]"));
        assert!(result.contains("Stack trace here"));
    }

    #[test]
    fn test_normalize_error_content_empty_blocks() {
        let blocks: Vec<serde_json::Value> = vec![];
        assert_eq!(normalize_error_content(&blocks), "");
    }

    #[test]
    fn test_normalize_error_content_unknown_type() {
        let blocks = vec![
            serde_json::json!({"type": "audio", "data": "..."}),
        ];
        let result = normalize_error_content(&blocks);
        assert_eq!(result, "[audio block]");
    }

    #[test]
    fn test_pooled_tool_adapter_category() {
        let pool = Arc::new(McpProcessPool::new());
        let adapter = PooledMcpToolAdapter::new(
            pool,
            "srv".to_string(),
            "tool".to_string(),
            "desc".to_string(),
            serde_json::json!({"type": "object"}),
            None,
        );
        assert_eq!(adapter.category(), "mcp");
    }

    #[test]
    fn test_pooled_tool_adapter_debug() {
        let pool = Arc::new(McpProcessPool::new());
        let adapter = PooledMcpToolAdapter::new(
            pool,
            "test".to_string(),
            "tool".to_string(),
            "desc".to_string(),
            serde_json::json!({"type": "object"}),
            None,
        );
        let debug_str = format!("{adapter:?}");
        assert!(debug_str.contains("test"));
        assert!(debug_str.contains("mcp__test"));
    }

    #[test]
    fn test_pool_default() {
        let pool = McpProcessPool::new();
        assert_eq!(pool.server_count(), 0);
    }

    #[tokio::test]
    async fn test_pool_list_servers_empty() {
        let pool = McpProcessPool::new();
        let servers = pool.list_servers().await;
        assert!(servers.is_empty());
    }

    #[tokio::test]
    async fn test_pool_call_tool_not_found() {
        let pool = McpProcessPool::new();
        let result = pool
            .call_tool("nonexistent", "tool", serde_json::json!({}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_pool_ping_not_found() {
        let pool = McpProcessPool::new();
        let result = pool.ping("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_pool_server_state_not_found() {
        let pool = McpProcessPool::new();
        let state = pool.server_state("nonexistent").await;
        assert!(state.is_none());
    }

    #[test]
    fn test_tool_annotations_read_only() {
        let pool = Arc::new(McpProcessPool::new());
        let annotations = crate::ToolAnnotations {
            read_only_hint: true,
            ..Default::default()
        };
        let adapter = PooledMcpToolAdapter::new(
            pool,
            "srv".to_string(),
            "read_tool".to_string(),
            "Read-only tool".to_string(),
            serde_json::json!({"type": "object"}),
            Some(annotations),
        );
        assert!(adapter.is_read_only());
        assert!(adapter.is_concurrency_safe());
    }

    #[test]
    fn test_tool_annotations_destructive() {
        let pool = Arc::new(McpProcessPool::new());
        let annotations = crate::ToolAnnotations {
            destructive_hint: true,
            ..Default::default()
        };
        let adapter = PooledMcpToolAdapter::new(
            pool,
            "srv".to_string(),
            "delete_tool".to_string(),
            "Destructive tool".to_string(),
            serde_json::json!({"type": "object"}),
            Some(annotations),
        );
        assert!(!adapter.is_read_only());
        assert!(!adapter.is_concurrency_safe());
    }

    #[test]
    fn test_tool_annotations_idempotent() {
        let pool = Arc::new(McpProcessPool::new());
        let annotations = crate::ToolAnnotations {
            idempotent_hint: true,
            ..Default::default()
        };
        let adapter = PooledMcpToolAdapter::new(
            pool,
            "srv".to_string(),
            "cache_tool".to_string(),
            "Idempotent tool".to_string(),
            serde_json::json!({"type": "object"}),
            Some(annotations),
        );
        assert!(!adapter.is_read_only());
        assert!(adapter.is_concurrency_safe());
    }

    #[test]
    fn test_tool_annotations_none() {
        let pool = Arc::new(McpProcessPool::new());
        let adapter = PooledMcpToolAdapter::new(
            pool,
            "srv".to_string(),
            "basic_tool".to_string(),
            "Basic tool".to_string(),
            serde_json::json!({"type": "object"}),
            None,
        );
        assert!(!adapter.is_read_only());
        assert!(!adapter.is_concurrency_safe());
    }

    // -- Tool permission tests --------------------------------------------

    #[test]
    fn test_glob_match_exact() {
        assert!(glob_match("mcp__fetch__fetch", "mcp__fetch__fetch"));
        assert!(!glob_match("mcp__fetch__fetch", "mcp__fetch__search"));
    }

    #[test]
    fn test_glob_match_star() {
        assert!(glob_match("mcp__fetch__*", "mcp__fetch__fetch"));
        assert!(glob_match("mcp__fetch__*", "mcp__fetch__search"));
        assert!(glob_match("mcp__*", "mcp__fetch__fetch"));
        assert!(!glob_match("mcp__fetch__*", "mcp__other__tool"));
    }

    #[test]
    fn test_glob_match_question() {
        assert!(glob_match("mcp__x__?", "mcp__x__a"));
        assert!(!glob_match("mcp__x__?", "mcp__x__ab"));
    }

    #[test]
    fn test_is_tool_allowed_empty_patterns() {
        assert!(is_tool_allowed_by_patterns("mcp__anything__tool", &[]));
    }

    #[test]
    fn test_is_tool_allowed_allow_pattern() {
        let patterns = vec!["mcp__fetch__*".to_string()];
        assert!(is_tool_allowed_by_patterns("mcp__fetch__fetch", &patterns));
        assert!(!is_tool_allowed_by_patterns("mcp__other__tool", &patterns));
    }

    #[test]
    fn test_is_tool_allowed_deny_pattern() {
        let patterns = vec!["!mcp__internal__*".to_string()];
        assert!(!is_tool_allowed_by_patterns("mcp__internal__secret", &patterns));
        assert!(is_tool_allowed_by_patterns("mcp__fetch__fetch", &patterns));
    }

    #[test]
    fn test_is_tool_allowed_mixed_patterns() {
        let patterns = vec![
            "mcp__fetch__*".to_string(),
            "mcp__memory__*".to_string(),
            "!mcp__internal__*".to_string(),
        ];
        assert!(is_tool_allowed_by_patterns("mcp__fetch__fetch", &patterns));
        assert!(is_tool_allowed_by_patterns("mcp__memory__create", &patterns));
        assert!(!is_tool_allowed_by_patterns("mcp__internal__secret", &patterns));
        assert!(!is_tool_allowed_by_patterns("mcp__other__tool", &patterns));
    }

    #[test]
    fn test_is_tool_allowed_deny_overrides_allow() {
        let patterns = vec![
            "mcp__*".to_string(),
            "!mcp__internal__*".to_string(),
        ];
        assert!(is_tool_allowed_by_patterns("mcp__fetch__fetch", &patterns));
        assert!(!is_tool_allowed_by_patterns("mcp__internal__tool", &patterns));
    }

    #[tokio::test]
    async fn test_pool_allowed_patterns() {
        let pool = McpProcessPool::new();
        assert!(pool.is_tool_allowed("mcp__anything__tool").await);

        pool.set_allowed_patterns(vec!["mcp__fetch__*".to_string()]).await;
        assert!(pool.is_tool_allowed("mcp__fetch__fetch").await);
        assert!(!pool.is_tool_allowed("mcp__other__tool").await);
    }

    #[test]
    fn test_enforce_output_limit_under() {
        let pool = McpProcessPool::new();
        let output = ToolOutput::success("hello world".to_string());
        let limited = pool.enforce_output_limit(output, 1000, "mcp__test__tool");
        assert_eq!(limited.content, "hello world");
    }

    #[test]
    fn test_enforce_output_limit_over() {
        let pool = McpProcessPool::new();
        let long_content = "a".repeat(2000);
        let output = ToolOutput::success(long_content);
        let limited = pool.enforce_output_limit(output, 1000, "mcp__test__tool");
        assert!(limited.content.len() < 1500); // budget + compression notice + chunk_id
        assert!(limited.content.contains("[compressed:") || limited.content.contains("chunk_id="));
    }

    #[test]
    fn test_enforce_output_limit_preserves_unicode() {
        let pool = McpProcessPool::new();
        // Unicode chars at boundary
        let content = "日本語".repeat(500); // Each char is 3 bytes
        let output = ToolOutput::success(content);
        let limited = pool.enforce_output_limit(output, 100, "mcp__test__tool");
        // Should not panic on char boundary
        assert!(limited.content.contains("[compressed:") || limited.content.contains("chunk_id=") || limited.content.len() <= 200);
    }

    #[tokio::test]
    async fn test_server_status_not_found() {
        let pool = McpProcessPool::new();
        let status = pool.server_status("nonexistent").await;
        assert!(status.is_none());
    }

    #[tokio::test]
    async fn test_pool_stop_health_checks_noop() {
        // Stopping when never started should not panic.
        let pool = McpProcessPool::new();
        pool.stop_health_checks().await;
        assert_eq!(pool.server_count(), 0);
    }

    #[tokio::test]
    async fn test_tool_result_store_roundtrip() {
        let pool = McpProcessPool::new();
        // Trigger enforce_output_limit with content that exceeds the limit.
        let long_content = "x".repeat(2000);
        let output = ToolOutput::success(long_content.clone());
        let limited = pool.enforce_output_limit(output, 100, "mcp__srv__tool");

        // Should contain a chunk_id in the truncation notice.
        assert!(limited.content.contains("[compressed:") || limited.content.contains("chunk_id="));
        let chunk_id = limited.content
            .split("chunk_id=")
            .nth(1)
            .and_then(|s| s.split(']').next())
            .expect("should have chunk_id")
            .to_string();

        // Retrieve full content.
        let (tool_name, full) = pool.get_stored_result(&chunk_id)
            .await
            .expect("should find stored result");
        assert_eq!(tool_name, "mcp__srv__tool");
        assert_eq!(full, long_content);
    }

    #[tokio::test]
    async fn test_tool_result_store_chunking() {
        let pool = McpProcessPool::new();
        let long_content = "abcdefghij".repeat(100); // 1000 chars
        let output = ToolOutput::success(long_content.clone());
        let limited = pool.enforce_output_limit(output, 50, "mcp__srv__tool");

        let chunk_id = limited.content
            .split("chunk_id=")
            .nth(1)
            .and_then(|s| s.split(']').next())
            .expect("should have chunk_id")
            .to_string();

        // Get first chunk.
        let chunk = pool.get_result_chunk(&chunk_id, 0, 100)
            .await
            .expect("should get chunk");
        assert_eq!(chunk.content.len(), 100);
        assert!(chunk.has_more);
        assert_eq!(chunk.total_len, 1000);

        // Get second chunk.
        let chunk2 = pool.get_result_chunk(&chunk_id, chunk.offset, 100)
            .await
            .expect("should get chunk 2");
        assert!(chunk2.has_more);

        // Get beyond end.
        let chunk_end = pool.get_result_chunk(&chunk_id, 2000, 100)
            .await
            .expect("should handle past-end");
        assert!(!chunk_end.has_more);
        assert!(chunk_end.content.is_empty());
    }

    #[tokio::test]
    async fn test_tool_result_store_missing() {
        let pool = McpProcessPool::new();
        assert!(pool.get_stored_result("nonexistent").await.is_none());
        assert!(pool.get_result_chunk("nonexistent", 0, 100).await.is_none());
    }

    #[test]
    fn test_disk_persistence_saves_file() {
        let pool = McpProcessPool::new();
        let content = "x".repeat(5000);
        let output = ToolOutput::success(content.clone());
        let limited = pool.enforce_output_limit(output, 100, "mcp__test__disk");

        // Should reference a file path.
        assert!(limited.content.contains("full result saved to:"));
        assert!(limited.content.contains(".shannon/mcp_results/"));

        // Extract chunk_id and verify the file exists.
        let chunk_id = limited.content
            .split("chunk_id=")
            .nth(1)
            .and_then(|s| s.split(']').next())
            .expect("should have chunk_id")
            .to_string();

        let path = std::path::Path::new(".shannon/mcp_results").join(format!("{chunk_id}.json"));
        assert!(path.exists(), "disk file should exist");

        // Verify file content.
        let file_data: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(file_data["tool_name"], "mcp__test__disk");
        assert_eq!(file_data["content"], content);
        assert!(file_data["stored_at"].is_string());

        // Clean up.
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn test_notification_channel_forwarding() {
        let pool = Arc::new(McpProcessPool::new());

        // Set up a callback that records which server changed.
        // Use std::sync::Mutex since the callback is sync (not async).
        let changed = Arc::new(std::sync::Mutex::new(None::<String>));
        let changed_clone = changed.clone();
        pool.set_on_tools_changed(Arc::new(move |server_name, _new_tools| {
            *changed_clone.lock().unwrap_or_else(|e| e.into_inner()) = Some(server_name.to_string());
        }))
        .await;

        // Start the notification handler.
        pool.start_notification_handler();

        // Simulate a notification from a server by sending through the channel.
        pool.notification_tx
            .send((
                "test-server".to_string(),
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/tools/list_changed"
                }),
            ))
            .await
            .unwrap();

        // Give the handler time to process.
        tokio::time::sleep(Duration::from_millis(50)).await;

        let guard = changed.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(guard.as_deref(), Some("test-server"));
    }

    #[test]
    fn test_progress_notification_deserialization() {
        let json = serde_json::json!({
            "progressToken": "pg-42",
            "progress": 50.0,
            "total": 100.0
        });
        let notif: crate::ProgressNotification =
            serde_json::from_value(json).unwrap();
        assert_eq!(notif.progress_token, serde_json::json!("pg-42"));
        assert_eq!(notif.progress, 50.0);
        assert_eq!(notif.total, Some(100.0));
    }

    #[test]
    fn test_progress_notification_without_total() {
        let json = serde_json::json!({
            "progressToken": 7,
            "progress": 3.0
        });
        let notif: crate::ProgressNotification =
            serde_json::from_value(json).unwrap();
        assert_eq!(notif.progress_token, serde_json::json!(7));
        assert_eq!(notif.progress, 3.0);
        assert_eq!(notif.total, None);
    }

    #[tokio::test]
    async fn test_progress_callback_routing() {
        use dashmap::DashMap;

        let pending: Arc<DashMap<u64, PendingRequest>> =
            Arc::new(DashMap::new());
        let (tx, _rx): (tokio::sync::mpsc::Sender<(String, Value)>, _) =
            tokio::sync::mpsc::channel(1024);

        let progress_reports = Arc::new(std::sync::Mutex::new(Vec::<(f64, Option<f64>)>::new()));
        let reports_clone = progress_reports.clone();

        // Insert a pending request with a progress token and callback.
        let (oneshot_tx, _oneshot_rx) = tokio::sync::oneshot::channel();
        pending.insert(
            42,
            PendingRequest {
                tx: oneshot_tx,
                created_at: Instant::now(),
                progress_token: Some(serde_json::json!("pg-test")),
                on_progress: Some(Arc::new(move |progress, total| {
                    reports_clone.lock().unwrap_or_else(|e| e.into_inner()).push((progress, total));
                })),
            },
        );

        // Simulate a progress notification line.
        let line = r#"{"jsonrpc":"2.0","method":"notifications/progress","params":{"progressToken":"pg-test","progress":25.0,"total":100.0}}"#;
        let value: Value = serde_json::from_str(line).unwrap();

        // Replicate the routing logic from read_responses.
        if value.get("method").and_then(|m| m.as_str()) == Some("notifications/progress") {
            if let Some(token) = value.get("params").and_then(|p| p.get("progressToken")).cloned() {
                for entry in pending.iter() {
                    if entry.value().progress_token.as_ref() == Some(&token) {
                        if let Some(ref cb) = entry.value().on_progress {
                            let progress = value
                                .get("params")
                                .and_then(|p| p.get("progress"))
                                .and_then(|v| v.as_f64())
                                .unwrap_or(0.0);
                            let total = value
                                .get("params")
                                .and_then(|p| p.get("total"))
                                .and_then(|v| v.as_f64());
                            cb(progress, total);
                        }
                        break;
                    }
                }
            }
        }

        let reports = progress_reports.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0], (25.0, Some(100.0)));

        // Verify pending request is still there (not removed by progress).
        assert!(pending.contains_key(&42));

        drop(tx);
    }

    #[test]
    fn test_root_serialization() {
        let root = crate::Root {
            uri: "file:///home/user/project".to_string(),
            name: Some("My Project".to_string()),
        };
        let json = serde_json::to_value(&root).unwrap();
        assert_eq!(json["uri"], "file:///home/user/project");
        assert_eq!(json["name"], "My Project");
    }

    #[test]
    fn test_root_without_name() {
        let root = crate::Root {
            uri: "file:///tmp".to_string(),
            name: None,
        };
        let json = serde_json::to_value(&root).unwrap();
        assert_eq!(json["uri"], "file:///tmp");
        assert!(json.get("name").is_none(), "name should be omitted when None");
    }

    #[test]
    fn test_list_roots_result_serialization() {
        let result = crate::ListRootsResult {
            roots: vec![
                crate::Root {
                    uri: "file:///a".to_string(),
                    name: Some("A".to_string()),
                },
                crate::Root {
                    uri: "file:///b".to_string(),
                    name: None,
                },
            ],
        };
        let json = serde_json::to_value(&result).unwrap();
        let roots = json["roots"].as_array().unwrap();
        assert_eq!(roots.len(), 2);
    }

    #[test]
    fn test_roots_capability_serialization() {
        let cap = crate::RootsCapability { list_changed: true };
        let json = serde_json::to_value(&cap).unwrap();
        assert_eq!(json["listChanged"], true);
    }

    #[tokio::test]
    async fn test_roots_provider_default_none() {
        let pool = McpProcessPool::new();
        let roots = pool.get_roots().await;
        assert!(roots.is_empty(), "default roots provider should return empty vec");
    }

    #[tokio::test]
    async fn test_set_and_get_roots() {
        let pool = McpProcessPool::new();
        pool.set_roots_provider(Arc::new(|| {
            vec![
                crate::Root {
                    uri: "file:///workspace".to_string(),
                    name: Some("Workspace".to_string()),
                },
            ]
        }))
        .await;

        let roots = pool.get_roots().await;
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].uri, "file:///workspace");
    }

    // -----------------------------------------------------------------------
    // Sampling tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_message_request_deserialization() {
        let json = serde_json::json!({
            "messages": [
                {
                    "role": "user",
                    "content": { "type": "text", "text": "Hello" }
                }
            ],
            "maxTokens": 100,
            "temperature": 0.7
        });
        let req: crate::CreateMessageRequest =
            serde_json::from_value(json).unwrap();
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.max_tokens, Some(100));
        assert_eq!(req.sampling_params.temperature, Some(0.7));
    }

    #[test]
    fn test_create_message_result_serialization() {
        let result = crate::CreateMessageResult {
            role: crate::SamplingMessageRole::Assistant,
            model: "test-model".to_string(),
            content: crate::SamplingContent::Text {
                text: "Hi there!".to_string(),
            },
            stop_reason: Some(crate::StopReason::EndTurn),
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["role"], "assistant");
        assert_eq!(json["model"], "test-model");
        assert_eq!(json["stopReason"], "endTurn");
    }

    #[test]
    fn test_sampling_message_role_serialization() {
        let user = serde_json::to_value(&crate::SamplingMessageRole::User).unwrap();
        assert_eq!(user, "user");
        let assistant = serde_json::to_value(&crate::SamplingMessageRole::Assistant).unwrap();
        assert_eq!(assistant, "assistant");
    }

    #[test]
    fn test_model_preferences_deserialization() {
        let json = serde_json::json!({
            "hints": [{ "name": "claude-3" }],
            "costPriority": 0.5,
            "speedPriority": 0.8,
            "intelligencePriority": 0.9
        });
        let prefs: crate::ModelPreferences = serde_json::from_value(json).unwrap();
        assert_eq!(prefs.hints.as_ref().unwrap().len(), 1);
        assert_eq!(prefs.cost_priority, Some(0.5));
        assert_eq!(prefs.speed_priority, Some(0.8));
        assert_eq!(prefs.intelligence_priority, Some(0.9));
    }

    #[tokio::test]
    async fn test_sampling_provider_default_none() {
        let pool = McpProcessPool::new();
        let guard = pool.sampling_provider.lock().await;
        assert!(guard.is_none(), "default sampling provider should be None");
    }

    #[tokio::test]
    async fn test_set_sampling_provider() {
        let pool = McpProcessPool::new();
        pool.set_sampling_provider(Arc::new(|req| {
            Box::pin(async move {
                Ok(crate::CreateMessageResult {
                    role: crate::SamplingMessageRole::Assistant,
                    model: "mock".to_string(),
                    content: crate::SamplingContent::Text {
                        text: format!("Echo: {} messages", req.messages.len()),
                    },
                    stop_reason: Some(crate::StopReason::EndTurn),
                })
            })
        }))
        .await;

        let guard = pool.sampling_provider.lock().await;
        assert!(guard.is_some());

        // Call the provider to verify it works.
        let provider = guard.as_ref().unwrap();
        let req = crate::CreateMessageRequest {
            messages: vec![crate::SamplingMessage {
                role: crate::SamplingMessageRole::User,
                content: crate::SamplingContent::Text {
                    text: "test".to_string(),
                },
            }],
            model_preferences: None,
            system_prompt: None,
            max_tokens: Some(50),
            sampling_params: crate::SamplingParams::default(),
        };
        let result = provider(req).await.unwrap();
        assert_eq!(result.model, "mock");
    }

    #[tokio::test]
    async fn test_budget_tracking_no_budget_by_default() {
        let pool = McpProcessPool::new();
        // No server → not over budget
        assert!(!pool.is_over_budget("nonexistent").await);
        assert_eq!(pool.server_total_result_bytes("nonexistent").await, None);
    }

    #[tokio::test]
    async fn test_track_result_bytes_for() {
        let pool = McpProcessPool::new();

        let (ntx, _) = tokio::sync::mpsc::channel(1024);
        let handle = Arc::new(McpServerHandle {
            name: "test-srv".to_string(),
            command: "echo".to_string(),
            args: vec![],
            env: HashMap::new(),
            stdin: Arc::new(Mutex::new(None)),
            next_id: AtomicU64::new(1),
            pending: Arc::new(DashMap::new()),
            state: Arc::new(RwLock::new(ServerState::Stopped)),
            child: Arc::new(Mutex::new(None)),
            reader_task: Arc::new(Mutex::new(None)),
            restart_count: Arc::new(AtomicU64::new(0)),
            max_restarts: 3,
            health_interval: Duration::from_secs(60),
            request_timeout: Duration::from_secs(30),
            connection_timeout: Duration::from_secs(30),
            tool_timeout: Duration::from_secs(120),
            started_at: Arc::new(RwLock::new(None)),
            request_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            total_result_bytes: AtomicU64::new(0),
            budget_bytes: Arc::new(RwLock::new(None)),
            last_health_check: Arc::new(RwLock::new(None)),
            notification_tx: ntx,
            roots_provider: Arc::new(Mutex::new(None)),
            sampling_provider: Arc::new(Mutex::new(None)),
            elicitation_provider: Arc::new(Mutex::new(None)),
            capabilities: Arc::new(RwLock::new(None)),
            protocol_version: Arc::new(RwLock::new(String::new())),
            concurrency_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        });

        pool.handles.insert("test-srv".to_string(), handle);

        // Track bytes
        pool.track_result_bytes_for("test-srv", 100);
        assert_eq!(pool.server_total_result_bytes("test-srv").await, Some(100));

        pool.track_result_bytes_for("test-srv", 250);
        assert_eq!(pool.server_total_result_bytes("test-srv").await, Some(350));

        // No budget set → not over budget
        assert!(!pool.is_over_budget("test-srv").await);
    }

    #[tokio::test]
    async fn test_budget_enforcement() {
        let pool = McpProcessPool::new();

        let (ntx, _) = tokio::sync::mpsc::channel(1024);
        let handle = Arc::new(McpServerHandle {
            name: "budget-srv".to_string(),
            command: "echo".to_string(),
            args: vec![],
            env: HashMap::new(),
            stdin: Arc::new(Mutex::new(None)),
            next_id: AtomicU64::new(1),
            pending: Arc::new(DashMap::new()),
            state: Arc::new(RwLock::new(ServerState::Stopped)),
            child: Arc::new(Mutex::new(None)),
            reader_task: Arc::new(Mutex::new(None)),
            restart_count: Arc::new(AtomicU64::new(0)),
            max_restarts: 3,
            health_interval: Duration::from_secs(60),
            request_timeout: Duration::from_secs(30),
            connection_timeout: Duration::from_secs(30),
            tool_timeout: Duration::from_secs(120),
            started_at: Arc::new(RwLock::new(None)),
            request_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            total_result_bytes: AtomicU64::new(800),
            budget_bytes: Arc::new(RwLock::new(Some(1000))),
            last_health_check: Arc::new(RwLock::new(None)),
            notification_tx: ntx,
            roots_provider: Arc::new(Mutex::new(None)),
            sampling_provider: Arc::new(Mutex::new(None)),
            elicitation_provider: Arc::new(Mutex::new(None)),
            capabilities: Arc::new(RwLock::new(None)),
            protocol_version: Arc::new(RwLock::new(String::new())),
            concurrency_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        });

        pool.handles.insert("budget-srv".to_string(), handle);

        // 800 / 1000 → not over budget yet
        assert!(!pool.is_over_budget("budget-srv").await);
        assert_eq!(pool.server_total_result_bytes("budget-srv").await, Some(800));

        // Add 250 bytes → 1050 > 1000 → over budget
        pool.track_result_bytes_for("budget-srv", 250);
        assert!(pool.is_over_budget("budget-srv").await);
    }

    #[tokio::test]
    async fn test_set_server_budget() {
        let pool = McpProcessPool::new();

        let (ntx, _) = tokio::sync::mpsc::channel(1024);
        let handle = Arc::new(McpServerHandle {
            name: "cfg-srv".to_string(),
            command: "echo".to_string(),
            args: vec![],
            env: HashMap::new(),
            stdin: Arc::new(Mutex::new(None)),
            next_id: AtomicU64::new(1),
            pending: Arc::new(DashMap::new()),
            state: Arc::new(RwLock::new(ServerState::Stopped)),
            child: Arc::new(Mutex::new(None)),
            reader_task: Arc::new(Mutex::new(None)),
            restart_count: Arc::new(AtomicU64::new(0)),
            max_restarts: 3,
            health_interval: Duration::from_secs(60),
            request_timeout: Duration::from_secs(30),
            connection_timeout: Duration::from_secs(30),
            tool_timeout: Duration::from_secs(120),
            started_at: Arc::new(RwLock::new(None)),
            request_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            total_result_bytes: AtomicU64::new(500),
            budget_bytes: Arc::new(RwLock::new(None)),
            last_health_check: Arc::new(RwLock::new(None)),
            notification_tx: ntx,
            roots_provider: Arc::new(Mutex::new(None)),
            sampling_provider: Arc::new(Mutex::new(None)),
            elicitation_provider: Arc::new(Mutex::new(None)),
            capabilities: Arc::new(RwLock::new(None)),
            protocol_version: Arc::new(RwLock::new(String::new())),
            concurrency_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        });

        pool.handles.insert("cfg-srv".to_string(), handle);

        // No budget → never over budget
        assert!(!pool.is_over_budget("cfg-srv").await);

        // Set budget
        pool.set_server_budget("cfg-srv", 600).await;
        // 500 < 600 → not over budget
        assert!(!pool.is_over_budget("cfg-srv").await);

        // Add 200 bytes → 700 > 600 → over budget
        pool.track_result_bytes_for("cfg-srv", 200);
        assert!(pool.is_over_budget("cfg-srv").await);
    }
