//! Configurable keybindings loaded from `~/.shannon/keybindings.json`.

use crossterm::event::{KeyCode, KeyModifiers};
use serde::Deserialize;
use std::path::PathBuf;

/// Parsed key binding: a key code plus optional modifiers.
#[derive(Debug, Clone)]
pub struct KeyBinding {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

/// All configurable keybindings for the TUI.
#[derive(Debug, Clone)]
pub struct KeyBindings {
    /// Quit the TUI
    pub quit: KeyBinding,
    /// Toggle sidebar panel
    pub toggle_sidebar: KeyBinding,
    /// Toggle tool output collapse
    pub toggle_tool_collapse: KeyBinding,
    /// Open command palette
    pub command_palette: KeyBinding,
    /// Incremental reverse search
    pub reverse_search: KeyBinding,
    /// Open model picker
    pub model_picker: KeyBinding,
    /// Activate leader key mode (Ctrl+X then second key)
    pub leader: KeyBinding,
    /// Open input in external editor ($EDITOR / $VISUAL)
    pub external_editor: KeyBinding,
    /// Toggle focus mode (hide header/statusbar)
    pub focus_mode: KeyBinding,
    /// Toggle fullscreen mode (hide ALL chrome)
    pub fullscreen: KeyBinding,
    /// Open transcript pager
    pub transcript: KeyBinding,
    /// Toggle chat search (highlight matches)
    pub chat_search: KeyBinding,
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self {
            quit: KeyBinding::ctrl('q'),
            toggle_sidebar: KeyBinding::ctrl('s'),
            toggle_tool_collapse: KeyBinding::ctrl('d'),
            command_palette: KeyBinding::ctrl('p'),
            reverse_search: KeyBinding::ctrl('r'),
            model_picker: KeyBinding::ctrl('m'),
            leader: KeyBinding::ctrl('x'),
            external_editor: KeyBinding::ctrl('e'),
            focus_mode: KeyBinding::ctrl('f'),
            fullscreen: KeyBinding {
                code: KeyCode::F(11),
                modifiers: KeyModifiers::NONE,
            },
            transcript: KeyBinding::ctrl('g'),
            chat_search: KeyBinding::ctrl('h'),
        }
    }
}

impl KeyBinding {
    pub fn ctrl(c: char) -> Self {
        Self {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::CONTROL,
        }
    }

    /// Check if a KeyEvent matches this binding.
    pub fn matches(&self, key: &crossterm::event::KeyEvent) -> bool {
        key.code == self.code && key.modifiers == self.modifiers
    }
}

/// File format for keybindings JSON.
#[derive(Deserialize, Default)]
#[serde(default)]
struct KeyBindingsFile {
    quit: Option<String>,
    toggle_sidebar: Option<String>,
    toggle_tool_collapse: Option<String>,
    command_palette: Option<String>,
    reverse_search: Option<String>,
    model_picker: Option<String>,
    leader: Option<String>,
    external_editor: Option<String>,
    focus_mode: Option<String>,
    fullscreen: Option<String>,
    transcript: Option<String>,
    chat_search: Option<String>,
}

/// Parse a key string like "ctrl+q", "ctrl+s", "escape", "f1" into a KeyBinding.
fn parse_key(s: &str) -> Option<KeyBinding> {
    let s = s.trim().to_lowercase();
    if let Some(rest) = s.strip_prefix("ctrl+") {
        let c = rest.chars().next()?;
        Some(KeyBinding::ctrl(c))
    } else if s == "escape" || s == "esc" {
        Some(KeyBinding {
            code: KeyCode::Esc,
            modifiers: KeyModifiers::NONE,
        })
    } else if s.starts_with('f') && s.len() <= 3 {
        let n: u8 = s[1..].parse().ok()?;
        if (1..=12).contains(&n) {
            Some(KeyBinding {
                code: KeyCode::F(n),
                modifiers: KeyModifiers::NONE,
            })
        } else {
            None
        }
    } else {
        None
    }
}

/// Load keybindings from `~/.shannon/keybindings.json`, falling back to defaults.
pub fn load_keybindings() -> KeyBindings {
    let config_path = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".shannon")
        .join("keybindings.json");

    let content = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(_) => return KeyBindings::default(),
    };

    let file: KeyBindingsFile = match serde_json::from_str(&content) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Warning: failed to parse keybindings.json: {e}");
            return KeyBindings::default();
        }
    };

    let defaults = KeyBindings::default();
    KeyBindings {
        quit: file.quit.as_deref().and_then(parse_key).unwrap_or(defaults.quit),
        toggle_sidebar: file.toggle_sidebar.as_deref().and_then(parse_key).unwrap_or(defaults.toggle_sidebar),
        toggle_tool_collapse: file.toggle_tool_collapse.as_deref().and_then(parse_key).unwrap_or(defaults.toggle_tool_collapse),
        command_palette: file.command_palette.as_deref().and_then(parse_key).unwrap_or(defaults.command_palette),
        reverse_search: file.reverse_search.as_deref().and_then(parse_key).unwrap_or(defaults.reverse_search),
        model_picker: file.model_picker.as_deref().and_then(parse_key).unwrap_or(defaults.model_picker),
        leader: file.leader.as_deref().and_then(parse_key).unwrap_or(defaults.leader),
        external_editor: file.external_editor.as_deref().and_then(parse_key).unwrap_or(defaults.external_editor),
        focus_mode: file.focus_mode.as_deref().and_then(parse_key).unwrap_or(defaults.focus_mode),
        fullscreen: file.fullscreen.as_deref().and_then(parse_key).unwrap_or(defaults.fullscreen),
        transcript: file.transcript.as_deref().and_then(parse_key).unwrap_or(defaults.transcript),
        chat_search: file.chat_search.as_deref().and_then(parse_key).unwrap_or(defaults.chat_search),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};

    #[test]
    fn test_parse_ctrl_key() {
        let kb = parse_key("ctrl+q").unwrap();
        assert_eq!(kb.code, KeyCode::Char('q'));
        assert_eq!(kb.modifiers, KeyModifiers::CONTROL);
    }

    #[test]
    fn test_parse_escape() {
        let kb = parse_key("escape").unwrap();
        assert_eq!(kb.code, KeyCode::Esc);
        assert_eq!(kb.modifiers, KeyModifiers::NONE);
    }

    #[test]
    fn test_parse_f_key() {
        let kb = parse_key("f1").unwrap();
        assert_eq!(kb.code, KeyCode::F(1));
    }

    #[test]
    fn test_parse_invalid() {
        assert!(parse_key("invalid").is_none());
        assert!(parse_key("").is_none());
    }

    #[test]
    fn test_key_binding_matches() {
        let kb = KeyBinding::ctrl('s');
        let event = crossterm::event::KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL);
        assert!(kb.matches(&event));
        let wrong_event = crossterm::event::KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE);
        assert!(!kb.matches(&wrong_event));
    }

    #[test]
    fn test_default_keybindings() {
        let kb = KeyBindings::default();
        assert_eq!(kb.quit.code, KeyCode::Char('q'));
        assert_eq!(kb.toggle_sidebar.code, KeyCode::Char('s'));
    }

    #[test]
    fn test_load_keybindings_no_file() {
        // Should return defaults when no file exists
        let kb = load_keybindings();
        assert_eq!(kb.quit.code, KeyCode::Char('q'));
    }
}
