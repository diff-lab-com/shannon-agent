//! REPL main loop and terminal management

use crate::{events::EventHandler, render::Renderer, Result};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    Terminal,
};
use std::io;

/// Main REPL application struct
pub struct Repl {
    /// Event handler for user input
    events: EventHandler,
    /// Renderer for UI drawing
    renderer: Renderer,
    /// Running state
    running: bool,
}

impl Repl {
    /// Create a new REPL instance
    pub fn new() -> Result<Self> {
        Ok(Self {
            events: EventHandler::new(250)?,
            renderer: Renderer::new(),
            running: false,
        })
    }

    /// Run the main REPL loop
    pub fn run(&mut self) -> Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        self.running = true;

        // Main event loop
        while self.running {
            // Draw UI
            terminal.draw(|f| {
                if let Err(e) = self.renderer.render(f) {
                    eprintln!("Render error: {}", e);
                }
            })?;

            // Handle events
            if let Some(event) = self.events.next()? {
                self.handle_event(event);
            }
        }

        // Restore terminal
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        Ok(())
    }

    /// Handle individual events
    fn handle_event(&mut self, event: crate::events::Event) {
        match event {
            crate::events::Event::Input(key) => {
                if let Err(e) = self.handle_input(key) {
                    eprintln!("Input error: {}", e);
                }
            }
            crate::events::Event::Tick => {
                // Handle periodic updates
            }
        }
    }

    /// Handle keyboard input
    fn handle_input(&mut self, key: crossterm::event::KeyEvent) -> Result<()> {
        match key.code {
            crossterm::event::KeyCode::Char('q') => self.running = false,
            crossterm::event::KeyCode::Char('c') => {
                if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) {
                    self.running = false;
                }
            }
            _ => {}
        }
        Ok(())
    }
}
