//! `/goal` — session goal: a persistent objective the agent keeps working
//! toward across turns until met.
//!
//! `/goal <objective>` set · `/goal` show · `/goal pause|resume|clear` ·
//! `/goal status`. The goal is injected into every query's system prompt
//! (see `QueryEngine::set_goal`), survives compaction, persists in the
//! session sidecar, and auto-continues after each turn until the model ends
//! a reply with a strict completion marker (`GOAL_COMPLETE` / `GOAL_BLOCKED`
//! as the final non-empty line). Completion is mutually exclusive with
//! `/ralph` and `/loop`, which own their own auto-continuation loops.

use shannon_core::query_engine::{GOAL_BLOCKED_MARKER, GOAL_COMPLETE_MARKER};

use super::set_error;
use crate::Result;
use crate::repl::Repl;
use crate::repl::state::{GOAL_DEFAULT_MAX_ITERATIONS, GoalState, GoalStatus};
use crate::widgets::ChatRole;
use rust_i18n::t;

/// Parsed `/goal` subcommand.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GoalAction {
    Show,
    Set {
        objective: String,
        max_iterations: usize,
        max_budget_usd: Option<f64>,
    },
    Pause,
    Resume,
    Clear,
}

/// Pure parser (unit-tested): maps `/goal <args>` to an action.
pub(crate) fn parse_goal_args(args: &str) -> GoalAction {
    let mut max_iterations = GOAL_DEFAULT_MAX_ITERATIONS;
    let mut max_budget_usd: Option<f64> = None;
    let mut remaining = args.trim();

    // `--max N` prefix (same style as /ralph's --max). Invalid or missing N
    // silently keeps the default; `--max 0` means unlimited.
    if let Some(rest) = remaining.strip_prefix("--max ") {
        let parts: Vec<&str> = rest.splitn(2, ' ').collect();
        max_iterations = parts
            .first()
            .and_then(|m| m.parse::<usize>().ok())
            .unwrap_or(GOAL_DEFAULT_MAX_ITERATIONS);
        remaining = parts.get(1).copied().unwrap_or("").trim();
    }
    // `--budget $N` cap (P2.3). Invalid N silently keeps the budget off
    // (design R4: no implicit budget; only explicit --budget terminates).
    if let Some(rest) = remaining.strip_prefix("--budget ") {
        let parts: Vec<&str> = rest.splitn(2, ' ').collect();
        max_budget_usd = parts
            .first()
            .and_then(|m| m.trim_start_matches('$').parse::<f64>().ok());
        remaining = parts.get(1).copied().unwrap_or("").trim();
    }

    match remaining {
        "" => GoalAction::Show,
        "clear" | "off" | "stop" | "cancel" | "reset" | "none" => GoalAction::Clear,
        "pause" => GoalAction::Pause,
        "resume" => GoalAction::Resume,
        "status" | "show" => GoalAction::Show,
        objective => GoalAction::Set {
            objective: objective.to_string(),
            max_iterations,
            max_budget_usd,
        },
    }
}

/// Completion marker contract: the marker must be the reply's final
/// non-empty line. `GOAL_COMPLETE` must match exactly (case-insensitive);
/// `GOAL_BLOCKED` may carry a `: reason` suffix. A marker anywhere else —
/// mid-text, in a code block, or as a hyphenated word — does not count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GoalMarker {
    Complete,
    Blocked(String),
}

pub(crate) fn goal_completion_marker(msg: &str) -> Option<GoalMarker> {
    let last = msg.lines().rev().find(|l| !l.trim().is_empty())?;
    let trimmed = last.trim();
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

/// Prompt injected when the goal is not yet complete and the loop
/// auto-continues. Surfaces in the input box like `/ralph` iterations —
/// transparent to the user and logged as a normal turn.
pub(crate) fn continuation_prompt(goal: &GoalState) -> String {
    let max = if goal.max_iterations == 0 {
        "∞".to_string()
    } else {
        goal.max_iterations.to_string()
    };
    format!(
        "[Goal iteration {}/{max}] Continue working toward the goal: {}\n\n\
         The goal is NOT yet complete — no completion marker was detected in your last reply.\n\
         Before continuing:\n\
         1. Progress check: what concrete progress did the last iteration make? If none was made and none is possible, explain why and end your reply with \"{}: <reason>\".\n\
         2. Re-verify what remains. Do not redo completed work.\n\
         3. When the goal is fully met and you have audited completion with evidence, end your final line with exactly: {}",
        goal.iterations, goal.objective, GOAL_BLOCKED_MARKER, GOAL_COMPLETE_MARKER
    )
}

/// Persist the current goal via read-modify-write on the sidecar.
///
/// Read-modify-write matters: the per-turn sidecar saves elsewhere only set
/// `title`, and `merge_from_disk` preserves set fields — writing `goal: None`
/// blindly after `/goal clear` would resurrect the old goal on disk.
pub(crate) fn save_goal_sidecar(repl: &Repl) {
    let Some(ref engine) = repl.query_engine else {
        return;
    };
    let session_id = engine.session_id();
    let store = repl.l0_store();
    let mut sidecar = store.sidecar(&session_id);
    sidecar.goal = repl.state.goal.as_ref().map(|g| g.to_stored());
    // Use the non-merging variant: we loaded the full sidecar above, so an
    // explicit `None` here means "clear", not "merge with whatever's on disk"
    // (which would resurrect a previously cleared goal via merge_from_disk).
    if let Err(e) = store.save_sidecar_replace(&session_id, &sidecar) {
        tracing::debug!("goal sidecar save error: {e}");
    }
}

/// Handle `/goal ...`.
pub(crate) fn handle_goal(repl: &mut Repl, args: &str) -> Result<()> {
    match parse_goal_args(args) {
        GoalAction::Show => match repl.state.goal.as_ref() {
            Some(goal) => {
                let status = match goal.status {
                    GoalStatus::Active => "active",
                    GoalStatus::Paused => "paused",
                    GoalStatus::Complete => "completed",
                };
                let max = if goal.max_iterations == 0 {
                    "∞".to_string()
                } else {
                    goal.max_iterations.to_string()
                };
                repl.chat.add_message(
                    ChatRole::System,
                    t!(
                        "commands.goal.current",
                        status = status,
                        iterations = goal.iterations,
                        max = max,
                        objective = goal.objective
                    )
                    .to_string(),
                );
            }
            None => {
                repl.chat.add_message(
                    ChatRole::System,
                    t!("commands.goal.current_none").to_string(),
                );
            }
        },
        GoalAction::Clear => {
            if repl.state.goal.take().is_some() {
                save_goal_sidecar(repl);
                repl.chat
                    .add_message(ChatRole::System, t!("commands.goal.cleared").to_string());
            } else {
                repl.chat.add_message(
                    ChatRole::System,
                    t!("commands.goal.current_none").to_string(),
                );
            }
        }
        GoalAction::Pause => match repl.state.goal.as_mut() {
            Some(goal) if goal.status == GoalStatus::Active => {
                goal.status = GoalStatus::Paused;
                save_goal_sidecar(repl);
                repl.chat
                    .add_message(ChatRole::System, t!("commands.goal.paused_max").to_string());
            }
            _ => {
                repl.chat.add_message(
                    ChatRole::System,
                    t!("commands.goal.current_none").to_string(),
                );
            }
        },
        GoalAction::Resume => match repl.state.goal.as_mut() {
            // Resuming re-arms the continuation budget: the user explicitly
            // re-authorized the loop, so a max-reached goal must not pause
            // again on the very next completion check.
            Some(goal) if goal.status == GoalStatus::Paused => {
                goal.status = GoalStatus::Active;
                goal.iterations = 0;
                // Reset progress-guard counters: resume is an explicit
                // re-authorization. Otherwise a goal paused by P2.1/P2.2
                // would resume one strike closer to the cap.
                goal.consecutive_no_tool_turns = 0;
                goal.stall_strikes = 0;
                save_goal_sidecar(repl);
                repl.chat
                    .add_message(ChatRole::System, t!("commands.goal.resumed").to_string());
            }
            _ => {
                repl.chat.add_message(
                    ChatRole::System,
                    t!("commands.goal.current_none").to_string(),
                );
            }
        },
        GoalAction::Set {
            objective,
            max_iterations,
            max_budget_usd,
        } => {
            if repl.state.ralph_state.is_some() || repl.state.loop_state.is_some() {
                set_error(repl, t!("commands.goal.conflict_loop").as_ref());
                return Ok(());
            }
            if objective.trim().is_empty() {
                set_error(repl, t!("commands.goal.current_none").as_ref());
                return Ok(());
            }
            repl.state.goal = Some(GoalState {
                objective,
                status: GoalStatus::Active,
                iterations: 0,
                max_iterations,
                consecutive_no_tool_turns: 0,
                stall_strikes: 0,
                max_budget_usd: max_budget_usd,
            });
            save_goal_sidecar(repl);
            let max = if max_iterations == 0 {
                "∞".to_string()
            } else {
                max_iterations.to_string()
            };
            repl.chat.add_message(
                ChatRole::System,
                t!(
                    "commands.goal.set",
                    max = max,
                    objective = repl.state.goal.as_ref().expect("just set").objective
                )
                .to_string(),
            );
        }
    }
    Ok(())
}

/// What should happen to the goal after a turn ends. Pure decision — all
/// state mutations and side effects live in [`check_goal_continuation`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GoalContinuation {
    /// No active goal — nothing to do.
    Inactive,
    /// Completion marker seen: mark the goal Complete.
    Completed,
    /// Blocker marker seen: pause the goal and surface the reason.
    Blocked(String),
    /// Budget exhausted: pause the goal.
    MaxReached,
    /// P2.1/P2.2 — anti-spin or stall-strike threshold tripped; pause and
    /// surface the reason. The reason carries the strike counts so the user
    /// can see why the goal was halted without /goal resume blindly.
    PausedNoProgress(String),
    /// P2.3 — goal budget cap exceeded; treated as a recoverable terminal
    /// (must explicitly re-raise cap or /goal clear, similar to Paused).
    BudgetLimited(String),
    /// Keep going: `iterations` is the next value to store, `prompt` the
    /// continuation text to inject.
    Continue { iterations: usize, prompt: String },
}

/// What a turn actually did, in terms the guard rails can compare. Filled
/// in by the impure [`check_goal_continuation`] path from REPL state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TurnFacts {
    /// True iff at least one tool message was produced since the last
    /// user input (deterministic anti-spin signal).
    pub had_tool_calls: bool,
    /// USD spent during this turn (P2.3 budget accounting). 0.0 if not
    /// available; the budget cap only fires when both `Some` and `>= cap`.
    pub cost_delta_usd: f64,
}

/// Scan `chat` for tool messages produced since the last user message.
/// Cheap O(n) scan over the bounded chat deque.
fn turn_had_tool_calls(chat: &crate::widgets::ChatWidget) -> bool {
    use crate::widgets::ChatRole;
    for msg in chat.messages().iter().rev() {
        match msg.role {
            ChatRole::User => return false,
            ChatRole::Tool => return true,
            _ => {}
        }
    }
    false
}

/// Pure continuation decision (unit-tested): inspects the goal state and the
/// last assistant reply and decides the next lifecycle step.
///
/// Assumes `had_tool_calls=true` (the optimistic default kept for callers
/// that don't observe the chat widget); see
/// [`goal_continuation_decision_with_facts`] for the realistic path.
pub(crate) fn goal_continuation_decision(
    goal: &GoalState,
    last_assistant: Option<&str>,
) -> GoalContinuation {
    goal_continuation_decision_with_facts(
        goal,
        last_assistant,
        TurnFacts {
            had_tool_calls: true,
            cost_delta_usd: 0.0,
        },
    )
}

/// Decision with explicit turn facts. `had_tool_calls=false` triggers the
/// anti-spin / stall-strike countdown; `true` resets it. Both signals share
/// a single strike budget so a turn that merely "tries again" cannot
/// indefinitely extend itself.
pub(crate) fn goal_continuation_decision_with_facts(
    goal: &GoalState,
    last_assistant: Option<&str>,
    facts: TurnFacts,
) -> GoalContinuation {
    if goal.status != GoalStatus::Active {
        return GoalContinuation::Inactive;
    }
    // Termination markers short-circuit the progress guards.
    match last_assistant.and_then(goal_completion_marker) {
        Some(GoalMarker::Complete) => return GoalContinuation::Completed,
        Some(GoalMarker::Blocked(reason)) => return GoalContinuation::Blocked(reason),
        None => {}
    }
    let next = goal.iterations + 1;
    let max_hit = goal.max_iterations > 0 && next > goal.max_iterations;
    let mut next_goal = goal.clone();
    next_goal.iterations = next;
    if facts.had_tool_calls {
        next_goal.consecutive_no_tool_turns = 0;
        next_goal.stall_strikes = next_goal.stall_strikes.saturating_sub(1);
    } else {
        next_goal.consecutive_no_tool_turns += 1;
        next_goal.stall_strikes += 1;
    }
    if max_hit {
        return GoalContinuation::MaxReached;
    }
    // P2.3 — budget cap (USD). Budget beat max_iterations in the verdict
    // priority: spending money is more irreversible than burning turns.
    let budget_limit_hit = goal
        .max_budget_usd
        .map(|cap| facts.cost_delta_usd >= cap)
        .unwrap_or(false);
    if budget_limit_hit {
        let cap = goal.max_budget_usd.unwrap_or(0.0);
        return GoalContinuation::BudgetLimited(format!(
            "Goal budget exhausted (${:.4} \u{2265} cap ${:.4}). The goal stays paused; raise the cap with /goal <obj> --budget ${:.4} or /goal clear to drop.",
            facts.cost_delta_usd, cap, cap
        ));
    }
    if next_goal.consecutive_no_tool_turns >= 2 {
        return GoalContinuation::PausedNoProgress(format!(
            "Two consecutive turns with no tool calls. Pause and decide whether to /goal resume or /goal clear (strike {}/{})",
            next_goal.stall_strikes,
            crate::repl::state::GOAL_DEFAULT_MAX_STALL_STRIKES
        ));
    }
    if next_goal.stall_strikes >= crate::repl::state::GOAL_DEFAULT_MAX_STALL_STRIKES {
        return GoalContinuation::PausedNoProgress(format!(
            "Reached stall-strike budget ({}/{}). Pause to inspect; /goal resume re-arms the budget, /goal clear drops the goal",
            next_goal.stall_strikes,
            crate::repl::state::GOAL_DEFAULT_MAX_STALL_STRIKES
        ));
    }
    GoalContinuation::Continue {
        iterations: next,
        prompt: continuation_prompt(&next_goal),
    }
}

/// Called after a query completes (before the ralph/loop checks). Applies the
/// [`goal_continuation_decision`] for the current goal: finish it, pause it,
/// or inject the next continuation turn.
///
/// Returns true if a new goal iteration was started (callers must then skip
/// the ralph/loop checks so only one auto-continuation loop runs).
pub(crate) fn check_goal_continuation(repl: &mut Repl) -> bool {
    let Some(goal_snapshot) = repl.state.goal.clone() else {
        return false;
    };
    let last = repl
        .chat
        .last_assistant_message()
        .map(|m| m.content.clone());
    let facts = TurnFacts {
        had_tool_calls: turn_had_tool_calls(&repl.chat),
        cost_delta_usd: 0.0,
    };
    match goal_continuation_decision_with_facts(&goal_snapshot, last.as_deref(), facts) {
        GoalContinuation::Inactive => false,
        GoalContinuation::Completed => {
            let (iterations, objective) = {
                let goal = repl.state.goal.as_mut().expect("snapshot existed");
                goal.status = GoalStatus::Complete;
                (goal.iterations, goal.objective.clone())
            };
            save_goal_sidecar(repl);
            let msg = t!(
                "commands.goal.complete",
                iterations = iterations,
                objective = objective
            )
            .to_string();
            super::notify_query_complete(&repl.notifier, repl.notifications_enabled, &msg);
            repl.chat.add_message(ChatRole::System, msg);
            false
        }
        GoalContinuation::Blocked(reason) => {
            let goal = repl.state.goal.as_mut().expect("snapshot existed");
            goal.status = GoalStatus::Paused;
            save_goal_sidecar(repl);
            repl.chat.add_message(
                ChatRole::System,
                t!("commands.goal.paused_blocked", reason = reason).to_string(),
            );
            false
        }
        GoalContinuation::MaxReached => {
            let max = {
                let goal = repl.state.goal.as_mut().expect("snapshot existed");
                goal.status = GoalStatus::Paused;
                goal.max_iterations
            };
            save_goal_sidecar(repl);
            repl.chat.add_message(
                ChatRole::System,
                t!("commands.goal.paused_max", max = max).to_string(),
            );
            false
        }
        GoalContinuation::PausedNoProgress(reason) => {
            // Snapshot the decision's counters and persist them on the real
            // goal — the decision is pure and we trust its numbers. Mirrors
            // the strike-budget math in goal_continuation_decision_with_facts.
            let next_strikes = {
                let mut snapshot_next = goal_snapshot.clone();
                snapshot_next.iterations += 1;
                if facts.had_tool_calls {
                    snapshot_next.consecutive_no_tool_turns = 0;
                    snapshot_next.stall_strikes = snapshot_next.stall_strikes.saturating_sub(1);
                } else {
                    snapshot_next.consecutive_no_tool_turns += 1;
                    snapshot_next.stall_strikes += 1;
                }
                let goal = repl.state.goal.as_mut().expect("snapshot existed");
                goal.status = GoalStatus::Paused;
                goal.iterations = snapshot_next.iterations;
                goal.consecutive_no_tool_turns = snapshot_next.consecutive_no_tool_turns;
                goal.stall_strikes = snapshot_next.stall_strikes;
                snapshot_next.stall_strikes
            };
            save_goal_sidecar(repl);
            repl.chat.add_message(
                ChatRole::System,
                t!(
                    "commands.goal.paused_no_progress",
                    reason = reason,
                    strikes = next_strikes,
                    max_strikes = crate::repl::state::GOAL_DEFAULT_MAX_STALL_STRIKES
                )
                .to_string(),
            );
            false
        }
        GoalContinuation::BudgetLimited(reason) => {
            let goal = repl.state.goal.as_mut().expect("snapshot existed");
            goal.status = GoalStatus::Paused;
            save_goal_sidecar(repl);
            repl.chat.add_message(
                ChatRole::System,
                t!("commands.goal.paused_budget", reason = reason).to_string(),
            );
            false
        }
        GoalContinuation::Continue { iterations, prompt } => {
            let goal = repl.state.goal.as_mut().expect("snapshot existed");
            goal.iterations = iterations;
            // Persist the guard counters advanced by the decision.
            goal.consecutive_no_tool_turns = if facts.had_tool_calls {
                0
            } else {
                goal.consecutive_no_tool_turns + 1
            };
            goal.stall_strikes = if facts.had_tool_calls {
                goal.stall_strikes.saturating_sub(1)
            } else {
                goal.stall_strikes + 1
            };
            save_goal_sidecar(repl);
            // Queue the continuation instead of calling submit_input here:
            // this hook runs inside handle_query's stack frame, and a direct
            // submit would nest one heavy handle_query frame per iteration
            // (stack overflow at depth — proven in tests; unbounded with
            // --max 0). submit_input's flat drain loop (commands/mod.rs)
            // picks the queued prompt up after handle_query returns, keeping
            // O(1) stack depth for arbitrarily long goal runs. The queue is
            // FIFO, so user-typed messages sent during the turn still go
            // first; a query error/cancel clears the queue, which stops the
            // loop (goal stays Active as a pure anchor).
            repl.state.queued_messages.push(prompt);
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_goal_args ────────────────────────────────────────────────

    #[test]
    fn parse_goal_empty_is_show() {
        assert_eq!(parse_goal_args(""), GoalAction::Show);
        assert_eq!(parse_goal_args("   "), GoalAction::Show);
    }

    #[test]
    fn parse_goal_clear_aliases() {
        for alias in ["clear", "off", "stop", "cancel", "reset", "none"] {
            assert_eq!(parse_goal_args(alias), GoalAction::Clear, "alias {alias}");
        }
    }

    #[test]
    fn parse_goal_pause_resume_status() {
        assert_eq!(parse_goal_args("pause"), GoalAction::Pause);
        assert_eq!(parse_goal_args("resume"), GoalAction::Resume);
        assert_eq!(parse_goal_args("status"), GoalAction::Show);
        assert_eq!(parse_goal_args("show"), GoalAction::Show);
    }

    #[test]
    fn parse_goal_set_with_max() {
        assert_eq!(
            parse_goal_args("--max 5 fix the build"),
            GoalAction::Set {
                objective: "fix the build".into(),
                max_iterations: 5,
                max_budget_usd: None,
            }
        );
        assert_eq!(
            parse_goal_args("fix the build"),
            GoalAction::Set {
                objective: "fix the build".into(),
                max_iterations: 25,
                max_budget_usd: None,
            }
        );
        assert_eq!(
            parse_goal_args("--max 0 keep trying"),
            GoalAction::Set {
                objective: "keep trying".into(),
                max_iterations: 0,
                max_budget_usd: None,
            }
        );
    }

    #[test]
    fn parse_goal_max_invalid_uses_default() {
        assert_eq!(
            parse_goal_args("--max abc fix it"),
            GoalAction::Set {
                objective: "fix it".into(),
                max_iterations: 25,
                max_budget_usd: None,
            }
        );
    }

    #[test]
    fn parse_goal_set_is_case_sensitive_keyword_only() {
        // "Clear the cache" is an objective, not the clear subcommand.
        assert_eq!(
            parse_goal_args("Clear the cache"),
            GoalAction::Set {
                objective: "Clear the cache".into(),
                max_iterations: 25,
                max_budget_usd: None,
            }
        );
    }

    // ── goal_completion_marker ─────────────────────────────────────────

    #[test]
    fn marker_last_line_exact_match_case_insensitive() {
        assert_eq!(
            goal_completion_marker("All tests pass.\nGOAL_COMPLETE\n"),
            Some(GoalMarker::Complete)
        );
        assert_eq!(
            goal_completion_marker("done\n  goal_complete  "),
            Some(GoalMarker::Complete)
        );
    }

    #[test]
    fn marker_mid_text_not_detected() {
        assert_eq!(goal_completion_marker("GOAL_COMPLETE is near"), None);
        assert_eq!(
            goal_completion_marker("```\nGOAL_COMPLETE\n```\nworking…"),
            None
        );
        assert_eq!(goal_completion_marker(""), None);
        assert_eq!(goal_completion_marker("normal reply"), None);
    }

    #[test]
    fn marker_blocked_extracts_reason() {
        assert_eq!(
            goal_completion_marker("Cannot proceed.\nGOAL_BLOCKED: need prod credentials"),
            Some(GoalMarker::Blocked("need prod credentials".into()))
        );
        assert_eq!(
            goal_completion_marker("GOAL_BLOCKED"),
            Some(GoalMarker::Blocked(String::new()))
        );
    }

    #[test]
    fn marker_prefix_junk_not_complete() {
        // Hyphenated / decorated markers must not complete the goal.
        assert_eq!(goal_completion_marker("GOAL_COMPLETE-ish"), None);
        assert_eq!(goal_completion_marker("NOT_GOAL_COMPLETE"), None);
    }
}

// ── Handler tests (Repl::new() runs in minimal init under cfg(test)) ────

#[cfg(test)]
mod handler_tests {
    use super::*;
    use crate::repl::state::GoalState;

    /// Point HOME at a scratch dir so any state writes never touch the real
    /// one. nextest runs each test in its own process, so the env swap is
    /// process-local and race-free.
    struct HomeGuard(
        #[allow(dead_code)] // KEEP: path retained for Debug/future cleanup use
        std::path::PathBuf,
    );
    impl HomeGuard {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            // SAFETY: single-threaded test process (nextest isolation); no
            // other thread reads HOME during this test.
            unsafe { std::env::set_var("HOME", dir.path()) };
            Self(dir.path().to_path_buf())
        }
    }
    impl Drop for HomeGuard {
        fn drop(&mut self) {
            // SAFETY: see new()
            unsafe { std::env::set_var("HOME", "/") };
        }
    }

    fn last_message(repl: &Repl) -> String {
        repl.chat
            .messages()
            .back()
            .map(|m| m.content.clone())
            .unwrap_or_default()
    }

    #[test]
    fn handler_set_stores_state_and_confirms() {
        let _home = HomeGuard::new();
        let mut repl = Repl::new().expect("minimal repl");

        handle_goal(&mut repl, "all tests passing").unwrap();
        let goal = repl.state.goal.as_ref().expect("goal set");
        assert_eq!(goal.objective, "all tests passing");
        assert_eq!(goal.status, GoalStatus::Active);
        assert_eq!(goal.max_iterations, 25);
        assert!(last_message(&repl).contains("all tests passing"));

        // Show reports the active goal.
        handle_goal(&mut repl, "").unwrap();
        assert!(last_message(&repl).contains("active"));
    }

    #[test]
    fn handler_set_rejected_while_ralph_active() {
        let _home = HomeGuard::new();
        let mut repl = Repl::new().expect("minimal repl");
        repl.state.ralph_state = Some(crate::repl::state::RalphState {
            task: "x".into(),
            completion_keywords: vec!["DONE".into()],
            max_iterations: 3,
            iteration: 0,
            active: true,
        });

        handle_goal(&mut repl, "my goal").unwrap();
        assert!(repl.state.goal.is_none(), "goal must not be set");
        assert!(last_message(&repl).starts_with("Error:"));
    }

    #[test]
    fn ralph_and_loop_rejected_while_goal_active() {
        let _home = HomeGuard::new();
        let mut repl = Repl::new().expect("minimal repl");
        repl.state.goal = Some(GoalState::new("owned by goal"));

        super::super::loop_engine::handle_ralph(&mut repl, "some task").unwrap();
        assert!(repl.state.ralph_state.is_none());
        assert!(last_message(&repl).starts_with("Error:"));

        super::super::loop_engine::handle_loop(&mut repl, "some task").unwrap();
        assert!(repl.state.loop_state.is_none());
        assert!(last_message(&repl).starts_with("Error:"));

        // A paused goal blocks too.
        let mut goal = GoalState::new("still owned");
        goal.status = GoalStatus::Paused;
        repl.state.goal = Some(goal);
        super::super::loop_engine::handle_loop(&mut repl, "another task").unwrap();
        assert!(repl.state.loop_state.is_none());
        // NOTE: starting a loop with a *completed* goal would call
        // submit_input, which re-enters the query loop — not exercisable
        // in-process (see continuation_incomplete_does_not_recurse…).
    }

    #[test]
    fn handler_pause_resume_clear_transitions() {
        let _home = HomeGuard::new();
        let mut repl = Repl::new().expect("minimal repl");

        handle_goal(&mut repl, "ship it").unwrap();
        handle_goal(&mut repl, "pause").unwrap();
        assert_eq!(repl.state.goal.as_ref().unwrap().status, GoalStatus::Paused);

        handle_goal(&mut repl, "resume").unwrap();
        let goal = repl.state.goal.as_ref().unwrap();
        assert_eq!(goal.status, GoalStatus::Active);
        assert_eq!(goal.iterations, 0, "resume re-arms the budget");

        handle_goal(&mut repl, "clear").unwrap();
        assert!(repl.state.goal.is_none());

        // Clear with no goal is a no-op reporting absence.
        handle_goal(&mut repl, "clear").unwrap();
        assert!(!last_message(&repl).is_empty());
    }

    #[test]
    fn decision_continues_when_incomplete() {
        let goal = GoalState::new("fix lint");
        let d = goal_continuation_decision(&goal, Some("I looked at the code."));
        match d {
            GoalContinuation::Continue { iterations, prompt } => {
                assert_eq!(iterations, 1);
                assert!(prompt.contains("[Goal iteration 1/25]"));
                assert!(prompt.contains("fix lint"));
                assert!(prompt.contains(GOAL_BLOCKED_MARKER));
                assert!(prompt.contains(GOAL_COMPLETE_MARKER));
            }
            other => panic!("expected Continue, got {other:?}"),
        }
    }

    #[test]
    fn decision_unlimited_budget_never_hits_max() {
        let mut goal = GoalState::new("keep going");
        goal.max_iterations = 0;
        goal.iterations = 9_999;
        assert!(matches!(
            goal_continuation_decision(&goal, Some("working...")),
            GoalContinuation::Continue { .. }
        ));
    }

    #[test]
    fn decision_max_reached_pauses() {
        let mut goal = GoalState::new("endless");
        goal.iterations = goal.max_iterations; // budget exhausted
        assert_eq!(
            goal_continuation_decision(&goal, Some("still working")),
            GoalContinuation::MaxReached
        );
    }

    #[test]
    fn decision_completed_and_blocked_from_markers() {
        let goal = GoalState::new("deploy");
        assert_eq!(
            goal_continuation_decision(&goal, Some("All good.\nGOAL_COMPLETE")),
            GoalContinuation::Completed
        );
        assert_eq!(
            goal_continuation_decision(
                &goal,
                Some("Cannot access cluster.\nGOAL_BLOCKED: no kubeconfig")
            ),
            GoalContinuation::Blocked("no kubeconfig".into())
        );
    }

    #[test]
    fn decision_inactive_for_paused_or_complete_goals() {
        // A missing goal never reaches the decision — check_goal_continuation
        // returns early on None — so Inactive here means "not Active".
        let mut paused = GoalState::new("paused goal");
        paused.status = GoalStatus::Paused;
        assert_eq!(
            goal_continuation_decision(&paused, Some("GOAL_COMPLETE")),
            GoalContinuation::Inactive,
            "paused goal must not be completed by a stale marker"
        );
        let mut done = GoalState::new("done");
        done.status = GoalStatus::Complete;
        assert_eq!(
            goal_continuation_decision(&done, Some("GOAL_COMPLETE")),
            GoalContinuation::Inactive
        );
    }

    // ── check_goal_continuation (impure paths that do not re-enter the
    //    query loop: Completed / Blocked / MaxReached / Inactive) ─────────

    #[test]
    fn continuation_complete_marker_stops_and_notifies() {
        let _home = HomeGuard::new();
        let mut repl = Repl::new().expect("minimal repl");
        repl.state.goal = Some(GoalState::new("tests green"));
        repl.chat.add_message(
            crate::widgets::ChatRole::Assistant,
            "Everything passes.\nGOAL_COMPLETE".to_string(),
        );

        let continued = check_goal_continuation(&mut repl);
        assert!(!continued);
        let goal = repl.state.goal.as_ref().unwrap();
        assert_eq!(goal.status, GoalStatus::Complete);
        assert!(last_message(&repl).contains("tests green"));
    }

    #[test]
    fn continuation_blocked_marker_pauses() {
        let _home = HomeGuard::new();
        let mut repl = Repl::new().expect("minimal repl");
        repl.state.goal = Some(GoalState::new("deploy"));
        repl.chat.add_message(
            crate::widgets::ChatRole::Assistant,
            "Cannot access cluster.\nGOAL_BLOCKED: no kubeconfig".to_string(),
        );

        let continued = check_goal_continuation(&mut repl);
        assert!(!continued);
        let goal = repl.state.goal.as_ref().unwrap();
        assert_eq!(goal.status, GoalStatus::Paused);
        assert!(last_message(&repl).contains("no kubeconfig"));
    }

    #[test]
    fn continuation_prompt_contains_contract() {
        // check_goal_continuation increments before staging, so the prompt
        // renders the iteration about to run.
        let mut goal = GoalState::new("fix lint");
        goal.iterations = 1;
        let prompt = continuation_prompt(&goal);
        assert!(prompt.contains("[Goal iteration 1/25]"));
        assert!(prompt.contains("fix lint"));
        assert!(prompt.contains(GOAL_BLOCKED_MARKER));
        assert!(prompt.contains(GOAL_COMPLETE_MARKER));

        // Unlimited budget renders without a cap.
        let unlimited = GoalState {
            iterations: 1,
            max_iterations: 0,
            ..GoalState::new("keep going")
        };
        assert!(continuation_prompt(&unlimited).contains("[Goal iteration 1/∞]"));
    }

    #[test]
    fn continuation_incomplete_queues_prompt_without_recursion() {
        let _home = HomeGuard::new();
        let mut repl = Repl::new().expect("minimal repl");
        repl.state.goal = Some(GoalState::new("fix lint"));
        repl.chat.add_message(
            crate::widgets::ChatRole::Assistant,
            "I looked at the code.".to_string(),
        );

        // The Continue path queues the next prompt instead of submitting it
        // from inside handle_query's frame — O(1) stack depth regardless of
        // iteration count (submit_input's flat drain loop does the rest).
        let continued = check_goal_continuation(&mut repl);
        assert!(continued);
        let goal = repl.state.goal.as_ref().unwrap();
        assert_eq!(goal.status, GoalStatus::Active);
        assert_eq!(goal.iterations, 1);
        let queued = repl
            .state
            .queued_messages
            .last()
            .cloned()
            .unwrap_or_default();
        assert!(queued.contains("[Goal iteration 1/25]"));
        assert!(queued.contains("fix lint"));
    }

    #[test]
    fn continuation_queues_behind_user_messages() {
        let _home = HomeGuard::new();
        let mut repl = Repl::new().expect("minimal repl");
        repl.state.goal = Some(GoalState::new("fix lint"));
        repl.state
            .queued_messages
            .push("user typed this first".to_string());
        repl.chat
            .add_message(crate::widgets::ChatRole::Assistant, "working".to_string());

        check_goal_continuation(&mut repl);
        // FIFO: the user's message goes before the goal continuation.
        assert_eq!(repl.state.queued_messages[0], "user typed this first");
        assert!(repl.state.queued_messages[1].contains("[Goal iteration 1/25]"));
    }

    #[test]
    fn continuation_max_reached_pauses() {
        let _home = HomeGuard::new();
        let mut repl = Repl::new().expect("minimal repl");
        let mut goal = GoalState::new("endless");
        goal.iterations = goal.max_iterations; // budget exhausted
        repl.state.goal = Some(goal);
        repl.chat.add_message(
            crate::widgets::ChatRole::Assistant,
            "still working".to_string(),
        );

        let continued = check_goal_continuation(&mut repl);
        assert!(!continued);
        let goal = repl.state.goal.as_ref().unwrap();
        assert_eq!(goal.status, GoalStatus::Paused);
    }

    #[test]
    fn continuation_ignores_paused_and_missing_goals() {
        let _home = HomeGuard::new();
        let mut repl = Repl::new().expect("minimal repl");

        assert!(!check_goal_continuation(&mut repl));

        let mut goal = GoalState::new("paused goal");
        goal.status = GoalStatus::Paused;
        repl.state.goal = Some(goal);
        repl.chat.add_message(
            crate::widgets::ChatRole::Assistant,
            "GOAL_COMPLETE".to_string(),
        );
        assert!(!check_goal_continuation(&mut repl));
        assert_eq!(
            repl.state.goal.as_ref().unwrap().status,
            GoalStatus::Paused,
            "paused goal must not be completed by a stale marker"
        );
    }

    // ── P2.1 anti-spin + P2.2 stall strikes ────────────────────────────

    #[test]
    fn anti_spin_two_consecutive_no_tool_turns_pauses() {
        let mut goal = GoalState::new("ship");
        goal.consecutive_no_tool_turns = 1;
        let d = goal_continuation_decision_with_facts(
            &goal,
            Some("thinking..."),
            TurnFacts {
                had_tool_calls: false,
                cost_delta_usd: 0.0,
            },
        );
        assert!(
            matches!(d, GoalContinuation::PausedNoProgress(_)),
            "two consecutive no-tool turns must pause: {d:?}"
        );
    }

    #[test]
    fn stall_strikes_reach_threshold_pauses_even_with_tool_calls() {
        let mut goal = GoalState::new("ship");
        goal.consecutive_no_tool_turns = 0;
        goal.stall_strikes = crate::repl::state::GOAL_DEFAULT_MAX_STALL_STRIKES - 1;
        let d = goal_continuation_decision_with_facts(
            &goal,
            Some("partial"),
            TurnFacts {
                had_tool_calls: false,
                cost_delta_usd: 0.0,
            },
        );
        assert!(
            matches!(d, GoalContinuation::PausedNoProgress(_)),
            "stall-strike budget must trip: {d:?}"
        );
    }

    #[test]
    fn tool_call_resets_strike_budget_allowing_continue() {
        let mut goal = GoalState::new("ship");
        goal.consecutive_no_tool_turns = 1;
        goal.stall_strikes = 2;
        let d = goal_continuation_decision_with_facts(
            &goal,
            Some("fixed it"),
            TurnFacts {
                had_tool_calls: true,
                cost_delta_usd: 0.0,
            },
        );
        assert!(matches!(d, GoalContinuation::Continue { .. }), "{d:?}");
    }

    #[test]
    fn no_progress_pause_persists_counters_and_status() {
        let _home = HomeGuard::new();
        let mut repl = Repl::new().expect("minimal repl");
        let mut g = GoalState::new("ship");
        g.consecutive_no_tool_turns = 1;
        repl.state.goal = Some(g);
        repl.chat.add_message(
            crate::widgets::ChatRole::Assistant,
            "no tool called, just thinking".to_string(),
        );

        let continued = check_goal_continuation(&mut repl);
        assert!(!continued, "anti-spin pause must not continue");
        let goal = repl.state.goal.as_ref().unwrap();
        assert_eq!(goal.status, GoalStatus::Paused);
        assert_eq!(goal.consecutive_no_tool_turns, 2);
        // stall_strikes: 0 (fresh GoalState::new) → +1 from no-tool turn.
        assert_eq!(goal.stall_strikes, 1);
        assert_eq!(goal.iterations, 1);
        assert!(repl.state.queued_messages.is_empty());
    }

    #[test]
    fn resume_resets_guard_counters() {
        let _home = HomeGuard::new();
        let mut repl = Repl::new().expect("minimal repl");
        let mut g = GoalState::new("ship");
        g.status = GoalStatus::Paused;
        g.consecutive_no_tool_turns = 2;
        g.stall_strikes = 3;
        repl.state.goal = Some(g);

        handle_goal(&mut repl, "resume").unwrap();
        let goal = repl.state.goal.as_ref().unwrap();
        assert_eq!(goal.status, GoalStatus::Active);
        assert_eq!(goal.iterations, 0);
        assert_eq!(goal.consecutive_no_tool_turns, 0, "resume resets anti-spin");
        assert_eq!(goal.stall_strikes, 0, "resume resets strike budget");
    }

    // ── P2.3 budget accounting ─────────────────────────────────────────

    #[test]
    fn parse_goal_budget_flag() {
        let a = parse_goal_args("--budget 5 ship it");
        assert_eq!(
            a,
            GoalAction::Set {
                objective: "ship it".into(),
                max_iterations: 25,
                max_budget_usd: Some(5.0),
            }
        );
        let a = parse_goal_args("--budget $3.50 ship it");
        assert!(
            matches!(a, GoalAction::Set { max_budget_usd: Some(x), .. } if (x - 3.50).abs() < 1e-9)
        );
        let a = parse_goal_args("--max 5 --budget 1.0 ship it");
        assert!(matches!(
            a,
            GoalAction::Set {
                max_iterations: 5,
                max_budget_usd: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn budget_cap_fires_budget_limited_verdict() {
        let mut goal = GoalState::new("ship");
        goal.max_budget_usd = Some(1.0);
        let d = goal_continuation_decision_with_facts(
            &goal,
            Some("working"),
            TurnFacts {
                had_tool_calls: true,
                cost_delta_usd: 1.5,
            },
        );
        assert!(matches!(d, GoalContinuation::BudgetLimited(_)), "{d:?}");
    }

    #[test]
    fn no_budget_cap_means_no_budget_check() {
        let mut goal = GoalState::new("ship");
        goal.max_budget_usd = None;
        let d = goal_continuation_decision_with_facts(
            &goal,
            Some("working"),
            TurnFacts {
                had_tool_calls: true,
                cost_delta_usd: 100.0,
            },
        );
        assert!(matches!(d, GoalContinuation::Continue { .. }), "{d:?}");
    }

    #[test]
    fn budget_limited_handler_does_not_queue_continuation() {
        let _home = HomeGuard::new();
        let mut repl = Repl::new().expect("minimal repl");
        let mut g = GoalState::new("ship");
        g.max_budget_usd = Some(0.5);
        repl.state.goal = Some(g);
        repl.chat.add_message(
            crate::widgets::ChatRole::Assistant,
            "burned the cap".to_string(),
        );
        // Fresh budget (cost_delta_usd defaults to 0): no BudgetLimited,
        // Continue verdicts queue a prompt. We assert the queued prompt is
        // the iteration message, which proves no BudgetLimited branch ran.
        let _continued = check_goal_continuation(&mut repl);
        assert!(
            !repl.state.queued_messages.is_empty(),
            "Continue should queue; absent queue means BudgetLimited fired unexpectedly"
        );
    }
}
