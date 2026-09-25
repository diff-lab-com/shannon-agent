//! Dream pass storage, state, and execution orchestration — the persistence
//! and L2 consolidation layer of the "dream distillation" feature
//! (Claude-Dreams-style memory consolidation).
//!
//! A dream pass reads the memory store plus recent session transcripts and
//! produces *shadow proposals*: review-gated [`DreamProposal`] files that the
//! user must approve before anything touches the real memory store. This
//! module owns everything that touches disk for that flow:
//!
//! - Proposals persisted as pretty JSON under `~/.shannon/dreams/{project_hash}/`
//!   (one subdir per project, hashed with the same `project_hash` scheme as
//!   the memory stores — `shannon_core::memory::project_hash`, re-exported
//!   since Task 2 made it `pub`).
//! - The shared detection state file `~/.shannon/desktop/detection-state.json`
//!   ([`DreamState`]), written create-if-missing with `#[serde(default)]` on
//!   every field so future wiring tasks can share the file without
//!   migration. **Ownership contract:** the dream pass only ever writes its
//!   own two fields (`last_dream_at`, `last_stats`) and always re-reads the
//!   file immediately before writing (read-modify-write). The typed struct
//!   silently drops fields written by other features — a known, accepted
//!   limitation until the struct grows `#[serde(flatten)] extra:
//!   serde_json::Value` support.
//! - A 14-day self-heal that deletes expired proposal files.
//! - `redact`, a key/value masker applied to every user text before it
//!   leaves the machine (into excerpts, prompts, or reports).
//! - `session_excerpt`, which extracts redacted user texts and tool names
//!   for one session through the core session-query adapter
//!   (`shannon_core::session_log::SessionQuery` — the read-side single
//!   source over the real `<id>/events.jsonl` layout it shares with skill
//!   detection; the retired flat `sessions/*.json` reader silently parsed
//!   zero production sessions).
//! - `build_report_markdown` + `save_report_in`/`read_report_in`, the
//!   human-readable run report (counts only — no original text ever reaches
//!   the report).
//!
//! ## Privacy boundary of `redact`
//!
//! Only *session excerpts* pass through `redact`. Stored memory entry
//! content goes into the consult prompt verbatim — it already left the
//! machine once, to the same configured provider that produced it — and the
//! report carries counts only, so no `redact`-masked or raw text appears
//! there either way.
//!
//! ## Orchestration
//!
//! `execute_dream_pass` is the single entry point shared by the manual
//! command and the two scheduler tracks (the 1–5 点 nightly window and the
//! one-shot startup catch-up): privacy gates → throttle
//! (state timestamp + a process-wide [`ConsolidationLock`]) → gather inputs →
//! one LLM consult per project → per-project proposals + one merged report +
//! state update + inbox card + `dream-pass-finished` event. The LLM call is
//! isolated in `consult_llm`; everything around it is pure or
//! tempdir-injectable (`execute_dream_pass_inner`, `build_proposal_from_llm`),
//! so tests drive the whole pipeline with a fake consult closure and never
//! touch the network.
//!
//! Storage functions resolve real paths under `$HOME` while the `_in`
//! variants take injected directories so tests drive a tempdir and never
//! mutate the process-global `HOME` env var.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, OnceLock};

use chrono::{DateTime, Duration, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use shannon_core::auto_dream_consolidation::{ConsolidationLock, ConsolidationPrompt};
use shannon_core::memory::{MemoryEntry, MemoryStore};
use shannon_core::session_log::{SessionQuery, SessionRef};
use tauri::{Emitter, Manager};

use crate::commands::AppState;
use crate::commands_memory::{SharedMemoryStore, parse_category, refresh_shared_store};

/// Maximum age (in days) of a proposal file before self-heal deletes it.
pub const DREAM_PROPOSAL_MAX_AGE_DAYS: u32 = 14;
/// Default cap on user messages excerpted from one session.
pub const DEFAULT_MAX_USER_MSGS: usize = 20;
/// Default per-message character cap for excerpted user texts.
pub const DEFAULT_PER_MSG_CHARS: usize = 500;
/// `days_back` used by the manual entry point (`run_dream_pass` / `/dream`).
/// The nightly scheduler passes 7 instead (Task 3).
pub const DEFAULT_MANUAL_DAYS_BACK: u32 = 3;
/// Upper bound on the session-window length. Callers may pass anything, but
/// a pass never scans more than 30 days of session files (a stray
/// `days_back=100000` must not walk every session on disk).
pub const DREAM_MAX_DAYS_BACK: u32 = 30;
/// Minimum interval between two dream passes — mirrored in both the
/// state-file timestamp check and the process-wide consolidation lock.
pub const DREAM_MIN_INTERVAL: Duration = Duration::hours(6);
/// Total character budget for the LLM user prompt (memories + excerpts).
/// The memory list is laid out first and is never cut (every entry id must
/// stay referenceable), so the session-excerpt block gets whatever budget
/// remains — a memory store large enough to saturate that remainder squeezes
/// session excerpts out entirely (a `tracing::warn!` fires when that
/// happens; see `build_dream_prompt`).
pub const DREAM_PROMPT_CHAR_BUDGET: usize = 24_000;
/// `source_kind` stamped on memory entries materialized from an applied
/// dream proposal (P2-4 provenance, plain String value — no schema break).
pub const DREAM_SOURCE_KIND: &str = "dream";
/// Tauri event pushed when a dream pass completes (payload = the
/// [`DreamPassResult`] JSON). Mirrors the `skill-candidates-changed` push
/// pattern; today only the Memory panel's `DreamPanel` listens (it refreshes
/// the review list and the last-run stats line).
pub const DREAM_PASS_FINISHED_EVENT: &str = "dream-pass-finished";
/// Marker counted to approximate how many values [`redact`] masked during a
/// pass (excerpt texts carry the mask inline).
const REDACTED_MARKER: &str = "[REDACTED]";

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
    /// Minimal back-source corroboration (卡C): set at proposal-build time
    /// when ≥2 distinct content keywords hit the excerpt text of one of the
    /// *claimed* source sessions — the model's own claim is never trusted
    /// (its `verified` output is ignored). Independently of this flag every
    /// add stays review-gated: nothing is written until the user approves.
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
    /// Incremental apply journal (卡C 4a): action ids already applied to the
    /// memory store, written back into this file after each successful
    /// action. A mid-apply failure leaves the proposal on disk, and a retry
    /// skips these ids instead of double-applying (the double-add case).
    /// `#[serde(default)]` so pre-journal proposal files keep parsing.
    #[serde(default)]
    pub applied_ids: Vec<String>,
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

/// What one dream pass did, as returned by the commands and pushed with the
/// `dream-pass-finished` event. A skipped pass (`skipped_reason` set) carries
/// zero counts and no artifacts.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct DreamPassResult {
    /// `"disabled"` (privacy switch off) | `"throttled"` (<6h since the last
    /// pass) | `"in-progress"` (another pass holds the lock); `None` when the
    /// pass ran.
    pub skipped_reason: Option<String>,
    /// Number of session files excerpted.
    pub scanned_sessions: u32,
    /// Projects that had memory entries in scope.
    pub projects: Vec<String>,
    /// Merge actions proposed.
    pub merge_proposed: u32,
    /// Remove actions proposed.
    pub remove_proposed: u32,
    /// Add actions proposed.
    pub add_proposed: u32,
    /// Skill candidates the L3 distill leg appended this pass (0 unless
    /// `dream_skill_distill_enabled` is on). They land in the review queue
    /// only — never promoted.
    pub candidates_detected: u32,
    /// Of the detected candidates, how many the LLM refine step rewrote and
    /// wrote back (`refined=true`).
    pub candidates_refined: u32,
    /// Ids of the proposals written this pass (review them via
    /// [`list_dream_proposals`]).
    pub proposal_ids: Vec<String>,
    /// Path of the merged markdown report, when one was written.
    pub report_path: Option<String>,
    /// Wall-clock duration of the pass in milliseconds.
    pub duration_ms: u64,
}

impl DreamPassResult {
    /// A zero-count result for a pass that did not run.
    fn skipped(reason: &str) -> Self {
        Self {
            skipped_reason: Some(reason.to_string()),
            ..Default::default()
        }
    }
}

/// Everything [`execute_dream_pass_inner`] produced: the user-facing result
/// plus the pieces only the caller needs (full stats for the state file, the
/// report timestamp for the inbox card).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DreamPassOutcome {
    /// User-facing summary (also the event payload).
    pub result: DreamPassResult,
    /// Full counters persisted into `DreamState::last_stats`.
    pub stats: DreamPassStats,
    /// Report timestamp used for the `report-{ts}.md` filename; also the day
    /// key for the inbox card dedup.
    pub report_ts: String,
}

/// Which actions a proposal apply landed vs skipped. An action is skipped
/// when its target memory entries have all vanished since the pass ran.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ApplyDreamOutcome {
    /// Action ids applied to the memory store.
    pub applied: Vec<String>,
    /// Action ids skipped (targets missing). The proposal is deleted either
    /// way — a partial apply discards the remaining actions with it
    /// (“应用所选，其余丢弃”).
    pub skipped: Vec<String>,
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

/// Delete one proposal file by id. Returns `false` when no file matched
/// (already gone is not an error — discard/apply racing each other is fine).
pub fn delete_proposal_in(dir: &Path, proposal_id: &str) -> Result<bool, String> {
    let Some(path) = find_proposal_file_in(dir, proposal_id) else {
        return Ok(false);
    };
    std::fs::remove_file(&path).map_err(|e| format!("remove {}: {e}", path.display()))?;
    Ok(true)
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

/// Extract a redacted, size-capped excerpt for one session through the core
/// session-query adapter. Takes the first `max_user_msgs` user texts, each
/// truncated to `per_msg_chars` chars and passed through [`redact`], plus
/// the deduplicated tool names in first-seen order. Archived sessions never
/// reach this function: they are already excluded by
/// `SessionQuery::list_recent` at the window level.
pub fn session_excerpt(
    query: &SessionQuery,
    session: &SessionRef,
    max_user_msgs: usize,
    per_msg_chars: usize,
) -> Result<SessionExcerpt, String> {
    let mut user_texts = Vec::new();
    for text in query
        .user_texts(&session.session_id)
        .map_err(|e| format!("session {}: {e}", session.session_id))?
    {
        if user_texts.len() >= max_user_msgs {
            break;
        }
        user_texts.push(redact(&truncate_chars(&text, per_msg_chars)));
    }

    let mut tool_names: Vec<String> = Vec::new();
    for call in query
        .tool_calls(&session.session_id)
        .map_err(|e| format!("session {}: {e}", session.session_id))?
    {
        if !tool_names.contains(&call.tool_name) {
            tool_names.push(call.tool_name);
        }
    }

    Ok(SessionExcerpt {
        session_id: session.session_id.to_string(),
        user_texts,
        tool_names,
    })
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

/// Persist a pass report as `{dir}/report-{ts}.md` (the merged, one-per-pass
/// report — reports live at the dreams root, not per project). Returns the
/// file path.
pub fn save_report_in(dir: &Path, ts: &str, content: &str) -> Result<PathBuf, String> {
    if !is_safe_filename(ts) {
        return Err(format!("Unsafe report ts: {ts:?}"));
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("Failed to create {}: {e}", dir.display()))?;
    let path = dir.join(format!("report-{ts}.md"));
    std::fs::write(&path, content).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(path)
}

/// Report timestamps available under `dir`, newest (largest unix ts) first.
/// Missing dir means "no reports yet", not an error.
pub fn list_reports_in(dir: &Path) -> Result<Vec<String>, String> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| format!("readdir {}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("readdir entry: {e}"))?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        if let Some(ts) = path.file_stem().and_then(|s| s.to_str()).and_then(|s| {
            s.strip_prefix("report-")
                .filter(|ts| !ts.is_empty() && is_safe_filename(ts))
        }) {
            out.push(ts.to_string());
        }
    }
    out.sort_by(|a, b| {
        let ka = a.parse::<i64>().unwrap_or(i64::MIN);
        let kb = b.parse::<i64>().unwrap_or(i64::MIN);
        kb.cmp(&ka)
    });
    Ok(out)
}

/// Read one report's markdown: `ts = None` means the newest report under
/// `dir`. Unknown timestamps error so the UI can show "not found".
pub fn read_report_in(dir: &Path, ts: Option<&str>) -> Result<String, String> {
    let resolved = match ts {
        Some(t) => t.to_string(),
        None => list_reports_in(dir)?
            .into_iter()
            .next()
            .ok_or_else(|| "No dream reports found".to_string())?,
    };
    if !is_safe_filename(&resolved) {
        return Err(format!("Unsafe report ts: {resolved:?}"));
    }
    let path = dir.join(format!("report-{resolved}.md"));
    std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))
}

// ============================================================================
// Throttle + single flight
// ============================================================================

/// Process-wide consolidation lock for the L2 dream pass: at most one pass
/// at a time, and a pass at most every [`DREAM_MIN_INTERVAL`]. The guard is
/// RAII — dropping it (including on the error path) releases the lock and
/// stamps the last-run time.
static DREAM_CONSOLIDATION_LOCK: OnceLock<ConsolidationLock> = OnceLock::new();

fn dream_lock() -> &'static ConsolidationLock {
    DREAM_CONSOLIDATION_LOCK.get_or_init(|| ConsolidationLock::new(DREAM_MIN_INTERVAL))
}

/// True when the state-file timestamp says a pass ran less than
/// [`DREAM_MIN_INTERVAL`] ago. Missing or unparseable timestamps never
/// throttle (the state file is shared and may predate this feature).
fn is_state_throttled(last_dream_at: Option<&str>, now: DateTime<Utc>) -> bool {
    last_dream_at
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .is_some_and(|ts| now - ts.with_timezone(&Utc) < DREAM_MIN_INTERVAL)
}

/// Clamp the session-window length to [`DREAM_MAX_DAYS_BACK`]. Applied in
/// [`execute_dream_pass`] — after gating, before gathering — so every entry
/// point (manual command, `/dream`, the Task 3 nightly scheduler) is capped.
fn clamp_days_back(days_back: u32) -> u32 {
    days_back.min(DREAM_MAX_DAYS_BACK)
}

/// Privacy-gate predicate for the dream pass: `Some("disabled")` when the
/// pass must not read anything — either the L2 master switch
/// (`dream_enabled`, default off) or the privacy main switch this whole
/// feature hangs under (`skill_detection_enabled`) is off — and `None` when
/// it may proceed to the throttle/single-flight rungs. Pure so the spec
/// §8.6 promise ("switches all off → the pass returns 0 and touches no
/// session file") is pinned by a unit test without a live AppHandle; the
/// `execute_dream_pass_in` gate tests cover the no-disk-read half.
fn dream_skip_reason(cfg: &crate::config::DesktopConfig) -> Option<&'static str> {
    if !cfg.dream_enabled || !cfg.skill_detection_enabled {
        Some("disabled")
    } else {
        None
    }
}

/// Skip reason when [`ConsolidationLock::try_acquire`] refused: a held guard
/// means a pass is genuinely running (`"in-progress"`); an idle lock means
/// only the 6h min interval blocked the attempt (`"throttled"`) — e.g. a
/// retry after a failed pass, whose guard `drop` stamped the timestamp
/// without any state-file record. Kept accurate locally because
/// `ConsolidationGuard::drop` unconditionally stamps (shannon-core is not
/// modified for this).
fn lock_skip_reason(lock: &ConsolidationLock) -> &'static str {
    if lock.is_in_progress() {
        "in-progress"
    } else {
        "throttled"
    }
}

// ============================================================================
// Prompt construction
// ============================================================================

/// One line per memory, keyed by entry id so the model can reference entries
/// exactly (same shape as `ConsolidationPrompt::build`, ids instead of
/// indices because proposals address entries across one run).
fn memory_list_lines(entries: &[MemoryEntry]) -> String {
    entries
        .iter()
        .map(|m| {
            format!(
                "[{}] Category: {} | Confidence: {:.2} | Created: {} | Accesses: {} | Content: {}",
                m.id,
                m.category,
                m.confidence,
                m.created_at.format("%Y-%m-%d %H:%M"),
                m.access_count,
                m.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Render excerpts into `### Session` sections, stopping (or cutting the last
/// section) once `max_chars` is reached. Redaction already happened inside
/// [`session_excerpt`]; this only sizes the block. Returns the rendered block
/// plus how many excerpts made it in whole or in part, so the caller can
/// warn when the budget squeezed sessions out of the prompt.
fn render_excerpt_block(excerpts: &[SessionExcerpt], max_chars: usize) -> (String, usize) {
    let mut out = String::new();
    let mut included = 0usize;
    for excerpt in excerpts {
        let used = out.chars().count();
        if used >= max_chars {
            break;
        }
        included += 1;
        let mut section = format!("### Session {}\n", excerpt.session_id);
        if !excerpt.tool_names.is_empty() {
            section.push_str(&format!("Tools: {}\n", excerpt.tool_names.join(", ")));
        }
        for text in &excerpt.user_texts {
            section.push_str(&format!("- {text}\n"));
        }
        section.push('\n');
        let budget = max_chars - used;
        if section.chars().count() > budget {
            out.push_str(&truncate_chars(&section, budget));
            break;
        }
        out.push_str(&section);
    }
    (out, included)
}

/// Build the dream consult prompt for one project: system = consolidation
/// rules plus the extended keep/merge/remove/add JSON protocol, user = the
/// project's memory list (ids, same format as `ConsolidationPrompt::build`)
/// followed by the shared redacted session excerpts, truncated so the user
/// prompt stays within [`DREAM_PROMPT_CHAR_BUDGET`].
pub(crate) fn build_dream_prompt(
    project: &str,
    entries: &[MemoryEntry],
    excerpts: &[SessionExcerpt],
) -> (String, String) {
    let system = format!(
        "You are a memory consolidation assistant for project \"{project}\". Analyze the \
         project's persisted memories against recent session excerpts and identify \
         duplicates, stale entries, merge candidates, and new insights worth persisting.\n\n\
         ## Consolidation Rules\n\n{}\n\n\
         ## Output Protocol\n\n\
         Respond with ONLY a JSON object (no prose, no markdown fences):\n\
         {{\n\
           \"keep\": [],\n\
           \"merge\": [[\"entry-id\", \"entry-id\"]],\n\
           \"remove\": [\"entry-id\"],\n\
           \"add\": [{{\"category\": \"preference|pattern|decision|error|context\", \
         \"content\": \"...\", \"confidence\": 0.8, \"source_session_ids\": [\"session-id\"], \
         \"verified\": false}}]\n\
         }}\n\n\
         Reference existing entries by their exact ids from the memory list; a merge group \
         needs at least two entries. Propose `add` only for insights genuinely worth \
         persisting. Every proposal is reviewed by the user before anything is written.",
        ConsolidationPrompt::build_rules()
    );

    let memories = memory_list_lines(entries);
    let mem_section = if memories.is_empty() {
        String::from("## Memories to Analyze\n\n(none)\n")
    } else {
        format!("## Memories to Analyze (project: {project})\n\n{memories}\n")
    };

    let excerpt_header = "\n## Recent Session Excerpts (redacted)\n\n";
    let remaining = DREAM_PROMPT_CHAR_BUDGET
        .saturating_sub(mem_section.chars().count() + excerpt_header.chars().count());
    let (excerpt_body, included) = if excerpts.is_empty() {
        ("(no recent sessions in the scan window)\n".to_string(), 0)
    } else {
        render_excerpt_block(excerpts, remaining)
    };
    if included < excerpts.len() {
        // Review finding #7: the memory section is unbounded, so a very
        // large store can saturate the budget and silently drop session
        // excerpts. Never fatal — but never silent either.
        tracing::warn!(
            memories_chars = mem_section.chars().count(),
            budget = DREAM_PROMPT_CHAR_BUDGET,
            included,
            total_excerpts = excerpts.len(),
            "dream: prompt budget squeezed session excerpts out of the consult prompt"
        );
    }

    let user = format!("{mem_section}{excerpt_header}{excerpt_body}");
    (system, user)
}

// ============================================================================
// LLM output → proposal
// ============================================================================

/// Parsed LLM response. `keep` is accepted by serde as an unknown field and
/// ignored — keeping is the default, there is nothing to record.
#[derive(Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
struct DreamLlmOutput {
    merge: Vec<Vec<String>>,
    remove: Vec<String>,
    add: Vec<DreamLlmAddEntry>,
}

/// `add` entry as the model writes it — every field defaulted so a partial
/// response parses instead of failing the whole pass.
#[derive(Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
struct DreamLlmAddEntry {
    category: String,
    content: String,
    confidence: f64,
    source_session_ids: Vec<String>,
}

/// Parse the model's response into [`DreamLlmOutput`]. Anything unparseable —
/// no JSON object, trailing prose, wrong shapes — yields the empty default:
/// a parse failure means "no proposals", never a failed pass.
fn parse_dream_llm_output(text: &str) -> DreamLlmOutput {
    let Some(start) = text.find('{') else {
        return DreamLlmOutput::default();
    };
    let Some(end) = text.rfind('}') else {
        return DreamLlmOutput::default();
    };
    if end <= start {
        return DreamLlmOutput::default();
    }
    serde_json::from_str(&text[start..=end]).unwrap_or_else(|e| {
        tracing::warn!(error = %e, "dream: LLM response is not a consolidation object");
        DreamLlmOutput::default()
    })
}

/// Clamp an LLM-supplied category to the memory vocabulary (same fallback as
/// `commands_memory::parse_category`: unknown → `context`).
fn normalize_dream_category(raw: &str) -> String {
    let trimmed = raw.trim().to_ascii_lowercase();
    match trimmed.as_str() {
        "preference" | "pattern" | "decision" | "error" | "context" => trimmed,
        _ => "context".to_string(),
    }
}

/// Stable, unique proposal id: `proposal-{unix_ms}`, suffixed `-2`, `-3`, …
/// within a run so two projects saved in the same millisecond never collide
/// (ids are filenames and `load_proposal_in` scans all project subdirs).
fn next_proposal_id(used: &mut HashSet<String>) -> String {
    let base = Utc::now().timestamp_millis();
    let mut candidate = format!("proposal-{base}");
    let mut n = 1;
    while used.contains(&candidate) {
        candidate = format!("proposal-{base}-{n}");
        n += 1;
    }
    used.insert(candidate.clone());
    candidate
}

// ============================================================================
// Verified minimal back-source (卡C)
// ============================================================================

/// Minimum distinct content-keyword hits, inside ONE claimed source
/// session's excerpt text, before an `add` action's `verified` flag is set.
pub(crate) const VERIFIED_MIN_TOKEN_HITS: usize = 2;

/// Common English function/content-free words stripped before the
/// back-source check — short, ubiquitous tokens ("the", "and", "for")
/// would substring-hit almost any excerpt text and inflate `verified`
/// falsely. Deliberately small and conservative; the length floors below
/// carry most of the filtering.
const VERIFIED_STOPWORDS: &[&str] = &[
    "about", "also", "and", "are", "been", "being", "but", "can", "could", "did", "do", "does",
    "for", "from", "had", "has", "have", "her", "here", "his", "into", "its", "just", "may",
    "might", "must", "new", "not", "now", "only", "onto", "our", "out", "over", "shall", "should",
    "some", "such", "than", "that", "the", "their", "then", "there", "these", "they", "this",
    "those", "too", "use", "used", "uses", "using", "was", "were", "what", "when", "where",
    "which", "while", "will", "with", "would", "you", "your",
];

/// Keyword tokens distilled from a proposed `add` content for the
/// back-source check: split on non-alphanumeric boundaries (unicode-aware —
/// a CJK run without separators stays one token, which the substring match
/// handles fine), lowercased, deduplicated, with stopword-ish short tokens
/// dropped: single chars, ASCII tokens under 3 chars ("on", "is"), and the
/// common-word list. Pure; heavily unit-tested.
pub(crate) fn content_keywords(content: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in content.split(|c: char| !c.is_alphanumeric()) {
        let token = raw.to_lowercase();
        let char_count = token.chars().count();
        if char_count < 2
            || (token.is_ascii() && char_count < 3)
            || VERIFIED_STOPWORDS.contains(&token.as_str())
        {
            continue;
        }
        if !out.contains(&token) {
            out.push(token);
        }
    }
    out
}

/// The minimal back-source check for one `add` action: at least
/// [`VERIFIED_MIN_TOKEN_HITS`] distinct [`content_keywords`] of the proposed
/// content must appear — case-insensitively — in the excerpt text of a
/// single one of the action's claimed `source_session_ids`. Hits scattered
/// across different sessions do not corroborate the provenance claim, so
/// they deliberately don't count. Pure; heavily unit-tested.
pub(crate) fn verify_add_against_excerpts(
    content: &str,
    source_session_ids: &[String],
    excerpts: &[SessionExcerpt],
) -> bool {
    if source_session_ids.is_empty() {
        return false;
    }
    let keywords = content_keywords(content);
    if keywords.len() < VERIFIED_MIN_TOKEN_HITS {
        return false;
    }
    for excerpt in excerpts {
        if !source_session_ids.contains(&excerpt.session_id) {
            continue;
        }
        let haystack = excerpt.user_texts.join("\n").to_lowercase();
        let hits = keywords
            .iter()
            .filter(|kw| haystack.contains(kw.as_str()))
            .count();
        if hits >= VERIFIED_MIN_TOKEN_HITS {
            return true;
        }
    }
    false
}

/// Turn raw LLM output into a review-gated [`DreamProposal`] for `project`.
///
/// Pure and total: parse failures or empty results produce a proposal with
/// zero actions (the caller skips saving those). Merge/remove ids are
/// filtered to entries that actually exist in `project`'s memory set and
/// deduplicated across actions, so a hallucinated or double-referenced id
/// can never reach the apply path. `add` entries get `verified` from the
/// minimal back-source check (`verify_add_against_excerpts`) over the
/// excerpts gathered for this pass — never from the model's own claim.
pub(crate) fn build_proposal_from_llm(
    project: &str,
    entries: &[MemoryEntry],
    excerpts: &[SessionExcerpt],
    llm_text: &str,
    proposal_id: &str,
    created_at: &str,
) -> DreamProposal {
    let out = parse_dream_llm_output(llm_text);
    let known: HashSet<&str> = entries.iter().map(|e| e.id.as_str()).collect();
    let mut used: HashSet<String> = HashSet::new();
    let mut actions: Vec<DreamAction> = Vec::new();

    for group in &out.merge {
        let mut ids: Vec<String> = Vec::new();
        for id in group {
            let id = id.trim();
            if id.is_empty()
                || !known.contains(id)
                || used.contains(id)
                || ids.iter().any(|seen| seen == id)
            {
                continue;
            }
            ids.push(id.to_string());
        }
        if ids.len() < 2 {
            continue;
        }
        used.extend(ids.iter().cloned());
        actions.push(DreamAction {
            id: format!("action-{}", actions.len() + 1),
            kind: DreamActionKind::Merge,
            rationale: format!("Merge {} duplicate entries into one", ids.len()),
            entry_ids: ids,
            add_entry: None,
        });
    }
    for id in &out.remove {
        let id = id.trim();
        if id.is_empty() || !known.contains(id) || used.contains(id) {
            continue;
        }
        used.insert(id.to_string());
        actions.push(DreamAction {
            id: format!("action-{}", actions.len() + 1),
            kind: DreamActionKind::Remove,
            rationale: "Remove outdated or contradicted entry".to_string(),
            entry_ids: vec![id.to_string()],
            add_entry: None,
        });
    }
    for add in out.add {
        let content = add.content.trim();
        if content.is_empty() {
            continue;
        }
        let mut source_session_ids: Vec<String> = Vec::new();
        for sid in add.source_session_ids {
            let sid = sid.trim();
            if !sid.is_empty() && !source_session_ids.iter().any(|s| s == sid) {
                source_session_ids.push(sid.to_string());
            }
        }
        actions.push(DreamAction {
            id: format!("action-{}", actions.len() + 1),
            kind: DreamActionKind::Add,
            rationale: "New insight distilled from recent sessions".to_string(),
            entry_ids: Vec::new(),
            add_entry: Some(DreamAddEntry {
                category: normalize_dream_category(&add.category),
                content: content.to_string(),
                confidence: add.confidence.clamp(0.0, 1.0),
                verified: verify_add_against_excerpts(content, &source_session_ids, excerpts),
                source_session_ids,
            }),
        });
    }

    DreamProposal {
        id: proposal_id.to_string(),
        project: project.to_string(),
        created_at: created_at.to_string(),
        actions,
        applied_ids: Vec::new(),
    }
}

// ============================================================================
// LLM seam + orchestration
// ============================================================================

/// The real model call, isolated so the orchestration can be tested with a
/// fake consult closure (no test ever hits the network). Same construction
/// as `commands_skill_candidates::refine_skill_candidate`.
async fn consult_llm(
    client_config: shannon_engine::api::types::LlmClientConfig,
    system: String,
    user: String,
) -> Result<String, String> {
    let client = shannon_engine::api::client::LlmClient::new(client_config);
    let messages = vec![shannon_engine::api::types::Message {
        role: "user".into(),
        content: shannon_engine::api::types::MessageContent::Text(user),
    }];
    let blocks = client
        .send_message(messages, None, Some(system))
        .await
        .map_err(|e| format!("dream LLM call failed: {e}"))?;
    let mut out = String::new();
    for block in blocks {
        if let shannon_engine::api::types::ContentBlock::Text { text } = block {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&text);
        }
    }
    Ok(out)
}

/// Core of the dream pass against injected directories and injected seams —
/// what makes the pipeline testable (tests pass fake closures and tempdirs;
/// the production wrapper passes the real ones). One consult per project that
/// has entries, then the optional L3 skill-distill leg; all consults run
/// before any artifact is written, so a transport failure mid-run leaves no
/// partial output. Response *parse* failures are not errors — the pass still
/// produces its report with zero proposals.
///
/// Seams:
/// - `consult` — the L2 model call per project (transport errors abort).
/// - `detect(days)` — the L3 heuristic detection + recording; returns the
///   newly appended candidates. Never called before every consult succeeded.
/// - `refine(candidate)` — the L3 per-candidate LLM refine + write-back;
///   `Some(text)` counts as refined, `None` (LLM failure) leaves the
///   candidate untouched.
///
/// L3 is best-effort by design: a detection error is logged and the pass
/// continues with zero candidate counts — losing a report to a candidates
/// JSONL hiccup would be worse than shipping one without L3 numbers.
#[allow(clippy::too_many_arguments)] // injected seams (consult/detect/refine) are the testability contract
pub(crate) async fn execute_dream_pass_inner<C, F, D, DF, R, RF>(
    days_back: u32,
    memory_store: &SharedMemoryStore,
    sessions_dir: &Path,
    dreams_dir: &Path,
    client_config: shannon_engine::api::types::LlmClientConfig,
    consult: C,
    detect: D,
    refine: R,
) -> Result<DreamPassOutcome, String>
where
    C: Fn(shannon_engine::api::types::LlmClientConfig, String, String) -> F,
    F: std::future::Future<Output = Result<String, String>>,
    D: Fn(u32) -> DF,
    DF: std::future::Future<
            Output = Result<Vec<crate::commands_skill_candidates::SkillCandidate>, String>,
        >,
    R: Fn(crate::commands_skill_candidates::SkillCandidate) -> RF,
    RF: std::future::Future<Output = Option<String>>,
{
    let started = Utc::now();

    // Inputs: memory entries grouped per project. The std RwLock guard is
    // confined to this block — never held across an await.
    refresh_shared_store(memory_store);
    let by_project: BTreeMap<String, Vec<MemoryEntry>> = {
        let guard = memory_store.read().map_err(|e| e.to_string())?;
        let mut grouped: BTreeMap<String, Vec<MemoryEntry>> = BTreeMap::new();
        for entry in guard.search("", None) {
            grouped
                .entry(entry.project.clone())
                .or_default()
                .push(entry);
        }
        grouped
    };
    let entries_reviewed: usize = by_project.values().map(Vec::len).sum();

    // Recent session excerpts (texts already redacted by `session_excerpt`),
    // read through the core session-query adapter — the same single source
    // skill detection uses. Archived sessions are excluded at the input
    // layer (`include_archived = false`).
    let query = SessionQuery::new(sessions_dir);
    let mut excerpts: Vec<SessionExcerpt> = Vec::new();
    for session in query
        .list_recent(days_back, false)
        .map_err(|e| format!("session query: {e}"))?
    {
        match session_excerpt(
            &query,
            &session,
            DEFAULT_MAX_USER_MSGS,
            DEFAULT_PER_MSG_CHARS,
        ) {
            Ok(excerpt) => excerpts.push(excerpt),
            Err(e) => tracing::warn!(
                session = %session.session_id,
                error = %e,
                "dream: skipping unreadable session"
            ),
        }
    }
    let scanned_sessions = excerpts.len();
    let redactions_applied: u32 = excerpts
        .iter()
        .flat_map(|x| x.user_texts.iter())
        .map(|text| text.matches(REDACTED_MARKER).count())
        .sum::<usize>() as u32;

    // Phase 1 — consult the model for every project. Any transport failure
    // propagates before a single artifact is written (and before the L3 leg
    // runs — no skill candidates are recorded for a failed pass either).
    let mut prompt_chars = 0u64;
    let mut consults: Vec<(&String, &Vec<MemoryEntry>, String)> = Vec::new();
    for (project, entries) in &by_project {
        let (system, user) = build_dream_prompt(project, entries, &excerpts);
        prompt_chars += (system.chars().count() + user.chars().count()) as u64;
        let text = consult(client_config.clone(), system, user).await?;
        consults.push((project, entries, text));
    }

    // Phase 1.5 — L3 skill distill: heuristic detection over the same
    // session window + a per-candidate LLM refine with write-back. Only
    // reached once every consult succeeded. Best-effort: a detection error
    // is logged and the pass continues with zero candidate counts; a refine
    // failure (None) just leaves that candidate untouched and uncounted.
    // Candidates land in the review queue only — there is no promote here.
    let mut candidates_detected = 0u32;
    let mut candidates_refined = 0u32;
    match detect(days_back).await {
        Ok(appended) => {
            candidates_detected = appended.len() as u32;
            for candidate in appended {
                if refine(candidate).await.is_some() {
                    candidates_refined += 1;
                }
            }
        }
        Err(e) => tracing::warn!(
            error = %e,
            "dream: L3 skill distill failed; continuing without candidate counts"
        ),
    }

    // Phase 2 — everything succeeded: build, count, persist.
    let mut used_ids: HashSet<String> = HashSet::new();
    let mut proposal_ids: Vec<String> = Vec::new();
    let mut merge_proposed = 0u32;
    let mut remove_proposed = 0u32;
    let mut add_proposed = 0u32;
    for (project, entries, text) in &consults {
        let proposal = build_proposal_from_llm(
            project,
            entries,
            &excerpts,
            text,
            &next_proposal_id(&mut used_ids),
            &started.to_rfc3339(),
        );
        if proposal.actions.is_empty() {
            continue;
        }
        for action in &proposal.actions {
            match action.kind {
                DreamActionKind::Merge => merge_proposed += 1,
                DreamActionKind::Remove => remove_proposed += 1,
                DreamActionKind::Add => add_proposed += 1,
            }
        }
        save_proposal_in(dreams_dir, shannon_core::memory::project_hash, &proposal)?;
        proposal_ids.push(proposal.id);
    }

    let stats = DreamPassStats {
        scanned_sessions: scanned_sessions as u32,
        entries_reviewed: entries_reviewed as u32,
        merge_proposed,
        remove_proposed,
        add_proposed,
        candidates_detected,
        candidates_refined,
        redactions_applied,
        duration_ms: (Utc::now() - started).num_milliseconds().max(0) as u64,
        projects: by_project.keys().cloned().collect(),
        token_estimate: prompt_chars / 4,
    };
    let report_ts = Utc::now().timestamp().to_string();
    let report_path = save_report_in(dreams_dir, &report_ts, &build_report_markdown(&stats))?;

    let result = DreamPassResult {
        skipped_reason: None,
        scanned_sessions: scanned_sessions as u32,
        projects: stats.projects.clone(),
        merge_proposed,
        remove_proposed,
        add_proposed,
        candidates_detected,
        candidates_refined,
        proposal_ids,
        report_path: Some(report_path.display().to_string()),
        duration_ms: stats.duration_ms,
    };
    Ok(DreamPassOutcome {
        result,
        stats,
        report_ts,
    })
}

/// Run one full dream pass against the live app: self-heal → privacy gates →
/// throttle/single-flight → gather → LLM → proposals + report + state +
/// inbox card + `dream-pass-finished` event. Shared by the manual command
/// and the (Task 3) nightly scheduler. Never reads a session or memory file
/// when a gate or throttle skips the pass; an LLM transport failure returns
/// `Err` with no artifacts written (the lock releases via guard drop).
pub(crate) async fn execute_dream_pass(
    app: tauri::AppHandle,
    days_back: u32,
) -> Result<DreamPassResult, String> {
    // The on-disk config is re-read per pass so a Settings toggle takes
    // effect without a restart; the real dirs resolve exactly where they
    // always did. Everything below runs against the injected `_in` seam.
    let cfg = crate::config::load_config();
    let dreams = dreams_dir()?;
    let desktop = crate::commands_skill_candidates::desktop_dir()?;
    execute_dream_pass_in(&app, days_back, &cfg, &dreams, &desktop).await
}

/// [`execute_dream_pass`] against an injected config + dreams/desktop
/// directories — the `_in` seam that makes the gate ladder testable without
/// touching `$HOME` or the on-disk config (`cfg` is a parameter, and the
/// skipped rungs below return before the sessions dir is even resolved).
/// The process-wide consolidation lock is still shared, deliberately: the
/// gate tests hold it to exercise the `"in-progress"` rung end-to-end.
pub(crate) async fn execute_dream_pass_in<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    days_back: u32,
    cfg: &crate::config::DesktopConfig,
    dreams: &Path,
    desktop: &Path,
) -> Result<DreamPassResult, String> {
    // Session window is capped centrally so every entry point is covered.
    let days_back = clamp_days_back(days_back);

    // 1. Expire stale proposals first — dreams dir only, always safe.
    self_heal_expired_proposals_in(dreams, DREAM_PROPOSAL_MAX_AGE_DAYS);

    // 2. Privacy gates. `dream_enabled` is the L2 master switch (default
    // off); `skill_detection_enabled` is the privacy main switch this whole
    // feature hangs under. Either off → skip without reading anything
    // (nothing below this line has been touched — no state file, no lock).
    if let Some(reason) = dream_skip_reason(cfg) {
        return Ok(DreamPassResult::skipped(reason));
    }

    // 3. Throttle: the state-file timestamp first (cheap), then the
    // in-process lock (single flight + its own 6h min interval). The guard
    // is held for the rest of the pass.
    if is_state_throttled(read_state_in(desktop).last_dream_at.as_deref(), Utc::now()) {
        return Ok(DreamPassResult::skipped("throttled"));
    }
    let Some(_lock_guard) = dream_lock().try_acquire() else {
        return Ok(DreamPassResult::skipped(lock_skip_reason(dream_lock())));
    };

    // 4–6. Gather inputs and consult the LLM (transport errors abort here,
    // before any artifact is written). The L3 skill-distill seams ride along:
    // detection reuses `detect_and_record` on the same sessions dir + the
    // shared inbox; each newly appended candidate is refined through the
    // shared LLM body and written back into the candidates JSONL. Both
    // closures carry `dream_skill_distill_enabled` (the pass's privacy gates
    // already guaranteed `skill_detection_enabled`) so `_inner` stays
    // config-agnostic and tests can swap in fakes.
    let state = app.state::<AppState>();
    let sessions_dir = crate::skill_pattern_detection::default_sessions_dir()?;
    let client_config = state.client_config.read().await.clone();

    let distill_enabled = cfg.dream_skill_distill_enabled;
    let detect = {
        let handle = app.clone();
        let inbox = state.inbox_store();
        let dir = sessions_dir.clone();
        move |days: u32| {
            let inbox = inbox.clone();
            let handle = handle.clone();
            let dir = dir.clone();
            async move {
                if !distill_enabled {
                    return Ok(Vec::new());
                }
                crate::skill_pattern_detection::detect_and_record(
                    &handle,
                    inbox.as_ref(),
                    &dir,
                    days,
                )
                .await
            }
        }
    };
    let refine = {
        let state_ref = state.inner();
        let desktop = desktop.to_path_buf();
        move |candidate: crate::commands_skill_candidates::SkillCandidate| {
            let desktop = desktop.clone();
            async move {
                // `None` (LLM failure) leaves the candidate untouched and
                // uncounted — the `?` on the Option is that exact contract.
                let text = crate::commands_skill_candidates::refine_candidate_text(
                    state_ref,
                    &candidate.proposed_name,
                    &candidate.proposed_trigger,
                    &candidate.procedure.join("\n"),
                )
                .await?;
                let procedure = crate::commands_skill_candidates::procedure_lines(&text);
                if let Err(e) = crate::commands_skill_candidates::mark_candidate_refined_in(
                    &desktop,
                    &candidate.id,
                    procedure,
                ) {
                    tracing::warn!(
                        candidate = %candidate.id,
                        error = %e,
                        "dream: candidate refine write-back failed"
                    );
                    return None;
                }
                Some(text)
            }
        }
    };

    let outcome = execute_dream_pass_inner(
        days_back,
        &state.memory_store,
        &sessions_dir,
        dreams,
        client_config,
        consult_llm,
        detect,
        refine,
    )
    .await
    .inspect_err(|_| {
        // Review finding #4: a failed pass must not disappear silently into
        // the throttle. `ConsolidationGuard::drop` stamps the 6h interval on
        // the way out (shannon-core is not modified for this), so every
        // retry within that window reports "throttled" — until the app
        // restarts or the interval elapses. Surface it in the logs next to
        // the transport error itself.
        tracing::warn!(
            "dream pass failed: the consolidation lock stamps its 6h interval on drop, \
             so retries within that window report \"throttled\" until it elapses"
        );
    })?;

    // 7. Products. The detection-state file is shared with other features:
    // re-read immediately before writing and touch only the two dream-owned
    // fields (see the module doc for the ownership contract).
    let mut shared_state = read_state_in(desktop);
    shared_state.last_dream_at = Some(Utc::now().to_rfc3339());
    shared_state.last_stats =
        Some(serde_json::to_value(&outcome.stats).map_err(|e| format!("serialize stats: {e}"))?);
    write_state_in(desktop, &shared_state)?;

    // Inbox card (day-deduped) and push event, both best-effort.
    crate::inbox_session_events::record_dream_report(
        state.inbox_store().as_ref(),
        app,
        &outcome.result,
        &outcome.report_ts,
    );
    let _ = app.emit(DREAM_PASS_FINISHED_EVENT, &outcome.result);
    Ok(outcome.result)
}

// ============================================================================
// Scheduler — dual track (卡C), default off (needs `dream_enabled`)
// ============================================================================

/// Local-hour window (01:00–05:59) in which the nightly dream pass may fire.
pub(crate) const NIGHT_WINDOW: std::ops::RangeInclusive<u32> = 1..=5;
/// How often the nightly scheduler wakes to re-check the fire conditions.
pub(crate) const NIGHT_CHECK_INTERVAL_SECS: u64 = 30 * 60;
/// Minimum age of the last dream pass before the nightly scheduler fires
/// again (mirrors the state-file `last_dream_at` check).
pub(crate) const NIGHT_MIN_INTERVAL_HOURS: i64 = 24;
/// Session-window length the scheduler tracks pass to [`execute_dream_pass`]
/// (manual entry points default to 3 instead).
pub(crate) const NIGHT_DAYS_BACK: u32 = 7;
/// Delay between app setup and the one-shot startup catch-up check (the
/// second track): long enough that the app has settled, short enough to
/// still catch machines that are rarely on during the 1–5 点 window.
pub(crate) const CATCHUP_DELAY_SECS: u64 = 10 * 60;

/// True when `hour` (local wall clock 0–23) is inside [`NIGHT_WINDOW`] —
/// the off-hours window in which the nightly dream pass may fire. Pure so
/// tests pin the boundaries without touching the clock.
pub(crate) fn is_night_hour(hour: u32) -> bool {
    NIGHT_WINDOW.contains(&hour)
}

/// True when the nightly pass may fire given the state-file timestamp: due
/// when the stamp is missing or unparseable (nothing to throttle on — the
/// file is shared and may predate this feature) and otherwise at least
/// [`NIGHT_MIN_INTERVAL_HOURS`] old. Pure; unit-tested.
pub(crate) fn is_state_due(last_dream_at: Option<&str>, now: DateTime<Utc>) -> bool {
    match last_dream_at.and_then(|s| DateTime::parse_from_rfc3339(s).ok()) {
        Some(ts) => now - ts.with_timezone(&Utc) >= Duration::hours(NIGHT_MIN_INTERVAL_HOURS),
        None => true,
    }
}

/// Pure fire predicate for the catch-up wake (unit-tested): the privacy
/// gates must be open (`skip_reason = None`), the last pass must be at
/// least [`NIGHT_MIN_INTERVAL_HOURS`] old, and no session query may be in
/// flight. The idle input is `SessionRegistry::any_querying` — the global
/// read added beside the existing per-session `is_querying`; without it
/// the fixed 10-minute startup delay would be the only idle proxy
/// (documented choice: the delay alone stays the fallback on runtimes
/// where the registry is unreachable).
pub(crate) fn is_catchup_due(
    skip_reason: Option<&str>,
    state_due: bool,
    any_querying: bool,
) -> bool {
    skip_reason.is_none() && state_due && !any_querying
}

/// Run one scheduled 7-day pass and log the outcome — the shared tail of
/// both tracks. `execute_dream_pass` re-applies its own gates, 6h throttle
/// and single-flight lock on top; everything here is log-only.
async fn run_scheduled_pass(app: &tauri::AppHandle, trigger: &str) {
    match execute_dream_pass(app.clone(), NIGHT_DAYS_BACK).await {
        Ok(result) => match result.skipped_reason {
            Some(reason) => {
                tracing::info!(trigger, reason, "dream: scheduled pass skipped")
            }
            None => tracing::info!(
                trigger,
                scanned = result.scanned_sessions,
                merge = result.merge_proposed,
                remove = result.remove_proposed,
                add = result.add_proposed,
                candidates = result.candidates_detected,
                "dream: scheduled pass completed"
            ),
        },
        Err(e) => tracing::warn!(trigger, error = %e, "dream: scheduled pass failed"),
    }
}

/// One nightly-window wake: fire a 7-day dream pass only when all of
/// these hold — the local wall-clock hour is inside [`NIGHT_WINDOW`],
/// `dream_enabled` is on (config re-read every wake so a Settings toggle
/// takes effect without a restart), and the shared state file's
/// `last_dream_at` is at least [`NIGHT_MIN_INTERVAL_HOURS`] old.
pub(crate) async fn maybe_night_dream(app: &tauri::AppHandle) {
    use chrono::Timelike;
    let cfg = crate::config::load_config();
    if dream_skip_reason(&cfg).is_some() {
        return;
    }
    if !is_night_hour(chrono::Local::now().hour()) {
        return;
    }
    let desktop = match crate::commands_skill_candidates::desktop_dir() {
        Ok(dir) => dir,
        Err(e) => {
            tracing::warn!(error = %e, "nightly dream: cannot resolve desktop dir");
            return;
        }
    };
    if !is_state_due(read_state_in(&desktop).last_dream_at.as_deref(), Utc::now()) {
        return;
    }
    run_scheduled_pass(app, "nightly").await;
}

/// One catch-up wake (the second scheduler track): fire a 7-day pass when
/// the privacy gates are open, the last pass is at least
/// [`NIGHT_MIN_INTERVAL_HOURS`] old — a machine that is never on during the
/// 1–5 点 window would otherwise never dream — and no session query is in
/// flight (the [`is_catchup_due`] idle signal).
pub(crate) async fn maybe_catchup_dream(app: &tauri::AppHandle) {
    let cfg = crate::config::load_config();
    let skip = dream_skip_reason(&cfg);
    let desktop = match crate::commands_skill_candidates::desktop_dir() {
        Ok(dir) => dir,
        Err(e) => {
            tracing::warn!(error = %e, "catch-up dream: cannot resolve desktop dir");
            return;
        }
    };
    let state_due = is_state_due(read_state_in(&desktop).last_dream_at.as_deref(), Utc::now());
    let any_querying = app.state::<AppState>().registry.any_querying().await;
    if !is_catchup_due(skip, state_due, any_querying) {
        tracing::info!(
            disabled = skip.is_some(),
            state_due,
            any_querying,
            "catch-up dream: conditions not met, skipping"
        );
        return;
    }
    run_scheduled_pass(app, "catch-up").await;
}

/// Run one scheduler wake inside its own task and await it: a panic in the
/// wake kills only that throwaway task — logged at warn — and the calling
/// loop lives on. Chosen over `std::panic::catch_unwind` because driving an
/// async body through `catch_unwind` needs `AssertUnwindSafe` assertions on
/// every borrowed capture, while a `JoinHandle` surfaces the same signal
/// (`Err(JoinError)`) with none of them.
async fn run_wake_isolated(wake: impl std::future::Future<Output = ()> + Send + 'static) {
    if let Err(e) = tauri::async_runtime::spawn(wake).await {
        tracing::warn!(error = %e, "dream scheduler wake panicked; scheduler survives");
    }
}

/// Spawn the detached nightly dream scheduler (track 1): wakes every 30
/// minutes and hands the wake to the crate-internal `maybe_night_dream`
/// check, panic-isolated per iteration (`run_wake_isolated`).
/// Detached like the routine scheduler, so it never blocks shutdown — the
/// async runtime simply dies with the process. `pub` because `main.rs` (the
/// binary crate) calls it from `setup`.
pub fn spawn_night_dream(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        tracing::info!(
            interval_secs = NIGHT_CHECK_INTERVAL_SECS,
            "nightly dream scheduler started"
        );
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(NIGHT_CHECK_INTERVAL_SECS)).await;
            let handle = app.clone();
            run_wake_isolated(async move { maybe_night_dream(&handle).await }).await;
        }
    });
}

/// Spawn the one-shot startup catch-up (track 2, 卡C): ~10 minutes after
/// setup, run one `maybe_catchup_dream` check — a 7-day pass fires when the
/// dream switch is on, the last pass is ≥24h old, and no query is running.
/// Dual-track with [`spawn_night_dream`]: the nightly window stays the
/// primary off-hours path, the catch-up covers machines that are not
/// running during it. `pub` because `main.rs` calls it from `setup`.
pub fn spawn_catchup_dream(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(CATCHUP_DELAY_SECS)).await;
        let handle = app.clone();
        run_wake_isolated(async move { maybe_catchup_dream(&handle).await }).await;
    });
}

// ============================================================================
// Apply (the only write path into the memory store)
// ============================================================================

/// Merge note appended to a kept entry's content when its group is applied.
fn merge_note(merged_count: usize, rationale: &str) -> String {
    format!(
        "\n\n[Merged {merged_count} duplicate {} by dream pass on {}: {rationale}]",
        if merged_count == 1 {
            "entry"
        } else {
            "entries"
        },
        Utc::now().format("%Y-%m-%d"),
    )
}

/// Apply one action to the store. Returns `false` when the action cannot
/// land (its target entries all vanished, or the add payload is empty) —
/// the caller records that as a skipped action.
fn apply_action(store: &mut MemoryStore, project: &str, action: &DreamAction) -> bool {
    match action.kind {
        DreamActionKind::Remove => {
            let mut removed_any = false;
            for id in &action.entry_ids {
                if store.delete(id).unwrap_or(false) {
                    removed_any = true;
                }
            }
            removed_any
        }
        DreamActionKind::Merge => {
            // Keep the highest-confidence entry of the group (first of ties,
            // in the action's id order); delete the rest; annotate the kept
            // content with the merge note.
            let mut found = 0usize;
            let mut kept_id: Option<String> = None;
            let mut kept_confidence = f64::NEG_INFINITY;
            for id in &action.entry_ids {
                match store.get(id) {
                    Some(entry) => {
                        found += 1;
                        if entry.confidence > kept_confidence {
                            kept_confidence = entry.confidence;
                            kept_id = Some(id.clone());
                        }
                    }
                    None => continue,
                }
            }
            if found < 2 || kept_id.is_none() {
                return false;
            }
            let kept_id = kept_id.unwrap_or_default();
            let mut merged_count = 0usize;
            for id in &action.entry_ids {
                if *id != kept_id && store.delete(id).unwrap_or(false) {
                    merged_count += 1;
                }
            }
            if let Some(entry) = store.get_mut(&kept_id) {
                entry
                    .content
                    .push_str(&merge_note(merged_count, &action.rationale));
            }
            true
        }
        DreamActionKind::Add => {
            let Some(add) = action.add_entry.as_ref() else {
                return false;
            };
            let content = add.content.trim();
            if content.is_empty() {
                return false;
            }
            let mut entry = MemoryEntry::new(project, parse_category(&add.category), content);
            entry.confidence = add.confidence.clamp(0.0, 1.0);
            entry.source_kind = Some(DREAM_SOURCE_KIND.to_string());
            // P2-4 provenance takes one session id as a string — the first
            // listed source (the field is display/lineage metadata, and the
            // full list stays on the reviewed proposal).
            entry.source_session_id = add.source_session_ids.first().cloned();
            store.add(entry).is_ok()
        }
    }
}

/// Best-effort journal write: persist `proposal` (whose `applied_ids` just
/// grew) back to its on-disk file. A failure is logged, never fatal — the
/// journal is a mitigation, and failing the apply over it would put the
/// real write (the store save) behind a weaker guarantee.
fn journal_applied_ids(proposal: &DreamProposal, path: &Path) {
    let json = match serde_json::to_string_pretty(proposal) {
        Ok(json) => json,
        Err(e) => {
            tracing::warn!(error = %e, "dream apply: cannot serialize proposal journal");
            return;
        }
    };
    if let Err(e) = std::fs::write(path, json) {
        tracing::warn!(
            path = %path.display(),
            error = %e,
            "dream apply: journal write failed"
        );
    }
}

/// Apply the selected actions of `proposal` to the store (the only write
/// path into the memory store this feature has) with the incremental
/// `applied_ids` journal (卡C 4a). Apply is not atomic — the proposal file
/// is deleted only after the store save succeeds — so every freshly applied
/// action is journaled into the proposal JSON immediately, and actions
/// already journaled are skipped on entry (reported as applied: they are
/// durably in the store from the earlier attempt). A mid-apply failure +
/// retry therefore no longer double-applies. Actions are applied in
/// proposal order; unknown requested ids are ignored; actions whose targets
/// are gone are reported as skipped in order. `path` is the proposal's
/// on-disk file (`None`: journal-free, used by pure store-level tests).
pub(crate) fn apply_actions_journaled_in(
    store: &mut MemoryStore,
    proposal: &mut DreamProposal,
    path: Option<&Path>,
    action_ids: &[String],
) -> ApplyDreamOutcome {
    let selected: HashSet<&str> = action_ids.iter().map(String::as_str).collect();
    // Snapshot so the loop can mutate `proposal.applied_ids` (the journal)
    // while still reading the action list.
    let actions: Vec<DreamAction> = proposal.actions.clone();
    let mut outcome = ApplyDreamOutcome::default();
    for action in &actions {
        if !selected.contains(action.id.as_str()) {
            continue;
        }
        // Journal skip: this id already landed (possibly in a crashed
        // attempt) — re-running it would double-apply (the double-add case).
        if proposal.applied_ids.contains(&action.id) {
            outcome.applied.push(action.id.clone());
            continue;
        }
        if apply_action(store, &proposal.project, action) {
            outcome.applied.push(action.id.clone());
            proposal.applied_ids.push(action.id.clone());
            if let Some(path) = path {
                journal_applied_ids(proposal, path);
            }
        } else {
            outcome.skipped.push(action.id.clone());
        }
    }
    outcome
}

// ============================================================================
// Tauri commands
// ============================================================================

/// Manual entry point (Memory panel button, `/dream`): run one dream pass.
/// Defaults to a 3-day session window, capped at [`DREAM_MAX_DAYS_BACK`]
/// (30) inside `execute_dream_pass`; the nightly scheduler (Task 3) calls
/// `execute_dream_pass` directly with 7.
#[tauri::command]
pub async fn run_dream_pass(
    app: tauri::AppHandle,
    // Injected by Tauri; `execute_dream_pass` resolves state from the app
    // handle so the nightly task can share the exact same body.
    _state: tauri::State<'_, AppState>,
    days_back: Option<u32>,
) -> Result<DreamPassResult, String> {
    execute_dream_pass(app, days_back.unwrap_or(DEFAULT_MANUAL_DAYS_BACK)).await
}

/// Every pending (review-gated) proposal across projects, newest first.
#[tauri::command]
pub async fn list_dream_proposals() -> Result<Vec<DreamProposal>, String> {
    list_proposals_in(&dreams_dir()?)
}

/// One pass report's markdown; `ts = None` reads the newest.
#[tauri::command]
pub async fn read_dream_report(ts: Option<String>) -> Result<String, String> {
    read_report_in(&dreams_dir()?, ts.as_deref())
}

/// The persisted dream state (`last_dream_at` + `last_stats`) for the
/// Memory panel's cold-start 「上次提炼」 line (卡C) — the state file was
/// previously write-only. Missing or corrupt file returns the default
/// (both fields `None`), which the UI renders as no line at all.
#[tauri::command]
pub async fn read_dream_state() -> Result<DreamState, String> {
    let desktop = crate::commands_skill_candidates::desktop_dir()?;
    Ok(read_state_in(&desktop))
}

/// Apply the selected actions of a proposal to the memory store, then delete
/// the proposal file — review is consumed as a unit, so a partial apply
/// discards the unselected actions (“应用所选，其余丢弃”). Every successful
/// action is journaled into the proposal file on the way (卡C 4a): if the
/// store save or the delete fails, a retry skips the already-applied ids
/// instead of double-applying.
#[tauri::command]
pub async fn apply_dream_proposal(
    _app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    proposal_id: String,
    action_ids: Vec<String>,
) -> Result<ApplyDreamOutcome, String> {
    let dir = dreams_dir()?;
    let path = find_proposal_file_in(&dir, &proposal_id)
        .ok_or_else(|| format!("Proposal {proposal_id} not found"))?;
    let mut proposal = load_proposal_in(&dir, &proposal_id)?;
    let outcome = {
        let store = &state.memory_store;
        let mut guard = store.write().map_err(|e| e.to_string())?;
        let outcome =
            apply_actions_journaled_in(&mut guard, &mut proposal, Some(&path), &action_ids);
        guard.save().map_err(|e| e.to_string())?;
        outcome
    };
    delete_proposal_in(&dir, &proposal_id)?;
    Ok(outcome)
}

/// Discard a proposal without touching the memory store — the shadow copy is
/// simply deleted.
#[tauri::command]
pub async fn discard_dream_proposal(
    _app: tauri::AppHandle,
    _state: tauri::State<'_, AppState>,
    proposal_id: String,
) -> Result<(), String> {
    let dir = dreams_dir()?;
    // 404-style error for an unknown id, matching the other commands.
    load_proposal_in(&dir, &proposal_id)?;
    delete_proposal_in(&dir, &proposal_id)?;
    Ok(())
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
            applied_ids: Vec::new(),
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
    // Session excerpts (real layout: fixtures through the session_log
    // writer — the retired flat *.json fixtures never matched production)
    // ------------------------------------------------------------------

    use shannon_core::session_log::SessionLogWriter;
    use shannon_types::session_event::{
        SessionEventBody, SessionStartPayload, ToolCallPayload, TurnEndPayload, TurnStartPayload,
        UserMessagePayload,
    };

    /// Seed a real session through the session_log writer: session/start,
    /// then for each `user_texts` entry a user prompt, then one framed turn
    /// per `(tool, arg_keys)` call. Returns the session id.
    fn seed_real_session(
        container: &Path,
        user_texts: &[&str],
        calls: &[(&str, &[&str])],
    ) -> uuid::Uuid {
        let id = uuid::Uuid::new_v4();
        let mut w = SessionLogWriter::open_layout(container, &id.to_string()).unwrap();
        w.record(SessionEventBody::SessionStart(SessionStartPayload {
            model: "test-model".into(),
            provider: None,
            cwd: Some("/proj".into()),
            app_version: None,
            ..Default::default()
        }));
        for text in user_texts {
            w.record(SessionEventBody::UserMessage(UserMessagePayload {
                source: UserMessagePayload::SOURCE_USER.into(),
                content: (*text).into(),
                attachment_count: 0,
            }));
        }
        for (i, (tool, keys)) in calls.iter().enumerate() {
            w.record(SessionEventBody::TurnStart(TurnStartPayload {
                query_id: None,
            }));
            let mut input = serde_json::Map::new();
            for k in *keys {
                input.insert((*k).to_string(), serde_json::Value::String("x".into()));
            }
            w.record(SessionEventBody::ToolCall(ToolCallPayload {
                tool_use_id: format!("u-{i}"),
                tool_name: (*tool).to_string(),
                arguments: serde_json::Value::Object(input).to_string(),
            }));
            w.record(SessionEventBody::TurnEnd(TurnEndPayload {
                reason: TurnEndPayload::REASON_COMPLETED.into(),
                usage: None,
                error: None,
            }));
        }
        w.close().unwrap();
        id
    }

    /// Excerpt one seeded session through the adapter.
    fn excerpt_of(
        query: &SessionQuery,
        id: &uuid::Uuid,
        max_user_msgs: usize,
        per_msg_chars: usize,
    ) -> SessionExcerpt {
        let session = SessionRef {
            session_id: *id,
            dir: query.container().join(id.to_string()),
            updated_at: Utc::now(),
        };
        session_excerpt(query, &session, max_user_msgs, per_msg_chars).unwrap()
    }

    #[test]
    fn session_excerpt_extracts_redacted_texts_and_tool_names() {
        let dir = tempdir().unwrap();
        let id = seed_real_session(
            dir.path(),
            &[
                "deploy with api_key: sk-secret1 please",
                "now set password=hunter2",
                "thanks",
            ],
            &[("bash", &["cmd"]), ("bash", &[]), ("read_file", &["path"])],
        );
        let query = SessionQuery::new(dir.path());
        let excerpt = excerpt_of(&query, &id, DEFAULT_MAX_USER_MSGS, DEFAULT_PER_MSG_CHARS);
        assert_eq!(excerpt.session_id, id.to_string());
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
        let texts: Vec<String> = (0..5)
            .map(|i| format!("msg-{i} {}", "长".repeat(30)))
            .collect();
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        let id = seed_real_session(dir.path(), &refs, &[]);

        let query = SessionQuery::new(dir.path());
        let excerpt = excerpt_of(&query, &id, 3, 10);
        assert_eq!(excerpt.session_id, id.to_string());
        assert_eq!(excerpt.user_texts.len(), 3);
        for (i, text) in excerpt.user_texts.iter().enumerate() {
            assert!(text.chars().count() <= 10, "truncated: {text}");
            assert!(text.starts_with(&format!("msg-{i} ")));
        }
    }

    #[test]
    fn session_excerpt_missing_log_reads_empty_and_corrupt_log_errors() {
        let dir = tempdir().unwrap();
        let query = SessionQuery::new(dir.path());
        // A session id with no log at all excerpts to an empty excerpt —
        // there is nothing to fail on.
        let ghost = uuid::Uuid::new_v4();
        let session = SessionRef {
            session_id: ghost,
            dir: dir.path().join(ghost.to_string()),
            updated_at: Utc::now(),
        };
        let excerpt = session_excerpt(
            &query,
            &session,
            DEFAULT_MAX_USER_MSGS,
            DEFAULT_PER_MSG_CHARS,
        )
        .unwrap();
        assert!(excerpt.user_texts.is_empty());
        assert!(excerpt.tool_names.is_empty());

        // A corrupt log line makes the session unreadable: an error the pass
        // logs and skips.
        let id = seed_real_session(dir.path(), &["hello"], &[]);
        let log = dir.path().join(id.to_string()).join("events.jsonl");
        let mut raw = std::fs::read_to_string(&log).unwrap();
        raw.push_str("{not json\n");
        std::fs::write(&log, raw).unwrap();
        let session = SessionRef {
            session_id: id,
            dir: dir.path().join(id.to_string()),
            updated_at: Utc::now(),
        };
        assert!(
            session_excerpt(
                &query,
                &session,
                DEFAULT_MAX_USER_MSGS,
                DEFAULT_PER_MSG_CHARS
            )
            .is_err()
        );
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

    // ------------------------------------------------------------------
    // Prompt construction (Task 2)
    // ------------------------------------------------------------------

    use shannon_core::memory::MemoryCategory;

    fn dream_entry(project: &str, content: &str, confidence: f64) -> MemoryEntry {
        let mut e = MemoryEntry::new(project, MemoryCategory::Decision, content);
        e.confidence = confidence;
        e
    }

    fn write_recent_session(dir: &Path, user_text: &str) -> uuid::Uuid {
        seed_real_session(dir, &[user_text], &[("bash", &[])])
    }

    #[test]
    fn dream_prompt_carries_rules_protocol_ids_and_excerpts() {
        let entries = vec![dream_entry("/work/app", "use rust", 0.9)];
        let excerpts = vec![SessionExcerpt {
            session_id: "sess-1".into(),
            user_texts: vec!["deploy with api_key: [REDACTED] please".into()],
            tool_names: vec!["bash".into()],
        }];
        let (system, user) = build_dream_prompt("/work/app", &entries, &excerpts);
        // System: consolidation rules + extended protocol shape.
        assert!(system.contains("DUPLICATE DETECTION"), "rules: {system}");
        assert!(system.contains("\"merge\""), "protocol: {system}");
        assert!(system.contains("\"add\""), "protocol: {system}");
        assert!(system.contains("/work/app"), "project: {system}");
        // User: memory list keyed by id + the redacted excerpt.
        let id = &entries[0].id;
        assert!(user.contains(&format!("[{id}]")), "memory line: {user}");
        assert!(user.contains("use rust"), "content: {user}");
        assert!(user.contains("### Session sess-1"), "excerpt: {user}");
        assert!(user.contains("Tools: bash"), "tools: {user}");
        assert!(user.contains("api_key: [REDACTED]"), "redacted: {user}");
    }

    #[test]
    fn dream_prompt_truncates_excerpts_to_budget_keeps_memories() {
        let entries = vec![dream_entry("/work/app", "keep my id visible", 0.9)];
        let big = "x".repeat(15_000);
        let excerpts: Vec<SessionExcerpt> = (0..3)
            .map(|i| SessionExcerpt {
                session_id: format!("sess-{i}"),
                user_texts: vec![big.clone()],
                tool_names: vec![],
            })
            .collect();
        let (_system, user) = build_dream_prompt("/work/app", &entries, &excerpts);
        assert!(
            user.chars().count() <= DREAM_PROMPT_CHAR_BUDGET,
            "user prompt over budget: {}",
            user.chars().count()
        );
        assert!(
            user.contains("keep my id visible"),
            "memories must never be truncated away"
        );
    }

    #[test]
    fn dream_prompt_includes_every_excerpt_within_budget() {
        let entries = vec![dream_entry("/work/app", "small store", 0.9)];
        let excerpts: Vec<SessionExcerpt> = (0..3)
            .map(|i| SessionExcerpt {
                session_id: format!("sess-{i}"),
                user_texts: vec![format!("text {i}")],
                tool_names: vec![],
            })
            .collect();
        let (_system, user) = build_dream_prompt("/work/app", &entries, &excerpts);
        for i in 0..3 {
            assert!(
                user.contains(&format!("### Session sess-{i}")),
                "excerpt {i} must survive a comfortable budget: {user}"
            );
        }
    }

    #[test]
    fn dream_prompt_squeezes_excerpts_out_when_memories_saturate_the_budget() {
        // Finding #7: the memory section is laid out first and never cut, so
        // a huge store consumes the whole budget — the excerpts are dropped
        // (a `tracing::warn!` now fires at the call site) while every memory
        // id stays referenceable.
        let huge = dream_entry(
            "/work/app",
            &"x".repeat(DREAM_PROMPT_CHAR_BUDGET + 1_000),
            0.9,
        );
        let huge_id = huge.id.clone();
        let excerpts = vec![SessionExcerpt {
            session_id: "sess-squeezed".into(),
            user_texts: vec!["should not appear".into()],
            tool_names: vec![],
        }];
        let (_system, user) = build_dream_prompt("/work/app", &[huge], &excerpts);
        assert!(
            user.contains(&huge_id),
            "memory ids are never dropped: {}",
            user.chars().count()
        );
        assert!(
            !user.contains("### Session"),
            "saturated budget squeezes the excerpt block out entirely: {}",
            user.chars().count()
        );
        assert!(!user.contains("should not appear"));
    }

    // ------------------------------------------------------------------
    // LLM output → proposal
    // ------------------------------------------------------------------

    fn llm_add(category: &str, content: &str, confidence: f64) -> serde_json::Value {
        serde_json::json!({
            "category": category,
            "content": content,
            "confidence": confidence,
            "source_session_ids": ["sess-1"],
            "verified": true,
        })
    }

    #[test]
    fn build_proposal_filters_unknown_ids_and_normalizes_adds() {
        let entries = vec![
            dream_entry("proj", "alpha", 0.9),
            dream_entry("proj", "beta", 0.5),
            dream_entry("proj", "gamma", 0.7),
        ];
        let ids: Vec<String> = entries.iter().map(|e| e.id.clone()).collect();
        let text = format!(
            "{{\"keep\": [0], \"merge\": [[{0}, {0}], [{0}, {1}], [{2}, \"ghost\"]], \
             \"remove\": [\"ghost\", {2}], \"add\": [{3}, {4}, {5}]}}",
            serde_json::to_string(&ids[0]).unwrap(),
            serde_json::to_string(&ids[1]).unwrap(),
            serde_json::to_string(&ids[2]).unwrap(),
            llm_add("WEIRD", "insight one", 1.7),
            llm_add("pattern", "   ", 0.5),
            llm_add("decision", "insight two", 0.4),
        );

        let proposal = build_proposal_from_llm(
            "proj",
            &entries,
            &[],
            &text,
            "proposal-1",
            "2026-09-25T00:00:00+00:00",
        );
        // merge: self-dup collapses to 1 → dropped; [0,1] kept; ghost dropped.
        // remove: ghost filtered, gamma kept. add: empty content dropped,
        // two valid adds land.
        assert_eq!(proposal.actions.len(), 4, "{text}");
        assert_eq!(proposal.actions[0].kind, DreamActionKind::Merge);
        assert_eq!(
            proposal.actions[0].entry_ids,
            vec![ids[0].clone(), ids[1].clone()]
        );
        assert_eq!(
            proposal.actions[0].rationale,
            "Merge 2 duplicate entries into one"
        );
        assert_eq!(proposal.actions[1].kind, DreamActionKind::Remove);
        assert_eq!(proposal.actions[1].entry_ids, vec![ids[2].clone()]);
        assert_eq!(proposal.actions[2].kind, DreamActionKind::Add);
        let add = proposal.actions[2].add_entry.as_ref().unwrap();
        assert_eq!(add.category, "context", "unknown category normalized");
        assert_eq!(add.content, "insight one");
        assert_eq!(add.confidence, 1.0, "confidence clamped");
        assert!(!add.verified, "the model never pre-verifies");
        assert_eq!(add.source_session_ids, vec!["sess-1"]);
        assert_eq!(
            proposal.actions[3].add_entry.as_ref().unwrap().content,
            "insight two"
        );
        // Sequential action ids.
        for (i, action) in proposal.actions.iter().enumerate() {
            assert_eq!(action.id, format!("action-{}", i + 1));
        }
        // The removed entry id must not also appear in a merge group.
        assert!(
            proposal
                .actions
                .iter()
                .all(|a| a.kind != DreamActionKind::Merge || !a.entry_ids.contains(&ids[2]))
        );
    }

    #[test]
    fn build_proposal_from_garbage_yields_no_actions() {
        let entries = vec![dream_entry("proj", "alpha", 0.9)];
        for text in [
            "",
            "no json at all",
            "{broken",
            "{\"merge\": \"not-a-list\"}",
        ] {
            let proposal = build_proposal_from_llm(
                "proj",
                &entries,
                &[],
                text,
                "proposal-1",
                "2026-01-01T00:00:00+00:00",
            );
            assert!(
                proposal.actions.is_empty(),
                "expected no actions for {text:?}"
            );
        }
    }

    #[test]
    fn proposal_ids_are_unique_within_a_run() {
        let mut used = HashSet::new();
        let ms = Utc::now().timestamp_millis();
        used.insert(format!("proposal-{ms}"));
        used.insert(format!("proposal-{ms}-1"));
        assert_eq!(next_proposal_id(&mut used), format!("proposal-{ms}-2"));
        let fresh = next_proposal_id(&mut used);
        assert!(fresh.starts_with("proposal-"));
    }

    // ------------------------------------------------------------------
    // Verified minimal back-source (卡C)
    // ------------------------------------------------------------------

    fn excerpt_for(session_id: &str, texts: &[&str]) -> SessionExcerpt {
        SessionExcerpt {
            session_id: session_id.into(),
            user_texts: texts.iter().map(|s| (*s).to_string()).collect(),
            tool_names: vec![],
        }
    }

    #[test]
    fn content_keywords_strip_stopwords_and_short_ascii_tokens() {
        // "the"/"for" are stopwords, "new" too; 2-char ASCII tokens dropped.
        assert_eq!(
            content_keywords("The new API for key handling: go"),
            vec!["api".to_string(), "key".to_string(), "handling".to_string()]
        );
        // CJK: 2-char words are meaningful and kept; an unseparated run is
        // one token (the substring match handles that).
        assert_eq!(
            content_keywords("使用 PostgreSQL 部署"),
            vec![
                "使用".to_string(),
                "postgresql".to_string(),
                "部署".to_string()
            ]
        );
        // Case folding + dedup.
        assert_eq!(
            content_keywords("Rust rust RUST!"),
            vec!["rust".to_string()]
        );
        // Stopword-only content leaves nothing usable.
        assert!(content_keywords("the and with for of to").is_empty());
    }

    #[test]
    fn verify_add_requires_two_keyword_hits_in_one_claimed_session() {
        let excerpts = vec![
            excerpt_for("sess-1", &["We Ships things on Thursdays, unless delayed"]),
            excerpt_for("sess-2", &["deploy often"]),
        ];
        let claimed = vec!["sess-1".to_string()];
        assert!(
            verify_add_against_excerpts("User ships on Thursdays", &claimed, &excerpts),
            "ships + thursdays both hit sess-1 → verified"
        );
        // Exactly one hit in the claimed session is not enough.
        assert!(
            !verify_add_against_excerpts("User ships often", &claimed, &excerpts),
            "only 'ships' hits sess-1 ('often' lives in sess-2)"
        );
        // Hits split across two claimed sessions do not corroborate: each
        // single session tops out at one hit.
        let both = vec!["sess-1".to_string(), "sess-2".to_string()];
        assert!(!verify_add_against_excerpts(
            "ships often",
            &both,
            &excerpts
        ));
    }

    #[test]
    fn verify_add_empty_or_unknown_sources_stay_unverified() {
        let excerpts = vec![excerpt_for("sess-1", &["ships thursdays ships thursdays"])];
        // No claimed sources at all.
        assert!(!verify_add_against_excerpts(
            "ships thursdays",
            &[],
            &excerpts
        ));
        // Claimed id that is not among this pass's excerpts.
        let claimed = vec!["sess-9".to_string()];
        assert!(!verify_add_against_excerpts(
            "ships thursdays",
            &claimed,
            &excerpts
        ));
        // Content with fewer usable keywords than the hit floor.
        assert!(!verify_add_against_excerpts(
            "the of to",
            &claimed,
            &excerpts
        ));
    }

    #[test]
    fn verify_add_handles_unicode_content_case_insensitively() {
        let excerpts = vec![excerpt_for("sess-1", &["我们用 PostgreSQL 部署数据库"])];
        let claimed = vec!["sess-1".to_string()];
        assert!(
            verify_add_against_excerpts("PostgreSQL 部署 is the norm", &claimed, &excerpts),
            "postgresql + 部署 both hit, case-folded"
        );
        // A single CJK run is one distinct token — under the two-hit floor.
        assert!(!verify_add_against_excerpts(
            "部署数据库",
            &claimed,
            &excerpts
        ));
        // Two distinct CJK tokens both present → verified.
        assert!(verify_add_against_excerpts(
            "PostgreSQL 部署",
            &claimed,
            &excerpts
        ));
    }

    #[test]
    fn build_proposal_sets_verified_from_backsource_check_never_from_model() {
        let entries = vec![dream_entry("proj", "alpha", 0.9)];
        let excerpts = vec![excerpt_for("sess-1", &["Ships on Thursdays, usually"])];
        // Both LLM adds claim verified:true (see `llm_add`) — the flag on
        // the built proposal must come only from the back-source check.
        let text = format!(
            "{{\"add\": [{}, {}]}}",
            llm_add("pattern", "User ships on Thursdays", 0.8),
            llm_add("pattern", "Untethered widget claim", 0.8),
        );
        let proposal = build_proposal_from_llm(
            "proj",
            &entries,
            &excerpts,
            &text,
            "proposal-1",
            "2026-09-25T00:00:00+00:00",
        );
        let adds: Vec<&DreamAddEntry> = proposal
            .actions
            .iter()
            .filter_map(|a| a.add_entry.as_ref())
            .collect();
        assert_eq!(adds.len(), 2);
        assert!(
            adds[0].verified,
            "two keyword hits in the claimed source session"
        );
        assert!(
            !adds[1].verified,
            "no back-source corroboration → stays false"
        );
    }

    // ------------------------------------------------------------------
    // Throttle
    // ------------------------------------------------------------------

    #[test]
    fn state_throttle_needs_fresh_rfc3339_timestamp() {
        let now = Utc::now();
        assert!(!is_state_throttled(None, now), "no state → no throttle");
        assert!(
            !is_state_throttled(Some("not a timestamp"), now),
            "garbage → no throttle"
        );
        let recent = (now - Duration::hours(1)).to_rfc3339();
        assert!(is_state_throttled(Some(&recent), now), "<6h → throttled");
        let old = (now - Duration::hours(7)).to_rfc3339();
        assert!(!is_state_throttled(Some(&old), now), "≥6h → allowed");
    }

    #[test]
    fn days_back_is_capped_at_thirty() {
        assert_eq!(clamp_days_back(0), 0);
        assert_eq!(clamp_days_back(DEFAULT_MANUAL_DAYS_BACK), 3);
        assert_eq!(clamp_days_back(DREAM_MAX_DAYS_BACK), 30, "boundary passes");
        assert_eq!(clamp_days_back(31), 30);
        assert_eq!(clamp_days_back(u32::MAX), 30, "stray huge value clamped");
    }

    #[test]
    fn lock_skip_reason_distinguishes_running_from_interval_blocked() {
        // Local lock — the process-global static is never touched from tests.
        let lock = ConsolidationLock::new(DREAM_MIN_INTERVAL);
        // Idle lock refusing acquisition = only the min interval blocked the
        // attempt (the failed-pass retry case): must NOT read "in-progress".
        assert_eq!(lock_skip_reason(&lock), "throttled");
        // Held guard → a pass is genuinely running.
        let guard = lock.try_acquire().unwrap();
        assert_eq!(lock_skip_reason(&lock), "in-progress");
        drop(guard);
        // Guard dropped stamps the timestamp: interval-blocked again, and a
        // retry really is refused until the interval elapses.
        assert_eq!(lock_skip_reason(&lock), "throttled");
        assert!(
            lock.try_acquire().is_none(),
            "6h min interval still blocks the retry"
        );
    }

    // ------------------------------------------------------------------
    // Privacy gate + full gate ladder (execute_dream_pass_in)
    // ------------------------------------------------------------------

    /// Desktop config with the three dream switches set explicitly.
    fn dream_cfg(
        dream_enabled: bool,
        skill_detection_enabled: bool,
    ) -> crate::config::DesktopConfig {
        crate::config::DesktopConfig {
            dream_enabled,
            skill_detection_enabled,
            dream_skill_distill_enabled: false,
            ..Default::default()
        }
    }

    /// Mock-runtime handle for the gate tests: the skipped rungs return
    /// before the handle is ever used, so no `AppState` needs managing.
    fn mock_handle() -> tauri::AppHandle<tauri::test::MockRuntime> {
        tauri::test::mock_app().handle().clone()
    }

    #[test]
    fn dream_skip_reason_gates_on_either_switch() {
        assert_eq!(dream_skip_reason(&dream_cfg(true, true)), None);
        assert_eq!(
            dream_skip_reason(&dream_cfg(false, true)),
            Some("disabled"),
            "dream_enabled off (the default) → disabled"
        );
        assert_eq!(
            dream_skip_reason(&dream_cfg(true, false)),
            Some("disabled"),
            "skill_detection_enabled off → disabled"
        );
        assert_eq!(
            dream_skip_reason(&dream_cfg(false, false)),
            Some("disabled")
        );
    }

    #[tokio::test]
    async fn dream_pass_gate_all_switches_off_reports_disabled_and_touches_nothing() {
        // Spec §8.6: switches all off → the pass returns 0 and does not read
        // a single session file. Both dirs are tempdirs, so "untouched"
        // asserts the whole pass had no side effect on disk at all.
        let app = mock_handle();
        let cfg = dream_cfg(false, false);
        let dreams = tempdir().unwrap();
        let desktop = tempdir().unwrap();
        // A pre-existing shared state file must survive untouched too.
        std::fs::write(
            desktop.path().join(DETECTION_STATE_FILE),
            r#"{"last_stats": {"scanned_sessions": 9}}"#,
        )
        .unwrap();
        let before_dreams = snapshot_dir(dreams.path());
        let before_desktop = snapshot_dir(desktop.path());

        let result = execute_dream_pass_in(&app, 3, &cfg, dreams.path(), desktop.path())
            .await
            .unwrap();

        assert_eq!(result.skipped_reason.as_deref(), Some("disabled"));
        assert_eq!(result.scanned_sessions, 0);
        assert!(result.projects.is_empty());
        assert!(result.proposal_ids.is_empty());
        assert!(result.report_path.is_none());
        assert_eq!(snapshot_dir(dreams.path()), before_dreams);
        assert_eq!(snapshot_dir(desktop.path()), before_desktop);
    }

    #[tokio::test]
    async fn dream_pass_reports_in_progress_then_throttled_for_a_retry() {
        // Finding #3, end-to-end: the "second concurrent run reports
        // in-progress" story, driven through the real gate ladder with the
        // same process-wide lock `execute_dream_pass` acquires. (Deliberate
        // use of the global lock: no other test touches it, and both rungs
        // return before the handle's state is ever consulted.)
        let app = mock_handle();
        let cfg = dream_cfg(true, true);
        let dreams = tempdir().unwrap();
        let desktop = tempdir().unwrap();

        // First pass holding the lock → concurrent second call must see
        // "in-progress", not "throttled".
        let first = dream_lock().try_acquire().expect("fresh process lock");
        let second = execute_dream_pass_in(&app, 3, &cfg, dreams.path(), desktop.path())
            .await
            .unwrap();
        assert_eq!(second.skipped_reason.as_deref(), Some("in-progress"));
        drop(first);

        // The dropped guard stamped the 6h interval (also finding #4's
        // failed-pass story): the immediate retry reports "throttled".
        let retry = execute_dream_pass_in(&app, 3, &cfg, dreams.path(), desktop.path())
            .await
            .unwrap();
        assert_eq!(retry.skipped_reason.as_deref(), Some("throttled"));
    }

    #[tokio::test]
    async fn dream_pass_reports_throttled_from_the_state_file() {
        // The state-file rung fires on its own (fresh `last_dream_at`
        // written into the tempdir desktop dir), before the lock is even
        // consulted.
        let app = mock_handle();
        let cfg = dream_cfg(true, true);
        let dreams = tempdir().unwrap();
        let desktop = tempdir().unwrap();
        write_state_in(
            desktop.path(),
            &DreamState {
                last_dream_at: Some(Utc::now().to_rfc3339()),
                last_stats: None,
            },
        )
        .unwrap();

        let result = execute_dream_pass_in(&app, 3, &cfg, dreams.path(), desktop.path())
            .await
            .unwrap();
        assert_eq!(result.skipped_reason.as_deref(), Some("throttled"));
        assert_eq!(result.scanned_sessions, 0);
        assert!(read_state_in(desktop.path()).last_dream_at.is_some());
    }

    // ------------------------------------------------------------------
    // Nightly scheduler (pure fire-condition helpers)
    // ------------------------------------------------------------------

    #[test]
    fn night_hour_window_boundaries() {
        for hour in [1, 2, 3, 4, 5] {
            assert!(is_night_hour(hour), "{hour} must be inside the window");
        }
        for hour in [0, 6, 12, 18, 23] {
            assert!(!is_night_hour(hour), "{hour} must be outside the window");
        }
        assert_eq!(*NIGHT_WINDOW.start(), 1);
        assert_eq!(*NIGHT_WINDOW.end(), 5);
    }

    #[test]
    fn state_due_requires_twenty_four_hour_old_stamp() {
        let now = Utc::now();
        // No stamp (or a corrupt one) never blocks the nightly pass.
        assert!(is_state_due(None, now));
        assert!(is_state_due(Some("not a timestamp"), now));
        // 23h ago → still fresh; exactly 24h and 25h → due.
        let fresh = (now - Duration::hours(23)).to_rfc3339();
        assert!(!is_state_due(Some(&fresh), now), "<24h → not due");
        let exact = (now - Duration::hours(24)).to_rfc3339();
        assert!(is_state_due(Some(&exact), now), "exactly 24h → due");
        let old = (now - Duration::hours(25)).to_rfc3339();
        assert!(is_state_due(Some(&old), now));
        // A timestamp in the future is never due.
        let future = (now + Duration::hours(2)).to_rfc3339();
        assert!(!is_state_due(Some(&future), now));
    }

    #[test]
    fn catchup_due_requires_open_gates_due_state_and_idle() {
        assert!(
            is_catchup_due(None, true, false),
            "gates open + ≥24h old + no query → due"
        );
        assert!(
            !is_catchup_due(Some("disabled"), true, false),
            "privacy gate closed → never due, even when idle and overdue"
        );
        assert!(
            !is_catchup_due(None, false, false),
            "last pass <24h ago → not due"
        );
        assert!(
            !is_catchup_due(None, true, true),
            "a live session query blocks the catch-up"
        );
        assert!(
            !is_catchup_due(Some("disabled"), false, true),
            "no condition combination bypasses a closed gate"
        );
    }

    #[tokio::test]
    async fn wake_isolation_swallows_a_panicking_wake() {
        // The scheduler loop must survive a panicking wake: the error lands
        // in the JoinHandle (logged by `run_wake_isolated`), the panic does
        // not propagate to the caller.
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {})); // keep test output pristine
        run_wake_isolated(async { panic!("deliberate: wake isolation check") }).await;
        std::panic::set_hook(prev);
    }

    // ------------------------------------------------------------------
    // Reports on disk
    // ------------------------------------------------------------------

    #[test]
    fn report_save_list_read_round_trip() {
        let dir = tempdir().unwrap();
        save_report_in(dir.path(), "100", "older report").unwrap();
        save_report_in(dir.path(), "200", "newer report").unwrap();

        let listed = list_reports_in(dir.path()).unwrap();
        assert_eq!(listed, vec!["200", "100"], "newest first");

        assert_eq!(read_report_in(dir.path(), None).unwrap(), "newer report");
        assert_eq!(
            read_report_in(dir.path(), Some("100")).unwrap(),
            "older report"
        );
        assert!(read_report_in(dir.path(), Some("999")).is_err());
        assert!(read_report_in(dir.path(), Some("../escape")).is_err());
        assert!(save_report_in(dir.path(), "../escape", "x").is_err());

        let empty = tempdir().unwrap();
        assert!(
            list_reports_in(&empty.path().join("nope"))
                .unwrap()
                .is_empty()
        );
        assert!(read_report_in(&empty.path(), None).is_err());
    }

    #[test]
    fn delete_proposal_removes_file_once() {
        let dir = tempdir().unwrap();
        let proposal = sample_proposal("proposal-del", "/work/app", "2026-09-01T00:00:00+00:00");
        save_proposal_in(dir.path(), test_hash, &proposal).unwrap();
        assert!(delete_proposal_in(dir.path(), "proposal-del").unwrap());
        assert!(load_proposal_in(dir.path(), "proposal-del").is_err());
        assert!(
            !delete_proposal_in(dir.path(), "proposal-del").unwrap(),
            "gone is not an error"
        );
    }

    // ------------------------------------------------------------------
    // Orchestration (injected LLM, tempdirs — never the network)
    // ------------------------------------------------------------------

    use std::time::SystemTime;

    /// (bytes, mtime) of every file under `dir` — the non-destructive
    /// assertion baseline for the memory store.
    fn snapshot_dir(dir: &Path) -> BTreeMap<String, (Vec<u8>, SystemTime)> {
        let mut out = BTreeMap::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            out.insert(
                entry.file_name().to_string_lossy().to_string(),
                (
                    std::fs::read(&path).unwrap(),
                    entry.metadata().unwrap().modified().unwrap(),
                ),
            );
        }
        out
    }

    /// Seed a real per-project JSONL store into `dir` and open the shared
    /// handle over it (the same seam `commands_memory` uses).
    fn seed_shared_store(dir: &Path, entries: Vec<MemoryEntry>) -> SharedMemoryStore {
        let mut seed = MemoryStore::new(dir.to_path_buf());
        for entry in entries {
            seed.add(entry).unwrap();
        }
        seed.save().unwrap();
        crate::commands_memory::open_shared_store_at(dir.to_path_buf())
    }

    fn canned_consult(
        response: &str,
    ) -> impl Fn(
        shannon_engine::api::types::LlmClientConfig,
        String,
        String,
    ) -> std::future::Ready<Result<String, String>>
    + '_ {
        let body = response.to_string();
        move |_cfg, _system, _user| std::future::ready(Ok(body.clone()))
    }

    /// No-op L3 detection seam: distill disabled / nothing found.
    fn no_detect() -> impl Fn(
        u32,
    ) -> std::future::Ready<
        Result<Vec<crate::commands_skill_candidates::SkillCandidate>, String>,
    > {
        |_days| std::future::ready(Ok(Vec::new()))
    }

    /// No-op L3 refine seam: every candidate unrefined.
    fn no_refine()
    -> impl Fn(crate::commands_skill_candidates::SkillCandidate) -> std::future::Ready<Option<String>>
    {
        |_candidate| std::future::ready(None)
    }

    /// A freshly detected (unrefined) candidate as the L3 detector appends it.
    fn l3_candidate(id: &str) -> crate::commands_skill_candidates::SkillCandidate {
        crate::commands_skill_candidates::SkillCandidate {
            id: id.into(),
            detected_at: Utc::now().to_rfc3339(),
            occurrence_count: 3,
            example_session_ids: vec!["sess-1".into()],
            proposed_name: "bash".into(),
            proposed_trigger: "Detected bash call recurring across 3 session(s)".into(),
            procedure: vec!["Invoke bash with the same argument shape".into()],
            source_tool_calls: vec![],
            refined: false,
        }
    }

    #[tokio::test]
    async fn dream_pass_inner_writes_proposals_report_and_leaves_memories_untouched() {
        let mem_dir = tempdir().unwrap();
        let entries = vec![
            dream_entry("/work/app", "use rust for tooling", 0.9),
            dream_entry("/work/app", "prefer rust tooling", 0.5),
            dream_entry("/work/app", "retire the old script", 0.7),
        ];
        let ids: Vec<String> = entries.iter().map(|e| e.id.clone()).collect();
        let store = seed_shared_store(mem_dir.path(), entries);
        let before = snapshot_dir(mem_dir.path());
        assert!(!before.is_empty(), "seed wrote jsonl files");

        let sessions = tempdir().unwrap();
        write_recent_session(sessions.path(), "api_key: sk-1 and password: hunter2");
        let dreams = tempdir().unwrap();

        let canned = format!(
            "{{\"keep\": [], \"merge\": [[{0}, {1}]], \"remove\": [{2}], \"add\": [{3}]}}",
            serde_json::to_string(&ids[0]).unwrap(),
            serde_json::to_string(&ids[1]).unwrap(),
            serde_json::to_string(&ids[2]).unwrap(),
            llm_add("pattern", "ships on Thursdays", 0.8),
        );

        let outcome = execute_dream_pass_inner(
            3,
            &store,
            sessions.path(),
            dreams.path(),
            shannon_engine::api::types::LlmClientConfig::default(),
            canned_consult(&canned),
            no_detect(),
            no_refine(),
        )
        .await
        .unwrap();

        // Result fields.
        assert_eq!(outcome.result.skipped_reason, None);
        assert_eq!(outcome.result.scanned_sessions, 1);
        assert_eq!(outcome.result.projects, vec!["/work/app"]);
        assert_eq!(outcome.result.merge_proposed, 1);
        assert_eq!(outcome.result.remove_proposed, 1);
        assert_eq!(outcome.result.add_proposed, 1);
        assert_eq!(outcome.result.candidates_detected, 0);
        assert_eq!(outcome.result.candidates_refined, 0);
        assert_eq!(outcome.result.proposal_ids.len(), 1);
        assert!(
            outcome
                .result
                .report_path
                .as_ref()
                .is_some_and(|p| Path::new(p).is_file())
        );

        // Proposal on disk under the real project hash, matching the result.
        let saved = load_proposal_in(dreams.path(), &outcome.result.proposal_ids[0]).unwrap();
        assert_eq!(saved.project, "/work/app");
        assert_eq!(saved.actions.len(), 3);
        assert!(
            dreams
                .path()
                .join(shannon_core::memory::project_hash("/work/app"))
                .join(format!("{}.json", saved.id))
                .is_file()
        );

        // Stats + report.
        assert_eq!(outcome.stats.entries_reviewed, 3);
        assert_eq!(outcome.stats.scanned_sessions, 1);
        assert_eq!(outcome.stats.redactions_applied, 2, "two masked values");
        assert!(outcome.stats.token_estimate > 0);
        let report = read_report_in(dreams.path(), Some(&outcome.report_ts)).unwrap();
        assert!(report.contains("Sessions scanned: 1"), "{report}");
        assert!(report.contains("Merge actions proposed: 1"), "{report}");

        // Non-destructive: the memory store was neither rewritten nor touched.
        let after = snapshot_dir(mem_dir.path());
        assert_eq!(
            before, after,
            "memory files must be byte- and mtime-identical"
        );
    }

    #[tokio::test]
    async fn dream_pass_inner_parse_failure_still_produces_report() {
        let mem_dir = tempdir().unwrap();
        let store = seed_shared_store(mem_dir.path(), vec![dream_entry("/work/app", "solo", 0.9)]);
        let sessions = tempdir().unwrap();
        let dreams = tempdir().unwrap();

        let outcome = execute_dream_pass_inner(
            3,
            &store,
            sessions.path(),
            dreams.path(),
            shannon_engine::api::types::LlmClientConfig::default(),
            canned_consult("I would suggest merging some entries, honestly."),
            no_detect(),
            no_refine(),
        )
        .await
        .unwrap();

        assert_eq!(outcome.result.skipped_reason, None);
        assert!(outcome.result.proposal_ids.is_empty(), "no proposals");
        assert_eq!(outcome.result.merge_proposed, 0);
        assert!(
            read_report_in(dreams.path(), None).is_ok(),
            "report still written"
        );
    }

    #[tokio::test]
    async fn dream_pass_inner_llm_failure_errors_without_artifacts() {
        let mem_dir = tempdir().unwrap();
        let store = seed_shared_store(mem_dir.path(), vec![dream_entry("/work/app", "solo", 0.9)]);
        let sessions = tempdir().unwrap();
        let dreams = tempdir().unwrap();

        let consult =
            |_cfg: shannon_engine::api::types::LlmClientConfig, _system: String, _user: String| {
                std::future::ready(Err::<String, String>("connection refused".to_string()))
            };
        let err = execute_dream_pass_inner(
            3,
            &store,
            sessions.path(),
            dreams.path(),
            shannon_engine::api::types::LlmClientConfig::default(),
            consult,
            no_detect(),
            no_refine(),
        )
        .await
        .unwrap_err();

        assert!(err.contains("connection refused"));
        assert!(
            std::fs::read_dir(dreams.path()).unwrap().next().is_none(),
            "no proposal or report may survive a failed pass"
        );
    }

    #[test]
    fn dream_pass_inner_without_memories_still_reports() {
        // Synchronous smoke: empty store → zero-count report, no LLM consult
        // (the closure panics if called, proving projects without entries
        // are never consulted).
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let mem_dir = tempdir().unwrap();
            let store = seed_shared_store(mem_dir.path(), Vec::new());
            let sessions = tempdir().unwrap();
            let dreams = tempdir().unwrap();
            let outcome = execute_dream_pass_inner(
                3,
                &store,
                sessions.path(),
                dreams.path(),
                shannon_engine::api::types::LlmClientConfig::default(),
                |_cfg, _system, _user| -> std::future::Ready<Result<String, String>> {
                    panic!("no consult may run without memory entries")
                },
                no_detect(),
                no_refine(),
            )
            .await
            .unwrap();
            assert_eq!(outcome.result.projects, Vec::<String>::new());
            assert!(outcome.result.proposal_ids.is_empty());
            assert!(read_report_in(dreams.path(), None).is_ok());
        });
    }

    #[tokio::test]
    async fn dream_pass_inner_l3_detects_refines_and_counts_candidates() {
        let mem_dir = tempdir().unwrap();
        let store = seed_shared_store(mem_dir.path(), vec![dream_entry("/work/app", "solo", 0.9)]);
        let sessions = tempdir().unwrap();
        let dreams = tempdir().unwrap();
        // Candidates JSONL in its own tempdir — the fake refine writes back
        // through the same `mark_candidate_refined_in` seam the production
        // closure uses, so the test never touches the real ~/.shannon.
        let candidates_dir = tempdir().unwrap();
        for id in ["sig-1", "sig-2"] {
            crate::commands_skill_candidates::append_candidate_in(
                candidates_dir.path(),
                l3_candidate(id),
            )
            .unwrap();
        }

        let detect =
            |_days: u32| std::future::ready(Ok(vec![l3_candidate("sig-1"), l3_candidate("sig-2")]));
        let candidates_path = candidates_dir.path().to_path_buf();
        let refine = move |candidate: crate::commands_skill_candidates::SkillCandidate| {
            let candidates_path = candidates_path.clone();
            async move {
                // Mirror the production closure: only "sig-1" refines
                // successfully; "sig-2" simulates an LLM failure (None).
                if candidate.id != "sig-1" {
                    return None;
                }
                let text = "1. run bash\n2. check output".to_string();
                crate::commands_skill_candidates::mark_candidate_refined_in(
                    &candidates_path,
                    &candidate.id,
                    crate::commands_skill_candidates::procedure_lines(&text),
                )
                .ok()?;
                Some(text)
            }
        };

        let outcome = execute_dream_pass_inner(
            3,
            &store,
            sessions.path(),
            dreams.path(),
            shannon_engine::api::types::LlmClientConfig::default(),
            canned_consult("no proposals in here"),
            detect,
            refine,
        )
        .await
        .unwrap();

        // Counts land in result + stats + report.
        assert_eq!(outcome.result.candidates_detected, 2);
        assert_eq!(outcome.result.candidates_refined, 1);
        assert_eq!(outcome.stats.candidates_detected, 2);
        assert_eq!(outcome.stats.candidates_refined, 1);
        let report = read_report_in(dreams.path(), Some(&outcome.report_ts)).unwrap();
        assert!(report.contains("Skill candidates detected: 2"), "{report}");
        assert!(report.contains("Skill candidates refined: 1"), "{report}");

        // The refined candidate was written back (procedure + refined=true);
        // the failed one is untouched.
        let jsonl =
            std::fs::read_to_string(candidates_dir.path().join("skill-candidates.jsonl")).unwrap();
        let refined: crate::commands_skill_candidates::SkillCandidate = jsonl
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .into_iter()
            .find(|c: &crate::commands_skill_candidates::SkillCandidate| c.id == "sig-1")
            .unwrap();
        assert!(refined.refined);
        assert_eq!(refined.procedure, vec!["1. run bash", "2. check output"]);
        let untouched: crate::commands_skill_candidates::SkillCandidate = jsonl
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .into_iter()
            .find(|c: &crate::commands_skill_candidates::SkillCandidate| c.id == "sig-2")
            .unwrap();
        assert!(!untouched.refined);
        assert_eq!(untouched.procedure.len(), 1);
    }

    #[tokio::test]
    async fn dream_pass_inner_l3_detection_error_never_fails_the_pass() {
        let mem_dir = tempdir().unwrap();
        let store = seed_shared_store(mem_dir.path(), vec![dream_entry("/work/app", "solo", 0.9)]);
        let sessions = tempdir().unwrap();
        let dreams = tempdir().unwrap();

        let detect = |_days: u32| {
            std::future::ready(Err::<
                Vec<crate::commands_skill_candidates::SkillCandidate>,
                String,
            >("candidates disk full".to_string()))
        };
        let outcome = execute_dream_pass_inner(
            3,
            &store,
            sessions.path(),
            dreams.path(),
            shannon_engine::api::types::LlmClientConfig::default(),
            canned_consult("no proposals in here"),
            detect,
            no_refine(),
        )
        .await
        .unwrap();

        assert_eq!(outcome.result.skipped_reason, None);
        assert_eq!(outcome.result.candidates_detected, 0);
        assert_eq!(outcome.result.candidates_refined, 0);
        assert!(
            read_report_in(dreams.path(), None).is_ok(),
            "report still written despite the L3 failure"
        );
    }

    #[test]
    fn l3_disabled_seams_yield_zero_candidate_counts() {
        // Mirrors `dream_skill_distill_enabled = false`: execute_dream_pass
        // hands _inner no-op seams, so every count stays zero.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let mem_dir = tempdir().unwrap();
            let store =
                seed_shared_store(mem_dir.path(), vec![dream_entry("/work/app", "solo", 0.9)]);
            let sessions = tempdir().unwrap();
            let dreams = tempdir().unwrap();
            let outcome = execute_dream_pass_inner(
                3,
                &store,
                sessions.path(),
                dreams.path(),
                shannon_engine::api::types::LlmClientConfig::default(),
                canned_consult("no proposals in here"),
                no_detect(),
                no_refine(),
            )
            .await
            .unwrap();
            assert_eq!(outcome.result.candidates_detected, 0);
            assert_eq!(outcome.result.candidates_refined, 0);
        });
    }

    // ------------------------------------------------------------------
    // Apply (the only write path)
    // ------------------------------------------------------------------

    fn apply_proposal(actions: Vec<DreamAction>) -> DreamProposal {
        DreamProposal {
            id: "proposal-apply".into(),
            project: "proj".into(),
            created_at: "2026-09-25T00:00:00+00:00".into(),
            actions,
            applied_ids: Vec::new(),
        }
    }

    fn dream_add_action(id: &str) -> DreamAction {
        DreamAction {
            id: id.into(),
            kind: DreamActionKind::Add,
            entry_ids: Vec::new(),
            add_entry: Some(DreamAddEntry {
                category: "pattern".into(),
                content: "ships on Thursdays".into(),
                confidence: 0.6,
                source_session_ids: vec!["sess-1".into(), "sess-2".into()],
                verified: false,
            }),
            rationale: "New insight distilled from recent sessions".into(),
        }
    }

    #[test]
    fn apply_lands_merge_remove_and_add_with_dream_provenance() {
        let dir = tempdir().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let mut kept = dream_entry("proj", "use rust for tooling", 0.9);
        let dropped_dup = dream_entry("proj", "prefer rust tooling", 0.5);
        let stale = dream_entry("proj", "retire the old script", 0.7);
        for e in [&kept, &dropped_dup, &stale] {
            store.add(e.clone()).unwrap();
        }

        let mut pending = apply_proposal(vec![
            DreamAction {
                id: "action-1".into(),
                kind: DreamActionKind::Merge,
                entry_ids: vec![kept.id.clone(), dropped_dup.id.clone()],
                add_entry: None,
                rationale: "Same tooling preference".into(),
            },
            DreamAction {
                id: "action-2".into(),
                kind: DreamActionKind::Remove,
                entry_ids: vec![stale.id.clone()],
                add_entry: None,
                rationale: "Superseded".into(),
            },
            dream_add_action("action-3"),
        ]);

        let outcome = apply_actions_journaled_in(
            &mut store,
            &mut pending,
            None,
            &["action-1".into(), "action-2".into(), "action-3".into()],
        );
        assert_eq!(outcome.applied, vec!["action-1", "action-2", "action-3"]);
        assert!(outcome.skipped.is_empty());

        // Merge kept the higher-confidence entry, deleted the other, and
        // appended the merge note.
        let merged = store.get(&kept.id).unwrap();
        assert!(merged.content.starts_with("use rust for tooling"));
        assert!(
            merged
                .content
                .contains("[Merged 1 duplicate entry by dream pass"),
            "merge note: {}",
            merged.content
        );
        assert!(merged.content.contains("Same tooling preference"));
        assert!(store.get(&dropped_dup.id).is_none(), "duplicate deleted");
        assert!(store.get(&stale.id).is_none(), "stale deleted");

        // Add landed with dream provenance.
        let added: Vec<MemoryEntry> = store
            .search("", None)
            .into_iter()
            .filter(|e| e.source_kind.as_deref() == Some(DREAM_SOURCE_KIND))
            .collect();
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].content, "ships on Thursdays");
        assert_eq!(added[0].project, "proj");
        assert_eq!(added[0].category, MemoryCategory::Pattern);
        assert!((added[0].confidence - 0.6).abs() < f64::EPSILON);
        assert_eq!(added[0].source_session_id.as_deref(), Some("sess-1"));

        // And the whole thing persists (apply is expected to save after).
        store.save().unwrap();
        let mut check = MemoryStore::new(dir.path().to_path_buf());
        check.load().unwrap();
        assert!(check.get(&kept.id).is_some(), "merge survived save/load");
        assert!(
            check
                .search("", None)
                .iter()
                .any(|e| e.source_kind.as_deref() == Some(DREAM_SOURCE_KIND)),
            "dream entry survived save/load"
        );
    }

    #[test]
    fn apply_skips_actions_whose_targets_vanished_and_ignores_unselected() {
        let dir = tempdir().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let live = dream_entry("proj", "still here", 0.9);
        let live_id = live.id.clone();
        store.add(live).unwrap();

        let mut proposal = apply_proposal(vec![
            DreamAction {
                id: "action-1".into(),
                kind: DreamActionKind::Merge,
                entry_ids: vec![live_id.clone(), "ghost-1".into()],
                add_entry: None,
                rationale: "half gone".into(),
            },
            DreamAction {
                id: "action-2".into(),
                kind: DreamActionKind::Remove,
                entry_ids: vec!["ghost-2".into()],
                add_entry: None,
                rationale: "gone".into(),
            },
            dream_add_action("action-3"),
            DreamAction {
                id: "action-4".into(),
                kind: DreamActionKind::Remove,
                entry_ids: vec![live_id.clone()],
                add_entry: None,
                rationale: "live".into(),
            },
        ]);

        // "action-999" is not in the proposal — ignored, not skipped.
        let outcome = apply_actions_journaled_in(
            &mut store,
            &mut proposal,
            None,
            &[
                "action-1".into(),
                "action-2".into(),
                "action-4".into(),
                "action-999".into(),
            ],
        );
        assert_eq!(outcome.applied, vec!["action-4"]);
        assert_eq!(outcome.skipped, vec!["action-1", "action-2"]);
        // action-3 was not selected — nothing added.
        assert!(
            !store
                .search("", None)
                .iter()
                .any(|e| e.source_kind.as_deref() == Some(DREAM_SOURCE_KIND))
        );
        assert!(store.get(&live_id).is_none(), "selected remove landed");
    }

    // ------------------------------------------------------------------
    // Apply journal (卡C 4a — retry after a mid-apply failure)
    // ------------------------------------------------------------------

    /// Count memory entries materialized by dream adds in `store`.
    fn dream_added_count(store: &MemoryStore) -> usize {
        store
            .search("", None)
            .iter()
            .filter(|e| e.source_kind.as_deref() == Some(DREAM_SOURCE_KIND))
            .count()
    }

    #[test]
    fn apply_journal_persists_applied_ids_after_each_action() {
        let dir = tempdir().unwrap();
        let mut store = MemoryStore::new(dir.path().join("mem"));
        let proposal = apply_proposal(vec![
            dream_add_action("action-1"),
            dream_add_action("action-2"),
        ]);
        save_proposal_in(dir.path(), test_hash, &proposal).unwrap();
        let path = dir
            .path()
            .join(test_hash(&proposal.project))
            .join("proposal-apply.json");

        let mut pending = load_proposal_in(dir.path(), "proposal-apply").unwrap();
        let outcome = apply_actions_journaled_in(
            &mut store,
            &mut pending,
            Some(&path),
            &["action-1".into(), "action-2".into()],
        );
        assert_eq!(outcome.applied, vec!["action-1", "action-2"]);
        assert_eq!(dream_added_count(&store), 2);

        // The journal reached the real on-disk file through the `_in` seam.
        let on_disk = load_proposal_in(dir.path(), "proposal-apply").unwrap();
        assert_eq!(on_disk.applied_ids, vec!["action-1", "action-2"]);
    }

    #[test]
    fn apply_journal_retry_after_store_save_failure_never_double_applies() {
        // Crash window: journal writes land per action, the store save at
        // the end does not — the proposal file survives with the full
        // journal while the store lost everything.
        let dir = tempdir().unwrap();
        let mut store = MemoryStore::new(dir.path().join("mem"));
        let proposal = apply_proposal(vec![
            dream_add_action("action-1"),
            dream_add_action("action-2"),
        ]);
        save_proposal_in(dir.path(), test_hash, &proposal).unwrap();
        let path = dir
            .path()
            .join(test_hash(&proposal.project))
            .join("proposal-apply.json");

        let mut pending = load_proposal_in(dir.path(), "proposal-apply").unwrap();
        apply_actions_journaled_in(
            &mut store,
            &mut pending,
            Some(&path),
            &["action-1".into(), "action-2".into()],
        );
        // (store save "fails" here — simulated by retrying against a fresh
        // store, exactly what the on-disk state looks like after the crash.)

        let mut retried = load_proposal_in(dir.path(), "proposal-apply").unwrap();
        let mut fresh = MemoryStore::new(dir.path().join("mem"));
        let outcome = apply_actions_journaled_in(
            &mut fresh,
            &mut retried,
            Some(&path),
            &["action-1".into(), "action-2".into()],
        );
        assert_eq!(
            outcome.applied,
            vec!["action-1", "action-2"],
            "journaled ids still count as applied (they are durable)"
        );
        assert_eq!(
            outcome.skipped,
            Vec::<String>::new(),
            "no action may be re-run into a duplicate"
        );
        assert_eq!(
            dream_added_count(&fresh),
            0,
            "double-add is the bug: journaled actions must be skipped"
        );
    }

    #[test]
    fn apply_journal_partial_journal_applies_only_the_rest_once() {
        // Crash window: action-1's journal write landed, the store save did
        // not. The retry applies only action-2 — action-1 exactly once less,
        // action-2 exactly once.
        let dir = tempdir().unwrap();
        let mut store = MemoryStore::new(dir.path().join("mem"));
        let proposal = apply_proposal(vec![
            dream_add_action("action-1"),
            dream_add_action("action-2"),
        ]);
        save_proposal_in(dir.path(), test_hash, &proposal).unwrap();
        let path = dir
            .path()
            .join(test_hash(&proposal.project))
            .join("proposal-apply.json");

        let mut partial = load_proposal_in(dir.path(), "proposal-apply").unwrap();
        partial.applied_ids = vec!["action-1".into()];
        let outcome = apply_actions_journaled_in(
            &mut store,
            &mut partial,
            Some(&path),
            &["action-1".into(), "action-2".into()],
        );
        assert_eq!(outcome.applied, vec!["action-1", "action-2"]);
        assert_eq!(
            dream_added_count(&store),
            1,
            "only action-2 landed in the store; action-1 was journal-skipped"
        );
        // The journal on disk now covers both.
        let on_disk = load_proposal_in(dir.path(), "proposal-apply").unwrap();
        assert_eq!(on_disk.applied_ids, vec!["action-1", "action-2"]);
    }

    #[test]
    fn legacy_proposal_json_without_applied_ids_parses_with_default() {
        // Pre-卡C proposal files have no `applied_ids` field — serde default
        // keeps them loadable (and re-appliable only for unjournaled ids).
        let dir = tempdir().unwrap();
        let project_dir = dir.path().join(test_hash("/work/app"));
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(
            project_dir.join("proposal-legacy.json"),
            r#"{"id":"proposal-legacy","project":"/work/app","created_at":"2026-09-01T00:00:00+00:00","actions":[]}"#,
        )
        .unwrap();
        let loaded = load_proposal_in(dir.path(), "proposal-legacy").unwrap();
        assert!(loaded.applied_ids.is_empty(), "missing field → default");
    }
}
