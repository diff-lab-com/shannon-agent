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

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use uuid::Uuid;

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
        let cutoff = Utc::now() - chrono::Duration::days(i64::from(days_back));
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
}
