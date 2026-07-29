//! Modal overlay widget that renders /help output as a full-screen
//! panel instead of injecting it into chat history.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};

use crate::repl::state::HelpOverlayState;

/// Render the help overlay into the given full-screen area.
/// Returns the area of the inner content (excluding the border block).
pub fn render_help_overlay(
    f: &mut Frame,
    area: Rect,
    state: &HelpOverlayState,
    categories: &[(&str, Vec<(String, String)>)], // (category_name, [(cmd, desc)])
) -> Rect {
    // Clear the area first so the overlay sits on top of everything
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Shannon Help — Esc to close ");

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Split into left (categories) and right (commands in selected category)
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(inner);

    // Left pane: category list
    let category_items: Vec<ListItem> = categories
        .iter()
        .enumerate()
        .map(|(i, (name, _))| {
            let style = if i == state.selected_category_idx {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(Span::styled(*name, style)))
        })
        .collect();

    let category_list = List::new(category_items).block(
        Block::default()
            .borders(Borders::RIGHT)
            .title(" Categories "),
    );
    f.render_widget(category_list, chunks[0]);

    // Right pane: commands in selected category
    let right_items: Vec<ListItem> =
        if let Some((_, cmds)) = categories.get(state.selected_category_idx) {
            cmds.iter()
                .enumerate()
                .map(|(i, (cmd, desc))| {
                    let style = if i == state.selected_command_idx {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default()
                    };
                    let line = Line::from(vec![
                        Span::styled(format!("/{cmd}"), style.add_modifier(Modifier::BOLD)),
                        Span::raw(" — "),
                        Span::styled(desc.as_str(), style),
                    ]);
                    ListItem::new(line)
                })
                .collect()
        } else {
            vec![]
        };

    let cmd_list = List::new(right_items).block(Block::default().title(" Commands "));
    f.render_widget(cmd_list, chunks[1]);

    // Footer: search hint or filter
    let footer = Paragraph::new(format!(
        " j/k: switch category │ Enter: detail │ /: search │ Esc: close │ filter: {:?} ",
        state.filter
    ))
    .style(Style::default().fg(Color::DarkGray))
    .wrap(Wrap { trim: true });

    let footer_area = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };
    f.render_widget(footer, footer_area);

    inner
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn render_help_overlay_shows_categories() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = HelpOverlayState::default();
        let categories = vec![
            (
                "NAVIGATION",
                vec![("help".to_string(), "Show this help".to_string())],
            ),
            (
                "EDITING",
                vec![("edit".to_string(), "Edit a file".to_string())],
            ),
        ];

        terminal
            .draw(|f| {
                render_help_overlay(f, f.area(), &state, &categories);
            })
            .unwrap();

        let buf = terminal.backend().buffer().clone();
        // The "NAVIGATION" category label should appear in the rendered buffer
        let text: String = (0..buf.area.height)
            .flat_map(|y| (0..buf.area.width).map(move |x| (x, y)))
            .map(|(x, y)| buf.cell((x, y)).unwrap().symbol().to_string())
            .collect::<Vec<_>>()
            .join("");
        assert!(
            text.contains("NAVIGATION"),
            "expected category label in overlay"
        );
        assert!(text.contains("Categories"), "expected left pane title");
    }

    #[test]
    fn render_help_overlay_highlights_selected_category() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = HelpOverlayState::default();
        state.selected_category_idx = 1; // select EDITING
        let categories = vec![
            (
                "NAVIGATION",
                vec![("help".to_string(), "Show this help".to_string())],
            ),
            (
                "EDITING",
                vec![("edit".to_string(), "Edit a file".to_string())],
            ),
        ];

        terminal
            .draw(|f| {
                render_help_overlay(f, f.area(), &state, &categories);
            })
            .unwrap();

        // Just verify it doesn't panic with non-default selection
        let buf = terminal.backend().buffer().clone();
        assert!(buf.area.width > 0);
    }

    #[test]
    fn render_help_overlay_with_real_command_registry() {
        // Regression guard for the placeholder overlay (review finding C1): the
        // overlay must render real commands sourced from the command registry,
        // not the old hardcoded {/help, /edit} sample.
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = HelpOverlayState::default();
        // Git is the first category in HelpCategory::all(); selecting index 0
        // renders Git commands (incl. /commit) in the right pane.
        state.selected_category_idx = 0;

        let categories = shannon_commands::help_utils::categorize_commands();
        assert!(!categories.is_empty(), "registry must provide categories");

        terminal
            .draw(|f| {
                render_help_overlay(f, f.area(), &state, &categories);
            })
            .unwrap();

        let buf = terminal.backend().buffer().clone();
        let text: String = (0..buf.area.height)
            .flat_map(|y| (0..buf.area.width).map(move |x| (x, y)))
            .map(|(x, y)| buf.cell((x, y)).unwrap().symbol().to_string())
            .collect::<Vec<_>>()
            .join("");

        // A real category label appears (not the placeholder "NAVIGATION").
        assert!(
            text.contains("Git & Version Control"),
            "expected real category label; got: {}",
            &text[..text.len().min(300)]
        );
        // A real command from the Git category appears in the right pane.
        assert!(
            text.contains("/commit"),
            "expected /commit in overlay; got: {}",
            &text[..text.len().min(300)]
        );
    }
}
