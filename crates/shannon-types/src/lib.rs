//! # Shannon Shared Types
//!
//! Common types used across the Shannon project.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Unique identifier for entities in the Shannon system
pub type EntityId = Uuid;

/// Timestamp type
pub type Timestamp = chrono::DateTime<chrono::Utc>;

/// Generic result type for Shannon operations
pub type ShannonResult<T> = Result<T, ShannonError>;

/// Common error type
#[derive(Debug, thiserror::Error)]
pub enum ShannonError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Not found: {0}")]
    NotFound(String),
}

/// Generic entity trait
pub trait Entity {
    /// Get the entity's unique ID
    fn id(&self) -> EntityId;

    /// Get the entity's creation timestamp
    fn created_at(&self) -> Timestamp;
}
