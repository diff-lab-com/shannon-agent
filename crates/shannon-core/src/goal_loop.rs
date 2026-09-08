//! Session-goal continuation decision (P0-2) — the pure goal loop logic.
//!
//! This is the single source of truth for "what should happen to a goal
//! after a turn ends": marker detection, anti-spin (two consecutive turns
//! without tool calls), stall strikes, the optional USD budget cap, the
//! iteration cap, and the exact continuation prompt injected into the next
//! turn. It was extracted verbatim from the TUI implementation
//! (`crates/shannon-ui/src/repl/commands/goal.rs`, functions
//! `goal_completion_marker` + `goal_continuation_decision_with_facts`) so
//! the desktop goal runner and the TUI cannot drift apart. The TUI now
//! delegates here; its behaviour is unchanged (covered by its own tests).
//!
//! Everything in this module is pure: no I/O, no engine access, no state
//! mutation. Lifecycle gating ("is the goal even active?") stays with the
//! caller, which is why [`GoalContinuation::Inactive`](goal_loop::GoalContinuation::Inactive) exists for them to
//! map to — [`decide_goal_continuation`](goal_loop::decide_goal_continuation) itself never returns it.

use crate::query_engine::{GOAL_BLOCKED_MARKER, GOAL_COMPLETE_MARKER};

/// Default cap on `stall_strikes` before the goal pauses (Magentic-One +
/// auto_test::no_progress_strikes both use 3 as the default).
pub const GOAL_DEFAULT_MAX_STALL_STRIKES: u32 = 3;

/// Default continuation cap — **unlimited** (0). The active guard rails
/// (strict completion contract, anti-spin, stall strikes, GOAL_BLOCKED,
/// optional budget cap) are the real stop signals; a total-turn cap only
/// exists when the user explicitly asks for one.
pub const GOAL_DEFAULT_MAX_ITERATIONS: u32 = 0;

/// Completion marker contract: the marker must be the reply's final
/// non-empty line. `GOAL_COMPLETE` must match exactly (case-insensitive);
/// `GOAL_BLOCKED` may carry a `: reason` suffix. A marker anywhere else —
/// mid-text, in a code block, or as a hyphenated word — does not count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalMarker {
    Complete,
    Blocked(String),
}

/// Detect a completion marker on a reply's final non-empty line.
///
/// The caller extracts the last non-empty line (`msg.lines().rev().find(|l|
/// !l.trim().is_empty())`); this function trims it and matches the whole
/// line — case-insensitively — against `GOAL_COMPLETE` / `GOAL_BLOCKED`.
/// `GOAL_BLOCKED` may carry a `: reason` suffix; the reason is returned
/// trimmed and may be empty.
pub fn detect_goal_marker(last_non_empty_line: &str) -> Option<GoalMarker> {
    let trimmed = last_non_empty_line.trim();
    if trimmed.eq_ignore_ascii_case(GOAL_COMPLETE_MARKER) {
        return Some(GoalMarker::Complete);
    }
    if trimmed.to_uppercase().starts_with(GOAL_BLOCKED_MARKER) {
        let reason = trimmed
            .get(GOAL_BLOCKED_MARKER.len()..)
            .unwrap_or("")
            .trim_start_matches([':', ' '])
            .trim()
            .to_string();
        return Some(GoalMarker::Blocked(reason));
    }
    None
}

/// What a turn actually did, in terms the guard rails can compare.
///
/// `spent_usd` carries the USD cost attributed to the run so far (desktop
/// accumulates `QueryEvent::Usage.cost_usd` across turns; the TUI feeds the
/// per-turn delta, which keeps its historical behaviour because it never
/// observed costs). The budget cap only fires when `max_budget_usd` is
/// `Some` and `spent_usd >= cap`.
#[derive(Debug, Clone, PartialEq)]
pub struct GoalDecisionInput {
    /// The user's objective / completion condition (verbatim user words).
    /// Needed to render the continuation prompt exactly as the TUI has
    /// always done.
    pub objective: String,
    /// Auto-continuations performed so far.
    pub iterations: u32,
    /// Continuation cap; 0 = unlimited.
    pub max_iterations: u32,
    /// Turns that produced zero tool calls since the last user input
    /// (anti-spin counter).
    pub consecutive_no_tool_turns: u32,
    /// Shared strike budget for anti-spin + stall detection.
    pub stall_strikes: u32,
    /// Budget cap (USD). `None` = no cap (design R4: only an explicit
    /// budget terminates).
    pub max_budget_usd: Option<f64>,
    /// USD spent so far (see struct docs for TUI/desktop attribution).
    pub spent_usd: f64,
    /// True iff at least one tool call was produced this turn
    /// (deterministic anti-spin signal).
    pub had_tool_calls: bool,
}

/// What should happen to the goal after a turn ends. Pure decision — state
/// mutations and side effects live with the callers (TUI
/// `check_goal_continuation`, desktop goal runner).
#[derive(Debug, Clone, PartialEq)]
pub enum GoalContinuation {
    /// No active goal — nothing to do. Never produced by
    /// [`decide_goal_continuation`]; callers gate on their own lifecycle
    /// status before calling and map that case to this variant.
    Inactive,
    /// Completion marker seen: mark the goal Complete.
    Completed,
    /// Blocker marker seen: pause the goal and surface the reason.
    Blocked(String),
    /// Iteration cap exhausted: pause the goal.
    MaxReached,
    /// Anti-spin or stall-strike threshold tripped; pause and surface the
    /// reason. The reason carries the strike counts so the user can see why
    /// the goal was halted before blindly resuming.
    PausedNoProgress(String),
    /// Goal budget cap exceeded; treated as a recoverable terminal (must
    /// explicitly re-raise the cap or clear the goal).
    BudgetLimited(String),
    /// Keep going: `iterations` is the next value to store, `prompt` the
    /// continuation text to inject as the next user turn.
    Continue { iterations: u32, prompt: String },
}

/// Prompt injected when the goal is not yet complete and the loop
/// auto-continues. Ported verbatim from the TUI (`continuation_prompt`) —
/// the format, wording and marker names are part of the model-facing
/// contract and must not drift between shells.
pub fn continuation_prompt(iterations: u32, max_iterations: u32, objective: &str) -> String {
    let max = if max_iterations == 0 {
        "∞".to_string()
    } else {
        max_iterations.to_string()
    };
    format!(
        "[Goal iteration {iterations}/{max}] Continue working toward the goal: {objective}\n\n\
         The goal is NOT yet complete — no completion marker was detected in your last reply.\n\
         Before continuing:\n\
         1. Progress check: what concrete progress did the last iteration make? If none was made and none is possible, explain why and end your reply with \"{GOAL_BLOCKED_MARKER}: <reason>\".\n\
         2. Re-verify what remains. Do not redo completed work.\n\
         3. When the goal is fully met and you have audited completion with evidence, end your final line with exactly: {GOAL_COMPLETE_MARKER}",
    )
}

/// Pure continuation decision: inspects the goal counters and the detected
/// completion marker of the last assistant reply and decides the next
/// lifecycle step. Ported 1:1 from the TUI's
/// `goal_continuation_decision_with_facts` — verdict priority: marker >
/// max-iterations > budget > anti-spin > stall strikes > continue.
pub fn decide_goal_continuation(
    input: &GoalDecisionInput,
    marker: Option<GoalMarker>,
) -> GoalContinuation {
    // Termination markers short-circuit the progress guards.
    match marker {
        Some(GoalMarker::Complete) => return GoalContinuation::Completed,
        Some(GoalMarker::Blocked(reason)) => return GoalContinuation::Blocked(reason),
        None => {}
    }
    let next = input.iterations + 1;
    let max_hit = input.max_iterations > 0 && next > input.max_iterations;
    let mut next_no_tool_turns = input.consecutive_no_tool_turns;
    let mut next_stall_strikes = input.stall_strikes;
    if input.had_tool_calls {
        next_no_tool_turns = 0;
        next_stall_strikes = next_stall_strikes.saturating_sub(1);
    } else {
        next_no_tool_turns += 1;
        next_stall_strikes += 1;
    }
    if max_hit {
        return GoalContinuation::MaxReached;
    }
    // Budget cap (USD). Budget beats max_iterations in the verdict
    // priority: spending money is more irreversible than burning turns.
    let budget_limit_hit = input
        .max_budget_usd
        .map(|cap| input.spent_usd >= cap)
        .unwrap_or(false);
    if budget_limit_hit {
        let cap = input.max_budget_usd.unwrap_or(0.0);
        return GoalContinuation::BudgetLimited(format!(
            "Goal budget exhausted (${:.4} \u{2265} cap ${:.4}). The goal stays paused; raise the cap with /goal <obj> --budget ${:.4} or /goal clear to drop.",
            input.spent_usd, cap, cap
        ));
    }
    if next_no_tool_turns >= 2 {
        return GoalContinuation::PausedNoProgress(format!(
            "Two consecutive turns with no tool calls. Pause and decide whether to /goal resume or /goal clear (strike {next_stall_strikes}/{GOAL_DEFAULT_MAX_STALL_STRIKES})"
        ));
    }
    if next_stall_strikes >= GOAL_DEFAULT_MAX_STALL_STRIKES {
        return GoalContinuation::PausedNoProgress(format!(
            "Reached stall-strike budget ({next_stall_strikes}/{GOAL_DEFAULT_MAX_STALL_STRIKES}). Pause to inspect; /goal resume re-arms the budget, /goal clear drops the goal"
        ));
    }
    GoalContinuation::Continue {
        iterations: next,
        prompt: continuation_prompt(next, input.max_iterations, &input.objective),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── detect_goal_marker ─────────────────────────────────────────────

    #[test]
    fn marker_last_line_exact_match_case_insensitive() {
        assert_eq!(
            detect_goal_marker("GOAL_COMPLETE"),
            Some(GoalMarker::Complete)
        );
        assert_eq!(
            detect_goal_marker("  goal_complete  "),
            Some(GoalMarker::Complete)
        );
    }

    #[test]
    fn marker_mid_text_not_detected() {
        assert_eq!(detect_goal_marker("GOAL_COMPLETE is near"), None);
        assert_eq!(detect_goal_marker("```GOAL_COMPLETE``` working…"), None);
        assert_eq!(detect_goal_marker(""), None);
        assert_eq!(detect_goal_marker("normal reply"), None);
    }

    #[test]
    fn marker_blocked_extracts_reason() {
        assert_eq!(
            detect_goal_marker("GOAL_BLOCKED: need prod credentials"),
            Some(GoalMarker::Blocked("need prod credentials".into()))
        );
        assert_eq!(
            detect_goal_marker("GOAL_BLOCKED"),
            Some(GoalMarker::Blocked(String::new()))
        );
    }

    #[test]
    fn marker_prefix_junk_not_complete() {
        // Hyphenated / decorated markers must not complete the goal.
        assert_eq!(detect_goal_marker("GOAL_COMPLETE-ish"), None);
        assert_eq!(detect_goal_marker("NOT_GOAL_COMPLETE"), None);
    }

    // ── decide_goal_continuation ───────────────────────────────────────

    fn input(objective: &str) -> GoalDecisionInput {
        GoalDecisionInput {
            objective: objective.into(),
            iterations: 0,
            max_iterations: 0,
            consecutive_no_tool_turns: 0,
            stall_strikes: 0,
            max_budget_usd: None,
            spent_usd: 0.0,
            had_tool_calls: true,
        }
    }

    #[test]
    fn decision_continues_when_incomplete() {
        let d = decide_goal_continuation(&input("fix lint"), None);
        match d {
            GoalContinuation::Continue { iterations, prompt } => {
                assert_eq!(iterations, 1);
                assert!(prompt.contains("[Goal iteration 1/∞]"), "{prompt}");
                assert!(prompt.contains("fix lint"), "{prompt}");
                assert!(prompt.contains(GOAL_BLOCKED_MARKER), "{prompt}");
                assert!(prompt.contains(GOAL_COMPLETE_MARKER), "{prompt}");
            }
            other => panic!("expected Continue, got {other:?}"),
        }
    }

    #[test]
    fn decision_unlimited_budget_never_hits_max() {
        let mut i = input("keep going");
        i.iterations = 9_999;
        assert!(matches!(
            decide_goal_continuation(&i, None),
            GoalContinuation::Continue { .. }
        ));
    }

    #[test]
    fn decision_max_reached_pauses() {
        let mut i = input("endless");
        i.max_iterations = 10;
        i.iterations = 10; // cap exhausted
        assert_eq!(
            decide_goal_continuation(&i, None),
            GoalContinuation::MaxReached
        );
    }

    #[test]
    fn decision_completed_and_blocked_from_markers() {
        let i = input("deploy");
        assert_eq!(
            decide_goal_continuation(&i, Some(GoalMarker::Complete)),
            GoalContinuation::Completed
        );
        assert_eq!(
            decide_goal_continuation(&i, Some(GoalMarker::Blocked("no kubeconfig".into()))),
            GoalContinuation::Blocked("no kubeconfig".into())
        );
    }

    // ── anti-spin + stall strikes ──────────────────────────────────────

    #[test]
    fn anti_spin_two_consecutive_no_tool_turns_pauses() {
        let mut i = input("ship");
        i.consecutive_no_tool_turns = 1;
        i.had_tool_calls = false;
        let d = decide_goal_continuation(&i, None);
        assert!(
            matches!(d, GoalContinuation::PausedNoProgress(_)),
            "two consecutive no-tool turns must pause: {d:?}"
        );
    }

    #[test]
    fn stall_strikes_reach_threshold_pauses_even_with_tool_calls() {
        let mut i = input("ship");
        i.stall_strikes = GOAL_DEFAULT_MAX_STALL_STRIKES - 1;
        i.had_tool_calls = false;
        let d = decide_goal_continuation(&i, None);
        assert!(
            matches!(d, GoalContinuation::PausedNoProgress(_)),
            "stall-strike budget must trip: {d:?}"
        );
    }

    #[test]
    fn tool_call_resets_strike_budget_allowing_continue() {
        let mut i = input("ship");
        i.consecutive_no_tool_turns = 1;
        i.stall_strikes = 2;
        let d = decide_goal_continuation(&i, None);
        assert!(matches!(d, GoalContinuation::Continue { .. }), "{d:?}");
    }

    // ── budget accounting ──────────────────────────────────────────────

    #[test]
    fn budget_cap_fires_budget_limited_verdict() {
        let mut i = input("ship");
        i.max_budget_usd = Some(1.0);
        i.spent_usd = 1.5;
        let d = decide_goal_continuation(&i, None);
        assert!(matches!(d, GoalContinuation::BudgetLimited(_)), "{d:?}");
    }

    #[test]
    fn no_budget_cap_means_no_budget_check() {
        let mut i = input("ship");
        i.spent_usd = 100.0;
        let d = decide_goal_continuation(&i, None);
        assert!(matches!(d, GoalContinuation::Continue { .. }), "{d:?}");
    }

    #[test]
    fn continuation_prompt_is_verbatim_tui_contract() {
        let p = continuation_prompt(1, 0, "fix lint");
        assert!(p.contains("[Goal iteration 1/∞]"), "{p}");
        assert!(p.contains("fix lint"), "{p}");
        assert!(p.contains(GOAL_BLOCKED_MARKER), "{p}");
        assert!(p.contains(GOAL_COMPLETE_MARKER), "{p}");

        let capped = continuation_prompt(2, 10, "x");
        assert!(capped.contains("[Goal iteration 2/10]"), "{capped}");
    }
}
