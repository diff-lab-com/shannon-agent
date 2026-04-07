//! Event handling for terminal UI

use crossterm::event::{self, Event as CrosstermEvent, KeyEvent};
use std::io;
use std::time::Duration;

/// Application event types
pub enum Event {
    /// Keyboard input event
    Input(KeyEvent),
    /// Tick event for periodic updates
    Tick,
}

/// Event handler for terminal events
pub struct EventHandler {
    /// Tick rate in milliseconds
    tick_rate: Duration,
}

impl EventHandler {
    /// Create a new event handler
    pub fn new(tick_rate_ms: u64) -> io::Result<Self> {
        Ok(Self {
            tick_rate: Duration::from_millis(tick_rate_ms),
        })
    }

    /// Get the next event with timeout
    pub fn next(&mut self) -> io::Result<Option<Event>> {
        // Poll for event with timeout
        if event::poll(self.tick_rate)? {
            // Read and properly handle all event types to prevent escape sequence leakage
            match event::read()? {
                CrosstermEvent::Key(key) => return Ok(Some(Event::Input(key))),
                // Consume and ignore mouse events to prevent escape sequences from appearing
                CrosstermEvent::Mouse(_) => {}
                // Ignore other event types (resize, focus, paste)
                _ => {}
            }
        }
        Ok(Some(Event::Tick))
    }
}
