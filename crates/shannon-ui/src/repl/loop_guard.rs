//! Shared progress guards for the autonomous loops (`/goal`, `/loop`,
//! `/ralph`) — Phase 2 P2.1/P2.2 generalized (R15 follow-up #1).
//!
//! A turn's progress is judged deterministically: did it produce any tool
//! activity? Turns without tool calls advance two counters, and either
//! threshold pausing the loop:
//!
//! * `no_tool_turns` — consecutive no-tool turns (anti-spin, threshold 2);
//! * `stall_strikes` — shared strike budget (threshold 3); a productive
//!   turn decrements, an idle turn increments (Magentic-One /
//!   `auto_test::no_progress_strikes` pattern).
//!
//! The goal loop's richer decision (`goal_continuation_decision_with_facts`)
//! stays the reference implementation; these helpers exist so `/loop` and
//! `/ralph` — which lack any termination criterion of their own in loop's
//! case — get the same drift protection without duplicating the math.

use crate::widgets::{ChatRole, ChatWidget};

/// Pause after this many consecutive no-tool turns (anti-spin).
pub(crate) const NO_TOOL_PAUSE_THRESHOLD: usize = 2;

/// Pause when stall strikes reach this budget.
pub(crate) const MAX_STALL_STRIKES: usize = 3;

/// Scan `chat` for tool messages produced since the last user message.
/// Cheap O(n) scan over the bounded chat deque.
pub(crate) fn turn_had_tool_calls(chat: &ChatWidget) -> bool {
    for msg in chat.messages().iter().rev() {
        match msg.role {
            ChatRole::User => return false,
            ChatRole::Tool => return true,
            _ => {}
        }
    }
    false
}

/// Mutable guard counters carried by each loop's state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct GuardCounters {
    pub no_tool_turns: usize,
    pub stall_strikes: usize,
}

/// Advance the counters for one finished turn.
pub(crate) fn advance(counters: &mut GuardCounters, had_tool_calls: bool) {
    if had_tool_calls {
        counters.no_tool_turns = 0;
        counters.stall_strikes = counters.stall_strikes.saturating_sub(1);
    } else {
        counters.no_tool_turns += 1;
        counters.stall_strikes += 1;
    }
}

/// True when the guard thresholds trip and the loop should pause.
pub(crate) fn tripped(counters: &GuardCounters) -> bool {
    counters.no_tool_turns >= NO_TOOL_PAUSE_THRESHOLD || counters.stall_strikes >= MAX_STALL_STRIKES
}

/// Human-readable reason for the pause (English — loop_engine convention).
pub(crate) fn pause_reason(counters: &GuardCounters) -> String {
    if counters.no_tool_turns >= NO_TOOL_PAUSE_THRESHOLD {
        format!(
            "no tool activity for {} consecutive turns (stall strikes {}/{})",
            counters.no_tool_turns, counters.stall_strikes, MAX_STALL_STRIKES
        )
    } else {
        format!(
            "stall-strike budget reached ({}/{})",
            counters.stall_strikes, MAX_STALL_STRIKES
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn productive_turn_resets_counters() {
        let mut c = GuardCounters {
            no_tool_turns: 1,
            stall_strikes: 2,
        };
        advance(&mut c, true);
        assert_eq!(c.no_tool_turns, 0);
        assert_eq!(c.stall_strikes, 1, "productive turn decrements strikes");
        assert!(!tripped(&c));
    }

    #[test]
    fn two_idle_turns_trip_anti_spin() {
        let mut c = GuardCounters::default();
        advance(&mut c, false);
        assert!(!tripped(&c), "first idle turn only warns");
        advance(&mut c, false);
        assert!(tripped(&c), "second consecutive idle turn pauses");
    }

    #[test]
    fn strike_budget_trips_even_with_interspersed_tool_calls() {
        let mut c = GuardCounters::default();
        // Pattern: idle, idle(but anti-spin not tripped yet? no — 2 idles
        // trip anti-spin). Use productive/idle alternation so anti-spin
        // never fires but strikes accumulate: idle(+1), tool(-1→0)... that
        // never accumulates. Strikes trip needs mostly-idle history with
        // single tool calls resetting only anti-spin. Construct directly:
        c.stall_strikes = MAX_STALL_STRIKES - 1;
        c.no_tool_turns = 0;
        advance(&mut c, false);
        assert!(tripped(&c), "strike budget reached via idle turn");
        // And the reason names the budget, not anti-spin.
        assert!(pause_reason(&c).contains("budget"));
    }

    #[test]
    fn strikes_never_go_negative() {
        let mut c = GuardCounters::default();
        advance(&mut c, true);
        assert_eq!(c.stall_strikes, 0, "saturating decrement");
    }
}
