//! Filesystem watcher for incremental repo-map updates.
//!
//! Wraps `notify::RecommendedWatcher` so callers can attach a callback that
//! receives one event per file change. Events are filtered to the
//! file extensions we actually parse (everything else is dropped silently).
//!
//! The watcher does *not* mutate the cache directly — it just forwards
//! paths. Callers feed those paths into [`crate::RepoMapCache::update_file`]
//! on whatever thread they prefer. This keeps the watcher side-effect-free
//! and easy to test (call the callback by hand; skip the OS round-trip).
//!
//! Lifetime: drop the watcher to stop watching. The internal `notify` handle
//! is held by an `Arc` so cloning is cheap; we expose only the guard type.

use anyhow::{Context, Result};
use notify::Watcher as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::warn;

use crate::parser::LanguageParser;

/// Lightweight guard that owns a `notify::RecommendedWatcher`. Drop it to
/// stop watching. The watcher is held inside an `Arc` so cloning the guard
/// is cheap and the underlying OS handle is shared.
pub struct RepoMapWatcher {
    _watcher: Arc<notify::RecommendedWatcher>,
    /// The root directory handed to `notify`. Cached for diagnostics.
    root: PathBuf,
}

impl std::fmt::Debug for RepoMapWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RepoMapWatcher")
            .field("root", &self.root)
            .finish()
    }
}

impl RepoMapWatcher {
    /// Start watching `root` recursively.
    ///
    /// `on_change` is invoked once per filesystem event that lands on a
    /// file with a supported extension. Errors from `notify` are logged and
    /// the callback is silently skipped — the engine must keep working even
    /// when the filesystem watcher is unavailable (e.g. inside a sandbox).
    pub fn start<F>(root: &Path, on_change: F) -> Result<Self>
    where
        F: Fn(WatcherEvent) + Send + Sync + 'static,
    {
        let root_buf = root.to_path_buf();
        let callback: Arc<dyn Fn(WatcherEvent) + Send + Sync> = Arc::new(on_change);

        let handler = move |res: notify::Result<notify::Event>| match res {
            Ok(event) => handle_event(event, &callback),
            Err(err) => warn!("repo map watcher: notify error: {err}"),
        };

        let mut watcher = notify::RecommendedWatcher::new(handler, notify::Config::default())
            .context("create notify watcher")?;
        // Best-effort watch: a missing root shouldn't kill the watcher
        // construction. The caller can decide what to do with the guard.
        if root.exists() {
            watcher
                .watch(&root_buf, notify::RecursiveMode::Recursive)
                .with_context(|| format!("watch root {}", root_buf.display()))?;
        } else {
            warn!(
                "repo map watcher: root {} does not exist; skipping watch",
                root_buf.display()
            );
        }
        Ok(Self {
            _watcher: Arc::new(watcher),
            root: root_buf,
        })
    }
}

/// Decoded filesystem event the watcher forwards to the cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatcherEvent {
    /// Absolute path of the file that changed.
    pub path: PathBuf,
    /// What kind of change occurred.
    pub kind: WatcherEventKind,
}

/// Coarse classification of a filesystem event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatcherEventKind {
    /// File was created or had its content modified.
    Modified,
    /// File was removed.
    Removed,
}

fn handle_event(event: notify::Event, callback: &Arc<dyn Fn(WatcherEvent) + Send + Sync>) {
    use notify::EventKind;
    use notify::event::{ModifyKind, RemoveKind, RenameMode};

    let kind = match event.kind {
        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => WatcherEventKind::Modified,
        EventKind::Modify(_) => WatcherEventKind::Modified,
        EventKind::Create(_) => WatcherEventKind::Modified,
        EventKind::Remove(RemoveKind::File) | EventKind::Remove(RemoveKind::Any) => {
            WatcherEventKind::Removed
        }
        EventKind::Remove(_) => WatcherEventKind::Removed,
        _ => return,
    };

    for path in event.paths {
        if !is_supported_file(&path) {
            continue;
        }
        callback(WatcherEvent { path, kind });
    }
}

/// Return `true` when `path` has an extension we know how to parse.
fn is_supported_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .and_then(|ext| LanguageParser::from_extension(ext).ok())
        .is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_extensions_are_accepted() {
        assert!(is_supported_file(Path::new("/tmp/foo.rs")));
        assert!(is_supported_file(Path::new("/tmp/foo.ts")));
        assert!(is_supported_file(Path::new("/tmp/foo.py")));
        assert!(is_supported_file(Path::new("/tmp/foo.go")));
    }

    #[test]
    fn unsupported_extensions_are_rejected() {
        assert!(!is_supported_file(Path::new("/tmp/foo.txt")));
        assert!(!is_supported_file(Path::new("/tmp/foo.md")));
        assert!(!is_supported_file(Path::new("/tmp/foo")));
    }
}
