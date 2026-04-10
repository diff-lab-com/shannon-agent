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
