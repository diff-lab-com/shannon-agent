//! Command palette with fuzzy search — triggered by Ctrl+P
//!
//! Provides an overlay-style command palette that lists available commands
//! organized by category with keyboard shortcut display and fuzzy filtering.

use crate::theme::Theme;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
    Frame,
};

/// Command entry in the palette
#[derive(Debug, Clone)]
pub struct PaletteCommand {
    pub name: String,
    pub description: String,
    pub shortcut: Option<String>,
    pub category: CommandCategory,
    /// Argument template shown when selected (e.g., "<file>", "<query>")
    pub args_template: Option<String>,
    /// Subcommands for multi-level commands
    pub subcommands: Vec<String>,
    /// Number of times this command has been used (for MRU sorting)
    pub use_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CommandCategory {
    Navigation,
    Editing,
    Session,
    Tools,
    Settings,
}

impl CommandCategory {
    /// Return a short icon/label for the category
    fn icon(&self) -> &'static str {
        match self {
            CommandCategory::Navigation => "\u{f07c}", // folder-open
            CommandCategory::Editing => "\u{f303}",    // pencil
            CommandCategory::Session => "\u{f0c0}",    // users
            CommandCategory::Tools => "\u{f0ad}",      // wrench
            CommandCategory::Settings => "\u{f013}",   // cog
        }
    }

    /// Return a display label for the category
    #[allow(dead_code)]
    fn label(&self) -> &'static str {
        match self {
            CommandCategory::Navigation => "Nav",
            CommandCategory::Editing => "Edit",
            CommandCategory::Session => "Session",
            CommandCategory::Tools => "Tools",
            CommandCategory::Settings => "Settings",
        }
    }
}

/// The command palette widget state
#[derive(Debug, Clone)]
pub struct CommandPaletteWidget {
    pub commands: Vec<PaletteCommand>,
    pub query: String,
    pub selected_index: usize,
    pub scroll_offset: usize,
    pub is_visible: bool,
}

/// Maximum visible items before scrolling
const MAX_VISIBLE: usize = 10;

impl CommandPaletteWidget {
    /// Create a new palette pre-loaded with all built-in commands
    pub fn new() -> Self {
        Self {
            commands: Self::default_commands(),
            query: String::new(),
            selected_index: 0,
            scroll_offset: 0,
            is_visible: false,
        }
    }

    /// Toggle palette visibility
    pub fn toggle(&mut self) {
        self.is_visible = !self.is_visible;
        if self.is_visible {
            self.query.clear();
            self.selected_index = 0;
            self.scroll_offset = 0;
        }
    }

    /// Show the palette
    pub fn show(&mut self) {
        self.is_visible = true;
        self.query.clear();
        self.selected_index = 0;
        self.scroll_offset = 0;
    }

    /// Hide the palette
    pub fn hide(&mut self) {
        self.is_visible = false;
        self.query.clear();
    }

    /// Return filtered commands matching the current query, sorted by MRU
    pub fn filter_commands(&self) -> Vec<&PaletteCommand> {
        let mut results: Vec<&PaletteCommand> = if self.query.is_empty() {
            self.commands.iter().collect()
        } else {
            let q = self.query.to_lowercase();
            self.commands
                .iter()
                .filter(|cmd| {
                    let name_match = fuzzy_match_cmd(&q, &cmd.name.to_lowercase());
                    let desc_match = fuzzy_match_cmd(&q, &cmd.description.to_lowercase());
                    let subcmd_match = cmd.subcommands.iter().any(|s| s.to_lowercase().contains(&q));
                    name_match || desc_match || subcmd_match
                })
                .collect()
        };
        // Sort by use_count descending (MRU first), stable sort preserves order for ties
        results.sort_by(|a, b| b.use_count.cmp(&a.use_count));
        results
    }

    /// Move selection up
    pub fn move_up(&mut self) {
        let count = self.filter_commands().len();
        if count == 0 {
            return;
        }
        if self.selected_index > 0 {
            self.selected_index -= 1;
        } else {
            self.selected_index = count - 1;
        }
        self.adjust_scroll();
    }

    /// Move selection down
    pub fn move_down(&mut self) {
        let count = self.filter_commands().len();
        if count == 0 {
            return;
        }
        if self.selected_index + 1 < count {
            self.selected_index += 1;
        } else {
            self.selected_index = 0;
        }
        self.adjust_scroll();
    }

    /// Add a character to the query and reset selection
    pub fn add_char(&mut self, c: char) {
        self.query.push(c);
        self.selected_index = 0;
        self.scroll_offset = 0;
    }

    /// Remove the last character from the query
    pub fn backspace(&mut self) {
        self.query.pop();
        self.selected_index = 0;
        self.scroll_offset = 0;
    }

    /// Get the currently selected command (if any)
    pub fn selected_command(&self) -> Option<&PaletteCommand> {
        self.filter_commands().get(self.selected_index).copied()
    }

    /// Adjust scroll offset so the selected item stays visible
    fn adjust_scroll(&mut self) {
        if self.selected_index < self.scroll_offset {
            self.scroll_offset = self.selected_index;
        } else if self.selected_index >= self.scroll_offset + MAX_VISIBLE {
            self.scroll_offset = self.selected_index - MAX_VISIBLE + 1;
        }
    }

    /// Render the command palette as an overlay
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if !self.is_visible {
            return;
        }

        let filtered = self.filter_commands();
        let visible_count = filtered.len().min(MAX_VISIBLE);

        // Palette dimensions — centered, 60% width, at top of screen
        let popup_width = (area.width as f64 * 0.6).min(80.0) as u16;
        let popup_height = (visible_count + 2).min(MAX_VISIBLE + 2) as u16; // +2: search bar + border
        let x = (area.width.saturating_sub(popup_width)) / 2;
        let y = area.y + 1; // just below the top border

        let popup_area = Rect {
            x,
            y,
            width: popup_width,
            height: popup_height,
        };

        // Clear background for overlay
        frame.render_widget(Clear, popup_area);

        // Build list items
        let start = self.scroll_offset;
        let end = (start + visible_count).min(filtered.len());

        let list_items: Vec<ListItem> = filtered[start..end]
            .iter()
            .enumerate()
            .map(|(i, cmd)| {
                let abs_index = start + i;
                let is_selected = abs_index == self.selected_index;

                // Category icon
                let icon_span = Span::styled(
                    cmd.category.icon().to_string(),
                    Style::default().fg(theme.muted),
                );

                // Command name
                let name_style = if is_selected {
                    Style::default()
                        .fg(theme.primary)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.text)
                };
                let name_span = Span::styled(format!(" {}", cmd.name), name_style);

                // Description (right-aligned with padding)
                let shortcut_text = cmd
                    .shortcut
                    .as_deref()
                    .map(|s| format!(" [{s}]"))
                    .unwrap_or_default();
                let args_text = cmd
                    .args_template
                    .as_deref()
                    .map(|a| format!(" {a}"))
                    .unwrap_or_default();
                let desc_text = format!(
                    " {}{}{}",
                    cmd.description,
                    args_text,
                    shortcut_text
                );

                let desc_span = Span::styled(desc_text, Style::default().fg(if is_selected {
                    theme.text_dim
                } else {
                    theme.muted
                }));

                // Selection indicator
                let indicator = Span::styled(
                    if is_selected { " >" } else { "  " },
                    Style::default().fg(if is_selected { theme.secondary } else { theme.muted }),
                );

                ListItem::new(Line::from(vec![indicator, icon_span, name_span, desc_span]))
            })
            .collect();

        // Search bar as the block title
        let title_text = if self.query.is_empty() {
            " Type to search... ".to_string()
        } else {
            format!(" {} ", self.query)
        };

        let list = List::new(list_items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.primary))
                    .title(Span::styled(
                        title_text,
                        Style::default()
                            .fg(theme.secondary)
                            .add_modifier(Modifier::BOLD),
                    )),
            )
            .highlight_style(
                Style::default().bg(theme.context_bar_bg),
            );

        let mut state = ListState::default();
        let relative_selected = self.selected_index.saturating_sub(self.scroll_offset);
        if relative_selected < visible_count {
            state.select(Some(relative_selected));
        }
        frame.render_stateful_widget(list, popup_area, &mut state);
    }

    /// Record that a command was used (for MRU sorting)
    pub fn record_use(&mut self, command_name: &str) {
        if let Some(cmd) = self.commands.iter_mut().find(|c| c.name == command_name) {
            cmd.use_count += 1;
        }
    }

    // -- Built-in commands --

    fn default_commands() -> Vec<PaletteCommand> {
        vec![
            // Navigation
            PaletteCommand {
                name: "Search Chat".into(),
                description: "Search through chat history".into(),
                shortcut: Some("Ctrl+H".into()),
                category: CommandCategory::Navigation,
                args_template: None,
                subcommands: vec![],
                use_count: 0,
            },
            PaletteCommand {
                name: "Toggle Sidebar".into(),
                description: "Show or hide sidebar".into(),
                shortcut: Some("Ctrl+S".into()),
                category: CommandCategory::Navigation,
                args_template: None,
                subcommands: vec![],
                use_count: 0,
            },
            PaletteCommand {
                name: "Go to Top".into(),
                description: "Scroll to top of chat".into(),
                shortcut: None,
                category: CommandCategory::Navigation,
                args_template: None,
                subcommands: vec![],
                use_count: 0,
            },
            PaletteCommand {
                name: "Go to Bottom".into(),
                description: "Scroll to bottom of chat".into(),
                shortcut: None,
                category: CommandCategory::Navigation,
                args_template: None,
                subcommands: vec![],
                use_count: 0,
            },
            // Editing
            PaletteCommand {
                name: "External Editor".into(),
                description: "Open input in external editor".into(),
                shortcut: Some("Ctrl+E".into()),
                category: CommandCategory::Editing,
                args_template: None,
                subcommands: vec![],
                use_count: 0,
            },
            PaletteCommand {
                name: "Multiline Mode".into(),
                description: "Toggle multiline input".into(),
                shortcut: None,
                category: CommandCategory::Editing,
                args_template: None,
                subcommands: vec![],
                use_count: 0,
            },
            PaletteCommand {
                name: "Undo".into(),
                description: "Undo last action".into(),
                shortcut: None,
                category: CommandCategory::Editing,
                args_template: Some("<list|number>".into()),
                subcommands: vec!["list".into()],
                use_count: 0,
            },
            // Session
            PaletteCommand {
                name: "New Session".into(),
                description: "Start a new session".into(),
                shortcut: None,
                category: CommandCategory::Session,
                args_template: None,
                subcommands: vec![],
                use_count: 0,
            },
            PaletteCommand {
                name: "Close Session".into(),
                description: "Close current session".into(),
                shortcut: None,
                category: CommandCategory::Session,
                args_template: None,
                subcommands: vec![],
                use_count: 0,
            },
            PaletteCommand {
                name: "Switch Session".into(),
                description: "Switch between sessions".into(),
                shortcut: Some("Ctrl+Tab".into()),
                category: CommandCategory::Session,
                args_template: None,
                subcommands: vec![],
                use_count: 0,
            },
            // Tools
            PaletteCommand {
                name: "Run Command".into(),
                description: "Run a shell command".into(),
                shortcut: None,
                category: CommandCategory::Tools,
                args_template: Some("<command>".into()),
                subcommands: vec![],
                use_count: 0,
            },
            PaletteCommand {
                name: "View Diff".into(),
                description: "View pending changes".into(),
                shortcut: Some("Ctrl+D".into()),
                category: CommandCategory::Tools,
                args_template: Some("[ref]".into()),
                subcommands: vec!["review".into(), "accept".into(), "reject".into(), "accept-all".into(), "reject-all".into(), "interactive".into()],
                use_count: 0,
            },
            PaletteCommand {
                name: "Compact Context".into(),
                description: "Compact conversation context".into(),
                shortcut: None,
                category: CommandCategory::Tools,
                args_template: Some("[strategy|preview|focus <topic>]".into()),
                subcommands: vec!["status".into(), "preview".into(), "truncate".into(), "micro".into(), "group".into(), "focus".into()],
                use_count: 0,
            },
            PaletteCommand {
                name: "Image".into(),
                description: "Attach an image to conversation".into(),
                shortcut: None,
                category: CommandCategory::Tools,
                args_template: Some("<path|paste|url> [prompt]".into()),
                subcommands: vec!["paste".into(), "url".into()],
                use_count: 0,
            },
            PaletteCommand {
                name: "Find".into(),
                description: "Search through conversation messages".into(),
                shortcut: None,
                category: CommandCategory::Tools,
                args_template: Some("<query>".into()),
                subcommands: vec![],
                use_count: 0,
            },
            PaletteCommand {
                name: "Grep".into(),
                description: "Search command history with regex".into(),
                shortcut: None,
                category: CommandCategory::Tools,
                args_template: Some("<pattern>".into()),
                subcommands: vec![],
                use_count: 0,
            },
            // Settings
            PaletteCommand {
                name: "Change Model".into(),
                description: "Switch active LLM model".into(),
                shortcut: Some("Ctrl+M".into()),
                category: CommandCategory::Settings,
                args_template: Some("[model-name]".into()),
                subcommands: vec![],
                use_count: 0,
            },
            PaletteCommand {
                name: "Change Theme".into(),
                description: "Switch UI theme".into(),
                shortcut: None,
                category: CommandCategory::Settings,
                args_template: Some("[theme]".into()),
                subcommands: vec!["dark".into(), "light".into()],
                use_count: 0,
            },
            PaletteCommand {
                name: "Toggle Vim Mode".into(),
                description: "Enable or disable vim keybindings".into(),
                shortcut: None,
                category: CommandCategory::Settings,
                args_template: None,
                subcommands: vec![],
                use_count: 0,
            },
            PaletteCommand {
                name: "Keybindings".into(),
                description: "Show or customize keybindings".into(),
                shortcut: Some("F1".into()),
                category: CommandCategory::Settings,
                args_template: Some("[list|save|load]".into()),
                subcommands: vec!["list".into(), "save".into(), "load".into()],
                use_count: 0,
            },
        ]
    }
}

impl Default for CommandPaletteWidget {
    fn default() -> Self {
        Self::new()
    }
}

/// Simple fuzzy match: every character of `query` must appear in `candidate` in order.
fn fuzzy_match_cmd(query: &str, candidate: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let mut qi = query.chars().peekable();
    for ch in candidate.chars() {
        if Some(ch) == qi.peek().copied() {
            qi.next();
        }
        if qi.peek().is_none() {
            return true;
        }
    }
    qi.peek().is_none()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_palette_has_commands() {
        let palette = CommandPaletteWidget::new();
        assert!(!palette.commands.is_empty());
        assert_eq!(palette.commands.len(), 20);
        assert!(!palette.is_visible);
    }

    #[test]
    fn test_toggle_visibility() {
        let mut palette = CommandPaletteWidget::new();
        assert!(!palette.is_visible);
        palette.toggle();
        assert!(palette.is_visible);
        palette.toggle();
        assert!(!palette.is_visible);
    }

    #[test]
    fn test_show_hide() {
        let mut palette = CommandPaletteWidget::new();
        palette.show();
        assert!(palette.is_visible);
        palette.hide();
        assert!(!palette.is_visible);
    }

    #[test]
    fn test_filter_commands_empty_query() {
        let palette = CommandPaletteWidget::new();
        let filtered = palette.filter_commands();
        assert_eq!(filtered.len(), palette.commands.len());
    }

    #[test]
    fn test_filter_commands_by_name() {
        let mut palette = CommandPaletteWidget::new();
        palette.query = "model".to_string();
        let filtered = palette.filter_commands();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "Change Model");
    }

    #[test]
    fn test_filter_commands_by_description() {
        let mut palette = CommandPaletteWidget::new();
        palette.query = "vim".to_string();
        let filtered = palette.filter_commands();
        let names: Vec<&str> = filtered.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"Toggle Vim Mode"));
    }

    #[test]
    fn test_filter_commands_fuzzy() {
        let mut palette = CommandPaletteWidget::new();
        palette.query = "cht".to_string(); // fuzzy match "Search Chat"
        let filtered = palette.filter_commands();
        assert!(!filtered.is_empty());
        let names: Vec<&str> = filtered.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"Search Chat"));
    }

    #[test]
    fn test_navigation_up_down() {
        let mut palette = CommandPaletteWidget::new();
        assert_eq!(palette.selected_index, 0);
        palette.move_down();
        assert_eq!(palette.selected_index, 1);
        palette.move_down();
        assert_eq!(palette.selected_index, 2);
        palette.move_up();
        assert_eq!(palette.selected_index, 1);
    }

    #[test]
    fn test_navigation_wraps() {
        let mut palette = CommandPaletteWidget::new();
        // At index 0, move_up wraps to last
        palette.move_up();
        assert_eq!(palette.selected_index, palette.commands.len() - 1);
        // Move down wraps back to 0
        palette.move_down();
        assert_eq!(palette.selected_index, 0);
    }

    #[test]
    fn test_add_char_resets_selection() {
        let mut palette = CommandPaletteWidget::new();
        palette.move_down();
        palette.move_down();
        assert_eq!(palette.selected_index, 2);
        palette.add_char('s');
        assert_eq!(palette.query, "s");
        assert_eq!(palette.selected_index, 0);
    }

    #[test]
    fn test_backspace() {
        let mut palette = CommandPaletteWidget::new();
        palette.query = "abc".to_string();
        palette.backspace();
        assert_eq!(palette.query, "ab");
        palette.backspace();
        assert_eq!(palette.query, "a");
    }

    #[test]
    fn test_selected_command() {
        let palette = CommandPaletteWidget::new();
        let cmd = palette.selected_command();
        assert!(cmd.is_some());
        assert_eq!(cmd.unwrap().name, "Search Chat");
    }

    #[test]
    fn test_command_category_icons() {
        assert!(!CommandCategory::Navigation.icon().is_empty());
        assert!(!CommandCategory::Editing.icon().is_empty());
        assert!(!CommandCategory::Session.icon().is_empty());
        assert!(!CommandCategory::Tools.icon().is_empty());
        assert!(!CommandCategory::Settings.icon().is_empty());
    }

    #[test]
    fn test_command_category_labels() {
        assert_eq!(CommandCategory::Navigation.label(), "Nav");
        assert_eq!(CommandCategory::Editing.label(), "Edit");
        assert_eq!(CommandCategory::Session.label(), "Session");
        assert_eq!(CommandCategory::Tools.label(), "Tools");
        assert_eq!(CommandCategory::Settings.label(), "Settings");
    }

    #[test]
    fn test_palette_command_clone() {
        let cmd = PaletteCommand {
            name: "test".into(),
            description: "desc".into(),
            shortcut: Some("Ctrl+T".into()),
            category: CommandCategory::Navigation,
            args_template: None,
            subcommands: vec![],
            use_count: 0,
        };
        let cloned = cmd.clone();
        assert_eq!(cloned.name, "test");
        assert_eq!(cloned.shortcut, Some("Ctrl+T".into()));
    }

    #[test]
    fn test_default_impl() {
        let palette = CommandPaletteWidget::default();
        assert!(!palette.is_visible);
        assert_eq!(palette.commands.len(), 20);
    }
}
