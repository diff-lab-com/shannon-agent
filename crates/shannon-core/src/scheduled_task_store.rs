//! Claude Code-style storage for scheduled tasks.
//!
//! Each task is stored as a directory containing:
//! - `SKILL.md`: the prompt content (markdown, human-editable)
//! - `task.json`: the [`ScheduledRoutine`] metadata (machine-managed)
//! - `working_dir`: optional one-line project path sidecar (P-E1; see
//!   [`ScheduledTaskStore::working_dir_of`])
//!
//! ## Layout
//! ```text
//! ~/.shannon/scheduled-tasks/
//! ├── <task-slug>-<id>/
//! │   ├── SKILL.md
//! │   ├── task.json
//! │   └── working_dir   (optional)
//! └── ...
//! ```
//!
//! ## Migration
//! Use [`ScheduledTaskStore::migrate_from_routines_json`] to import legacy
//! `~/.shannon/routines.json` data. The original file is renamed to
//! `routines.json.bak` (not deleted) for safety.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::scheduled_routines::{RoutineManager, ScheduledRoutine};

/// Errors returned by the scheduled task store.
#[derive(Debug, thiserror::Error)]
pub enum TaskStoreError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("task not found: {0}")]
    NotFound(String),
}

/// Sidecar file name holding a task's working directory (one line).
const WORKING_DIR_SIDECAR: &str = "working_dir";

/// Claude Code-style scheduled task storage.
///
/// Stores each task as `SKILL.md` + `task.json` under a per-task directory.
#[derive(Debug, Clone)]
pub struct ScheduledTaskStore {
    base_dir: PathBuf,
}

impl ScheduledTaskStore {
    /// Create a store at the default location (`~/.shannon/scheduled-tasks/`).
    pub fn new() -> Self {
        Self {
            base_dir: default_base_dir(),
        }
    }

    /// Create a store at a custom base directory (useful for testing).
    pub fn with_base(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    /// Return the base directory path.
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Save a routine as `SKILL.md` + `task.json`.
    ///
    /// Creates the per-task directory if it doesn't exist. Overwrites
    /// existing files with the same slug. The directory name embeds the name
    /// slug (`<slug>-<id>`), so saving a *renamed* routine migrates the
    /// previous directory — `task.json`, `SKILL.md` and the `working_dir`
    /// sidecar travel with it — and no `<old-slug>-<id>` directory remains:
    /// a task's durable state lives in exactly one directory per id.
    pub fn save(&self, routine: &ScheduledRoutine) -> Result<PathBuf, TaskStoreError> {
        fs::create_dir_all(&self.base_dir)?;
        let task_dir = self.task_dir(&routine.id, &routine.name);
        self.migrate_task_dir(&routine.id, &task_dir)?;
        fs::create_dir_all(&task_dir)?;

        fs::write(task_dir.join("SKILL.md"), &routine.prompt)?;
        let json = serde_json::to_string_pretty(routine)?;
        fs::write(task_dir.join("task.json"), json)?;

        Ok(task_dir)
    }

    /// Keep the store invariant "exactly one directory per task id".
    ///
    /// The directory name embeds the name slug, so a rename changes the
    /// expected path. Any other same-id directory is a leftover of a previous
    /// name: when `target` does not exist yet, the deterministically-first
    /// leftover is moved there — the whole directory travels, carrying
    /// `task.json`, `SKILL.md` and the `working_dir` sidecar — and any
    /// further leftovers are removed. When a directory already exists at the
    /// target (the freshly-saved, authoritative one), every leftover is
    /// removed. Without this, prefix resolution could race between multiple
    /// `-<id>` directories and read a stale `task.json`/sidecar.
    fn migrate_task_dir(&self, id: &str, target: &Path) -> io::Result<()> {
        let stales: Vec<PathBuf> = self
            .same_id_dirs(id)
            .into_iter()
            .filter(|d| d != target)
            .collect();
        if stales.is_empty() {
            return Ok(());
        }
        if !target.exists() {
            // Same filesystem by construction (both under base_dir), so the
            // directory move is atomic.
            fs::rename(&stales[0], target)?;
            for stale in &stales[1..] {
                fs::remove_dir_all(stale)?;
            }
        } else {
            for stale in &stales {
                fs::remove_dir_all(stale)?;
            }
        }
        Ok(())
    }

    /// Load a routine by ID, ID prefix, or slug prefix.
    pub fn load(&self, id_or_name: &str) -> Result<Option<ScheduledRoutine>, TaskStoreError> {
        let task_dir = match self.resolve_task_dir(id_or_name) {
            Some(p) => p,
            None => return Ok(None),
        };
        let task_json_path = task_dir.join("task.json");
        if !task_json_path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&task_json_path)?;
        let routine: ScheduledRoutine = serde_json::from_str(&content)?;
        Ok(Some(routine))
    }

    /// List all routines, sorted by `created_at`.
    pub fn list(&self) -> Result<Vec<ScheduledRoutine>, TaskStoreError> {
        if !self.base_dir.exists() {
            return Ok(Vec::new());
        }
        let mut routines = Vec::new();
        for entry in fs::read_dir(&self.base_dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let task_json = entry.path().join("task.json");
            if !task_json.exists() {
                continue;
            }
            let content = fs::read_to_string(&task_json)?;
            let routine: ScheduledRoutine = serde_json::from_str(&content)?;
            routines.push(routine);
        }
        routines.sort_by_key(|r| r.created_at);
        Ok(routines)
    }

    /// Delete a routine by ID or slug prefix. Returns true if deleted.
    pub fn delete(&self, id_or_name: &str) -> Result<bool, TaskStoreError> {
        let task_dir = match self.resolve_task_dir(id_or_name) {
            Some(p) => p,
            None => return Ok(false),
        };
        fs::remove_dir_all(&task_dir)?;
        Ok(true)
    }

    /// Read the task's working-directory sidecar (P-E1).
    ///
    /// The sidecar is the `working_dir` file inside the task directory, one
    /// line, the path. `Ok(None)` when the task exists but has no working
    /// directory set; `Err` (`io::ErrorKind::NotFound`) when no task
    /// directory matches `id`.
    ///
    /// Deliberately a sidecar: [`ScheduledRoutine`] keeps its serialized
    /// shape, so hosts gain the field additively (desktop DTO) instead of
    /// through a struct change.
    pub fn working_dir_of(&self, id: &str) -> io::Result<Option<String>> {
        let task_dir = self
            .resolve_task_dir(id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("task not found: {id}")))?;
        match fs::read_to_string(task_dir.join(WORKING_DIR_SIDECAR)) {
            Ok(content) => Ok(Some(content.trim().to_string())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Write or clear the task's working-directory sidecar (P-E1).
    ///
    /// `Some(dir)` stores the path (trimmed, one line) atomically via a
    /// temp-file + rename inside the task directory; an empty/whitespace
    /// `dir` clears the sidecar, as does `None` (removing the file is
    /// idempotent). `Err` (`io::ErrorKind::NotFound`) when no task
    /// directory matches `id`.
    pub fn set_working_dir(&self, id: &str, dir: Option<&str>) -> io::Result<()> {
        let task_dir = self
            .resolve_task_dir(id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("task not found: {id}")))?;
        let sidecar = task_dir.join(WORKING_DIR_SIDECAR);
        let trimmed = dir.map(str::trim).unwrap_or("");
        if trimmed.is_empty() {
            // Clear (idempotent): a missing file is already "no working dir".
            return match fs::remove_file(&sidecar) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e),
            };
        }
        // tmp + rename keeps a crash from leaving a truncated sidecar.
        let tmp = task_dir.join(format!("{WORKING_DIR_SIDECAR}.tmp"));
        fs::write(&tmp, format!("{trimmed}\n"))?;
        fs::rename(&tmp, &sidecar)
    }

    /// Every distinct working directory across all tasks, sorted (P-E1).
    ///
    /// Best-effort collector for project adoption: unreadable or missing
    /// sidecars are skipped, blank entries never surface. Empty when the
    /// store does not exist yet.
    pub fn working_dirs(&self) -> Vec<String> {
        let mut dirs = Vec::new();
        let Ok(entries) = fs::read_dir(&self.base_dir) else {
            return dirs;
        };
        for entry in entries.flatten() {
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            if let Ok(content) = fs::read_to_string(entry.path().join(WORKING_DIR_SIDECAR)) {
                let dir = content.trim();
                if !dir.is_empty() {
                    dirs.push(dir.to_string());
                }
            }
        }
        dirs.sort();
        dirs.dedup();
        dirs
    }

    /// Migrate from legacy `~/.shannon/routines.json` to per-task SKILL.md + task.json.
    ///
    /// - If `legacy_path` doesn't exist, returns `Ok(0)`.
    /// - On success, renames `legacy_path` to `<legacy_path>.bak`.
    /// - Idempotent: skips tasks whose directory already exists in the new store.
    pub fn migrate_from_routines_json(&self, legacy_path: &Path) -> Result<usize, TaskStoreError> {
        if !legacy_path.exists() {
            return Ok(0);
        }
        let content = fs::read_to_string(legacy_path)?;
        let manager: RoutineManager = serde_json::from_str(&content)?;

        fs::create_dir_all(&self.base_dir)?;
        let mut migrated = 0usize;
        for routine in manager.routines.values() {
            let task_dir = self.task_dir(&routine.id, &routine.name);
            if task_dir.exists() {
                continue;
            }
            fs::create_dir_all(&task_dir)?;
            fs::write(task_dir.join("SKILL.md"), &routine.prompt)?;
            let json = serde_json::to_string_pretty(routine)?;
            fs::write(task_dir.join("task.json"), json)?;
            migrated += 1;
        }

        let backup = legacy_path.with_extension("json.bak");
        fs::rename(legacy_path, &backup)?;

        Ok(migrated)
    }

    /// Compute the per-task directory path: `<base>/<slug>-<id>`.
    fn task_dir(&self, id: &str, name: &str) -> PathBuf {
        let slug = slugify(name);
        self.base_dir.join(format!("{slug}-{id}"))
    }

    /// Resolve an ID or name prefix to a task directory path.
    ///
    /// Deterministic: candidates are resolved in sorted directory-name order,
    /// so the winner never depends on filesystem enumeration order. With the
    /// [`save`](Self::save) migration there is exactly one directory per id;
    /// more than one match means a legacy orphan survived — the (stable)
    /// first candidate wins and the ambiguity is warned about.
    fn resolve_task_dir(&self, id_or_name: &str) -> Option<PathBuf> {
        if !self.base_dir.exists() {
            return None;
        }
        let candidates = self.matching_dirs(id_or_name);
        match candidates.len() {
            0 => None,
            1 => candidates.into_iter().next(),
            _ => {
                tracing::warn!(
                    id = %id_or_name,
                    count = candidates.len(),
                    winner = %candidates[0].display(),
                    "scheduled task store: ambiguous id resolves to multiple directories (legacy orphan); \
                     the next save cleans this up"
                );
                candidates.into_iter().next()
            }
        }
    }

    /// Every task directory matching an ID / name / slug prefix, in sorted
    /// (deterministic) directory-name order.
    fn matching_dirs(&self, id_or_name: &str) -> Vec<PathBuf> {
        if !self.base_dir.exists() {
            return Vec::new();
        }
        let Ok(entries) = fs::read_dir(&self.base_dir) else {
            return Vec::new();
        };
        let mut dirs: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| {
                let name = e.file_name();
                let name = name.to_string_lossy();
                name == id_or_name
                    || name.ends_with(&format!("-{id_or_name}"))
                    || name.starts_with(&format!("{id_or_name}-"))
            })
            .map(|e| e.path())
            .collect();
        dirs.sort();
        dirs
    }

    /// Every directory holding this exact task id (any name slug), sorted.
    fn same_id_dirs(&self, id: &str) -> Vec<PathBuf> {
        if !self.base_dir.exists() {
            return Vec::new();
        }
        let Ok(entries) = fs::read_dir(&self.base_dir) else {
            return Vec::new();
        };
        let mut dirs: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| {
                e.file_type().map(|t| t.is_dir()).unwrap_or(false)
                    && e.file_name()
                        .to_string_lossy()
                        .ends_with(&format!("-{id}"))
            })
            .map(|e| e.path())
            .collect();
        dirs.sort();
        dirs
    }
}

impl Default for ScheduledTaskStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert a task name to a filesystem-safe slug.
fn slugify(name: &str) -> String {
    let slug: String = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "task".to_string()
    } else {
        slug
    }
}

/// Default base directory: `~/.shannon/scheduled-tasks/`.
fn default_base_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".shannon")
        .join("scheduled-tasks")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_slugify_basic() {
        assert_eq!(slugify("Daily Standup"), "daily-standup");
        assert_eq!(slugify("Weekly Report!"), "weekly-report");
        assert_eq!(slugify("  spaced  "), "spaced");
    }

    #[test]
    fn test_slugify_empty() {
        assert_eq!(slugify(""), "task");
        assert_eq!(slugify("---"), "task");
    }

    #[test]
    fn test_save_and_load() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledTaskStore::with_base(tmp.path().to_path_buf());
        let routine = ScheduledRoutine::new("Test Task".into(), "Hello, world!".into(), 60);
        store.save(&routine).unwrap();

        let loaded = store.load(&routine.id).unwrap().unwrap();
        assert_eq!(loaded.id, routine.id);
        assert_eq!(loaded.prompt, "Hello, world!");
    }

    #[test]
    fn test_load_by_slug_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledTaskStore::with_base(tmp.path().to_path_buf());
        let routine = ScheduledRoutine::new("My Task".into(), "prompt".into(), 60);
        store.save(&routine).unwrap();

        let loaded = store.load("my-task").unwrap();
        assert!(loaded.is_some());
    }

    #[test]
    fn test_list_sorted_by_created_at() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledTaskStore::with_base(tmp.path().to_path_buf());
        store
            .save(&ScheduledRoutine::new("a".into(), "p".into(), 60))
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        store
            .save(&ScheduledRoutine::new("b".into(), "p".into(), 60))
            .unwrap();

        let list = store.list().unwrap();
        assert_eq!(list.len(), 2);
        assert!(list[0].created_at <= list[1].created_at);
    }

    #[test]
    fn test_delete() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledTaskStore::with_base(tmp.path().to_path_buf());
        let routine = ScheduledRoutine::new("del".into(), "p".into(), 60);
        store.save(&routine).unwrap();
        assert!(store.delete(&routine.id).unwrap());
        assert!(store.load(&routine.id).unwrap().is_none());
    }

    #[test]
    fn test_delete_nonexistent_returns_false() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledTaskStore::with_base(tmp.path().to_path_buf());
        assert!(!store.delete("nonexistent").unwrap());
    }

    #[test]
    fn test_skill_md_written() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledTaskStore::with_base(tmp.path().to_path_buf());
        let routine = ScheduledRoutine::new("Task".into(), "# Title\n\nbody text".into(), 60);
        let task_dir = store.save(&routine).unwrap();
        let skill = std::fs::read_to_string(task_dir.join("SKILL.md")).unwrap();
        assert!(skill.contains("# Title"));
    }

    #[test]
    fn test_migrate_from_legacy() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().to_path_buf();

        let mut mgr = RoutineManager::new();
        mgr.add(ScheduledRoutine::new(
            "legacy1".into(),
            "prompt1".into(),
            60,
        ));
        mgr.add(ScheduledRoutine::new(
            "legacy2".into(),
            "prompt2".into(),
            3600,
        ));
        let legacy_path = tmp.path().join("routines.json");
        mgr.save_to_file(&legacy_path).unwrap();

        let store = ScheduledTaskStore::with_base(base.join("scheduled-tasks"));
        let count = store.migrate_from_routines_json(&legacy_path).unwrap();
        assert_eq!(count, 2);

        assert!(tmp.path().join("routines.json.bak").exists());
        assert!(!legacy_path.exists());

        let list = store.list().unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_migrate_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().to_path_buf();

        let mut mgr = RoutineManager::new();
        mgr.add(ScheduledRoutine::new(
            "legacy1".into(),
            "prompt1".into(),
            60,
        ));
        let legacy_path = tmp.path().join("routines.json");
        mgr.save_to_file(&legacy_path).unwrap();

        let store = ScheduledTaskStore::with_base(base.join("scheduled-tasks"));
        let count1 = store.migrate_from_routines_json(&legacy_path).unwrap();
        assert_eq!(count1, 1);

        mgr.save_to_file(&legacy_path).unwrap();
        let count2 = store.migrate_from_routines_json(&legacy_path).unwrap();
        assert_eq!(count2, 0);
    }

    #[test]
    fn test_migrate_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledTaskStore::with_base(tmp.path().to_path_buf());
        let missing = tmp.path().join("nope.json");
        let count = store.migrate_from_routines_json(&missing).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_load_nonexistent_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledTaskStore::with_base(tmp.path().to_path_buf());
        assert!(store.load("nonexistent").unwrap().is_none());
    }

    #[test]
    fn test_list_empty_when_no_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledTaskStore::with_base(tmp.path().to_path_buf());
        assert!(store.list().unwrap().is_empty());
    }

    // ── working_dir sidecar (P-E1) ──────────────────────────────────────

    #[test]
    fn working_dir_set_then_get_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledTaskStore::with_base(tmp.path().to_path_buf());
        let routine = ScheduledRoutine::new("Sidecar Task".into(), "p".into(), 60);
        store.save(&routine).unwrap();

        assert_eq!(store.working_dir_of(&routine.id).unwrap(), None);
        store
            .set_working_dir(&routine.id, Some("/work/proj"))
            .unwrap();
        assert_eq!(
            store.working_dir_of(&routine.id).unwrap().as_deref(),
            Some("/work/proj")
        );

        // Overwrite replaces the previous line (tmp+rename path).
        store
            .set_working_dir(&routine.id, Some("/work/other"))
            .unwrap();
        assert_eq!(
            store.working_dir_of(&routine.id).unwrap().as_deref(),
            Some("/work/other")
        );
        // No temp file lingers after the rename.
        let task_dir = store.load(&routine.id).unwrap();
        assert!(task_dir.is_some());
        assert!(
            !store
                .base_dir()
                .read_dir()
                .unwrap()
                .flatten()
                .any(|e| e.file_name().to_string_lossy().ends_with(".tmp")),
            "tmp sidecar must not survive a successful write"
        );
    }

    #[test]
    fn working_dir_whitespace_is_trimmed_on_both_sides() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledTaskStore::with_base(tmp.path().to_path_buf());
        let routine = ScheduledRoutine::new("trim".into(), "p".into(), 60);
        store.save(&routine).unwrap();

        store
            .set_working_dir(&routine.id, Some("  /work/spaced  "))
            .unwrap();
        assert_eq!(
            store.working_dir_of(&routine.id).unwrap().as_deref(),
            Some("/work/spaced")
        );
    }

    #[test]
    fn working_dir_clear_removes_the_file_idempotently() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledTaskStore::with_base(tmp.path().to_path_buf());
        let routine = ScheduledRoutine::new("clear".into(), "p".into(), 60);
        store.save(&routine).unwrap();
        store
            .set_working_dir(&routine.id, Some("/work/proj"))
            .unwrap();

        store.set_working_dir(&routine.id, None).unwrap();
        assert_eq!(store.working_dir_of(&routine.id).unwrap(), None);

        // Clearing again (and clearing when never set) stays Ok.
        store.set_working_dir(&routine.id, None).unwrap();
        assert_eq!(store.working_dir_of(&routine.id).unwrap(), None);
    }

    #[test]
    fn working_dir_empty_value_clears() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledTaskStore::with_base(tmp.path().to_path_buf());
        let routine = ScheduledRoutine::new("empty".into(), "p".into(), 60);
        store.save(&routine).unwrap();
        store
            .set_working_dir(&routine.id, Some("/work/proj"))
            .unwrap();

        store.set_working_dir(&routine.id, Some("   ")).unwrap();
        assert_eq!(store.working_dir_of(&routine.id).unwrap(), None);
    }

    #[test]
    fn working_dir_missing_task_is_a_not_found_error() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledTaskStore::with_base(tmp.path().to_path_buf());

        let err = store.working_dir_of("nonexistent").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        let err = store.set_working_dir("nonexistent", Some("/x")).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        let err = store.set_working_dir("nonexistent", None).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn working_dirs_dedupes_sorts_and_skips_blank() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledTaskStore::with_base(tmp.path().to_path_buf());

        let a = ScheduledRoutine::new("alpha".into(), "p".into(), 60);
        let b = ScheduledRoutine::new("beta".into(), "p".into(), 60);
        let c = ScheduledRoutine::new("gamma".into(), "p".into(), 60);
        let d = ScheduledRoutine::new("delta".into(), "p".into(), 60);
        for r in [&a, &b, &c, &d] {
            store.save(r).unwrap();
        }
        // Two tasks share a project; one has another; one has none; one
        // carries a blank line that must never surface.
        store.set_working_dir(&a.id, Some("/work/zeta")).unwrap();
        store.set_working_dir(&b.id, Some("/work/zeta/")).unwrap();
        store.set_working_dir(&c.id, Some("/work/alpha")).unwrap();
        store.set_working_dir(&d.id, Some("   ")).unwrap();

        assert_eq!(
            store.working_dirs(),
            ["/work/alpha", "/work/zeta", "/work/zeta/"],
            "sorted, deduplicated, blank entries skipped"
        );
    }

    #[test]
    fn working_dirs_empty_when_store_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledTaskStore::with_base(tmp.path().join("nope"));
        assert!(store.working_dirs().is_empty());
    }

    #[test]
    fn delete_also_drops_the_working_dir_sidecar() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledTaskStore::with_base(tmp.path().to_path_buf());
        let routine = ScheduledRoutine::new("gone".into(), "p".into(), 60);
        store.save(&routine).unwrap();
        store
            .set_working_dir(&routine.id, Some("/work/proj"))
            .unwrap();

        assert!(store.delete(&routine.id).unwrap());
        assert!(store.working_dirs().is_empty());
        let err = store.working_dir_of(&routine.id).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    // ── rename is non-orphaning (one directory per id) ──────────────────

    fn task_dirs(base: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(base)
            .unwrap()
            .flatten()
            .filter(|e| e.file_type().unwrap().is_dir())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn rename_migrates_dir_and_working_dir_sidecar() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledTaskStore::with_base(tmp.path().to_path_buf());
        let mut routine = ScheduledRoutine::new("Old Name".into(), "p".into(), 60);
        store.save(&routine).unwrap();
        store
            .set_working_dir(&routine.id, Some("/work/proj"))
            .unwrap();
        let old_dir = store.task_dir(&routine.id, "Old Name");
        assert!(old_dir.exists());

        routine.name = "New Name".into();
        let new_dir = store.save(&routine).unwrap();

        // Exactly one directory for the id — the NEW slug's.
        assert!(!old_dir.exists(), "orphaned old dir must be gone");
        assert!(new_dir.exists());
        assert_eq!(
            task_dirs(tmp.path()),
            [new_dir.file_name().unwrap().to_string_lossy().into_owned()],
        );
        // The sidecar traveled with the directory…
        assert_eq!(
            store.working_dir_of(&routine.id).unwrap().as_deref(),
            Some("/work/proj")
        );
        // …as did the routine itself.
        let loaded = store.load(&routine.id).unwrap().unwrap();
        assert_eq!(loaded.name, "New Name");
        // And the collector no longer reports anything stale — the path moved,
        // it didn't duplicate.
        assert_eq!(store.working_dirs(), ["/work/proj"]);
    }

    #[test]
    fn save_removes_legacy_same_id_orphan_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledTaskStore::with_base(tmp.path().to_path_buf());
        let routine = ScheduledRoutine::new("Current Name".into(), "p".into(), 60);
        store.save(&routine).unwrap();

        // Simulated legacy state: an orphan dir from a pre-migration rename,
        // carrying a stale sidecar. Target dir already exists.
        let orphan = tmp.path().join(format!("stale-slug-{}", routine.id));
        std::fs::create_dir_all(&orphan).unwrap();
        std::fs::write(orphan.join("working_dir"), "/stale/legacy\n").unwrap();

        store.save(&routine).unwrap();

        assert!(!orphan.exists(), "legacy orphan must be cleaned up");
        let dirs: Vec<String> = store.working_dirs();
        assert!(
            dirs.is_empty(),
            "stale sidecar must not feed adoption: {dirs:?}"
        );
        assert_eq!(
            task_dirs(tmp.path()).len(),
            1,
            "exactly one directory per id after save"
        );
    }

    #[test]
    fn resolve_is_deterministic_with_duplicate_id_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ScheduledTaskStore::with_base(tmp.path().to_path_buf());
        let id = "abc12345";
        // Legacy pair: two same-id dirs, each with a (different) sidecar.
        for (dir, slug) in [("/work/alpha", "alpha"), ("/work/beta", "beta")] {
            let path = tmp.path().join(format!("{slug}-{id}"));
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join("working_dir"), format!("{dir}\n")).unwrap();
        }

        // Sorted-name winner, stable across calls (no enumeration-order race).
        let first = store.working_dir_of(id).unwrap();
        let second = store.working_dir_of(id).unwrap();
        assert_eq!(first, second, "resolution must not flip between calls");
        assert_eq!(first.as_deref(), Some("/work/alpha"), "lexicographic winner");
        assert!(
            store.load(id).unwrap().is_none(),
            "sanity: synthetic dirs have no task.json; resolve itself was still exercised"
        );

        // The next save reconciles: the deterministically-first leftover is
        // promoted into the new slug's directory (its sidecar travels with
        // it), the second leftover is removed, and the save writes fresh
        // content files — one directory, deterministic winner.
        let mut routine = ScheduledRoutine::new("Beta Name".into(), "p".into(), 60);
        routine.id = id.into();
        store.save(&routine).unwrap();
        let dirs = task_dirs(tmp.path());
        assert_eq!(dirs, ["beta-name-abc12345"], "single deterministic dir");
        assert_eq!(
            store.working_dir_of(id).unwrap().as_deref(),
            Some("/work/alpha"),
            "promoted leftover keeps its sidecar; the beta leftover is gone"
        );
        assert_eq!(
            store.load(id).unwrap().map(|r| r.name).as_deref(),
            Some("Beta Name")
        );
    }
}
