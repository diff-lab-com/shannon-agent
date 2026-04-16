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

        // Overlay completion suggestions popup above the prompt
        if !state.completion_suggestions.is_empty() {
            render_completion_suggestions(f, f.area(), &state.completion_suggestions, state.completion_suggestion_index);
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
    // Calculate dialog area (centered) — taller if diff preview present
    let base_height: u16 = if dialog.diff_preview.is_some() { 30 } else { 20 };
    let dialog_width = 70.min(area.width.saturating_sub(4));
    let dialog_height = base_height.min(area.height.saturating_sub(4));

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
    ];

    // Show diff preview for file edit/write, raw input for other tools
    if let Some(ref diff) = dialog.diff_preview {
        content_lines.push(Line::from(vec![
            Span::styled("-- Diff Preview ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled("--------------------------", Style::default().fg(Color::DarkGray)),
        ]));
        let diff_lines: Vec<&str> = diff.lines().collect();
        for line in diff_lines.iter().take(15) {
            let color = if line.starts_with('-') && !line.starts_with("---") {
                Color::Red
            } else if line.starts_with('+') && !line.starts_with("+++") {
                Color::Green
            } else if line.starts_with('@') {
                Color::Cyan
            } else {
                Color::White
            };
            content_lines.push(Line::from(Span::styled(line.to_string(), Style::default().fg(color))));
        }
        if diff_lines.len() > 15 {
            content_lines.push(Line::from(Span::styled(
                format!("... ({} more lines)", diff_lines.len().saturating_sub(15)),
                Style::default().fg(Color::DarkGray),
            )));
        }
    } else {
        content_lines.push(Line::from("Input:"));
        content_lines.push(Line::from(serde_json::to_string_pretty(&dialog.tool_input).unwrap_or_else(|_| "(invalid)".to_string()).to_string()));
    }

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

/// Render a completion suggestions popup above the prompt area.
fn render_completion_suggestions(
    frame: &mut ratatui::Frame,
    area: Rect,
    suggestions: &[String],
    selected_index: usize,
) {
    let max_visible = 8u16;
    let visible = max_visible.min(suggestions.len() as u16);
    if visible == 0 {
        return;
    }

    let popup_height = visible + 2; // +2 for borders
    let popup_width = 40u16.min(area.width.saturating_sub(4));

    // Position just above the bottom status/prompt area (approx 4 lines from bottom)
    let y = area.bottom().saturating_sub(popup_height + 4);
    let x = area.x + 1;

    let popup_area = Rect {
        x,
        y,
        width: popup_width,
        height: popup_height,
    };

    frame.render_widget(Clear, popup_area);

    let lines: Vec<Line> = suggestions
        .iter()
        .take(visible as usize)
        .enumerate()
        .map(|(i, s)| {
            let text = truncate_visual(s, (popup_width - 4) as usize);
            if i == selected_index {
                Line::from(Span::styled(
                    format!("▶ {text}"),
                    Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD),
                ))
            } else {
                Line::from(Span::styled(
                    format!("  {text}"),
                    Style::default().fg(Color::Cyan),
                ))
            }
        })
        .collect();

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .border_type(ratatui::widgets::BorderType::Rounded)
                .title(" Completions "),
        );

    frame.render_widget(paragraph, popup_area);
}

/// Truncate a string to fit within a visual width, appending "…" if truncated.
fn truncate_visual(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let mut end = max_len.saturating_sub(1);
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &s[..end])
    }
}
