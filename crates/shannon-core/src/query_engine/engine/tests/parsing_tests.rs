use super::*;

#[test]
fn truncation_stop_reason_detection() {
    // OpenAI-compatible and Anthropic/Gemini spell the output-limit
    // cutoff differently; everything else is a normal stop.
    assert!(is_truncation_stop(Some("length")));
    assert!(is_truncation_stop(Some("max_tokens")));
    assert!(!is_truncation_stop(Some("end_turn")));
    assert!(!is_truncation_stop(Some("stop")));
    assert!(!is_truncation_stop(Some("tool_use")));
    assert!(!is_truncation_stop(None));
}

#[test]
fn cap_tool_result_passthrough_under_cap() {
    let (capped, truncated) = cap_tool_result("short output".to_string());
    assert_eq!(capped, "short output");
    assert!(!truncated);
}

#[test]
fn cap_tool_result_truncates_on_char_boundary_and_notifies() {
    let big = "x".repeat(DEFAULT_MAX_TOOL_RESULT_CHARS + 1000);
    let (capped, truncated) = cap_tool_result(big);
    assert!(truncated);
    assert!(capped.contains("[shannon: output truncated"));
    assert!(capped.len() < DEFAULT_MAX_TOOL_RESULT_CHARS + 200);
    // CJK content must be cut on a char boundary, never mid-codepoint.
    let cjk = "漢字".repeat(DEFAULT_MAX_TOOL_RESULT_CHARS);
    let (capped, truncated) = cap_tool_result(cjk);
    assert!(truncated);
    assert!(
        capped.is_char_boundary(
            capped
                .find("[shannon: output truncated")
                .unwrap_or(capped.len())
        )
    );
}

#[test]
fn cap_tool_result_zero_disables_cap() {
    // env::set_var is process-wide and unsafe under edition 2024 —
    // serialize against other env tests and wrap each call.
    static ENV_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    let _guard = ENV_LOCK
        .get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .expect("env-var test mutex poisoned");
    let key = "SHANNON_MAX_TOOL_OUTPUT_CHARS";
    let saved = std::env::var(key).ok();
    unsafe { std::env::set_var(key, "0") };
    let big = "y".repeat(DEFAULT_MAX_TOOL_RESULT_CHARS * 2);
    let (capped, truncated) = cap_tool_result(big.clone());
    assert!(!truncated);
    assert_eq!(capped, big);
    match saved {
        Some(v) => unsafe { std::env::set_var(key, v) },
        None => unsafe { std::env::remove_var(key) },
    }
}

#[test]
fn split_think_content_extracts_visible_answer() {
    // Closed reasoning block: only the residue is visible.
    let (saw, visible) = split_think_content("<think>plan the fix</think>\n\nDone.");
    assert!(saw);
    assert_eq!(visible.trim(), "Done.");

    // Unclosed block (stream finished mid-reasoning): everything after
    // `<think>` is reasoning — no visible answer.
    let (saw, visible) = split_think_content("<think>still planning the fix");
    assert!(saw);
    assert!(visible.trim().is_empty());

    // Multiple blocks keep both the surrounding text.
    let (saw, visible) = split_think_content("a<think>x</think>b<think>y</think>c");
    assert!(saw);
    assert_eq!(visible, "abc");

    // No reasoning markup: text passes through untouched.
    let (saw, visible) = split_think_content("plain answer");
    assert!(!saw);
    assert_eq!(visible, "plain answer");
}

/// Feed `chunks` through the splitter, concatenating the results —
/// the concatenation must equal feeding the joined text in one chunk.
fn feed_all(splitter: &mut ThinkStreamSplitter, chunks: &[&str]) -> (String, String) {
    let mut thinking = String::new();
    let mut visible = String::new();
    for c in chunks {
        let (t, v) = splitter.feed(c);
        thinking.push_str(&t);
        visible.push_str(&v);
    }
    let (t, v) = splitter.finish();
    thinking.push_str(&t);
    visible.push_str(&v);
    (thinking, visible)
}

#[test]
fn think_stream_splitter_routes_reasoning_to_thinking_channel() {
    // The WP-15 P0-2 shape: reasoning inline, then the visible answer.
    let mut s = ThinkStreamSplitter::default();
    let (t, v) = feed_all(
        &mut s,
        &["<think>I should use the bash tool</think>\n\nCreating the file now."],
    );
    assert_eq!(t, "I should use the bash tool");
    assert_eq!(v, "\n\nCreating the file now.");
}

#[test]
fn think_stream_splitter_handles_tags_split_across_chunks() {
    // Every tag split point must produce the same output as one chunk.
    let text = "Sure.<think>plan more</think>Done.";
    for split in 1..text.len() {
        let mut s = ThinkStreamSplitter::default();
        // Split at a char boundary.
        let mut end = split;
        while !text.is_char_boundary(end) {
            end += 1;
        }
        let (a, b) = text.split_at(end);
        let (t, v) = feed_all(&mut s, &[a, b]);
        assert_eq!(t, "plan more", "split at {end}");
        assert_eq!(v, "Sure.Done.", "split at {end}");
    }
}

#[test]
fn think_stream_splitter_unclosed_block_swallows_tail() {
    // Stream cut mid-reasoning: no visible answer, all reasoning.
    let mut s = ThinkStreamSplitter::default();
    let (t, v) = feed_all(&mut s, &["<think>still thinking", " about it"]);
    assert_eq!(t, "still thinking about it");
    assert_eq!(v, "");
}

#[test]
fn think_stream_splitter_never_panics_on_multi_byte_text() {
    // WP-15 CI regression: Chinese replies end in multi-byte chars; the
    // hold-back used to slice at raw byte offsets and panicked. Real
    // streaming chunks also split text at arbitrary char boundaries.
    let text = "在仙女座星系的外环站，指挥官苏瑞收到了信号。<think>规划一下</think>完成。";
    for split in 0..text.len() {
        if !text.is_char_boundary(split) {
            continue;
        }
        let mut s = ThinkStreamSplitter::default();
        let (a, b) = text.split_at(split);
        let (t, v) = feed_all(&mut s, &[a, b]);
        assert_eq!(t, "规划一下", "split at {split}");
        assert_eq!(
            v, "在仙女座星系的外环站，指挥官苏瑞收到了信号。完成。",
            "split at {split}"
        );
    }
    // A trailing multi-byte char must not panic and must be held back /
    // flushed as visible text (never a phantom tag prefix).
    let mut s = ThinkStreamSplitter::default();
    let (t, v) = feed_all(&mut s, &["指令官苏瑞"]);
    assert_eq!(t, "");
    assert_eq!(v, "指令官苏瑞");
}

#[test]
fn think_stream_splitter_partial_tag_at_end_is_literal_text() {
    // A trailing "<thi" with no continuation is ordinary text.
    let mut s = ThinkStreamSplitter::default();
    let (t, v) = feed_all(&mut s, &["a < b and c<thi"]);
    assert_eq!(t, "");
    assert_eq!(v, "a < b and c<thi");
}

#[test]
fn markdown_bash_command_fallback_matrix() {
    // Bare block, no language tag.
    assert_eq!(
        markdown_bash_command("```\nmkdir -p /tmp/demo\n```").as_deref(),
        Some("mkdir -p /tmp/demo")
    );
    // Explicit bash tag + short lead-in prose (the field-observed shape).
    assert_eq!(
        markdown_bash_command(
            "Creating the file:\n```bash\nprintf 'hello' > /tmp/shannon_demo.txt\n```"
        )
        .as_deref(),
        Some("printf 'hello' > /tmp/shannon_demo.txt")
    );
    // Multi-line block content is kept verbatim (heredocs).
    assert_eq!(
        markdown_bash_command("```sh\ncat > a.txt <<EOF\nhi\nEOF\n```").as_deref(),
        Some("cat > a.txt <<EOF\nhi\nEOF")
    );
    // Long explanation containing an example → NOT a tool call.
    let explained = format!(
        "Here is how you can do it. {}\n```bash\nls -la\n```\n",
        "This command lists files. ".repeat(12)
    );
    assert_eq!(markdown_bash_command(&explained), None);
    // Two blocks → ambiguous → no fallback.
    assert_eq!(
        markdown_bash_command("```bash\nls\n```\nand\n```bash\npwd\n```"),
        None
    );
    // Non-shell language → never executed.
    assert_eq!(markdown_bash_command("```python\nprint('hi')\n```"), None);
    // Empty block → no-op.
    assert_eq!(markdown_bash_command("```bash\n\n```"), None);
    // Plain answer, no fence → None.
    assert_eq!(markdown_bash_command("Done."), None);
}

#[test]
fn parse_text_tool_calls_glm_minimax_form() {
    // The exact WP-15 P0-1 (upgraded) field shape: vendor special-token
    // garbage before the anchor, XML invoke inside.
    let minimax = "] < ]minimax[ > [<tool_call>\n<invoke name=\"Write\">\n<parameter name=\"file_path\">/tmp/shannon_demo.txt</parameter>\n<parameter name=\"content\">hello from my phone</parameter>\n</invoke>\n</tool_call>";
    let calls = parse_text_tool_calls(minimax).expect("should recover the call");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "Write");
    assert_eq!(calls[0].1["file_path"], "/tmp/shannon_demo.txt".to_string());
    assert_eq!(calls[0].1["content"], "hello from my phone".to_string());

    // JSON-typed parameter values keep their type.
    let typed = "<tool_call><invoke name=\"Bash\"><parameter name=\"timeout_ms\"> 30000 </parameter></invoke></tool_call>";
    let calls = parse_text_tool_calls(typed).unwrap();
    assert_eq!(calls[0].1["timeout_ms"], 30000);

    // Multiple calls in one reply.
    let multi = "<tool_call><invoke name=\"Read\"><parameter name=\"path\">a.rs</parameter></invoke></tool_call>\ntext\n<tool_call><invoke name=\"Grep\"><parameter name=\"pattern\">foo</parameter></invoke></tool_call>";
    assert_eq!(parse_text_tool_calls(multi).unwrap().len(), 2);

    // No calls → None.
    assert_eq!(parse_text_tool_calls("plain answer"), None);
    // Unclosed block (stream cut) → None.
    assert_eq!(
        parse_text_tool_calls("<tool_call><invoke name=\"Write\"><parameter name=\"x\">1"),
        None
    );
}

#[test]
fn think_only_default_threshold_only_nudges_blank_answers() {
    // WP-15 P0-1 (upgraded): the shipped default (0) must nudge ONLY blank
    // visible answers — a terse-but-real reply like "cli-ok" is final.
    let threshold = DEFAULT_THINK_ONLY_MIN_ANSWER_CHARS;
    assert_eq!(threshold, 0);
    // Blank visible answer (everything was reasoning) → nudge.
    assert!(is_think_only_response(
        "<think>deliberating</think>",
        0,
        threshold
    ));
    // Terse real answer after reasoning → NOT think-only.
    assert!(!is_think_only_response(
        "<think>deliberating</think>cli-ok",
        0,
        threshold
    ));
    // Plain short answer without reasoning markup → never nudged.
    assert!(!is_think_only_response("cli-ok", 0, threshold));
    // An explicit larger threshold keeps the legacy short-answer behavior.
    assert!(is_think_only_response("<think>x</think>cli-ok", 0, 200));
}

#[test]
fn think_only_detection_matrix() {
    const THRESHOLD: usize = 200;

    // Empty text (incl. thinking-delta-only responses) → nudge.
    assert!(is_think_only_response("", 0, THRESHOLD));
    assert!(is_think_only_response("   \n  ", 0, THRESHOLD));

    // All-reasoning text → nudge.
    assert!(is_think_only_response(
        "<think>I should look at the failing test first.</think>",
        0,
        THRESHOLD
    ));

    // Reasoning-dominated with a tiny residue → nudge (the minimax
    // think-only shape).
    assert!(is_think_only_response(
        "<think>long reasoning about the repo layout and the failing
         test, plenty of deliberation that burns the turn</think>ok",
        0,
        THRESHOLD
    ));

    // Any tool call → never a nudge, whatever the text.
    assert!(!is_think_only_response("", 1, THRESHOLD));
    assert!(!is_think_only_response("<think>x</think>", 2, THRESHOLD));

    // Reasoning + a substantive visible answer → normal completion.
    let answer = "I fixed the failing test by correcting the assertion in \
                  src/lib.rs: the expected value was inverted after the \
                  refactor. The helper now normalizes line endings before \
                  comparison, which is what the spec requires. All 14 tests \
                  in the suite pass, including the two previously flaky \
                  ones, and I re-ran the full build to confirm no \
                  regressions elsewhere.";
    assert!(
        answer.chars().count() > 200,
        "sample must clear the default threshold"
    );
    assert!(!is_think_only_response(
        &format!("<think>deliberation</think>{answer}"),
        0,
        THRESHOLD
    ));

    // Plain short text WITHOUT reasoning markup is a deliberate terse
    // answer — never nudged (behavior completely unchanged for it).
    assert!(!is_think_only_response("Done.", 0, THRESHOLD));
    assert!(!is_think_only_response("ok", 0, THRESHOLD));
}

#[test]
fn think_only_env_overrides_parse() {
    // Non-env tests below rely on defaults; this group locks the parsing
    // contract (unset/empty/garbage → defaults) behind the module env
    // lock so parallel tests can't race the process-global table.
    use std::sync::{Mutex, OnceLock};
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = ENV_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("env-var test mutex poisoned");

    let saved_max = env::var("SHANNON_THINK_ONLY_NUDGE_MAX").ok();
    let saved_min = env::var("SHANNON_THINK_ONLY_MIN_ANSWER_CHARS").ok();

    // Defaults when unset.
    unsafe {
        env::remove_var("SHANNON_THINK_ONLY_NUDGE_MAX");
    };
    unsafe {
        env::remove_var("SHANNON_THINK_ONLY_MIN_ANSWER_CHARS");
    };
    assert_eq!(think_only_nudge_max(), 2);
    // WP-15 P0-1 (upgraded): shipped default is 0 (nudge only blank answers).
    assert_eq!(think_only_min_answer_chars(), 0);

    // Explicit overrides.
    unsafe {
        env::set_var("SHANNON_THINK_ONLY_NUDGE_MAX", "5");
    };
    unsafe {
        env::set_var("SHANNON_THINK_ONLY_MIN_ANSWER_CHARS", "40");
    };
    assert_eq!(think_only_nudge_max(), 5);
    assert_eq!(think_only_min_answer_chars(), 40);

    // 0 disables nudging entirely (budget always exhausted).
    unsafe {
        env::set_var("SHANNON_THINK_ONLY_NUDGE_MAX", "0");
    };
    assert_eq!(think_only_nudge_max(), 0);

    // Empty / garbage → defaults, no panic.
    unsafe {
        env::set_var("SHANNON_THINK_ONLY_NUDGE_MAX", "");
    };
    unsafe {
        env::set_var("SHANNON_THINK_ONLY_MIN_ANSWER_CHARS", "bogus");
    };
    assert_eq!(think_only_nudge_max(), 2);
    // WP-15 P0-1 (upgraded): shipped default is 0 (nudge only blank answers).
    assert_eq!(think_only_min_answer_chars(), 0);

    // Restore the prior process state for other tests.
    match saved_max {
        Some(v) => unsafe { env::set_var("SHANNON_THINK_ONLY_NUDGE_MAX", v) },
        None => unsafe { env::remove_var("SHANNON_THINK_ONLY_NUDGE_MAX") },
    }
    match saved_min {
        Some(v) => unsafe { env::set_var("SHANNON_THINK_ONLY_MIN_ANSWER_CHARS", v) },
        None => unsafe { env::remove_var("SHANNON_THINK_ONLY_MIN_ANSWER_CHARS") },
    }
}

/// B.6: SHANNON_TOKEN_BUDGET parses the same way as the think-only
/// overrides — unset / empty / unparseable → default (0 = disabled).
/// Locks the process-global env behind the same mutex used by the
/// think-only env tests to keep parallel tests from racing.
#[test]
fn token_budget_env_override_parses() {
    use std::sync::{Mutex, OnceLock};
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = ENV_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("env-var test mutex poisoned");

    let saved = env::var("SHANNON_TOKEN_BUDGET").ok();

    unsafe {
        env::remove_var("SHANNON_TOKEN_BUDGET");
    }
    assert_eq!(
        token_budget_limit(),
        0,
        "unset should default to 0 (disabled)"
    );

    unsafe {
        env::set_var("SHANNON_TOKEN_BUDGET", "120000");
    }
    assert_eq!(token_budget_limit(), 120_000);

    unsafe {
        env::set_var("SHANNON_TOKEN_BUDGET", "0");
    }
    assert_eq!(token_budget_limit(), 0, "explicit 0 must disable");

    unsafe {
        env::set_var("SHANNON_TOKEN_BUDGET", "");
    }
    assert_eq!(
        token_budget_limit(),
        0,
        "empty value must fall back to default"
    );

    unsafe {
        env::set_var("SHANNON_TOKEN_BUDGET", "bogus");
    }
    assert_eq!(token_budget_limit(), 0, "garbage must fall back to default");

    match saved {
        Some(v) => unsafe { env::set_var("SHANNON_TOKEN_BUDGET", v) },
        None => unsafe { env::remove_var("SHANNON_TOKEN_BUDGET") },
    }
}

/// B.6: pure nudge builder contract. Pinned so the wired-in engine
/// call site and any future tweak share the exact phrase tested here.
#[test]
fn token_budget_nudge_for_pure_contract() {
    // Disabled budget → no nudge.
    assert!(token_budget_nudge_for(50_000, 0).is_none());
    // At or under budget → no nudge.
    assert!(token_budget_nudge_for(100, 100).is_none());
    assert!(token_budget_nudge_for(99, 100).is_none());
    // Over budget → nudge with the required prefix.
    let nudge = token_budget_nudge_for(101, 100).expect("nudge fires");
    assert!(
        nudge.contains("Context is large"),
        "must contain the exact phrase 'Context is large'; got: {nudge:?}"
    );
    assert!(
        nudge.contains("101/100 tokens"),
        "must report used/budget; got: {nudge:?}"
    );
}

/// B.6 integration-style test: simulate the per-turn `total_input_tokens`
/// accumulating past the budget. Each turn where the cumulative total
/// exceeds the cap must yield a nudge message starting with the
/// required phrase. The plan-doc test contract: "模拟 budget=100, 注入
/// 两轮累计 token > 100, 断言第三轮 user message 含 'Context is large'".
#[test]
fn token_budget_nudge_fires_across_three_turns() {
    let budget: u64 = 100;
    // Per-turn usage: 60, 60, 60 → cumulative 60, 120, 180.
    // Only turn 2 and turn 3 exceed the budget.
    let per_turn = [60u64, 60, 60];
    let mut cumulative: u64 = 0;
    let mut nudge_count = 0;
    let mut last_nudge: Option<String> = None;

    for (turn_idx, inc) in per_turn.iter().enumerate() {
        cumulative = cumulative.saturating_add(*inc);
        // Mirror the engine loop's gating: `total_input_tokens > budget`.
        if let Some(text) = token_budget_nudge_for(cumulative, budget) {
            nudge_count += 1;
            last_nudge = Some(text);
            eprintln!(
                "turn {} nudge fired (cumulative={cumulative})",
                turn_idx + 1
            );
        }
    }

    assert_eq!(
        nudge_count, 2,
        "two turns exceeded the budget (turn 2 and turn 3)"
    );
    let text = last_nudge.expect("a nudge must have fired");
    assert!(
        text.contains("Context is large"),
        "the third turn's user message must contain 'Context is large' \
         (plan-doc test contract); got: {text}"
    );
    assert!(
        text.contains("180/100 tokens"),
        "must report the third turn's cumulative total; got: {text}"
    );
}
