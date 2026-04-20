//! Theme system for Shannon UI
//!
//! Provides configurable color themes with auto-detection of terminal background.

use ratatui::style::Color;

/// Color theme for the terminal UI.
///
/// Every color used across widgets is defined here so the entire
/// interface can be restyled by swapping the theme.
#[derive(Debug, Clone)]
pub struct Theme {
    pub name: &'static str,

    // Role colors
    pub user_msg: Color,
    pub assistant_msg: Color,
    pub system_msg: Color,
    pub tool_msg: Color,

    // UI chrome
    pub primary: Color,
    pub secondary: Color,
    pub accent: Color,
    pub border: Color,
    pub border_dim: Color,
    pub header_text: Color,

    // Status
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub muted: Color,

    // Text
    pub text: Color,
    pub text_dim: Color,

    // Diff
    pub diff_added: Color,
    pub diff_removed: Color,
    pub diff_header: Color,

    // Context bar
    pub context_bar_fg: Color,
    pub context_bar_bg: Color,
}

impl Theme {
    /// Default dark theme (Shannon's original palette).
    pub fn default_dark() -> Self {
        Self {
            name: "dark",
            user_msg: Color::Green,
            assistant_msg: Color::Cyan,
            system_msg: Color::Yellow,
            tool_msg: Color::Magenta,
            primary: Color::Cyan,
            secondary: Color::Yellow,
            accent: Color::Magenta,
            border: Color::Cyan,
            border_dim: Color::DarkGray,
            header_text: Color::Cyan,
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Red,
            muted: Color::DarkGray,
            text: Color::White,
            text_dim: Color::Gray,
            diff_added: Color::Green,
            diff_removed: Color::Red,
            diff_header: Color::Cyan,
            context_bar_fg: Color::Cyan,
            context_bar_bg: Color::DarkGray,
        }
    }

    /// Light theme for light terminal backgrounds.
    pub fn default_light() -> Self {
        Self {
            name: "light",
            user_msg: Color::Rgb(0, 100, 0),       // dark green
            assistant_msg: Color::Rgb(0, 80, 160),  // dark blue
            system_msg: Color::Rgb(180, 120, 0),    // dark gold
            tool_msg: Color::Rgb(128, 0, 128),      // dark magenta
            primary: Color::Rgb(0, 80, 160),
            secondary: Color::Rgb(180, 120, 0),
            accent: Color::Rgb(128, 0, 128),
            border: Color::Rgb(0, 80, 160),
            border_dim: Color::Rgb(180, 180, 180),
            header_text: Color::Rgb(0, 80, 160),
            success: Color::Rgb(0, 120, 0),
            warning: Color::Rgb(200, 120, 0),
            error: Color::Rgb(200, 0, 0),
            muted: Color::Rgb(160, 160, 160),
            text: Color::Rgb(30, 30, 30),
            text_dim: Color::Rgb(100, 100, 100),
            diff_added: Color::Rgb(0, 120, 0),
            diff_removed: Color::Rgb(200, 0, 0),
            diff_header: Color::Rgb(0, 80, 160),
            context_bar_fg: Color::Rgb(0, 80, 160),
            context_bar_bg: Color::Rgb(200, 200, 200),
        }
    }

    /// Dracula-inspired theme.
    pub fn dracula() -> Self {
        Self {
            name: "dracula",
            user_msg: Color::Rgb(80, 250, 123),     // green
            assistant_msg: Color::Rgb(189, 147, 249), // purple
            system_msg: Color::Rgb(241, 250, 140),  // yellow
            tool_msg: Color::Rgb(255, 121, 198),    // pink
            primary: Color::Rgb(189, 147, 249),
            secondary: Color::Rgb(241, 250, 140),
            accent: Color::Rgb(255, 121, 198),
            border: Color::Rgb(98, 114, 164),       // comment
            border_dim: Color::Rgb(68, 71, 90),     // current line
            header_text: Color::Rgb(189, 147, 249),
            success: Color::Rgb(80, 250, 123),
            warning: Color::Rgb(241, 250, 140),
            error: Color::Rgb(255, 85, 85),
            muted: Color::Rgb(98, 114, 164),
            text: Color::Rgb(248, 248, 242),
            text_dim: Color::Rgb(98, 114, 164),
            diff_added: Color::Rgb(80, 250, 123),
            diff_removed: Color::Rgb(255, 85, 85),
            diff_header: Color::Rgb(139, 233, 253), // cyan
            context_bar_fg: Color::Rgb(189, 147, 249),
            context_bar_bg: Color::Rgb(68, 71, 90),
        }
    }

    /// Auto-detect theme based on terminal background color.
    ///
    /// Uses the `COLORFGBG` environment variable (set by many terminals)
    /// to determine if the background is light or dark.
    pub fn detect() -> Self {
        if is_light_background() {
            Self::default_light()
        } else {
            Self::default_dark()
        }
    }

    /// Get a theme by name.
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "dark" | "default" => Some(Self::default_dark()),
            "light" => Some(Self::default_light()),
            "dracula" => Some(Self::dracula()),
            _ => None,
        }
    }

    /// List available theme names.
    pub fn available() -> &'static [&'static str] {
        &["dark", "light", "dracula"]
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::detect()
    }
}

/// Check if the terminal has a light background.
///
/// Reads `COLORFGBG` which many terminals set as "fg;bg" ANSI color indices.
/// Values 0-7 are standard dark colors, 8-15 are bright/light.
fn is_light_background() -> bool {
    std::env::var("COLORFGBG")
        .ok()
        .and_then(|val| {
            // Format: "fg;bg" e.g. "15;0" means white fg, black bg
            let parts: Vec<&str> = val.split(';').collect();
            parts.last().and_then(|bg| bg.parse::<u8>().ok())
        })
        .map(|bg| bg >= 8) // 8-15 are bright/light colors
        .unwrap_or(false) // Default to dark if unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_dark_theme() {
        let theme = Theme::default_dark();
        assert_eq!(theme.name, "dark");
        assert_eq!(theme.user_msg, Color::Green);
        assert_eq!(theme.assistant_msg, Color::Cyan);
    }

    #[test]
    fn test_default_light_theme() {
        let theme = Theme::default_light();
        assert_eq!(theme.name, "light");
    }

    #[test]
    fn test_dracula_theme() {
        let theme = Theme::dracula();
        assert_eq!(theme.name, "dracula");
    }

    #[test]
    fn test_theme_named() {
        assert!(Theme::named("dark").is_some());
        assert!(Theme::named("light").is_some());
        assert!(Theme::named("dracula").is_some());
        assert!(Theme::named("nonexistent").is_none());
    }

    #[test]
    fn test_theme_available() {
        let names = Theme::available();
        assert!(names.contains(&"dark"));
        assert!(names.contains(&"light"));
        assert!(names.contains(&"dracula"));
    }

    #[test]
    fn test_detect_returns_valid_theme() {
        let theme = Theme::detect();
        assert!(theme.name == "dark" || theme.name == "light");
    }
}
