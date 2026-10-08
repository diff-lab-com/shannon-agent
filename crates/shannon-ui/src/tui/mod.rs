//! Terminal lifecycle helpers shared with the REPL.
//!
//! The inline-viewport `Tui` wrapper this module once carried was dead code
//! — the REPL manages its own terminal. What was worth adopting is the
//! external-program suspend/restore pair it documented: a full-screen child
//! (editor, pager) must get the terminal back in its normal state, without
//! raw mode, bracketed paste, or mouse capture leaking into it — otherwise
//! mouse clicks inside the editor arrive as garbage escape sequences.

use std::io;

use crossterm::{execute, terminal::EnableLineWrap};

/// Suspend terminal interactivity so an external full-screen program can own
/// the terminal: raw mode off, bracketed paste and mouse capture disabled,
/// line wrap restored. Pairs with [`restore_terminal_after_external`].
pub fn restore_terminal_for_external() -> io::Result<()> {
    crossterm::terminal::disable_raw_mode()?;
    execute!(
        io::stdout(),
        crossterm::event::DisableBracketedPaste,
        crossterm::event::DisableMouseCapture,
        EnableLineWrap,
    )?;
    Ok(())
}

/// Re-arm the REPL's terminal after an external program exits: raw mode on,
/// line wrap, bracketed paste, and mouse capture restored. Pairs with
/// [`restore_terminal_for_external`].
pub fn restore_terminal_after_external() -> io::Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    execute!(
        io::stdout(),
        EnableLineWrap,
        crossterm::event::EnableBracketedPaste,
        crossterm::event::EnableMouseCapture,
    )?;
    Ok(())
}
