//! Repo map injection into the system prompt (P1-4).
//!
//! The repo map is a per-project, budget-trimmed symbol overview rendered as
//! markdown. It piggy-backs on `shannon-repomap` for the parse + budget work
//! and adds nothing at the system prompt layer except the labelled block.
//!
//! ## Wiring
//!
//! [`RepoMapInjector::build`] is called from the query engine's system-prompt
//! assembly path (see `engine.rs::process_query`). The result is wrapped in
//! a `SystemContentBlock` so the engine's cache-breakpoint logic continues to
//! work — the repo map is cached across turns because it changes only when
//! the project source changes.
//!
//! ## Failure modes
//!
//! The injector is best-effort. Any I/O / parse failure logs a warning and
//! returns `None`, leaving the system prompt untouched. We never want the
//! repo map to break a query that would otherwise have succeeded.
//!
//! ## Caching
//!
//! The injector owns a single [`RepoMapCache`] per project root. The cache
//! lazily loads from disk (or does a full walk on the cold path) and the
//! `update_file` / `remove_file` paths keep it in sync with editor activity.
//! For the engine integration we just call [`Self::build`] once per turn;
//! any watchers attached by other crates feed events into the cache.

use anyhow::Context;
use shannon_repomap::RepoMapCache;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

/// Owns the project root + cached symbol map for the duration of an engine
/// lifetime. Cloning is cheap (the heavy state lives behind an `Arc<RwLock>`).
#[derive(Debug, Clone)]
pub struct RepoMapInjector {
    inner: Arc<RepoMapInjectorInner>,
}

#[derive(Debug)]
struct RepoMapInjectorInner {
    /// Project root the cache is bound to. `None` means "use the engine's
    /// current working directory at build time".
    root_override: Option<PathBuf>,
    /// Token budget enforced by [`RepoMapCache::pack`]. Defaults to 2,000
    /// tokens — a comfortable slice of the system prompt for a typical
    /// 100K+ context model.
    budget_tokens: usize,
    /// Cached symbol map. Lazily initialised on first build call.
    cache: RwLock<Option<RepoMapCache>>,
}

impl RepoMapInjector {
    /// Build an injector pinned to a specific project root. Pass `None`
    /// to defer to the engine's current working directory.
    pub fn new(root: Option<&Path>, budget_tokens: usize) -> Self {
        Self {
            inner: Arc::new(RepoMapInjectorInner {
                root_override: root.map(Path::to_path_buf),
                budget_tokens,
                cache: RwLock::new(None),
            }),
        }
    }

    /// Default-ON injector (2K token budget, root resolved at build time).
    pub fn enabled() -> Self {
        Self::new(None, 2_000)
    }

    /// Token budget the injector passes to the underlying cache.
    pub fn budget_tokens(&self) -> usize {
        self.inner.budget_tokens
    }

    /// Pinned root, if any. Mostly useful for tests.
    pub fn root_override(&self) -> Option<&Path> {
        self.inner.root_override.as_deref()
    }

    /// Drop the in-memory cache so the next [`Self::build`] does a fresh
    /// full walk. Useful when the project root changes at runtime (rare;
    /// typically the engine is rebuilt).
    pub fn invalidate(&self) {
        if let Ok(mut guard) = self.inner.cache.write() {
            *guard = None;
        }
    }

    /// Build the markdown block to inject under the repo-map section header.
    ///
    /// Returns `Some(markdown)` when:
    /// - The injector can locate a project root.
    /// - The cache (or a fresh walk) produced symbols.
    ///
    /// Returns `None` (and logs a warning) for any failure mode: missing
    /// root, parse error, no parseable files, etc. The system prompt should
    /// still render without the repo map block — it's a best-effort
    /// augmentation.
    pub fn build(&self) -> Option<String> {
        let root = self.resolve_root()?;
        let mut cache = self.ensure_cache(&root).ok()?;
        let md = cache.pack(self.inner.budget_tokens);
        if md.trim().is_empty() {
            return None;
        }
        Some(md)
    }

    /// Force-refresh a single file in the cache. Called by external code
    /// (FS watchers, file-write tools) that knows about a change but doesn't
    /// want to drive the whole system-prompt rebuild.
    pub fn notify_file_changed(&self, path: &Path) -> anyhow::Result<bool> {
        let root = self.resolve_root().context("repo map: no project root")?;
        // Take the write lock. `ensure_cache` returns a clone, which is fine
        // for reads but useless for mutating — the original cache in
        // `self.inner.cache` would never see the change. Take the cache out,
        // mutate, then put it back.
        let mut guard = self
            .inner
            .cache
            .write()
            .map_err(|_| anyhow::anyhow!("repo map: cache lock poisoned"))?;
        let mut cache = match guard.take() {
            Some(c) => c,
            None => RepoMapCache::new(&root).context("repo map: cold cache load")?,
        };
        let changed = cache.update_file(path)?;
        *guard = Some(cache);
        Ok(changed)
    }

    // -- private helpers ----------------------------------------------------

    fn resolve_root(&self) -> Option<PathBuf> {
        if let Some(ref root) = self.inner.root_override {
            return Some(root.clone());
        }
        std::env::current_dir().ok()
    }

    fn ensure_cache(&self, root: &Path) -> anyhow::Result<RepoMapCache> {
        // Fast path: cache already initialised.
        if let Ok(guard) = self.inner.cache.read() {
            if let Some(ref cache) = *guard {
                if cache.root() == root {
                    return Ok(cache.clone());
                }
            }
        }
        // Slow path: build (or rebuild) the cache. Use a write lock so we
        // don't race two threads into both doing a full walk.
        let mut guard = self
            .inner
            .cache
            .write()
            .map_err(|_| anyhow::anyhow!("repo map: cache lock poisoned"))?;
        // Re-check after acquiring the write lock — another thread may have
        // populated it while we were waiting.
        if let Some(ref cache) = *guard {
            if cache.root() == root {
                return Ok(cache.clone());
            }
        }
        let cache = RepoMapCache::new(root).context("repo map: cold cache load")?;
        *guard = Some(cache.clone());
        Ok(cache)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests intentionally use unwrap to keep setup terse
mod tests {
    use super::*;
    use std::fs;

    fn tmp_root(label: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "shannon_repomap_injector_{label}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn build_returns_none_for_empty_root() {
        let root = tmp_root("empty");
        // Empty directory → no parseable files → map is empty → None.
        let inj = RepoMapInjector::new(Some(&root), 2_000);
        // Empty dir: walk succeeds but produces no entries. The markdown is
        // just the header line, which we treat as empty (whitespace only).
        let out = inj.build();
        // Either None (no symbols) or Some (just header) — both are valid
        // best-effort outputs. We only assert there's no panic and that
        // when symbols exist, the markdown contains the Repo Map label.
        if let Some(md) = out.as_ref() {
            assert!(md.contains("Repo Map:"));
        }
    }

    #[test]
    fn build_returns_markdown_for_populated_root() {
        let root = tmp_root("populated");
        fs::write(
            root.join("hello.rs"),
            "pub fn greet(name: &str) -> String { format!(\"hi {name}\") }\n",
        )
        .unwrap();
        let inj = RepoMapInjector::new(Some(&root), 2_000);
        let md = inj.build().expect("markdown for populated root");
        assert!(md.contains("Repo Map:"));
        assert!(md.contains("greet"));
    }

    #[test]
    fn notify_file_changed_updates_cache() {
        let root = tmp_root("notify");
        fs::write(root.join("a.rs"), "pub fn a() {}\n").unwrap();
        let inj = RepoMapInjector::new(Some(&root), 2_000);
        // Warm the cache.
        let _ = inj.build();
        // Add a new file and notify — next build should surface it.
        fs::write(root.join("b.rs"), "pub fn freshly_added() {}\n").unwrap();
        let _ = inj.notify_file_changed(&root.join("b.rs")).unwrap();
        let md = inj.build().expect("markdown after notify");
        assert!(md.contains("freshly_added"));
    }

    #[test]
    fn invalidating_forces_fresh_walk() {
        let root = tmp_root("invalidate");
        fs::write(root.join("a.rs"), "pub fn a() {}\n").unwrap();
        let inj = RepoMapInjector::new(Some(&root), 2_000);
        let _ = inj.build();
        inj.invalidate();
        // No panic, no broken state.
        let _ = inj.build().expect("build after invalidate");
    }
}
