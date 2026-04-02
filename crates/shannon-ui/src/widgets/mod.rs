//! Ratatui widgets for Shannon UI

use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

/// Welcome widget for initial screen
pub struct WelcomeWidget;

impl WelcomeWidget {
    /// Render the welcome message
    pub fn render(frame: &mut Frame, area: Rect) {
        let title = vec![
            Line::from(vec![
                Span::styled("Shannon", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::from(" - "),
                Span::styled("Terminal AI Agent Interface", Style::default().fg(Color::White)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Press ", Style::default().fg(Color::Gray)),
                Span::styled("q", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::from(" to quit"),
            ]),
            Line::from(vec![
                Span::styled("Press ", Style::default().fg(Color::Gray)),
                Span::styled("Ctrl+C", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::from(" to exit"),
            ]),
        ];

        let paragraph = Paragraph::new(title)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan))
                    .title(" Welcome ")
            )
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });

        frame.render_widget(paragraph, area);
    }
}

/// Status bar widget
pub struct StatusBarWidget;

impl StatusBarWidget {
    /// Render the status bar
    pub fn render(frame: &mut Frame, area: Rect, message: &str) {
        let line = vec![
            Span::styled(" Status: ", Style::default().fg(Color::Gray)),
            Span::styled(message, Style::default().fg(Color::White)),
        ];

        let paragraph = Paragraph::new(Line::from(line))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::DarkGray))
            )
            .alignment(Alignment::Left);

        frame.render_widget(paragraph, area);
    }
}
