//! REPL frame rendering and dialog display

use crate::{
    Result,
};
use ratatui::backend::CrosstermBackend;
use ratatui::{
    Terminal,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use std::io;

use super::Repl;

/// Draw the main REPL frame, dispatching to the appropriate overlay.
pub fn draw_frame(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, repl: &mut Repl) -> Result<()> {
    let chat = &repl.chat;
    let prompt = &repl.prompt;
    let state = repl.state.clone();
    let spinner = &repl.state.spinner;

    terminal.draw(|f| {
        let pb = if state.progress_bar_visible {
            Some(&state.progress_bar)
        } else {
            None
        };

        // Determine which overlay to render
        if let Some(ref dialog) = state.permission_dialog {
            render_permission_dialog(f, f.area(), dialog);
        } else if state.active_dialog.is_some()
            || state.input_dialog.is_some()
            || state.fuzzy_picker.is_some()
            || state.file_selector.is_some()
            || state.multi_select.is_some()
        {
            // Render base layout first
            crate::widgets::MainLayoutWidget::render_complete_with_spinner(
                f, chat, prompt, &state.status,
                state.model.as_deref(), Some(state.tokens_used),
                &state.working_directory, Some(spinner), pb,
            );
            // Then render the active overlay
            if let Some(ref dialog) = state.active_dialog {
                dialog.render(f, f.area());
            } else if let Some(ref input_dlg) = state.input_dialog {
                input_dlg.render(f, f.area());
            } else if let Some(ref picker) = state.fuzzy_picker {
                picker.render(f, f.area());
            } else if let Some(ref selector) = state.file_selector {
                selector.render(f, f.area());
            } else if let Some(ref msel) = state.multi_select {
                msel.render(f, f.area());
            }
        } else {
            crate::widgets::MainLayoutWidget::render_complete_with_spinner(
                f, chat, prompt, &state.status,
                state.model.as_deref(), Some(state.tokens_used),
                &state.working_directory, Some(spinner), pb,
            );
        }

        // Overlay multi-progress bars at the bottom if active
        if state.multi_progress_visible {
            let mp_height = 3u16.min(f.area().height.saturating_sub(10));
            let mp_area = Rect {
                x: f.area().x + 2,
                y: f.area().bottom().saturating_sub(mp_height + 3),
                width: f.area().width.saturating_sub(4),
                height: mp_height,
            };
            state.multi_progress.render(f, mp_area);
        }
    })?;

    Ok(())
}

/// Render permission dialog
pub fn render_permission_dialog(
    frame: &mut ratatui::Frame,
    area: Rect,
    dialog: &shannon_core::permissions::PermissionPrompt,
) {
    // Calculate dialog area (centered)
    let dialog_width = 60.min(area.width.saturating_sub(4));
    let dialog_height = 20.min(area.height.saturating_sub(4));

    let x = (area.width.saturating_sub(dialog_width)) / 2;
    let y = (area.height.saturating_sub(dialog_height)) / 2;

    let dialog_area = Rect {
        x: area.x + x,
        y: area.y + y,
        width: dialog_width,
        height: dialog_height,
    };

    // Clear background for modal effect
    frame.render_widget(Clear, dialog_area);

    // Build dialog content
    let risk_indicator = match dialog.risk_level {
        shannon_core::permissions::RiskLevel::Safe => "✓",
        shannon_core::permissions::RiskLevel::Low => "⚠",
        shannon_core::permissions::RiskLevel::Medium => "⚡",
        shannon_core::permissions::RiskLevel::High => "🔥",
        shannon_core::permissions::RiskLevel::Critical => "☢️",
    };

    let risk_color = match dialog.risk_level {
        shannon_core::permissions::RiskLevel::Safe => Color::Green,
        shannon_core::permissions::RiskLevel::Low => Color::Yellow,
        shannon_core::permissions::RiskLevel::Medium => Color::Magenta,
        shannon_core::permissions::RiskLevel::High => Color::Red,
        shannon_core::permissions::RiskLevel::Critical => Color::Red,
    };

    let mut content_lines = vec![
        Line::from(vec![
            Span::styled(risk_indicator, Style::default().fg(risk_color).add_modifier(Modifier::BOLD)),
            Span::from(" Permission Request"),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Tool: ", Style::default().fg(Color::Gray)),
            Span::styled(&dialog.tool_name, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Description: ", Style::default().fg(Color::Gray)),
            Span::styled(&dialog.description, Style::default().fg(Color::White)),
        ]),
        Line::from(""),
        Line::from("Input:"),
        Line::from(serde_json::to_string_pretty(&dialog.tool_input).unwrap_or_else(|_| "(invalid)".to_string()).to_string()),
    ];

    // Add options
    content_lines.push(Line::from(""));
    content_lines.push(Line::from(""));
    content_lines.push(Line::from(vec![
        Span::styled("[Enter] ", Style::default().fg(Color::Green)),
        Span::styled("Allow Once    ", Style::default().fg(Color::White)),
        Span::styled("[A] ", Style::default().fg(Color::Cyan)),
        Span::styled("Always Allow  ", Style::default().fg(Color::White)),
        Span::styled("[Esc] ", Style::default().fg(Color::Red)),
        Span::styled("Deny", Style::default().fg(Color::White)),
    ]));

    let paragraph = Paragraph::new(content_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .border_type(ratatui::widgets::BorderType::Rounded)
                .title(" Permission Required "),
        )
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, dialog_area);
}
