//! Per-session state registry (P0-4 / `query-coordinator-concurrency`).
//!
//! `AppState` was originally structured for "one active chat at a time" —
//! a single `Mutex<Vec<ChatMessage>>`, a single `Mutex<bool>` querying flag,
//! and a single `Mutex<Option<CancellationToken>>` were all stored on
//! `AppState` directly. Any second `send_message` call hit the hard-rejection
//! at `commands.rs:421-428` ("A query is already in progress"). This module
//! is the *spike* refactor: replace those single-session fields with a
//! `SessionRegistry` keyed by [`SessionKey`], so future work (P2-5b) can
//! multiplex queries across sessions without redesigning the public
//! `AppState` surface.
//!
//! The spike keeps single-session semantics: the registry is created empty,
//! and the first command that needs session state materialises the active
//! session lazily. Multi-thread support is intentionally deferred.

use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::commands::ChatMessage;
use shannon_core::query_engine::QueryEngine;

/// Stable, hashable identifier for a session in the [`SessionRegistry`].
///
/// Wraps a `Uuid` because session IDs are UUIDs everywhere else in the
/// codebase (StateManager, SessionInfo, etc.). `Copy` + `Eq` + `Hash` so it
/// can serve as a `DashMap` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionKey(pub Uuid);

impl SessionKey {
    /// Generate a fresh random key. Convenience for callers that don't have
    /// a UUID in hand yet (e.g. `new_session` in the spike).
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Build from an existing UUID. Returns `None` if `raw` is not a valid
    /// UUID string — keeps the public surface tight (no panics on bad input).
    pub fn parse(raw: &str) -> Option<Self> {
        Uuid::parse_str(raw).ok().map(Self)
    }
}

impl Default for SessionKey {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SessionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<Uuid> for SessionKey {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

/// Per-session mutable state.
///
/// Owns everything that used to live on `AppState` but belonged to a single
/// chat:
/// - `messages` — the rolling conversation buffer surfaced to the UI
/// - `querying` — "a query is in flight" gate (used by `get_status` and
///   enforced at the top of `send_message`)
/// - `cancellation_token` — the `CancellationToken` for the in-flight query
///   (so `cancel_query` can fire it without touching the registry)
/// - `session_id` — the canonical UUID used by StateManager for persistence
///
/// All `Mutex` fields are `tokio::sync::Mutex` because the call sites
/// (`send_message`, `cancel_query`, the `get_status` status bar) all `.lock()`
/// across `await` points.
pub struct SessionState {
    pub messages: Mutex<Vec<ChatMessage>>,
    pub querying: Mutex<bool>,
    pub cancellation_token: Mutex<Option<CancellationToken>>,
    pub session_id: Uuid,
    /// Per-session `QueryEngine` — lazy-initialised on the first
    /// `send_message` so we don't pay the construction cost for
    /// `SessionState`s that never receive a query (e.g. a freshly listed
    /// session the user only opens to view history). `QueryEngine` is
    /// `Clone` so the engine can be referenced both here and inside the
    /// spawned `tokio::spawn` task without `Arc<QueryEngine>` plumbing.
    pub query_engine: tokio::sync::Mutex<Option<QueryEngine>>,
}

impl SessionState {
    /// Build a fresh, empty session state for the given UUID. Used by
    /// [`SessionRegistry::get_or_create`] when the registry has no entry
    /// for the key yet.
    pub fn new(session_id: Uuid) -> Self {
        Self {
            messages: Mutex::new(Vec::new()),
            querying: Mutex::new(false),
            cancellation_token: Mutex::new(None),
            session_id,
            query_engine: tokio::sync::Mutex::new(None),
        }
    }
}

/// Session-scoped state registry.
///
/// Two collaborators:
/// - `sessions` — `DashMap<SessionKey, Arc<SessionState>>`. Concurrent
///   read/write across threads; `Arc<SessionState>` so callers can hold a
///   cheap clone while releasing the map shard lock.
/// - `active_session` — `Mutex<Option<SessionKey>>`. The "focused" session
///   in the UI. Spike scope: any command that doesn't carry an explicit
///   `session_id` argument falls back to the active session.
///
/// The registry is intentionally minimal: no eviction, no quotas, no
/// background pruning. Those belong to P2-5b. The single hard contract is
/// `get_or_create` is *idempotent* — calling it twice with the same key
/// returns the same `Arc<SessionState>`.
pub struct SessionRegistry {
    pub sessions: DashMap<SessionKey, Arc<SessionState>>,
    pub active_session: Mutex<Option<SessionKey>>,
}

impl Default for SessionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionRegistry {
    /// Construct an empty registry. No sessions are pre-loaded; the first
    /// `get_or_create` call materialises the active session.
    pub fn new() -> Self {
        Self {
            sessions: DashMap::new(),
            active_session: Mutex::new(None),
        }
    }

    /// Create a new session with a freshly-generated UUID, insert it into
    /// the registry, and mark it as the active session. Returns the key.
    ///
    /// Unlike [`get_or_create`], this *always* allocates a new entry —
    /// duplicates are not deduplicated. Use [`new_with_id`] if the UUID
    /// is already known (e.g. loaded from StateManager).
    pub fn create(&self) -> SessionKey {
        let key = SessionKey::new();
        self.sessions
            .insert(key, Arc::new(SessionState::new(key.0)));
        key
    }

    /// Insert a session with a specific UUID (no-op if it already exists).
    /// Returns `true` if a new entry was created, `false` if the key was
    /// already present. After insertion the session is *not* automatically
    /// marked active — call [`set_active`] separately.
    pub fn insert(&self, session_id: Uuid) -> bool {
        let key = SessionKey(session_id);
        let created = !self.sessions.contains_key(&key);
        self.sessions
            .entry(key)
            .or_insert_with(|| Arc::new(SessionState::new(session_id)));
        created
    }

    /// Return the [`SessionState`] for `key`, creating an empty one if
    /// the key isn't yet present. Idempotent: repeated calls with the same
    /// key return the same `Arc<SessionState>`.
    pub fn get_or_create(&self, key: SessionKey) -> Arc<SessionState> {
        if let Some(existing) = self.sessions.get(&key) {
            return existing.clone();
        }
        // Insert under a shard lock — `entry().or_insert_with` guarantees
        // only one SessionState is ever created for a given key, even
        // under concurrent first-time access.
        self.sessions
            .entry(key)
            .or_insert_with(|| Arc::new(SessionState::new(key.0)))
            .clone()
    }

    /// Look up the session without creating. Returns `None` if absent.
    pub fn get(&self, key: SessionKey) -> Option<Arc<SessionState>> {
        self.sessions.get(&key).map(|s| s.clone())
    }

    /// Remove a session. Returns `true` if the key was present and
    /// removed, `false` otherwise.
    pub fn destroy(&self, key: SessionKey) -> bool {
        let removed = self.sessions.remove(&key).is_some();
        // Best-effort: if the destroyed session was active, clear active.
        // Held under the active_session lock so we don't race a parallel
        // set_active call.
        if removed {
            let mut active = self
                .active_session
                .try_lock()
                .expect("active_session lock contended in destroy");
            if active.as_ref() == Some(&key) {
                *active = None;
            }
        }
        removed
    }

    /// Snapshot of `(key, Arc<session>)` pairs currently in the registry.
    /// Order is unspecified (DashMap iteration order). The returned vec
    /// is owned — the underlying `Arc` clones keep the session state
    /// alive after this call.
    pub fn list(&self) -> Vec<(SessionKey, Arc<SessionState>)> {
        self.sessions
            .iter()
            .map(|entry| (*entry.key(), entry.value().clone()))
            .collect()
    }

    /// Return the active session, creating one if no session has ever
    /// been created. Used by every command that doesn't yet take an
    /// explicit `session_id` argument (the spike's "single-session
    /// fallback" behaviour).
    pub fn get_or_create_active(&self) -> Arc<SessionState> {
        // Fast path: an active session already exists. `try_lock` keeps
        // this synchronous and panic-free on contention — the worst case
        // is we fall through and create one anyway, which is harmless.
        if let Some(key) = self.active_session.try_lock().ok().and_then(|guard| *guard) {
            if let Some(session) = self.sessions.get(&key) {
                return session.clone();
            }
        }
        // Slow path: allocate a fresh session and promote it to active.
        let key = self.create();
        if let Ok(mut guard) = self.active_session.try_lock() {
            *guard = Some(key);
        }
        self.get_or_create(key)
    }

    /// Promote `key` to the active session. The session must already
    /// exist (call [`get_or_create`] first if not).
    pub fn set_active(&self, key: SessionKey) {
        if let Ok(mut guard) = self.active_session.try_lock() {
            *guard = Some(key);
        }
    }

    /// Current active key, if any. `None` before any session has been
    /// created or after `destroy` cleared the active pointer.
    pub fn active_key(&self) -> Option<SessionKey> {
        self.active_session.try_lock().ok().and_then(|guard| *guard)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_registry_create_returns_unique_keys() {
        let reg = SessionRegistry::new();
        let k1 = reg.create();
        let k2 = reg.create();
        assert_ne!(k1, k2, "two create() calls must produce distinct keys");
        assert_eq!(reg.list().len(), 2);
    }

    #[test]
    fn session_registry_get_or_create_idempotent() {
        let reg = SessionRegistry::new();
        let key = SessionKey::new();
        let s1 = reg.get_or_create(key);
        let s2 = reg.get_or_create(key);
        assert!(
            Arc::ptr_eq(&s1, &s2),
            "get_or_create must return the same Arc for the same key"
        );
        assert_eq!(reg.list().len(), 1, "no duplicate SessionState created");
    }

    #[test]
    fn session_registry_destroy_returns_true_when_present_false_otherwise() {
        let reg = SessionRegistry::new();
        let key = reg.create();
        assert!(reg.destroy(key), "destroy must return true for present key");
        assert!(
            !reg.destroy(key),
            "destroy must return false when called a second time"
        );
        let never = SessionKey::new();
        assert!(
            !reg.destroy(never),
            "destroy must return false for a never-inserted key"
        );
    }

    #[test]
    fn session_registry_list_returns_all_created() {
        let reg = SessionRegistry::new();
        let k1 = reg.create();
        let k2 = reg.create();
        let k3 = reg.create();
        let listed: std::collections::HashSet<_> = reg.list().into_iter().map(|(k, _)| k).collect();
        assert_eq!(listed.len(), 3, "three creates must list three entries");
        assert!(listed.contains(&k1));
        assert!(listed.contains(&k2));
        assert!(listed.contains(&k3));
    }

    #[test]
    fn session_registry_destroy_clears_active_when_destroyed_session_was_active() {
        let reg = SessionRegistry::new();
        let key = reg.create();
        reg.set_active(key);
        assert_eq!(reg.active_key(), Some(key));
        reg.destroy(key);
        assert_eq!(
            reg.active_key(),
            None,
            "destroying the active session must clear the active pointer"
        );
    }

    #[test]
    fn session_registry_get_or_create_active_returns_a_session() {
        let reg = SessionRegistry::new();
        assert_eq!(reg.active_key(), None);
        let s = reg.get_or_create_active();
        assert_eq!(reg.list().len(), 1);
        assert_eq!(s.session_id, reg.active_key().unwrap().0);
    }

    #[test]
    fn session_registry_parse_round_trip() {
        let uuid = Uuid::new_v4();
        let key = SessionKey::from(uuid);
        assert_eq!(SessionKey::parse(&uuid.to_string()), Some(key));
        assert_eq!(SessionKey::parse("not-a-uuid"), None);
    }
}
