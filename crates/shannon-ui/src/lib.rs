// Suppress lints that conflict with rustfmt or are style preferences from newer clippy.
#![allow(
    clippy::collapsible_if,
    clippy::collapsible_match,
    clippy::derivable_impls
)]

//! Shannon Terminal UI
//!
//! Terminal-based user interface for Shannon Code using Ratatui.
//!
//! ## Components
//! - **REPL**: Interactive read-eval-print loop with command history and search
//! - **Widgets**: Markdown rendering, diff viewer, token counter, context bar, agent dashboard
//! - **Rendering**: Virtual scroll, progress indicators, syntax highlighting
//! - **Vim mode**: Modal editing support in the input area
//!
//! ## Key modules
//! - `app` — Application state and event handling
//! - `repl` — REPL logic, input processing, command dispatch
//! - `widgets` — UI components (chat, diff, status bar, etc.)
//! - `render` — Frame rendering and layout

// Initialize i18n translations — path is relative to this crate's src/ dir
rust_i18n::i18n!("../../locales", fallback = "en");

pub mod a11y;
pub mod adapter;
pub mod ansi_render;
mod events;
pub mod keybindings;
pub mod lsp_bridge;
pub mod markdown_table;
mod render;
pub mod repl;
pub mod repl_enhancement;
pub mod screenshot;
pub mod skill_bridge;
pub mod stream_buffer;
pub mod stream_render;
pub mod streaming_diff;
pub mod terminal_image;
pub mod theme;
pub mod tool_format;
pub mod tui;
pub mod vim;
pub mod voice;
mod widgets;

pub use adapter::{
    DefaultUiAdapter, DisplayMessage, MessageSeverity, NullUiAdapter, UiAdapter, UiError, UiResult,
    UserChoice,
};
pub use events::{Event, EventHandler};
pub use render::Renderer;
pub use render::render_diff;
pub use repl::{Repl, ReplState};
pub use terminal_image::{
    ImageProtocol, ImageRenderConfig, detect_protocol, image_placeholder, render_image_base64,
    render_image_bytes,
};
pub use theme::Theme;
pub use vim::{VimAction, VimHandler, VimMode};
pub use widgets::{
    ChatMessage, ChatRole, ChatWidget, HeaderWidget, MainLayoutWidget, PromptWidget, SidebarInfo,
    StatusBarWidget,
};

/// Terminal UI application result type
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Main UI application entry point
pub fn run() -> Result<()> {
    let mut repl = Repl::new()?;
    repl.run()
}
