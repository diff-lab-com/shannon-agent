//! Custom TUI wrapper for inline viewport with history injection.
//!
//! Architecture (based on Codex CLI):
//! - Inline viewport at the bottom of the terminal for status bar + prompt + active content
//! - Chat messages committed to terminal scrollback via `insert_before()`
//! - Overlays (pager, model picker, diff viewer) use alternate screen

use std::io::{self, Write as IoWrite};

use crossterm::{
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture},
    execute,
    terminal::{
        EnableLineWrap, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode,
    },
};
use ratatui::{
    Frame, Terminal, TerminalOptions, Viewport, backend::CrosstermBackend, layout::Rect,
    prelude::Widget, widgets::Paragraph,
};

/// Detected terminal multiplexer environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Multiplexer {
    None,
    Tmux,
    Zellij,
    Screen,
}

impl Multiplexer {
    fn detect() -> Self {
        if std::env::var("TMUX").is_ok() {
            return Self::Tmux;
        }
        if std::env::var("ZELLIJ").is_ok() || std::env::var("ZELLIJ_SESSION_NAME").is_ok() {
            return Self::Zellij;
        }
        if std::env::var("TERM").as_deref() == Ok("screen") || std::env::var("STY").is_ok() {
            return Self::Screen;
        }
        Self::None
    }

    /// Whether the terminal likely supports OSC 52 clipboard.
    pub fn supports_osc52(self) -> bool {
        matches!(self, Self::Tmux | Self::Zellij)
    }

    /// Whether alternate screen is already managed by the multiplexer.
    pub fn manages_alt_screen(self) -> bool {
        matches!(self, Self::Tmux)
    }
}

/// Detected terminal environment capabilities.
#[derive(Debug, Clone, Copy)]
pub struct TerminalEnv {
    pub multiplexer: Multiplexer,
    pub truecolor: bool,
}

impl TerminalEnv {
    fn detect() -> Self {
        let multiplexer = Multiplexer::detect();
        let colorterm = std::env::var("COLORTERM").unwrap_or_default();
        let truecolor = colorterm == "truecolor" || colorterm == "24bit";
        Self {
            multiplexer,
            truecolor,
        }
    }
}

/// Custom TUI wrapper that manages the terminal lifecycle.
pub struct Tui {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
    pending_history: Vec<ratatui::text::Line<'static>>,
    alt_screen_active: bool,
    env: TerminalEnv,
}

impl Tui {
    /// Initialize with an inline viewport at the bottom of the screen.
    ///
    /// `viewport_height` leaves room above for history injection.
    /// Recommended: `screen_height - 2` to avoid the "borrow top line" code path.
    pub fn init(viewport_height: u16) -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(
            stdout,
            EnableLineWrap,
            EnableBracketedPaste,
            EnableMouseCapture
        )?;

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
            env: TerminalEnv::detect(),
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
            env: TerminalEnv::detect(),
        })
    }

    /// Mutable reference to the inner terminal.
    pub fn terminal_mut(&mut self) -> &mut Terminal<CrosstermBackend<io::Stdout>> {
        &mut self.terminal
    }

    /// Get the viewport area (drawable region).
    pub fn viewport_area(&self) -> Rect {
        let s = self
            .terminal
            .size()
            .unwrap_or_else(|_| ratatui::layout::Size::new(80, 24));
        Rect::new(0, 0, s.width, s.height)
    }

    /// Detected terminal environment (multiplexer, color capabilities).
    pub fn env(&self) -> &TerminalEnv {
        &self.env
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

    /// Write text to clipboard via OSC 52 escape sequence.
    /// Only works if the terminal/multiplexer supports it.
    pub fn set_clipboard_osc52(&mut self, text: &str) -> io::Result<()> {
        if !self.env.multiplexer.supports_osc52() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "OSC 52 not supported",
            ));
        }
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
        // OSC 52: c=clipboard, p=primary
        let seq = format!("\x1b]52;c;{encoded}\x07");
        self.terminal.backend_mut().write_all(seq.as_bytes())?;
        self.terminal.backend_mut().flush()?;
        Ok(())
    }

    /// Clear only from cursor to end of screen (cheaper than full clear).
    pub fn clear_to_end(&mut self) -> io::Result<()> {
        execute!(
            self.terminal.backend_mut(),
            crossterm::terminal::Clear(crossterm::terminal::ClearType::FromCursorDown)
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
