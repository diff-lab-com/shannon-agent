//! SQLite-backed inbox store (P0-3, review decision D6).
//!
//! Two concerns live in one database at `~/.shannon/inbox.db`:
//!
//! 1. **Inbox items** — entries the user should look at (finished routine
//!    runs, external triggers, …). Sources: `routine` (scheduled task runs),
//!    `scheduled_task` (alias kept for UI clarity), `goal`, `trigger`
//!    (executions fired through the HMAC trigger endpoint). Status flow:
//!    `pending` → `read` → `archived`.
//! 2. **Automation run history** (`routine_runs`) — *new* run records are
//!    written here going forward. The legacy JSONL store
//!    (`crates/shannon_core::scheduled_runs`) keeps receiving the same runs
//!    (mirrored by the desktop) until the UI switches over.
//!
//! Session storage (`events.jsonl` / `meta.json`) is intentionally **not**
//! touched — this module never reads or writes session logs.
//!
//! ## Legacy triage migration
//!
//! The previous inbox was `~/.shannon/triage.jsonl` (JSONL, latest-revision-
//! wins). On the first [`InboxStore::open`] with an empty `inbox_items`
//! table, existing triage entries are imported once and the `meta` key
//! `legacy_triage_imported` is set to `1` so the migration never re-runs.
//! Pass `legacy = None` (via [`InboxStore::open_with_legacy`]) to skip it —
//! tests use this to point the migrator at a fixture file.
//!
//! ## Mapping from triage items
//!
//! Legacy triage items have no `source` field. They are mapped to:
//! - `source = "routine"` when `task_id` is present (they were produced by
//!   scheduled runs — `source_id` keeps the task id so rerun works),
//! - `source = "trigger"` otherwise (generic alerts),
//! - `title = kind`, `summary = message`, `status = archived/read/pending`
//!   (preserved), `created_at_ms = created_at * 1000`.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

// ── Source / status vocabulary ──────────────────────────────────────────

/// Inbox source: run of a scheduled task (task-store routine).
pub const SOURCE_ROUTINE: &str = "routine";
/// Inbox source: alias used by the desktop for task-store entries.
pub const SOURCE_SCHEDULED_TASK: &str = "scheduled_task";
/// Inbox source: goal-related event.
pub const SOURCE_GOAL: &str = "goal";
/// Inbox source: execution fired through an external endpoint trigger.
pub const SOURCE_TRIGGER: &str = "trigger";

/// Status vocabulary for inbox items (validated at the write boundary).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboxStatus {
    Pending,
    Read,
    Archived,
}

impl InboxStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Read => "read",
            Self::Archived => "archived",
        }
    }

    /// Parse a status string (case-insensitive). Unknown values are an error
    /// so the DB never accumulates off-vocabulary rows.
    pub fn parse(s: &str) -> Result<Self, InboxStoreError> {
        match s.trim().to_ascii_lowercase().as_str() {
            "pending" => Ok(Self::Pending),
            "read" => Ok(Self::Read),
            "archived" => Ok(Self::Archived),
            other => Err(InboxStoreError::BadStatus(other.to_string())),
        }
    }
}

// ── Errors ──────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum InboxStoreError {
    #[error("database error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid status: {0}")]
    BadStatus(String),
    #[error("invalid legacy triage line: {0}")]
    LegacyLine(String),
    #[error("inbox store lock poisoned")]
    Poisoned,
}

// ── Data types (serde contract is camelCase for the TS frontend) ───────

/// A single inbox row as returned by the store.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InboxItem {
    pub id: i64,
    pub source: String,
    pub source_id: Option<String>,
    pub session_id: Option<String>,
    pub title: String,
    pub summary: String,
    pub error: Option<String>,
    pub status: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// Payload for [`InboxStore::append_item`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxItemNew {
    pub source: String,
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub error: Option<String>,
}

/// Aggregate counts for the sidebar badge.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InboxStats {
    pub pending: u64,
    pub today: u64,
}

/// One automation run record (`routine_runs` table).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    pub id: String,
    pub task_id: String,
    pub task_name: Option<String>,
    pub status: String,
    pub error: Option<String>,
    pub started_at_ms: Option<i64>,
    pub finished_at_ms: Option<i64>,
    pub duration_ms: Option<i64>,
    pub inbox_item_id: Option<i64>,
}

/// Legacy `triage.jsonl` line shape (subset — unknown fields are ignored).
#[derive(Debug, Clone, Deserialize)]
struct LegacyTriageItem {
    #[serde(default)]
    task_id: Option<String>,
    kind: String,
    #[serde(default)]
    message: String,
    /// Seconds since epoch (legacy field).
    #[serde(default)]
    created_at: i64,
    #[serde(default)]
    read: bool,
    #[serde(default)]
    archived: bool,
}

// ── Store ───────────────────────────────────────────────────────────────

const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS inbox_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source TEXT NOT NULL,
    source_id TEXT,
    session_id TEXT,
    title TEXT NOT NULL,
    summary TEXT NOT NULL DEFAULT '',
    error TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_inbox_items_created ON inbox_items (created_at_ms DESC);
CREATE INDEX IF NOT EXISTS idx_inbox_items_status ON inbox_items (status);

CREATE TABLE IF NOT EXISTS routine_runs (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    task_name TEXT,
    status TEXT NOT NULL,
    error TEXT,
    started_at_ms INTEGER,
    finished_at_ms INTEGER,
    duration_ms INTEGER,
    inbox_item_id INTEGER
);
CREATE INDEX IF NOT EXISTS idx_routine_runs_started ON routine_runs (started_at_ms DESC);

CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

/// SQLite inbox + automation-run store. `&self` and `Send + Sync` — safe to
/// share behind an `Arc` from any thread (the connection is mutex-guarded).
pub struct InboxStore {
    conn: Mutex<Connection>,
}

impl InboxStore {
    /// Open (creating if needed) the store at `path`, running migrations and
    /// the one-shot legacy triage import from the default location
    /// (`~/.shannon/triage.jsonl`).
    pub fn open(path: &Path) -> Result<Self, InboxStoreError> {
        Self::open_with_legacy(path, Some(&default_legacy_triage_path()))
    }

    /// Open the store at the default location (`~/.shannon/inbox.db`).
    pub fn open_default() -> Result<Self, InboxStoreError> {
        Self::open(&default_db_path())
    }

    /// Open with an explicit legacy-triage path override (test seam). Pass
    /// `None` to skip the migration entirely.
    pub fn open_with_legacy(path: &Path, legacy: Option<&Path>) -> Result<Self, InboxStoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        Self::init(conn, legacy)
    }

    /// Open a throwaway in-memory store (tests + degraded fallback).
    pub fn open_in_memory() -> Result<Self, InboxStoreError> {
        Self::init(Connection::open_in_memory()?, None)
    }

    fn init(conn: Connection, legacy: Option<&Path>) -> Result<Self, InboxStoreError> {
        // WAL keeps concurrent desktop readers (UI polling + runner writes)
        // from blocking each other. On in-memory databases the pragma is a
        // no-op, which is fine.
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        conn.execute_batch(SCHEMA_SQL)?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.migrate_legacy_triage(legacy)?;
        Ok(store)
    }

    fn lock_conn(&self) -> Result<MutexGuard<'_, Connection>, InboxStoreError> {
        self.conn.lock().map_err(|_| InboxStoreError::Poisoned)
    }

    // ── inbox_items ─────────────────────────────────────────────────────

    /// Append a new pending item. Returns the stored row (with id).
    pub fn append_item(&self, item: InboxItemNew) -> Result<InboxItem, InboxStoreError> {
        let now = now_ms();
        let conn = self.lock_conn()?;
        conn.execute(
            "INSERT INTO inbox_items
                (source, source_id, session_id, title, summary, error, status, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7, ?7)",
            params![
                item.source,
                item.source_id,
                item.session_id,
                item.title,
                item.summary,
                item.error,
                now
            ],
        )?;
        Ok(InboxItem {
            id: conn.last_insert_rowid(),
            source: item.source,
            source_id: item.source_id,
            session_id: item.session_id,
            title: item.title,
            summary: item.summary,
            error: item.error,
            status: InboxStatus::Pending.as_str().to_string(),
            created_at_ms: now,
            updated_at_ms: now,
        })
    }

    /// Fetch a single item by id.
    pub fn get_item(&self, id: i64) -> Result<Option<InboxItem>, InboxStoreError> {
        let conn = self.lock_conn()?;
        conn.query_row(
            "SELECT id, source, source_id, session_id, title, summary, error, status, created_at_ms, updated_at_ms
             FROM inbox_items WHERE id = ?1",
            params![id],
            row_to_item,
        )
        .optional()
        .map_err(InboxStoreError::from)
    }

    /// List items, newest first, optionally filtered by status and source.
    pub fn list(
        &self,
        status: Option<InboxStatus>,
        source: Option<&str>,
        limit: u32,
    ) -> Result<Vec<InboxItem>, InboxStoreError> {
        let conn = self.lock_conn()?;
        let limit = i64::from(limit.max(1));
        // One branch per optional-filter combination so `params!` stays
        // positionally sound.
        match (status, source) {
            (Some(s), Some(src)) => query_items(
                &conn,
                " AND status = ?1 AND source = ?2",
                limit,
                params![s.as_str(), src],
            ),
            (Some(s), None) => query_items(&conn, " AND status = ?1", limit, params![s.as_str()]),
            (None, Some(src)) => query_items(&conn, " AND source = ?1", limit, params![src]),
            (None, None) => query_items(&conn, "", limit, params![]),
        }
    }

    /// Transition an item's status. Returns the updated row; errors with
    /// `QueryReturnedNoRows` when the id does not exist.
    pub fn update_status(
        &self,
        id: i64,
        status: InboxStatus,
    ) -> Result<InboxItem, InboxStoreError> {
        {
            let conn = self.lock_conn()?;
            let changed = conn.execute(
                "UPDATE inbox_items SET status = ?1, updated_at_ms = ?2 WHERE id = ?3",
                params![status.as_str(), now_ms(), id],
            )?;
            if changed == 0 {
                return Err(InboxStoreError::Sql(rusqlite::Error::QueryReturnedNoRows));
            }
        }
        self.get_item(id)?
            .ok_or(InboxStoreError::Sql(rusqlite::Error::QueryReturnedNoRows))
    }

    /// Badge counts: pending items and items created today (local time).
    pub fn stats(&self) -> Result<InboxStats, InboxStoreError> {
        let conn = self.lock_conn()?;
        let pending: u64 = conn.query_row(
            "SELECT COUNT(*) FROM inbox_items WHERE status = 'pending'",
            [],
            |r| r.get(0),
        )?;
        let today: u64 = conn.query_row(
            "SELECT COUNT(*) FROM inbox_items WHERE created_at_ms >= ?1",
            params![local_midnight_ms()],
            |r| r.get(0),
        )?;
        Ok(InboxStats { pending, today })
    }

    // ── routine_runs ────────────────────────────────────────────────────

    /// Record a new `running` run. Returns its id (used as the run id by
    /// every store, including the legacy JSONL mirror).
    pub fn record_run_start(
        &self,
        task_id: &str,
        task_name: &str,
    ) -> Result<String, InboxStoreError> {
        let id = uuid::Uuid::new_v4().to_string();
        let conn = self.lock_conn()?;
        conn.execute(
            "INSERT INTO routine_runs (id, task_id, task_name, status, started_at_ms)
             VALUES (?1, ?2, ?3, 'running', ?4)",
            params![id, task_id, task_name, now_ms()],
        )?;
        Ok(id)
    }

    /// Complete a run: sets status/error, computes `duration_ms` from the
    /// start timestamp, and links the inbox item produced by the run.
    pub fn record_run_finish(
        &self,
        run_id: &str,
        status: &str,
        error: Option<&str>,
        inbox_item_id: Option<i64>,
    ) -> Result<(), InboxStoreError> {
        let conn = self.lock_conn()?;
        conn.execute(
            "UPDATE routine_runs
             SET status = ?1, error = ?2, finished_at_ms = ?3,
                 duration_ms = ?3 - COALESCE(started_at_ms, ?3),
                 inbox_item_id = ?4
             WHERE id = ?5",
            params![status, error, now_ms(), inbox_item_id, run_id],
        )?;
        Ok(())
    }

    /// List runs, newest first. `rowid` breaks ties so same-millisecond
    /// starts come back in insertion order (newest insert first).
    pub fn list_runs(&self, limit: u32) -> Result<Vec<RunRecord>, InboxStoreError> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {RUN_COLUMNS} FROM routine_runs \
             ORDER BY started_at_ms DESC, rowid DESC LIMIT ?1"
        ))?;
        let limit = i64::from(limit.max(1));
        let mut q = stmt.query(params![limit])?;
        let mut out = Vec::new();
        while let Some(row) = q.next()? {
            out.push(RunRecord {
                id: row.get(0)?,
                task_id: row.get(1)?,
                task_name: row.get(2)?,
                status: row.get(3)?,
                error: row.get(4)?,
                started_at_ms: row.get(5)?,
                finished_at_ms: row.get(6)?,
                duration_ms: row.get(7)?,
                inbox_item_id: row.get(8)?,
            });
        }
        Ok(out)
    }

    // ── meta / legacy migration ─────────────────────────────────────────

    fn get_meta(&self, key: &str) -> Result<Option<String>, InboxStoreError> {
        let conn = self.lock_conn()?;
        conn.query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
            r.get(0)
        })
        .optional()
        .map_err(InboxStoreError::from)
    }

    fn set_meta(&self, key: &str, value: &str) -> Result<(), InboxStoreError> {
        let conn = self.lock_conn()?;
        conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// One-shot import of the legacy JSONL triage store (decision D6).
    ///
    /// Runs only when (a) the legacy file exists, (b) `inbox_items` is still
    /// empty, and (c) the `legacy_triage_imported` flag is unset. The flag is
    /// written after a successful import so the migration never re-runs.
    fn migrate_legacy_triage(&self, legacy: Option<&Path>) -> Result<(), InboxStoreError> {
        const FLAG: &str = "legacy_triage_imported";
        let Some(path) = legacy else {
            return Ok(());
        };
        if !path.exists() {
            return Ok(());
        }
        if self.get_meta(FLAG)?.as_deref() == Some("1") {
            return Ok(());
        }
        let count: u64 = {
            let conn = self.lock_conn()?;
            conn.query_row("SELECT COUNT(*) FROM inbox_items", [], |r| r.get(0))?
        };
        if count > 0 {
            // The user already has inbox rows — never merge legacy data in.
            return Ok(());
        }

        let content = std::fs::read_to_string(path)?;
        let mut items = Vec::new();
        for (lineno, line) in content.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            items.push(serde_json::from_str::<LegacyTriageItem>(line).map_err(|e| {
                InboxStoreError::LegacyLine(format!("{}:{lineno}: {e}", path.display()))
            })?);
        }

        {
            let conn = self.lock_conn()?;
            for item in items {
                let status = if item.archived {
                    InboxStatus::Archived
                } else if item.read {
                    InboxStatus::Read
                } else {
                    InboxStatus::Pending
                };
                let created_ms = if item.created_at > 0 {
                    item.created_at.saturating_mul(1000)
                } else {
                    now_ms()
                };
                conn.execute(
                    "INSERT INTO inbox_items
                        (source, source_id, session_id, title, summary, error, status, created_at_ms, updated_at_ms)
                     VALUES (?1, ?2, NULL, ?3, ?4, NULL, ?5, ?6, ?6)",
                    params![
                        if item.task_id.is_some() {
                            SOURCE_ROUTINE
                        } else {
                            SOURCE_TRIGGER
                        },
                        item.task_id,
                        item.kind,
                        item.message,
                        status.as_str(),
                        created_ms,
                    ],
                )?;
            }
        }
        self.set_meta(FLAG, "1")
    }
}

impl std::fmt::Debug for InboxStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InboxStore").finish_non_exhaustive()
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────

const ITEM_COLUMNS: &str = "id, source, source_id, session_id, title, summary, error, status, created_at_ms, updated_at_ms";
const RUN_COLUMNS: &str = "id, task_id, task_name, status, error, started_at_ms, finished_at_ms, duration_ms, inbox_item_id";

/// Shared list tail for [`InboxStore::list`]: newest first (created_at DESC,
/// id DESC as tiebreaker for same-millisecond inserts). `limit` is a trusted
/// internal value (derived from a `u32`), inlined into the SQL.
fn query_items<P: rusqlite::Params>(
    conn: &Connection,
    where_suffix: &str,
    limit: i64,
    p: P,
) -> Result<Vec<InboxItem>, InboxStoreError> {
    let sql = format!(
        "SELECT {ITEM_COLUMNS} FROM inbox_items WHERE 1=1{where_suffix} \
         ORDER BY created_at_ms DESC, id DESC LIMIT {limit}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut q = stmt.query(p)?;
    let mut out = Vec::new();
    while let Some(row) = q.next()? {
        out.push(row_to_item(row)?);
    }
    Ok(out)
}

fn row_to_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<InboxItem> {
    Ok(InboxItem {
        id: row.get(0)?,
        source: row.get(1)?,
        source_id: row.get(2)?,
        session_id: row.get(3)?,
        title: row.get(4)?,
        summary: row.get(5)?,
        error: row.get(6)?,
        status: row.get(7)?,
        created_at_ms: row.get(8)?,
        updated_at_ms: row.get(9)?,
    })
}

/// Milliseconds since the Unix epoch.
fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Start of the current local day in epoch milliseconds ("today" badge).
fn local_midnight_ms() -> i64 {
    use chrono::TimeZone;
    let midnight = chrono::Local::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|naive| chrono::Local.from_local_datetime(&naive).earliest());
    match midnight {
        Some(dt) => dt.timestamp_millis(),
        None => chrono::Utc::now().timestamp_millis(),
    }
}

/// Default database location: `~/.shannon/inbox.db`.
pub fn default_db_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".shannon")
        .join("inbox.db")
}

/// Default legacy triage store: `~/.shannon/triage.jsonl`.
pub fn default_legacy_triage_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".shannon")
        .join("triage.jsonl")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn item_new(title: &str) -> InboxItemNew {
        InboxItemNew {
            source: SOURCE_ROUTINE.to_string(),
            source_id: Some("task-1".to_string()),
            session_id: None,
            title: title.to_string(),
            summary: String::new(),
            error: None,
        }
    }

    // ── CRUD ────────────────────────────────────────────────────────────

    #[test]
    fn append_and_get_roundtrip() {
        let store = InboxStore::open_in_memory().unwrap();
        let new = InboxItemNew {
            session_id: Some("sess-9".into()),
            summary: "done in 3s".into(),
            ..item_new("Nightly scan")
        };
        let item = store.append_item(new).unwrap();
        assert_eq!(item.id, 1);
        assert_eq!(item.status, "pending");
        assert_eq!(item.session_id.as_deref(), Some("sess-9"));
        assert_eq!(item.source, SOURCE_ROUTINE);

        let back = store.get_item(item.id).unwrap().unwrap();
        assert_eq!(back, item);
        assert!(store.get_item(999).unwrap().is_none());
    }

    #[test]
    fn list_orders_newest_first_and_respects_limit() {
        let store = InboxStore::open_in_memory().unwrap();
        for i in 0..5 {
            let it = store.append_item(item_new(&format!("t{i}"))).unwrap();
            // Force distinct, increasing timestamps (same-ms inserts would
            // otherwise rely on the id tiebreaker).
            let conn = store.lock_conn().unwrap();
            conn.execute(
                "UPDATE inbox_items SET created_at_ms = ?1 WHERE id = ?2",
                params![1_000 + i, it.id],
            )
            .unwrap();
            drop(conn);
        }
        let all = store.list(None, None, 100).unwrap();
        assert_eq!(all.len(), 5);
        let ids_desc: Vec<i64> = all.iter().map(|i| i.id).collect();
        let mut sorted = ids_desc.clone();
        sorted.sort_unstable();
        sorted.reverse();
        assert_eq!(ids_desc, sorted, "newest first");

        let limited = store.list(None, None, 2).unwrap();
        assert_eq!(limited.len(), 2);
        assert_eq!(limited[0].id, ids_desc[0], "limit keeps newest");
    }

    #[test]
    fn list_filters_by_status_and_source() {
        let store = InboxStore::open_in_memory().unwrap();
        let a = store.append_item(item_new("a")).unwrap();
        store
            .append_item(InboxItemNew {
                source: SOURCE_TRIGGER.to_string(),
                ..item_new("b")
            })
            .unwrap();
        store.update_status(a.id, InboxStatus::Archived).unwrap();

        let pending = store.list(Some(InboxStatus::Pending), None, 100).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].title, "b");

        let archived = store.list(Some(InboxStatus::Archived), None, 100).unwrap();
        assert_eq!(archived.len(), 1);
        assert_eq!(archived[0].title, "a");

        let triggers = store.list(None, Some(SOURCE_TRIGGER), 100).unwrap();
        assert_eq!(triggers.len(), 1);
        assert_eq!(triggers[0].title, "b");

        let none = store
            .list(Some(InboxStatus::Read), Some(SOURCE_TRIGGER), 100)
            .unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn update_status_validates_and_updates_updated_at() {
        let store = InboxStore::open_in_memory().unwrap();
        let item = store.append_item(item_new("x")).unwrap();
        let updated = store.update_status(item.id, InboxStatus::Read).unwrap();
        assert_eq!(updated.status, "read");
        let err = InboxStatus::parse("nope").unwrap_err();
        assert!(err.to_string().contains("invalid status"));
        // Missing id errors.
        assert!(store.update_status(4242, InboxStatus::Read).is_err());
    }

    // ── stats ───────────────────────────────────────────────────────────

    #[test]
    fn stats_counts_pending_and_today() {
        let store = InboxStore::open_in_memory().unwrap();
        let a = store.append_item(item_new("a")).unwrap();
        store.append_item(item_new("b")).unwrap();
        store.update_status(a.id, InboxStatus::Read).unwrap();

        let stats = store.stats().unwrap();
        assert_eq!(stats.pending, 1);
        assert_eq!(stats.today, 2);

        // An item created yesterday still counts toward neither bucket.
        let conn = store.lock_conn().unwrap();
        conn.execute(
            "UPDATE inbox_items SET created_at_ms = ?1 WHERE title = 'b'",
            params![now_ms() - 48 * 3600 * 1000],
        )
        .unwrap();
        drop(conn);
        let stats = store.stats().unwrap();
        assert_eq!(stats.pending, 1, "read item no longer pending");
        assert_eq!(stats.today, 1, "yesterday's item excluded from today");
    }

    #[test]
    fn stats_empty_store_is_zero() {
        let store = InboxStore::open_in_memory().unwrap();
        let stats = store.stats().unwrap();
        assert_eq!(stats, InboxStats::default());
    }

    // ── runs ────────────────────────────────────────────────────────────

    #[test]
    fn run_start_finish_roundtrip() {
        let store = InboxStore::open_in_memory().unwrap();
        let run_id = store.record_run_start("task-1", "Task One").unwrap();

        let running = &store.list_runs(10).unwrap()[0];
        assert_eq!(running.id, run_id);
        assert_eq!(running.status, "running");
        assert_eq!(running.task_id, "task-1");
        assert!(running.finished_at_ms.is_none());
        assert!(running.duration_ms.is_none());

        let item = store.append_item(item_new("run output")).unwrap();
        store
            .record_run_finish(&run_id, "succeeded", None, Some(item.id))
            .unwrap();

        let done = &store.list_runs(10).unwrap()[0];
        assert_eq!(done.status, "succeeded");
        assert!(done.error.is_none());
        assert!(done.finished_at_ms.is_some());
        assert!(done.duration_ms.is_some());
        assert_eq!(done.inbox_item_id, Some(item.id));
    }

    #[test]
    fn run_finish_records_error() {
        let store = InboxStore::open_in_memory().unwrap();
        let run_id = store.record_run_start("t", "T").unwrap();
        store
            .record_run_finish(&run_id, "failed", Some("boom"), None)
            .unwrap();
        let run = &store.list_runs(10).unwrap()[0];
        assert_eq!(run.status, "failed");
        assert_eq!(run.error.as_deref(), Some("boom"));
        assert_eq!(run.inbox_item_id, None);
    }

    #[test]
    fn list_runs_orders_newest_first_and_limits() {
        let store = InboxStore::open_in_memory().unwrap();
        let first = store.record_run_start("t", "T").unwrap();
        let second = store.record_run_start("t", "T").unwrap();
        store
            .record_run_finish(&first, "succeeded", None, None)
            .unwrap(); // keeps start older
        let runs = store.list_runs(10).unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].id, second, "newest run first");
        assert_eq!(store.list_runs(1).unwrap().len(), 1);
    }

    // ── legacy triage migration ─────────────────────────────────────────

    fn write_legacy(path: &Path, lines: &[&str]) {
        std::fs::write(path, lines.join("\n") + "\n").unwrap();
    }

    #[test]
    fn migration_imports_legacy_items_and_sets_flag() {
        let tmp = tempfile::tempdir().unwrap();
        let legacy = tmp.path().join("triage.jsonl");
        write_legacy(
            &legacy,
            &[
                r#"{"id":"a1","kind":"failed_run","message":"run xyz failed","task_id":"task-9","task_name":"Scan","created_at":1700000000,"read":false,"archived":false,"revision":0}"#,
                r#"{"id":"b2","kind":"needs_review","message":"check this","created_at":1700000100,"read":true,"archived":false,"revision":1}"#,
                r#"{"id":"c3","kind":"budget_exceeded","message":"over budget","created_at":1700000200,"read":true,"archived":true,"revision":2}"#,
            ],
        );
        let db = tmp.path().join("inbox.db");
        let store = InboxStore::open_with_legacy(&db, Some(&legacy)).unwrap();

        let items = store.list(None, None, 100).unwrap();
        assert_eq!(items.len(), 3);
        // Newest first.
        assert_eq!(items[0].title, "budget_exceeded");

        let run_failure = &items[2];
        assert_eq!(run_failure.source, SOURCE_ROUTINE);
        assert_eq!(run_failure.source_id.as_deref(), Some("task-9"));
        assert_eq!(run_failure.status, "pending");
        assert_eq!(run_failure.created_at_ms, 1_700_000_000_000);
        assert_eq!(run_failure.summary, "run xyz failed");

        assert_eq!(items[1].source, SOURCE_TRIGGER);
        assert_eq!(items[1].status, "read");

        assert_eq!(items[0].status, "archived");

        assert_eq!(
            store.get_meta("legacy_triage_imported").unwrap().as_deref(),
            Some("1")
        );
    }

    #[test]
    fn migration_is_idempotent_across_reopens() {
        let tmp = tempfile::tempdir().unwrap();
        let legacy = tmp.path().join("triage.jsonl");
        write_legacy(
            &legacy,
            &[r#"{"id":"a1","kind":"failed_run","message":"m","created_at":1700000000}"#],
        );
        let db = tmp.path().join("inbox.db");
        let _ = InboxStore::open_with_legacy(&db, Some(&legacy)).unwrap();

        // Grow the legacy file after the first import.
        write_legacy(
            &legacy,
            &[
                r#"{"id":"a1","kind":"failed_run","message":"m","created_at":1700000000}"#,
                r#"{"id":"b2","kind":"needs_review","message":"late","created_at":1700000500}"#,
            ],
        );
        let store = InboxStore::open_with_legacy(&db, Some(&legacy)).unwrap();
        assert_eq!(
            store.list(None, None, 100).unwrap().len(),
            1,
            "no re-import"
        );
    }

    #[test]
    fn migration_skipped_when_inbox_not_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let legacy = tmp.path().join("triage.jsonl");
        write_legacy(
            &legacy,
            &[r#"{"id":"a1","kind":"failed_run","message":"m","created_at":1700000000}"#],
        );
        let db = tmp.path().join("inbox.db");
        {
            // The inbox is already in use (opened without the legacy file).
            let store = InboxStore::open_with_legacy(&db, None).unwrap();
            store.append_item(item_new("existing")).unwrap();
        }
        // A later open that points at the legacy file must NOT merge legacy
        // rows into a non-empty inbox — even with the flag unset.
        let store = InboxStore::open_with_legacy(&db, Some(&legacy)).unwrap();
        let items = store.list(None, None, 100).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "existing");
        assert!(store.get_meta("legacy_triage_imported").unwrap().is_none());
    }

    #[test]
    fn migration_missing_legacy_file_is_ok() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("inbox.db");
        let store =
            InboxStore::open_with_legacy(&db, Some(&tmp.path().join("nope.jsonl"))).unwrap();
        assert!(store.list(None, None, 10).unwrap().is_empty());
        assert!(store.get_meta("legacy_triage_imported").unwrap().is_none());
    }

    #[test]
    fn migration_skipped_when_flag_set() {
        let tmp = tempfile::tempdir().unwrap();
        let legacy = tmp.path().join("triage.jsonl");
        write_legacy(
            &legacy,
            &[r#"{"id":"a1","kind":"k","message":"m","created_at":1700000000}"#],
        );
        let db = tmp.path().join("inbox.db");
        {
            let store = InboxStore::open_with_legacy(&db, Some(&legacy)).unwrap();
            assert_eq!(store.list(None, None, 10).unwrap().len(), 1);
            // User clears the inbox afterwards.
            let conn = store.lock_conn().unwrap();
            conn.execute("DELETE FROM inbox_items", []).unwrap();
        }
        // Flag is still set → no re-import of the (still present) file.
        let store = InboxStore::open_with_legacy(&db, Some(&legacy)).unwrap();
        assert!(store.list(None, None, 10).unwrap().is_empty());
    }

    #[test]
    fn migration_malformed_line_is_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let legacy = tmp.path().join("triage.jsonl");
        write_legacy(&legacy, &["{not json"]);
        let db = tmp.path().join("inbox.db");
        let err = InboxStore::open_with_legacy(&db, Some(&legacy)).unwrap_err();
        assert!(err.to_string().contains("legacy triage line"));
    }

    // ── storage details / contract ──────────────────────────────────────

    #[test]
    fn file_store_uses_wal_mode() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("inbox.db");
        let store = InboxStore::open_with_legacy(&db, None).unwrap();
        let conn = store.lock_conn().unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode.to_ascii_lowercase(), "wal");
    }

    #[test]
    fn inbox_item_serializes_camel_case() {
        let item = InboxItem {
            id: 7,
            source: SOURCE_TRIGGER.into(),
            source_id: Some("task-2".into()),
            session_id: None,
            title: "T".into(),
            summary: "s".into(),
            error: None,
            status: "pending".into(),
            created_at_ms: 1,
            updated_at_ms: 2,
        };
        let json = serde_json::to_string(&item).unwrap();
        for key in ["sourceId", "sessionId", "createdAtMs", "updatedAtMs"] {
            assert!(json.contains(key), "missing {key} in {json}");
        }
        assert!(!json.contains("source_id"));
        // The TS contract expects every key present (null, not omitted).
        assert!(json.contains("\"sessionId\":null"));
    }

    #[test]
    fn status_parse_is_case_insensitive() {
        assert_eq!(InboxStatus::parse("PENDING").unwrap(), InboxStatus::Pending);
        assert_eq!(InboxStatus::parse("Read").unwrap(), InboxStatus::Read);
        assert_eq!(
            InboxStatus::parse(" archived ").unwrap(),
            InboxStatus::Archived
        );
    }

    #[test]
    fn store_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<InboxStore>();
    }

    #[test]
    fn default_paths_live_under_shannon_dir() {
        assert!(default_db_path().ends_with(".shannon/inbox.db"));
        assert!(default_legacy_triage_path().ends_with(".shannon/triage.jsonl"));
    }
}
