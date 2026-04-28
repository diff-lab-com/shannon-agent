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
        max_tokens: Option<u64>,
        cost_usd: Option<f64>,
        git_branch: Option<&str>,
        spinner: Option<&crate::widgets::progress::SpinnerWidget>,
        progress_bar: Option<&crate::widgets::progress::ProgressBarWidget>,
        theme: &Theme,
        approval_mode: Option<&str>,
        goal: Option<&str>,
    ) {
        // Build span with owned strings for proper lifetime
        let mut span_vec: Vec<Span<'static>> = Vec::new();
        let mut right_vec: Vec<Span<'static>> = Vec::new();

        // Separator helper
        let sep = || -> Span<'static> {
            Span::styled(" │ ", Style::default().fg(theme.muted))
        };
        let right_sep = || -> Span<'static> {
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

        // Show goal with target icon if provided
        if let Some(goal_text) = goal {
            span_vec.push(Span::styled("🎯 ", Style::default().fg(theme.primary)));
            let truncated_goal = if goal_text.chars().count() > 40 {
                let truncated: String = goal_text.chars().take(37).collect();
                format!("{}...", truncated)
            } else {
                goal_text.to_string()
            };
            span_vec.push(Span::styled(truncated_goal, Style::default().fg(theme.text)));
            span_vec.push(sep());
        }

        span_vec.push(Span::styled(status.to_string(), Style::default().fg(theme.text)));

        if let Some(m) = model {
            span_vec.push(sep());
            span_vec.push(Span::styled(m.to_string(), Style::default().fg(theme.primary)));
        }

        // Context window usage with mini progress bar
        if let Some(used) = tokens_used {
            span_vec.push(sep());
            if let Some(max) = max_tokens {
                let pct = (used as f64 / max as f64).min(1.0);
                let bar_w = 8usize;
                let filled = (pct * bar_w as f64).round() as usize;
                let mut bar = String::with_capacity(bar_w + 2);
                bar.push('[');
                for i in 0..bar_w {
                    bar.push(if i < filled { '█' } else { '░' });
                }
                bar.push(']');
                // Color based on usage: green < 50%, yellow < 80%, red >= 80%
                let bar_color = if pct < 0.5 { theme.success }
                    else if pct < 0.8 { theme.warning }
                    else { theme.error };
                span_vec.push(Span::styled(bar, Style::default().fg(bar_color)));
                let used_k = used as f64 / 1000.0;
                let max_k = max as f64 / 1000.0;
                span_vec.push(Span::styled(
                    format!(" {used_k:.1}k/{max_k:.0}k"),
                    Style::default().fg(theme.secondary),
                ));
            } else {
                span_vec.push(Span::styled(format_tokens(used), Style::default().fg(theme.secondary)));
            }
        }

        // Cost estimation (right side)
        if let Some(cost) = cost_usd {
            right_vec.push(right_sep());
            let cost_color = if cost < 0.01 { theme.text_dim }
                else if cost < 0.10 { theme.text }
                else { theme.warning };
            right_vec.push(Span::styled(format!("${cost:.4}"), Style::default().fg(cost_color)));
        }

        // Git branch (right side)
        if let Some(branch) = git_branch {
            right_vec.push(right_sep());
            right_vec.push(Span::styled(
                format!(" {} {}", branch_icon(), branch),
                Style::default().fg(theme.primary),
            ));
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

        // Combine left and right spans with padding
        let left_content: usize = span_vec.iter().map(|s| s.content.chars().count()).sum::<usize>();
        let right_content: usize = right_vec.iter().map(|s| s.content.chars().count()).sum::<usize>();
        let total_content = left_content + right_content;
        let available = area.width as usize;
        let padding_needed = available.saturating_sub(total_content);
        if !right_vec.is_empty() && padding_needed > 3 {
            span_vec.push(Span::raw(" ".repeat(padding_needed)));
            span_vec.extend(right_vec);
        }

        let paragraph = Paragraph::new(Line::from(span_vec))
            .style(Style::default().bg(theme.context_bar_bg))
            .alignment(Alignment::Left);

        frame.render_widget(paragraph, area);
    }
}

/// Format token count as human-readable string (e.g., "12.3k").
fn format_tokens(tokens: u64) -> String {
    if tokens < 1000 {
        format!("{tokens}")
    } else if tokens < 1_000_000 {
        format!("{:.1}k", tokens as f64 / 1000.0)
    } else {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    }
}

/// Git branch icon (uses the standard branch symbol).
fn branch_icon() -> &'static str {
    "\u{E0A0}" //  git branch symbol from Powerline
}
