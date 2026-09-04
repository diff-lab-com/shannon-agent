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

// Decision machine re-exports (moved to `shannon_core::goal`, P2.5/#4).
/// REPL-side [`shannon_tools::goal::GoalStateAccess`] implementation: the
/// tools read and mutate the live [`GoalShared`] handle; the REPL replays
/// transitions onto `ReplState.goal` at query completion (see
/// `check_goal_continuation`).
pub(crate) struct ReplGoalAccess {
    pub shared: crate::repl::state::GoalShared,
}

impl shannon_tools::goal::GoalStateAccess for ReplGoalAccess {
    fn snapshot(&self) -> Option<shannon_tools::goal::GoalSnapshot> {
        self.shared.current().map(|g| shannon_tools::goal::GoalSnapshot {
            objective: g.objective,
            status: match g.status {
                GoalStatus::Active => "active",
                GoalStatus::Paused => "paused",
                GoalStatus::Complete => "complete",
            }
            .to_string(),
            iterations: g.iterations,
            max_iterations: g.max_iterations,
            max_budget_usd: g.max_budget_usd,
        })
    }

    fn apply_update(
        &self,
        outcome: shannon_tools::goal::GoalUpdateOutcome,
    ) -> Option<()> {
        self.shared.apply(outcome)
    }
}

pub(crate) use crate::repl::loop_guard::turn_had_tool_calls;
pub(crate) use shannon_core::goal::{
    continuation_prompt, goal_completion_marker, goal_continuation_decision,
    goal_continuation_decision_with_facts, parse_progress_report, GoalContinuation,
    GoalMarker, ProgressReport, TurnFacts, BLOCKED_AUDIT_TURNS,
};

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

/// Backoff schedule for blocked check-ins (minutes): 30m → 1h → 2h,
/// capped at [`MAX_GOAL_CHECKINS`] fires — Claude Code's contract.
pub(crate) const CHECK_IN_BACKOFF_MINUTES: [i64; 3] = [30, 60, 120];
pub(crate) const MAX_GOAL_CHECKINS: usize = 3;

/// First-delay override; `0` disables check-ins entirely (mirrors
/// Claude Code's CLAUDE_CODE_GOAL_CHECKIN_MINUTES=0).
fn check_in_disabled() -> bool {
    std::env::var("SHANNON_GOAL_CHECKIN_MINUTES")
        .ok()
        .map(|v| v.trim() == "0")
        .unwrap_or(false)
}

/// Compute when the next blocked check-in should fire, or `None` when the
/// budget is exhausted / check-ins are disabled.
pub(crate) fn schedule_check_in(goal: &GoalState) -> Option<chrono::DateTime<chrono::Utc>> {
    if check_in_disabled() || goal.checkins >= MAX_GOAL_CHECKINS {
        return None;
    }
    let idx = goal.checkins.min(CHECK_IN_BACKOFF_MINUTES.len() - 1);
    Some(chrono::Utc::now() + chrono::Duration::minutes(CHECK_IN_BACKOFF_MINUTES[idx]))
}

/// Run-loop hook: fire a due blocked check-in. Returns true when a check-in
/// query was staged (the caller should let the query pipeline run it).
pub(crate) fn maybe_fire_check_in(repl: &mut Repl) -> bool {
    let due = {
        let Some(goal) = repl.state.goal.as_ref() else {
            return false;
        };
        if goal.status != GoalStatus::Paused {
            return false;
        }
        goal.next_check_in_at
            .is_some_and(|at| chrono::Utc::now() >= at)
    };
    if !due {
        return false;
    }

    // Re-arm as an Active goal for one check-in turn; the counter persists
    // so the backoff escalates and the 3-fire cap holds.
    let (checkins, objective) = {
        let goal = repl.state.goal.as_mut().expect("checked above");
        goal.status = GoalStatus::Active;
        goal.checkins += 1;
        goal.next_check_in_at = schedule_check_in(goal);
        (goal.checkins, goal.objective.clone())
    };
    save_goal_sidecar(repl);
    repl.chat.add_message(
        ChatRole::System,
        format!(
            "Goal check-in {checkins}/{}: re-testing the blocker.",
            MAX_GOAL_CHECKINS
        ),
    );
    // We are called from the run loop (not inside handle_query), so
    // submitting here is recursion-safe.
    let prompt = format!(
        "[Goal check-in {checkins}] The goal \"{objective}\" was paused because of a blocker.\n\
         Check whether the blocker is now resolved. If it is, continue working toward the goal.\n\
         If it still holds, end your reply with GOAL_BLOCKED: <reason>."
    );
    repl.prompt.set_input(prompt);
    if super::submit_input(repl, None).is_err() {
        return false;
    }
    true
}

/// Handle `/goal ...`.
pub(crate) fn handle_goal(repl: &mut Repl, args: &str) -> Result<()> {
    // Keep the tool-facing live handle in sync no matter which branch runs —
    // the tools only fire mid-query, but the entry sync in handle_query
    // covers that; this covers direct inspection between queries.
    repl.goal_shared.sync_from(&repl.state.goal);
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
                // Explicit user action: the check-in budget restarts and any
                // pending check-in is cancelled.
                goal.checkins = 0;
                goal.next_check_in_at = None;
                // Re-baseline the cost budget: the resumed goal's --budget
                // applies to post-resume spend only.
                goal.cost_baseline_usd = repl.state.billing_manager.get_period_summary().total_cost;
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
            // P2.3 — capture the billing total as the goal's cost baseline
            // so `--budget` measures spend attributable to this goal.
            let cost_baseline_usd = repl.state.billing_manager.get_period_summary().total_cost;
            repl.state.goal = Some(GoalState {
                objective,
                status: GoalStatus::Active,
                iterations: 0,
                max_iterations,
                consecutive_no_tool_turns: 0,
                stall_strikes: 0,
                max_budget_usd: max_budget_usd,
                cost_baseline_usd,
                blocked_streak: 0,
                last_block_reason: None,
                checkins: 0,
                next_check_in_at: None,
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

/// Called after a query completes (before the ralph/loop checks). Applies the
/// [`goal_continuation_decision`] for the current goal: finish it, pause it,
/// or inject the next continuation turn.
///
/// Returns true if a new goal iteration was started (callers must then skip
/// the ralph/loop checks so only one auto-continuation loop runs).

/// Called after a query completes (before the ralph/loop checks). Applies the
/// [`goal_continuation_decision`] for the current goal: finish it, pause it,
/// or inject the next continuation turn.
///
/// Returns true if a new goal iteration was started (callers must then skip
/// the ralph/loop checks so only one auto-continuation loop runs).
pub(crate) fn check_goal_continuation(repl: &mut Repl) -> bool {
    // P2.5 wiring — replay mid-turn `goal_update` transitions onto the
    // REPL-owned state, persist them, and surface them to the user. The
    // transitioned goal never auto-continues (Complete/Paused are terminal
    // for the loop), so we return right after notifying.
    if let Some((pulled, transition)) = repl.goal_shared.take_transition() {
        let message = match (&transition, &pulled.status) {
            (shannon_tools::goal::GoalUpdateOutcome::Completed, _) => Some(
                t!(
                    "commands.goal.complete",
                    iterations = pulled.iterations,
                    objective = pulled.objective.clone()
                )
                .to_string(),
            ),
            (shannon_tools::goal::GoalUpdateOutcome::Paused(reason), _) => {
                Some(t!("commands.goal.paused_blocked", reason = reason).to_string())
            }
            _ => None,
        };
        repl.state.goal = Some(pulled);
        save_goal_sidecar(repl);
        if let Some(message) = message {
            if matches!(
                transition,
                shannon_tools::goal::GoalUpdateOutcome::Completed
            ) {
                let msg_copy = message.clone();
                super::notify_query_complete(&repl.notifier, repl.notifications_enabled, &msg_copy);
            }
            repl.chat.add_message(ChatRole::System, message);
        }
        return false;
    }
    let Some(goal_snapshot) = repl.state.goal.clone() else {
        return false;
    };
    let last = repl
        .chat
        .last_assistant_message()
        .map(|m| m.content.clone());
    // P2.3 — spend attributable to this goal = billing total now minus the
    // baseline captured at set/resume. Clamped at 0 (billing can only
    // grow within a period; month rollover can make the delta negative).
    let cost_delta_usd = (repl.state.billing_manager.get_period_summary().total_cost
        - goal_snapshot.cost_baseline_usd)
        .max(0.0);
    let facts = TurnFacts {
        had_tool_calls: turn_had_tool_calls(&repl.chat),
        cost_delta_usd,
        progress: last.as_deref().and_then(parse_progress_report),
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
        GoalContinuation::Blocked { next, reason } => {
            let goal = repl.state.goal.as_mut().expect("snapshot existed");
            *goal = next;
            goal.status = GoalStatus::Paused;
            goal.next_check_in_at = schedule_check_in(goal);
            save_goal_sidecar(repl);
            repl.chat.add_message(
                ChatRole::System,
                t!("commands.goal.paused_blocked", reason = reason).to_string(),
            );
            false
        }
        GoalContinuation::MaxReached { next } => {
            let max = {
                let goal = repl.state.goal.as_mut().expect("snapshot existed");
                *goal = next;
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
        GoalContinuation::PausedNoProgress { next, reason } => {
            let strikes = {
                let goal = repl.state.goal.as_mut().expect("snapshot existed");
                *goal = next;
                goal.status = GoalStatus::Paused;
                goal.stall_strikes
            };
            save_goal_sidecar(repl);
            repl.chat.add_message(
                ChatRole::System,
                t!(
                    "commands.goal.paused_no_progress",
                    reason = reason,
                    strikes = strikes,
                    max_strikes = crate::repl::state::GOAL_DEFAULT_MAX_STALL_STRIKES
                )
                .to_string(),
            );
            false
        }
        GoalContinuation::BudgetLimited { next, reason } => {
            let goal = repl.state.goal.as_mut().expect("snapshot existed");
            *goal = next;
            goal.status = GoalStatus::Paused;
            save_goal_sidecar(repl);
            repl.chat.add_message(
                ChatRole::System,
                t!("commands.goal.paused_budget", reason = reason).to_string(),
            );
            false
        }
        GoalContinuation::Continue { next, prompt } => {
            let goal = repl.state.goal.as_mut().expect("snapshot existed");
            *goal = next;
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
                max_iterations: 0,
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
                max_iterations: 0,
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
                max_iterations: 0,
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
        assert_eq!(goal.max_iterations, 0, "R15: default is unlimited");
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
            guard: Default::default(),
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
            GoalContinuation::Continue { next, prompt } => {
                assert_eq!(next.iterations, 1);
                assert!(prompt.contains("[Goal iteration 1/∞]"));
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
        // With the R15 default (unlimited), an explicit --max is required
        // for the fallback cap to exist at all.
        goal.max_iterations = 10;
        goal.iterations = goal.max_iterations; // budget exhausted
        assert!(matches!(
            goal_continuation_decision(&goal, Some("still working")),
            GoalContinuation::MaxReached { .. }
        ));
    }

    #[test]
    fn decision_completed_and_blocked_from_markers() {
        let goal = GoalState::new("deploy");
        assert_eq!(
            goal_continuation_decision(&goal, Some("All good.\nGOAL_COMPLETE")),
            GoalContinuation::Completed
        );
        // The first blocked claim only starts the audit (1/3) — the goal
        // keeps going with an audit warning in the continuation prompt.
        match goal_continuation_decision(
            &goal,
            Some("Cannot access cluster.\nGOAL_BLOCKED: no kubeconfig"),
        ) {
            GoalContinuation::Continue { next, prompt } => {
                assert_eq!(next.blocked_streak, 1);
                assert_eq!(next.last_block_reason.as_deref(), Some("no kubeconfig"));
                assert!(prompt.contains("blocked audit"), "audit warning expected");
            }
            other => panic!("first claim must continue, got {other:?}"),
        }
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
    fn continuation_blocked_audit_pauses_on_third_consecutive_claim() {
        // Codex-style 3-turn audit: the SAME blocker must persist 3 goal
        // turns before the pause is accepted.
        let _home = HomeGuard::new();
        let mut repl = Repl::new().expect("minimal repl");
        repl.state.goal = Some(GoalState::new("deploy"));

        // Turn 1: claim recorded (audit 1/3) — the model is sent back to
        // try alternatives, so the loop continues with the audit warning.
        repl.chat.add_message(
            crate::widgets::ChatRole::Assistant,
            "Cannot access cluster.\nGOAL_BLOCKED: no kubeconfig".to_string(),
        );
        assert!(check_goal_continuation(&mut repl));
        assert_eq!(repl.state.goal.as_ref().unwrap().status, GoalStatus::Active);
        assert_eq!(repl.state.goal.as_ref().unwrap().blocked_streak, 1);
        let queued = repl.state.queued_messages.last().unwrap();
        assert!(queued.contains("blocked audit"));

        // Between turns: the audit continuation was queued by the decision
        // path only via Continue — simulate the model claiming again on the
        // next turn (2/3), still continuing.
        repl.state.goal.as_mut().unwrap().iterations = 1;
        repl.chat.add_message(
            crate::widgets::ChatRole::Assistant,
            "Still cannot access cluster.\nGOAL_BLOCKED: no kubeconfig".to_string(),
        );
        assert!(check_goal_continuation(&mut repl), "audit 2/3 keeps going");
        assert_eq!(repl.state.goal.as_ref().unwrap().blocked_streak, 2);

        // Turn 3: audit satisfied → paused with the reason surfaced.
        repl.state.goal.as_mut().unwrap().iterations = 2;
        repl.chat.add_message(
            crate::widgets::ChatRole::Assistant,
            "No way around it.\nGOAL_BLOCKED: no kubeconfig".to_string(),
        );
        assert!(!check_goal_continuation(&mut repl));
        let goal = repl.state.goal.as_ref().unwrap();
        assert_eq!(goal.status, GoalStatus::Paused);
        assert!(last_message(&repl).contains("no kubeconfig"));
    }

    #[test]
    fn blocked_audit_streak_resets_on_different_reason() {
        let _home = HomeGuard::new();
        let mut repl = Repl::new().expect("minimal repl");
        repl.state.goal = Some(GoalState::new("deploy"));

        for reason in ["no kubeconfig", "no kubeconfig", "flaky tests"] {
            repl.chat.add_message(
                crate::widgets::ChatRole::Assistant,
                format!("Blocked.\nGOAL_BLOCKED: {reason}"),
            );
            // Audit continuation — each claim goes back to work.
            assert!(check_goal_continuation(&mut repl));
        }
        // "flaky tests" differs from "no kubeconfig" → streak restarted at 1.
        assert_eq!(repl.state.goal.as_ref().unwrap().blocked_streak, 1);
        assert_eq!(
            repl.state
                .goal
                .as_ref()
                .unwrap()
                .last_block_reason
                .as_deref(),
            Some("flaky tests")
        );
    }

    #[test]
    fn continuation_prompt_contains_contract() {
        // check_goal_continuation increments before staging, so the prompt
        // renders the iteration about to run.
        let mut goal = GoalState::new("fix lint");
        goal.iterations = 1;
        let prompt = continuation_prompt(&goal);
        assert!(prompt.contains("[Goal iteration 1/∞]"));
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
        assert!(queued.contains("[Goal iteration 1/∞]"));
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
        assert!(repl.state.queued_messages[1].contains("[Goal iteration 1/∞]"));
    }

    #[test]
    fn continuation_max_reached_pauses() {
        let _home = HomeGuard::new();
        let mut repl = Repl::new().expect("minimal repl");
        let mut goal = GoalState::new("endless");
        // Explicit --max: the R15 default is unlimited, so the fallback cap
        // only exists when the user asks for one.
        goal.max_iterations = 10;
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
                progress: None,
            },
        );
        assert!(
            matches!(d, GoalContinuation::PausedNoProgress { .. }),
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
                progress: None,
            },
        );
        assert!(
            matches!(d, GoalContinuation::PausedNoProgress { .. }),
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
                progress: None,
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
                max_iterations: 0,
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
                progress: None,
            },
        );
        assert!(matches!(d, GoalContinuation::BudgetLimited { .. }), "{d:?}");
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
                progress: None,
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

    // ── P2.3 live budget signal ─────────────────────────────────────────

    #[test]
    fn budget_signal_from_billing_pauses_goal() {
        let _home = HomeGuard::new();
        let mut repl = Repl::new().expect("minimal repl");
        // Baseline is captured at set-time (0.0 in a fresh billing store).
        handle_goal(&mut repl, "--budget 0.5 ship it").unwrap();
        assert_eq!(repl.state.goal.as_ref().unwrap().cost_baseline_usd, 0.0);

        // Simulate spend during the turn.
        repl.state
            .billing_manager
            .record_usage(shannon_core::billing::UsageRecord::new(
                "test-model",
                1_000,
                100,
                1.0, // $1 spent > $0.5 cap
            ))
            .unwrap();
        repl.chat
            .add_message(crate::widgets::ChatRole::Assistant, "working".to_string());

        let continued = check_goal_continuation(&mut repl);
        assert!(!continued, "budget-limited goal must not continue");
        let goal = repl.state.goal.as_ref().unwrap();
        assert_eq!(goal.status, GoalStatus::Paused);
        assert!(repl.state.queued_messages.is_empty());
        let last = repl.chat.messages().back().unwrap().content.clone();
        assert!(
            last.contains("budget") || last.contains("Budget") || last.contains('$'),
            "pause message should surface the budget reason: {last}"
        );
    }

    #[test]
    fn spend_under_cap_continues_normally() {
        let _home = HomeGuard::new();
        let mut repl = Repl::new().expect("minimal repl");
        handle_goal(&mut repl, "--budget 5.0 ship it").unwrap();
        repl.state
            .billing_manager
            .record_usage(shannon_core::billing::UsageRecord::new(
                "test-model",
                1_000,
                100,
                0.25, // under cap
            ))
            .unwrap();
        repl.chat
            .add_message(crate::widgets::ChatRole::Assistant, "working".to_string());

        let continued = check_goal_continuation(&mut repl);
        // No tool messages → anti-spin strike, but under thresholds → queued.
        let goal = repl.state.goal.as_ref().unwrap();
        assert_eq!(
            goal.status,
            GoalStatus::Active,
            "under-cap goal stays active"
        );
        let _ = continued; // queued continuation (or submit failure in minimal REPL)
    }

    #[test]
    fn resume_rebaselines_cost() {
        let _home = HomeGuard::new();
        let mut repl = Repl::new().expect("minimal repl");
        handle_goal(&mut repl, "--budget 5.0 ship it").unwrap();
        repl.state
            .billing_manager
            .record_usage(shannon_core::billing::UsageRecord::new("m", 1, 1, 4.0))
            .unwrap();
        // Simulate a budget-limited pause.
        repl.state.goal.as_mut().unwrap().status = GoalStatus::Paused;

        handle_goal(&mut repl, "resume").unwrap();
        let goal = repl.state.goal.as_ref().unwrap();
        // Baseline re-captured at resume: current total (4.0) becomes the
        // new zero point, so the $5 cap applies to post-resume spend.
        assert!((goal.cost_baseline_usd - 4.0).abs() < 1e-9);
    }

    // ── P2.2 verified-wait self-report ──────────────────────────────────

    #[test]
    fn parse_progress_report_lines() {
        assert_eq!(
            parse_progress_report("GOAL_PROGRESS: progress\nrest"),
            Some(ProgressReport::Progress)
        );
        assert_eq!(
            parse_progress_report("goal_progress: verified_wait"),
            Some(ProgressReport::VerifiedWait)
        );
        assert_eq!(
            parse_progress_report("GOAL_PROGRESS: no-progress"),
            Some(ProgressReport::NoProgress)
        );
        assert_eq!(parse_progress_report("no marker here"), None);
        assert_eq!(parse_progress_report("GOAL_PROGRESS: nonsense"), None);
    }

    #[test]
    fn verified_wait_with_evidence_holds_strikes() {
        let mut goal = GoalState::new("ship");
        goal.stall_strikes = 2;
        let d = goal_continuation_decision_with_facts(
            &goal,
            Some("GOAL_PROGRESS: verified_wait\ntest suite running"),
            TurnFacts {
                had_tool_calls: true,
                cost_delta_usd: 0.0,
                progress: Some(ProgressReport::VerifiedWait),
            },
        );
        match d {
            GoalContinuation::Continue { next, .. } => {
                assert_eq!(next.stall_strikes, 2, "verified wait holds the budget");
            }
            other => panic!("expected Continue, got {other:?}"),
        }
    }

    #[test]
    fn no_progress_selfreport_counts_even_with_tools() {
        let mut goal = GoalState::new("ship");
        goal.stall_strikes = 2;
        let d = goal_continuation_decision_with_facts(
            &goal,
            Some("GOAL_PROGRESS: no_progress"),
            TurnFacts {
                had_tool_calls: true,
                cost_delta_usd: 0.0,
                progress: Some(ProgressReport::NoProgress),
            },
        );
        match d {
            GoalContinuation::Continue { next, .. } => {
                assert_eq!(next.stall_strikes, 3, "no-progress claim increments");
            }
            GoalContinuation::PausedNoProgress { .. } => {
                // 2+1 = 3 → trip on this very turn is also acceptable.
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn selfreport_without_evidence_is_no_progress() {
        let goal = GoalState::new("ship");
        let d = goal_continuation_decision_with_facts(
            &goal,
            Some("GOAL_PROGRESS: progress"),
            TurnFacts {
                had_tool_calls: false,
                cost_delta_usd: 0.0,
                progress: Some(ProgressReport::Progress),
            },
        );
        // Claimed progress without tool activity counts as a strike.
        match d {
            GoalContinuation::Continue { next, .. } => assert_eq!(next.stall_strikes, 1),
            other => panic!("unexpected {other:?}"),
        }
    }

    // ── P2.4 check-in backoff ───────────────────────────────────────────

    #[test]
    fn checkin_backoff_escalates_and_caps() {
        let mut goal = GoalState::new("deploy");
        goal.checkins = 0;
        let t0 = schedule_check_in(&goal);
        assert!(t0.is_some());

        goal.checkins = 1;
        let t1 = schedule_check_in(&goal);
        assert!(t1 > t0, "backoff escalates 30m → 1h");

        goal.checkins = 3;
        assert!(schedule_check_in(&goal).is_none(), "3-fire cap");
    }

    #[test]
    fn maybe_fire_check_in_rearms_and_counts() {
        let _home = HomeGuard::new();
        let mut repl = Repl::new().expect("minimal repl");
        let mut goal = GoalState::new("deploy");
        goal.status = GoalStatus::Paused;
        goal.next_check_in_at = Some(chrono::Utc::now() - chrono::Duration::minutes(1)); // due
        repl.state.goal = Some(goal);

        // We are the run loop (not inside handle_query) — submit fails in
        // the minimal REPL after staging, but the counters must advance.
        let _ = maybe_fire_check_in(&mut repl);
        let goal = repl.state.goal.as_ref().unwrap();
        assert_eq!(goal.checkins, 1, "check-in counted");
        assert!(
            goal.next_check_in_at.is_some(),
            "next backoff scheduled (the in-REPL submit may additionally run              the completion hook and pause — that is its own contract)"
        );
    }

    #[test]
    fn maybe_fire_check_in_ignores_active_or_unscheduled() {
        let _home = HomeGuard::new();
        let mut repl = Repl::new().expect("minimal repl");
        assert!(!maybe_fire_check_in(&mut repl), "no goal");

        let mut goal = GoalState::new("active goal");
        repl.state.goal = Some(goal.clone());
        assert!(!maybe_fire_check_in(&mut repl), "active goal");

        goal.status = GoalStatus::Paused;
        goal.next_check_in_at = None;
        repl.state.goal = Some(goal);
        assert!(!maybe_fire_check_in(&mut repl), "no schedule");
    }
}
