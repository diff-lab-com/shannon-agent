//! Full-screen diff viewer overlay widget

use crate::theme::Theme;
use crate::repl_enhancement::{DiffData, FileChange};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem},
    Frame,
};

/// Full-screen overlay for viewing diffs
#[derive(Debug, Clone)]
pub struct DiffViewerWidget {
    /// Scroll offset
    pub scroll_offset: usize,
    /// Index of the currently selected file (across all turns)
    pub selected_index: usize,
    /// Which files are expanded to show their diff details
    pub expanded: Vec<bool>,
}

impl DiffViewerWidget {
    /// Create a new diff viewer
    pub fn new() -> Self {
        Self {
            scroll_offset: 0,
            selected_index: 0,
            expanded: Vec::new(),
        }
    }

    /// Get unique modified files from diff data
    fn unique_files(diff_data: &DiffData) -> Vec<FileChange> {
        let mut files: Vec<FileChange> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for turn in diff_data.get_session_diffs() {
            for fc in &turn.files_modified {
                if seen.insert(fc.path.clone()) {
                    files.push(fc.clone());
                }
            }
        }
        files
    }

    /// Ensure the expanded vector is the right length
    pub fn sync_expanded(&mut self, file_count: usize) {
        while self.expanded.len() < file_count {
            self.expanded.push(false);
        }
        self.expanded.truncate(file_count);
    }

    /// Move selection up
    pub fn move_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    /// Move selection down
    pub fn move_down(&mut self, max: usize) {
        if self.selected_index < max.saturating_sub(1) {
            self.selected_index += 1;
        }
    }

    /// Toggle expansion of the currently selected file
    pub fn toggle_expand(&mut self) {
        if let Some(e) = self.expanded.get_mut(self.selected_index) {
            *e = !*e;
        }
    }

    /// Render the diff viewer as a full-screen overlay
    pub fn render(&self, frame: &mut Frame, area: Rect, diff_data: &DiffData, theme: &Theme) {
        // Clear the area first
        frame.render_widget(Clear, area);

        let files = Self::unique_files(diff_data);
        let total_adds = diff_data.total_additions();
        let total_dels = diff_data.total_deletions();

        let title = format!(" Diff Viewer ({} files, +{} -{}) ", files.len(), total_adds, total_dels);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(Line::from(Span::styled(
                title,
                Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
            )))
            .title_bottom(Line::from(Span::styled(
                " j/k: navigate | Enter: expand | Esc: close ",
                Style::default().fg(theme.muted),
            )));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let mut items: Vec<ListItem> = Vec::new();
        let inner_width = inner.width as usize;

        if files.is_empty() {
            let line = Line::from(Span::styled(
                "No file changes recorded in this session.",
                Style::default().fg(theme.text_dim),
            ));
            items.push(ListItem::new(line));
        } else {
            // Also list created and deleted files
            let mut all_entries: Vec<Entry> = files.into_iter().map(|fc| Entry::Modified(fc)).collect();
            for turn in diff_data.get_session_diffs() {
                for p in &turn.files_created {
                    all_entries.push(Entry::Created(p.clone()));
                }
                for p in &turn.files_deleted {
                    all_entries.push(Entry::Deleted(p.clone()));
                }
            }

            for (i, entry) in all_entries.iter().enumerate() {
                let is_selected = i == self.selected_index;
                let is_expanded = self.expanded.get(i).copied().unwrap_or(false);
                let cursor = if is_selected { ">" } else { " " };
                let expand_icon = if is_expanded { "▼" } else { "▶" };

                let line = match entry {
                    Entry::Modified(fc) => {
                        let fname = truncate_path(&fc.path, inner_width.saturating_sub(20));
                        let changes = format!("+{} -{}", fc.additions, fc.deletions);
                        let bg = if is_selected { theme.context_bar_bg } else { theme.text };
                        let _ = bg;
                        Line::from(vec![
                            Span::styled(format!("{cursor} "), Style::default().fg(theme.text_dim)),
                            Span::styled(format!("{expand_icon} "), Style::default().fg(theme.muted)),
                            Span::styled(fname, Style::default().fg(if is_selected { theme.primary } else { theme.text })),
                            Span::styled(" ", Style::default().fg(theme.text_dim)),
                            Span::styled(changes, Style::default().fg(theme.muted)),
                        ])
                    }
                    Entry::Created(p) => {
                        let fname = truncate_path(p, inner_width.saturating_sub(20));
                        Line::from(vec![
                            Span::styled(format!("{cursor} "), Style::default().fg(theme.text_dim)),
                            Span::styled("+ ", Style::default().fg(theme.success)),
                            Span::styled(fname, Style::default().fg(if is_selected { theme.success } else { theme.text })),
                            Span::styled(" new", Style::default().fg(theme.success)),
                        ])
                    }
                    Entry::Deleted(p) => {
                        let fname = truncate_path(p, inner_width.saturating_sub(20));
                        Line::from(vec![
                            Span::styled(format!("{cursor} "), Style::default().fg(theme.text_dim)),
                            Span::styled("x ", Style::default().fg(theme.error)),
                            Span::styled(fname, Style::default().fg(if is_selected { theme.error } else { theme.text })),
                            Span::styled(" deleted", Style::default().fg(theme.error)),
                        ])
                    }
                };
                items.push(ListItem::new(line));

                // Show diff summary when expanded
                if is_expanded {
                    if let Entry::Modified(fc) = entry {
                        let detail = format!("  {} (+{} additions, -{} deletions)", fc.path, fc.additions, fc.deletions);
                        items.push(ListItem::new(Line::from(Span::styled(
                            truncate_to(&detail, inner_width),
                            Style::default().fg(theme.text_dim),
                        ))));
                    }
                }
            }
        }

        // Apply scroll offset
        let visible_rows = inner.height as usize;
        let total = items.len();
        let start = if total > visible_rows {
            self.scroll_offset.min(total.saturating_sub(visible_rows))
        } else {
            0
        };
        let visible_items: Vec<ListItem> = items.into_iter().skip(start).take(visible_rows).collect();

        let list = List::new(visible_items);
        frame.render_widget(list, inner);
    }
}

impl Default for DiffViewerWidget {
    fn default() -> Self {
        Self::new()
    }
}

/// Entry types for the diff viewer
enum Entry {
    Modified(FileChange),
    Created(String),
    Deleted(String),
}

/// Truncate a file path to fit within max chars, keeping the filename
fn truncate_path(path: &str, max_chars: usize) -> String {
    if path.chars().count() <= max_chars {
        return path.to_string();
    }
    // Try to keep the filename
    if let Some(slash_idx) = path.rfind('/') {
        let fname = &path[slash_idx + 1..];
        if fname.chars().count() + 4 <= max_chars {
            let budget = max_chars - fname.chars().count() - 4;
            let prefix: String = path.chars().take(budget).collect();
            return format!("{prefix}...{fname}");
        }
    }
    truncate_to(path, max_chars)
}

/// Truncate string to fit within max chars
fn truncate_to(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else if max_chars > 1 {
        let truncated: String = s.chars().take(max_chars - 1).collect();
        format!("{truncated}…")
    } else {
        "…".to_string()
    }
}
