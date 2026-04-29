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
    } else if expanded { "▼" } else { "▶" }
}

/// Return a status dot icon.
pub fn status_dot(active: bool) -> &'static str {
    if is_enabled() {
        if active { "*" } else { "o" }
    } else if active { "●" } else { "○" }
}

/// Return the check mark or X mark.
pub fn check(ok: bool) -> &'static str {
    if is_enabled() {
        if ok { "[ok]" } else { "[x]" }
    } else if ok { "✓" } else { "✗" }
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
    } else if streaming { "✦" } else { "◇" }
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
}
