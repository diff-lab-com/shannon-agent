//! Engine tunables: environment overrides, nudges, and result caps.
//!
//! Split out of `engine.rs` (Wave 2 architecture step). Everything here is a
//! self-contained knob or policy constant with the env-var contract
//! (unset/empty/garbage → default).

/// the model must resume mid-task rather than restart or restate itself.
pub(super) const TRUNCATION_CONTINUATION_PROMPT: &str = "Your previous response was cut off by the \
     output token limit before you finished. Continue exactly where you left off and \
     complete the task. Do not repeat what you already wrote.";

// ── Think-only continuation nudge (A1) ───────────────────────────────
//
// eval-findings-2026-09-glm.md A1: models occasionally return a
// "think-only" response — reasoning content only, no tool_use, no
// user-facing answer. The engine treated that as a normal completion,
// ended the session headless, and produced an empty patch (minimax b11
// lost 3/50 SWE tasks — 4-8pp — to exactly this pattern). When the final
// response carries no tool call and no substantive answer, the engine now
// synthesizes a user-side re-prompt instead of completing.

/// Re-prompt sent when a response has no tool calls and no usable final
/// answer (see [`is_think_only_response`]).
pub(super) const THINK_ONLY_NUDGE_PROMPT: &str = "Your previous response contained no tool calls \
     and no final answer. Either invoke the appropriate tool to continue the task, or \
     reply with your final answer to the user.";

/// Default cap on consecutive think-only nudges per query. Override with
/// `SHANNON_THINK_ONLY_NUDGE_MAX`. When exhausted the query ends exactly as
/// it did before this feature existed.
pub(super) const DEFAULT_THINK_ONLY_NUDGE_MAX: u32 = 2;

/// Default minimum length (chars) of the visible — i.e. non-reasoning —
/// answer for a response to count as substantive. Override with
/// `SHANNON_THINK_ONLY_MIN_ANSWER_CHARS`.
///
/// WP-15 P0-1 (upgraded): the default dropped from 200 to 0 — nudge only when
/// the visible answer is *blank*. The old 200-char threshold re-prompted
/// models that had already answered tersely ("Reply with exactly: cli-ok" →
/// `cli-ok` is 6 visible chars), and the "no final answer" nudge then sent
/// reasoning models into a self-doubt loop (7k–21k tokens for one Q&A, field-
/// observed on MiniMax M3). An unhelpfully-short-but-present answer is the
/// model's call; blank-only replies still get one chance to recover.
pub(super) const DEFAULT_THINK_ONLY_MIN_ANSWER_CHARS: usize = 0;

/// Read a non-negative integer env override, falling back to `default` when
/// unset, empty, or unparseable (same conventions as
/// [`QueryEngine::apply_env_overrides`]: only non-empty values are
/// intentional overrides; garbage is silently ignored).
pub(super) fn env_num_override(name: &str, default: u32) -> u32 {
    match std::env::var(name) {
        Ok(v) if !v.trim().is_empty() => v.trim().parse::<u32>().unwrap_or_else(|_| {
            tracing::warn!("Invalid {name}={v:?} — using default {default}");
            default
        }),
        _ => default,
    }
}

/// Max consecutive think-only nudges for this query
/// (`SHANNON_THINK_ONLY_NUDGE_MAX`, default
/// [`DEFAULT_THINK_ONLY_NUDGE_MAX`]).
pub(super) fn think_only_nudge_max() -> u32 {
    env_num_override("SHANNON_THINK_ONLY_NUDGE_MAX", DEFAULT_THINK_ONLY_NUDGE_MAX)
}

/// Visible-answer threshold in chars for the think-only classifier
/// (`SHANNON_THINK_ONLY_MIN_ANSWER_CHARS`, default
/// [`DEFAULT_THINK_ONLY_MIN_ANSWER_CHARS`]).
pub(super) fn think_only_min_answer_chars() -> usize {
    env_num_override(
        "SHANNON_THINK_ONLY_MIN_ANSWER_CHARS",
        DEFAULT_THINK_ONLY_MIN_ANSWER_CHARS as u32,
    ) as usize
}

// ── Tool-result cap ────────────────────────────────────────────────────────
// A single tool result is capped before it enters model context. Claude Code
// caps tool outputs (~25k tokens); without a cap one noisy Bash call (e.g.
// `cat` of a multi-MB file) evicts the working context and triggers cascading
// compaction. Override: `SHANNON_MAX_TOOL_OUTPUT_CHARS`.

/// Default cap for a single tool result, in bytes.
pub(super) const DEFAULT_MAX_TOOL_RESULT_CHARS: usize = 40_000;

/// Context-usage ratio above which stale tool results in older turns are
/// cleared (micro-compaction) before full lossy compaction is considered.
pub(super) const MICRO_PRUNE_THRESHOLD: f32 = 0.7;

/// Resolve the tool-result cap. `0` disables the cap entirely.
pub(super) fn max_tool_result_chars() -> usize {
    env_num_override(
        "SHANNON_MAX_TOOL_OUTPUT_CHARS",
        DEFAULT_MAX_TOOL_RESULT_CHARS as u32,
    ) as usize
}

/// Cap a tool result's content at [`max_tool_result_chars`] bytes on a char
/// boundary, appending a truncation notice. Returns `(content, truncated)`.
pub(super) fn cap_tool_result(content: String) -> (String, bool) {
    let cap = max_tool_result_chars();
    if cap == 0 || content.len() <= cap {
        return (content, false);
    }
    let mut end = cap;
    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }
    let omitted = content.len() - end;
    (
        format!(
            "{}\n\n[shannon: output truncated — {omitted} of {} bytes omitted. \
             Re-run with a narrower scope (head/tail/grep) to read specific content.]",
            &content[..end],
            content.len()
        ),
        true,
    )
}

// ── B.6 ────────────────────────────────────────────────────────────────────
// SHANNON_TOKEN_BUDGET: a hard cap on cumulative input tokens. When the cap
// is exceeded the engine synthesizes a user-side message that pushes the
// model away from full-file reads (the eval failure mode measured in
// eval-findings-2026-09-glm.md F2 — single-shot `cat` of multi-MB files
// burned the remaining turn budget). Disabling the cap = SHANNON_TOKEN_BUDGET=0.

/// Default value for the B.6 token-budget watchdog (`SHANNON_TOKEN_BUDGET`).
/// Zero disables it entirely so non-eval users are unaffected.
pub(super) const DEFAULT_TOKEN_BUDGET: u64 = 0;

/// Recommended eval setting for SWE-bench / TB2.1 runs (120k tokens ≈
/// the cap after which the model starts losing recent context in
/// `estimate_tokens`).
#[allow(dead_code)] // KEEP: documentation anchor; eval reads the env directly.
pub const RECOMMENDED_TOKEN_BUDGET: u64 = 120_000;

/// Resolve the configured B.6 budget. `0` (default) disables the watchdog.
/// Honors the same parse contract as [`env_num_override`]: unset, empty,
/// or unparseable → `DEFAULT_TOKEN_BUDGET`.
pub(super) fn token_budget_limit() -> u64 {
    env_num_override("SHANNON_TOKEN_BUDGET", DEFAULT_TOKEN_BUDGET as u32) as u64
}

/// Build the targeted-read nudge the model receives when the B.6 budget is
/// exceeded. Pure: depends only on `(used, budget)` so the call site can
/// pass live counts and unit tests can pin both arguments.
///
/// Returns `None` when `budget == 0` (disabled) or `used <= budget`. The
/// text is the exact phrase required by the plan — verbatim so the unit
/// test's `contains("Context is large")` assertion matches.
pub(super) fn token_budget_nudge_for(used: u64, budget: u64) -> Option<String> {
    if budget == 0 || used <= budget {
        return None;
    }
    Some(format!(
        "Context is large ({used}/{budget} tokens). Prefer targeted reads \
         (`Grep`, `head -c`, `Read` with offset+limit) over full-file reads \
         or `cat`. Re-read only what you need; commit fixes promptly."
    ))
}
