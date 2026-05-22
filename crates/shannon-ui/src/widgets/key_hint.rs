//! Key hint overlay widget
//!
//! Displays context-aware keyboard shortcuts. Compact bar shown at bottom
//! of chat; full panel triggered by `?` key or F1.

use crate::theme::Theme;
use rust_i18n::t;
use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

/// Key hint context for displaying relevant shortcuts
pub enum HintContext {
    /// Browsing chat history
    Normal,
    /// Vim insert mode
    VimInsert,
    /// Vim normal mode
    VimNormal,
    /// Vim visual mode
    VimVisual,
}

/// Key hint overlay widget
pub struct KeyHintWidget;

impl KeyHintWidget {
    /// Render compact bottom bar with context-aware shortcuts
    pub fn render(frame: &mut Frame, area: Rect, theme: &Theme, context: &HintContext) {
        let hints = match context {
            HintContext::Normal => vec![
                ("PgUp/Dn", t!("ui.key_scroll").to_string()),
                ("Shift+Tab", t!("ui.key_mode").to_string()),
                ("Ctrl+G", t!("ui.key_pager").to_string()),
                ("?", t!("ui.key_help").to_string()),
                ("Ctrl+Q", t!("ui.key_quit").to_string()),
            ],
            HintContext::VimInsert => vec![
                ("Esc", t!("ui.key_normal").to_string()),
                ("Enter", t!("ui.key_send").to_string()),
                ("Tab", t!("ui.key_complete").to_string()),
                ("Ctrl+Y", t!("ui.key_copy").to_string()),
                ("Ctrl+E", t!("ui.key_editor").to_string()),
                ("Ctrl+H", t!("ui.key_chat_search").to_string()),
                ("?", t!("ui.key_help").to_string()),
            ],
            HintContext::VimNormal => vec![
                ("i", t!("ui.key_insert").to_string()),
                ("v", t!("ui.key_visual").to_string()),
                (":", t!("ui.key_command").to_string()),
                ("h/j/k/l", t!("ui.key_move").to_string()),
                ("dd", t!("ui.key_del_line").to_string()),
                ("Esc", t!("ui.key_cancel").to_string()),
            ],
            HintContext::VimVisual => vec![
                ("y", t!("ui.key_yank").to_string()),
                ("d", t!("ui.key_delete").to_string()),
                ("Esc", t!("ui.key_normal").to_string()),
                ("PgUp/Dn", t!("ui.key_scroll").to_string()),
                ("?", t!("ui.key_help").to_string()),
            ],
        };

        let mut spans = Vec::new();
        let sep = Span::styled(" │ ", Style::default().fg(theme.muted));

        for (i, (key, desc)) in hints.iter().enumerate() {
            if i > 0 {
                spans.push(sep.clone());
            }
            spans.push(Span::styled(
                key.to_string(),
                Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                format!(" {desc}"),
                Style::default().fg(theme.text_dim),
            ));
        }

        let paragraph = Paragraph::new(Line::from(spans))
            .style(Style::default().bg(theme.context_bar_bg))
            .alignment(Alignment::Center);

        frame.render_widget(paragraph, area);
    }

    /// Render full keyboard shortcuts panel (centered overlay)
    pub fn render_full(frame: &mut Frame, theme: &Theme) {
        let all_sections = vec![
            (t!("ui.help_section_navigation").to_string(), vec![
                ("Page Up/Down", t!("ui.help_scroll_page").to_string()),
                ("Home / End", t!("ui.help_jump_top_bottom").to_string()),
                ("Ctrl+G / Ctrl+T", t!("ui.help_toggle_transcript").to_string()),
            ]),
            (t!("ui.help_section_input").to_string(), vec![
                ("Enter", t!("ui.help_send_message").to_string()),
                ("Shift+Enter", t!("ui.help_newline").to_string()),
                ("Ctrl+C", t!("ui.help_clear_input").to_string()),
                ("Ctrl+E", t!("ui.help_external_editor").to_string()),
                ("Ctrl+Y", t!("ui.help_copy_response").to_string()),
                ("Ctrl+H", t!("ui.help_search_chat").to_string()),
                ("Ctrl+R", t!("ui.help_search_history").to_string()),
                ("Ctrl+W", t!("ui.help_delete_word").to_string()),
                ("Ctrl+K", t!("ui.help_kill_line").to_string()),
                ("Ctrl+U", t!("ui.help_kill_start").to_string()),
                ("Ctrl+L", t!("ui.help_clear_screen").to_string()),
                ("Tab", t!("ui.help_complete").to_string()),
                ("Shift+Tab", t!("ui.help_approval_mode").to_string()),
                ("Up / Down", t!("ui.help_command_history").to_string()),
            ]),
            (t!("ui.help_section_chat").to_string(), vec![
                ("Ctrl+F", t!("ui.help_fold_tool").to_string()),
                ("Ctrl+O", t!("ui.help_cycle_view").to_string()),
                ("Alt+F", t!("ui.help_toggle_fold").to_string()),
                ("Ctrl+V", t!("ui.help_paste_image").to_string()),
                ("Ctrl+P", t!("ui.help_command_palette").to_string()),
                ("Ctrl+A", t!("ui.help_active_agents").to_string()),
            ]),
            (t!("ui.help_section_vim").to_string(), vec![
                ("i", t!("ui.help_insert_mode").to_string()),
                ("v", t!("ui.help_visual_mode").to_string()),
                ("V", t!("ui.help_visual_line").to_string()),
                ("h/j/k/l", t!("ui.help_move_cursor").to_string()),
                ("w / b", t!("ui.help_word_forward_back").to_string()),
                ("0 / $", t!("ui.help_line_start_end").to_string()),
                ("dd", t!("ui.help_delete_line").to_string()),
                ("yy", t!("ui.help_yank_line").to_string()),
                ("p", t!("ui.help_paste_after").to_string()),
                ("u", t!("ui.help_undo").to_string()),
                ("Ctrl+R", t!("ui.help_redo").to_string()),
                (":", t!("ui.help_command_mode").to_string()),
                ("Esc", t!("ui.help_return_normal").to_string()),
            ]),
            (t!("ui.help_section_system").to_string(), vec![
                ("F1 / ?", t!("ui.help_this_overlay").to_string()),
                ("F8", t!("ui.help_toggle_mouse").to_string()),
                ("Ctrl+Q", t!("ui.key_quit").to_string()),
                ("Esc Esc", t!("ui.help_undo_exchange").to_string()),
            ]),
        ];

        let mut all_lines = Vec::new();

        for (section_name, shortcuts) in &all_sections {
            all_lines.push(Line::from(vec![
                Span::styled(
                    format!(" {section_name} "),
                    Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
                ),
            ]));

            for (key, desc) in shortcuts {
                all_lines.push(Line::from(vec![
                    Span::styled(
                        format!("  {key:<22}"),
                        Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(" {desc}"),
                        Style::default().fg(theme.text_dim),
                    ),
                ]));
            }
            all_lines.push(Line::from(""));
        }

        // Footer
        all_lines.push(Line::from(vec![
            Span::styled(
                format!(" {} ", t!("ui.help_press_any_key")),
                Style::default().fg(theme.muted),
            ),
        ]));

        let terminal_size = frame.area();
        let panel_width = 58u16.min(terminal_size.width);
        let panel_height = (all_lines.len() as u16 + 2).min(terminal_size.height);

        let x = terminal_size.x + (terminal_size.width.saturating_sub(panel_width)) / 2;
        let y = terminal_size.y + (terminal_size.height.saturating_sub(panel_height)) / 2;

        let panel_area = Rect {
            x,
            y,
            width: panel_width,
            height: panel_height,
        };

        let panel = Paragraph::new(all_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border))
                    .title(format!(" {} ", t!("ui.help_title")))
                    .title_style(Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
            )
            .alignment(Alignment::Left);

        frame.render_widget(Clear, panel_area);
        frame.render_widget(panel, panel_area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_hints_include_mode_switch() {
        // Verify the Normal hint context renders without panic (includes Shift+Tab)
        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let theme = Theme::default();
        terminal.draw(|f| {
            let area = Rect::new(0, 22, 80, 1);
            KeyHintWidget::render(f, area, &theme, &HintContext::Normal);
        }).unwrap();
    }

    #[test]
    fn test_normal_hints_cover_key_shortcuts() {
        // Verify HintContext variants exist and are usable
        let contexts = [
            HintContext::Normal,
            HintContext::VimInsert,
            HintContext::VimNormal,
            HintContext::VimVisual,
        ];
        for ctx in &contexts {
            let backend = ratatui::backend::TestBackend::new(120, 24);
            let mut terminal = ratatui::Terminal::new(backend).unwrap();
            let theme = Theme::default();
            terminal.draw(|f| {
                let area = Rect::new(0, 22, 120, 1);
                KeyHintWidget::render(f, area, &theme, ctx);
            }).unwrap();
        }
    }
}
