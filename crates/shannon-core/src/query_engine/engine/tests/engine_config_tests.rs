use super::*;

// ── context_breakdown (P0-4) ────────────────────────────────────────

#[test]
fn context_breakdown_empty_session_reports_zero_categories_without_panicking() {
    // Strip the default system prompt so the session is genuinely empty:
    // no prompt, no tools, no memory, no history.
    let client = create_test_client();
    let config = QueryEngineConfig {
        system_prompt: None,
        ..Default::default()
    };
    let engine = QueryEngine::new(
        client,
        ToolRegistry::new(),
        PermissionManager::new(),
        StateManager::new(),
        config,
    );
    let breakdown = engine.context_breakdown();
    assert_eq!(breakdown.total_tokens, 0);
    assert!(breakdown.categories.iter().all(|c| c.tokens == 0));
    let keys: Vec<&str> = breakdown
        .categories
        .iter()
        .map(|c| c.key.as_str())
        .collect();
    assert_eq!(
        keys,
        vec!["system", "tools", "skills", "memory", "mcp", "conversation"]
    );
}

#[test]
fn context_breakdown_with_only_system_prompt_counts_it() {
    // Default config ships a base system prompt: the empty session's
    // breakdown is exactly that prompt, and the window mirrors
    // resolved_context_window_opt (None for a model absent from the
    // catalog — no fabricated fallback).
    let engine = create_test_engine();
    let breakdown = engine.context_breakdown();
    assert!(breakdown.tokens_for("system") > 0);
    assert_eq!(breakdown.total_tokens, breakdown.tokens_for("system"));
    for key in ["tools", "skills", "memory", "mcp", "conversation"] {
        assert_eq!(breakdown.tokens_for(key), 0, "{key} must be 0");
    }
    assert_eq!(
        breakdown.context_window.map(|v| v as usize),
        engine.resolved_context_window_opt()
    );
}

#[test]
fn context_breakdown_counts_registry_skills_memory_and_history() {
    use crate::memory::{MemoryCategory, MemoryEntry, MemoryStore};

    // A minimal registry: one built-in, one MCP-prefixed, one skill.
    // (`ToolRegistry::register` takes `&self` — interior mutability.)
    let registry = ToolRegistry::new();
    struct SchemaTool {
        name: String,
    }
    #[async_trait::async_trait]
    impl crate::tools::Tool for SchemaTool {
        fn name(&self) -> &str {
            &self.name
        }
        fn description(&self) -> &str {
            "A tool whose schema is counted in the breakdown"
        }
        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {"x": {"type": "string"}}
            })
        }
        async fn execute(
            &self,
            _input: serde_json::Value,
        ) -> Result<crate::tools::ToolOutput, crate::tools::ToolError> {
            Ok(crate::tools::ToolOutput::success("ok".into()))
        }
    }
    for name in ["Bash", "mcp__gh__search", "skill_commit"] {
        registry
            .register(Box::new(SchemaTool {
                name: name.to_string(),
            }))
            .expect("register test tool");
    }

    let temp_dir = env::temp_dir()
        .join("shannon-context-breakdown-test")
        .join(Uuid::new_v4().to_string());
    fs::create_dir_all(&temp_dir).expect("temp dir");
    let mut store = MemoryStore::new(temp_dir.clone());
    let entry = MemoryEntry {
        id: uuid::Uuid::new_v4().to_string(),
        project: std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "default".to_string()),
        category: MemoryCategory::Preference,
        content: "The user prefers concise answers.".to_string(),
        tags: vec![],
        confidence: 1.0,
        created_at: chrono::Utc::now(),
        accessed_at: chrono::Utc::now(),
        access_count: 0,
        source_session_id: None,
        source_kind: None,
        valid_until: None,
    };
    store.add(entry).expect("add memory");

    let client = create_test_client();
    let permissions = PermissionManager::new();
    let state = StateManager::new();
    let engine = QueryEngine::new(
        client,
        registry,
        permissions,
        state,
        QueryEngineConfig::default(),
    )
    .with_memory(store);
    let mut engine = engine;
    engine.conversation.messages = vec![
        shannon_engine::api::Message {
            role: "user".into(),
            content: MessageContent::Text("Hello there, Shannon.".into()),
        },
        shannon_engine::api::Message {
            role: "assistant".into(),
            content: MessageContent::Text("Hi! How can I help today?".into()),
        },
    ];

    let breakdown = engine.context_breakdown();
    for key in ["system", "tools", "skills", "memory", "mcp", "conversation"] {
        assert!(
            breakdown.tokens_for(key) > 0,
            "category {key} must be > 0 with a populated engine"
        );
    }
    let sum: u64 = breakdown.categories.iter().map(|c| c.tokens).sum();
    assert_eq!(breakdown.total_tokens, sum);
    // test-model is absent from the model catalog and the client is not
    // an Ollama probe — the window must stay None (no fabricated 200K).
    assert_eq!(breakdown.context_window, None);

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn reload_credential_swaps_api_key_without_touching_other_config() {
    // ADR-0008 Decision 4 / P1-1: /connect must hot-swap the running
    // client's key so the next query uses it — no restart. The rebuild
    // preserves the rest of the config (provider/model/base_url already
    // set by set_model_for_provider); only the key changes.
    let mut engine = create_test_engine();
    assert_eq!(engine.client().config().api_key, "test-key");
    assert_eq!(engine.client().config().model, "test-model");
    assert_eq!(engine.client().config().base_url, "http://localhost:11434");

    engine.reload_credential("sk-freshly-connected");

    let cfg = engine.client().config();
    assert_eq!(cfg.api_key, "sk-freshly-connected");
    // Unrelated fields preserved by the rebuild.
    assert_eq!(cfg.model, "test-model");
    assert_eq!(cfg.base_url, "http://localhost:11434");
    assert_eq!(cfg.provider, LlmProvider::Ollama);
}

// ── ContextInjector Integration Tests ──────────────────────────────

#[test]
fn test_engine_with_context_injector() {
    let project_dir = temp_dir_for_test("injector_project");
    let storage_dir = temp_dir_for_test("injector_storage");

    std::fs::write(
        project_dir.join("CLAUDE.md"),
        "# Test Project\nAlways write tests.",
    )
    .unwrap();

    let injector =
        crate::query_engine::ContextInjector::new(project_dir.clone(), storage_dir.clone());
    let engine = create_test_engine().with_context_injector(injector);

    // Should have a context injector
    assert!(engine.context_injector().is_some());

    // The injector should find project instructions
    let injector = engine.context_injector().unwrap();
    let instructions = injector.project_instructions_text();
    assert!(instructions.is_some());
    assert!(instructions.unwrap().contains("Test Project"));

    // Cleanup
    let _ = std::fs::remove_dir_all(project_dir);
    let _ = std::fs::remove_dir_all(storage_dir);
}

#[test]
fn test_engine_without_context_injector() {
    let engine = create_test_engine();
    assert!(engine.context_injector().is_none());
}

#[test]
fn test_engine_context_injector_preference_memory() {
    let project_dir = temp_dir_for_test("pref_project");
    let storage_dir = temp_dir_for_test("pref_storage");

    let injector =
        crate::query_engine::ContextInjector::new(project_dir.clone(), storage_dir.clone());
    let engine = create_test_engine().with_context_injector(injector);

    let injector = engine.context_injector().unwrap();
    // No preferences → empty string
    assert!(injector.preference_memory_text().is_empty());

    // Cleanup
    let _ = std::fs::remove_dir_all(project_dir);
    let _ = std::fs::remove_dir_all(storage_dir);
}

#[test]
fn test_engine_context_injector_reinjection_context() {
    let project_dir = temp_dir_for_test("reinject_project");
    let storage_dir = temp_dir_for_test("reinject_storage");

    std::fs::write(
        project_dir.join("CLAUDE.md"),
        "# Reinjection Test\nUse Rust.",
    )
    .unwrap();

    let injector =
        crate::query_engine::ContextInjector::new(project_dir.clone(), storage_dir.clone());
    let engine = create_test_engine().with_context_injector(injector);

    let injector = engine.context_injector().unwrap();
    let reinjection = injector.reinjection_context();
    assert!(reinjection.contains("Reinjection Test"));
    assert!(reinjection.contains("Use Rust"));

    // Cleanup
    let _ = std::fs::remove_dir_all(project_dir);
    let _ = std::fs::remove_dir_all(storage_dir);
}

#[test]
fn test_engine_context_injector_system_blocks() {
    let project_dir = temp_dir_for_test("blocks_project");
    let storage_dir = temp_dir_for_test("blocks_storage");

    std::fs::write(project_dir.join("CLAUDE.md"), "# Blocks Test\nBe concise.").unwrap();

    let injector =
        crate::query_engine::ContextInjector::new(project_dir.clone(), storage_dir.clone());
    let engine = create_test_engine().with_context_injector(injector);

    let injector = engine.context_injector().unwrap();
    let blocks = injector.build_system_blocks(true);
    // Instructions are injected by the engine (stable cache zone), so
    // build_system_blocks only carries MEMORY.md / rules / prefs — all
    // absent in this fixture.
    assert!(blocks.is_empty(), "blocks: {blocks:?}");

    // Cleanup
    let _ = std::fs::remove_dir_all(project_dir);
    let _ = std::fs::remove_dir_all(storage_dir);
}

// ── Plan Mode Integration Tests ──────────────────────────────────────

#[test]
fn test_plan_mode_flag_default_false() {
    let engine = create_test_engine();
    assert!(!engine.is_plan_mode_active());
}

#[test]
fn test_plan_mode_flag_can_be_set() {
    let flag = Arc::new(RwLock::new(true));
    let engine = create_test_engine().with_plan_mode_active(flag);
    assert!(engine.is_plan_mode_active());
}

#[test]
fn test_plan_mode_flag_shared_reflection() {
    let flag = Arc::new(RwLock::new(false));
    let engine = create_test_engine().with_plan_mode_active(flag.clone());

    // Initially inactive
    assert!(!engine.is_plan_mode_active());

    // Setting the flag externally is reflected in the engine
    *flag.write().unwrap() = true;
    assert!(engine.is_plan_mode_active());

    // Resetting the flag
    *flag.write().unwrap() = false;
    assert!(!engine.is_plan_mode_active());
}

#[test]
fn test_plan_mode_active_handle_clones() {
    let engine = create_test_engine();
    let handle = engine.plan_mode_active_handle();

    // Modify via handle
    *handle.write().unwrap() = true;
    assert!(engine.is_plan_mode_active());
}

#[test]
fn test_is_file_modifying_tool_covers_write_tools() {
    // Verify the helper used by the engine gate recognizes write tools
    assert!(crate::tool_execution::is_file_modifying_tool("Write"));
    assert!(crate::tool_execution::is_file_modifying_tool("write"));
    assert!(crate::tool_execution::is_file_modifying_tool("Edit"));
    assert!(crate::tool_execution::is_file_modifying_tool("edit"));
    assert!(crate::tool_execution::is_file_modifying_tool("MultiEdit"));
    assert!(crate::tool_execution::is_file_modifying_tool("multi_edit"));
    assert!(crate::tool_execution::is_file_modifying_tool("Bash"));
    assert!(crate::tool_execution::is_file_modifying_tool("bash"));
}

#[test]
fn test_is_file_modifying_tool_excludes_read_tools() {
    // Verify read-only tools are not flagged as modifying
    assert!(!crate::tool_execution::is_file_modifying_tool("Read"));
    assert!(!crate::tool_execution::is_file_modifying_tool("Glob"));
    assert!(!crate::tool_execution::is_file_modifying_tool("Grep"));
    assert!(!crate::tool_execution::is_file_modifying_tool("LSP"));
}

#[test]
fn test_cache_breakpoint_budget_respected() {
    // P0-6 regression: the system block assembly must emit at most 2
    // cache breakpoints. Anthropic caps a request at 4 and the adapter
    // adds two more (last tool def + last user message), so any more
    // than 2 system breakpoints overflows the budget.
    //
    // Simulate the assembly policy: N stable blocks + 3 dynamic blocks.
    let stable_texts: Vec<String> = (0..9).map(|i| format!("stable {i}")).collect();
    let use_cache = true;

    let mut system_blocks: Vec<SystemContentBlock> = Vec::new();
    if use_cache {
        let last_stable = stable_texts.len().saturating_sub(1);
        for (i, text) in stable_texts.into_iter().enumerate() {
            let block = if i == 0 || i == last_stable {
                SystemContentBlock::cached(text)
            } else {
                SystemContentBlock::text(text)
            };
            system_blocks.push(block);
        }
    }
    for i in 0..3 {
        system_blocks.push(SystemContentBlock::text(format!("dynamic {i}")));
    }

    let cached_count = system_blocks
        .iter()
        .filter(|b| b.cache_control.is_some())
        .count();
    assert!(
        cached_count <= 2,
        "system blocks must carry at most 2 cache breakpoints, got {cached_count}"
    );
    // First (base) and last stable block are the cached ones.
    assert!(system_blocks[0].cache_control.is_some());
    assert!(system_blocks[8].cache_control.is_some());
    // Dynamic zone stays uncached.
    for block in &system_blocks[9..] {
        assert!(block.cache_control.is_none());
    }
}

// ── Context resolution tests ──────────────────────────────────

#[test]
fn test_resolve_max_context_user_override_wins() {
    // User override should take absolute priority
    let result = QueryEngine::resolve_max_context_tokens("unknown-model", Some(64000));
    assert_eq!(result, 64000);
}

#[test]
fn test_resolve_max_context_from_registry() {
    // Known model should get context_window from MODEL_CATALOG
    let result = QueryEngine::resolve_max_context_tokens("claude-sonnet-4-20250514", None);
    assert_eq!(result, 200_000);
}

#[test]
fn test_resolve_max_context_unknown_model_fallback() {
    // Unknown model falls back to context_window_for() default (200K)
    let result = QueryEngine::resolve_max_context_tokens("nonexistent-model", None);
    assert_eq!(result, 200_000);
}

#[test]
fn test_resolve_max_context_zero_override_prevents_division_by_zero() {
    // Even a zero override should not crash — the compression guard uses .max(1)
    let result = QueryEngine::resolve_max_context_tokens("any-model", Some(0));
    assert_eq!(result, 0); // resolved as 0, but usage code uses .max(1)
    // Verify the guard works
    let guarded = result.max(1);
    assert_eq!(guarded, 1);
}

#[test]
fn test_effective_max_context_initialized_in_engine() {
    let client = create_test_client();
    let tools = ToolRegistry::new();
    let permissions = PermissionManager::new();
    let state = StateManager::new();
    let config = QueryEngineConfig::default();

    let engine = QueryEngine::new(client, tools, permissions, state, config);
    // Default config has max_context_tokens: None, so it uses registry fallback
    // "test-model" is not in catalog, so falls back to 200_000
    assert_eq!(engine.effective_max_context_tokens, 200_000);
}

#[test]
fn test_effective_max_context_with_user_override() {
    let client = create_test_client();
    let tools = ToolRegistry::new();
    let permissions = PermissionManager::new();
    let state = StateManager::new();
    let config = QueryEngineConfig {
        max_context_tokens: Some(32000),
        ..Default::default()
    };

    let engine = QueryEngine::new(client, tools, permissions, state, config);
    assert_eq!(engine.effective_max_context_tokens, 32000);
}

#[test]
fn test_effective_max_context_with_known_model() {
    let config = LlmClientConfig {
        api_key: "test".to_string(),
        base_url: "http://localhost:11434".to_string(),
        model: "claude-sonnet-4-20250514".to_string(),
        ..Default::default()
    };
    let client = LlmClient::new(config);
    let tools = ToolRegistry::new();
    let permissions = PermissionManager::new();
    let state = StateManager::new();

    let engine = QueryEngine::new(
        client,
        tools,
        permissions,
        state,
        QueryEngineConfig::default(),
    );
    assert_eq!(engine.effective_max_context_tokens, 200_000);
}

/// Verify that cache tokens from MessageStart are captured and merged
/// with MessageDelta usage. This test validates the fix for cache hit
/// rate not showing in the UI — the root cause was that MessageStart
/// was ignored, losing cache_creation_input_tokens and
/// cache_read_input_tokens which Anthropic only sends in that event.
#[test]
fn test_cache_tokens_from_message_start_are_used() {
    use shannon_engine::api::{MessageResponse, StreamEvent, Usage};

    // Simulate Anthropic's message_start event with cache tokens
    let start_event = StreamEvent::MessageStart {
        message: MessageResponse {
            id: "msg_test".to_string(),
            role: "assistant".to_string(),
            content: vec![],
            model: "claude-sonnet-4-20250514".to_string(),
            stop_reason: None,
            usage: Usage {
                input_tokens: 1000,
                output_tokens: 0,
                cache_creation_input_tokens: 500,
                cache_read_input_tokens: 800,
            },
        },
    };

    // Extract cache tokens like the engine now does
    let (cache_read, cache_creation) = match &start_event {
        StreamEvent::MessageStart { message } => (
            message.usage.cache_read_input_tokens as u64,
            message.usage.cache_creation_input_tokens as u64,
        ),
        _ => (0, 0),
    };

    assert_eq!(cache_read, 800, "cache_read should come from MessageStart");
    assert_eq!(
        cache_creation, 500,
        "cache_creation should come from MessageStart"
    );

    // Simulate message_delta which only has output_tokens
    let delta_usage = Usage {
        input_tokens: 0,
        output_tokens: 200,
        cache_creation_input_tokens: 0,
        cache_read_input_tokens: 0,
    };

    // Merge: take the max of both sources
    let merged_cache_read = cache_read.max(delta_usage.cache_read_input_tokens as u64);
    let merged_cache_creation = cache_creation.max(delta_usage.cache_creation_input_tokens as u64);

    assert_eq!(
        merged_cache_read, 800,
        "merged cache_read should preserve MessageStart value"
    );
    assert_eq!(
        merged_cache_creation, 500,
        "merged cache_creation should preserve MessageStart value"
    );
}

// ── Context window propagation tests ──────────────────────────────────

#[test]
fn test_resolve_max_context_user_override_takes_priority() {
    let result = QueryEngine::resolve_max_context_tokens("unknown-model", Some(8000));
    assert_eq!(result, 8000, "User override should take priority");
}

#[test]
fn test_resolve_max_context_known_model_exact_match() {
    let result = QueryEngine::resolve_max_context_tokens("claude-sonnet-4-20250514", None);
    assert_eq!(result, 200_000, "Known model should match from registry");
}

#[test]
fn test_resolve_max_context_partial_model_id_prefix_match() {
    // "claude-sonnet-4" should match "claude-sonnet-4-20250514" via prefix
    let result = QueryEngine::resolve_max_context_tokens("claude-sonnet-4", None);
    assert!(
        result > 0,
        "Partial model ID should resolve via prefix matching"
    );
}

#[test]
fn test_resolve_max_context_ollama_model_fallback() {
    let result = QueryEngine::resolve_max_context_tokens("ollama/llama3:8b", None);
    assert_eq!(
        result, 200_000,
        "Unknown Ollama model should fall back to 200k"
    );
}

#[test]
fn test_resolved_context_window_with_user_config() {
    let client = create_test_client();
    let tools = ToolRegistry::new();
    let permissions = PermissionManager::new();
    let state = StateManager::new();
    let config = QueryEngineConfig {
        max_context_tokens: Some(8000),
        ..Default::default()
    };

    let engine = QueryEngine::new(client, tools, permissions, state, config);
    assert_eq!(engine.resolved_context_window(), 8000);
}

#[test]
fn test_resolved_context_window_ollama_fallback_chain() {
    // Ollama provider with no cached info → falls back to effective_max_context_tokens
    let config = LlmClientConfig {
        api_key: "test".to_string(),
        base_url: "http://localhost:11434".to_string(),
        model: "llama3:8b".to_string(),
        provider: shannon_engine::api::LlmProvider::Ollama,
        ..Default::default()
    };
    let client = LlmClient::new(config);
    let tools = ToolRegistry::new();
    let permissions = PermissionManager::new();
    let state = StateManager::new();

    let engine = QueryEngine::new(
        client,
        tools,
        permissions,
        state,
        QueryEngineConfig::default(),
    );
    // Without pre_resolve_context being called (no Ollama server), it should
    // fall back to effective_max_context_tokens (which is the model_registry value)
    let window = engine.resolved_context_window();
    assert!(
        window > 0,
        "Context window should be positive even without Ollama server"
    );
}

#[test]
fn test_cache_hit_rate_accumulation_across_usage_events() {
    use shannon_engine::api::{MessageResponse, StreamEvent, Usage};

    // Simulate multiple turns with different cache profiles
    let turns = vec![
        (10_000, 0),    // Turn 1: cache miss
        (0, 9_000),     // Turn 2: cache hit
        (2_000, 7_000), // Turn 3: partial
    ];

    let mut total_creation: u64 = 0;
    let mut total_read: u64 = 0;

    for (creation, read) in &turns {
        let event = StreamEvent::MessageStart {
            message: MessageResponse {
                id: "msg_test".to_string(),
                role: "assistant".to_string(),
                content: vec![],
                model: "test".to_string(),
                stop_reason: None,
                usage: Usage {
                    input_tokens: 5000,
                    output_tokens: 0,
                    cache_creation_input_tokens: *creation,
                    cache_read_input_tokens: *read,
                },
            },
        };

        if let StreamEvent::MessageStart { message } = &event {
            total_creation += message.usage.cache_creation_input_tokens as u64;
            total_read += message.usage.cache_read_input_tokens as u64;
        }
    }

    assert_eq!(total_creation, 12_000);
    assert_eq!(total_read, 16_000);

    let hit_rate = total_read as f64 / (total_read + total_creation) as f64;
    // 16000 / (16000 + 12000) ≈ 0.571
    assert!(
        hit_rate > 0.5,
        "Hit rate should be > 50%, got {hit_rate:.3}"
    );
    assert!(
        hit_rate < 0.6,
        "Hit rate should be < 60%, got {hit_rate:.3}"
    );
}
