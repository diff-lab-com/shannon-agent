//! Status bar widget
//!
//! Zone-based layout for stable, flicker-free rendering:
//! `[spinner] [model] [context bar] [progress] [padding] [cost] [git] [mode]`

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
    /// Render the status bar (simple mode, no spinner)
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

    /// Render enhanced status bar using a `RenderContext`.
    ///
    /// Derives files/tools/duration from `sidebar_info` internally.
    pub fn render_from_ctx(frame: &mut Frame, area: Rect, ctx: &crate::widgets::RenderContext) {
        let files_info = ctx.sidebar_info.map(|si| (si.modified_files.len(), si.total_additions, si.total_deletions));
        let tools_invoked = ctx.sidebar_info.map(|si| si.tools_invoked);
        let session_duration = ctx.sidebar_info.map(|si| si.session_duration_secs);
        Self::render_with_spinner(
            frame, area, ctx.status, ctx.model, ctx.tokens_used,
            ctx.max_tokens, ctx.cost_usd, ctx.git_branch, ctx.spinner,
            ctx.progress_bar, ctx.theme, ctx.approval_mode, ctx.token_breakdown,
            ctx.diag_counts, ctx.rate_limit, files_info, tools_invoked, session_duration,
        );
    }

    /// Render enhanced status bar with spinner animation and zone-based layout.
    ///
    /// Expects a 2-line area. Line 1: spinner, status, model, context, cost.
    /// Line 2: files, tools, duration, diagnostics, rate limit, git branch.
    #[allow(clippy::too_many_arguments)]
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
        token_breakdown: Option<(u64, u64)>,
        diag_counts: Option<(usize, usize)>,
        rate_limit: Option<(u32, u32)>,
        files_info: Option<(usize, usize, usize)>,
        tools_invoked: Option<usize>,
        session_duration: Option<u64>,
    ) {
        // Split area into 2 lines
        let line1 = Rect::new(area.x, area.y, area.width, 1);
        let line2 = Rect::new(area.x, area.y + 1, area.width, 1);

        // ── LINE 1: spinner, status, model, context, progress, cost ──
        let mut left: Vec<Span<'static>> = Vec::new();
        let mut right: Vec<Span<'static>> = Vec::new();

        // Spinner + status
        if let Some(sp) = spinner {
            if status != "Ready" {
                let frame_str = sp.current_char().to_string();
                left.push(Span::styled(
                    frame_str,
                    Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
                ));
                left.push(Span::raw(" "));
            }
        }
        left.push(Span::styled(
            status.to_string(),
            Style::default().fg(theme.text),
        ));

        // Approval mode
        if let Some(mode_label) = approval_mode {
            left.push(Span::raw(" "));
            let mode_style = match mode_label {
                "ASK" | "PLAN" => Style::default().fg(theme.warning),
                "EDIT" => Style::default().fg(theme.success),
                "AUTO" => Style::default().fg(theme.primary),
                "FULL" => Style::default().fg(ratatui::style::Color::Red),
                _ => Style::default().fg(theme.text_dim),
            };
            left.push(Span::styled(
                format!("[{mode_label}]"),
                mode_style.add_modifier(Modifier::BOLD),
            ));
        }

        // Model
        if let Some(m) = model {
            left.push(Span::styled(" · ", Style::default().fg(theme.border_dim)));
            left.push(Span::styled(
                truncate_model(m),
                Style::default().fg(theme.primary),
            ));
        }

        // Context window
        if let Some(used) = tokens_used {
            left.push(Span::styled(" · ", Style::default().fg(theme.border_dim)));
            if let Some(max) = max_tokens {
                let pct = (used as f64 / max as f64).min(1.0);
                let bar_w = 8usize;
                let filled = (pct * bar_w as f64).round() as usize;
                let mut bar = String::with_capacity(bar_w + 2);
                bar.push('[');
                for i in 0..bar_w {
                    bar.push(if i < filled {
                        crate::a11y::bar_filled().chars().next().unwrap_or('█')
                    } else {
                        crate::a11y::bar_empty().chars().next().unwrap_or('░')
                    });
                }
                bar.push(']');
                let bar_color = if pct < 0.5 {
                    theme.success
                } else if pct < 0.8 {
                    theme.warning
                } else {
                    theme.error
                };
                left.push(Span::styled(bar, Style::default().fg(bar_color)));
                let used_k = used as f64 / 1000.0;
                let max_k = max as f64 / 1000.0;
                left.push(Span::styled(
                    format!(" {used_k:.1}k/{max_k:.0}k"),
                    Style::default().fg(theme.secondary),
                ));
            } else {
                left.push(Span::styled(
                    format_tokens(used),
                    Style::default().fg(theme.secondary),
                ));
            }
        }

        // Token breakdown
        if let Some((input, output)) = token_breakdown {
            if input > 0 || output > 0 {
                left.push(Span::styled(
                    format!(" {}↑ {}↓", format_tokens(input), format_tokens(output)),
                    Style::default().fg(theme.secondary),
                ));
            }
        }

        // Token output rate during streaming (shown in status text by query.rs)
        let _ = spinner; // used by caller for animation phase

        // Tool progress bar
        if let Some(pb) = progress_bar {
            let pct = pb.percentage();
            if pct > 0.0 {
                left.push(Span::styled(" · ", Style::default().fg(theme.border_dim)));
                let bar_width = 12usize;
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
                left.push(Span::styled(
                    bar_str,
                    Style::default().fg(theme.primary),
                ));
                left.push(Span::styled(
                    format!(" {pct:.0}%"),
                    Style::default()
                        .fg(theme.secondary)
                        .add_modifier(Modifier::BOLD),
                ));
            }
        }

        // Cost (right-aligned)
        if let Some(cost) = cost_usd {
            right.push(Span::raw("  "));
            let cost_color = if cost < 0.01 {
                theme.text_dim
            } else if cost < 0.10 {
                theme.text
            } else {
                theme.warning
            };
            right.push(Span::styled(
                format!("${cost:.4}"),
                Style::default().fg(cost_color),
            ));
        }

        // Render line 1
        render_line(frame, line1, left, right, theme);

        // ── LINE 2: files, tools, duration, diagnostics, rate limit, git ──
        let mut left2: Vec<Span<'static>> = Vec::new();
        let mut right2: Vec<Span<'static>> = Vec::new();

        // Files modified
        if let Some((count, additions, deletions)) = files_info {
            if count > 0 {
                left2.push(Span::styled(
                    format!(" {count}f (+{additions}/-{deletions})"),
                    Style::default().fg(theme.secondary),
                ));
            }
        }

        // Tools invoked
        if let Some(tools) = tools_invoked {
            if tools > 0 {
                left2.push(Span::styled(
                    " · ",
                    Style::default().fg(theme.border_dim),
                ));
                left2.push(Span::styled(
                    format!("\u{1f527}{tools}"),
                    Style::default().fg(theme.secondary),
                ));
            }
        }

        // Session duration
        if let Some(secs) = session_duration {
            left2.push(Span::styled(
                " · ",
                Style::default().fg(theme.border_dim),
            ));
            left2.push(Span::styled(
                format_duration(secs).to_string(),
                Style::default().fg(theme.text_dim),
            ));
        }

        // Diagnostics
        if let Some((errors, warnings)) = diag_counts {
            if errors > 0 || warnings > 0 {
                right2.push(Span::styled(" · ", Style::default().fg(theme.border_dim)));
                if errors > 0 {
                    right2.push(Span::styled(
                        format!("E:{errors}"),
                        Style::default().fg(theme.error),
                    ));
                    if warnings > 0 {
                        right2.push(Span::raw(" "));
                    }
                }
                if warnings > 0 {
                    right2.push(Span::styled(
                        format!("W:{warnings}"),
                        Style::default().fg(theme.warning),
                    ));
                }
            }
        }

        // Rate limit
        if let Some((used, total)) = rate_limit {
            if total > 0 {
                let pct = used as f64 / total as f64;
                let color = if pct < 0.5 {
                    theme.success
                } else if pct < 0.8 {
                    theme.warning
                } else {
                    theme.error
                };
                right2.push(Span::styled(" · ", Style::default().fg(theme.border_dim)));
                right2.push(Span::styled(
                    format!("RL:{used}/{total}"),
                    Style::default().fg(color),
                ));
            }
        }

        // Git branch
        if let Some(branch) = git_branch {
            right2.push(Span::styled(" · ", Style::default().fg(theme.border_dim)));
            right2.push(Span::styled(
                format!("{} {}", branch_icon(), branch),
                Style::default().fg(theme.primary),
            ));
        }

        render_line(frame, line2, left2, right2, theme);
    }

    /// Render a custom statusline from a user-configured shell script.
    /// Replaces the entire status bar with the script's output.
    pub fn render_custom(frame: &mut Frame, area: Rect, text: &str, theme: &Theme) {
        let paragraph = Paragraph::new(Line::from(Span::styled(
            format!(" {text}"),
            Style::default().fg(theme.text),
        )))
        .style(Style::default().bg(theme.context_bar_bg))
        .alignment(Alignment::Left);

        frame.render_widget(paragraph, area);
    }
}

/// Truncate model name to fit status bar.
fn truncate_model(model: &str) -> String {
    const MAX_MODEL_LEN: usize = 24;
    if model.chars().count() > MAX_MODEL_LEN {
        let truncated: String = model.chars().take(MAX_MODEL_LEN - 1).collect();
        format!("{truncated}…")
    } else {
        model.to_string()
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

/// Format duration in seconds as "Xm Ys" or "Ys".
fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else {
        let m = secs / 60;
        let s = secs % 60;
        format!("{m}m {s}s")
    }
}

/// Git branch icon (Powerline symbol).
fn branch_icon() -> &'static str {
    "\u{E0A0}"
}

/// Render a single status line with left/right zones and padding.
fn render_line(frame: &mut Frame, area: Rect, left: Vec<Span<'static>>, right: Vec<Span<'static>>, theme: &Theme) {
    let left_w: usize = left.iter().map(|s| s.content.chars().count()).sum();
    let right_w: usize = right.iter().map(|s| s.content.chars().count()).sum();
    let total = left_w + right_w;
    let available = area.width as usize;
    let padding = available.saturating_sub(total);

    let mut spans = left;
    if !right.is_empty() && padding > 0 {
        spans.push(Span::raw(" ".repeat(padding)));
        spans.extend(right);
    } else if left_w < available {
        spans.push(Span::raw(" ".repeat(available.saturating_sub(left_w))));
    }

    let paragraph = Paragraph::new(Line::from(spans))
        .style(Style::default().bg(theme.context_bar_bg))
        .alignment(Alignment::Left);

    frame.render_widget(paragraph, area);
}
