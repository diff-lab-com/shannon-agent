//! R5-1 — durable sidecar for session-level model overrides.
//!
//! R2-1 shipped the session model override as in-memory-only state on
//! [`crate::session_registry::SessionState`]: a desktop restart re-inherited
//! the global default and the composer chip's "· session" badge silently
//! dropped. Engine session records were explicitly rejected as the storage
//! venue (R2 review: too invasive — the engine's session persistence is the
//! L0 event log, and routing a desktop UI preference through it would couple
//! the two layers). This module is the deferred piece: a small desktop-owned
//! JSON sidecar at
//!
//! ```text
//! ~/.shannon/desktop/session-model-overrides.json
//! ```
//!
//! storing a flat map `session_id → {provider, model}`. The values are
//! identifiers only (no secrets), so the file carries no key material and
//! needs no special permissions — but writes are still **atomic** (temp file
//! then same-directory rename) so a crash mid-write can never leave a
//! half-serialized map behind.
//!
//! ### Lifecycle / prune policy (documented decision)
//!
//! - **Load** happens once, at `AppState` construction: the file is parsed
//!   (missing file ⇒ empty; corrupt file ⇒ empty + `tracing::warn` — a
//!   hand-edited or truncated sidecar must never block startup), pruned
//!   against the L0 session log, and applied into the
//!   [`crate::session_registry::SessionRegistry`] so `get_session_model`
//!   and the query-time resolution see the restored overrides immediately.
//! - **Prune** drops entries whose session id has no L0 log
//!   (`<sessions>/<uuid>/events.jsonl`). Every desktop session — even one
//!   that never received a message — gets its L0 container at `new_session`,
//!   so a missing log means the session was deleted (or came from a legacy
//!   install): its override is dead weight. The prune is **memory-only at
//!   load** (startup never writes); the pruned set reaches disk on the next
//!   `record` (set/clear write-through), which re-prunes before saving. This
//!   keeps `AppState::new()` — which also runs in every unit test — strictly
//!   read-only, and doubles as the periodic sweep: any set/clear reconciles
//!   the whole map.
//! - **Write-through**: `set_session_model` / `clear_session_model` call
//!   [`SessionOverrideSidecar::record`] after the in-memory update. A failed
//!   save is logged and swallowed — the override stays live for the session,
//!   and the next write-through retries the disk.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::session_registry::{SessionKey, SessionModelOverride, SessionRegistry};

/// Wire/disk entry shape — mirrors [`SessionModelOverride`] so the sidecar
/// stays a plain `{ "<uuid>": {"provider": …, "model": …} }` JSON map.
type EntryMap = BTreeMap<String, SessionModelOverride>;

/// The desktop-owned override sidecar: parsed map + the path it round-trips
/// through. `BTreeMap` keeps the serialized form key-sorted, so re-saving an
/// unchanged map is byte-stable (diff-friendly, no churn).
pub struct SessionOverrideSidecar {
    path: PathBuf,
    entries: EntryMap,
}

/// Resolve the sidecar path: `~/.shannon/desktop/session-model-overrides.json`.
/// Same HOME resolution as `config::config_path` (HOME, then USERPROFILE,
/// then the process directory — dev-container fallback).
fn default_sidecar_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    home.join(".shannon")
        .join("desktop")
        .join("session-model-overrides.json")
}

impl SessionOverrideSidecar {
    /// Load the sidecar from its default path.
    pub fn load_default() -> Self {
        Self::load_from(default_sidecar_path())
    }

    /// Load from an explicit path (tests inject a temp dir). Graceful
    /// degradation contract:
    /// - missing file → empty map (first launch, or pruned to nothing);
    /// - unparsable JSON → empty map + `tracing::warn` — the next successful
    ///   write-through overwrites the corrupt file; reads never fail.
    pub fn load_from(path: PathBuf) -> Self {
        let entries = match std::fs::read_to_string(&path) {
            Ok(raw) => match serde_json::from_str::<EntryMap>(&raw) {
                Ok(map) => map,
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "session model override sidecar is corrupt — starting empty"
                    );
                    BTreeMap::new()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "could not read session model override sidecar — starting empty"
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

    /// True when no overrides are stored.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Look up one session's persisted override.
    pub fn get(&self, session_id: uuid::Uuid) -> Option<&SessionModelOverride> {
        self.entries.get(&session_id.to_string())
    }

    /// Write-through one mutation: `Some(ov)` upserts the entry, `None`
    /// removes it, then the whole map is persisted (pruned) atomically.
    ///
    /// `live_session` decides which entries survive the pre-save prune: it
    /// receives each stored session id and returns `true` when the session
    /// still exists (see the module doc for the L0-log policy). Passing a
    /// `Some` for the id being recorded is the caller's job — `record`
    /// itself never second-guesses the mutation.
    pub fn record(
        &mut self,
        session_id: uuid::Uuid,
        override_value: Option<SessionModelOverride>,
        live_session: impl Fn(&str) -> bool,
    ) -> Result<(), String> {
        let key = session_id.to_string();
        match override_value {
            Some(ov) => {
                self.entries.insert(key, ov);
            }
            None => {
                self.entries.remove(&key);
            }
        }
        self.prune(live_session);
        self.save()
    }

    /// Drop entries whose session id fails `live_session`. Returns the
    /// number of pruned entries. Also drops entries whose key is not a
    /// valid UUID (hand-edited file) — they can never match a session.
    pub fn prune(&mut self, live_session: impl Fn(&str) -> bool) -> usize {
        let before = self.entries.len();
        self.entries
            .retain(|id, _| uuid::Uuid::parse_str(id).is_ok() && live_session(id));
        before - self.entries.len()
    }

    /// Apply every stored entry into `registry` (the startup hydrate).
    /// Sessions are materialized (`get_or_create`) — they are lazy, so a
    /// restored entry must bring its `SessionState` into existence for
    /// `get_session_model` to find it. Call after [`Self::prune`]: entries
    /// for dead sessions would otherwise resurrect empty session states.
    pub fn apply_to_registry(&self, registry: &SessionRegistry) {
        for (id_raw, ov) in &self.entries {
            let Ok(id) = uuid::Uuid::parse_str(id_raw) else {
                continue; // prune() already drops these; belt-and-braces
            };
            let state = registry.get_or_create(SessionKey(id));
            *state
                .model_override
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(ov.clone());
        }
    }

    /// Atomically persist the map: serialize → write `.<name>.tmp` in the
    /// same directory → rename over the target. Same-directory rename is
    /// atomic on the platforms the desktop ships on, so a reader (next
    /// launch) sees either the old map or the new one, never a torn write.
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
            .map_err(|e| format!("could not serialize override sidecar: {e}"))?;
        let tmp = parent.join(format!(
            ".{}.tmp",
            self.path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("session-model-overrides.json")
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

    fn ov(provider: &str, model: &str) -> SessionModelOverride {
        SessionModelOverride {
            provider: provider.into(),
            model: model.into(),
        }
    }

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "shannon-override-store-{}-{}",
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
        let path = dir.join("session-model-overrides.json");
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();

        let mut store = SessionOverrideSidecar::load_from(path.clone());
        assert!(store.is_empty(), "missing file loads empty");
        store
            .record(a, Some(ov("openai", "gpt-5")), all_live)
            .expect("first write");
        store
            .record(b, Some(ov("anthropic", "claude-opus-4-7")), all_live)
            .expect("second write");
        store.record(a, None, all_live).expect("clear write");

        let reloaded = SessionOverrideSidecar::load_from(path);
        assert_eq!(reloaded.len(), 1, "A cleared, B survives");
        assert_eq!(
            reloaded.get(b).cloned(),
            Some(ov("anthropic", "claude-opus-4-7")),
            "override survives the process boundary (the restart story)"
        );
        assert_eq!(reloaded.get(a), None, "cleared entry stays cleared");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn corrupt_file_degrades_to_empty_instead_of_failing_startup() {
        let dir = tempdir();
        let path = dir.join("session-model-overrides.json");
        std::fs::write(&path, "{ not json at all").expect("seed corrupt file");

        let mut store = SessionOverrideSidecar::load_from(path.clone());
        assert!(
            store.is_empty(),
            "corrupt sidecar must degrade to an empty map, never block startup"
        );
        // And the next write-through repairs the file in place.
        let id = uuid::Uuid::new_v4();
        store
            .record(id, Some(ov("deepseek", "deepseek-chat")), all_live)
            .expect("write over corrupt file");
        let repaired = SessionOverrideSidecar::load_from(path);
        assert_eq!(
            repaired.get(id).map(|o| o.provider.as_str()),
            Some("deepseek")
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn prune_drops_dead_sessions_and_non_uuid_keys() {
        let dir = tempdir();
        let store_path = dir.join("session-model-overrides.json");
        let live = uuid::Uuid::new_v4();
        let dead = uuid::Uuid::new_v4();

        let mut store = SessionOverrideSidecar::load_from(store_path);
        store
            .record(live, Some(ov("openai", "gpt-5")), all_live)
            .unwrap();
        store
            .record(dead, Some(ov("openai", "gpt-5")), all_live)
            .unwrap();

        // Only `live` still has an L0 log.
        let pruned = store.prune(|id| id == live.to_string());
        assert_eq!(pruned, 1, "the dead session's entry is dropped");
        assert_eq!(store.get(dead), None);
        assert!(store.get(live).is_some());

        // A hand-edited non-UUID key can never match a session — dropped.
        let mut raw = SessionOverrideSidecar::load_from(dir.join("hand-edited.json"));
        raw.entries
            .insert("not-a-uuid".to_string(), ov("openai", "gpt-5"));
        assert_eq!(
            raw.prune(all_live),
            1,
            "non-UUID keys are pruned unconditionally"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn apply_to_registry_restores_overrides_on_lazy_sessions() {
        let dir = tempdir();
        let store_path = dir.join("session-model-overrides.json");
        let live = uuid::Uuid::new_v4();

        let mut store = SessionOverrideSidecar::load_from(store_path);
        store
            .record(live, Some(ov("gemini", "gemini-2.5-pro")), all_live)
            .unwrap();

        // A fresh registry (the restart scenario) — nothing materialized.
        let registry = SessionRegistry::new();
        assert!(registry.get(SessionKey(live)).is_none());
        store.apply_to_registry(&registry);
        let state = registry
            .get(SessionKey(live))
            .expect("materialized by hydrate");
        assert_eq!(
            state
                .model_override_snapshot()
                .as_ref()
                .map(|o| o.model.as_str()),
            Some("gemini-2.5-pro"),
            "the restored override must be visible to get_session_model / query resolution"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn save_is_atomic_and_leaves_no_tmp_behind() {
        let dir = tempdir();
        let path = dir.join("session-model-overrides.json");
        let mut store = SessionOverrideSidecar::load_from(path.clone());
        store
            .record(uuid::Uuid::new_v4(), Some(ov("openai", "gpt-5")), all_live)
            .expect("write");
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .expect("dir")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            leftovers,
            vec!["session-model-overrides.json".to_string()],
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
            PathBuf::from("/home/tester/.shannon/desktop/session-model-overrides.json")
        );
    }
}
