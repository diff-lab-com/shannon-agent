//! Integration tests for the §4.2 session-log tee.
//!
//! Verification standard ①: after a session completes, the request envelope
//! rebuilt from `events.jsonl` must be **byte-identical** to the request the
//! engine actually sent. mockito (1.x) cannot capture request bodies, so the
//! equality is proven the other way around:
//!
//! 1. Run a deterministic two-step query (tool call, then final answer)
//!    against a loose mock and collect every `request/header.wire_body`
//!    from the log (the adapter's own serialized product, teed at
//!    serialization time).
//! 2. Re-run the *identical* query against fresh mocks whose body matchers
//!    are `Matcher::Binary` (raw byte equality) fed with the rebuilt bytes.
//!    A single hit per mock proves the live request equals the
//!    reconstruction byte for byte.

#![allow(clippy::unwrap_used)]

mod session_log_tee {
    use async_trait::async_trait;
    use futures::StreamExt;
    use mockito::{Matcher, Server, ServerGuard};
    use serde_json::{Value, json};
    use shannon_core::query_engine::{
        QueryContext, QueryEngine, QueryEngineConfig, QueryEvent, QueryMetadata,
    };
    use shannon_core::session_log::{SessionLogReader, session_events_path};
    use shannon_core::tools::{Tool, ToolOutput, ToolRegistry, ToolResult};
    use shannon_engine::api::{LlmClientConfig, LlmProvider};
    use shannon_engine::permissions::PermissionManager;
    use shannon_engine::state::StateManager;
    use std::collections::HashMap;
    use tempfile::TempDir;
    use uuid::Uuid;

    /// Sets `SHANNON_HOME` (and optionally `SHANNON_SESSION_LOG`) for one
    /// test, restoring the previous values on drop.
    struct EnvGuard {
        vars: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    /// Serializes the tests in this module: every one points the
    /// process-global `SHANNON_HOME` at its own tempdir via [`EnvGuard`],
    /// and under plain `cargo test` (libtest: one process, many threads —
    /// the Nightly Coverage invocation) the guards overwrite each other,
    /// sending one test's log writes into another test's home directory.
    /// nextest never sees this (one process per test); this lock restores
    /// the same isolation under libtest.
    fn global_state_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    impl EnvGuard {
        fn set(vars: Vec<(&'static str, Option<&str>)>) -> Self {
            let mut saved = Vec::new();
            for (key, value) in vars {
                let old = std::env::var_os(key);
                unsafe {
                    match value {
                        Some(v) => std::env::set_var(key, v),
                        None => std::env::remove_var(key),
                    }
                }
                saved.push((key, old));
            }
            Self { vars: saved }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, old) in &self.vars {
                unsafe {
                    match old {
                        Some(v) => std::env::set_var(key, v),
                        None => std::env::remove_var(key),
                    }
                }
            }
        }
    }

    /// A deterministic tool used by both query phases.
    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
            let label = input["label"].as_str().unwrap_or("none").to_string();
            Ok(ToolOutput::success(format!("echo: {label}")))
        }
        fn name(&self) -> &str {
            "echo"
        }
        fn description(&self) -> &str {
            "echoes its label back"
        }
        fn input_schema(&self) -> Value {
            json!({
                "type": "object",
                "properties": {"label": {"type": "string"}},
                "required": ["label"]
            })
        }
    }

    fn make_registry() -> ToolRegistry {
        let registry = ToolRegistry::new();
        registry.register(Box::new(EchoTool)).unwrap();
        registry
    }

    fn make_engine(mock_url: &str, session_id: Uuid) -> QueryEngine {
        let config = LlmClientConfig {
            api_key: "test-key".to_string(),
            base_url: mock_url.to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 4096,
            timeout_seconds: 10,
            api_version: "2023-06-01".to_string(),
            provider: LlmProvider::Anthropic,
            extra_headers: HashMap::new(),
            retry_config: shannon_engine::api::RetryConfig::default(),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 0,
            budget_tokens: None,
            reasoning_effort: None,
        };
        QueryEngine::with_session_id(
            shannon_engine::api::LlmClient::new(config),
            make_registry(),
            PermissionManager::new(),
            StateManager::new(),
            QueryEngineConfig::default(),
            session_id,
        )
    }

    fn make_context(msg: &str) -> QueryContext {
        QueryContext {
            query_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            user_message: msg.to_string(),
            metadata: QueryMetadata {
                timestamp: chrono::Utc::now(),
                tools_allowed: true,
                max_tokens: Some(4096),
                model: "claude-sonnet-4-20250514".to_string(),
                temperature: None,
                top_p: None,
            },
        }
    }

    /// SSE response: text + one tool_use block (stop_reason: tool_use).
    fn sse_tool_response() -> String {
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"role\":\"assistant\",\"content\":[],\"model\":\"test-model\",\"stop_reason\":null,\"usage\":{\"input_tokens\":20,\"output_tokens\":0}}}\n\n\
         data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
         data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Let me echo.\"}}\n\n\
         data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
         data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_e1\",\"name\":\"echo\",\"input\":{}}}\n\n\
         data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"label\\\":\\\"alpha\\\"}\"}}\n\n\
         data: {\"type\":\"content_block_stop\",\"index\":1}\n\n\
         data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"input_tokens\":20,\"output_tokens\":15}}\n\n\
         data: {\"type\":\"message_stop\"}\n\n"
            .to_string()
    }

    /// SSE response: final text (stop_reason: end_turn).
    fn sse_text_response() -> String {
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_2\",\"role\":\"assistant\",\"content\":[],\"model\":\"test-model\",\"stop_reason\":null,\"usage\":{\"input_tokens\":30,\"output_tokens\":0}}}\n\n\
         data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
         data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"All done.\"}}\n\n\
         data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
         data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"input_tokens\":30,\"output_tokens\":10}}\n\n\
         data: {\"type\":\"message_stop\"}\n\n"
            .to_string()
    }

    async fn run_query(engine: &QueryEngine, msg: &str) -> Vec<QueryEvent> {
        let stream = engine.process_query(make_context(msg), None).await;
        let mut events = Vec::new();
        let mut pinned = Box::pin(stream);
        while let Some(Ok(event)) = pinned.next().await {
            events.push(event);
        }
        events
    }

    /// Read the events log and return the raw `wire_body` values of every
    /// `request/header`, in order.
    fn logged_wire_bodies(home: &TempDir, session_id: Uuid) -> Vec<Value> {
        let path = session_events_path(home.path(), &session_id.to_string());
        let reader = SessionLogReader::open(&path).expect("events.jsonl exists");
        reader
            .read_events(false)
            .expect("read events")
            .into_iter()
            .filter_map(|e| match e.body {
                shannon_types::session_event::SessionEventBody::RequestHeader(p) => p.wire_body,
                _ => None,
            })
            .collect()
    }

    /// Verification ①: the request envelopes rebuilt from `events.jsonl`
    /// are byte-identical to the requests the engine actually sends.
    #[tokio::test]
    async fn test_request_envelope_rebuild_is_byte_identical_to_wire() {
        let _guard = global_state_lock();
        let home = TempDir::new().expect("tempdir");
        let session_id = Uuid::new_v4();
        let _env = EnvGuard::set(vec![("SHANNON_HOME", Some(home.path().to_str().unwrap()))]);

        // ── Phase 1: loose mocks, deterministic two-step query. ──
        let mut server = Server::new_async().await;
        let mock_url = server.url();
        let _loose1 = server
            .mock("POST", "/v1/messages")
            .match_body(Matcher::Any)
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(sse_tool_response())
            .create();
        let _loose2 = server
            .mock("POST", "/v1/messages")
            .match_body(Matcher::Any)
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(sse_text_response())
            .create();

        let engine = make_engine(&mock_url, session_id);
        let events = run_query(&engine, "please echo alpha").await;
        assert!(
            events
                .iter()
                .any(|e| matches!(e, QueryEvent::Completed { .. })),
            "phase-1 query must complete"
        );

        // Rebuild the envelopes from the log (the wire product, serialized
        // compactly exactly like reqwest's `.json(&value)` does).
        let bodies = logged_wire_bodies(&home, session_id);
        assert_eq!(bodies.len(), 2, "one request/header per engine turn");
        let rebuilt: Vec<Vec<u8>> = bodies
            .iter()
            .map(|b| serde_json::to_vec(b).expect("serialize wire body"))
            .collect();

        // ── Phase 2: byte-exact mocks fed with the rebuilt envelopes. ──
        let mut server2 = Server::new_async().await;
        let mock_url2 = server2.url();
        let strict1 = byte_exact_mock(&mut server2, rebuilt[0].clone(), sse_tool_response(), 1);
        let strict2 = byte_exact_mock(&mut server2, rebuilt[1].clone(), sse_text_response(), 1);

        let engine2 = make_engine(&mock_url2, session_id);
        let events2 = run_query(&engine2, "please echo alpha").await;
        if !events2
            .iter()
            .any(|e| matches!(e, QueryEvent::Completed { .. }))
        {
            // A byte mismatch makes mockito 404 the live request. Dump
            // everything needed to diagnose the divergence: what the engine
            // saw (error events carry the HTTP status) and the exact bytes
            // the matchers demanded. Without this the failure is
            // undiagnosable from CI logs alone.
            let expected: Vec<String> = rebuilt
                .iter()
                .map(|b| String::from_utf8_lossy(b).to_string())
                .collect();
            panic!(
                "phase-2 query must complete (a byte mismatch would 404 here)\n\
                 engine events: {events2:#?}\n\
                 expected byte-exact bodies (rebuilt from the phase-1 log):\n\
                 [0] = {}\n\
                 [1] = {}",
                expected[0], expected[1]
            );
        }

        // Exactly one hit each: the live request bytes equal the rebuilt
        // envelope bytes — no extra tolerance, no ordering leeway.
        strict1.assert();
        strict2.assert();
    }

    fn byte_exact_mock(
        server: &mut ServerGuard,
        body: Vec<u8>,
        response: String,
        hits: usize,
    ) -> mockito::Mock {
        server
            .mock("POST", "/v1/messages")
            .match_body(Matcher::from(body))
            .expect(hits)
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(response)
            .create()
    }

    /// Verification ④: a 10-turn session stays well under 5 MB on disk.
    #[tokio::test]
    async fn test_ten_turn_session_under_5mb() {
        let _guard = global_state_lock();
        let home = TempDir::new().expect("tempdir");
        let session_id = Uuid::new_v4();
        let _env = EnvGuard::set(vec![("SHANNON_HOME", Some(home.path().to_str().unwrap()))]);

        let mut server = Server::new_async().await;
        let mock_url = server.url();
        let _mock = server
            .mock("POST", "/v1/messages")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(sse_text_response())
            .create();

        let engine = make_engine(&mock_url, session_id);
        for i in 0..10 {
            run_query(&engine, &format!("turn {i}: summarize")).await;
        }

        let path = session_events_path(home.path(), &session_id.to_string());
        let size = std::fs::metadata(&path).expect("events.jsonl").len();
        assert!(size < 5 * 1024 * 1024, "10-turn log is {size} bytes");

        // The log is a coherent session: one session/start, ten turns.
        let reader = SessionLogReader::open(&path).unwrap();
        let events = reader.read_events(false).unwrap();
        let starts = events
            .iter()
            .filter(|e| e.kind() == shannon_types::session_event::SessionEventKind::SessionStart)
            .count();
        let turn_starts = events
            .iter()
            .filter(|e| e.kind() == shannon_types::session_event::SessionEventKind::TurnStart)
            .count();
        let turn_ends = events
            .iter()
            .filter(|e| e.kind() == shannon_types::session_event::SessionEventKind::TurnEnd)
            .count();
        assert_eq!(starts, 1);
        assert_eq!(turn_starts, 10);
        assert_eq!(turn_ends, 10);
        // Envelope turn numbering is continuous across the session.
        let last_turn = events.last().unwrap().turn;
        assert_eq!(last_turn, 10);
    }

    /// `SHANNON_SESSION_LOG=off` writes nothing.
    #[tokio::test]
    async fn test_switch_off_writes_nothing() {
        let _guard = global_state_lock();
        let home = TempDir::new().expect("tempdir");
        let session_id = Uuid::new_v4();
        let _env = EnvGuard::set(vec![
            ("SHANNON_HOME", Some(home.path().to_str().unwrap())),
            ("SHANNON_SESSION_LOG", Some("off")),
        ]);

        let mut server = Server::new_async().await;
        let mock_url = server.url();
        let _mock = server
            .mock("POST", "/v1/messages")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(sse_text_response())
            .create();

        let engine = make_engine(&mock_url, session_id);
        let events = run_query(&engine, "hello").await;
        assert!(
            events
                .iter()
                .any(|e| matches!(e, QueryEvent::Completed { .. }))
        );
        assert!(
            !session_events_path(home.path(), &session_id.to_string()).exists(),
            "disabled tee must not create events.jsonl"
        );
    }

    /// Secrets surfaced in tool output are masked in the log (minimal
    /// redaction, plan §4.2; the full policy lands in §4.14).
    #[tokio::test]
    async fn test_secret_in_tool_output_is_redacted() {
        let _guard = global_state_lock();
        struct LeakyTool;

        #[async_trait]
        impl Tool for LeakyTool {
            async fn execute(&self, _input: Value) -> ToolResult<ToolOutput> {
                Ok(ToolOutput::success(
                    "token sk-ant-abc123def456 rejected".to_string(),
                ))
            }
            fn name(&self) -> &str {
                "leak"
            }
            fn description(&self) -> &str {
                "leaks a token"
            }
            fn input_schema(&self) -> Value {
                json!({"type": "object", "properties": {}})
            }
        }

        let home = TempDir::new().expect("tempdir");
        let session_id = Uuid::new_v4();
        let _env = EnvGuard::set(vec![("SHANNON_HOME", Some(home.path().to_str().unwrap()))]);

        let mut server = Server::new_async().await;
        let mock_url = server.url();
        let sse_leak = sse_tool_response().replace("\"name\":\"echo\"", "\"name\":\"leak\"");
        let _m1 = server
            .mock("POST", "/v1/messages")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(sse_leak)
            .create();
        let _m2 = server
            .mock("POST", "/v1/messages")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(sse_text_response())
            .create();

        let config = LlmClientConfig {
            api_key: "test-key".to_string(),
            base_url: mock_url.clone(),
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 4096,
            timeout_seconds: 10,
            api_version: "2023-06-01".to_string(),
            provider: LlmProvider::Anthropic,
            extra_headers: HashMap::new(),
            retry_config: shannon_engine::api::RetryConfig::default(),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 0,
            budget_tokens: None,
            reasoning_effort: None,
        };
        let registry = ToolRegistry::new();
        registry.register(Box::new(LeakyTool)).unwrap();
        let engine = QueryEngine::with_session_id(
            shannon_engine::api::LlmClient::new(config),
            registry,
            PermissionManager::new(),
            StateManager::new(),
            QueryEngineConfig::default(),
            session_id,
        );
        run_query(&engine, "run the leaky tool").await;

        let path = session_events_path(home.path(), &session_id.to_string());
        let raw = std::fs::read_to_string(&path).expect("events.jsonl");
        assert!(
            !raw.contains("sk-ant-abc123def456"),
            "secret must be masked in the log"
        );
        assert!(raw.contains("[REDACTED]"));
    }
}
