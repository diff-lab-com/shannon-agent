//! Provider-based directory walking with simplified gitignore support.
//!
//! The default [`FileSystemProvider::walk_blocking`] implementation lives
//! here so every execution world (local, SSH, Docker) inherits it; `LocalFs`
//! overrides the trait method with an `ignore::WalkBuilder`-backed version
//! that keeps today's local traversal byte-for-byte.
//!
//! Gitignore support is intentionally a subset: per-directory `.gitignore`
//! files, `!` negation, trailing `/` directory-only patterns, `*`/`?`/`**`
//! globs, and the "last matching rule wins" precedence with deeper files
//! overriding shallower ones. Global gitignores and `.git/info/exclude` are
//! not consulted (documented limitation). Like `ignore::WalkBuilder`'s
//! default (`require_git`), gitignore files only apply when the walk root
//! contains a `.git` entry.

use std::io;
use std::path::{Path, PathBuf};

use crate::{DirEntryInfo, FileMeta};

/// Directories never traversed regardless of gitignore files.
pub const BUILTIN_EXCLUDES: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    ".venv",
    "__pycache__",
];

/// Walk `root` depth-first, invoking `cb` for every entry (including the
/// root itself). Returning `false` from `cb` prunes that directory's
/// subtree. Entries arrive in lexicographic order for determinism.
pub type StatFn<'a> = &'a dyn Fn(&Path) -> io::Result<FileMeta>;
pub type ReadTextFn<'a> = &'a dyn Fn(&Path) -> io::Result<String>;
pub type ListDirFn<'a> = &'a dyn Fn(&Path) -> io::Result<Vec<DirEntryInfo>>;

/// Walk `root` using the three primitive ops (stat / read_text / list_dir).
/// Kept free of the trait so trait-default bodies can call it without
/// proving `Self: 'static`.
pub fn provider_walk(
    metadata: StatFn<'_>,
    read_text: ReadTextFn<'_>,
    list_dir: ListDirFn<'_>,
    root: &Path,
    cb: &mut dyn FnMut(&DirEntryInfo) -> bool,
) -> io::Result<()> {
    let meta = metadata(root)?;
    let root_entry = DirEntryInfo {
        path: root.to_path_buf(),
        len: meta.len,
        is_dir: meta.is_dir,
    };
    if !cb(&root_entry) || !meta.is_dir {
        return Ok(());
    }

    // `require_git` parity: gitignore files only matter inside a repo.
    let use_gitignore = metadata(&root.join(".git")).is_ok();

    let mut levels: Vec<(PathBuf, Option<GitignoreMatcher>)> = Vec::new();
    walk_dir(
        metadata,
        read_text,
        list_dir,
        root,
        use_gitignore,
        &mut levels,
        cb,
    )
}

fn walk_dir(
    _metadata: StatFn<'_>,
    read_text: ReadTextFn<'_>,
    list_dir: ListDirFn<'_>,
    dir: &Path,
    use_gitignore: bool,
    levels: &mut Vec<(PathBuf, Option<GitignoreMatcher>)>,
    cb: &mut dyn FnMut(&DirEntryInfo) -> bool,
) -> io::Result<()> {
    let matcher = if use_gitignore {
        read_text(&dir.join(".gitignore"))
            .ok()
            .map(|text| GitignoreMatcher::compile(&text))
    } else {
        None
    };
    levels.push((dir.to_path_buf(), matcher));

    let mut entries = list_dir(dir)?;
    entries.sort_by(|a, b| a.path.cmp(&b.path));

    for entry in entries {
        let Some(name) = entry.path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        // `hidden(true)` parity: skip dotfiles/dot-dirs (the walk root
        // itself was already yielded).
        if name.starts_with('.') {
            continue;
        }
        if entry.is_dir && BUILTIN_EXCLUDES.contains(&name) {
            continue;
        }
        if use_gitignore && is_ignored(levels, &entry.path, entry.is_dir) {
            continue;
        }
        let descend = cb(&entry);
        if entry.is_dir && descend {
            walk_dir(
                _metadata,
                read_text,
                list_dir,
                &entry.path,
                use_gitignore,
                levels,
                cb,
            )?;
        }
    }

    levels.pop();
    Ok(())
}

/// Deepest level with a matching rule decides (deeper files override
/// shallower ones); within a file the last matching rule wins.
fn is_ignored(levels: &[(PathBuf, Option<GitignoreMatcher>)], path: &Path, is_dir: bool) -> bool {
    for (dir, matcher) in levels.iter().rev() {
        let Some(matcher) = matcher else { continue };
        let Ok(rel) = path.strip_prefix(dir) else {
            continue;
        };
        let rel = rel
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        if let Some(decision) = matcher.decide(&rel, is_dir) {
            return decision;
        }
    }
    false
}

/// One compiled `.gitignore` file.
#[derive(Debug, Default)]
pub struct GitignoreMatcher {
    rules: Vec<GitignoreRule>,
}

#[derive(Debug)]
struct GitignoreRule {
    /// `!pattern` — a match un-ignores.
    negated: bool,
    /// Trailing `/` — only matches directories.
    dir_only: bool,
    /// Contains a `/` — matches against the path relative to the gitignore
    /// directory instead of just the basename.
    anchored: bool,
    pattern: String,
}

impl GitignoreMatcher {
    /// Compile gitignore text (comments and blank lines skipped).
    pub fn compile(text: &str) -> Self {
        let mut rules = Vec::new();
        for raw in text.lines() {
            let line = raw.trim_end();
            let line = line.strip_prefix('\u{feff}').unwrap_or(line);
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (negated, line) = match line.strip_prefix('!') {
                Some(rest) => (true, rest),
                None => (false, line),
            };
            let (dir_only, line) = match line.strip_suffix('/') {
                Some(rest) => (true, rest),
                None => (false, line),
            };
            let anchored = line.contains('/');
            let pattern = line.strip_prefix('/').unwrap_or(line).to_string();
            if pattern.is_empty() {
                continue;
            }
            rules.push(GitignoreRule {
                negated,
                dir_only,
                anchored,
                pattern,
            });
        }
        Self { rules }
    }

    /// `Some(true)` = ignored, `Some(false)` = explicitly re-included,
    /// `None` = no rule matched.
    pub fn decide(&self, rel: &str, is_dir: bool) -> Option<bool> {
        let mut decision = None;
        for rule in &self.rules {
            if rule.matches(rel, is_dir) {
                decision = Some(!rule.negated);
            }
        }
        decision
    }
}

impl GitignoreRule {
    fn matches(&self, rel: &str, is_dir: bool) -> bool {
        if self.dir_only && !is_dir {
            return false;
        }
        if self.anchored {
            let pat: Vec<&str> = self.pattern.split('/').filter(|s| !s.is_empty()).collect();
            let txt: Vec<&str> = rel.split('/').filter(|s| !s.is_empty()).collect();
            match_segments(&pat, &txt)
        } else {
            let Some(basename) = rel.rsplit('/').next() else {
                return false;
            };
            match_segment(&self.pattern, basename)
        }
    }
}

/// Wildcard match within one path segment: `*` (any run), `?` (one char),
/// otherwise literal.
fn match_segment(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    seg_rec(&p, &t)
}

fn seg_rec(p: &[char], t: &[char]) -> bool {
    match p.first() {
        None => t.is_empty(),
        Some('*') => {
            // Skip consecutive stars, then try consuming any number of chars.
            let rest = &p[p.iter().take_while(|c| **c == '*').count()..];
            (0..=t.len()).any(|skip| seg_rec(rest, &t[skip..]))
        }
        Some('?') => !t.is_empty() && seg_rec(&p[1..], &t[1..]),
        Some(c) => !t.is_empty() && t[0] == *c && seg_rec(&p[1..], &t[1..]),
    }
}

/// Segment-wise match for anchored patterns with `**` spanning directories
/// (`**` matches zero or more whole segments).
fn match_segments(pat: &[&str], txt: &[&str]) -> bool {
    match pat.first() {
        None => txt.is_empty(),
        Some(&"**") => {
            match_segments(&pat[1..], txt) || (!txt.is_empty() && match_segments(pat, &txt[1..]))
        }
        Some(p) => {
            !txt.is_empty() && match_segment(p, txt[0]) && match_segments(&pat[1..], &txt[1..])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ignored(gitignore: &str, rel: &str, is_dir: bool) -> Option<bool> {
        GitignoreMatcher::compile(gitignore).decide(rel, is_dir)
    }

    #[test]
    fn basename_patterns_match_at_any_depth() {
        let g = "*.log\n";
        assert_eq!(ignored(g, "a.log", false), Some(true));
        assert_eq!(ignored(g, "sub/dir/b.log", false), Some(true));
        assert_eq!(ignored(g, "keep.txt", false), None);
    }

    #[test]
    fn dir_only_patterns_skip_files() {
        let g = "build/\n";
        assert_eq!(ignored(g, "build", true), Some(true));
        assert_eq!(ignored(g, "build", false), None);
        assert_eq!(ignored(g, "a/build", true), Some(true));
    }

    #[test]
    fn negation_last_match_wins() {
        let g = "*.log\n!keep.log\n";
        assert_eq!(ignored(g, "a.log", false), Some(true));
        assert_eq!(ignored(g, "keep.log", false), Some(false));
    }

    #[test]
    fn anchored_patterns_match_full_relative_path() {
        let g = "/vendor/\n";
        assert_eq!(ignored(g, "vendor", true), Some(true));
        assert_eq!(ignored(g, "sub/vendor", true), None);

        let g2 = "docs/*.md\n";
        assert_eq!(ignored(g2, "docs/x.md", false), Some(true));
        assert_eq!(ignored(g2, "docs/deep/x.md", false), None);
    }

    #[test]
    fn double_star_spans_directories() {
        let g = "**/temp\n";
        assert_eq!(ignored(g, "temp", true), Some(true));
        assert_eq!(ignored(g, "a/b/temp", true), Some(true));

        let g2 = "a/**/b\n";
        assert_eq!(ignored(g2, "a/b", true), Some(true));
        assert_eq!(ignored(g2, "a/x/y/b", true), Some(true));
    }

    #[test]
    fn question_mark_matches_single_char() {
        let g = "file?.txt\n";
        assert_eq!(ignored(g, "file1.txt", false), Some(true));
        assert_eq!(ignored(g, "file10.txt", false), None);
    }

    #[test]
    fn comments_and_blanks_are_skipped() {
        let g = "# comment\n\n*.tmp\n";
        assert_eq!(ignored(g, "x.tmp", false), Some(true));
        assert_eq!(ignored(g, "# comment", false), None);
    }
}
