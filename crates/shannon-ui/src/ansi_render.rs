//! Convert ratatui `Line`/`Span` to ANSI escape sequences for stdout output.
//!
//! Used by the inline viewport mode to print chat messages directly to the
//! terminal scrollback buffer instead of rendering them through ratatui's
//! alternate screen.

use std::fmt::Write;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;

/// Convert a slice of ratatui Lines to an ANSI-colored string.
/// Each line is terminated with `\n`. Empty lines produce just `\n`.
pub fn lines_to_ansi(lines: &[Line<'_>], width: u16) -> String {
    let mut out = String::new();
    for line in lines {
        let mut prev_style = Style::default();
        let mut needs_reset = false;

        for span in &line.spans {
            let s = span.style;
            if s != prev_style {
                if needs_reset {
                    out.push_str("\x1b[0m");
                }
                out.push_str(&style_to_ansi(s));
                prev_style = s;
                needs_reset = true;
            }
            out.push_str(&span.content);
        }

        if needs_reset {
            out.push_str("\x1b[0m");
        }
        // Pad to width so terminal scrollback lines align
        let content_width: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
        if content_width < width as usize {
            for _ in 0..width as usize - content_width {
                out.push(' ');
            }
        }
        out.push('\n');
    }
    out
}

/// Convert a ratatui Style to ANSI escape sequence prefix.
fn style_to_ansi(style: Style) -> String {
    let mut buf = String::from("\x1b[");

    let mut codes: Vec<String> = Vec::new();

    // Modifiers
    let mods = style.add_modifier;
    if mods.contains(Modifier::BOLD) {
        codes.push("1".into());
    }
    if mods.contains(Modifier::DIM) {
        codes.push("2".into());
    }
    if mods.contains(Modifier::ITALIC) {
        codes.push("3".into());
    }
    if mods.contains(Modifier::UNDERLINED) {
        codes.push("4".into());
    }
    if mods.contains(Modifier::SLOW_BLINK) {
        codes.push("5".into());
    }
    if mods.contains(Modifier::REVERSED) {
        codes.push("7".into());
    }
    if mods.contains(Modifier::CROSSED_OUT) {
        codes.push("9".into());
    }

    // Foreground color
    if let Some(fg) = color_to_sgr_fg(style.fg) {
        codes.push(fg);
    }

    // Background color
    if let Some(bg) = color_to_sgr_bg(style.bg) {
        codes.push(bg);
    }

    if codes.is_empty() {
        return "\x1b[0m".into();
    }

    write!(buf, "{}m", codes.join(";")).unwrap();
    buf
}

/// Convert a Color to an SGR foreground parameter string.
fn color_to_sgr_fg(color: Option<Color>) -> Option<String> {
    match color? {
        Color::Black => Some("30".into()),
        Color::Red => Some("31".into()),
        Color::Green => Some("32".into()),
        Color::Yellow => Some("33".into()),
        Color::Blue => Some("34".into()),
        Color::Magenta => Some("35".into()),
        Color::Cyan => Some("36".into()),
        Color::Gray => Some("37".into()),
        Color::DarkGray => Some("90".into()),
        Color::LightRed => Some("91".into()),
        Color::LightGreen => Some("92".into()),
        Color::LightYellow => Some("93".into()),
        Color::LightBlue => Some("94".into()),
        Color::LightMagenta => Some("95".into()),
        Color::LightCyan => Some("96".into()),
        Color::White => Some("97".into()),
        Color::Indexed(i) => Some(format!("38;5;{i}")),
        Color::Rgb(r, g, b) => Some(format!("38;2;{r};{g};{b}")),
        Color::Reset => None,
    }
}

/// Convert a Color to an SGR background parameter string.
fn color_to_sgr_bg(color: Option<Color>) -> Option<String> {
    match color? {
        Color::Black => Some("40".into()),
        Color::Red => Some("41".into()),
        Color::Green => Some("42".into()),
        Color::Yellow => Some("43".into()),
        Color::Blue => Some("44".into()),
        Color::Magenta => Some("45".into()),
        Color::Cyan => Some("46".into()),
        Color::Gray => Some("47".into()),
        Color::DarkGray => Some("100".into()),
        Color::LightRed => Some("101".into()),
        Color::LightGreen => Some("102".into()),
        Color::LightYellow => Some("103".into()),
        Color::LightBlue => Some("104".into()),
        Color::LightMagenta => Some("105".into()),
        Color::LightCyan => Some("106".into()),
        Color::White => Some("107".into()),
        Color::Indexed(i) => Some(format!("48;5;{i}")),
        Color::Rgb(r, g, b) => Some(format!("48;2;{r};{g};{b}")),
        Color::Reset => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::text::Span;

    #[test]
    fn test_basic_line_to_ansi() {
        let lines = vec![Line::from("Hello, world!")];
        let result = lines_to_ansi(&lines, 20);
        assert!(result.contains("Hello, world!"));
        assert!(result.ends_with('\n'));
    }

    #[test]
    fn test_styled_span() {
        let lines = vec![Line::from(vec![
            Span::styled("red", Style::default().fg(Color::Red)),
            Span::styled(" plain", Style::default()),
        ])];
        let result = lines_to_ansi(&lines, 20);
        assert!(result.contains("\x1b[31mred\x1b[0m"));
        assert!(result.contains(" plain"));
    }

    #[test]
    fn test_multiple_lines() {
        let lines = vec![
            Line::from("line 1"),
            Line::from("line 2"),
        ];
        let result = lines_to_ansi(&lines, 10);
        let line_count = result.matches('\n').count();
        assert_eq!(line_count, 2);
    }
}
