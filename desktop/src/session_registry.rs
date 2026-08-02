//! Per-session state registry (P0-4 / `query-coordinator-concurrency`,
//! P2-5b per-session event queue).
//!
//! `AppState` was originally structured for "one active chat at a time" —
//! a single `Mutex<Vec<ChatMessage>>`, a single `Mutex<bool>` querying flag,
//! and a single `Mutex<Option<CancellationToken>>` were all stored on
//! `AppState` directly. Any second `send_message` call hit the hard-rejection
//! at `commands.rs:421-428` ("A query is already in progress"). Phase 0 of
//! P2-5b replaced those single-session fields with a `SessionRegistry`
//! keyed by [`SessionKey`], so concurrent queries across sessions don't
//! redesign the public `AppState` surface.
//!
//! This Phase 1 (P2-5b spike, see `docs/plans/chat-upgrade.md` §3.x) layers
//! a **per-session event queue** on top of the existing fields. The global
//! Tauri `app.emit(...)` wire still fires (the UI listens via
//! `@tauri-apps/api/event`), but each session additionally owns an
//! `mpsc::UnboundedSender<SessionEvent>` that any in-process consumer
//! (the upcoming thread switcher, replay-on-attach logic, etc.) can take
//! once from `events_rx` and drain independently. Crucially the queue is
//! per-session: events from session A never bleed into a consumer that's
//! listening to session B.
//!
//! ### Serialization story
//!
//! `SessionEvent` is the *in-process* analogue of Tauri events. The
//! Tauri wire still uses the typed payloads re-exported from
//! `shannon_types::events` (`QueryTextPayload`, `ToolStartPayload`,
//! `ToolResultPayload`, `QueryCompletedPayload`, …) so the JS side
//! continues to receive the same `event_names::*` it already consumes.
//! `SessionEvent` is **not** sent to the frontend directly — it is the
//! handle an in-process Rust consumer uses to follow a single session's
//! stream (e.g. the future thread-switcher). The frontend keeps listening
//! to the same `event_names::*` over Tauri; we do not invent new event
//! names for the in-process channel.
//!
//! ### Spike scope (what this commit does NOT change)
//!
//! - The legacy `messages: Mutex<Vec<ChatMessage>>` and global Tauri
//!   emit are untouched. Single-session users see no behaviour change.
//! - This commit only **adds** the channel pair + wires `send_message`
//!   to also feed it. No consumer reads from it yet — the thread
//!   switcher is the next iteration.
//! - The receiver is `Option<mpsc::UnboundedReceiver<...>>` so consumers
//!   can take() it at most once without disturbing existing callers.

use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
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

/// Per-session event stream payload.
///
/// In-process analogue of the Tauri events this same `send_message`
/// loop already emits to the frontend (`event_names::QUERY_TEXT`,
/// `event_names::QUERY_TOOL_START`, etc.). Carries the **same** typed
/// payload structs (`QueryTextPayload`, `ToolStartPayload`, …), re-exported
/// here as plain field moves rather than a reference, so a consumer can
/// own events independently of Tauri.
///
/// The `Cancelled` / `Completed` / `Failed` / `Status` variants are sent
/// **only** through the in-process channel — the Tauri wire already has
/// its own `event_names::QUERY_CANCELLED` / `QUERY_COMPLETED` / `QUERY_FAILED`
/// events. They live here too so a future in-process replay tool (e.g. a
/// thread switcher that joins an existing query) can observe the lifecycle
/// without having to also subscribe to Tauri.
///
/// `Clone` is implemented manually so all payload structs don't need a
/// blanket `Clone` derive. Each arm clones the payload; the variants are
/// small by construction (no buffers, just JSON-ish scalars).
#[derive(Debug, Clone)]
pub enum SessionEvent {
    /// Final assistant or user `ChatMessage` was appended to the buffer.
    /// Mirrors `app.emit(...)` of `event_names::SESSION_UPDATED` semantics,
    /// but payload uses the internal `ChatMessage` (not the wire shape).
    Message(ChatMessage),
    /// `event_names::QUERY_TOOL_START` payload.
    ToolStart(crate::events::ToolStartPayload),
    /// `event_names::QUERY_TOOL_RESULT` payload.
    ToolResult(crate::events::ToolResultPayload),
    /// `event_names::QUERY_TEXT` payload.
    QueryText(crate::events::QueryTextPayload),
    /// `event_names::QUERY_THINKING` payload.
    Thinking(crate::events::ThinkingPayload),
    /// `event_names::QUERY_USAGE` payload.
    Usage(crate::events::UsagePayload),
    /// `event_names::QUERY_TOOL_PROGRESS` payload.
    ToolProgress(crate::events::ToolProgressPayload),
    /// Coarse-grained status update (idle, cancelled, completed, failed).
    /// In-process only — see the type's docstring.
    Status(SessionEventStatus),
}

/// Coarse-grained lifecycle signal. The Tauri equivalents are
/// `QUERY_CANCELLED` / `QUERY_COMPLETED` / `QUERY_FAILED`; this enum
/// folds them into one shape so the consumer doesn't have to match
/// across three variants.
#[derive(Debug, Clone)]
pub enum SessionEventStatus {
    Started,
    Cancelled,
    Completed,
    Failed(String),
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
/// - `events_tx` / `events_rx` — per-session mpsc channel (P2-5b).
///   `events_tx` is cloned into the `send_message` task so it can emit
///   lifecycle + tool + text events directly to a per-session consumer.
///   `events_rx` is `Option<...>` so a consumer can `take()` it at most
///   once.
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
    /// Per-session mpsc **sender**. Cheap to clone; held alongside the
    /// `Arc<SessionState>` clones that `send_message` keeps alive for
    /// the spawned task. `UnboundedSender::send` is non-blocking (returns
    /// `Err` only if the receiver was dropped, which we ignore — we
    /// never want the engine stream to die because nobody is listening).
    pub events_tx: mpsc::UnboundedSender<SessionEvent>,
    /// Per-session mpsc **receiver**, stored behind `Option<…>` so a
    /// single in-process consumer can `take()` it once. Multi-consumer
    /// fanout is intentionally not supported in the spike — the future
    /// thread switcher holds the sole receiver while the user has the
    /// session focused.
    pub events_rx: Mutex<Option<mpsc::UnboundedReceiver<SessionEvent>>>,
}

impl SessionState {
    /// Build a fresh, empty session state for the given UUID. Used by
    /// [`SessionRegistry::get_or_create`] when the registry has no entry
    /// for the key yet.
    ///
    /// Allocates the per-session mpsc channel pair with the default
    /// unbounded buffer. There is no back-pressure today (a consumer
    /// that lags will accumulate in the unbounded channel); this is
    /// acceptable for the single-conversation streaming profile but a
    /// bound + drop-oldest switch is the next iteration's hardening.
    pub fn new(session_id: Uuid) -> Self {
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        Self {
            messages: Mutex::new(Vec::new()),
            querying: Mutex::new(false),
            cancellation_token: Mutex::new(None),
            session_id,
            query_engine: tokio::sync::Mutex::new(None),
            events_tx,
            events_rx: Mutex::new(Some(events_rx)),
        }
    }

    /// Best-effort send on the per-session channel. `mpsc::UnboundedSender`
    /// only fails when the receiver was dropped (i.e. nobody is listening
    /// anymore); we silently drop those failures because:
    /// 1. We never want a missing consumer to break the engine stream.
    /// 2. Even when nobody is listening, the wire to the frontend is
    ///    already covered by `app.emit(...)` so the user still sees the
    ///    conversation progress.
    pub fn try_send_event(&self, event: SessionEvent) {
        let _ = self.events_tx.send(event);
    }

    /// Take the receiver out of the session. After this call,
    /// `events_rx` is `None` and `try_send_event` still works (the sender
    /// half is independent). A second call returns `None`.
    pub async fn take_event_receiver(&self) -> Option<mpsc::UnboundedReceiver<SessionEvent>> {
        let mut guard = self.events_rx.lock().await;
        guard.take()
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

    // === P2-5b: per-session event queue ===

    /// Each freshly-created `SessionState` must own a complete mpsc
    /// channel pair, not just the sender half. A consumer (the future
    /// thread switcher) needs the receiver to follow the stream.
    #[tokio::test]
    async fn session_state_new_initialises_event_channel_pair() {
        let s = SessionState::new(Uuid::new_v4());
        assert!(
            s.take_event_receiver().await.is_some(),
            "new SessionState must hold a Some(receiver) so a consumer can take it"
        );
        // The sender half is independent — repeated `try_send_event`
        // calls after the receiver was taken must not panic.
        s.try_send_event(SessionEvent::Status(SessionEventStatus::Started));
        s.try_send_event(SessionEvent::Status(SessionEventStatus::Started));
    }

    /// After a consumer calls `take_event_receiver`, a second call must
    /// return `None`. The single-consumer contract is critical: a
    /// second consumer would either steal events mid-flight or hang on
    /// receive forever.
    #[tokio::test]
    async fn session_state_take_event_receiver_is_singleton() {
        let s = SessionState::new(Uuid::new_v4());
        let _first = s.take_event_receiver().await;
        assert!(
            s.take_event_receiver().await.is_none(),
            "second take must return None"
        );
    }

    /// Sending on session A must not appear in session B's receiver.
    /// This is the load-bearing invariant for multi-thread chat: if it
    /// breaks, two concurrent streams interleave and a thread switch
    /// would corrupt the focused session's view.
    #[tokio::test]
    async fn session_event_queue_is_per_session_isolated() {
        let reg = SessionRegistry::new();
        let k_a = reg.create();
        let k_b = reg.create();
        let a = reg.get(k_a).expect("session A must exist");
        let b = reg.get(k_b).expect("session B must exist");

        // Take each session's receiver up front.
        let mut rx_a = a.take_event_receiver().await.expect("A receiver");
        let mut rx_b = b.take_event_receiver().await.expect("B receiver");

        // Send a ToolStart on session A.
        let a_payload = crate::events::ToolStartPayload {
            query_id: "qa".into(),
            tool_use_id: "tu-a".into(),
            tool_name: "bash".into(),
            tool_input: serde_json::json!({"command": "ls"}),
        };
        a.try_send_event(SessionEvent::ToolStart(a_payload.clone()));

        // And a QueryText on session B with a different content so we
        // can't confuse the two messages.
        let b_payload = crate::events::QueryTextPayload {
            query_id: "qb".into(),
            content: "session B says hi".into(),
        };
        b.try_send_event(SessionEvent::QueryText(b_payload.clone()));

        // Drain A — must see ONLY the ToolStart, never any session-B event.
        let got_a = rx_a.recv().await.expect("A must have an event");
        match got_a {
            SessionEvent::ToolStart(p) => {
                assert_eq!(p.tool_use_id, "tu-a");
                assert_eq!(p.query_id, "qa");
            }
            other => panic!("session A saw a non-ToolStart event: {other:?}"),
        }
        // A must have no further events.
        assert!(
            rx_a.try_recv().is_err(),
            "session A must not have bled session B's QueryText"
        );

        // Drain B — must see ONLY the QueryText.
        let got_b = rx_b.recv().await.expect("B must have an event");
        match got_b {
            SessionEvent::QueryText(p) => assert_eq!(p.content, "session B says hi"),
            other => panic!("session B saw a non-QueryText event: {other:?}"),
        }
        assert!(
            rx_b.try_recv().is_err(),
            "session B must not have bled session A's ToolStart"
        );
    }

    /// `get_or_create` must remain idempotent under concurrent access —
    /// the per-session event channel must not be re-allocated, or a
    /// consumer that raced a `get_or_create` could end up holding a
    /// stale receiver.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn session_registry_get_or_create_idempotent_under_concurrency() {
        let reg = std::sync::Arc::new(SessionRegistry::new());
        let key = SessionKey::new();

        // Fan out 32 concurrent `get_or_create` calls — all of them
        // must converge on the same `Arc<SessionState>` and therefore
        // the same `events_tx`.
        let mut handles = Vec::new();
        for _ in 0..32 {
            let reg = reg.clone();
            handles.push(tokio::spawn(async move { reg.get_or_create(key) }));
        }
        let mut all_same = true;
        let first = handles.pop().unwrap().await.unwrap();
        for h in handles {
            let s = h.await.unwrap();
            if !Arc::ptr_eq(&first, &s) {
                all_same = false;
                break;
            }
        }
        assert!(
            all_same,
            "concurrent get_or_create must return the same Arc (channel included)"
        );

        // And the receiver is reachable exactly once from the
        // canonical handle.
        let rx = first.take_event_receiver().await;
        assert!(rx.is_some(), "first call takes the receiver");
    }

    /// `try_send_event` after the receiver was dropped must not panic.
    /// `UnboundedSender::send` would otherwise return `Err` for every
    /// event, which we silently absorb — `send_message` does not depend
    /// on anybody listening.
    #[tokio::test]
    async fn session_state_try_send_event_after_receiver_dropped_is_silent() {
        let s = SessionState::new(Uuid::new_v4());
        let rx = s.take_event_receiver().await.expect("receiver");
        drop(rx); // consumer went away
        // These two calls must not panic. They will both fail silently
        // because the receiver is gone, but `send_message` must keep
        // streaming.
        s.try_send_event(SessionEvent::Status(SessionEventStatus::Started));
        s.try_send_event(SessionEvent::Status(SessionEventStatus::Cancelled));
    }
}
