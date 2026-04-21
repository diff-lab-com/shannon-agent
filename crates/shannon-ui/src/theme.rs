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

    /// Get a theme by name. Checks built-in themes first, then custom themes
    /// from `~/.shannon/themes/<name>.json`.
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "dark" | "default" => Some(Self::default_dark()),
            "light" => Some(Self::default_light()),
            "dracula" => Some(Self::dracula()),
            _ => load_custom_theme(name),
        }
    }

    /// List available theme names (built-in + custom from `~/.shannon/themes/`).
    pub fn available() -> Vec<&'static str> {
        let mut names: Vec<&'static str> = vec!["dark", "light", "dracula"];
        if let Some(dir) = dirs::home_dir().map(|h| h.join(".shannon").join("themes")) {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().map(|e| e == "json").unwrap_or(false) {
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            // Leak the string to get &'static str (loaded once, lives forever)
                            let leaked: &'static str = Box::leak(stem.to_string().into_boxed_str());
                            if !names.contains(&leaked) {
                                names.push(leaked);
                            }
                        }
                    }
                }
            }
        }
        names
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::detect()
    }
}

/// Load a custom theme from `~/.shannon/themes/<name>.json`.
///
/// Theme files are JSON with hex color values:
/// ```json
/// {
///   "primary": "#00FF00",
///   "text": "#FFFFFF",
///   "error": "#FF0000",
///   ...
/// }
/// ```
///
/// Any missing fields fall back to the dark theme defaults.
fn load_custom_theme(name: &str) -> Option<Theme> {
    let path = dirs::home_dir()?
        .join(".shannon")
        .join("themes")
        .join(format!("{name}.json"));
    let content = std::fs::read_to_string(&path).ok()?;
    let file: ThemeFile = serde_json::from_str(&content).ok()?;
    let base = Theme::default_dark();
    Some(Theme {
        name: Box::leak(name.to_string().into_boxed_str()),
        user_msg: resolve_color(&file.user_msg, base.user_msg),
        assistant_msg: resolve_color(&file.assistant_msg, base.assistant_msg),
        system_msg: resolve_color(&file.system_msg, base.system_msg),
        tool_msg: resolve_color(&file.tool_msg, base.tool_msg),
        primary: resolve_color(&file.primary, base.primary),
        secondary: resolve_color(&file.secondary, base.secondary),
        accent: resolve_color(&file.accent, base.accent),
        border: resolve_color(&file.border, base.border),
        border_dim: resolve_color(&file.border_dim, base.border_dim),
        header_text: resolve_color(&file.header_text, base.header_text),
        success: resolve_color(&file.success, base.success),
        warning: resolve_color(&file.warning, base.warning),
        error: resolve_color(&file.error, base.error),
        muted: resolve_color(&file.muted, base.muted),
        text: resolve_color(&file.text, base.text),
        text_dim: resolve_color(&file.text_dim, base.text_dim),
        diff_added: resolve_color(&file.diff_added, base.diff_added),
        diff_removed: resolve_color(&file.diff_removed, base.diff_removed),
        diff_header: resolve_color(&file.diff_header, base.diff_header),
        context_bar_fg: resolve_color(&file.context_bar_fg, base.context_bar_fg),
        context_bar_bg: resolve_color(&file.context_bar_bg, base.context_bar_bg),
    })
}

/// Serde-deserializable theme file. All fields are optional hex color strings.
#[derive(serde::Deserialize, Default)]
struct ThemeFile {
    user_msg: Option<String>,
    assistant_msg: Option<String>,
    system_msg: Option<String>,
    tool_msg: Option<String>,
    primary: Option<String>,
    secondary: Option<String>,
    accent: Option<String>,
    border: Option<String>,
    border_dim: Option<String>,
    header_text: Option<String>,
    success: Option<String>,
    warning: Option<String>,
    error: Option<String>,
    muted: Option<String>,
    text: Option<String>,
    text_dim: Option<String>,
    diff_added: Option<String>,
    diff_removed: Option<String>,
    diff_header: Option<String>,
    context_bar_fg: Option<String>,
    context_bar_bg: Option<String>,
}

/// Parse a color string into a `Color`.
///
/// Accepts hex like `"#FF0000"` or named colors like `"Red"`, `"Cyan"`.
fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();

    // Hex format: "#RRGGBB"
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            return Some(Color::Rgb(r, g, b));
        }
    }

    // Named colors
    match s.to_lowercase().as_str() {
        "black" => Some(Color::Black),
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "blue" => Some(Color::Blue),
        "magenta" => Some(Color::Magenta),
        "cyan" => Some(Color::Cyan),
        "gray" | "grey" => Some(Color::Gray),
        "darkgray" | "dark_grey" | "darkgrey" | "dark grey" => Some(Color::DarkGray),
        "white" => Some(Color::White),
        _ => None,
    }
}

/// Helper: resolve a theme file field to a `Color`, falling back to `base`.
fn resolve_color(val: &Option<String>, base: Color) -> Color {
    val.as_ref().and_then(|s| parse_color(s)).unwrap_or(base)
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
