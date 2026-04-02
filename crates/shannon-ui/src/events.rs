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
            if let CrosstermEvent::Key(key) = event::read()? {
                return Ok(Some(Event::Input(key)));
            }
        }
        Ok(Some(Event::Tick))
    }
}
