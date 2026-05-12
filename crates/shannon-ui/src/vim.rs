//! Vim-like keybinding mode for Shannon TUI
//!
//! Implements Normal, Insert, Visual, and Command modes with
//! common vim keybindings for navigation, editing, and command execution.

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::{Deserialize, Serialize};

/// Vim editing mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VimMode {
    /// Normal mode: navigation and command entry
    Normal,
    /// Insert mode: text input
    Insert,
    /// Visual mode: text selection
    Visual,
    /// Command mode: : prefix entered, building a command string
    Command,
}

impl std::fmt::Display for VimMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VimMode::Normal => write!(f, "NORMAL"),
            VimMode::Insert => write!(f, "INSERT"),
            VimMode::Visual => write!(f, "VISUAL"),
            VimMode::Command => write!(f, "COMMAND"),
        }
    }
}

/// Direction for cursor movement
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
    LineStart,
    LineEnd,
    FileStart,
    FileEnd,
    WordForward,
    WordBackward,
}

/// Direction for scrolling
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDirection {
    Up,
    Down,
    HalfPageUp,
    HalfPageDown,
    FullPageUp,
    FullPageDown,
}

/// Resulting action from processing a key sequence
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VimAction {
    /// No action (key not recognized or still accumulating sequence)
    None,
    /// Insert a character at the cursor position
    InsertChar { c: char },
    /// Delete character before cursor (backspace)
    Backspace,
    /// Submit the current input (Enter in insert mode)
    SubmitInput,
    /// Move cursor in a direction
    MoveCursor { direction: Direction, count: usize },
    /// Delete the current line
    DeleteLine { count: usize },
    /// Delete character(s) under cursor (vim `x`)
    DeleteChar { count: usize },
    /// Yank (copy) the current line into buffer
    YankLine { count: usize },
    /// Paste buffer contents after cursor
    PasteAfter,
    /// Undo the last action
    Undo,
    /// Enter insert mode at cursor position
    EnterInsertMode,
    /// Enter insert mode after cursor (append)
    EnterInsertModeAppend,
    /// Enter insert mode on a new line below
    EnterInsertModeBelow,
    /// Enter insert mode on a new line above
    EnterInsertModeAbove,
    /// Enter visual mode (character-wise)
    EnterVisualMode,
    /// Enter visual line mode
    EnterVisualLineMode,
    /// Enter command mode (clearing any pending prefix)
    EnterCommandMode,
    /// Append a character to the command buffer (while in Command mode)
    CommandChar { c: char },
    /// Delete last character from command buffer
    CommandBackspace,
    /// Execute the command currently in the buffer
    ExecuteCommand { command: String },
    /// Scroll the view
    Scroll { direction: ScrollDirection, count: usize },
    /// No-op: the key was handled but produces no external action
    Noop,
    /// Quit the application
    Quit,
    /// Clear the current input line
    ClearInput,
    /// Search forward (enter search mode -- uses command buffer for pattern)
    SearchForward { pattern: String },
    /// Search backward
    SearchBackward { pattern: String },
    /// Delete word(s) forward
    DeleteWord { count: usize },
    /// Yank (copy) word(s) into buffer
    YankWord { count: usize },
    /// Set a mark at current position
    SetMark { mark: char },
    /// Jump to a mark position
    JumpToMark { mark: char },
}

/// Vim key handler state machine
///
/// Processes key events and produces [`VimAction`] values that the REPL
/// can interpret. Maintains internal state for mode, pending key sequences,
/// count prefixes, and the yank/delete buffer.
pub struct VimHandler {
    /// Current editing mode
    mode: VimMode,
    /// Pending key sequence accumulator (for multi-key commands like `gg`, `dd`)
    pending_keys: String,
    /// Numeric count prefix (e.g. `3` in `3j`)
    count_buffer: String,
    /// Yank buffer (clipboard)
    yank_buffer: String,
    /// Command buffer (for `:` commands and `/` search)
    command_buffer: String,
    /// Whether the handler is building a search pattern (`/` or `?` prefix)
    search_mode: bool,
    /// True = forward search (`/`), false = backward search (`?`)
    search_forward: bool,
    /// Named marks: lowercase letter → byte offset in prompt
    marks: HashMap<char, usize>,
    /// Waiting for mark destination key (after `m` or `'`)
    mark_pending: Option<char>,
}

impl VimHandler {
    /// Create a new vim handler starting in Normal mode
    pub fn new() -> Self {
        Self {
            mode: VimMode::Normal,
            pending_keys: String::new(),
            count_buffer: String::new(),
            yank_buffer: String::new(),
            command_buffer: String::new(),
            search_mode: false,
            search_forward: true,
            marks: HashMap::new(),
            mark_pending: None,
        }
    }

    /// Get the current vim mode
    pub fn mode(&self) -> VimMode {
        self.mode
    }

    /// Store text into the yank buffer (called by the REPL when handling YankLine/YankWord).
    pub fn set_yank_buffer(&mut self, text: String) {
        self.yank_buffer = text;
    }

    /// Retrieve text from the yank buffer (called by the REPL when handling PasteAfter).
    pub fn yank_buffer(&self) -> &str {
        &self.yank_buffer
    }

    /// Get the current command buffer contents (for display in status bar)
    pub fn command_buffer(&self) -> &str {
        &self.command_buffer
    }

    /// Get the current search mode flag
    pub fn is_search_mode(&self) -> bool {
        self.search_mode
    }

    /// Get the pending count as a parsed number (if valid)
    fn parsed_count(&self) -> usize {
        if self.count_buffer.is_empty() {
            1
        } else if let Ok(n) = self.count_buffer.parse::<usize>() {
            n.max(1)
        } else {
            1
        }
    }

    /// Set a named mark at the given position
    pub fn set_mark(&mut self, mark: char, pos: usize) {
        if mark.is_ascii_lowercase() {
            self.marks.insert(mark, pos);
        }
    }

    /// Get the position of a named mark
    pub fn get_mark(&self, mark: char) -> Option<&usize> {
        self.marks.get(&mark)
    }

    /// Reset all transient state (count, pending keys) but keep mode
    fn reset_transient(&mut self) {
        self.pending_keys.clear();
        self.count_buffer.clear();
    }

    /// Transition to a new mode, resetting transient state
    pub fn set_mode(&mut self, mode: VimMode) {
        self.mode = mode;
        self.pending_keys.clear();
        self.count_buffer.clear();
        self.mark_pending = None;
        if mode != VimMode::Command {
            self.command_buffer.clear();
            self.search_mode = false;
        }
    }

    /// Reset to normal mode
    pub fn reset(&mut self) {
        self.set_mode(VimMode::Normal);
    }

    /// Process a crossterm key event and return an action
    pub fn process_key(&mut self, key: KeyEvent) -> VimAction {
        match self.mode {
            VimMode::Normal => self.process_normal(key),
            VimMode::Insert => self.process_insert(key),
            VimMode::Visual => self.process_visual(key),
            VimMode::Command => self.process_command(key),
        }
    }

    /// Process a single character for insert mode (used when chars arrive
    /// outside of the crossterm event system)
    pub fn process_char(&mut self, c: char) -> VimAction {
        if self.mode == VimMode::Insert {
            VimAction::InsertChar { c }
        } else {
            VimAction::None
        }
    }

    // ── Normal mode ─────────────────────────────────────────────

    fn process_normal(&mut self, key: KeyEvent) -> VimAction {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        match key.code {
            // Escape: clear pending state
            KeyCode::Esc => {
                self.reset_transient();
                VimAction::Noop
            }

            // Control sequences
            KeyCode::Char('d') if ctrl => {
                let count = self.parsed_count();
                self.reset_transient();
                VimAction::Scroll {
                    direction: ScrollDirection::HalfPageDown,
                    count,
                }
            }
            KeyCode::Char('u') if ctrl => {
                let count = self.parsed_count();
                self.reset_transient();
                VimAction::Scroll {
                    direction: ScrollDirection::HalfPageUp,
                    count,
                }
            }
            KeyCode::Char('f') if ctrl => {
                let count = self.parsed_count();
                self.reset_transient();
                VimAction::Scroll {
                    direction: ScrollDirection::FullPageDown,
                    count,
                }
            }
            KeyCode::Char('b') if ctrl => {
                let count = self.parsed_count();
                self.reset_transient();
                VimAction::Scroll {
                    direction: ScrollDirection::FullPageUp,
                    count,
                }
            }

            // Character keys
            KeyCode::Char(c) => {
                if c.is_ascii_digit() && (c != '0' || !self.count_buffer.is_empty()) {
                    // Accumulate count prefix
                    self.count_buffer.push(c);
                    self.pending_keys.clear();
                    return VimAction::None;
                }

                // Handle mark pending state (after 'm' or '\'')
                if let Some(prefix) = self.mark_pending.take() {
                    if c.is_ascii_lowercase() {
                        self.reset_transient();
                        return match prefix {
                            'm' => VimAction::SetMark { mark: c },
                            '\'' => VimAction::JumpToMark { mark: c },
                            _ => VimAction::None,
                        };
                    }
                    // Invalid mark key, cancel and fall through
                    self.reset_transient();
                }

                // Push to pending sequence
                self.pending_keys.push(c);

                // Check for complete sequences
                let seq = self.pending_keys.as_str();
                let count = self.parsed_count();

                // Two-char sequences
                if seq.len() == 2 {
                    match seq {
                        "dd" => {
                            let action = VimAction::DeleteLine { count };
                            self.reset_transient();
                            return action;
                        }
                        "yy" => {
                            let action = VimAction::YankLine { count };
                            self.reset_transient();
                            return action;
                        }
                        "gg" => {
                            let action = VimAction::MoveCursor {
                                direction: Direction::FileStart,
                                count,
                            };
                            self.reset_transient();
                            return action;
                        }
                        "dw" => {
                            let action = VimAction::DeleteWord { count };
                            self.reset_transient();
                            return action;
                        }
                        "yw" => {
                            let action = VimAction::YankWord { count };
                            self.reset_transient();
                            return action;
                        }
                        _ => {
                            // Unknown 2-char sequence, reset
                            self.reset_transient();
                            return VimAction::None;
                        }
                    }
                }

                // Single-char (or completed) sequences
                // Only match single-char commands when no second char could extend them
                if seq.len() == 1 {
                    match c {
                        'h' => {
                            let action = VimAction::MoveCursor {
                                direction: Direction::Left,
                                count,
                            };
                            self.reset_transient();
                            return action;
                        }
                        'j' => {
                            let action = VimAction::MoveCursor {
                                direction: Direction::Down,
                                count,
                            };
                            self.reset_transient();
                            return action;
                        }
                        'k' => {
                            let action = VimAction::MoveCursor {
                                direction: Direction::Up,
                                count,
                            };
                            self.reset_transient();
                            return action;
                        }
                        'l' => {
                            let action = VimAction::MoveCursor {
                                direction: Direction::Right,
                                count,
                            };
                            self.reset_transient();
                            return action;
                        }
                        'w' => {
                            let action = VimAction::MoveCursor {
                                direction: Direction::WordForward,
                                count,
                            };
                            self.reset_transient();
                            return action;
                        }
                        'b' => {
                            let action = VimAction::MoveCursor {
                                direction: Direction::WordBackward,
                                count,
                            };
                            self.reset_transient();
                            return action;
                        }
                        '0' => {
                            let action = VimAction::MoveCursor {
                                direction: Direction::LineStart,
                                count,
                            };
                            self.reset_transient();
                            return action;
                        }
                        '$' => {
                            let action = VimAction::MoveCursor {
                                direction: Direction::LineEnd,
                                count,
                            };
                            self.reset_transient();
                            return action;
                        }
                        'G' => {
                            let action = VimAction::MoveCursor {
                                direction: Direction::FileEnd,
                                count,
                            };
                            self.reset_transient();
                            return action;
                        }
                        'p' => {
                            self.reset_transient();
                            return VimAction::PasteAfter;
                        }
                        'u' => {
                            self.reset_transient();
                            return VimAction::Undo;
                        }
                        'i' => {
                            self.set_mode(VimMode::Insert);
                            return VimAction::EnterInsertMode;
                        }
                        'a' => {
                            self.set_mode(VimMode::Insert);
                            return VimAction::EnterInsertModeAppend;
                        }
                        'o' => {
                            self.set_mode(VimMode::Insert);
                            return VimAction::EnterInsertModeBelow;
                        }
                        'O' => {
                            self.set_mode(VimMode::Insert);
                            return VimAction::EnterInsertModeAbove;
                        }
                        'v' => {
                            self.set_mode(VimMode::Visual);
                            return VimAction::EnterVisualMode;
                        }
                        'V' => {
                            self.set_mode(VimMode::Visual);
                            return VimAction::EnterVisualLineMode;
                        }
                        'x' => {
                            self.reset_transient();
                            return VimAction::DeleteChar { count };
                        }
                        '/' => {
                            self.command_buffer.clear();
                            self.set_mode(VimMode::Command);
                            self.search_mode = true;
                            self.search_forward = true;
                            return VimAction::Noop;
                        }
                        '?' => {
                            self.command_buffer.clear();
                            self.set_mode(VimMode::Command);
                            self.search_mode = true;
                            self.search_forward = false;
                            return VimAction::Noop;
                        }
                        'm' => {
                            // Set mark: waiting for next lowercase letter
                            self.mark_pending = Some('m');
                            self.pending_keys.clear();
                            return VimAction::None;
                        }
                        '\'' => {
                            // Jump to mark: waiting for next lowercase letter
                            self.mark_pending = Some('\'');
                            self.pending_keys.clear();
                            return VimAction::None;
                        }
                        ':' => {
                            self.command_buffer.clear();
                            self.set_mode(VimMode::Command);
                            self.search_mode = false;
                            return VimAction::EnterCommandMode;
                        }
                        // Keys that start two-char sequences: wait for next key
                        'd' | 'y' | 'g' => {
                            // Don't reset -- keep pending_keys for the next keystroke
                            return VimAction::None;
                        }
                        _ => {
                            // Unknown key in normal mode
                            self.reset_transient();
                            return VimAction::None;
                        }
                    }
                }

                // If we get here with a 2-char pending that didn't match,
                // the first char was probably a motion and the second is new.
                if seq.len() >= 3 {
                    // Too long, reset
                    self.reset_transient();
                    return VimAction::None;
                }

                // Still accumulating
                VimAction::None
            }

            // Arrow keys in normal mode (treated like h/j/k/l)
            KeyCode::Left => {
                VimAction::MoveCursor {
                    direction: Direction::Left,
                    count: self.parsed_count(),
                }
            }
            KeyCode::Right => {
                VimAction::MoveCursor {
                    direction: Direction::Right,
                    count: self.parsed_count(),
                }
            }
            KeyCode::Up => {
                VimAction::MoveCursor {
                    direction: Direction::Up,
                    count: self.parsed_count(),
                }
            }
            KeyCode::Down => {
                VimAction::MoveCursor {
                    direction: Direction::Down,
                    count: self.parsed_count(),
                }
            }

            _ => VimAction::None,
        }
    }

    // ── Insert mode ─────────────────────────────────────────────

    fn process_insert(&mut self, key: KeyEvent) -> VimAction {
        match key.code {
            KeyCode::Esc => {
                self.set_mode(VimMode::Normal);
                VimAction::Noop
            }
            KeyCode::Char('[') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Ctrl-[ is equivalent to Escape
                self.set_mode(VimMode::Normal);
                VimAction::Noop
            }
            KeyCode::Enter => VimAction::SubmitInput,
            KeyCode::Backspace => VimAction::Backspace,
            KeyCode::Char(c) => VimAction::InsertChar { c },
            KeyCode::Left => VimAction::MoveCursor {
                direction: Direction::Left,
                count: 1,
            },
            KeyCode::Right => VimAction::MoveCursor {
                direction: Direction::Right,
                count: 1,
            },
            KeyCode::Home => VimAction::MoveCursor {
                direction: Direction::LineStart,
                count: 1,
            },
            KeyCode::End => VimAction::MoveCursor {
                direction: Direction::LineEnd,
                count: 1,
            },
            KeyCode::Delete => VimAction::DeleteChar { count: 1 },
            _ => VimAction::None,
        }
    }

    // ── Visual mode ─────────────────────────────────────────────

    fn process_visual(&mut self, key: KeyEvent) -> VimAction {
        match key.code {
            KeyCode::Esc => {
                self.set_mode(VimMode::Normal);
                VimAction::Noop
            }
            KeyCode::Char('h') | KeyCode::Left => VimAction::MoveCursor {
                direction: Direction::Left,
                count: 1,
            },
            KeyCode::Char('j') | KeyCode::Down => VimAction::MoveCursor {
                direction: Direction::Down,
                count: 1,
            },
            KeyCode::Char('k') | KeyCode::Up => VimAction::MoveCursor {
                direction: Direction::Up,
                count: 1,
            },
            KeyCode::Char('l') | KeyCode::Right => VimAction::MoveCursor {
                direction: Direction::Right,
                count: 1,
            },
            KeyCode::Char('w') => VimAction::MoveCursor {
                direction: Direction::WordForward,
                count: 1,
            },
            KeyCode::Char('b') => VimAction::MoveCursor {
                direction: Direction::WordBackward,
                count: 1,
            },
            KeyCode::Char('0') => VimAction::MoveCursor {
                direction: Direction::LineStart,
                count: 1,
            },
            KeyCode::Char('$') => VimAction::MoveCursor {
                direction: Direction::LineEnd,
                count: 1,
            },
            KeyCode::Char('G') => VimAction::MoveCursor {
                direction: Direction::FileEnd,
                count: 1,
            },
            KeyCode::Char('g') => {
                self.pending_keys.push('g');
                if self.pending_keys == "gg" {
                    self.reset_transient();
                    VimAction::MoveCursor {
                        direction: Direction::FileStart,
                        count: 1,
                    }
                } else {
                    VimAction::None
                }
            }
            KeyCode::Char('y') => {
                // Yank selection
                self.set_mode(VimMode::Normal);
                VimAction::YankLine { count: 1 }
            }
            KeyCode::Char('d') | KeyCode::Char('x') => {
                // Delete selection
                self.set_mode(VimMode::Normal);
                VimAction::DeleteLine { count: 1 }
            }
            KeyCode::Char('i') => {
                self.set_mode(VimMode::Insert);
                VimAction::EnterInsertMode
            }
            KeyCode::Char('a') => {
                self.set_mode(VimMode::Insert);
                VimAction::EnterInsertModeAppend
            }
            _ => VimAction::None,
        }
    }

    // ── Command mode ────────────────────────────────────────────

    fn process_command(&mut self, key: KeyEvent) -> VimAction {
        match key.code {
            KeyCode::Esc => {
                self.set_mode(VimMode::Normal);
                VimAction::Noop
            }
            KeyCode::Enter => {
                let cmd = self.command_buffer.clone();
                let is_search = self.search_mode;
                let forward = self.search_forward;
                self.set_mode(VimMode::Normal);

                if is_search {
                    return if forward {
                        VimAction::SearchForward { pattern: cmd }
                    } else {
                        VimAction::SearchBackward { pattern: cmd }
                    };
                }

                // Parse built-in commands
                let trimmed = cmd.trim();
                match trimmed {
                    "q" | "quit" => VimAction::Quit,
                    "w" | "write" => VimAction::Noop, // save not applicable in REPL
                    "wq" | "x" => VimAction::Quit,
                    _ => {
                        if let Some(rest) = trimmed.strip_prefix('!') {
                            // Shell command execution
                            VimAction::ExecuteCommand {
                                command: rest.to_string(),
                            }
                        } else {
                            VimAction::ExecuteCommand { command: cmd }
                        }
                    }
                }
            }
            KeyCode::Backspace => {
                if self.command_buffer.is_empty() {
                    // Backspace on empty command buffer exits command mode
                    self.set_mode(VimMode::Normal);
                    VimAction::Noop
                } else {
                    self.command_buffer.pop();
                    VimAction::CommandBackspace
                }
            }
            KeyCode::Char(c) => {
                self.command_buffer.push(c);
                VimAction::CommandChar { c }
            }
            _ => VimAction::None,
        }
    }
}

impl Default for VimHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Helper ──────────────────────────────────────────────────

    fn char_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn ctrl_char_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn esc_key() -> KeyEvent {
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
    }

    fn enter_key() -> KeyEvent {
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
    }

    fn backspace_key() -> KeyEvent {
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)
    }

    // ── Mode transitions ────────────────────────────────────────

    #[test]
    fn test_initial_mode_is_normal() {
        let handler = VimHandler::new();
        assert_eq!(handler.mode(), VimMode::Normal);
    }

    #[test]
    fn test_i_enters_insert_mode() {
        let mut handler = VimHandler::new();
        let action = handler.process_key(char_key('i'));
        assert_eq!(handler.mode(), VimMode::Insert);
        assert_eq!(action, VimAction::EnterInsertMode);
    }

    #[test]
    fn test_a_enters_insert_mode_append() {
        let mut handler = VimHandler::new();
        let action = handler.process_key(char_key('a'));
        assert_eq!(handler.mode(), VimMode::Insert);
        assert_eq!(action, VimAction::EnterInsertModeAppend);
    }

    #[test]
    fn test_o_enters_insert_mode_below() {
        let mut handler = VimHandler::new();
        let action = handler.process_key(char_key('o'));
        assert_eq!(handler.mode(), VimMode::Insert);
        assert_eq!(action, VimAction::EnterInsertModeBelow);
    }

    #[test]
    fn test_o_enters_insert_mode_above() {
        let mut handler = VimHandler::new();
        let action = handler.process_key(char_key('O'));
        assert_eq!(handler.mode(), VimMode::Insert);
        assert_eq!(action, VimAction::EnterInsertModeAbove);
    }

    #[test]
    fn test_esc_from_insert_returns_to_normal() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key('i'));
        assert_eq!(handler.mode(), VimMode::Insert);

        let action = handler.process_key(esc_key());
        assert_eq!(handler.mode(), VimMode::Normal);
        assert_eq!(action, VimAction::Noop);
    }

    #[test]
    fn test_ctrl_bracket_from_insert_returns_to_normal() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key('i'));
        assert_eq!(handler.mode(), VimMode::Insert);

        let action = handler.process_key(ctrl_char_key('['));
        assert_eq!(handler.mode(), VimMode::Normal);
        assert_eq!(action, VimAction::Noop);
    }

    #[test]
    fn test_v_enters_visual_mode() {
        let mut handler = VimHandler::new();
        let action = handler.process_key(char_key('v'));
        assert_eq!(handler.mode(), VimMode::Visual);
        assert_eq!(action, VimAction::EnterVisualMode);
    }

    #[test]
    fn test_v_enters_visual_line_mode() {
        let mut handler = VimHandler::new();
        let action = handler.process_key(char_key('V'));
        assert_eq!(handler.mode(), VimMode::Visual);
        assert_eq!(action, VimAction::EnterVisualLineMode);
    }

    #[test]
    fn test_colon_enters_command_mode() {
        let mut handler = VimHandler::new();
        let action = handler.process_key(char_key(':'));
        assert_eq!(handler.mode(), VimMode::Command);
        assert_eq!(action, VimAction::EnterCommandMode);
    }

    #[test]
    fn test_slash_enters_search_mode() {
        let mut handler = VimHandler::new();
        let action = handler.process_key(char_key('/'));
        assert_eq!(handler.mode(), VimMode::Command);
        assert!(handler.is_search_mode());
        assert_eq!(action, VimAction::Noop);
    }

    #[test]
    fn test_esc_from_visual_returns_to_normal() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key('v'));
        assert_eq!(handler.mode(), VimMode::Visual);

        handler.process_key(esc_key());
        assert_eq!(handler.mode(), VimMode::Normal);
    }

    #[test]
    fn test_esc_from_command_returns_to_normal() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key(':'));
        assert_eq!(handler.mode(), VimMode::Command);

        handler.process_key(esc_key());
        assert_eq!(handler.mode(), VimMode::Normal);
    }

    // ── Motion commands ─────────────────────────────────────────

    #[test]
    fn test_h_moves_left() {
        let mut handler = VimHandler::new();
        let action = handler.process_key(char_key('h'));
        assert_eq!(
            action,
            VimAction::MoveCursor {
                direction: Direction::Left,
                count: 1,
            }
        );
    }

    #[test]
    fn test_j_moves_down() {
        let mut handler = VimHandler::new();
        let action = handler.process_key(char_key('j'));
        assert_eq!(
            action,
            VimAction::MoveCursor {
                direction: Direction::Down,
                count: 1,
            }
        );
    }

    #[test]
    fn test_k_moves_up() {
        let mut handler = VimHandler::new();
        let action = handler.process_key(char_key('k'));
        assert_eq!(
            action,
            VimAction::MoveCursor {
                direction: Direction::Up,
                count: 1,
            }
        );
    }

    #[test]
    fn test_l_moves_right() {
        let mut handler = VimHandler::new();
        let action = handler.process_key(char_key('l'));
        assert_eq!(
            action,
            VimAction::MoveCursor {
                direction: Direction::Right,
                count: 1,
            }
        );
    }

    #[test]
    fn test_w_moves_word_forward() {
        let mut handler = VimHandler::new();
        let action = handler.process_key(char_key('w'));
        assert_eq!(
            action,
            VimAction::MoveCursor {
                direction: Direction::WordForward,
                count: 1,
            }
        );
    }

    #[test]
    fn test_b_moves_word_backward() {
        let mut handler = VimHandler::new();
        let action = handler.process_key(char_key('b'));
        assert_eq!(
            action,
            VimAction::MoveCursor {
                direction: Direction::WordBackward,
                count: 1,
            }
        );
    }

    #[test]
    fn test_zero_moves_line_start() {
        let mut handler = VimHandler::new();
        let action = handler.process_key(char_key('0'));
        assert_eq!(
            action,
            VimAction::MoveCursor {
                direction: Direction::LineStart,
                count: 1,
            }
        );
    }

    #[test]
    fn test_dollar_moves_line_end() {
        let mut handler = VimHandler::new();
        let action = handler.process_key(char_key('$'));
        assert_eq!(
            action,
            VimAction::MoveCursor {
                direction: Direction::LineEnd,
                count: 1,
            }
        );
    }

    #[test]
    fn test_gg_moves_file_start() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key('g')); // first g
        let action = handler.process_key(char_key('g')); // second g
        assert_eq!(
            action,
            VimAction::MoveCursor {
                direction: Direction::FileStart,
                count: 1,
            }
        );
    }

    #[test]
    fn test_g_moves_file_end() {
        let mut handler = VimHandler::new();
        let action = handler.process_key(char_key('G'));
        assert_eq!(
            action,
            VimAction::MoveCursor {
                direction: Direction::FileEnd,
                count: 1,
            }
        );
    }

    // ── Count prefix ────────────────────────────────────────────

    #[test]
    fn test_count_prefix_simple() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key('3'));
        let action = handler.process_key(char_key('j'));
        assert_eq!(
            action,
            VimAction::MoveCursor {
                direction: Direction::Down,
                count: 3,
            }
        );
    }

    #[test]
    fn test_count_prefix_dd() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key('5'));
        let action = handler.process_key(char_key('d'));
        // After first 'd', should still be accumulating
        assert_eq!(action, VimAction::None);
        let action = handler.process_key(char_key('d'));
        assert_eq!(action, VimAction::DeleteLine { count: 5 });
    }

    #[test]
    fn test_count_prefix_multidigit() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key('1'));
        handler.process_key(char_key('0'));
        let action = handler.process_key(char_key('j'));
        assert_eq!(
            action,
            VimAction::MoveCursor {
                direction: Direction::Down,
                count: 10,
            }
        );
    }

    #[test]
    fn test_zero_does_not_start_count() {
        let mut handler = VimHandler::new();
        let action = handler.process_key(char_key('0'));
        // '0' with empty count buffer is LineStart, not a count prefix
        assert_eq!(
            action,
            VimAction::MoveCursor {
                direction: Direction::LineStart,
                count: 1,
            }
        );
    }

    #[test]
    fn test_zero_after_digit_is_count() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key('1'));
        handler.process_key(char_key('0')); // should accumulate into count
        let action = handler.process_key(char_key('j'));
        assert_eq!(
            action,
            VimAction::MoveCursor {
                direction: Direction::Down,
                count: 10,
            }
        );
    }

    // ── Edit commands ───────────────────────────────────────────

    #[test]
    fn test_dd_deletes_line() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key('d'));
        let action = handler.process_key(char_key('d'));
        assert_eq!(action, VimAction::DeleteLine { count: 1 });
    }

    #[test]
    fn test_yy_yanks_line() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key('y'));
        let action = handler.process_key(char_key('y'));
        assert_eq!(action, VimAction::YankLine { count: 1 });
    }

    #[test]
    fn test_p_pastes() {
        let mut handler = VimHandler::new();
        let action = handler.process_key(char_key('p'));
        assert_eq!(action, VimAction::PasteAfter);
    }

    #[test]
    fn test_u_undos() {
        let mut handler = VimHandler::new();
        let action = handler.process_key(char_key('u'));
        assert_eq!(action, VimAction::Undo);
    }

    // ── Scroll commands ─────────────────────────────────────────

    #[test]
    fn test_ctrl_d_half_page_down() {
        let mut handler = VimHandler::new();
        let action = handler.process_key(ctrl_char_key('d'));
        assert_eq!(
            action,
            VimAction::Scroll {
                direction: ScrollDirection::HalfPageDown,
                count: 1,
            }
        );
    }

    #[test]
    fn test_ctrl_u_half_page_up() {
        let mut handler = VimHandler::new();
        let action = handler.process_key(ctrl_char_key('u'));
        assert_eq!(
            action,
            VimAction::Scroll {
                direction: ScrollDirection::HalfPageUp,
                count: 1,
            }
        );
    }

    #[test]
    fn test_ctrl_f_full_page_down() {
        let mut handler = VimHandler::new();
        let action = handler.process_key(ctrl_char_key('f'));
        assert_eq!(
            action,
            VimAction::Scroll {
                direction: ScrollDirection::FullPageDown,
                count: 1,
            }
        );
    }

    #[test]
    fn test_ctrl_b_full_page_up() {
        let mut handler = VimHandler::new();
        let action = handler.process_key(ctrl_char_key('b'));
        assert_eq!(
            action,
            VimAction::Scroll {
                direction: ScrollDirection::FullPageUp,
                count: 1,
            }
        );
    }

    #[test]
    fn test_ctrl_d_with_count() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key('3'));
        let action = handler.process_key(ctrl_char_key('d'));
        assert_eq!(
            action,
            VimAction::Scroll {
                direction: ScrollDirection::HalfPageDown,
                count: 3,
            }
        );
    }

    // ── Command mode parsing ────────────────────────────────────

    #[test]
    fn test_command_q_quits() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key(':'));
        handler.process_key(char_key('q'));
        let action = handler.process_key(enter_key());
        assert_eq!(action, VimAction::Quit);
    }

    #[test]
    fn test_command_quit_quits() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key(':'));
        handler.process_key(char_key('q'));
        handler.process_key(char_key('u'));
        handler.process_key(char_key('i'));
        handler.process_key(char_key('t'));
        let action = handler.process_key(enter_key());
        assert_eq!(action, VimAction::Quit);
    }

    #[test]
    fn test_command_wq_quits() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key(':'));
        handler.process_key(char_key('w'));
        handler.process_key(char_key('q'));
        let action = handler.process_key(enter_key());
        assert_eq!(action, VimAction::Quit);
    }

    #[test]
    fn test_command_w_noop() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key(':'));
        handler.process_key(char_key('w'));
        let action = handler.process_key(enter_key());
        assert_eq!(action, VimAction::Noop);
    }

    #[test]
    fn test_command_bang_executes() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key(':'));
        handler.process_key(char_key('!'));
        handler.process_key(char_key('l'));
        handler.process_key(char_key('s'));
        let action = handler.process_key(enter_key());
        assert_eq!(
            action,
            VimAction::ExecuteCommand {
                command: "ls".to_string(),
            }
        );
    }

    #[test]
    fn test_command_backspace_on_empty_exits() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key(':'));
        assert_eq!(handler.mode(), VimMode::Command);

        let action = handler.process_key(backspace_key());
        assert_eq!(handler.mode(), VimMode::Normal);
        assert_eq!(action, VimAction::Noop);
    }

    #[test]
    fn test_command_backspace_removes_char() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key(':'));
        handler.process_key(char_key('q'));
        handler.process_key(char_key('w'));
        assert_eq!(handler.command_buffer(), "qw");

        handler.process_key(backspace_key());
        assert_eq!(handler.command_buffer(), "q");
        assert_eq!(handler.mode(), VimMode::Command);
    }

    #[test]
    fn test_search_pattern() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key('/'));
        handler.process_key(char_key('f'));
        handler.process_key(char_key('o'));
        handler.process_key(char_key('o'));
        let action = handler.process_key(enter_key());
        assert_eq!(handler.mode(), VimMode::Normal);
        assert_eq!(
            action,
            VimAction::SearchForward {
                pattern: "foo".to_string(),
            }
        );
    }

    // ── Insert mode behavior ────────────────────────────────────

    #[test]
    fn test_insert_mode_char_input() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key('i'));
        let action = handler.process_key(char_key('h'));
        assert_eq!(action, VimAction::InsertChar { c: 'h' });
    }

    #[test]
    fn test_insert_mode_enter_submits() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key('i'));
        let action = handler.process_key(enter_key());
        assert_eq!(action, VimAction::SubmitInput);
    }

    #[test]
    fn test_insert_mode_backspace() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key('i'));
        let action = handler.process_key(backspace_key());
        assert_eq!(action, VimAction::Backspace);
    }

    #[test]
    fn test_insert_mode_arrow_keys() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key('i'));

        let action = handler.process_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(
            action,
            VimAction::MoveCursor {
                direction: Direction::Left,
                count: 1,
            }
        );

        let action = handler.process_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(
            action,
            VimAction::MoveCursor {
                direction: Direction::Right,
                count: 1,
            }
        );
    }

    #[test]
    fn test_process_char_in_insert_mode() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key('i'));
        let action = handler.process_char('z');
        assert_eq!(action, VimAction::InsertChar { c: 'z' });
    }

    #[test]
    fn test_process_char_in_normal_mode_is_none() {
        let mut handler = VimHandler::new();
        let action = handler.process_char('z');
        assert_eq!(action, VimAction::None);
    }

    // ── Visual mode behavior ────────────────────────────────────

    #[test]
    fn test_visual_mode_hjk() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key('v'));

        let action = handler.process_key(char_key('h'));
        assert_eq!(
            action,
            VimAction::MoveCursor {
                direction: Direction::Left,
                count: 1,
            }
        );

        let action = handler.process_key(char_key('j'));
        assert_eq!(
            action,
            VimAction::MoveCursor {
                direction: Direction::Down,
                count: 1,
            }
        );

        let action = handler.process_key(char_key('k'));
        assert_eq!(
            action,
            VimAction::MoveCursor {
                direction: Direction::Up,
                count: 1,
            }
        );

        let action = handler.process_key(char_key('l'));
        assert_eq!(
            action,
            VimAction::MoveCursor {
                direction: Direction::Right,
                count: 1,
            }
        );
    }

    #[test]
    fn test_visual_mode_y_exits_to_normal() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key('v'));
        let action = handler.process_key(char_key('y'));
        assert_eq!(handler.mode(), VimMode::Normal);
        assert_eq!(action, VimAction::YankLine { count: 1 });
    }

    #[test]
    fn test_visual_mode_d_exits_to_normal() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key('v'));
        let action = handler.process_key(char_key('d'));
        assert_eq!(handler.mode(), VimMode::Normal);
        assert_eq!(action, VimAction::DeleteLine { count: 1 });
    }

    // ── Edge cases ──────────────────────────────────────────────

    #[test]
    fn test_unknown_key_returns_none() {
        let mut handler = VimHandler::new();
        let action = handler.process_key(char_key('z'));
        assert_eq!(action, VimAction::None);
        assert_eq!(handler.mode(), VimMode::Normal);
    }

    #[test]
    fn test_unknown_two_char_sequence_resets() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key('d'));
        handler.process_key(char_key('z')); // dz is not a valid command
        assert_eq!(handler.mode(), VimMode::Normal);
    }

    #[test]
    fn test_three_char_sequence_resets() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key('d'));
        handler.process_key(char_key('d'));
        // After dd is processed, next char should be fresh
        handler.process_key(char_key('x'));
        assert_eq!(handler.mode(), VimMode::Normal);
    }

    #[test]
    fn test_reset_clears_all_state() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key('3')); // set count
        handler.process_key(char_key('d')); // start sequence
        handler.reset();
        assert_eq!(handler.mode(), VimMode::Normal);
        assert_eq!(handler.command_buffer(), "");
        assert!(!handler.is_search_mode());
    }

    #[test]
    fn test_mode_display_formatting() {
        assert_eq!(format!("{}", VimMode::Normal), "NORMAL");
        assert_eq!(format!("{}", VimMode::Insert), "INSERT");
        assert_eq!(format!("{}", VimMode::Visual), "VISUAL");
        assert_eq!(format!("{}", VimMode::Command), "COMMAND");
    }

    #[test]
    fn test_default_trait() {
        let handler = VimHandler::default();
        assert_eq!(handler.mode(), VimMode::Normal);
    }

    #[test]
    fn test_serde_serialization() {
        let mode = VimMode::Normal;
        let json = serde_json::to_string(&mode).unwrap();
        let parsed: VimMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, parsed);
    }

    #[test]
    fn test_visual_mode_arrow_keys() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key('v'));

        let action = handler.process_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(
            action,
            VimAction::MoveCursor {
                direction: Direction::Up,
                count: 1,
            }
        );
    }

    #[test]
    fn test_insert_mode_home_end() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key('i'));

        let action = handler.process_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
        assert_eq!(
            action,
            VimAction::MoveCursor {
                direction: Direction::LineStart,
                count: 1,
            }
        );

        let action = handler.process_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        assert_eq!(
            action,
            VimAction::MoveCursor {
                direction: Direction::LineEnd,
                count: 1,
            }
        );
    }

    // ── Reverse search ──────────────────────────────────────────

    #[test]
    fn test_question_mark_enters_backward_search() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key('?'));
        assert_eq!(handler.mode(), VimMode::Command);
        assert!(handler.is_search_mode());
        assert!(!handler.search_forward);
    }

    #[test]
    fn test_backward_search_executes_on_enter() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key('?'));
        handler.process_key(char_key('f'));
        handler.process_key(char_key('o'));
        let action = handler.process_key(enter_key());
        assert_eq!(handler.mode(), VimMode::Normal);
        assert_eq!(action, VimAction::SearchBackward { pattern: "fo".to_string() });
    }

    #[test]
    fn test_forward_search_preserves_direction() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key('/'));
        assert!(handler.search_forward);
        handler.process_key(char_key('t'));
        handler.process_key(char_key('e'));
        let action = handler.process_key(enter_key());
        assert_eq!(action, VimAction::SearchForward { pattern: "te".to_string() });
    }

    // ── dw / yw sequences ──────────────────────────────────────

    #[test]
    fn test_dw_deletes_word() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key('d'));
        let action = handler.process_key(char_key('w'));
        assert_eq!(action, VimAction::DeleteWord { count: 1 });
    }

    #[test]
    fn test_yw_yanks_word() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key('y'));
        let action = handler.process_key(char_key('w'));
        assert_eq!(action, VimAction::YankWord { count: 1 });
    }

    #[test]
    fn test_3dw_respects_count() {
        let mut handler = VimHandler::new();
        handler.process_key(char_key('3'));
        handler.process_key(char_key('d'));
        let action = handler.process_key(char_key('w'));
        assert_eq!(action, VimAction::DeleteWord { count: 3 });
    }

    // ── Marks ───────────────────────────────────────────────────

    #[test]
    fn test_set_and_jump_mark() {
        let mut handler = VimHandler::new();
        // Set mark 'a' — press 'm' then 'a'
        let action = handler.process_key(char_key('m'));
        assert_eq!(action, VimAction::None); // pending
        let action = handler.process_key(char_key('a'));
        assert_eq!(action, VimAction::SetMark { mark: 'a' });

        // Jump to mark 'a' — press '\'' then 'a'
        let action = handler.process_key(char_key('\''));
        assert_eq!(action, VimAction::None); // pending
        let action = handler.process_key(char_key('a'));
        assert_eq!(action, VimAction::JumpToMark { mark: 'a' });
    }

    #[test]
    fn test_mark_invalid_key_resets() {
        let mut handler = VimHandler::new();
        // Press 'm' then digit (not a valid mark)
        handler.process_key(char_key('m'));
        let action = handler.process_key(char_key('5'));
        // Should fall through to count accumulation, not SetMark
        assert_ne!(action, VimAction::SetMark { mark: '5' });
    }

    #[test]
    fn test_set_mark_stores_position() {
        let mut handler = VimHandler::new();
        handler.set_mark('x', 42);
        assert_eq!(handler.get_mark('x'), Some(&42));
        assert_eq!(handler.get_mark('z'), None);
    }
}
