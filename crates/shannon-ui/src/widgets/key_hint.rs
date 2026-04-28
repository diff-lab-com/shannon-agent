//! Key hint overlay widget
//!
//! HintContext variants beyond Normal/Input are wired conditionally based on
//! active mode. render_full is triggered by `?` key or F1. Allow dead_code
//! for currently unused variants and the full panel renderer.

#[allow(dead_code)]
use crate::theme::Theme;
use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

/// Key hint context for displaying relevant shortcuts
#[allow(dead_code)]
pub enum HintContext {
    /// Browsing chat history
    Normal,
    /// Typing in prompt input
    Input,
    /// Vim normal mode
    VimNormal,
    /// Vim insert mode
    VimInsert,
    /// Viewing diff output
    DiffView,
}

/// Key hint overlay widget
pub struct KeyHintWidget;

impl KeyHintWidget {
    /// Render compact bottom bar with context-aware shortcuts
    pub fn render(frame: &mut Frame, area: Rect, theme: &Theme, context: &HintContext) {
        let hints = match context {
            HintContext::Normal => vec![
                ("Ctrl+S", "Sidebar"),
                ("Ctrl+F", "Focus"),
                ("?", "Help"),
                ("q", "Quit"),
            ],
            HintContext::Input => vec![
                ("Enter", "Submit"),
                ("Esc", "Cancel"),
                ("Ctrl+C", "Clear"),
                ("Tab", "Complete"),
            ],
            HintContext::VimNormal => vec![
                ("i", "Insert"),
                ("/", "Search"),
                ("n", "Next"),
                ("dd", "Delete"),
            ],
            HintContext::VimInsert => vec![
                ("Esc", "Normal"),
                ("Ctrl+C", "Cancel"),
                ("Tab", "Indent"),
                ("Ctrl+D", "Exit"),
            ],
            HintContext::DiffView => vec![
                ("j/k", "Nav"),
                ("f", "Fold"),
                ("Enter", "Apply"),
                ("Esc", "Close"),
            ],
        };

        let mut spans = Vec::new();
        let sep = Span::styled(" │ ", Style::default().fg(theme.muted));

        for (i, (key, desc)) in hints.iter().enumerate() {
            if i > 0 {
                spans.push(sep.clone());
            }
            // Key binding in primary color
            spans.push(Span::styled(
                format!("{key}"),
                Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
            ));
            // Description in dim color
            spans.push(Span::styled(
                format!(": {desc}"),
                Style::default().fg(theme.text_dim),
            ));
        }

        let paragraph = Paragraph::new(Line::from(spans))
            .style(Style::default().bg(theme.context_bar_bg))
            .alignment(Alignment::Center);

        frame.render_widget(paragraph, area);
    }

    /// Render full keyboard shortcuts panel (centered overlay)
    #[allow(dead_code)]
    pub fn render_full(frame: &mut Frame, theme: &Theme) {
        let all_sections = vec![
            ("Navigation", vec![
                ("Ctrl+N / Ctrl+P", "Next/Previous message"),
                ("Ctrl+F", "Focus chat/input"),
                ("Ctrl+S", "Toggle sidebar"),
                ("Page Up/Down", "Scroll chat"),
                ("Home/End", "Jump to top/bottom"),
            ]),
            ("Input Editing", vec![
                ("Ctrl+C", "Cancel/Clear input"),
                ("Ctrl+U", "Clear to start of line"),
                ("Ctrl+K", "Clear to end of line"),
                ("Ctrl+W", "Delete word"),
                ("Tab", "Auto-complete"),
            ]),
            ("Search", vec![
                ("Ctrl+R", "Reverse search"),
                ("Ctrl+S", "Forward search"),
                ("n / N", "Next/Previous match"),
                ("Esc", "Exit search"),
            ]),
            ("Actions", vec![
                ("Ctrl+O", "Open file"),
                ("Ctrl+G", "Git status"),
                ("Ctrl+D", "Diff view"),
                ("Ctrl+T", "Run tests"),
                ("F1 / ?", "Keyboard shortcuts"),
            ]),
            ("Vim Mode", vec![
                ("i", "Enter insert mode"),
                ("Esc", "Return to normal mode"),
                ("dd", "Delete current message"),
                ("/", "Search in chat"),
                ("n", "Next search result"),
            ]),
            ("System", vec![
                ("Ctrl+Q", "Quit"),
                ("Ctrl+L", "Force redraw"),
                ("Ctrl+Z", "Suspend (Unix)"),
                ("Ctrl+]", "Show debug info"),
                ("Ctrl+X", "Toggle focus mode"),
            ]),
        ];

        let mut all_lines = Vec::new();

        for (section_name, shortcuts) in all_sections {
            // Section header
            all_lines.push(Line::from(vec![
                Span::styled(
                    format!(" {section_name} "),
                    Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
                ),
            ]));

            // Shortcuts in two columns
            for (i, (key, desc)) in shortcuts.iter().enumerate() {
                let left_col = format!("  {key:<20} {desc:<25}");
                let right_col = if i + 1 < shortcuts.len() {
                    let (next_key, next_desc) = &shortcuts[i + 1];
                    format!("{next_key:<20} {next_desc:<25}")
                } else {
                    String::new()
                };

                all_lines.push(Line::from(vec![
                    Span::styled(left_col, Style::default().fg(theme.text)),
                    Span::styled(right_col, Style::default().fg(theme.text)),
                ]));

                // Skip the next one since we processed it
                if i + 1 < shortcuts.len() {
                    // Continue, but we'll skip in the loop by incrementing by 2
                }
            }
            // Blank line between sections
            all_lines.push(Line::from(""));
        }

        // Calculate panel size (max 80x25, or fit within terminal)
        let terminal_size = frame.area();
        let panel_width = (terminal_size.width.min(80) as usize).max(60);
        let panel_height = (terminal_size.height.min(30) as usize).max(20);

        // Center the panel
        let x = terminal_size.x + (terminal_size.width.saturating_sub(panel_width as u16)) / 2;
        let y = terminal_size.y + (terminal_size.height.saturating_sub(panel_height as u16)) / 2;

        let panel_area = Rect {
            x,
            y,
            width: panel_width as u16,
            height: panel_height as u16,
        };

        // Render with Clear to show overlay on top
        let panel = Paragraph::new(all_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border))
                    .title(" Keyboard Shortcuts ")
                    .title_style(Style::default().fg(theme.primary).add_modifier(Modifier::BOLD))
            )
            .alignment(Alignment::Left);

        frame.render_widget(Clear, panel_area); // Clear background
        frame.render_widget(panel, panel_area);
    }
}
