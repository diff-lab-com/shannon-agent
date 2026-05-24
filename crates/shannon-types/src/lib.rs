// Suppress lints that conflict with rustfmt or are style preferences from newer clippy.
#![allow(
    clippy::collapsible_if,
    clippy::collapsible_match,
    clippy::derivable_impls,
)]

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

impl Entity for Message {
    fn id(&self) -> EntityId {
        self.id.parse().unwrap_or_else(|_| EntityId::new_v4())
    }

    fn created_at(&self) -> Timestamp {
        self.timestamp
    }
}

/// Recover from a poisoned lock by extracting the inner guard.
///
/// When a thread panics while holding a `std::sync` lock, the lock becomes
/// "poisoned". In most cases the data is still valid, so we recover by
/// extracting the inner value rather than propagating the panic.
pub fn recover_lock<T>(lock_result: std::sync::LockResult<T>) -> T {
    lock_result.unwrap_or_else(|e| e.into_inner())
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
        let result: Result<serde_json::Value, serde_json::Error> =
            serde_json::from_str("invalid json");
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
        let ts: Timestamp = chrono::DateTime::parse_from_rfc3339(ts_str)
            .unwrap()
            .with_timezone(&chrono::Utc);
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

    // ── ToolUse tests ──────────────────────────────────────────────────

    #[test]
    fn test_tool_use_serialization_roundtrip() {
        let tool = ToolUse {
            id: "tool_123".to_string(),
            name: "bash".to_string(),
            input: serde_json::json!({"command": "ls"}),
            output: Some(serde_json::json!("file1\nfile2")),
        };
        let json = serde_json::to_string(&tool).unwrap();
        let parsed: ToolUse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "tool_123");
        assert_eq!(parsed.name, "bash");
        assert!(parsed.output.is_some());
    }

    #[test]
    fn test_tool_use_no_output() {
        let tool = ToolUse {
            id: "tool_456".to_string(),
            name: "read".to_string(),
            input: serde_json::json!({"path": "/tmp/test"}),
            output: None,
        };
        let json = serde_json::to_string(&tool).unwrap();
        let parsed: ToolUse = serde_json::from_str(&json).unwrap();
        assert!(parsed.output.is_none());
    }

    #[test]
    fn test_tool_use_clone() {
        let tool = ToolUse {
            id: "t1".to_string(),
            name: "edit".to_string(),
            input: serde_json::json!({}),
            output: None,
        };
        let cloned = tool.clone();
        assert_eq!(cloned.id, tool.id);
        assert_eq!(cloned.name, tool.name);
    }

    // ── Message tests ──────────────────────────────────────────────────

    #[test]
    fn test_message_serialization_roundtrip() {
        let msg = Message {
            id: "msg_1".to_string(),
            role: "user".to_string(),
            content: "Hello, world!".to_string(),
            timestamp: chrono::Utc::now(),
            metadata: serde_json::json!({"key": "value"}),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "msg_1");
        assert_eq!(parsed.role, "user");
        assert_eq!(parsed.content, "Hello, world!");
    }

    #[test]
    fn test_message_entity_trait() {
        let uuid = EntityId::new_v4();
        let msg = Message {
            id: uuid.to_string(),
            role: "assistant".to_string(),
            content: "response".to_string(),
            timestamp: chrono::Utc::now(),
            metadata: serde_json::json!(null),
        };
        assert_eq!(msg.id(), uuid);
        assert_eq!(msg.created_at(), msg.timestamp);
    }

    #[test]
    fn test_message_entity_invalid_uuid_falls_back() {
        let msg = Message {
            id: "not-a-uuid".to_string(),
            role: "user".to_string(),
            content: "test".to_string(),
            timestamp: chrono::Utc::now(),
            metadata: serde_json::json!({}),
        };
        // Should not panic — returns a new random UUID
        let id = msg.id();
        assert_ne!(id.to_string(), "not-a-uuid");
    }

    // ── recover_lock tests ─────────────────────────────────────────────

    #[test]
    fn test_recover_lock_clean() {
        let lock = std::sync::Mutex::new(42);
        let guard = lock.lock().unwrap();
        drop(guard);
        let result = lock.lock();
        let value = recover_lock(result);
        assert_eq!(*value, 42);
    }

    #[test]
    fn test_recover_lock_poisoned() {
        let lock = std::sync::Mutex::new(99);
        let lock_clone = std::sync::Arc::new(lock);
        let lock_ref = lock_clone.clone();

        let handle = std::thread::spawn(move || {
            let _guard = lock_ref.lock().unwrap();
            panic!("intentional panic to poison lock");
        });

        // Thread panicked, lock is now poisoned
        assert!(handle.join().is_err());

        // recover_lock should recover the value despite poisoning
        let result = lock_clone.lock();
        let value = recover_lock(result);
        assert_eq!(*value, 99);
    }

    #[test]
    fn test_recover_lock_rwlock_clean() {
        let lock = std::sync::RwLock::new("hello");
        let guard = lock.read().unwrap();
        drop(guard);
        let result = lock.read();
        let value = recover_lock(result);
        assert_eq!(*value, "hello");
    }

    // ── ShannonError variant tests ─────────────────────────────────────

    #[test]
    fn test_shannon_error_not_found() {
        let err = ShannonError::NotFound("resource".to_string());
        assert!(err.to_string().contains("resource"));
    }

    #[test]
    fn test_shannon_error_io_kind() {
        let err = ShannonError::Io(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "pipe broke",
        ));
        assert!(err.to_string().contains("pipe broke"));
    }

    #[test]
    fn test_shannon_error_chain() {
        fn inner() -> ShannonResult<()> {
            Err(ShannonError::NotFound("inner error".to_string()))
        }
        let result = inner();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ShannonError::NotFound(_)));
    }

    #[test]
    fn test_types_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ShannonError>();
        assert_send_sync::<Message>();
        assert_send_sync::<ToolUse>();
        assert_send_sync::<EntityId>();
    }
}
