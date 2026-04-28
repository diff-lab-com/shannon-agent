//! REPL frame rendering and dialog display

use crate::{
    Result,
    theme::Theme,
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
    let theme = repl.state.theme.clone();

    // Build sidebar info via Repl method (pub(crate) fields only accessible from mod.rs)
    let sidebar_info = repl.sidebar_info();

    // Clone diff viewer state and data for rendering inside closure
    let diff_viewer_state = repl.state.diff_viewer.clone();
    let diff_data = repl.diff_data.clone();

    // Sync terminal window title with current state (OSC 0)
    {
        let model_short = state.model.as_deref()
            .map(|m| m.split('/').last().unwrap_or(m))
            .unwrap_or("shannon");
        let icon = if state.streaming_active { "✦" } else { "◇" };
        let title = format!("{icon} Shannon — {model_short} — {}", state.status);
        let _ = crossterm::execute!(io::stdout(), crossterm::terminal::SetTitle(&title));
    }

    terminal.draw(|f| {
        let pb = if state.progress_bar_visible {
            Some(&state.progress_bar)
        } else {
            None
        };
        let sidebar_ref = sidebar_info.as_ref();

        // Build status with model, tokens, turn count, session duration, and notification badge
        let notif_count = state.pending_notifications.iter()
            .filter(|(_, t)| t.elapsed().as_secs() < 30)
            .count();
        let mut status_parts = Vec::new();

        // Model name
        if let Some(ref model) = state.model {
            let short = model.split('/').last().unwrap_or(model);
            status_parts.push(short.to_string());
        }

        // Status or Ready with turn count
        if state.turn_count > 0 && state.status == "Ready" {
            status_parts.push(format!("Turn: {}", state.turn_count));
        } else if state.status != "Ready" {
            status_parts.push(state.status.clone());
        }

        // Token usage
        if state.tokens_used > 0 {
            let k = state.tokens_used as f64 / 1000.0;
            status_parts.push(format!("{:.1}k tokens", k));
        }

        // Session duration
        if let Some(start) = state.session_start {
            let dur = start.elapsed();
            if dur.as_secs() >= 60 {
                let mins = dur.as_secs() / 60;
                let secs = dur.as_secs() % 60;
                status_parts.push(format!("{}m{}s", mins, secs));
            }
        }

        // Notification badge
        if notif_count > 0 {
            status_parts.push(format!("{} notif", notif_count));
        }

        // Streaming state indicator
        match &state.streaming_state {
            crate::widgets::StreamingState::Thinking => {
                status_parts.push("thinking".to_string());
            }
            crate::widgets::StreamingState::CallingTool { name } => {
                status_parts.push(format!("tool: {name}"));
            }
            crate::widgets::StreamingState::Generating { elapsed_secs } => {
                status_parts.push(format!("streaming {elapsed_secs}s"));
            }
            crate::widgets::StreamingState::Idle => {}
        }

        let display_status = if status_parts.is_empty() {
            "Ready".to_string()
        } else {
            status_parts.join(" │ ")
        };

        // Compute search matches if chat search is active
        let (search_query, search_matches, search_focused_idx) = if state.chat_search_active || !state.chat_search_query.is_empty() {
            let matches = chat.find_search_matches(&state.chat_search_query);
            let focused = if matches.is_empty() { None } else { Some(state.chat_search_match_index.min(matches.len() - 1)) };
            (Some(state.chat_search_query.clone()), matches, focused)
        } else {
            (None, Vec::new(), None)
        };

        // Determine which overlay to render
        if let Some(ref dialog) = state.permission_dialog {
            render_permission_dialog(f, f.area(), dialog, &theme);
        } else if state.active_dialog.is_some()
            || state.input_dialog.is_some()
            || state.fuzzy_picker.is_some()
            || state.file_selector.is_some()
            || state.multi_select.is_some()
            || state.model_picker.is_some()
        {
            // Render base layout first
            crate::widgets::MainLayoutWidget::render_complete_with_spinner(
                f, chat, prompt, &display_status,
                state.model.as_deref(), Some(state.tokens_used),
                &state.working_directory, Some(spinner), pb, sidebar_ref, &theme, state.sidebar_tab,
                Some(&state.approval_mode_label),
                state.focus_mode, state.fullscreen_mode,
                search_query.as_deref(), &search_matches, search_focused_idx,
                None, None, None, None,
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
            } else if let Some(ref mp) = state.model_picker {
                mp.render(f, f.area());
            }
        } else {
            crate::widgets::MainLayoutWidget::render_complete_with_spinner(
                f, chat, prompt, &display_status,
                state.model.as_deref(), Some(state.tokens_used),
                &state.working_directory, Some(spinner), pb, sidebar_ref, &theme, state.sidebar_tab,
                Some(&state.approval_mode_label),
                state.focus_mode, state.fullscreen_mode,
                search_query.as_deref(), &search_matches, search_focused_idx,
                None, None, None, None,
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

        // Overlay diff viewer if active
        if let Some(ref viewer) = diff_viewer_state {
            if state.diff_interactive && !state.interactive_hunks.is_empty() {
                viewer.render_interactive(f, f.area(), &diff_data, &theme, &state.interactive_hunks, state.interactive_selected);
            } else {
                viewer.render(f, f.area(), &diff_data, &theme);
            }
        }

        // Overlay toast notification if active
        if let Some((ref msg, started)) = state.toast {
            let elapsed = started.elapsed().as_secs();
            let toast_text = format!(" {msg} ({elapsed}s) ");
            let toast_width = toast_text.chars().count() as u16;
            let y = f.area().bottom().saturating_sub(5);
            let x = f.area().x + 1;
            let toast_area = ratatui::layout::Rect {
                x,
                y,
                width: toast_width.min(f.area().width.saturating_sub(2)),
                height: 1,
            };
            let toast = Paragraph::new(toast_text)
                .style(ratatui::style::Style::default().fg(theme.text).bg(theme.accent));
            f.render_widget(toast, toast_area);
        }

        // Overlay history search bar when Ctrl+R active
        if state.incremental_search_active {
            render_history_search_overlay(f, f.area(), &state);
        }

        // Overlay plan review when plan is active and not yet approved
        if state.plan.active && !state.plan.approved {
            render_plan_overlay(f, f.area(), &state.plan, &theme);
        }

        // Overlay completion suggestions popup above the prompt
        if !state.completion_suggestions.is_empty() {
            render_completion_suggestions(f, f.area(), &state.completion_suggestions, state.completion_suggestion_index);
        }

        // Overlay pager when active
        if state.pager_active {
            render_pager_overlay(f, f.area(), chat, &theme);
        }

        // Overlay onboarding dialog on first run
        if state.onboarding_active {
            render_onboarding_overlay(f, f.area(), &theme);
        }

        // Overlay tool approval widget when active
        if state.tool_approval.is_active() {
            state.tool_approval.render(f, f.area(), &theme);
        }

        // Overlay command palette when visible
        if let Some(ref palette) = state.command_palette {
            palette.render(f, f.area(), &theme);
        }

        // Render attachment bar above prompt area
        if !state.attachment_bar.is_empty() {
            let bar_height = 1u16;
            let bar_area = ratatui::layout::Rect {
                x: f.area().x + 1,
                y: f.area().bottom().saturating_sub(5),
                width: f.area().width.saturating_sub(2),
                height: bar_height,
            };
            state.attachment_bar.render(f, bar_area, &theme);
        }

        // Overlay fullscreen indicator in top-right corner
        if state.fullscreen_mode {
            let indicator = " [FS] ";
            let width = indicator.chars().count() as u16;
            let indicator_area = ratatui::layout::Rect {
                x: f.area().right().saturating_sub(width + 1),
                y: f.area().y,
                width,
                height: 1,
            };
            let ind = Paragraph::new(indicator)
                .style(ratatui::style::Style::default().fg(theme.primary).add_modifier(Modifier::BOLD));
            f.render_widget(ind, indicator_area);
        }

        // Overlay chat search bar when active
        if state.chat_search_active {
            render_chat_search_overlay(f, f.area(), &state, &theme);
        }

        // Render session tab bar at the very top when visible
        state.session_tab.render(f, f.area(), &theme);

        // Render key hints at the very bottom line when no overlay is active
        if state.permission_dialog.is_none()
            && state.active_dialog.is_none()
            && state.command_palette.is_none()
            && !state.pager_active
            && !state.chat_search_active
        {
            let hint_area = ratatui::layout::Rect {
                x: f.area().x,
                y: f.area().bottom().saturating_sub(1),
                width: f.area().width,
                height: 1,
            };
            let ctx = if state.streaming_active || state.thinking_phase {
                crate::widgets::key_hint::HintContext::Normal
            } else {
                crate::widgets::key_hint::HintContext::Input
            };
            crate::widgets::KeyHintWidget::render(f, hint_area, &theme, &ctx);
        }
    })?;

    Ok(())
}

/// Render permission dialog
pub fn render_permission_dialog(
    frame: &mut ratatui::Frame,
    area: Rect,
    dialog: &shannon_core::permissions::PermissionPrompt,
    theme: &Theme,
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
        shannon_core::permissions::RiskLevel::Safe => theme.success,
        shannon_core::permissions::RiskLevel::Low => theme.warning,
        shannon_core::permissions::RiskLevel::Medium => theme.accent,
        shannon_core::permissions::RiskLevel::High => theme.error,
        shannon_core::permissions::RiskLevel::Critical => theme.error,
    };

    let mut content_lines = vec![
        Line::from(vec![
            Span::styled(risk_indicator, Style::default().fg(risk_color).add_modifier(Modifier::BOLD)),
            Span::from(" Permission Request"),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Tool: ", Style::default().fg(theme.muted)),
            Span::styled(&dialog.tool_name, Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Description: ", Style::default().fg(theme.muted)),
            Span::styled(&dialog.description, Style::default().fg(theme.text)),
        ]),
        Line::from(""),
    ];

    // Show diff preview for file edit/write, raw input for other tools
    if let Some(ref diff) = dialog.diff_preview {
        content_lines.push(Line::from(vec![
            Span::styled("-- Diff Preview ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
            Span::styled("--------------------------", Style::default().fg(theme.text_dim)),
        ]));
        let diff_lines: Vec<&str> = diff.lines().collect();
        let lang = crate::widgets::detect_diff_language(diff);
        for line in diff_lines.iter().take(15) {
            let color = if line.starts_with('-') && !line.starts_with("---") {
                theme.diff_removed
            } else if line.starts_with('+') && !line.starts_with("+++") {
                theme.diff_added
            } else if line.starts_with('@') {
                theme.primary
            } else {
                theme.text
            };
            let word_color = if line.starts_with('+') && !line.starts_with("+++") {
                Some(theme.diff_added_word)
            } else if line.starts_with('-') && !line.starts_with("---") {
                Some(theme.diff_removed_word)
            } else {
                None
            };
            let spans = crate::widgets::highlight_diff_line(line, lang.as_deref(), color, word_color);
            content_lines.push(Line::from(spans));
        }
        if diff_lines.len() > 15 {
            content_lines.push(Line::from(Span::styled(
                format!("... ({} more lines)", diff_lines.len().saturating_sub(15)),
                Style::default().fg(theme.text_dim),
            )));
        }
    } else {
        content_lines.push(Line::from("Input:"));
        content_lines.push(Line::from(serde_json::to_string_pretty(&dialog.tool_input).unwrap_or_else(|_| "(invalid)".to_string()).to_string()));
    }

    // Show risk reason if available
    if !dialog.risk_reason.is_empty() {
        content_lines.push(Line::from(""));
        content_lines.push(Line::from(vec![
            Span::styled("Why: ", Style::default().fg(theme.muted)),
            Span::styled(&dialog.risk_reason, Style::default().fg(theme.text_dim)),
        ]));
    }

    // Add options
    content_lines.push(Line::from(""));
    content_lines.push(Line::from(vec![
        Span::styled("[Enter] ", Style::default().fg(theme.success)),
        Span::styled("Allow Once    ", Style::default().fg(theme.text)),
        Span::styled("[A] ", Style::default().fg(theme.primary)),
        Span::styled("Always Allow  ", Style::default().fg(theme.text)),
        Span::styled("[E] ", Style::default().fg(theme.warning)),
        Span::styled("Edit+Run  ", Style::default().fg(theme.text)),
        Span::styled("[Esc] ", Style::default().fg(theme.error)),
        Span::styled("Deny", Style::default().fg(theme.text)),
    ]));

    let paragraph = Paragraph::new(content_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.primary))
                .border_type(ratatui::widgets::BorderType::Rounded)
                .title(" Permission Required "),
        )
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, dialog_area);
}

/// Render a completion suggestions popup above the prompt area.
pub(crate) fn render_completion_suggestions(
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

/// Render a history search overlay bar at the bottom of the screen.
fn render_history_search_overlay(
    frame: &mut ratatui::Frame,
    area: Rect,
    state: &super::ReplState,
) {
    let bar_height = 3u16;
    let bar_width = area.width.saturating_sub(4).min(60);
    let y = area.bottom().saturating_sub(bar_height + 5);
    let x = (area.width.saturating_sub(bar_width)) / 2;

    let bar_area = Rect {
        x: area.x + x,
        y,
        width: bar_width,
        height: bar_height,
    };

    frame.render_widget(Clear, bar_area);

    let query_display = if state.incremental_search_query.is_empty() {
        "(type to search)".to_string()
    } else {
        state.incremental_search_query.clone()
    };

    let query_color = if state.incremental_search_query.is_empty() {
        Color::DarkGray
    } else {
        Color::Yellow
    };

    let lines = vec![
        Line::from(vec![
            Span::styled(" Ctrl+R ", Style::default().fg(Color::Black).bg(Color::Cyan)),
            Span::styled(" reverse-i-search  ", Style::default().fg(Color::DarkGray)),
            Span::styled(&query_display, Style::default().fg(query_color).add_modifier(Modifier::BOLD)),
            Span::styled("▌", Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled(" ↑↓ navigate  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Enter", Style::default().fg(Color::Green)),
            Span::styled(" accept  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc", Style::default().fg(Color::Red)),
            Span::styled(" cancel", Style::default().fg(Color::DarkGray)),
        ]),
    ];

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .border_type(ratatui::widgets::BorderType::Rounded),
        );

    frame.render_widget(paragraph, bar_area);
}

/// Render transcript pager overlay — full-screen conversation viewer.
fn render_pager_overlay(
    frame: &mut ratatui::Frame,
    area: Rect,
    chat: &crate::widgets::ChatWidget,
    theme: &Theme,
) {
    // Clear the entire area
    frame.render_widget(Clear, area);

    // Render chat content filling the full area minus the pager header/footer
    let pager_header = 1u16;
    let pager_footer = 1u16;
    let content_area = Rect {
        x: area.x,
        y: area.y + pager_header,
        width: area.width,
        height: area.height.saturating_sub(pager_header + pager_footer),
    };

    // Header bar
    let header = Paragraph::new(Line::from(vec![
        Span::styled(" TRANSCRIPT ", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
        Span::styled("─ j/k: scroll  g/G: top/bottom  q/Esc: close ", Style::default().fg(theme.text_dim)),
    ]))
    .style(Style::default().bg(theme.context_bar_bg));
    frame.render_widget(header, Rect { x: area.x, y: area.y, width: area.width, height: 1 });

    // Render chat widget content in the pager area
    chat.render(frame, content_area, theme);

    // Footer bar
    let footer = Paragraph::new(Line::from(Span::styled(
        " q: quit pager ",
        Style::default().fg(theme.text_dim),
    )))
    .style(Style::default().bg(theme.context_bar_bg));
    let footer_y = area.y + area.height.saturating_sub(1);
    frame.render_widget(footer, Rect { x: area.x, y: footer_y, width: area.width, height: 1 });
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

/// Render plan review overlay — shows pending plan with approve/reject options.
fn render_plan_overlay(
    frame: &mut ratatui::Frame,
    area: Rect,
    plan: &super::PlanState,
    theme: &Theme,
) {
    let dialog_width = 72.min(area.width.saturating_sub(4));
    let dialog_height = 24.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(dialog_width)) / 2;
    let y = (area.height.saturating_sub(dialog_height)) / 2;
    let dialog_area = Rect {
        x: area.x + x,
        y: area.y + y,
        width: dialog_width,
        height: dialog_height,
    };

    frame.render_widget(Clear, dialog_area);

    // Count numbered steps for the title
    let all_lines: Vec<&str> = plan.content.lines().collect();
    let step_count = all_lines.iter().filter(|l| l.starts_with("## ") || (l.starts_with(|c: char| c.is_ascii_digit()) && l.contains('.'))).count();
    let step_label = if step_count > 0 { format!(" ({step_count} steps)") } else { String::new() };

    let inner_width = dialog_width.saturating_sub(2) as usize;
    let mut content_lines = vec![
        Line::from(vec![
            Span::styled(" Goal: ", Style::default().fg(theme.muted)),
            Span::styled(truncate_visual(&plan.description, inner_width.saturating_sub(8)), Style::default().fg(theme.text)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            format!("{} Steps {}{}", "─".repeat(3), "─".repeat(inner_width.saturating_sub(12)), step_label),
            Style::default().fg(theme.text_dim),
        )),
    ];

    // Header takes 3 lines + 2 for spacing + 2 for action bar + 1 trailing
    let header_lines = 3;
    let footer_lines = 3;
    let available_body = dialog_height as usize - header_lines - footer_lines;

    // Collect step lines with styling
    let mut step_lines: Vec<Line> = Vec::new();
    let mut step_num = 0u32;
    for line in &all_lines {
        let styled = if line.starts_with("## ") {
            step_num += 1;
            let label = line.trim_start_matches("## ").trim();
            Line::from(vec![
                Span::styled(format!(" {step_num}. "), Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
                Span::styled(label.to_string(), Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
            ])
        } else if line.starts_with("- ") {
            Line::from(vec![
                Span::styled("    ", Style::default().fg(theme.text_dim)),
                Span::styled("• ", Style::default().fg(theme.muted)),
                Span::styled(line[2..].to_string(), Style::default().fg(theme.text)),
            ])
        } else if line.starts_with(|c: char| c.is_ascii_digit()) && line.contains('.') {
            step_num += 1;
            Line::from(vec![
                Span::styled(format!(" {step_num}. "), Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
                Span::styled(line.to_string(), Style::default().fg(theme.text)),
            ])
        } else if line.is_empty() {
            Line::raw("")
        } else {
            Line::from(Span::styled(
                truncate_visual(&format!("  {line}"), inner_width),
                Style::default().fg(theme.text_dim),
            ))
        };
        step_lines.push(styled);
    }

    // Apply scroll offset
    let total = step_lines.len();
    let scroll = plan.scroll_offset.min(total.saturating_sub(available_body));
    let visible: Vec<Line> = step_lines.into_iter().skip(scroll).take(available_body).collect();
    content_lines.extend(visible);

    // Scroll indicator
    let remaining_content = total.saturating_sub(scroll + available_body);
    if remaining_content > 0 || scroll > 0 {
        let pos_info = format!(" line {}-{} of {total} ", scroll + 1, (scroll + available_body).min(total));
        content_lines.push(Line::from(Span::styled(
            format!("  j/k: scroll {pos_info}"),
            Style::default().fg(theme.muted),
        )));
    } else {
        content_lines.push(Line::from(""));
    }

    content_lines.push(Line::from(vec![
        Span::styled("[Enter] ", Style::default().fg(theme.success)),
        Span::styled("Approve    ", Style::default().fg(theme.text)),
        Span::styled("[Esc] ", Style::default().fg(theme.warning)),
        Span::styled("Reject    ", Style::default().fg(theme.text)),
        Span::styled("[P] ", Style::default().fg(theme.muted)),
        Span::styled("Dismiss", Style::default().fg(theme.text)),
    ]));

    let title = format!(" Plan Awaiting Review{step_label} ");
    let paragraph = Paragraph::new(content_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.accent))
                .border_type(ratatui::widgets::BorderType::Rounded)
                .title(Span::styled(
                    title,
                    Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
                )),
        )
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, dialog_area);
}

/// Render first-run onboarding overlay showing essential keybindings and tips.
fn render_onboarding_overlay(
    frame: &mut ratatui::Frame,
    area: Rect,
    theme: &Theme,
) {
    let dialog_width = 60.min(area.width.saturating_sub(4));
    let dialog_height = 28.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(dialog_width)) / 2;
    let y = (area.height.saturating_sub(dialog_height)) / 2;
    let dialog_area = Rect {
        x: area.x + x,
        y: area.y + y,
        width: dialog_width,
        height: dialog_height,
    };

    frame.render_widget(Clear, dialog_area);

    let accent_style = Style::default().fg(theme.accent).add_modifier(Modifier::BOLD);
    let text_style = Style::default().fg(theme.text);

    let sep_width = dialog_width as usize - 4;
    let content_lines = vec![
        Line::from(Span::styled(
            " Welcome to Shannon Code",
            Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(" Essential Keybindings", Style::default().fg(theme.text_dim))),
        Line::from("─".repeat(sep_width)),
        Line::from(vec![Span::styled("  Enter           ", accent_style), Span::styled("Send message", text_style)]),
        Line::from(vec![Span::styled("  Ctrl+E          ", accent_style), Span::styled("Open external editor", text_style)]),
        Line::from(vec![Span::styled("  Ctrl+F          ", accent_style), Span::styled("Toggle focus mode", text_style)]),
        Line::from(vec![Span::styled("  F11             ", accent_style), Span::styled("Toggle fullscreen", text_style)]),
        Line::from(vec![Span::styled("  Ctrl+H          ", accent_style), Span::styled("Search chat messages", text_style)]),
        Line::from(vec![Span::styled("  Ctrl+G          ", accent_style), Span::styled("Transcript pager (j/k scroll)", text_style)]),
        Line::from(vec![Span::styled("  Ctrl+R          ", accent_style), Span::styled("Search command history", text_style)]),
        Line::from(vec![Span::styled("  Ctrl+X          ", accent_style), Span::styled("Leader key prefix", text_style)]),
        Line::from(vec![Span::styled("  Tab             ", accent_style), Span::styled("Autocomplete suggestions", text_style)]),
        Line::from(vec![Span::styled("  Esc             ", accent_style), Span::styled("Cancel / close dialog", text_style)]),
        Line::from(""),
        Line::from(Span::styled(" Commands (type in prompt)", Style::default().fg(theme.text_dim))),
        Line::from("─".repeat(sep_width)),
        Line::from(vec![Span::styled("  /help           ", accent_style), Span::styled("Show all commands", text_style)]),
        Line::from(vec![Span::styled("  /model          ", accent_style), Span::styled("Switch AI model", text_style)]),
        Line::from(vec![Span::styled("  /config         ", accent_style), Span::styled("Edit configuration", text_style)]),
        Line::from(vec![Span::styled("  /vim            ", accent_style), Span::styled("Toggle vim mode", text_style)]),
        Line::from(vec![Span::styled("  /sessions       ", accent_style), Span::styled("List saved sessions", text_style)]),
        Line::from(""),
        Line::from("─".repeat(sep_width)),
        Line::from(vec![
            Span::styled(" [Enter] ", Style::default().fg(theme.success)),
            Span::styled("Get started", Style::default().fg(theme.text)),
        ]),
    ];

    let paragraph = Paragraph::new(content_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.primary))
                .border_type(ratatui::widgets::BorderType::Rounded),
        )
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, dialog_area);
}

/// Render chat search overlay bar (Ctrl+H activated).
/// Shows the search query, match count, and navigation hints.
fn render_chat_search_overlay(
    frame: &mut ratatui::Frame,
    area: Rect,
    state: &super::ReplState,
    theme: &Theme,
) {
    let bar_height = 3u16;
    let bar_width = area.width.saturating_sub(4).min(60);
    let y = area.bottom().saturating_sub(bar_height + 5);
    let x = (area.width.saturating_sub(bar_width)) / 2;

    let bar_area = Rect {
        x: area.x + x,
        y,
        width: bar_width,
        height: bar_height,
    };

    frame.render_widget(Clear, bar_area);

    let query_display = if state.chat_search_query.is_empty() {
        "(type to search chat)".to_string()
    } else {
        state.chat_search_query.clone()
    };

    let query_color = if state.chat_search_query.is_empty() {
        Color::DarkGray
    } else {
        theme.primary
    };

    let match_info = if state.chat_search_total_matches > 0 {
        format!("match {} of {}", state.chat_search_match_index + 1, state.chat_search_total_matches)
    } else if state.chat_search_query.is_empty() {
        String::new()
    } else {
        "no matches".to_string()
    };

    let lines = vec![
        Line::from(vec![
            Span::styled(" Ctrl+H ", Style::default().fg(Color::Black).bg(theme.primary)),
            Span::styled(" chat-search  ", Style::default().fg(Color::DarkGray)),
            Span::styled(&query_display, Style::default().fg(query_color).add_modifier(Modifier::BOLD)),
            Span::styled("▌", Style::default().fg(theme.primary)),
        ]),
        Line::from(vec![
            Span::styled(" ↑↓ navigate  ", Style::default().fg(Color::DarkGray)),
            Span::styled(match_info, Style::default().fg(theme.secondary)),
            Span::styled("  Enter", Style::default().fg(Color::Green)),
            Span::styled(" accept  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc", Style::default().fg(Color::Red)),
            Span::styled(" cancel", Style::default().fg(Color::DarkGray)),
        ]),
    ];

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.primary))
                .border_type(ratatui::widgets::BorderType::Rounded),
        );

    frame.render_widget(paragraph, bar_area);
}
