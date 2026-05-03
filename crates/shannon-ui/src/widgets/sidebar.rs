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
    /// LSP diagnostics for the Context tab
    pub diagnostics: Vec<crate::lsp_bridge::Diagnostic>,
    /// Session duration in seconds
    pub session_duration_secs: u64,
    /// Number of turns (user queries) in this session
    pub turn_count: usize,
    /// Total commands run
    pub commands_run: usize,
    /// Tokens per second (if measurable)
    pub tokens_per_sec: Option<f64>,
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
        let ctx_label = " Ctx ";
        let files_label = " Files ";
        let agents_label = " Agents ";
        let perf_label = " Perf ";
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
        let perf_style = if tab == crate::repl::SidebarTab::Perf {
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
            Span::styled("|", sep),
            Span::styled(perf_label, perf_style),
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

                // Pressure level indicator
                let (level_label, level_color) = if pct > 95.0 {
                    ("EMERGENCY", theme.error)
                } else if pct > 85.0 {
                    ("CRITICAL", theme.error)
                } else if pct > 75.0 {
                    ("HIGH", theme.warning)
                } else if pct > 50.0 {
                    ("NORMAL", theme.text_dim)
                } else {
                    ("LOW", theme.success)
                };
                lines.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(level_label, Style::default().fg(level_color).add_modifier(Modifier::BOLD)),
                ]));
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

                // Diff stats
                if !info.modified_files.is_empty() {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled("Changes", Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD))));
                    lines.push(Line::from(vec![
                        Span::styled("+", Style::default().fg(theme.success)),
                        Span::styled(info.total_additions.to_string(), Style::default().fg(theme.success)),
                        Span::styled(" ", Style::default().fg(theme.text_dim)),
                        Span::styled("-", Style::default().fg(theme.error)),
                        Span::styled(info.total_deletions.to_string(), Style::default().fg(theme.error)),
                        Span::styled(format!("  ({} files)", info.modified_files.len()), Style::default().fg(theme.muted)),
                    ]));
                }

                // Diagnostics section
                if !info.diagnostics.is_empty() {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled("Diagnostics", Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD))));
                    let errs = info.diagnostics.iter().filter(|d| matches!(d.severity, super::super::lsp_bridge::DiagnosticSeverity::Error)).count();
                    let warns = info.diagnostics.iter().filter(|d| matches!(d.severity, super::super::lsp_bridge::DiagnosticSeverity::Warning)).count();
                    lines.push(Line::from(vec![
                        Span::styled(format!("{errs}"), Style::default().fg(theme.error)),
                        Span::styled("E ", Style::default().fg(theme.text_dim)),
                        Span::styled(format!("{warns}"), Style::default().fg(theme.warning)),
                        Span::styled("W", Style::default().fg(theme.text_dim)),
                    ]));
                    for diag in info.diagnostics.iter().take(8) {
                        let color = match diag.severity {
                            super::super::lsp_bridge::DiagnosticSeverity::Error => theme.error,
                            super::super::lsp_bridge::DiagnosticSeverity::Warning => theme.warning,
                            super::super::lsp_bridge::DiagnosticSeverity::Info => theme.primary,
                            super::super::lsp_bridge::DiagnosticSeverity::Hint => theme.text_dim,
                        };
                        let icon = match diag.severity {
                            super::super::lsp_bridge::DiagnosticSeverity::Error => "E",
                            super::super::lsp_bridge::DiagnosticSeverity::Warning => "W",
                            super::super::lsp_bridge::DiagnosticSeverity::Info => "I",
                            super::super::lsp_bridge::DiagnosticSeverity::Hint => "H",
                        };
                        let fname = diag.file_path.split('/').next_back().unwrap_or(&diag.file_path);
                        lines.push(Line::from(vec![
                            Span::styled(format!("[{icon}]"), Style::default().fg(color)),
                            Span::styled(format!(" {}", truncate_to(fname, w.saturating_sub(6))), Style::default().fg(theme.text_dim)),
                        ]));
                        lines.push(Line::from(Span::styled(
                            format!("  {}", truncate_to(&diag.message, w.saturating_sub(4))),
                            Style::default().fg(color),
                        )));
                    }
                    if info.diagnostics.len() > 8 {
                        lines.push(Line::from(Span::styled(
                            format!("  ...+{} more", info.diagnostics.len() - 8),
                            Style::default().fg(theme.muted),
                        )));
                    }
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
            crate::repl::SidebarTab::Perf => {
                // Session duration
                lines.push(Line::from(Span::styled("Session", Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD))));
                let dur = info.session_duration_secs;
                let dur_str = if dur >= 3600 {
                    format!("{}h {}m", dur / 3600, (dur % 3600) / 60)
                } else if dur >= 60 {
                    format!("{}m {}s", dur / 60, dur % 60)
                } else {
                    format!("{dur}s")
                };
                lines.push(Line::from(Span::styled(dur_str, Style::default().fg(theme.text))));
                lines.push(Line::from(""));

                // Throughput
                lines.push(Line::from(Span::styled("Throughput", Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD))));
                let tok_str = format_tokens(info.tokens_used);
                if let Some(tps) = info.tokens_per_sec {
                    lines.push(Line::from(vec![
                        Span::styled(tok_str, Style::default().fg(theme.text)),
                        Span::styled(format!(" ({tps:.0} tok/s)"), Style::default().fg(theme.text_dim)),
                    ]));
                } else {
                    lines.push(Line::from(Span::styled(tok_str, Style::default().fg(theme.text))));
                }
                // Turns and rate
                let turns_str = format!("{} turns", info.turn_count);
                let avg_dur = if info.turn_count > 0 && dur > 0 {
                    format!(" (~{}s/turn)", dur / info.turn_count as u64)
                } else {
                    String::new()
                };
                lines.push(Line::from(vec![
                    Span::styled(turns_str, Style::default().fg(theme.text)),
                    Span::styled(avg_dur, Style::default().fg(theme.text_dim)),
                ]));
                lines.push(Line::from(""));

                // Cost efficiency
                lines.push(Line::from(Span::styled("Cost", Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD))));
                lines.push(Line::from(Span::styled(format!("${:.4}", info.cost_usd), Style::default().fg(theme.warning))));
                if info.turn_count > 0 {
                    let per_turn = info.cost_usd / info.turn_count as f64;
                    lines.push(Line::from(Span::styled(
                        format!("  ${per_turn:.4}/turn"),
                        Style::default().fg(theme.text_dim),
                    )));
                }
                if info.tokens_used > 0 {
                    let per_tok = info.cost_usd / info.tokens_used as f64 * 1000.0;
                    lines.push(Line::from(Span::styled(
                        format!("  ${per_tok:.4}/1k tok"),
                        Style::default().fg(theme.text_dim),
                    )));
                }
                lines.push(Line::from(""));

                // Activity
                lines.push(Line::from(Span::styled("Activity", Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD))));
                lines.push(Line::from(vec![
                    Span::styled(format!("{}", info.tools_invoked), Style::default().fg(theme.text)),
                    Span::styled(" tools", Style::default().fg(theme.text_dim)),
                ]));
                lines.push(Line::from(vec![
                    Span::styled(format!("{}", info.commands_run), Style::default().fg(theme.text)),
                    Span::styled(" commands", Style::default().fg(theme.text_dim)),
                ]));
                if info.error_count > 0 {
                    lines.push(Line::from(Span::styled(
                        format!("{} errors", info.error_count),
                        Style::default().fg(theme.error),
                    )));
                }

                // Diff stats
                if !info.modified_files.is_empty() {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled("Changes", Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD))));
                    lines.push(Line::from(vec![
                        Span::styled("+", Style::default().fg(theme.success)),
                        Span::styled(info.total_additions.to_string(), Style::default().fg(theme.success)),
                        Span::styled(" ", Style::default().fg(theme.text_dim)),
                        Span::styled("-", Style::default().fg(theme.error)),
                        Span::styled(info.total_deletions.to_string(), Style::default().fg(theme.error)),
                        Span::styled(format!("  ({} files)", info.modified_files.len()), Style::default().fg(theme.muted)),
                    ]));
                    let chg_rate = if dur > 0 {
                        let lines_per_min = ((info.total_additions + info.total_deletions) as f64 / dur as f64) * 60.0;
                        format!("  {lines_per_min:.0} lines/min")
                    } else {
                        String::new()
                    };
                    lines.push(Line::from(Span::styled(chg_rate, Style::default().fg(theme.text_dim))));
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
                        // Status line with progress bar
                        let turns_label = if agent.max_turns > 0 {
                            let turns_pct = if agent.max_turns > 0 {
                                (agent.turns_used as f64 / agent.max_turns as f64).min(1.0)
                            } else {
                                0.0
                            };
                            let bar_w = w.saturating_sub(14).clamp(3, 8);
                            let filled = (turns_pct * bar_w as f64).round() as usize;
                            let turns_bar = format!(
                                "{}{}",
                                crate::a11y::bar_filled().repeat(filled),
                                crate::a11y::bar_empty().repeat(bar_w.saturating_sub(filled))
                            );
                            format!("  {}/{} {}", agent.turns_used, agent.max_turns, turns_bar)
                        } else {
                            format!("  {}", agent.status)
                        };
                        let turns_color = if agent.status == "running" {
                            theme.success
                        } else if agent.status.starts_with("failed") {
                            theme.error
                        } else {
                            theme.text_dim
                        };
                        lines.push(Line::from(Span::styled(
                            truncate_to(&turns_label, w),
                            Style::default().fg(turns_color),
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
