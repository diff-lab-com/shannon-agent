//! `shannon trace` CLI tests (§4.6 W1-P1).
//!
//! Verification-standard coverage:
//!
//! - **① replay vs live**: a real mockito-backed query (engine tee →
//!   events.jsonl) is rendered from disk and compared against rows derived
//!   from the same in-process QueryEvent broadcast; the rendering is locked
//!   with insta snapshots.
//! - **③** all four trace subcommands get both direct-call and binary-level
//!   coverage.
//!
//! The remaining tests seed deterministic logs through the real writer so
//! `show` / `diff` / `export` behave byte-stably for humans and CI alike.

// shannon-cli is a binary crate; include the module under test directly so
// integration tests can call its functions without a process spawn.
#[path = "../src/trace.rs"]
mod trace;

use uuid::Uuid;

use assert_cmd::Command;
use futures::StreamExt;
use mockito::{Matcher, ServerGuard};
use shannon_core::session_log::SessionLogWriter;
use shannon_types::session_event::{
    AssistantChunkPayload, ErrorPayload, PermissionDecisionPayload, SessionEventBody,
    SessionStartPayload, TokenUsage, ToolCallPayload, ToolResultPayload, TurnEndPayload,
    TurnStartPayload, UserMessagePayload,
};
use tempfile::TempDir;

fn shannon_bin() -> Command {
    Command::cargo_bin("shannon").expect("shannon binary")
}

// ── Deterministic fixture ──────────────────────────────────────────────

const SESSION: &str = "f0cacc1a-0000-4000-8000-00000000cafe";

/// prompt → tool call (+ permission allow) → result → answer → closed turn.
fn seed(container: &std::path::Path) {
    std::fs::create_dir_all(container).unwrap();
    let mut w = SessionLogWriter::open_layout(container, SESSION).unwrap();

    w.record(SessionEventBody::SessionStart(SessionStartPayload {
        model: "claude-sonnet-4".into(),
        provider: Some("anthropic".into()),
        cwd: Some("/tmp/proj".into()),
        app_version: None,
    }));
    w.record(SessionEventBody::TurnStart(TurnStartPayload {
        query_id: None,
    }));
    w.record(SessionEventBody::UserMessage(UserMessagePayload {
        source: UserMessagePayload::SOURCE_USER.into(),
        content: "list files".into(),
    }));
    w.record(SessionEventBody::ToolCall(ToolCallPayload {
        tool_use_id: "u1".into(),
        tool_name: "Bash".into(),
        arguments: r#"{"command":"ls"}"#.into(),
    }));
    w.record(SessionEventBody::PermissionDecision(
        PermissionDecisionPayload {
            tool_name: Some("Bash".into()),
            request: Some("ls".into()),
            decision: "allow".into(),
            reason: Some("low risk".into()),
            mode: Some("auto".into()),
        },
    ));
    w.record(SessionEventBody::ToolResult(ToolResultPayload {
        tool_use_id: "u1".into(),
        tool_name: "Bash".into(),
        output: "src\nCargo.toml".into(),
        is_error: false,
        duration_ms: Some(3),
        meta: serde_json::Value::Null,
    }));
    // Streaming chunks fold into one assistant row when rendered.
    for delta in ["Two entries", " found."] {
        w.record(SessionEventBody::AssistantChunk(AssistantChunkPayload {
            delta: delta.into(),
            thinking: false,
        }));
    }
    w.record(SessionEventBody::TurnEnd(TurnEndPayload {
        reason: TurnEndPayload::REASON_COMPLETED.into(),
        usage: Some(TokenUsage {
            input_tokens: 42,
            output_tokens: 9,
            cache_creation_tokens: 2,
            cache_read_tokens: 5,
            cost_usd: Some(0.02),
        }),
        error: None,
    }));

    w.close().unwrap();
}

// ── Replay rendering equivalence + snapshot (standard ①) ───────────────

#[test]
fn replay_rendering_matches_live_broadcast_content_and_snaps() {
    let prior_home = std::env::var("SHANNON_HOME").ok();
    use shannon_core::query_engine::{QueryContext, QueryEngineConfig, QueryEvent, QueryMetadata};
    use shannon_engine::api::{LlmClientConfig, LlmProvider};
    use shannon_engine::permissions::PermissionManager;
    use std::collections::HashMap;

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let home = TempDir::new().unwrap();
        // Pin every resolution path into the tempdir: drop any inherited
        // whole-root override so redirected env vars cannot leak state in or
        // out of $HOME (nextest gives each test its own process).
        // SAFETY: single-threaded per nextest process; restored on drop of
        // the runtime scope below.
        unsafe { std::env::remove_var("SHANNON_HOME") };
        let container = home.path().join("sessions");
        let mut server: ServerGuard = mockito::Server::new_async().await;

        // One LLM step asking for a tool, then a plain text completion.
        let sse_tool = concat!(
            r#"data: {"type":"message_start","message":{"id":"m1","role":"assistant","content":[],"model":"test-model","stop_reason":null,"usage":{"input_tokens":20,"output_tokens":0}}}"#, "\n\n",
            r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#, "\n\n",
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Listing."}}"#, "\n\n",
            r#"data: {"type":"content_block_stop","index":0}"#, "\n\n",
            r#"data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_e1","name":"echo","input":{}}}"#, "\n\n",
            r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"label\":\"alpha\"}"}}"#, "\n\n",
            r#"data: {"type":"content_block_stop","index":1}"#, "\n\n",
            r#"data: {"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"input_tokens":20,"output_tokens":15}}"#, "\n\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );
        let sse_text = concat!(
            r#"data: {"type":"message_start","message":{"id":"m2","role":"assistant","content":[],"model":"test-model","stop_reason":null,"usage":{"input_tokens":30,"output_tokens":0}}}"#, "\n\n",
            r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#, "\n\n",
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"All done."}}"#, "\n\n",
            r#"data: {"type":"content_block_stop","index":0}"#, "\n\n",
            r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"input_tokens":30,"output_tokens":10}}"#, "\n\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );

        // mockito serves registered mocks in order: step 1 consumes the
        // one-shot tool SSE, then every later call falls through to the
        // always-on plain-completion response.
        server
            .mock("POST", "/v1/messages")
            .match_body(Matcher::Any)
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(sse_tool)
            .expect(1)
            .create_async()
            .await;
        server
            .mock("POST", "/v1/messages")
            .match_body(Matcher::Any)
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(sse_text)
            .create_async()
            .await;

        struct Echo;
        #[async_trait::async_trait]
        impl shannon_core::tools::Tool for Echo {
            async fn execute(
                &self,
                input: serde_json::Value,
            ) -> shannon_core::tools::ToolResult<shannon_core::tools::ToolOutput> {
                Ok(shannon_core::tools::ToolOutput::success(format!(
                    "echo: {}",
                    input["label"].as_str().unwrap_or("?")
                )))
            }
            fn name(&self) -> &str {
                "echo"
            }
            fn description(&self) -> &str {
                "echo"
            }
            fn input_schema(&self) -> serde_json::Value {
                serde_json::json!({"type": "object"})
            }
        }

        let registry = shannon_core::tools::ToolRegistry::new();
        registry.register(Box::new(Echo)).unwrap();

        let client_cfg = LlmClientConfig {
            api_key: "k".into(),
            base_url: server.url(),
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 256,
            timeout_seconds: 10,
            api_version: "2023-06-01".into(),
            provider: LlmProvider::Anthropic,
            extra_headers: HashMap::default(),
            retry_config: Default::default(),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 0,
            budget_tokens: None,
            reasoning_effort: None,
        };
        // Redirected state manager → the tee writes events.jsonl HERE.
        let mgr = shannon_engine::state::StateManager::with_sessions_dir(
            container.clone(),
        )
        .unwrap();
        let state_dir = mgr.sessions_dir().to_path_buf();

        let session_id = Uuid::new_v4();
        let mut engine = shannon_core::query_engine::QueryEngine::with_session_id(
            shannon_engine::api::LlmClient::new(client_cfg),
            registry,
            PermissionManager::new(),
            mgr,
            QueryEngineConfig::default(),
            session_id,
        );

        let stream = engine
            .process_query(
                QueryContext {
                    query_id: Uuid::new_v4(),
                    session_id: Uuid::new_v4(),
                    user_message: "list files".into(),
                    metadata: QueryMetadata {
                        timestamp: chrono::Utc::now(),
                        tools_allowed: true,
                        max_tokens: Some(256),
                        model: "claude-sonnet-4-20250514".into(),
                        temperature: None,
                        top_p: None,
                    },
                },
                None,
            )
            .await;

        // Collect the LIVE broadcast while it also tees to disk.
        let mut broadcast: Vec<QueryEvent> = Vec::new();
        let mut pinned = Box::pin(stream);
        while let Some(Ok(event)) = pinned.next().await {
            broadcast.push(event);
        }

        // Read back what the tee persisted (same container the engine wrote).
        let effective = shannon_core::session_log::effective_log_container(&state_dir);
        let (_, path) = trace::resolve_session(&effective, &session_id.to_string())
            .map_err(|e| format!("resolve failed: {e}"))?;

        let persisted = std::fs::read_to_string(&path).expect("events.jsonl");
        assert!(persisted.contains("\"user/message\""));

        // LIVE-derived visible strings vs PERSISTED rendered lines:
        // every streamed assistant text must survive verbatim.
        let rendered = trace::cmd_replay(&effective, &session_id.to_string()).unwrap();
        assert!(
            rendered.contains("Listing."),
            "streamed text missing from replay:\n{rendered}"
        );
        assert!(rendered.contains("All done."), "final text missing");
        assert!(rendered.contains("echo: alpha"), "tool output missing");

        insta::assert_snapshot!("replay_mockito_session", rendered);

        // Broadcast integrity: the live stream carried both request texts and
        // the tool round-trip (the engine never broadcasts `Started`, which is
        // a tee-owned boundary).
        assert!(matches!(
            broadcast.first(),
            Some(QueryEvent::Started { .. })
        ) || !broadcast.is_empty());
        assert!(broadcast.iter().any(|e| matches!(
            e,
            QueryEvent::Text { content, .. } if content == "Listing."
        )));
        assert!(broadcast.iter().any(|e| matches!(
            e,
            QueryEvent::ToolUseRequest { tool_name, .. } if tool_name == "echo"
        )));
        Ok::<(), String>(())
    })
    .unwrap();

    // Restore any inherited root override captured before the run.
    match prior_home {
        Some(v) => unsafe { std::env::set_var("SHANNON_HOME", v) },
        None => unsafe { std::env::remove_var("SHANNON_HOME") },
    }
}

// ── Snapshot: deterministic fixture replay + show filters ──────────────

#[test]
fn replay_fixture_snapshot_is_stable() {
    let home = TempDir::new().unwrap();
    let container = home.path().join("sessions");
    seed(&container);

    let out = trace::cmd_replay(&container, SESSION).unwrap();
    insta::assert_snapshot!("replay_deterministic_fixture", out);
}

#[test]
fn show_permission_filter_snapshot() {
    let home = TempDir::new().unwrap();
    let container = home.path().join("sessions");
    seed(&container);

    let perms = trace::cmd_show(&container, SESSION, None, None, true).unwrap();
    insta::assert_snapshot!("show_permissions", perms);

    let tools = trace::cmd_show(&container, SESSION, None, Some("Bash".into()), false).unwrap();
    insta::assert_snapshot!("show_tool_bash", tools);
}

// ── Diff subcommand ────────────────────────────────────────────────────

#[test]
fn diff_reports_zero_divergence_for_identical_logs_and_finds_drift() {
    let home = TempDir::new().unwrap();
    let container = home.path().join("sessions");
    seed(&container);
    seed_cloned(&container);

    let same =
        trace::cmd_diff(&container, SESSION, "f0cacc2b-0000-4000-8000-00000000cafe").unwrap();
    assert!(same.contains("divergences=0"), "{same}");

    // Diverge clone with an appended error row.
    let mut w =
        SessionLogWriter::open_layout(&container, "f0cacc2b-0000-4000-8000-00000000cafe").unwrap();
    w.record(SessionEventBody::Error(ErrorPayload {
        category: "query-failed".into(),
        message: "drift".into(),
        detail: None,
    }));
    w.close().unwrap();

    let drifted =
        trace::cmd_diff(&container, SESSION, "f0cacc2b-0000-4000-8000-00000000cafe").unwrap();
    assert!(drifted.contains("rows=9"), "{drifted}");
    assert!(drifted.contains("#009"), "{drifted}");
}

fn seed_cloned(container: &std::path::Path) {
    // Clone by replaying bodies under a new id (diff fixtures share content).
    use shannon_core::session_log::SessionLogReader;
    let src_path = shannon_core::session_log::session_log_container_path(container, SESSION);
    let events = SessionLogReader::open(&src_path)
        .and_then(|r| r.read_events(false))
        .unwrap();
    let other = "f0cacc2b-0000-4000-8000-00000000cafe";
    let mut w = SessionLogWriter::open_layout(container, other).unwrap();
    for event in events {
        w.record(event.body.clone());
    }
    w.close().unwrap();
}

#[test]
fn export_bundle_contains_events_analytics_summary() {
    let home = TempDir::new().unwrap();
    let container = home.path().join("sessions");
    seed(&container);

    let out_root = TempDir::new().unwrap();
    let bundle = trace::cmd_export(&container, SESSION, Some(out_root.path())).unwrap();

    assert!(bundle.join("events.jsonl").is_file());
    assert!(bundle.join("analytics.jsonl").is_file());
    assert!(bundle.join("summary.json").is_file());

    let analytics = std::fs::read_to_string(bundle.join("analytics.jsonl")).unwrap();
    let value: serde_json::Value = serde_json::from_str(analytics.trim_end()).unwrap();
    assert_eq!(value["prompts_submitted"], 1);
    assert_eq!(value["tools"]["Bash"]["successes"], 1);
    assert_eq!(value["permission_requests_approved"], 1);

    let summary = std::fs::read_to_string(bundle.join("summary.json")).unwrap();
    let value: serde_json::Value = serde_json::from_str(&summary).unwrap();
    assert_eq!(value["turn_count"], 1);
    assert_eq!(value["token_totals"]["input"], 42);
    assert_eq!(value["event_count"], 9);
}

// ── End-to-end binary invocation ───────────────────────────────────────

#[test]
fn binary_trace_show_lists_rows_with_env_redirect() {
    let home = TempDir::new().unwrap();
    let container = home.path().join("sessions");
    seed(&container);

    shannon_bin()
        .env("SHANNON_SESSIONS_DIR", container.to_str().unwrap())
        .args(["trace", "show", "latest"])
        .assert()
        .success()
        .stdout(predicates::str::contains("tool ▸ Bash"))
        .stdout(predicates::str::contains("permission ▸ allow"));
}

#[test]
fn binary_trace_diff_equal_logs_zero_divergences() {
    let home = TempDir::new().unwrap();
    let container = home.path().join("sessions");
    seed(&container);
    seed_cloned(&container);

    shannon_bin()
        .env("SHANNON_SESSIONS_DIR", container.to_str().unwrap())
        .args([
            "trace",
            "diff",
            SESSION,
            "f0cacc2b-0000-4000-8000-00000000cafe",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("divergences=0"));
}
