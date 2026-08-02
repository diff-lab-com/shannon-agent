//! Config file watcher — fires `HookEvent::ConfigChange` when `.shannon.toml`
//! (and friends) are modified on disk.
//!
//! This is the emit site for the `ConfigChange` hook event (P1-2c). The
//! watcher is intentionally lightweight and lives entirely in
//! `shannon-core` so other crates (CLI, REPL) can opt in by calling
//! [`ConfigWatcher::start`] and routing the resulting events through their
//! existing `HookManager`.
//!
//! Design choices:
//!  - One `notify::RecommendedWatcher` per process; multiple call sites share
//!    the underlying OS handle but each gets its own callback closure.
//!  - Coarse-grained path matching: we watch the file's *parent* directory
//!    (not the file itself) because editors often perform atomic-replace
//!    (rename / unlink + create), which breaks per-file watches on some
//!    platforms.
//!  - The callback fires `HookEvent::ConfigChange { config_path, change_type }`
//!    for every Create / Modify / Remove that lands on the watched path.
//!    Dedup and policy are left to the caller's `HookManager`.
//!
//! Limitations:
//!  - We do not reload the config. Callers can read the changed path after
//!    receiving the event and re-run their own load logic.
//!  - Debouncing is the responsibility of the caller; notify can fire multiple
//!    events for a single editor save.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{debug, warn};

/// Lightweight guard that owns a `notify::RecommendedWatcher`. Drop the
/// guard to stop watching. The watcher is held inside an `Arc` so cloning is
/// cheap and the underlying OS handle is shared across threads.
pub struct ConfigWatcher {
    /// Keep the watcher alive for the lifetime of this guard.
    _watcher: Arc<notify::RecommendedWatcher>,
    /// The path being watched (cached for diagnostics / dedup tests).
    watched_path: PathBuf,
    /// The parent directory actually handed to `notify` (may differ from
    /// `watched_path` when the file does not exist yet).
    watched_dir: PathBuf,
}

impl std::fmt::Debug for ConfigWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfigWatcher")
            .field("watched_path", &self.watched_path)
            .field("watched_dir", &self.watched_dir)
            .finish()
    }
}

impl ConfigWatcher {
    /// Start watching the config file at `path`.
    ///
    /// `on_change` is invoked once per filesystem event that touches `path`
    /// (create / modify / remove). Errors from `notify` are logged and the
    /// callback is silently skipped — the engine must keep working even when
    /// the filesystem watcher is unavailable (e.g. inside a sandbox).
    ///
    /// Returns `None` when the file's parent directory does not exist (so
    /// there is nothing to watch yet) or when `notify` itself fails to start.
    pub fn start<F>(path: impl Into<PathBuf>, mut on_change: F) -> Option<Self>
    where
        F: FnMut(ConfigChange) + Send + 'static,
    {
        let watched_path = path.into();

        // We always watch the parent directory because editors commonly do
        // atomic-replace, which surfaces as Remove(Create) of the target file.
        let watched_dir: PathBuf = watched_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));

        if !watched_dir.exists() {
            debug!(
                "ConfigWatcher: parent dir {} does not exist yet, skipping",
                watched_dir.display()
            );
            return None;
        }

        let target_name = watched_path
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_default();

        let handler = move |event: notify::Result<notify::Event>| {
            let event = match event {
                Ok(ev) => ev,
                Err(e) => {
                    warn!("ConfigWatcher: notify error: {e}");
                    return;
                }
            };

            // Filter to events that actually touch our target path.
            use notify::EventKind;
            let change_type = match event.kind {
                EventKind::Create(_) => Some("created"),
                EventKind::Modify(_) => Some("modified"),
                EventKind::Remove(_) => Some("removed"),
                _ => None,
            };
            let Some(change_type) = change_type else {
                return;
            };

            for path in &event.paths {
                if path.file_name() == Some(&target_name) {
                    let change = ConfigChange {
                        config_path: path.to_string_lossy().into_owned(),
                        change_type: change_type.to_string(),
                    };
                    debug!("ConfigWatcher: firing event {change:?}");
                    on_change(change);
                    return;
                }
            }
        };

        let mut watcher = match notify::recommended_watcher(handler) {
            Ok(w) => w,
            Err(e) => {
                warn!("ConfigWatcher: failed to construct watcher: {e}");
                return None;
            }
        };

        use notify::Watcher;
        if let Err(e) = watcher.watch(&watched_dir, notify::RecursiveMode::NonRecursive) {
            warn!(
                "ConfigWatcher: failed to watch {}: {e}",
                watched_dir.display()
            );
            return None;
        }

        debug!(
            "ConfigWatcher: watching {} (via {})",
            watched_path.display(),
            watched_dir.display()
        );

        let watcher = Arc::new(watcher);
        Some(Self {
            _watcher: watcher,
            watched_path,
            watched_dir,
        })
    }

    /// Path the watcher is monitoring.
    pub fn path(&self) -> &Path {
        &self.watched_path
    }

    /// Parent directory the watcher is actually subscribed to.
    pub fn dir(&self) -> &Path {
        &self.watched_dir
    }
}

/// Payload of a single config-change detection. Same field names as
/// `HookEvent::ConfigChange` so callers can convert trivially.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigChange {
    /// Path to the changed file (as observed on disk).
    pub config_path: String,
    /// One of `"created"`, `"modified"`, or `"removed"`.
    pub change_type: String,
}

impl ConfigChange {
    /// Convert this detection into the matching `HookEvent::ConfigChange`.
    pub fn into_hook_event(self) -> shannon_engine::hooks::HookEvent {
        shannon_engine::hooks::HookEvent::ConfigChange {
            config_path: self.config_path,
            change_type: self.change_type,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_into_hook_event_round_trip() {
        let change = ConfigChange {
            config_path: ".shannon.toml".to_string(),
            change_type: "modified".to_string(),
        };
        let event = change.into_hook_event();
        match event {
            shannon_engine::hooks::HookEvent::ConfigChange {
                config_path,
                change_type,
            } => {
                assert_eq!(config_path, ".shannon.toml");
                assert_eq!(change_type, "modified");
            }
            other => panic!("expected ConfigChange, got {other:?}"),
        }
    }

    #[test]
    fn test_watch_nonexistent_parent_returns_none() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();
        let result = ConfigWatcher::start(
            "/no/such/dir/.shannon.toml",
            move |_change| {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            },
        );
        assert!(result.is_none(), "should not start when parent dir is missing");
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_path_and_dir_accessors() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join(".shannon.toml");
        std::fs::write(&target, "# initial").unwrap();
        let watcher = ConfigWatcher::start(&target, |_change| {}).expect("watcher should start");
        assert_eq!(watcher.path(), target);
        assert_eq!(watcher.dir(), dir.path());
    }

    /// Real-filesystem integration test: writes to `.shannon.toml` are
    /// observed by the watcher. Skipped when `notify` cannot attach (e.g.
    /// in some sandboxed CI runners) so the suite stays deterministic.
    #[test]
    fn test_watcher_fires_on_modify() {
        let dir = match tempfile::tempdir() {
            Ok(d) => d,
            Err(_) => return,
        };
        let target = dir.path().join(".shannon.toml");
        std::fs::write(&target, "# v1").unwrap();

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();
        let last_change: Arc<std::sync::Mutex<Option<ConfigChange>>> =
            Arc::new(std::sync::Mutex::new(None));
        let last_clone = last_change.clone();

        let watcher = match ConfigWatcher::start(target.clone(), move |change| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
            *last_clone.lock().unwrap() = Some(change);
        }) {
            Some(w) => w,
            None => {
                eprintln!("ConfigWatcher: notify unavailable, skipping modify test");
                return;
            }
        };

        // Give notify a tick to settle.
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Modify the file and wait for the OS event to land.
        std::fs::write(&target, "# v2").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));

        let hits = counter.load(Ordering::SeqCst);
        assert!(hits >= 1, "watcher should have fired at least once, got {hits}");
        let last = last_change.lock().unwrap().clone().expect("change recorded");
        assert!(
            last.change_type == "modified" || last.change_type == "created",
            "expected modified/created, got {:?}",
            last.change_type
        );
        assert_eq!(last.config_path, target.to_string_lossy().to_string());

        // Drop the watcher and ensure further writes do not fire.
        drop(watcher);
        let counter_before_idle = counter.load(Ordering::SeqCst);
        std::fs::write(&target, "# v3").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert_eq!(
            counter.load(Ordering::SeqCst),
            counter_before_idle,
            "watcher should be inert after drop"
        );
    }
}