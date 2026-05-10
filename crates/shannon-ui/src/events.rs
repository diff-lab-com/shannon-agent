//! Event handling for terminal UI

use crossterm::event::{self, Event as CrosstermEvent, KeyEvent, MouseEvent};
use std::io;
use std::time::Duration;

/// Application event types
pub enum Event {
    /// Keyboard input event
    Input(KeyEvent),
    /// Bracketed paste event (multi-line text pasted from terminal)
    Paste(String),
    /// Mouse event (scroll wheel, click, etc.)
    Mouse(MouseEvent),
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
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> io::Result<Option<Event>> {
        // Drain pending events to prevent queue buildup and escape sequence leakage.
        // Cap iterations to prevent starving tick events when events arrive faster
        // than they can be processed.
        const MAX_EVENTS_PER_TICK: usize = 64;
        let mut drained = 0;
        loop {
            if drained >= MAX_EVENTS_PER_TICK {
                break;
            }
            if event::poll(self.tick_rate)? {
                match event::read()? {
                    CrosstermEvent::Key(key) => return Ok(Some(Event::Input(key))),
                    CrosstermEvent::Paste(content) => return Ok(Some(Event::Paste(content))),
                    CrosstermEvent::Mouse(mouse) => return Ok(Some(Event::Mouse(mouse))),
                    // Ignore other event types (resize, focus)
                    _ => {
                        drained += 1;
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
