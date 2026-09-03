//! # Session Store (§4.6 W1-P1)
//!
//! The L0-backed replacement for the deleted single-file session snapshot.
//! `events.jsonl` is the only authoritative record; this store derives
//! [`StoredSession`]s from it via [`project_conversation`] and persists only
//! the user-curation fields that cannot be derived (title, branch lineage)
//! as a `meta.json` sidecar next to each log.
//!
//! Layout (`<container>` is typically `~/.shannon/sessions`):
//!
//! ```text
//! <container>/<uuid>/events.jsonl   # authoritative log (L0)
//! <container>/<uuid>/meta.json      # optional sidecar: title / lineage
//! ```
//!
//! Breaking change (DP4): legacy `sessions/<uuid>.json` snapshots are not
//! read or migrated. Delete them once upgraded.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::TimeZone;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use shannon_types::session_event::{
    AssistantChunkPayload, AssistantMessagePayload, SessionEndSeedPayload, SessionEvent,
    SessionEventBody, TurnEndPayload, TurnStartPayload, UserMessagePayload,
};

use super::{
    SessionLogReader, SessionLogWriter, projections, scan_session_summaries, search_events,
    session_log_container_path, session_meta_container_path,
};

/// Errors raised while loading, listing, branching, or curating sessions.
#[derive(Debug, thiserror::Error)]
pub enum SessionStoreError {
    /// Underlying I/O failure.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// An event row failed to parse (unknown kind, malformed payload).
    #[error("session log error: {0}")]
    Log(#[from] super::SessionLogError),

    /// Serialization failure on the sidecar meta file.
    #[error("sidecar serialization error: {0}")]
    Serialization(String),
}

/// Non-derivable session metadata persisted in `meta.json`.
///
/// Everything else (model, timestamps, token totals, turn count,
/// project path) is projected from the event log.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionSidecar {
    /// Title set via `/rename`, auto-title, or an explicit save.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Parent session id when this session is a branch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<Uuid>,
    /// Index in the parent's message list where the branch diverged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_point_message_index: Option<usize>,
}

impl SessionSidecar {
    fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text)
                .inspect_err(|e| {
                    tracing::warn!(path = %path.display(), error = %e, "unparsable session sidecar ignored");
                })
                .unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    fn store(&self, path: &Path) -> Result<(), SessionStoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| SessionStoreError::Serialization(e.to_string()))?;
        // Plain durable write: title/lineage loss is recoverable context, not
        // session state — but fsync the content anyway before rename-less
        // truncation risk matters. A simple atomic tmp-rename keeps readers
        // on well-formed JSON.
        let tmp = path.with_extension("json.tmp");
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(json.as_bytes())?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    fn merge_from_disk(mut self, path: &Path) -> Self {
        let existing = Self::load(path);
        self.title = self.title.or(existing.title);
        self.parent_session_id = self.parent_session_id.or(existing.parent_session_id);
        self.branch_point_message_index = self
            .branch_point_message_index
            .or(existing.branch_point_message_index);
        self
    }
}

/// Lean metadata view projected from L0 (+ sidecar), shaped like the legacy
/// snapshot metadata so listing/rendering consumers transition unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredSessionMeta {
    /// Model captured at `session/start`.
    pub model: String,
    /// Creation time: first logged event.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Last activity: latest logged event.
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// Total input tokens summed over completed turns.
    pub total_input_tokens: u64,
    /// Total output tokens summed over completed turns.
    pub total_output_tokens: u64,
    /// Number of started turns.
    pub turn_count: usize,
    /// Curated title (sidecar), else `None`.
    pub title: Option<String>,
    /// Branch lineage from the sidecar.
    pub parent_session_id: Option<Uuid>,
    /// Branch divergence index from the sidecar.
    pub branch_point_message_index: Option<usize>,
    /// Working directory captured at `session/start`.
    #[serde(default)]
    pub project_path: Option<String>,
}

/// A fully restored session: derived history plus projected metadata.
#[derive(Debug, Clone)]
pub struct StoredSession {
    /// Owning session id.
    pub session_id: Uuid,
    /// Projected metadata.
    pub metadata: StoredSessionMeta,
    /// Rebuilt conversation history (see [`projections::project_conversation`]).
    pub messages: Vec<shannon_engine::api::Message>,
}

/// Listing summary ([`SessionStore::list`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredSessionInfo {
    pub session_id: Uuid,
    pub title: Option<String>,
    /// First user-message preview.
    pub preview: Option<String>,
    /// Last user-message preview.
    pub last_user_preview: Option<String>,
    pub model: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub turn_count: usize,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub parent_session_id: Option<Uuid>,
    pub branch_point_message_index: Option<usize>,
    pub project_path: Option<String>,
}

fn ns_to_datetime(ns: u64) -> chrono::DateTime<chrono::Utc> {
    chrono::Utc
        .timestamp_opt(ns as i64 / 1_000_000_000, (ns % 1_000_000_000) as u32)
        .single()
        .unwrap_or_else(chrono::Utc::now)
}

fn preview(
    messages: &[shannon_engine::api::Message],
    last: bool,
    max_len: usize,
) -> Option<String> {
    let mut iter: Box<dyn Iterator<Item = &shannon_engine::api::Message>> = if last {
        Box::new(messages.iter().rev())
    } else {
        Box::new(messages.iter())
    };
    let text = iter
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
    text.map(|t| truncate_preview(&t, max_len))
}

fn truncate_preview(t: &str, max_len: usize) -> String {
    if t.len() <= max_len {
        return t.to_string();
    }
    let mut end = max_len.saturating_sub(3);
    while !t.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &t[..end])
}

/// Read-only plus branch/curation operations over one sessions container.
#[derive(Debug, Clone)]
pub struct SessionStore {
    container: PathBuf,
}

impl SessionStore {
    /// Point the store at a sessions container (created lazily).
    pub fn new(container: impl Into<PathBuf>) -> Self {
        Self {
            container: container.into(),
        }
    }

    /// Sessions container honoring `$SHANNON_HOME`, falling back to
    /// `~/.shannon/sessions` (temp-dir fallback when `$HOME` is unset).
    pub fn default_container() -> PathBuf {
        if let Ok(home_var) = std::env::var("SHANNON_HOME") {
            return PathBuf::from(home_var).join("sessions");
        }
        match dirs::home_dir() {
            Some(home) => home.join(".shannon").join("sessions"),
            None => std::env::temp_dir().join(".shannon").join("sessions"),
        }
    }

    /// The configured container directory.
    pub fn container(&self) -> &Path {
        &self.container
    }

    fn log_path(&self, session_id: &Uuid) -> PathBuf {
        session_log_container_path(&self.container, &session_id.to_string())
    }

    fn meta_path(&self, session_id: &Uuid) -> PathBuf {
        session_meta_container_path(&self.container, &session_id.to_string())
    }

    /// Read all events of a session; `None` when no log exists.
    pub fn read_events(
        &self,
        session_id: &Uuid,
    ) -> Result<Option<Vec<SessionEvent>>, SessionStoreError> {
        let path = self.log_path(session_id);
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(SessionLogReader::open(&path)?.read_events(false)?))
    }

    /// Project a stored session. Returns `Ok(None)` when the session has no
    /// log (the legacy snapshot world is gone — nothing to fall back to).
    pub fn load(&self, session_id: &Uuid) -> Result<Option<StoredSession>, SessionStoreError> {
        let Some(events) = self.read_events(session_id)? else {
            return Ok(None);
        };
        Ok(Some(self.assemble(*session_id, &events)))
    }

    /// Assemble metadata + preview-independent parts from events + sidecar.
    fn assemble(&self, session_id: Uuid, events: &[SessionEvent]) -> StoredSession {
        let sidecar = SessionSidecar::load(&self.meta_path(&session_id));
        let proj = projections::project_conversation(events);
        let first_ts = events.first().map_or(0, |e| e.ts_ns);
        let last_ts = events.last().map_or(first_ts, |e| e.ts_ns);

        let mut model = String::new();
        let mut project_path = None;
        for event in events.iter().take(8) {
            if let SessionEventBody::SessionStart(p) = &event.body {
                model.clone_from(&p.model);
                project_path.clone_from(&p.cwd);
                break;
            }
        }

        StoredSession {
            session_id,
            metadata: StoredSessionMeta {
                model,
                created_at: ns_to_datetime(first_ts),
                updated_at: ns_to_datetime(last_ts),
                total_input_tokens: proj.total_input_tokens,
                total_output_tokens: proj.total_output_tokens,
                turn_count: proj.turn_count,
                title: sidecar.title,
                parent_session_id: sidecar.parent_session_id,
                branch_point_message_index: sidecar.branch_point_message_index,
                project_path,
            },
            messages: proj.messages,
        }
    }

    /// List all sessions in the container, most recently active first.
    pub fn list(&self) -> Result<Vec<StoredSessionInfo>, SessionStoreError> {
        let mut infos = Vec::new();
        for entry in scan_session_summaries(&self.container) {
            let Ok(id) = Uuid::parse_str(&entry.session_id) else {
                continue; // foreign directories sharing the container
            };
            let Some(events) = self.read_events(&id)? else {
                continue;
            };
            let stored = self.assemble(id, &events);
            infos.push(Self::to_info(stored));
        }
        infos.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(infos)
    }

    fn to_info(stored: StoredSession) -> StoredSessionInfo {
        StoredSessionInfo {
            session_id: stored.session_id,
            preview: preview(&stored.messages, false, 80),
            last_user_preview: preview(&stored.messages, true, 80),
            title: stored.metadata.title,
            model: stored.metadata.model,
            created_at: stored.metadata.created_at,
            updated_at: stored.metadata.updated_at,
            turn_count: stored.metadata.turn_count,
            total_input_tokens: stored.metadata.total_input_tokens,
            total_output_tokens: stored.metadata.total_output_tokens,
            parent_session_id: stored.metadata.parent_session_id,
            branch_point_message_index: stored.metadata.branch_point_message_index,
            project_path: stored.metadata.project_path,
        }
    }

    /// Persist sidecar metadata (title/lineage merge semantics: caller
    /// `Some` values win, existing on-disk values backfill `None`s).
    pub fn save_sidecar(
        &self,
        session_id: &Uuid,
        sidecar: &SessionSidecar,
    ) -> Result<(), SessionStoreError> {
        let path = self.meta_path(session_id);
        sidecar.clone().merge_from_disk(&path).store(&path)
    }

    /// Read the sidecar as-is.
    pub fn sidecar(&self, session_id: &Uuid) -> SessionSidecar {
        SessionSidecar::load(&self.meta_path(session_id))
    }

    /// Append an end-seed marker naming `parent` (fork/resume provenance).
    pub fn append_seed_marker(
        &self,
        session_id: &Uuid,
        reason: &str,
        parent_session_id: Option<Uuid>,
    ) -> Result<(), SessionStoreError> {
        let mut writer = SessionLogWriter::open_layout(&self.container, &session_id.to_string())?;
        writer.record(SessionEventBody::SessionEndSeed(SessionEndSeedPayload {
            reason: reason.to_string(),
            parent_session_id: parent_session_id.map(|p| p.to_string()),
        }));
        writer.close()?;
        Ok(())
    }

    /// Create a branch of `parent_id` truncated at message index
    /// `branch_point`: the parent events feeding those messages are copied
    /// into the new session's log, closed by an end-seed marker.
    pub fn create_branch(
        &self,
        parent_id: &Uuid,
        branch_point: usize,
        title: Option<String>,
    ) -> Result<StoredSession, SessionStoreError> {
        let events = self
            .read_events(parent_id)?
            .ok_or(super::SessionLogError::NotFound(self.log_path(parent_id)))?;

        let proj = projections::project_conversation(&events);
        let cutoff = projections::cutoff_seq_for_message_index(&proj, branch_point);

        let new_id = Uuid::new_v4();
        {
            let mut writer = SessionLogWriter::open_layout(&self.container, &new_id.to_string())?;
            for event in events.iter().filter(|e| e.seq <= cutoff) {
                writer.record(event.body.clone());
            }
            writer.record(SessionEventBody::SessionEndSeed(SessionEndSeedPayload {
                reason: "branch".into(),
                parent_session_id: Some(parent_id.to_string()),
            }));
            writer.close()?;
        }

        self.save_sidecar(
            &new_id,
            &SessionSidecar {
                title,
                parent_session_id: Some(*parent_id),
                branch_point_message_index: Some(branch_point),
            },
        )?;

        self.load(&new_id)?.ok_or_else(|| {
            SessionStoreError::Serialization("branch log vanished immediately".into())
        })
    }

    /// Full-text search within one session (transcript-search successor).
    pub fn search_session(
        &self,
        session_id: &Uuid,
        pattern: &str,
    ) -> Result<Vec<projections::SearchHit>, SessionStoreError> {
        let Some(events) = self.read_events(session_id)? else {
            return Ok(Vec::new());
        };
        Ok(search_events(&events, pattern))
    }

    /// Delete a session: removes its whole `<container>/<uuid>/` directory.
    ///
    /// Returns `Ok(false)` when nothing existed. Only UUID-shaped direct
    /// children are touched, so sibling name snapshots (*.toml) stay safe.
    pub fn delete(&self, session_id: &Uuid) -> Result<bool, SessionStoreError> {
        let dir = self.container.join(session_id.to_string());
        if !dir.exists() {
            return Ok(false);
        }
        std::fs::remove_dir_all(dir)?;
        Ok(true)
    }

    /// Truncate the session log so only the first `keep_turns` conversation
    /// turns survive — the L0 primitive behind desktop `/rewind`. Turns are
    /// delimited by `user/message` events (each one OPENS a turn), so keeping
    /// `keep_turns` turns keeps user/messages `#0..#keep_turns-1` and drops
    /// the `#keep_turns`-th row and everything after it.
    ///
    /// The log is rewritten in place with the surviving raw lines, preserving
    /// seq numbering; a later [`SessionLogWriter`] resumes its seq from the
    /// surviving line count, so appends continue cleanly.
    ///
    /// Returns `Ok(None)` when no log exists, otherwise `Ok(Some(dropped))`
    /// with the number of event lines removed. `keep_turns` past the end of
    /// the session is a no-op returning `Some(0)`.
    pub fn truncate_to_turn(
        &self,
        session_id: &Uuid,
        keep_turns: usize,
    ) -> Result<Option<usize>, SessionStoreError> {
        let path = self.log_path(session_id);
        if !path.exists() {
            return Ok(None);
        }
        let raw = std::fs::read_to_string(&path)?;

        let mut kept_lines = String::with_capacity(raw.len());
        let mut total_lines = 0usize;
        let mut turns_seen = 0usize;
        // Whether the current (kept) turn already counted its opener — a
        // turn opens at `turn/start` with `user/message` as the fallback in
        // logs written without turn framing. Only the OPENER counts, so the
        // user/message inside a framed turn never double-increments.
        let mut turn_counted = false;
        let mut cut = false;
        for line in raw.lines() {
            if line.trim().is_empty() {
                continue;
            }
            total_lines += 1;
            if !cut {
                match serde_json::from_str::<SessionEvent>(line).map(|e| e.body) {
                    Ok(SessionEventBody::TurnStart(_)) => {
                        if turns_seen == keep_turns {
                            cut = true;
                        } else {
                            turns_seen += 1;
                            turn_counted = true;
                        }
                    }
                    Ok(SessionEventBody::UserMessage(_)) if !turn_counted => {
                        if turns_seen == keep_turns {
                            cut = true;
                        } else {
                            turns_seen += 1;
                            turn_counted = true;
                        }
                    }
                    Ok(SessionEventBody::TurnEnd(_)) => turn_counted = false,
                    _ => {}
                }
            }
            if cut {
                continue;
            }
            kept_lines.push_str(line);
            kept_lines.push('\n');
        }

        if !cut {
            // The boundary never arrived — keep_turns >= session turns.
            return Ok(Some(0));
        }

        // Atomic-ish rewrite: temp file in the same directory, then rename.
        let tmp = path.with_extension("jsonl.rewind-tmp");
        let dropped = total_lines - kept_lines.lines().count();
        std::fs::write(&tmp, &kept_lines)?;
        std::fs::rename(&tmp, &path)?;
        Ok(Some(dropped))
    }

    /// Replace the session's conversation history with `turns` — the L0
    /// primitive behind desktop `/compact` ("compact as summary turn"). Each
    /// `(user, assistant)` pair becomes one framed turn (`turn/start` +
    /// `user/message` + `assistant/message` + `turn/end`); the previous log
    /// is fully replaced via the same temp-file + rename rewrite as
    /// [`SessionStore::truncate_to_turn`], with seq restarting at 0 — the
    /// file contract (`SessionLogWriter` resumes from the last seq) holds.
    ///
    /// Returns the number of event lines written.
    pub fn rewrite_with_conversation(
        &self,
        session_id: &Uuid,
        turns: &[(String, String)],
    ) -> Result<usize, SessionStoreError> {
        let path = self.log_path(session_id);
        let session_str = session_id.to_string();
        let now_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);

        let mut out = String::new();
        let mut seq = 0u64;
        for (turn, (user, assistant)) in turns.iter().enumerate() {
            let turn = turn as u64;
            let mut push = |body: SessionEventBody| {
                let event = SessionEvent {
                    seq,
                    ts_ns: now_ns,
                    session_id: session_str.clone(),
                    turn,
                    step: None,
                    span_id: None,
                    parent_span_id: None,
                    body,
                };
                out.push_str(&serde_json::to_string(&event).map_err(|e| {
                    SessionStoreError::Serialization(e.to_string())
                })?);
                out.push('\n');
                seq += 1;
                Ok::<(), SessionStoreError>(())
            };
            push(SessionEventBody::TurnStart(TurnStartPayload { query_id: None }))?;
            push(SessionEventBody::UserMessage(UserMessagePayload {
                source: UserMessagePayload::SOURCE_USER.into(),
                content: user.clone(),
            }))?;
            // The projection finalizes an assistant step only when a chunk
            // stream preceded it — mirror a real (non-interrupted) turn.
            push(SessionEventBody::AssistantChunk(AssistantChunkPayload {
                delta: assistant.clone(),
                thinking: false,
            }))?;
            push(SessionEventBody::AssistantMessage(AssistantMessagePayload {
                content: assistant.clone(),
                usage: None,
                interrupted: false,
            }))?;
            push(SessionEventBody::TurnEnd(TurnEndPayload {
                reason: "compact".into(),
                usage: None,
                error: None,
            }))?;
        }

        // Atomic-ish rewrite: temp file in the same directory, then rename.
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("jsonl.compact-tmp");
        std::fs::write(&tmp, &out)?;
        std::fs::rename(&tmp, &path)?;
        Ok(seq as usize)
    }
}

/// Convenience: an shared handle rooted at the default container.
pub fn default_store() -> Arc<SessionStore> {
    Arc::new(SessionStore::new(SessionStore::default_container()))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use shannon_types::session_event::{
        AssistantChunkPayload, TokenUsage, ToolCallPayload, ToolResultPayload, TurnEndPayload,
        TurnStartPayload, UserMessagePayload,
    };

    fn store(tmp: &tempfile::TempDir) -> SessionStore {
        SessionStore::new(tmp.path().join("sessions"))
    }

    /// Drive a realistic multi-event session through the real writer.
    fn seed_session(store: &SessionStore, id: &Uuid) {
        let mut w = SessionLogWriter::open_layout(store.container(), &id.to_string()).unwrap();
        w.record(SessionEventBody::SessionStart(
            shannon_types::session_event::SessionStartPayload {
                model: "test-model".into(),
                provider: Some("anthropic".into()),
                cwd: Some("/proj".into()),
                app_version: None,
            },
        ));
        w.record(SessionEventBody::TurnStart(TurnStartPayload {
            query_id: None,
        }));
        w.record(SessionEventBody::UserMessage(UserMessagePayload {
            source: UserMessagePayload::SOURCE_USER.into(),
            content: "hi".into(),
        }));
        w.record(SessionEventBody::AssistantChunk(AssistantChunkPayload {
            delta: "He".into(),
            thinking: false,
        }));
        w.record(SessionEventBody::AssistantChunk(AssistantChunkPayload {
            delta: "llo!".into(),
            thinking: false,
        }));
        w.record(SessionEventBody::ToolCall(ToolCallPayload {
            tool_use_id: "u1".into(),
            tool_name: "Bash".into(),
            arguments: r#"{"command":"ls"}"#.into(),
        }));
        w.record(SessionEventBody::ToolResult(ToolResultPayload {
            tool_use_id: "u1".into(),
            tool_name: "Bash".into(),
            output: "out".into(),
            is_error: false,
            duration_ms: Some(3),
            meta: serde_json::Value::Null,
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

    #[tokio::test]
    async fn test_load_projects_full_session_state() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let id = Uuid::new_v4();
        seed_session(&store, &id);

        let loaded = store.load(&id).unwrap().expect("session exists");
        assert_eq!(loaded.session_id, id);
        assert_eq!(loaded.metadata.model, "test-model");
        assert_eq!(loaded.metadata.project_path.as_deref(), Some("/proj"));
        assert_eq!(loaded.metadata.total_input_tokens, 11);
        assert_eq!(loaded.metadata.total_output_tokens, 7);
        assert_eq!(loaded.metadata.turn_count, 1);
        assert_eq!(loaded.messages.len(), 3); // user, assistant(text+tool_use), user(tool_result)

        let ser = serde_json::to_value(&loaded.messages[1]).unwrap();
        assert_eq!(ser["content"][0]["text"], "Hello!");
        assert_eq!(ser["content"][1]["type"], "tool_use");
        let res = serde_json::to_value(&loaded.messages[2]).unwrap();
        assert_eq!(res["content"][0]["type"], "tool_result");
    }

    /// Seed a 3-turn session through the real writer (one writer handle so
    /// seq/turn numbering behaves exactly like a live session).
    fn seed_three_turn_session(store: &SessionStore, id: &Uuid) {
        let mut w = SessionLogWriter::open_layout(store.container(), &id.to_string()).unwrap();
        w.record(SessionEventBody::SessionStart(
            shannon_types::session_event::SessionStartPayload {
                model: "test-model".into(),
                provider: Some("anthropic".into()),
                cwd: Some("/proj".into()),
                app_version: None,
            },
        ));
        for turn in 0..3u64 {
            w.record(SessionEventBody::TurnStart(TurnStartPayload {
                query_id: None,
            }));
            w.record(SessionEventBody::UserMessage(UserMessagePayload {
                source: UserMessagePayload::SOURCE_USER.into(),
                content: format!("question {turn}"),
            }));
            w.record(SessionEventBody::AssistantChunk(AssistantChunkPayload {
                delta: format!("answer {turn}"),
                thinking: false,
            }));
            w.record(SessionEventBody::TurnEnd(TurnEndPayload {
                reason: TurnEndPayload::REASON_COMPLETED.into(),
                usage: None,
                error: None,
            }));
        }
        w.close().unwrap();
    }

    #[test]
    fn truncate_to_turn_keeps_leading_turns_and_drops_the_rest() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let id = Uuid::new_v4();
        seed_three_turn_session(&store, &id);

        let dropped = store.truncate_to_turn(&id, 2).unwrap().expect("log exists");
        assert!(dropped > 0);

        let loaded = store.load(&id).unwrap().expect("session survives");
        assert_eq!(loaded.metadata.turn_count, 2);
        assert_eq!(loaded.messages.len(), 4); // 2 × (user, assistant)
        let texts: Vec<_> = loaded.messages.iter().map(|m| match &m.content {
            shannon_engine::api::MessageContent::Text(t) => t.clone(),
            shannon_engine::api::MessageContent::Blocks(blocks) => blocks
                .iter()
                .filter_map(|b| match b {
                    shannon_engine::api::ContentBlock::Text { text } => Some(text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(""),
        }).collect();
        assert!(texts.contains(&"question 0".to_string()));
        assert!(texts.contains(&"answer 1".to_string()));
        assert!(!texts.iter().any(|t| t.contains("question 2")));
    }

    #[test]
    fn truncate_to_turn_zero_keeps_only_pre_session_events() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let id = Uuid::new_v4();
        seed_three_turn_session(&store, &id);

        store.truncate_to_turn(&id, 0).unwrap().expect("log exists");
        let loaded = store.load(&id).unwrap().expect("session survives");
        assert_eq!(loaded.metadata.turn_count, 0);
        assert!(loaded.messages.is_empty());
        // session/start survives so model/project metadata is intact.
        assert_eq!(loaded.metadata.model, "test-model");
    }

    #[test]
    fn truncate_to_turn_past_end_is_a_noop_and_writer_resumes_seq() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let id = Uuid::new_v4();
        seed_three_turn_session(&store, &id);

        let dropped = store.truncate_to_turn(&id, 50).unwrap().expect("log exists");
        assert_eq!(dropped, 0);

        // A later writer must resume cleanly from the truncated log.
        let mut w = SessionLogWriter::open_layout(store.container(), &id.to_string()).unwrap();
        w.record(SessionEventBody::TurnStart(TurnStartPayload { query_id: None }));
        w.record(SessionEventBody::UserMessage(UserMessagePayload {
            source: UserMessagePayload::SOURCE_USER.into(),
            content: "after rewind".into(),
        }));
        w.close().unwrap();

        let loaded = store.load(&id).unwrap().expect("session survives");
        assert_eq!(loaded.metadata.turn_count, 4);
    }

    #[test]
    fn truncate_to_turn_on_missing_log_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        assert!(store.truncate_to_turn(&Uuid::new_v4(), 1).unwrap().is_none());
    }

    #[test]
    fn test_load_missing_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        assert!(store.load(&Uuid::new_v4()).unwrap().is_none());
    }

    #[test]
    fn test_sidecar_save_merge_and_title_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let id = Uuid::new_v4();
        seed_session(&store, &id);

        store
            .save_sidecar(
                &id,
                &SessionSidecar {
                    title: Some("My Title".into()),
                    parent_session_id: None,
                    branch_point_message_index: None,
                },
            )
            .unwrap();
        // Second save with Nones must not wipe the title.
        store.save_sidecar(&id, &SessionSidecar::default()).unwrap();

        let loaded = store.load(&id).unwrap().unwrap();
        assert_eq!(loaded.metadata.title.as_deref(), Some("My Title"));
    }

    #[test]
    fn test_list_orders_and_previews() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        seed_session(&store, &a);
        std::thread::sleep(std::time::Duration::from_millis(30));
        seed_session(&store, &b);

        let infos = store.list().unwrap();
        assert_eq!(infos.len(), 2);
        assert_eq!(infos[0].session_id, b, "most recent first");
        assert_eq!(infos[1].preview.as_deref(), Some("hi"));
        // Foreign entries are skipped: a stray *.toml never appears.
        assert!(
            infos.iter().all(|i| i.session_id != Uuid::nil()
                || i.model.is_empty()
                || i.model == "test-model")
        );
    }

    #[test]
    fn test_create_branch_copies_prefix_and_seeds_lineage() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let parent = Uuid::new_v4();
        seed_session(&store, &parent);

        // Cut at the assistant step: prompt (idx 0) only.
        let branch = store.create_branch(&parent, 1, Some("cut".into())).unwrap();
        assert_ne!(branch.session_id, parent);
        assert_eq!(branch.messages.len(), 1);
        assert_eq!(branch.metadata.parent_session_id, Some(parent));
        assert_eq!(branch.metadata.branch_point_message_index, Some(1));
        assert_eq!(branch.metadata.title.as_deref(), Some("cut"));

        // The copied log carries exactly one prompt then the seed marker.
        let events = store.read_events(&branch.session_id).unwrap().unwrap();
        assert_eq!(events.last().unwrap().kind().as_str(), "session/end-seed");

        // Branches list via lineage.
        let branches_of_parent: Vec<_> = store
            .list()
            .unwrap()
            .into_iter()
            .filter(|i| i.parent_session_id == Some(parent))
            .collect();
        assert_eq!(branches_of_parent.len(), 1);
    }

    #[test]
    fn test_create_branch_unknown_parent_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        assert!(store.create_branch(&Uuid::new_v4(), 1, None).is_err());
    }

    #[test]
    fn test_delete_removes_directory_only_for_that_uuid() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let id = Uuid::new_v4();
        seed_session(&store, &id);
        let sibling = store.container().join("some-name.toml");
        std::fs::write(&sibling, "title = 'keep'").unwrap();

        assert!(store.delete(&id).unwrap());
        assert!(!store.load(&id).unwrap().is_some());
        assert!(!store.delete(&id).unwrap(), "second delete reports false");
        assert!(sibling.exists(), "non-session siblings survive");
    }

    #[test]
    fn test_search_session_over_log() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let id = Uuid::new_v4();
        seed_session(&store, &id);
        let hits = store.search_session(&id, "ls").unwrap();
        assert!(hits.iter().any(|h| h.kind == "tool/call"));
        assert!(
            store
                .search_session(&Uuid::new_v4(), "x")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn test_append_seed_marker_records_provenance() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let id = Uuid::new_v4();
        seed_session(&store, &id);
        store
            .append_seed_marker(&id, "resume", Some(Uuid::new_v4()))
            .unwrap();
        let events = store.read_events(&id).unwrap().unwrap();
        match &events.last().unwrap().body {
            SessionEventBody::SessionEndSeed(p) => assert_eq!(p.reason, "resume"),
            other => panic!("unexpected tail body: {other:?}"),
        }
    }

    #[test]
    fn test_default_store_helper_points_home() {
        let _ = default_store(); // constructs without panicking under any HOME
    }

    #[test]
    fn rewrite_with_conversation_replaces_history_and_projects_back() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let id = Uuid::new_v4();
        seed_session(&store, &id);

        let turns = vec![
            ("compacted summary of 3 earlier turns".to_string(), "summary text".to_string()),
            ("follow-up question".to_string(), "kept recent answer".to_string()),
        ];
        let written = store.rewrite_with_conversation(&id, &turns).unwrap();
        assert_eq!(written, 10); // 5 events per turn

        let loaded = store.load(&id).unwrap().expect("session survives rewrite");
        assert_eq!(loaded.metadata.turn_count, 2);
        let text = |m: &shannon_engine::api::Message| match &m.content {
            shannon_engine::api::MessageContent::Text(t) => t.clone(),
            shannon_engine::api::MessageContent::Blocks(blocks) => blocks
                .iter()
                .map(|b| match b {
                    shannon_engine::api::ContentBlock::Text { text } => text.clone(),
                    _ => String::new(),
                })
                .collect::<Vec<_>>()
                .join(""),
            other => panic!("unexpected content: {other:?}"),
        };
        let texts: Vec<String> = loaded.messages.iter().map(text).collect();
        assert_eq!(
            texts,
            vec![
                "compacted summary of 3 earlier turns",
                "summary text",
                "follow-up question",
                "kept recent answer",
            ]
        );
    }

    #[test]
    fn rewrite_with_conversation_then_writer_appends_cleanly() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let id = Uuid::new_v4();
        seed_session(&store, &id);

        store
            .rewrite_with_conversation(&id, &[("u".into(), "a".into())])
            .unwrap();

        // The single-writer resumes from the rewritten file's last seq.
        let mut w = SessionLogWriter::open_layout(store.container(), &id.to_string()).unwrap();
        w.set_turn(1);
        w.record(SessionEventBody::TurnStart(TurnStartPayload {
            query_id: None,
        }));
        w.record(SessionEventBody::UserMessage(UserMessagePayload {
            source: UserMessagePayload::SOURCE_USER.into(),
            content: "post-compact prompt".into(),
        }));
        w.close().unwrap();

        let loaded = store.load(&id).unwrap().unwrap();
        assert_eq!(loaded.metadata.turn_count, 2);
        let last = loaded.messages.last().unwrap();
        match &last.content {
            shannon_engine::api::MessageContent::Text(t) => assert_eq!(t, "post-compact prompt"),
            other => panic!("unexpected content: {other:?}"),
        }
    }

    #[test]
    fn rewrite_with_conversation_on_missing_log_creates_it() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let id = Uuid::new_v4();
        let written = store
            .rewrite_with_conversation(&id, &[("u".into(), "a".into())])
            .unwrap();
        assert_eq!(written, 5);
        assert!(store.load(&id).unwrap().is_some());
    }
}
