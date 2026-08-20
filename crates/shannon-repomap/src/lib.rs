//! # Shannon Repo Map (P1-4 Phase B + incremental update)
//!
//! Walk a source tree, parse every supported file with tree-sitter, and emit
//! a structured symbol map small enough to inject into the LLM system
//! prompt.
//!
//! Phase A (Rust only) shipped the foundation. Phase B extends the
//! [`LanguageParser`] enum to cover TypeScript (`.ts`/`.tsx`), Python
//! (`.py`/`.pyi`), and Go (`.go`); see `docs/plans/repo-map.md` §Phase B for
//! the design and the plan's §tree-sitter-versions section for the version
//! pin rationale.
//!
//! Phase C (this update) adds:
//!
//! - [`RepoMapCache`] — a persistent, incrementally updatable symbol cache
//!   keyed by the project root. Replaces the "re-walk the world" path with
//!   per-file updates that don't touch sibling files.
//! - [`RepoMapWatcher`] — a thin `notify` wrapper that turns filesystem
//!   events into [`WatcherEvent`]s the cache can consume.
//! - [`RepoMapCache::pack`] — a one-shot "trim to budget and render markdown"
//!   helper used by the query engine when building the system prompt.
//!
//! Typical usage:
//!
//! ```no_run
//! use shannon_repomap::{RepoMap, RepoMapCache, RepoMapWatcher};
//! use std::path::Path;
//!
//! // Mixed-language directory walk (one-shot).
//! let mut repo_map = RepoMap::from_dir(Path::new("."))?;
//! repo_map.trim_to_budget(4_000);
//! let md = repo_map.to_system_prompt_markdown();
//! println!("{md}");
//!
//! // Or a single file with extension-based language detection.
//! let mut single = RepoMap::from_path(Path::new("src/main.py"))?;
//! let md = single.to_system_prompt_markdown();
//! # Ok::<(), anyhow::Error>(())
//! ```
//!
//! ```no_run
//! # use shannon_repomap::{RepoMapCache, RepoMapWatcher};
//! # use std::path::Path;
//! // Incremental: load the disk cache (or full-walk on cold start) and keep
//! // it up to date via filesystem events.
//! let mut cache = RepoMapCache::new(Path::new("."))?;
//! let _watcher = RepoMapWatcher::start(
//!     Path::new("."),
//!     move |evt| {
//!         let mut cache = cache.clone();
//!         let path = evt.path.clone();
//!         // Best-effort: ignore errors so a single broken file can't kill
//!         // the watcher loop.
//!         let _ = cache.update_file(&path);
//!     },
//! )?;
//! # Ok::<(), anyhow::Error>(())
//! ```

pub mod budget;
pub mod cache;
pub mod parser;
pub mod symbol_tree;
pub mod watcher;

pub use cache::{CacheError, RepoMapCache};
pub use parser::{LanguageParser, RepoMapError};
pub use symbol_tree::{Span, SymbolKind, SymbolMap, SymbolNode};
pub use watcher::{RepoMapWatcher, WatcherEvent, WatcherEventKind};

use anyhow::Result;
use std::path::{Path, PathBuf};
use walkdir::{DirEntry, WalkDir};

/// Well-known dependency and build-output directories that never hold the
/// user's own source. Walking them floods the map — this monorepo's
/// `desktop/ui/node_modules` alone contributed 1.6MB of `.d.ts` headers to
/// the system prompt (588k prompt tokens per headless query).
pub(crate) const IGNORED_DIRS: [&str; 4] = [".git", "node_modules", "target", "dist"];

/// `true` for directory entries the repo map walk should prune (never the
/// walk root itself, so a repo literally named `target` still works).
pub(crate) fn is_ignored_dir(entry: &DirEntry) -> bool {
    entry.depth() > 0
        && entry.file_type().is_dir()
        && entry
            .file_name()
            .to_str()
            .is_some_and(|name| IGNORED_DIRS.contains(&name))
}

/// Concrete repo map wrapper. Owns a [`SymbolMap`] plus the `root` the walk
/// started from (useful for rendering relative paths in the markdown view).
#[derive(Debug, Clone)]
pub struct RepoMap {
    pub map: SymbolMap,
}

impl RepoMap {
    /// Walk `cwd`, parse every supported file under it, and collect symbols.
    ///
    /// Phase A called this `for_workspace` and only handled `.rs`. Phase B
    /// keeps the same name for backwards compatibility but expands the
    /// extension filter to all languages in [`LanguageParser`].
    ///
    /// Files that fail to parse are skipped silently (with a `tracing::debug`
    /// log) rather than aborting the whole walk — a single broken file
    /// shouldn't blank out the whole repo map.
    pub fn for_workspace(cwd: &Path) -> Result<Self> {
        Self::from_dir(cwd)
    }

    /// Walk `cwd` and parse every file whose extension maps to a supported
    /// language. See [`LanguageParser::from_extension`] for the list.
    pub fn from_dir(cwd: &Path) -> Result<Self> {
        let mut files = Vec::new();
        for entry in WalkDir::new(cwd)
            .into_iter()
            .filter_entry(|e| !is_ignored_dir(e))
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            if LanguageParser::from_extension(ext).is_err() {
                continue;
            }
            match parser::parse_file(path) {
                Ok(syms) => files.push((path.to_path_buf(), syms)),
                Err(err) => {
                    tracing::debug!(
                        path = %path.display(),
                        error = %err,
                        "shannon-repomap: skipping unparseable file"
                    );
                }
            }
        }
        Ok(Self {
            map: SymbolMap {
                root: cwd.to_path_buf(),
                files,
            },
        })
    }

    /// Parse a single file. The language is detected from the extension;
    /// unknown extensions return [`RepoMapError::UnsupportedLanguage`].
    pub fn from_path(path: &Path) -> std::result::Result<Self, RepoMapError> {
        // Validate the extension up front so the user gets a clean error
        // even before we open the file.
        let _ = LanguageParser::from_path(path)?;
        let syms = parser::parse_file(path)?;
        let path_buf = path.to_path_buf();
        let root = path_buf
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        Ok(Self {
            map: SymbolMap {
                root,
                files: vec![(path_buf, syms)],
            },
        })
    }

    /// Apply the token budget trim. See [`budget::trim_to_budget`].
    pub fn trim_to_budget(&mut self, budget: usize) {
        budget::trim_to_budget(&mut self.map, budget);
    }

    /// Estimated tokens across the (possibly trimmed) map.
    pub fn token_estimate(&self) -> usize {
        budget::total_tokens(&self.map)
    }

    /// Render the map as markdown suitable for system-prompt injection.
    ///
    /// Format:
    ///
    /// ```text
    /// # Repo Map: <root>
    ///
    /// ## <relative/path.ext>
    /// - **fn** `name(args) -> Ret` — at line 12
    ///   - **fn** `method(&self) -> ()` — at line 24
    /// - **struct** `Foo` — at line 30
    ///
    /// ## <other/file.ext>
    /// ...
    /// ```
    pub fn to_system_prompt_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("# Repo Map: {}\n\n", self.map.root.display()));
        for (path, syms) in &self.map.files {
            let rel = relative_path(&self.map.root, path);
            out.push_str(&format!("## {}\n", rel.display()));
            if syms.is_empty() {
                out.push_str("_(no top-level symbols after trim)_\n\n");
                continue;
            }
            for sym in syms {
                render_symbol(&mut out, sym, 0);
            }
            out.push('\n');
        }
        out
    }
}

/// Render one symbol at the given indent depth (one level = two spaces).
fn render_symbol(out: &mut String, sym: &SymbolNode, depth: usize) {
    let indent = "  ".repeat(depth);
    let line_no = sym.span.start_line + 1; // tree-sitter is 0-indexed; humans aren't
    out.push_str(&format!(
        "{indent}- **{kind}** `{sig}` — at line {line_no}\n",
        kind = sym.kind.label(),
        sig = sym.signature,
    ));
    for child in &sym.children {
        render_symbol(out, child, depth + 1);
    }
}

/// Best-effort relative path display. Falls back to absolute if the path
/// can't be made relative (e.g. a symlink that escapes the root).
fn relative_path(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root)
        .map(PathBuf::from)
        .unwrap_or_else(|_| path.to_path_buf())
}

/// Re-export so call sites don't need to know the module path.
pub use budget::estimate_tokens;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_skips_node_modules_and_target() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::write(root.join("main.rs"), "fn main() {}\n").expect("write main.rs");
        for ignored in IGNORED_DIRS {
            let sub = root.join(ignored);
            std::fs::create_dir_all(&sub).expect("mkdir");
            std::fs::write(sub.join("vendor.rs"), "fn vendor_only() {}\n")
                .expect("write vendor.rs");
        }
        let map = RepoMap::from_dir(root).expect("walk");
        let paths: Vec<String> = map
            .map
            .files
            .iter()
            .map(|(p, _)| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        // Only the user's own source file: vendor trees pruned at the dir
        // level (regression: desktop/ui/node_modules flooded the system
        // prompt with 1.6MB of .d.ts headers).
        assert_eq!(paths, vec!["main.rs".to_string()]);
    }
}
