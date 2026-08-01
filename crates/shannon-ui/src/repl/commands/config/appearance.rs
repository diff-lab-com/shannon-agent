//! Appearance command handlers — split from `config.rs` (ADR-0008 P2-8).
//!
//! The terminal-look group: `/theme` (color theme switch + picker),
//! `/accessibility` (a11y toggle), `/terminal-setup` (environment diagnostic),
//! `/color` (per-session prompt-bar color), `/statusline` (custom statusline
//! command), and `/lang` (i18n locale). These share no helpers with the other
//! groups, so this module stands alone. `parse_color_string` is `pub(super)`
//! so the parent test module (which keeps all unit tests) can still reach it.

use crate::repl::Repl;
use crate::{Result, widgets::ChatRole};
use rust_i18n::t;

/// /theme — switch color theme or list available themes.
pub(crate) fn handle_theme(repl: &mut Repl, args: &str) -> Result<()> {
    use crate::theme::Theme;

    let args = args.trim();

    if args == "pick" || args == "picker" || args == "preview" {
        let themes = Theme::available();
        let current = &repl.state.theme.name;
        let items: Vec<_> = themes
            .into_iter()
            .map(|name| {
                let label = if name == *current {
                    format!("{name} (current)")
                } else {
                    name.clone()
                };
                crate::widgets::select::SelectItem::new(label, name)
            })
            .collect();

        let picker = crate::widgets::select::FuzzyPickerWidget::new("Theme Picker".to_string())
            .with_items(items);
        repl.state.theme_picker = Some(picker);
        return Ok(());
    }

    if args.is_empty() || args == "list" {
        let current = &repl.state.theme.name;
        let available = Theme::available();
        let mut msg = String::from("Available themes:\n");
        for name in available {
            if name == *current {
                msg.push_str(&format!("  * {name} (current)\n"));
            } else {
                msg.push_str(&format!("    {name}\n"));
            }
        }
        msg.push_str("\nUsage: /theme <name>");
        repl.chat.add_message(ChatRole::System, msg);
        return Ok(());
    }

    match Theme::named(args) {
        Some(theme) => {
            let name = theme.name.clone();
            repl.renderer.set_theme(&theme);
            repl.state.theme = theme;
            crate::repl::preferences::save_preferences(&crate::repl::preferences::Preferences {
                model: repl.state.model.clone(),
                provider: repl.state.selected_provider.clone(),
                theme: Some(name.to_string()),
            });
            repl.chat
                .add_message(ChatRole::System, format!("Theme switched to '{name}'."));
        }
        None => {
            let available = Theme::available().join(", ");
            repl.chat.add_message(
                ChatRole::System,
                format!("Unknown theme '{args}'. Available: {available}"),
            );
        }
    }

    Ok(())
}

/// /accessibility — toggle or check accessibility mode.
pub(crate) fn handle_accessibility(repl: &mut Repl, args: &str) -> Result<()> {
    let arg = args.trim();
    match arg {
        "on" | "enable" | "true" | "1" => {
            repl.state.accessibility_mode = true;
            crate::a11y::set_enabled(true);
            repl.chat.add_message(
                ChatRole::System,
                "Accessibility mode enabled. Decorative characters replaced with plain text."
                    .to_string(),
            );
        }
        "off" | "disable" | "false" | "0" => {
            repl.state.accessibility_mode = false;
            crate::a11y::set_enabled(false);
            repl.chat
                .add_message(ChatRole::System, "Accessibility mode disabled.".to_string());
        }
        "" | "status" => {
            let state = if repl.state.accessibility_mode {
                "enabled"
            } else {
                "disabled"
            };
            repl.chat.add_message(ChatRole::System,
                format!("Accessibility mode: {state}\n\nUsage: /accessibility on|off\nAlso auto-enabled via NO_GRAPHICS or ACCESSIBILITY env vars."));
        }
        _ => {
            repl.chat.add_message(
                ChatRole::System,
                "Usage: /accessibility on|off|status".to_string(),
            );
        }
    }
    Ok(())
}

pub(crate) fn handle_terminal_setup(repl: &mut Repl) -> Result<()> {
    let mut report = String::from("Terminal Setup Check\n\n");

    // 1. Shell detection
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "unknown".to_string());
    let shell_name = std::path::Path::new(&shell)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| shell.clone());
    report.push_str(&format!("Shell: {shell_name} ({shell})\n"));

    // 2. Terminal type
    let term = std::env::var("TERM").unwrap_or_else(|_| "not set".to_string());
    report.push_str(&format!("TERM: {term}\n"));

    // 3. Check if shannon is on PATH
    let shannon_on_path = std::process::Command::new("which")
        .arg("shannon")
        .output()
        .ok()
        .map(|o| o.status.success())
        .unwrap_or(false);
    report.push_str(&format!(
        "shannon on PATH: {}\n",
        if shannon_on_path {
            "yes"
        } else {
            "no — add shannon to your PATH"
        }
    ));

    // 4. Check for common terminal tools
    for tool in &["git", "gh", "node"] {
        let found = std::process::Command::new("which")
            .arg(tool)
            .output()
            .ok()
            .map(|o| o.status.success())
            .unwrap_or(false);
        report.push_str(&format!(
            "{tool}: {}\n",
            if found { "found" } else { "not found" }
        ));
    }

    // 5. Check shell integration markers
    // Claude Code uses SHANNON_INTEGRATION_DIR or similar env vars
    let has_integration = std::env::var("SHANNON_SHELL_INTEGRATION")
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    report.push_str(&format!(
        "Shell integration: {}\n",
        if has_integration {
            "active"
        } else {
            "not detected — add `eval \"$(shannon init)\"` to your shell profile for inline diagnostics and key bindings"
        }
    ));

    // 6. Check terminal dimensions
    let (w, h) = crossterm::terminal::size().unwrap_or((0, 0));
    report.push_str(&format!("Terminal size: {w}x{h}\n"));
    if w < 80 {
        report.push_str("  ⚠ Terminal width < 80 columns — UI may be cramped\n");
    }

    // 7. Color support
    let colors = std::env::var("COLORTERM").unwrap_or_else(|_| "not set".to_string());
    report.push_str(&format!("COLORTERM: {colors}\n"));

    // 8. Key binding hint
    report.push_str("\nKey bindings:\n");
    report.push_str("  Enter      — submit input\n");
    report.push_str("  Ctrl+C     — cancel current operation\n");
    report.push_str("  Ctrl+D     — exit Shannon\n");
    report.push_str("  Tab        — autocomplete\n");
    report.push_str("  Up/Down    — navigate history\n");
    report.push_str("  Escape     — enter/exit vim normal mode\n");

    report.push_str("\nShell profile setup:\n");
    match shell_name.as_str() {
        "zsh" => report.push_str("  Add to ~/.zshrc:\n    eval \"$(shannon init zsh)\"\n"),
        "bash" => report.push_str("  Add to ~/.bashrc:\n    eval \"$(shannon init bash)\"\n"),
        "fish" => report
            .push_str("  Add to ~/.config/fish/config.fish:\n    shannon init fish | source\n"),
        other => report.push_str(&format!(
            "  Unknown shell '{other}'. Add the appropriate init line to your shell profile.\n"
        )),
    }

    repl.chat.add_message(ChatRole::System, report);
    Ok(())
}

/// Handle /color command — set prompt bar color per session
pub(crate) fn handle_color(repl: &mut Repl, args: &str) -> Result<()> {
    let color = args.trim();
    if color.is_empty() || color == "default" || color == "reset" {
        repl.state.prompt_bar_color = None;
        repl.prompt.set_border_color(None);
        repl.chat.add_message(
            ChatRole::System,
            "Prompt bar color reset to default.".to_string(),
        );
    } else {
        // Validate color by trying to parse it
        let parsed = parse_color_string(color);
        match parsed {
            Some(c) => {
                repl.state.prompt_bar_color = Some(color.to_string());
                repl.prompt.set_border_color(Some(c));
                repl.chat.add_message(
                    ChatRole::System,
                    format!("Prompt bar color set to {color}."),
                );
            }
            None => {
                repl.chat.add_message(ChatRole::System, format!(
                    "Unknown color: \"{color}\". Use a named color (red, green, blue, ...) or hex (#ff0000), or \"default\" to reset."
                ));
            }
        }
    }
    Ok(())
}

/// Parse a color string into a ratatui Color
pub(crate) fn handle_statusline(repl: &mut Repl, args: &str) -> Result<()> {
    let cmd = args.trim();
    if cmd.is_empty() || cmd == "off" || cmd == "reset" || cmd == "default" {
        repl.state.statusline_command = None;
        repl.state.cached_statusline = None;
        repl.chat
            .add_message(ChatRole::System, "Custom statusline disabled.".to_string());
    } else {
        repl.state.statusline_command = Some(cmd.to_string());
        repl.state.cached_statusline = None;
        repl.state.statusline_last_update = None;
        repl.chat
            .add_message(ChatRole::System, format!("Custom statusline set to: {cmd}"));
    }
    Ok(())
}

pub(super) fn parse_color_string(s: &str) -> Option<ratatui::style::Color> {
    use ratatui::style::Color;
    let lower = s.to_lowercase();
    match lower.as_str() {
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "blue" => Some(Color::Blue),
        "yellow" => Some(Color::Yellow),
        "magenta" | "purple" | "pink" => Some(Color::Magenta),
        "cyan" | "teal" => Some(Color::Cyan),
        "white" => Some(Color::White),
        "gray" | "grey" => Some(Color::Gray),
        "darkgray" | "dark_grey" | "darkgrey" => Some(Color::DarkGray),
        "lightred" | "light_red" => Some(Color::LightRed),
        "lightgreen" | "light_green" => Some(Color::LightGreen),
        "lightblue" | "light_blue" => Some(Color::LightBlue),
        "lightyellow" | "light_yellow" => Some(Color::LightYellow),
        "lightmagenta" | "light_magenta" => Some(Color::LightMagenta),
        "lightcyan" | "light_cyan" => Some(Color::LightCyan),
        "black" => Some(Color::Black),
        _ => {
            // Try hex color
            let hex = s.trim_start_matches('#');
            if hex.len() == 6 {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                Some(Color::Rgb(r, g, b))
            } else {
                None
            }
        }
    }
}

pub(crate) fn handle_lang(repl: &mut Repl, args: &str) -> Result<()> {
    let supported = ["en", "zh", "hi", "es", "fr", "ar", "bn", "pt", "ru", "ja"];
    let input = args.trim();

    if input.is_empty() {
        let current = shannon_core::i18n::current_locale();
        repl.chat.add_message(
            ChatRole::System,
            format!(
                "Current language: {current}\n\nUsage: /lang <code>\nSupported: {}",
                supported.join(", ")
            ),
        );
        return Ok(());
    }

    let lang = input.to_lowercase();
    if supported.contains(&lang.as_str()) {
        shannon_core::i18n::set_locale(&lang);
        // Refresh status bar to reflect the new language immediately
        repl.state.status = t!("status.ready").to_string();
        let lang_names = [
            ("en", "English"),
            ("zh", "中文"),
            ("hi", "हिन्दी"),
            ("es", "Español"),
            ("fr", "Français"),
            ("ar", "العربية"),
            ("bn", "বাংলা"),
            ("pt", "Português"),
            ("ru", "Русский"),
            ("ja", "日本語"),
        ];
        let native_name = lang_names
            .iter()
            .find(|(c, _)| *c == lang)
            .map(|(_, n)| *n)
            .unwrap_or(&lang);
        repl.chat.add_message(
            ChatRole::System,
            format!("Language: {native_name} ({lang})"),
        );
    } else {
        repl.chat.add_message(
            ChatRole::System,
            format!(
                "Unsupported language: {lang}\nSupported: {}",
                supported.join(", ")
            ),
        );
    }
    Ok(())
}
