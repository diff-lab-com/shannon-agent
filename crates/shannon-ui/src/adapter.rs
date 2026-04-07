//! UI Adapter Trait
//!
//! Provides a unified interface for different UI implementations (terminal, web, etc.).

use async_trait::async_trait;

/// Result type for UI operations
pub type UiResult<T> = Result<T, UiError>;

/// Error type for UI operations
#[derive(Debug, thiserror::Error)]
pub enum UiError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Rendering error: {0}")]
    Render(String),

    #[error("Input error: {0}")]
    Input(String),

    #[error("Not supported: {0}")]
    NotSupported(String),
}

/// UI Adapter trait - provides a unified interface for different UI backends
///
/// This trait allows Shannon to work with different UI implementations:
/// - Terminal/TUI (Ratatui-based Repl)
/// - Web interfaces
/// - Test/mocking implementations
#[async_trait]
pub trait UiAdapter: Send + Sync {
    /// Display output to the user
    async fn display_output(&self, content: &str) -> UiResult<()>;

    /// Display an error message to the user
    async fn display_error(&self, error: &str) -> UiResult<()>;

    /// Display a progress message with optional percentage
    async fn display_progress(&self, message: &str, percent: Option<u8>) -> UiResult<()>;

    /// Read input from the user
    async fn read_input(&self, prompt: &str) -> UiResult<String>;

    /// Request confirmation from the user (yes/no)
    async fn confirm(&self, message: &str) -> UiResult<bool>;

    /// Check if this adapter supports streaming output
    fn supports_streaming(&self) -> bool {
        false
    }
}

/// Null UI adapter - used for testing or headless mode
#[derive(Debug, Default, Clone)]
pub struct NullUiAdapter;

#[async_trait]
impl UiAdapter for NullUiAdapter {
    async fn display_output(&self, _content: &str) -> UiResult<()> {
        Ok(())
    }

    async fn display_error(&self, _error: &str) -> UiResult<()> {
        Ok(())
    }

    async fn display_progress(&self, _message: &str, _percent: Option<u8>) -> UiResult<()> {
        Ok(())
    }

    async fn read_input(&self, prompt: &str) -> UiResult<String> {
        // Return empty string for headless mode
        Ok(prompt.to_string())
    }

    async fn confirm(&self, _message: &str) -> UiResult<bool> {
        // Default to true for headless mode
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_null_adapter_display() {
        let adapter = NullUiAdapter;
        assert!(adapter.display_output("test").await.is_ok());
        assert!(adapter.display_error("error").await.is_ok());
    }

    #[tokio::test]
    async fn test_null_adapter_progress() {
        let adapter = NullUiAdapter;
        assert!(adapter.display_progress("loading", Some(50)).await.is_ok());
        assert!(adapter.display_progress("loading", None).await.is_ok());
    }

    #[tokio::test]
    async fn test_null_adapter_input() {
        let adapter = NullUiAdapter;
        let result = adapter.read_input("prompt: ").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "prompt: ");
    }

    #[tokio::test]
    async fn test_null_adapter_confirm() {
        let adapter = NullUiAdapter;
        assert!(adapter.confirm("Continue?").await.unwrap());
    }

    #[test]
    fn test_null_adapter_streaming() {
        let adapter = NullUiAdapter;
        assert!(!adapter.supports_streaming());
    }
}
