//! Obsidian vault data source fetcher.
//!
//! Reads markdown notes directly from a local Obsidian vault directory —
//! no network, no API. Config comes from the installed data source TOML
//! (`~/.shannon/data-sources/obsidian-vault.toml`):
//!
//! - `vault_path` (required) — absolute path to the vault root.
//!
//! Query semantics:
//! - Empty query — list the most recently modified notes (mtime descending).
//! - Non-empty query — case-insensitive filename + content keyword match,
//!   ranked by title match > content hit count > mtime. The excerpt shows
//!   ~200 characters around the first content hit.
//!
//! The walk is plain `std::fs` recursion (no walkdir dep here): hidden
//! directories (`.obsidian/`, `.git/`, …) are skipped, non-`.md` files are
//! ignored, and symlinks are never followed.

use super::{DataSourceError, DataSourceFetcher, DataSourceItem, DataSourceResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Default number of results returned per query. The `DataSourceFetcher`
/// trait carries no limit parameter, so this is the fixed page size.
pub const DEFAULT_QUERY_LIMIT: usize = 20;

/// Size of the excerpt window (in chars) around the first content match.
pub const EXCERPT_WINDOW: usize = 200;

/// Notes larger than this are indexed by filename only — their content is
/// skipped to bound memory on pathological vaults.
const MAX_NOTE_BYTES: u64 = 2 * 1024 * 1024;

/// Obsidian vault fetcher.
#[derive(Debug, Clone, Copy)]
pub struct ObsidianFetcher;

/// One discovered note with its ranking inputs.
struct NoteCandidate {
    /// Path relative to the vault root (forward slashes).
    rel_path: String,
    /// Absolute path on disk.
    abs_path: PathBuf,
    /// Filename without the `.md` extension.
    title: String,
    mtime: Option<SystemTime>,
}

#[async_trait]
impl DataSourceFetcher for ObsidianFetcher {
    async fn fetch(
        &self,
        config: &BTreeMap<String, String>,
        query: &str,
    ) -> Result<DataSourceResult, DataSourceError> {
        let vault = config
            .get("vault_path")
            .ok_or(DataSourceError::MissingConfig("vault_path".into()))?;
        let vault_path = PathBuf::from(vault);
        let query = query.to_string();

        tokio::task::spawn_blocking(move || fetch_sync(&vault_path, &query, DEFAULT_QUERY_LIMIT))
            .await
            .map_err(|e| {
                DataSourceError::UpstreamError(format!("Obsidian query task failed: {e}"))
            })?
    }
}

/// Blocking implementation of the vault query. Pure `std::fs` — unit tests
/// drive this directly against a tempdir vault.
pub(crate) fn fetch_sync(
    vault: &Path,
    query: &str,
    limit: usize,
) -> Result<DataSourceResult, DataSourceError> {
    if !vault.exists() {
        return Err(DataSourceError::UpstreamError(format!(
            "Obsidian vault path does not exist: {}",
            vault.display()
        )));
    }
    if !vault.is_dir() {
        return Err(DataSourceError::UpstreamError(format!(
            "Obsidian vault path is not a directory: {}",
            vault.display()
        )));
    }

    let mut notes = Vec::new();
    collect_notes(vault, vault, &mut notes);

    if query.trim().is_empty() {
        // Empty query — most recently modified notes first.
        notes.sort_by_key(|note| std::cmp::Reverse(mtime_of(note)));
        let total = notes.len();
        let items: Vec<DataSourceItem> = notes
            .into_iter()
            .take(limit)
            .map(|note| {
                let body = read_preview(&note.abs_path);
                to_item(note, body)
            })
            .collect();
        return Ok(DataSourceResult {
            has_more: total > items.len(),
            total,
            items,
        });
    }

    // Keyword query — score every note, keep hits, rank, take `limit`.
    let needle = query.trim().to_lowercase();
    let mut hits: Vec<(bool, usize, NoteCandidate, Option<String>)> = notes
        .into_iter()
        .filter_map(|note| {
            let content = read_content(&note.abs_path)?;
            let title_hit = note.title.to_lowercase().contains(&needle);
            let hit_count = count_occurrences(&content, &needle);
            if !title_hit && hit_count == 0 {
                return None;
            }
            let excerpt = if hit_count > 0 {
                Some(excerpt_around(&content, &needle))
            } else {
                read_preview(&note.abs_path)
            };
            Some((title_hit, hit_count, note, excerpt))
        })
        .collect();

    // Rank: title hit > content hit count > mtime (newest first).
    hits.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then(b.1.cmp(&a.1))
            .then(mtime_of(&b.2).cmp(&mtime_of(&a.2)))
    });

    let total = hits.len();
    let items: Vec<DataSourceItem> = hits
        .into_iter()
        .take(limit)
        .map(|(_, _, note, excerpt)| to_item(note, excerpt))
        .collect();
    Ok(DataSourceResult {
        has_more: total > items.len(),
        total,
        items,
    })
}

/// Recursively collect `.md` notes under `dir`, skipping hidden entries and
/// symlinks. Errors on individual entries/directories are skipped — a single
/// unreadable folder must not fail the whole query.
fn collect_notes(root: &Path, dir: &Path, out: &mut Vec<NoteCandidate>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        // Never follow symlinks — neither for files nor directories.
        if file_type.is_symlink() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // Skip hidden files/directories (.obsidian, .trash, .git, ., ..).
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            collect_notes(root, &path, out);
        } else if name.to_lowercase().ends_with(".md") {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            // The `.ends_with(".md")` check above guarantees the last three
            // bytes are the ASCII extension, so slicing is boundary-safe.
            let title = name[..name.len() - 3].to_string();
            out.push(NoteCandidate {
                rel_path: rel,
                title,
                mtime: entry.metadata().ok().and_then(|m| m.modified().ok()),
                abs_path: path,
            });
        }
    }
}

fn mtime_of(note: &NoteCandidate) -> SystemTime {
    note.mtime.unwrap_or(SystemTime::UNIX_EPOCH)
}

/// Read the note body if it is valid UTF-8 and within the size cap.
fn read_content(path: &Path) -> Option<String> {
    let len = std::fs::metadata(path).ok()?.len();
    if len > MAX_NOTE_BYTES {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// Short preview for list-mode cards: first non-empty line, truncated.
fn read_preview(path: &Path) -> Option<String> {
    let content = read_content(path)?;
    content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| truncate_chars(line, EXCERPT_WINDOW))
}

/// Number of case-insensitive occurrences of `needle` in `haystack`.
fn count_occurrences(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let hay = haystack.to_lowercase();
    let mut count = 0usize;
    let mut offset = 0usize;
    while let Some(pos) = hay[offset..].find(needle) {
        count += 1;
        offset += pos + needle.len();
    }
    count
}

/// Build a ~[`EXCERPT_WINDOW`] char excerpt around the first (case-insensitive)
/// match in `content`, on char boundaries, with ellipses marking truncation.
fn excerpt_around(content: &str, needle: &str) -> String {
    let hay = content.to_lowercase();
    let Some(match_pos) = hay.find(needle) else {
        return truncate_chars(content.trim(), EXCERPT_WINDOW);
    };

    // Convert the byte offset into a char index so we can window on chars.
    let match_char = content[..match_pos].chars().count();
    let half = EXCERPT_WINDOW / 2;
    let start_char = match_char.saturating_sub(half);
    let chars: Vec<char> = content.chars().collect();
    let end_char = (start_char + EXCERPT_WINDOW).min(chars.len());

    let mut excerpt = String::new();
    if start_char > 0 {
        excerpt.push('…');
    }
    excerpt.extend(chars.iter().skip(start_char).take(end_char - start_char));
    if end_char < chars.len() {
        excerpt.push('…');
    }
    excerpt.trim().to_string()
}

/// Truncate a string to at most `max` chars on char boundaries.
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max).collect();
    format!("{truncated}…")
}

fn to_item(note: NoteCandidate, body: Option<String>) -> DataSourceItem {
    DataSourceItem {
        id: note.rel_path.clone(),
        title: note.title,
        body,
        // Vault-relative path doubles as the URL/path handle (the vault is
        // local — there is no upstream web UI to link to).
        url: Some(note.rel_path),
        kind: "note".into(),
        updated_at: note.mtime.map(|t| DateTime::<Utc>::from(t).to_rfc3339()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::thread::sleep;
    use std::time::Duration;

    /// Create a vault with the fixture layout used by every test.
    fn make_vault() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().expect("tempdir");
        let vault = tmp.path();
        fs::create_dir_all(vault.join("projects/alpha")).expect("mkdir");
        fs::create_dir_all(vault.join(".obsidian")).expect("mkdir");
        fs::create_dir_all(vault.join(".trash")).expect("mkdir");

        fs::write(vault.join("README.md"), "# Vault home\n\nWelcome.\n").expect("write");
        sleep(Duration::from_millis(30));
        fs::write(
            vault.join("projects/alpha/plan.md"),
            "# Plan\n\nShip the obsidian fetcher this week.\n",
        )
        .expect("write");
        sleep(Duration::from_millis(30));
        fs::write(
            vault.join("projects/alpha/notes.md"),
            "Meeting notes:\nthe obsidian fetcher needs tests.\n",
        )
        .expect("write");
        sleep(Duration::from_millis(30));
        fs::write(vault.join("scratch.md"), "random thoughts about widgets\n").expect("write");

        // Must all be ignored.
        fs::write(vault.join(".obsidian/workspace.json"), "{}").expect("write");
        fs::write(vault.join(".trash/deleted.md"), "gone\n").expect("write");
        fs::write(vault.join("projects/alpha/diagram.png"), "not a note").expect("write");
        fs::write(vault.join("notes.txt"), "plain text is not a note\n").expect("write");
        tmp
    }

    fn rel_paths(result: &DataSourceResult) -> Vec<String> {
        result.items.iter().map(|i| i.id.clone()).collect()
    }

    #[test]
    fn fetch_requires_vault_path_config() {
        let fetcher = ObsidianFetcher;
        let config = BTreeMap::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async { fetcher.fetch(&config, "test").await });
        match result {
            Err(DataSourceError::MissingConfig(field)) => assert_eq!(field, "vault_path"),
            other => panic!("Expected MissingConfig, got {other:?}"),
        }
    }

    #[test]
    fn empty_query_lists_recent_notes_excluding_hidden_and_non_md() {
        let tmp = make_vault();
        let result = fetch_sync(tmp.path(), "", DEFAULT_QUERY_LIMIT).expect("fetch");

        let paths = rel_paths(&result);
        assert_eq!(paths.len(), 4, "only .md outside hidden dirs: {paths:?}");
        assert!(!paths.iter().any(|p| p.contains(".obsidian")));
        assert!(!paths.iter().any(|p| p.contains(".trash")));
        assert!(!paths.iter().any(|p| p.ends_with(".png")));
        assert!(!paths.iter().any(|p| p.ends_with(".txt")));

        // mtime descending: scratch.md was written last, README.md first.
        assert_eq!(paths[0], "scratch.md");
        assert_eq!(paths[3], "README.md");
        assert!(!result.has_more);
    }

    #[test]
    fn empty_query_respects_limit_and_has_more() {
        let tmp = make_vault();
        let result = fetch_sync(tmp.path(), "", 2).expect("fetch");
        assert_eq!(result.items.len(), 2);
        assert_eq!(result.total, 4, "total counts every listed note");
        assert!(result.has_more);
    }

    #[test]
    fn keyword_query_finds_matches_in_subdirectories() {
        let tmp = make_vault();
        let result = fetch_sync(tmp.path(), "fetcher", DEFAULT_QUERY_LIMIT).expect("fetch");
        let paths = rel_paths(&result);
        assert!(paths.contains(&"projects/alpha/plan.md".to_string()));
        assert!(paths.contains(&"projects/alpha/notes.md".to_string()));
        assert!(!paths.contains(&"scratch.md".to_string()));
    }

    #[test]
    fn keyword_query_is_case_insensitive() {
        let tmp = make_vault();
        let result = fetch_sync(tmp.path(), "WIDGETS", DEFAULT_QUERY_LIMIT).expect("fetch");
        assert_eq!(rel_paths(&result), vec!["scratch.md".to_string()]);
    }

    #[test]
    fn title_hit_outranks_content_hits() {
        let tmp = make_vault();
        // "obsidian" appears in the content of plan.md/notes.md (one hit each)
        // but "Rust Obsidian Guide" has it in the title with zero content hits.
        fs::write(
            tmp.path().join("rust obsidian guide.md"),
            "Totally unrelated body text.\n",
        )
        .expect("write");

        let result = fetch_sync(tmp.path(), "obsidian", DEFAULT_QUERY_LIMIT).expect("fetch");
        let titles: Vec<&str> = result.items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles.first(), Some(&"rust obsidian guide"));
        assert_eq!(result.items.len(), 3);
    }

    #[test]
    fn more_content_hits_rank_higher_than_fewer() {
        let tmp = make_vault();
        fs::write(
            tmp.path().join("many.md"),
            "queryterm and queryterm again\nplus queryterm thrice\nunrelated filler\n",
        )
        .expect("write");
        fs::write(tmp.path().join("few.md"), "queryterm once\n").expect("write");

        let result = fetch_sync(tmp.path(), "queryterm", DEFAULT_QUERY_LIMIT).expect("fetch");
        let titles: Vec<&str> = result.items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, vec!["many", "few"]);
    }

    #[test]
    fn excerpt_is_around_first_matching_line() {
        let tmp = make_vault();
        let result = fetch_sync(tmp.path(), "fetcher", DEFAULT_QUERY_LIMIT).expect("fetch");
        let plan = result
            .items
            .iter()
            .find(|i| i.title == "plan")
            .expect("plan.md in results");
        let body = plan.body.as_deref().expect("excerpt body");
        assert!(body.to_lowercase().contains("fetcher"));
        assert!(body.chars().count() <= EXCERPT_WINDOW + 2); // + ellipses
    }

    #[test]
    fn non_matching_query_returns_empty_result() {
        let tmp = make_vault();
        let result =
            fetch_sync(tmp.path(), "zzz-no-such-term", DEFAULT_QUERY_LIMIT).expect("fetch");
        assert!(result.items.is_empty());
        assert_eq!(result.total, 0);
        assert!(!result.has_more);
    }

    #[test]
    fn items_carry_kind_note_and_relative_url() {
        let tmp = make_vault();
        let result = fetch_sync(tmp.path(), "", DEFAULT_QUERY_LIMIT).expect("fetch");
        let item = &result.items[0];
        assert_eq!(item.kind, "note");
        assert_eq!(item.url.as_deref(), Some(item.id.as_str()));
        assert!(item.updated_at.is_some(), "mtime rendered as RFC 3339");
    }

    #[test]
    fn symlinks_are_not_followed() {
        let tmp = make_vault();
        let outside = tempfile::tempdir().expect("outside tempdir");
        fs::write(outside.path().join("linked.md"), "should stay out\n").expect("write");

        #[cfg(unix)]
        {
            if std::os::unix::fs::symlink(outside.path(), tmp.path().join("linked-dir")).is_ok() {
                let result = fetch_sync(tmp.path(), "", DEFAULT_QUERY_LIMIT).expect("fetch");
                assert!(!rel_paths(&result).iter().any(|p| p.contains("linked.md")));
            }
        }
        #[cfg(windows)]
        {
            if std::os::windows::fs::symlink_dir(outside.path(), tmp.path().join("linked-dir"))
                .is_ok()
            {
                let result = fetch_sync(tmp.path(), "", DEFAULT_QUERY_LIMIT).expect("fetch");
                assert!(!rel_paths(&result).iter().any(|p| p.contains("linked.md")));
            }
        }
    }

    #[test]
    fn missing_vault_errors_clearly() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let result = fetch_sync(&tmp.path().join("nope"), "", DEFAULT_QUERY_LIMIT);
        match result {
            Err(DataSourceError::UpstreamError(msg)) => {
                assert!(msg.contains("does not exist"), "got: {msg}");
            }
            other => panic!("Expected UpstreamError, got {other:?}"),
        }
    }

    #[test]
    fn vault_path_must_be_a_directory() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("file.md");
        fs::write(&file, "x").expect("write");
        let result = fetch_sync(&file, "", DEFAULT_QUERY_LIMIT);
        match result {
            Err(DataSourceError::UpstreamError(msg)) => {
                assert!(msg.contains("not a directory"), "got: {msg}");
            }
            other => panic!("Expected UpstreamError, got {other:?}"),
        }
    }

    #[test]
    fn excerpt_helpers_respect_char_boundaries() {
        let content = "héllo wörld — ünïcode text with the needle ünicode inside";
        let excerpt = excerpt_around(content, "needle");
        assert!(excerpt.contains("needle"));
        let truncated = truncate_chars("abcdef", 3);
        assert_eq!(truncated, "abc…");
        assert_eq!(count_occurrences("Aa bb aa", "aa"), 2);
        assert_eq!(
            count_occurrences("anything", ""),
            0,
            "empty needle hits nothing"
        );
    }
}
