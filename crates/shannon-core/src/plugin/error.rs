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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_error_display_variants() {
        assert!(PluginError::NotFound("x".into()).to_string().contains("x"));
        assert!(PluginError::AlreadyInstalled("p".into()).to_string().contains("p"));
        assert!(PluginError::InvalidManifest("bad".into()).to_string().contains("bad"));
        assert!(PluginError::InvalidDirectory(PathBuf::from("/tmp")).to_string().contains("/tmp"));
        assert!(PluginError::GitFailed("clone".into()).to_string().contains("clone"));
        assert!(PluginError::Generic("msg".into()).to_string().contains("msg"));
        assert!(PluginError::Network("timeout".into()).to_string().contains("timeout"));
        assert!(PluginError::PermissionDenied("denied".into()).to_string().contains("denied"));
        assert!(PluginError::IndexRefreshFailed("err".into()).to_string().contains("err"));
    }

    #[test]
    fn plugin_error_wrong_state_display() {
        let err = PluginError::WrongState {
            name: "my-plugin".into(),
            state: "disabled".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("my-plugin"));
        assert!(msg.contains("disabled"));
    }

    #[test]
    fn plugin_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let err: PluginError = io_err.into();
        assert!(err.to_string().contains("missing"));
    }

    #[test]
    fn plugin_error_from_serde_json() {
        let json_err = serde_json::from_str::<i32>("bad").unwrap_err();
        let err: PluginError = json_err.into();
        assert!(err.to_string().contains("Serialization"));
    }

    #[test]
    fn plugin_error_from_toml() {
        let toml_err = toml::from_str::<toml::Value>("{invalid").unwrap_err();
        let err: PluginError = toml_err.into();
        assert!(err.to_string().contains("TOML"));
    }

    #[test]
    fn plugin_result_ok() {
        let result: PluginResult<i32> = Ok(42);
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn plugin_result_err() {
        let result: PluginResult<i32> = Err(PluginError::NotFound("test".into()));
        assert!(result.is_err());
    }

    #[test]
    fn send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<PluginError>();
    }
}
