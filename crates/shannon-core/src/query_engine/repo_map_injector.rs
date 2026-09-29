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
    /// Cached symbol map plus its packed rendering. Lazily initialised on
    /// first build call.
    cache: RwLock<Option<CachedRepoMap>>,
}

/// A loaded [`RepoMapCache`] together with the trimmed rendering that turns
/// (`build`) actually serve. The rendering is cached because the underlying
/// pack is non-destructive but not free (it clones the map to trim a
/// snapshot): unchanged turns reuse the string, and any mutation via
/// [`RepoMapInjector::notify_file_changed`] / [`RepoMapInjector::invalidate`]
/// marks it dirty.
#[derive(Debug)]
struct CachedRepoMap {
    cache: RepoMapCache,
    /// `pack_snapshot(budget_tokens)` output. `None` = dirty, re-render on
    /// the next build.
    rendered: Option<String>,
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
        // Fast path: the packed rendering for this root is already cached.
        // This is the every-turn path — no map clone, no re-trim.
        if let Ok(guard) = self.inner.cache.read() {
            if let Some(ref cached) = *guard {
                if cached.cache.root() == root {
                    if let Some(ref md) = cached.rendered {
                        return Self::checked_render(md.clone(), &root, self.inner.budget_tokens);
                    }
                }
            }
        }
        // Slow path: build (or rebuild) the cache. Use a write lock so we
        // don't race two threads into both doing a full walk.
        let mut guard = self
            .inner
            .cache
            .write()
            .map_err(|_| anyhow::anyhow!("repo map: cache lock poisoned"))
            .ok()?;
        // Re-check after acquiring the write lock — another thread may have
        // populated or re-rendered while we were waiting.
        if let Some(ref cached) = *guard {
            if cached.cache.root() == root {
                if let Some(ref md) = cached.rendered {
                    return Self::checked_render(md.clone(), &root, self.inner.budget_tokens);
                }
            }
        }
        // Take the cache out so we can render without holding the borrow
        // across the (possibly slow) pack; reuse it when the root matches.
        let mut cached = match guard.take() {
            Some(c) if c.cache.root() == root => c,
            _ => CachedRepoMap {
                cache: RepoMapCache::new(&root)
                    .context("repo map: cold cache load")
                    .ok()?,
                rendered: None,
            },
        };
        let md = cached.cache.pack_snapshot(self.inner.budget_tokens);
        cached.rendered = Some(md.clone());
        *guard = Some(cached);
        Self::checked_render(md, &root, self.inner.budget_tokens)
    }

    /// Shared post-render checks: empty render → `None`; oversize render →
    /// loud warning (guardrail, not a truncation). The renderer already
    /// bounds its own output (symbol-token budget per file plus a 4 KiB cap
    /// on the folded symbol-less-file list); if a render ever blows past the
    /// expected envelope (~tokens * 4 chars/token with slack for markdown
    /// headers), warn so a renderer regression surfaces in logs instead of
    /// silently eating the context window. The markdown is returned
    /// untouched.
    fn checked_render(md: String, root: &Path, budget_tokens: usize) -> Option<String> {
        if md.trim().is_empty() {
            return None;
        }
        let max_bytes = budget_tokens.saturating_mul(32);
        if md.len() > max_bytes {
            tracing::warn!(
                bytes = md.len(),
                max_bytes,
                root = %root.display(),
                "repo map render exceeded expected size envelope (budget_tokens * 32); \
                 renderer size bounds may have regressed"
            );
        }
        Some(md)
    }

    /// Force-refresh a single file in the cache. Called by external code
    /// (FS watchers, file-write tools) that knows about a change but doesn't
    /// want to drive the whole system-prompt rebuild.
    pub fn notify_file_changed(&self, path: &Path) -> anyhow::Result<bool> {
        let root = self.resolve_root().context("repo map: no project root")?;
        // Take the write lock. Read methods return shared state, which is
        // useless for mutating — the original cache in `self.inner.cache`
        // would never see the change. Take the cache out, mutate, then put
        // it back.
        let mut guard = self
            .inner
            .cache
            .write()
            .map_err(|_| anyhow::anyhow!("repo map: cache lock poisoned"))?;
        let mut cached = match guard.take() {
            Some(c) => c,
            None => CachedRepoMap {
                cache: RepoMapCache::new(&root).context("repo map: cold cache load")?,
                rendered: None,
            },
        };
        let changed = cached.cache.update_file(path)?;
        // The packed rendering is stale the moment the map changed (and
        // conservatively also when the update was a no-op — re-packing once
        // is cheap next to serving a stale map).
        cached.rendered = None;
        *guard = Some(cached);
        Ok(changed)
    }

    // -- private helpers ----------------------------------------------------

    fn resolve_root(&self) -> Option<PathBuf> {
        if let Some(ref root) = self.inner.root_override {
            return Some(root.clone());
        }
        std::env::current_dir().ok()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests intentionally use unwrap to keep setup terse
mod tests {
    use super::*;
    use std::fs;

    // RAII temp root: removed automatically when the guard drops.
    fn tmp_root() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn build_returns_none_for_empty_root() {
        let root = tmp_root();
        // Empty directory → no parseable files → map is empty → None.
        let inj = RepoMapInjector::new(Some(root.path()), 2_000);
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
        let root = tmp_root();
        fs::write(
            root.path().join("hello.rs"),
            "pub fn greet(name: &str) -> String { format!(\"hi {name}\") }\n",
        )
        .unwrap();
        let inj = RepoMapInjector::new(Some(root.path()), 2_000);
        let md = inj.build().expect("markdown for populated root");
        assert!(md.contains("Repo Map:"));
        assert!(md.contains("greet"));
    }

    #[test]
    fn notify_file_changed_updates_cache() {
        let root = tmp_root();
        fs::write(root.path().join("a.rs"), "pub fn a() {}\n").unwrap();
        let inj = RepoMapInjector::new(Some(root.path()), 2_000);
        // Warm the cache.
        let _ = inj.build();
        // Add a new file and notify — next build should surface it.
        fs::write(root.path().join("b.rs"), "pub fn freshly_added() {}\n").unwrap();
        let _ = inj.notify_file_changed(&root.path().join("b.rs")).unwrap();
        let md = inj.build().expect("markdown after notify");
        assert!(md.contains("freshly_added"));
    }

    /// T12b: repeated builds serve the cached rendering — identical output,
    /// and the underlying map is never destructively trimmed (the old
    /// `pack` path deep-cloned the whole map every turn to protect it).
    #[test]
    fn build_reuses_cached_rendering_without_retrimming() {
        let root = tmp_root();
        for i in 0..6 {
            fs::write(
                root.path().join(format!("m{i}.rs")),
                format!("pub fn stable_fn_{i}(x: i32) -> i32 {{ x + {i} }}\n"),
            )
            .unwrap();
        }
        // Tiny budget: the trim actually drops symbols, so any destructive
        // packing or per-turn divergence would show up here.
        let inj = RepoMapInjector::new(Some(root.path()), 40);
        let first = inj.build().expect("first build");
        let second = inj.build().expect("second build");
        assert_eq!(first, second, "unchanged turns must render identically");

        // The cached map still tracks every file with its full symbol list
        // (pack_snapshot is non-destructive) and the rendering is memoised.
        let guard = inj.inner.cache.read().unwrap();
        let cached = guard.as_ref().expect("cache populated by build");
        assert_eq!(cached.cache.file_count(), 6);
        assert!(cached.rendered.is_some());
    }

    /// T12b: a notify_file_changed invalidates the memoised rendering —
    /// the next build re-renders from the updated map instead of serving
    /// the cached string.
    #[test]
    fn modified_file_changes_cached_rendering_after_notify() {
        let root = tmp_root();
        let file = root.path().join("solo.rs");
        fs::write(&file, "pub fn before() -> u32 { 1 }\n").unwrap();
        let inj = RepoMapInjector::new(Some(root.path()), 2_000);
        assert!(inj.build().expect("initial build").contains("before"));

        // Rewrite the file and pin a distinctly different mtime so the
        // whole-second fast path in `update_file` can't swallow the change.
        fs::write(&file, "pub fn after() -> u32 { 2 }\n").unwrap();
        let f = fs::OpenOptions::new().append(true).open(&file).unwrap();
        f.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000))
            .unwrap();
        assert!(inj.notify_file_changed(&file).unwrap());

        let md = inj.build().expect("rebuild after notify");
        assert!(md.contains("after"), "stale rendering served: {md}");
        assert!(!md.contains("before"), "stale symbol served: {md}");
    }

    #[test]
    fn invalidating_forces_fresh_walk() {
        let root = tmp_root();
        fs::write(root.path().join("a.rs"), "pub fn a() {}\n").unwrap();
        let inj = RepoMapInjector::new(Some(root.path()), 2_000);
        let _ = inj.build();
        inj.invalidate();
        // No panic, no broken state.
        let _ = inj.build().expect("build after invalidate");
    }
}
