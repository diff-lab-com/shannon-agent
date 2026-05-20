//! Shannon Code error types
//!
//! NOTE: This module is not yet integrated into the crate's module tree.
//! The `error` module in `lib.rs` is an inline re-export block for specific
//! error types. This file defines a unified `ShannonError` enum intended
//! for future use as a top-level error type.

use thiserror::Error;

pub type Result<T> = std::result::Result<T, ShannonError>;

#[derive(Error, Debug)]
pub enum ShannonError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Tool execution error: {0}")]
    ToolExecution(String),

    #[error("Agent error: {0}")]
    Agent(String),

    #[error("Configuration error: {0}")]
    Configuration(String),

    #[error("Not found: {0}")]
    NotFound(String),
}
