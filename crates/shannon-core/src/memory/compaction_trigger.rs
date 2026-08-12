//! Periodic compaction trigger for the curated memory store (ADR-0010 C5').
//!
//! [`AutoDreamService`](super::auto_dream::AutoDreamService) is recreated per
//! query, so the trigger keeps its schedule (last-compaction time + session
//! count per project) in a sidecar JSON next to the memory files. A compaction
//! pass runs when **either** ~24 h of wall-clock has elapsed **or** ≥ 5 sessions
//! have accumulated for the project — Claude Code's curated-memory cadence.
//!
//! The compaction itself is the rule-based [`MemoryConsolidator`] (dedupe +
//! stale + per-category caps) followed by token-budget pruning, all persisted
//! through [`MemoryStore::save`]'s multi-agent-safe reload-reconcile.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::consolidator::MemoryConsolidator;
use super::error::MemoryError;
use super::store::MemoryStore;
use super::types::SessionMemoryConfig;

/// Default wall-clock interval between compactions for one project (~24 h).
pub const DEFAULT_MAX_AGE: Duration = Duration::hours(24);
/// Default session count that forces a compaction regardless of age.
pub const DEFAULT_MAX_SESSIONS: u32 = 5;
/// Default injected-memory budget in approximate tokens (~4 chars/token).
pub const DEFAULT_TOKEN_BUDGET: usize = 2000;

/// Sidecar filename holding the trigger schedule, next to the JSONL files.
const STATE_FILENAME: &str = "compaction-state.json";

/// Summary of a single compaction pass (ADR-0010 C5').
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactSummary {
    /// Near-duplicate entries merged (higher-confidence kept).
    pub duplicates_merged: usize,
    /// Entries older than the TTL removed.
    pub stale_removed: usize,
    /// Entries pruned to fit the injection token budget.
    pub budget_pruned: usize,
    /// Store size before compaction.
    pub before_count: usize,
    /// Store size after compaction.
    pub after_count: usize,
}

/// Per-project trigger schedule, persisted in a sidecar so it survives the
/// per-query lifecycle of `AutoDreamService`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompactionState {
    /// project -> last compaction timestamp (UTC).
    #[serde(default)]
    pub last_compaction_at: HashMap<String, DateTime<Utc>>,
    /// project -> sessions seen since the last compaction.
    #[serde(default)]
    pub session_count: HashMap<String, u32>,
}

/// Schedules and runs periodic memory-store compaction (ADR-0010 C5').
pub struct MemoryCompactionTrigger {
    max_age: Duration,
    max_sessions: u32,
    token_budget: usize,
    state_path: PathBuf,
}

fn sidecar_path(storage_path: &Path) -> PathBuf {
    storage_path.join(STATE_FILENAME)
}

impl MemoryCompactionTrigger {
    /// Build a trigger with an explicit schedule and sidecar `state_path`.
    pub fn new(
        state_path: PathBuf,
        max_age: Duration,
        max_sessions: u32,
        token_budget: usize,
    ) -> Self {
        Self {
            max_age,
            max_sessions,
            token_budget,
            state_path,
        }
    }

    /// Build a trigger whose sidecar lives next to `store`'s JSONL files, with
    /// the default schedule (24 h / 5 sessions / 2000 tokens).
    pub fn for_store(store: &MemoryStore) -> Self {
        Self::new(
            sidecar_path(store.storage_path()),
            DEFAULT_MAX_AGE,
            DEFAULT_MAX_SESSIONS,
            DEFAULT_TOKEN_BUDGET,
        )
    }

    /// Load the persisted schedule. A missing or corrupt sidecar yields a fresh
    /// state (compaction is safe to re-run, so a lost schedule is not fatal).
    pub fn load_state(&self) -> CompactionState {
        match std::fs::read_to_string(&self.state_path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => CompactionState::default(),
        }
    }

    fn save_state(&self, state: &CompactionState) {
        if let Some(parent) = self.state_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(state) {
            let _ = std::fs::write(&self.state_path, json);
        }
    }

    /// Whether `project` should be compacted at `now`: the wall-clock interval
    /// has elapsed (or it has never been compacted), **or** the session count
    /// has reached the threshold.
    pub fn should_compact(
        &self,
        state: &CompactionState,
        project: &str,
        now: DateTime<Utc>,
    ) -> bool {
        let age_due = match state.last_compaction_at.get(project) {
            Some(last) => now - *last >= self.max_age,
            None => true, // never compacted
        };
        let sessions = state.session_count.get(project).copied().unwrap_or(0);
        age_due || sessions >= self.max_sessions
    }

    /// Run one compaction pass on `store` for `project`: dedupe near-duplicates,
    /// drop stale entries, enforce per-category caps (all via the rule-based
    /// consolidator, respecting `config` bounds), then prune to the token
    /// budget and persist through [`MemoryStore::save`]. No schedule side
    /// effects — callers update the sidecar via [`CompactionState`].
    pub fn run_compaction(
        &self,
        store: &mut MemoryStore,
        project: &str,
        config: &SessionMemoryConfig,
    ) -> Result<CompactSummary, MemoryError> {
        let before_count = store.len();
        let result = MemoryConsolidator::default().consolidate(store, config)?;
        let budget_pruned = store.prune_to_token_budget(project, self.token_budget);
        store.save()?;
        Ok(CompactSummary {
            duplicates_merged: result.duplicates_merged,
            stale_removed: result.stale_removed,
            budget_pruned,
            before_count,
            after_count: store.len(),
        })
    }

    /// Record a session for `project` and compact if the schedule says so.
    /// Returns `Some(summary)` when a compaction ran this call, else `None`.
    /// Either way the (possibly incremented) schedule is persisted.
    pub fn maybe_compact(
        &self,
        store: &mut MemoryStore,
        project: &str,
        config: &SessionMemoryConfig,
    ) -> Result<Option<CompactSummary>, MemoryError> {
        let now = Utc::now();
        let mut state = self.load_state();
        *state.session_count.entry(project.to_string()).or_insert(0) += 1;

        if !self.should_compact(&state, project, now) {
            self.save_state(&state);
            return Ok(None);
        }

        let summary = self.run_compaction(store, project, config)?;
        state.last_compaction_at.insert(project.to_string(), now);
        state.session_count.insert(project.to_string(), 0);
        self.save_state(&state);
        Ok(Some(summary))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::memory::types::{MemoryCategory, MemoryEntry};
    use chrono::Duration;
    use tempfile::TempDir;

    fn trigger(dir: &TempDir, max_age: Duration, max_sessions: u32) -> MemoryCompactionTrigger {
        MemoryCompactionTrigger::new(
            dir.path().join(STATE_FILENAME),
            max_age,
            max_sessions,
            DEFAULT_TOKEN_BUDGET,
        )
    }

    fn empty_store(dir: &TempDir) -> MemoryStore {
        MemoryStore::new(dir.path().to_path_buf())
    }

    #[allow(clippy::too_many_arguments)]
    fn entry_at(
        id: &str,
        project: &str,
        category: MemoryCategory,
        content: &str,
        confidence: f64,
        created_at: DateTime<Utc>,
    ) -> MemoryEntry {
        MemoryEntry {
            id: id.to_string(),
            project: project.to_string(),
            category,
            content: content.to_string(),
            tags: vec![],
            confidence,
            created_at,
            accessed_at: created_at,
            access_count: 0,
        }
    }

    // --- should_compact ---

    #[test]
    fn test_should_compact_when_never_compacted() {
        let dir = TempDir::new().unwrap();
        let t = trigger(&dir, Duration::hours(24), 5);
        let state = CompactionState::default();
        assert!(t.should_compact(&state, "p", Utc::now()));
    }

    #[test]
    fn test_should_not_compact_when_recent_and_under_sessions() {
        let dir = TempDir::new().unwrap();
        let t = trigger(&dir, Duration::hours(24), 5);
        let mut state = CompactionState::default();
        state.last_compaction_at.insert("p".into(), Utc::now());
        state.session_count.insert("p".into(), 2);
        assert!(!t.should_compact(&state, "p", Utc::now()));
    }

    #[test]
    fn test_should_compact_when_sessions_threshold_met() {
        let dir = TempDir::new().unwrap();
        let t = trigger(&dir, Duration::hours(24), 5);
        let mut state = CompactionState::default();
        state.last_compaction_at.insert("p".into(), Utc::now()); // age not due
        state.session_count.insert("p".into(), 5);
        assert!(t.should_compact(&state, "p", Utc::now()));
    }

    #[test]
    fn test_should_compact_when_age_threshold_elapsed() {
        let dir = TempDir::new().unwrap();
        let t = trigger(&dir, Duration::hours(24), 5);
        let mut state = CompactionState::default();
        state
            .last_compaction_at
            .insert("p".into(), Utc::now() - Duration::hours(25));
        state.session_count.insert("p".into(), 0);
        assert!(t.should_compact(&state, "p", Utc::now()));
    }

    // --- sidecar persistence ---

    #[test]
    fn test_state_sidecar_roundtrip() {
        let dir = TempDir::new().unwrap();
        let t = trigger(&dir, Duration::hours(24), 5);
        let mut state = CompactionState::default();
        state
            .last_compaction_at
            .insert("p".into(), Utc::now() - Duration::hours(3));
        state.session_count.insert("p".into(), 4);
        t.save_state(&state);

        let loaded = t.load_state();
        assert_eq!(loaded.session_count.get("p").copied(), Some(4));
        assert!(loaded.last_compaction_at.contains_key("p"));
    }

    #[test]
    fn test_load_state_missing_file_is_default() {
        let dir = TempDir::new().unwrap();
        let t = trigger(&dir, Duration::hours(24), 5);
        assert!(t.load_state().last_compaction_at.is_empty());
    }

    #[test]
    fn test_load_state_corrupt_file_is_default() {
        let dir = TempDir::new().unwrap();
        let t = trigger(&dir, Duration::hours(24), 5);
        std::fs::write(dir.path().join(STATE_FILENAME), "{not json").unwrap();
        assert!(t.load_state().last_compaction_at.is_empty());
    }

    // --- run_compaction ---

    #[test]
    fn test_run_compaction_merges_and_prunes_stale() {
        let dir = TempDir::new().unwrap();
        let t = trigger(&dir, Duration::hours(24), 5);
        let mut store = empty_store(&dir);
        // Two near-duplicate preferences (raw add -> both persist pre-compaction).
        store
            .add(entry_at(
                "a",
                "p",
                MemoryCategory::Preference,
                "always use tabs for indentation",
                0.9,
                Utc::now(),
            ))
            .unwrap();
        store
            .add(entry_at(
                "b",
                "p",
                MemoryCategory::Preference,
                "always use tabs for indentation",
                0.6,
                Utc::now(),
            ))
            .unwrap();
        // One stale entry (older than the default 90-day TTL).
        store
            .add(entry_at(
                "c",
                "p",
                MemoryCategory::Context,
                "stale fact",
                1.0,
                Utc::now() - Duration::days(100),
            ))
            .unwrap();
        let config = SessionMemoryConfig::default();

        let summary = t.run_compaction(&mut store, "p", &config).unwrap();

        assert_eq!(summary.before_count, 3);
        assert!(summary.after_count < summary.before_count);
        assert!(summary.duplicates_merged >= 1, "near-dup merged");
        assert!(summary.stale_removed >= 1, "stale entry pruned");
        // Persisted: a fresh store loading from disk sees the compacted set.
        let mut reloaded = empty_store(&dir);
        reloaded.load().unwrap();
        assert_eq!(reloaded.len(), summary.after_count);
    }

    // --- maybe_compact orchestration ---

    #[test]
    fn test_maybe_compact_skips_until_threshold_then_runs() {
        let dir = TempDir::new().unwrap();
        // 24 h age with max_sessions = 2; age is gated off by seeding a recent
        // compaction timestamp so only the session count is exercised.
        let t = trigger(&dir, Duration::hours(24), 2);
        let mut store = empty_store(&dir);
        let config = SessionMemoryConfig::default();

        let mut seed = CompactionState::default();
        seed.last_compaction_at.insert("p".into(), Utc::now());
        seed.session_count.insert("p".into(), 0);
        t.save_state(&seed);

        // Session 1: under threshold, no compaction.
        assert!(t.maybe_compact(&mut store, "p", &config).unwrap().is_none());
        let state = t.load_state();
        assert_eq!(state.session_count.get("p").copied(), Some(1));

        // Session 2: threshold met -> compaction runs, counters reset.
        let summary = t
            .maybe_compact(&mut store, "p", &config)
            .unwrap()
            .expect("compaction should run at the session threshold");
        assert_eq!(summary.before_count, 0); // empty store
        let state = t.load_state();
        assert_eq!(state.session_count.get("p").copied(), Some(0));
        assert!(state.last_compaction_at.contains_key("p"));
    }

    #[test]
    fn test_maybe_compact_preserves_other_agent_entries() {
        // The trigger's save() reconcile must not drop entries another process
        // appended — the multi-agent-safety contract end-to-end.
        let dir = TempDir::new().unwrap();
        let t = trigger(&dir, Duration::hours(24), 1); // compact on first call
        let mut store = empty_store(&dir);
        let ours = entry_at(
            "x",
            "p",
            MemoryCategory::Context,
            "our fact",
            1.0,
            Utc::now(),
        );
        store.add(ours).unwrap();

        // Another agent appends to the same project file on disk.
        let path = dir.path().join(format!(
            "{}.jsonl",
            // same hash the store uses for project "p"
            {
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut h = DefaultHasher::new();
                "p".hash(&mut h);
                format!("{:016x}", h.finish())
            }
        ));
        let theirs = entry_at(
            "y",
            "p",
            MemoryCategory::Context,
            "their fact",
            1.0,
            Utc::now(),
        );
        let mut content = std::fs::read_to_string(&path).unwrap();
        content.push_str(&format!("{}\n", serde_json::to_string(&theirs).unwrap()));
        std::fs::write(&path, content).unwrap();

        let config = SessionMemoryConfig::default();
        t.maybe_compact(&mut store, "p", &config).unwrap();

        let mut reloaded = empty_store(&dir);
        reloaded.load().unwrap();
        assert!(reloaded.get("x").is_some(), "our entry kept");
        assert!(reloaded.get("y").is_some(), "other agent's entry kept");
    }
}
