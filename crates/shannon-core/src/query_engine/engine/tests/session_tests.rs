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
        thinking_type: None,
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
    // RAII temp root: removed automatically when the guard drops.
    let temp_root = tempfile::tempdir().unwrap();
    let temp_dir = temp_root.path().to_path_buf();

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
            outcome: Default::default(),
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

// ── Rewind turn-opener semantics (tool results / synthetic reminders) ──

use shannon_types::session_event::{
    AssistantChunkPayload, TurnEndPayload, TurnStartPayload, UserMessagePayload,
};

/// A user-role tool-result message, exactly as the agent loop drains them
/// into `conversation.messages` (assistant `tool_use` → user `tool_result`).
fn user_tool_result(tool_use_id: &str) -> Message {
    Message {
        role: "user".to_string(),
        content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
            tool_use_id: tool_use_id.to_string(),
            content: None,
            is_error: Some(false),
        }]),
    }
}

/// An assistant message carrying a tool call.
fn assistant_tool_use(id: &str) -> Message {
    Message {
        role: "assistant".to_string(),
        content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
            id: id.to_string(),
            name: "Bash".to_string(),
            input: serde_json::json!({ "command": "ls" }),
        }]),
    }
}

#[test]
fn rewind_conversation_skips_tool_results_when_finding_turn_openers() {
    let mut engine = create_test_engine();
    engine.add_user_message("Run tests".to_string());
    engine.add_assistant_message(vec![]);
    engine.conversation.messages.push(assistant_tool_use("u1"));
    engine.conversation.messages.push(user_tool_result("u1"));
    engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
        text: "All tests passed".to_string(),
    }]);
    engine.add_user_message("Now commit".to_string());
    engine.conversation.messages.push(assistant_tool_use("u2"));
    engine.conversation.messages.push(user_tool_result("u2"));
    assert_eq!(engine.conversation.messages.len(), 8);

    // The old role=="user" scan treated the trailing tool_result as a turn
    // opener: it cut right after the dangling assistant tool_use and
    // decremented turn_count twice. The rewind must remove the WHOLE final
    // turn ("Now commit" + tool_use + tool_result).
    let removed = engine.rewind_conversation(1);
    assert_eq!(removed, 3);
    assert_eq!(engine.conversation.messages.len(), 5);
    // The kept prefix never ends in a tool_use whose result was removed.
    assert!(!is_dangling_tool_use(
        engine.conversation.messages.last().expect("non-empty")
    ));
    assert_eq!(engine.conversation_turn_count(), 1);
}

#[test]
fn rewind_conversation_ignores_synthetic_reminders_as_turn_openers() {
    let mut engine = create_test_engine();
    engine.add_user_message("Refactor the parser".to_string());
    engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
        text: "Sure".to_string(),
    }]);
    // P-M token-budget reminder: pushed as user-role TEXT by the agent loop.
    let reminder = "[Token budget at 62%] ~62% of the context window used \
                    (120000/190000 tokens). Focus on completing the task.";
    engine.conversation.messages.push(Message {
        role: "user".to_string(),
        content: MessageContent::Text(reminder.to_string()),
    });
    engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
        text: "Wrapping up".to_string(),
    }]);
    assert_eq!(engine.conversation.messages.len(), 4);

    // The reminder must not count as a turn opener: rewinding 1 turn removes
    // the whole exchange, not just the reminder's tail.
    let removed = engine.rewind_conversation(1);
    assert_eq!(removed, 4);
    assert!(engine.conversation.messages.is_empty());
}

#[test]
fn synthetic_reminder_pins_match_producer_prompts() {
    // Every deterministic synthetic prompt the agent loop pushes as a
    // user-role message is recognized; a real prompt never matches.
    assert!(is_synthetic_reminder(
        "[Token budget at 80%] ~80% of the context window used"
    ));
    assert!(is_synthetic_reminder(
        "Context is large (120000/100000 tokens). Prefer targeted reads"
    ));
    assert!(is_synthetic_reminder(TRUNCATION_CONTINUATION_PROMPT));
    assert!(is_synthetic_reminder(THINK_ONLY_NUDGE_PROMPT));
    assert!(is_synthetic_reminder(WRAP_UP_NUDGE_PROMPT));
    assert!(is_synthetic_reminder(
        recovery::TURN_CONTINUATION_NUDGE_PROMPT
    ));
    assert!(!is_synthetic_reminder("Fix the flaky test in CI"));
    // Only turn openers consult the reminder list, and a real prompt is an
    // opener even when it merely starts like a reminder would.
    let prompt = Message {
        role: "user".to_string(),
        content: MessageContent::Text("Token budget report please".to_string()),
    };
    assert!(is_turn_opener(&prompt));
}

/// T15b: the runtime `user_notices` messages (denial soft-limit warning and
/// auto-test outcomes) travel as user-role TEXT with variable content, but
/// each producer's text is pinned here against `agent_loop.rs` /
/// `auto_test.rs`. If a producer rewording breaks one of these pins, /rewind
/// would treat the notice as a turn opener again — fix the pin AND the
/// producer together.
#[test]
fn synthetic_reminder_pins_cover_runtime_user_notices() {
    use crate::auto_test::TestOutcome;

    // Denial soft-limit warning (agent_loop.rs): fixed sentence around the
    // variable denial count.
    assert!(is_synthetic_reminder(
        "The user has denied 2 consecutive tool calls. Stop retrying the same or \
         similar operations. Ask the user for clarification or try a completely \
         different approach."
    ));

    // Auto-test outcomes (auto_test::TestOutcome::describe) — every variant.
    assert!(is_synthetic_reminder("All tests passed."));
    assert!(is_synthetic_reminder(
        &TestOutcome::Failed {
            summary: "test result: FAILED. 3 passed; 1 failed".to_string(),
        }
        .describe()
    ));
    assert!(is_synthetic_reminder(&TestOutcome::TimedOut.describe()));
    assert!(is_synthetic_reminder(
        &TestOutcome::SpawnError("cargo: not found".to_string()).describe()
    ));

    // Joined batches drain as ONE message (join("\n\n")): a batch is
    // recognized through its FIRST notice, including when that notice is an
    // exact-match one.
    assert!(is_synthetic_reminder(
        "All tests passed.\n\nThe user has denied 3 consecutive tool calls. \
         Stop retrying the same or similar operations. Ask the user for \
         clarification or try a completely different approach."
    ));
    assert!(is_synthetic_reminder(
        "The user has denied 1 consecutive tool calls. Stop retrying the same or \
         similar operations. Ask the user for clarification or try a completely \
         different approach.\n\nAll tests passed."
    ));

    // A real prompt is never mistaken for a notice.
    assert!(!is_synthetic_reminder(
        "All tests passed; now summarize what changed"
    ));
    assert!(!is_synthetic_reminder("Run the test suite"));
}

/// The turn-N checkpoint reminder (SHANNON_TURN_CHECKPOINT, agent_loop.rs
/// P-B block) is pushed as user-role text OUTSIDE the user_notices drain,
/// so it needs its own pin. Its shape is "[Turn {n} reminder] You have used
/// {n} of your turn budget …" around a variable turn number.
#[test]
fn synthetic_reminder_pins_turn_checkpoint_reminder() {
    assert!(is_synthetic_reminder(
        "[Turn 12 reminder] You have used 12 of your turn budget and have NOT yet \
         called Edit or Write. STOP exploring and commit a fix now — a wrong or \
         partial fix is better than an empty patch. The official harness will \
         judge correctness; you do not need to verify locally."
    ));
    // The generic "[Turn " prefix alone is not enough — a real user prompt
    // that happens to open with bracketed turn talk must stay a turn opener.
    assert!(!is_synthetic_reminder(
        "[Turn 3 of my plan] Please continue with the refactor"
    ));
}

/// T15b end-to-end: a drained user notice between turns must not open a
/// rewind turn — rewinding past it removes the whole exchange including the
/// notice, exactly like the other synthetic reminders.
#[test]
fn rewind_conversation_ignores_user_notices_as_turn_openers() {
    let mut engine = create_test_engine();
    engine.add_user_message("Fix the failing test".to_string());
    engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
        text: "Working on it".to_string(),
    }]);
    // Auto-test failure notice: drained as a user-role TEXT message after
    // the tool results (agent_loop.rs user_notices).
    engine.conversation.messages.push(Message {
        role: "user".to_string(),
        content: MessageContent::Text(
            "Tests failed:\n```\ntest result: FAILED. 0 passed; 1 failed\n```".to_string(),
        ),
    });
    engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
        text: "Fixed it".to_string(),
    }]);
    assert_eq!(engine.conversation.messages.len(), 4);

    // The notice must not count as a turn opener: rewinding 1 turn removes
    // the whole exchange (prompt, answer, notice, answer).
    let removed = engine.rewind_conversation(1);
    assert_eq!(removed, 4);
    assert!(engine.conversation.messages.is_empty());
    assert_eq!(engine.conversation_turn_count(), 0);
}

#[test]
fn rewind_never_leaves_a_dangling_assistant_tool_use() {
    // Interrupted turn: user A → assistant tool_use (never answered), then a
    // fresh user prompt. Cutting at the new prompt would keep the orphaned
    // tool_use in the kept prefix.
    let mut engine = create_test_engine();
    engine.add_user_message("A".to_string());
    engine.conversation.messages.push(assistant_tool_use("u1"));
    engine.add_user_message("B".to_string());
    engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
        text: "ok".to_string(),
    }]);

    let removed = engine.rewind_conversation(1);
    // The cut moved back past the dangling tool_use: only ["A"] survives.
    assert_eq!(removed, 3);
    assert_eq!(engine.conversation.messages.len(), 1);
    assert!(!is_dangling_tool_use(
        engine.conversation.messages.last().expect("non-empty")
    ));
}

// ── Rewind ↔ L0 log alignment (surviving TURN count, not message count) ──

#[test]
fn rewind_truncates_l0_log_to_surviving_turns() {
    let temp_root = tempfile::tempdir().unwrap();
    let state = Arc::new(StateManager::with_sessions_dir(temp_root.path().to_path_buf()).unwrap());
    let session_id = Uuid::new_v4();

    // Seed a 5-turn framed log through the real writer.
    {
        let mut w = crate::session_log::SessionLogWriter::open_layout(
            state.sessions_dir(),
            &session_id.to_string(),
        )
        .expect("open writer");
        for turn in 0..5u64 {
            w.record(shannon_types::session_event::SessionEventBody::TurnStart(
                TurnStartPayload { query_id: None },
            ));
            w.record(shannon_types::session_event::SessionEventBody::UserMessage(
                UserMessagePayload {
                    source: UserMessagePayload::SOURCE_USER.into(),
                    content: format!("question {turn}"),
                    attachment_count: 0,
                },
            ));
            w.record(
                shannon_types::session_event::SessionEventBody::AssistantChunk(
                    AssistantChunkPayload {
                        delta: format!("answer {turn}"),
                        thinking: false,
                    },
                ),
            );
            w.record(shannon_types::session_event::SessionEventBody::TurnEnd(
                TurnEndPayload {
                    llm_steps: None,
                    reason: TurnEndPayload::REASON_COMPLETED.into(),
                    usage: None,
                    error: None,
                },
            ));
        }
        w.close().expect("close writer");
    }

    // Mirror the same 5 turns in engine memory.
    let mut engine = create_test_engine_with_state(
        StateManager::with_sessions_dir(temp_root.path().to_path_buf()).unwrap(),
    );
    engine.session_id = session_id;
    for turn in 0..5 {
        engine.add_user_message(format!("question {turn}"));
        engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
            text: format!("answer {turn}"),
        }]);
    }

    // /rewind 1: memory keeps 4 turns; the L0 log must keep exactly 4 turns.
    // The old code passed the removed-MESSAGE count (2) as keep_turns, which
    // destroyed 3 more turns from the authoritative log.
    engine.rewind_conversation(1);
    let keep_turns = engine.conversation_turn_count();
    assert_eq!(keep_turns, 4);

    let store = crate::session_log::SessionStore::new(state.sessions_dir().to_path_buf());
    store
        .truncate_to_turn(&session_id, keep_turns)
        .unwrap()
        .expect("log exists");

    let loaded = store.load(&session_id).unwrap().expect("session survives");
    assert_eq!(loaded.metadata.turn_count, 4);
    assert_eq!(loaded.messages.len(), 8); // 4 × (user, assistant)
    // Collect every text fragment (plain user text and assistant blocks —
    // the projection renders chunk text as `Blocks([Text])`).
    let texts: Vec<String> = loaded
        .messages
        .iter()
        .map(|m| match &m.content {
            MessageContent::Text(t) => t.clone(),
            MessageContent::Blocks(blocks) => blocks
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(""),
        })
        .collect();
    assert!(texts.contains(&"question 3".to_string()));
    assert!(texts.contains(&"answer 0".to_string()));
    assert!(!texts.iter().any(|t| t.contains("question 4")));
    assert!(!texts.iter().any(|t| t.contains("answer 4")));
}

#[test]
fn rewind_log_alignment_with_tool_heavy_last_turn() {
    let temp_root = tempfile::tempdir().unwrap();
    let state = Arc::new(StateManager::with_sessions_dir(temp_root.path().to_path_buf()).unwrap());
    let session_id = Uuid::new_v4();

    // 6 turns in the log: turns 0..=4 plain, turn 5 tool-heavy
    // (user → tool_use → tool_result → assistant).
    {
        use shannon_types::session_event::{SessionEventBody, ToolCallPayload, ToolResultPayload};
        let mut w = crate::session_log::SessionLogWriter::open_layout(
            state.sessions_dir(),
            &session_id.to_string(),
        )
        .expect("open writer");
        for turn in 0..6u64 {
            w.record(SessionEventBody::TurnStart(TurnStartPayload {
                query_id: None,
            }));
            w.record(SessionEventBody::UserMessage(UserMessagePayload {
                source: UserMessagePayload::SOURCE_USER.into(),
                content: format!("question {turn}"),
                attachment_count: 0,
            }));
            if turn == 5 {
                w.record(SessionEventBody::ToolCall(ToolCallPayload {
                    tool_use_id: "u5".into(),
                    tool_name: "Bash".into(),
                    arguments: r#"{"command":"ls"}"#.into(),
                }));
                w.record(SessionEventBody::ToolResult(ToolResultPayload {
                    tool_use_id: "u5".into(),
                    tool_name: "Bash".into(),
                    output: "out".into(),
                    is_error: false,
                    duration_ms: Some(1),
                    meta: serde_json::Value::Null,
                }));
            }
            w.record(SessionEventBody::AssistantChunk(AssistantChunkPayload {
                delta: format!("answer {turn}"),
                thinking: false,
            }));
            w.record(SessionEventBody::TurnEnd(TurnEndPayload {
                llm_steps: None,
                reason: TurnEndPayload::REASON_COMPLETED.into(),
                usage: None,
                error: None,
            }));
        }
        w.close().expect("close writer");
    }

    // Mirror in memory: 5 plain turns + the tool-heavy turn 5.
    let mut engine = create_test_engine_with_state(
        StateManager::with_sessions_dir(temp_root.path().to_path_buf()).unwrap(),
    );
    engine.session_id = session_id;
    for turn in 0..5 {
        engine.add_user_message(format!("question {turn}"));
        engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
            text: format!("answer {turn}"),
        }]);
    }
    engine.add_user_message("question 5".to_string());
    engine.conversation.messages.push(assistant_tool_use("u5"));
    engine.conversation.messages.push(user_tool_result("u5"));
    engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
        text: "answer 5".to_string(),
    }]);

    // /rewind 1 removes exactly the tool-heavy turn (4 messages), and the
    // log is truncated to the 5 surviving turns — not to 4 (the removed
    // message count, the old bug) and not left at 6.
    let removed = engine.rewind_conversation(1);
    assert_eq!(removed, 4);
    let keep_turns = engine.conversation_turn_count();
    assert_eq!(keep_turns, 5);

    let store = crate::session_log::SessionStore::new(state.sessions_dir().to_path_buf());
    store
        .truncate_to_turn(&session_id, keep_turns)
        .unwrap()
        .expect("log exists");

    let loaded = store.load(&session_id).unwrap().expect("session survives");
    assert_eq!(loaded.metadata.turn_count, 5);
    assert_eq!(loaded.messages.len(), 10); // 5 × (user, assistant)
    let has_question_5 = loaded
        .messages
        .iter()
        .any(|m| matches!(&m.content, MessageContent::Text(t) if t.contains("question 5")));
    assert!(!has_question_5, "rewound turn must not survive in the log");
}
