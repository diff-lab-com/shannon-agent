//! P2-5 — durable sidecar for the session-level "temporary chat" flag.
//!
//! Sibling of [`crate::session_override_store`] (the R5-1 model-override
//! sidecar) and built to the same contract, because the flag has the same
//! lifetime requirements: a desktop restart must not silently re-enable
//! memory injection for a session the user turned it off for, and engine
//! session records stay out of it (a UI preference must not couple to the
//! L0 event log). The map lives at
//!
//! ```text
//! ~/.shannon/desktop/session-memory-bypass.json
//! ```
//!
//! as a flat `{ "<uuid>": true }` JSON map. Only **bypassed** sessions have
//! entries — the default (`false`) is the absence of a key, so the file
//! stays tiny and a hand-edited `false` is normalized away on the next
//! write-through. Values are identifiers only (no secrets, no permissions),
//! but writes are still **atomic** (temp file then same-directory rename).
//!
//! ### Lifecycle / prune policy (same decision as the model-override sidecar)
//!
//! - **Load** once, at `AppState` construction: parse (missing file ⇒ empty;
//!   corrupt file ⇒ empty + `tracing::warn`), prune against the L0 session
//!   log, hydrate into the [`crate::session_registry::SessionRegistry`].
//! - **Prune** drops entries whose session has no L0 log
//!   (`<sessions-dir>/<session_id>/events.jsonl`) or whose key is not a
//!   UUID. Memory-only at load (startup never writes); the pruned set
//!   reaches disk on the next write-through, which re-prunes first.
//! - **Write-through**: `set_session_memory_bypass` calls
//!   [`SessionMemoryBypassSidecar::record`] after the in-memory update. A
//!   failed save is logged and swallowed — the flag stays live for the
//!   session, and the next write-through retries the disk.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::session_registry::{SessionKey, SessionRegistry};

/// Wire/disk shape — only bypassed sessions appear (`"<uuid>": true`).
/// `BTreeMap` keeps the serialized form key-sorted, so re-saving an
/// unchanged map is byte-stable (diff-friendly, no churn).
type EntryMap = BTreeMap<String, bool>;

/// The desktop-owned bypass sidecar: parsed map + the path it round-trips
/// through.
pub struct SessionMemoryBypassSidecar {
    path: PathBuf,
    entries: EntryMap,
}

/// Resolve the sidecar path: `~/.shannon/desktop/session-memory-bypass.json`.
/// Same HOME resolution as the model-override sidecar (HOME, then
/// USERPROFILE, then the process directory — dev-container fallback).
fn default_sidecar_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    home.join(".shannon")
        .join("desktop")
        .join("session-memory-bypass.json")
}

impl SessionMemoryBypassSidecar {
    /// Load the sidecar from its default path.
    pub fn load_default() -> Self {
        Self::load_from(default_sidecar_path())
    }

    /// Load from an explicit path (tests inject a temp dir). Graceful
    /// degradation contract: missing file → empty map; unparsable JSON →
    /// empty map + `tracing::warn`; reads never fail.
    pub fn load_from(path: PathBuf) -> Self {
        let entries = match std::fs::read_to_string(&path) {
            Ok(raw) => match serde_json::from_str::<EntryMap>(&raw) {
                Ok(map) => map.into_iter().filter(|(_, v)| *v).collect(),
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "session memory bypass sidecar is corrupt — starting empty"
                    );
                    BTreeMap::new()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "could not read session memory bypass sidecar — starting empty"
                );
                BTreeMap::new()
            }
        };
        Self { path, entries }
    }

    /// Number of live entries (tests + the prune log line).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when no bypassed sessions are stored.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Look up one session's persisted flag.
    pub fn get(&self, session_id: uuid::Uuid) -> bool {
        *self.entries.get(&session_id.to_string()).unwrap_or(&false)
    }

    /// Write-through one mutation, then persist the whole map (pruned)
    /// atomically. `live_session` decides which entries survive the
    /// pre-save prune; passing a `Some` for the id being recorded is the
    /// caller's job.
    pub fn record(
        &mut self,
        session_id: uuid::Uuid,
        bypassed: bool,
        live_session: impl Fn(&str) -> bool,
    ) -> Result<(), String> {
        let key = session_id.to_string();
        if bypassed {
            self.entries.insert(key, true);
        } else {
            self.entries.remove(&key);
        }
        self.prune(live_session);
        self.save()
    }

    /// Drop entries whose session id fails `live_session`, plus non-UUID
    /// keys (hand-edited file) and explicit `false` values. Returns the
    /// number of pruned entries.
    pub fn prune(&mut self, live_session: impl Fn(&str) -> bool) -> usize {
        let before = self.entries.len();
        self.entries.retain(|id, bypassed| {
            uuid::Uuid::parse_str(id).is_ok() && live_session(id) && *bypassed
        });
        before - self.entries.len()
    }

    /// Apply every stored entry into `registry` (the startup hydrate).
    /// Sessions are materialized (`get_or_create`) — they are lazy, so a
    /// restored flag must bring its `SessionState` into existence for
    /// `get_session_memory_bypass` and query-time resolution to see it.
    /// Call after [`Self::prune`].
    pub fn apply_to_registry(&self, registry: &SessionRegistry) {
        for id_raw in self.entries.keys() {
            let Ok(id) = uuid::Uuid::parse_str(id_raw) else {
                continue; // prune() already drops these; belt-and-braces
            };
            registry
                .get_or_create(SessionKey(id))
                .set_memory_disabled(true);
        }
    }

    /// Atomically persist the map (serialize → temp file → rename). Same
    /// contract as the model-override sidecar's save.
    pub fn save(&self) -> Result<(), String> {
        let Some(parent) = self.path.parent() else {
            return Err(format!(
                "sidecar path `{}` has no parent directory",
                self.path.display()
            ));
        };
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
        let body = serde_json::to_string_pretty(&self.entries)
            .map_err(|e| format!("could not serialize memory bypass sidecar: {e}"))?;
        let tmp = parent.join(format!(
            ".{}.tmp",
            self.path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("session-memory-bypass.json")
        ));
        std::fs::write(&tmp, body)
            .map_err(|e| format!("could not write {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path)
            .map_err(|e| format!("could not finalize {}: {e}", self.path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "shannon-memory-bypass-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn all_live(_id: &str) -> bool {
        true
    }

    #[test]
    fn round_trip_persists_entries_across_reload() {
        let dir = tempdir();
        let path = dir.join("session-memory-bypass.json");
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();

        let mut store = SessionMemoryBypassSidecar::load_from(path.clone());
        assert!(store.is_empty(), "missing file loads empty");
        assert!(!store.get(a), "default is off");
        store.record(a, true, all_live).expect("first write");
        store.record(b, true, all_live).expect("second write");
        store.record(a, false, all_live).expect("clear write");

        let reloaded = SessionMemoryBypassSidecar::load_from(path);
        assert_eq!(reloaded.len(), 1, "A cleared, B survives");
        assert!(reloaded.get(b), "the flag survives the process boundary");
        assert!(!reloaded.get(a), "cleared entry stays cleared");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn corrupt_file_degrades_to_empty_instead_of_failing_startup() {
        let dir = tempdir();
        let path = dir.join("session-memory-bypass.json");
        std::fs::write(&path, "{ not json at all").expect("seed corrupt file");

        let mut store = SessionMemoryBypassSidecar::load_from(path.clone());
        assert!(
            store.is_empty(),
            "corrupt sidecar must degrade to an empty map, never block startup"
        );
        let id = uuid::Uuid::new_v4();
        store
            .record(id, true, all_live)
            .expect("write over corrupt file");
        let repaired = SessionMemoryBypassSidecar::load_from(path);
        assert!(repaired.get(id));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn false_values_and_dead_sessions_and_non_uuid_keys_are_pruned() {
        let dir = tempdir();
        let store_path = dir.join("session-memory-bypass.json");
        let live = uuid::Uuid::new_v4();
        let dead = uuid::Uuid::new_v4();

        let mut store = SessionMemoryBypassSidecar::load_from(store_path);
        store.record(live, true, all_live).unwrap();
        store.record(dead, true, all_live).unwrap();

        // Only `live` still has an L0 log.
        let pruned = store.prune(|id| id == live.to_string());
        assert_eq!(pruned, 1, "the dead session's entry is dropped");
        assert!(!store.get(dead));
        assert!(store.get(live));

        // A hand-edited non-UUID key / explicit false can never match.
        let mut raw = SessionMemoryBypassSidecar::load_from(dir.join("hand-edited.json"));
        raw.entries.insert("not-a-uuid".to_string(), true);
        raw.entries.insert(live.to_string(), false);
        assert_eq!(
            raw.prune(all_live),
            2,
            "non-UUID keys and explicit false values are pruned unconditionally"
        );
        assert!(
            !raw.get(live),
            "the explicit false entry is dropped — absent means off"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn apply_to_registry_restores_flag_on_lazy_sessions() {
        let dir = tempdir();
        let store_path = dir.join("session-memory-bypass.json");
        let live = uuid::Uuid::new_v4();

        let mut store = SessionMemoryBypassSidecar::load_from(store_path);
        store.record(live, true, all_live).unwrap();

        // A fresh registry (the restart scenario) — nothing materialized.
        let registry = SessionRegistry::new();
        assert!(registry.get(SessionKey(live)).is_none());
        store.apply_to_registry(&registry);
        let state = registry
            .get(SessionKey(live))
            .expect("materialized by hydrate");
        assert!(
            state.memory_disabled_snapshot(),
            "the restored flag must be visible to get_session_memory_bypass / query resolution"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn save_is_atomic_and_leaves_no_tmp_behind() {
        let dir = tempdir();
        let path = dir.join("session-memory-bypass.json");
        let mut store = SessionMemoryBypassSidecar::load_from(path.clone());
        store
            .record(uuid::Uuid::new_v4(), true, all_live)
            .expect("write");
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .expect("dir")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            leftovers,
            vec!["session-memory-bypass.json".to_string()],
            "the temp file must be renamed away, not left behind: {leftovers:?}"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn default_path_matches_the_documented_sidecar_location() {
        // Pins the module doc's contract (`~/.shannon/desktop/…`) against a
        // silent relocation. HOME is forced for determinism.
        // SAFETY: single-threaded test process (nextest runs tests in
        // separate processes); unique env var touched only here.
        unsafe { std::env::set_var("HOME", "/home/tester") };
        unsafe { std::env::remove_var("USERPROFILE") };
        let path = default_sidecar_path();
        assert_eq!(
            path,
            PathBuf::from("/home/tester/.shannon/desktop/session-memory-bypass.json")
        );
    }
}
