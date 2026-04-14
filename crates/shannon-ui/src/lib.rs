//! Shannon Terminal UI
//!
//! Terminal-based user interface for Shannon using Ratatui.

pub mod repl;
mod widgets;
mod events;
mod render;
pub mod adapter;
pub mod vim;
pub mod repl_enhancement;
pub mod skill_bridge;
pub mod tool_format;

pub use repl::{Repl, ReplState};
pub use events::{Event, EventHandler};
pub use render::Renderer;
pub use render::render_diff;
pub use widgets::{ChatWidget, ChatRole, PromptWidget, MainLayoutWidget};
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
