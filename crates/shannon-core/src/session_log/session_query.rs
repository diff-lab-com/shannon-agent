//! # Session Query (the read-side single source)
//!
//! The one adapter cross-session consumers use to answer "what happened in
//! recent sessions?" over the real L0 layout
//! (`<container>/<uuid>/events.jsonl` + sidecars). Before this module,
//! skill-pattern detection and the dream pass's session excerpts each
//! hand-rolled a **flat `sessions/*.json` single-document** reader that
//! production never writes — both silently scanned zero sessions (adversarial
//! review §2.1 F1). Both consumers now go through here, so layout, window,
//! and archive semantics can only evolve in one place.
//!
//! - [`SessionQuery::list_recent`]: the recent-session window, served from
//!   [`SessionStore::list`] (the E-9 index keeps it O(sessions)) and
//!   filtered by the projected `updated_at`. Archived sessions (the
//!   `<id>/curation.json` sidecar's `archived` flag, Task 2's archive MVP)
//!   are excluded unless `include_archived` is set.
//! - [`SessionQuery::tool_calls`]: the per-session tool-call sequence
//!   (name + **sorted argument keys**) detection needs. Values are never
//!   read — argument keys only, so no file path, token, or other secret can
//!   leak into derived artifacts.
//! - [`SessionQuery::user_texts`]: the user-prompt texts the dream excerpts
//!   redact and size-cap on top.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use uuid::Uuid;

use serde::{Deserialize, Serialize};
use shannon_types::session_event::{SessionEventBody, UserMessagePayload};

use super::SessionStore;

/// One recent session resolved from the store listing
/// ([`SessionQuery::list_recent`]).
#[derive(Debug, Clone, PartialEq)]
pub struct SessionRef {
    /// Owning session id (the directory name under the container).
    pub session_id: Uuid,
    /// The session directory itself (`<container>/<id>/`).
    pub dir: PathBuf,
    /// Last activity projected from the log (latest event timestamp).
    pub updated_at: DateTime<Utc>,
}

/// One tool call projected from a `tool/call` event
/// ([`SessionQuery::tool_calls`]).
#[derive(Debug, Clone, PartialEq)]
pub struct SessionToolCall {
    /// Tool that was invoked.
    pub tool_name: String,
    /// The call's argument **keys**, sorted. Values are dropped on purpose:
    /// signatures built from these keys must never carry file paths, tokens,
    /// or other user secrets into candidates or hashes.
    pub arg_keys: Vec<String>,
}

/// Aggregate per-tool invocation stats across the recent-session window
/// ([`SessionQuery::tool_call_stats`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallStat {
    /// Tool name exactly as the log carries it (`Bash`, `skill_<id>`,
    /// `mcp__<server>__<tool>`, …) — bucketing is the caller's policy.
    pub name: String,
    /// Number of `tool/call` events seen for the tool.
    pub calls: u64,
    /// Sum of the per-call `tokens_used` attribution over the tool's events
    /// (0 while no event carries the field — see [`SessionQuery::tool_call_stats`]).
    pub total_tokens: u64,
}

/// Read-only query adapter over one sessions container (the single source
/// both desktop consumers share). Cheap to clone; the underlying
/// [`SessionStore`] is a path handle.
#[derive(Debug, Clone)]
pub struct SessionQuery {
    store: SessionStore,
}

impl SessionQuery {
    /// Point the query at a sessions container (e.g. `~/.shannon/sessions`).
    pub fn new(container: impl Into<PathBuf>) -> Self {
        Self {
            store: SessionStore::new(container),
        }
    }

    /// The underlying store (for consumers that also need curated sidecars
    /// or event reads).
    pub fn store(&self) -> &SessionStore {
        &self.store
    }

    /// The configured container directory.
    pub fn container(&self) -> &Path {
        self.store.container()
    }

    /// Sessions active within `days_back` days, most recently active first.
    ///
    /// Listing rides [`SessionStore::list`] (E-9 index sidecars; a missing
    /// or stale cache falls back to a full projection and is rebuilt
    /// opportunistically), the window filters on the projected `updated_at`,
    /// and sessions whose curation sidecar marks them `archived` are dropped
    /// unless `include_archived` is set. Missing curation files default to
    /// "not archived", so nothing disappears before Task 2 writes flags.
    ///
    /// Degraded mode: if the store listing itself fails (one unreadable log
    /// can fail the full-projection path), the listing falls back to
    /// directory mtimes — age accuracy degrades from event timestamps to
    /// filesystem mtimes, but a single corrupt log can never zero the whole
    /// input layer both desktop consumers share.
    pub fn list_recent(
        &self,
        days_back: u32,
        include_archived: bool,
    ) -> Result<Vec<SessionRef>, super::SessionStoreError> {
        // Checked: a window wide enough to underflow the representable date
        // range means "everything", so the cutoff clamps to the minimum
        // instead of panicking in the subtraction.
        let cutoff = Utc::now()
            .checked_sub_signed(chrono::Duration::days(i64::from(days_back)))
            .unwrap_or(DateTime::<Utc>::MIN_UTC);
        let listed = match self.store.list() {
            Ok(infos) => infos
                .into_iter()
                .map(|info| (info.session_id, info.updated_at))
                .collect::<Vec<_>>(),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    container = %self.store.container().display(),
                    "session query: store listing failed; falling back to directory mtimes"
                );
                super::scan_session_summaries(self.store.container())
                    .into_iter()
                    .filter_map(|entry| {
                        let session_id = uuid::Uuid::parse_str(&entry.session_id).ok()?;
                        let mtime = entry
                            .events_path
                            .metadata()
                            .and_then(|m| m.modified())
                            .ok()?;
                        Some((session_id, DateTime::<Utc>::from(mtime)))
                    })
                    .collect()
            }
        };
        Ok(listed
            .into_iter()
            .filter(|(_, updated_at)| *updated_at >= cutoff)
            .filter(|(session_id, _)| include_archived || !self.store.curation(session_id).archived)
            .map(|(session_id, updated_at)| SessionRef {
                dir: self.store.container().join(session_id.to_string()),
                session_id,
                updated_at,
            })
            .collect())
    }

    /// One targeted session by id, regardless of its curation `archived`
    /// flag and regardless of the recency window — the explicit include for
    /// consumers that already know exactly which session they want (T4: the
    /// post-archive dream pass distills the session that was just archived,
    /// whose flag is set by the time the pass gathers). `Ok(None)` when the
    /// session has no log; a log that fails to parse is an `Err` (callers
    /// decide per-session whether that is fatal).
    pub fn session_by_id(
        &self,
        session_id: &Uuid,
    ) -> Result<Option<SessionRef>, super::SessionStoreError> {
        let Some(events) = self.store.read_events(session_id)? else {
            return Ok(None);
        };
        let updated_at = events
            .last()
            .map(|event| super::session_store::ns_to_datetime(event.ts_ns))
            .unwrap_or_else(Utc::now);
        Ok(Some(SessionRef {
            dir: self.store.container().join(session_id.to_string()),
            session_id: *session_id,
            updated_at,
        }))
    }

    /// The session's tool-call sequence in encounter order, projected from
    /// the `tool/call` events of its log. Empty when the session has no log
    /// or logged no tool calls.
    pub fn tool_calls(
        &self,
        session_id: &Uuid,
    ) -> Result<Vec<SessionToolCall>, super::SessionStoreError> {
        let Some(events) = self.store.read_events(session_id)? else {
            return Ok(Vec::new());
        };
        Ok(events
            .iter()
            .filter_map(|event| match &event.body {
                SessionEventBody::ToolCall(call) => Some(SessionToolCall {
                    tool_name: call.tool_name.clone(),
                    arg_keys: sorted_arg_keys(&call.arguments),
                }),
                _ => None,
            })
            .collect())
    }

    /// Aggregate per-tool invocation stats over the recent sessions
    /// (X7: the Extensions page's "30 天调用 N 次 · ~X tokens" subtext).
    ///
    /// The scan is read-only over the logs the adapter already owns:
    ///
    /// - Sessions enter via [`SessionQuery::list_recent`] with
    ///   `include_archived = false` — the same window + archive policy
    ///   every other consumer here rides, bounded by `days_back`;
    /// - **per-event window**: a long-lived session must not drag its whole
    ///   history into "recent", so a tool event only counts when its own
    ///   `ts_ns` is at or after the same cutoff `list_recent` computes;
    /// - calls are counted from `tool/call` events (the same rows
    ///   [`SessionQuery::tool_calls`] projects), keyed by tool name only —
    ///   arguments are never read;
    /// - token cost sums the per-tool `tokens_used` attribution
    ///   (`shannon-types` `events::ToolResultPayload`) wherever a tool event
    ///   carries it. The L0 writer does not persist that field today, so
    ///   sums read 0 until a writer emits it — this aggregation is already
    ///   shaped for it and must not change when one does.
    ///
    /// Error/degradation policy (deliberately softer than
    /// [`SessionQuery::tool_calls`], which fails a session on any malformed
    /// line): stats are advisory, so a malformed line is **skipped and
    /// warned about** (once per session, with the skipped-line count) and an
    /// unreadable or vanished log costs only that session's contribution —
    /// one bad file can never zero the whole aggregation. The `Err` arm is
    /// reserved for the listing itself.
    ///
    /// Deterministic order: most-called first, ties broken by name.
    pub fn tool_call_stats(
        &self,
        days_back: u64,
    ) -> Result<Vec<ToolCallStat>, super::SessionStoreError> {
        // `list_recent` windows on u32 days; a wider u64 ask simply means
        // "everything", so saturate instead of erroring. The cutoff is
        // computed with checked arithmetic: a window so wide that
        // `now - window` underflows the representable range (or whose
        // cutoff sits before the epoch, where a naive `as u64` would wrap
        // and filter everything out) degenerates to `cutoff_ns = 0` —
        // every logged event counts, exactly what "everything" means.
        let window_days = u32::try_from(days_back).unwrap_or(u32::MAX);
        let cutoff_ns = Utc::now()
            .checked_sub_signed(chrono::Duration::days(i64::from(window_days)))
            .and_then(|cutoff| cutoff.timestamp_nanos_opt())
            .map(|ns| ns.max(0) as u64)
            .unwrap_or(0);

        let mut stats: BTreeMap<String, ToolCallStat> = BTreeMap::new();
        for session in self.list_recent(window_days, false)? {
            scan_session_tool_rows(&session.dir.join("events.jsonl"), cutoff_ns, &mut stats);
        }
        let mut rows: Vec<ToolCallStat> = stats.into_values().collect();
        rows.sort_by(|a, b| b.calls.cmp(&a.calls).then_with(|| a.name.cmp(&b.name)));
        Ok(rows)
    }

    /// The session's user-prompt texts in log order (only events whose
    /// source is [`UserMessagePayload::SOURCE_USER`]; tool results are their
    /// own events and never appear here). Whitespace-only prompts are
    /// skipped. Unredacted — callers that send these texts off the machine
    /// (the dream excerpt path) must pass them through their redactor.
    pub fn user_texts(&self, session_id: &Uuid) -> Result<Vec<String>, super::SessionStoreError> {
        let Some(events) = self.store.read_events(session_id)? else {
            return Ok(Vec::new());
        };
        Ok(events
            .iter()
            .filter_map(|event| match &event.body {
                SessionEventBody::UserMessage(payload)
                    if payload.source == UserMessagePayload::SOURCE_USER =>
                {
                    Some(payload.content.clone())
                }
                _ => None,
            })
            .filter(|text| !text.trim().is_empty())
            .collect())
    }

    /// The curation sidecar for one session (`archived` flag; missing file
    /// → default). Exposed so consumers answer curation questions from the
    /// same handle that serves content.
    pub fn curation(&self, session_id: &Uuid) -> super::SessionCuration {
        self.store.curation(session_id)
    }

    /// Persist the curation sidecar for one session (Task 2's archive MVP
    /// is the writer; exposed here so the read adapter is also the write
    /// seam for lifecycle flags).
    pub fn save_curation(
        &self,
        session_id: &Uuid,
        curation: &super::SessionCuration,
    ) -> Result<(), super::SessionStoreError> {
        self.store.save_curation(session_id, curation)
    }
}

/// Sorted keys of one tool call's serialized `arguments` JSON object.
/// Anything but a JSON object (or an unparsable payload) yields no keys —
/// a malformed log line must never fail a detection or excerpt run.
fn sorted_arg_keys(arguments: &str) -> Vec<String> {
    match serde_json::from_str::<serde_json::Value>(arguments) {
        Ok(serde_json::Value::Object(map)) => {
            let mut keys: Vec<String> = map.keys().cloned().collect();
            keys.sort();
            keys
        }
        _ => Vec::new(),
    }
}

/// Fold one session log's tool rows into `stats` (the raw half of
/// [`SessionQuery::tool_call_stats`]).
///
/// Lines are parsed leniently (`serde_json::Value`, not the typed reader):
/// stats must also survive rows the typed schema will only grow into
/// (extra fields such as `tokens_used`) and rows a truncated/corrupted
/// write left behind — those are skipped, never fatal. Only two row kinds
/// are touched, and only three fields of each: `kind`, `ts_ns`,
/// `tool_name` (plus the optional `tokens_used` summand). Anything else in
/// the line — argument values, outputs — is never surfaced.
fn scan_session_tool_rows(
    log: &Path,
    cutoff_ns: u64,
    stats: &mut BTreeMap<String, ToolCallStat>,
) {
    let Ok(file) = std::fs::File::open(log) else {
        // A vanished log costs only this session's contribution.
        return;
    };
    let mut skipped = 0u64;
    for line in std::io::BufRead::lines(std::io::BufReader::new(file)) {
        let Ok(line) = line else {
            skipped += 1;
            continue;
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(row) = serde_json::from_str::<serde_json::Value>(trimmed) else {
            skipped += 1;
            continue;
        };
        // Window: without a usable event timestamp the row cannot be
        // attributed to the window, so it does not count (logged with the
        // rest of the skipped tally below).
        let Some(ts_ns) = row.get("ts_ns").and_then(serde_json::Value::as_u64) else {
            skipped += 1;
            continue;
        };
        if ts_ns < cutoff_ns {
            continue;
        }
        match row.get("kind").and_then(serde_json::Value::as_str) {
            Some("tool/call") => {
                if let Some(name) = tool_row_name(&row) {
                    stats
                        .entry(name.clone())
                        .or_insert_with(|| ToolCallStat {
                            name,
                            calls: 0,
                            total_tokens: 0,
                        })
                        .calls += 1;
                }
            }
            Some("tool/result") => {
                if let Some(tokens) = row.get("tokens_used").and_then(serde_json::Value::as_u64) {
                    if let Some(name) = tool_row_name(&row) {
                        stats
                            .entry(name.clone())
                            .or_insert_with(|| ToolCallStat {
                                name,
                                calls: 0,
                                total_tokens: 0,
                            })
                            .total_tokens += tokens;
                    }
                }
            }
            _ => {}
        }
    }
    if skipped > 0 {
        tracing::warn!(
            skipped,
            log = %log.display(),
            "tool_call_stats: skipped unreadable rows in session log"
        );
    }
}

/// The `tool_name` of a raw tool row, cloned out as an owned String.
fn tool_row_name(row: &serde_json::Value) -> Option<String> {
    row.get("tool_name")
        .and_then(serde_json::Value::as_str)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

// ============================================================================
// Tests (real-layout smoke: fixtures through the real write path only)
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::session_log::{SessionCuration, SessionLogWriter};
    use shannon_types::session_event::{
        SessionStartPayload, TokenUsage, ToolCallPayload, ToolResultPayload, TurnEndPayload,
        TurnStartPayload,
    };

    /// Seed one realistic session through the real writer: session/start, a
    /// framed turn with two user prompts, a bash call (args as a JSON
    /// object) and a read call with **unsorted** argument keys, plus a tool
    /// result so the projection machinery sees a full turn.
    fn seed_session(store: &SessionStore, id: &Uuid, prompt_prefix: &str) {
        let mut w = SessionLogWriter::open_layout(store.container(), &id.to_string()).unwrap();
        w.record(SessionEventBody::SessionStart(SessionStartPayload {
            model: "test-model".into(),
            provider: Some("anthropic".into()),
            cwd: Some("/proj".into()),
            app_version: None,
            ..Default::default()
        }));
        w.record(SessionEventBody::TurnStart(TurnStartPayload {
            query_id: None,
        }));
        w.record(SessionEventBody::UserMessage(UserMessagePayload {
            source: UserMessagePayload::SOURCE_USER.into(),
            content: format!("{prompt_prefix} deploy with api_key: sk-secret"),
            attachment_count: 0,
        }));
        w.record(SessionEventBody::UserMessage(UserMessagePayload {
            source: "tool".into(),
            content: "synthetic non-user row".into(),
            attachment_count: 0,
        }));
        w.record(SessionEventBody::ToolCall(ToolCallPayload {
            tool_use_id: "u1".into(),
            tool_name: "Bash".into(),
            arguments: r#"{"command":"ls","cwd":"/tmp"}"#.into(),
        }));
        w.record(SessionEventBody::ToolResult(ToolResultPayload {
            tool_use_id: "u1".into(),
            tool_name: "Bash".into(),
            output: "out".into(),
            is_error: false,
            duration_ms: Some(3),
            meta: serde_json::Value::Null,
        }));
        w.record(SessionEventBody::ToolCall(ToolCallPayload {
            tool_use_id: "u2".into(),
            tool_name: "read_file".into(),
            arguments: r#"{"mode":"r","path":"/x"}"#.into(),
        }));
        w.record(SessionEventBody::TurnEnd(TurnEndPayload {
            reason: TurnEndPayload::REASON_COMPLETED.into(),
            usage: Some(TokenUsage {
                input_tokens: 11,
                output_tokens: 7,
                cache_creation_tokens: 0,
                cache_read_tokens: 0,
                cost_usd: None,
            }),
            error: None,
        }));
        w.close().unwrap();
    }

    #[test]
    fn list_recent_window_and_order_over_real_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        let query = SessionQuery::new(tmp.path().join("sessions"));
        for _ in 0..3 {
            seed_session(query.store(), &Uuid::new_v4(), "prompt");
        }
        // A foreign directory sharing the container is invisible.
        let junk = query.container().join("not-a-uuid");
        std::fs::create_dir_all(&junk).unwrap();
        std::fs::write(junk.join("events.jsonl"), "").unwrap();

        let refs = query.list_recent(7, false).unwrap();
        assert_eq!(refs.len(), 3, "all three fresh sessions in the window");
        assert!(refs.windows(2).all(|w| w[0].updated_at >= w[1].updated_at));
        for r in &refs {
            assert_eq!(r.dir, query.container().join(r.session_id.to_string()));
        }

        // days_back = 0 puts the cutoff at "now": every session (written
        // strictly before the call) falls out of the window.
        assert!(
            query.list_recent(0, false).unwrap().is_empty(),
            "the window must actually filter"
        );
    }

    #[test]
    fn archived_sessions_are_invisible_unless_include_archived() {
        let tmp = tempfile::tempdir().unwrap();
        let query = SessionQuery::new(tmp.path().join("sessions"));
        let visible = Uuid::new_v4();
        let archived = Uuid::new_v4();
        seed_session(query.store(), &visible, "keep");
        seed_session(query.store(), &archived, "hide");
        // Default curation (missing file) never archives anything.
        assert!(!query.curation(&visible).archived);
        query
            .save_curation(&archived, &SessionCuration { archived: true })
            .unwrap();

        let default_view = query.list_recent(7, false).unwrap();
        assert_eq!(default_view.len(), 1);
        assert_eq!(default_view[0].session_id, visible);

        let everything = query.list_recent(7, true).unwrap();
        assert_eq!(everything.len(), 2, "include_archived lifts the filter");
        // The archived session stays on disk and fully readable.
        assert!(!query.tool_calls(&archived).unwrap().is_empty());
        assert!(!query.user_texts(&archived).unwrap().is_empty());
    }

    #[test]
    fn session_by_id_fetches_a_targeted_session_ignoring_the_flag() {
        // T4 (final review F1): the explicit include must reach a session the
        // default window path excludes because it is archived.
        let tmp = tempfile::tempdir().unwrap();
        let query = SessionQuery::new(tmp.path().join("sessions"));
        let id = Uuid::new_v4();
        seed_session(query.store(), &id, "distill me");
        query
            .save_curation(&id, &SessionCuration { archived: true })
            .unwrap();
        assert!(query.curation(&id).archived);

        assert_eq!(
            query.list_recent(7, false).unwrap().len(),
            0,
            "the archived session is invisible to the window path"
        );
        let fetched = query.session_by_id(&id).unwrap().expect("targeted fetch");
        assert_eq!(fetched.session_id, id);
        assert_eq!(
            fetched.dir,
            query.container().join(id.to_string()),
            "the ref points at the session's real directory"
        );
        // Its content reads exactly like any other session's.
        assert!(
            query
                .user_texts(&id)
                .unwrap()
                .iter()
                .all(|t| t.starts_with("distill me"))
        );

        // An id with no log is `Ok(None)`, never an error.
        assert!(query.session_by_id(&Uuid::new_v4()).unwrap().is_none());
    }

    #[test]
    fn tool_calls_project_name_and_sorted_arg_keys_from_real_events() {
        let tmp = tempfile::tempdir().unwrap();
        let query = SessionQuery::new(tmp.path().join("sessions"));
        let id = Uuid::new_v4();
        seed_session(query.store(), &id, "go");

        let calls = query.tool_calls(&id).unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].tool_name, "Bash");
        assert_eq!(calls[0].arg_keys, vec!["command", "cwd"]);
        assert_eq!(
            calls[1].arg_keys,
            vec!["mode", "path"],
            "argument keys must come out sorted"
        );

        // A session with no log reads as empty, never an error.
        assert!(query.tool_calls(&Uuid::new_v4()).unwrap().is_empty());
    }

    #[test]
    fn user_texts_keep_only_user_source_and_non_empty_rows() {
        let tmp = tempfile::tempdir().unwrap();
        let query = SessionQuery::new(tmp.path().join("sessions"));
        let id = Uuid::new_v4();
        seed_session(query.store(), &id, "please");

        let texts = query.user_texts(&id).unwrap();
        assert_eq!(texts.len(), 1, "the non-user-source row is dropped");
        assert!(texts[0].starts_with("please"));
        assert!(
            texts[0].contains("api_key: sk-secret"),
            "unredacted at this layer — redaction is the caller's duty"
        );
    }

    #[test]
    fn aged_sessions_drop_out_of_the_window_by_event_timestamp() {
        let tmp = tempfile::tempdir().unwrap();
        let query = SessionQuery::new(tmp.path().join("sessions"));
        let id = Uuid::new_v4();
        seed_session(query.store(), &id, "old");
        assert_eq!(query.list_recent(7, false).unwrap().len(), 1);

        // Age the session by rewriting its real log's event timestamps to 40
        // days ago (the shape production sessions age into). The file was
        // produced by the real writer above; only ts_ns moves.
        let log = query.container().join(id.to_string()).join("events.jsonl");
        let old_ns = (Utc::now() - chrono::Duration::days(40))
            .timestamp_nanos_opt()
            .unwrap() as u64;
        let aged: Vec<String> = std::fs::read_to_string(&log)
            .unwrap()
            .lines()
            .map(|line| {
                let mut event: shannon_types::session_event::SessionEvent =
                    serde_json::from_str(line).unwrap();
                event.ts_ns = old_ns;
                serde_json::to_string(&event).unwrap()
            })
            .collect();
        std::fs::write(&log, aged.join("\n") + "\n").unwrap();

        assert!(
            query.list_recent(7, false).unwrap().is_empty(),
            "a 40-day-old session is outside the 7-day window"
        );
        assert_eq!(
            query.list_recent(90, false).unwrap().len(),
            1,
            "…but inside a 90-day window"
        );
    }

    #[test]
    fn one_corrupt_log_degrades_the_listing_instead_of_failing_it() {
        let tmp = tempfile::tempdir().unwrap();
        let query = SessionQuery::new(tmp.path().join("sessions"));
        let healthy = Uuid::new_v4();
        let corrupt = Uuid::new_v4();
        seed_session(query.store(), &healthy, "fine");
        seed_session(query.store(), &corrupt, "broken");
        // Corrupt one log after the writer closed it (its E-9 index is now
        // stale, so the store listing takes the full-projection path and
        // fails on the malformed line).
        let log = query
            .container()
            .join(corrupt.to_string())
            .join("events.jsonl");
        let mut raw = std::fs::read_to_string(&log).unwrap();
        raw.push_str("{not json\n");
        std::fs::write(&log, raw).unwrap();

        // The listing itself degrades to directory mtimes — both sessions
        // stay visible to consumers (per-session reads skip the bad one).
        let refs = query.list_recent(7, false).unwrap();
        let ids: Vec<Uuid> = refs.iter().map(|r| r.session_id).collect();
        assert!(ids.contains(&healthy), "healthy session must survive");
        assert!(ids.contains(&corrupt), "the corrupt session still lists");
        // The healthy session's content reads normally; the corrupt one is
        // empty rather than an error.
        assert!(!query.tool_calls(&healthy).unwrap().is_empty());
        assert!(query.tool_calls(&corrupt).is_err());
    }

    // ── X7: per-tool invocation stats ────────────────────────────────────

    use std::io::Write as _;

    /// Append one raw (handcrafted) event line to a session's real log —
    /// the same shape the writer emits, plus fields a future writer may
    /// grow into (the typed payload cannot express `tokens_used` yet).
    fn append_raw(query: &SessionQuery, id: &Uuid, raw: &str) {
        let log = query.container().join(id.to_string()).join("events.jsonl");
        let mut f = std::fs::OpenOptions::new().append(true).open(log).unwrap();
        writeln!(f, "{raw}").unwrap();
    }

    /// One raw `tool/call` / `tool/result` row, "now" by default.
    fn raw_tool_row(kind: &str, name: &str, ts_ns: u64, tokens: Option<u64>) -> String {
        let tokens_field = tokens
            .map(|t| format!(r#","tokens_used":{t}"#))
            .unwrap_or_default();
        format!(
            r#"{{"seq":999,"ts_ns":{ts_ns},"session_id":"raw-appended","turn":1,"kind":"{kind}","tool_use_id":"raw-1","tool_name":"{name}","output":"","is_error":false,"meta":null{tokens_field}}}"#
        )
    }

    fn now_ns() -> u64 {
        Utc::now().timestamp_nanos_opt().unwrap() as u64
    }

    fn stats_by_name(stats: &[ToolCallStat]) -> std::collections::HashMap<String, ToolCallStat> {
        stats.iter().cloned().map(|s| (s.name.clone(), s)).collect()
    }

    #[test]
    fn tool_call_stats_counts_calls_and_sums_tokens_across_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        let query = SessionQuery::new(tmp.path().join("sessions"));
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        // Each seeded session carries one Bash + one read_file tool/call.
        seed_session(query.store(), &a, "a");
        seed_session(query.store(), &b, "b");
        let ts = now_ns();
        append_raw(&query, &a, &raw_tool_row("tool/call", "mcp__notion__search", ts, None));
        append_raw(&query, &a, &raw_tool_row("tool/result", "mcp__notion__search", ts, Some(42)));
        append_raw(&query, &b, &raw_tool_row("tool/call", "skill_deploy", ts, None));
        append_raw(&query, &b, &raw_tool_row("tool/result", "skill_deploy", ts, Some(7)));
        // A result without the token field must not disturb the sums.
        append_raw(&query, &b, &raw_tool_row("tool/result", "skill_deploy", ts, None));

        let by_name = stats_by_name(&query.tool_call_stats(7).unwrap());
        assert_eq!(
            by_name["Bash"],
            ToolCallStat { name: "Bash".into(), calls: 2, total_tokens: 0 }
        );
        assert_eq!(
            by_name["read_file"],
            ToolCallStat { name: "read_file".into(), calls: 2, total_tokens: 0 }
        );
        assert_eq!(
            by_name["mcp__notion__search"],
            ToolCallStat { name: "mcp__notion__search".into(), calls: 1, total_tokens: 42 }
        );
        assert_eq!(
            by_name["skill_deploy"],
            ToolCallStat { name: "skill_deploy".into(), calls: 1, total_tokens: 7 }
        );

        // Deterministic order: most-called first, ties by name.
        let rows = query.tool_call_stats(7).unwrap();
        assert_eq!(rows[0].name, "Bash");
        assert_eq!(rows[1].name, "read_file");

        // Archived sessions fall out of the default view like everywhere
        // else in this adapter.
        query
            .save_curation(&b, &SessionCuration { archived: true })
            .unwrap();
        let by_name = stats_by_name(&query.tool_call_stats(7).unwrap());
        assert_eq!(by_name["Bash"].calls, 1, "only session a's Bash remains");
        assert!(!by_name.contains_key("skill_deploy"));
    }

    #[test]
    fn tool_call_stats_windows_by_event_timestamp() {
        let tmp = tempfile::tempdir().unwrap();
        let query = SessionQuery::new(tmp.path().join("sessions"));
        let id = Uuid::new_v4();
        seed_session(query.store(), &id, "fresh"); // Bash + read_file, recent
        let old_ns = (Utc::now() - chrono::Duration::days(40))
            .timestamp_nanos_opt()
            .unwrap() as u64;
        // Two 40-day-old tool rows share the log with the fresh ones.
        append_raw(&query, &id, &raw_tool_row("tool/call", "Bash", old_ns, None));
        append_raw(&query, &id, &raw_tool_row("tool/call", "skill_old", old_ns, None));

        let by_name = stats_by_name(&query.tool_call_stats(7).unwrap());
        assert_eq!(by_name["Bash"].calls, 1, "aged Bash row is outside the window");
        assert!(!by_name.contains_key("skill_old"));

        let by_name = stats_by_name(&query.tool_call_stats(90).unwrap());
        assert_eq!(by_name["Bash"].calls, 2, "wider window re-admits the aged row");
        assert_eq!(by_name["skill_old"].calls, 1);

        // days_back = 0 puts the cutoff at "now": nothing counts.
        assert!(query.tool_call_stats(0).unwrap().is_empty());
    }

    #[test]
    fn tool_call_stats_survive_corrupt_rows_and_empty_containers() {
        let tmp = tempfile::tempdir().unwrap();
        let query = SessionQuery::new(tmp.path().join("sessions"));
        // An empty container aggregates to an empty, non-error answer.
        assert!(query.tool_call_stats(30).unwrap().is_empty());

        let id = Uuid::new_v4();
        seed_session(query.store(), &id, "go");
        let log = query.container().join(id.to_string()).join("events.jsonl");
        let mut raw = std::fs::read_to_string(&log).unwrap();
        // Corrupted write + a well-formed row missing its timestamp: both
        // must be skipped without failing (or zeroing) the aggregation.
        raw.push_str("{not json\n");
        raw.push_str(
            r#"{"seq":998,"session_id":"x","turn":1,"kind":"tool/call","tool_use_id":"u","tool_name":"Bash"}"#,
        );
        raw.push('\n');
        std::fs::write(&log, raw).unwrap();

        let by_name = stats_by_name(&query.tool_call_stats(7).unwrap());
        assert_eq!(
            by_name["Bash"],
            ToolCallStat { name: "Bash".into(), calls: 1, total_tokens: 0 },
            "the session's healthy rows still count"
        );
        assert_eq!(by_name["read_file"].calls, 1);
    }

    // M9 review fix: a `days_back` so wide that `now - window` underflows
    // the representable date range must saturate to "scan everything",
    // not panic in the subtraction (the "saturate instead of erroring"
    // contract, now actually true).
    #[test]
    fn tool_call_stats_huge_days_back_degenerates_to_a_full_scan_without_panicking() {
        let tmp = tempfile::tempdir().unwrap();
        let query = SessionQuery::new(tmp.path().join("sessions"));
        let id = Uuid::new_v4();
        seed_session(query.store(), &id, "any age"); // Bash + read_file

        // u64::MAX saturates to u32::MAX days ≈ 11.7M years — far outside
        // DateTime's representable range, the exact panic trigger.
        let by_name = stats_by_name(&query.tool_call_stats(u64::MAX).unwrap());
        assert_eq!(by_name["Bash"].calls, 1, "full scan: every event counts");
        assert_eq!(by_name["read_file"].calls, 1);

        // Same for the u32 edge itself and for a value that survives the
        // subtraction but lands before the epoch (where a naive `as u64`
        // cutoff would wrap and silently filter everything out).
        let by_name = stats_by_name(&query.tool_call_stats(u64::from(u32::MAX)).unwrap());
        assert_eq!(by_name["Bash"].calls, 1);
        let by_name = stats_by_name(&query.tool_call_stats(60 * 365).unwrap()).remove("Bash").unwrap();
        assert_eq!(by_name.calls, 1, "pre-epoch cutoffs still count every event");
    }
}
