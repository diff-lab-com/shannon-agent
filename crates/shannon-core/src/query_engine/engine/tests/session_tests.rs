use super::*;

#[tokio::test]
async fn probe_active_health_errors_on_unreachable_endpoint_without_swapping_key() {
    // `/provider health` reuses the running client's existing key (no swap,
    // unlike validate_credential) and must surface an Err — never panic or
    // hang — when the endpoint is down. Port 1 cannot be bound without root,
    // so connecting is refused near-instantly; this exercises the full
    // validate_connection() → send_message() → HTTP path deterministically,
    // without fragile mockito path-matching. send_message (not the _with_retry
    // variant) is single-attempt, so there is no retry backoff to wait out.
    let config = LlmClientConfig {
        api_key: "running-client-key".to_string(),
        base_url: "http://127.0.0.1:1".to_string(),
        model: "test-model".to_string(),
        provider: LlmProvider::Ollama,
        ..Default::default()
    };
    let client = LlmClient::new(config);
    let engine = QueryEngine::new(
        client,
        ToolRegistry::new(),
        PermissionManager::new(),
        StateManager::new(),
        QueryEngineConfig::default(),
    );
    let result = engine.probe_active_health().await;
    assert!(
        result.is_err(),
        "an unreachable endpoint must surface an error, not Ok or a panic"
    );
}

#[tokio::test]
async fn probe_all_health_returns_valid_verdicts() {
    // Smoke test: `probe_all_health` returns a Vec<ProviderHealth> whose
    // entries are well-formed. We do NOT assert specific providers (the
    // SHANNON_*_PROVIDERS allowlist may filter them in CI) or specific
    // statuses (those depend on the local env's keys and network state).
    // We only require the structure to be sound.
    let engine = create_test_engine();
    let health = engine
        .probe_all_health(std::time::Duration::from_millis(200))
        .await;
    // Every entry has a documented status variant; NotConfigured carries
    // no latency.
    for h in &health {
        assert!(
            matches!(
                h.status,
                ProviderHealthStatus::Reachable
                    | ProviderHealthStatus::AuthFailed
                    | ProviderHealthStatus::Unreachable
                    | ProviderHealthStatus::NotConfigured
            ),
            "unexpected status: {:?}",
            h.status
        );
        if h.status == ProviderHealthStatus::NotConfigured {
            assert!(
                h.latency_ms.is_none(),
                "NotConfigured must have no latency_ms"
            );
        }
    }
}

#[test]
fn test_query_engine_session_id() {
    let client = create_test_client();
    let tools = ToolRegistry::new();
    let permissions = PermissionManager::new();
    let state = StateManager::new();
    let config = QueryEngineConfig::default();

    let engine = QueryEngine::new(client, tools, permissions, state, config);
    let session_id = engine.session_id();

    // Should generate a valid UUID
    assert_ne!(session_id, Uuid::nil());
}

#[test]
fn test_query_engine_with_session_id() {
    let client = create_test_client();
    let tools = ToolRegistry::new();
    let permissions = PermissionManager::new();
    let state = StateManager::new();
    let config = QueryEngineConfig::default();

    let specific_id = Uuid::new_v4();
    let engine =
        QueryEngine::with_session_id(client, tools, permissions, state, config, specific_id);

    assert_eq!(engine.session_id(), specific_id);
}

#[test]
fn test_save_and_restore_session_roundtrips_through_l0() {
    // §4.6: the restore roundtrip runs through the L0 event log only —
    // a live-looking session is written by the tee, then projected back.
    let temp_dir = env::temp_dir()
        .join("shannon-session-test")
        .join(Uuid::new_v4().to_string());
    fs::create_dir_all(&temp_dir).unwrap();

    let state = Arc::new(StateManager::with_sessions_dir(temp_dir.clone()).unwrap());
    let session_id = Uuid::new_v4();

    // Seed the L0 log exactly as a real query would (tee + writer).
    {
        let mut tee = crate::session_log::SessionTee::open_in_container(
            state.sessions_dir(),
            &session_id.to_string(),
            "test-model",
            Some("anthropic"),
        );
        tee.record_user_message("Hello, how are you?");
        tee.record_turn_start(None);
        tee.record_query_event(&QueryEvent::Text {
            query_id: Uuid::new_v4(),
            content: "I'm doing well".into(),
        });
        tee.record_query_event(&QueryEvent::Usage {
            query_id: Uuid::new_v4(),
            input_tokens: 5,
            output_tokens: 4,
            cost_usd: 0.01,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
        });
        tee.record_query_event(&QueryEvent::Completed {
            query_id: Uuid::new_v4(),
        });
        tee.close();
    }

    let mut engine =
        create_test_engine_with_state(StateManager::with_sessions_dir(temp_dir.clone()).unwrap());
    engine.session_id = session_id;
    let restored = engine.restore_session(session_id);
    assert!(matches!(restored, Ok(true)), "restore must succeed");

    assert_eq!(engine.conversation_history().len(), 2);
    let first = &engine.conversation_history()[0];
    assert_eq!(first.role, "user");
    match &first.content {
        MessageContent::Text(t) => assert_eq!(t, "Hello, how are you?"),
        other => panic!("wrong content: {other:?}"),
    }
    assert_eq!(engine.conversation.turn_count, 1);
    assert_eq!(engine.conversation.total_tokens, 9);

    // Cleanup
    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_restore_session_nonexistent() {
    let client = create_test_client();
    let tools = ToolRegistry::new();
    let permissions = PermissionManager::new();
    let state = StateManager::new();
    let config = QueryEngineConfig::default();

    let mut engine = QueryEngine::new(client, tools, permissions, state, config);
    let nonexistent_id = Uuid::new_v4();

    // Should return Ok(false) for nonexistent session
    let result = engine.restore_session(nonexistent_id);
    assert!(result.is_ok());
    assert!(!result.unwrap());
}

// ── Rewind Conversation Tests ────────────────────────────────────

#[test]
fn test_rewind_conversation_single_turn() {
    let mut engine = create_test_engine();
    engine.add_user_message("Hello".to_string());
    engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
        text: "Hi there".to_string(),
    }]);
    engine.add_user_message("How are you?".to_string());
    engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
        text: "Fine".to_string(),
    }]);
    assert_eq!(engine.conversation.messages.len(), 4);
    assert_eq!(engine.conversation.turn_count, 0); // turn_count not auto-incremented in test

    let removed = engine.rewind_conversation(1);
    assert_eq!(removed, 2);
    assert_eq!(engine.conversation.messages.len(), 2);
    assert_eq!(engine.conversation.messages[0].role, "user");
}

#[test]
fn test_rewind_conversation_multiple_turns() {
    let mut engine = create_test_engine();
    engine.add_user_message("Q1".to_string());
    engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
        text: "A1".to_string(),
    }]);
    engine.add_user_message("Q2".to_string());
    engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
        text: "A2".to_string(),
    }]);
    engine.add_user_message("Q3".to_string());
    engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
        text: "A3".to_string(),
    }]);
    assert_eq!(engine.conversation.messages.len(), 6);

    let removed = engine.rewind_conversation(2);
    assert_eq!(removed, 4);
    assert_eq!(engine.conversation.messages.len(), 2);
}

#[test]
fn test_rewind_conversation_all() {
    let mut engine = create_test_engine();
    engine.add_user_message("Q1".to_string());
    engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
        text: "A1".to_string(),
    }]);

    let removed = engine.rewind_conversation(5);
    assert_eq!(removed, 2);
    assert!(engine.conversation.messages.is_empty());
}

#[test]
fn test_rewind_conversation_empty() {
    let mut engine = create_test_engine();
    let removed = engine.rewind_conversation(1);
    assert_eq!(removed, 0);
    assert!(engine.conversation.messages.is_empty());
}

#[test]
fn test_rewind_conversation_zero() {
    let mut engine = create_test_engine();
    engine.add_user_message("Q1".to_string());
    let removed = engine.rewind_conversation(0);
    assert_eq!(removed, 0);
    assert_eq!(engine.conversation.messages.len(), 1);
}

#[test]
fn test_rewind_conversation_with_tool_messages() {
    let mut engine = create_test_engine();
    engine.add_user_message("Run tests".to_string());
    // Simulate tool result as assistant messages
    engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
        text: "I'll run the tests".to_string(),
    }]);
    engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
        text: "All tests passed".to_string(),
    }]);
    engine.add_user_message("Now commit".to_string());
    engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
        text: "Committed".to_string(),
    }]);
    // Total: 5 messages (1 user + 2 asst + 1 user + 1 asst)
    assert_eq!(engine.conversation.messages.len(), 5);

    // Rewind 1 turn removes "Now commit" + "Committed" = 2 messages
    let removed = engine.rewind_conversation(1);
    assert_eq!(removed, 2);
    assert_eq!(engine.conversation.messages.len(), 3);

    // Rewind 1 more turn removes "Run tests" + 2 assistant messages = 3
    let removed = engine.rewind_conversation(1);
    assert_eq!(removed, 3);
    assert!(engine.conversation.messages.is_empty());
}

#[test]
fn test_rewind_conversation_no_user_messages() {
    let mut engine = create_test_engine();
    // Only assistant messages, no user message to anchor a turn
    engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
        text: "Hello".to_string(),
    }]);
    engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
        text: "World".to_string(),
    }]);

    let removed = engine.rewind_conversation(1);
    assert_eq!(removed, 0);
    assert_eq!(engine.conversation.messages.len(), 2);
}
