//! Product-level end-to-end integration tests for Shannon Code.
//!
//! These tests verify the complete product pipeline from user input to final

#![allow(clippy::collapsible_match)]
//! output, covering the most critical user-facing flows:
//!
//! - Full query pipeline (user → LLM → tool → response)
//! - Multi-provider support (Anthropic, OpenAI, Ollama)
//! - Tool execution with permission checks
//! - Session persistence and restoration
//! - Cost tracking across turns
//! - Streaming tool executor lifecycle
//! - Streaming response assembly
//! - Error recovery and fallback

use async_trait::async_trait;
use futures::StreamExt;
use mockito::{Server, ServerGuard};
use serde_json::json;
use shannon_core::query_engine::CostTracker;
use shannon_core::query_engine::{QueryContext, QueryEngine, QueryEvent, QueryMetadata};
use shannon_core::tools::{Tool, ToolOutput, ToolRegistry, ToolResult};
use shannon_engine::api::{ContentBlock, LlmClient, LlmClientConfig, LlmProvider, RetryConfig};
use shannon_engine::permissions::PermissionManager;
use shannon_engine::state::StateManager;
use shannon_engine::streaming_tool_executor::{StreamingToolExecutor, ToolStatus};
use uuid::Uuid;

// ============================================================================
// Test Helpers
// ============================================================================

struct AnthropicKeyGuard(Option<std::ffi::OsString>);
impl AnthropicKeyGuard {
    fn set() -> Self {
        let old = std::env::var_os("ANTHROPIC_API_KEY");
        unsafe {
            std::env::set_var("ANTHROPIC_API_KEY", "test-key");
        }
        Self(old)
    }
}
impl Drop for AnthropicKeyGuard {
    fn drop(&mut self) {
        match &self.0 {
            Some(v) => unsafe { std::env::set_var("ANTHROPIC_API_KEY", v) },
            None => unsafe { std::env::remove_var("ANTHROPIC_API_KEY") },
        }
    }
}

fn make_client(server: &ServerGuard, provider: LlmProvider) -> LlmClient {
    let config = LlmClientConfig {
        api_key: "test-key".to_string(),
        base_url: server.url(),
        model: "test-model".to_string(),
        max_tokens: 1024,
        timeout_seconds: 30,
        api_version: "2023-06-01".to_string(),
        provider,
        extra_headers: Default::default(),
        retry_config: RetryConfig::default(),
        fallback_provider: None,
        fallback_base_url: None,
        max_stream_reconnects: 3,
        budget_tokens: None,
        reasoning_effort: None,
    };
    LlmClient::new(config)
}

fn make_context(message: &str) -> QueryContext {
    QueryContext {
        query_id: Uuid::new_v4(),
        session_id: Uuid::new_v4(),
        user_message: message.to_string(),
        metadata: QueryMetadata {
            timestamp: chrono::Utc::now(),
            tools_allowed: true,
            max_tokens: Some(1024),
            model: "test-model".to_string(),
            temperature: None,
            top_p: None,
        },
    }
}

/// A mock tool that records calls and returns configurable output.
struct RecordableTool {
    name: String,
    responses: std::sync::Mutex<Vec<ToolOutput>>,
    call_count: std::sync::atomic::AtomicUsize,
}

impl RecordableTool {
    fn new(name: &str, response: ToolOutput) -> Self {
        Self {
            name: name.to_string(),
            responses: std::sync::Mutex::new(vec![response]),
            call_count: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    #[allow(dead_code)] // KEEP: test helper
    fn with_responses(name: &str, responses: Vec<ToolOutput>) -> Self {
        Self {
            name: name.to_string(),
            responses: std::sync::Mutex::new(responses),
            call_count: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    #[allow(dead_code)] // KEEP: test helper
    fn call_count(&self) -> usize {
        self.call_count.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[async_trait]
impl Tool for RecordableTool {
    async fn execute(&self, _input: serde_json::Value) -> ToolResult<ToolOutput> {
        self.call_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            Ok(ToolOutput::success("default response".to_string()))
        } else {
            Ok(responses[0].clone())
        }
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        "recordable test tool"
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({"type": "object", "properties": {}})
    }
}

/// Build an SSE body for Anthropic streaming response with text content.
fn anthropic_sse_text(msg_id: &str, text: &str) -> String {
    format!(
        concat!(
            "data: {{\"type\":\"message_start\",\"message\":{{\"id\":\"{mid}\",\"role\":\"assistant\",\"content\":[],\"model\":\"test-model\",\"stop_reason\":null,\"usage\":{{\"input_tokens\":10,\"output_tokens\":0}}}}}}\n\n",
            "data: {{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{{\"type\":\"text\",\"text\":\"\"}}}}\n\n",
            "data: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"{txt}\"}}}}\n\n",
            "data: {{\"type\":\"content_block_stop\",\"index\":0}}\n\n",
            "data: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"end_turn\"}},\"usage\":{{\"input_tokens\":10,\"output_tokens\":8}}}}\n\n",
            "data: {{\"type\":\"message_stop\"}}\n\n",
        ),
        mid = msg_id,
        txt = text,
    )
}

/// Build an SSE body for Anthropic with a single tool use followed by text.
fn anthropic_sse_tool_then_text(
    msg_id: &str,
    tool_name: &str,
    tool_id: &str,
    tool_input: &str,
    _final_text: &str,
) -> String {
    format!(
        concat!(
            "data: {{\"type\":\"message_start\",\"message\":{{\"id\":\"{mid}\",\"role\":\"assistant\",\"content\":[],\"model\":\"test-model\",\"stop_reason\":null,\"usage\":{{\"input_tokens\":15,\"output_tokens\":0}}}}}}\n\n",
            "data: {{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{{\"type\":\"tool_use\",\"id\":\"{tid}\",\"name\":\"{tname}\",\"input\":{{}}}}}}\n\n",
            "data: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"input_json_delta\",\"partial_json\":\"{tinput}\"}}}}\n\n",
            "data: {{\"type\":\"content_block_stop\",\"index\":0}}\n\n",
            "data: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"tool_use\"}},\"usage\":{{\"input_tokens\":15,\"output_tokens\":10}}}}\n\n",
            "data: {{\"type\":\"message_stop\"}}\n\n",
        ),
        mid = msg_id,
        tid = tool_id,
        tname = tool_name,
        tinput = tool_input,
    )
}

/// Build final text response after tool result.
fn anthropic_sse_final_text(msg_id: &str, text: &str) -> String {
    format!(
        concat!(
            "data: {{\"type\":\"message_start\",\"message\":{{\"id\":\"{mid}\",\"role\":\"assistant\",\"content\":[],\"model\":\"test-model\",\"stop_reason\":null,\"usage\":{{\"input_tokens\":25,\"output_tokens\":0}}}}}}\n\n",
            "data: {{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{{\"type\":\"text\",\"text\":\"\"}}}}\n\n",
            "data: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"{txt}\"}}}}\n\n",
            "data: {{\"type\":\"content_block_stop\",\"index\":0}}\n\n",
            "data: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"end_turn\"}},\"usage\":{{\"input_tokens\":25,\"output_tokens\":10}}}}\n\n",
            "data: {{\"type\":\"message_stop\"}}\n\n",
        ),
        mid = msg_id,
        txt = text,
    )
}

// ============================================================================
// E2E Test: Simple text query pipeline
// ============================================================================

#[tokio::test]
async fn test_e2e_simple_text_query_produces_complete_output() {
    let _guard = AnthropicKeyGuard::set();
    let mut server = Server::new_async().await;

    server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(anthropic_sse_text(
            "msg_simple",
            "Rust is a systems programming language.",
        ))
        .create();

    let client = make_client(&server, LlmProvider::Anthropic);
    let engine = QueryEngine::with_defaults(
        client,
        ToolRegistry::new(),
        PermissionManager::new(),
        StateManager::new(),
    );

    let ctx = make_context("What is Rust?");
    let mut stream = engine.process_query(ctx, None).await;

    let mut text = String::new();
    let mut has_completed = false;

    while let Some(result) = stream.next().await {
        match result.unwrap() {
            QueryEvent::Text { content, .. } => text.push_str(&content),
            QueryEvent::Completed { .. } => has_completed = true,
            _ => {}
        }
    }

    assert!(!text.is_empty(), "Pipeline should produce text output");
    assert!(has_completed, "Pipeline should emit Completed event");
}

// ============================================================================
// E2E Test: Tool execution pipeline
// ============================================================================

#[tokio::test]
async fn test_e2e_tool_execution_pipeline() {
    let _guard = AnthropicKeyGuard::set();
    let mut server = Server::new_async().await;

    // First response: LLM requests tool use (same format as working test)
    let tool_sse = concat!(
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_tool\",\"role\":\"assistant\",\"content\":[],\"model\":\"test-model\",\"stop_reason\":null,\"usage\":{\"input_tokens\":20,\"output_tokens\":0}}}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Let me check that.\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_ls\",\"name\":\"bash\",\"input\":{}}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"command\\\":\\\"ls -la\\\"}\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"input_tokens\":20,\"output_tokens\":15}}\n\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );

    // Second response: after tool result, LLM responds with text
    let final_sse = concat!(
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_final\",\"role\":\"assistant\",\"content\":[],\"model\":\"test-model\",\"stop_reason\":null,\"usage\":{\"input_tokens\":30,\"output_tokens\":0}}}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"The directory is empty.\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"input_tokens\":30,\"output_tokens\":10}}\n\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );

    server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(tool_sse)
        .expect(1)
        .create();

    server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(final_sse)
        .expect(1)
        .create();

    let client = make_client(&server, LlmProvider::Anthropic);
    let registry = ToolRegistry::new();
    registry
        .register(Box::new(RecordableTool::new(
            "bash",
            ToolOutput::success("total 0\ndrwxr-xr-x 2 user user 64 Jan 1 00:00 .".to_string()),
        )))
        .unwrap();

    let engine = QueryEngine::with_defaults(
        client,
        registry,
        PermissionManager::new(),
        StateManager::new(),
    );

    let ctx = make_context("List files in current directory");
    let mut stream = engine.process_query(ctx, None).await;

    let mut events: Vec<QueryEvent> = Vec::new();
    while let Some(result) = stream.next().await {
        match result {
            Ok(event) => events.push(event),
            Err(e) => panic!("Stream error: {e}"),
        }
    }

    // Verify the full pipeline produced the expected event sequence
    let has_tool_request = events
        .iter()
        .any(|e| matches!(e, QueryEvent::ToolUseRequest { tool_name, .. } if tool_name == "bash"));
    let has_tool_result = events.iter().any(|e| matches!(e, QueryEvent::ToolUseResult { tool_name, is_error, .. } if tool_name == "bash" && !is_error));
    let has_completed = events
        .iter()
        .any(|e| matches!(e, QueryEvent::Completed { .. }));

    // Extract final text
    let final_text: String = events
        .iter()
        .filter_map(|e| match e {
            QueryEvent::Text { content, .. } => Some(content.as_str()),
            _ => None,
        })
        .collect();

    assert!(has_tool_request, "Pipeline should request bash tool");
    assert!(has_tool_result, "Pipeline should produce bash tool result");
    assert!(has_completed, "Pipeline should complete");
    assert!(
        final_text.contains("The directory is empty."),
        "Final output should contain tool-derived response"
    );
}

// ============================================================================
// E2E Test: Multi-turn conversation
// ============================================================================

#[tokio::test]
async fn test_e2e_multi_turn_conversation_preserves_context() {
    let _guard = AnthropicKeyGuard::set();
    let mut server = Server::new_async().await;

    // Turn 1
    server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(anthropic_sse_text("msg_t1", "I'll remember that."))
        .expect(1)
        .create();

    // Turn 2
    server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(anthropic_sse_text(
            "msg_t2",
            "You told me Rust is your favorite.",
        ))
        .expect(1)
        .create();

    let client = make_client(&server, LlmProvider::Anthropic);
    let mut engine = QueryEngine::with_defaults(
        client,
        ToolRegistry::new(),
        PermissionManager::new(),
        StateManager::new(),
    );

    // Turn 1
    let ctx1 = make_context("My favorite language is Rust.");
    let mut stream1 = engine.process_query(ctx1, None).await;
    let mut text1 = String::new();
    while let Some(result) = stream1.next().await {
        if let Ok(QueryEvent::Text { content, .. }) = result {
            text1.push_str(&content);
        }
    }
    assert_eq!(text1, "I'll remember that.");

    // Update conversation history
    engine.add_user_message("My favorite language is Rust.".to_string());
    engine.add_assistant_message(vec![ContentBlock::Text {
        text: "I'll remember that.".to_string(),
    }]);

    // Verify history has 2 messages
    assert_eq!(engine.conversation_history().len(), 2);

    // Turn 2
    let ctx2 = make_context("What is my favorite language?");
    let mut stream2 = engine.process_query(ctx2, None).await;
    let mut text2 = String::new();
    while let Some(result) = stream2.next().await {
        if let Ok(QueryEvent::Text { content, .. }) = result {
            text2.push_str(&content);
        }
    }
    assert_eq!(text2, "You told me Rust is your favorite.");
}

// ============================================================================
// E2E Test: Authentication error handling
// ============================================================================

#[tokio::test]
async fn test_e2e_auth_error_produces_failed_event() {
    let _guard = AnthropicKeyGuard::set();
    let mut server = Server::new_async().await;

    server
        .mock("POST", "/v1/messages")
        .with_status(401)
        .with_header("content-type", "application/json")
        .with_body(r#"{"type":"error","error":{"type":"authentication_error","message":"Invalid API key"}}"#)
        .create();

    let client = make_client(&server, LlmProvider::Anthropic);
    let engine = QueryEngine::with_defaults(
        client,
        ToolRegistry::new(),
        PermissionManager::new(),
        StateManager::new(),
    );

    let ctx = make_context("Hello");
    let mut stream = engine.process_query(ctx, None).await;

    let mut has_failed = false;
    while let Some(result) = stream.next().await {
        if let Ok(QueryEvent::Failed { error, .. }) = result {
            assert!(
                error.to_lowercase().contains("authentication")
                    || error.contains("401")
                    || error.to_lowercase().contains("unauthorized"),
                "Error should mention auth: {error}"
            );
            has_failed = true;
        }
    }
    assert!(
        has_failed,
        "Pipeline should emit Failed event for auth errors"
    );
}

// ============================================================================
// E2E Test: OpenAI provider text pipeline
// ============================================================================

#[tokio::test]
async fn test_e2e_openai_provider_text_response() {
    let _guard = AnthropicKeyGuard::set();
    let mut server = Server::new_async().await;

    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"GPT says hello\"},\"index\":0}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\",\"index\":0}]}\n\n",
        "data: [DONE]\n\n",
    );

    server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create();

    let client = make_client(&server, LlmProvider::OpenAI);
    let engine = QueryEngine::with_defaults(
        client,
        ToolRegistry::new(),
        PermissionManager::new(),
        StateManager::new(),
    );

    let ctx = make_context("Say hello");
    let mut stream = engine.process_query(ctx, None).await;

    let mut text = String::new();
    let mut has_completed = false;
    while let Some(result) = stream.next().await {
        match result.unwrap() {
            QueryEvent::Text { content, .. } => text.push_str(&content),
            QueryEvent::Completed { .. } => has_completed = true,
            _ => {}
        }
    }

    assert_eq!(text, "GPT says hello");
    assert!(has_completed);
}

// ============================================================================
// E2E Test: Streaming tool executor lifecycle
// ============================================================================

#[tokio::test]
async fn test_streaming_tool_executor_lifecycle() {
    let executor = StreamingToolExecutor::new(16);

    // Submit a tool
    let tool_id = executor
        .submit_tool("bash", json!({"command": "ls -la"}), true)
        .await
        .expect("submit should succeed");
    assert!(!tool_id.is_empty(), "Tool ID should be assigned");

    // Start the tool (transition to Executing)
    executor
        .start_tool(&tool_id)
        .await
        .expect("start should succeed");

    // Check status is Executing via tools()
    let tools = executor.tools().await;
    let tool = tools
        .iter()
        .find(|t| t.id == tool_id)
        .expect("tool should exist");
    assert_eq!(tool.status, ToolStatus::Executing);

    // Add progress
    executor.add_progress(&tool_id, "Listing files...");

    // Complete the tool
    executor
        .complete_tool(
            &tool_id,
            ToolOutput::success("file1.txt\nfile2.txt".to_string()),
        )
        .await
        .expect("complete should succeed");

    // Check status is Completed
    let tools = executor.tools().await;
    let tool = tools
        .iter()
        .find(|t| t.id == tool_id)
        .expect("tool should exist");
    assert_eq!(tool.status, ToolStatus::Completed);

    // Abort should not panic on completed tool
    executor.abort();
    assert!(executor.is_aborted());
}

#[tokio::test]
async fn test_streaming_tool_executor_multiple_concurrent_tools() {
    let executor = StreamingToolExecutor::new(16);

    let id1 = executor
        .submit_tool("bash", json!({}), true)
        .await
        .expect("submit 1");
    let id2 = executor
        .submit_tool("read", json!({}), true)
        .await
        .expect("submit 2");

    assert_ne!(id1, id2, "Each tool should get a unique ID");

    // Start and complete both
    executor.start_tool(&id1).await.expect("start 1");
    executor.start_tool(&id2).await.expect("start 2");
    executor
        .complete_tool(&id1, ToolOutput::success("output1".to_string()))
        .await
        .expect("complete 1");
    executor
        .complete_tool(&id2, ToolOutput::success("output2".to_string()))
        .await
        .expect("complete 2");

    let tools = executor.tools().await;
    let t1 = tools.iter().find(|t| t.id == id1).expect("tool 1");
    let t2 = tools.iter().find(|t| t.id == id2).expect("tool 2");
    assert_eq!(t1.status, ToolStatus::Completed);
    assert_eq!(t2.status, ToolStatus::Completed);
}

#[tokio::test]
async fn test_streaming_tool_executor_fail() {
    let executor = StreamingToolExecutor::new(16);

    let tool_id = executor
        .submit_tool("bash", json!({}), true)
        .await
        .expect("submit");
    executor.start_tool(&tool_id).await.expect("start");
    executor
        .fail_tool(&tool_id, "Command not found")
        .await
        .expect("fail");

    // Failed tools also land in Completed status with error output
    let tools = executor.tools().await;
    let tool = tools.iter().find(|t| t.id == tool_id).expect("tool");
    assert_eq!(tool.status, ToolStatus::Completed);
    assert!(tool.output.as_ref().unwrap().is_error);
}

// ============================================================================
// E2E Test: CostTracker through pipeline
// ============================================================================

#[test]
fn test_cost_tracker_accumulates_across_models() {
    let mut tracker = CostTracker::new("claude-sonnet-4".to_string());

    // Simulate a multi-model session with realistic token counts
    // claude-sonnet-4 pricing: $3.0/1M input, $15.0/1M output
    tracker.record_turn(1, "claude-sonnet-4", 500_000, 200_000);
    tracker.record_turn(2, "gpt-4o", 300_000, 100_000);
    tracker.record_turn(3, "claude-sonnet-4", 800_000, 400_000);

    // Total input: 1.6M, total output: 700K
    assert_eq!(tracker.total_input_tokens, 1_600_000);
    assert_eq!(tracker.total_output_tokens, 700_000);
    // Cost should be significant: well over $1.0
    assert!(tracker.total_cost() > 1.0, "Cost should exceed $1.0");

    // Per-model breakdown
    let breakdowns = tracker.model_breakdowns();
    assert_eq!(breakdowns.len(), 2);

    let sonnet = breakdowns.get("claude-sonnet-4").unwrap();
    assert_eq!(sonnet.input_tokens, 1_300_000);
    assert_eq!(sonnet.turn_count, 2);

    let gpt = breakdowns.get("gpt-4o").unwrap();
    assert_eq!(gpt.input_tokens, 300_000);
    assert_eq!(gpt.turn_count, 1);

    // Budget tracking
    tracker.set_budget(1.0);
    assert!(tracker.is_budget_exceeded());

    let report = tracker.detailed_report();
    assert!(report.contains("Budget"));
    assert!(report.contains("Per-model breakdown"));
}

#[test]
fn test_cost_tracker_pricing_accuracy() {
    // Claude Sonnet: $3/M input, $15/M output
    let cost = CostTracker::calculate_cost("claude-3-5-sonnet-20241022", 1_000_000, 1_000_000);
    assert!(
        (cost - 18.0).abs() < 0.01,
        "Sonnet 1M+1M should cost ~$18, got ${cost}"
    );

    // GPT-4o: $2.5/M input, $10/M output
    let cost = CostTracker::calculate_cost("gpt-4o", 1_000_000, 1_000_000);
    assert!(
        (cost - 12.5).abs() < 0.01,
        "GPT-4o 1M+1M should cost ~$12.5, got ${cost}"
    );

    // Ollama: free
    let cost = CostTracker::calculate_cost("llama3", 1_000_000, 1_000_000);
    assert_eq!(cost, 0.0, "Local models should be free");
}

// ============================================================================
// E2E Test: Tool registry + execution integration
// ============================================================================

#[tokio::test]
async fn test_tool_registry_execution_integration() {
    let registry = ToolRegistry::new();

    // Register multiple tools
    registry
        .register(Box::new(RecordableTool::new(
            "read_file",
            ToolOutput::success("fn main() { println!(\"hello\"); }".to_string()),
        )))
        .unwrap();

    registry
        .register(Box::new(RecordableTool::new(
            "write_file",
            ToolOutput::success("File written successfully".to_string()),
        )))
        .unwrap();

    registry
        .register(Box::new(RecordableTool::new(
            "bash",
            ToolOutput::success("Build succeeded".to_string()),
        )))
        .unwrap();

    // Verify all registered
    let tools = registry.list_tools_info();
    assert_eq!(tools.len(), 3);

    // Execute each tool
    let read_result = registry
        .execute("read_file", json!({"path": "main.rs"}))
        .await;
    assert!(read_result.is_ok());
    assert!(read_result.unwrap().content.contains("fn main()"));

    let write_result = registry
        .execute("write_file", json!({"path": "out.rs", "content": "x"}))
        .await;
    assert!(write_result.is_ok());
    assert!(write_result.unwrap().content.contains("written"));

    let bash_result = registry
        .execute("bash", json!({"command": "cargo build"}))
        .await;
    assert!(bash_result.is_ok());
    assert!(bash_result.unwrap().content.contains("succeeded"));
}

#[tokio::test]
async fn test_tool_registry_duplicate_rejected() {
    let registry = ToolRegistry::new();
    registry
        .register(Box::new(RecordableTool::new(
            "dup",
            ToolOutput::success("ok".to_string()),
        )))
        .unwrap();

    let result = registry.register(Box::new(RecordableTool::new(
        "dup",
        ToolOutput::success("ok2".to_string()),
    )));
    assert!(
        result.is_err(),
        "Duplicate tool registration should be rejected"
    );
}

#[tokio::test]
async fn test_tool_registry_missing_tool_returns_error() {
    let registry = ToolRegistry::new();
    let result = registry.execute("nonexistent", json!({})).await;
    assert!(result.is_err());
}

// ============================================================================
// E2E Test: Session persistence round-trip over the L0 event log (§4.6)
// ============================================================================

#[test]
fn test_session_persistence_round_trip() {
    // The snapshot file is gone; persistence IS the event log now. Seed a
    // two-turn session the way the engine tee does, then project it back.
    let home = tempfile::tempdir().unwrap();
    let container = home.path().join("sessions");
    let store = shannon_core::session_log::SessionStore::new(&container);
    let session_id = Uuid::new_v4();

    {
        let mut w = shannon_core::session_log::SessionLogWriter::open_layout(
            &container,
            &session_id.to_string(),
        )
        .unwrap();
        w.record(
            shannon_types::session_event::SessionEventBody::SessionStart(
                shannon_types::session_event::SessionStartPayload {
                    model: "claude-sonnet-4".into(),
                    provider: Some("anthropic".into()),
                    cwd: None,
                    app_version: None,
                },
            ),
        );
        for (prompt, reply) in [
            (
                "Write a hello world in Rust",
                "Here's a simple hello world:",
            ),
            ("Now add error handling", "Added Result type handling"),
        ] {
            w.record(shannon_types::session_event::SessionEventBody::TurnStart(
                shannon_types::session_event::TurnStartPayload { query_id: None },
            ));
            w.record(shannon_types::session_event::SessionEventBody::UserMessage(
                shannon_types::session_event::UserMessagePayload {
                    source: shannon_types::session_event::UserMessagePayload::SOURCE_USER.into(),
                    content: prompt.into(),
                },
            ));
            w.record(
                shannon_types::session_event::SessionEventBody::AssistantChunk(
                    shannon_types::session_event::AssistantChunkPayload {
                        delta: reply.into(),
                        thinking: false,
                    },
                ),
            );
            w.record(shannon_types::session_event::SessionEventBody::TurnEnd(
                shannon_types::session_event::TurnEndPayload {
                    reason: shannon_types::session_event::TurnEndPayload::REASON_COMPLETED.into(),
                    usage: Some(shannon_types::session_event::TokenUsage {
                        input_tokens: 1250,
                        output_tokens: 600,
                        cache_creation_tokens: 0,
                        cache_read_tokens: 0,
                        cost_usd: None,
                    }),
                    error: None,
                },
            ));
        }
        w.close().unwrap();
    }

    // Curated title survives through the sidecar.
    store
        .save_sidecar(
            &session_id,
            &shannon_core::session_log::SessionSidecar {
                title: Some("Rust hello world".into()),
                ..Default::default()
            },
        )
        .unwrap();

    let loaded = store.load(&session_id).unwrap();
    assert!(loaded.is_some());

    let data = loaded.unwrap();
    assert_eq!(data.session_id, session_id);
    // user → assistant ×2 turns
    assert_eq!(data.messages.len(), 4);
    assert_eq!(data.metadata.title, Some("Rust hello world".to_string()));
    assert_eq!(data.metadata.model, "claude-sonnet-4");
    assert_eq!(data.metadata.total_input_tokens, 2500);
    assert_eq!(data.metadata.total_output_tokens, 1200);
    assert_eq!(data.metadata.turn_count, 2);

    // Delete removes the whole session directory.
    assert!(store.delete(&session_id).unwrap());
    assert!(store.load(&session_id).unwrap().is_none());
}
