//! Plugin error types

use std::path::PathBuf;
use thiserror::Error;

/// Plugin error type
#[derive(Debug, Error)]
pub enum PluginError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parsing error: {0}")]
    TomlParse(#[from] toml::de::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Plugin not found: {0}")]
    NotFound(String),

    #[error("Plugin already installed: {0}")]
    AlreadyInstalled(String),

    #[error("Invalid manifest: {0}")]
    InvalidManifest(String),

    #[error("Invalid plugin directory: {0}")]
    InvalidDirectory(PathBuf),

    #[error("Git operation failed: {0}")]
    GitFailed(String),

    #[error("Generic error: {0}")]
    Generic(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Index refresh failed: {0}")]
    IndexRefreshFailed(String),

    #[error("Plugin '{name}' is {state}")]
    WrongState { name: String, state: String },
}

/// Plugin result type
pub type PluginResult<T> = Result<T, PluginError>;
