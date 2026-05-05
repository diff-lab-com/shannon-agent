//! UiAdapter trait implementation for Repl.

use crate::adapter::{UiAdapter, UiError, UiResult, DisplayMessage};
use async_trait::async_trait;

/// Implement UiAdapter for Repl to allow it to be used as a UI backend.
#[async_trait]
impl UiAdapter for super::Repl {
    fn supports_streaming(&self) -> bool {
        true // Terminal UI supports streaming output
    }

    async fn display(&self, message: &DisplayMessage) -> UiResult<()> {
        // The TUI event loop handles rendering via the chat widget.
        // This method exists so the Repl satisfies the trait; actual output
        // flows through QueryEvent streams in the main loop.
        let _ = message;
        Ok(())
    }

    async fn display_progress(&self, message: &str, percent: Option<u8>) -> UiResult<()> {
        // Update status with progress message.
        let _ = (message, percent);
        Ok(())
    }

    async fn read_input(&self, prompt: &str) -> UiResult<String> {
        // In the current terminal UI, input is handled by the event loop.
        let _ = prompt;
        Err(UiError::NotSupported(
            "read_input not supported in terminal UI - use the prompt widget instead".to_string(),
        ))
    }

    async fn confirm(&self, message: &str) -> UiResult<bool> {
        // Confirmation is handled through dialog widgets in the event loop.
        let _ = message;
        Err(UiError::NotSupported(
            "confirm not directly supported - use dialog widgets instead".to_string(),
        ))
    }
}
