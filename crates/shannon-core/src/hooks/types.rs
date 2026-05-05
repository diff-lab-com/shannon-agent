//! Core error types for the hooks system.

use thiserror::Error;

/// Errors that can occur during hook operations
#[derive(Error, Debug)]
pub enum HookError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization/deserialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Hook execution timed out after {timeout_secs}s: {command}")]
    Timeout { command: String, timeout_secs: u64 },

    #[error("Hook command failed with exit code {exit_code}: {command}")]
    CommandFailed {
        command: String,
        exit_code: i32,
        stderr: String,
    },

    #[error("Invalid matcher pattern: {0}")]
    InvalidMatcher(String),

    #[error("Hook denied operation: {reason}")]
    Denied { reason: String },

    #[error("Home directory not found")]
    HomeNotFound,
}
