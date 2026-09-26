use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use super::consolidator::{ConsolidationResult, MemoryConsolidator};
use super::error::MemoryError;
use super::types::{MemoryCategory, MemoryEntry, MemoryType, SessionMemoryConfig};
use crate::team_memory_sync::SecretScanner;
use fs2::FileExt;

// Hash a project path to a safe filename.
//
// FNV-1a (not `DefaultHasher`): std's hasher is SipHash with
// implementation-defined keys whose output may change across Rust releases —
// a toolchain bump would re-key every project and orphan all JSONL stores.
// FNV-1a output is stable forever.
//
// `pub` (re-exported from `memory`) so the desktop dream pass names its
// per-project proposal subdirs with the same hash scheme as the memory
// stores — one naming vocabulary, no drift.
pub fn project_hash(project: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in project.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Hash with the legacy `DefaultHasher` scheme, used only to migrate
/// pre-existing store files to the stable hash on load.
fn project_hash_legacy(project: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    project.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

// ============================================================================
// JSONL storage helpers (ADR-0010 D1/D3)
// ============================================================================

/// Per-project append-only store path: `{storage_path}/{project_hash}.jsonl`.
fn project_jsonl_path(storage_path: &Path, project: &str) -> PathBuf {
    storage_path.join(format!("{}.jsonl", project_hash(project)))
}

/// Sidecar `<path>.lock` for cross-process `flock(LOCK_EX)` on the cold
/// (compaction) write path. Mirrors `provider_config_store::lockfile_for`.
fn lockfile_for(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".lock");
    PathBuf::from(s)
}

/// Acquire a blocking `flock(LOCK_EX)` on the sidecar lockfile for `path`,
/// creating it (and missing parent dirs) on demand. The returned `File`
/// releases the lock on drop via Linux `flock(2)` close-on-release semantics.
/// Hot-path appends do **not** take this lock; only the compaction rewriter
/// (`save`) does. ADR-0010 D3.
fn acquire_exclusive_lock(path: &Path) -> std::io::Result<File> {
    let lock_path = lockfile_for(path);
    if let Some(parent) = lock_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)?;
    FileExt::lock_exclusive(&file)?;
    Ok(file)
}

/// Atomically write `content` to `path` via a temp file + rename, so a crash
/// mid-write cannot leave a partial store. Mirrors `config_persist::atomic_write`.
/// Used by the compaction writer only.
fn atomic_write(path: &Path, content: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("jsonl.tmp");
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

// Simple word-level Jaccard similarity between two strings.
fn content_similarity(a: &str, b: &str) -> f64 {
    let words_a = tokenize(a);
    let words_b = tokenize(b);

    if words_a.is_empty() && words_b.is_empty() {
        return 1.0;
    }

    let intersection = words_a.intersection(&words_b).count();
    let union = words_a.union(&words_b).count();

    if union == 0 {
        return 1.0;
    }

    intersection as f64 / union as f64
}

/// Whether `ch` is a CJK ideograph / kana / hangul character.
fn is_cjk_char(ch: char) -> bool {
    matches!(ch as u32,
        0x3400..=0x4DBF   // CJK Extension A
        | 0x4E00..=0x9FFF // CJK Unified Ideographs
        | 0x3040..=0x30FF // Hiragana + Katakana
        | 0xAC00..=0xD7AF // Hangul syllables
        | 0xF900..=0xFAFF // CJK Compatibility Ideographs
    )
}

/// Tokenize text for similarity comparison.
///
/// Plain `split_whitespace` made every Chinese/Japanese/Korean sentence a
/// single token, pinning CJK similarity at 0-or-1 and silently disabling
/// both write-time dedup and compaction merge for CJK content. This
/// tokenizer splits `snake_case` / `kebab-case` / `camelCase` identifiers
/// at their boundaries and treats each CJK character as its own token, so
/// two paraphrased CJK sentences share most character tokens.
fn tokenize(text: &str) -> HashSet<String> {
    let mut out: HashSet<String> = HashSet::new();
    let mut word = String::new();
    let mut prev_lowercase = false;
    for ch in text.chars() {
        let is_separator = ch.is_whitespace() || matches!(ch, '_' | '-' | '.' | '/' | ':');
        if is_separator {
            if !word.is_empty() {
                out.insert(word.to_lowercase());
                word.clear();
            }
            prev_lowercase = false;
        } else if is_cjk_char(ch) {
            if !word.is_empty() {
                out.insert(word.to_lowercase());
                word.clear();
            }
            out.insert(ch.to_lowercase().to_string());
            prev_lowercase = false;
        } else if ch.is_uppercase() && prev_lowercase {
            // camelCase boundary: flush "foo" before "Bar".
            if !word.is_empty() {
                out.insert(word.to_lowercase());
                word.clear();
            }
            word.push(ch);
            prev_lowercase = false;
        } else {
            word.push(ch);
            prev_lowercase = ch.is_lowercase() || ch.is_numeric();
        }
    }
    if !word.is_empty() {
        out.insert(word.to_lowercase());
    }
    out
}

/// Rough token estimate for injection budgeting.
///
/// `chars / 4` underestimates CJK by ~4x (one CJK char is ~one token), which
/// let a Chinese memory budget of 2000 tokens actually inject ~8000. CJK
/// characters count as one token each; everything else keeps the codebase's
/// `chars / 4` heuristic.
pub(crate) fn estimate_tokens(text: &str) -> usize {
    let mut cjk = 0usize;
    let mut other = 0usize;
    for ch in text.chars() {
        if is_cjk_char(ch) {
            cjk += 1;
        } else {
            other += 1;
        }
    }
    cjk + other.div_ceil(4)
}

/// Mask secret-shaped substrings (API keys, cloud tokens) before content
/// enters the store. Memory write paths previously had no defense, so a
/// spoken "my api key is sk-..." was extracted verbatim and re-injected
/// into every later session. Rules are shared with team-memory sync.
pub(crate) fn redact_secrets(content: &str) -> String {
    static RULES: std::sync::OnceLock<Vec<(String, regex::Regex)>> = std::sync::OnceLock::new();
    let rules = RULES.get_or_init(|| {
        SecretScanner::default_rules()
            .into_iter()
            .filter_map(|mut rule| {
                let re = rule.compiled().ok()?.clone();
                Some((rule.id, re))
            })
            .collect()
    });
    let mut out = content.to_string();
    for (id, re) in rules {
        if re.is_match(&out) {
            out = re
                .replace_all(&out, format!("[REDACTED:{id}]"))
                .into_owned();
        }
    }
    out
}

/// Sentinel project key for cross-project (user-level) memories. Global
/// entries live in their own `<hash>.jsonl` like any project and are
/// injected into every session alongside the active project's entries.
pub const GLOBAL_SCOPE: &str = "__global__";

/// A durable deletion marker, stored as its own JSONL line:
/// `{"tombstone":"<id>","at":"<ts>"}`. In-process tombstones alone cannot
/// cross processes: a long-running REPL that never reloads would resurrect
/// an entry deleted by the desktop (or vice versa) from its in-memory map
/// on its next `save`. Appending the marker to the shared file makes the
/// deletion visible to any later `load`, same atomicity argument as `add`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoreTombstone {
    /// Id of the deleted entry.
    pub tombstone: String,
    /// When the deletion happened (UTC). Lets `save`'s reconcile ignore a
    /// tombstone that is older than a concurrent re-add of the same id.
    pub at: DateTime<Utc>,
}

/// Parse a `<hash>.jsonl` file into entries and tombstone markers,
/// tolerating a trailing partial line (skipped + logged) so a crash
/// mid-append never blocks a load. Shared by [`MemoryStore::load`] and the
/// [`MemoryStore::save`] reload-reconcile. Line order is preserved by the
/// caller — for tombstones, last writer per id wins.
fn parse_jsonl_file(path: &Path) -> (Vec<MemoryEntry>, Vec<StoreTombstone>) {
    let Ok(file) = File::open(path) else {
        return (Vec::new(), Vec::new());
    };
    let mut entries = Vec::new();
    let mut tombstones = Vec::new();
    for line in BufReader::new(file).lines() {
        let Ok(line) = line else { continue };
        if line.trim().is_empty() {
            continue;
        }
        if line.contains("\"tombstone\"") {
            match serde_json::from_str::<StoreTombstone>(&line) {
                Ok(t) => {
                    tombstones.push(t);
                    continue;
                }
                Err(e) => tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "skipping unparseable tombstone line"
                ),
            }
        }
        match serde_json::from_str::<MemoryEntry>(&line) {
            Ok(mem) => entries.push(mem),
            Err(e) => tracing::warn!(
                path = %path.display(),
                error = %e,
                "skipping unparseable memory line"
            ),
        }
    }
    (entries, tombstones)
}

// ============================================================================
// Memory Store
// ============================================================================

/// Persistent storage for memory entries, backed by per-project JSONL files.
///
/// Each project's memories live in a separate append-only file:
/// `{storage_path}/{project_hash}.jsonl` (ADR-0010 D1).
pub struct MemoryStore {
    entries: HashMap<String, MemoryEntry>,
    storage_path: PathBuf,
    /// Per-project ids deliberately removed since [`load`](Self::load), so
    /// [`save`](Self::save)'s reload-reconcile can propagate the deletion to
    /// disk without resurrecting the entry from a stale line — and without
    /// clobbering entries another agent appended concurrently (ADR-0010 C5').
    /// The value is the deletion time, mirrored into the durable
    /// `StoreTombstone` line [`evict`](Self::evict) appends.
    tombstones: HashMap<String, HashMap<String, DateTime<Utc>>>,
}

/// Conservative cap on how many memories [`MemoryStore::format_for_injection`]
/// loads into the system prompt (ADR-0010 C3' default, ~Claude Code's curated
/// `memory/` volume). Tuned empirically once injection is exercised.
const MAX_INJECTED_MEMORIES: usize = 50;

/// Cap on cross-project ([`GLOBAL_SCOPE`]) entries injected alongside the
/// active project's memories. User-level preferences are few and high-signal;
/// they must not crowd out project facts.
const MAX_INJECTED_GLOBAL: usize = 10;

/// Injection token budget for [`MemoryStore::format_for_injection`]
/// (ADR-0010 C5'). Enforced at injection time, not just at compaction time.
const MAX_INJECTED_TOKENS: usize = 2000;

/// Jaccard word-overlap at/above which two same-project, same-category entries
/// are treated as the same fact at write time (ADR-0010 D4). Matches the
/// compaction-time threshold used by [`MemoryStore::merge_duplicates`] and the
/// [`MemoryConsolidator`](super::consolidator::MemoryConsolidator) default.
const DEDUP_SIMILARITY_THRESHOLD: f64 = 0.8;

/// Durable tombstone lines older than this are garbage-collected from the
/// store file at the next rewrite. A tombstone only needs to outlive
/// processes that loaded before the deletion; weeks is generous.
const TOMBSTONE_TTL: Duration = Duration::days(30);

/// Outcome of a dedup-aware write ([`MemoryStore::add_or_update`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddOutcome {
    /// A brand-new entry was appended under a fresh id.
    Inserted,
    /// A near-duplicate was found and updated in place (its id reused); the
    /// previous JSONL line is now a stale duplicate that compaction reclaims.
    Updated,
}

impl MemoryStore {
    /// Create a new empty memory store.
    ///
    /// The storage directory will be created on [`load`](Self::load) if it
    /// does not already exist.
    pub fn new(storage_path: PathBuf) -> Self {
        Self {
            entries: HashMap::new(),
            storage_path,
            tombstones: HashMap::new(),
        }
    }

    /// Path under which this store reads/writes its per-project JSONL files.
    /// The compaction trigger sidecar lives alongside it (ADR-0010 C5').
    pub fn storage_path(&self) -> &Path {
        &self.storage_path
    }

    /// Remove `id` from the in-memory map and record it as a deliberate
    /// deletion — in the per-project tombstone set for the next
    /// [`save`](Self::save), **and** as a durable `StoreTombstone` line in
    /// the project's JSONL so other processes' loads see the deletion
    /// without waiting for our rewrite. Returns `true` if the id was
    /// present.
    fn evict(&mut self, id: &str) -> bool {
        if let Some(entry) = self.entries.remove(id) {
            let at = Utc::now();
            self.tombstones
                .entry(entry.project.clone())
                .or_default()
                .insert(id.to_string(), at);
            self.append_tombstone_line(&entry.project, id, at);
            true
        } else {
            false
        }
    }

    /// Best-effort durable tombstone append (hot path, same atomicity
    /// argument as [`add`](Self::add)). A failed append is non-fatal: the
    /// in-memory tombstone still reaches disk at the next `save`.
    fn append_tombstone_line(&self, project: &str, id: &str, at: DateTime<Utc>) {
        let path = project_jsonl_path(&self.storage_path, project);
        let Ok(line) = serde_json::to_string(&StoreTombstone {
            tombstone: id.to_string(),
            at,
        }) else {
            return;
        };
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
            let _ = file.write_all(format!("{line}\n").as_bytes());
        }
    }

    /// Add a memory entry to the store.
    ///
    /// Appends one JSONL line to `{storage_path}/{project_hash}.jsonl`
    /// immediately (ADR-0010 D1 hot path) so the write is durable the instant
    /// it returns. Append is atomic at the OS level, so concurrent agents never
    /// lose writes; no flock is taken here. The in-memory map is updated in
    /// lockstep and stays the read-side source of truth.
    ///
    /// If an entry with the same ID already exists, the new line supersedes it
    /// on the next load (last-write-wins); the old line becomes a stale
    /// duplicate reclaimed by the compaction pass (ADR-0010 D4/C5').
    pub fn add(&mut self, entry: MemoryEntry) -> Result<(), MemoryError> {
        let path = project_jsonl_path(&self.storage_path, &entry.project);
        fs::create_dir_all(&self.storage_path)?;
        // Adding (or re-adding) this id cancels any prior deliberate deletion.
        if let Some(removed) = self.tombstones.get_mut(&entry.project) {
            removed.remove(&entry.id);
        }
        let line = serde_json::to_string(&entry)?;
        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
        // One write syscall: with O_APPEND a single write() of a small buffer
        // appends atomically, so concurrent agents never interleave a line with
        // a newline from another writer (which would corrupt both lines).
        file.write_all(format!("{line}\n").as_bytes())?;
        self.entries.insert(entry.id.clone(), entry);
        Ok(())
    }

    /// Find the id of an existing entry that [`add_or_update`](Self::add_or_update)
    /// would treat as the same fact: same project, same category, still
    /// valid (not bi-temporally expired), content overlap ≥
    /// [`DEDUP_SIMILARITY_THRESHOLD`].
    fn find_dedup_match(&self, entry: &MemoryEntry) -> Option<String> {
        let now = Utc::now();
        self.entries
            .values()
            .find(|e| {
                e.project == entry.project
                    && e.category == entry.category
                    && !e.is_expired(now)
                    && content_similarity(&e.content, &entry.content) >= DEDUP_SIMILARITY_THRESHOLD
            })
            .map(|e| e.id.clone())
    }

    /// Append `entry`, **unless** a near-duplicate already exists for the same
    /// project + category, in which case update that entry in place
    /// (ADR-0010 D4). This is the production write path; use [`add`](Self::add)
    /// for a raw append with no dedup (tests, the consolidator).
    ///
    /// "Update in place" reuses the matched entry's `id` so the freshly
    /// appended JSONL line supersedes the prior one on the next load
    /// (last-write-wins by id); the old line becomes a stale duplicate that
    /// the next compaction pass reclaims. The merged entry keeps the newer
    /// content, the higher confidence, the union of tags, the earliest
    /// `created_at`, and a refreshed `accessed_at`.
    /// Like [`add_or_update`](Self::add_or_update) but also returns the id of
    /// the entry that now holds the fact (the matched id on merge, the new
    /// id on insert). Used by the model-facing `MemorySave` tool.
    pub fn add_or_update_with_id(
        &mut self,
        entry: MemoryEntry,
    ) -> Result<(AddOutcome, String), MemoryError> {
        let matched = self.find_dedup_match(&entry);
        let final_id = matched.unwrap_or_else(|| entry.id.clone());
        let outcome = self.add_or_update(entry)?;
        Ok((outcome, final_id))
    }

    pub fn add_or_update(&mut self, mut entry: MemoryEntry) -> Result<AddOutcome, MemoryError> {
        // Choke-point redaction: every production write path (MemorySave tool,
        // /remember, auto-extraction, desktop CRUD) funnels through here, so
        // secret-shaped content is masked exactly once, before dedup.
        entry.content = redact_secrets(&entry.content);
        let matched_id = self.find_dedup_match(&entry);
        if let Some(id) = matched_id {
            if let Some(existing) = self.entries.get(&id).cloned() {
                entry.id = existing.id;
                entry.confidence = entry.confidence.max(existing.confidence);
                let mut tags = existing.tags.clone();
                for t in entry.tags.clone() {
                    if !tags.contains(&t) {
                        tags.push(t);
                    }
                }
                entry.tags = tags;
                entry.created_at = existing.created_at.min(entry.created_at);
                entry.accessed_at = Utc::now();
                entry.access_count = existing.access_count;
                self.add(entry)?;
                return Ok(AddOutcome::Updated);
            }
        }
        self.add(entry)?;
        Ok(AddOutcome::Inserted)
    }

    /// Prune `project`'s entries until [`format_for_injection`](Self::format_for_injection)
    /// fits within `budget_tokens`. Victims are **invalidated** (bi-temporal
    /// close), not destroyed — size control must not destroy data; expired
    /// entries remain on disk and `/recall --all` still finds them. Lowest-
    /// confidence entries first (ties broken by oldest `accessed_at`).
    /// Returns the number invalidated. Does not persist; the caller is
    /// expected to [`save`](Self::save) (ADR-0010 C5' size control).
    pub fn prune_to_token_budget(&mut self, project: &str, budget_tokens: usize) -> usize {
        // Victim ids in ascending value order: lowest confidence first, then
        // least-recently-accessed.
        let now = Utc::now();
        let mut victims: Vec<(String, f64, DateTime<Utc>)> = self
            .entries
            .values()
            .filter(|e| e.project == project && !e.is_expired(now))
            .map(|e| (e.id.clone(), e.confidence, e.accessed_at))
            .collect();
        victims.sort_by(|a, b| {
            a.1.partial_cmp(&b.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.2.cmp(&b.2))
        });

        let mut removed = 0;
        for (id, _, _) in &victims {
            let fits = self
                .format_for_injection(project, None)
                .map(|t| estimate_tokens(&t) <= budget_tokens)
                .unwrap_or(true);
            if fits {
                break;
            }
            if let Some(entry) = self.entries.get_mut(id) {
                entry.expire(Utc::now());
                removed += 1;
            }
        }
        removed
    }

    /// Retrieve a memory entry by ID.
    ///
    /// Returns `None` if no entry with the given ID exists.
    pub fn get(&self, id: &str) -> Option<&MemoryEntry> {
        self.entries.get(id)
    }

    /// Retrieve a mutable memory entry by ID and record an access.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut MemoryEntry> {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.touch();
        }
        self.entries.get_mut(id)
    }

    /// Search memories by substring match on content and tags.
    ///
    /// If `project` is provided, results are filtered to that project.
    /// Bi-temporally expired entries are excluded (see
    /// [`search_including_expired`](Self::search_including_expired)).
    /// Results are sorted by confidence descending.
    pub fn search(&self, query: &str, project: Option<&str>) -> Vec<MemoryEntry> {
        let now = Utc::now();
        let mut results: Vec<MemoryEntry> = self
            .entries
            .values()
            .filter(|e| {
                let project_match = project.is_none_or(|p| e.project == p);
                project_match && !e.is_expired(now) && e.matches_query(query)
            })
            .cloned()
            .collect();

        results.sort_by(|a, b| {
            let score_a = relevance_score(a);
            let score_b = relevance_score(b);
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results
    }

    /// Like [`search`](Self::search) but including bi-temporally expired
    /// entries (used by `/recall --all`).
    pub fn search_including_expired(&self, query: &str, project: Option<&str>) -> Vec<MemoryEntry> {
        let mut results: Vec<MemoryEntry> = self
            .entries
            .values()
            .filter(|e| {
                let project_match = project.is_none_or(|p| e.project == p);
                project_match && e.matches_query(query)
            })
            .cloned()
            .collect();

        results.sort_by(|a, b| {
            let score_a = relevance_score(a);
            let score_b = relevance_score(b);
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results
    }

    /// Get all memories belonging to a specific project.
    ///
    /// Results are sorted by creation date, most recent first. Bi-temporally
    /// expired entries are excluded — listing surfaces live facts; use
    /// [`project_memories_all`](Self::project_memories_all) for the full set.
    pub fn project_memories(&self, project: &str) -> Vec<MemoryEntry> {
        let now = Utc::now();
        let mut results: Vec<MemoryEntry> = self
            .entries
            .values()
            .filter(|e| e.project == project && !e.is_expired(now))
            .cloned()
            .collect();

        results.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        results
    }

    /// Like [`project_memories`](Self::project_memories) but including
    /// bi-temporally expired entries (used by delete paths so stale facts
    /// remain forgettable, and by `/recall --all`).
    pub fn project_memories_all(&self, project: &str) -> Vec<MemoryEntry> {
        let mut results: Vec<MemoryEntry> = self
            .entries
            .values()
            .filter(|e| e.project == project)
            .cloned()
            .collect();

        results.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        results
    }

    /// Format the active project's (plus global-scope) memories for
    /// system-prompt injection (ADR-0010 D2 scoped retrieval).
    ///
    /// Unlike [`search`](Self::search) (substring match, used by the REPL
    /// `/memory` command), this returns **every** live memory for `project`
    /// — content only, grouped by [`MemoryCategory`] — capped at
    /// `MAX_INJECTED_MEMORIES` entries plus up to `MAX_INJECTED_GLOBAL`
    /// cross-project ([`GLOBAL_SCOPE`]) entries, all inside a
    /// `MAX_INJECTED_TOKENS` budget measured with the CJK-aware
    /// `estimate_tokens`. When `query` is provided (the user's current
    /// prompt), candidates beyond the cap are ranked by
    /// `semantic_relevance_score` instead of raw recency, so the most
    /// relevant facts survive truncation. Returns `None` when there is
    /// nothing to inject.
    pub fn format_for_injection(&self, project: &str, query: Option<&str>) -> Option<String> {
        let now = Utc::now();
        let mut project_entries: Vec<MemoryEntry> = self
            .entries
            .values()
            .filter(|e| e.project == project && !e.is_expired(now))
            .cloned()
            .collect();
        let mut global_entries: Vec<MemoryEntry> = if project == GLOBAL_SCOPE {
            Vec::new()
        } else {
            self.entries
                .values()
                .filter(|e| e.project == GLOBAL_SCOPE && !e.is_expired(now))
                .cloned()
                .collect()
        };
        if project_entries.is_empty() && global_entries.is_empty() {
            return None;
        }

        if let Some(q) = query {
            let q_lower = q.to_lowercase();
            let terms: std::collections::HashSet<&str> = q_lower.split_whitespace().collect();
            let score = |e: &MemoryEntry| semantic_relevance_score(e, &terms);
            project_entries.sort_by(|a, b| {
                score(b)
                    .partial_cmp(&score(a))
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| b.created_at.cmp(&a.created_at))
            });
            global_entries.sort_by(|a, b| {
                score(b)
                    .partial_cmp(&score(a))
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| b.created_at.cmp(&a.created_at))
            });
        } else {
            project_entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            global_entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        }
        global_entries.truncate(MAX_INJECTED_GLOBAL);
        project_entries.truncate(MAX_INJECTED_MEMORIES);

        // Injection-time token budget (~`MAX_INJECTED_TOKENS`), measured with
        // the CJK-aware estimator — the previous chars/4 accounting let CJK
        // memories quadruple the intended prompt volume. Entries beyond the
        // budget are simply not injected — never deleted or expired here
        // (size control must not destroy data).
        let budget_tokens = MAX_INJECTED_TOKENS;
        let mut used = estimate_tokens("## Project Memories\n");
        let mut sections: std::collections::BTreeMap<
            &str,
            std::collections::BTreeMap<&MemoryCategory, Vec<&str>>,
        > = std::collections::BTreeMap::new();
        let mut omitted = 0usize;
        for (scope, entries) in [("Project", &project_entries), ("Global", &global_entries)] {
            let section = if scope == "Global" {
                "## Global Memories\n"
            } else {
                "## Project Memories\n"
            };
            for e in entries {
                let cost = estimate_tokens(&format!("- {}\n", e.content)) + 1;
                if used + cost > budget_tokens {
                    omitted += 1;
                    continue;
                }
                used += cost;
                sections
                    .entry(section)
                    .or_default()
                    .entry(&e.category)
                    .or_default()
                    .push(&e.content);
            }
        }
        let mut out = String::new();
        for (section, by_cat) in &sections {
            out.push_str(section);
            for (cat, contents) in by_cat {
                out.push_str(&format!("### {cat}\n"));
                for c in contents {
                    out.push_str(&format!("- {c}\n"));
                }
            }
        }
        if omitted > 0 {
            out.push_str(&format!(
                "\n({omitted} older memories not shown — use /recall to search the full store)\n"
            ));
        }
        Some(out)
    }

    /// Delete a memory entry by ID.
    ///
    /// Returns `Ok(true)` if the entry was found and removed, `Ok(false)` otherwise.
    pub fn delete(&mut self, id: &str) -> Result<bool, MemoryError> {
        Ok(self.evict(id))
    }

    /// Move an entry to another project, preserving its id, timestamps, and
    /// provenance. Returns the moved entry.
    ///
    /// This is the update path behind the desktop's `update_memory(project)`
    /// (decision 3-A). It deliberately does **not** go through
    /// [`delete`](Self::delete) + [`add`](Self::add): persistence is
    /// per-project JSONL, and a tombstone line left in the old project's file
    /// removes its id from the in-memory map **globally** on the next
    /// [`load`](Self::load) — file iteration order decides whether the
    /// re-added line in the new project's file is processed before or after
    /// that tombstone, so the moved entry would resurrect or vanish
    /// nondeterministically. Instead the move rewrites the old project's file
    /// without the entry (no tombstone: the id intentionally continues to
    /// live, elsewhere) and hot-appends the updated entry to the new
    /// project's file.
    pub fn move_entry(&mut self, id: &str, new_project: &str) -> Result<MemoryEntry, MemoryError> {
        let mut moved = self
            .entries
            .get(id)
            .cloned()
            .ok_or_else(|| MemoryError::NotFound(id.to_string()))?;
        if moved.project == new_project {
            return Ok(moved);
        }
        let old_project = moved.project.clone();
        moved.project = new_project.to_string();

        fs::create_dir_all(&self.storage_path)?;

        // Rewrite the old project's file minus the moved entry. Same cold-path
        // guarantees as `save`: sidecar flock + temp/atomic rename. Other
        // agents' entries and tombstone lines in the file are preserved.
        {
            let old_path = project_jsonl_path(&self.storage_path, &old_project);
            let _lock = acquire_exclusive_lock(&old_path)?;
            let (disk, disk_tombstones) = parse_jsonl_file(&old_path);
            let mut jsonl = String::new();
            for e in &disk {
                if e.id != id {
                    jsonl.push_str(&serde_json::to_string(e)?);
                    jsonl.push('\n');
                }
            }
            for t in &disk_tombstones {
                jsonl.push_str(&serde_json::to_string(t)?);
                jsonl.push('\n');
            }
            atomic_write(&old_path, &jsonl)?;
        }

        // Hot-append the moved entry under its new project (and clear any
        // in-memory tombstone recorded for the id in the old project — a
        // deliberate delete followed by a move of a since-re-added id).
        self.tombstones.entry(old_project).or_default().remove(id);
        self.add(moved.clone())?;
        Ok(moved)
    }

    /// Persist the in-memory view to disk by rewriting each project's JSONL.
    ///
    /// This is the **cold path** — the only writer that rewrites the whole
    /// file (used by compaction / cleanup / conflict-resolution). It takes an
    /// exclusive `flock` on a sidecar lockfile and writes via temp + atomic
    /// rename, so a crash mid-write cannot corrupt the store and concurrent
    /// compaction passes serialize (ADR-0010 D3, mirrors
    /// `provider_config_store`). Hot-path appends go through [`add`](Self::add)
    /// and do not rewrite.
    ///
    /// Under the lock the on-disk file is reloaded and reconciled with the
    /// in-memory view (ADR-0010 C5'): entries another agent appended since our
    /// [`load`](Self::load) are **preserved** (not clobbered), while ids in
    /// the per-project tombstone set (deliberate deletions) are **dropped**
    /// rather than resurrected from stale lines — unless the disk line is
    /// *newer* than our tombstone (another agent re-added the id after we
    /// deleted it), in which case the re-add wins and our tombstone is
    /// retired. Durable tombstone lines (ours and other agents') survive the
    /// rewrite so deletions stay visible to processes that have not reloaded;
    /// they are garbage-collected after `TOMBSTONE_TTL`.
    pub fn save(&mut self) -> Result<(), MemoryError> {
        fs::create_dir_all(&self.storage_path)?;

        // Group in-memory entries by project (clone out so we can mutate
        // `tombstones` inside the loop without borrowing `self`).
        let mut by_project: HashMap<String, Vec<MemoryEntry>> = HashMap::new();
        for entry in self.entries.values() {
            by_project
                .entry(entry.project.clone())
                .or_default()
                .push(entry.clone());
        }
        // Projects that only have tombstones (no live entries) still need a
        // rewrite so the deletion propagates.
        for project in self.tombstone_projects() {
            by_project.entry(project).or_default();
        }

        for (project, mem_entries) in by_project {
            let path = project_jsonl_path(&self.storage_path, &project);
            let _lock = acquire_exclusive_lock(&path)?;

            let (disk, disk_tombstones) = parse_jsonl_file(&path);
            let own_tombstones = self.tombstones.get(&project).cloned().unwrap_or_default();
            let mut retired_tombstones: HashSet<String> = HashSet::new();
            let mem_ids: HashSet<&String> = mem_entries.iter().map(|e| &e.id).collect();

            let mut reconciled: Vec<MemoryEntry> = Vec::new();
            let mut seen: HashSet<String> = HashSet::new();
            for e in disk {
                if mem_ids.contains(&e.id) {
                    continue; // our (possibly updated) version wins; emitted below
                }
                if let Some(deleted_at) = own_tombstones.get(&e.id) {
                    if *deleted_at >= e.accessed_at {
                        continue; // deliberate deletion — drop the stale line
                    }
                    // The disk line is newer than our deletion: another agent
                    // re-added this id. Keep the entry, retire the tombstone.
                    retired_tombstones.insert(e.id.clone());
                }
                // Another agent's append we don't know about — preserve it.
                seen.insert(e.id.clone());
                reconciled.push(e);
            }
            for e in &mem_entries {
                if seen.insert(e.id.clone()) {
                    reconciled.push(e.clone());
                }
            }
            // Deterministic order so re-saves produce stable diffs.
            reconciled.sort_by(|a, b| a.created_at.cmp(&b.created_at));

            // Preserve surviving tombstones (ours not yet applied + other
            // agents'), GC'd past the TTL so they don't accumulate forever.
            let gc_cutoff = Utc::now() - TOMBSTONE_TTL;
            let mut tombstone_lines: Vec<StoreTombstone> = disk_tombstones
                .iter()
                .filter(|t| t.at > gc_cutoff && !retired_tombstones.contains(&t.tombstone))
                .cloned()
                .collect();
            for (id, at) in &own_tombstones {
                if !retired_tombstones.contains(id) && *at > gc_cutoff {
                    tombstone_lines.push(StoreTombstone {
                        tombstone: id.clone(),
                        at: *at,
                    });
                }
            }

            let mut jsonl = String::new();
            for e in &reconciled {
                jsonl.push_str(&serde_json::to_string(e)?);
                jsonl.push('\n');
            }
            // Deduplicate tombstone ids (disk + own may overlap) before emitting.
            let mut emitted: HashSet<&str> = HashSet::new();
            for t in &tombstone_lines {
                if emitted.insert(t.tombstone.as_str()) {
                    jsonl.push_str(&serde_json::to_string(t)?);
                    jsonl.push('\n');
                }
            }
            atomic_write(&path, &jsonl)?;

            // Tombstones for this project are consumed (excluded from disk).
            if let Some(r) = self.tombstones.get_mut(&project) {
                for id in retired_tombstones {
                    r.remove(&id);
                }
                r.clear();
            }
        }

        Ok(())
    }

    /// Projects that currently have at least one tombstoned id (used by
    /// [`save`](Self::save) to ensure deletion-only projects still rewrite).
    fn tombstone_projects(&self) -> Vec<String> {
        self.tombstones
            .iter()
            .filter(|(_, ids)| !ids.is_empty())
            .map(|(p, _)| p.clone())
            .collect()
    }

    /// Load memories from disk.
    ///
    /// On first load across the JSONL boundary, each legacy
    /// `{project_hash}.json` array is rewritten as `{project_hash}.jsonl` and
    /// the `.json` set aside as `.json.migrated` (never read again — no
    /// read-compat tail; ADR-0010 D7). Then every `{project_hash}.jsonl` is
    /// streamed line-by-line into the in-memory store in **line order**, so
    /// the last writer per id wins: an entry line re-added after a
    /// `StoreTombstone` line revives the id, and a tombstone after an
    /// entry deletes it. This is what makes deletions durable across
    /// processes. A trailing partial line (a crash mid-append) is skipped +
    /// logged rather than failing the whole store (ADR-0010 D1 crash-safety).
    pub fn load(&mut self) -> Result<(), MemoryError> {
        fs::create_dir_all(&self.storage_path)?;
        if !self.storage_path.exists() {
            return Ok(());
        }

        // A fresh load has no deliberate deletions to carry forward, and the
        // in-memory view must be REBUILT from disk (M-4): insert-only loading
        // resurrected entries another process had deleted, which the next
        // local save() then re-persisted.
        self.tombstones.clear();
        self.entries.clear();

        // One-shot migration: legacy `<hash>.json` → `<hash>.jsonl`. Skipped
        // when the `.jsonl` already exists (already migrated, or written by a
        // newer build). Unparseable `.json` files are left untouched.
        for entry in fs::read_dir(&self.storage_path)? {
            let path = match entry {
                Ok(e) => e.path(),
                Err(_) => continue,
            };
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let jsonl_path = path.with_extension("jsonl");
            if jsonl_path.exists() {
                continue;
            }
            let Ok(contents) = fs::read_to_string(&path) else {
                continue;
            };
            let Ok(arr) = serde_json::from_str::<Vec<MemoryEntry>>(&contents) else {
                continue;
            };
            let jsonl: String = arr
                .iter()
                .map(|e| serde_json::to_string(e).map(|s| s + "\n"))
                .collect::<Result<String, _>>()?;
            if atomic_write(&jsonl_path, &jsonl).is_ok() {
                let migrated = {
                    let mut s = path.as_os_str().to_owned();
                    s.push(".migrated");
                    PathBuf::from(s)
                };
                let _ = fs::rename(&path, &migrated);
            }
        }

        // One-shot migration: legacy `DefaultHasher`-hashed `.jsonl` files →
        // stable-hash names (see `project_hash`). The hash is not
        // reversible, so the target name is recovered from the first
        // entry's `project` field inside each candidate file.
        {
            if let Ok(dir_entries) = fs::read_dir(&self.storage_path) {
                for dir_entry in dir_entries.flatten() {
                    let path = dir_entry.path();
                    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                        continue;
                    };
                    let Some(stem) = name.strip_suffix(".jsonl") else {
                        continue;
                    };
                    // Legacy and stable hashes are both 16 lowercase hex
                    // chars — a collision on the new scheme would mean the
                    // file is already migrated; only rename when the stem
                    // equals the LEGACY hash of that file's own project.
                    if stem.len() != 16 || !stem.chars().all(|c| c.is_ascii_hexdigit()) {
                        continue;
                    }
                    let Some(project) = parse_jsonl_file(&path)
                        .0
                        .into_iter()
                        .next()
                        .map(|f| f.project)
                    else {
                        continue;
                    };
                    if project_hash_legacy(&project) == stem && project_hash(&project) != stem {
                        let target = self
                            .storage_path
                            .join(format!("{}.jsonl", project_hash(&project)));
                        if !target.exists() {
                            let _ = fs::rename(&path, &target);
                        }
                    }
                }
            }
        }

        // Stream every `<hash>.jsonl`, one record per line, in line order
        // (last writer wins by id — how add_or_update's supersede, C4'
        // reclaim, and durable tombstones all work).
        for entry in fs::read_dir(&self.storage_path)? {
            let path = match entry {
                Ok(e) => e.path(),
                Err(_) => continue,
            };
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let (file_entries, file_tombstones) = parse_jsonl_file(&path);
            // zip the two streams back into line order via a merge: entries
            // and tombstones were collected in file order each; a simple
            // apply-all-entries-then-all-tombstones would mis-order. Instead
            // re-walk the file once, dispatching per line.
            if !file_tombstones.is_empty() {
                let Ok(file) = File::open(&path) else {
                    continue;
                };
                for line in BufReader::new(file).lines().map_while(Result::ok) {
                    if line.trim().is_empty() {
                        continue;
                    }
                    if line.contains("\"tombstone\"") {
                        if let Ok(t) = serde_json::from_str::<StoreTombstone>(&line) {
                            self.entries.remove(&t.tombstone);
                            self.tombstones
                                .entry(self.project_key_for_line(&t.tombstone, &file_entries))
                                .or_default()
                                .insert(t.tombstone.clone(), t.at);
                            continue;
                        }
                    }
                    if let Ok(mem) = serde_json::from_str::<MemoryEntry>(&line) {
                        self.tombstones
                            .entry(mem.project.clone())
                            .or_default()
                            .remove(&mem.id);
                        self.entries.insert(mem.id.clone(), mem);
                    }
                }
            } else {
                for mem in file_entries {
                    self.entries.insert(mem.id.clone(), mem);
                }
            }
        }

        Ok(())
    }

    /// Project key for a tombstone loaded from the file of `fallback_project`:
    /// the tombstone line itself doesn't carry the project, so we attribute it
    /// to the project of the deleted entry when we saw one, else the store
    /// file's dominant project (the first entry parsed from this file).
    fn project_key_for_line(&self, _id: &str, file_entries: &[MemoryEntry]) -> String {
        file_entries
            .first()
            .map(|e| e.project.clone())
            .unwrap_or_else(|| _id.to_string())
    }

    /// Remove old entries and cap the total count **for one project**.
    ///
    /// Entries of `project` older than `max_age` are bi-temporally
    /// invalidated (recoverable, excluded from injection); if the project
    /// still exceeds `max_entries` live entries, the least-recently-accessed
    /// ones are deleted outright. Other projects are never touched —
    /// `cleanup` used to sweep every project in the store and delete
    /// another project's history (review 2026-09 P0).
    ///
    /// Returns the total number of entries invalidated or removed.
    pub fn cleanup(
        &mut self,
        project: &str,
        max_age: Duration,
        max_entries: usize,
    ) -> Result<usize, MemoryError> {
        let cutoff = Utc::now() - max_age;
        let now = Utc::now();
        let mut affected = 0usize;

        // Invalidate entries older than max_age (scoped to `project`).
        let aged_out: Vec<String> = self
            .entries
            .values()
            .filter(|entry| {
                entry.project == project && !entry.is_expired(now) && entry.created_at <= cutoff
            })
            .map(|entry| entry.id.clone())
            .collect();
        for id in aged_out {
            if let Some(entry) = self.entries.get_mut(&id) {
                entry.expire(now);
                affected += 1;
            }
        }

        // If still over capacity, delete least-recently-accessed live entries.
        let live: Vec<(String, DateTime<Utc>)> = self
            .entries
            .values()
            .filter(|e| e.project == project && !e.is_expired(now))
            .map(|e| (e.id.clone(), e.accessed_at))
            .collect();
        if live.len() > max_entries {
            let mut access_times = live;
            access_times.sort_by_key(|(_, t)| *t);

            let to_remove = access_times.len() - max_entries;
            for (id, _) in access_times.into_iter().take(to_remove) {
                if self.evict(&id) {
                    affected += 1;
                }
            }
        }

        // Persist changes after cleanup
        self.save()?;

        Ok(affected)
    }

    /// Return the number of live (non-expired) entries currently in the
    /// store.
    pub fn len(&self) -> usize {
        let now = Utc::now();
        self.entries.values().filter(|e| !e.is_expired(now)).count()
    }

    /// Return the total number of entries including bi-temporally expired
    /// ones.
    pub fn total_len(&self) -> usize {
        self.entries.len()
    }

    /// Return true if the store contains no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Snapshot of every loaded entry, oldest first (ties broken by id for a
    /// stable order). P2-2 pack export walks this to serialize content fields;
    /// callers decide which fields to keep (packs never carry provenance
    /// session ids or project paths).
    pub fn all_entries(&self) -> Vec<MemoryEntry> {
        let mut out: Vec<MemoryEntry> = self.entries.values().cloned().collect();
        out.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        out
    }

    /// Get memories filtered by [`MemoryType`].
    ///
    /// Maps the `MemoryType` to the corresponding [`MemoryCategory`] and
    /// returns all entries matching that category.
    pub fn get_memories_by_type(&self, memory_type: &MemoryType) -> Vec<MemoryEntry> {
        let category: MemoryCategory = memory_type.clone().into();
        self.entries
            .values()
            .filter(|e| e.category == category)
            .cloned()
            .collect()
    }

    /// Consolidate memories: merge duplicates, invalidate stale entries,
    /// enforce caps — scoped to `project`.
    ///
    /// This is a convenience method that creates a default [`MemoryConsolidator`]
    /// and runs consolidation with the given config.
    pub fn consolidate_memories(
        &mut self,
        project: &str,
        config: &SessionMemoryConfig,
    ) -> Result<ConsolidationResult, MemoryError> {
        let consolidator = MemoryConsolidator::default();
        consolidator.consolidate(self, project, config)
    }

    /// Auto-extract memories from a list of message summaries.
    ///
    /// Uses pattern matching to detect preferences, decisions, errors, and
    /// conventions in the provided messages, returning newly extracted
    /// [`MemoryEntry`] instances.
    pub fn auto_extract_from_messages(
        &self,
        messages: &[crate::extract_memories::MessageSummary],
        config: &SessionMemoryConfig,
    ) -> Vec<MemoryEntry> {
        if !config.auto_extract_enabled {
            return Vec::new();
        }

        let mut memories = Vec::new();

        for msg in messages {
            let lower = msg.content.to_lowercase();

            // --- UserPreference detection ---
            for kw in &[
                "i always",
                "i never",
                "i prefer",
                "please always",
                "please never",
                "don't use",
                "do not use",
            ] {
                if lower.contains(kw) {
                    memories.push(
                        MemoryEntry::with_confidence(
                            "auto",
                            MemoryCategory::Preference,
                            &msg.content,
                            0.7,
                            vec!["auto-extracted".to_string(), "preference".to_string()],
                        )
                        .unwrap_or_else(|_| {
                            MemoryEntry::new("auto", MemoryCategory::Preference, &msg.content)
                        }),
                    );
                    break;
                }
            }

            // --- ProjectConvention detection ---
            for kw in &[
                "in this project we",
                "our convention",
                "naming convention",
                "the standard approach",
            ] {
                if lower.contains(kw) {
                    memories.push(
                        MemoryEntry::with_confidence(
                            "auto",
                            MemoryCategory::Pattern,
                            &msg.content,
                            0.7,
                            vec!["auto-extracted".to_string(), "convention".to_string()],
                        )
                        .unwrap_or_else(|_| {
                            MemoryEntry::new("auto", MemoryCategory::Pattern, &msg.content)
                        }),
                    );
                    break;
                }
            }

            // --- TechnicalDecision detection ---
            for kw in &[
                "we decided",
                "let's use",
                "going with",
                "the decision",
                "decided to",
            ] {
                if lower.contains(kw) {
                    memories.push(
                        MemoryEntry::with_confidence(
                            "auto",
                            MemoryCategory::Decision,
                            &msg.content,
                            0.7,
                            vec!["auto-extracted".to_string(), "decision".to_string()],
                        )
                        .unwrap_or_else(|_| {
                            MemoryEntry::new("auto", MemoryCategory::Decision, &msg.content)
                        }),
                    );
                    break;
                }
            }

            // --- DebuggingInsight detection ---
            for kw in &[
                "the issue was",
                "the error was",
                "the fix was",
                "root cause",
                "the workaround",
            ] {
                if lower.contains(kw) {
                    memories.push(
                        MemoryEntry::with_confidence(
                            "auto",
                            MemoryCategory::Error,
                            &msg.content,
                            0.7,
                            vec!["auto-extracted".to_string(), "debugging".to_string()],
                        )
                        .unwrap_or_else(|_| {
                            MemoryEntry::new("auto", MemoryCategory::Error, &msg.content)
                        }),
                    );
                    break;
                }
            }
        }

        // Deduplicate by content similarity
        deduplicate_memories(memories)
    }

    /// Merge duplicate memories based on Jaccard similarity, **scoped to one
    /// project**. Merging used to sweep every project in the store, so
    /// compacting project A could delete project B's similar-but-distinct
    /// entries (review 2026-09 P0).
    ///
    /// When two live entries have similarity above the threshold, the one
    /// with the higher source trust (manual > import > auto-extract), then
    /// the higher confidence, is kept and the other is deleted. Expired
    /// entries never participate.
    /// Returns the number of duplicates removed.
    pub fn merge_duplicates(
        &mut self,
        similarity_threshold: f64,
        project: &str,
    ) -> Result<usize, MemoryError> {
        let now = Utc::now();
        let mut to_remove: Vec<String> = Vec::new();
        let ids: Vec<String> = self
            .entries
            .values()
            .filter(|e| e.project == project && !e.is_expired(now))
            .map(|e| e.id.clone())
            .collect();

        for i in 0..ids.len() {
            if to_remove.contains(&ids[i]) {
                continue;
            }
            for j in (i + 1)..ids.len() {
                if to_remove.contains(&ids[j]) {
                    continue;
                }
                let entry_i = &self.entries[&ids[i]];
                let entry_j = &self.entries[&ids[j]];

                if entry_i.category == entry_j.category
                    && content_similarity(&entry_i.content, &entry_j.content) > similarity_threshold
                {
                    // Remove the lower-value one: source trust first (a
                    // hand-saved fact must not be eaten by an auto-extracted
                    // paraphrase), then confidence.
                    let keep_j = (trust_rank(entry_j), entry_j.confidence)
                        > (trust_rank(entry_i), entry_i.confidence);
                    let remove_id = if keep_j { &ids[i] } else { &ids[j] };
                    to_remove.push(remove_id.clone());
                }
            }
        }

        for id in &to_remove {
            self.evict(id);
        }

        Ok(to_remove.len())
    }

    /// Invalidate entries of **one project** older than the given TTL
    /// (bi-temporal close — they stay on disk and `/recall --all` finds
    /// them, but never reach injection again). Returns the number
    /// invalidated.
    pub fn remove_stale(&mut self, ttl: Duration, project: &str) -> Result<usize, MemoryError> {
        let cutoff = Utc::now() - ttl;
        let now = Utc::now();

        let stale: Vec<String> = self
            .entries
            .values()
            .filter(|entry| {
                entry.project == project && !entry.is_expired(now) && entry.created_at <= cutoff
            })
            .map(|entry| entry.id.clone())
            .collect();
        for id in &stale {
            if let Some(entry) = self.entries.get_mut(id) {
                entry.expire(now);
            }
        }

        Ok(stale.len())
    }

    /// Enforce per-category caps by removing the least-accessed **live**
    /// entries of one project. Expired entries are skipped (they are already
    /// out of injection).
    pub fn enforce_category_caps(&mut self, max_per_category: usize, project: &str) {
        let now = Utc::now();
        let mut by_category: HashMap<MemoryCategory, Vec<String>> = HashMap::new();

        for (id, entry) in &self.entries {
            if entry.project == project && !entry.is_expired(now) {
                by_category
                    .entry(entry.category.clone())
                    .or_default()
                    .push(id.clone());
            }
        }

        for (_category, mut ids) in by_category {
            if ids.len() <= max_per_category {
                continue;
            }

            // Sort by access count ascending, then by accessed_at ascending
            ids.sort_by(|a, b| {
                let entry_a = &self.entries[a];
                let entry_b = &self.entries[b];
                entry_a
                    .access_count
                    .cmp(&entry_b.access_count)
                    .then_with(|| entry_a.accessed_at.cmp(&entry_b.accessed_at))
            });

            let to_remove = ids.len() - max_per_category;
            for id in ids.into_iter().take(to_remove) {
                self.evict(&id);
            }
        }
    }

    /// Search memories ranked by multi-signal relevance to the query.
    ///
    /// Unlike `search` which requires a substring match, this method scores
    /// every memory against the query using term overlap, category affinity,
    /// temporal decay, confidence, and access frequency. Results are returned
    /// in descending relevance order.
    pub fn search_by_relevance(
        &self,
        query: &str,
        project: Option<&str>,
        max_results: usize,
    ) -> Vec<MemoryEntry> {
        let query_lower = query.to_lowercase();
        let query_terms: std::collections::HashSet<&str> = query_lower.split_whitespace().collect();

        let mut scored: Vec<(f64, MemoryEntry)> = self
            .entries
            .values()
            .filter(|e| project.is_none_or(|p| e.project == p))
            .filter_map(|e| {
                let score = semantic_relevance_score(e, &query_terms);
                // Only include results above a minimal threshold
                if score > 0.05 {
                    Some((score, e.clone()))
                } else {
                    None
                }
            })
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(max_results);
        scored.into_iter().map(|(_, e)| e).collect()
    }

    /// Detect and resolve contradictory memories **within one project**.
    ///
    /// Finds pairs of live memories in the same category that express
    /// opposing sentiments (e.g., "always use X" vs "never use X"). The
    /// older memory is bi-temporally invalidated — it stays on disk for
    /// audit (`/recall --all`) but stops being injected; the newer fact
    /// wins. Wired into the compaction pass so contradictions between
    /// injected memories are actually resolved (previously this existed
    /// with no production caller). Returns the number of conflicts
    /// resolved.
    pub fn resolve_conflicts(&mut self, project: &str) -> Result<usize, MemoryError> {
        let now = Utc::now();
        let mut to_expire: Vec<String> = Vec::new();

        let ids: Vec<String> = self
            .entries
            .values()
            .filter(|e| e.project == project && !e.is_expired(now))
            .map(|e| e.id.clone())
            .collect();

        for i in 0..ids.len() {
            if to_expire.contains(&ids[i]) {
                continue;
            }
            for j in (i + 1)..ids.len() {
                if to_expire.contains(&ids[j]) {
                    continue;
                }

                let entry_i = &self.entries[&ids[i]];
                let entry_j = &self.entries[&ids[j]];

                if entry_i.category == entry_j.category
                    && are_contradictory(&entry_i.content, &entry_j.content)
                {
                    // Invalidate the older one
                    let stale_id = if entry_i.created_at < entry_j.created_at {
                        &ids[i]
                    } else {
                        &ids[j]
                    };
                    to_expire.push(stale_id.clone());
                }
            }
        }

        let count = to_expire.len();
        for id in &to_expire {
            if let Some(entry) = self.entries.get_mut(id) {
                entry.expire(now);
            }
        }

        Ok(count)
    }

    /// Health statistics for the `/memory doctor` report: live vs expired
    /// counts, per-category distribution, and a near-duplicate pair count
    /// (same project+category, Jaccard ≥ 0.8 but not yet merged).
    pub fn doctor_stats(&self, project: Option<&str>) -> MemoryDoctorStats {
        let now = Utc::now();
        let mut stats = MemoryDoctorStats::default();
        let mut live: Vec<&MemoryEntry> = Vec::new();

        for entry in self.entries.values() {
            if project.is_some_and(|p| entry.project != p) {
                continue;
            }
            stats.total += 1;
            if entry.is_expired(now) {
                stats.expired += 1;
            } else {
                live.push(entry);
                *stats.by_category.entry(entry.category.clone()).or_default() += 1;
            }
        }

        for i in 0..live.len() {
            for j in (i + 1)..live.len() {
                if live[i].category == live[j].category
                    && live[i].project == live[j].project
                    && content_similarity(&live[i].content, &live[j].content)
                        >= DEDUP_SIMILARITY_THRESHOLD
                {
                    stats.near_duplicate_pairs += 1;
                }
            }
        }

        stats
    }
}

/// Health snapshot produced by [`MemoryStore::doctor_stats`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryDoctorStats {
    /// All entries in scope, including expired ones.
    pub total: usize,
    /// Entries bi-temporally invalidated (excluded from injection).
    pub expired: usize,
    /// Live entries per category.
    pub by_category: HashMap<MemoryCategory, usize>,
    /// Live same-project, same-category pairs above the dedup threshold
    /// that a compaction pass should merge.
    pub near_duplicate_pairs: usize,
}

/// Source-trust rank used when two near-duplicates compete: a hand-saved
/// fact must not be eaten by an auto-extracted paraphrase of it.
fn trust_rank(entry: &MemoryEntry) -> u8 {
    match entry.source_kind.as_deref() {
        Some(MemoryEntry::SOURCE_MANUAL) => 3,
        Some(MemoryEntry::SOURCE_IMPORT) => 2,
        Some(MemoryEntry::SOURCE_AUTO_EXTRACT) => 1,
        _ => 0,
    }
}

// Deduplicate memories by removing entries with very similar content.
fn deduplicate_memories(memories: Vec<MemoryEntry>) -> Vec<MemoryEntry> {
    let mut unique: Vec<MemoryEntry> = Vec::new();

    for memory in memories {
        let is_dup = unique
            .iter()
            .any(|existing| content_similarity(&existing.content, &memory.content) > 0.8);

        if !is_dup {
            unique.push(memory);
        }
    }

    unique
}

/// Compute a composite relevance score for a memory entry (used by `search`).
///
/// Combines confidence (40%), access frequency (30%), and recency (30%).
fn relevance_score(entry: &MemoryEntry) -> f64 {
    let confidence = entry.confidence;
    let access = (entry.access_count as f64).ln_1p() / 5.0_f64.ln_1p().max(0.01);
    let age_hours = (Utc::now() - entry.accessed_at).num_hours().max(0) as f64;
    let recency = 1.0 / (1.0 + age_hours / 168.0);
    0.4 * confidence + 0.3 * access.min(1.0) + 0.3 * recency
}

/// Compute a semantic relevance score combining query-term overlap with
/// temporal decay, confidence, and access frequency.
///
/// Weight breakdown:
/// - 35% query term overlap (TF overlap between query and memory content)
/// - 25% temporal decay (half-life of 2 weeks)
/// - 25% confidence
/// - 15% access frequency
fn semantic_relevance_score(
    entry: &MemoryEntry,
    query_terms: &std::collections::HashSet<&str>,
) -> f64 {
    if query_terms.is_empty() {
        return relevance_score(entry);
    }

    // Term overlap: fraction of query terms found in content or tags
    let content_lower: String = entry.content.to_lowercase();
    let tag_text: String = entry
        .tags
        .iter()
        .map(|t| t.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    let combined = format!("{content_lower} {tag_text}");
    let content_terms: std::collections::HashSet<&str> = combined.split_whitespace().collect();

    let overlap =
        query_terms.intersection(&content_terms).count() as f64 / query_terms.len() as f64;

    // Temporal decay: half-life of 2 weeks (336 hours)
    let age_hours = (Utc::now() - entry.created_at).num_hours().max(0) as f64;
    let decay = 1.0 / (1.0 + age_hours / 336.0);

    // Access frequency (logarithmic normalization)
    let access = (entry.access_count as f64).ln_1p() / 10.0_f64.ln_1p().max(0.01);

    0.35 * overlap + 0.25 * decay + 0.25 * entry.confidence + 0.15 * access.min(1.0)
}

/// Detect whether two memory contents express contradictory sentiments.
///
/// Looks for opposing signal words (always/never, do/don't, etc.) while
/// requiring sufficient content overlap to ensure the memories are about
/// the same topic.
fn are_contradictory(a: &str, b: &str) -> bool {
    let a_lower = a.to_lowercase();
    let b_lower = b.to_lowercase();

    // Must share at least moderate content overlap (same topic)
    let overlap = content_similarity(&a_lower, &b_lower);
    if overlap < 0.3 {
        return false;
    }

    // Opposing signal pairs
    const OPPOSING: &[(&str, &str)] = &[
        ("always", "never"),
        ("must", "must not"),
        ("should", "should not"),
        ("use", "don't use"),
        ("use", "do not use"),
        ("enable", "disable"),
        ("prefer", "avoid"),
        ("include", "exclude"),
        ("allow", "deny"),
        ("required", "forbidden"),
    ];

    for (pos, neg) in OPPOSING {
        let a_pos = a_lower.contains(pos) && !a_lower.contains(neg);
        let a_neg = a_lower.contains(neg) && !a_lower.contains(pos);
        let b_pos = b_lower.contains(pos) && !b_lower.contains(neg);
        let b_neg = b_lower.contains(neg) && !b_lower.contains(pos);

        if (a_pos && b_neg) || (a_neg && b_pos) {
            return true;
        }
    }

    false
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::extract_memories::MessageSummary;
    use tempfile::TempDir;

    fn make_entry(project: &str, category: MemoryCategory, content: &str) -> MemoryEntry {
        MemoryEntry::new(project, category, content)
    }

    fn make_entry_with_confidence(
        project: &str,
        category: MemoryCategory,
        content: &str,
        confidence: f64,
    ) -> MemoryEntry {
        MemoryEntry::with_confidence(project, category, content, confidence, vec![]).unwrap()
    }

    #[allow(clippy::too_many_arguments)]
    fn make_entry_with_timestamps(
        id: &str,
        project: &str,
        category: MemoryCategory,
        content: &str,
        confidence: f64,
        created_at: DateTime<Utc>,
        accessed_at: DateTime<Utc>,
        access_count: u32,
    ) -> MemoryEntry {
        MemoryEntry {
            id: id.to_string(),
            project: project.to_string(),
            category,
            content: content.to_string(),
            tags: vec![],
            confidence,
            created_at,
            accessed_at,
            access_count,
            source_session_id: None,
            source_kind: None,
            valid_until: None,
        }
    }

    // --- Add / Get / Delete ---

    #[test]
    fn test_add_and_get() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let entry = make_entry("proj", MemoryCategory::Preference, "Use tabs");
        let id = entry.id.clone();
        store.add(entry).unwrap();
        let retrieved = store.get(&id).unwrap();
        assert_eq!(retrieved.content, "Use tabs");
        assert_eq!(retrieved.project, "proj");
    }

    #[test]
    fn test_get_nonexistent_returns_none() {
        let dir = TempDir::new().unwrap();
        let store = MemoryStore::new(dir.path().to_path_buf());
        assert!(store.get("no-such-id").is_none());
    }

    #[test]
    fn test_delete_existing() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let entry = make_entry("p", MemoryCategory::Pattern, "data");
        let id = entry.id.clone();
        store.add(entry).unwrap();
        assert!(store.delete(&id).unwrap());
        assert!(store.get(&id).is_none());
    }

    #[test]
    fn test_delete_nonexistent_returns_false() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        assert!(!store.delete("ghost").unwrap());
    }

    // --- Search ---

    #[test]
    fn test_search_content_match() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry(
                "p",
                MemoryCategory::Preference,
                "I prefer dark mode",
            ))
            .unwrap();
        store
            .add(make_entry("p", MemoryCategory::Decision, "Use PostgreSQL"))
            .unwrap();
        let results = store.search("dark mode", None);
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("dark mode"));
    }

    #[test]
    fn test_search_tag_match() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let mut entry = make_entry("p", MemoryCategory::Pattern, "some content");
        entry.tags.push("rust-pattern".to_string());
        store.add(entry).unwrap();
        assert_eq!(store.search("rust-pattern", None).len(), 1);
    }

    #[test]
    fn test_search_case_insensitive() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry("p", MemoryCategory::Decision, "Use PostgreSQL"))
            .unwrap();
        assert_eq!(store.search("postgresql", None).len(), 1);
        assert_eq!(store.search("POSTGRESQL", None).len(), 1);
    }

    #[test]
    fn test_search_with_project_filter() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry("proj-a", MemoryCategory::Preference, "Use tabs"))
            .unwrap();
        store
            .add(make_entry(
                "proj-b",
                MemoryCategory::Preference,
                "Use spaces",
            ))
            .unwrap();
        let results = store.search("use", Some("proj-a"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].project, "proj-a");
    }

    #[test]
    fn test_search_no_match_returns_empty() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry("p", MemoryCategory::Context, "hello"))
            .unwrap();
        assert!(store.search("xyz", None).is_empty());
    }

    #[test]
    fn test_search_sorted_by_relevance() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let mut high =
            make_entry_with_confidence("p", MemoryCategory::Preference, "test query", 0.95);
        high.access_count = 10;
        high.touch();
        let low =
            make_entry_with_confidence("p", MemoryCategory::Preference, "test query other", 0.5);
        store.add(high).unwrap();
        store.add(low).unwrap();
        let results = store.search("test query", None);
        assert_eq!(results.len(), 2);
        assert!(results[0].confidence > results[1].confidence);
    }

    // --- project_memories ---

    #[test]
    fn test_project_memories_filters_by_project() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry("proj-a", MemoryCategory::Context, "a1"))
            .unwrap();
        store
            .add(make_entry("proj-b", MemoryCategory::Context, "b1"))
            .unwrap();
        store
            .add(make_entry("proj-a", MemoryCategory::Context, "a2"))
            .unwrap();
        let results = store.project_memories("proj-a");
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|e| e.project == "proj-a"));
    }

    #[test]
    fn test_project_memories_sorted_by_created_at_desc() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let older = make_entry_with_timestamps(
            "p1",
            "p",
            MemoryCategory::Context,
            "old",
            1.0,
            Utc::now() - Duration::hours(2),
            Utc::now(),
            0,
        );
        let newer = make_entry_with_timestamps(
            "p2",
            "p",
            MemoryCategory::Context,
            "new",
            1.0,
            Utc::now(),
            Utc::now(),
            0,
        );
        store.add(older).unwrap();
        store.add(newer).unwrap();
        let results = store.project_memories("p");
        assert_eq!(results.len(), 2);
        assert!(results[0].created_at >= results[1].created_at);
    }

    // --- Save + Load roundtrip ---

    #[test]
    fn test_save_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let e1 = make_entry("proj-x", MemoryCategory::Preference, "Use tabs");
        let e2 = make_entry("proj-x", MemoryCategory::Decision, "Use Rust");
        let id1 = e1.id.clone();
        let id2 = e2.id.clone();
        store.add(e1).unwrap();
        store.add(e2).unwrap();
        store.save().unwrap();

        let mut store2 = MemoryStore::new(dir.path().to_path_buf());
        store2.load().unwrap();
        assert_eq!(store2.len(), 2);
        assert_eq!(store2.get(&id1).unwrap().content, "Use tabs");
        assert_eq!(store2.get(&id2).unwrap().content, "Use Rust");
    }

    #[test]
    fn test_save_load_multiple_projects() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let e1 = make_entry("proj-a", MemoryCategory::Context, "A content");
        let e2 = make_entry("proj-b", MemoryCategory::Context, "B content");
        let id1 = e1.id.clone();
        let id2 = e2.id.clone();
        store.add(e1).unwrap();
        store.add(e2).unwrap();
        store.save().unwrap();

        let mut store2 = MemoryStore::new(dir.path().to_path_buf());
        store2.load().unwrap();
        assert_eq!(store2.len(), 2);
        assert_eq!(store2.get(&id1).unwrap().project, "proj-a");
        assert_eq!(store2.get(&id2).unwrap().project, "proj-b");
    }

    #[test]
    fn test_load_empty_directory() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store.load().unwrap();
        assert!(store.is_empty());
    }

    #[test]
    fn test_load_skips_non_json_files() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("readme.txt"), "not json").unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store.load().unwrap();
        assert!(store.is_empty());
    }

    // --- JSONL format + migration (ADR-0010 C2') ---

    #[test]
    fn test_add_appends_one_jsonl_line() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let entry = make_entry("proj", MemoryCategory::Preference, "Use tabs");
        let expected = serde_json::to_string(&entry).unwrap();
        store.add(entry).unwrap();
        let path = dir.path().join(format!("{}.jsonl", project_hash("proj")));
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, format!("{expected}\n"));
    }

    #[test]
    fn test_save_writes_jsonl_not_json() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry("proj", MemoryCategory::Decision, "Use Rust"))
            .unwrap();
        store.save().unwrap();
        let jsonl = dir.path().join(format!("{}.jsonl", project_hash("proj")));
        let json = dir.path().join(format!("{}.json", project_hash("proj")));
        assert!(jsonl.exists(), ".jsonl should exist after save");
        assert!(!json.exists(), "legacy .json must not be written");
        let lines = std::fs::read_to_string(&jsonl).unwrap();
        assert_eq!(lines.lines().count(), 1);
    }

    #[test]
    fn test_load_migrates_legacy_json_to_jsonl() {
        let dir = TempDir::new().unwrap();
        let entries = vec![
            make_entry("proj", MemoryCategory::Preference, "legacy pref"),
            make_entry("proj", MemoryCategory::Decision, "legacy dec"),
        ];
        let json_path = dir.path().join(format!("{}.json", project_hash("proj")));
        std::fs::write(&json_path, serde_json::to_string_pretty(&entries).unwrap()).unwrap();

        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store.load().unwrap();

        assert_eq!(store.len(), 2, "both legacy entries load post-migration");
        let jsonl = dir.path().join(format!("{}.jsonl", project_hash("proj")));
        assert!(jsonl.exists(), "migration produces a .jsonl");
        assert!(
            !json_path.exists(),
            "legacy .json is renamed away (no read-compat)"
        );
        let migrated = {
            let mut s = json_path.clone().into_os_string();
            s.push(".migrated");
            std::path::PathBuf::from(s)
        };
        assert!(
            migrated.exists(),
            "legacy .json backed up as .json.migrated"
        );
    }

    #[test]
    fn test_load_skips_partial_trailing_line() {
        let dir = TempDir::new().unwrap();
        let valid = make_entry("proj", MemoryCategory::Context, "good entry");
        let valid_line = serde_json::to_string(&valid).unwrap();
        // A valid line followed by a truncated (partial) line — the on-disk
        // state after a crash mid-append.
        let content = format!(
            "{valid_line}\n{{\"id\":\"broken\",\"project\":\"proj\",\"category\":\"Context\""
        );
        let path = dir.path().join(format!("{}.jsonl", project_hash("proj")));
        std::fs::write(&path, &content).unwrap();

        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store.load().unwrap();

        assert_eq!(store.len(), 1, "only the complete line loads");
        assert_eq!(
            store.get(&valid.id).map(|e| e.content.as_str()),
            Some("good entry")
        );
    }

    #[test]
    fn test_concurrent_appends_lose_nothing() {
        // Two independent stores (two agents) append to the same project file
        // concurrently. Append-only storage must durably keep every write.
        let dir = TempDir::new().unwrap();
        let dir_path = std::sync::Arc::new(dir.path().to_path_buf());

        let d1 = dir_path.clone();
        let h1 = std::thread::spawn(move || {
            let mut s = MemoryStore::new((*d1).clone());
            for i in 0..20 {
                s.add(make_entry(
                    "proj",
                    MemoryCategory::Context,
                    &format!("A-{i}"),
                ))
                .unwrap();
            }
        });
        let d2 = dir_path.clone();
        let h2 = std::thread::spawn(move || {
            let mut s = MemoryStore::new((*d2).clone());
            for i in 0..20 {
                s.add(make_entry(
                    "proj",
                    MemoryCategory::Context,
                    &format!("B-{i}"),
                ))
                .unwrap();
            }
        });
        h1.join().unwrap();
        h2.join().unwrap();

        let mut store = MemoryStore::new(dir_path.as_ref().clone());
        store.load().unwrap();
        assert_eq!(store.len(), 40, "all concurrent appends survive");
    }

    // --- Scoped injection (ADR-0010 C3') ---

    #[test]
    fn test_format_for_injection_none_when_empty() {
        let dir = TempDir::new().unwrap();
        let store = MemoryStore::new(dir.path().to_path_buf());
        assert!(
            store
                .format_for_injection("no-such-project", None)
                .is_none()
        );
    }

    #[test]
    fn test_format_for_injection_groups_by_category_content_only() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry_with_confidence(
                "proj",
                MemoryCategory::Preference,
                "use tabs",
                0.3,
            ))
            .unwrap();
        store
            .add(make_entry_with_confidence(
                "proj",
                MemoryCategory::Preference,
                "dark mode",
                0.9,
            ))
            .unwrap();
        store
            .add(make_entry_with_confidence(
                "proj",
                MemoryCategory::Decision,
                "use rust",
                0.95,
            ))
            .unwrap();

        let out = store.format_for_injection("proj", None).unwrap();
        // Header + both categories present, content included ...
        assert!(out.starts_with("## Project Memories\n"));
        assert!(out.contains("### preference\n"));
        assert!(out.contains("### decision\n"));
        assert!(out.contains("- use tabs\n"));
        assert!(out.contains("- dark mode\n"));
        assert!(out.contains("- use rust\n"));
        // ... confidence NOT injected (ADR-0010 D2: content only).
        assert!(!out.contains("confidence"));
    }

    #[test]
    fn test_format_for_injection_scoped_to_project() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry("proj-a", MemoryCategory::Context, "from a"))
            .unwrap();
        store
            .add(make_entry("proj-b", MemoryCategory::Context, "from b"))
            .unwrap();
        let out = store.format_for_injection("proj-a", None).unwrap();
        assert!(out.contains("from a"));
        assert!(!out.contains("from b"));
    }

    #[test]
    fn test_format_for_injection_caps_at_fifty() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        for i in 0..120 {
            store
                .add(make_entry(
                    "proj",
                    MemoryCategory::Context,
                    &format!("e{i}"),
                ))
                .unwrap();
        }
        let out = store.format_for_injection("proj", None).unwrap();
        // 120 stored, but injection capped at MAX_INJECTED_MEMORIES (50).
        assert_eq!(out.lines().filter(|l| l.starts_with("- ")).count(), 50);
    }

    // --- Write-time dedup (ADR-0010 C4') ---

    #[test]
    fn test_add_or_update_inserts_when_no_match() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add_or_update(make_entry(
                "p",
                MemoryCategory::Preference,
                "use tabs for indentation",
            ))
            .unwrap();
        let outcome = store
            .add_or_update(make_entry(
                "p",
                MemoryCategory::Decision,
                "use postgres for the database",
            ))
            .unwrap();
        assert_eq!(outcome, AddOutcome::Inserted);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn test_add_or_update_updates_near_duplicate() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let first = make_entry_with_confidence(
            "p",
            MemoryCategory::Preference,
            "always use tabs for indentation",
            0.6,
        );
        let first_id = first.id.clone();
        store.add_or_update(first).unwrap();
        // Near-duplicate: same project + category, Jaccard overlap 5/6 = 0.83.
        let outcome = store
            .add_or_update(make_entry_with_confidence(
                "p",
                MemoryCategory::Preference,
                "always use tabs for indentation rust",
                0.9,
            ))
            .unwrap();
        assert_eq!(outcome, AddOutcome::Updated);
        assert_eq!(store.len(), 1, "near-dup updates rather than appends");
        let survivor = store.get(&first_id).unwrap();
        assert_eq!(survivor.id, first_id, "existing id reused (supersede)");
        assert!(
            survivor.content.contains("rust"),
            "newer content wins on update"
        );
        assert!(
            (survivor.confidence - 0.9).abs() < f64::EPSILON,
            "higher confidence kept"
        );
    }

    #[test]
    fn test_add_or_update_different_category_inserts_both() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add_or_update(make_entry(
                "p",
                MemoryCategory::Preference,
                "always use tabs for indentation",
            ))
            .unwrap();
        let outcome = store
            .add_or_update(make_entry(
                "p",
                MemoryCategory::Decision,
                "always use tabs for indentation",
            ))
            .unwrap();
        assert_eq!(outcome, AddOutcome::Inserted);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn test_add_or_update_different_project_inserts_both() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add_or_update(make_entry(
                "proj-a",
                MemoryCategory::Preference,
                "always use tabs for indentation",
            ))
            .unwrap();
        let outcome = store
            .add_or_update(make_entry(
                "proj-b",
                MemoryCategory::Preference,
                "always use tabs for indentation",
            ))
            .unwrap();
        assert_eq!(outcome, AddOutcome::Inserted);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn test_add_or_update_stale_line_reclaimed_on_save() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add_or_update(make_entry(
                "p",
                MemoryCategory::Preference,
                "always use tabs for indentation",
            ))
            .unwrap();
        store
            .add_or_update(make_entry_with_confidence(
                "p",
                MemoryCategory::Preference,
                "always use tabs for indentation rust",
                0.9,
            ))
            .unwrap();
        let path = dir.path().join(format!("{}.jsonl", project_hash("p")));
        // Before compaction: the stale prior line is still on disk.
        assert_eq!(
            std::fs::read_to_string(&path).unwrap().lines().count(),
            2,
            "update appended a superseding line; old line still present"
        );
        // Compaction rewrites from the deduped in-memory map → stale line gone.
        store.save().unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap().lines().count(),
            1,
            "stale duplicate line reclaimed by save"
        );
    }

    // --- save() reload-reconcile (ADR-0010 C5' multi-agent safety) ---

    #[test]
    fn test_save_preserves_other_agent_appends() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let ours = make_entry("p", MemoryCategory::Context, "our fact");
        let our_id = ours.id.clone();
        store.add(ours).unwrap();
        let path = dir.path().join(format!("{}.jsonl", project_hash("p")));

        // Another agent appends a line this store never loaded.
        let theirs = make_entry("p", MemoryCategory::Context, "their fact");
        let their_id = theirs.id.clone();
        let mut content = std::fs::read_to_string(&path).unwrap();
        content.push_str(&format!("{}\n", serde_json::to_string(&theirs).unwrap()));
        std::fs::write(&path, content).unwrap();

        // Our save() must NOT clobber the other agent's append.
        store.save().unwrap();

        let mut reloaded = MemoryStore::new(dir.path().to_path_buf());
        reloaded.load().unwrap();
        assert!(reloaded.get(&our_id).is_some(), "our entry preserved");
        assert!(
            reloaded.get(&their_id).is_some(),
            "other agent's append preserved (not clobbered)"
        );
    }

    #[test]
    fn test_save_drops_deliberately_removed_entry() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let keep = make_entry("p", MemoryCategory::Context, "keep me");
        let drop_ = make_entry("p", MemoryCategory::Context, "drop me");
        let keep_id = keep.id.clone();
        let drop_id = drop_.id.clone();
        store.add(keep).unwrap();
        store.add(drop_).unwrap();
        store.delete(&drop_id).unwrap();

        store.save().unwrap();

        let mut reloaded = MemoryStore::new(dir.path().to_path_buf());
        reloaded.load().unwrap();
        assert!(reloaded.get(&keep_id).is_some(), "kept entry survives");
        assert!(
            reloaded.get(&drop_id).is_none(),
            "deliberately removed entry not resurrected from stale line"
        );
    }

    #[test]
    fn test_save_reconcile_drops_removed_but_keeps_others_append() {
        // Combines the two above: a deletion must propagate AND another agent's
        // concurrent append must survive in the same save() pass.
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let victim = make_entry("p", MemoryCategory::Context, "we remove this");
        let victim_id = victim.id.clone();
        store.add(victim).unwrap();
        let path = dir.path().join(format!("{}.jsonl", project_hash("p")));

        // Other agent appends after our load.
        let theirs = make_entry("p", MemoryCategory::Context, "their fact");
        let their_id = theirs.id.clone();
        let mut content = std::fs::read_to_string(&path).unwrap();
        content.push_str(&format!("{}\n", serde_json::to_string(&theirs).unwrap()));
        std::fs::write(&path, content).unwrap();

        // We delete our entry (tombstone), then compact.
        store.delete(&victim_id).unwrap();
        store.save().unwrap();

        let mut reloaded = MemoryStore::new(dir.path().to_path_buf());
        reloaded.load().unwrap();
        assert!(
            reloaded.get(&victim_id).is_none(),
            "our deletion propagated"
        );
        assert!(
            reloaded.get(&their_id).is_some(),
            "other agent's append kept despite our deletion"
        );
    }

    // --- Token-budget pruning (ADR-0010 C5' size control) ---

    #[test]
    fn test_prune_to_token_budget_removes_lowest_confidence_first() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let low = make_entry_with_confidence("p", MemoryCategory::Context, "content aaaa", 0.2);
        let mid = make_entry_with_confidence("p", MemoryCategory::Context, "content bbbb", 0.6);
        let high = make_entry_with_confidence("p", MemoryCategory::Context, "content cccc", 0.95);
        let low_id = low.id.clone();
        let mid_id = mid.id.clone();
        let high_id = high.id.clone();
        store.add(low).unwrap();
        store.add(mid).unwrap();
        store.add(high).unwrap();

        // 18 tokens (~72 chars): the 3-entry injection (~77 chars) is over
        // budget, but the 2-entry one (~62) fits — so exactly the lowest-
        // confidence entry is pruned.
        let removed = store.prune_to_token_budget("p", 18);
        assert_eq!(removed, 1);
        // Prune invalidates (bi-temporal close), it never destroys: the
        // entry remains retrievable via /recall --all.
        assert!(
            store.get(&low_id).unwrap().is_expired(Utc::now()),
            "lowest-confidence pruned (invalidated) first"
        );
        assert!(
            !store.get(&mid_id).unwrap().is_expired(Utc::now()),
            "mid-confidence survives"
        );
        assert!(
            !store.get(&high_id).unwrap().is_expired(Utc::now()),
            "highest-confidence survives"
        );
    }

    #[test]
    fn test_prune_to_token_budget_noop_when_under_budget() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry("p", MemoryCategory::Context, "tiny"))
            .unwrap();
        // Generous budget — nothing to prune.
        assert_eq!(store.prune_to_token_budget("p", 10_000), 0);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn test_prune_to_token_budget_empty_project() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        assert_eq!(store.prune_to_token_budget("no-such-project", 8), 0);
    }

    // --- Cleanup ---

    #[test]
    fn test_cleanup_removes_old_entries() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let old = make_entry_with_timestamps(
            "1",
            "p",
            MemoryCategory::Context,
            "old",
            1.0,
            Utc::now() - Duration::days(100),
            Utc::now(),
            0,
        );
        let recent = make_entry_with_timestamps(
            "2",
            "p",
            MemoryCategory::Context,
            "recent",
            1.0,
            Utc::now(),
            Utc::now(),
            0,
        );
        store.add(old).unwrap();
        store.add(recent).unwrap();
        let removed = store.cleanup("p", Duration::days(30), 100).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(store.len(), 1);
        assert_eq!(store.get("2").unwrap().content, "recent");
    }

    #[test]
    fn test_cleanup_enforces_cap() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        for i in 0..5u32 {
            let mut entry = make_entry_with_timestamps(
                &format!("{i}"),
                "p",
                MemoryCategory::Context,
                &format!("entry-{i}"),
                1.0,
                Utc::now(),
                Utc::now() - Duration::hours(i as i64 + 1),
                i,
            );
            entry.access_count = i;
            store.add(entry).unwrap();
        }
        let removed = store.cleanup("p", Duration::days(365), 2).unwrap();
        assert_eq!(removed, 3);
        assert_eq!(store.len(), 2);
    }

    // --- merge_duplicates ---

    #[test]
    fn test_merge_duplicates_removes_similar_same_category() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry_with_confidence(
                "p",
                MemoryCategory::Preference,
                "always use tabs for indentation",
                0.9,
            ))
            .unwrap();
        store
            .add(make_entry_with_confidence(
                "p",
                MemoryCategory::Preference,
                "always use tabs for indentation",
                0.7,
            ))
            .unwrap();
        assert_eq!(store.len(), 2);
        let merged = store.merge_duplicates(0.8, "p").unwrap();
        assert_eq!(merged, 1);
        assert_eq!(store.len(), 1);
        let survivor = store.entries.values().next().unwrap();
        assert!((survivor.confidence - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn test_merge_duplicates_different_category_keeps_both() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry_with_confidence(
                "p",
                MemoryCategory::Preference,
                "always use tabs for indentation",
                0.9,
            ))
            .unwrap();
        store
            .add(make_entry_with_confidence(
                "p",
                MemoryCategory::Decision,
                "always use tabs for indentation",
                0.9,
            ))
            .unwrap();
        let merged = store.merge_duplicates(0.8, "p").unwrap();
        assert_eq!(merged, 0);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn test_merge_duplicates_below_threshold_keeps_both() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry_with_confidence(
                "p",
                MemoryCategory::Preference,
                "use rust programming language",
                0.9,
            ))
            .unwrap();
        store
            .add(make_entry_with_confidence(
                "p",
                MemoryCategory::Preference,
                "deploy with kubernetes cluster",
                0.9,
            ))
            .unwrap();
        let merged = store.merge_duplicates(0.8, "p").unwrap();
        assert_eq!(merged, 0);
        assert_eq!(store.len(), 2);
    }

    // --- remove_stale ---

    #[test]
    fn test_remove_stale_removes_old_entries() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let old = make_entry_with_timestamps(
            "1",
            "p",
            MemoryCategory::Context,
            "old",
            1.0,
            Utc::now() - Duration::days(60),
            Utc::now(),
            0,
        );
        let fresh = make_entry_with_timestamps(
            "2",
            "p",
            MemoryCategory::Context,
            "fresh",
            1.0,
            Utc::now(),
            Utc::now(),
            0,
        );
        store.add(old).unwrap();
        store.add(fresh).unwrap();
        let removed = store.remove_stale(Duration::days(30), "p").unwrap();
        assert_eq!(removed, 1);
        assert_eq!(store.len(), 1);
        assert_eq!(store.get("2").unwrap().content, "fresh");
    }

    #[test]
    fn test_remove_stale_nothing_to_remove() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry("p", MemoryCategory::Context, "fresh"))
            .unwrap();
        let removed = store.remove_stale(Duration::days(365), "p").unwrap();
        assert_eq!(removed, 0);
    }

    // --- enforce_category_caps ---

    #[test]
    fn test_enforce_category_caps_removes_least_accessed() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        for i in 0..3u32 {
            let mut entry = make_entry_with_timestamps(
                &format!("p{i}"),
                "proj",
                MemoryCategory::Preference,
                &format!("pref {i}"),
                0.8,
                Utc::now(),
                Utc::now(),
                i,
            );
            entry.access_count = i;
            store.add(entry).unwrap();
        }
        store.enforce_category_caps(2, "proj");
        assert_eq!(store.len(), 2);
        for entry in store.entries.values() {
            assert!(entry.access_count > 0);
        }
    }

    #[test]
    fn test_enforce_category_caps_no_removal_when_under_cap() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry("p", MemoryCategory::Context, "one"))
            .unwrap();
        store
            .add(make_entry("p", MemoryCategory::Decision, "two"))
            .unwrap();
        store.enforce_category_caps(10, "p");
        assert_eq!(store.len(), 2);
    }

    // --- resolve_conflicts ---

    #[test]
    fn test_resolve_conflicts_keeps_newer() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let older = make_entry_with_timestamps(
            "1",
            "p",
            MemoryCategory::Preference,
            "always use spaces for formatting code",
            0.9,
            Utc::now() - Duration::hours(2),
            Utc::now(),
            0,
        );
        let newer = make_entry_with_timestamps(
            "2",
            "p",
            MemoryCategory::Preference,
            "never use spaces for formatting code",
            0.9,
            Utc::now(),
            Utc::now(),
            0,
        );
        store.add(older).unwrap();
        store.add(newer).unwrap();
        let resolved = store.resolve_conflicts("p").unwrap();
        assert_eq!(resolved, 1);
        assert_eq!(store.len(), 1, "only the newer fact stays live");
        assert!(store.get("1").unwrap().is_expired(Utc::now()));
        assert!(!store.get("2").unwrap().is_expired(Utc::now()));
    }

    #[test]
    fn test_resolve_conflicts_no_conflicts() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry(
                "p",
                MemoryCategory::Preference,
                "I prefer dark mode",
            ))
            .unwrap();
        store
            .add(make_entry("p", MemoryCategory::Decision, "Use PostgreSQL"))
            .unwrap();
        let resolved = store.resolve_conflicts("p").unwrap();
        assert_eq!(resolved, 0);
    }

    // --- search_by_relevance ---

    #[test]
    fn test_search_by_relevance_returns_relevant() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry(
                "p",
                MemoryCategory::Preference,
                "I prefer rust programming language",
            ))
            .unwrap();
        store
            .add(make_entry(
                "p",
                MemoryCategory::Decision,
                "Deploy with kubernetes",
            ))
            .unwrap();
        let results = store.search_by_relevance("rust programming", None, 10);
        assert!(!results.is_empty());
        // The most relevant result should mention rust
        assert!(results[0].content.contains("rust"));
    }

    #[test]
    fn test_search_by_relevance_respects_max_results() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        for i in 0..5 {
            store
                .add(make_entry(
                    "p",
                    MemoryCategory::Context,
                    &format!("test entry number {i}"),
                ))
                .unwrap();
        }
        let results = store.search_by_relevance("test", None, 2);
        assert!(results.len() <= 2);
    }

    #[test]
    fn test_search_by_relevance_filters_by_project() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry(
                "proj-a",
                MemoryCategory::Context,
                "test content here",
            ))
            .unwrap();
        store
            .add(make_entry(
                "proj-b",
                MemoryCategory::Context,
                "test content here",
            ))
            .unwrap();
        let results = store.search_by_relevance("test", Some("proj-a"), 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].project, "proj-a");
    }

    // --- auto_extract_from_messages ---

    #[test]
    fn test_auto_extract_preference() {
        let dir = TempDir::new().unwrap();
        let store = MemoryStore::new(dir.path().to_path_buf());
        let config = SessionMemoryConfig::default();
        let msgs = vec![MessageSummary::new("user", "I always use tabs not spaces")];
        let extracted = store.auto_extract_from_messages(&msgs, &config);
        assert_eq!(extracted.len(), 1);
        assert_eq!(extracted[0].category, MemoryCategory::Preference);
    }

    #[test]
    fn test_auto_extract_decision() {
        let dir = TempDir::new().unwrap();
        let store = MemoryStore::new(dir.path().to_path_buf());
        let config = SessionMemoryConfig::default();
        let msgs = vec![MessageSummary::new(
            "user",
            "We decided to use Rust for the backend",
        )];
        let extracted = store.auto_extract_from_messages(&msgs, &config);
        assert_eq!(extracted.len(), 1);
        assert_eq!(extracted[0].category, MemoryCategory::Decision);
    }

    #[test]
    fn test_auto_extract_error() {
        let dir = TempDir::new().unwrap();
        let store = MemoryStore::new(dir.path().to_path_buf());
        let config = SessionMemoryConfig::default();
        let msgs = vec![MessageSummary::new(
            "user",
            "The error was a null pointer dereference",
        )];
        let extracted = store.auto_extract_from_messages(&msgs, &config);
        assert_eq!(extracted.len(), 1);
        assert_eq!(extracted[0].category, MemoryCategory::Error);
    }

    #[test]
    fn test_auto_extract_pattern() {
        let dir = TempDir::new().unwrap();
        let store = MemoryStore::new(dir.path().to_path_buf());
        let config = SessionMemoryConfig::default();
        let msgs = vec![MessageSummary::new(
            "user",
            "In this project we use snake_case for variables",
        )];
        let extracted = store.auto_extract_from_messages(&msgs, &config);
        assert_eq!(extracted.len(), 1);
        assert_eq!(extracted[0].category, MemoryCategory::Pattern);
    }

    #[test]
    fn test_auto_extract_disabled_returns_empty() {
        let dir = TempDir::new().unwrap();
        let store = MemoryStore::new(dir.path().to_path_buf());
        let config = SessionMemoryConfig {
            auto_extract_enabled: false,
            ..SessionMemoryConfig::default()
        };
        let msgs = vec![MessageSummary::new("user", "I always use tabs")];
        assert!(store.auto_extract_from_messages(&msgs, &config).is_empty());
    }

    #[test]
    fn test_auto_extract_no_match_returns_empty() {
        let dir = TempDir::new().unwrap();
        let store = MemoryStore::new(dir.path().to_path_buf());
        let config = SessionMemoryConfig::default();
        let msgs = vec![MessageSummary::new("user", "The weather is nice today")];
        assert!(store.auto_extract_from_messages(&msgs, &config).is_empty());
    }

    #[test]
    fn test_auto_extract_deduplicates_similar() {
        let dir = TempDir::new().unwrap();
        let store = MemoryStore::new(dir.path().to_path_buf());
        let config = SessionMemoryConfig::default();
        // Identical content triggers the same keyword and produces entries with identical content,
        // which should be deduplicated by the extraction logic.
        let msgs = vec![
            MessageSummary::new("user", "I always use tabs for indentation"),
            MessageSummary::new("user", "I always use tabs for indentation"),
        ];
        let extracted = store.auto_extract_from_messages(&msgs, &config);
        assert!(extracted.len() <= 2);
    }

    // --- content_similarity edge cases ---

    #[test]
    fn test_content_similarity_empty_strings() {
        assert!((content_similarity("", "") - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_content_similarity_one_empty() {
        assert!((content_similarity("hello world", "") - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_content_similarity_identical() {
        assert!(
            (content_similarity("hello world foo bar", "hello world foo bar") - 1.0).abs() < 0.001
        );
    }

    #[test]
    fn test_content_similarity_completely_different() {
        assert!((content_similarity("alpha beta", "gamma delta") - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_content_similarity_partial_overlap() {
        let sim = content_similarity("the quick brown fox", "the quick lazy dog");
        assert!((sim - (2.0 / 6.0)).abs() < 0.001);
    }

    // --- len / is_empty ---

    #[test]
    fn test_len_and_is_empty() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
        store
            .add(make_entry("p", MemoryCategory::Context, "data"))
            .unwrap();
        assert!(!store.is_empty());
        assert_eq!(store.len(), 1);
    }

    // --- get_memories_by_type ---

    #[test]
    fn test_get_memories_by_type() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry("p", MemoryCategory::Preference, "pref"))
            .unwrap();
        store
            .add(make_entry("p", MemoryCategory::Decision, "dec"))
            .unwrap();
        store
            .add(make_entry("p", MemoryCategory::Preference, "pref2"))
            .unwrap();
        let prefs = store.get_memories_by_type(&MemoryType::UserPreference);
        assert_eq!(prefs.len(), 2);
    }

    // --- get_mut touches ---

    #[test]
    fn test_get_mut_touches_entry() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let entry = make_entry("p", MemoryCategory::Context, "data");
        let id = entry.id.clone();
        let orig_count = entry.access_count;
        store.add(entry).unwrap();
        let retrieved = store.get_mut(&id).unwrap();
        assert_eq!(retrieved.access_count, orig_count + 1);
    }

    // --- are_contradictory ---

    #[test]
    fn test_are_contradictory_with_signal_pairs() {
        assert!(are_contradictory("always use tabs", "never use tabs"));
        assert!(are_contradictory("enable feature X", "disable feature X"));
        // "prefer" vs "avoid" -- neither word contains the other
        assert!(are_contradictory(
            "prefer tabs for indentation",
            "avoid tabs for indentation"
        ));
    }

    #[test]
    fn test_are_contradictory_no_contradiction() {
        assert!(!are_contradictory("use rust", "use rust with cargo"));
        assert!(!are_contradictory("enable feature A", "enable feature B"));
    }

    #[test]
    fn test_are_contradictory_low_overlap_not_contradictory() {
        assert!(!are_contradictory("always alpha beta", "never gamma delta"));
    }

    // --- Review 2026-09 regression tests ---

    #[test]
    fn compaction_of_project_a_never_touches_project_b() {
        // P0 regression: merge_duplicates/remove_stale/enforce_category_caps
        // used to sweep every project, so compacting A deleted B's
        // similar-but-distinct entries.
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let a = make_entry_with_confidence(
            "proj-a",
            MemoryCategory::Preference,
            "always use tabs for indentation",
            0.6,
        );
        let b = make_entry_with_confidence(
            "proj-b",
            MemoryCategory::Preference,
            "always use tabs for indentation",
            0.9,
        );
        let b_id = b.id.clone();
        store.add(a).unwrap();
        store.add(b).unwrap();

        store
            .consolidate_memories("proj-a", &SessionMemoryConfig::default())
            .unwrap();

        assert!(
            store.get(&b_id).is_some(),
            "project B's entry must survive A's compaction"
        );
    }

    #[test]
    fn deletion_is_visible_to_a_fresh_load_without_save() {
        // P0 regression: evict now appends a durable tombstone line, so a
        // deletion made by process A is honored by any later load even if A
        // never rewrites the file.
        let dir = TempDir::new().unwrap();
        let path = dir.path().to_path_buf();

        let mut writer = MemoryStore::new(path.clone());
        writer.load().unwrap();
        let e = make_entry("p", MemoryCategory::Context, "ephemeral fact");
        let id = e.id.clone();
        writer.add(e).unwrap();
        writer.delete(&id).unwrap();

        let mut reader = MemoryStore::new(path);
        reader.load().unwrap();
        assert!(
            reader.get(&id).is_none(),
            "fresh load must honor the tombstone without a rewrite"
        );
    }

    #[test]
    fn other_process_save_does_not_resurrect_deleted_entry() {
        // P0 regression: a long-running process B that loaded before A's
        // deletion used to resurrect the entry from its in-memory map on its
        // next save. Now B's save honors A's (older) tombstone line.
        let dir = TempDir::new().unwrap();
        let path = dir.path().to_path_buf();

        let mut b = MemoryStore::new(path.clone());
        b.load().unwrap();
        let e = make_entry("p", MemoryCategory::Context, "shared fact");
        let id = e.id.clone();
        b.add(e).unwrap();

        // Process A loads, deletes, saves (rewrites without the entry but
        // with a tombstone line).
        let mut a = MemoryStore::new(path.clone());
        a.load().unwrap();
        a.delete(&id).unwrap();
        a.save().unwrap();

        // B has not reloaded; its next save must not resurrect the entry.
        b.save().unwrap();

        let mut fresh = MemoryStore::new(path);
        fresh.load().unwrap();
        assert!(
            fresh.get(&id).is_none(),
            "deletion must win over B's stale view"
        );
    }

    #[test]
    fn readd_after_tombstone_revives_the_entry() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().to_path_buf();

        let mut store = MemoryStore::new(path.clone());
        store.load().unwrap();
        let e = make_entry("p", MemoryCategory::Context, "comeback fact");
        let id = e.id.clone();
        store.add(e).unwrap();
        store.delete(&id).unwrap();

        // Re-add the same id after the tombstone: last writer wins.
        let mut revived = make_entry("p", MemoryCategory::Context, "comeback fact");
        revived.id = id.clone();
        store.add(revived).unwrap();

        let mut fresh = MemoryStore::new(path);
        fresh.load().unwrap();
        assert!(
            fresh.get(&id).is_some(),
            "entry line after tombstone revives the id"
        );
    }

    #[test]
    fn global_memories_inject_alongside_project_memories() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry(
                super::super::store::GLOBAL_SCOPE,
                MemoryCategory::Preference,
                "user prefers pnpm over npm",
            ))
            .unwrap();
        store
            .add(make_entry(
                "proj",
                MemoryCategory::Decision,
                "use postgres here",
            ))
            .unwrap();

        let out = store.format_for_injection("proj", None).unwrap();
        assert!(out.contains("## Global Memories"), "{out}");
        assert!(out.contains("## Project Memories"), "{out}");
        assert!(out.contains("pnpm"), "{out}");
        assert!(out.contains("postgres"), "{out}");

        // Other projects must not see proj's entries.
        let other = store.format_for_injection("other-proj", None).unwrap();
        assert!(!other.contains("postgres"), "{other}");
        assert!(other.contains("pnpm"), "globals are cross-project: {other}");
    }

    #[test]
    fn expired_entries_are_excluded_from_injection_and_search() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let mut e = make_entry("p", MemoryCategory::Context, "stale beyond recall");
        e.valid_until = Some(Utc::now() - Duration::hours(1));
        store.add(e).unwrap();

        assert!(store.format_for_injection("p", None).is_none());
        assert!(store.search("stale", Some("p")).is_empty());
        assert_eq!(store.len(), 0);
        assert_eq!(store.total_len(), 1);
        // --all surfaces it for audit.
        assert_eq!(store.search_including_expired("stale", Some("p")).len(), 1);
        // And it is still on disk.
        let mut reloaded = MemoryStore::new(dir.path().to_path_buf());
        reloaded.load().unwrap();
        assert_eq!(reloaded.total_len(), 1, "invalidation is not deletion");
    }

    #[test]
    fn cjk_similarity_detects_paraphrases() {
        // split_whitespace made a whole CJK sentence one token (similarity
        // 0-or-1), silently disabling dedup for Chinese content.
        let a = "这个项目总是使用 cargo 构建和测试";
        let b = "这个项目总是使用 cargo 构建与测试";
        assert!(
            content_similarity(a, b) >= 0.8,
            "CJK paraphrase must merge: {}",
            content_similarity(a, b)
        );
        let c = "完全不同的另一句话";
        assert!(content_similarity(a, c) < 0.8);
    }

    #[test]
    fn estimate_tokens_counts_cjk_as_one_token_each() {
        // chars/4 underestimated CJK ~4x, letting 2000-token budgets inject
        // ~8000 tokens of Chinese.
        let ascii = "abcdefgh"; // 8 chars -> 2 tokens
        assert_eq!(estimate_tokens(ascii), 2);
        let cjk = "四个汉字"; // 4 chars -> 4 tokens
        assert_eq!(estimate_tokens(cjk), 4);
        let mixed = "abc四个"; // 3 ascii -> 1 + 2 cjk
        assert_eq!(estimate_tokens(mixed), 3);
    }

    #[test]
    fn add_or_update_masks_secrets() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let secret = format!("my api key is sk-ant-api03-{}", "a".repeat(95));
        let e = make_entry("p", MemoryCategory::Context, &secret);
        store.add_or_update(e).unwrap();

        let stored = store.project_memories("p").remove(0);
        assert!(
            !stored.content.contains("sk-ant-api03-"),
            "secret must not persist: {}",
            stored.content
        );
        assert!(stored.content.contains("[REDACTED:"), "{}", stored.content);
    }

    #[test]
    fn injection_orders_by_relevance_to_query_when_truncating() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        // MAX_INJECTED_MEMORIES + 10 entries, all recent; the kubernetes one
        // is oldest but must survive when the query is about kubernetes.
        for i in 0..(MAX_INJECTED_MEMORIES + 10) {
            let content = if i == 0 {
                "kubernetes deploy pipeline details".to_string()
            } else {
                format!("filler entry number {i} about unrelated things")
            };
            let mut e = make_entry_with_timestamps(
                &format!("e{i}"),
                "p",
                MemoryCategory::Context,
                &content,
                0.8,
                Utc::now(),
                Utc::now(),
                0,
            );
            // oldest first for i == 0 so recency ordering would drop it.
            e.created_at = Utc::now() - Duration::hours(i as i64 + 1);
            store.add(e).unwrap();
        }
        let out = store
            .format_for_injection("p", Some("kubernetes deploy pipeline"))
            .unwrap();
        assert!(
            out.contains("kubernetes deploy pipeline"),
            "relevance must outrank recency under the cap: {{out}}"
        );
    }

    #[test]
    fn conflict_resolution_invalidates_instead_of_deleting() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let older = make_entry_with_timestamps(
            "1",
            "p",
            MemoryCategory::Preference,
            "always use spaces for formatting code",
            0.9,
            Utc::now() - Duration::hours(2),
            Utc::now(),
            0,
        );
        let newer = make_entry_with_timestamps(
            "2",
            "p",
            MemoryCategory::Preference,
            "never use spaces for formatting code",
            0.9,
            Utc::now(),
            Utc::now(),
            0,
        );
        store.add(older).unwrap();
        store.add(newer).unwrap();

        let resolved = store.resolve_conflicts("p").unwrap();
        assert_eq!(resolved, 1);
        // The loser stays on disk (auditable) but is out of injection.
        assert!(store.get("1").is_some());
        assert!(store.get("1").unwrap().is_expired(Utc::now()));
        assert!(store.format_for_injection("p", None).is_some());
        assert!(
            !store
                .format_for_injection("p", None)
                .unwrap()
                .contains("always use spaces")
        );
    }

    #[test]
    fn merge_prefers_manual_over_auto_extract_source() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let mut manual = make_entry_with_confidence(
            "p",
            MemoryCategory::Preference,
            "always use tabs for indentation",
            0.5,
        );
        manual.source_kind = Some(MemoryEntry::SOURCE_MANUAL.to_string());
        let mut auto = make_entry_with_confidence(
            "p",
            MemoryCategory::Preference,
            "always use tabs for indentation",
            0.99,
        );
        auto.source_kind = Some(MemoryEntry::SOURCE_AUTO_EXTRACT.to_string());
        let manual_id = manual.id.clone();
        store.add(manual).unwrap();
        store.add(auto).unwrap();

        store.merge_duplicates(0.8, "p").unwrap();
        assert_eq!(store.project_memories("p").len(), 1);
        let survivor = store.project_memories("p").remove(0);
        assert_eq!(survivor.id, manual_id, "hand-saved fact must win");
        // merge_duplicates deletes the loser outright; only add_or_update
        // merges confidence — the manual entry keeps its own score.
        assert!((survivor.confidence - 0.5).abs() < f64::EPSILON);
    }

    // --- move_entry (B3-24: update_memory project moves) ---

    #[test]
    fn move_entry_moves_between_projects_preserving_identity() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let e = make_entry("proj-a", MemoryCategory::Decision, "portable fact");
        let id = e.id.clone();
        store.add(e).unwrap();

        let moved = store.move_entry(&id, "proj-b").unwrap();
        assert_eq!(moved.id, id, "a move keeps the entry's identity");
        assert_eq!(moved.project, "proj-b");
        assert_eq!(moved.content, "portable fact");
        assert!(store.project_memories("proj-a").is_empty());
        assert_eq!(store.project_memories("proj-b").len(), 1);
    }

    #[test]
    fn move_entry_unknown_id_errors() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        assert!(matches!(
            store.move_entry("missing", "proj-b"),
            Err(MemoryError::NotFound(_))
        ));
    }

    #[test]
    fn move_entry_to_same_project_is_noop() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let e = make_entry("proj-a", MemoryCategory::Context, "already home");
        let id = e.id.clone();
        store.add(e).unwrap();

        let moved = store.move_entry(&id, "proj-a").unwrap();
        assert_eq!(moved.project, "proj-a");
        assert_eq!(store.project_memories("proj-a").len(), 1);
    }

    #[test]
    fn move_entry_survives_reload_without_old_project_stale_line() {
        // The invariant the desktop move relies on: after the move, the old
        // project's JSONL no longer carries the entry line, so a reload —
        // in any file order — finds the entry exactly once, under the new
        // project. (A tombstone-based move would be order-dependent: the
        // durable tombstone removes the id globally on load.)
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let e = make_entry("proj-a", MemoryCategory::Pattern, "durable move");
        let id = e.id.clone();
        store.add(e).unwrap();
        store.move_entry(&id, "proj-b").unwrap();

        let mut reloaded = MemoryStore::new(dir.path().to_path_buf());
        reloaded.load().unwrap();
        assert!(reloaded.project_memories("proj-a").is_empty());
        let proj_b = reloaded.project_memories("proj-b");
        assert_eq!(proj_b.len(), 1);
        assert_eq!(proj_b[0].id, id);
        assert_eq!(proj_b[0].content, "durable move");
    }

    #[test]
    fn move_entry_keeps_other_entries_in_old_project_file() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let moved_e = make_entry("proj-a", MemoryCategory::Context, "moves away");
        let staying = make_entry("proj-a", MemoryCategory::Context, "stays put");
        let moved_id = moved_e.id.clone();
        store.add(moved_e).unwrap();
        store.add(staying).unwrap();

        store.move_entry(&moved_id, "proj-b").unwrap();
        assert_eq!(store.project_memories("proj-a").len(), 1);
        assert_eq!(store.project_memories("proj-a")[0].content, "stays put");
    }
}
