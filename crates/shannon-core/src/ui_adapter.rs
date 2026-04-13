//! # UI Adapter
//!
//! Trait that abstracts UI operations so the core engine does not directly
//! depend on any particular UI layer (terminal, web, etc.).
//!
//! The core engine communicates results through [`QueryEvent`] streams and
//! permission channels. Downstream consumers (REPL, web UI, test harnesses)
//! implement [`UiAdapter`] to render those events and collect user input.

use async_trait::async_trait;
use std::fmt;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors that can occur during UI operations.
#[derive(Debug, thiserror::Error)]
pub enum UiError {
    /// An I/O error occurred.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// A rendering / display failure.
    #[error("Rendering error: {0}")]
    Render(String),

    /// A failure to read user input.
    #[error("Input error: {0}")]
    Input(String),

    /// The operation is not supported by this adapter.
    #[error("Not supported: {0}")]
    NotSupported(String),
}

/// Convenience alias used by all [`UiAdapter`] methods.
pub type UiResult<T> = Result<T, UiError>;

// ---------------------------------------------------------------------------
// Severity for display messages
// ---------------------------------------------------------------------------

/// Severity level for messages shown to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MessageSeverity {
    /// Normal informational output.
    Info,
    /// A non-fatal warning.
    Warning,
    /// An error that the user should act on.
    Error,
}

impl fmt::Display for MessageSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Info => write!(f, "info"),
            Self::Warning => write!(f, "warning"),
            Self::Error => write!(f, "error"),
        }
    }
}

impl Default for MessageSeverity {
    fn default() -> Self {
        Self::Info
    }
}

// ---------------------------------------------------------------------------
// User choice returned from multi-option prompts
// ---------------------------------------------------------------------------

/// A choice the user made in response to a prompt.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum UserChoice {
    /// The user approved / confirmed.
    Confirm,
    /// The user declined / cancelled.
    Cancel,
    /// The user selected a custom option identified by label.
    Other(String),
}

// ---------------------------------------------------------------------------
// DisplayMessage -- structured content the UI should render
// ---------------------------------------------------------------------------

/// A structured message that can be presented to the user.
#[derive(Debug, Clone)]
pub struct DisplayMessage {
    /// Primary text to display.
    pub content: String,
    /// Optional severity hint.
    pub severity: MessageSeverity,
    /// Optional title / heading.
    pub title: Option<String>,
}

impl DisplayMessage {
    /// Create a simple informational message.
    pub fn info(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            severity: MessageSeverity::Info,
            title: None,
        }
    }

    /// Create a warning message.
    pub fn warning(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            severity: MessageSeverity::Warning,
            title: None,
        }
    }

    /// Create an error message.
    pub fn error(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            severity: MessageSeverity::Error,
            title: None,
        }
    }

    /// Attach a title / heading to the message.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

// ---------------------------------------------------------------------------
// UiAdapter trait
// ---------------------------------------------------------------------------

/// Trait that decouples the core engine from any specific UI implementation.
///
/// Implementations:
/// - [`DefaultUiAdapter`] -- simple stdout/stderr printing (suitable for CLI
///   tools and tests).
/// - `NullUiAdapter` (in `shannon-ui`) -- silently discards everything
///   (headless / testing).
/// - `Repl` (in `shannon-ui`) -- full TUI via Ratatui.
///
/// # Design note
///
/// The core engine primarily communicates through the [`QueryEvent`](crate::query_engine::QueryEvent)
/// stream returned by [`QueryEngine::process_query`](crate::query_engine::QueryEngine).
/// That stream already carries text, tool-use notifications, progress, and
/// usage events.  The `UiAdapter` trait therefore covers the *remaining*
/// interaction surface -- prompts that genuinely require a user response
/// outside the normal event stream.
#[async_trait]
pub trait UiAdapter: Send + Sync {
    // -- Output ----------------------------------------------------------------

    /// Display a message to the user.
    async fn display(&self, message: &DisplayMessage) -> UiResult<()>;

    /// Display a progress update with an optional completion percentage
    /// (0-100).
    async fn display_progress(&self, message: &str, percent: Option<u8>) -> UiResult<()>;

    // -- Input -----------------------------------------------------------------

    /// Read a single line of text from the user.
    async fn read_input(&self, prompt: &str) -> UiResult<String>;

    /// Ask a yes/no confirmation.
    async fn confirm(&self, message: &str) -> UiResult<bool>;

    /// Present the user with a list of choices and return the selection.
    ///
    /// The default implementation falls back to a yes/no prompt using
    /// [`confirm`](UiAdapter::confirm).
    async fn choose(&self, prompt: &str, _choices: &[String]) -> UiResult<UserChoice> {
        let yes = self.confirm(prompt).await?;
        if yes {
            Ok(UserChoice::Confirm)
        } else {
            Ok(UserChoice::Cancel)
        }
    }

    // -- Capabilities ----------------------------------------------------------

    /// Whether this adapter supports real-time streaming output.
    fn supports_streaming(&self) -> bool {
        false
    }

    /// Whether this adapter supports interactive input.
    ///
    /// Returning `false` lets callers fall back to non-interactive defaults.
    fn supports_input(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// DefaultUiAdapter -- println/eprintln implementation
// ---------------------------------------------------------------------------

/// A minimal [`UiAdapter`] that writes to stdout / stderr.
///
/// Useful for CLI tools, scripts, and test harnesses that do not need a
/// full TUI.
#[derive(Debug, Default, Clone)]
pub struct DefaultUiAdapter;

#[async_trait]
impl UiAdapter for DefaultUiAdapter {
    async fn display(&self, message: &DisplayMessage) -> UiResult<()> {
        let prefix = match message.severity {
            MessageSeverity::Info => "",
            MessageSeverity::Warning => "[warn] ",
            MessageSeverity::Error => "[error] ",
        };

        if let Some(ref title) = message.title {
            println!("{prefix}{title}");
            println!("{}{}", prefix, message.content);
        } else {
            println!("{}{}", prefix, message.content);
        }
        Ok(())
    }

    async fn display_progress(&self, message: &str, percent: Option<u8>) -> UiResult<()> {
        match percent {
            Some(p) => print!("\r[{p}%] {message}"),
            None => print!("\r  {message}"),
        }
        use std::io::Write;
        std::io::stdout().flush()?;
        Ok(())
    }

    async fn read_input(&self, prompt: &str) -> UiResult<String> {
        print!("{prompt} ");
        use std::io::Write;
        std::io::stdout().flush()?;

        let mut buf = String::new();
        std::io::stdin().read_line(&mut buf)?;
        Ok(buf.trim_end().to_string())
    }

    async fn confirm(&self, message: &str) -> UiResult<bool> {
        let input = self.read_input(&format!("{message} [y/N]")).await?;
        Ok(input.eq_ignore_ascii_case("y") || input.eq_ignore_ascii_case("yes"))
    }

    fn supports_streaming(&self) -> bool {
        false
    }

    fn supports_input(&self) -> bool {
        // Conservative: assume stdin is available unless the caller overrides.
        true
    }
}

// ---------------------------------------------------------------------------
// NullUiAdapter -- silently discards everything
// ---------------------------------------------------------------------------

/// A [`UiAdapter`] that silently discards output and returns default values
/// for input.  Useful for headless operation and unit tests.
#[derive(Debug, Default, Clone)]
pub struct NullUiAdapter;

#[async_trait]
impl UiAdapter for NullUiAdapter {
    async fn display(&self, _message: &DisplayMessage) -> UiResult<()> {
        Ok(())
    }

    async fn display_progress(&self, _message: &str, _percent: Option<u8>) -> UiResult<()> {
        Ok(())
    }

    async fn read_input(&self, _prompt: &str) -> UiResult<String> {
        Ok(String::new())
    }

    async fn confirm(&self, _message: &str) -> UiResult<bool> {
        Ok(true)
    }

    async fn choose(&self, _prompt: &str, _choices: &[String]) -> UiResult<UserChoice> {
        Ok(UserChoice::Confirm)
    }

    fn supports_input(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- DisplayMessage --------------------------------------------------------

    #[test]
    fn test_display_message_info() {
        let msg = DisplayMessage::info("hello");
        assert_eq!(msg.content, "hello");
        assert_eq!(msg.severity, MessageSeverity::Info);
        assert!(msg.title.is_none());
    }

    #[test]
    fn test_display_message_warning() {
        let msg = DisplayMessage::warning("careful");
        assert_eq!(msg.severity, MessageSeverity::Warning);
    }

    #[test]
    fn test_display_message_error() {
        let msg = DisplayMessage::error("broken");
        assert_eq!(msg.severity, MessageSeverity::Error);
    }

    #[test]
    fn test_display_message_with_title() {
        let msg = DisplayMessage::info("details").with_title("Heading");
        assert_eq!(msg.title.as_deref(), Some("Heading"));
    }

    // -- MessageSeverity -------------------------------------------------------

    #[test]
    fn test_message_severity_default() {
        assert_eq!(MessageSeverity::default(), MessageSeverity::Info);
    }

    #[test]
    fn test_message_severity_display() {
        assert_eq!(MessageSeverity::Info.to_string(), "info");
        assert_eq!(MessageSeverity::Warning.to_string(), "warning");
        assert_eq!(MessageSeverity::Error.to_string(), "error");
    }

    // -- UserChoice ------------------------------------------------------------

    #[test]
    fn test_user_choice_equality() {
        assert_eq!(UserChoice::Confirm, UserChoice::Confirm);
        assert_eq!(UserChoice::Cancel, UserChoice::Cancel);
        assert_eq!(UserChoice::Other("a".into()), UserChoice::Other("a".into()));
        assert_ne!(UserChoice::Confirm, UserChoice::Cancel);
    }

    // -- NullUiAdapter ---------------------------------------------------------

    #[tokio::test]
    async fn test_null_adapter_display() {
        let adapter = NullUiAdapter;
        assert!(adapter
            .display(&DisplayMessage::info("test"))
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn test_null_adapter_progress() {
        let adapter = NullUiAdapter;
        assert!(adapter.display_progress("loading", Some(50)).await.is_ok());
        assert!(adapter.display_progress("loading", None).await.is_ok());
    }

    #[tokio::test]
    async fn test_null_adapter_read_input() {
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

    #[tokio::test]
    async fn test_null_adapter_choose() {
        let adapter = NullUiAdapter;
        let result = adapter
            .choose("Pick one", &["a".to_string(), "b".to_string()])
            .await;
        assert!(matches!(result, Ok(UserChoice::Confirm)));
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

    // -- DefaultUiAdapter (non-interactive methods) ----------------------------

    #[tokio::test]
    async fn test_default_adapter_display() {
        let adapter = DefaultUiAdapter;
        let msg = DisplayMessage::info("hello");
        assert!(adapter.display(&msg).await.is_ok());
    }

    #[tokio::test]
    async fn test_default_adapter_display_with_title() {
        let adapter = DefaultUiAdapter;
        let msg = DisplayMessage::warning("disk full").with_title("Warning");
        assert!(adapter.display(&msg).await.is_ok());
    }

    #[tokio::test]
    async fn test_default_adapter_display_error_severity() {
        let adapter = DefaultUiAdapter;
        let msg = DisplayMessage::error("something broke");
        assert!(adapter.display(&msg).await.is_ok());
    }

    #[tokio::test]
    async fn test_default_adapter_progress() {
        let adapter = DefaultUiAdapter;
        assert!(adapter.display_progress("working", Some(42)).await.is_ok());
        assert!(adapter.display_progress("working", None).await.is_ok());
    }

    #[test]
    fn test_default_adapter_no_streaming() {
        let adapter = DefaultUiAdapter;
        assert!(!adapter.supports_streaming());
    }

    // -- UiError ---------------------------------------------------------------

    #[test]
    fn test_ui_error_from_io() {
        let err = UiError::Io(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe"));
        assert!(err.to_string().contains("IO error"));
    }

    #[test]
    fn test_ui_error_render() {
        let err = UiError::Render("bad frame".to_string());
        assert!(err.to_string().contains("Rendering error"));
    }

    #[test]
    fn test_ui_error_input() {
        let err = UiError::Input("no tty".to_string());
        assert!(err.to_string().contains("Input error"));
    }

    #[test]
    fn test_ui_error_not_supported() {
        let err = UiError::NotSupported("choose".to_string());
        assert!(err.to_string().contains("Not supported"));
    }
}
