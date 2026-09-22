use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SessionSummary {
    pub id: Uuid,
    pub created_at: String,
    pub message_count: usize,
}

#[derive(Clone)]
pub struct SessionState {
    pub summary: SessionSummary,
    pub engine: Arc<tokio::sync::Mutex<shannon_core::query_engine::QueryEngine>>,
    /// Last time this session was created or looked up. Drives the idle-TTL
    /// and LRU eviction added by review §P2-1.
    last_access: Instant,
}

/// Server-side session registry.
///
/// Review §P2-1: previously this was an unbounded `HashMap` — every
/// `POST /v1/sessions` (and every GitHub-hook routine execution) inserted an
/// engine that was never removed, so a long-running `shannon serve` leaked
/// memory without limit. The registry is now bounded two ways:
///
/// - **LRU capacity** ([`SessionRegistry::DEFAULT_CAPACITY`]): inserting
///   beyond the capacity first drops TTL-expired sessions, then the
///   least-recently-accessed one.
/// - **Idle TTL** ([`SessionRegistry::DEFAULT_IDLE_TTL`]): sessions not
///   accessed (created / GET / message posted) within the TTL are removed
///   lazily on lookup and eagerly on insert.
///
/// Eviction drops the registry's `Arc` to the engine; the engine itself is
/// only destroyed once no in-flight request still holds a clone, so an
/// active SSE stream always finishes and `QueryEngine`'s own `Drop` handles
/// any cleanup — eviction is therefore safe mid-query.
#[derive(Clone)]
pub struct SessionRegistry {
    sessions: Arc<RwLock<HashMap<Uuid, SessionState>>>,
    capacity: usize,
    idle_ttl: Duration,
}

impl Default for SessionRegistry {
    fn default() -> Self {
        Self::with_limits(Self::DEFAULT_CAPACITY, Self::DEFAULT_IDLE_TTL)
    }
}

impl SessionRegistry {
    /// Maximum retained sessions. Sized for a personal serve deployment:
    /// hundreds of idle engines would each hold provider clients and
    /// conversation buffers, far beyond any realistic single-process usage.
    pub const DEFAULT_CAPACITY: usize = 256;
    /// Idle eviction window (access refreshes it). 24h mirrors the desktop
    /// session-retention intuition: a REST/webhook client coming back after
    /// a day starts a fresh session anyway.
    pub const DEFAULT_IDLE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

    /// Test/ops seam: explicit capacity and idle TTL.
    pub fn with_limits(capacity: usize, idle_ttl: Duration) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            capacity: capacity.max(1),
            idle_ttl,
        }
    }

    pub async fn create(&self, engine: shannon_core::query_engine::QueryEngine) -> SessionSummary {
        let id = Uuid::new_v4();
        let summary = SessionSummary {
            id,
            created_at: chrono::Utc::now().to_rfc3339(),
            message_count: 0,
        };
        let mut sessions = self.sessions.write().await;
        self.evict_locked(&mut sessions, id);
        sessions.insert(
            id,
            SessionState {
                summary: summary.clone(),
                engine: Arc::new(tokio::sync::Mutex::new(engine)),
                last_access: Instant::now(),
            },
        );
        summary
    }

    pub async fn get(&self, id: Uuid) -> Option<SessionState> {
        let mut sessions = self.sessions.write().await;
        let ttl = self.idle_ttl;
        let state = sessions.get_mut(&id)?;
        if state.last_access.elapsed() > ttl {
            // Lazy TTL expiry: idle session, drop it (see struct docs for the
            // engine-drop safety argument).
            sessions.remove(&id);
            return None;
        }
        state.last_access = Instant::now();
        Some(state.clone())
    }

    /// Number of retained sessions (used by tests and health introspection).
    pub async fn len(&self) -> usize {
        self.sessions.read().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.sessions.read().await.is_empty()
    }

    /// Make room for `incoming` before it is inserted: first remove every
    /// TTL-expired session, then — if still full — the least-recently
    /// accessed one. Caller holds the write lock.
    fn evict_locked(&self, sessions: &mut HashMap<Uuid, SessionState>, incoming: Uuid) {
        let ttl = self.idle_ttl;
        sessions.retain(|_, state| state.last_access.elapsed() <= ttl);
        if sessions.len() < self.capacity {
            return;
        }
        let Some(oldest) = sessions
            .iter()
            .filter(|(id, _)| **id != incoming)
            .min_by_key(|(_, state)| state.last_access)
            .map(|(id, _)| *id)
        else {
            return;
        };
        sessions.remove(&oldest);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use shannon_engine::api::{LlmClient, LlmClientConfig, types::LlmProvider};

    fn test_registry(capacity: usize, ttl: Duration) -> SessionRegistry {
        SessionRegistry::with_limits(capacity, ttl)
    }

    fn test_engine() -> shannon_core::query_engine::QueryEngine {
        let config = LlmClientConfig {
            provider: LlmProvider::Ollama,
            model: "test-model".into(),
            base_url: "http://127.0.0.1:1".into(),
            ..Default::default()
        };
        let client = LlmClient::new_unauthenticated(config);
        shannon_core::query_engine::QueryEngine::with_defaults(
            client,
            shannon_core::tools::ToolRegistry::new(),
            shannon_engine::permissions::PermissionManager::new(),
            shannon_engine::state::StateManager::new(),
        )
    }

    async fn create_session(registry: &SessionRegistry) -> SessionSummary {
        registry.create(test_engine()).await
    }

    // Review §P2-1: inserting beyond the LRU capacity evicts the
    // least-recently-accessed session, not the newest one.
    #[tokio::test]
    async fn capacity_evicts_least_recently_accessed() {
        let registry = test_registry(2, Duration::from_secs(3600));
        let a = create_session(&registry).await;
        let b = create_session(&registry).await;
        // Touch `a` so `b` becomes the least-recently-accessed entry.
        let _ = registry.get(a.id).await;
        let c = create_session(&registry).await;

        assert_eq!(registry.len().await, 2, "capacity must be enforced");
        assert!(
            registry.get(a.id).await.is_some(),
            "recently accessed session must survive"
        );
        assert!(
            registry.get(b.id).await.is_none(),
            "least-recently-accessed session must be evicted"
        );
        assert!(
            registry.get(c.id).await.is_some(),
            "newest session must survive"
        );
    }

    // Review §P2-1: a session idle beyond the TTL is removed (lazily on get).
    #[tokio::test]
    async fn ttl_expiry_removes_idle_session() {
        let registry = test_registry(8, Duration::from_millis(20));
        let s = create_session(&registry).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            registry.get(s.id).await.is_none(),
            "session idle past the TTL must be expired"
        );
        assert_eq!(registry.len().await, 0, "expired entry must be removed");
    }

    // Review §P2-1: an actively-accessed session must NOT be dropped even
    // when the elapsed wall time exceeds the TTL — every access refreshes it.
    #[tokio::test]
    async fn active_session_is_not_evicted() {
        let registry = test_registry(8, Duration::from_millis(60));
        let s = create_session(&registry).await;
        // Keep touching the session across a total wall time > TTL.
        for _ in 0..5 {
            tokio::time::sleep(Duration::from_millis(25)).await;
            assert!(
                registry.get(s.id).await.is_some(),
                "actively accessed session must not expire while touches are within the TTL"
            );
        }
        assert_eq!(registry.len().await, 1);
    }

    // Review §P2-1: TTL-expired sessions are reclaimed eagerly on insert, so
    // a full registry of dead sessions does not force an LRU eviction of a
    // live one.
    #[tokio::test]
    async fn insert_reclaims_expired_before_evicting_live_sessions() {
        let registry = test_registry(2, Duration::from_millis(20));
        let a = create_session(&registry).await;
        let b = create_session(&registry).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        // Both a and b are expired; inserting c must reclaim them instead of
        // evicting anything live, leaving room for all three slots to settle.
        let c = create_session(&registry).await;
        assert!(registry.get(c.id).await.is_some());
        assert!(registry.get(a.id).await.is_none());
        assert!(registry.get(b.id).await.is_none());
        assert!(registry.len().await <= 2, "capacity still enforced");
    }

    // Review §P2-1: default limits match the values cited in the review fix.
    #[test]
    fn default_limits_are_bounded() {
        let registry = SessionRegistry::default();
        assert_eq!(registry.capacity, SessionRegistry::DEFAULT_CAPACITY);
        assert_eq!(registry.idle_ttl, SessionRegistry::DEFAULT_IDLE_TTL);
        assert_eq!(SessionRegistry::DEFAULT_CAPACITY, 256);
        assert_eq!(
            SessionRegistry::DEFAULT_IDLE_TTL,
            Duration::from_secs(24 * 60 * 60)
        );
    }
}
