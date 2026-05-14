//! Agents dropdown panel — triggered by Ctrl+A
//!
//! Shows all active and recent sub-agents with their status, team, and turn usage.

use crate::theme::Theme;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem},
    Frame,
};

/// Renders an agents dropdown panel overlay.
pub fn render_agents_panel(
    frame: &mut Frame,
    area: Rect,
    agents: &[super::super::repl::state::AgentDisplay],
    theme: &Theme,
) {
    if agents.is_empty() {
        let popup = centered_rect(area, 40, 5);
        frame.render_widget(Clear, popup);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border_dim))
            .title(" Agents ")
            .title_style(Style::default().fg(theme.primary).add_modifier(Modifier::BOLD));
        let line = Line::from(Span::styled(
            "  No active agents",
            Style::default().fg(theme.text_dim),
        ));
        let para = ratatui::widgets::Paragraph::new(line).block(block);
        frame.render_widget(para, popup);
        return;
    }

    let max_h = (agents.len() as u16 + 2).min(area.height.saturating_sub(4));
    let popup = centered_rect(area, 50, max_h);

    frame.render_widget(Clear, popup);

    let items: Vec<ListItem> = agents
        .iter()
        .map(|a| {
            let (status_icon, status_color) = agent_status_style(&a.status, a.active, theme);
            let name = if a.name.len() > 20 {
                format!("{}…", &a.name[..19])
            } else {
                a.name.clone()
            };

            let mut spans = vec![
                Span::styled(format!("{status_icon} "), Style::default().fg(status_color)),
                Span::styled(name, Style::default().fg(if a.active { theme.text } else { theme.text_dim })),
            ];

            // Team
            if let Some(team) = &a.team {
                spans.push(Span::styled(
                    format!(" [{team}]"),
                    Style::default().fg(theme.secondary),
                ));
            }

            // Turn usage
            if a.max_turns > 0 {
                let pct = a.turns_used as f64 / a.max_turns as f64;
                let turn_color = if pct < 0.5 { theme.success } else if pct < 0.8 { theme.warning } else { theme.error };
                spans.push(Span::styled(
                    format!(" {}{}/{}", turn_icon(), a.turns_used, a.max_turns),
                    Style::default().fg(turn_color),
                ));
            }

            // Status text
            spans.push(Span::styled(
                format!(" {}", a.status),
                Style::default().fg(status_color),
            ));

            ListItem::new(Line::from(spans))
        })
        .collect();

    let active_count = agents.iter().filter(|a| a.active).count();
    let total = agents.len();
    let title = format!(" Agents ({active_count}/{total} active) ");

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_dim))
        .title(title)
        .title_style(Style::default().fg(theme.primary).add_modifier(Modifier::BOLD));

    let list = List::new(items).block(block);
    frame.render_widget(list, popup);
}

fn agent_status_style(status: &str, active: bool, theme: &Theme) -> (&'static str, ratatui::style::Color) {
    match status {
        "spawning" => ("\u{25CB}", theme.text_dim),        // ○
        "running" => ("\u{25CF}", theme.primary),          // ●
        "idle" => ("\u{25D0}", theme.warning),             // ◐
        "completed" => ("\u{2713}", theme.success),        // ✓
        s if s.starts_with("failed") => ("\u{2717}", theme.error), // ✗
        _ if active => ("\u{25CF}", theme.primary),        // ●
        _ => ("\u{25CB}", theme.text_dim),                 // ○
    }
}

fn turn_icon() -> &'static str {
    "\u{21BB}" // ↻
}

/// Calculate a centered rect within `area` with given width and height.
fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width.saturating_sub(2));
    let h = height.min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 3;
    Rect::new(x, y, w, h)
}
