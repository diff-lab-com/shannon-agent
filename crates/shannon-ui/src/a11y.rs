//! Accessibility helpers — plain-text replacements for decorative characters.

use std::cell::Cell;

thread_local! {
    static ACCESSIBILITY: Cell<bool> = const { Cell::new(false) };
}

/// Set the accessibility mode for the current thread.
pub fn set_enabled(enabled: bool) {
    ACCESSIBILITY.with(|c| c.set(enabled));
}

/// Check if accessibility mode is enabled.
pub fn is_enabled() -> bool {
    ACCESSIBILITY.with(|c| c.get())
}

/// Return the bar fill char based on accessibility mode.
pub fn bar_filled() -> &'static str {
    if is_enabled() { "#" } else { "█" }
}

/// Return the bar empty char based on accessibility mode.
pub fn bar_empty() -> &'static str {
    if is_enabled() { "-" } else { "░" }
}

/// Return the expand/collapse icon.
pub fn expand_icon(expanded: bool) -> &'static str {
    if is_enabled() {
        if expanded { "[-]" } else { "[+]" }
    } else if expanded {
        "▼"
    } else {
        "▶"
    }
}

/// Return a status dot icon.
pub fn status_dot(active: bool) -> &'static str {
    if is_enabled() {
        if active { "*" } else { "o" }
    } else if active {
        "●"
    } else {
        "○"
    }
}

/// Return the check mark or X mark.
pub fn check(ok: bool) -> &'static str {
    if is_enabled() {
        if ok { "[ok]" } else { "[x]" }
    } else if ok {
        "✓"
    } else {
        "✗"
    }
}

/// Return separator character for panels.
pub fn separator() -> &'static str {
    if is_enabled() { "|" } else { "│" }
}

/// Return the blockquote bar character.
pub fn blockquote_bar() -> &'static str {
    if is_enabled() { "|" } else { "│" }
}

/// Return the cursor character for input.
pub fn cursor() -> &'static str {
    if is_enabled() { "|" } else { "▌" }
}

/// Return the window title icon.
pub fn title_icon(streaming: bool) -> &'static str {
    if is_enabled() {
        if streaming { "*" } else { "-" }
    } else if streaming {
        "✦"
    } else {
        "◇"
    }
}

/// Return the git branch icon.
pub fn branch_icon() -> &'static str {
    if is_enabled() { "git:" } else { "⎇ " }
}

/// Sanitize a string, replacing all decorative characters with plain text.
pub fn sanitize(text: &str) -> String {
    if !is_enabled() {
        return text.to_string();
    }
    text.replace('█', "#")
        .replace('░', "-")
        .replace(['▌', '▎', '▏'], "|")
        .replace('▶', ">")
        .replace('▼', "v")
        .replace(['◆', '●', '★'], "*")
        .replace('✓', "[ok]")
        .replace('✗', "[x]")
        .replace('✦', "*")
        .replace('◇', "o")
        .replace('⚡', "!")
        .replace(['╭', '╮', '╰', '╯'], "+")
        .replace('│', "|")
        .replace('═', "=")
        .replace('─', "-")
        .replace('⎇', "git:")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_filled_default() {
        set_enabled(false);
        assert_eq!(bar_filled(), "█");
    }

    #[test]
    fn bar_filled_a11y() {
        set_enabled(true);
        assert_eq!(bar_filled(), "#");
        set_enabled(false);
    }

    #[test]
    fn bar_empty_default() {
        set_enabled(false);
        assert_eq!(bar_empty(), "░");
    }

    #[test]
    fn bar_empty_a11y() {
        set_enabled(true);
        assert_eq!(bar_empty(), "-");
        set_enabled(false);
    }

    #[test]
    fn expand_icon_collapsed_default() {
        set_enabled(false);
        assert_eq!(expand_icon(false), "▶");
    }

    #[test]
    fn expand_icon_expanded_default() {
        set_enabled(false);
        assert_eq!(expand_icon(true), "▼");
    }

    #[test]
    fn expand_icon_collapsed_a11y() {
        set_enabled(true);
        assert_eq!(expand_icon(false), "[+]");
        set_enabled(false);
    }

    #[test]
    fn expand_icon_expanded_a11y() {
        set_enabled(true);
        assert_eq!(expand_icon(true), "[-]");
        set_enabled(false);
    }

    #[test]
    fn status_dot_active_default() {
        set_enabled(false);
        assert_eq!(status_dot(true), "●");
    }

    #[test]
    fn status_dot_inactive_default() {
        set_enabled(false);
        assert_eq!(status_dot(false), "○");
    }

    #[test]
    fn status_dot_a11y() {
        set_enabled(true);
        assert_eq!(status_dot(true), "*");
        assert_eq!(status_dot(false), "o");
        set_enabled(false);
    }

    #[test]
    fn check_ok_default() {
        set_enabled(false);
        assert_eq!(check(true), "✓");
    }

    #[test]
    fn check_fail_default() {
        set_enabled(false);
        assert_eq!(check(false), "✗");
    }

    #[test]
    fn check_a11y() {
        set_enabled(true);
        assert_eq!(check(true), "[ok]");
        assert_eq!(check(false), "[x]");
        set_enabled(false);
    }

    #[test]
    fn separator_default() {
        set_enabled(false);
        assert_eq!(separator(), "│");
    }

    #[test]
    fn separator_a11y() {
        set_enabled(true);
        assert_eq!(separator(), "|");
        set_enabled(false);
    }

    #[test]
    fn blockquote_bar_default() {
        set_enabled(false);
        assert_eq!(blockquote_bar(), "│");
    }

    #[test]
    fn blockquote_bar_a11y() {
        set_enabled(true);
        assert_eq!(blockquote_bar(), "|");
        set_enabled(false);
    }

    #[test]
    fn cursor_default() {
        set_enabled(false);
        assert_eq!(cursor(), "▌");
    }

    #[test]
    fn cursor_a11y() {
        set_enabled(true);
        assert_eq!(cursor(), "|");
        set_enabled(false);
    }

    #[test]
    fn title_icon_idle_default() {
        set_enabled(false);
        assert_eq!(title_icon(false), "◇");
    }

    #[test]
    fn title_icon_streaming_default() {
        set_enabled(false);
        assert_eq!(title_icon(true), "✦");
    }

    #[test]
    fn title_icon_a11y() {
        set_enabled(true);
        assert_eq!(title_icon(false), "-");
        assert_eq!(title_icon(true), "*");
        set_enabled(false);
    }

    #[test]
    fn branch_icon_default() {
        set_enabled(false);
        assert_eq!(branch_icon(), "⎇ ");
    }

    #[test]
    fn branch_icon_a11y() {
        set_enabled(true);
        assert_eq!(branch_icon(), "git:");
        set_enabled(false);
    }

    #[test]
    fn sanitize_disabled_returns_same() {
        set_enabled(false);
        let text = "hello █░ world";
        assert_eq!(sanitize(text), text);
    }

    #[test]
    fn sanitize_enabled_replaces_chars() {
        set_enabled(true);
        let result = sanitize("█░▶▼●✓✗");
        assert_eq!(result, "#->v*[ok][x]");
        set_enabled(false);
    }

    #[test]
    fn is_enabled_default_false() {
        set_enabled(false);
        assert!(!is_enabled());
    }

    #[test]
    fn set_enabled_roundtrip() {
        set_enabled(true);
        assert!(is_enabled());
        set_enabled(false);
        assert!(!is_enabled());
    }
}
