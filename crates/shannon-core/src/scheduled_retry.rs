//! Failure-retry policy for scheduled routines.
//!
//! Implements exponential backoff with jitter for failed scheduled runs.
//! Backed by the JSONL run history in [`crate::scheduled_runs`]: every
//! retry attempt appends a new run record under the same parent `task_id`,
//! linked via `parent_run_id`, so the history remains a faithful audit log.
//!
//! ## Behaviour
//!
//! When a scheduled routine's [`crate::scheduled_routines::ExecutionPolicy::max_retries`]
//! is > 0, a failed run will be scheduled for retry with an exponentially
//! growing delay: `base_delay_secs * 2^attempt`, capped at `max_delay_secs`.
//! A small uniform jitter (±25%) is applied to spread retries across
//! reconnect attempts and avoid thundering-herd against backing services.
//!
//! Defaults are tuned for transient failure modes (network blip, rate
//! limit, brief API outage) — not for persistent errors (bad credentials,
//! missing model). A retryable error returns
//! [`RetryDecision::Retry`] with the next attempt number and scheduled
//! time; a non-retryable error or an exhausted budget returns
//! [`RetryDecision::GiveUp`].
//!
//! ## Example
//!
//! ```rust,ignore
//! use shannon_core::scheduled_retry::{RetryPolicy, decide_retry, RetryOutcome};
//!
//! let policy = RetryPolicy::default(); // base=2s, max=300s, max_attempts=3
//! let outcome = decide_retry(&policy, 1, &err_msg);
//! match outcome.decision {
//!     RetryDecision::Retry { next_attempt, run_at } => {
//!         // schedule next attempt
//!     }
//!     RetryDecision::GiveUp { reason } => {
//!         // mark task as failed
//!     }
//! }
//! ```

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Default base delay (seconds) for the first retry attempt.
///
/// Small enough to recover quickly from transient blips, large enough
/// to ride out brief rate-limit windows.
pub const DEFAULT_BASE_DELAY_SECS: u64 = 2;

/// Default maximum delay between retries (seconds).
///
/// Caps the exponential growth so a long-lived retry chain doesn't
/// stretch into hours.
pub const DEFAULT_MAX_DELAY_SECS: u64 = 300;

/// Default maximum retry attempts (in addition to the original run).
///
/// Matches the `WebhookHandler` retry budget (3 attempts: original + 2 retries
/// before falling through to a 4th delivery, see v0.5.5 T7).
pub const DEFAULT_MAX_ATTEMPTS: u32 = 3;

/// Jitter ratio applied to the computed delay (±25% by default).
///
/// Uniform random in `(delay * (1 - JITTER), delay * (1 + JITTER))`.
pub const DEFAULT_JITTER_RATIO: f64 = 0.25;

/// Retry policy derived from [`crate::scheduled_routines::ExecutionPolicy`].
///
/// The defaults match a sensible "transient failure" posture. Callers
/// that want aggressive retries (e.g., local cron jobs hitting a flaky
/// webhook) can construct a custom policy with a larger `max_attempts`
/// and `max_delay_secs`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RetryPolicy {
    /// Maximum retry attempts (0 = no retries, 1 = one retry, etc.).
    pub max_attempts: u32,
    /// Base delay in seconds (first retry waits roughly this long).
    pub base_delay_secs: u64,
    /// Maximum delay cap (exponential growth is clamped to this).
    pub max_delay_secs: u64,
    /// Jitter ratio (0.0 = no jitter, 0.25 = ±25%, etc.).
    pub jitter_ratio: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            base_delay_secs: DEFAULT_BASE_DELAY_SECS,
            max_delay_secs: DEFAULT_MAX_DELAY_SECS,
            jitter_ratio: DEFAULT_JITTER_RATIO,
        }
    }
}

impl RetryPolicy {
    /// Construct from [`crate::scheduled_routines::ExecutionPolicy::max_retries`].
    ///
    /// `max_retries == 0` disables retries entirely.
    pub fn from_max_retries(max_retries: u32) -> Self {
        Self {
            max_attempts: max_retries,
            ..Self::default()
        }
    }

    /// Construct an explicit policy (used by tests and CLI overrides).
    pub fn new(max_attempts: u32, base_delay_secs: u64, max_delay_secs: u64) -> Self {
        Self {
            max_attempts,
            base_delay_secs,
            max_delay_secs,
            jitter_ratio: DEFAULT_JITTER_RATIO,
        }
    }

    /// Disable retries entirely (helper for `ExecutionPolicy { max_retries: 0 }`).
    pub fn disabled() -> Self {
        Self {
            max_attempts: 0,
            ..Self::default()
        }
    }

    /// Whether this policy permits at least one retry.
    pub fn allows_retry(&self) -> bool {
        self.max_attempts > 0
    }
}

/// Outcome of a retry decision.
///
/// Either schedule another attempt, or give up.
#[derive(Debug, Clone, PartialEq)]
pub enum RetryDecision {
    /// Schedule another retry attempt.
    Retry {
        /// Attempt number that will run next (1-indexed).
        next_attempt: u32,
        /// Wall-clock time at which the retry should fire.
        run_at: DateTime<Utc>,
    },
    /// Do not retry — either the attempt budget is exhausted, retries are
    /// disabled, or the failure is not retryable.
    GiveUp {
        /// Human-readable reason for giving up (recorded in run history).
        reason: GiveUpReason,
    },
}

/// Reason a retry was abandoned. Recorded in the failed run's
/// `error_message` field so operators can audit why a task stopped firing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GiveUpReason {
    /// Retries are disabled (`max_retries == 0`).
    RetriesDisabled,
    /// Retry attempt budget exhausted.
    AttemptsExhausted,
    /// Underlying error was deemed non-retryable.
    NonRetryableError,
}

/// Decide whether to retry a failed run.
///
/// `current_attempt` is the attempt number that just failed (1 = original run,
/// 2 = first retry, etc.). `error_msg` is the underlying error string and may
/// be inspected for retryability hints.
///
/// The decision is deterministic given `current_attempt` + `policy` + `now`,
/// except for the jitter term which uses [`rand::random`].
pub fn decide_retry(policy: &RetryPolicy, current_attempt: u32, error_msg: &str) -> RetryOutcome {
    if !policy.allows_retry() {
        return RetryOutcome {
            decision: RetryDecision::GiveUp {
                reason: GiveUpReason::RetriesDisabled,
            },
            delay: Duration::ZERO,
        };
    }

    if !is_retryable_error(error_msg) {
        return RetryOutcome {
            decision: RetryDecision::GiveUp {
                reason: GiveUpReason::NonRetryableError,
            },
            delay: Duration::ZERO,
        };
    }

    // current_attempt = 1 means the original run just failed;
    // the next attempt is `current_attempt` (1-indexed retry count).
    if current_attempt >= policy.max_attempts {
        return RetryOutcome {
            decision: RetryDecision::GiveUp {
                reason: GiveUpReason::AttemptsExhausted,
            },
            delay: Duration::ZERO,
        };
    }

    let next_attempt = current_attempt + 1;
    let delay = compute_delay(policy, current_attempt);
    let run_at = Utc::now() + ChronoDuration::seconds(delay.as_secs() as i64);

    RetryOutcome {
        decision: RetryDecision::Retry {
            next_attempt,
            run_at,
        },
        delay,
    }
}

/// Outcome bundle returned by [`decide_retry`].
///
/// `delay` mirrors the delay encoded in `decision` for callers that want
/// to schedule via a duration (e.g., a tokio timer) rather than an
/// absolute timestamp.
#[derive(Debug, Clone, PartialEq)]
pub struct RetryOutcome {
    pub decision: RetryDecision,
    pub delay: Duration,
}

/// Compute the delay (in seconds) before the next retry attempt.
///
/// Uses exponential growth: `base * 2^(current_attempt - 1)`, capped at
/// `max_delay_secs`. Adds uniform jitter in `(1 - jitter_ratio, 1 + jitter_ratio)`
/// to spread attempts across reconnect windows.
///
/// `current_attempt = 1` → first retry waits roughly `base_delay_secs`.
pub fn compute_delay(policy: &RetryPolicy, current_attempt: u32) -> Duration {
    let current = current_attempt.max(1);
    // Saturating shift — 2^32 would overflow on 32-bit usize.
    let shift = current.saturating_sub(1).min(31);
    let raw = (policy.base_delay_secs as u128).saturating_mul(1u128 << shift);
    let raw_u64 = raw.min(u64::MAX as u128) as u64;
    let bounded = raw_u64.min(policy.max_delay_secs);
    apply_jitter(bounded, policy.jitter_ratio)
}

fn apply_jitter(delay_secs: u64, jitter_ratio: f64) -> Duration {
    if jitter_ratio <= 0.0 || delay_secs == 0 {
        return Duration::from_secs(delay_secs);
    }
    // rand::random::<f64>() returns 0.0..1.0 — shift to -jitter..+jitter.
    let n: f64 = rand::random();
    let factor = 1.0 + (n * 2.0 - 1.0) * jitter_ratio;
    let jittered = (delay_secs as f64 * factor).max(0.0);
    Duration::from_secs(jittered as u64)
}

/// Heuristic: is this error message likely transient (and therefore retryable)?
///
/// Conservative — anything we don't recognize as a hard failure is treated
/// as retryable. Operators who need stricter semantics can replace this
/// with a custom matcher.
pub fn is_retryable_error(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    // Hard-fail signatures: auth, malformed input, missing config.
    const HARD_FAIL_MARKERS: &[&str] = &[
        "401",
        "403",
        "unauthorized",
        "forbidden",
        "invalid api key",
        "invalid_request",
        "schema",
        "parse error",
        "malformed",
        "not found",
        "404",
        "schema validation",
    ];
    if HARD_FAIL_MARKERS.iter().any(|m| lower.contains(m)) {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as ChronoDuration;

    fn fast_policy() -> RetryPolicy {
        // Deterministic base/cap/jitter=0 for predictable math in tests.
        RetryPolicy {
            max_attempts: 3,
            base_delay_secs: 10,
            max_delay_secs: 1000,
            jitter_ratio: 0.0,
        }
    }

    #[test]
    fn disabled_policy_always_gives_up() {
        let policy = RetryPolicy::disabled();
        let out = decide_retry(&policy, 1, "anything");
        assert_eq!(
            out.decision,
            RetryDecision::GiveUp {
                reason: GiveUpReason::RetriesDisabled
            }
        );
    }

    #[test]
    fn auth_errors_are_not_retried() {
        let policy = fast_policy();
        let out = decide_retry(&policy, 1, "401 Unauthorized: invalid api key");
        assert_eq!(
            out.decision,
            RetryDecision::GiveUp {
                reason: GiveUpReason::NonRetryableError
            }
        );
    }

    #[test]
    fn exhausted_budget_gives_up() {
        let policy = fast_policy();
        let out = decide_retry(&policy, 3, "transient network blip");
        assert_eq!(
            out.decision,
            RetryDecision::GiveUp {
                reason: GiveUpReason::AttemptsExhausted
            }
        );
    }

    #[test]
    fn first_retry_uses_base_delay() {
        let policy = fast_policy();
        let out = decide_retry(&policy, 1, "transient timeout");
        match out.decision {
            RetryDecision::Retry {
                next_attempt,
                run_at,
            } => {
                assert_eq!(next_attempt, 2);
                let now = Utc::now();
                let delta = run_at - now;
                // Allow a small skew for clock advance during the test.
                assert!(
                    delta >= ChronoDuration::seconds(9) && delta <= ChronoDuration::seconds(11),
                    "expected ~10s delay, got {}s",
                    delta.num_seconds()
                );
            }
            other => panic!("expected Retry, got {:?}", other),
        }
    }

    #[test]
    fn second_retry_doubles_delay() {
        let policy = fast_policy();
        let out = decide_retry(&policy, 2, "still failing");
        assert_eq!(out.delay, Duration::from_secs(20));
    }

    #[test]
    fn delay_is_capped() {
        let policy = RetryPolicy {
            max_attempts: 10,
            base_delay_secs: 10,
            max_delay_secs: 50,
            jitter_ratio: 0.0,
        };
        // attempt 5 would be 10 * 2^4 = 160s, capped to 50s.
        assert_eq!(compute_delay(&policy, 5), Duration::from_secs(50));
    }

    #[test]
    fn jitter_keeps_delay_within_window() {
        let policy = RetryPolicy {
            max_attempts: 5,
            base_delay_secs: 100,
            max_delay_secs: 1000,
            jitter_ratio: 0.25,
        };
        for _ in 0..50 {
            let d = compute_delay(&policy, 1).as_secs();
            assert!((75..=125).contains(&d), "delay {} out of jitter window", d);
        }
    }

    #[test]
    fn from_max_retries_constructs_policy() {
        let policy = RetryPolicy::from_max_retries(5);
        assert_eq!(policy.max_attempts, 5);
        assert!(policy.allows_retry());
    }

    #[test]
    fn retryable_markers_are_treated_as_retryable() {
        assert!(is_retryable_error("connection reset by peer"));
        assert!(is_retryable_error("rate limit exceeded"));
        assert!(is_retryable_error("timeout"));
        assert!(is_retryable_error(""));
    }

    #[test]
    fn non_retryable_markers_block_retry() {
        for m in [
            "401 unauthorized",
            "403 forbidden",
            "invalid api key",
            "schema validation failed",
            "malformed request",
            "404 not found",
            "parse error in config",
        ] {
            assert!(!is_retryable_error(m), "{} should be non-retryable", m);
        }
    }

    #[test]
    fn compute_delay_zero_attempt_falls_back_to_base() {
        let policy = fast_policy();
        // attempt 0 is nonsense but must not panic; clamp to base.
        assert_eq!(compute_delay(&policy, 0), Duration::from_secs(10));
    }
}
