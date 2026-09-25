//! Dream pass storage and state primitives — the persistence layer for the
//! "dream distillation" feature (Claude-Dreams-style memory consolidation).
//!
//! A dream pass reads the memory store plus recent session transcripts and
//! produces *shadow proposals*: review-gated `DreamProposal` files that the
//! user must approve before anything touches the real memory store. This
//! module owns everything that touches disk for that flow:
//!
//! - Proposals persisted as pretty JSON under `~/.shannon/dreams/{project_hash}/`
//!   (one subdir per project, hashed with the same `project_hash` scheme as
//!   the memory stores).
//! - The shared detection state file `~/.shannon/desktop/detection-state.json`
//!   (`DreamState`), written create-if-missing with `#[serde(default)]` on
//!   every field so future wiring tasks can share the file without
//!   migration.
//! - A 14-day self-heal that deletes expired proposal files.
//! - `redact`, a key/value masker applied to every user text before it
//!   leaves the machine (into excerpts, prompts, or reports).
//! - `session_excerpt`, which extracts redacted user texts and tool names
//!   from a session JSON file in the format read by
//!   `skill_pattern_detection::load_session`.
//! - `build_report_markdown`, the human-readable run report (counts only —
//!   no original text ever reaches the report).
//!
//! The LLM orchestration and the Tauri commands that drive it are wired by a
//! later task; everything here is pure Rust storage/state with no LLM and no
//! Tauri dependency. Following the repo convention, production functions
//! resolve real paths under `$HOME` while the `_in` variants take injected
//! directories so tests drive a tempdir and never mutate the process-global
//! `HOME` env var. `~/.shannon/dreams` subdirs are named by a caller-supplied
//! hash function: `shannon_core::memory::store::project_hash` is `pub(crate)`
//! in its own crate today, so the hashing is injected as a parameter and the
//! real function is passed at the (future) production call sites.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use chrono::{DateTime, Duration, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};

/// Maximum age (in days) of a proposal file before self-heal deletes it.
pub const DREAM_PROPOSAL_MAX_AGE_DAYS: u32 = 14;
/// Default cap on user messages excerpted from one session.
pub const DEFAULT_MAX_USER_MSGS: usize = 20;
/// Default per-message character cap for excerpted user texts.
pub const DEFAULT_PER_MSG_CHARS: usize = 500;

/// Filename of the shared detection state inside the desktop dir.
const DETECTION_STATE_FILE: &str = "detection-state.json";

// ============================================================================
// Types
// ============================================================================

/// Kind of consolidation action a proposal proposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DreamActionKind {
    /// Merge several memory entries into one.
    Merge,
    /// Remove outdated or contradicted memory entries.
    Remove,
    /// Add a new memory entry distilled from sessions.
    Add,
}

/// A new memory entry a dream pass wants to add (the `add` action payload).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DreamAddEntry {
    /// One of `preference` | `pattern` | `decision` | `error` | `context`.
    pub category: String,
    /// The proposed memory content.
    pub content: String,
    /// LLM confidence in `0.0..=1.0`.
    pub confidence: f64,
    /// Session ids the content was distilled from (provenance).
    pub source_session_ids: Vec<String>,
    /// True once the content has been user-verified; dream proposals always
    /// start `false` and stay review-gated.
    pub verified: bool,
}

/// One consolidation action inside a [`DreamProposal`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DreamAction {
    /// Stable action id, unique within its proposal.
    pub id: String,
    /// What to do with [`Self::entry_ids`].
    pub kind: DreamActionKind,
    /// Memory entry ids the action applies to (merge group / remove targets).
    pub entry_ids: Vec<String>,
    /// Payload for [`DreamActionKind::Add`] actions; `None` otherwise.
    #[serde(default)]
    pub add_entry: Option<DreamAddEntry>,
    /// Human-readable why, shown in the review UI.
    pub rationale: String,
}

/// A review-gated set of consolidation actions for one project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DreamProposal {
    /// `proposal-{unix_ts}`; also the on-disk filename stem.
    pub id: String,
    /// Project path the proposal belongs to.
    pub project: String,
    /// RFC 3339 creation timestamp (UTC in production).
    pub created_at: String,
    /// The proposed actions, reviewed (apply/discard) as a unit.
    pub actions: Vec<DreamAction>,
}

/// Shared, forward-compatible detection state persisted at
/// `~/.shannon/desktop/detection-state.json`. Every field is
/// `#[serde(default)]` so older/newer writers never break each other.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DreamState {
    /// RFC 3339 timestamp of the last completed dream pass.
    #[serde(default)]
    pub last_dream_at: Option<String>,
    /// JSON blob of the last [`DreamPassStats`] (as produced by
    /// `serde_json::to_value`).
    #[serde(default)]
    pub last_stats: Option<serde_json::Value>,
}

/// Counters describing one dream pass run; rendered into the markdown report
/// by [`build_report_markdown`] and stored in [`DreamState::last_stats`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DreamPassStats {
    /// Number of session files excerpted.
    pub scanned_sessions: u32,
    /// Number of memory entries the pass reviewed.
    pub entries_reviewed: u32,
    /// Merge actions proposed.
    pub merge_proposed: u32,
    /// Remove actions proposed.
    pub remove_proposed: u32,
    /// Add actions proposed.
    pub add_proposed: u32,
    /// Skill candidates detected by the L3 skill-distill leg.
    pub candidates_detected: u32,
    /// Skill candidates refined by the LLM.
    pub candidates_refined: u32,
    /// Sensitive values masked by [`redact`] during the pass.
    pub redactions_applied: u32,
    /// Wall-clock duration of the pass in milliseconds.
    pub duration_ms: u64,
    /// Projects processed.
    pub projects: Vec<String>,
    /// Rough prompt-size estimate in tokens.
    pub token_estimate: u64,
}

/// Redacted, size-capped slice of one session file, sized to feed an LLM.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionExcerpt {
    /// Session id from the file, falling back to the file stem.
    pub session_id: String,
    /// First `max_user_msgs` user texts, truncated and redacted.
    pub user_texts: Vec<String>,
    /// Tool names invoked by the assistant, deduplicated in first-seen order.
    pub tool_names: Vec<String>,
}

// ============================================================================
// Paths
// ============================================================================

/// Resolve the production dreams root `~/.shannon/dreams` (creating it).
/// Tests drive the `_in` variants with a tempdir instead.
pub fn dreams_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("Cannot determine home directory")?;
    let dir = home.join(".shannon").join("dreams");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create {}: {e}", dir.display()))?;
    Ok(dir)
}

/// Per-project proposals subdir under an explicit dreams root: the project
/// name run through the injected hash function (same naming scheme as the
/// memory stores). Created lazily by [`save_proposal_in`], never here.
fn project_dir_in(dir: &Path, project_hash: impl Fn(&str) -> String, project: &str) -> PathBuf {
    dir.join(project_hash(project))
}

// ============================================================================
// Proposal storage
// ============================================================================

/// Persist a proposal as pretty JSON at `{dir}/{project_hash}/{id}.json`.
/// Returns the file path. `project_hash` is injected because
/// `shannon_core::memory::store::project_hash` is not yet visible to this
/// crate; production passes the real function.
pub fn save_proposal_in(
    dir: &Path,
    project_hash: impl Fn(&str) -> String,
    proposal: &DreamProposal,
) -> Result<PathBuf, String> {
    if !is_safe_filename(&proposal.id) {
        return Err(format!("Unsafe proposal id: {:?}", proposal.id));
    }
    let project_dir = project_dir_in(dir, project_hash, &proposal.project);
    std::fs::create_dir_all(&project_dir)
        .map_err(|e| format!("Failed to create {}: {e}", project_dir.display()))?;
    let path = project_dir.join(format!("{}.json", proposal.id));
    let json =
        serde_json::to_string_pretty(proposal).map_err(|e| format!("serialize proposal: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(path)
}

/// Load one proposal by id, scanning the per-project subdirs of `dir`.
/// Errors when the file is missing or unparseable; [`list_proposals_in`]
/// skips corrupt files instead.
pub fn load_proposal_in(dir: &Path, proposal_id: &str) -> Result<DreamProposal, String> {
    let path = find_proposal_file_in(dir, proposal_id)
        .ok_or_else(|| format!("Proposal {proposal_id} not found"))?;
    let contents =
        std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str(&contents).map_err(|e| format!("parse {}: {e}", path.display()))
}

/// Load every parseable proposal under `dir` (all project subdirs), newest
/// first by `created_at` (RFC 3339, UTC in production so lexicographic order
/// is chronological). Corrupt files are logged and skipped, never fatal.
pub fn list_proposals_in(dir: &Path) -> Result<Vec<DreamProposal>, String> {
    let mut out = Vec::new();
    for path in proposal_files_in(dir)? {
        let contents = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
                    "dream proposal {}: unreadable, skipped: {e}",
                    path.display()
                );
                continue;
            }
        };
        match serde_json::from_str::<DreamProposal>(&contents) {
            Ok(p) => out.push(p),
            Err(e) => {
                tracing::warn!("dream proposal {}: corrupt, skipped: {e}", path.display());
            }
        }
    }
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(out)
}

/// Delete proposal files whose `created_at` is older than `max_age_days`.
/// Returns how many files were removed. Best-effort: corrupt files and
/// per-file IO failures are logged and left alone. The production call site
/// passes [`DREAM_PROPOSAL_MAX_AGE_DAYS`].
pub fn self_heal_expired_proposals_in(dir: &Path, max_age_days: u32) -> usize {
    let cutoff = Utc::now() - Duration::days(i64::from(max_age_days));
    let mut removed = 0usize;
    let files = match proposal_files_in(dir) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("dream self-heal: cannot scan {}: {e}", dir.display());
            return 0;
        }
    };
    for path in files {
        let expired = std::fs::read_to_string(&path)
            .map_err(|e| format!("read {}: {e}", path.display()))
            .and_then(|c| serde_json::from_str::<DreamProposal>(&c).map_err(|e| e.to_string()))
            .and_then(|p| {
                DateTime::parse_from_rfc3339(&p.created_at)
                    .map_err(|e| format!("parse created_at: {e}"))
            })
            .map(|ts| ts < cutoff);
        match expired {
            Ok(true) => match std::fs::remove_file(&path) {
                Ok(()) => {
                    tracing::info!(
                        "dream self-heal: removed expired proposal {}",
                        path.display()
                    );
                    removed += 1;
                }
                Err(e) => tracing::warn!("dream self-heal: cannot remove {}: {e}", path.display()),
            },
            Ok(false) => {}
            Err(e) => tracing::warn!("dream self-heal: skipping {}: {e}", path.display()),
        }
    }
    removed
}

/// All `*.json` files in the per-project subdirs of `dir`. Missing dir means
/// "no proposals yet", not an error.
fn proposal_files_in(dir: &Path) -> Result<Vec<PathBuf>, String> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| format!("readdir {}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("readdir entry: {e}"))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        for file in
            std::fs::read_dir(&path).map_err(|e| format!("readdir {}: {e}", path.display()))?
        {
            let file = file.map_err(|e| format!("readdir entry: {e}"))?;
            let file_path = file.path();
            if file_path.extension().and_then(|s| s.to_str()) == Some("json") {
                out.push(file_path);
            }
        }
    }
    Ok(out)
}

/// Locate the file for `proposal_id` by scanning project subdirs. Filenames
/// are `{proposal.id}.json`, so no parse is needed.
fn find_proposal_file_in(dir: &Path, proposal_id: &str) -> Option<PathBuf> {
    if !is_safe_filename(proposal_id) {
        return None;
    }
    for entry in std::fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let candidate = path.join(format!("{proposal_id}.json"));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// True when `name` can be used as a filename component without escaping the
/// proposals directory.
fn is_safe_filename(name: &str) -> bool {
    !name.is_empty() && !name.contains('/') && !name.contains('\\') && !name.contains("..")
}

// ============================================================================
// Detection state
// ============================================================================

/// Read [`DreamState`] from `{dir}/detection-state.json`. Missing or corrupt
/// file falls back to the default (logged, never a panic) — the state file is
/// shared with other detection features, so a bad write must not take the
/// dream pass down.
pub fn read_state_in(dir: &Path) -> DreamState {
    let path = dir.join(DETECTION_STATE_FILE);
    if !path.exists() {
        return DreamState::default();
    }
    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(
                "dream state {}: unreadable, using defaults: {e}",
                path.display()
            );
            return DreamState::default();
        }
    };
    match serde_json::from_str::<DreamState>(&contents) {
        Ok(state) => state,
        Err(e) => {
            tracing::warn!(
                "dream state {}: corrupt, using defaults: {e}",
                path.display()
            );
            DreamState::default()
        }
    }
}

/// Write [`DreamState`] to `{dir}/detection-state.json`, creating `dir` and
/// the file if missing.
pub fn write_state_in(dir: &Path, state: &DreamState) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("Failed to create {}: {e}", dir.display()))?;
    let path = dir.join(DETECTION_STATE_FILE);
    let json = serde_json::to_string_pretty(state).map_err(|e| format!("serialize state: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("write {}: {e}", path.display()))
}

// ============================================================================
// Redaction
// ============================================================================

/// Matches sensitive `key(:|=)value` pairs. Key = any `[\w-]` prefix plus one
/// of the sensitive keywords (so `access_token` / `client_secret` are caught
/// too), matched case-insensitively; an optional closing quote (JSON
/// `"key":` shape) may sit between key and separator; value = a quoted string
/// or a bare token stopping at whitespace / common JSON-YAML-URL delimiters.
/// An optional `bearer ` scheme is preserved in front of the mask.
static REDACT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)\b([\w-]*(?:api[_-]?key|token|secret|password|passwd|authorization|bearer))\b",
        r#"(\s*['"]?\s*[:=]\s*)"#,
        r"(bearer\s+)?",
        r#"("[^"]*"|'[^']*'|[^\s,;}&)\]]+)"#,
    ))
    .expect("redaction regex must compile")
});

/// Mask sensitive values in `s`: case-insensitive matches of
/// `api[_-]?key|token|secret|password|passwd|authorization|bearer` (with
/// word-char prefixes, e.g. `access_token`) followed by `:` or `=`, in both
/// bare `key: value` and JSON `"key": "value"` shapes, get their value
/// replaced with `[REDACTED]`. Non-secret text is returned unchanged.
pub fn redact(s: &str) -> String {
    REDACT_RE
        .replace_all(s, |caps: &regex::Captures| {
            let key = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
            let sep = caps.get(2).map(|m| m.as_str()).unwrap_or_default();
            let scheme = caps.get(3).map(|m| m.as_str()).unwrap_or_default();
            format!("{key}{sep}{scheme}[REDACTED]")
        })
        .into_owned()
}

// ============================================================================
// Session excerpts
// ============================================================================

/// Parsed session file — same shape [`crate::skill_pattern_detection`] reads.
#[derive(Debug, Deserialize)]
struct SessionFile {
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    messages: Vec<serde_json::Value>,
}

/// Extract a redacted, size-capped excerpt from the session file at `path`.
/// Takes the first `max_user_msgs` user texts (string content or `text`
/// blocks; `tool_result` blocks are never included), each truncated to
/// `per_msg_chars` chars and passed through [`redact`], plus the deduplicated
/// assistant tool names in first-seen order.
pub fn session_excerpt(
    path: &Path,
    max_user_msgs: usize,
    per_msg_chars: usize,
) -> Result<SessionExcerpt, String> {
    let contents =
        std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let session: SessionFile =
        serde_json::from_str(&contents).map_err(|e| format!("parse {}: {e}", path.display()))?;
    let session_id = if session.session_id.is_empty() {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string()
    } else {
        session.session_id
    };

    let mut user_texts = Vec::new();
    let mut tool_names: Vec<String> = Vec::new();
    for msg in &session.messages {
        match msg.get("role").and_then(|v| v.as_str()).unwrap_or_default() {
            "user" => {
                if user_texts.len() >= max_user_msgs {
                    continue;
                }
                if let Some(text) = extract_text(msg.get("content")) {
                    if text.trim().is_empty() {
                        continue;
                    }
                    user_texts.push(redact(&truncate_chars(&text, per_msg_chars)));
                }
            }
            "assistant" => {
                for block in content_blocks(msg.get("content")) {
                    if block
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        != "tool_use"
                        && block
                            .get("type")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            != "tool_call"
                    {
                        continue;
                    }
                    let name = block
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    if !tool_names.contains(&name) {
                        tool_names.push(name);
                    }
                }
            }
            _ => {}
        }
    }
    Ok(SessionExcerpt {
        session_id,
        user_texts,
        tool_names,
    })
}

/// User-visible text from a message `content` field: a plain string, or the
/// concatenated `text` of `text`-type blocks (tool results and other block
/// types are ignored on purpose).
fn extract_text(content: Option<&serde_json::Value>) -> Option<String> {
    match content? {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Array(blocks) => {
            let texts: Vec<String> = blocks
                .iter()
                .filter(|b| b.get("type").and_then(|v| v.as_str()) == Some("text"))
                .filter_map(|b| b.get("text").and_then(|v| v.as_str()).map(String::from))
                .collect();
            if texts.is_empty() {
                None
            } else {
                Some(texts.join("\n"))
            }
        }
        _ => None,
    }
}

/// Content blocks when `content` is an array; anything else yields nothing.
fn content_blocks(content: Option<&serde_json::Value>) -> &[serde_json::Value] {
    match content {
        Some(serde_json::Value::Array(a)) => a,
        _ => &[],
    }
}

/// Truncate to at most `max` chars (char-boundary safe, CJK friendly).
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect()
    }
}

// ============================================================================
// Report
// ============================================================================

/// Render the dream pass report as markdown. Counts only — no session or
/// memory text is ever embedded, so no `[REDACTED]` marker can appear.
pub fn build_report_markdown(stats: &DreamPassStats) -> String {
    let mut out = String::new();
    out.push_str("# Dream Pass Report\n\n");
    out.push_str(&format!("Generated: {}\n\n", Utc::now().to_rfc3339()));
    out.push_str("## Summary\n\n");
    out.push_str(&format!("- Sessions scanned: {}\n", stats.scanned_sessions));
    out.push_str(&format!(
        "- Memory entries reviewed: {}\n",
        stats.entries_reviewed
    ));
    out.push_str(&format!(
        "- Merge actions proposed: {}\n",
        stats.merge_proposed
    ));
    out.push_str(&format!(
        "- Remove actions proposed: {}\n",
        stats.remove_proposed
    ));
    out.push_str(&format!("- Add actions proposed: {}\n", stats.add_proposed));
    out.push_str(&format!(
        "- Skill candidates detected: {}\n",
        stats.candidates_detected
    ));
    out.push_str(&format!(
        "- Skill candidates refined: {}\n",
        stats.candidates_refined
    ));
    out.push_str(&format!(
        "- Redactions applied: {}\n",
        stats.redactions_applied
    ));
    out.push_str(&format!("- Duration: {} ms\n", stats.duration_ms));
    out.push_str(&format!("- Estimated tokens: {}\n", stats.token_estimate));
    out.push_str("\n## Projects\n\n");
    if stats.projects.is_empty() {
        out.push_str("- (none)\n");
    } else {
        for project in &stats.projects {
            out.push_str(&format!("- {project}\n"));
        }
    }
    out
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Deterministic, separator-free stand-in for shannon-core's
    /// `project_hash` (16 hex chars, no `/`), which stays `pub(crate)` there
    /// until Task 2 — storage fns take the hash as an injected parameter.
    fn test_hash(project: &str) -> String {
        let cleaned: String = project
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        format!("hash-{cleaned}")
    }

    fn sample_proposal(id: &str, project: &str, created_at: &str) -> DreamProposal {
        DreamProposal {
            id: id.to_string(),
            project: project.to_string(),
            created_at: created_at.to_string(),
            actions: vec![DreamAction {
                id: format!("{id}-action-1"),
                kind: DreamActionKind::Remove,
                entry_ids: vec!["entry-7".into()],
                add_entry: None,
                rationale: "Superseded by a newer decision".into(),
            }],
        }
    }

    // ------------------------------------------------------------------
    // Proposal storage
    // ------------------------------------------------------------------

    #[test]
    fn proposal_round_trips_through_disk() {
        let dir = tempdir().unwrap();
        let proposal = sample_proposal("proposal-1000", "/work/app", "2026-09-01T00:00:00+00:00");
        let path = save_proposal_in(dir.path(), test_hash, &proposal).unwrap();
        assert_eq!(
            path,
            dir.path()
                .join(test_hash("/work/app"))
                .join("proposal-1000.json")
        );
        let loaded = load_proposal_in(dir.path(), "proposal-1000").unwrap();
        assert_eq!(loaded, proposal);
    }

    #[test]
    fn save_proposal_rejects_unsafe_ids() {
        let dir = tempdir().unwrap();
        for bad in ["", "proposal-../escape", "a/b", "a\\b"] {
            let proposal = sample_proposal(bad, "/work/app", "2026-09-01T00:00:00+00:00");
            assert!(
                save_proposal_in(dir.path(), test_hash, &proposal).is_err(),
                "expected rejection for id {bad:?}"
            );
        }
    }

    #[test]
    fn list_proposals_sorted_newest_first() {
        let dir = tempdir().unwrap();
        for (id, ts) in [
            ("proposal-1001", "2026-01-01T00:00:00+00:00"),
            ("proposal-1003", "2026-06-01T00:00:00+00:00"),
            ("proposal-1002", "2026-03-01T00:00:00+00:00"),
        ] {
            save_proposal_in(dir.path(), test_hash, &sample_proposal(id, "/work/app", ts)).unwrap();
        }
        let listed = list_proposals_in(dir.path()).unwrap();
        let ids: Vec<&str> = listed.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["proposal-1003", "proposal-1002", "proposal-1001"]);
    }

    #[test]
    fn list_proposals_skips_corrupt_files() {
        let dir = tempdir().unwrap();
        save_proposal_in(
            dir.path(),
            test_hash,
            &sample_proposal("proposal-2001", "/work/app", "2026-05-01T00:00:00+00:00"),
        )
        .unwrap();
        // Corrupt JSON in the same project subdir must not break the listing.
        let project_dir = dir.path().join(test_hash("/work/app"));
        std::fs::write(project_dir.join("proposal-9999.json"), "{not json").unwrap();
        // A file with the right name but the wrong shape too.
        std::fs::write(
            project_dir.join("proposal-9998.json"),
            r#"{"unrelated": true}"#,
        )
        .unwrap();

        let listed = list_proposals_in(dir.path()).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "proposal-2001");
        // Loading the corrupt one by id reports an error instead of panicking.
        assert!(load_proposal_in(dir.path(), "proposal-9999").is_err());
        // Unknown ids report "not found".
        assert!(load_proposal_in(dir.path(), "proposal-does-not-exist").is_err());
    }

    #[test]
    fn list_proposals_empty_when_dir_missing() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("nope");
        assert!(list_proposals_in(&missing).unwrap().is_empty());
        assert_eq!(self_heal_expired_proposals_in(&missing, 14), 0);
    }

    // ------------------------------------------------------------------
    // Self-heal
    // ------------------------------------------------------------------

    #[test]
    fn self_heal_removes_only_expired_proposals() {
        let dir = tempdir().unwrap();
        let old = Utc::now() - Duration::days(15);
        let recent = Utc::now() - Duration::days(2);
        save_proposal_in(
            dir.path(),
            test_hash,
            &sample_proposal("proposal-old", "/work/app", &old.to_rfc3339()),
        )
        .unwrap();
        save_proposal_in(
            dir.path(),
            test_hash,
            &sample_proposal("proposal-recent", "/work/app", &recent.to_rfc3339()),
        )
        .unwrap();

        let removed = self_heal_expired_proposals_in(dir.path(), DREAM_PROPOSAL_MAX_AGE_DAYS);
        assert_eq!(removed, 1);
        assert!(
            load_proposal_in(dir.path(), "proposal-old").is_err(),
            "expired proposal should be gone"
        );
        assert!(
            load_proposal_in(dir.path(), "proposal-recent").is_ok(),
            "recent proposal must survive"
        );
    }

    // ------------------------------------------------------------------
    // Detection state
    // ------------------------------------------------------------------

    #[test]
    fn state_round_trips_and_defaults_when_missing() {
        let dir = tempdir().unwrap();
        assert_eq!(read_state_in(dir.path()), DreamState::default());

        let state = DreamState {
            last_dream_at: Some("2026-09-25T03:00:00+00:00".into()),
            last_stats: Some(serde_json::json!({"scanned_sessions": 3})),
        };
        write_state_in(dir.path(), &state).unwrap();
        assert_eq!(read_state_in(dir.path()), state);
        assert!(dir.path().join(DETECTION_STATE_FILE).is_file());
    }

    #[test]
    fn corrupt_state_falls_back_to_default() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path()).unwrap();
        std::fs::write(dir.path().join(DETECTION_STATE_FILE), "{oops not json").unwrap();
        assert_eq!(read_state_in(dir.path()), DreamState::default());
    }

    #[test]
    fn state_parses_legacy_json_with_missing_fields() {
        let parsed: DreamState =
            serde_json::from_str(r#"{"last_dream_at": "2026-01-01T00:00:00+00:00"}"#).unwrap();
        assert_eq!(
            parsed.last_dream_at.as_deref(),
            Some("2026-01-01T00:00:00+00:00")
        );
        assert_eq!(parsed.last_stats, None);
    }

    // ------------------------------------------------------------------
    // Redaction
    // ------------------------------------------------------------------

    #[test]
    fn redact_masks_json_colon_and_equals_forms() {
        let out = redact(r#""api_key": "sk-abc123""#);
        assert!(out.contains("[REDACTED]"), "json form: {out}");
        assert!(!out.contains("sk-abc123"), "json form: {out}");
        assert!(out.contains(r#""api_key": "#), "key preserved: {out}");

        let out = redact("password: hunter2");
        assert_eq!(out, "password: [REDACTED]");

        let out = redact("Authorization: Bearer xyz");
        assert!(!out.contains("xyz"), "bearer value gone: {out}");
        assert!(out.contains("[REDACTED]"), "bearer form: {out}");
    }

    #[test]
    fn redact_is_case_insensitive_and_covers_prefixed_keys() {
        assert_eq!(redact("PASSWORD=hunter2"), "PASSWORD=[REDACTED]");
        assert_eq!(redact("Api-Key: sk-1"), "Api-Key: [REDACTED]");
        let out = redact(r#""access_token": "t-123""#);
        assert!(!out.contains("t-123"));
        assert!(out.contains("[REDACTED]"));
        let out = redact("https://api.example.com?api_key=abc123&user=bob");
        assert!(!out.contains("abc123"));
        assert!(
            out.contains("user=bob"),
            "non-secret query param kept: {out}"
        );
    }

    #[test]
    fn redact_leaves_plain_text_untouched() {
        assert_eq!(redact("hello world"), "hello world");
        assert_eq!(
            redact("The password policy requires 12 characters"),
            "The password policy requires 12 characters"
        );
        assert_eq!(
            redact(r#"{"role": "user", "type": "text"}"#),
            r#"{"role": "user", "type": "text"}"#
        );
        assert_eq!(redact("tokens used today: 42"), "tokens used today: 42");
    }

    // ------------------------------------------------------------------
    // Session excerpts
    // ------------------------------------------------------------------

    fn write_session_file(dir: &Path, name: &str, body: serde_json::Value) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, serde_json::to_vec(&body).unwrap()).unwrap();
        path
    }

    #[test]
    fn session_excerpt_extracts_redacted_texts_and_tool_names() {
        let dir = tempdir().unwrap();
        let path = write_session_file(
            dir.path(),
            "s1.json",
            serde_json::json!({
                "session_id": "sess-1",
                "messages": [
                    {"role": "user", "content": "deploy with api_key: sk-secret1 please"},
                    {"role": "assistant", "content": [
                        {"type": "text", "text": "sure"},
                        {"type": "tool_use", "name": "bash", "input": {"cmd": "deploy"}}
                    ]},
                    {"role": "user", "content": [
                        {"type": "tool_result", "content": "leaked? no"},
                        {"type": "text", "text": "now set password=hunter2"}
                    ]},
                    {"role": "assistant", "content": [
                        {"type": "tool_use", "name": "bash", "input": {}},
                        {"type": "tool_call", "name": "read_file", "input": {}}
                    ]},
                    {"role": "user", "content": "thanks"}
                ]
            }),
        );

        let excerpt = session_excerpt(&path, DEFAULT_MAX_USER_MSGS, DEFAULT_PER_MSG_CHARS).unwrap();
        assert_eq!(excerpt.session_id, "sess-1");
        assert_eq!(
            excerpt.user_texts,
            vec![
                "deploy with api_key: [REDACTED] please",
                "now set password=[REDACTED]",
                "thanks",
            ]
        );
        assert_eq!(excerpt.tool_names, vec!["bash", "read_file"]);
    }

    #[test]
    fn session_excerpt_caps_messages_and_truncates_chars() {
        let dir = tempdir().unwrap();
        let mut messages = Vec::new();
        for i in 0..5 {
            messages.push(serde_json::json!({
                "role": "user",
                "content": format!("msg-{i} {}", "长".repeat(30)),
            }));
        }
        let path = write_session_file(
            dir.path(),
            "s2.json",
            serde_json::json!({"messages": messages}),
        );

        let excerpt = session_excerpt(&path, 3, 10).unwrap();
        // Missing session_id falls back to the file stem.
        assert_eq!(excerpt.session_id, "s2");
        assert_eq!(excerpt.user_texts.len(), 3);
        for (i, text) in excerpt.user_texts.iter().enumerate() {
            assert!(text.chars().count() <= 10, "truncated: {text}");
            assert!(text.starts_with(&format!("msg-{i} ")));
        }
    }

    #[test]
    fn session_excerpt_errors_on_missing_or_invalid_file() {
        let dir = tempdir().unwrap();
        assert!(session_excerpt(&dir.path().join("missing.json"), 20, 500).is_err());
        let bad = write_session_file(dir.path(), "bad.json", serde_json::json!([1, 2, 3]));
        assert!(session_excerpt(&bad, 20, 500).is_err());
    }

    // ------------------------------------------------------------------
    // Report
    // ------------------------------------------------------------------

    #[test]
    fn report_markdown_contains_all_counts_and_no_redacted_marker() {
        let stats = DreamPassStats {
            scanned_sessions: 3,
            entries_reviewed: 42,
            merge_proposed: 2,
            remove_proposed: 1,
            add_proposed: 1,
            candidates_detected: 4,
            candidates_refined: 2,
            redactions_applied: 5,
            duration_ms: 1234,
            projects: vec!["/work/app".into(), "/work/web".into()],
            token_estimate: 4567,
        };
        let report = build_report_markdown(&stats);
        for expected in [
            "Sessions scanned: 3",
            "Memory entries reviewed: 42",
            "Merge actions proposed: 2",
            "Remove actions proposed: 1",
            "Add actions proposed: 1",
            "Skill candidates detected: 4",
            "Skill candidates refined: 2",
            "Redactions applied: 5",
            "Duration: 1234 ms",
            "Estimated tokens: 4567",
            "- /work/app",
            "- /work/web",
        ] {
            assert!(
                report.contains(expected),
                "missing {expected:?} in:\n{report}"
            );
        }
        assert!(
            !report.contains("[REDACTED]"),
            "report is counts-only:\n{report}"
        );
        assert!(report.starts_with("# Dream Pass Report"));
    }

    #[test]
    fn report_markdown_handles_empty_projects() {
        let report = build_report_markdown(&DreamPassStats::default());
        assert!(report.contains("- (none)"));
        assert!(report.contains("Sessions scanned: 0"));
    }
}
