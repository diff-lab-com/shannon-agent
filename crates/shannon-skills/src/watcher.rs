//! Mtime-based skill hot-reload watcher
//!
//! Detects changes to SKILL.md files by comparing modification times on each
//! check. Follows the same pattern as `InstructionWatcher` in
//! `shannon-core::project_instructions`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tracing::{debug, trace, warn};
use walkdir::WalkDir;

use crate::loader::load_skill_from_file;
use crate::registry::SkillRegistry;

/// Watches skill directories for SKILL.md file changes using mtime comparison.
///
/// Call [`SkillWatcher::check_and_reload`] periodically (e.g. before each
/// query) to pick up new, changed, or removed skill files.
#[derive(Debug)]
pub struct SkillWatcher {
    /// Directories that may contain skill subdirectories with SKILL.md files.
    watch_dirs: Vec<PathBuf>,
    /// Map of SKILL.md path → last known modification time.
    mtimes: HashMap<PathBuf, SystemTime>,
}

impl SkillWatcher {
    /// Create a new watcher for the given skill directories.
    ///
    /// Performs an initial scan so that the first call to
    /// [`check_and_reload`] will not report files that already existed.
    pub fn new(watch_dirs: Vec<PathBuf>) -> Self {
        let mut watcher = Self {
            watch_dirs,
            mtimes: HashMap::new(),
        };
        // Initial scan — seed mtimes so the first check_and_reload does not
        // report files that already existed at construction time.
        watcher.mtimes = watcher.scan_mtimes();
        watcher
    }

    /// Scan all watched directories and return the current set of
    /// `(SKILL.md path, mtime)` pairs.
    fn scan_mtimes(&mut self) -> HashMap<PathBuf, SystemTime> {
        let mut current = HashMap::new();

        for dir in &self.watch_dirs {
            if !dir.is_dir() {
                continue;
            }

            for entry in WalkDir::new(dir)
                .min_depth(1)
                .max_depth(2)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let path = entry.path();
                if path.file_name().is_some_and(|n| n == "SKILL.md") {
                    if let Ok(meta) = std::fs::metadata(path) {
                        if let Ok(mtime) = meta.modified() {
                            current.insert(path.to_path_buf(), mtime);
                        }
                    }
                }
            }
        }

        current
    }

    /// Check watched directories for changes and reload affected skills.
    ///
    /// Returns `Some(changed_skill_names)` when at least one skill was added,
    /// modified, or removed.  Returns `None` when nothing changed.
    pub fn check_and_reload(&mut self, registry: &SkillRegistry) -> Option<Vec<String>> {
        let new_mtimes = self.scan_mtimes();

        // Quick check: are the maps identical?
        let changed = new_mtimes.len() != self.mtimes.len()
            || new_mtimes.iter().any(|(path, mtime)| {
                self.mtimes.get(path) != Some(mtime)
            });

        if !changed {
            trace!("SkillWatcher: no changes detected");
            return None;
        }

        let mut changed_names = Vec::new();

        // --- Detect removed files (present before, gone now) ---
        for old_path in self.mtimes.keys() {
            if !new_mtimes.contains_key(old_path) {
                if let Some(name) = skill_name_from_path(old_path) {
                    debug!("SkillWatcher: skill file removed — {name} ({old_path:?})");
                    // Best-effort removal by id (directory name) and by name
                    let _ = registry.remove(&name);
                    // Also try to look it up and remove by its display name
                    if let Ok(skill) = registry.get_by_name(&name) {
                        let _ = registry.remove(&skill.id);
                    }
                    changed_names.push(name);
                }
            }
        }

        // --- Detect new and modified files ---
        for (path, mtime) in &new_mtimes {
            let is_new = !self.mtimes.contains_key(path);
            let is_modified = !is_new && self.mtimes.get(path) != Some(mtime);

            if is_new || is_modified {
                if is_new {
                    debug!("SkillWatcher: new skill file discovered — {path:?}");
                } else {
                    debug!("SkillWatcher: skill file modified — {path:?}");
                }

                match load_skill_from_file(path) {
                    Ok(skill) => {
                        let name = skill.name.clone();
                        let id = skill.id.clone();

                        // Remove old entry if it exists (handles name → id remap)
                        let _ = registry.remove(&id);
                        if let Ok(old) = registry.get_by_name(&name) {
                            let _ = registry.remove(&old.id);
                        }

                        match registry.register(skill) {
                            Ok(()) => {
                                debug!("SkillWatcher: reloaded skill \"{name}\" ({id})");
                                changed_names.push(name);
                            }
                            Err(e) => {
                                warn!("SkillWatcher: failed to register skill from {path:?}: {e}");
                            }
                        }
                    }
                    Err(e) => {
                        warn!("SkillWatcher: failed to load skill from {path:?}: {e}");
                    }
                }
            }
        }

        // Swap in the new mtimes regardless of individual errors so we don't
        // keep retrying broken files every check.
        self.mtimes = new_mtimes;

        if changed_names.is_empty() {
            None
        } else {
            Some(changed_names)
        }
    }

    /// Return the current set of watched file paths and their mtimes (for
    /// diagnostics / testing).
    pub fn watched_files(&self) -> &HashMap<PathBuf, SystemTime> {
        &self.mtimes
    }
}

/// Derive a skill identifier from a SKILL.md path by using the parent directory name.
fn skill_name_from_path(path: &Path) -> Option<String> {
    path.parent()
        .and_then(|dir| dir.file_name())
        .and_then(|name| name.to_str())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Helper: create a temp skill directory with a SKILL.md inside.
    fn create_skill_dir(parent: &Path, skill_name: &str, content: &str) -> PathBuf {
        let dir = parent.join(skill_name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), content).unwrap();
        dir
    }

    fn skill_content(name: &str) -> String {
        format!(
            "---\nname: {name}\ndescription: Test skill {name}\n---\n\n# {name}\n\nBody text.\n"
        )
    }

    #[test]
    fn test_no_changes_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        create_skill_dir(tmp.path(), "alpha", &skill_content("alpha"));

        let registry = SkillRegistry::new();
        // Load initial skill
        let skill_path = tmp.path().join("alpha").join("SKILL.md");
        registry.load_from_file(&skill_path).unwrap();

        let mut watcher = SkillWatcher::new(vec![tmp.path().to_path_buf()]);
        // Initial scan captured the file; a second check should be clean.
        let result = watcher.check_and_reload(&registry);
        assert!(result.is_none());
    }

    #[test]
    fn test_detects_new_skill() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = SkillRegistry::new();

        let mut watcher = SkillWatcher::new(vec![tmp.path().to_path_buf()]);
        // Empty dir — nothing to reload.
        assert!(watcher.check_and_reload(&registry).is_none());

        // Add a new skill file.
        create_skill_dir(tmp.path(), "bravo", &skill_content("bravo"));

        let result = watcher.check_and_reload(&registry);
        assert!(result.is_some());
        let names = result.unwrap();
        assert!(names.contains(&"bravo".to_string()));

        // Registry should now contain the skill.
        assert!(registry.get_by_name("bravo").is_ok());
    }

    #[test]
    fn test_detects_modified_skill() {
        let tmp = tempfile::tempdir().unwrap();
        create_skill_dir(tmp.path(), "charlie", &skill_content("charlie"));

        let registry = SkillRegistry::new();
        let skill_path = tmp.path().join("charlie").join("SKILL.md");
        registry.load_from_file(&skill_path).unwrap();

        let mut watcher = SkillWatcher::new(vec![tmp.path().to_path_buf()]);
        assert!(watcher.check_and_reload(&registry).is_none());

        // Modify the file.  On filesystems with coarse mtime granularity we
        // force a distinct mtime via `file::set_times` (stable Rust) or fall
        // back to a brief sleep.
        fs::write(&skill_path, skill_content("charlie-v2")).unwrap();

        // Try to advance the mtime by at least 1 ns.  std::fs doesn't expose
        // utimensat directly, so we use a small sleep as a portable fallback.
        let original_mtime = fs::metadata(&skill_path)
            .and_then(|m| m.modified())
            .ok();
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            // Re-write to bump mtime
            fs::write(&skill_path, skill_content("charlie-v2")).unwrap();
            if let Ok(cur) = fs::metadata(&skill_path).and_then(|m| m.modified()) {
                if original_mtime.is_none_or(|orig| cur > orig) {
                    break;
                }
            }
        }

        let result = watcher.check_and_reload(&registry);
        assert!(result.is_some());
        let names = result.unwrap();
        assert!(names.contains(&"charlie-v2".to_string()));
    }

    #[test]
    fn test_detects_removed_skill() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = create_skill_dir(tmp.path(), "delta", &skill_content("delta"));

        let registry = SkillRegistry::new();
        let skill_path = skill_dir.join("SKILL.md");
        registry.load_from_file(&skill_path).unwrap();
        assert!(registry.get_by_name("delta").is_ok());

        let mut watcher = SkillWatcher::new(vec![tmp.path().to_path_buf()]);
        assert!(watcher.check_and_reload(&registry).is_none());

        // Delete the skill file.
        fs::remove_dir_all(&skill_dir).unwrap();

        let result = watcher.check_and_reload(&registry);
        assert!(result.is_some());
        let names = result.unwrap();
        assert!(names.contains(&"delta".to_string()));

        // Skill should be gone from registry.
        assert!(registry.get_by_name("delta").is_err());
    }

    #[test]
    fn test_multiple_dirs() {
        let tmp1 = tempfile::tempdir().unwrap();
        let tmp2 = tempfile::tempdir().unwrap();

        create_skill_dir(tmp1.path(), "echo", &skill_content("echo"));
        create_skill_dir(tmp2.path(), "foxtrot", &skill_content("foxtrot"));

        let registry = SkillRegistry::new();
        let mut watcher = SkillWatcher::new(vec![
            tmp1.path().to_path_buf(),
            tmp2.path().to_path_buf(),
        ]);

        // First check after initial scan — files already seen during new().
        assert!(watcher.check_and_reload(&registry).is_none());

        // Add a new skill in the second directory.
        create_skill_dir(tmp2.path(), "golf", &skill_content("golf"));

        let result = watcher.check_and_reload(&registry);
        assert!(result.is_some());
        let names = result.unwrap();
        assert!(names.contains(&"golf".to_string()));
        assert!(registry.get_by_name("golf").is_ok());
    }

    #[test]
    fn test_watched_files_reflects_state() {
        let tmp = tempfile::tempdir().unwrap();
        create_skill_dir(tmp.path(), "hotel", &skill_content("hotel"));

        let registry = SkillRegistry::new();
        let mut watcher = SkillWatcher::new(vec![tmp.path().to_path_buf()]);

        // After initial scan, watched_files should contain the SKILL.md path.
        assert_eq!(watcher.watched_files().len(), 1);
        let path = tmp.path().join("hotel").join("SKILL.md");
        assert!(watcher.watched_files().contains_key(&path));

        // Remove the skill dir.
        fs::remove_dir_all(tmp.path().join("hotel")).unwrap();
        let _ = watcher.check_and_reload(&registry);

        assert_eq!(watcher.watched_files().len(), 0);
    }
}
