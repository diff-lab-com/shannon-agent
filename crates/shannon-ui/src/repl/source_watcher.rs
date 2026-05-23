//! Source file watcher for detecting project file changes.
//!
//! Uses the `notify` crate to watch source code files in the project directory.
//! When changes are detected, the dirty flag is set so the REPL main loop can
//! react (e.g., show notification, trigger diagnostics refresh).

/// File extensions considered source code worth watching.
const SOURCE_EXTENSIONS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "kt", "swift", "c", "cpp", "h", "hpp",
    "cs", "rb", "sh", "toml", "yaml", "yml", "json", "html", "css", "scss", "md",
];

/// Directories to skip when watching.
#[cfg(test)]
const SKIP_DIRS: &[&str] = &[
    "target",
    "node_modules",
    ".git",
    "dist",
    "build",
    "__pycache__",
    ".next",
    ".nuxt",
    "vendor",
    ".venv",
    "venv",
];

/// Watches project source files for changes using filesystem events.
pub(crate) struct SourceWatcher {
    #[allow(dead_code)]
    watcher: Option<notify::RecommendedWatcher>,
    dirty: std::sync::Arc<std::sync::atomic::AtomicBool>,
    changed_files: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    #[allow(dead_code)]
    project_dir: std::path::PathBuf,
}

impl SourceWatcher {
    pub(super) fn new(project_dir: std::path::PathBuf) -> Self {
        let dirty = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let changed_files: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

        let dirty_flag = dirty.clone();
        let files_list = changed_files.clone();

        let handler = move |event: notify::Result<notify::Event>| {
            if let Ok(event) = event {
                use notify::EventKind;
                if matches!(
                    event.kind,
                    EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
                ) {
                    for path in &event.paths {
                        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                            if SOURCE_EXTENSIONS.contains(&ext) {
                                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                                    // Skip common generated/temp files
                                    if name.ends_with(".lock")
                                        || name.starts_with('.')
                                        || name.contains('#')
                                    {
                                        continue;
                                    }
                                }
                                if let Ok(mut files) = files_list.lock() {
                                    let path_str = path.to_string_lossy().to_string();
                                    if !files.contains(&path_str) {
                                        files.push(path_str);
                                    }
                                    // Cap the list to avoid unbounded growth
                                    if files.len() > 100 {
                                        files.drain(0..50);
                                    }
                                }
                            }
                        }
                    }
                    dirty_flag.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
        };

        let watcher = match notify::recommended_watcher(handler) {
            Ok(mut w) => {
                use notify::Watcher;
                if project_dir.exists() {
                    let _ = w.watch(&project_dir, notify::RecursiveMode::Recursive);
                    tracing::debug!("SourceWatcher watching {}", project_dir.display());
                    Some(w)
                } else {
                    None
                }
            }
            Err(e) => {
                tracing::debug!("SourceWatcher unavailable: {e}");
                None
            }
        };

        Self {
            watcher,
            dirty,
            changed_files,
            project_dir,
        }
    }

    /// Check if source files changed since last check. Returns list of changed file paths.
    pub(crate) fn check_changes(&self) -> Vec<String> {
        if !self.dirty.swap(false, std::sync::atomic::Ordering::Relaxed) {
            return Vec::new();
        }

        if let Ok(mut files) = self.changed_files.lock() {
            std::mem::take(&mut *files)
        } else {
            Vec::new()
        }
    }

    /// Returns the project directory being watched.
    #[allow(dead_code)]
    pub(crate) fn project_dir(&self) -> &std::path::Path {
        &self.project_dir
    }
}

/// Check if a path should be skipped during source watching.
#[cfg(test)]
pub(crate) fn should_skip_path(path: &std::path::Path) -> bool {
    for component in path.components() {
        if let std::path::Component::Normal(os_str) = component {
            if let Some(name) = os_str.to_str() {
                if SKIP_DIRS.contains(&name) {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_skip_target() {
        assert!(should_skip_path(std::path::Path::new(
            "/project/target/debug/foo.rs"
        )));
    }

    #[test]
    fn test_should_skip_node_modules() {
        assert!(should_skip_path(std::path::Path::new(
            "/project/node_modules/react/index.js"
        )));
    }

    #[test]
    fn test_should_skip_git() {
        assert!(should_skip_path(std::path::Path::new("/project/.git/HEAD")));
    }

    #[test]
    fn test_should_not_skip_src() {
        assert!(!should_skip_path(std::path::Path::new(
            "/project/src/main.rs"
        )));
    }

    #[test]
    fn test_should_not_skip_nested() {
        assert!(!should_skip_path(std::path::Path::new(
            "/project/crates/foo/src/lib.rs"
        )));
    }

    #[test]
    fn test_source_extensions_include_rust() {
        assert!(SOURCE_EXTENSIONS.contains(&"rs"));
    }

    #[test]
    fn test_source_extensions_include_typescript() {
        assert!(SOURCE_EXTENSIONS.contains(&"ts"));
        assert!(SOURCE_EXTENSIONS.contains(&"tsx"));
    }

    #[test]
    fn test_source_extensions_include_python() {
        assert!(SOURCE_EXTENSIONS.contains(&"py"));
    }

    #[test]
    fn test_skip_dirs_common() {
        assert!(SKIP_DIRS.contains(&"target"));
        assert!(SKIP_DIRS.contains(&"node_modules"));
        assert!(SKIP_DIRS.contains(&".git"));
        assert!(SKIP_DIRS.contains(&"build"));
    }

    #[test]
    fn test_check_changes_empty_when_not_dirty() {
        let dir = tempfile::tempdir().unwrap();
        let watcher = SourceWatcher::new(dir.path().to_path_buf());
        let changes = watcher.check_changes();
        assert!(changes.is_empty());
    }

    #[test]
    fn test_project_dir_returns_input() {
        let dir = tempfile::tempdir().unwrap();
        let watcher = SourceWatcher::new(dir.path().to_path_buf());
        assert_eq!(watcher.project_dir(), dir.path());
    }
}
