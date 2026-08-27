//! Session state integration tests over the L0 event log (§4.6 W1-P1).
//!
//! The legacy single-file snapshot store is gone; these tests pin the
//! cutover contract:
//!
//! 1. **Restore round-trip equivalence** — a multi-turn session written the
//!    way a live engine writes it projects back into the exact in-memory
//!    state (`write → "process exit" → re-enter → state equal`).
//! 2. Listing / project filtering / branch cut-off / delete semantics.
//! 3. Golden agreement between what was written and what is projected at
//!    every step (the retained form of the pre-cutover reconciliation).

use shannon_core::session_log::{SessionSidecar, SessionStore, SessionTee};
use shannon_engine::api::{ContentBlock, Message, MessageContent};
use uuid::Uuid;

fn store(tmp: &tempfile::TempDir) -> SessionStore {
    SessionStore::new(tmp.path().join("sessions"))
}

fn user(text: &str) -> Message {
    Message {
        role: "user".to_string(),
        content: MessageContent::Text(text.to_string()),
    }
}

fn assistant_text(text: &str) -> Message {
    Message {
        role: "assistant".to_string(),
        content: MessageContent::Blocks(vec![ContentBlock::Text {
            text: text.to_string(),
        }]),
    }
}

/// Seed a realistic two-turn session: prompt → assistant(tool_use) →
/// tool_result user message → assistant final; then a plain second turn.
/// Writes through the same tee/writer primitives the engine uses live.
fn seed_two_turn_session(store: &SessionStore, id: &Uuid) {
    let container = store.container().to_path_buf();
    let mut tee =
        SessionTee::open_in_container(&container, &id.to_string(), "test-model", Some("anthropic"));

    // Turn 1
    tee.record_user_message("Run ls for me");
    tee.record_turn_start(None);
    tee.record_query_event(&shannon_core::QueryEvent::Text {
        query_id: Uuid::new_v4(),
        content: "Let me check.".into(),
    });
    tee.record_query_event(&shannon_core::QueryEvent::ToolUseRequest {
        query_id: Uuid::new_v4(),
        tool_use_id: "toolu_1".into(),
        tool_name: "Bash".into(),
        tool_input: serde_json::json!({"command": "ls"}),
    });
    tee.record_query_event(&shannon_core::QueryEvent::ToolUseResult {
        query_id: Uuid::new_v4(),
        tool_use_id: "toolu_1".into(),
        tool_name: "Bash".into(),
        result: "src\nCargo.toml".into(),
        is_error: false,
    });
    tee.record_query_event(&shannon_core::QueryEvent::Text {
        query_id: Uuid::new_v4(),
        content: "Two entries.".into(),
    });
    tee.record_query_event(&shannon_core::QueryEvent::Usage {
        query_id: Uuid::new_v4(),
        input_tokens: 100,
        output_tokens: 30,
        cost_usd: 0.05,
        cache_creation_tokens: 12,
        cache_read_tokens: 44,
    });
    tee.record_query_event(&shannon_core::QueryEvent::Completed {
        query_id: Uuid::new_v4(),
    });

    // Turn 2 (plain prompt)
    tee.record_user_message("Thanks");
    tee.record_turn_start(None);
    tee.record_query_event(&shannon_core::QueryEvent::Text {
        query_id: Uuid::new_v4(),
        content: "Anytime!".into(),
    });
    tee.record_query_event(&shannon_core::QueryEvent::Usage {
        query_id: Uuid::new_v4(),
        input_tokens: 20,
        output_tokens: 2,
        cost_usd: 0.01,
        cache_creation_tokens: 0,
        cache_read_tokens: 0,
    });
    tee.record_query_event(&shannon_core::QueryEvent::Completed {
        query_id: Uuid::new_v4(),
    });
    tee.close();

    store
        .save_sidecar(
            id,
            &SessionSidecar {
                title: Some("Golden session".into()),
                ..Default::default()
            },
        )
        .expect("sidecar write");
}

// ── Restore round-trip equivalence ──────────────────────────────────────

#[test]
fn test_restore_roundtrip_is_state_equivalent() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(&tmp);
    let session_id = Uuid::new_v4();

    seed_two_turn_session(&store, &session_id);

    // ── in-memory ground truth of what the live conversation held ──
    let expected_messages = vec![
        user("Run ls for me"),
        Message {
            role: "assistant".to_string(),
            content: MessageContent::Blocks(vec![
                ContentBlock::Text {
                    text: "Let me check.".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "toolu_1".to_string(),
                    name: "Bash".to_string(),
                    input: serde_json::json!({"command": "ls"}),
                },
            ]),
        },
        Message {
            role: "user".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: "toolu_1".to_string(),
                content: Some(shannon_engine::api::ToolResultContent::Single(
                    "src\nCargo.toml".to_string(),
                )),
                is_error: Some(false),
            }]),
        },
        assistant_text("Two entries."),
        user("Thanks"),
        assistant_text("Anytime!"),
    ];

    // Re-open "after process exit": brand-new store instance, fresh read.
    let reopened = SessionStore::new(tmp.path().join("sessions"));
    let restored = reopened
        .load(&session_id)
        .expect("load after restart")
        .expect("session present");

    assert_eq!(restored.session_id, session_id);
    assert_eq!(restored.messages.len(), expected_messages.len());
    for (got, want) in restored.messages.iter().zip(expected_messages.iter()) {
        assert_eq!(
            serde_json::to_value(got).unwrap(),
            serde_json::to_value(want).unwrap(),
            "projected message diverged from live state"
        );
    }

    // Derived totals + curated metadata agree with what went in.
    assert_eq!(restored.metadata.model, "test-model");
    assert_eq!(restored.metadata.title.as_deref(), Some("Golden session"));
    assert_eq!(restored.metadata.turn_count, 2);
    assert_eq!(restored.metadata.total_input_tokens, 120);
    assert_eq!(restored.metadata.total_output_tokens, 32);
    assert_eq!(
        restored.messages[0].role, expected_messages[0].role,
        "history order preserved"
    );
}

#[test]
fn test_roundtrip_second_process_reentry_matches_first_projection() {
    // Simulate two successive process lifetimes against one log: each
    // projection must be identical to the previous one.
    let tmp = tempfile::tempdir().unwrap();
    let first = store(&tmp);
    let id = Uuid::new_v4();
    seed_two_turn_session(&first, &id);

    let a = first.load(&id).unwrap().unwrap();
    let b = store(&tmp).load(&id).unwrap().unwrap();
    assert_eq!(a.messages.len(), b.messages.len());
    for (ma, mb) in a.messages.iter().zip(b.messages.iter()) {
        assert_eq!(
            serde_json::to_value(ma).unwrap(),
            serde_json::to_value(mb).unwrap()
        );
    }
    assert_eq!(a.metadata, b.metadata);
}

// ── Engine-facing restore (the actual REPL resume path) ─────────────────

#[tokio::test]
async fn test_query_engine_restore_reads_l0_only() {
    use shannon_core::query_engine::QueryEngine;
    use shannon_engine::api::LlmClientConfig;
    use shannon_engine::permissions::PermissionManager;
    use std::collections::HashMap;

    let tmp = tempfile::tempdir().unwrap();
    let mgr = shannon_engine::state::StateManager::with_sessions_dir(tmp.path().join("sessions"))
        .unwrap();
    let store = SessionStore::new(mgr.sessions_dir());
    let session_id = Uuid::new_v4();
    seed_two_turn_session(&store, &session_id);

    let client_cfg = LlmClientConfig {
        api_key: "k".into(),
        base_url: "http://localhost".into(),
        model: "m".into(),
        max_tokens: 64,
        timeout_seconds: 5,
        api_version: String::new(),
        provider: shannon_engine::api::LlmProvider::Anthropic,
        extra_headers: HashMap::new(),
        retry_config: Default::default(),
        fallback_provider: None,
        fallback_base_url: None,
        max_stream_reconnects: 0,
        budget_tokens: None,
        reasoning_effort: None,
    };
    let mut engine = QueryEngine::with_defaults(
        shannon_engine::api::LlmClient::new(client_cfg),
        shannon_core::tools::ToolRegistry::new(),
        PermissionManager::new(),
        mgr,
    );
    assert_ne!(engine.session_id(), session_id);

    let found = engine.restore_session(session_id).expect("restore ok");
    assert!(found, "L0 log must restore");
    assert_eq!(engine.session_id(), session_id);
    assert_eq!(engine.conversation_history().len(), 6);
}

// ── Listing / project scoping / branching / delete ──────────────────────

#[test]
fn test_listing_previews_and_empty_store() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(&tmp);
    assert!(store.list().unwrap().is_empty());

    let id = Uuid::new_v4();
    seed_two_turn_session(&store, &id);

    let infos = store.list().unwrap();
    assert_eq!(infos.len(), 1);
    assert_eq!(infos[0].preview.as_deref(), Some("Run ls for me"));
    assert_eq!(infos[0].last_user_preview.as_deref(), Some("Thanks"));
    assert_eq!(infos[0].turn_count, 2);
}

#[test]
fn test_branch_cut_off_keeps_prefix_events() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(&tmp);
    let parent = Uuid::new_v4();
    seed_two_turn_session(&store, &parent);

    // Branch keeping only the first prompt.
    let branch = store.create_branch(&parent, 1, None).unwrap();
    assert_eq!(branch.messages.len(), 1);
    assert_eq!(branch.metadata.parent_session_id, Some(parent));
    assert_eq!(branch.metadata.branch_point_message_index, Some(1));

    // Empty branch carries just the seed marker.
    let empty_branch = store.create_branch(&parent, 0, None).unwrap();
    assert!(empty_branch.messages.is_empty());
}

#[test]
fn test_delete_then_reload_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(&tmp);
    let id = Uuid::new_v4();
    seed_two_turn_session(&store, &id);

    assert!(store.delete(&id).unwrap());
    assert!(store.load(&id).unwrap().is_none());
}
