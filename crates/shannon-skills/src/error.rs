//! Error types for the skills system

use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur in the skills system
#[derive(Error, Debug)]
pub enum SkillError {
    /// IO error during skill loading
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Failed to parse skill frontmatter
    #[error("Failed to parse frontmatter for skill '{name}': {message}")]
    FrontmatterParse { name: String, message: String },

    /// Invalid skill metadata
    #[error("Invalid skill metadata in '{path}': {reason}")]
    InvalidMetadata { path: PathBuf, reason: String },

    /// Skill not found
    #[error("Skill not found: {0}")]
    NotFound(String),

    /// Skill execution failed
    #[error("Skill execution failed for '{name}': {message}")]
    ExecutionFailed { name: String, message: String },

    /// Invalid skill file format
    #[error("Invalid skill file format: {0}")]
    InvalidFormat(String),

    /// Path traversal attempt detected
    #[error("Path traversal attempt detected: {0}")]
    PathTraversal(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Result type for skill operations
pub type SkillResult<T> = Result<T, SkillError>;
