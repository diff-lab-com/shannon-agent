//! Header bar widget showing session information and keyboard shortcuts

use crate::theme::Theme;
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use rust_i18n::t;

/// Header bar widget showing session information
pub struct HeaderWidget;

impl HeaderWidget {
    /// Render the header bar with welcome message and keyboard shortcuts
    pub fn render(frame: &mut Frame, area: Rect, theme: &Theme) {
        let mut lines: Vec<Line<'static>> = Vec::new();

        // Line 1: Branded welcome with accent bar
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled("\u{258F}", Style::default().fg(theme.primary)),
            Span::styled(
                " Shannon",
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" \u{00B7} ", Style::default().fg(theme.border_dim)),
            Span::styled("AI code assistant", Style::default().fg(theme.text_dim)),
        ]));

        // Line 2: Key bindings (if area is tall enough)
        if area.height >= 3 {
            let sep = Span::styled(" \u{2502} ", Style::default().fg(theme.border_dim));
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                key("Enter", theme),
                desc(&t!("ui.header_send"), theme),
                sep.clone(),
                key("Shift+Enter", theme),
                desc(&t!("ui.header_newline"), theme),
                sep.clone(),
                key("Ctrl+E", theme),
                desc(&t!("ui.header_editor"), theme),
                sep.clone(),
                key("F11", theme),
                desc(&t!("ui.header_fullscreen"), theme),
            ]));

            // Line 3: More shortcuts (if area has room)
            if area.height >= 5 {
                lines.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    key("Ctrl+O", theme),
                    desc("Verbose", theme),
                    sep.clone(),
                    key("Alt+Enter", theme),
                    desc("Mode", theme),
                    sep.clone(),
                    key("Ctrl+G", theme),
                    desc("Pager", theme),
                    sep.clone(),
                    key("/help", theme),
                    desc("Commands", theme),
                ]));
            }
        }

        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::TOP | Borders::BOTTOM)
                    .border_style(Style::default().fg(theme.border)),
            )
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true });

        frame.render_widget(paragraph, area);
    }

    /// Get the recommended height for the header bar
    pub fn height() -> usize {
        5 // Top border + welcome + keys row 1 + keys row 2 + bottom border
    }
}

/// Styled keyboard key label
fn key(label: &str, theme: &Theme) -> Span<'static> {
    Span::styled(
        label.to_string(),
        Style::default()
            .fg(theme.secondary)
            .add_modifier(Modifier::BOLD),
    )
}

/// Styled description text after a key
fn desc(text: &str, theme: &Theme) -> Span<'static> {
    Span::styled(format!(" {text}"), Style::default().fg(theme.text_dim))
}
