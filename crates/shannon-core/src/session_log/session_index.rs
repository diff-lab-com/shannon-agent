//! # Session Index Sidecar (E-9)
//!
//! A per-session derived-stats cache next to the authoritative log:
//!
//! ```text
//! <container>/<uuid>/events.jsonl   # authoritative log (L0)
//! <container>/<uuid>/index.json     # this file: projection stats cache
//! ```
//!
//! `SessionStore::list()` used to decode and project **every** session's
//! full `events.jsonl` — O(total bytes of all logs) per picker open (audit
//! E-9; WP-15 measured 30s+ on large containers). This sidecar caches the
//! handful of projection-derived numbers the picker needs so `list()` is
//! O(sessions), falling back to the full projection whenever the cache is
//! missing or stale.
//!
//! Contract:
//!
//! - **Pure cache.** Every field is recomputable from `events.jsonl`; losing
//!   or deleting `index.json` never loses session state. All stats are
//!   produced by [`SessionIndexAccumulator`] — the *same* code serves the
//!   live writer (incremental O(1) per event) and the rebuild path (fold a
//!   fully-read event slice), so the two can never drift.
//! - **Self-invalidating.** The index records the `events.jsonl` byte length
//!   and mtime it was computed over; readers re-stat and discard the cache
//!   on mismatch. Raw rewrites ([`SessionStore::truncate_to_turn`] /
//!   `rewrite_with_conversation`) and crash windows therefore degrade to one
//!   full rebuild on the next `list()`, never to wrong answers.
//! - **Never partial.** The writer only republishes the index when it could
//!   seed its accumulator from a complete base: a valid prior index over the
//!   exact prefix it resumed, or an empty log. Otherwise it stays silent and
//!   lets the next `list()` rebuild — a wrong-but-plausible cache is the one
//!   unacceptable outcome.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use shannon_types::session_event::{SessionEvent, SessionEventBody};

/// Index schema version. Bump on any field-semantics change; readers drop
/// indexes with a foreign version and rebuild.
pub const SESSION_INDEX_VERSION: u32 = 1;

/// File name of the index sidecar within a session directory.
pub const SESSION_INDEX_FILE_NAME: &str = "index.json";

/// The index sidecar path for a given `events.jsonl` path (its sibling).
pub fn index_path_for(events_path: &Path) -> PathBuf {
    events_path.with_file_name(SESSION_INDEX_FILE_NAME)
}

/// How much of a user-message body the index retains for previews.
///
/// The picker previews are `truncate_preview(text, 80)`; keeping the first
/// 256 **chars** reproduces that truncation exactly (any 256-char prefix is
/// at least 80 bytes long, and prefix-truncation composes) while bounding
/// `index.json` size for prompts that embed pasted files.
const PREVIEW_CAPTURE_CHARS: usize = 256;

/// Derived projection stats for one session log (see [module docs](self)).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionIndex {
    /// Schema version ([`SESSION_INDEX_VERSION`]).
    pub version: u32,
    /// `events.jsonl` byte length the stats were computed over.
    pub log_len: u64,
    /// `events.jsonl` mtime (ns since epoch) at index time — the second
    /// invalidation signal alongside [`Self::log_len`].
    pub log_mtime_ns: u64,
    /// Number of events folded into the stats (== last seq + 1 for the
    /// seq-continuous logs the writer produces).
    pub event_count: u64,
    /// First event timestamp (ns since epoch); 0 for an empty log.
    pub created_at_ns: u64,
    /// Last event timestamp (ns since epoch); 0 for an empty log.
    pub updated_at_ns: u64,
    /// Model captured from the first `session/start` within the first 8
    /// events (mirrors `SessionStore::assemble`).
    pub model: String,
    /// Working directory from the same `session/start`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_path: Option<String>,
    /// Number of `turn/start` events (== `ConversationProjection::turn_count`).
    pub turn_count: usize,
    /// Summed input tokens over `turn/end` usage payloads.
    pub total_input_tokens: u64,
    /// Summed output tokens over `turn/end` usage payloads.
    pub total_output_tokens: u64,
    /// First `user/message` body (seq + content prefix), for the
    /// first-user preview.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_user_message: Option<IndexedUserText>,
    /// Last `user/message` body (seq + content prefix).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_user_message: Option<IndexedUserText>,
    /// Seq of the first / last `tool/result` events. The projection folds
    /// tool results into *user-role* messages, so when a tool result lands
    /// before the first (or after the last) `user/message`, the corresponding
    /// preview is `None` — exactly what `preview(&messages, ..)` computes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_tool_result_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_tool_result_seq: Option<u64>,
}

/// A captured user-message body: its event seq plus a bounded content prefix.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexedUserText {
    pub seq: u64,
    pub content: String,
}

impl SessionIndex {
    /// Load the index at `index_path`, but only when it is still a faithful
    /// cache of the log at `events_path` (parseable, current schema, and the
    /// log's length + mtime both match). `None` means "rebuild".
    pub fn load_if_valid(events_path: &Path, index_path: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(index_path).ok()?;
        let index: SessionIndex = serde_json::from_str(&text).ok()?;
        if index.version != SESSION_INDEX_VERSION {
            return None;
        }
        let (log_len, log_mtime_ns) = stat_len_mtime(events_path)?;
        if index.log_len != log_len || index.log_mtime_ns != log_mtime_ns {
            return None;
        }
        Some(index)
    }

    /// Atomically persist the index (tmp file + rename, mirroring the meta
    /// sidecar's write discipline).
    pub fn store(&self, index_path: &Path) -> std::io::Result<()> {
        if let Some(parent) = index_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json =
            serde_json::to_string_pretty(self).map_err(|e| std::io::Error::other(e.to_string()))?;
        let tmp = index_path.with_extension("json.tmp");
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(json.as_bytes())?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, index_path)
    }

    /// Text for the first-user preview (`preview(&messages, false, ..)`):
    /// `None` when the first projected user-role message is a tool result.
    pub fn first_preview_text(&self) -> Option<&str> {
        let first = self.first_user_message.as_ref()?;
        match self.first_tool_result_seq {
            Some(tr) if tr < first.seq => None,
            _ => Some(&first.content),
        }
    }

    /// Text for the last-user preview (`preview(&messages, true, ..)`):
    /// `None` when the last projected user-role message is a tool result.
    pub fn last_preview_text(&self) -> Option<&str> {
        let last = self.last_user_message.as_ref()?;
        match self.last_tool_result_seq {
            Some(tr) if tr > last.seq => None,
            _ => Some(&last.content),
        }
    }
}

/// `(len, mtime_ns)` of a file, or `None` when it cannot be stat'd.
pub fn stat_len_mtime(path: &Path) -> Option<(u64, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime_ns = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    Some((meta.len(), mtime_ns))
}

/// Incremental builder for [`SessionIndex`].
///
/// One implementation serves both producers, which is what keeps the cache
/// honest:
///
/// - the live [`SessionLogWriter`](super::SessionLogWriter) seeds it from the
///   previous valid index at open and feeds each recorded event in O(1);
/// - the rebuild path in `SessionStore::list` folds a fully-read event slice
///   through the same `observe`.
///
/// Field semantics deliberately mirror `ConversationProjection::fold` (the
/// restore projection) — `turn_count` counts `turn/start` events only, token
/// totals come only from `turn/end` usage payloads, and every `user/message`
/// event becomes a user text message regardless of its source.
#[derive(Debug)]
pub struct SessionIndexAccumulator {
    event_count: u64,
    first_ts_ns: u64,
    last_ts_ns: u64,
    model: String,
    project_path: Option<String>,
    turn_count: usize,
    total_input_tokens: u64,
    total_output_tokens: u64,
    first_user_message: Option<IndexedUserText>,
    last_user_message: Option<IndexedUserText>,
    first_tool_result_seq: Option<u64>,
    last_tool_result_seq: Option<u64>,
    /// False when seeded against a log whose earlier events this accumulator
    /// never saw (no valid prior index to continue from): `finish` must then
    /// refuse to emit an index rather than publish partial stats.
    complete_base: bool,
}

impl SessionIndexAccumulator {
    /// An empty accumulator over an empty log.
    pub fn fresh() -> Self {
        Self {
            event_count: 0,
            first_ts_ns: 0,
            last_ts_ns: 0,
            model: String::new(),
            project_path: None,
            turn_count: 0,
            total_input_tokens: 0,
            total_output_tokens: 0,
            first_user_message: None,
            last_user_message: None,
            first_tool_result_seq: None,
            last_tool_result_seq: None,
            complete_base: true,
        }
    }

    /// Seed from a previous index over the same log.
    pub fn from_index(previous: &SessionIndex) -> Self {
        Self {
            event_count: previous.event_count,
            first_ts_ns: previous.created_at_ns,
            last_ts_ns: previous.updated_at_ns,
            model: previous.model.clone(),
            project_path: previous.project_path.clone(),
            turn_count: previous.turn_count,
            total_input_tokens: previous.total_input_tokens,
            total_output_tokens: previous.total_output_tokens,
            first_user_message: previous.first_user_message.clone(),
            last_user_message: previous.last_user_message.clone(),
            first_tool_result_seq: previous.first_tool_result_seq,
            last_tool_result_seq: previous.last_tool_result_seq,
            complete_base: true,
        }
    }

    /// Switch to the "partial base" state: earlier events were never folded,
    /// so [`Self::finish`] will decline to publish.
    pub fn mark_base_partial(&mut self) {
        self.complete_base = false;
    }

    /// True while no event has been folded yet.
    pub fn is_empty(&self) -> bool {
        self.event_count == 0
    }

    /// Fold one event. Must be called in log order (append order), which is
    /// how both producers iterate.
    pub fn observe(&mut self, event: &SessionEvent) {
        let seq = event.seq;
        if self.event_count == 0 {
            self.first_ts_ns = event.ts_ns;
        }
        self.last_ts_ns = event.ts_ns;
        self.event_count += 1;
        match &event.body {
            // SessionStore::assemble scans only the first 8 events for the
            // session/start row; the event_count guard reproduces that window.
            SessionEventBody::SessionStart(p) if self.model.is_empty() && self.event_count <= 8 => {
                self.model.clone_from(&p.model);
                self.project_path.clone_from(&p.cwd);
            }
            SessionEventBody::TurnStart(_) => self.turn_count += 1,
            SessionEventBody::TurnEnd(p) => {
                if let Some(usage) = &p.usage {
                    self.total_input_tokens += usage.input_tokens;
                    self.total_output_tokens += usage.output_tokens;
                }
            }
            SessionEventBody::UserMessage(p) => {
                let entry = IndexedUserText {
                    seq,
                    content: capture_prefix(&p.content),
                };
                // First stays the first; last always moves. A single message
                // therefore populates both, matching preview(..., last=true)
                // over a one-prompt conversation.
                if self.first_user_message.is_none() {
                    self.first_user_message = Some(entry.clone());
                }
                self.last_user_message = Some(entry);
            }
            SessionEventBody::ToolResult(_) => {
                if self.first_tool_result_seq.is_none() {
                    self.first_tool_result_seq = Some(seq);
                }
                self.last_tool_result_seq = Some(seq);
            }
            _ => {}
        }
    }

    /// Finish against `events_path` as of `(log_len, log_mtime_ns)` — stat
    /// the file at the moment its content matches what was folded (writers:
    /// after the final flush; rebuilders: before reading the events).
    ///
    /// Returns `None` when the base was partial ([`Self::mark_base_partial`])
    /// or the log cannot be stat'd; callers then simply leave the cache
    /// absent and let the next `list()` rebuild it.
    pub fn finish(self, stat: Option<(u64, u64)>) -> Option<SessionIndex> {
        if !self.complete_base {
            return None;
        }
        let (log_len, log_mtime_ns) = stat?;
        Some(SessionIndex {
            version: SESSION_INDEX_VERSION,
            log_len,
            log_mtime_ns,
            event_count: self.event_count,
            created_at_ns: self.first_ts_ns,
            updated_at_ns: self.last_ts_ns,
            model: self.model,
            project_path: self.project_path,
            turn_count: self.turn_count,
            total_input_tokens: self.total_input_tokens,
            total_output_tokens: self.total_output_tokens,
            first_user_message: self.first_user_message,
            last_user_message: self.last_user_message,
            first_tool_result_seq: self.first_tool_result_seq,
            last_tool_result_seq: self.last_tool_result_seq,
        })
    }
}

/// Keep at most [`PREVIEW_CAPTURE_CHARS`] chars of a message body.
fn capture_prefix(text: &str) -> String {
    if text.chars().count() <= PREVIEW_CAPTURE_CHARS {
        return text.to_string();
    }
    text.chars().take(PREVIEW_CAPTURE_CHARS).collect()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use shannon_types::session_event::{
        AssistantChunkPayload, SessionStartPayload, TokenUsage, ToolCallPayload, ToolResultPayload,
        TurnEndPayload, TurnStartPayload, UserMessagePayload,
    };

    fn event(seq: u64, body: SessionEventBody) -> SessionEvent {
        SessionEvent {
            seq,
            ts_ns: 1_000 + seq * 10,
            session_id: "idx-test".into(),
            turn: 1,
            step: None,
            span_id: None,
            parent_span_id: None,
            body,
        }
    }

    fn user_msg(seq: u64, content: &str) -> SessionEvent {
        event(
            seq,
            SessionEventBody::UserMessage(UserMessagePayload {
                source: UserMessagePayload::SOURCE_USER.into(),
                content: content.into(),
                attachment_count: 0,
            }),
        )
    }

    fn chunk(seq: u64, delta: &str) -> SessionEvent {
        event(
            seq,
            SessionEventBody::AssistantChunk(AssistantChunkPayload {
                delta: delta.into(),
                thinking: false,
            }),
        )
    }

    fn tool_result(seq: u64) -> SessionEvent {
        event(
            seq,
            SessionEventBody::ToolResult(ToolResultPayload {
                tool_use_id: "u1".into(),
                tool_name: "Bash".into(),
                output: "out".into(),
                is_error: false,
                duration_ms: Some(1),
                meta: serde_json::Value::Null,
            }),
        )
    }

    fn turn_start(seq: u64) -> SessionEvent {
        event(
            seq,
            SessionEventBody::TurnStart(TurnStartPayload { query_id: None }),
        )
    }

    fn turn_end(seq: u64, input: u64, output: u64) -> SessionEvent {
        event(
            seq,
            SessionEventBody::TurnEnd(TurnEndPayload {
                reason: TurnEndPayload::REASON_COMPLETED.into(),
                usage: Some(TokenUsage {
                    input_tokens: input,
                    output_tokens: output,
                    cache_creation_tokens: 0,
                    cache_read_tokens: 0,
                    cost_usd: None,
                }),
                error: None,
            }),
        )
    }

    /// Fold `events` through the accumulator AND the real projection; the
    /// accumulated stats must equal what the restore projection reports.
    fn assert_matches_projection(events: &[SessionEvent]) {
        let mut acc = SessionIndexAccumulator::fresh();
        for e in events {
            acc.observe(e);
        }
        let index = acc.finish(Some((123, 456))).unwrap();
        let proj = super::super::projections::project_conversation(events);

        assert_eq!(index.turn_count, proj.turn_count);
        assert_eq!(index.total_input_tokens, proj.total_input_tokens);
        assert_eq!(index.total_output_tokens, proj.total_output_tokens);

        // Replicate session_store::preview() over the projected messages and
        // compare with the index's preview answers.
        let first_preview = proj
            .messages
            .iter()
            .find(|m| m.role == "user")
            .and_then(|m| match &m.content {
                shannon_engine::api::MessageContent::Text(t) => Some(t.clone()),
                shannon_engine::api::MessageContent::Blocks(blocks) => {
                    blocks.iter().find_map(|b| match b {
                        shannon_engine::api::ContentBlock::Text { text } => Some(text.clone()),
                        _ => None,
                    })
                }
            });
        let last_preview = proj
            .messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .and_then(|m| match &m.content {
                shannon_engine::api::MessageContent::Text(t) => Some(t.clone()),
                shannon_engine::api::MessageContent::Blocks(blocks) => {
                    blocks.iter().find_map(|b| match b {
                        shannon_engine::api::ContentBlock::Text { text } => Some(text.clone()),
                        _ => None,
                    })
                }
            });
        let clamp = |t: Option<String>| {
            t.map(|t| {
                if t.chars().count() <= PREVIEW_CAPTURE_CHARS {
                    t
                } else {
                    t.chars().take(PREVIEW_CAPTURE_CHARS).collect()
                }
            })
        };
        assert_eq!(
            index.first_preview_text().map(str::to_string),
            clamp(first_preview),
            "first preview mismatch"
        );
        assert_eq!(
            index.last_preview_text().map(str::to_string),
            clamp(last_preview),
            "last preview mismatch"
        );
    }

    #[test]
    fn accumulator_matches_projection_for_tool_session() {
        // prompt → assistant text+tool_use → tool_result → more text → end.
        let events = vec![
            event(
                0,
                SessionEventBody::SessionStart(SessionStartPayload {
                    model: "m1".into(),
                    provider: None,
                    cwd: Some("/p".into()),
                    app_version: None,
                    ..Default::default()
                }),
            ),
            turn_start(1),
            user_msg(2, "hi"),
            chunk(3, "He"),
            chunk(4, "llo"),
            event(
                5,
                SessionEventBody::ToolCall(ToolCallPayload {
                    tool_use_id: "u1".into(),
                    tool_name: "Bash".into(),
                    arguments: "{}".into(),
                }),
            ),
            tool_result(6),
            chunk(7, "done"),
            turn_end(8, 11, 7),
        ];
        assert_matches_projection(&events);
        let mut acc = SessionIndexAccumulator::fresh();
        for e in &events {
            acc.observe(e);
        }
        let index = acc.finish(Some((1, 2))).unwrap();
        // Last projected user-role message is the tool result → no last
        // preview; first is the prompt.
        assert_eq!(index.first_preview_text(), Some("hi"));
        assert_eq!(index.last_preview_text(), None);
        assert_eq!(index.model, "m1");
        assert_eq!(index.project_path.as_deref(), Some("/p"));
        assert_eq!(index.created_at_ns, 1_000);
        assert_eq!(index.updated_at_ns, 1_080);
    }

    #[test]
    fn accumulator_matches_projection_for_plain_session() {
        // No tools: the last user-role message is the prompt itself.
        let events = vec![
            turn_start(0),
            user_msg(1, "question"),
            chunk(2, "answer"),
            turn_end(3, 5, 9),
            turn_start(4),
            user_msg(5, "follow-up"),
            chunk(6, "answer 2"),
            turn_end(7, 3, 4),
        ];
        assert_matches_projection(&events);
        let mut acc = SessionIndexAccumulator::fresh();
        for e in &events {
            acc.observe(e);
        }
        let index = acc.finish(Some((1, 2))).unwrap();
        assert_eq!(index.turn_count, 2);
        assert_eq!(index.total_input_tokens, 8);
        assert_eq!(index.total_output_tokens, 13);
        assert_eq!(index.first_preview_text(), Some("question"));
        assert_eq!(index.last_preview_text(), Some("follow-up"));
        assert_eq!(index.updated_at_ns, 1_070);
    }

    #[test]
    fn accumulator_seeded_from_index_continues_counts() {
        let first = vec![turn_start(0), user_msg(1, "q"), turn_end(2, 5, 9)];
        let mut acc = SessionIndexAccumulator::fresh();
        for e in &first {
            acc.observe(e);
        }
        let index = acc.finish(Some((10, 20))).unwrap();

        // Resume: seed from the published index and fold a second episode.
        let mut acc2 = SessionIndexAccumulator::from_index(&index);
        acc2.observe(&turn_start(3));
        acc2.observe(&user_msg(4, "q2"));
        acc2.observe(&turn_end(5, 7, 1));
        let index2 = acc2.finish(Some((30, 40))).unwrap();

        assert_eq!(index2.turn_count, 2);
        assert_eq!(index2.total_input_tokens, 12);
        assert_eq!(index2.total_output_tokens, 10);
        assert_eq!(index2.event_count, 6);
        assert_eq!(index2.created_at_ns, 1_000, "first-ts survives the seed");
        assert_eq!(index2.last_preview_text(), Some("q2"));

        // And the seeded result equals folding everything at once.
        let mut acc3 = SessionIndexAccumulator::fresh();
        for e in first
            .iter()
            .chain([turn_start(3), user_msg(4, "q2"), turn_end(5, 7, 1)].iter())
        {
            acc3.observe(e);
        }
        assert_eq!(acc3.finish(Some((30, 40))).unwrap(), index2);
    }

    #[test]
    fn partial_base_refuses_to_publish() {
        let mut acc = SessionIndexAccumulator::fresh();
        acc.mark_base_partial();
        acc.observe(&user_msg(0, "only this episode"));
        assert!(acc.finish(Some((1, 2))).is_none());
    }

    #[test]
    fn load_if_valid_rejects_len_mtime_version_drift() {
        let tmp = tempfile::tempdir().unwrap();
        let log = tmp.path().join("events.jsonl");
        std::fs::write(&log, b"x\n").unwrap();
        let index_path = tmp.path().join("index.json");

        let mut acc = SessionIndexAccumulator::fresh();
        acc.observe(&user_msg(0, "hi"));
        let index = acc.finish(stat_len_mtime(&log)).unwrap();
        index.store(&index_path).unwrap();

        // Fresh and matching → Some.
        assert!(SessionIndex::load_if_valid(&log, &index_path).is_some());

        // Content drift → None.
        std::fs::write(&log, b"xy\n").unwrap();
        assert!(SessionIndex::load_if_valid(&log, &index_path).is_none());

        // Restore content, tamper the version → None.
        std::fs::write(&log, b"x\n").unwrap();
        let mut tampered = index.clone();
        tampered.version = 999;
        tampered.store(&index_path).unwrap();
        assert!(SessionIndex::load_if_valid(&log, &index_path).is_none());

        // Garbage index file → None (not a panic).
        std::fs::write(&index_path, "not json").unwrap();
        assert!(SessionIndex::load_if_valid(&log, &index_path).is_none());
    }

    #[test]
    fn preview_capture_is_bounded_and_char_safe() {
        let long = "é".repeat(1_000);
        let mut acc = SessionIndexAccumulator::fresh();
        acc.observe(&user_msg(0, &long));
        let index = acc.finish(Some((1, 2))).unwrap();
        let stored = index.first_user_message.as_ref().unwrap();
        assert_eq!(stored.content.chars().count(), PREVIEW_CAPTURE_CHARS);
        assert!(index.first_preview_text().is_some());
        // All-multibyte content: the stored prefix is a valid char boundary.
        assert!(stored.content.ends_with('é'));
    }
}
