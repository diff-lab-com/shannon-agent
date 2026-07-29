//! First-screen status snapshot card: provider/model/tier + available providers/models.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardStatus {
    /// No provider connected.
    Unconfigured,
    /// At least one provider connected.
    Configured,
}

/// Render the status card. Caller provides the full area; card adapts
/// to width (single-line pill below 80 cols).
#[allow(clippy::too_many_arguments)] // KEEP: widget view data, all params co-rendered (matches status_bar.rs convention)
pub fn render_status_card(
    f: &mut Frame,
    area: Rect,
    status: CardStatus,
    active_provider: Option<&str>,
    active_model: Option<&str>,
    active_tier: Option<&str>,
    available: &[(String, Vec<String>)], // (provider_id, [model_ids])
    connected: &[&str],                  // provider ids with a stored credential/profile
) {
    let is_narrow = area.width < 80;

    if is_narrow {
        render_pill(f, area, status, active_provider, active_model, active_tier);
    } else {
        render_full(
            f,
            area,
            status,
            active_provider,
            active_model,
            active_tier,
            available,
            connected,
        );
    }
}

fn render_pill(
    f: &mut Frame,
    area: Rect,
    status: CardStatus,
    provider: Option<&str>,
    model: Option<&str>,
    tier: Option<&str>,
) {
    let line = match status {
        CardStatus::Unconfigured => Line::from(vec![
            Span::styled(" ⚠ ", Style::default().fg(Color::Yellow)),
            Span::styled(
                "No provider connected. Run /connect to get started.",
                Style::default().fg(Color::Yellow),
            ),
        ]),
        CardStatus::Configured => Line::from(vec![
            Span::styled(" Active: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("[{}]", provider.unwrap_or("?")),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                format!("[{}]", model.unwrap_or("?")),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                format!("Tier: [{}]", tier.unwrap_or("?")),
                Style::default().fg(Color::Cyan),
            ),
        ]),
    };
    f.render_widget(Paragraph::new(line).wrap(Wrap { trim: true }), area);
}

#[allow(clippy::too_many_arguments)] // KEEP: widget view data, all params co-rendered (matches status_bar.rs convention)
fn render_full(
    f: &mut Frame,
    area: Rect,
    status: CardStatus,
    provider: Option<&str>,
    model: Option<&str>,
    tier: Option<&str>,
    available: &[(String, Vec<String>)],
    connected: &[&str],
) {
    let is_connected = |id: &str| connected.contains(&id);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" Status ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // active line
            Constraint::Length(1), // available header
            Constraint::Min(0),    // available list
            Constraint::Length(1), // commands footer
        ])
        .split(inner);

    // Row 1: active
    let active_line = match status {
        CardStatus::Unconfigured => Line::from(vec![
            Span::styled(" ⚠ ", Style::default().fg(Color::Yellow)),
            Span::styled(
                "No provider connected. Run /connect to get started.",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        CardStatus::Configured => Line::from(vec![
            Span::styled("Active: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("[{}]", provider.unwrap_or("?")),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                format!("[{}]", model.unwrap_or("?")),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                format!("Tier: [{}]", tier.unwrap_or("?")),
                Style::default().fg(Color::Magenta),
            ),
        ]),
    };
    f.render_widget(Paragraph::new(active_line), chunks[0]);

    // Row 2: available providers header
    let connected_count = available.iter().filter(|(id, _)| is_connected(id)).count();
    let header = Line::from(vec![Span::styled(
        format!(
            "Available providers ({} connected / {} supported):",
            connected_count,
            available.len()
        ),
        Style::default().fg(Color::DarkGray),
    )]);
    f.render_widget(Paragraph::new(header), chunks[1]);

    // Row 3: provider list
    let items: Vec<ListItem> = available
        .iter()
        .map(|(id, models)| {
            let marker = if is_connected(id) { "●" } else { "○" };
            let model_list = models.join(" · ");
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("  {marker} "),
                    Style::default().fg(if is_connected(id) {
                        Color::Green
                    } else {
                        Color::DarkGray
                    }),
                ),
                Span::styled(
                    id.to_string(),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("  {model_list}"),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();
    f.render_widget(List::new(items), chunks[2]);

    // Row 4: commands footer
    let cmd_line = Line::from(vec![
        Span::styled("Commands: ", Style::default().fg(Color::DarkGray)),
        Span::styled("/connect", Style::default().fg(Color::Cyan)),
        Span::styled(" · ", Style::default().fg(Color::DarkGray)),
        Span::styled("/model", Style::default().fg(Color::Cyan)),
        Span::styled(" · ", Style::default().fg(Color::DarkGray)),
        Span::styled("/provider", Style::default().fg(Color::Cyan)),
        Span::styled(" · ", Style::default().fg(Color::DarkGray)),
        Span::styled("/profile", Style::default().fg(Color::Cyan)),
        Span::styled(" · ", Style::default().fg(Color::DarkGray)),
        Span::styled("/help", Style::default().fg(Color::Cyan)),
    ]);
    f.render_widget(Paragraph::new(cmd_line), chunks[3]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    fn buffer_text(buf: &Buffer) -> String {
        (0..buf.area.height)
            .flat_map(|y| (0..buf.area.width).map(move |x| (x, y)))
            .map(|(x, y)| buf.cell((x, y)).unwrap().symbol().to_string())
            .collect::<Vec<_>>()
            .join("")
    }

    #[test]
    fn empty_state_shows_warning() {
        let backend = TestBackend::new(100, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_status_card(
                    f,
                    f.area(),
                    CardStatus::Unconfigured,
                    None,
                    None,
                    None,
                    &[],
                    &[],
                );
            })
            .unwrap();
        let text = buffer_text(&terminal.backend().buffer().clone());
        assert!(
            text.contains("No provider connected"),
            "got: {}",
            &text[..text.len().min(200)]
        );
    }

    #[test]
    fn configured_state_shows_provider_model_tier() {
        let backend = TestBackend::new(100, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_status_card(
                    f,
                    f.area(),
                    CardStatus::Configured,
                    Some("anthropic"),
                    Some("claude-sonnet-4-20250514"),
                    Some("Standard"),
                    &[(
                        "anthropic".to_string(),
                        vec![
                            "claude-opus-4".to_string(),
                            "claude-sonnet-4".to_string(),
                            "claude-haiku-4-5".to_string(),
                        ],
                    )],
                    &["anthropic"],
                );
            })
            .unwrap();
        let text = buffer_text(&terminal.backend().buffer().clone());
        assert!(text.contains("anthropic"), "missing provider");
        assert!(text.contains("claude-sonnet-4"), "missing model");
        assert!(text.contains("Standard"), "missing tier");
    }

    #[test]
    fn narrow_terminal_collapses_to_pill() {
        let backend = TestBackend::new(60, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_status_card(
                    f,
                    f.area(),
                    CardStatus::Configured,
                    Some("anthropic"),
                    Some("claude-sonnet-4"),
                    Some("Standard"),
                    &[],
                    &["anthropic"],
                );
            })
            .unwrap();
        let text = buffer_text(&terminal.backend().buffer().clone());
        assert!(text.contains("anthropic"), "narrow pill missing provider");
        assert!(
            !text.contains("Available providers"),
            "narrow should not show full block"
        );
    }
}
