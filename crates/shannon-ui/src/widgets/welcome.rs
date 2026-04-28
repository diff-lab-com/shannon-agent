//! Welcome widget for initial screen

use crate::theme::Theme;
use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

/// Welcome widget for initial screen
pub struct WelcomeWidget;

impl WelcomeWidget {
    /// Render the welcome message
    pub fn render(frame: &mut Frame, area: Rect, theme: &Theme) {
        let title = vec![
            Line::from(vec![
                Span::styled("Shannon", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
                Span::from(" - "),
                Span::styled("Terminal AI Agent Interface", Style::default().fg(theme.text)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Press ", Style::default().fg(theme.text_dim)),
                Span::styled("q", Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD)),
                Span::from(" to quit"),
            ]),
            Line::from(vec![
                Span::styled("Press ", Style::default().fg(theme.text_dim)),
                Span::styled("Ctrl+C", Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD)),
                Span::from(" to exit"),
            ]),
        ];

        let paragraph = Paragraph::new(title)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border))
                    .title(" Welcome ")
            )
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });

        frame.render_widget(paragraph, area);
    }
}
