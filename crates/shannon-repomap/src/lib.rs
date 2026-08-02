//! # Shannon Repo Map (P1-4 Phase A)
//!
//! Walk a Rust workspace, parse every `.rs` file with tree-sitter, and emit a
//! structured symbol map small enough to inject into the LLM system prompt.
//!
//! Phase A scope (per `docs/plans/repo-map.md`):
//!
//! - Rust only. Other languages come in Phase B.
//! - No incremental cache. Each `for_workspace` call re-parses from disk.
//! - No `notify` watcher. Debounced file change invalidation is Phase B.
//!
//! Typical usage:
//!
//! ```no_run
//! use shannon_repomap::RepoMap;
//! use std::path::Path;
//!
//! let mut repo_map = RepoMap::for_workspace(Path::new("crates/shannon-types"))?;
//! repo_map.trim_to_budget(2_000);
//! let md = repo_map.to_system_prompt_markdown();
//! println!("{md}");
//! # Ok::<(), anyhow::Error>(())
//! ```

pub mod budget;
pub mod parser;
pub mod symbol_tree;

pub use symbol_tree::{Span, SymbolKind, SymbolMap, SymbolNode};

use anyhow::Result;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Concrete repo map wrapper. Owns a [`SymbolMap`] plus the `root` the walk
/// started from (useful for rendering relative paths in the markdown view).
#[derive(Debug, Clone)]
pub struct RepoMap {
    pub map: SymbolMap,
}

impl RepoMap {
    /// Walk `cwd`, parse every `.rs` file under it, and collect symbols.
    ///
    /// Files that fail to parse are skipped silently (with a `tracing::debug`
    /// log) rather than aborting the whole walk — a single broken file
    /// shouldn't blank out the whole repo map.
    pub fn for_workspace(cwd: &Path) -> Result<Self> {
        let mut files = Vec::new();
        for entry in WalkDir::new(cwd).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            match parser::parse_rust_file(path) {
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
    /// ## <relative/path.rs>
    /// - **fn** `name(args) -> Ret` — at line 12
    ///   - **fn** `method(&self) -> ()` — at line 24
    /// - **struct** `Foo` — at line 30
    ///
    /// ## <other/file.rs>
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
