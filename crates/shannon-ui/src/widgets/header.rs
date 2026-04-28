//! Header bar widget showing session information

use crate::theme::Theme;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Rect},
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

    /// Render the header bar with all session information
    pub fn render(
        frame: &mut Frame,
        area: Rect,
        model: Option<&str>,
        tokens_used: Option<u64>,
        working_dir: &str,
        theme: &Theme,
    ) {
        // Split header area into left and right sections
        let chunks = ratatui::layout::Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        let left_area = chunks[0];
        let right_area = chunks[1];

        // Left side: Welcome + Tips (using helper methods)
        let mut left_spans: Vec<Span<'static>> = Self::welcome_message(theme);
        left_spans.push(Span::styled(" | ", Style::default().fg(theme.muted)));
        left_spans.extend(Self::tip_message(theme));

        let left_paragraph = Paragraph::new(Line::from(left_spans))
            .block(
                Block::default()
                    .borders(Borders::TOP | Borders::BOTTOM)
                    .border_style(Style::default().fg(theme.border))
            )
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true });

        frame.render_widget(left_paragraph, left_area);

        // Right side: Model | Tokens | Working Directory
        let mut right_spans: Vec<Span<'static>> = Vec::new();

        if let Some(m) = model {
            right_spans.push(Span::styled("Model: ", Style::default().fg(theme.text_dim)));
            right_spans.push(Span::styled(m.to_string(), Style::default().fg(theme.primary)));
            right_spans.push(Span::styled(" | ", Style::default().fg(theme.muted)));
        }

        right_spans.push(Span::styled("Tokens: ", Style::default().fg(theme.text_dim)));
        let tokens = tokens_used.unwrap_or(0);
        right_spans.push(Span::styled(tokens.to_string(), Style::default().fg(theme.secondary)));
        right_spans.push(Span::styled(" | ", Style::default().fg(theme.muted)));

        // Truncate working directory if too long
        let display_dir = if working_dir.len() > 20 {
            format!("...{}", &working_dir[working_dir.len() - 20..])
        } else {
            working_dir.to_string()
        };
        right_spans.push(Span::styled("Dir: ", Style::default().fg(theme.text_dim)));
        right_spans.push(Span::styled(display_dir, Style::default().fg(theme.text)));

        let right_paragraph = Paragraph::new(Line::from(right_spans))
            .block(
                Block::default()
                    .borders(Borders::TOP | Borders::BOTTOM)
                    .border_style(Style::default().fg(theme.border))
            )
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true });

        frame.render_widget(right_paragraph, right_area);
    }

    /// Get the recommended height for the header bar
    pub fn height() -> usize {
        3 // Top border + content + bottom padding
    }
}
