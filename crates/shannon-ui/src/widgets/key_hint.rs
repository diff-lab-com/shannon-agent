//! Key hint overlay widget
//!
//! Displays context-aware keyboard shortcuts. Compact bar shown at bottom
//! of chat; full panel triggered by `?` key or F1.

use crate::theme::Theme;
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
                ("PgUp/Dn", "Scroll"),
                ("Ctrl+G", "Pager"),
                ("?", "Help"),
                ("Ctrl+Q", "Quit"),
            ],
            HintContext::VimInsert => vec![
                ("Esc", "Normal"),
                ("Enter", "Send"),
                ("Tab", "Complete"),
                ("Ctrl+Y", "Copy"),
                ("Ctrl+E", "Editor"),
                ("Ctrl+H", "Chat search"),
                ("?", "Help"),
            ],
            HintContext::VimNormal => vec![
                ("i", "Insert"),
                ("v", "Visual"),
                (":", "Command"),
                ("h/j/k/l", "Move"),
                ("dd", "Del line"),
                ("Esc", "Cancel"),
            ],
            HintContext::VimVisual => vec![
                ("y", "Yank"),
                ("d", "Delete"),
                ("Esc", "Normal"),
                ("PgUp/Dn", "Scroll"),
                ("?", "Help"),
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
            ("Navigation", vec![
                ("Page Up/Down", "Scroll chat by page"),
                ("Home / End", "Jump to top / bottom"),
                ("Ctrl+G / Ctrl+T", "Toggle transcript pager"),
            ]),
            ("Input", vec![
                ("Enter", "Send message"),
                ("Shift+Enter", "Insert newline"),
                ("Ctrl+C", "Clear input (or quit)"),
                ("Ctrl+E", "Open external editor"),
                ("Ctrl+Y", "Copy last response to clipboard"),
                ("Ctrl+H", "Search chat messages"),
                ("Ctrl+R", "Search command history"),
                ("Ctrl+W", "Delete word back"),
                ("Ctrl+K", "Kill to end of line"),
                ("Ctrl+U", "Kill to start of line"),
                ("Ctrl+L", "Clear screen"),
                ("Tab", "Complete / queue while streaming"),
                ("Shift+Tab", "Cycle approval mode"),
                ("Up / Down", "Command history"),
            ]),
            ("Chat", vec![
                ("Ctrl+F", "Fold/unfold last tool output"),
                ("Ctrl+O", "Cycle view mode (Default/Verbose)"),
                ("Alt+F", "Toggle all tool folding"),
                ("Ctrl+V", "Paste image from clipboard"),
                ("Ctrl+P", "Command palette"),
                ("Ctrl+A", "Show active agents"),
            ]),
            ("Vim", vec![
                ("i", "Enter insert mode"),
                ("v", "Visual mode (select text)"),
                ("V", "Visual line mode"),
                ("h/j/k/l", "Move cursor"),
                ("w / b", "Word forward / back"),
                ("0 / $", "Line start / end"),
                ("dd", "Delete line"),
                ("yy", "Yank (copy) line"),
                ("p", "Paste after cursor"),
                ("u", "Undo"),
                ("Ctrl+R", "Redo"),
                (":", "Command mode"),
                ("Esc", "Return to normal mode"),
            ]),
            ("Help & System", vec![
                ("F1 / ?", "This help overlay"),
                ("F8", "Toggle mouse scroll"),
                ("Ctrl+Q", "Quit"),
                ("Esc Esc", "Undo last exchange"),
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
                " Press any key to close ",
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
                    .title(" Shortcuts ")
                    .title_style(Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
            )
            .alignment(Alignment::Left);

        frame.render_widget(Clear, panel_area);
        frame.render_widget(panel, panel_area);
    }
}
