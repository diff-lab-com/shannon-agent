//! Claude Code-style storage for scheduled tasks.
//!
//! Each task is stored as a directory containing:
//! - `SKILL.md`: the prompt content (markdown, human-editable)
//! - `task.json`: the [`ScheduledRoutine`] metadata (machine-managed)
//!
//! ## Layout
//! ```text
//! ~/.shannon/scheduled-tasks/
//! ├── <task-slug>-<id>/
//! │   ├── SKILL.md
//! │   └── task.json
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
    /// existing files with the same slug.
    pub fn save(&self, routine: &ScheduledRoutine) -> Result<PathBuf, TaskStoreError> {
        fs::create_dir_all(&self.base_dir)?;
        let task_dir = self.task_dir(&routine.id, &routine.name);
        fs::create_dir_all(&task_dir)?;

        fs::write(task_dir.join("SKILL.md"), &routine.prompt)?;
        let json = serde_json::to_string_pretty(routine)?;
        fs::write(task_dir.join("task.json"), json)?;

        Ok(task_dir)
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
    fn resolve_task_dir(&self, id_or_name: &str) -> Option<PathBuf> {
        if !self.base_dir.exists() {
            return None;
        }
        let entries = fs::read_dir(&self.base_dir).ok()?;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == id_or_name
                || name.ends_with(&format!("-{id_or_name}"))
                || name.starts_with(&format!("{id_or_name}-"))
            {
                return Some(entry.path());
            }
        }
        None
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
}
