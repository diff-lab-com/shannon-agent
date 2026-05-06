//! Header bar widget showing session information

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
    /// Get welcome message
    pub fn welcome_message(theme: &Theme) -> Vec<Span<'static>> {
        vec![
            Span::styled("Welcome to ", Style::default().fg(theme.success)),
            Span::styled("Shannon", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
            Span::styled("! ", Style::default().fg(theme.success)),
        ]
    }

    /// Get tip message
    pub fn tip_message(theme: &Theme) -> Vec<Span<'static>> {
        vec![
            Span::styled("Tip: ", Style::default().fg(theme.secondary)),
            Span::styled("/help", Style::default().fg(theme.text)),
        ]
    }

    /// Render the header bar with welcome message (shown only when chat is empty)
    pub fn render(
        frame: &mut Frame,
        area: Rect,
        theme: &Theme,
    ) {
        let mut spans: Vec<Span<'static>> = Self::welcome_message(theme);
        spans.push(Span::styled(" | ", Style::default().fg(theme.muted)));
        spans.extend(Self::tip_message(theme));

        let paragraph = Paragraph::new(Line::from(spans))
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
        3 // Top border + content + bottom padding
    }
}
