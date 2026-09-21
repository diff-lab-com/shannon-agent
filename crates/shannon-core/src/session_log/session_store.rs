//! # Session Store (§4.6 W1-P1)
//!
//! The L0-backed replacement for the deleted single-file session snapshot.
//! `events.jsonl` is the only authoritative record; this store derives
//! [`StoredSession`]s from it via
//! [`project_conversation`](super::projections::project_conversation) and persists only
//! the user-curation fields that cannot be derived (title, branch lineage)
//! as a `meta.json` sidecar next to each log.
//!
//! Layout (`<container>` is typically `~/.shannon/sessions`):
//!
//! ```text
//! <container>/<uuid>/events.jsonl   # authoritative log (L0)
//! <container>/<uuid>/meta.json      # optional sidecar: title / lineage
//! <container>/<uuid>/index.json     # optional cache: projection stats (E-9)
//! ```
//!
//! `list()` consults the E-9 index sidecar first: when it validates against
//! the log's current length/mtime, the listing is O(sessions) instead of
//! O(total log bytes). A missing/stale index falls back to the full
//! projection and opportunistically rebuilds the cache — see
//! [`super::session_index`].
//!
//! Breaking change (DP4): legacy `sessions/<uuid>.json` snapshots are not
//! read or migrated. Delete them once upgraded.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::TimeZone;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use shannon_types::session_event::{
    AssistantChunkPayload, AssistantMessagePayload, SessionEndSeedPayload, SessionEvent,
    SessionEventBody, TurnEndPayload, TurnStartPayload, UserMessagePayload,
};

use super::session_index::{SessionIndex, SessionIndexAccumulator, index_path_for, stat_len_mtime};
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
    /// Session goal (set via `/goal`), restored on resume.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<StoredGoal>,
    /// Active `/loop` state at sidecar-save time, restored on resume.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_state: Option<StoredLoop>,
    /// Active `/ralph` state at sidecar-save time, restored on resume.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ralph_state: Option<StoredRalph>,
    /// P0-4: optional per-session spend cap in USD. Enforced by the desktop
    /// shell (pre-turn reject / mid-turn cancel); `None` = no cap. Serde
    /// default keeps older `meta.json` files (and `events.jsonl`, which this
    /// sidecar never touches) fully backward-compatible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_usd: Option<f64>,
}

/// Persistence DTO for an active `/loop`. The kind discriminates "task
/// iteration loop" (`Loop`) vs. "completion-keyword loop" (`Ralph`) — both
/// ride the same flat-drain-loop scheduler but have different continuation
/// prompts. `active=false` rows are dropped on load to avoid stale
/// restorations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredLoop {
    pub task: String,
    pub max_iterations: usize,
    pub iteration: usize,
    #[serde(default = "default_true")]
    pub active: bool,
    /// Progress-guard counters (P2.1/P2.2), defaulted for older sidecars.
    #[serde(default)]
    pub no_tool_turns: usize,
    #[serde(default)]
    pub stall_strikes: usize,
}

/// Persistence DTO for an active `/ralph`. Same shape as `StoredLoop`
/// plus the completion keywords used by the legacy keyword matcher (will
/// be deprecated by the strict-marker contract in P2.6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredRalph {
    pub task: String,
    pub completion_keywords: Vec<String>,
    pub max_iterations: usize,
    pub iteration: usize,
    #[serde(default = "default_true")]
    pub active: bool,
    /// Progress-guard counters (P2.1/P2.2), defaulted for older sidecars.
    #[serde(default)]
    pub no_tool_turns: usize,
    #[serde(default)]
    pub stall_strikes: usize,
}

fn default_true() -> bool {
    true
}

/// Persistence DTO for a session goal. Kept dependency-free from the UI
/// crate: `status` is one of `"active" | "paused" | "complete"`; unknown
/// values degrade to paused on load rather than failing the session.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StoredGoal {
    pub objective: String,
    pub status: String,
    #[serde(default)]
    pub iterations: usize,
    #[serde(default)]
    pub max_iterations: usize,
    /// Fired blocked check-ins (caps at 3 across restarts).
    #[serde(default)]
    pub checkins: usize,
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
        // Loop/goal/ralph all share the same contract: the writer always
        // supplies a complete value (Some to upsert, None to clear). Filter
        // the on-disk row by `active` so stopped loops do not get resurrected
        // by a fresh session restore before the user can /loop again.
        self.goal = self.goal.or(existing.goal);
        if self.loop_state.is_none() {
            self.loop_state = existing.loop_state.filter(|l| l.active);
        }
        if self.ralph_state.is_none() {
            self.ralph_state = existing.ralph_state.filter(|r| r.active);
        }
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
    ///
    /// Served from the per-session `index.json` sidecars when they validate
    /// against the logs (the common case — the writer refreshes them on
    /// close), which keeps this O(number of sessions) instead of
    /// O(total bytes of all logs) (audit E-9: pickers on large containers
    /// used to re-decode and re-project every log per open). Any session
    /// whose index is missing or stale takes the full-projection path and
    /// rebuilds its cache opportunistically, so the two paths can never
    /// disagree: both are answers to the same projection.
    pub fn list(&self) -> Result<Vec<StoredSessionInfo>, SessionStoreError> {
        let mut infos = Vec::new();
        for entry in scan_session_summaries(&self.container) {
            let Ok(id) = Uuid::parse_str(&entry.session_id) else {
                continue; // foreign directories sharing the container
            };
            if let Some(info) = Self::info_from_index(&entry, &id) {
                infos.push(info);
                continue;
            }
            // Slow path: project the whole log, then refresh the cache.
            // Stat BEFORE reading — if the log grows underneath us the
            // recorded (pre-read) length mismatches at the next validation
            // and the fresh cache is discarded rather than trusted.
            let pre_stat = stat_len_mtime(&entry.events_path);
            let Some(events) = self.read_events(&id)? else {
                continue;
            };
            let stored = self.assemble(id, &events);
            Self::rebuild_index(&entry.events_path, pre_stat, &events);
            infos.push(Self::to_info(stored));
        }
        infos.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(infos)
    }

    /// Fast-path listing for one session from its `index.json`, or `None`
    /// when the cache is absent, unparsable, or stale (log length/mtime
    /// drift). Curation fields still come from `meta.json` — the index only
    /// caches the projection-derived numbers.
    fn info_from_index(entry: &super::SessionScanEntry, id: &Uuid) -> Option<StoredSessionInfo> {
        let index_path = index_path_for(&entry.events_path);
        let index = SessionIndex::load_if_valid(&entry.events_path, &index_path)?;
        let sidecar = SessionSidecar::load(&entry.meta_path);
        Some(StoredSessionInfo {
            session_id: *id,
            preview: index.first_preview_text().map(|t| truncate_preview(t, 80)),
            last_user_preview: index.last_preview_text().map(|t| truncate_preview(t, 80)),
            title: sidecar.title,
            model: index.model,
            created_at: ns_to_datetime(index.created_at_ns),
            updated_at: ns_to_datetime(index.updated_at_ns),
            turn_count: index.turn_count,
            total_input_tokens: index.total_input_tokens,
            total_output_tokens: index.total_output_tokens,
            parent_session_id: sidecar.parent_session_id,
            branch_point_message_index: sidecar.branch_point_message_index,
            project_path: index.project_path,
        })
    }

    /// Rebuild a session's index sidecar from a fully-read event slice.
    /// Best-effort: a failed write costs one rebuild on the next `list`.
    fn rebuild_index(events_path: &Path, pre_stat: Option<(u64, u64)>, events: &[SessionEvent]) {
        let mut acc = SessionIndexAccumulator::fresh();
        for event in events {
            acc.observe(event);
        }
        if let Some(index) = acc.finish(pre_stat) {
            let _ = index.store(&index_path_for(events_path));
        }
    }

    /// The most recently active session id, WITHOUT parsing any event logs.
    ///
    /// `list()` decodes and projects every session's full `events.jsonl` —
    /// O(total bytes of all sessions) — which made `shannon trace show
    /// latest` appear to hang on containers with large/many logs (WP-15:
    /// 30s+ with zero output). Directory mtimes already give the same
    /// "most recent" answer, so callers that only need the newest id should
    /// use this.
    pub fn latest_id(&self) -> Option<String> {
        scan_session_summaries(&self.container)
            .into_iter()
            .find(|entry| Uuid::parse_str(&entry.session_id).is_ok())
            .map(|entry| entry.session_id)
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

    /// Persist a complete sidecar, replacing whatever is on disk. Unlike
    /// [`SessionStore::save_sidecar`] this does not merge against the
    /// existing on-disk values — the caller is treated as authoritative.
    /// Used by writers that have already loaded the full sidecar (e.g.
    /// `/goal clear`, `/loop stop`, `/ralph stop`) and want explicit `None`
    /// to actually clear a row.
    pub fn save_sidecar_replace(
        &self,
        session_id: &Uuid,
        sidecar: &SessionSidecar,
    ) -> Result<(), SessionStoreError> {
        let path = self.meta_path(session_id);
        sidecar.clone().store(&path)
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
                goal: None,
                loop_state: None,
                ralph_state: None,
                budget_usd: None,
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

    /// Cross-session full-text search over the whole container (audit:
    /// nothing between "one session" and "open every picker" existed).
    ///
    /// Every session's `events.jsonl` is skimmed line-by-line with a
    /// [`BufReader`] (never loaded whole); lines over
    /// [`SEARCH_MAX_LINE_BYTES`] are skipped (they are almost always one
    /// embedded blob — pasted file, base64 dump — whose processing cost
    /// outweighs its recall). The query is a case-insensitive substring;
    /// each hit carries the best identifying metadata available without a
    /// full projection: the sidecar title, the E-9 index's first-user
    /// summary, and the matched line's `ts_ns` when parseable.
    ///
    /// Sessions are visited most-recently-modified first (the
    /// [`scan_session_summaries`] order) and the scan stops as soon as
    /// `limit` hits are collected.
    pub fn search_all(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SessionSearchHit>, SessionStoreError> {
        Ok(self.search_all_with_stats(query, limit)?.hits)
    }

    /// [`SessionStore::search_all`] plus scan coverage, so callers can say
    /// "searched N sessions" honestly even when the limit stopped the scan
    /// early.
    pub fn search_all_with_stats(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<SessionSearchOutcome, SessionStoreError> {
        let query = query.trim();
        let total = self.session_count();
        if query.is_empty() {
            return Ok(SessionSearchOutcome {
                hits: Vec::new(),
                sessions_scanned: 0,
                sessions_total: total,
            });
        }

        let entries = scan_session_summaries(&self.container);
        let mut hits = Vec::new();
        let mut scanned = 0usize;
        for entry in &entries {
            if hits.len() >= limit {
                break;
            }
            scanned += 1;
            // Identify the session from the sidecars before touching the
            // log: title from meta.json, summary from the E-9 index when it
            // still validates (never re-project the log for a search hit).
            let sidecar = SessionSidecar::load(&entry.meta_path);
            let index = SessionIndex::load_if_valid(
                &entry.events_path,
                &index_path_for(&entry.events_path),
            );
            let title = sidecar.title.clone();
            let summary = index
                .as_ref()
                .and_then(|i| i.first_preview_text())
                .map(|t| truncate_preview(t, 80));

            let Ok(file) = std::fs::File::open(&entry.events_path) else {
                continue;
            };
            let mut reader = BufReader::new(file);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                if line.len() > SEARCH_MAX_LINE_BYTES {
                    continue;
                }
                let Some((match_start, match_end)) = find_case_insensitive(&line, query) else {
                    continue;
                };
                let timestamp =
                    ts_ns_from_raw_line(&line).map(|ns| ns_to_datetime(ns).to_rfc3339());
                hits.push(SessionSearchHit {
                    session_id: entry.session_id.clone(),
                    title: title.clone(),
                    summary: summary.clone(),
                    timestamp,
                    snippet: snippet_around(&line, match_start, match_end),
                });
                if hits.len() >= limit {
                    break;
                }
            }
        }
        Ok(SessionSearchOutcome {
            hits,
            sessions_scanned: scanned,
            sessions_total: total,
        })
    }

    /// Number of sessions in the container (directory walk only, no log
    /// reads) — cheap context for search/UI surfaces.
    pub fn session_count(&self) -> usize {
        scan_session_summaries(&self.container).len()
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
        // The raw rewrite bypasses the writer's index accumulator: drop the
        // cache so the next list() rebuilds it from the surviving log.
        let _ = std::fs::remove_file(index_path_for(&path));
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
                out.push_str(
                    &serde_json::to_string(&event)
                        .map_err(|e| SessionStoreError::Serialization(e.to_string()))?,
                );
                out.push('\n');
                seq += 1;
                Ok::<(), SessionStoreError>(())
            };
            push(SessionEventBody::TurnStart(TurnStartPayload {
                query_id: None,
            }))?;
            push(SessionEventBody::UserMessage(UserMessagePayload {
                source: UserMessagePayload::SOURCE_USER.into(),
                content: user.clone(),
                attachment_count: 0,
            }))?;
            // The projection finalizes an assistant step only when a chunk
            // stream preceded it — mirror a real (non-interrupted) turn.
            push(SessionEventBody::AssistantChunk(AssistantChunkPayload {
                delta: assistant.clone(),
                thinking: false,
            }))?;
            push(SessionEventBody::AssistantMessage(
                AssistantMessagePayload {
                    content: assistant.clone(),
                    usage: None,
                    interrupted: false,
                },
            ))?;
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
        // The raw rewrite bypasses the writer's index accumulator: drop the
        // cache so the next list() rebuilds it from the rewritten log.
        let _ = std::fs::remove_file(index_path_for(&path));
        Ok(seq as usize)
    }
}

/// Convenience: an shared handle rooted at the default container.
pub fn default_store() -> Arc<SessionStore> {
    Arc::new(SessionStore::new(SessionStore::default_container()))
}

// ============================================================================
// Cross-session search (search_all)
// ============================================================================

/// Default hit cap for [`SessionStore::search_all`] when the caller has no
/// stronger opinion.
pub const DEFAULT_SEARCH_LIMIT: usize = 50;

/// Raw lines larger than this are skipped by cross-session search: a single
/// `events.jsonl` row approaching 1 MB is almost always one embedded blob
/// (pasted file, base64 dump) whose scan cost outweighs its recall value.
const SEARCH_MAX_LINE_BYTES: usize = 1024 * 1024;

/// Chars of context kept on each side of a search-hit snippet.
const SEARCH_SNIPPET_CONTEXT_CHARS: usize = 80;

/// One match of a cross-session search ([`SessionStore::search_all`]).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionSearchHit {
    /// Owning session id (string form of the directory name).
    pub session_id: String,
    /// Curated title from the `meta.json` sidecar, when present.
    pub title: Option<String>,
    /// Best-effort summary from the E-9 index (first user-message prefix)
    /// when no curated title exists.
    pub summary: Option<String>,
    /// Timestamp of the matched event (RFC 3339) when its `ts_ns` parsed.
    pub timestamp: Option<String>,
    /// Single-line excerpt around the match.
    pub snippet: String,
}

/// Search hits plus scan coverage, so callers can report "searched N
/// sessions" honestly even when the limit stopped the scan early.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionSearchOutcome {
    pub hits: Vec<SessionSearchHit>,
    /// Sessions actually skimmed (the scan stops once `limit` is reached).
    pub sessions_scanned: usize,
    /// Sessions present in the container.
    pub sessions_total: usize,
}

/// Case-insensitive char equality (full Unicode simple lowercase folding on
/// both sides, so e.g. `Ä` matches `ä`).
fn chars_eq_ignore_case(a: char, b: char) -> bool {
    let mut la = a.to_lowercase();
    let mut lb = b.to_lowercase();
    loop {
        match (la.next(), lb.next()) {
            (None, None) => return true,
            (Some(x), Some(y)) if x == y => continue,
            _ => return false,
        }
    }
}

/// Case-insensitive substring search that does NOT allocate a lowercased
/// copy of the haystack (`search_all` runs this per log line; lines can be
/// large). Returns the `(start, end)` byte span of the first match, or
/// `None`. An empty needle matches at 0.
fn find_case_insensitive(haystack: &str, needle: &str) -> Option<(usize, usize)> {
    if needle.is_empty() {
        return Some((0, 0));
    }
    let needle_chars: Vec<char> = needle.chars().collect();
    let first = needle_chars[0];
    'outer: for (idx, ch) in haystack.char_indices() {
        if !chars_eq_ignore_case(ch, first) {
            continue;
        }
        let mut consumed = 0usize;
        for (n, h) in needle_chars.iter().zip(haystack[idx..].chars()) {
            if !chars_eq_ignore_case(*n, h) {
                continue 'outer;
            }
            consumed += 1;
        }
        if consumed == needle_chars.len() {
            // Match end: byte offset just past the matched chars (char-count
            // based, since case folding can change byte lengths).
            let end = haystack[idx..]
                .char_indices()
                .nth(needle_chars.len())
                .map(|(o, _)| idx + o)
                .unwrap_or(haystack.len());
            return Some((idx, end));
        }
    }
    None
}

/// Single-line excerpt of ±[`SEARCH_SNIPPET_CONTEXT_CHARS`] chars around the
/// match, with `…` ellipses where content was cut and control chars
/// (newlines, tabs) flattened to spaces.
fn snippet_around(line: &str, match_start: usize, match_end: usize) -> String {
    let line = line.trim();
    let mut start = match_start
        .min(line.len())
        .saturating_sub(SEARCH_SNIPPET_CONTEXT_CHARS);
    while start > 0 && !line.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = (match_end + SEARCH_SNIPPET_CONTEXT_CHARS).min(line.len());
    while end > match_start && !line.is_char_boundary(end) {
        end -= 1;
    }
    let mut snippet: String = line[start..end]
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if start > 0 {
        snippet.insert(0, '…');
    }
    if end < line.len() {
        snippet.push('…');
    }
    snippet
}

/// Extract `ts_ns` from a raw JSONL line without deserializing the whole
/// event: serde renders the field as `"ts_ns":<integer>`, so a targeted
/// digit scan recovers it (cheap, and never fails the search).
fn ts_ns_from_raw_line(line: &str) -> Option<u64> {
    const KEY: &str = "\"ts_ns\":";
    let idx = line.find(KEY)?;
    let rest = &line[idx + KEY.len()..];
    let digits_end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..digits_end].parse().ok()
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
                ..Default::default()
            },
        ));
        w.record(SessionEventBody::TurnStart(TurnStartPayload {
            query_id: None,
        }));
        w.record(SessionEventBody::UserMessage(UserMessagePayload {
            source: UserMessagePayload::SOURCE_USER.into(),
            content: "hi".into(),
            attachment_count: 0,
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
                ..Default::default()
            },
        ));
        for turn in 0..3u64 {
            w.record(SessionEventBody::TurnStart(TurnStartPayload {
                query_id: None,
            }));
            w.record(SessionEventBody::UserMessage(UserMessagePayload {
                source: UserMessagePayload::SOURCE_USER.into(),
                content: format!("question {turn}"),
                attachment_count: 0,
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
        let texts: Vec<_> = loaded
            .messages
            .iter()
            .map(|m| match &m.content {
                shannon_engine::api::MessageContent::Text(t) => t.clone(),
                shannon_engine::api::MessageContent::Blocks(blocks) => blocks
                    .iter()
                    .filter_map(|b| match b {
                        shannon_engine::api::ContentBlock::Text { text } => Some(text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(""),
            })
            .collect();
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

        let dropped = store
            .truncate_to_turn(&id, 50)
            .unwrap()
            .expect("log exists");
        assert_eq!(dropped, 0);

        // A later writer must resume cleanly from the truncated log.
        let mut w = SessionLogWriter::open_layout(store.container(), &id.to_string()).unwrap();
        w.record(SessionEventBody::TurnStart(TurnStartPayload {
            query_id: None,
        }));
        w.record(SessionEventBody::UserMessage(UserMessagePayload {
            source: UserMessagePayload::SOURCE_USER.into(),
            content: "after rewind".into(),
            attachment_count: 0,
        }));
        w.close().unwrap();

        let loaded = store.load(&id).unwrap().expect("session survives");
        assert_eq!(loaded.metadata.turn_count, 4);
    }

    #[test]
    fn truncate_to_turn_on_missing_log_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        assert!(
            store
                .truncate_to_turn(&Uuid::new_v4(), 1)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn test_load_missing_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        assert!(store.load(&Uuid::new_v4()).unwrap().is_none());
    }

    /// WP-15: `latest_id` must answer from directory mtimes alone — no event
    /// parsing. Guards the `shannon trace show latest` 30s-hang fix.
    #[test]
    fn latest_id_picks_newest_by_mtime_without_parsing() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let container = tmp.path().join("sessions"); // matches the store helper
        assert!(store.latest_id().is_none());

        let older = Uuid::new_v4().to_string();
        let newer = Uuid::new_v4().to_string();
        for id in [&older, &newer] {
            let dir = container.join(id);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("events.jsonl"), "").unwrap();
            // Give the second directory a strictly newer mtime.
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        assert_eq!(store.latest_id().as_deref(), Some(newer.as_str()));

        // Non-UUID directories (foreign/leftover) never win the scan.
        let junk = container.join("not-a-uuid");
        std::fs::create_dir_all(&junk).unwrap();
        std::fs::write(junk.join("events.jsonl"), "").unwrap();
        assert_eq!(store.latest_id().as_deref(), Some(newer.as_str()));
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
                    goal: None,
                    loop_state: None,
                    ralph_state: None,
                    budget_usd: None,
                },
            )
            .unwrap();
        // Second save with Nones must not wipe the title.
        store.save_sidecar(&id, &SessionSidecar::default()).unwrap();

        let loaded = store.load(&id).unwrap().unwrap();
        assert_eq!(loaded.metadata.title.as_deref(), Some("My Title"));
    }

    #[test]
    fn stored_goal_serde_roundtrip() {
        let goal = StoredGoal {
            objective: "all tests pass".into(),
            status: "active".into(),
            iterations: 3,
            max_iterations: 25,
            checkins: 0,
        };
        let json = serde_json::to_string(&goal).unwrap();
        let back: StoredGoal = serde_json::from_str(&json).unwrap();
        assert_eq!(back, goal);
    }

    #[test]
    fn sidecar_without_goal_omits_field() {
        let sidecar = SessionSidecar::default();
        let json = serde_json::to_string(&sidecar).unwrap();
        assert!(!json.contains("goal"), "skip_serializing_if leaked: {json}");
    }

    #[test]
    fn sidecar_goal_roundtrip_through_store() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let id = Uuid::new_v4();
        seed_session(&store, &id);

        store
            .save_sidecar(
                &id,
                &SessionSidecar {
                    goal: Some(StoredGoal {
                        objective: "ship it".into(),
                        status: "active".into(),
                        iterations: 1,
                        max_iterations: 25,
                        checkins: 0,
                    }),
                    ..Default::default()
                },
            )
            .unwrap();

        let loaded = store.sidecar(&id);
        assert_eq!(
            loaded.goal,
            Some(StoredGoal {
                objective: "ship it".into(),
                status: "active".into(),
                iterations: 1,
                max_iterations: 25,
                checkins: 0,
            })
        );
        // A goal-less save must not wipe the stored goal.
        store.save_sidecar(&id, &SessionSidecar::default()).unwrap();
        assert!(store.sidecar(&id).goal.is_some());
    }

    #[test]
    fn sidecar_unknown_status_string_loads() {
        let json =
            r#"{"goal":{"objective":"x","status":"bogus","iterations":0,"max_iterations":25}}"#;
        let sidecar: SessionSidecar = serde_json::from_str(json).unwrap();
        assert_eq!(sidecar.goal.unwrap().status, "bogus");
    }

    #[test]
    fn stored_loop_serde_roundtrip() {
        let lp = StoredLoop {
            task: "ship it".into(),
            max_iterations: 5,
            iteration: 3,
            active: true,
            no_tool_turns: 0,
            stall_strikes: 0,
        };
        let back: StoredLoop = serde_json::from_str(&serde_json::to_string(&lp).unwrap()).unwrap();
        assert_eq!(back, lp);
    }

    #[test]
    fn stored_ralph_serde_roundtrip() {
        let rp = StoredRalph {
            task: "make it green".into(),
            completion_keywords: vec!["DONE".into(), "FIXED".into()],
            max_iterations: 4,
            iteration: 2,
            active: true,
            no_tool_turns: 0,
            stall_strikes: 0,
        };
        let back: StoredRalph = serde_json::from_str(&serde_json::to_string(&rp).unwrap()).unwrap();
        assert_eq!(back, rp);
    }

    #[test]
    fn sidecar_loop_state_roundtrip_through_store() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let id = Uuid::new_v4();
        seed_session(&store, &id);

        store
            .save_sidecar_replace(
                &id,
                &SessionSidecar {
                    loop_state: Some(StoredLoop {
                        task: "ship it".into(),
                        max_iterations: 7,
                        iteration: 3,
                        active: true,
                        no_tool_turns: 0,
                        stall_strikes: 0,
                    }),
                    ..Default::default()
                },
            )
            .unwrap();

        assert_eq!(store.sidecar(&id).loop_state.unwrap().task, "ship it",);
        // Clearing the loop on a second save must remove the row entirely.
        // save_sidecar (merge variant) would resurrect the old row from
        // disk, so use save_sidecar_replace for the explicit-clear path.
        store
            .save_sidecar_replace(
                &id,
                &SessionSidecar {
                    loop_state: None,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(store.sidecar(&id).loop_state.is_none());
    }

    #[test]
    fn sidecar_without_loop_or_ralph_omits_fields() {
        let sidecar = SessionSidecar::default();
        let json = serde_json::to_string(&sidecar).unwrap();
        assert!(!json.contains("loop_state"), "loop_state leaked: {json}");
        assert!(!json.contains("ralph_state"), "ralph_state leaked: {json}");
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

    // =========================================================================
    // Cross-session search (search_all)
    // =========================================================================

    /// Seed a one-turn session whose prompt mentions `needle`.
    fn seed_session_with_prompt(store: &SessionStore, id: &Uuid, prompt: &str) {
        let mut w = SessionLogWriter::open_layout(store.container(), &id.to_string()).unwrap();
        w.record(SessionEventBody::SessionStart(
            shannon_types::session_event::SessionStartPayload {
                model: "search-model".into(),
                provider: None,
                cwd: Some("/proj".into()),
                app_version: None,
                ..Default::default()
            },
        ));
        w.record(SessionEventBody::TurnStart(TurnStartPayload {
            query_id: None,
        }));
        w.record(SessionEventBody::UserMessage(UserMessagePayload {
            source: UserMessagePayload::SOURCE_USER.into(),
            content: prompt.into(),
            attachment_count: 0,
        }));
        w.record(SessionEventBody::AssistantChunk(AssistantChunkPayload {
            delta: "working on it".into(),
            thinking: false,
        }));
        w.record(SessionEventBody::TurnEnd(TurnEndPayload {
            reason: TurnEndPayload::REASON_COMPLETED.into(),
            usage: None,
            error: None,
        }));
        w.close().unwrap();
    }

    #[test]
    fn search_all_matches_case_insensitively_across_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let old = Uuid::new_v4();
        seed_session_with_prompt(&store, &old, "the NEEDLE is buried here");
        std::thread::sleep(std::time::Duration::from_millis(30));
        let new = Uuid::new_v4();
        seed_session_with_prompt(&store, &new, "please find my needle, thanks");

        let outcome = store
            .search_all_with_stats("NeEdLe", DEFAULT_SEARCH_LIMIT)
            .unwrap();
        assert_eq!(outcome.sessions_total, 2);
        assert_eq!(outcome.sessions_scanned, 2);
        assert_eq!(outcome.hits.len(), 2);
        // Most recently modified session first.
        assert_eq!(outcome.hits[0].session_id, new.to_string());
        assert_eq!(outcome.hits[1].session_id, old.to_string());
        let hit = &outcome.hits[0];
        assert!(hit.snippet.to_lowercase().contains("needle"));
        assert!(!hit.snippet.contains('\n'), "snippet must be single-line");
        // ts_ns parsed from the matched raw line → RFC 3339 timestamp.
        assert!(hit.timestamp.is_some(), "matched line should carry ts_ns");
    }

    #[test]
    fn search_all_reports_metadata_from_sidecars() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let id = Uuid::new_v4();
        seed_session_with_prompt(&store, &id, "the quokka habitat");
        store
            .save_sidecar(
                &id,
                &SessionSidecar {
                    title: Some("Quokka research".into()),
                    ..Default::default()
                },
            )
            .unwrap();

        let hits = store.search_all("QUOKKA", DEFAULT_SEARCH_LIMIT).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title.as_deref(), Some("Quokka research"));
        // Without a title the index's first-user message summarizes.
        let bare = Uuid::new_v4();
        seed_session_with_prompt(&store, &bare, "aardvark migration");
        std::thread::sleep(std::time::Duration::from_millis(20));
        let hits = store.search_all("aardvark", DEFAULT_SEARCH_LIMIT).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].title.is_none());
        assert_eq!(hits[0].summary.as_deref(), Some("aardvark migration"));
    }

    #[test]
    fn search_all_respects_limit_and_stops_scanning() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        for _ in 0..3 {
            seed_session_with_prompt(&store, &Uuid::new_v4(), "find the zebra");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let outcome = store.search_all_with_stats("zebra", 2).unwrap();
        assert_eq!(outcome.hits.len(), 2);
        assert_eq!(outcome.sessions_total, 3);
        assert_eq!(outcome.sessions_scanned, 2, "scan stops once limit is hit");
    }

    #[test]
    fn search_all_skips_oversized_lines_and_empty_queries() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let id = Uuid::new_v4();
        seed_session_with_prompt(&store, &id, "start");
        // A >1MB raw line (a blob without real JSON semantics) must be
        // skipped rather than matched or loaded wholesale.
        let log = store.container().join(id.to_string()).join("events.jsonl");
        let mut big = std::fs::read_to_string(&log).unwrap();
        big.push_str(&format!(
            "{{\"blob\":\"{}\"}}\n",
            "x".repeat(1024 * 1024 + 16)
        ));
        std::fs::write(&log, big).unwrap();

        assert!(
            store
                .search_all("xxxx", DEFAULT_SEARCH_LIMIT)
                .unwrap()
                .is_empty(),
            "oversized lines are skipped"
        );
        // Normal content in the same container still matches ("start" hits
        // the user/message line plus the session/start and turn/start kind
        // strings — all from the one seeded session).
        let hits = store.search_all("start", DEFAULT_SEARCH_LIMIT).unwrap();
        assert!(!hits.is_empty(), "normal content still matches");
        assert!(hits.iter().all(|h| h.session_id == id.to_string()));
        // Empty/whitespace queries match nothing.
        assert!(
            store
                .search_all("   ", DEFAULT_SEARCH_LIMIT)
                .unwrap()
                .is_empty()
        );
        assert!(
            store
                .search_all("", DEFAULT_SEARCH_LIMIT)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn search_all_snippet_is_bounded_and_char_safe() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let id = Uuid::new_v4();
        let filler = "é".repeat(400);
        seed_session_with_prompt(&store, &id, &format!("{filler} TARGET {filler}"));

        let hits = store.search_all("target", DEFAULT_SEARCH_LIMIT).unwrap();
        assert_eq!(hits.len(), 1);
        let snippet = &hits[0].snippet;
        assert!(snippet.contains("TARGET"));
        assert!(snippet.starts_with('…') && snippet.ends_with('…'));
        assert!(snippet.chars().count() <= 2 * 80 + "TARGET".len() + 2);
        assert!(!snippet.contains('\n'));
    }

    #[test]
    fn search_all_on_empty_container_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let outcome = store
            .search_all_with_stats("anything", DEFAULT_SEARCH_LIMIT)
            .unwrap();
        assert_eq!(outcome.sessions_total, 0);
        assert!(outcome.hits.is_empty());
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
            (
                "compacted summary of 3 earlier turns".to_string(),
                "summary text".to_string(),
            ),
            (
                "follow-up question".to_string(),
                "kept recent answer".to_string(),
            ),
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
            attachment_count: 0,
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

    // =========================================================================
    // E-9: index.json sidecar
    // =========================================================================

    fn strip_index_files(store: &SessionStore) {
        for entry in scan_session_summaries(store.container()) {
            let _ = std::fs::remove_file(index_path_for(&entry.events_path));
        }
    }

    fn index_files(store: &SessionStore) -> Vec<PathBuf> {
        scan_session_summaries(store.container())
            .iter()
            .map(|e| index_path_for(&e.events_path))
            .filter(|p| p.exists())
            .collect()
    }

    /// Seed a tool-calling turn: the projected LAST user-role message is the
    /// tool result, so `last_user_preview` must be `None` on both paths.
    fn seed_tool_turn_session(store: &SessionStore, id: &Uuid) {
        let mut w = SessionLogWriter::open_layout(store.container(), &id.to_string()).unwrap();
        w.record(SessionEventBody::SessionStart(
            shannon_types::session_event::SessionStartPayload {
                model: "tool-model".into(),
                provider: None,
                cwd: Some("/tool/proj".into()),
                app_version: None,
                ..Default::default()
            },
        ));
        w.record(SessionEventBody::TurnStart(TurnStartPayload {
            query_id: None,
        }));
        w.record(SessionEventBody::UserMessage(UserMessagePayload {
            source: UserMessagePayload::SOURCE_USER.into(),
            content: "run something".into(),
            attachment_count: 0,
        }));
        w.record(SessionEventBody::AssistantChunk(AssistantChunkPayload {
            delta: "running".into(),
            thinking: false,
        }));
        w.record(SessionEventBody::ToolCall(ToolCallPayload {
            tool_use_id: "u9".into(),
            tool_name: "Bash".into(),
            arguments: r#"{"command":"ls"}"#.into(),
        }));
        w.record(SessionEventBody::ToolResult(ToolResultPayload {
            tool_use_id: "u9".into(),
            tool_name: "Bash".into(),
            output: "out".into(),
            is_error: false,
            duration_ms: None,
            meta: serde_json::Value::Null,
        }));
        w.record(SessionEventBody::TurnEnd(TurnEndPayload {
            reason: TurnEndPayload::REASON_COMPLETED.into(),
            usage: Some(shannon_types::session_event::TokenUsage {
                input_tokens: 100,
                output_tokens: 20,
                cache_creation_tokens: 0,
                cache_read_tokens: 0,
                cost_usd: None,
            }),
            error: None,
        }));
        w.close().unwrap();
    }

    /// The E-9 core contract: listing through the index sidecars answers
    /// exactly what the full projection answers — for every session shape,
    /// on both the writer-maintained caches and the rebuilt ones.
    #[test]
    fn indexed_list_equals_full_projection_list() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);

        // Shape 1: tool session (last projected user-role message is a
        // tool_result → last_user_preview None).
        seed_tool_turn_session(&store, &Uuid::new_v4());
        // Shape 2: plain three-turn session.
        std::thread::sleep(std::time::Duration::from_millis(20));
        seed_three_turn_session(&store, &Uuid::new_v4());
        // Shape 3: metadata-only session (SessionStart, no conversation).
        std::thread::sleep(std::time::Duration::from_millis(20));
        let bare = Uuid::new_v4();
        {
            let mut w =
                SessionLogWriter::open_layout(store.container(), &bare.to_string()).unwrap();
            w.record(SessionEventBody::SessionStart(
                shannon_types::session_event::SessionStartPayload {
                    model: "bare-model".into(),
                    provider: None,
                    cwd: Some("/bare".into()),
                    app_version: None,
                    ..Default::default()
                },
            ));
            w.close().unwrap();
        }

        // 1) writer-maintained caches.
        let via_writer_index = store.list().unwrap();
        assert_eq!(via_writer_index.len(), 3);

        // 2) caches stripped → full projection (and in-contention rebuild).
        strip_index_files(&store);
        let via_full_projection = store.list().unwrap();
        assert_eq!(via_writer_index, via_full_projection);

        // The slow path rebuilt the caches; the next listing rides them to
        // the same answer.
        assert_eq!(index_files(&store).len(), 3, "rebuild republished caches");
        assert_eq!(store.list().unwrap(), via_full_projection);

        // Spot-check the tool session's quirks survived the fast path: its
        // preview is the prompt and last_user_preview is the tool result.
        let tool = via_full_projection
            .iter()
            .find(|i| i.model == "tool-model")
            .unwrap();
        assert_eq!(tool.preview.as_deref(), Some("run something"));
        assert_eq!(tool.last_user_preview, None);
        assert_eq!(tool.total_input_tokens, 100);
        assert_eq!(tool.total_output_tokens, 20);
        assert_eq!(tool.project_path.as_deref(), Some("/tool/proj"));
    }

    /// A writer episode that resumes without a valid prior index has a
    /// partial base: it must stay silent (no index file) rather than publish
    /// stats covering only its own episode. The next list() rebuilds.
    #[test]
    fn writer_with_partial_base_does_not_publish_index() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let id = Uuid::new_v4();
        seed_session(&store, &id);
        let index_path = index_path_for(&store.log_path(&id));
        assert!(index_path.exists(), "writer close published an index");

        // Force the partial-base path: cache gone, log non-empty.
        std::fs::remove_file(&index_path).unwrap();
        {
            let mut w = SessionLogWriter::open_layout(store.container(), &id.to_string()).unwrap();
            w.set_turn(2);
            w.record(SessionEventBody::TurnStart(TurnStartPayload {
                query_id: None,
            }));
            w.record(SessionEventBody::UserMessage(UserMessagePayload {
                source: UserMessagePayload::SOURCE_USER.into(),
                content: "episode two".into(),
                attachment_count: 0,
            }));
            w.close().unwrap();
        }
        assert!(
            !index_path.exists(),
            "partial-base close must not publish an index"
        );

        // list() still answers correctly and rebuilds the cache.
        let infos = store.list().unwrap();
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].preview.as_deref(), Some("hi"));
        assert_eq!(
            infos[0].last_user_preview.as_deref(),
            Some("episode two"),
            "last user message is a plain prompt (no trailing tool result)"
        );
        assert_eq!(infos[0].turn_count, 2);
        assert!(index_path.exists(), "list() rebuilt the cache");

        // And the rebuilt cache agrees with the full projection.
        let via_index = infos;
        strip_index_files(&store);
        assert_eq!(store.list().unwrap(), via_index);
    }

    /// A tampered/stale index (e.g. hand-edited, or a same-name rewrite that
    /// slipped past mtime) is discarded, the full projection answers, and
    /// the cache is refreshed.
    #[test]
    fn stale_index_is_discarded_and_rebuilt() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let id = Uuid::new_v4();
        seed_session(&store, &id);
        let index_path = index_path_for(&store.log_path(&id));

        let mut stale = store.list().unwrap().into_iter().next().unwrap();
        // Tamper: claim stats over a different log length + a fake token sum.
        let mut raw: SessionIndex =
            serde_json::from_str(&std::fs::read_to_string(&index_path).unwrap()).unwrap();
        raw.log_len += 999;
        raw.total_input_tokens += 4242;
        raw.store(&index_path).unwrap();

        let infos = store.list().unwrap();
        assert_eq!(infos.len(), 1);
        assert_ne!(
            infos[0].total_input_tokens,
            stale.total_input_tokens + 4242,
            "tampered stats must not leak through"
        );
        stale = infos.into_iter().next().unwrap();

        // The rebuild replaced the tampered file with a valid one; the next
        // listing takes the fast path to the same answer.
        let again = store.list().unwrap();
        assert_eq!(again, vec![stale]);
        let healed: SessionIndex =
            serde_json::from_str(&std::fs::read_to_string(&index_path).unwrap()).unwrap();
        assert_ne!(healed.log_len, raw.log_len, "cache was rewritten");
    }

    /// Raw rewrites (desktop rewind/compact) bypass the writer's accumulator:
    /// they must drop the cache, and the next list() must reflect the
    /// rewritten log, not the pre-rewrite stats.
    #[test]
    fn raw_rewrites_invalidate_index_and_list_follows() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let id = Uuid::new_v4();
        seed_three_turn_session(&store, &id);
        let index_path = index_path_for(&store.log_path(&id));
        assert!(store.list().unwrap().iter().all(|i| i.turn_count == 3));
        assert!(index_path.exists());

        store.truncate_to_turn(&id, 1).unwrap();
        assert!(
            !index_path.exists(),
            "truncate_to_turn must drop the stale cache"
        );
        let infos = store.list().unwrap();
        assert_eq!(infos[0].turn_count, 1);
        assert_eq!(infos[0].preview.as_deref(), Some("question 0"));

        store
            .rewrite_with_conversation(&id, &[("sum q".into(), "sum a".into())])
            .unwrap();
        assert!(!index_path.exists(), "compact must drop the cache");
        let infos = store.list().unwrap();
        assert_eq!(infos[0].turn_count, 1);
        assert_eq!(infos[0].preview.as_deref(), Some("sum q"));
        assert_eq!(infos[0].last_user_preview.as_deref(), Some("sum q"));

        // Index-backed listing agrees with the projection after rewrites.
        let via_index = infos;
        strip_index_files(&store);
        assert_eq!(store.list().unwrap(), via_index);
    }

    /// A second writer episode seeds from the published index: the refreshed
    /// sidecar covers the whole log, not just the episode.
    #[test]
    fn writer_episode_seeds_from_index_and_covers_whole_log() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        let id = Uuid::new_v4();
        seed_session(&store, &id);

        {
            let mut w = SessionLogWriter::open_layout(store.container(), &id.to_string()).unwrap();
            w.set_turn(2);
            w.record(SessionEventBody::TurnStart(TurnStartPayload {
                query_id: None,
            }));
            w.record(SessionEventBody::UserMessage(UserMessagePayload {
                source: UserMessagePayload::SOURCE_USER.into(),
                content: "follow-up".into(),
                attachment_count: 0,
            }));
            w.record(SessionEventBody::TurnEnd(TurnEndPayload {
                reason: TurnEndPayload::REASON_COMPLETED.into(),
                usage: Some(shannon_types::session_event::TokenUsage {
                    input_tokens: 5,
                    output_tokens: 6,
                    cache_creation_tokens: 0,
                    cache_read_tokens: 0,
                    cost_usd: None,
                }),
                error: None,
            }));
            w.close().unwrap();
        }

        let via_index = store.list().unwrap();
        assert_eq!(via_index[0].turn_count, 2);
        assert_eq!(via_index[0].total_input_tokens, 11 + 5);
        assert_eq!(via_index[0].total_output_tokens, 7 + 6);
        assert_eq!(via_index[0].preview.as_deref(), Some("hi"));
        assert_eq!(via_index[0].last_user_preview.as_deref(), Some("follow-up"));

        strip_index_files(&store);
        assert_eq!(store.list().unwrap(), via_index);
    }
}
