//! Inbox write/resolve seam for the three T5 "needs attention" sources.
//!
//! The unified inbox stream (proposal §3.2 T5) folds three event families
//! into the SQLite inbox alongside the automation sources:
//!
//! - `session_approval` — an interactive permission prompt is waiting on the
//!   user. Written next to the `PERMISSION_REQUEST` emit in
//!   `commands_permissions::prompt_user` (the same origin the session rail's
//!   amber dot derives from), resolved when the prompt settles (answer,
//!   timeout auto-deny, or dropped channel) — still inside `prompt_user`.
//! - `session_failed` — the session's most recent turn failed. Written at the
//!   two `QUERY_FAILED` emit sites of the interactive `send_message` stream
//!   loop (the same events the rail's red dot derives from), resolved when a
//!   later turn completes or the user opens the session
//!   (`commands_sessions::switch_session`).
//! - `skill_candidate` — pattern detection appended candidates. Written in
//!   `skill_pattern_detection::trigger_skill_pattern_detection`, right where
//!   `skill-candidates-changed` fires; resolved (archived) by the
//!   approve/reject commands in `commands_skill_candidates`.
//! - `dream_report` — a dream distillation pass finished and its report is
//!   ready. Written in `commands_dream::execute_dream_pass` when the pass
//!   completes; day-deduplicated (`dream-{YYYY-MM-DD}`), never resolved by
//!   this module — the Triage read interaction consumes it.
//!
//! Dedup contract (store-level `upsert_pending` / `resolve_by_source`):
//! approvals and failures key on the session (`source_id` = session id, one
//! live entry per session per kind), candidates key on the candidate id,
//! dream reports key on the calendar day. Everything here is **best-effort**:
//! an inbox write failure is logged and dropped so the interactive query /
//! permission paths can never fail because of it. Generic over
//! `tauri::Runtime` so tests drive it with `tauri::test::mock_app()`.

use shannon_core::inbox_store::{
    InboxItem, InboxItemNew, InboxStatus, InboxStore, SOURCE_DREAM_REPORT, SOURCE_SESSION_APPROVAL,
    SOURCE_SESSION_FAILED, SOURCE_SKILL_CANDIDATE,
};
use tauri::Emitter;

use crate::commands::AppState;
use crate::events::event_names;

/// Summary/error cap — matches the routine-run inbox items (`inbox_commands`).
const SUMMARY_MAX_CHARS: usize = 500;

/// Best-effort session display title from the desktop session list.
///
/// `state.sessions` is the display list (`SessionMeta`, kept current by
/// `new_session` / auto-title / rename). Falls back to the generated
/// placeholder shape (`Session {uuid-prefix}`) when the session is unknown.
pub(crate) async fn session_display_title(state: &AppState, session_id: &str) -> String {
    let sessions = state.sessions.lock().await;
    let title = sessions
        .iter()
        .find(|s| s.id == session_id)
        .map(|s| s.title.clone())
        .unwrap_or_else(|| {
            let prefix = session_id.split('-').next().unwrap_or(session_id);
            format!("Session {prefix}")
        });
    truncate_chars(title.trim(), 80)
}

/// Emit the inbox refresh signal (best-effort — mirrors `inbox_commands`).
fn notify<R: tauri::Runtime>(app: &tauri::AppHandle<R>, tag: &str) {
    let _ = app.emit(event_names::INBOX_UPDATED, tag.to_string());
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    format!("{cut}…")
}

/// First non-empty line, trimmed and capped — failure summaries usually live
/// on one line; the full text still goes into `error`.
fn first_error_line(error: &str) -> String {
    let line = error
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    truncate_chars(line, 200)
}

// ── session_approval ────────────────────────────────────────────────────

/// Record (or refresh) the session's pending-approval entry. Called right
/// after the `PERMISSION_REQUEST` emit for session-scoped prompts.
pub(crate) fn record_session_approval<R: tauri::Runtime>(
    inbox: &InboxStore,
    app: &tauri::AppHandle<R>,
    session_id: &str,
    session_title: &str,
    tool: &str,
    risk: &str,
) -> Option<InboxItem> {
    let item = inbox
        .upsert_pending(InboxItemNew {
            source: SOURCE_SESSION_APPROVAL.into(),
            // Dedup entity = the session: one live approval entry per session.
            source_id: Some(session_id.to_string()),
            session_id: Some(session_id.to_string()),
            title: truncate_chars(tool, 120),
            summary: format!(
                "Session \u{201c}{session_title}\u{201d} asked to run \u{201c}{tool}\u{201d} ({risk} risk)"
            ),
            error: None,
        })
        .map_err(|e| e.to_string());
    match item {
        Ok(saved) => {
            notify(app, session_id);
            Some(saved)
        }
        Err(e) => {
            tracing::warn!(session = %session_id, error = %e, "inbox: failed to record session approval");
            None
        }
    }
}

/// Mark the session's approval entry read. Fired when the prompt settles —
/// user answer, timeout auto-deny, or dropped channel all resolve it.
pub(crate) fn resolve_session_approval<R: tauri::Runtime>(
    inbox: &InboxStore,
    app: &tauri::AppHandle<R>,
    session_id: &str,
) -> Option<InboxItem> {
    resolve(
        inbox,
        app,
        SOURCE_SESSION_APPROVAL,
        session_id,
        InboxStatus::Read,
    )
}

// ── session_failed ──────────────────────────────────────────────────────

/// Record (or refresh) the session's failed-turn entry. Called at the
/// `QUERY_FAILED` emit sites of the interactive stream loop.
pub(crate) fn record_session_failure<R: tauri::Runtime>(
    inbox: &InboxStore,
    app: &tauri::AppHandle<R>,
    session_id: &str,
    session_title: &str,
    error: &str,
) -> Option<InboxItem> {
    let error_trimmed = error.trim();
    if error_trimmed.is_empty() {
        return None;
    }
    let item = inbox
        .upsert_pending(InboxItemNew {
            source: SOURCE_SESSION_FAILED.into(),
            // Dedup entity = the session: one live failure entry per session.
            source_id: Some(session_id.to_string()),
            session_id: Some(session_id.to_string()),
            title: truncate_chars(session_title, 120),
            summary: first_error_line(error_trimmed),
            error: Some(truncate_chars(error_trimmed, SUMMARY_MAX_CHARS)),
        })
        .map_err(|e| e.to_string());
    match item {
        Ok(saved) => {
            notify(app, session_id);
            Some(saved)
        }
        Err(e) => {
            tracing::warn!(session = %session_id, error = %e, "inbox: failed to record session failure");
            None
        }
    }
}

/// Mark the session's failure entry read — the failure has been handled:
/// either a later turn succeeded, or the user opened the session
/// (`commands_sessions::switch_session`) and has seen it.
pub(crate) fn resolve_session_failure<R: tauri::Runtime>(
    inbox: &InboxStore,
    app: &tauri::AppHandle<R>,
    session_id: &str,
) -> Option<InboxItem> {
    resolve(
        inbox,
        app,
        SOURCE_SESSION_FAILED,
        session_id,
        InboxStatus::Read,
    )
}

// ── skill_candidate ─────────────────────────────────────────────────────

/// Record (or refresh) the entry for an appended skill candidate.
pub(crate) fn record_skill_candidate<R: tauri::Runtime>(
    inbox: &InboxStore,
    app: &tauri::AppHandle<R>,
    candidate: &crate::commands_skill_candidates::SkillCandidate,
) -> Option<InboxItem> {
    let item = inbox
        .upsert_pending(InboxItemNew {
            source: SOURCE_SKILL_CANDIDATE.into(),
            // Dedup entity = the candidate id (stable sig-hash).
            source_id: Some(candidate.id.clone()),
            // No live session behind a detected pattern — the candidate is
            // reviewed on its own, not continued in a chat.
            session_id: None,
            title: truncate_chars(&candidate.proposed_name, 120),
            summary: format!(
                "{} \u{00b7} {} occurrence(s)",
                candidate.proposed_trigger, candidate.occurrence_count
            ),
            error: None,
        })
        .map_err(|e| e.to_string());
    match item {
        Ok(saved) => {
            notify(app, &candidate.id);
            Some(saved)
        }
        Err(e) => {
            tracing::warn!(candidate = %candidate.id, error = %e, "inbox: failed to record skill candidate");
            None
        }
    }
}

/// Archive the entry for an approved/rejected candidate — the candidate no
/// longer exists, so the entry leaves the pending stream entirely.
pub(crate) fn resolve_skill_candidate<R: tauri::Runtime>(
    inbox: &InboxStore,
    app: &tauri::AppHandle<R>,
    candidate_id: &str,
) -> Option<InboxItem> {
    resolve(
        inbox,
        app,
        SOURCE_SKILL_CANDIDATE,
        candidate_id,
        InboxStatus::Archived,
    )
}

// ── dream_report ────────────────────────────────────────────────────────

/// Record (or refresh) the dream distillation report card.
///
/// Dedup entity = the calendar day (`dream-{YYYY-MM-DD}`, derived from the
/// report timestamp), so the daily noise budget is at most one card: two
/// passes on the same day refresh the same entry instead of adding a second.
/// `session_id` stays `None` — a dream report is reviewed on its own, not
/// continued in a chat. No resolve function: the card is consumed by the
/// Triage read interaction like any other report item.
pub(crate) fn record_dream_report<R: tauri::Runtime>(
    inbox: &InboxStore,
    app: &tauri::AppHandle<R>,
    result: &crate::commands_dream::DreamPassResult,
    report_ts: &str,
) -> Option<InboxItem> {
    // Day key of the report: unix seconds → `YYYY-MM-DD` (UTC), falling back
    // to today when the ts is not parseable.
    let date = report_ts
        .parse::<i64>()
        .ok()
        .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d").to_string());
    let item = inbox
        .upsert_pending(InboxItemNew {
            source: SOURCE_DREAM_REPORT.into(),
            // Dedup entity = the day: ≤1 dream card per day.
            source_id: Some(format!("dream-{date}")),
            session_id: None,
            title: truncate_chars("Dream distillation report", 120),
            summary: format!(
                "Merged {} \u{00b7} Removed {} \u{00b7} New insights {} \u{00b7} {} session(s) scanned",
                result.merge_proposed, result.remove_proposed, result.add_proposed,
                result.scanned_sessions
            ),
            error: None,
        })
        .map_err(|e| e.to_string());
    match item {
        Ok(saved) => {
            notify(app, &format!("dream-{date}"));
            Some(saved)
        }
        Err(e) => {
            tracing::warn!(report_ts = %report_ts, error = %e, "inbox: failed to record dream report");
            None
        }
    }
}

// ── shared resolve ──────────────────────────────────────────────────────

fn resolve<R: tauri::Runtime>(
    inbox: &InboxStore,
    app: &tauri::AppHandle<R>,
    source: &str,
    source_id: &str,
    status: InboxStatus,
) -> Option<InboxItem> {
    let resolved = inbox
        .resolve_by_source(source, source_id, status)
        .map_err(|e| e.to_string());
    match resolved {
        Ok(Some(item)) => {
            notify(app, source_id);
            Some(item)
        }
        Ok(None) => None,
        Err(e) => {
            tracing::warn!(source = %source, entity = %source_id, error = %e, "inbox: failed to resolve entry");
            None
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use shannon_core::inbox_store::{SOURCE_SESSION_APPROVAL, SOURCE_SESSION_FAILED};

    fn store() -> std::sync::Arc<InboxStore> {
        std::sync::Arc::new(InboxStore::open_in_memory().unwrap())
    }

    fn by_source(inbox: &InboxStore, source: &str, id: &str) -> InboxItem {
        inbox.find_by_source(source, id).unwrap().unwrap()
    }

    // ── 写入 (write) ─────────────────────────────────────────────────────

    #[test]
    fn approval_write_creates_session_scoped_pending_entry() {
        let app = tauri::test::mock_app();
        let inbox = store();
        let item = record_session_approval(
            &inbox,
            app.handle(),
            "sess-approve-1",
            "Refactor the parser",
            "bash",
            "high",
        )
        .unwrap();
        assert_eq!(item.source, SOURCE_SESSION_APPROVAL);
        assert_eq!(item.status, "pending");
        assert_eq!(item.session_id.as_deref(), Some("sess-approve-1"));
        assert_eq!(item.source_id.as_deref(), Some("sess-approve-1"));
        assert_eq!(item.title, "bash");
        assert!(item.summary.contains("Refactor the parser"));
        assert!(item.summary.contains("bash"));
        let stored = by_source(&inbox, SOURCE_SESSION_APPROVAL, "sess-approve-1");
        assert_eq!(stored.id, item.id);
    }

    #[test]
    fn failure_write_carries_session_title_and_error() {
        let app = tauri::test::mock_app();
        let inbox = store();
        let item = record_session_failure(
            &inbox,
            app.handle(),
            "sess-fail-1",
            "Fix the flaky test",
            "provider unreachable: connection refused\nretry loop exhausted",
        )
        .unwrap();
        assert_eq!(item.source, SOURCE_SESSION_FAILED);
        assert_eq!(item.status, "pending");
        assert_eq!(item.title, "Fix the flaky test");
        assert_eq!(item.summary, "provider unreachable: connection refused");
        assert!(item.error.unwrap().contains("retry loop exhausted"));
    }

    #[test]
    fn failure_write_with_empty_error_is_skipped() {
        let app = tauri::test::mock_app();
        let inbox = store();
        assert!(
            record_session_failure(&inbox, app.handle(), "sess-x", "t", "   \n  ").is_none(),
            "no error text → no entry"
        );
        assert!(inbox.list(None, None, 10).unwrap().is_empty());
    }

    #[test]
    fn candidate_write_keys_on_candidate_id() {
        let app = tauri::test::mock_app();
        let inbox = store();
        let candidate = crate::commands_skill_candidates::SkillCandidate {
            id: "sig-abcd1234".into(),
            detected_at: "2026-09-23T00:00:00Z".into(),
            occurrence_count: 5,
            example_session_ids: vec![],
            proposed_name: "bash".into(),
            proposed_trigger: "Detected bash call recurring across 3 session(s)".into(),
            procedure: vec![],
            source_tool_calls: vec![],
            refined: false,
        };
        let item = record_skill_candidate(&inbox, app.handle(), &candidate).unwrap();
        assert_eq!(item.source, SOURCE_SKILL_CANDIDATE);
        assert_eq!(item.source_id.as_deref(), Some("sig-abcd1234"));
        assert!(
            item.session_id.is_none(),
            "detected patterns have no live session"
        );
        assert_eq!(item.title, "bash");
        assert!(item.summary.contains("5 occurrence(s)"));
    }

    // ── 去重 (dedup) ─────────────────────────────────────────────────────

    #[test]
    fn repeated_failures_for_one_session_collapse_to_one_entry() {
        let app = tauri::test::mock_app();
        let inbox = store();
        record_session_failure(
            &inbox,
            app.handle(),
            "sess-dup",
            "Session dup",
            "first boom",
        )
        .unwrap();
        let second = record_session_failure(
            &inbox,
            app.handle(),
            "sess-dup",
            "Session dup",
            "second boom",
        )
        .unwrap();
        let all = inbox.list(None, None, 10).unwrap();
        assert_eq!(all.len(), 1, "no duplicate rows per (session, kind)");
        assert_eq!(all[0].id, second.id);
        assert_eq!(all[0].summary, "second boom", "content refreshed");
    }

    #[test]
    fn approval_and_failure_for_one_session_are_separate_entries() {
        let app = tauri::test::mock_app();
        let inbox = store();
        record_session_failure(&inbox, app.handle(), "sess-both", "Session both", "boom").unwrap();
        record_session_approval(
            &inbox,
            app.handle(),
            "sess-both",
            "Session both",
            "bash",
            "high",
        )
        .unwrap();
        assert_eq!(inbox.list(None, None, 10).unwrap().len(), 2);
    }

    // ── 解决即已读 (resolve-as-read) ─────────────────────────────────────

    #[test]
    fn approval_resolve_marks_entry_read() {
        let app = tauri::test::mock_app();
        let inbox = store();
        record_session_approval(
            &inbox,
            app.handle(),
            "sess-res",
            "Session res",
            "bash",
            "low",
        )
        .unwrap();
        let resolved = resolve_session_approval(&inbox, app.handle(), "sess-res").unwrap();
        assert_eq!(resolved.status, "read");
        // Settling again (e.g. duplicate resolve) is a no-op, not an error.
        let again = resolve_session_approval(&inbox, app.handle(), "sess-res").unwrap();
        assert_eq!(again.status, "read");
    }

    #[test]
    fn failure_resolves_read_on_success_and_reopens_on_new_failure() {
        let app = tauri::test::mock_app();
        let inbox = store();
        record_session_failure(
            &inbox,
            app.handle(),
            "sess-cycle",
            "Session cycle",
            "boom #1",
        )
        .unwrap();
        // New turn succeeds → mark read.
        let resolved = resolve_session_failure(&inbox, app.handle(), "sess-cycle").unwrap();
        assert_eq!(resolved.status, "read");
        // Another turn fails → the same entry re-opens (needs attention).
        let reopened = record_session_failure(
            &inbox,
            app.handle(),
            "sess-cycle",
            "Session cycle",
            "boom #2",
        )
        .unwrap();
        assert_eq!(reopened.id, resolved.id);
        assert_eq!(reopened.status, "pending");
        assert_eq!(reopened.summary, "boom #2");
    }

    #[test]
    fn opening_a_session_resolves_its_failure_entry() {
        // `switch_session` calls exactly this seam when the user opens the
        // session — covered here against the same store + emit path.
        let app = tauri::test::mock_app();
        let inbox = store();
        record_session_failure(&inbox, app.handle(), "sess-open", "Session open", "boom").unwrap();
        let resolved = resolve_session_failure(&inbox, app.handle(), "sess-open").unwrap();
        assert_eq!(resolved.status, "read");
    }

    #[test]
    fn candidate_resolve_archives_and_is_tolerant_of_missing_entries() {
        let app = tauri::test::mock_app();
        let inbox = store();
        // Resolving an unknown candidate must be a silent no-op.
        assert!(resolve_skill_candidate(&inbox, app.handle(), "sig-nope").is_none());
        let candidate = crate::commands_skill_candidates::SkillCandidate {
            id: "sig-gone".into(),
            detected_at: "2026-09-23T00:00:00Z".into(),
            occurrence_count: 2,
            example_session_ids: vec![],
            proposed_name: "edit_file".into(),
            proposed_trigger: "Detected edit_file call".into(),
            procedure: vec![],
            source_tool_calls: vec![],
            refined: false,
        };
        record_skill_candidate(&inbox, app.handle(), &candidate).unwrap();
        let archived = resolve_skill_candidate(&inbox, app.handle(), "sig-gone").unwrap();
        assert_eq!(archived.status, "archived");
        assert_eq!(inbox.stats().unwrap().pending, 0);
    }

    #[test]
    fn resolve_is_silent_when_nothing_matches() {
        let app = tauri::test::mock_app();
        let inbox = store();
        assert!(resolve_session_approval(&inbox, app.handle(), "sess-none").is_none());
        assert!(resolve_session_failure(&inbox, app.handle(), "sess-none").is_none());
    }

    // ── dream_report ─────────────────────────────────────────────────────

    fn dream_result(
        merge: u32,
        remove: u32,
        add: u32,
        scanned: u32,
    ) -> crate::commands_dream::DreamPassResult {
        crate::commands_dream::DreamPassResult {
            skipped_reason: None,
            scanned_sessions: scanned,
            projects: vec!["/work/app".into()],
            merge_proposed: merge,
            remove_proposed: remove,
            add_proposed: add,
            candidates_detected: 0,
            candidates_refined: 0,
            proposal_ids: vec!["proposal-1".into()],
            report_path: None,
            duration_ms: 42,
        }
    }

    #[test]
    fn dream_write_carries_source_day_key_and_summary() {
        let app = tauri::test::mock_app();
        let inbox = store();
        let item = record_dream_report(
            &inbox,
            app.handle(),
            &dream_result(4, 2, 3, 7),
            "1758768000", // 2025-09-25 UTC
        )
        .unwrap();
        assert_eq!(item.source, SOURCE_DREAM_REPORT);
        assert_eq!(item.status, "pending");
        assert_eq!(item.source_id.as_deref(), Some("dream-2025-09-25"));
        assert!(item.session_id.is_none(), "no live session behind a report");
        assert_eq!(item.title, "Dream distillation report");
        assert_eq!(
            item.summary,
            "Merged 4 \u{00b7} Removed 2 \u{00b7} New insights 3 \u{00b7} 7 session(s) scanned"
        );
        let stored = by_source(&inbox, SOURCE_DREAM_REPORT, "dream-2025-09-25");
        assert_eq!(stored.id, item.id);
    }

    #[test]
    fn dream_reports_on_one_day_dedup_to_a_single_card() {
        let app = tauri::test::mock_app();
        let inbox = store();
        // Two runs on the same UTC day (both ts fall on 1970-01-01): the
        // second refreshes the first — daily noise budget ≤1 card.
        record_dream_report(&inbox, app.handle(), &dream_result(1, 0, 0, 2), "100").unwrap();
        let second =
            record_dream_report(&inbox, app.handle(), &dream_result(4, 2, 3, 7), "200").unwrap();
        assert_eq!(second.source_id.as_deref(), Some("dream-1970-01-01"));
        let all = inbox.list(None, None, 10).unwrap();
        assert_eq!(all.len(), 1, "one card per day");
        assert_eq!(
            all[0].summary,
            "Merged 4 \u{00b7} Removed 2 \u{00b7} New insights 3 \u{00b7} 7 session(s) scanned",
            "content refreshed by the later run"
        );
        // A different day gets its own card.
        record_dream_report(&inbox, app.handle(), &dream_result(0, 0, 0, 1), "172800").unwrap();
        assert_eq!(inbox.list(None, None, 10).unwrap().len(), 2);
    }

    #[test]
    fn dream_write_with_unparseable_ts_falls_back_to_today() {
        let app = tauri::test::mock_app();
        let inbox = store();
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let item =
            record_dream_report(&inbox, app.handle(), &dream_result(0, 0, 0, 0), "oops").unwrap();
        assert_eq!(
            item.source_id.as_deref(),
            Some(format!("dream-{today}").as_str())
        );
    }

    // ── helpers ──────────────────────────────────────────────────────────

    #[test]
    fn first_error_line_picks_the_first_non_empty_line() {
        assert_eq!(first_error_line("\n  boom  \nnext"), "boom");
        assert_eq!(first_error_line(""), "");
    }

    #[test]
    fn truncate_chars_is_utf8_safe() {
        let cut = truncate_chars("🦀🦀🦀", 2);
        assert_eq!(cut.chars().count(), 3); // 2 + ellipsis
    }
}
