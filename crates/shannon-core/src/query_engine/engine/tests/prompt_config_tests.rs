use super::*;

// ── System prompt tests ─────────────────────────────────────────

#[test]
fn test_system_prompt_default_is_set() {
    // Default config includes a built-in system prompt
    let engine = create_test_engine();
    let prompt = engine
        .system_prompt()
        .expect("default should have a system prompt");
    assert!(prompt.contains("Shannon"));
}

#[test]
fn test_system_prompt_overrides_default() {
    let engine = create_test_engine().with_system_prompt("You are a code reviewer.".to_string());
    assert_eq!(
        engine.system_prompt(),
        Some("You are a code reviewer.".to_string())
    );
}

#[test]
fn test_append_system_prompt_adds_to_existing() {
    let mut engine = create_test_engine();
    let original = engine.system_prompt().unwrap();
    engine.append_system_prompt("Always write tests.");
    let appended = engine.system_prompt().unwrap();
    assert!(appended.starts_with(&original));
    assert!(appended.contains("Always write tests."));
}

#[test]
fn test_append_system_prompt_accumulates() {
    let mut engine = create_test_engine().with_system_prompt("Base prompt.".to_string());
    engine.append_system_prompt("Section A.");
    engine.append_system_prompt("Section B.");
    let prompt = engine.system_prompt().unwrap();
    assert!(prompt.starts_with("Base prompt."));
    assert!(prompt.contains("Section A."));
    assert!(prompt.contains("Section B."));
}

#[test]
fn test_system_prompt_default_has_no_cwd() {
    let engine = create_test_engine();
    let prompt = engine.system_prompt().unwrap();
    assert!(
        !prompt.contains("Working directory"),
        "Default system prompt should NOT contain CWD (it is injected at query time): {prompt}"
    );
}

#[test]
fn test_cwd_injection_appends_to_system_prompt() {
    let engine = create_test_engine();
    let mut prompt = engine.system_prompt().unwrap();

    // Simulate the CWD injection that process_query does
    if let Ok(cwd) = std::env::current_dir() {
        prompt.push_str(&format!(
            "\n\n## Environment\n\nWorking directory: {}",
            cwd.display()
        ));
    }

    assert!(
        prompt.contains("Working directory"),
        "After CWD injection, prompt should contain 'Working directory'"
    );
    let cwd = std::env::current_dir().unwrap();
    assert!(
        prompt.contains(&*cwd.to_string_lossy()),
        "After CWD injection, prompt should contain the actual CWD path"
    );
}

// ── Memory store tests ──────────────────────────────────────────

#[test]
fn test_memory_default_is_none() {
    let engine = create_test_engine();
    assert!(engine.memory().is_none());
}

#[test]
fn test_memory_with_store_returns_some() {
    let temp_dir = env::temp_dir()
        .join("shannon-memory-test")
        .join(Uuid::new_v4().to_string());
    fs::create_dir_all(&temp_dir).unwrap();

    let store = MemoryStore::new(temp_dir.clone());
    let engine = create_test_engine().with_memory(store);
    assert!(engine.memory().is_some());

    // Cleanup
    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_with_memory_arc_shares_one_instance_across_engines() {
    use crate::memory::{MemoryCategory, MemoryEntry};
    // P2-4b seam: desktop hosts hold one shared handle and thread clones
    // of it into every engine. Both engines must observe the same store —
    // a write through one handle is visible (and injectable) through the
    // other's.
    let temp_dir = env::temp_dir()
        .join("shannon-memory-arc-test")
        .join(Uuid::new_v4().to_string());
    fs::create_dir_all(&temp_dir).unwrap();
    let project = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "default".to_string());

    let shared = Arc::new(std::sync::RwLock::new(MemoryStore::new(temp_dir.clone())));
    let engine_a = create_test_engine().with_memory_arc(shared.clone());
    let engine_b = create_test_engine().with_memory_arc(shared.clone());

    let handle_a = engine_a.memory().cloned().expect("engine a has memory");
    let handle_b = engine_b.memory().expect("engine b has memory");
    assert!(
        Arc::ptr_eq(&handle_a, handle_b),
        "both engines must reference the same Arc instance"
    );

    {
        let mut store = handle_a.write().unwrap_or_else(|e| e.into_inner());
        let mut entry = MemoryEntry::new(&project, MemoryCategory::Preference, "shared fact");
        entry.source_kind = Some(MemoryEntry::SOURCE_AUTO_EXTRACT.to_string());
        store.add(entry).expect("add through engine a's handle");
    }

    let store_b = handle_b.read().unwrap_or_else(|e| e.into_inner());
    let injected = store_b
        .format_for_injection(&project)
        .expect("injection text for the cwd project");
    assert!(
        injected.contains("shared fact"),
        "a write through engine a must be injectable from engine b"
    );

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_with_memory_arc_feeds_the_injection_read_path() {
    use crate::memory::{MemoryCategory, MemoryEntry};
    // The same read (`format_for_injection` over the cwd project key)
    // process_query and context_breakdown perform — must produce text
    // once the shared store carries entries for that key.
    let temp_dir = env::temp_dir()
        .join("shannon-memory-arc-inject-test")
        .join(Uuid::new_v4().to_string());
    fs::create_dir_all(&temp_dir).unwrap();
    let project = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "default".to_string());

    let shared = Arc::new(std::sync::RwLock::new(MemoryStore::new(temp_dir.clone())));
    {
        let mut store = shared.write().unwrap_or_else(|e| e.into_inner());
        store
            .add(MemoryEntry::new(
                &project,
                MemoryCategory::Context,
                "engine injected this",
            ))
            .expect("seed memory");
    }

    let engine = create_test_engine().with_memory_arc(shared);
    let text = engine
        .memory()
        .unwrap()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .format_for_injection(&project)
        .expect("memory text");
    assert!(text.contains("engine injected this"));

    // And the context breakdown's `memory` category (which snapshots the
    // identical injection text) is non-zero with the store attached.
    let breakdown = engine.context_breakdown();
    assert!(
        breakdown.tokens_for("memory") > 0,
        "memory category must be non-zero with a populated shared store"
    );

    let _ = fs::remove_dir_all(temp_dir);
}

// ── set_model_for_provider tests ────────────────────────────────

#[test]
fn test_set_model_for_provider_updates_model_and_provider() {
    let mut engine = create_test_engine();

    // Initial state from create_test_client: model=test-model, provider=Ollama
    assert_eq!(engine.client.model(), "test-model");
    assert_eq!(*engine.client.provider(), LlmProvider::Ollama);

    // Switch to Anthropic with a different model
    engine.set_model_for_provider(
        "claude-sonnet-4-20250514".to_string(),
        LlmProvider::Anthropic,
    );

    assert_eq!(engine.client.model(), "claude-sonnet-4-20250514");
    assert_eq!(*engine.client.provider(), LlmProvider::Anthropic);
}

#[test]
fn test_set_model_for_provider_updates_cost_tracker_model_name() {
    let mut engine = create_test_engine();

    engine.set_model_for_provider("claude-opus-4-20250514".to_string(), LlmProvider::Anthropic);

    let tracker = engine
        .cost_tracker
        .read()
        .unwrap_or_else(|e| e.into_inner());
    assert_eq!(tracker.model_name, "claude-opus-4-20250514");
}

#[test]
fn test_set_model_for_provider_updates_effective_context_window() {
    let mut engine = create_test_engine();
    // Default test-model -> 200K
    assert_eq!(engine.effective_max_context_tokens, 200_000);

    // Switch to claude-sonnet-4 which also has 200K — but verify the field was recalculated
    engine.set_model_for_provider(
        "claude-sonnet-4-20250514".to_string(),
        LlmProvider::Anthropic,
    );
    assert_eq!(engine.effective_max_context_tokens, 200_000);
}

// ── Configuration setter tests ──────────────────────────────────

#[test]
fn test_set_effort_level() {
    let mut engine = create_test_engine();
    assert_eq!(engine.config.effort, EffortLevel::Standard);

    engine.set_effort(EffortLevel::High);
    assert_eq!(engine.config.effort, EffortLevel::High);

    engine.set_effort(EffortLevel::Standard);
    assert_eq!(engine.config.effort, EffortLevel::Standard);
}

#[test]
fn test_set_focus_area() {
    let mut engine = create_test_engine();
    assert!(engine.config.focus_area.is_none());

    engine.set_focus_area(Some("security".to_string()));
    assert_eq!(engine.config.focus_area, Some("security".to_string()));

    engine.set_focus_area(None);
    assert!(engine.config.focus_area.is_none());
}

#[test]
fn test_set_goal_roundtrip() {
    let mut engine = create_test_engine();
    assert!(engine.config.goal.is_none());

    engine.set_goal(Some(GoalSpec {
        objective: "all tests pass".to_string(),
        paused: false,
    }));
    assert_eq!(
        engine.config.goal,
        Some(GoalSpec {
            objective: "all tests pass".to_string(),
            paused: false,
        })
    );

    engine.set_goal(None);
    assert!(engine.config.goal.is_none());
}

#[test]
fn goal_block_injected_when_active() {
    let block = goal_system_block(&GoalSpec {
        objective: "all tests pass".to_string(),
        paused: false,
    });
    assert!(block.text.contains("## Current Goal"));
    assert!(block.text.contains("all tests pass"));
    assert!(block.text.contains(GOAL_COMPLETE_MARKER));
    assert!(block.text.contains(GOAL_BLOCKED_MARKER));
    assert!(block.text.contains("audit"));
}

#[test]
fn goal_block_paused_contains_pause_line() {
    let block = goal_system_block(&GoalSpec {
        objective: "ship it".to_string(),
        paused: true,
    });
    assert!(block.text.contains("PAUSED"));
    assert!(block.text.contains("ship it"));
}

#[test]
fn goal_block_is_non_cached_text() {
    let block = goal_system_block(&GoalSpec {
        objective: "x".to_string(),
        paused: false,
    });
    assert_eq!(block.block_type, "text");
    assert!(block.cache_control.is_none());
}

#[test]
fn effort_suffix_standard_is_none() {
    // Standard is the default: no suffix, byte-identical requests.
    assert!(effort_system_suffix(EffortLevel::Standard).is_none());
}

#[test]
fn effort_suffix_low_content() {
    let suffix = effort_system_suffix(EffortLevel::Low).expect("Low has a suffix");
    assert!(suffix.contains("Be brief"));
    assert!(suffix.contains("minimize exploration"));
    assert!(suffix.contains("state assumptions"));
}

#[test]
fn effort_suffix_high_and_max_content() {
    for level in [EffortLevel::High, EffortLevel::Max] {
        let suffix = effort_system_suffix(level).expect("High/Max have a suffix");
        assert!(
            suffix.contains("Think carefully and exhaustively"),
            "{suffix}"
        );
        assert!(suffix.contains("multi-step verification"), "{suffix}");
    }
}

#[test]
fn effort_dial_sets_thinking_budgets_and_max_tokens_headroom() {
    // The client-config build lives inside the producer; here we pin the
    // dial's contract: High/Max budgets must stay below the raised
    // max_tokens (budget + EFFORT_THINKING_HEADROOM_TOKENS), and a
    // larger pre-set max_tokens is never lowered.
    let default_max_tokens: u32 = 4_096;
    for level in [EffortLevel::High, EffortLevel::Max] {
        let budget = level.thinking_budget().expect("High/Max think");
        let max_tokens = default_max_tokens.max(budget + EFFORT_THINKING_HEADROOM_TOKENS);
        assert!(
            max_tokens > budget,
            "{level}: max_tokens ({max_tokens}) must exceed budget ({budget})"
        );
    }
    // A pre-set max_tokens above the needed floor is preserved as-is.
    let preset: u32 = 32_000;
    let budget = EffortLevel::Max.thinking_budget().unwrap();
    let needed = budget.saturating_add(EFFORT_THINKING_HEADROOM_TOKENS);
    let max_tokens = preset.max(needed);
    assert_eq!(max_tokens, preset);
}

#[test]
fn test_set_effort_updates_config() {
    let mut engine = create_test_engine();
    assert_eq!(engine.config.effort, EffortLevel::Standard);
    engine.set_effort(EffortLevel::Max);
    assert_eq!(engine.config.effort, EffortLevel::Max);
}

#[test]
fn test_set_max_turns() {
    let mut engine = create_test_engine();
    let default_turns = engine.config.max_turns;

    engine.set_max_turns(42);
    assert_eq!(engine.config.max_turns, 42);
    assert_ne!(engine.config.max_turns, default_turns);
}

// ── ToolResultEntry::to_tool_result_content — direct coverage ─────
// Locks down the dual-metadata-image convention added in the
