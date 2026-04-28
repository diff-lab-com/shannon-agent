//! Status bar widget

use crate::theme::Theme;
use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

/// Status bar widget
pub struct StatusBarWidget;

impl StatusBarWidget {
    /// Render the status bar
    pub fn render(frame: &mut Frame, area: Rect, message: &str, theme: &Theme) {
        let line = vec![
            Span::styled(" Status: ", Style::default().fg(theme.text_dim)),
            Span::styled(message, Style::default().fg(theme.text)),
        ];

        let paragraph = Paragraph::new(Line::from(line))
            .style(Style::default().bg(theme.context_bar_bg))
            .alignment(Alignment::Left);

        frame.render_widget(paragraph, area);
    }

    /// Render enhanced status bar with spinner animation and optional progress bar.
    /// Dense single-line format for maximum screen real estate.
    pub fn render_with_spinner(
        frame: &mut Frame,
        area: Rect,
        status: &str,
        model: Option<&str>,
        tokens_used: Option<u64>,
        spinner: Option<&crate::widgets::progress::SpinnerWidget>,
        progress_bar: Option<&crate::widgets::progress::ProgressBarWidget>,
        theme: &Theme,
        approval_mode: Option<&str>,
    ) {
        // Build span with owned strings for proper lifetime
        let mut span_vec: Vec<Span<'static>> = Vec::new();

        // Separator helper
        let sep = || -> Span<'static> {
            Span::styled(" │ ", Style::default().fg(theme.muted))
        };

        // Show spinner frame when processing
        if let Some(sp) = spinner {
            if status != "Ready" {
                let frame_str = sp.current_char().to_string();
                span_vec.push(Span::styled(frame_str, Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)));
                span_vec.push(Span::raw(" "));
            }
        }

        span_vec.push(Span::styled(status.to_string(), Style::default().fg(theme.text)));

        if let Some(m) = model {
            span_vec.push(sep());
            span_vec.push(Span::styled(m.to_string(), Style::default().fg(theme.primary)));
        }

        if let Some(t) = tokens_used {
            span_vec.push(sep());
            span_vec.push(Span::styled(format!("Ctx: {t}"), Style::default().fg(theme.secondary)));
        }

        // If a progress bar is provided with active progress, show inline progress
        if let Some(pb) = progress_bar {
            let pct = pb.percentage();
            if pct > 0.0 {
                span_vec.push(sep());
                // Inline progress bar: [████████░░░░] 45.2%
                let bar_width = 10usize;
                let filled = (pb.progress() * bar_width as f64) as usize;
                let mut bar_str = String::from("[");
                for i in 0..bar_width {
                    if i < filled {
                        bar_str.push('█');
                    } else {
                        bar_str.push('░');
                    }
                }
                bar_str.push(']');
                span_vec.push(Span::styled(bar_str, Style::default().fg(theme.primary)));
                span_vec.push(Span::styled(
                    format!(" {pct:.0}%"),
                    Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD),
                ));
            }
        }

        // Approval mode indicator
        if let Some(mode_label) = approval_mode {
            span_vec.push(sep());
            let mode_style = match mode_label {
                label if label == "SUGGEST" || label == "PLAN" || label == "RO" => {
                    Style::default().fg(theme.warning)
                }
                label if label == "AUTO" => Style::default().fg(theme.success),
                label if label == "FULL" => Style::default().fg(theme.primary),
                label if label == "BYPASS" || label == "YOLO" => {
                    Style::default().fg(ratatui::style::Color::Red)
                }
                _ => Style::default().fg(theme.text_dim),
            };
            span_vec.push(Span::styled(mode_label.to_string(), mode_style.add_modifier(Modifier::BOLD)));
        }

        let paragraph = Paragraph::new(Line::from(span_vec))
            .style(Style::default().bg(theme.context_bar_bg))
            .alignment(Alignment::Left);

        frame.render_widget(paragraph, area);
    }
}
