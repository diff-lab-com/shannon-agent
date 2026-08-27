//! # Online signals (§4.15 W2-M4): anonymous aggregate counters + opt-in upload
//!
//! Collects four product usage signals as **aggregate counts only** and — when
//! explicitly opted in — posts them to a user-configured endpoint through the
//! existing [`crate::notifier`] webhook transport. Everything lands locally
//! first (`analytics/counters.jsonl` under the Shannon home), mirroring the
//! DP5 decision: **default off, anonymous, counts only**.
//!
//! ## Data items (the complete list — nothing else is ever recorded)
//!
//! | counter                 | meaning                                                     |
//! |-------------------------|-------------------------------------------------------------|
//! | `feedback_up`           | explicit positive session feedback (`shannon feedback up`)  |
//! | `feedback_down`         | explicit negative session feedback (`shannon feedback down`)|
//! | `turns_ended`           | turns that reached any terminal close (rate denominator)     |
//! | `turns_interrupted`     | turns closed as `interrupted` (interruption-rate numerator) |
//! | `turns_user_takeover`   | turns where the human answered a permission prompt (takeover-rate numerator) |
//! | `permission_prompts`    | permission asks surfaced (`ask` decisions)                  |
//! | `rewind_conversation`   | `/rewind <n>` invocations                                   |
//! | `rewind_code`           | `/rewind code <n>` invocations                              |
//! | `rewind_both`           | `/rewind both <n>` invocations                              |
//! | `rewind_file`           | `/rewind <path>` invocations                                |
//!
//! No conversation content, tool arguments, file paths, repository names, or
//! session ids exist anywhere in the counters, the local file, or the wire
//! payload — enforced by the field-whitelist tests below.
//!
//! ## Switches (all off by default ⇒ zero network traffic)
//!
//! | Variable                    | Effect                                           |
//! |-----------------------------|--------------------------------------------------|
//! | `SHANNON_SIGNALS_UPLOAD`    | `1`/`true` enables outbound posting              |
//! | `SHANNON_SIGNALS_ENDPOINT`  | target URL receiving the JSON payload            |
//! | `SHANNON_SIGNALS_SECRET`    | optional HMAC-SHA256 secret (`X-Shannon-Signature`) |
//!
//! With the switch unset, [`report`] only appends the aggregate snapshot to
//! `<home>/analytics/counters.jsonl`; no HTTP client is ever constructed.
//! An independent counters file (instead of extending the §4.6
//! `project_analytics_jsonl` view) is deliberate: CLI feedback has no session
//! log to project from, and these aggregates span many sessions while L0
//! projections stay pure per-session derivations.
//!
//! ## Observed hook points
//!
//! - turn start/close: [`crate::session_log`] tee (`SessionTee`) — every
//!   terminal close funnels through `close_turn`, including cancellation.
//! - permission decisions: `crate::query_engine::guard_nodes::emit_decision`
//!   — mode `"USER"` marks human-taken decisions.
//! - `/rewind`: REPL command handler (`shannon-ui`, `handle_rewind`).
//! - feedback: `shannon feedback <up|down>` (`shannon-cli`).
//!
//! ```
//! use shannon_core::signals::{self, FeedbackDirection};
//!
//! signals::observe_feedback(FeedbackDirection::Up);
//! let snap = signals::snapshot();
//! assert!(snap.feedback_up >= 1);
//! ```

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

/// Payload schema tag (bump on any shape change).
pub const SIGNALS_SCHEMA: &str = "shannon.signals.v1";
/// Environment toggle enabling outbound upload.
pub const ENV_UPLOAD: &str = "SHANNON_SIGNALS_UPLOAD";
/// Opt-in target URL for outbound upload.
pub const ENV_ENDPOINT: &str = "SHANNON_SIGNALS_ENDPOINT";
/// Optional HMAC-SHA256 signing secret for the target endpoint.
pub const ENV_SECRET: &str = "SHANNON_SIGNALS_SECRET";

// ============================================================================
// Counters snapshot (serializable — doubles as the wire shape)
// ============================================================================

/// Aggregate counters for one reporting period. Field order fixes the wire
/// key order; adding a counter is a schema change.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignalCounters {
    /// Explicit positive session feedback count.
    #[serde(default)]
    pub feedback_up: u64,
    /// Explicit negative session feedback count.
    #[serde(default)]
    pub feedback_down: u64,
    /// Turns reaching any terminal close.
    #[serde(default)]
    pub turns_ended: u64,
    /// Turns closed with reason `interrupted`.
    #[serde(default)]
    pub turns_interrupted: u64,
    /// Turns in which a human answered at least one permission prompt.
    #[serde(default)]
    pub turns_user_takeover: u64,
    /// Permission asks surfaced to the operator (`ask` decisions).
    #[serde(default)]
    pub permission_prompts: u64,
    /// `/rewind <n>` conversation rewinds.
    #[serde(default)]
    pub rewind_conversation: u64,
    /// `/rewind code <n>` file-history reverts.
    #[serde(default)]
    pub rewind_code: u64,
    /// `/rewind both <n>` combined reverts.
    #[serde(default)]
    pub rewind_both: u64,
    /// `/rewind <path>` per-file reverts.
    #[serde(default)]
    pub rewind_file: u64,
}

impl SignalCounters {
    /// True when every counter is zero (nothing worth flushing).
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Sum the rewind-kind counters (total `/rewind` usage excluding
    /// `history` listings).
    pub fn rewind_total(&self) -> u64 {
        self.rewind_conversation + self.rewind_code + self.rewind_both + self.rewind_file
    }
}

/// Which kind of `/rewind` invocation was issued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewindKind {
    /// `/rewind <n>` — conversation rewind.
    Conversation,
    /// `/rewind code <n>` — revert file changes to a checkpoint.
    Code,
    /// `/rewind both <n>` — revert files and rewind the conversation.
    Both,
    /// `/rewind <path>` — single-file content-snapshot revert.
    File,
}

/// Direction of explicit session feedback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedbackDirection {
    /// Positive (thumbs-up / `up`).
    Up,
    /// Negative (thumbs-down / `down`).
    Down,
}

impl FeedbackDirection {
    /// Parse common textual spellings; returns `None` for anything else.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "up" | "+1" | "👍" => Some(Self::Up),
            "down" | "-1" | "👎" => Some(Self::Down),
            _ => None,
        }
    }
}

// ============================================================================
// Process-global registry (in-memory deltas between flushes)
// ============================================================================

struct Registry {
    feedback_up: AtomicU64,
    feedback_down: AtomicU64,
    turns_ended: AtomicU64,
    turns_interrupted: AtomicU64,
    turns_user_takeover: AtomicU64,
    permission_prompts: AtomicU64,
    rewind_conversation: AtomicU64,
    rewind_code: AtomicU64,
    rewind_both: AtomicU64,
    rewind_file: AtomicU64,
    /// Whether a turn is currently open (start observed, close pending).
    turn_open: AtomicBool,
    /// Latch: a human answered a permission prompt during the open turn.
    takeover_in_turn: AtomicBool,
}

fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| Registry {
        feedback_up: AtomicU64::new(0),
        feedback_down: AtomicU64::new(0),
        turns_ended: AtomicU64::new(0),
        turns_interrupted: AtomicU64::new(0),
        turns_user_takeover: AtomicU64::new(0),
        permission_prompts: AtomicU64::new(0),
        rewind_conversation: AtomicU64::new(0),
        rewind_code: AtomicU64::new(0),
        rewind_both: AtomicU64::new(0),
        rewind_file: AtomicU64::new(0),
        turn_open: AtomicBool::new(false),
        takeover_in_turn: AtomicBool::new(false),
    })
}

/// Record explicit session feedback.
pub fn observe_feedback(direction: FeedbackDirection) {
    match direction {
        FeedbackDirection::Up => {
            registry().feedback_up.fetch_add(1, Ordering::Relaxed);
        }
        FeedbackDirection::Down => {
            registry().feedback_down.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Record a turn start (re-arms the takeover latch for the new turn).
pub fn observe_turn_start() {
    let r = registry();
    r.turn_open.store(true, Ordering::Relaxed);
    r.takeover_in_turn.store(false, Ordering::Relaxed);
}

/// Record a turn close. `reason` matches the
/// [`shannon_types::session_event::TurnEndPayload::REASON_*`] constants;
/// `interrupted` feeds the interruption counter and the per-turn takeover
/// latch folds into the totals.
pub fn observe_turn_end(reason: &str) {
    let r = registry();
    let was_open = r.turn_open.swap(false, Ordering::Relaxed);
    r.turns_ended.fetch_add(1, Ordering::Relaxed);
    if reason == shannon_types::session_event::TurnEndPayload::REASON_INTERRUPTED {
        r.turns_interrupted.fetch_add(1, Ordering::Relaxed);
    }
    if was_open && r.takeover_in_turn.swap(false, Ordering::Relaxed) {
        r.turns_user_takeover.fetch_add(1, Ordering::Relaxed);
    }
}

/// Record one `permission/decision`. Only two shapes move counters:
/// `decision == "ask"` (a prompt surfaced) and a human-resolved prompt
/// (`mode == "USER"` with a concrete allow/deny — the manual-takeover proxy).
/// Rule-based allow/deny decisions are not counted.
pub fn observe_permission_decision(decision: &str, mode: Option<&str>) {
    let r = registry();
    if decision == "ask" {
        r.permission_prompts.fetch_add(1, Ordering::Relaxed);
        return;
    }
    if mode == Some("USER") {
        r.takeover_in_turn.store(true, Ordering::Relaxed);
    }
}

/// Record a `/rewind` invocation. Labels carry no argument values — never a
/// path, only the variant.
pub fn observe_rewind(kind: RewindKind) {
    let cell = match kind {
        RewindKind::Conversation => &registry().rewind_conversation,
        RewindKind::Code => &registry().rewind_code,
        RewindKind::Both => &registry().rewind_both,
        RewindKind::File => &registry().rewind_file,
    };
    cell.fetch_add(1, Ordering::Relaxed);
}

/// Read the current in-memory counters without clearing them.
pub fn snapshot() -> SignalCounters {
    let r = registry();
    SignalCounters {
        feedback_up: r.feedback_up.load(Ordering::Relaxed),
        feedback_down: r.feedback_down.load(Ordering::Relaxed),
        turns_ended: r.turns_ended.load(Ordering::Relaxed),
        turns_interrupted: r.turns_interrupted.load(Ordering::Relaxed),
        turns_user_takeover: r.turns_user_takeover.load(Ordering::Relaxed),
        permission_prompts: r.permission_prompts.load(Ordering::Relaxed),
        rewind_conversation: r.rewind_conversation.load(Ordering::Relaxed),
        rewind_code: r.rewind_code.load(Ordering::Relaxed),
        rewind_both: r.rewind_both.load(Ordering::Relaxed),
        rewind_file: r.rewind_file.load(Ordering::Relaxed),
    }
}

/// Drain the in-memory deltas into a snapshot, leaving any in-flight turn's
/// takeover latch untouched (it belongs to the next close, not this flush).
fn take_snapshot_zeroing() -> SignalCounters {
    let r = registry();
    SignalCounters {
        feedback_up: r.feedback_up.swap(0, Ordering::Relaxed),
        feedback_down: r.feedback_down.swap(0, Ordering::Relaxed),
        turns_ended: r.turns_ended.swap(0, Ordering::Relaxed),
        turns_interrupted: r.turns_interrupted.swap(0, Ordering::Relaxed),
        turns_user_takeover: r.turns_user_takeover.swap(0, Ordering::Relaxed),
        permission_prompts: r.permission_prompts.swap(0, Ordering::Relaxed),
        rewind_conversation: r.rewind_conversation.swap(0, Ordering::Relaxed),
        rewind_code: r.rewind_code.swap(0, Ordering::Relaxed),
        rewind_both: r.rewind_both.swap(0, Ordering::Relaxed),
        rewind_file: r.rewind_file.swap(0, Ordering::Relaxed),
    }
}

fn add_back(counters: &SignalCounters) {
    let r = registry();
    r.feedback_up
        .fetch_add(counters.feedback_up, Ordering::Relaxed);
    r.feedback_down
        .fetch_add(counters.feedback_down, Ordering::Relaxed);
    r.turns_ended
        .fetch_add(counters.turns_ended, Ordering::Relaxed);
    r.turns_interrupted
        .fetch_add(counters.turns_interrupted, Ordering::Relaxed);
    r.turns_user_takeover
        .fetch_add(counters.turns_user_takeover, Ordering::Relaxed);
    r.permission_prompts
        .fetch_add(counters.permission_prompts, Ordering::Relaxed);
    r.rewind_conversation
        .fetch_add(counters.rewind_conversation, Ordering::Relaxed);
    r.rewind_code
        .fetch_add(counters.rewind_code, Ordering::Relaxed);
    r.rewind_both
        .fetch_add(counters.rewind_both, Ordering::Relaxed);
    r.rewind_file
        .fetch_add(counters.rewind_file, Ordering::Relaxed);
}

// ============================================================================
// Wire payload (whitelist-shaped)
// ============================================================================

/// Serialize the exact upload payload: metadata plus aggregate counters.
/// Key order follows construction order — fully deterministic.
pub fn payload_json(
    counters: &SignalCounters,
    app_version: &str,
    generated_at_utc: &str,
    period_day_utc: &str,
) -> String {
    serde_json::json!({
        "schema": SIGNALS_SCHEMA,
        "app_version": app_version,
        "generated_at_utc": generated_at_utc,
        "period_day_utc": period_day_utc,
        "counters": counters,
    })
    .to_string()
}

// ============================================================================
// Configuration
// ============================================================================

/// Runtime switches for the signals subsystem. Defaults are inert.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SignalsConfig {
    /// Outbound posting master switch (opt-in; default `false`).
    pub upload_enabled: bool,
    /// Target URL — required together with `upload_enabled` to transmit.
    pub endpoint: Option<String>,
    /// Optional HMAC-SHA256 signing secret shared with the receiver.
    pub secret: Option<String>,
}

impl SignalsConfig {
    /// Read switches from an injectable environment lookup (keeps tests
    /// hermetic — production passes [`std::env::var`]).
    pub fn from_env_lookup<F>(lookup: F) -> Self
    where
        F: Fn(&str) -> Option<String>,
    {
        let truthy = |v: Option<String>| matches!(v.as_deref(), Some("1") | Some("true"));
        Self {
            upload_enabled: truthy(lookup(ENV_UPLOAD)),
            endpoint: lookup(ENV_ENDPOINT).filter(|v| !v.trim().is_empty()),
            secret: lookup(ENV_SECRET).filter(|v| !v.trim().is_empty()),
        }
    }

    /// Read switches from the process environment.
    pub fn from_env() -> Self {
        Self::from_env_lookup(|key| std::env::var(key).ok())
    }

    /// Whether an outbound transmission would be attempted at all.
    fn can_upload(&self) -> bool {
        self.upload_enabled
            && self
                .endpoint
                .as_deref()
                .is_some_and(|u| u.trim().starts_with("http"))
    }
}

// ============================================================================
// Local projection + report
// ============================================================================

/// Resolve `<shannon-home>/analytics`, honoring `$SHANNON_HOME` (falls back
/// to `~/.shannon`), overridable for tests.
pub fn analytics_dir(home_override: Option<&Path>) -> PathBuf {
    if let Some(home) = home_override {
        return home.join("analytics");
    }
    match crate::session_log::default_shannon_home() {
        Ok(home) => home.join("analytics"),
        // No resolvable home: fall back beside the process CWD rather than
        // panicking during shutdown-time flushes.
        Err(_) => PathBuf::from("analytics"),
    }
}

/// Append one JSONL line carrying `counters` to `<dir>/counters.jsonl`.
/// Returns `Ok(None)` for empty snapshots (no noise lines for all-zero
/// periods). Aggregate history accumulates across flushes by design.
pub fn append_local(
    counters: &SignalCounters,
    analytics_home: &Path,
    timestamp_override: Option<&str>,
) -> std::io::Result<Option<PathBuf>> {
    if counters.is_empty() {
        return Ok(None);
    }
    std::fs::create_dir_all(analytics_home)?;
    let ts = timestamp_override.map_or_else(
        || chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        str::to_string,
    );
    let line = serde_json::json!({
        "schema": SIGNALS_SCHEMA,
        "app_version": env!("CARGO_PKG_VERSION"),
        "ts_utc": ts,
        "counters": counters,
    });
    let path = analytics_home.join("counters.jsonl");
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(file, "{line}")?;
    Ok(Some(path))
}

/// Outcome of a [`report`] pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportOutcome {
    /// All counters were zero — nothing written, nothing sent.
    NothingNew,
    /// Snapshot persisted locally; upload skipped (disabled or no endpoint).
    LocalOnly(PathBuf),
    /// Snapshot persisted locally and handed to the webhook transport
    /// (delivery itself stays fire-and-forget with bounded retries).
    LocalAndUploadQueued(PathBuf),
}

/// Flush drained deltas to the local projection and, when opted in, queue an
/// upload through the notifier/webhook transport. A failed local write
/// restores the drained totals so nothing is lost.
pub fn report(config: &SignalsConfig, analytics_home: Option<&Path>) -> ReportOutcome {
    let counters = take_snapshot_zeroing();
    if counters.is_empty() {
        return ReportOutcome::NothingNew;
    }
    let home = match analytics_home {
        Some(path) => path.join("analytics"),
        None => analytics_dir(None),
    };
    let written = match append_local(&counters, &home, None) {
        Ok(path) => path,
        Err(e) => {
            tracing::warn!(target: "shannon_core::signals", error = %e, "counters flush failed; restoring in-memory totals");
            add_back(&counters);
            return ReportOutcome::NothingNew;
        }
    };
    let Some(path) = written else {
        return ReportOutcome::NothingNew;
    };

    if !config.can_upload() {
        return ReportOutcome::LocalOnly(path);
    }

    let now = chrono::Utc::now();
    let body = payload_json(
        &counters,
        env!("CARGO_PKG_VERSION"),
        &now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        &now.format("%Y-%m-%d").to_string(),
    );
    match WebhookHandler::new(WebhookConfig {
        url: config.endpoint.clone().unwrap_or_default(),
        secret: config.secret.clone(),
        ..WebhookConfig::default()
    }) {
        Ok(handler) => match handler.deliver(body) {
            Ok(()) => ReportOutcome::LocalAndUploadQueued(path),
            Err(e) => {
                tracing::warn!(target: "shannon_core::signals", error = %e, "signals upload queue failed");
                ReportOutcome::LocalOnly(path)
            }
        },
        Err(e) => {
            tracing::warn!(target: "shannon_core::signals", error = %e, "signals upload client init failed");
            ReportOutcome::LocalOnly(path)
        }
    }
}

/// Best-effort shutdown-time flush honoring the environment configuration;
/// errors are logged, never propagated (safe in process teardown paths).
pub fn try_flush_default() {
    let _ = report(&SignalsConfig::from_env(), None);
}

// ============================================================================
// Transport reuse — analytics ride the notifier/webhook pipeline
// ============================================================================

use crate::notifier::{WebhookConfig, WebhookHandler};

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use shannon_types::session_event::TurnEndPayload;
    use std::sync::atomic::AtomicUsize;

    /// Per-test observation window around the shared in-process registry:
    /// read before/after snapshots so unrelated observer traffic cancels out
    /// arithmetically regardless of scheduling.
    struct Delta {
        before: SignalCounters,
    }

    impl Delta {
        fn take() -> Self {
            Self { before: snapshot() }
        }
        fn measure(self) -> SignalCounters {
            let after = snapshot();
            SignalCounters {
                feedback_up: after.feedback_up.wrapping_sub(self.before.feedback_up),
                feedback_down: after.feedback_down.wrapping_sub(self.before.feedback_down),
                turns_ended: after.turns_ended.wrapping_sub(self.before.turns_ended),
                turns_interrupted: after
                    .turns_interrupted
                    .wrapping_sub(self.before.turns_interrupted),
                turns_user_takeover: after
                    .turns_user_takeover
                    .wrapping_sub(self.before.turns_user_takeover),
                permission_prompts: after
                    .permission_prompts
                    .wrapping_sub(self.before.permission_prompts),
                rewind_conversation: after
                    .rewind_conversation
                    .wrapping_sub(self.before.rewind_conversation),
                rewind_code: after.rewind_code.wrapping_sub(self.before.rewind_code),
                rewind_both: after.rewind_both.wrapping_sub(self.before.rewind_both),
                rewind_file: after.rewind_file.wrapping_sub(self.before.rewind_file),
            }
        }
    }

    #[test]
    fn turn_interruption_counts_as_ended_and_interrupted() {
        let d = Delta::take();
        observe_turn_start();
        observe_turn_end(TurnEndPayload::REASON_INTERRUPTED);
        assert_eq!(
            d.measure(),
            SignalCounters {
                turns_ended: 1,
                turns_interrupted: 1,
                ..SignalCounters::default()
            }
        );
    }

    #[test]
    fn completed_turn_without_takeover_only_counts_denominator() {
        let d = Delta::take();
        observe_turn_start();
        observe_turn_end(TurnEndPayload::REASON_COMPLETED);
        assert_eq!(
            d.measure(),
            SignalCounters {
                turns_ended: 1,
                ..SignalCounters::default()
            }
        );
    }

    #[test]
    fn user_takeover_latches_per_turn_and_resets_on_next_start() {
        let d = Delta::take();
        observe_turn_start();
        observe_permission_decision("ask", Some("AUTO"));
        observe_permission_decision("allow", Some("USER"));
        observe_turn_end(TurnEndPayload::REASON_COMPLETED);
        // A fresh turn must start latched-off: a later auto-decision alone
        // must not fabricate a second takeover.
        observe_permission_decision("allow", Some("USER")); // no open turn ⇒ ignored
        observe_turn_start();
        observe_permission_decision("deny", Some("AUTO"));
        observe_turn_end(TurnEndPayload::REASON_FAILED);
        assert_eq!(
            d.measure(),
            SignalCounters {
                turns_ended: 2,
                turns_user_takeover: 1,
                permission_prompts: 1,
                ..SignalCounters::default()
            }
        );
    }

    #[test]
    fn rewind_kinds_route_to_distinct_counters() {
        let d = Delta::take();
        observe_rewind(RewindKind::Conversation);
        observe_rewind(RewindKind::Conversation);
        observe_rewind(RewindKind::Code);
        observe_rewind(RewindKind::Both);
        observe_rewind(RewindKind::File);
        let measured = d.measure();
        assert_eq!(measured.rewind_conversation, 2);
        assert_eq!(measured.rewind_code, 1);
        assert_eq!(measured.rewind_both, 1);
        assert_eq!(measured.rewind_file, 1);
        assert_eq!(measured.rewind_total(), 5);
    }

    #[test]
    fn feedback_directions_parse_and_count() {
        let d = Delta::take();
        assert_eq!(FeedbackDirection::parse("Up"), Some(FeedbackDirection::Up));
        assert_eq!(
            FeedbackDirection::parse(" 👎 "),
            Some(FeedbackDirection::Down)
        );
        assert_eq!(FeedbackDirection::parse("sideways"), None);
        observe_feedback(FeedbackDirection::Up);
        observe_feedback(FeedbackDirection::Down);
        observe_feedback(FeedbackDirection::Down);
        let measured = d.measure();
        assert_eq!(measured.feedback_up, 1);
        assert_eq!(measured.feedback_down, 2);
    }

    #[test]
    fn payload_is_whitelist_of_aggregate_fields_only() {
        let counters = SignalCounters {
            feedback_up: 3,
            feedback_down: 1,
            turns_ended: 20,
            turns_interrupted: 4,
            turns_user_takeover: 6,
            permission_prompts: 9,
            rewind_conversation: 2,
            rewind_code: 1,
            rewind_both: 0,
            rewind_file: 0,
        };
        let value: Value = serde_json::from_str(&payload_json(
            &counters,
            "0.10.0",
            "2026-08-27T00:00:00+00:00",
            "2026-08-27",
        ))
        .expect("payload parses");

        // Exact structural equality — no extra/misnamed field can sneak in.
        assert_eq!(
            value,
            json!({
                "schema": "shannon.signals.v1",
                "app_version": "0.10.0",
                "generated_at_utc": "2026-08-27T00:00:00+00:00",
                "period_day_utc": "2026-08-27",
                "counters": {
                    "feedback_up": 3,
                    "feedback_down": 1,
                    "turns_ended": 20,
                    "turns_interrupted": 4,
                    "turns_user_takeover": 6,
                    "permission_prompts": 9,
                    "rewind_conversation": 2,
                    "rewind_code": 1,
                    "rewind_both": 0,
                    "rewind_file": 0,
                }
            })
        );

        // Content-carrier vocabulary may not appear anywhere in the payload.
        let text = value.to_string();
        for banned in [
            "content",
            "message",
            "prompt_text",
            "arguments",
            "diff",
            "transcript",
        ] {
            assert!(
                !text.contains(banned),
                "payload must not embed '{banned}'-carrying fields"
            );
        }
    }

    #[test]
    fn append_local_writes_one_jsonl_line_and_skips_empty_snapshots() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();

        let none = append_local(
            &SignalCounters::default(),
            home,
            Some("2026-08-27T01:02:03+00:00"),
        )
        .unwrap();
        assert_eq!(none, None, "empty snapshots must not produce lines");

        let path = append_local(
            &SignalCounters {
                turns_ended: 2,
                turns_interrupted: 1,
                ..SignalCounters::default()
            },
            home,
            Some("2026-08-27T01:02:04+00:00"),
        )
        .unwrap()
        .expect("non-empty snapshot writes");
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches('\n').count(), 1);
        let line: Value = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(line["schema"], SIGNALS_SCHEMA);
        assert_eq!(line["ts_utc"], "2026-08-27T01:02:04+00:00");
        assert_eq!(line["counters"]["turns_interrupted"], 1);

        // Repeated flushes grow the same aggregate-history file.
        append_local(
            &SignalCounters {
                feedback_up: 7,
                ..SignalCounters::default()
            },
            home,
            Some("2026-08-28T00:00:00+00:00"),
        )
        .unwrap();
        let grown = std::fs::read_to_string(&path).unwrap();
        assert_eq!(grown.matches('\n').count(), 2);
    }

    #[test]
    fn config_defaults_to_upload_disabled_even_with_endpoint_present() {
        let cfg = SignalsConfig::from_env_lookup(|k| match k {
            ENV_ENDPOINT => Some("http://collector.example.internal/hook".into()),
            _ => None,
        });
        assert!(!cfg.upload_enabled);
        assert!(!cfg.can_upload());

        let cfg_on = SignalsConfig::from_env_lookup(|k| match k {
            ENV_UPLOAD => Some("true".into()),
            ENV_ENDPOINT => Some("https://collector.example.internal/hook".into()),
            ENV_SECRET => Some("hmac-key".into()),
            _ => None,
        });
        assert!(cfg_on.can_upload());
    }

    // -- Wire-level verification (§4.15 standards ① and ②) -------------------

    /// Minimal packet-capture sink: a loopback TCP listener accumulating raw
    /// request bytes from the webhook transport. Reads stop per connection
    /// once headers + declared Content-Length have arrived (keep-alive means
    /// waiting for EOF would hang), tracked by wall-clock read timeouts.
    struct Capture {
        addr: String,
        captured: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
        connections: std::sync::Arc<AtomicUsize>,
    }

    impl Capture {
        fn start() -> Self {
            use std::io::Read;
            use std::net::TcpListener;
            let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback");
            let addr = listener.local_addr().expect("local addr").to_string();
            let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let connections = std::sync::Arc::new(AtomicUsize::new(0));
            let sink = std::sync::Arc::clone(&captured);
            let hits = std::sync::Arc::clone(&connections);
            std::thread::spawn(move || {
                for stream in listener.incoming().flatten() {
                    hits.fetch_add(1, Ordering::SeqCst);
                    let mut got: Vec<u8> = Vec::new();
                    let mut chunk = [0u8; 8192];
                    let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(750)));
                    let mut reader = stream;
                    loop {
                        match reader.read(&mut chunk) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                got.extend_from_slice(&chunk[..n]);
                                if request_complete(&got) {
                                    break;
                                }
                            }
                        }
                    }
                    sink.lock().expect("sink lock").extend_from_slice(&got);
                }
            });
            Self {
                addr,
                captured,
                connections,
            }
        }

        fn text(&self) -> String {
            String::from_utf8_lossy(&self.captured.lock().expect("lock")).into_owned()
        }

        fn connection_count(&self) -> usize {
            self.connections.load(Ordering::SeqCst)
        }
    }

    /// True once the buffered bytes carry complete headers plus the declared
    /// request body length (HTTP/1.1 + Content-Length is all we post).
    fn request_complete(buf: &[u8]) -> bool {
        let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
            return false;
        };
        let headers = String::from_utf8_lossy(&buf[..pos]).to_ascii_lowercase();
        let declared: usize = headers
            .lines()
            .find_map(|l| l.strip_prefix("content-length:"))
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0);
        buf.len() >= pos + 4 + declared
    }

    /// Extract the first balanced JSON object starting at `s` (our payloads
    /// embed no braces inside strings, so a depth scan suffices). Retry
    /// deliveries may append further requests after it, so no tail assumptions.
    fn extract_first_json_object(s: &str) -> Option<&str> {
        let bytes = s.as_bytes();
        let start = bytes.iter().position(|b| *b == b'{')?;
        let mut depth = 0usize;
        for (i, b) in bytes.iter().enumerate().skip(start) {
            match b {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(&s[start..=i]);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Async wait so the current-thread runtime keeps polling the spawned
    /// delivery task while we watch for the outbound connection.
    async fn drain_until(capture: &Capture, min_connections: usize) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while capture.connection_count() < min_connections {
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for outbound connection #{min_connections}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    }

    /// Standard ① (on-state): the captured request carries exactly the
    /// whitelisted aggregate-counters payload — five top-level keys, counter
    /// deltas matching what was observed — signed when a secret is set.
    #[tokio::test(flavor = "current_thread")]
    async fn enabled_report_posts_only_the_whitelisted_payload() {
        let capture = Capture::start();
        let dir = tempfile::tempdir().unwrap();

        let d = Delta::take();
        observe_rewind(RewindKind::File);
        let _own_delta = d.measure();

        let cfg = SignalsConfig {
            upload_enabled: true,
            endpoint: Some(format!("http://{}/ingest", capture.addr)),
            secret: Some("shared-secret".into()),
        };
        let outcome = report(&cfg, Some(dir.path()));
        assert!(
            matches!(outcome, ReportOutcome::LocalAndUploadQueued(_)),
            "expected queued upload, got {outcome:?}"
        );

        drain_until(&capture, 1).await;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let raw = loop {
            let text = capture.text();
            if request_complete(text.as_bytes()) || std::time::Instant::now() > deadline {
                break text;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        };
        assert!(
            raw.starts_with("POST /ingest HTTP/1.1"),
            "must hit the configured path; captured:\n{raw}"
        );
        assert!(
            raw.to_ascii_lowercase()
                .contains("x-shannon-signature: sha256="),
            "signed requests must carry the signature header"
        );

        // Body follows the header block; key order inside it is map-sorted,
        // so anchor on the delimiter rather than a serialized prefix.
        let body_text = extract_first_json_object(
            raw.split_once("\r\n\r\n")
                .expect("header/body delimiter present")
                .1,
        )
        .expect("complete aggregate JSON object in captured request");
        let parsed: Value = serde_json::from_str(body_text).expect("captured body parses as JSON");

        // Field-set equality: ONLY the four metadata keys + counters object.
        let keys: Vec<&String> = parsed.as_object().expect("object").keys().collect();
        assert_eq!(
            keys.len(),
            5,
            "only whitelisted keys may travel, saw {parsed}"
        );
        for key in [
            "schema",
            "app_version",
            "generated_at_utc",
            "period_day_utc",
            "counters",
        ] {
            assert!(parsed.get(key).is_some(), "missing key '{key}'");
        }
        assert_eq!(parsed["schema"], json!(SIGNALS_SCHEMA));
        assert_eq!(parsed["app_version"], env!("CARGO_PKG_VERSION"));

        // Counters: exactly the ten whitelisted aggregate names (values may
        // legitimately fold in deltas from any concurrent activity sharing
        // this process), ours among them.
        let counters = parsed["counters"].as_object().expect("counters obj");
        assert_eq!(
            counters.len(),
            10,
            "counter name set is fixed: {counters:?}"
        );
        for name in [
            "feedback_up",
            "feedback_down",
            "turns_ended",
            "turns_interrupted",
            "turns_user_takeover",
            "permission_prompts",
            "rewind_conversation",
            "rewind_code",
            "rewind_both",
            "rewind_file",
        ] {
            assert!(counters.contains_key(name));
        }
        assert!(
            parsed["counters"]["rewind_file"].as_u64().unwrap_or(0) >= 1,
            "our own delta must be included"
        );

        // And no content-carrier name appears anywhere in the exchange.
        let lowered = raw.to_ascii_lowercase();
        for banned in ["\"content\"", "\"message\"", "\"arguments\"", "\"diff\""] {
            assert!(!lowered.contains(banned), "{banned} leaked over the wire");
        }
    }

    /// Standard ② (off-state): with the switch unset (the shipped default) a
    /// report persists locally and transmits nothing — zero TCP connections
    /// ever reach the capture sink despite its address being the endpoint.
    #[tokio::test(flavor = "current_thread")]
    async fn disabled_report_never_touches_the_network() {
        let capture = Capture::start();
        let dir = tempfile::tempdir().unwrap();

        observe_rewind(RewindKind::Code);
        observe_feedback(FeedbackDirection::Down);

        let cfg = SignalsConfig {
            upload_enabled: false,
            endpoint: Some(format!("http://{}/ingest", capture.addr)),
            secret: None,
        };
        let outcome = report(&cfg, Some(dir.path()));
        assert!(
            matches!(outcome, ReportOutcome::LocalOnly(_)),
            "off-state must stop at the local projection, got {outcome:?}"
        );

        // Give any hypothetical misfire ample time to surface.
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        assert_eq!(
            capture.connection_count(),
            0,
            "off-state transmitted data — privacy violation"
        );
        assert!(capture.text().is_empty());

        // The activity went to disk, never through a socket.
        let path = dir.path().join("analytics").join("counters.jsonl");
        let text = std::fs::read_to_string(&path).unwrap();
        let rows: Vec<Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert!(!rows.is_empty());
        assert!(
            rows.iter()
                .any(|r| r["counters"]["rewind_code"].as_u64().unwrap_or(0) >= 1)
        );

        // Env-free default config is likewise inert.
        assert!(!SignalsConfig::from_env_lookup(|_| None).can_upload());
    }

    /// Local-only double duty: repeated reports land distinct lines whose
    /// counters sum back to the observed activity (the "local-first
    /// analytics projection" requirement).
    #[tokio::test(flavor = "current_thread")]
    async fn repeated_off_state_reports_accumulate_in_the_projection() {
        let dir = tempfile::tempdir().unwrap();

        observe_feedback(FeedbackDirection::Up);
        assert!(matches!(
            report(&SignalsConfig::default(), Some(dir.path())),
            ReportOutcome::LocalOnly(_)
        ));

        observe_rewind(RewindKind::Both);
        assert!(matches!(
            report(&SignalsConfig::default(), Some(dir.path())),
            ReportOutcome::LocalOnly(_)
        ));

        let path = dir.path().join("analytics").join("counters.jsonl");
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches('\n').count(), 2);

        let mut up_total = 0u64;
        let mut both_total = 0u64;
        for line in text.lines() {
            let row: Value = serde_json::from_str(line).unwrap();
            up_total += row["counters"]["feedback_up"].as_u64().unwrap_or(0);
            both_total += row["counters"]["rewind_both"].as_u64().unwrap_or(0);
        }
        assert_eq!(up_total, 1);
        assert_eq!(both_total, 1);
    }
}
