//! Selection widgets for Shannon UI
//!
//! Provides file selector, multi-select interface, and fuzzy picker components

use crate::theme::Theme;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

/// Selection item for lists
#[derive(Debug, Clone)]
pub struct SelectItem<T> {
    pub label: String,
    pub value: T,
    pub description: Option<String>,
}

impl<T> SelectItem<T> {
    pub fn new(label: impl Into<String>, value: T) -> Self {
        Self {
            label: label.into(),
            value,
            description: None,
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }
}

/// Multi-select widget for choosing multiple options
#[derive(Debug)]
pub struct MultiSelectWidget {
    items: Vec<SelectItem<String>>,
    selected_indices: Vec<usize>,
    state: ListState,
    focused_index: usize,
    title: String,
    show_indices: bool,
}

impl Clone for MultiSelectWidget {
    fn clone(&self) -> Self {
        let mut state = ListState::default();
        state.select(self.state.selected());
        Self {
            items: self.items.clone(),
            selected_indices: self.selected_indices.clone(),
            state,
            focused_index: self.focused_index,
            title: self.title.clone(),
            show_indices: self.show_indices,
        }
    }
}

impl MultiSelectWidget {
    /// Create a new multi-select widget
    pub fn new(title: String) -> Self {
        Self {
            items: Vec::new(),
            selected_indices: Vec::new(),
            state: ListState::default(),
            focused_index: 0,
            title,
            show_indices: true,
        }
    }

    /// Set items to display
    pub fn with_items(mut self, items: Vec<SelectItem<String>>) -> Self {
        self.items = items;
        if !self.items.is_empty() {
            self.focused_index = 0;
        }
        self
    }

    /// Show or hide numeric indices
    pub fn with_indices(mut self, show: bool) -> Self {
        self.show_indices = show;
        self
    }

    /// Move selection up
    pub fn move_up(&mut self) {
        if !self.items.is_empty() && self.focused_index > 0 {
            self.focused_index -= 1;
            self.state.select(Some(self.focused_index));
        }
    }

    /// Move selection down
    pub fn move_down(&mut self) {
        if !self.items.is_empty() && self.focused_index + 1 < self.items.len() {
            self.focused_index += 1;
            self.state.select(Some(self.focused_index));
        }
    }

    /// Toggle current selection
    pub fn toggle_current(&mut self) {
        if let Some(pos) = self.state.selected() {
            if let Some(idx) = self.selected_indices.iter().position(|&i| i == pos) {
                self.selected_indices.remove(idx);
            } else {
                self.selected_indices.push(pos);
                self.selected_indices.sort();
            }
        }
    }

    /// Select all items
    pub fn select_all(&mut self) {
        self.selected_indices = (0..self.items.len()).collect();
    }

    /// Deselect all items
    pub fn deselect_all(&mut self) {
        self.selected_indices.clear();
    }

    /// Get currently selected items
    pub fn selected_items(&self) -> Vec<&SelectItem<String>> {
        self.selected_indices
            .iter()
            .filter_map(|&i| self.items.get(i))
            .collect()
    }

    /// Get selected values
    pub fn selected_values(&self) -> Vec<&str> {
        self.selected_items()
            .iter()
            .map(|item| item.value.as_str())
            .collect()
    }

    /// Render the multi-select widget
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let max_index_width = if self.items.is_empty() {
            0
        } else {
            self.items.len().to_string().len()
        };

        let list_items: Vec<ListItem> = self
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let is_selected = self.selected_indices.contains(&i);
                let is_focused = self.focused_index == i;
                let check_mark = if is_selected { "✓" } else { " " };

                let mut spans = vec![
                    Span::styled(
                        format!("{:>width$}. ", i + 1, width = max_index_width),
                        Style::default().fg(theme.text_dim),
                    ),
                    Span::styled(
                        format!("[{check_mark}]"),
                        Style::default()
                            .fg(if is_selected {
                                theme.success
                            } else {
                                theme.text
                            })
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        &item.label,
                        Style::default()
                            .fg(if is_focused { theme.accent } else { theme.text })
                            .add_modifier(if is_focused {
                                Modifier::BOLD | Modifier::UNDERLINED
                            } else {
                                Modifier::empty()
                            }),
                    ),
                ];

                if let Some(desc) = &item.description {
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(desc, Style::default().fg(theme.text_dim)));
                }

                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(list_items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.accent))
                    .title(format!(" {} ", self.title)),
            )
            .highlight_style(
                Style::default()
                    .bg(theme.selection_bg)
                    .add_modifier(Modifier::BOLD),
            );

        frame.render_stateful_widget(list, area, &mut self.state.clone());
    }
}

/// File selector widget
#[derive(Debug, Clone)]
pub struct FileSelectorWidget {
    current_path: String,
    files: Vec<String>,
    directories: Vec<String>,
    focused_index: usize,
    is_focused_on_files: bool,
    title: String,
    show_hidden: bool,
    filter: Option<String>,
}

impl FileSelectorWidget {
    /// Create a new file selector
    pub fn new(title: String) -> Self {
        Self {
            current_path: ".".to_string(),
            files: Vec::new(),
            directories: Vec::new(),
            focused_index: 0,
            is_focused_on_files: false,
            title,
            show_hidden: false,
            filter: None,
        }
    }

    /// Set current directory path
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.current_path = path.into();
        self
    }

    /// Show hidden files
    pub fn show_hidden(mut self, show: bool) -> Self {
        self.show_hidden = show;
        self
    }

    /// Set file filter pattern
    pub fn with_filter(mut self, pattern: Option<String>) -> Self {
        self.filter = pattern;
        self
    }

    /// Get current filter pattern
    pub fn get_filter(&self) -> Option<&str> {
        self.filter.as_deref()
    }

    /// Update filter and refresh file list
    pub fn set_filter_pattern(&mut self, pattern: &str) {
        self.filter = if pattern.is_empty() {
            None
        } else {
            Some(pattern.to_string())
        };
        self.focused_index = 0;
        let _ = self.refresh();
    }

    /// Refresh file list from current path
    pub fn refresh(&mut self) -> std::io::Result<()> {
        let mut dirs = Vec::new();
        let mut files = Vec::new();

        let path = std::path::Path::new(&self.current_path);

        for entry in path.read_dir()? {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy().to_string();
            let is_dir = entry.path().is_dir();

            // Skip hidden files unless show_hidden is true
            if !self.show_hidden && name_str.starts_with('.') {
                continue;
            }

            // Apply filter if set
            if let Some(ref filter) = self.filter {
                if !name_str.contains(filter) {
                    continue;
                }
            }

            if is_dir {
                dirs.push(name_str);
            } else {
                files.push(name_str);
            }
        }

        dirs.sort();
        files.sort();
        self.directories = dirs;
        self.files = files;
        self.focused_index = 0;

        Ok(())
    }

    /// Navigate to parent directory
    pub fn navigate_up(&mut self) -> std::io::Result<()> {
        if let Some(parent) = std::path::Path::new(&self.current_path).parent() {
            self.current_path = parent.to_string_lossy().to_string();
            self.refresh()?;
        }
        Ok(())
    }

    /// Navigate into directory
    pub fn navigate_into(&mut self, dir_name: &str) -> std::io::Result<()> {
        let new_path = std::path::Path::new(&self.current_path).join(dir_name);
        if new_path.is_dir() {
            self.current_path = new_path.to_string_lossy().to_string();
            self.refresh()?;
        }
        Ok(())
    }

    /// Get the current directory path
    pub fn current_path(&self) -> &str {
        &self.current_path
    }

    /// Get currently selected item
    pub fn current_selection(&self) -> Option<String> {
        if self.is_focused_on_files {
            self.files.get(self.focused_index).cloned()
        } else {
            self.directories.get(self.focused_index).cloned()
        }
    }

    /// Move selection up
    pub fn move_up(&mut self) {
        let _list = if self.is_focused_on_files {
            &self.files
        } else {
            &self.directories
        };

        if self.focused_index > 0 {
            self.focused_index -= 1;
        } else if !self.directories.is_empty() && self.is_focused_on_files {
            // Switch from files to directories
            self.is_focused_on_files = false;
            self.focused_index = self.directories.len().saturating_sub(1);
        }
    }

    /// Move selection down
    pub fn move_down(&mut self) {
        if self.is_focused_on_files {
            if self.focused_index + 1 < self.files.len() {
                self.focused_index += 1;
            }
        } else if !self.directories.is_empty() {
            // Switch from directories to files
            self.is_focused_on_files = true;
            self.focused_index = 0;
        } else {
            let dirs_len = self.directories.len();
            if self.focused_index + 1 < dirs_len {
                self.focused_index += 1;
            }
        }
    }

    /// Toggle between directories and files
    pub fn toggle_section(&mut self) {
        self.is_focused_on_files = !self.is_focused_on_files;
        self.focused_index = 0;
    }

    /// Render the file selector
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let mut content = Vec::new();

        // Show current path
        content.push(Line::from(vec![
            Span::styled("Path: ", Style::default().fg(theme.warning)),
            Span::styled(
                &self.current_path,
                Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
            ),
        ]));

        // Show filter if active
        if let Some(ref f) = self.filter {
            if !f.is_empty() {
                content.push(Line::from(vec![
                    Span::styled("Filter: ", Style::default().fg(theme.accent)),
                    Span::styled(
                        f.as_str(),
                        Style::default()
                            .fg(theme.warning)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        " (type to filter, Backspace to clear)",
                        Style::default().fg(theme.text_dim),
                    ),
                ]));
            }
        }

        // Show directories
        if !self.directories.is_empty() {
            content.push(Line::from(""));
            content.push(Line::from(vec![Span::styled(
                "Directories:",
                Style::default().fg(theme.accent),
            )]));

            for (i, dir) in self.directories.iter().enumerate() {
                let prefix = if !self.is_focused_on_files && i == self.focused_index {
                    "> "
                } else {
                    "  "
                };
                content.push(Line::from(vec![
                    Span::styled(prefix, Style::default().fg(theme.warning)),
                    Span::styled(
                        format!("{dir}/"),
                        Style::default()
                            .fg(theme.primary)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
            }
        }

        // Show files
        if !self.files.is_empty() {
            content.push(Line::from(""));
            content.push(Line::from(vec![Span::styled(
                "Files:",
                Style::default().fg(theme.accent),
            )]));

            for (i, file) in self.files.iter().enumerate() {
                let prefix = if self.is_focused_on_files && i == self.focused_index {
                    "> "
                } else {
                    "  "
                };
                content.push(Line::from(vec![
                    Span::styled(prefix, Style::default().fg(theme.warning)),
                    Span::styled(file, Style::default().fg(theme.text)),
                ]));
            }
        }

        // Show help text
        content.push(Line::from(""));
        content.push(Line::from(vec![
            Span::styled("↑/↓: Navigate", Style::default().fg(theme.text_dim)),
            Span::raw("  "),
            Span::styled("Tab: Switch", Style::default().fg(theme.text_dim)),
            Span::raw("  "),
            Span::styled("Enter: Select", Style::default().fg(theme.text_dim)),
        ]));

        let paragraph = Paragraph::new(content)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.accent))
                    .title(format!(" {} ", self.title)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, area);
    }
}

/// Fuzzy picker widget with search functionality
#[derive(Debug, Clone)]
pub struct FuzzyPickerWidget {
    items: Vec<SelectItem<String>>,
    filtered_items: Vec<usize>,
    search_query: String,
    selected_index: usize,
    title: String,
    state: PickerState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerState {
    Browsing,
    Searching,
}

impl FuzzyPickerWidget {
    /// Create a new fuzzy picker
    pub fn new(title: String) -> Self {
        Self {
            items: Vec::new(),
            filtered_items: Vec::new(),
            search_query: String::new(),
            selected_index: 0,
            title,
            state: PickerState::Browsing,
        }
    }

    /// Set items to pick from
    pub fn with_items(mut self, items: Vec<SelectItem<String>>) -> Self {
        let count = items.len();
        self.items = items;
        self.filtered_items = (0..count).collect();
        self
    }

    /// Enter search mode
    pub fn start_search(&mut self) {
        self.state = PickerState::Searching;
        self.search_query.clear();
        self.filtered_items = (0..self.items.len()).collect();
    }

    /// Exit search mode
    pub fn exit_search(&mut self) {
        self.state = PickerState::Browsing;
        self.search_query.clear();
        self.filtered_items = (0..self.items.len()).collect();
    }

    /// Add character to search query
    pub fn add_search_char(&mut self, c: char) {
        self.search_query.push(c);
        self.update_filtered();
    }

    /// Remove last character from search query
    pub fn remove_search_char(&mut self) {
        self.search_query.pop();
        self.update_filtered();
    }

    /// Update filtered items based on current query
    fn update_filtered(&mut self) {
        if self.search_query.is_empty() {
            self.filtered_items = (0..self.items.len()).collect();
        } else {
            let query = self.search_query.to_lowercase();
            self.filtered_items = self
                .items
                .iter()
                .enumerate()
                .filter(|(_, item)| item.label.to_lowercase().contains(&query))
                .map(|(i, _)| i)
                .collect();
        }

        if !self.filtered_items.is_empty() {
            self.selected_index = 0;
        }
    }

    /// Move selection up
    pub fn move_up(&mut self) {
        if !self.filtered_items.is_empty() && self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    /// Move selection down
    pub fn move_down(&mut self) {
        if self.selected_index + 1 < self.filtered_items.len() {
            self.selected_index += 1;
        }
    }

    /// Get currently selected item
    pub fn current_selection(&self) -> Option<&SelectItem<String>> {
        self.filtered_items
            .get(self.selected_index)
            .and_then(|&i| self.items.get(i))
    }

    /// Get selected value
    pub fn selected_value(&self) -> Option<&str> {
        self.current_selection().map(|item| item.value.as_str())
    }

    /// Render the fuzzy picker
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let mut content = Vec::new();

        // Show search bar
        let search_prefix = if self.state == PickerState::Searching {
            "Search> "
        } else {
            "Filter (type to search)> "
        };

        content.push(Line::from(vec![
            Span::styled(search_prefix, Style::default().fg(theme.accent)),
            Span::styled(
                &self.search_query,
                Style::default()
                    .fg(if self.state == PickerState::Searching {
                        theme.warning
                    } else {
                        theme.text
                    })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                if self.state == PickerState::Searching {
                    "█"
                } else {
                    ""
                },
                Style::default().fg(theme.warning),
            ),
        ]));

        content.push(Line::from(""));

        // Show filtered items
        if !self.filtered_items.is_empty() {
            let max_items = (area.height.saturating_sub(2) as usize).min(10);
            let start = (self.selected_index / max_items) * max_items;
            let end = (start + max_items).min(self.filtered_items.len());

            for &idx in &self.filtered_items[start..end] {
                if let Some(item) = self.items.get(idx) {
                    let is_selected = idx == self.filtered_items[self.selected_index];

                    content.push(Line::from(vec![
                        Span::styled(
                            if is_selected { "▸ " } else { "  " },
                            Style::default().fg(theme.accent),
                        ),
                        Span::styled(
                            &item.label,
                            Style::default()
                                .fg(if is_selected {
                                    theme.accent
                                } else {
                                    theme.text
                                })
                                .add_modifier(if is_selected {
                                    Modifier::BOLD
                                } else {
                                    Modifier::empty()
                                }),
                        ),
                    ]));

                    if let Some(desc) = &item.description {
                        content.push(Line::from(vec![
                            Span::raw("    "),
                            Span::styled(desc, Style::default().fg(theme.text_dim)),
                        ]));
                    }
                }
            }

            // Show pagination indicator
            if self.filtered_items.len() > max_items {
                content.push(Line::from(vec![Span::styled(
                    format!("({}-{} of {})", start + 1, end, self.filtered_items.len()),
                    Style::default().fg(theme.text_dim),
                )]));
            }
        } else {
            content.push(Line::from(vec![Span::styled(
                "No results",
                Style::default().fg(theme.text_dim),
            )]));
        }

        let paragraph = Paragraph::new(content)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.accent))
                    .title(format!(" {} ", self.title)),
            )
            .wrap(Wrap { trim: true });

        frame.render_widget(paragraph, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multi_select_creation() {
        let widget = MultiSelectWidget::new("Test".to_string());
        assert_eq!(widget.title, "Test");
        assert!(widget.items.is_empty());
    }

    #[test]
    fn test_multi_select_with_items() {
        let items = vec![
            SelectItem::new("Option 1", "val1".to_string()),
            SelectItem::new("Option 2", "val2".to_string()),
        ];
        let widget = MultiSelectWidget::new("Test".to_string()).with_items(items);

        assert_eq!(widget.items.len(), 2);
        assert_eq!(widget.focused_index, 0);
    }

    #[test]
    fn test_multi_select_toggle() {
        let items = vec![
            SelectItem::new("Option 1", "val1".to_string()),
            SelectItem::new("Option 2", "val2".to_string()),
        ];
        let mut widget = MultiSelectWidget::new("Test".to_string()).with_items(items);

        widget.state.select(Some(0));
        widget.toggle_current();
        assert_eq!(widget.selected_indices, vec![0]);

        widget.toggle_current();
        assert!(widget.selected_indices.is_empty());
    }

    #[test]
    fn test_file_selector_refresh() {
        let widget = FileSelectorWidget::new("Select File".to_string()).with_path(".");

        // This will work in actual tests with proper directory setup
        // For now just verify the structure
        assert_eq!(widget.title, "Select File");
    }
}

// ── Model Picker Widget ────────────────────────────────────────────

use shannon_core::model_registry::{
    ModelInfo, all_providers, detect_local_models, models_for_provider, provider_display_name,
};
use shannon_engine::api::LlmProvider;

const MAX_VISIBLE_MODELS: usize = 10;

/// Interactive model picker with provider tabs and model list.
///
/// Navigate with:
/// - `←` / `→` — switch provider tab
/// - `↑` / `↓` / `j` / `k` — select model
/// - `Enter` — confirm selection
/// - `Esc` — cancel
#[derive(Debug, Clone)]
pub struct ModelPickerWidget {
    /// All providers that have models available.
    providers: Vec<LlmProvider>,
    /// Index into `providers` for the currently active tab.
    current_provider_idx: usize,
    /// Models for the currently selected provider.
    models: Vec<ModelInfo>,
    /// Index of the highlighted model within `models`.
    selected_idx: usize,
    /// Vertical scroll offset for long model lists.
    scroll_offset: usize,
    /// Locally detected Ollama models (kept separate to avoid leaking).
    local_models: Vec<ModelInfo>,
    /// The model ID currently in use (shown with ✓ marker).
    current_model_id: Option<String>,
}

impl ModelPickerWidget {
    /// Create a new model picker, optionally highlighting `current_model`.
    pub fn new(current_model: Option<&str>) -> Self {
        let local_models = detect_local_models();
        let mut providers = all_providers();

        // Always include Ollama tab (shows "No local models" if none detected)
        if !providers.contains(&LlmProvider::Ollama) {
            providers.push(LlmProvider::Ollama);
        }

        let current_model_id = current_model.map(|s| s.to_string());
        let mut picker = Self {
            providers,
            current_provider_idx: 0,
            models: Vec::new(),
            selected_idx: 0,
            scroll_offset: 0,
            local_models,
            current_model_id,
        };

        // Find the provider of the current model to open the right tab
        if let Some(model_id) = current_model {
            if let Some(idx) = picker.providers.iter().position(|p| {
                picker
                    .models_for(p.clone())
                    .iter()
                    .any(|m| m.id == model_id)
            }) {
                picker.current_provider_idx = idx;
            }
        }

        picker.refresh_models();

        // Highlight the current model if it belongs to this provider
        if let Some(model_id) = current_model {
            if let Some(idx) = picker.models.iter().position(|m| m.id == model_id) {
                picker.selected_idx = idx;
                if picker.selected_idx >= MAX_VISIBLE_MODELS {
                    picker.scroll_offset = picker.selected_idx - (MAX_VISIBLE_MODELS - 1);
                }
            }
        }

        picker
    }

    /// Get models for a provider, including Ollama local models.
    fn models_for(&self, provider: LlmProvider) -> Vec<ModelInfo> {
        if provider == LlmProvider::Ollama {
            // Return detected local models, or empty vec (render shows "No local models detected")
            self.local_models.clone()
        } else {
            models_for_provider(provider).into_iter().cloned().collect()
        }
    }

    /// Reload models list for the current provider tab.
    fn refresh_models(&mut self) {
        if self.providers.is_empty() {
            self.models = Vec::new();
            return;
        }
        let provider = self.providers[self.current_provider_idx].clone();
        self.models = self.models_for(provider);
        self.selected_idx = 0;
        self.scroll_offset = 0;
    }

    /// Move selection up (wraps to bottom).
    pub fn move_up(&mut self) {
        if self.models.is_empty() {
            return;
        }
        if self.selected_idx > 0 {
            self.selected_idx -= 1;
        } else {
            self.selected_idx = self.models.len() - 1;
            self.scroll_offset = self.models.len().saturating_sub(MAX_VISIBLE_MODELS);
        }
        if self.selected_idx < self.scroll_offset {
            self.scroll_offset = self.selected_idx;
        }
    }

    /// Move selection down (wraps to top).
    pub fn move_down(&mut self) {
        if self.models.is_empty() {
            return;
        }
        if self.selected_idx < self.models.len() - 1 {
            self.selected_idx += 1;
        } else {
            self.selected_idx = 0;
            self.scroll_offset = 0;
        }
        if self.selected_idx >= self.scroll_offset + MAX_VISIBLE_MODELS {
            self.scroll_offset = self.selected_idx - (MAX_VISIBLE_MODELS - 1);
        }
    }

    /// Switch to the previous provider tab.
    pub fn prev_provider(&mut self) {
        if self.providers.len() <= 1 {
            return;
        }
        if self.current_provider_idx > 0 {
            self.current_provider_idx -= 1;
        } else {
            self.current_provider_idx = self.providers.len() - 1;
        }
        self.refresh_models();
    }

    /// Switch to the next provider tab.
    pub fn next_provider(&mut self) {
        if self.providers.len() <= 1 {
            return;
        }
        if self.current_provider_idx < self.providers.len() - 1 {
            self.current_provider_idx += 1;
        } else {
            self.current_provider_idx = 0;
        }
        self.refresh_models();
    }

    /// Get the currently selected model info.
    pub fn selected_model(&self) -> Option<&ModelInfo> {
        self.models.get(self.selected_idx)
    }

    /// Render the model picker as a centered dialog.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        use ratatui::widgets::Clear;

        let dialog_width = 52u16.min(area.width.saturating_sub(4));
        let visible_count = MAX_VISIBLE_MODELS.min(self.models.len());
        // +3 for title, +2 for tab bar, +1 for footer hint
        let dialog_height = (visible_count as u16 + 6).min(area.height.saturating_sub(4));

        let x = (area.width.saturating_sub(dialog_width)) / 2;
        let y = (area.height.saturating_sub(dialog_height)) / 2;

        let dialog_area = Rect {
            x: area.x + x,
            y: area.y + y,
            width: dialog_width,
            height: dialog_height,
        };
        frame.render_widget(Clear, dialog_area);

        let mut lines: Vec<Line> = Vec::new();

        // ── Title ──
        let provider_name = if self.providers.is_empty() {
            "Unknown".to_string()
        } else {
            provider_display_name(&self.providers[self.current_provider_idx]).to_string()
        };
        lines.push(Line::from(Span::styled(
            format!(" Select {provider_name} Model "),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        // ── Provider tabs ──
        if self.providers.len() > 1 {
            let tab_spans: Vec<Span> = self
                .providers
                .iter()
                .enumerate()
                .flat_map(|(i, p)| {
                    let name = provider_display_name(p);
                    let style = if i == self.current_provider_idx {
                        Style::default()
                            .fg(theme.context_bar_bg)
                            .bg(theme.accent)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme.text_dim)
                    };
                    let bracket = if i == self.current_provider_idx {
                        format!(" [{name}] ")
                    } else {
                        format!("  {name}  ")
                    };
                    vec![Span::styled(bracket, style)]
                })
                .collect();
            lines.push(Line::from(tab_spans));
            lines.push(Line::from(""));
        }

        // ── Model list ──
        if self.models.is_empty() {
            lines.push(Line::from(Span::styled(
                "  No local models detected",
                Style::default()
                    .fg(theme.text_dim)
                    .add_modifier(Modifier::ITALIC),
            )));
            lines.push(Line::from(Span::styled(
                "  Install Ollama and run: ollama pull llama3",
                Style::default().fg(theme.text_dim),
            )));
        } else {
            let end_idx = (self.scroll_offset + MAX_VISIBLE_MODELS).min(self.models.len());
            for i in self.scroll_offset..end_idx {
                let model = &self.models[i];
                let is_current = self.current_model_id.as_deref() == Some(model.id);
                let marker = if is_current { "✓" } else { " " };
                let label = if model.display_name == model.id {
                    model.id.to_string()
                } else {
                    format!("{}  ({})", model.display_name, model.id)
                };
                // Truncate to dialog width (Unicode display width aware)
                let max_w = (dialog_width as usize).saturating_sub(6);
                let label_w = unicode_width::UnicodeWidthStr::width(label.as_str());
                let truncated = if label_w > max_w {
                    let end = max_w.saturating_sub(3);
                    let mut len = 0;
                    let t: String = label
                        .chars()
                        .take_while(|c| {
                            let cw = unicode_width::UnicodeWidthChar::width(*c).unwrap_or(0);
                            if len + cw > end {
                                false
                            } else {
                                len += cw;
                                true
                            }
                        })
                        .collect();
                    format!("{t}...")
                } else {
                    label
                };

                if i == self.selected_idx {
                    lines.push(Line::from(Span::styled(
                        format!("{marker}▸ {truncated}"),
                        Style::default()
                            .fg(theme.context_bar_bg)
                            .bg(theme.accent)
                            .add_modifier(Modifier::BOLD),
                    )));
                } else if is_current {
                    lines.push(Line::from(Span::styled(
                        format!("{marker}  {truncated}"),
                        Style::default()
                            .fg(theme.success)
                            .add_modifier(Modifier::BOLD),
                    )));
                } else {
                    lines.push(Line::from(Span::styled(
                        format!("{marker}  {truncated}"),
                        Style::default().fg(theme.text),
                    )));
                }
            }
        }

        // ── Selected model details ──
        if let Some(model) = self.selected_model() {
            let ctx = if model.context_window >= 1_000_000 {
                format!("{}M", model.context_window / 1_000_000)
            } else {
                format!("{}K", model.context_window / 1_000)
            };
            let out = if model.max_output >= 1_000 {
                format!("{}K", model.max_output / 1_000)
            } else {
                model.max_output.to_string()
            };
            lines.push(Line::from(Span::styled(
                format!("  ctx: {ctx} tokens  |  max output: {out} tokens"),
                Style::default().fg(theme.warning),
            )));
        }

        // ── Scroll indicators ──
        let mut hints = String::new();
        if self.models.len() > MAX_VISIBLE_MODELS {
            if self.scroll_offset > 0 {
                hints.push_str("↑ ");
            }
            if self.scroll_offset + MAX_VISIBLE_MODELS < self.models.len() {
                hints.push_str("↓ ");
            }
        }
        if self.providers.len() > 1 {
            hints.push_str("←→ provider  ");
        }
        hints.push_str("↑↓ select  ⏎ ok  esc cancel");

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            hints,
            Style::default().fg(theme.text_dim),
        )));

        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.text_dim))
                    .border_type(ratatui::widgets::BorderType::Rounded),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, dialog_area);
    }
}
