//! UI Adapter -- backward-compatible re-exports
//!
//! The canonical [`UiAdapter`] trait now lives in `shannon-core::ui_adapter`
//! so that the core engine can define the interface without depending on the
//! UI crate.  This module re-exports those types for crates that still import
//! them from `shannon-ui::adapter`.

// Re-export the canonical types from shannon-core.
pub use shannon_core::ui_adapter::{
    DefaultUiAdapter, DisplayMessage, MessageSeverity, NullUiAdapter, UiAdapter, UiError, UiResult,
    UserChoice,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_null_adapter_display() {
        let adapter = NullUiAdapter;
        assert!(adapter.display(&DisplayMessage::info("test")).await.is_ok());
        assert!(adapter.display_progress("loading", Some(50)).await.is_ok());
    }

    #[tokio::test]
    async fn test_null_adapter_input() {
        let adapter = NullUiAdapter;
        let result = adapter.read_input("prompt: ").await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_null_adapter_confirm() {
        let adapter = NullUiAdapter;
        assert!(adapter.confirm("Continue?").await.unwrap());
    }

    #[test]
    fn test_null_adapter_no_streaming() {
        let adapter = NullUiAdapter;
        assert!(!adapter.supports_streaming());
    }

    #[test]
    fn test_null_adapter_no_input() {
        let adapter = NullUiAdapter;
        assert!(!adapter.supports_input());
    }

    #[test]
    fn test_display_message_reexport() {
        let msg = DisplayMessage::info("hello");
        assert_eq!(msg.content, "hello");
        assert_eq!(msg.severity, MessageSeverity::Info);
    }
}
