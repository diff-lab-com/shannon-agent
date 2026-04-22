//! Shannon Terminal UI
//!
//! Terminal-based user interface for Shannon using Ratatui.

// Initialize i18n translations — path is relative to this crate's src/ dir
rust_i18n::i18n!("../../locales", fallback = "en");

pub mod repl;
pub mod theme;
pub mod keybindings;
mod widgets;
mod events;
mod render;
pub mod adapter;
pub mod vim;
pub mod repl_enhancement;
pub mod skill_bridge;
pub mod tool_format;
pub mod terminal_image;
pub mod screenshot;

pub use repl::{Repl, ReplState};
pub use events::{Event, EventHandler};
pub use render::Renderer;
pub use render::render_diff;
pub use widgets::{ChatWidget, ChatRole, ChatMessage, PromptWidget, MainLayoutWidget, HeaderWidget, StatusBarWidget, SidebarInfo};
pub use theme::Theme;
pub use terminal_image::{
    ImageProtocol, ImageRenderConfig,
    detect_protocol, render_image_base64, render_image_bytes, image_placeholder,
};
pub use vim::{VimHandler, VimMode, VimAction};
pub use adapter::{
    UiAdapter, UiResult, UiError, NullUiAdapter,
    DefaultUiAdapter, DisplayMessage, MessageSeverity, UserChoice,
};

/// Terminal UI application result type
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Main UI application entry point
pub fn run() -> Result<()> {
    let mut repl = Repl::new()?;
    repl.run()
}
