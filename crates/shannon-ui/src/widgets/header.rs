//! Header bar widget showing session information and keyboard shortcuts

use crate::theme::Theme;
use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

/// Header bar widget showing session information
pub struct HeaderWidget;

impl HeaderWidget {
    /// Render the header bar with welcome message and keyboard shortcuts
    pub fn render(
        frame: &mut Frame,
        area: Rect,
        theme: &Theme,
    ) {
        let mut lines: Vec<Line<'static>> = Vec::new();

        // Line 1: Welcome
        lines.push(Line::from(vec![
            Span::styled("  Welcome to ", Style::default().fg(theme.text_dim)),
            Span::styled("Shannon", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
            Span::styled(" — AI code assistant", Style::default().fg(theme.text_dim)),
        ]));

        // Line 2: Key bindings (if area is tall enough)
        if area.height >= 3 {
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled("Enter", Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD)),
                Span::styled(" Send  ", Style::default().fg(theme.text_dim)),
                Span::styled("Shift+Enter", Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD)),
                Span::styled(" Newline  ", Style::default().fg(theme.text_dim)),
                Span::styled("Ctrl+E", Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD)),
                Span::styled(" Editor  ", Style::default().fg(theme.text_dim)),
                Span::styled("F11", Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD)),
                Span::styled(" Fullscreen", Style::default().fg(theme.text_dim)),
            ]));

            // Line 3: More shortcuts (if area has room)
            if area.height >= 5 {
                lines.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled("Ctrl+O", Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD)),
                    Span::styled(" Verbose  ", Style::default().fg(theme.text_dim)),
                    Span::styled("Alt+Enter", Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD)),
                    Span::styled(" Mode  ", Style::default().fg(theme.text_dim)),
                    Span::styled("Ctrl+G", Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD)),
                    Span::styled(" Pager  ", Style::default().fg(theme.text_dim)),
                    Span::styled("/help", Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD)),
                    Span::styled(" Commands", Style::default().fg(theme.text_dim)),
                ]));
            }
        }

        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::TOP | Borders::BOTTOM)
                    .border_style(Style::default().fg(theme.border))
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
