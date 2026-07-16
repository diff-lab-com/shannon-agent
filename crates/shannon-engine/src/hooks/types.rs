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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_io_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let hook_err: HookError = io_err.into();
        let msg = hook_err.to_string();
        assert!(msg.contains("I/O error"));
        assert!(msg.contains("file missing"));
    }

    #[test]
    fn test_json_error_conversion() {
        let json_err = serde_json::from_str::<i32>("not a number").unwrap_err();
        let hook_err: HookError = json_err.into();
        let msg = hook_err.to_string();
        assert!(msg.contains("JSON"));
    }

    #[test]
    fn test_timeout_message() {
        let err = HookError::Timeout {
            command: "sleep 999".to_string(),
            timeout_secs: 30,
        };
        let msg = err.to_string();
        assert!(msg.contains("30"));
        assert!(msg.contains("sleep 999"));
    }

    #[test]
    fn test_command_failed_message() {
        let err = HookError::CommandFailed {
            command: "bad-cmd".to_string(),
            exit_code: 1,
            stderr: "oops".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("exit code 1"));
        assert!(msg.contains("bad-cmd"));
    }

    #[test]
    fn test_invalid_matcher_message() {
        let err = HookError::InvalidMatcher("[".to_string());
        assert!(err.to_string().contains("["));
    }

    #[test]
    fn test_denied_message() {
        let err = HookError::Denied {
            reason: "unsafe".to_string(),
        };
        assert!(err.to_string().contains("unsafe"));
    }

    #[test]
    fn test_home_not_found_message() {
        let err = HookError::HomeNotFound;
        assert!(err.to_string().contains("Home directory"));
    }

    #[test]
    fn test_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<HookError>();
    }
}
