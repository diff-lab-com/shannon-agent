//! Agent dashboard widget — bottom status bar + expanded overlay.
//!
//! Shows a 1-line agent bar (auto-visible when agents exist) and a full-screen
//! overlay (Ctrl+A) for detailed agent status and output.

use crate::theme::Theme;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

/// Maximum lines kept per agent's output ring buffer.
const MAX_OUTPUT_LINES: usize = 100;

/// Per-agent data for the dashboard.
#[derive(Debug, Clone)]
pub struct AgentEntry {
    pub name: String,
    pub status: String,
    pub active: bool,
    pub team: Option<String>,
    pub turns_used: u32,
    pub max_turns: u32,
    /// One-line summary of last action (from SubAgent.last_output).
    pub last_action: String,
    /// Ring buffer of recent output lines.
    pub recent_lines: Vec<String>,
}

/// Dashboard view mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DashboardMode {
    #[default]
    /// Collapsed: 1-line bottom bar only.
    Bar,
    /// Expanded: multi-line overlay listing all agents.
    List,
    /// Expanded: single agent full output, scrollable.
    Detail,
}

/// State for the agent dashboard.
#[derive(Debug, Clone, Default)]
pub struct AgentDashboardState {
    pub entries: Vec<AgentEntry>,
    pub expanded: bool,
    pub selected_index: usize,
    pub mode: DashboardMode,
    pub scroll_offset: u16,
}

impl AgentDashboardState {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            expanded: false,
            selected_index: 0,
            mode: DashboardMode::Bar,
            scroll_offset: 0,
        }
    }

    /// Sync basic fields from the REPL's active_agents list.
    pub fn sync_from_agents(&mut self, agents: &[crate::repl::AgentDisplay]) {
        // Update existing entries or add new ones
        for agent in agents {
            if let Some(entry) = self.entries.iter_mut().find(|e| e.name == agent.name) {
                entry.status = agent.status.clone();
                entry.active = agent.active;
                entry.team = agent.team.clone();
                entry.turns_used = agent.turns_used;
                entry.max_turns = agent.max_turns;
            } else {
                self.entries.push(AgentEntry {
                    name: agent.name.clone(),
                    status: agent.status.clone(),
                    active: agent.active,
                    team: agent.team.clone(),
                    turns_used: agent.turns_used,
                    max_turns: agent.max_turns,
                    last_action: String::new(),
                    recent_lines: Vec::new(),
                });
            }
        }
        // Remove entries for agents that no longer exist
        let names: std::collections::HashSet<String> =
            agents.iter().map(|a| a.name.clone()).collect();
        self.entries.retain(|e| names.contains(&e.name));
        // Clamp selected_index
        if !self.entries.is_empty() {
            self.selected_index = self.selected_index.min(self.entries.len() - 1);
        }
    }

    /// Handle a coordinator event — append output chunks.
    pub fn handle_coordinator_event(&mut self, event: &shannon_agents::CoordinatorEvent) {
        match event {
            shannon_agents::CoordinatorEvent::AgentOutput { agent, chunk, .. } => {
                if let Some(entry) = self.entries.iter_mut().find(|e| e.name == *agent) {
                    for line in chunk.lines() {
                        if !line.is_empty() {
                            if entry.recent_lines.len() >= MAX_OUTPUT_LINES {
                                entry.recent_lines.remove(0);
                            }
                            entry.recent_lines.push(line.to_string());
                        }
                    }
                    // Update last_action from the latest line
                    if let Some(last) = entry.recent_lines.last() {
                        entry.last_action = truncate_str(last, 40).to_string();
                    }
                }
            }
            shannon_agents::CoordinatorEvent::StatusChanged { agent, status } => {
                if let Some(entry) = self.entries.iter_mut().find(|e| e.name == *agent) {
                    entry.status = format!("{status:?}");
                }
            }
            shannon_agents::CoordinatorEvent::AgentCompleted {
                agent, success, output, ..
            } => {
                if let Some(entry) = self.entries.iter_mut().find(|e| e.name == *agent) {
                    entry.active = false;
                    entry.status = if *success {
                        "completed".to_string()
                    } else {
                        "failed".to_string()
                    };
                    if !output.is_empty() {
                        entry.last_action = truncate_str(output, 60).to_string();
                    }
                }
            }
            _ => {}
        }
    }

    /// Render the 1-line bottom bar.
    pub fn render_bar(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if self.entries.is_empty() || area.width < 10 {
            return;
        }
        let mut spans: Vec<Span> = Vec::new();
        let sep = Span::styled(" │ ", Style::default().fg(theme.muted));

        for (i, entry) in self.entries.iter().enumerate() {
            if i > 0 {
                spans.push(sep.clone());
            }
            let (icon, color) = status_icon_and_color(&entry.status, theme);
            // Highlight selected agent
            let name_style = if i == self.selected_index {
                Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(color)
            };
            spans.push(Span::styled(icon.to_string(), Style::default().fg(color)));
            spans.push(Span::styled(" ", Style::default()));
            spans.push(Span::styled(truncate_str(&entry.name, 12).to_string(), name_style));
            if !entry.last_action.is_empty() {
                let remaining = (area.width as usize).saturating_sub(spans.iter().map(|s| s.content.chars().count()).sum::<usize>());
                if remaining > 4 {
                    spans.push(Span::styled(
                        format!(" {}", truncate_str(&entry.last_action, remaining.min(20))),
                        Style::default().fg(theme.text_dim),
                    ));
                }
            }
        }

        let bar = Paragraph::new(Line::from(spans))
            .style(Style::default().bg(theme.context_bar_bg));
        frame.render_widget(Clear, area);
        frame.render_widget(bar, area);
    }

    /// Render the full-screen overlay (list mode).
    pub fn render_list_overlay(&self, frame: &mut Frame, _area: Rect, theme: &Theme) {
        let terminal = frame.area();
        let panel_height = terminal.height.saturating_sub(4).min(30).max(10);
        let panel_width = terminal.width.saturating_sub(4).min(90).max(40);
        let x = terminal.x + (terminal.width.saturating_sub(panel_width)) / 2;
        let y = terminal.y + (terminal.height.saturating_sub(panel_height)) / 2;
        let panel_area = Rect {
            x,
            y,
            width: panel_width,
            height: panel_height,
        };

        let active_count = self.entries.iter().filter(|e| e.active).count();
        let total = self.entries.len();
        let title = format!(" Agents ({active_count}/{total} active) ");

        let w = (panel_width as usize).saturating_sub(4);
        let mut lines: Vec<Line> = Vec::new();

        if self.entries.is_empty() {
            lines.push(Line::from(Span::styled(
                "No agents running",
                Style::default().fg(theme.muted),
            )));
        } else {
            for (i, entry) in self.entries.iter().enumerate() {
                let (icon, color) = status_icon_and_color(&entry.status, theme);
                let selected = i == self.selected_index;

                // Name + status line
                let name_style = if selected {
                    Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.text)
                };
                let prefix = if selected { "▸ " } else { "  " };
                let turns = if entry.max_turns > 0 {
                    format!("  turns: {}/{}", entry.turns_used, entry.max_turns)
                } else {
                    String::new()
                };
                let turns_w = turns.len();
                let name_w = w.saturating_sub(4 + turns_w);
                lines.push(Line::from(vec![
                    Span::styled(prefix.to_string(), Style::default().fg(theme.accent)),
                    Span::styled(icon.to_string(), Style::default().fg(color)),
                    Span::styled(" ", Style::default()),
                    Span::styled(truncate_str(&entry.name, name_w).to_string(), name_style),
                    Span::styled(turns, Style::default().fg(theme.text_dim)),
                ]));

                // Status + last action
                let detail = if entry.last_action.is_empty() {
                    format!("  {}", entry.status)
                } else {
                    format!("  {} — {}", entry.status, truncate_str(&entry.last_action, w.saturating_sub(6)))
                };
                lines.push(Line::from(Span::styled(
                    truncate_str(&detail, w).to_string(),
                    Style::default().fg(theme.text_dim),
                )));

                // Recent output (last 2 lines)
                let visible_lines = entry.recent_lines.iter().rev().take(2).collect::<Vec<_>>();
                for line in visible_lines.into_iter().rev() {
                    lines.push(Line::from(Span::styled(
                        format!("  ▸ {}", truncate_str(line, w.saturating_sub(4))),
                        Style::default().fg(theme.text_dim),
                    )));
                }
                lines.push(Line::from(""));
            }
        }

        // Footer with shortcuts
        lines.push(Line::from(vec![
            Span::styled(" ↑↓", Style::default().fg(theme.primary)),
            Span::styled(" navigate  ", Style::default().fg(theme.text_dim)),
            Span::styled("Enter", Style::default().fg(theme.primary)),
            Span::styled(" detail  ", Style::default().fg(theme.text_dim)),
            Span::styled("q/Esc", Style::default().fg(theme.primary)),
            Span::styled(" close", Style::default().fg(theme.text_dim)),
        ]));

        let panel = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border))
                    .title(title)
                    .title_style(Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(Clear, panel_area);
        frame.render_widget(panel, panel_area);
    }

    /// Render the detail view for the selected agent.
    pub fn render_detail_overlay(&self, frame: &mut Frame, _area: Rect, theme: &Theme) {
        let terminal = frame.area();
        let panel_height = terminal.height.saturating_sub(4).min(40).max(15);
        let panel_width = terminal.width.saturating_sub(4).min(90).max(40);
        let x = terminal.x + (terminal.width.saturating_sub(panel_width)) / 2;
        let y = terminal.y + (terminal.height.saturating_sub(panel_height)) / 2;
        let panel_area = Rect {
            x,
            y,
            width: panel_width,
            height: panel_height,
        };

        let entry = match self.entries.get(self.selected_index) {
            Some(e) => e,
            None => return,
        };

        let title = format!(" Agent: {} ", entry.name);
        let w = (panel_width as usize).saturating_sub(4);
        let mut lines: Vec<Line> = Vec::new();

        // Status header
        let (icon, color) = status_icon_and_color(&entry.status, theme);
        lines.push(Line::from(vec![
            Span::styled(format!("{icon} "), Style::default().fg(color)),
            Span::styled(entry.status.clone(), Style::default().fg(theme.text)),
            if entry.max_turns > 0 {
                Span::styled(
                    format!("  turns: {}/{}", entry.turns_used, entry.max_turns),
                    Style::default().fg(theme.text_dim),
                )
            } else {
                Span::styled(String::new(), Style::default())
            },
            if let Some(ref team) = entry.team {
                if team != "_global" {
                    Span::styled(format!("  team: {team}"), Style::default().fg(theme.muted))
                } else {
                    Span::styled(String::new(), Style::default())
                }
            } else {
                Span::styled(String::new(), Style::default())
            },
        ]));
        lines.push(Line::from(Span::styled(
            "─".repeat(w.min(60)),
            Style::default().fg(theme.border_dim),
        )));

        // Output lines (scrollable)
        let inner_height = panel_height.saturating_sub(6) as usize; // header + footer
        let total = entry.recent_lines.len();
        let max_scroll = total.saturating_sub(inner_height);
        let scroll = (self.scroll_offset as usize).min(max_scroll);
        let visible = entry.recent_lines.iter().skip(scroll).take(inner_height);
        for line in visible {
            lines.push(Line::from(Span::styled(
                truncate_str(line, w).to_string(),
                Style::default().fg(theme.text),
            )));
        }
        // Pad remaining lines
        let shown = entry.recent_lines.iter().skip(scroll).take(inner_height).count();
        for _ in shown..inner_height {
            lines.push(Line::from(""));
        }

        // Footer
        lines.push(Line::from(vec![
            Span::styled(" ↑↓", Style::default().fg(theme.primary)),
            Span::styled(" scroll  ", Style::default().fg(theme.text_dim)),
            Span::styled("Esc", Style::default().fg(theme.primary)),
            Span::styled(" back  ", Style::default().fg(theme.text_dim)),
            Span::styled("q", Style::default().fg(theme.primary)),
            Span::styled(" close", Style::default().fg(theme.text_dim)),
        ]));

        let panel = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border))
                    .title(title)
                    .title_style(Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(Clear, panel_area);
        frame.render_widget(panel, panel_area);
    }

    /// Move selection up.
    pub fn select_prev(&mut self) {
        if !self.entries.is_empty() {
            self.selected_index = self.selected_index.saturating_sub(1);
            self.scroll_offset = 0;
        }
    }

    /// Move selection down.
    pub fn select_next(&mut self) {
        if !self.entries.is_empty() {
            self.selected_index = (self.selected_index + 1).min(self.entries.len() - 1);
            self.scroll_offset = 0;
        }
    }

    /// Scroll output down in detail mode.
    pub fn scroll_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_add(3);
    }

    /// Scroll output up in detail mode.
    pub fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(3);
    }

    /// Toggle expanded state (Bar ↔ List).
    pub fn toggle_expand(&mut self) {
        if self.expanded {
            self.expanded = false;
            self.mode = DashboardMode::Bar;
        } else {
            self.expanded = true;
            self.mode = DashboardMode::List;
        }
    }

    /// Enter detail mode for selected agent.
    pub fn enter_detail(&mut self) {
        if !self.entries.is_empty() {
            self.mode = DashboardMode::Detail;
            self.scroll_offset = 0;
        }
    }

    /// Exit detail mode back to list.
    pub fn exit_detail(&mut self) {
        self.mode = DashboardMode::List;
        self.scroll_offset = 0;
    }

    /// Close the expanded overlay.
    pub fn close(&mut self) {
        self.expanded = false;
        self.mode = DashboardMode::Bar;
        self.scroll_offset = 0;
    }
}

/// Get status icon character and color for an agent status string.
fn status_icon_and_color(status: &str, theme: &Theme) -> (char, ratatui::style::Color) {
    match status {
        "running" | "Running" => ('●', theme.success),
        "spawning" | "Spawning" => ('◐', theme.warning),
        "idle" | "Idle" => ('○', theme.muted),
        "completed" | "Completed" => ('✓', theme.secondary),
        s if s.starts_with("failed") || s.starts_with("Failed") || s.starts_with("Error") => {
            ('✗', theme.error)
        }
        _ => ('·', theme.text_dim),
    }
}

/// Truncate a string to max_len characters, appending "…" if truncated.
fn truncate_str(s: &str, max_len: usize) -> std::borrow::Cow<'_, str> {
    if s.chars().count() <= max_len {
        std::borrow::Cow::Borrowed(s)
    } else {
        let truncated: String = s.chars().take(max_len.saturating_sub(1)).collect();
        std::borrow::Cow::Owned(format!("{truncated}…"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_str_short() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_str_exact() {
        assert_eq!(truncate_str("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_str_long() {
        let result = truncate_str("hello world", 6);
        assert_eq!(result, "hello…");
    }

    #[test]
    fn test_dashboard_new() {
        let d = AgentDashboardState::new();
        assert!(d.entries.is_empty());
        assert!(!d.expanded);
        assert_eq!(d.mode, DashboardMode::Bar);
    }

    #[test]
    fn test_dashboard_sync_from_agents() {
        let mut d = AgentDashboardState::new();
        let agents = vec![crate::repl::AgentDisplay {
            name: "researcher".to_string(),
            status: "running".to_string(),
            active: true,
            team: Some("team-a".to_string()),
            turns_used: 3,
            max_turns: 20,
        }];
        d.sync_from_agents(&agents);
        assert_eq!(d.entries.len(), 1);
        assert_eq!(d.entries[0].name, "researcher");
        assert_eq!(d.entries[0].turns_used, 3);
    }

    #[test]
    fn test_dashboard_sync_updates_existing() {
        let mut d = AgentDashboardState::new();
        let agents = vec![crate::repl::AgentDisplay {
            name: "researcher".to_string(),
            status: "running".to_string(),
            active: true,
            team: None,
            turns_used: 3,
            max_turns: 20,
        }];
        d.sync_from_agents(&agents);
        // Update with new turn count
        let agents2 = vec![crate::repl::AgentDisplay {
            name: "researcher".to_string(),
            status: "idle".to_string(),
            active: true,
            team: None,
            turns_used: 5,
            max_turns: 20,
        }];
        d.sync_from_agents(&agents2);
        assert_eq!(d.entries.len(), 1);
        assert_eq!(d.entries[0].turns_used, 5);
        assert_eq!(d.entries[0].status, "idle");
    }

    #[test]
    fn test_dashboard_sync_removes_missing() {
        let mut d = AgentDashboardState::new();
        let agents = vec![
            crate::repl::AgentDisplay {
                name: "a".to_string(),
                status: "running".to_string(),
                active: true,
                team: None,
                turns_used: 1,
                max_turns: 10,
            },
            crate::repl::AgentDisplay {
                name: "b".to_string(),
                status: "running".to_string(),
                active: true,
                team: None,
                turns_used: 2,
                max_turns: 10,
            },
        ];
        d.sync_from_agents(&agents);
        assert_eq!(d.entries.len(), 2);
        // Remove agent "a"
        let agents2 = vec![crate::repl::AgentDisplay {
            name: "b".to_string(),
            status: "running".to_string(),
            active: true,
            team: None,
            turns_used: 2,
            max_turns: 10,
        }];
        d.sync_from_agents(&agents2);
        assert_eq!(d.entries.len(), 1);
        assert_eq!(d.entries[0].name, "b");
    }

    #[test]
    fn test_dashboard_navigation() {
        let mut d = AgentDashboardState::new();
        let agents = vec![
            crate::repl::AgentDisplay {
                name: "a".to_string(),
                status: "running".to_string(),
                active: true,
                team: None,
                turns_used: 1,
                max_turns: 10,
            },
            crate::repl::AgentDisplay {
                name: "b".to_string(),
                status: "running".to_string(),
                active: true,
                team: None,
                turns_used: 2,
                max_turns: 10,
            },
            crate::repl::AgentDisplay {
                name: "c".to_string(),
                status: "running".to_string(),
                active: true,
                team: None,
                turns_used: 3,
                max_turns: 10,
            },
        ];
        d.sync_from_agents(&agents);
        assert_eq!(d.selected_index, 0);
        d.select_next();
        assert_eq!(d.selected_index, 1);
        d.select_next();
        assert_eq!(d.selected_index, 2);
        d.select_next(); // should clamp at last
        assert_eq!(d.selected_index, 2);
        d.select_prev();
        assert_eq!(d.selected_index, 1);
        d.select_prev();
        assert_eq!(d.selected_index, 0);
        d.select_prev(); // should clamp at 0
        assert_eq!(d.selected_index, 0);
    }

    #[test]
    fn test_dashboard_toggle_expand() {
        let mut d = AgentDashboardState::new();
        assert!(!d.expanded);
        d.toggle_expand();
        assert!(d.expanded);
        assert_eq!(d.mode, DashboardMode::List);
        d.toggle_expand();
        assert!(!d.expanded);
        assert_eq!(d.mode, DashboardMode::Bar);
    }

    #[test]
    fn test_dashboard_detail_mode() {
        let mut d = AgentDashboardState::new();
        let agents = vec![crate::repl::AgentDisplay {
            name: "a".to_string(),
            status: "running".to_string(),
            active: true,
            team: None,
            turns_used: 1,
            max_turns: 10,
        }];
        d.sync_from_agents(&agents);
        d.toggle_expand();
        d.enter_detail();
        assert_eq!(d.mode, DashboardMode::Detail);
        d.exit_detail();
        assert_eq!(d.mode, DashboardMode::List);
        d.close();
        assert_eq!(d.mode, DashboardMode::Bar);
    }

    #[test]
    fn test_ring_buffer_eviction() {
        let mut entry = AgentEntry {
            name: "test".to_string(),
            status: "running".to_string(),
            active: true,
            team: None,
            turns_used: 0,
            max_turns: 10,
            last_action: String::new(),
            recent_lines: Vec::new(),
        };
        // Fill beyond max
        for i in 0..(MAX_OUTPUT_LINES + 20) {
            if entry.recent_lines.len() >= MAX_OUTPUT_LINES {
                entry.recent_lines.remove(0);
            }
            entry.recent_lines.push(format!("line {i}"));
        }
        assert_eq!(entry.recent_lines.len(), MAX_OUTPUT_LINES);
        // Should have the latest lines
        assert_eq!(entry.recent_lines[0], format!("line {}", 20));
    }

    #[test]
    fn test_handle_coordinator_output_event() {
        use shannon_agents::CoordinatorEvent;
        let mut d = AgentDashboardState::new();
        let agents = vec![crate::repl::AgentDisplay {
            name: "researcher".to_string(),
            status: "running".to_string(),
            active: true,
            team: None,
            turns_used: 1,
            max_turns: 10,
        }];
        d.sync_from_agents(&agents);

        let event = CoordinatorEvent::AgentOutput {
            team: "team".to_string(),
            agent: "researcher".to_string(),
            chunk: "Found 3 results\nSearching auth module".to_string(),
        };
        d.handle_coordinator_event(&event);
        assert_eq!(d.entries[0].recent_lines.len(), 2);
        assert_eq!(d.entries[0].last_action, "Searching auth module");
    }

    #[test]
    fn test_handle_coordinator_completed_event() {
        use shannon_agents::CoordinatorEvent;
        let mut d = AgentDashboardState::new();
        let agents = vec![crate::repl::AgentDisplay {
            name: "tester".to_string(),
            status: "running".to_string(),
            active: true,
            team: None,
            turns_used: 5,
            max_turns: 10,
        }];
        d.sync_from_agents(&agents);

        let event = CoordinatorEvent::AgentCompleted {
            team: "team".to_string(),
            agent: "tester".to_string(),
            success: true,
            output: "All 12 tests passing".to_string(),
        };
        d.handle_coordinator_event(&event);
        assert_eq!(d.entries[0].status, "completed");
        assert!(!d.entries[0].active);
        assert_eq!(d.entries[0].last_action, "All 12 tests passing");
    }

    #[test]
    fn test_status_icon_and_color() {
        use crate::theme::Theme;
        let theme = Theme::default();
        let (icon, _) = status_icon_and_color("running", &theme);
        assert_eq!(icon, '●');
        let (icon, _) = status_icon_and_color("idle", &theme);
        assert_eq!(icon, '○');
        let (icon, _) = status_icon_and_color("completed", &theme);
        assert_eq!(icon, '✓');
        let (icon, _) = status_icon_and_color("failed", &theme);
        assert_eq!(icon, '✗');
    }
}
