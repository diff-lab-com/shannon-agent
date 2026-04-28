//! Autocomplete system for input field with file, command, and path completion
//!
//! Provides completion suggestions based on trigger characters (`/`, `@`, `./`)
//! and renders them as a floating list above the input area.

use std::path::{Path, PathBuf};

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
    Frame,
};

use crate::theme::Theme;

/// Completion item with metadata
#[derive(Debug, Clone)]
pub struct CompletionItem {
    pub label: String,
    pub kind: CompletionKind,
    pub detail: Option<String>,
    pub insert_text: String,
}

/// Icon displayed for each completion kind
impl CompletionKind {
    fn icon(&self) -> &'static str {
        match self {
            CompletionKind::File => "\u{f15b}",     // file icon
            CompletionKind::Directory => "\u{f07b}", // folder icon
            CompletionKind::Command => "\u{f054}",   // chevron
            CompletionKind::Snippet => "\u{f0c5}",   // copy icon
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CompletionKind {
    File,
    Directory,
    Command,
    Snippet,
}

/// Autocomplete engine that provides completions based on trigger characters
#[derive(Debug, Clone)]
pub struct AutocompleteEngine {
    /// Working directory for file completions
    working_dir: PathBuf,
    /// Available slash commands
    commands: Vec<CompletionItem>,
    /// Cached file list (refreshed periodically)
    file_cache: Vec<PathBuf>,
    /// Maximum results to return
    max_results: usize,
}

impl AutocompleteEngine {
    /// Create a new engine rooted at the given working directory
    pub fn new(working_dir: impl Into<PathBuf>) -> Self {
        let commands = Self::default_commands();
        Self {
            working_dir: working_dir.into(),
            commands,
            file_cache: Vec::new(),
            max_results: 20,
        }
    }

    /// Return completions for the given input at the given cursor position
    pub fn get_completions(&self, input: &str, cursor_pos: usize) -> Vec<CompletionItem> {
        let relevant = &input[..cursor_pos.min(input.len())];

        // 1. Slash-command completion
        if relevant.starts_with('/') && !relevant[1..].contains(' ') {
            let prefix = &relevant[1..];
            return self
                .commands
                .iter()
                .filter(|c| fuzzy_match(prefix, &c.label))
                .take(self.max_results)
                .cloned()
                .collect();
        }

        // 2. File mention via @
        if let Some(at) = relevant.rfind('@') {
            let query = &relevant[at + 1..];
            return self
                .file_cache
                .iter()
                .filter(|p| {
                    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    fuzzy_match(query, &name)
                })
                .take(self.max_results)
                .map(|p| {
                    let label = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    let detail = p.parent().map(|d| d.to_string_lossy().to_string());
                    let is_dir = p.is_dir();
                    CompletionItem {
                        label,
                        kind: if is_dir { CompletionKind::Directory } else { CompletionKind::File },
                        detail,
                        insert_text: p.to_string_lossy().to_string(),
                    }
                })
                .collect();
        }

        // 3. Path completion via ./ or /
        if let Some(slash) = relevant.rfind("./") {
            let query = &relevant[slash + 2..];
            return self
                .file_cache
                .iter()
                .filter(|p| {
                    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    fuzzy_match(query, &name)
                })
                .take(self.max_results)
                .map(|p| {
                    let label = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    let is_dir = p.is_dir();
                    CompletionItem {
                        label,
                        kind: if is_dir { CompletionKind::Directory } else { CompletionKind::File },
                        detail: None,
                        insert_text: p.to_string_lossy().to_string(),
                    }
                })
                .collect();
        }

        Vec::new()
    }

    /// Walk the working directory (max depth 5), excluding common ignore dirs.
    /// Fills `file_cache` with discovered paths.
    pub fn refresh_file_cache(&mut self) {
        let mut cache = Vec::new();
        let exclude = ["node_modules", ".git", "target", ".next", "dist", "__pycache__"];
        walk_dir(&self.working_dir, 0, 5, &exclude, &mut cache);
        self.file_cache = cache;
    }

    /// Set the working directory (e.g. on directory change)
    pub fn set_working_dir(&mut self, dir: PathBuf) {
        self.working_dir = dir;
    }

    /// Return the current working directory
    pub fn working_dir(&self) -> &Path {
        &self.working_dir
    }

    // -- helpers --

    fn default_commands() -> Vec<CompletionItem> {
        let specs: &[(&str, &str)] = &[
            ("help", "Show available commands"),
            ("config", "View or change configuration"),
            ("compact", "Compact conversation context"),
            ("diff", "Show pending changes"),
            ("commit", "Create a git commit"),
            ("clear", "Clear the conversation"),
            ("model", "Change the active model"),
            ("theme", "Change the UI theme"),
            ("quit", "Exit Shannon"),
        ];
        specs
            .iter()
            .map(|(name, desc)| CompletionItem {
                label: (*name).to_string(),
                kind: CompletionKind::Command,
                detail: Some((*desc).to_string()),
                insert_text: (*name).to_string(),
            })
            .collect()
    }
}

/// Recursive directory walker that respects depth and exclusion lists
fn walk_dir(dir: &Path, depth: usize, max_depth: usize, exclude: &[&str], out: &mut Vec<PathBuf>) {
    if depth >= max_depth {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if exclude.contains(&name_str.as_ref()) {
            continue;
        }
        let path = entry.path();
        out.push(path.clone());
        if path.is_dir() {
            walk_dir(&path, depth + 1, max_depth, exclude, out);
        }
    }
}

/// Simple fuzzy matching: every character of `query` must appear in `candidate` in order.
fn fuzzy_match(query: &str, candidate: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let q = query.to_lowercase();
    let c = candidate.to_lowercase();
    let mut qi = q.chars().peekable();
    for ch in c.chars() {
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
// Rendering
// ---------------------------------------------------------------------------

/// Maximum number of visible items in the popup
const MAX_VISIBLE: usize = 8;

/// Render the completion popup as a floating list above the input area.
///
/// `items` are the completions to display. `selected` is the index of the
/// currently selected item (will be highlighted). `area` should be the area
/// above the input field where the popup is drawn.
pub fn render_completions(
    items: &[CompletionItem],
    selected: usize,
    area: Rect,
    frame: &mut Frame,
    theme: &Theme,
) {
    if items.is_empty() {
        return;
    }

    let visible_count = items.len().min(MAX_VISIBLE);
    let popup_height = visible_count.saturating_add(2) as u16; // +2 for border
    let popup_width = area.width.min(60);

    // Position above the input area
    let popup_area = Rect {
        x: area.x,
        y: area.y.saturating_sub(popup_height),
        width: popup_width,
        height: popup_height,
    };

    // Clear the area behind the popup
    frame.render_widget(Clear, popup_area);

    // Build list items
    let list_items: Vec<ListItem> = items
        .iter()
        .take(visible_count)
        .enumerate()
        .map(|(i, item)| {
            let is_selected = i == selected;
            let icon = item.kind.icon();
            let detail_spans = item.detail.as_deref().map(|d| {
                vec![
                    Span::raw(" "),
                    Span::styled(
                        truncate_with_ellipsis(d, 30),
                        Style::default().fg(theme.text_dim),
                    ),
                ]
            }).unwrap_or_default();

            let mut spans = vec![
                Span::styled(
                    if is_selected { " > " } else { "   " },
                    Style::default().fg(if is_selected { theme.secondary } else { theme.text_dim }),
                ),
                Span::styled(
                    icon.to_string(),
                    Style::default().fg(theme.muted),
                ),
                Span::raw(" "),
                Span::styled(
                    item.label.clone(),
                    Style::default()
                        .fg(if is_selected { theme.primary } else { theme.text })
                        .add_modifier(if is_selected { Modifier::BOLD } else { Modifier::empty() }),
                ),
            ];
            spans.extend(detail_spans);

            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(list_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.primary)),
        )
        .highlight_style(
            Style::default()
                .bg(theme.context_bar_bg),
        );

    let mut state = ListState::default();
    if selected < visible_count {
        state.select(Some(selected));
    }
    frame.render_stateful_widget(list, popup_area, &mut state);
}

/// Truncate a string to `max` chars, appending "..." if truncated.
fn truncate_with_ellipsis(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let end = s.char_indices().take(max - 1).last().map(|(i, c)| i + c.len_utf8()).unwrap_or(0);
        format!("{}...", &s[..end])
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_new_has_commands() {
        let engine = AutocompleteEngine::new("/tmp");
        assert!(!engine.commands.is_empty());
        assert_eq!(engine.commands.len(), 9); // 9 default commands
    }

    #[test]
    fn test_slash_command_completion() {
        let engine = AutocompleteEngine::new("/tmp");
        let results = engine.get_completions("/hel", 4);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].label, "help");
    }

    #[test]
    fn test_slash_command_partial_match() {
        let engine = AutocompleteEngine::new("/tmp");
        let results = engine.get_completions("/c", 2);
        // Should match: config, compact, clear
        let labels: Vec<&str> = results.iter().map(|r| r.label.as_str()).collect();
        assert!(labels.contains(&"config"));
        assert!(labels.contains(&"compact"));
        assert!(labels.contains(&"clear"));
    }

    #[test]
    fn test_slash_command_no_match() {
        let engine = AutocompleteEngine::new("/tmp");
        let results = engine.get_completions("/xyz", 4);
        assert!(results.is_empty());
    }

    #[test]
    fn test_no_trigger_no_completions() {
        let engine = AutocompleteEngine::new("/tmp");
        let results = engine.get_completions("hello world", 11);
        assert!(results.is_empty());
    }

    #[test]
    fn test_fuzzy_match_basic() {
        assert!(fuzzy_match("hlp", "help"));
        assert!(fuzzy_match("", "anything"));
        assert!(!fuzzy_match("xyz", "abc"));
    }

    #[test]
    fn test_fuzzy_match_case_insensitive() {
        assert!(fuzzy_match("HLP", "help"));
        assert!(fuzzy_match("hlp", "HELP"));
    }

    #[test]
    fn test_fuzzy_match_order_matters() {
        assert!(fuzzy_match("hlp", "help"));
        assert!(!fuzzy_match("lph", "help"));
    }

    #[test]
    fn test_truncate_with_ellipsis_short() {
        assert_eq!(truncate_with_ellipsis("short", 10), "short");
    }

    #[test]
    fn test_truncate_with_ellipsis_long() {
        let result = truncate_with_ellipsis("a very long string that needs truncation", 10);
        assert!(result.ends_with("..."));
        assert!(result.len() <= 13); // 10 + "..."
    }

    #[test]
    fn test_completion_item_clone() {
        let item = CompletionItem {
            label: "test".to_string(),
            kind: CompletionKind::Command,
            detail: Some("desc".to_string()),
            insert_text: "test".to_string(),
        };
        let cloned = item.clone();
        assert_eq!(cloned.label, "test");
        assert_eq!(cloned.kind, CompletionKind::Command);
    }

    #[test]
    fn test_completion_kind_equality() {
        assert_eq!(CompletionKind::File, CompletionKind::File);
        assert_ne!(CompletionKind::File, CompletionKind::Directory);
    }

    #[test]
    fn test_completion_kind_icons() {
        // Just verify icons don't panic and return non-empty strings
        assert!(!CompletionKind::File.icon().is_empty());
        assert!(!CompletionKind::Directory.icon().is_empty());
        assert!(!CompletionKind::Command.icon().is_empty());
        assert!(!CompletionKind::Snippet.icon().is_empty());
    }
}
