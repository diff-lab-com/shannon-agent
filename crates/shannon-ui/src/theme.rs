//! Theme system for Shannon UI
//!
//! Provides configurable color themes with auto-detection of terminal background.
//! Supports 15 built-in themes, custom JSON themes, colorblind adaptation,
//! and live reload from `~/.shannon/themes/`.

use ratatui::style::Color;

/// Color theme for the terminal UI.
///
/// Every color used across widgets is defined here so the entire
/// interface can be restyled by swapping the theme.
#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,

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
    pub diff_added_bg: Color,
    pub diff_removed_bg: Color,
    pub diff_context: Color,
    pub diff_context_bg: Color,
    pub diff_added_word: Color,
    pub diff_removed_word: Color,
    pub diff_line_number: Color,
    pub diff_line_number_bg: Color,

    // Syntax highlighting
    pub syntax_keyword: Color,
    pub syntax_function: Color,
    pub syntax_string: Color,
    pub syntax_number: Color,
    pub syntax_comment: Color,
    pub syntax_type: Color,
    pub syntax_variable: Color,
    pub syntax_operator: Color,

    // Tool categories
    pub tool_read: Color,
    pub tool_write: Color,
    pub tool_search: Color,
    pub tool_bash: Color,

    // Fullscreen mode
    pub fullscreen_bg: Color,
    pub fullscreen_border: Color,

    // Subagent colors (8 distinct colors for parallel agents)
    pub subagent_1: Color,
    pub subagent_2: Color,
    pub subagent_3: Color,
    pub subagent_4: Color,
    pub subagent_5: Color,
    pub subagent_6: Color,
    pub subagent_7: Color,
    pub subagent_8: Color,

    // Misc
    pub context_bar_fg: Color,
    pub context_bar_bg: Color,
    pub selection_bg: Color,
    pub link: Color,
}

impl Theme {
    /// Default dark theme (Shannon's original palette).
    pub fn default_dark() -> Self {
        Self {
            name: "dark".to_string(),
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
            diff_added_bg: Color::Rgb(30, 60, 30),
            diff_removed_bg: Color::Rgb(60, 30, 30),
            diff_context: Color::Gray,
            diff_context_bg: Color::Rgb(40, 40, 40),
            diff_added_word: Color::Rgb(100, 220, 100),
            diff_removed_word: Color::Rgb(220, 100, 100),
            diff_line_number: Color::DarkGray,
            diff_line_number_bg: Color::Rgb(40, 40, 40),
            syntax_keyword: Color::Magenta,
            syntax_function: Color::Cyan,
            syntax_string: Color::Green,
            syntax_number: Color::Yellow,
            syntax_comment: Color::DarkGray,
            syntax_type: Color::Blue,
            syntax_variable: Color::White,
            syntax_operator: Color::Yellow,
            tool_read: Color::Blue,
            tool_write: Color::Yellow,
            tool_search: Color::Magenta,
            tool_bash: Color::Green,
            fullscreen_bg: Color::Black,
            fullscreen_border: Color::DarkGray,
            subagent_1: Color::Cyan,
            subagent_2: Color::Magenta,
            subagent_3: Color::Yellow,
            subagent_4: Color::Green,
            subagent_5: Color::Blue,
            subagent_6: Color::Red,
            subagent_7: Color::Rgb(255, 165, 0),   // orange
            subagent_8: Color::Rgb(180, 100, 255),  // purple
            context_bar_fg: Color::Cyan,
            context_bar_bg: Color::DarkGray,
            selection_bg: Color::Rgb(50, 50, 80),
            link: Color::Cyan,
        }
    }

    /// Light theme for light terminal backgrounds.
    pub fn default_light() -> Self {
        Self {
            name: "light".to_string(),
            user_msg: Color::Rgb(0, 100, 0),
            assistant_msg: Color::Rgb(0, 80, 160),
            system_msg: Color::Rgb(180, 120, 0),
            tool_msg: Color::Rgb(128, 0, 128),
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
            diff_added_bg: Color::Rgb(220, 240, 220),
            diff_removed_bg: Color::Rgb(240, 220, 220),
            diff_context: Color::Rgb(100, 100, 100),
            diff_context_bg: Color::Rgb(240, 240, 240),
            diff_added_word: Color::Rgb(0, 140, 0),
            diff_removed_word: Color::Rgb(180, 0, 0),
            diff_line_number: Color::Rgb(160, 160, 160),
            diff_line_number_bg: Color::Rgb(230, 230, 230),
            syntax_keyword: Color::Rgb(128, 0, 128),
            syntax_function: Color::Rgb(0, 80, 160),
            syntax_string: Color::Rgb(0, 100, 0),
            syntax_number: Color::Rgb(180, 120, 0),
            syntax_comment: Color::Rgb(130, 130, 130),
            syntax_type: Color::Rgb(0, 0, 180),
            syntax_variable: Color::Rgb(30, 30, 30),
            syntax_operator: Color::Rgb(180, 120, 0),
            tool_read: Color::Rgb(0, 80, 160),
            tool_write: Color::Rgb(180, 120, 0),
            tool_search: Color::Rgb(128, 0, 128),
            tool_bash: Color::Rgb(0, 100, 0),
            fullscreen_bg: Color::Rgb(255, 255, 255),
            fullscreen_border: Color::Rgb(180, 180, 180),
            subagent_1: Color::Rgb(0, 80, 160),
            subagent_2: Color::Rgb(128, 0, 128),
            subagent_3: Color::Rgb(180, 120, 0),
            subagent_4: Color::Rgb(0, 100, 0),
            subagent_5: Color::Rgb(0, 0, 180),
            subagent_6: Color::Rgb(200, 0, 0),
            subagent_7: Color::Rgb(200, 120, 0),
            subagent_8: Color::Rgb(140, 60, 200),
            context_bar_fg: Color::Rgb(0, 80, 160),
            context_bar_bg: Color::Rgb(200, 200, 200),
            selection_bg: Color::Rgb(180, 200, 240),
            link: Color::Rgb(0, 80, 160),
        }
    }

    /// Dracula-inspired theme.
    pub fn dracula() -> Self {
        Self {
            name: "dracula".to_string(),
            user_msg: Color::Rgb(80, 250, 123),
            assistant_msg: Color::Rgb(189, 147, 249),
            system_msg: Color::Rgb(241, 250, 140),
            tool_msg: Color::Rgb(255, 121, 198),
            primary: Color::Rgb(189, 147, 249),
            secondary: Color::Rgb(241, 250, 140),
            accent: Color::Rgb(255, 121, 198),
            border: Color::Rgb(98, 114, 164),
            border_dim: Color::Rgb(68, 71, 90),
            header_text: Color::Rgb(189, 147, 249),
            success: Color::Rgb(80, 250, 123),
            warning: Color::Rgb(241, 250, 140),
            error: Color::Rgb(255, 85, 85),
            muted: Color::Rgb(98, 114, 164),
            text: Color::Rgb(248, 248, 242),
            text_dim: Color::Rgb(98, 114, 164),
            diff_added: Color::Rgb(80, 250, 123),
            diff_removed: Color::Rgb(255, 85, 85),
            diff_header: Color::Rgb(139, 233, 253),
            diff_added_bg: Color::Rgb(30, 60, 40),
            diff_removed_bg: Color::Rgb(60, 30, 40),
            diff_context: Color::Rgb(98, 114, 164),
            diff_context_bg: Color::Rgb(50, 52, 68),
            diff_added_word: Color::Rgb(120, 255, 160),
            diff_removed_word: Color::Rgb(255, 130, 130),
            diff_line_number: Color::Rgb(98, 114, 164),
            diff_line_number_bg: Color::Rgb(50, 52, 68),
            syntax_keyword: Color::Rgb(255, 121, 198),
            syntax_function: Color::Rgb(139, 233, 253),
            syntax_string: Color::Rgb(241, 250, 140),
            syntax_number: Color::Rgb(189, 147, 249),
            syntax_comment: Color::Rgb(98, 114, 164),
            syntax_type: Color::Rgb(139, 233, 253),
            syntax_variable: Color::Rgb(248, 248, 242),
            syntax_operator: Color::Rgb(255, 121, 198),
            tool_read: Color::Rgb(139, 233, 253),
            tool_write: Color::Rgb(241, 250, 140),
            tool_search: Color::Rgb(255, 121, 198),
            tool_bash: Color::Rgb(80, 250, 123),
            fullscreen_bg: Color::Rgb(40, 42, 54),
            fullscreen_border: Color::Rgb(68, 71, 90),
            subagent_1: Color::Rgb(139, 233, 253),
            subagent_2: Color::Rgb(255, 121, 198),
            subagent_3: Color::Rgb(241, 250, 140),
            subagent_4: Color::Rgb(80, 250, 123),
            subagent_5: Color::Rgb(189, 147, 249),
            subagent_6: Color::Rgb(255, 85, 85),
            subagent_7: Color::Rgb(255, 184, 108),
            subagent_8: Color::Rgb(98, 114, 164),
            context_bar_fg: Color::Rgb(189, 147, 249),
            context_bar_bg: Color::Rgb(68, 71, 90),
            selection_bg: Color::Rgb(80, 80, 120),
            link: Color::Rgb(139, 233, 253),
        }
    }

    /// Tokyo Night theme.
    pub fn tokyonight() -> Self {
        let bg = Color::Rgb(26, 27, 38);
        let blue = Color::Rgb(122, 162, 247);
        let purple = Color::Rgb(187, 154, 247);
        let cyan = Color::Rgb(125, 207, 255);
        let green = Color::Rgb(158, 206, 106);
        let red = Color::Rgb(247, 118, 142);
        let yellow = Color::Rgb(224, 175, 104);
        let orange = Color::Rgb(255, 158, 100);
        let fg = Color::Rgb(192, 202, 245);
        let comment = Color::Rgb(82, 95, 128);
        let border_c = Color::Rgb(55, 61, 99);
        Self {
            name: "tokyonight".to_string(),
            user_msg: green,
            assistant_msg: blue,
            system_msg: yellow,
            tool_msg: purple,
            primary: blue,
            secondary: yellow,
            accent: purple,
            border: border_c,
            border_dim: Color::Rgb(36, 39, 58),
            header_text: blue,
            success: green,
            warning: yellow,
            error: red,
            muted: comment,
            text: fg,
            text_dim: comment,
            diff_added: green,
            diff_removed: red,
            diff_header: cyan,
            diff_added_bg: Color::Rgb(30, 50, 30),
            diff_removed_bg: Color::Rgb(50, 30, 30),
            diff_context: comment,
            diff_context_bg: Color::Rgb(32, 34, 48),
            diff_added_word: Color::Rgb(180, 230, 130),
            diff_removed_word: Color::Rgb(255, 150, 170),
            diff_line_number: comment,
            diff_line_number_bg: Color::Rgb(32, 34, 48),
            syntax_keyword: purple,
            syntax_function: cyan,
            syntax_string: green,
            syntax_number: orange,
            syntax_comment: comment,
            syntax_type: cyan,
            syntax_variable: fg,
            syntax_operator: yellow,
            tool_read: cyan,
            tool_write: yellow,
            tool_search: purple,
            tool_bash: green,
            fullscreen_bg: bg,
            fullscreen_border: border_c,
            subagent_1: blue,
            subagent_2: purple,
            subagent_3: yellow,
            subagent_4: green,
            subagent_5: cyan,
            subagent_6: red,
            subagent_7: orange,
            subagent_8: comment,
            context_bar_fg: blue,
            context_bar_bg: Color::Rgb(36, 39, 58),
            selection_bg: Color::Rgb(55, 61, 99),
            link: cyan,
        }
    }

    /// Catppuccin Mocha theme.
    pub fn catppuccin_mocha() -> Self {
        let mauve = Color::Rgb(203, 166, 247);
        let blue = Color::Rgb(137, 180, 250);
        let green = Color::Rgb(166, 227, 161);
        let red = Color::Rgb(243, 139, 168);
        let yellow = Color::Rgb(249, 226, 175);
        let peach = Color::Rgb(250, 179, 135);
        let teal = Color::Rgb(148, 226, 213);
        let pink = Color::Rgb(245, 194, 231);
        let surface0 = Color::Rgb(49, 50, 68);
        let base = Color::Rgb(30, 30, 46);
        let text_c = Color::Rgb(205, 214, 244);
        let overlay0 = Color::Rgb(108, 112, 134);
        Self {
            name: "catppuccin_mocha".to_string(),
            user_msg: green,
            assistant_msg: blue,
            system_msg: yellow,
            tool_msg: mauve,
            primary: blue,
            secondary: yellow,
            accent: mauve,
            border: surface0,
            border_dim: Color::Rgb(24, 24, 37),
            header_text: blue,
            success: green,
            warning: yellow,
            error: red,
            muted: overlay0,
            text: text_c,
            text_dim: overlay0,
            diff_added: green,
            diff_removed: red,
            diff_header: teal,
            diff_added_bg: Color::Rgb(30, 46, 30),
            diff_removed_bg: Color::Rgb(46, 30, 35),
            diff_context: overlay0,
            diff_context_bg: Color::Rgb(36, 36, 52),
            diff_added_word: Color::Rgb(190, 240, 185),
            diff_removed_word: Color::Rgb(255, 165, 185),
            diff_line_number: overlay0,
            diff_line_number_bg: Color::Rgb(36, 36, 52),
            syntax_keyword: mauve,
            syntax_function: blue,
            syntax_string: green,
            syntax_number: peach,
            syntax_comment: overlay0,
            syntax_type: teal,
            syntax_variable: text_c,
            syntax_operator: yellow,
            tool_read: teal,
            tool_write: yellow,
            tool_search: mauve,
            tool_bash: green,
            fullscreen_bg: base,
            fullscreen_border: surface0,
            subagent_1: blue,
            subagent_2: mauve,
            subagent_3: yellow,
            subagent_4: green,
            subagent_5: teal,
            subagent_6: red,
            subagent_7: peach,
            subagent_8: pink,
            context_bar_fg: blue,
            context_bar_bg: Color::Rgb(24, 24, 37),
            selection_bg: Color::Rgb(68, 71, 90),
            link: teal,
        }
    }

    /// Gruvbox Dark theme.
    pub fn gruvbox_dark() -> Self {
        let bg = Color::Rgb(40, 40, 40);
        let green = Color::Rgb(184, 187, 38);
        let red = Color::Rgb(251, 73, 52);
        let yellow = Color::Rgb(250, 189, 47);
        let blue = Color::Rgb(131, 165, 152);
        let purple = Color::Rgb(211, 134, 155);
        let aqua = Color::Rgb(142, 192, 124);
        let orange = Color::Rgb(254, 128, 25);
        let fg = Color::Rgb(235, 219, 178);
        let comment = Color::Rgb(124, 111, 100);
        let dim = Color::Rgb(80, 73, 69);
        Self {
            name: "gruvbox_dark".to_string(),
            user_msg: green,
            assistant_msg: blue,
            system_msg: yellow,
            tool_msg: purple,
            primary: yellow,
            secondary: orange,
            accent: purple,
            border: dim,
            border_dim: Color::Rgb(60, 56, 54),
            header_text: yellow,
            success: green,
            warning: yellow,
            error: red,
            muted: comment,
            text: fg,
            text_dim: comment,
            diff_added: green,
            diff_removed: red,
            diff_header: blue,
            diff_added_bg: Color::Rgb(40, 54, 30),
            diff_removed_bg: Color::Rgb(54, 30, 30),
            diff_context: comment,
            diff_context_bg: Color::Rgb(50, 48, 47),
            diff_added_word: Color::Rgb(210, 220, 60),
            diff_removed_word: Color::Rgb(255, 100, 80),
            diff_line_number: comment,
            diff_line_number_bg: Color::Rgb(50, 48, 47),
            syntax_keyword: red,
            syntax_function: green,
            syntax_string: yellow,
            syntax_number: purple,
            syntax_comment: comment,
            syntax_type: blue,
            syntax_variable: fg,
            syntax_operator: orange,
            tool_read: blue,
            tool_write: yellow,
            tool_search: purple,
            tool_bash: green,
            fullscreen_bg: bg,
            fullscreen_border: dim,
            subagent_1: blue,
            subagent_2: purple,
            subagent_3: yellow,
            subagent_4: green,
            subagent_5: aqua,
            subagent_6: red,
            subagent_7: orange,
            subagent_8: comment,
            context_bar_fg: yellow,
            context_bar_bg: Color::Rgb(60, 56, 54),
            selection_bg: Color::Rgb(80, 73, 69),
            link: blue,
        }
    }

    /// Nord theme.
    pub fn nord() -> Self {
        let polar_night = Color::Rgb(46, 52, 64);
        let snow_storm = Color::Rgb(216, 222, 233);
        let frost_blue = Color::Rgb(136, 192, 208);
        let frost_cyan = Color::Rgb(136, 192, 208);
        let frost_green = Color::Rgb(163, 190, 140);
        let aurora_purple = Color::Rgb(180, 142, 173);
        let aurora_red = Color::Rgb(191, 97, 106);
        let aurora_yellow = Color::Rgb(235, 203, 139);
        let aurora_orange = Color::Rgb(208, 135, 112);
        let dim = Color::Rgb(76, 86, 106);
        Self {
            name: "nord".to_string(),
            user_msg: frost_green,
            assistant_msg: frost_blue,
            system_msg: aurora_yellow,
            tool_msg: aurora_purple,
            primary: frost_blue,
            secondary: aurora_yellow,
            accent: aurora_purple,
            border: dim,
            border_dim: Color::Rgb(59, 66, 82),
            header_text: frost_blue,
            success: frost_green,
            warning: aurora_yellow,
            error: aurora_red,
            muted: dim,
            text: snow_storm,
            text_dim: dim,
            diff_added: frost_green,
            diff_removed: aurora_red,
            diff_header: frost_cyan,
            diff_added_bg: Color::Rgb(36, 48, 36),
            diff_removed_bg: Color::Rgb(48, 36, 38),
            diff_context: dim,
            diff_context_bg: Color::Rgb(52, 58, 72),
            diff_added_word: Color::Rgb(185, 210, 160),
            diff_removed_word: Color::Rgb(210, 120, 125),
            diff_line_number: dim,
            diff_line_number_bg: Color::Rgb(52, 58, 72),
            syntax_keyword: aurora_purple,
            syntax_function: frost_blue,
            syntax_string: frost_green,
            syntax_number: aurora_orange,
            syntax_comment: dim,
            syntax_type: frost_cyan,
            syntax_variable: snow_storm,
            syntax_operator: aurora_yellow,
            tool_read: frost_cyan,
            tool_write: aurora_yellow,
            tool_search: aurora_purple,
            tool_bash: frost_green,
            fullscreen_bg: polar_night,
            fullscreen_border: dim,
            subagent_1: frost_blue,
            subagent_2: aurora_purple,
            subagent_3: aurora_yellow,
            subagent_4: frost_green,
            subagent_5: frost_cyan,
            subagent_6: aurora_red,
            subagent_7: aurora_orange,
            subagent_8: dim,
            context_bar_fg: frost_blue,
            context_bar_bg: Color::Rgb(59, 66, 82),
            selection_bg: Color::Rgb(76, 86, 106),
            link: frost_cyan,
        }
    }

    /// Kanagawa theme.
    pub fn kanagawa() -> Self {
        let wave_blue = Color::Rgb(126, 156, 216);
        let sakura_red = Color::Rgb(195, 64, 67);
        let spring_yellow = Color::Rgb(192, 163, 110);
        let spring_green = Color::Rgb(152, 180, 120);
        let fuji_purple = Color::Rgb(149, 137, 218);
        let ronin_yellow = Color::Rgb(230, 195, 132);
        let old_white = Color::Rgb(220, 212, 188);
        let dragon_bg = Color::Rgb(24, 26, 34);
        let crystal_blue = Color::Rgb(134, 193, 221);
        let autumn_green = Color::Rgb(104, 157, 106);
        let peach = Color::Rgb(221, 158, 117);
        let dim = Color::Rgb(98, 100, 116);
        Self {
            name: "kanagawa".to_string(),
            user_msg: spring_green,
            assistant_msg: wave_blue,
            system_msg: spring_yellow,
            tool_msg: fuji_purple,
            primary: wave_blue,
            secondary: spring_yellow,
            accent: fuji_purple,
            border: dim,
            border_dim: Color::Rgb(40, 42, 54),
            header_text: wave_blue,
            success: autumn_green,
            warning: ronin_yellow,
            error: sakura_red,
            muted: dim,
            text: old_white,
            text_dim: dim,
            diff_added: autumn_green,
            diff_removed: sakura_red,
            diff_header: crystal_blue,
            diff_added_bg: Color::Rgb(30, 45, 30),
            diff_removed_bg: Color::Rgb(45, 30, 32),
            diff_context: dim,
            diff_context_bg: Color::Rgb(34, 36, 46),
            diff_added_word: Color::Rgb(170, 200, 140),
            diff_removed_word: Color::Rgb(220, 90, 95),
            diff_line_number: dim,
            diff_line_number_bg: Color::Rgb(34, 36, 46),
            syntax_keyword: fuji_purple,
            syntax_function: crystal_blue,
            syntax_string: spring_green,
            syntax_number: peach,
            syntax_comment: dim,
            syntax_type: wave_blue,
            syntax_variable: old_white,
            syntax_operator: spring_yellow,
            tool_read: crystal_blue,
            tool_write: spring_yellow,
            tool_search: fuji_purple,
            tool_bash: autumn_green,
            fullscreen_bg: dragon_bg,
            fullscreen_border: dim,
            subagent_1: wave_blue,
            subagent_2: fuji_purple,
            subagent_3: spring_yellow,
            subagent_4: spring_green,
            subagent_5: crystal_blue,
            subagent_6: sakura_red,
            subagent_7: peach,
            subagent_8: dim,
            context_bar_fg: wave_blue,
            context_bar_bg: Color::Rgb(40, 42, 54),
            selection_bg: Color::Rgb(60, 62, 80),
            link: crystal_blue,
        }
    }

    /// Monokai theme.
    pub fn monokai() -> Self {
        let bg = Color::Rgb(39, 40, 34);
        let yellow = Color::Rgb(230, 219, 116);
        let pink = Color::Rgb(249, 38, 114);
        let green = Color::Rgb(166, 226, 46);
        let orange = Color::Rgb(253, 151, 31);
        let purple = Color::Rgb(174, 129, 255);
        let blue = Color::Rgb(102, 217, 239);
        let fg = Color::Rgb(248, 248, 242);
        let comment = Color::Rgb(117, 113, 94);
        Self {
            name: "monokai".to_string(),
            user_msg: green,
            assistant_msg: blue,
            system_msg: yellow,
            tool_msg: purple,
            primary: yellow,
            secondary: orange,
            accent: purple,
            border: Color::Rgb(73, 72, 62),
            border_dim: Color::Rgb(55, 55, 45),
            header_text: yellow,
            success: green,
            warning: yellow,
            error: pink,
            muted: comment,
            text: fg,
            text_dim: comment,
            diff_added: green,
            diff_removed: pink,
            diff_header: blue,
            diff_added_bg: Color::Rgb(34, 50, 28),
            diff_removed_bg: Color::Rgb(50, 28, 38),
            diff_context: comment,
            diff_context_bg: Color::Rgb(45, 46, 40),
            diff_added_word: Color::Rgb(190, 240, 80),
            diff_removed_word: Color::Rgb(255, 80, 145),
            diff_line_number: comment,
            diff_line_number_bg: Color::Rgb(45, 46, 40),
            syntax_keyword: pink,
            syntax_function: green,
            syntax_string: yellow,
            syntax_number: purple,
            syntax_comment: comment,
            syntax_type: blue,
            syntax_variable: fg,
            syntax_operator: orange,
            tool_read: blue,
            tool_write: yellow,
            tool_search: purple,
            tool_bash: green,
            fullscreen_bg: bg,
            fullscreen_border: Color::Rgb(73, 72, 62),
            subagent_1: blue,
            subagent_2: purple,
            subagent_3: yellow,
            subagent_4: green,
            subagent_5: blue,
            subagent_6: pink,
            subagent_7: orange,
            subagent_8: comment,
            context_bar_fg: yellow,
            context_bar_bg: Color::Rgb(55, 55, 45),
            selection_bg: Color::Rgb(73, 72, 82),
            link: blue,
        }
    }

    /// One Dark theme.
    pub fn onedark() -> Self {
        let bg = Color::Rgb(40, 44, 52);
        let blue = Color::Rgb(97, 175, 239);
        let red = Color::Rgb(224, 108, 117);
        let green = Color::Rgb(152, 195, 121);
        let purple = Color::Rgb(198, 120, 221);
        let yellow = Color::Rgb(229, 192, 123);
        let cyan = Color::Rgb(86, 182, 194);
        let orange = Color::Rgb(209, 154, 102);
        let fg = Color::Rgb(171, 178, 191);
        let comment = Color::Rgb(92, 99, 112);
        let gutter = Color::Rgb(55, 60, 70);
        Self {
            name: "onedark".to_string(),
            user_msg: green,
            assistant_msg: blue,
            system_msg: yellow,
            tool_msg: purple,
            primary: blue,
            secondary: yellow,
            accent: purple,
            border: gutter,
            border_dim: Color::Rgb(48, 52, 60),
            header_text: blue,
            success: green,
            warning: yellow,
            error: red,
            muted: comment,
            text: fg,
            text_dim: comment,
            diff_added: green,
            diff_removed: red,
            diff_header: cyan,
            diff_added_bg: Color::Rgb(35, 50, 35),
            diff_removed_bg: Color::Rgb(50, 35, 38),
            diff_context: comment,
            diff_context_bg: Color::Rgb(48, 50, 58),
            diff_added_word: Color::Rgb(180, 215, 150),
            diff_removed_word: Color::Rgb(240, 130, 140),
            diff_line_number: comment,
            diff_line_number_bg: Color::Rgb(48, 50, 58),
            syntax_keyword: purple,
            syntax_function: blue,
            syntax_string: green,
            syntax_number: orange,
            syntax_comment: comment,
            syntax_type: cyan,
            syntax_variable: red,
            syntax_operator: yellow,
            tool_read: cyan,
            tool_write: yellow,
            tool_search: purple,
            tool_bash: green,
            fullscreen_bg: bg,
            fullscreen_border: gutter,
            subagent_1: blue,
            subagent_2: purple,
            subagent_3: yellow,
            subagent_4: green,
            subagent_5: cyan,
            subagent_6: red,
            subagent_7: orange,
            subagent_8: comment,
            context_bar_fg: blue,
            context_bar_bg: Color::Rgb(48, 52, 60),
            selection_bg: Color::Rgb(65, 70, 82),
            link: cyan,
        }
    }

    /// Everforest theme.
    pub fn everforest() -> Self {
        let bg = Color::Rgb(45, 53, 59);
        let green = Color::Rgb(167, 192, 128);
        let red = Color::Rgb(230, 126, 128);
        let yellow = Color::Rgb(219, 188, 127);
        let blue = Color::Rgb(127, 187, 179);
        let purple = Color::Rgb(211, 165, 165);
        let orange = Color::Rgb(230, 172, 104);
        let fg = Color::Rgb(211, 213, 203);
        let comment = Color::Rgb(126, 137, 138);
        let dim = Color::Rgb(60, 68, 72);
        Self {
            name: "everforest".to_string(),
            user_msg: green,
            assistant_msg: blue,
            system_msg: yellow,
            tool_msg: purple,
            primary: blue,
            secondary: yellow,
            accent: green,
            border: dim,
            border_dim: Color::Rgb(52, 60, 65),
            header_text: blue,
            success: green,
            warning: yellow,
            error: red,
            muted: comment,
            text: fg,
            text_dim: comment,
            diff_added: green,
            diff_removed: red,
            diff_header: blue,
            diff_added_bg: Color::Rgb(40, 55, 40),
            diff_removed_bg: Color::Rgb(55, 40, 42),
            diff_context: comment,
            diff_context_bg: Color::Rgb(50, 56, 62),
            diff_added_word: Color::Rgb(190, 210, 155),
            diff_removed_word: Color::Rgb(245, 150, 155),
            diff_line_number: comment,
            diff_line_number_bg: Color::Rgb(50, 56, 62),
            syntax_keyword: red,
            syntax_function: green,
            syntax_string: yellow,
            syntax_number: orange,
            syntax_comment: comment,
            syntax_type: blue,
            syntax_variable: fg,
            syntax_operator: orange,
            tool_read: blue,
            tool_write: yellow,
            tool_search: purple,
            tool_bash: green,
            fullscreen_bg: bg,
            fullscreen_border: dim,
            subagent_1: blue,
            subagent_2: purple,
            subagent_3: yellow,
            subagent_4: green,
            subagent_5: blue,
            subagent_6: red,
            subagent_7: orange,
            subagent_8: comment,
            context_bar_fg: blue,
            context_bar_bg: Color::Rgb(52, 60, 65),
            selection_bg: Color::Rgb(70, 78, 82),
            link: blue,
        }
    }

    /// Ayu Dark theme.
    pub fn ayu() -> Self {
        let bg = Color::Rgb(10, 14, 20);
        let blue = Color::Rgb(89, 194, 255);
        let red = Color::Rgb(240, 113, 120);
        let green = Color::Rgb(194, 217, 76);
        let yellow = Color::Rgb(255, 213, 79);
        let orange = Color::Rgb(255, 153, 64);
        let cyan = Color::Rgb(105, 220, 255);
        let fg = Color::Rgb(227, 230, 232);
        let comment = Color::Rgb(98, 114, 131);
        let gutter = Color::Rgb(30, 36, 44);
        Self {
            name: "ayu".to_string(),
            user_msg: green,
            assistant_msg: blue,
            system_msg: yellow,
            tool_msg: Color::Rgb(255, 153, 64),
            primary: blue,
            secondary: orange,
            accent: Color::Rgb(255, 153, 64),
            border: gutter,
            border_dim: Color::Rgb(20, 24, 32),
            header_text: blue,
            success: green,
            warning: yellow,
            error: red,
            muted: comment,
            text: fg,
            text_dim: comment,
            diff_added: green,
            diff_removed: red,
            diff_header: cyan,
            diff_added_bg: Color::Rgb(20, 36, 20),
            diff_removed_bg: Color::Rgb(36, 20, 22),
            diff_context: comment,
            diff_context_bg: Color::Rgb(16, 20, 28),
            diff_added_word: Color::Rgb(215, 235, 100),
            diff_removed_word: Color::Rgb(255, 140, 145),
            diff_line_number: comment,
            diff_line_number_bg: Color::Rgb(16, 20, 28),
            syntax_keyword: orange,
            syntax_function: blue,
            syntax_string: green,
            syntax_number: Color::Rgb(255, 214, 102),
            syntax_comment: comment,
            syntax_type: cyan,
            syntax_variable: fg,
            syntax_operator: red,
            tool_read: cyan,
            tool_write: yellow,
            tool_search: orange,
            tool_bash: green,
            fullscreen_bg: bg,
            fullscreen_border: gutter,
            subagent_1: blue,
            subagent_2: orange,
            subagent_3: yellow,
            subagent_4: green,
            subagent_5: cyan,
            subagent_6: red,
            subagent_7: Color::Rgb(255, 153, 64),
            subagent_8: comment,
            context_bar_fg: blue,
            context_bar_bg: Color::Rgb(20, 24, 32),
            selection_bg: Color::Rgb(40, 48, 60),
            link: cyan,
        }
    }

    /// Flexoki theme.
    pub fn flexoki() -> Self {
        let bg = Color::Rgb(40, 40, 40);
        let red = Color::Rgb(209, 77, 65);
        let green = Color::Rgb(135, 154, 57);
        let blue = Color::Rgb(67, 133, 190);
        let yellow = Color::Rgb(208, 162, 21);
        let purple = Color::Rgb(175, 88, 186);
        let cyan = Color::Rgb(66, 152, 148);
        let orange = Color::Rgb(218, 137, 55);
        let fg = Color::Rgb(206, 202, 190);
        let comment = Color::Rgb(110, 106, 96);
        let dim = Color::Rgb(90, 86, 76);
        Self {
            name: "flexoki".to_string(),
            user_msg: green,
            assistant_msg: blue,
            system_msg: yellow,
            tool_msg: purple,
            primary: blue,
            secondary: yellow,
            accent: purple,
            border: dim,
            border_dim: Color::Rgb(60, 56, 48),
            header_text: blue,
            success: green,
            warning: yellow,
            error: red,
            muted: comment,
            text: fg,
            text_dim: comment,
            diff_added: green,
            diff_removed: red,
            diff_header: cyan,
            diff_added_bg: Color::Rgb(40, 50, 30),
            diff_removed_bg: Color::Rgb(50, 35, 35),
            diff_context: comment,
            diff_context_bg: Color::Rgb(50, 48, 44),
            diff_added_word: Color::Rgb(160, 180, 80),
            diff_removed_word: Color::Rgb(235, 100, 85),
            diff_line_number: comment,
            diff_line_number_bg: Color::Rgb(50, 48, 44),
            syntax_keyword: purple,
            syntax_function: blue,
            syntax_string: green,
            syntax_number: orange,
            syntax_comment: comment,
            syntax_type: cyan,
            syntax_variable: fg,
            syntax_operator: yellow,
            tool_read: cyan,
            tool_write: yellow,
            tool_search: purple,
            tool_bash: green,
            fullscreen_bg: bg,
            fullscreen_border: dim,
            subagent_1: blue,
            subagent_2: purple,
            subagent_3: yellow,
            subagent_4: green,
            subagent_5: cyan,
            subagent_6: red,
            subagent_7: orange,
            subagent_8: comment,
            context_bar_fg: blue,
            context_bar_bg: Color::Rgb(60, 56, 48),
            selection_bg: Color::Rgb(80, 76, 68),
            link: cyan,
        }
    }

    /// Dark theme with colorblind-friendly palette (replaces red/green with blue/orange).
    pub fn dark_daltonized() -> Self {
        let base = Self::default_dark();
        Self {
            name: "dark_daltonized".to_string(),
            success: Color::Rgb(80, 140, 210),      // blue instead of green
            warning: base.warning,
            error: Color::Rgb(80, 140, 210),         // blue instead of red
            diff_added: Color::Rgb(230, 160, 50),    // orange instead of green
            diff_removed: Color::Rgb(80, 140, 210),  // blue instead of red
            diff_added_bg: Color::Rgb(50, 40, 20),   // dark orange tint
            diff_removed_bg: Color::Rgb(25, 40, 60),  // dark blue tint
            diff_added_word: Color::Rgb(255, 190, 80),
            diff_removed_word: Color::Rgb(120, 175, 240),
            syntax_keyword: base.syntax_keyword,
            syntax_string: Color::Rgb(230, 160, 50), // orange instead of green
            syntax_comment: base.syntax_comment,
            tool_bash: Color::Rgb(230, 160, 50),     // orange instead of green
            tool_read: Color::Rgb(80, 140, 210),     // blue
            ..base
        }
    }

    /// Light theme with colorblind-friendly palette.
    pub fn light_daltonized() -> Self {
        let base = Self::default_light();
        Self {
            name: "light_daltonized".to_string(),
            success: Color::Rgb(40, 90, 160),
            warning: base.warning,
            error: Color::Rgb(40, 90, 160),
            diff_added: Color::Rgb(200, 130, 0),
            diff_removed: Color::Rgb(40, 90, 160),
            diff_added_bg: Color::Rgb(245, 230, 200),
            diff_removed_bg: Color::Rgb(200, 220, 245),
            diff_added_word: Color::Rgb(220, 150, 20),
            diff_removed_word: Color::Rgb(60, 110, 190),
            syntax_string: Color::Rgb(200, 130, 0),
            tool_bash: Color::Rgb(200, 130, 0),
            tool_read: Color::Rgb(40, 90, 160),
            ..base
        }
    }

    /// Auto-detect theme based on terminal background color.
    pub fn detect() -> Self {
        if is_light_background() {
            Self::default_light()
        } else {
            Self::default_dark()
        }
    }

    /// Get a theme by name. Checks built-in themes first, then custom themes.
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "dark" | "default" => Some(Self::default_dark()),
            "light" => Some(Self::default_light()),
            "dracula" => Some(Self::dracula()),
            "tokyonight" | "tokyo-night" | "tokyo_night" => Some(Self::tokyonight()),
            "catppuccin" | "catppuccin_mocha" | "catppuccin-mocha" => Some(Self::catppuccin_mocha()),
            "gruvbox" | "gruvbox_dark" | "gruvbox-dark" => Some(Self::gruvbox_dark()),
            "nord" => Some(Self::nord()),
            "kanagawa" => Some(Self::kanagawa()),
            "monokai" => Some(Self::monokai()),
            "onedark" | "one-dark" | "one_dark" => Some(Self::onedark()),
            "everforest" => Some(Self::everforest()),
            "ayu" | "ayu-dark" | "ayu_dark" => Some(Self::ayu()),
            "flexoki" => Some(Self::flexoki()),
            "dark-daltonized" | "dark_daltonized" => Some(Self::dark_daltonized()),
            "light-daltonized" | "light_daltonized" => Some(Self::light_daltonized()),
            _ => load_custom_theme(name),
        }
    }

    /// List available theme names (built-in + custom).
    pub fn available() -> Vec<String> {
        let mut names: Vec<String> = vec![
            "dark".into(), "light".into(), "dracula".into(),
            "tokyonight".into(), "catppuccin_mocha".into(), "gruvbox_dark".into(),
            "nord".into(), "kanagawa".into(), "monokai".into(), "onedark".into(),
            "everforest".into(), "ayu".into(), "flexoki".into(),
            "dark_daltonized".into(), "light_daltonized".into(),
        ];
        if let Some(dir) = dirs::home_dir().map(|h| h.join(".shannon").join("themes")) {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().map(|e| e == "json").unwrap_or(false) {
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            let name = stem.to_string();
                            if !names.contains(&name) {
                                names.push(name);
                            }
                        }
                    }
                }
            }
        }
        names
    }

    /// Whether this is a dark theme (used to pick appropriate syntax highlighting).
    pub fn is_dark(&self) -> bool {
        // Heuristic: if the fullscreen background is darker than mid-gray, it's dark.
        fn luminance(c: ratatui::style::Color) -> f32 {
            match c {
                ratatui::style::Color::Rgb(r, g, b) => (r as f32 * 0.299 + g as f32 * 0.587 + b as f32 * 0.114) / 255.0,
                ratatui::style::Color::Black => 0.0,
                ratatui::style::Color::White => 1.0,
                ratatui::style::Color::DarkGray => 0.25,
                ratatui::style::Color::Gray => 0.5,
                _ => 0.0,
            }
        }
        luminance(self.fullscreen_bg) < 0.5
    }

    /// Return the syntect theme name that best matches this UI theme.
    pub fn syntect_theme_name(&self) -> &'static str {
        if self.is_dark() {
            "base16-eighties.dark"
        } else {
            "InspiredGitHub"
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::detect()
    }
}

/// Load a custom theme from `~/.shannon/themes/<name>.json`.
fn load_custom_theme(name: &str) -> Option<Theme> {
    let path = dirs::home_dir()?
        .join(".shannon")
        .join("themes")
        .join(format!("{name}.json"));
    let content = std::fs::read_to_string(&path).ok()?;
    let file: ThemeFile = serde_json::from_str(&content).ok()?;
    let base = Theme::default_dark();
    Some(Theme {
        name: name.to_string(),
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
        diff_added_bg: resolve_color(&file.diff_added_bg, base.diff_added_bg),
        diff_removed_bg: resolve_color(&file.diff_removed_bg, base.diff_removed_bg),
        diff_context: resolve_color(&file.diff_context, base.diff_context),
        diff_context_bg: resolve_color(&file.diff_context_bg, base.diff_context_bg),
        diff_added_word: resolve_color(&file.diff_added_word, base.diff_added_word),
        diff_removed_word: resolve_color(&file.diff_removed_word, base.diff_removed_word),
        diff_line_number: resolve_color(&file.diff_line_number, base.diff_line_number),
        diff_line_number_bg: resolve_color(&file.diff_line_number_bg, base.diff_line_number_bg),
        syntax_keyword: resolve_color(&file.syntax_keyword, base.syntax_keyword),
        syntax_function: resolve_color(&file.syntax_function, base.syntax_function),
        syntax_string: resolve_color(&file.syntax_string, base.syntax_string),
        syntax_number: resolve_color(&file.syntax_number, base.syntax_number),
        syntax_comment: resolve_color(&file.syntax_comment, base.syntax_comment),
        syntax_type: resolve_color(&file.syntax_type, base.syntax_type),
        syntax_variable: resolve_color(&file.syntax_variable, base.syntax_variable),
        syntax_operator: resolve_color(&file.syntax_operator, base.syntax_operator),
        tool_read: resolve_color(&file.tool_read, base.tool_read),
        tool_write: resolve_color(&file.tool_write, base.tool_write),
        tool_search: resolve_color(&file.tool_search, base.tool_search),
        tool_bash: resolve_color(&file.tool_bash, base.tool_bash),
        fullscreen_bg: resolve_color(&file.fullscreen_bg, base.fullscreen_bg),
        fullscreen_border: resolve_color(&file.fullscreen_border, base.fullscreen_border),
        subagent_1: resolve_color(&file.subagent_1, base.subagent_1),
        subagent_2: resolve_color(&file.subagent_2, base.subagent_2),
        subagent_3: resolve_color(&file.subagent_3, base.subagent_3),
        subagent_4: resolve_color(&file.subagent_4, base.subagent_4),
        subagent_5: resolve_color(&file.subagent_5, base.subagent_5),
        subagent_6: resolve_color(&file.subagent_6, base.subagent_6),
        subagent_7: resolve_color(&file.subagent_7, base.subagent_7),
        subagent_8: resolve_color(&file.subagent_8, base.subagent_8),
        context_bar_fg: resolve_color(&file.context_bar_fg, base.context_bar_fg),
        context_bar_bg: resolve_color(&file.context_bar_bg, base.context_bar_bg),
        selection_bg: resolve_color(&file.selection_bg, base.selection_bg),
        link: resolve_color(&file.link, base.link),
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
    diff_added_bg: Option<String>,
    diff_removed_bg: Option<String>,
    diff_context: Option<String>,
    diff_context_bg: Option<String>,
    diff_added_word: Option<String>,
    diff_removed_word: Option<String>,
    diff_line_number: Option<String>,
    diff_line_number_bg: Option<String>,
    syntax_keyword: Option<String>,
    syntax_function: Option<String>,
    syntax_string: Option<String>,
    syntax_number: Option<String>,
    syntax_comment: Option<String>,
    syntax_type: Option<String>,
    syntax_variable: Option<String>,
    syntax_operator: Option<String>,
    tool_read: Option<String>,
    tool_write: Option<String>,
    tool_search: Option<String>,
    tool_bash: Option<String>,
    fullscreen_bg: Option<String>,
    fullscreen_border: Option<String>,
    subagent_1: Option<String>,
    subagent_2: Option<String>,
    subagent_3: Option<String>,
    subagent_4: Option<String>,
    subagent_5: Option<String>,
    subagent_6: Option<String>,
    subagent_7: Option<String>,
    subagent_8: Option<String>,
    context_bar_fg: Option<String>,
    context_bar_bg: Option<String>,
    selection_bg: Option<String>,
    link: Option<String>,
}

/// Parse a color string into a `Color`.
///
/// Accepts:
/// - Hex: `"#RRGGBB"` or `"#RGB"`
/// - RGB function: `"rgb(r,g,b)"` or `"rgb(r, g, b)"`
/// - ANSI 256: `"ansi256(n)"`
/// - Named: `"Red"`, `"Cyan"`, etc.
fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();

    // Hex format: "#RRGGBB" or "#RGB"
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            return Some(Color::Rgb(r, g, b));
        }
        if hex.len() == 3 {
            let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
            return Some(Color::Rgb(r, g, b));
        }
    }

    // rgb(r,g,b) format
    let lower = s.to_lowercase();
    if let Some(inner) = lower.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) {
        let parts: Vec<&str> = inner.split(',').map(|p| p.trim()).collect();
        if parts.len() == 3 {
            let r = parts[0].parse::<u8>().ok()?;
            let g = parts[1].parse::<u8>().ok()?;
            let b = parts[2].parse::<u8>().ok()?;
            return Some(Color::Rgb(r, g, b));
        }
    }

    // ansi256(n) format
    if let Some(inner) = lower.strip_prefix("ansi256(").and_then(|s| s.strip_suffix(')')) {
        if let Ok(n) = inner.trim().parse::<u8>() {
            return Some(Color::Indexed(n));
        }
    }

    // Named colors
    match lower.as_str() {
        "black" => Some(Color::Black),
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "blue" => Some(Color::Blue),
        "magenta" | "purple" => Some(Color::Magenta),
        "cyan" => Some(Color::Cyan),
        "gray" | "grey" => Some(Color::Gray),
        "darkgray" | "dark_grey" | "darkgrey" | "dark grey" => Some(Color::DarkGray),
        "white" => Some(Color::White),
        _ => None,
    }
}

fn resolve_color(val: &Option<String>, base: Color) -> Color {
    val.as_ref().and_then(|s| parse_color(s)).unwrap_or(base)
}

/// Check if the terminal has a light background.
fn is_light_background() -> bool {
    std::env::var("COLORFGBG")
        .ok()
        .and_then(|val| {
            let parts: Vec<&str> = val.split(';').collect();
            parts.last().and_then(|bg| bg.parse::<u8>().ok())
        })
        .map(|bg| bg >= 8)
        .unwrap_or(false)
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
    fn test_all_builtin_themes_loadable() {
        let names = [
            "dark", "light", "dracula", "tokyonight", "catppuccin_mocha",
            "gruvbox_dark", "nord", "kanagawa", "monokai", "onedark",
            "everforest", "ayu", "flexoki", "dark_daltonized", "light_daltonized",
        ];
        for name in &names {
            assert!(Theme::named(name).is_some(), "Theme '{}' should load", name);
            let theme = Theme::named(name).unwrap();
            assert_eq!(theme.name, *name);
        }
    }

    #[test]
    fn test_theme_aliases() {
        assert!(Theme::named("tokyo-night").is_some());
        assert!(Theme::named("catppuccin").is_some());
        assert!(Theme::named("gruvbox").is_some());
        assert!(Theme::named("one-dark").is_some());
    }

    #[test]
    fn test_theme_available() {
        let names = Theme::available();
        assert!(names.contains(&"dark".to_string()));
        assert!(names.contains(&"tokyonight".to_string()));
        assert!(names.contains(&"catppuccin_mocha".to_string()));
        assert!(names.contains(&"dark_daltonized".to_string()));
    }

    #[test]
    fn test_nonexistent_theme() {
        assert!(Theme::named("nonexistent").is_none());
    }

    #[test]
    fn test_detect_returns_valid_theme() {
        let theme = Theme::detect();
        assert!(theme.name == "dark" || theme.name == "light");
    }

    #[test]
    fn test_parse_color_hex_6digit() {
        assert_eq!(parse_color("#FF0000"), Some(Color::Rgb(255, 0, 0)));
        assert_eq!(parse_color("#00ff00"), Some(Color::Rgb(0, 255, 0)));
    }

    #[test]
    fn test_parse_color_hex_3digit() {
        assert_eq!(parse_color("#F00"), Some(Color::Rgb(255, 0, 0)));
        assert_eq!(parse_color("#0F0"), Some(Color::Rgb(0, 255, 0)));
    }

    #[test]
    fn test_parse_color_rgb_function() {
        assert_eq!(parse_color("rgb(255,0,0)"), Some(Color::Rgb(255, 0, 0)));
        assert_eq!(parse_color("rgb(0, 255, 0)"), Some(Color::Rgb(0, 255, 0)));
    }

    #[test]
    fn test_parse_color_ansi256() {
        assert_eq!(parse_color("ansi256(196)"), Some(Color::Indexed(196)));
    }

    #[test]
    fn test_parse_color_named() {
        assert_eq!(parse_color("Red"), Some(Color::Red));
        assert_eq!(parse_color("cyan"), Some(Color::Cyan));
        assert_eq!(parse_color("purple"), Some(Color::Magenta));
    }

    #[test]
    fn test_daltonized_themes_replace_red_green() {
        let dark = Theme::dark_daltonized();
        // Should not use standard red/green for diff
        assert_ne!(dark.diff_added, Color::Green);
        assert_ne!(dark.diff_removed, Color::Red);

        let light = Theme::light_daltonized();
        assert_ne!(light.diff_added, Color::Rgb(0, 120, 0));
        assert_ne!(light.diff_removed, Color::Rgb(200, 0, 0));
    }

    #[test]
    fn test_all_themes_have_new_fields() {
        for name in Theme::available() {
            if let Some(theme) = Theme::named(&name) {
                // Verify new fields are not default (should be set to real colors)
                assert!(matches!(theme.diff_added_bg, Color::Rgb(..)));
                assert!(matches!(theme.diff_removed_bg, Color::Rgb(..)));
                assert!(matches!(theme.syntax_keyword, Color::Rgb(..) | Color::Magenta | Color::Red | Color::Indexed(..)));
                assert!(matches!(theme.tool_bash, Color::Rgb(..) | Color::Green));
            }
        }
    }
}
