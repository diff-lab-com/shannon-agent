//! SQLite-backed project registry (P-E3, 阶段三·项目实体化).
//!
//! "Project" becomes a first-class engine entity: one row per absolute
//! working-directory path, with user curation (custom name, icon, color,
//! archived flag) attached. The store lives in its own database at
//! `~/.shannon/projects.db` — deliberately **not** shared with the inbox
//! store (`~/.shannon/inbox.db`), whose schema/migration lifecycle is
//! independent.
//!
//! ## Adoption, not migration
//!
//! The registry is seeded lazily from where project paths already appear —
//! session logs (`project_path`), routine working dirs, memory project
//! labels — via [`ProjectRegistry::ensure_adopted`]. Adoption is strictly
//! additive: existing rows (including archived ones) are never overwritten,
//! renamed or unarchived by a re-adoption; only never-seen paths are
//! registered. The desktop drives this on `list_projects` (backfill) and on
//! `set_session_working_dir` (incremental).
//!
//! Storage mirrors [`crate::inbox_store::InboxStore`]: idempotent
//! `SCHEMA_SQL` batch, WAL journal mode, a 2000 ms busy timeout, and a
//! `Mutex<Connection>` so one handle is `Send + Sync` behind an `Arc`.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

// ── Errors ──────────────────────────────────────────────────────────────

/// Errors surfaced by the project registry.
#[derive(Debug, thiserror::Error)]
pub enum ProjectRegistryError {
    #[error("database error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("project registry lock poisoned")]
    Poisoned,
}

// ── Data types (serde contract is camelCase for the TS frontend) ───────

/// One registered project row.
///
/// `path` is the unique key (absolute, canonicalized by the callers that
/// produce it). `name`/`icon`/`color` are user curation layers on top of the
/// derived path — `None` means "render the default" (the path's tail
/// segment) rather than "unsettable".
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRecord {
    /// Unique project path (the registry key).
    pub path: String,
    /// Custom display name; `None` → callers render the path's basename.
    pub name: Option<String>,
    /// Custom icon identifier (reserved for UI use; not yet populated).
    pub icon: Option<String>,
    /// Custom color token (reserved for UI use; not yet populated).
    pub color: Option<String>,
    /// When the project was archived (epoch ms); `None` = active.
    pub archived_at_ms: Option<i64>,
    /// When the project was first registered (epoch ms). Preserved across
    /// [`ProjectRegistry::upsert`].
    pub created_at_ms: i64,
}

/// One adoption candidate for [`ProjectRegistry::ensure_adopted`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectAdoptCandidate {
    /// Project path to register if not already present.
    pub path: String,
    /// Suggested display name stored only on first registration (typically
    /// the path's tail segment, or a memory project label). Existing rows —
    /// including archived ones — keep whatever they have.
    pub name_hint: Option<String>,
}

// ── Store ───────────────────────────────────────────────────────────────

const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS projects (
    path TEXT PRIMARY KEY,
    name TEXT,
    icon TEXT,
    color TEXT,
    archived_at_ms INTEGER,
    created_at_ms INTEGER NOT NULL
);
"#;

/// SQLite project registry (P-E3). `&self` and `Send + Sync` — safe to share
/// behind an `Arc` from any thread (the connection is mutex-guarded).
pub struct ProjectRegistry {
    conn: Mutex<Connection>,
}

impl ProjectRegistry {
    /// Open (creating if needed) the store at `path`.
    pub fn open(path: &Path) -> Result<Self, ProjectRegistryError> {
        Self::with_path(path)
    }

    /// Open the store at the default location (`~/.shannon/projects.db`).
    pub fn open_default() -> Result<Self, ProjectRegistryError> {
        Self::with_path(&default_db_path())
    }

    /// Open (creating if needed) the store at an explicit path — the test
    /// seam: tests point this at a `tempfile::tempdir()` file and never
    /// touch the process `HOME`.
    pub fn with_path(path: &Path) -> Result<Self, ProjectRegistryError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Self::init(Connection::open(path)?)
    }

    /// Open a throwaway in-memory store (tests + degraded fallback).
    pub fn open_in_memory() -> Result<Self, ProjectRegistryError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, ProjectRegistryError> {
        // WAL keeps concurrent desktop readers from blocking each other; a
        // busy timeout absorbs short cross-process write collisions (same
        // reasoning as the inbox store). Both pragmas are no-ops on
        // in-memory databases, which is fine.
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        let _ = conn.pragma_update(None, "busy_timeout", "2000");
        conn.execute_batch(SCHEMA_SQL)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn lock_conn(&self) -> Result<MutexGuard<'_, Connection>, ProjectRegistryError> {
        self.conn.lock().map_err(|_| ProjectRegistryError::Poisoned)
    }

    /// All projects sorted by path ascending. Archived rows are included
    /// only when `include_archived` is `true`.
    pub fn list(&self, include_archived: bool) -> Result<Vec<ProjectRecord>, ProjectRegistryError> {
        let conn = self.lock_conn()?;
        let sql = if include_archived {
            "SELECT path, name, icon, color, archived_at_ms, created_at_ms
             FROM projects ORDER BY path ASC"
        } else {
            "SELECT path, name, icon, color, archived_at_ms, created_at_ms
             FROM projects WHERE archived_at_ms IS NULL ORDER BY path ASC"
        };
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt
            .query_map([], row_to_record)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Fetch a single project by path.
    pub fn get(&self, path: &str) -> Result<Option<ProjectRecord>, ProjectRegistryError> {
        let conn = self.lock_conn()?;
        conn.query_row(
            "SELECT path, name, icon, color, archived_at_ms, created_at_ms
             FROM projects WHERE path = ?1",
            params![path],
            row_to_record,
        )
        .optional()
        .map_err(ProjectRegistryError::from)
    }

    /// Overwrite the curation fields of `record.path`, preserving the
    /// original `created_at_ms` of an existing row (a brand-new path starts
    /// at the record's own `created_at_ms`). Returns the stored record.
    pub fn upsert(&self, record: ProjectRecord) -> Result<ProjectRecord, ProjectRegistryError> {
        {
            let conn = self.lock_conn()?;
            conn.execute(
                "INSERT INTO projects (path, name, icon, color, archived_at_ms, created_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(path) DO UPDATE SET
                     name = excluded.name,
                     icon = excluded.icon,
                     color = excluded.color,
                     archived_at_ms = excluded.archived_at_ms",
                params![
                    record.path,
                    record.name,
                    record.icon,
                    record.color,
                    record.archived_at_ms,
                    record.created_at_ms
                ],
            )?;
        }
        self.get(&record.path)?
            .ok_or_else(|| ProjectRegistryError::Sql(rusqlite::Error::QueryReturnedNoRows))
    }

    /// Set the custom display name (`None` clears it). An unknown path is
    /// auto-created first (upsert semantics — keeps adoption idempotence
    /// simple). Returns the stored record.
    pub fn rename(
        &self,
        path: &str,
        name: Option<String>,
    ) -> Result<ProjectRecord, ProjectRegistryError> {
        {
            let conn = self.lock_conn()?;
            ensure_row(&conn, path)?;
            conn.execute(
                "UPDATE projects SET name = ?2 WHERE path = ?1",
                params![path, name],
            )?;
        }
        self.get(path)?
            .ok_or_else(|| ProjectRegistryError::Sql(rusqlite::Error::QueryReturnedNoRows))
    }

    /// Set the custom icon and color (`None` clears a field). An unknown
    /// path is auto-created first (upsert semantics). Returns the stored
    /// record.
    pub fn set_appearance(
        &self,
        path: &str,
        icon: Option<String>,
        color: Option<String>,
    ) -> Result<ProjectRecord, ProjectRegistryError> {
        {
            let conn = self.lock_conn()?;
            ensure_row(&conn, path)?;
            conn.execute(
                "UPDATE projects SET icon = ?2, color = ?3 WHERE path = ?1",
                params![path, icon, color],
            )?;
        }
        self.get(path)?
            .ok_or_else(|| ProjectRegistryError::Sql(rusqlite::Error::QueryReturnedNoRows))
    }

    /// Archive or unarchive a project. Archiving stamps `archived_at_ms`
    /// with the current time; unarchiving clears it. An unknown path is
    /// auto-created first (upsert semantics). Returns the stored record.
    pub fn set_archived(
        &self,
        path: &str,
        archived: bool,
    ) -> Result<ProjectRecord, ProjectRegistryError> {
        {
            let conn = self.lock_conn()?;
            ensure_row(&conn, path)?;
            conn.execute(
                "UPDATE projects SET archived_at_ms = ?2 WHERE path = ?1",
                params![path, if archived { Some(now_ms()) } else { None }],
            )?;
        }
        self.get(path)?
            .ok_or_else(|| ProjectRegistryError::Sql(rusqlite::Error::QueryReturnedNoRows))
    }

    /// Register every candidate path that is not already present — the
    /// adopt-not-migrate primitive. Existing rows are never touched: no
    /// rename, no appearance change, no unarchive (an archived row stays
    /// archived), and the `name_hint` only lands on first registration.
    /// Returns the number of newly registered rows.
    pub fn ensure_adopted(
        &self,
        candidates: &[ProjectAdoptCandidate],
    ) -> Result<usize, ProjectRegistryError> {
        let conn = self.lock_conn()?;
        let mut adopted = 0usize;
        for candidate in candidates {
            if candidate.path.trim().is_empty() {
                continue;
            }
            adopted += conn.execute(
                "INSERT OR IGNORE INTO projects (path, name, icon, color, archived_at_ms, created_at_ms)
                 VALUES (?1, ?2, NULL, NULL, NULL, ?3)",
                params![candidate.path, candidate.name_hint, now_ms()],
            )?;
        }
        Ok(adopted)
    }
}

/// Insert the blank row for `path` if it does not exist yet, so the
/// mutation commands (`rename` / `set_appearance` / `set_archived`) can
/// auto-create with upsert semantics instead of erroring on unknown paths.
fn ensure_row(conn: &Connection, path: &str) -> Result<(), ProjectRegistryError> {
    conn.execute(
        "INSERT OR IGNORE INTO projects (path, name, icon, color, archived_at_ms, created_at_ms)
         VALUES (?1, NULL, NULL, NULL, NULL, ?2)",
        params![path, now_ms()],
    )?;
    Ok(())
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectRecord> {
    Ok(ProjectRecord {
        path: row.get(0)?,
        name: row.get(1)?,
        icon: row.get(2)?,
        color: row.get(3)?,
        archived_at_ms: row.get(4)?,
        created_at_ms: row.get(5)?,
    })
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Default database location: `~/.shannon/projects.db`.
pub fn default_db_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".shannon")
        .join("projects.db")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn in_memory_store_starts_empty() {
        let store = ProjectRegistry::open_in_memory().unwrap();
        assert!(store.list(true).unwrap().is_empty());
        assert_eq!(store.get("/nope").unwrap(), None);
    }

    #[test]
    fn with_path_persists_across_reopen() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("projects.db");
        let created = ProjectRegistry::with_path(&db).unwrap();
        let record = created
            .upsert(ProjectRecord {
                path: "/work/proj-a".into(),
                name: Some("Alpha".into()),
                icon: None,
                color: Some("teal".into()),
                archived_at_ms: None,
                created_at_ms: 1_234,
            })
            .unwrap();
        assert_eq!(record.created_at_ms, 1_234);

        let reopened = ProjectRegistry::with_path(&db).unwrap();
        let got = reopened.get("/work/proj-a").unwrap().unwrap();
        assert_eq!(got.name.as_deref(), Some("Alpha"));
        assert_eq!(got.color.as_deref(), Some("teal"));
        assert_eq!(got.created_at_ms, 1_234, "created_at survives reopen");
    }

    #[test]
    fn upsert_overwrites_curation_but_preserves_created_at() {
        let store = ProjectRegistry::open_in_memory().unwrap();
        store
            .upsert(ProjectRecord {
                path: "/work/proj-a".into(),
                name: Some("First".into()),
                icon: Some("code".into()),
                color: None,
                archived_at_ms: None,
                created_at_ms: 1_000,
            })
            .unwrap();

        let updated = store
            .upsert(ProjectRecord {
                path: "/work/proj-a".into(),
                name: Some("Second".into()),
                icon: None,
                color: Some("amber".into()),
                archived_at_ms: Some(5_555),
                created_at_ms: 9_999, // ignored: original must win
            })
            .unwrap();
        assert_eq!(updated.created_at_ms, 1_000, "upsert keeps created_at");
        assert_eq!(updated.name.as_deref(), Some("Second"));
        assert_eq!(updated.icon, None);
        assert_eq!(updated.color.as_deref(), Some("amber"));
        assert_eq!(updated.archived_at_ms, Some(5_555));

        // Exactly one row per path.
        assert_eq!(store.list(true).unwrap().len(), 1);
    }

    #[test]
    fn rename_sets_and_clears_custom_name() {
        let store = ProjectRegistry::open_in_memory().unwrap();
        let renamed = store.rename("/work/proj-a", Some("Custom".into())).unwrap();
        assert_eq!(renamed.name.as_deref(), Some("Custom"));
        assert!(renamed.name.is_some());
        let cleared = store.rename("/work/proj-a", None).unwrap();
        assert_eq!(cleared.name, None);
    }

    #[test]
    fn set_appearance_sets_and_clears_icon_and_color() {
        let store = ProjectRegistry::open_in_memory().unwrap();
        let styled = store
            .set_appearance("/work/proj-a", Some("folder".into()), Some("teal".into()))
            .unwrap();
        assert_eq!(styled.icon.as_deref(), Some("folder"));
        assert_eq!(styled.color.as_deref(), Some("teal"));
        let cleared = store.set_appearance("/work/proj-a", None, None).unwrap();
        assert_eq!(cleared.icon, None);
        assert_eq!(cleared.color, None);
    }

    #[test]
    fn set_archived_round_trips() {
        let store = ProjectRegistry::open_in_memory().unwrap();
        let archived = store.set_archived("/work/proj-a", true).unwrap();
        assert!(archived.archived_at_ms.is_some());
        // Hidden from the default listing, visible with include_archived.
        assert!(store.list(false).unwrap().is_empty());
        assert_eq!(store.list(true).unwrap().len(), 1);
        let unarchived = store.set_archived("/work/proj-a", false).unwrap();
        assert_eq!(unarchived.archived_at_ms, None);
        assert_eq!(store.list(false).unwrap().len(), 1);
    }

    #[test]
    fn mutations_auto_create_unknown_paths() {
        let store = ProjectRegistry::open_in_memory().unwrap();
        let created = store.rename("/work/new", Some("Fresh".into())).unwrap();
        assert_eq!(created.path, "/work/new");
        assert_eq!(created.name.as_deref(), Some("Fresh"));
        assert!(created.created_at_ms > 0, "auto-created row is timestamped");

        let created = store
            .set_appearance("/work/new2", None, Some("rose".into()))
            .unwrap();
        assert_eq!(created.color.as_deref(), Some("rose"));

        let created = store.set_archived("/work/new3", true).unwrap();
        assert!(created.archived_at_ms.is_some());

        assert_eq!(store.list(true).unwrap().len(), 3);
    }

    #[test]
    fn list_sorts_by_path_ascending() {
        let store = ProjectRegistry::open_in_memory().unwrap();
        for path in ["/work/zeta", "/work/alpha", "/home/aa"] {
            store.rename(path, None).unwrap();
        }
        let paths: Vec<String> = store
            .list(true)
            .unwrap()
            .into_iter()
            .map(|r| r.path)
            .collect();
        assert_eq!(paths, ["/home/aa", "/work/alpha", "/work/zeta"]);
    }

    #[test]
    fn ensure_adopted_registers_only_missing_paths() {
        let store = ProjectRegistry::open_in_memory().unwrap();
        let candidates = [
            ProjectAdoptCandidate {
                path: "/work/proj-a".into(),
                name_hint: Some("proj-a".into()),
            },
            ProjectAdoptCandidate {
                path: "/work/proj-b".into(),
                name_hint: Some("proj-b".into()),
            },
            ProjectAdoptCandidate {
                path: "/work/proj-a".into(), // duplicate candidate
                name_hint: Some("different".into()),
            },
            ProjectAdoptCandidate {
                path: "   ".into(), // blank paths are skipped, not registered
                name_hint: None,
            },
        ];
        assert_eq!(store.ensure_adopted(&candidates).unwrap(), 2);
        // Re-adoption is a no-op and returns zero.
        assert_eq!(store.ensure_adopted(&candidates).unwrap(), 0);
        // The first hint wins; the duplicate never overwrote it.
        assert_eq!(
            store.get("/work/proj-a").unwrap().unwrap().name.as_deref(),
            Some("proj-a")
        );
        assert_eq!(store.list(true).unwrap().len(), 2);
    }

    #[test]
    fn ensure_adopted_never_touches_archived_rows() {
        let store = ProjectRegistry::open_in_memory().unwrap();
        store.set_archived("/work/archived", true).unwrap();
        let before = store.get("/work/archived").unwrap().unwrap();

        let count = store
            .ensure_adopted(&[ProjectAdoptCandidate {
                path: "/work/archived".into(),
                name_hint: Some("re-adopted".into()),
            }])
            .unwrap();
        assert_eq!(count, 0, "an archived row is never re-adopted");
        assert_eq!(store.get("/work/archived").unwrap().unwrap(), before);
        // Still archived, and still hidden from the active listing.
        assert!(store.list(false).unwrap().is_empty());
    }

    #[test]
    fn ensure_adopted_never_reverts_customization() {
        let store = ProjectRegistry::open_in_memory().unwrap();
        store.rename("/work/proj-a", Some("Mine".into())).unwrap();
        store
            .ensure_adopted(&[ProjectAdoptCandidate {
                path: "/work/proj-a".into(),
                name_hint: Some("proj-a".into()),
            }])
            .unwrap();
        assert_eq!(
            store.get("/work/proj-a").unwrap().unwrap().name.as_deref(),
            Some("Mine"),
            "adoption must not overwrite a curated name"
        );
    }

    #[test]
    fn default_db_path_is_under_shannon_home() {
        let path = default_db_path();
        assert!(path.ends_with(".shannon/projects.db"), "{path:?}");
    }
}
