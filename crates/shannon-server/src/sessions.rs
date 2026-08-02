use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
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
}

#[derive(Clone, Default)]
pub struct SessionRegistry {
    sessions: Arc<RwLock<HashMap<Uuid, SessionState>>>,
}

impl SessionRegistry {
    pub async fn create(&self, engine: shannon_core::query_engine::QueryEngine) -> SessionSummary {
        let id = Uuid::new_v4();
        let summary = SessionSummary {
            id,
            created_at: chrono::Utc::now().to_rfc3339(),
            message_count: 0,
        };
        self.sessions.write().await.insert(
            id,
            SessionState {
                summary: summary.clone(),
                engine: Arc::new(tokio::sync::Mutex::new(engine)),
            },
        );
        summary
    }
    pub async fn get(&self, id: Uuid) -> Option<SessionState> {
        self.sessions.read().await.get(&id).cloned()
    }
}
