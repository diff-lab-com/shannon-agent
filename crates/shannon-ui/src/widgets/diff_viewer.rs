//! Full-screen diff viewer overlay widget

use crate::repl_enhancement::{DiffData, FileChange};
use crate::theme::Theme;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem},
};
use std::collections::HashMap;

/// Return a style for a unified-diff line with syntax-aware coloring.
///
/// Categories (checked in order):
/// - `--- a/file`, `+++ b/file` → yellow + bold (file headers)
/// - `@@ ... @@`               → cyan (hunk headers)
/// - `+content`                → green (added, via `theme.diff_added`)
/// - `-content`                → red (removed, via `theme.diff_removed`)
/// - `diff ...`, `index ...`, `Binary ...` → dim (git meta)
/// - everything else           → dim (context)
fn diff_line_style(line: &str, theme: &Theme) -> Style {
    if (line.starts_with("---") || line.starts_with("+++")) && line.len() > 3 {
        Style::default()
            .fg(theme.warning)
            .add_modifier(Modifier::BOLD)
    } else if line.starts_with('+') {
        Style::default().fg(theme.diff_added)
    } else if line.starts_with('-') {
        Style::default().fg(theme.diff_removed)
    } else if line.starts_with('@') {
        Style::default().fg(theme.secondary)
    } else if line.starts_with("diff ") || line.starts_with("index ") || line.starts_with("Binary ")
    {
        Style::default().fg(theme.muted)
    } else {
        Style::default().fg(theme.text_dim)
    }
}

/// Full-screen overlay for viewing diffs
#[derive(Debug, Clone)]
pub struct DiffViewerWidget {
    /// Scroll offset
    pub scroll_offset: usize,
    /// Index of the currently selected file (across all turns)
    pub selected_index: usize,
    /// Which files are expanded to show their diff details
    pub expanded: Vec<bool>,
    /// Cached diff output per file path
    pub diff_cache: HashMap<String, Vec<String>>,
    /// When set, show this file's diff in full detail instead of the file list
    pub detail_file: Option<String>,
}

impl DiffViewerWidget {
    /// Create a new diff viewer
    pub fn new() -> Self {
        Self {
            scroll_offset: 0,
            selected_index: 0,
            expanded: Vec::new(),
            diff_cache: HashMap::new(),
            detail_file: None,
        }
    }

    /// Load git diff for a file, using cache if available.
    pub fn load_diff(&mut self, path: &str) {
        if self.diff_cache.contains_key(path) {
            return;
        }
        // Try git diff for the file (staged + unstaged vs HEAD)
        let output = std::process::Command::new("git")
            .args(["diff", "HEAD", "--", path])
            .output();

        let lines = match output {
            Ok(o) if o.status.success() => {
                let stdout = String::from_utf8_lossy(&o.stdout).into_owned();
                let all_lines: Vec<&str> = stdout.lines().collect();
                if all_lines.len() > 200 {
                    let mut capped: Vec<String> =
                        all_lines[..200].iter().map(|s| s.to_string()).collect();
                    capped.push(format!(
                        "  ... diff truncated ({} more lines)",
                        all_lines.len() - 200
                    ));
                    capped
                } else {
                    all_lines.into_iter().map(String::from).collect()
                }
            }
            _ => {
                // Fallback: try unstaged diff
                let output2 = std::process::Command::new("git")
                    .args(["diff", "--", path])
                    .output();
                match output2 {
                    Ok(o) => {
                        let stdout = String::from_utf8_lossy(&o.stdout).into_owned();
                        let all_lines: Vec<&str> = stdout.lines().collect();
                        if all_lines.len() > 200 {
                            let mut capped: Vec<String> =
                                all_lines[..200].iter().map(|s| s.to_string()).collect();
                            capped.push(format!(
                                "  ... diff truncated ({} more lines)",
                                all_lines.len() - 200
                            ));
                            capped
                        } else {
                            all_lines.into_iter().map(String::from).collect()
                        }
                    }
                    Err(_) => vec!["(unable to read diff)".to_string()],
                }
            }
        };
        self.diff_cache.insert(path.to_string(), lines);
    }

    /// Load raw diff text directly (e.g., from a preview_revert) into a virtual "file" entry.
    pub fn load_raw_diff(&mut self, diff_text: &str) {
        let key = "__undo_preview__".to_string();
        if self.diff_cache.contains_key(&key) {
            return;
        }
        let all_lines: Vec<&str> = diff_text.lines().collect();
        let lines = if all_lines.len() > 500 {
            let mut capped: Vec<String> =
                all_lines[..500].iter().map(|s| s.to_string()).collect();
            capped.push(format!(
                "  ... diff truncated ({} more lines)",
                all_lines.len() - 500
            ));
            capped
        } else if all_lines.is_empty() {
            vec!["(no differences)".to_string()]
        } else {
            all_lines.into_iter().map(String::from).collect()
        };
        self.diff_cache.insert(key.clone(), lines);
        self.expanded = vec![true];
        self.detail_file = Some(key);
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

    /// Toggle expansion of the currently selected file, loading diff on expand.
    pub fn toggle_expand(&mut self, diff_data: &DiffData) {
        if let Some(e) = self.expanded.get_mut(self.selected_index) {
            *e = !*e;
            // Load diff content when expanding
            if *e {
                let path = self.get_selected_path(diff_data);
                if let Some(path) = path {
                    self.load_diff(&path);
                }
            }
        }
    }

    /// Get the file path for the currently selected entry.
    pub fn get_selected_path(&self, diff_data: &DiffData) -> Option<String> {
        let files = Self::unique_files(diff_data);
        let mut all_entries: Vec<Entry> = files.into_iter().map(Entry::Modified).collect();
        for turn in diff_data.get_session_diffs() {
            for p in &turn.files_created {
                all_entries.push(Entry::Created(p.clone()));
            }
            for p in &turn.files_deleted {
                all_entries.push(Entry::Deleted(p.clone()));
            }
        }
        all_entries
            .get(self.selected_index)
            .and_then(|e| e.path().map(String::from))
    }

    /// Render the diff viewer as a full-screen overlay
    pub fn render(&self, frame: &mut Frame, area: Rect, diff_data: &DiffData, theme: &Theme) {
        if let Some(ref file) = self.detail_file {
            self.render_file_detail(frame, area, file, theme);
            return;
        }
        self.render_file_list(frame, area, diff_data, theme);
    }

    /// Render the file list view (default mode)
    fn render_file_list(&self, frame: &mut Frame, area: Rect, diff_data: &DiffData, theme: &Theme) {
        // Clear the area first
        frame.render_widget(Clear, area);

        let files = Self::unique_files(diff_data);
        let total_adds = diff_data.total_additions();
        let total_dels = diff_data.total_deletions();

        let title = format!(
            " Diff Viewer ({} files, +{} -{}) ",
            files.len(),
            total_adds,
            total_dels
        );
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(Line::from(Span::styled(
                title,
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            )))
            .title_bottom(Line::from(Span::styled(
                " j/k: navigate | Enter: view diff | Esc: close ",
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
            let mut all_entries: Vec<Entry> = files.into_iter().map(Entry::Modified).collect();
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
                let cursor = if is_selected { "▸" } else { " " };
                let expand_icon = crate::a11y::expand_icon(is_expanded);
                let sel_bg = if is_selected {
                    theme.context_bar_bg
                } else {
                    ratatui::style::Color::Reset
                };

                let line = match entry {
                    Entry::Modified(fc) => {
                        let fname = truncate_path(&fc.path, inner_width.saturating_sub(20));
                        let changes = format!("+{} -{}", fc.additions, fc.deletions);
                        let sel_style = Style::default().bg(sel_bg);
                        Line::from(vec![
                            Span::styled(
                                format!("{cursor} "),
                                Style::default().fg(theme.primary).bg(sel_bg),
                            ),
                            Span::styled(
                                format!("{expand_icon} "),
                                Style::default().fg(theme.muted).bg(sel_bg),
                            ),
                            Span::styled(
                                fname,
                                sel_style.fg(if is_selected {
                                    theme.primary
                                } else {
                                    theme.text
                                }),
                            ),
                            Span::styled(" ", sel_style),
                            Span::styled(changes, Style::default().fg(theme.muted).bg(sel_bg)),
                        ])
                    }
                    Entry::Created(p) => {
                        let fname = truncate_path(p, inner_width.saturating_sub(20));
                        Line::from(vec![
                            Span::styled(
                                format!("{cursor} "),
                                Style::default().fg(theme.primary).bg(sel_bg),
                            ),
                            Span::styled("+ ", Style::default().fg(theme.success).bg(sel_bg)),
                            Span::styled(
                                fname,
                                Style::default()
                                    .fg(if is_selected {
                                        theme.success
                                    } else {
                                        theme.text
                                    })
                                    .bg(sel_bg),
                            ),
                            Span::styled(" new", Style::default().fg(theme.success).bg(sel_bg)),
                        ])
                    }
                    Entry::Deleted(p) => {
                        let fname = truncate_path(p, inner_width.saturating_sub(20));
                        Line::from(vec![
                            Span::styled(
                                format!("{cursor} "),
                                Style::default().fg(theme.primary).bg(sel_bg),
                            ),
                            Span::styled("x ", Style::default().fg(theme.error).bg(sel_bg)),
                            Span::styled(
                                fname,
                                Style::default()
                                    .fg(if is_selected { theme.error } else { theme.text })
                                    .bg(sel_bg),
                            ),
                            Span::styled(" deleted", Style::default().fg(theme.error).bg(sel_bg)),
                        ])
                    }
                };
                items.push(ListItem::new(line));

                // Show actual diff when expanded
                if is_expanded {
                    if let Some(path) = entry.path() {
                        if let Some(diff_lines) = self.diff_cache.get(path) {
                            if diff_lines.is_empty() {
                                items.push(ListItem::new(Line::from(Span::styled(
                                    "  (no changes)",
                                    Style::default().fg(theme.text_dim),
                                ))));
                            } else {
                                for line in diff_lines.iter() {
                                    let display = truncate_to(&format!("  {line}"), inner_width);
                                    items.push(ListItem::new(Line::from(Span::styled(
                                        display,
                                        diff_line_style(line, theme),
                                    ))));
                                }
                            }
                        } else {
                            items.push(ListItem::new(Line::from(Span::styled(
                                "  (loading diff...)",
                                Style::default().fg(theme.text_dim),
                            ))));
                        }
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
        let visible_items: Vec<ListItem> =
            items.into_iter().skip(start).take(visible_rows).collect();

        let list = List::new(visible_items);
        frame.render_widget(list, inner);
    }

    /// Render a single file's diff in full detail
    fn render_file_detail(&self, frame: &mut Frame, area: Rect, file_path: &str, theme: &Theme) {
        frame.render_widget(Clear, area);

        let title = format!(" Diff: {file_path} ");
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(Line::from(Span::styled(
                title,
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            )))
            .title_bottom(Line::from(Span::styled(
                " j/k: scroll | Backspace: back | Esc: close ",
                Style::default().fg(theme.muted),
            )));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let mut items: Vec<ListItem> = Vec::new();
        let inner_width = inner.width as usize;

        if let Some(diff_lines) = self.diff_cache.get(file_path) {
            if diff_lines.is_empty() {
                items.push(ListItem::new(Line::from(Span::styled(
                    "(no changes or diff not loaded)",
                    Style::default().fg(theme.text_dim),
                ))));
            } else {
                for line in diff_lines {
                    let display = truncate_to(line, inner_width);
                    items.push(ListItem::new(Line::from(Span::styled(
                        display,
                        diff_line_style(line, theme),
                    ))));
                }
            }
        } else {
            items.push(ListItem::new(Line::from(Span::styled(
                "(diff not available — try reloading)",
                Style::default().fg(theme.text_dim),
            ))));
        }

        let visible_rows = inner.height as usize;
        let total = items.len();
        let start = if total > visible_rows {
            self.scroll_offset.min(total.saturating_sub(visible_rows))
        } else {
            0
        };
        let visible_items: Vec<ListItem> =
            items.into_iter().skip(start).take(visible_rows).collect();
        frame.render_widget(List::new(visible_items), inner);
    }
}

impl Default for DiffViewerWidget {
    fn default() -> Self {
        Self::new()
    }
}

/// Action state for a diff hunk in interactive mode
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HunkAction {
    Pending,
    Accepted,
    Rejected,
}

/// A diff hunk with interactive accept/reject state
#[derive(Debug, Clone)]
pub struct InteractiveHunk {
    pub start_line: usize,
    pub lines: Vec<String>,
    pub action: HunkAction,
    pub file_path: Option<String>,
}

impl InteractiveHunk {
    /// Parse hunks from a unified diff string
    pub fn parse_from_diff(diff: &str, file_path: Option<String>) -> Vec<Self> {
        let mut hunks = Vec::new();
        let mut current_lines: Vec<String> = Vec::new();
        let mut current_start = 0;
        let mut in_hunk = false;

        for (i, line) in diff.lines().enumerate() {
            if line.starts_with("@@") {
                if in_hunk && !current_lines.is_empty() {
                    hunks.push(Self {
                        start_line: current_start,
                        lines: current_lines.clone(),
                        action: HunkAction::Pending,
                        file_path: file_path.clone(),
                    });
                    current_lines.clear();
                }
                in_hunk = true;
                current_start = i;
                current_lines.push(line.to_string());
            } else if in_hunk {
                if line.starts_with('+') || line.starts_with('-') || line.starts_with(' ') {
                    current_lines.push(line.to_string());
                } else if !line.starts_with('\\') {
                    // End of hunk (non-diff content)
                    if !current_lines.is_empty() {
                        hunks.push(Self {
                            start_line: current_start,
                            lines: current_lines.clone(),
                            action: HunkAction::Pending,
                            file_path: file_path.clone(),
                        });
                        current_lines.clear();
                    }
                    in_hunk = false;
                }
            }
        }

        if in_hunk && !current_lines.is_empty() {
            hunks.push(Self {
                start_line: current_start,
                lines: current_lines,
                action: HunkAction::Pending,
                file_path,
            });
        }

        hunks
    }

    /// Get only the accepted lines as the resulting content
    pub fn accepted_content(&self) -> String {
        if self.action == HunkAction::Rejected {
            return String::new();
        }
        self.lines
            .iter()
            .filter(|l| !l.starts_with('-') && !l.starts_with("@@"))
            .map(|l| {
                l.strip_prefix('+')
                    .unwrap_or(l.strip_prefix(' ').unwrap_or(l))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl DiffViewerWidget {
    /// Render with interactive hunk accept/reject controls
    pub fn render_interactive(
        &self,
        frame: &mut Frame,
        area: Rect,
        _diff_data: &DiffData,
        theme: &Theme,
        hunks: &[InteractiveHunk],
        selected_hunk: usize,
    ) {
        frame.render_widget(Clear, area);

        let title = " Diff Review — Interactive ";
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(Line::from(Span::styled(
                title,
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            )))
            .title_bottom(Line::from(Span::styled(
                " a: accept | r: reject | A: accept all | Esc: cancel | Enter: apply ",
                Style::default().fg(theme.muted),
            )));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let mut items: Vec<ListItem> = Vec::new();
        let inner_width = inner.width as usize;

        for (i, hunk) in hunks.iter().enumerate() {
            let is_selected = i == selected_hunk;
            let action_label = match hunk.action {
                HunkAction::Pending => "[a]ccept [r]eject",
                HunkAction::Accepted => "ACCEPTED",
                HunkAction::Rejected => "REJECTED",
            };
            let action_color = match hunk.action {
                HunkAction::Pending => theme.text_dim,
                HunkAction::Accepted => theme.success,
                HunkAction::Rejected => theme.error,
            };

            // Hunk header
            let header_style = if is_selected {
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.muted)
            };
            items.push(ListItem::new(Line::from(vec![
                Span::styled(
                    if is_selected { ">" } else { " " }.to_string(),
                    header_style,
                ),
                Span::styled(format!(" Hunk {}  ", i + 1), header_style),
                Span::styled(
                    format!(" {action_label}"),
                    Style::default().fg(action_color),
                ),
            ])));

            // Hunk content lines
            for line in &hunk.lines {
                let base_style = match hunk.action {
                    HunkAction::Accepted if line.starts_with('+') => {
                        Style::default().fg(theme.success)
                    }
                    HunkAction::Rejected if line.starts_with('-') => {
                        Style::default().fg(theme.error)
                    }
                    _ => diff_line_style(line, theme),
                };
                let display = truncate_to(&format!("  {line}"), inner_width);
                let style = if hunk.action == HunkAction::Rejected
                    && !line.starts_with('-')
                    && !line.starts_with("@@")
                {
                    base_style.add_modifier(Modifier::CROSSED_OUT)
                } else {
                    base_style
                };
                items.push(ListItem::new(Line::from(Span::styled(display, style))));
            }
        }

        if hunks.is_empty() {
            items.push(ListItem::new(Line::from(Span::styled(
                "No diff hunks to review.",
                Style::default().fg(theme.text_dim),
            ))));
        }

        let visible_rows = inner.height as usize;
        let start = if items.len() > visible_rows {
            self.scroll_offset
                .min(items.len().saturating_sub(visible_rows))
        } else {
            0
        };
        let visible_items: Vec<ListItem> =
            items.into_iter().skip(start).take(visible_rows).collect();
        frame.render_widget(List::new(visible_items), inner);
    }
}

/// Entry types for the diff viewer
enum Entry {
    Modified(FileChange),
    Created(String),
    Deleted(String),
}

impl Entry {
    /// Get the file path for this entry.
    fn path(&self) -> Option<&str> {
        match self {
            Entry::Modified(fc) => Some(&fc.path),
            Entry::Created(p) | Entry::Deleted(p) => Some(p),
        }
    }
}

/// Truncate a file path to fit within max display columns, keeping the filename
fn truncate_path(path: &str, max_chars: usize) -> String {
    let w = unicode_width::UnicodeWidthStr::width(path);
    if w <= max_chars {
        return path.to_string();
    }
    // Try to keep the filename
    if let Some(slash_idx) = path.rfind('/') {
        let fname = &path[slash_idx + 1..];
        let fname_w = unicode_width::UnicodeWidthStr::width(fname);
        if fname_w + 4 <= max_chars {
            let budget = max_chars - fname_w - 4;
            let mut len = 0;
            let prefix: String = path
                .chars()
                .take_while(|c| {
                    let cw = unicode_width::UnicodeWidthChar::width(*c).unwrap_or(0);
                    if len + cw > budget {
                        false
                    } else {
                        len += cw;
                        true
                    }
                })
                .collect();
            return format!("{prefix}...{fname}");
        }
    }
    truncate_to(path, max_chars)
}

/// Truncate string to fit within max display columns
fn truncate_to(s: &str, max_chars: usize) -> String {
    let w = unicode_width::UnicodeWidthStr::width(s);
    if w <= max_chars {
        s.to_string()
    } else if max_chars > 1 {
        let mut len = 0;
        let truncated: String = s
            .chars()
            .take_while(|c| {
                let cw = unicode_width::UnicodeWidthChar::width(*c).unwrap_or(0);
                if len + cw > max_chars - 1 {
                    false
                } else {
                    len += cw;
                    true
                }
            })
            .collect();
        format!("{truncated}…")
    } else {
        "…".to_string()
    }
}
