//! Right sidebar panel showing session metadata

use crate::theme::Theme;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Data needed to render the sidebar
pub struct SidebarInfo {
    /// Model name
    pub model: Option<String>,
    /// Tokens used so far
    pub tokens_used: u64,
    /// Total session cost in USD
    pub cost_usd: f64,
    /// Number of tools invoked
    pub tools_invoked: usize,
    /// Modified files: (path, additions, deletions)
    pub modified_files: Vec<(String, usize, usize)>,
    /// Total additions across all files
    pub total_additions: usize,
    /// Total deletions across all files
    pub total_deletions: usize,
    /// Number of tool errors in session
    pub error_count: usize,
    /// Context window size for the current model (for progress bar)
    pub context_window: usize,
    /// Active sub-agents for the Agents tab
    pub active_agents: Vec<crate::repl::AgentDisplay>,
}

/// Right sidebar panel showing session metadata
pub struct SidebarWidget;

/// Minimum terminal width for the sidebar to appear
const SIDEBAR_WIDTH: u16 = 28;
const MIN_MAIN_WIDTH: u16 = 50;
/// Below this width, auto-hide sidebar even if toggled on
const MIN_SIDEBAR_WIDTH: u16 = 80;
/// Below this width, collapse header to single line
const COLLAPSE_HEADER_WIDTH: u16 = 60;
/// Minimum usable terminal size
const MIN_TERMINAL_WIDTH: u16 = 30;
const MIN_TERMINAL_HEIGHT: u16 = 8;

/// Truncate a string to fit within `max_chars` characters, appending "…" if truncated.
fn truncate_to(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else if max_chars > 1 {
        let truncated: String = s.chars().take(max_chars - 1).collect();
        format!("{truncated}…")
    } else {
        "…".to_string()
    }
}

/// Format token count as a human-readable string.
pub(super) fn format_tokens(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}k", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}

impl SidebarWidget {
    /// Render the sidebar panel
    pub fn render(frame: &mut Frame, area: Rect, info: &SidebarInfo, theme: &Theme, tab: crate::repl::SidebarTab) {
        let block = Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(theme.border))
            .title(Line::from(Span::styled(" Info ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD))));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let mut lines: Vec<Line<'static>> = Vec::new();
        let w = inner.width as usize;

        // Tab header
        let ctx_label = if tab == crate::repl::SidebarTab::Context { " Ctx " } else { " Ctx " };
        let files_label = if tab == crate::repl::SidebarTab::Files { " Files " } else { " Files " };
        let agents_label = if tab == crate::repl::SidebarTab::Agents { " Agents " } else { " Agents " };
        let ctx_style = if tab == crate::repl::SidebarTab::Context {
            Style::default().fg(theme.primary).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(theme.muted)
        };
        let files_style = if tab == crate::repl::SidebarTab::Files {
            Style::default().fg(theme.primary).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(theme.muted)
        };
        let agents_style = if tab == crate::repl::SidebarTab::Agents {
            Style::default().fg(theme.primary).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(theme.muted)
        };
        let sep = Style::default().fg(theme.border);
        lines.push(Line::from(vec![
            Span::styled(ctx_label, ctx_style),
            Span::styled("|", sep),
            Span::styled(files_label, files_style),
            Span::styled("|", sep),
            Span::styled(agents_label, agents_style),
        ]));
        lines.push(Line::from(""));

        match tab {
            crate::repl::SidebarTab::Context => {
                // Model section
                lines.push(Line::from(Span::styled("Model", Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD))));
                let model_name = info.model.as_deref().unwrap_or("unknown");
                lines.push(Line::from(Span::styled(truncate_to(model_name, w), Style::default().fg(theme.primary))));
                lines.push(Line::from(""));

                // Context usage
                lines.push(Line::from(Span::styled("Context", Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD))));
                let tokens_str = format_tokens(info.tokens_used);
                let pct = if info.context_window > 0 {
                    ((info.tokens_used as f64 / info.context_window as f64) * 100.0).min(100.0)
                } else {
                    0.0
                };
                let pct_label = format!("{tokens_str} ({pct:.0}%)");
                lines.push(Line::from(Span::styled(pct_label, Style::default().fg(theme.text))));
                // Progress bar based on actual context window percentage
                let bar_width = w.saturating_sub(2).max(4);
                let filled = (pct / 100.0 * bar_width as f64).round() as usize;
                let filled = filled.min(bar_width);
                let bar_color = if pct > 90.0 {
                    theme.error
                } else if pct > 75.0 {
                    theme.warning
                } else {
                    theme.secondary
                };
                let bar_str = format!(" {}{}", crate::a11y::bar_filled().repeat(filled), crate::a11y::bar_empty().repeat(bar_width.saturating_sub(filled)));
                lines.push(Line::from(Span::styled(truncate_to(&bar_str, w), Style::default().fg(bar_color))));
                lines.push(Line::from(""));

                // Cost
                lines.push(Line::from(Span::styled("Cost", Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD))));
                let cost_str = format!("${:.4}", info.cost_usd);
                lines.push(Line::from(Span::styled(cost_str, Style::default().fg(theme.warning))));
                lines.push(Line::from(""));

                // Tools
                lines.push(Line::from(Span::styled("Tools", Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD))));
                lines.push(Line::from(Span::styled(info.tools_invoked.to_string(), Style::default().fg(theme.text))));
                if info.error_count > 0 {
                    lines.push(Line::from(Span::styled(
                        format!("  {} errors", info.error_count),
                        Style::default().fg(theme.error),
                    )));
                }
            }
            crate::repl::SidebarTab::Files => {
                if info.modified_files.is_empty() {
                    lines.push(Line::from(Span::styled("No modified files", Style::default().fg(theme.muted))));
                } else {
                    // Summary line
                    lines.push(Line::from(vec![
                        Span::styled("+", Style::default().fg(theme.success)),
                        Span::styled(info.total_additions.to_string(), Style::default().fg(theme.success)),
                        Span::styled(" ", Style::default().fg(theme.text_dim)),
                        Span::styled("-", Style::default().fg(theme.error)),
                        Span::styled(info.total_deletions.to_string(), Style::default().fg(theme.error)),
                        Span::styled(format!("  ({} files)", info.modified_files.len()), Style::default().fg(theme.muted)),
                    ]));
                    lines.push(Line::from(""));

                    // Show up to 20 files (more space since this is a dedicated tab)
                    for (path, adds, dels) in info.modified_files.iter().take(20) {
                        let fname = path.split('/').next_back().unwrap_or(path);
                        let changes = if *adds > 0 && *dels > 0 {
                            format!("+{adds}-{dels}")
                        } else if *adds > 0 {
                            format!("+{adds}")
                        } else {
                            format!("-{dels}")
                        };
                        lines.push(Line::from(vec![
                            Span::styled(truncate_to(fname, w.saturating_sub(8)), Style::default().fg(theme.text)),
                            Span::styled(" ", Style::default().fg(theme.text_dim)),
                            Span::styled(changes, Style::default().fg(theme.muted)),
                        ]));
                        // Show parent path if it fits
                        if let Some(parent) = path.strip_suffix(fname).and_then(|p| p.strip_suffix('/')) {
                            if !parent.is_empty() && w > 20 {
                                lines.push(Line::from(Span::styled(
                                    format!("  {}", truncate_to(parent, w - 2)),
                                    Style::default().fg(theme.text_dim),
                                )));
                            }
                        }
                    }
                    if info.modified_files.len() > 20 {
                        lines.push(Line::from(Span::styled(
                            format!("  ...+{} more", info.modified_files.len() - 20),
                            Style::default().fg(theme.muted),
                        )));
                    }
                }
            }
            crate::repl::SidebarTab::Agents => {
                if info.active_agents.is_empty() {
                    lines.push(Line::from(Span::styled("No active agents", Style::default().fg(theme.muted))));
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled("Use /team or /agent", Style::default().fg(theme.text_dim))));
                    lines.push(Line::from(Span::styled("to spawn agents", Style::default().fg(theme.text_dim))));
                } else {
                    // Count active vs total
                    let active_count = info.active_agents.iter().filter(|a| a.active).count();
                    let total = info.active_agents.len();
                    lines.push(Line::from(Span::styled(
                        format!("{active_count}/{total} active"),
                        Style::default().fg(theme.text),
                    )));
                    lines.push(Line::from(""));

                    for agent in info.active_agents.iter().take(15) {
                        let status_icon = match agent.status.as_str() {
                            "running" => crate::a11y::status_dot(true),
                            "spawning" => if crate::a11y::is_enabled() { "~" } else { "◐" },
                            "idle" => crate::a11y::status_dot(false),
                            "completed" => crate::a11y::check(true),
                            s if s.starts_with("failed") => crate::a11y::check(false),
                            _ => if crate::a11y::is_enabled() { "." } else { "·" },
                        };
                        let status_color = match agent.status.as_str() {
                            "running" => theme.success,
                            "spawning" => theme.warning,
                            "idle" => theme.muted,
                            "completed" => theme.secondary,
                            s if s.starts_with("failed") => theme.error,
                            _ => theme.text_dim,
                        };
                        let name_display = truncate_to(&agent.name, w.saturating_sub(6));
                        lines.push(Line::from(vec![
                            Span::styled(status_icon, Style::default().fg(status_color)),
                            Span::styled(" ", Style::default()),
                            Span::styled(name_display, Style::default().fg(theme.text)),
                        ]));
                        // Status line
                        let turns_label = if agent.max_turns > 0 {
                            format!("  {}/{} turns", agent.turns_used, agent.max_turns)
                        } else {
                            format!("  {}", agent.status)
                        };
                        lines.push(Line::from(Span::styled(
                            truncate_to(&turns_label, w),
                            Style::default().fg(theme.text_dim),
                        )));
                        if let Some(ref team) = agent.team {
                            if team != "_global" {
                                lines.push(Line::from(Span::styled(
                                    format!("  team: {}", truncate_to(team, w.saturating_sub(8))),
                                    Style::default().fg(theme.muted),
                                )));
                            }
                        }
                    }
                    if info.active_agents.len() > 15 {
                        lines.push(Line::from(Span::styled(
                            format!("  ...+{} more", info.active_agents.len() - 15),
                            Style::default().fg(theme.muted),
                        )));
                    }
                }
            }
        }

        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, inner);
    }

    /// Check if the terminal is wide enough for the sidebar
    pub fn fits(total_width: u16) -> bool {
        total_width >= MIN_MAIN_WIDTH + SIDEBAR_WIDTH
    }

    /// Width the sidebar occupies (including border)
    pub fn width() -> u16 {
        SIDEBAR_WIDTH
    }
}

// Re-export the layout constants for use by MainLayoutWidget
pub(super) const MIN_MAIN_WIDTH_VAL: u16 = MIN_MAIN_WIDTH;
pub(super) const MIN_SIDEBAR_WIDTH_VAL: u16 = MIN_SIDEBAR_WIDTH;
pub(super) const COLLAPSE_HEADER_WIDTH_VAL: u16 = COLLAPSE_HEADER_WIDTH;
pub(super) const MIN_TERMINAL_WIDTH_VAL: u16 = MIN_TERMINAL_WIDTH;
pub(super) const MIN_TERMINAL_HEIGHT_VAL: u16 = MIN_TERMINAL_HEIGHT;
