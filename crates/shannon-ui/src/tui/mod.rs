//! Custom TUI wrapper for inline viewport with history injection.
//!
//! Architecture (based on Codex CLI):
//! - Inline viewport at the bottom of the terminal for status bar + prompt + active content
//! - Chat messages committed to terminal scrollback via `insert_before()`
//! - Overlays (pager, model picker, diff viewer) use alternate screen

use std::io::{self, Write as IoWrite};

use crossterm::{
    event::{DisableBracketedPaste, EnableBracketedPaste, DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{
        EnableLineWrap, EnterAlternateScreen,
        LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    },
};
use ratatui::{
    backend::CrosstermBackend,
    layout::Rect,
    prelude::Widget,
    widgets::Paragraph,
    Frame, Terminal, TerminalOptions, Viewport,
};

/// Custom TUI wrapper that manages the terminal lifecycle.
pub struct Tui {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
    pending_history: Vec<ratatui::text::Line<'static>>,
    alt_screen_active: bool,
}

impl Tui {
    /// Initialize with an inline viewport at the bottom of the screen.
    ///
    /// `viewport_height` leaves room above for history injection.
    /// Recommended: `screen_height - 2` to avoid the "borrow top line" code path.
    pub fn init(viewport_height: u16) -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnableLineWrap, EnableBracketedPaste, EnableMouseCapture)?;

        let (_w, h) = crossterm::terminal::size().unwrap_or((80, 24));
        let height = viewport_height.min(h.saturating_sub(2)).max(5);

        // Print newlines to push cursor down so viewport sits at bottom of screen.
        let start_row = h.saturating_sub(height);
        for _ in 0..start_row {
            stdout.write_all(b"\n")?;
        }
        stdout.flush()?;

        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(height),
            },
        )?;
        terminal.clear()?;

        Ok(Self {
            terminal,
            pending_history: Vec::new(),
            alt_screen_active: false,
        })
    }

    /// Initialize with full alternate screen (original ratatui fullscreen mode).
    pub fn init_alt_screen() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(
            stdout,
            EnableLineWrap,
            EnterAlternateScreen,
            EnableBracketedPaste,
            EnableMouseCapture
        )?;

        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;

        Ok(Self {
            terminal,
            pending_history: Vec::new(),
            alt_screen_active: false,
        })
    }

    /// Mutable reference to the inner terminal.
    pub fn terminal_mut(&mut self) -> &mut Terminal<CrosstermBackend<io::Stdout>> {
        &mut self.terminal
    }

    /// Get the viewport area (drawable region).
    pub fn viewport_area(&self) -> Rect {
        let s = self.terminal.size().unwrap_or_else(|_| ratatui::layout::Size::new(80, 24));
        Rect::new(0, 0, s.width, s.height)
    }

    /// Get screen size as (width, height).
    pub fn screen_size(&self) -> (u16, u16) {
        crossterm::terminal::size().unwrap_or((80, 24))
    }

    /// Buffer history lines for injection. Flushed on next `draw()`.
    pub fn insert_history_lines(&mut self, lines: Vec<ratatui::text::Line<'static>>) {
        if !lines.is_empty() {
            self.pending_history.extend(lines);
        }
    }

    /// Flush pending history and draw the viewport.
    pub fn draw(&mut self, draw_fn: impl FnOnce(&mut Frame)) -> io::Result<()> {
        if !self.pending_history.is_empty() && !self.alt_screen_active {
            let lines = std::mem::take(&mut self.pending_history);
            let height = lines.len() as u16;
            self.terminal.insert_before(height, |buf| {
                let paragraph = Paragraph::new(lines);
                paragraph.render(buf.area, buf);
            })?;
        }
        self.terminal.draw(draw_fn)?;
        Ok(())
    }

    /// Enter alternate screen for overlay rendering.
    pub fn enter_alt_screen(&mut self) -> io::Result<()> {
        if self.alt_screen_active {
            return Ok(());
        }
        execute!(self.terminal.backend_mut(), EnterAlternateScreen)?;
        self.terminal.clear()?;
        self.alt_screen_active = true;
        Ok(())
    }

    /// Leave alternate screen, restoring the inline viewport.
    pub fn leave_alt_screen(&mut self) -> io::Result<()> {
        if !self.alt_screen_active {
            return Ok(());
        }
        execute!(self.terminal.backend_mut(), LeaveAlternateScreen)?;
        self.terminal.clear()?;
        self.alt_screen_active = false;
        Ok(())
    }

    /// Whether alternate screen is currently active.
    pub fn is_alt_screen(&self) -> bool {
        self.alt_screen_active
    }

    /// Restore terminal for external program execution.
    pub fn restore_for_external(&self) -> io::Result<()> {
        disable_raw_mode()?;
        execute!(
            io::stdout(),
            DisableBracketedPaste,
            DisableMouseCapture,
            EnableLineWrap,
        )?;
        Ok(())
    }

    /// Re-initialize after external program execution.
    pub fn restore_after_external(&self) -> io::Result<()> {
        enable_raw_mode()?;
        execute!(
            io::stdout(),
            EnableLineWrap,
            EnableBracketedPaste,
            EnableMouseCapture,
        )?;
        Ok(())
    }

    /// Shut down the terminal cleanly.
    pub fn shutdown(&mut self) -> io::Result<()> {
        if self.alt_screen_active {
            execute!(self.terminal.backend_mut(), LeaveAlternateScreen)?;
            self.alt_screen_active = false;
        }
        disable_raw_mode()?;
        execute!(
            self.terminal.backend_mut(),
            DisableBracketedPaste,
            DisableMouseCapture,
            EnableLineWrap,
            crossterm::cursor::Show,
        )?;
        Ok(())
    }
}
