//! Rendering logic for terminal UI

use crate::widgets;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    Frame,
};

/// Main renderer for the UI
pub struct Renderer {
    /// Current status message
    status_message: String,
}

impl Renderer {
    /// Create a new renderer
    pub fn new() -> Self {
        Self {
            status_message: "Ready".to_string(),
        }
    }

    /// Render the UI
    pub fn render(&mut self, frame: &mut Frame) -> Result<()> {
        let size = frame.area();

        // Create layout chunks
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints(
                [
                    Constraint::Min(0),   // Main content
                    Constraint::Length(3), // Status bar
                ]
                .as_ref(),
            )
            .split(size);

        // Render main content area
        self.render_main_content(frame, chunks[0]);

        // Render status bar
        widgets::StatusBarWidget::render(frame, chunks[1], &self.status_message);

        Ok(())
    }

    /// Render the main content area
    fn render_main_content(&self, frame: &mut Frame, area: Rect) {
        widgets::WelcomeWidget::render(frame, area);
    }

    /// Update the status message
    pub fn set_status(&mut self, message: impl Into<String>) {
        self.status_message = message.into();
    }
}

/// Terminal UI result type
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
