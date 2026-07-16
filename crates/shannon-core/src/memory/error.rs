use thiserror::Error;

/// Errors that can occur during memory operations
#[derive(Error, Debug)]
pub enum MemoryError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization/deserialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Memory not found: {0}")]
    NotFound(String),

    #[error("Invalid confidence value: {0}. Must be between 0.0 and 1.0")]
    InvalidConfidence(f64),
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_io_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let mem_err: MemoryError = io_err.into();
        assert!(matches!(mem_err, MemoryError::Io(_)));
        assert!(mem_err.to_string().contains("file missing"));
    }

    #[test]
    fn test_json_error_conversion() {
        let json_err = serde_json::from_str::<i32>("not json").unwrap_err();
        let mem_err: MemoryError = json_err.into();
        assert!(matches!(mem_err, MemoryError::Json(_)));
    }

    #[test]
    fn test_not_found_display() {
        let err = MemoryError::NotFound("id-123".to_string());
        assert!(err.to_string().contains("id-123"));
    }

    #[test]
    fn test_invalid_confidence_display() {
        let err = MemoryError::InvalidConfidence(1.5);
        let msg = err.to_string();
        assert!(msg.contains("1.5"));
        assert!(msg.contains("0.0"));
        assert!(msg.contains("1.0"));
    }

    #[test]
    fn test_error_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MemoryError>();
    }
}
