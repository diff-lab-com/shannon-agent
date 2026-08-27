//! CLI surface for the §4.15 online-signal counters (`shannon feedback`,
//! `shannon signals`). Everything here is counts-only: no free text, no
//! content fields, and outbound upload exclusively behind
//! `SHANNON_SIGNALS_UPLOAD` + `SHANNON_SIGNALS_ENDPOINT` (see
//! `shannon_core::signals`).

use std::path::Path;

use anyhow::{Result, bail};

use shannon_core::signals::{self, FeedbackDirection, ReportOutcome, SignalsConfig};

/// Handle `shannon feedback <up|down>`: record one aggregate count and
/// immediately flush/queue per current switches.
pub fn run_feedback(raw_direction: &str) -> Result<String> {
    run_feedback_in(raw_direction, None)
}

/// Same flow with an injectable projection home (tests keep the real user
/// directory untouched).
fn run_feedback_in(raw_direction: &str, home: Option<&Path>) -> Result<String> {
    let Some(direction) = FeedbackDirection::parse(raw_direction) else {
        bail!(
            "unknown feedback direction '{raw_direction}' \
             (expected: up | down)"
        );
    };
    // Only a ±1 ever gets recorded — comments would be content.
    signals::observe_feedback(direction);
    Ok(describe_push(signals::report(
        &SignalsConfig::from_env(),
        home,
    )))
}

/// Handle `shannon signals push`.
pub fn push_now() -> Result<String> {
    Ok(describe_push(signals::report(
        &SignalsConfig::from_env(),
        None,
    )))
}

/// Human-readable rendering of one flush/report pass.
fn describe_push(outcome: ReportOutcome) -> String {
    match outcome {
        ReportOutcome::NothingNew => "counters unchanged — nothing to persist".to_string(),
        ReportOutcome::LocalOnly(path) => {
            if SignalsConfig::from_env().upload_enabled {
                format!(
                    "counters flushed to {} (no usable endpoint configured)",
                    path.display()
                )
            } else {
                format!("counters flushed to {} (upload disabled)", path.display())
            }
        }
        ReportOutcome::LocalAndUploadQueued(path) => format!(
            "counters flushed to {}; aggregate payload queued for upload",
            path.display()
        ),
    }
}

/// Render `shannon signals status`: live snapshot, derived rates, effective
/// switch state. Purely informational — prints, never sends.
pub fn status_report() -> String {
    let snap = signals::snapshot();
    let cfg = SignalsConfig::from_env();

    let mut out = String::from("§4.15 usage counters (aggregate, local):\n");
    out.push_str(&format!(
        "  feedback up/down          : {} / {}\n",
        snap.feedback_up, snap.feedback_down
    ));
    out.push_str(&format!(
        "  turns ended               : {}\n",
        snap.turns_ended
    ));
    out.push_str(&format!(
        "  turns interrupted         : {}{}\n",
        snap.turns_interrupted,
        rate_line(snap.turns_interrupted, snap.turns_ended)
    ));
    out.push_str(&format!(
        "  turns human-taken-over    : {}{}\n",
        snap.turns_user_takeover,
        rate_line(snap.turns_user_takeover, snap.turns_ended)
    ));
    out.push_str(&format!(
        "  permission prompts        : {}\n",
        snap.permission_prompts
    ));
    out.push_str(&format!(
        "  /rewind conv/code/both/file: {} / {} / {} / {}\n",
        snap.rewind_conversation, snap.rewind_code, snap.rewind_both, snap.rewind_file
    ));

    if cfg.upload_enabled {
        match cfg.endpoint.as_deref() {
            Some(url) => out.push_str(&format!("upload: ENABLED → {url}\n")),
            None => out.push_str("upload: enabled but SHANNON_SIGNALS_ENDPOINT unset\n"),
        }
    } else {
        out.push_str("upload: disabled (default — set SHANNON_SIGNALS_UPLOAD=1 and SHANNON_SIGNALS_ENDPOINT to opt in)\n");
    }
    out
}

/// ` · <pct>% of <total>` when a denominator exists, else "".
fn rate_line(numerator: u64, total: u64) -> String {
    if total == 0 {
        return String::new();
    }
    format!(
        "  ({:.1}% of {total})",
        numerator as f64 / total as f64 * 100.0
    )
}

#[allow(clippy::unwrap_used)]
mod tests {
    // Call-site-qualified paths (no `use`) so the compile stays warning-free
    // whether or not `cfg(test)` modules participate in linting.
    #[test]
    fn unknown_direction_is_rejected_without_counting() {
        let before = shannon_core::signals::snapshot();
        let err = match super::run_feedback("sideways") {
            Err(e) => e.to_string(),
            Ok(msg) => panic!("unexpected success: {msg}"),
        };
        assert!(err.contains("up"), "error mentions valid inputs: {err}");
        let after = shannon_core::signals::snapshot();
        assert_eq!(after.feedback_up - before.feedback_up, 0);
        assert_eq!(after.feedback_down - before.feedback_down, 0);
    }

    #[test]
    fn accepted_direction_counts_exactly_one_and_flushes_locally() {
        let scratch = tempfile::tempdir().unwrap();
        let message =
            super::run_feedback_in("down", Some(scratch.path())).expect("valid direction");
        assert!(message.contains("upload disabled"), "{message}");

        let path = scratch.path().join("analytics").join("counters.jsonl");
        let text = std::fs::read_to_string(path).unwrap();
        let row: serde_json::Value = serde_json::from_str(text.lines().last().unwrap()).unwrap();
        assert_eq!(row["counters"]["feedback_down"], 1);
    }

    #[test]
    fn status_report_lists_counters_and_default_switch_state() {
        let text = super::status_report();
        assert!(text.contains("turns_ended") || text.contains("turns ended"));
        assert!(text.contains("upload: disabled"));
        assert!(
            !text.contains("SHANNON_HOME"),
            "internal env noise stays out of operator output"
        );
    }
}
