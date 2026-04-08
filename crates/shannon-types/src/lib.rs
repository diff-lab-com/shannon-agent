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

/// Message role (user, assistant, system, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUse {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
    pub output: Option<serde_json::Value>,
}

/// Message in a conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub role: String,
    pub content: String,
    pub timestamp: Timestamp,
    pub metadata: serde_json::Value,
}

/// Generic entity trait
pub trait Entity {
    /// Get the entity's unique ID
    fn id(&self) -> EntityId;

    /// Get the entity's creation timestamp
    fn created_at(&self) -> Timestamp;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shannon_error_display() {
        let err = ShannonError::NotFound("test resource".to_string());
        assert_eq!(format!("{err}"), "Not found: test resource");

        let err = ShannonError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file not found",
        ));
        assert!(format!("{err}").contains("file not found"));
    }

    #[test]
    fn test_shannon_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let err: ShannonError = io_err.into();
        assert!(matches!(err, ShannonError::Io(_)));
    }

    #[test]
    fn test_shannon_error_from_serde() {
        let result: Result<serde_json::Value, serde_json::Error> = serde_json::from_str("invalid json");
        let err: ShannonError = result.unwrap_err().into();
        assert!(matches!(err, ShannonError::Serialization(_)));
    }

    #[test]
    fn test_entity_id_serialization_roundtrip() {
        let id = EntityId::new_v4();
        let json = serde_json::to_string(&id).unwrap();
        let parsed: EntityId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn test_entity_id_known_value() {
        let id_str = "550e8400-e29b-41d4-a716-446655440000";
        let id: EntityId = id_str.parse().unwrap();
        let json = serde_json::to_string(&id).unwrap();
        assert!(json.contains(id_str));
        let parsed: EntityId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn test_timestamp_serialization_roundtrip() {
        let ts = chrono::Utc::now();
        let json = serde_json::to_string(&ts).unwrap();
        let parsed: Timestamp = serde_json::from_str(&json).unwrap();
        assert_eq!(ts, parsed);
    }

    #[test]
    fn test_timestamp_rfc3339_format() {
        let ts_str = "2024-01-15T12:00:00Z";
        let ts: Timestamp = chrono::DateTime::parse_from_rfc3339(ts_str).unwrap().with_timezone(&chrono::Utc);
        let json = serde_json::to_string(&ts).unwrap();
        let parsed: Timestamp = serde_json::from_str(&json).unwrap();
        assert_eq!(ts, parsed);
    }

    #[test]
    fn test_shannon_result_ok() {
        let result: ShannonResult<i32> = Ok(42);
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_shannon_result_err() {
        let result: ShannonResult<i32> = Err(ShannonError::NotFound("missing".to_string()));
        assert!(result.is_err());
    }
}
