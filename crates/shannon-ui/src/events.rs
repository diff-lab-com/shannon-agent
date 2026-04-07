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
        // Drain ALL pending events to prevent queue buildup and escape sequence leakage
        // This is critical because mouse events can accumulate rapidly
        loop {
            if event::poll(self.tick_rate)? {
                match event::read()? {
                    CrosstermEvent::Key(key) => return Ok(Some(Event::Input(key))),
                    // Consume ALL mouse events, not just one per tick
                    CrosstermEvent::Mouse(_) => {
                        // Continue draining without returning
                        continue;
                    }
                    // Ignore other event types (resize, focus, paste)
                    _ => {
                        // Continue draining to prevent buildup
                        continue;
                    }
                }
            } else {
                // No more events pending
                break;
            }
        }
        Ok(Some(Event::Tick))
    }
}
