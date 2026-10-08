//! 04b — structured test-count extraction for batch verification cards.
//!
//! Source of truth: the branch's **last** `ToolUseResult` text (captured
//! live by `batch_commands::EngineBatchBranchRunner::stream_branch`; the
//! engine emits `QueryEvent::ToolUseResult { result, .. }` on the same
//! stream the runner already consumes, so no session-log re-reading is
//! needed — the L0 `tool/result` payload is a clone of this same string).
//!
//! Strictness contract: only the three major test runners' **final summary
//! line** formats are recognized, each **anchored** (line must start with
//! the runner's fixed prefix, pytest must carry its `===` rails). Anything
//! else — conversation text, shell output, a summary line truncated
//! mid-token — yields `None`. Callers must treat `None` as "no data"
//! (counts stay `None` on the verification card), never as zero, and must
//! never guess counts from unstructured session text.
//!
//! Last-match-wins: a run of N test binaries (cargo prints one summary per
//! binary) or a re-run contributes its final summary, matching the
//! "terminal aggregate" semantics of the branch exit.

/// `(passed, total)` from a test-runner final summary, when one is present.
///
/// `total = passed + failed`: what the runner itself reported as realized
/// outcomes. Skipped/ignored/todo/xfailed are deliberately excluded — the
/// card promises "passed / total", and padding the total with not-run tests
/// would blur the pass rate.
pub(crate) fn parse_test_summary(output: &str) -> Option<(u32, u32)> {
    let mut verdict = None;
    for line in output.lines() {
        let line = line.trim();
        // Order is arbitrary — the formats are mutually exclusive — but a
        // line parses as at most one runner's summary.
        if let Some(parsed) = parse_cargo_line(line)
            .or_else(|| parse_jest_line(line))
            .or_else(|| parse_pytest_line(line))
        {
            verdict = Some(parsed);
        }
    }
    verdict
}

// ── cargo / rustc libtest ────────────────────────────────────────────────

/// `test result: ok. 12 passed; 0 failed; 2 ignored; 5 measured; ...`
///
/// One line per test binary; [`parse_test_summary`] keeps the last.
/// libtest always prints **both** counts (even when zero), so a line
/// missing either is a truncated tool output and is rejected rather than
/// half-parsed.
fn parse_cargo_line(line: &str) -> Option<(u32, u32)> {
    let rest = line.strip_prefix("test result: ")?;
    // Skip the libtest verdict token ("ok" / "FAILED") before its period.
    let counts = rest.split_once(". ")?.1;
    counts_from_segments(counts.split(';'), true)
}

// ── jest / vitest (jest-compatible reporter) ─────────────────────────────

/// `Tests: 3 skipped, 13 passed, 2 failed, 18 total`
///
/// The runner prints only non-zero categories, so subsets like
/// `Tests: 5 passed, 5 total` or `Tests: 2 failed` are valid; a line with
/// neither `passed` nor `failed` (e.g. all-skipped) says nothing about
/// realized outcomes and is rejected.
fn parse_jest_line(line: &str) -> Option<(u32, u32)> {
    let rest = line.strip_prefix("Tests: ")?;
    counts_from_segments(rest.split(','), false)
}

// ── pytest ───────────────────────────────────────────────────────────────

/// `============= 12 passed, 1 xfailed in 0.13s =============`
///
/// The `===` rails (three or more `=` on each side) are the anchor; the
/// inner text is a loose segment list where only `passed`/`failed` count
/// (variants like `xfailed`, `warnings` and the trailing `in <dur>` are
/// present in real output and simply not tracked).
fn parse_pytest_line(line: &str) -> Option<(u32, u32)> {
    if !line.starts_with("===") || !line.ends_with("===") {
        return None;
    }
    let inner = line.trim_matches('=').trim();
    if inner.is_empty() {
        return None;
    }
    counts_from_segments(inner.split(','), false)
}

// ── shared segment parser ────────────────────────────────────────────────

/// Scan `<n> <label>` segments for the tracked labels, ignoring every other
/// segment (`18 total`, `1 warning in 0.12s`, `3 xfailed`, `0 measured`).
///
/// A segment counts only when its first token is a `u32` and its second
/// token is the label — so a count truncated mid-token (tool-output caps)
/// fails to match and the segment is rejected rather than half-parsed. A
/// line matches only if it names at least one realized outcome (or, with
/// `require_both`, both); absent labels default to 0, which is the runner's
/// own convention for jest/pytest (non-zero categories are always printed).
///
/// Known residual limit: a jest/pytest line truncated *exactly* at a
/// category boundary (`… 13 passed, 2 fai`) is indistinguishable from a
/// legitimate subset line — the counts stay runner-reported either way.
fn counts_from_segments<'a>(
    segments: impl Iterator<Item = &'a str>,
    require_both: bool,
) -> Option<(u32, u32)> {
    let mut passed = None;
    let mut failed = None;
    // A numeric token beyond u32 is not a runner convention — it means the
    // line is not a sane summary. Reject the whole line rather than skip
    // the segment (skipping would fabricate a default for that category).
    let mut poisoned = false;
    for segment in segments {
        let mut tokens = segment.split_whitespace();
        let Some(first) = tokens.next() else {
            continue;
        };
        let Ok(count) = first.parse::<u32>() else {
            if first.as_bytes().first().is_some_and(u8::is_ascii_digit) {
                poisoned = true;
            }
            continue;
        };
        match tokens.next() {
            Some("passed") if passed.is_none() => passed = Some(count),
            Some("failed") if failed.is_none() => failed = Some(count),
            // A count with no label is a line cut mid-category — no real
            // runner emits a trailing bare number.
            None => poisoned = true,
            _ => {}
        }
    }
    if poisoned {
        return None;
    }
    let complete = if require_both {
        passed.is_some() && failed.is_some()
    } else {
        passed.is_some() || failed.is_some()
    };
    if !complete {
        return None;
    }
    let passed = passed.unwrap_or(0);
    let failed = failed.unwrap_or(0);
    Some((passed, passed.saturating_add(failed)))
}

#[cfg(test)]
mod tests {
    use super::parse_test_summary;

    // ── cargo / libtest ──────────────────────────────────────────────────

    #[test]
    fn cargo_summary_passes_and_totals() {
        let out = "running 12 tests\ntest a ... ok\ntest b ... ok\n\n\
                   test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; \
                   3 filtered out; finished in 0.01s\n";
        assert_eq!(parse_test_summary(out), Some((12, 12)));
    }

    #[test]
    fn cargo_summary_with_failures_sums_total() {
        let out = "test result: FAILED. 3 passed; 2 failed; 1 ignored; \
                   0 measured; 0 filtered out; finished in 2.10s";
        assert_eq!(parse_test_summary(out), Some((3, 5)));
    }

    #[test]
    fn cargo_last_binary_summary_wins() {
        // Multi-crate `cargo test` prints one summary per binary.
        let out = "test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; \
                   0 filtered out; finished in 0.01s\n\n\
                   test result: FAILED. 1 passed; 1 failed; 0 ignored; \
                   0 measured; 0 filtered out; finished in 0.02s";
        assert_eq!(parse_test_summary(out), Some((1, 2)));
    }

    #[test]
    fn cargo_doctest_summary_is_terminal_and_counts() {
        let out = "test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; \
                   finished in 0.00s\n\n\
                   Doc-tests shannon-desktop\n\
                   test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; \
                   finished in 0.00s";
        assert_eq!(parse_test_summary(out), Some((0, 1)));
    }

    // ── jest / vitest ────────────────────────────────────────────────────

    #[test]
    fn jest_full_summary_ignores_skipped_and_total() {
        let out = "PASS src/a.test.js\nTests: 3 skipped, 13 passed, 2 failed, 18 total\n\
                   Snapshots: 0 total\nTime: 1.234 s";
        assert_eq!(parse_test_summary(out), Some((13, 15)));
    }

    #[test]
    fn jest_all_green_subset_line() {
        assert_eq!(parse_test_summary("Tests: 5 passed, 5 total"), Some((5, 5)));
    }

    #[test]
    fn jest_failures_only_subset_line_defaults_passed_to_zero() {
        assert_eq!(parse_test_summary("Tests: 2 failed, 7 total"), Some((0, 2)));
    }

    #[test]
    fn vitest_jest_reporter_summary() {
        let out = " ✓ src/b.test.ts (4 tests) 12ms\n\n\
                   Test Files  1 passed (1)\nTests: 4 passed, 4 total";
        assert_eq!(parse_test_summary(out), Some((4, 4)));
    }

    #[test]
    fn jest_skipped_only_line_says_nothing_and_is_rejected() {
        assert_eq!(parse_test_summary("Tests: 3 skipped, 3 total"), None);
    }

    #[test]
    fn jest_summary_without_total_segment_still_parses() {
        assert_eq!(parse_test_summary("Tests: 2 failed"), Some((0, 2)));
    }

    // ── pytest ───────────────────────────────────────────────────────────

    #[test]
    fn pytest_all_green_rail_line() {
        let out = "test_a.py .\n\n============================= 5 passed in 0.12s \
                   =============================";
        assert_eq!(parse_test_summary(out), Some((5, 5)));
    }

    #[test]
    fn pytest_mixed_outcome_line_takes_only_passed_and_failed() {
        let out = "=========== 2 failed, 8 passed, 1 xfailed, 12 warnings in 0.13s ===========";
        assert_eq!(parse_test_summary(out), Some((8, 10)));
    }

    #[test]
    fn pytest_errors_only_line_defaults_passed_to_zero() {
        let out = "========== 1 failed, 3 errors in 0.05s ==========";
        assert_eq!(parse_test_summary(out), Some((0, 1)));
    }

    #[test]
    fn pytest_line_without_rails_is_rejected() {
        // Not the final summary line — e.g. echoed progress text.
        assert_eq!(parse_test_summary("5 passed, 1 failed in 0.1s"), None);
    }

    #[test]
    fn pytest_short_rail_run_is_rejected() {
        assert_eq!(parse_test_summary("== 5 passed =="), None);
    }

    #[test]
    fn pytest_warnings_summary_rail_without_counts_is_rejected() {
        assert_eq!(
            parse_test_summary("========= warnings summary ========="),
            None
        );
    }

    // ── honesty: no match / truncation / overflow ────────────────────────

    #[test]
    fn non_test_output_is_none() {
        for out in [
            "src/lib.rs\nsrc/main.rs\n",                               // ls
            "On branch main\nnothing to commit, working tree clean\n", // git
            "d34db33f fix: things\ncafe1234 feat: stuff\n",            // git log
            "",                                                        // empty
            "we ran 42 tests and all passed",                          // conversation text
            "Tests passed: 5",                                         // near-miss prefix
            "test result: ok. 5 passed, 0 failed",                     // libtest needs ';'
        ] {
            assert_eq!(parse_test_summary(out), None, "input: {out:?}");
        }
    }

    #[test]
    fn truncated_summary_line_is_none_not_half_parsed() {
        // Tool outputs get capped; a summary cut mid-token must not yield
        // partial counts.
        for out in [
            "test result: ok. 5 pas",
            "test result: ok. 5",
            "test result: ok. ",
            "test result:",
            "Tests: 3 skipped, 13 pas",
            "Tests: 13 passed, 2", // dangling count, no label
            "========= 12 passed, 1 xf",
        ] {
            assert_eq!(parse_test_summary(out), None, "input: {out:?}");
        }
    }

    #[test]
    fn cargo_line_missing_a_count_is_rejected() {
        // libtest always prints both counts; either one missing means the
        // output was cut — never fabricate the zero.
        for out in [
            "test result: ok. 5 passed",
            "test result: FAILED. 2 failed",
            "test result: ok. 99999999999 passed; 0 failed;",
        ] {
            assert_eq!(parse_test_summary(out), None, "input: {out:?}");
        }
    }

    #[test]
    fn count_overflowing_u32_rejects_the_line() {
        assert_eq!(
            parse_test_summary("Tests: 99999999999 passed, 3 failed, 2 total"),
            None
        );
    }

    // ── last match wins ──────────────────────────────────────────────────

    #[test]
    fn last_summary_across_runners_wins() {
        // An earlier vitest run, then the final cargo run: the terminal
        // aggregate is the cargo verdict.
        let out = "Tests: 9 passed, 9 total\n\
                   test result: ok. 4 passed; 1 failed; 0 ignored; finished in 1.00s";
        assert_eq!(parse_test_summary(out), Some((4, 5)));
    }
}
