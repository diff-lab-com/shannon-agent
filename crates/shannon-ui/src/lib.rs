//! Shannon Terminal UI
//!
//! Terminal-based user interface for Shannon using Ratatui.

mod repl;
mod widgets;
mod events;
mod render;

pub use repl::{Repl, ReplState};
pub use events::{Event, EventHandler};
pub use render::Renderer;
pub use render::render_diff;
pub use widgets::{ChatWidget, ChatRole, PromptWidget, MainLayoutWidget};

/// Terminal UI application result type
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Main UI application entry point
pub fn run() -> Result<()> {
    let mut repl = Repl::new()?;
    repl.run()
}
