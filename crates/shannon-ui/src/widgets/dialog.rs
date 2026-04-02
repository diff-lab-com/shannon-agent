//! Dialog widgets for Shannon UI
//!
//! Provides modal dialogs for user interaction and confirmation

use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

/// Dialog button for user actions
#[derive(Debug, Clone)]
pub struct DialogButton {
    pub label: String,
    pub action: String,
    pub is_primary: bool,
    pub is_dangerous: bool,
}

impl DialogButton {
    /// Create a new dialog button
    pub fn new(label: String, action: String) -> Self {
        Self {
            label,
            action,
            is_primary: false,
            is_dangerous: false,
        }
    }

    /// Mark as primary button
    pub fn primary(mut self) -> Self {
        self.is_primary = true;
        self
    }

    /// Mark as dangerous button
    pub fn dangerous(mut self) -> Self {
        self.is_dangerous = true;
        self
    }
}

/// Dialog widget for modal interactions
#[derive(Clone)]
pub struct DialogWidget {
    title: String,
    subtitle: Option<String>,
    content: Vec<String>,
    buttons: Vec<DialogButton>,
    selected_button: usize,
    width: u16,
    height: u16,
    closable: bool,
}

impl DialogWidget {
    /// Create a new dialog
    pub fn new(title: String) -> Self {
        Self {
            title,
            subtitle: None,
            content: Vec::new(),
            buttons: Vec::new(),
            selected_button: 0,
            width: 60,
            height: 20,
            closable: true,
        }
    }

    /// Set the subtitle
    pub fn with_subtitle(mut self, subtitle: String) -> Self {
        self.subtitle = Some(subtitle);
        self
    }

    /// Add content line
    pub fn with_content(mut self, content: String) -> Self {
        self.content.push(content);
        self
    }

    /// Set all content at once
    pub fn set_content(mut self, content: Vec<String>) -> Self {
        self.content = content;
        self
    }

    /// Add a button
    pub fn with_button(mut self, button: DialogButton) -> Self {
        self.buttons.push(button);
        if self.buttons.len() == 1 {
            self.selected_button = 0;
        }
        self
    }

    /// Set dialog size
    pub fn with_size(mut self, width: u16, height: u16) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// Set whether dialog is closable with Esc
    pub fn with_closable(mut self, closable: bool) -> Self {
        self.closable = closable;
        self
    }

    /// Navigate to next button
    pub fn next_button(&mut self) {
        if !self.buttons.is_empty() {
            self.selected_button = (self.selected_button + 1) % self.buttons.len();
        }
    }

    /// Navigate to previous button
    pub fn prev_button(&mut self) {
        if !self.buttons.is_empty() {
            self.selected_button = if self.selected_button == 0 {
                self.buttons.len() - 1
            } else {
                self.selected_button - 1
            };
        }
    }

    /// Get the currently selected button action
    pub fn selected_action(&self) -> Option<&str> {
        self.buttons.get(self.selected_button).map(|b| b.action.as_str())
    }

    /// Get button by action
    pub fn get_button(&self, action: &str) -> Option<&DialogButton> {
        self.buttons.iter().find(|b| b.action == action)
    }

    /// Render the dialog
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        // Calculate centered dialog area
        let dialog_width = self.width.min(area.width.saturating_sub(4));
        let dialog_height = self.height.min(area.height.saturating_sub(4));

        let x = (area.width.saturating_sub(dialog_width)) / 2;
        let y = (area.height.saturating_sub(dialog_height)) / 2;

        let dialog_area = Rect {
            x: area.x + x,
            y: area.y + y,
            width: dialog_width,
            height: dialog_height,
        };

        // Clear the background for modal effect
        frame.render_widget(Clear, dialog_area);

        // Build dialog content
        let mut content_lines = Vec::new();

        // Title
        content_lines.push(Line::from(vec![
            Span::styled(
                &self.title,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        content_lines.push(Line::from(""));

        // Subtitle
        if let Some(ref subtitle) = self.subtitle {
            for line in Self::wrap_text(subtitle, dialog_width.saturating_sub(4) as usize) {
                content_lines.push(Line::from(vec![
                    Span::styled(line, Style::default().fg(Color::Gray)),
                ]));
            }
            content_lines.push(Line::from(""));
        }

        // Content
        for line in &self.content {
            for wrapped in Self::wrap_text(line, dialog_width.saturating_sub(4) as usize) {
                content_lines.push(Line::from(wrapped));
            }
        }

        content_lines.push(Line::from(""));

        // Buttons
        let button_row = self.render_button_row();
        content_lines.push(button_row);

        // Help text
        if self.closable && !self.buttons.is_empty() {
            content_lines.push(Line::from(""));
            content_lines.push(Line::from(vec![
                Span::styled("Enter: Select  ", Style::default().fg(Color::DarkGray)),
                Span::styled("←/→: Navigate", Style::default().fg(Color::DarkGray)),
                if self.closable {
                    Span::styled("  Esc: Close", Style::default().fg(Color::DarkGray))
                } else {
                    Span::raw("")
                },
            ]));
        }

        let paragraph = Paragraph::new(content_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan))
                    .border_type(ratatui::widgets::BorderType::Rounded)
            )
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });

        frame.render_widget(paragraph, dialog_area);
    }

    /// Render button row
    fn render_button_row(&self) -> Line<'static> {
        let mut spans: Vec<Span<'static>> = Vec::new();
        let button_spacing = "  ";

        for (i, button) in self.buttons.iter().enumerate() {
            let is_selected = i == self.selected_button;

            let style = if button.is_dangerous {
                Style::default().fg(if is_selected {
                    Color::Red
                } else {
                    Color::DarkGray
                }).add_modifier(Modifier::BOLD)
            } else if button.is_primary {
                Style::default().fg(if is_selected {
                    Color::Green
                } else {
                    Color::DarkGray
                }).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(if is_selected {
                    Color::White
                } else {
                    Color::DarkGray
                }).add_modifier(if is_selected {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                })
            };

            let prefix = if is_selected { "▸ " } else { "  " };
            let suffix = if is_selected { " ▸" } else { "  " };

            spans.push(Span::styled(prefix, style));
            spans.push(Span::styled(button.label.clone(), style));
            spans.push(Span::styled(suffix, style));

            if i < self.buttons.len() - 1 {
                spans.push(Span::raw(button_spacing));
            }
        }

        Line::from(spans)
    }

    /// Wrap text to fit width
    fn wrap_text(text: &str, width: usize) -> Vec<String> {
        let mut lines = Vec::new();
        let mut current_line = String::new();
        let mut current_length = 0;

        for word in text.split_whitespace() {
            let word_len = word.len();

            if current_length == 0 {
                current_line.push_str(word);
                current_length = word_len;
            } else if current_length + 1 + word_len <= width {
                current_line.push(' ');
                current_line.push_str(word);
                current_length += 1 + word_len;
            } else {
                lines.push(current_line);
                current_line = word.to_string();
                current_length = word_len;
            }
        }

        if !current_line.is_empty() {
            lines.push(current_line);
        }

        if lines.is_empty() {
            lines.push(text.to_string());
        }

        lines
    }
}

impl Default for DialogWidget {
    fn default() -> Self {
        Self::new("Dialog".to_string())
    }
}

/// Confirmation dialog builder
pub struct ConfirmDialog {
    dialog: DialogWidget,
}

impl ConfirmDialog {
    /// Create a new confirmation dialog
    pub fn new(title: String) -> Self {
        let dialog = DialogWidget::new(title)
            .with_button(DialogButton::new("Cancel".to_string(), "cancel".to_string()))
            .with_button(
                DialogButton::new("Confirm".to_string(), "confirm".to_string())
                    .primary()
            );

        Self { dialog }
    }

    /// Set the confirmation message
    pub fn with_message(mut self, message: String) -> Self {
        self.dialog = self.dialog.with_content(message);
        self
    }

    /// Build the dialog
    pub fn build(self) -> DialogWidget {
        self.dialog
    }
}

/// Alert dialog builder
pub struct AlertDialog {
    dialog: DialogWidget,
}

impl AlertDialog {
    /// Create a new alert dialog
    pub fn new(title: String) -> Self {
        let dialog = DialogWidget::new(title)
            .with_button(
                DialogButton::new("OK".to_string(), "ok".to_string())
                    .primary()
            );

        Self { dialog }
    }

    /// Set the alert message
    pub fn with_message(mut self, message: String) -> Self {
        self.dialog = self.dialog.with_content(message);
        self
    }

    /// Mark as danger alert
    pub fn with_danger(mut self) -> Self {
        if let Some(button) = self.dialog.buttons.get_mut(0) {
            button.is_dangerous = true;
        }
        self
    }

    /// Build the dialog
    pub fn build(self) -> DialogWidget {
        self.dialog
    }
}

/// Input dialog for text input
pub struct InputDialog {
    dialog: DialogWidget,
    value: String,
    placeholder: String,
}

impl InputDialog {
    /// Create a new input dialog
    pub fn new(title: String) -> Self {
        let dialog = DialogWidget::new(title)
            .with_button(DialogButton::new("Cancel".to_string(), "cancel".to_string()))
            .with_button(
                DialogButton::new("Submit".to_string(), "submit".to_string())
                    .primary()
            );

        Self {
            dialog,
            value: String::new(),
            placeholder: "Enter value...".to_string(),
        }
    }

    /// Set the placeholder
    pub fn with_placeholder(mut self, placeholder: String) -> Self {
        self.placeholder = placeholder;
        self
    }

    /// Set initial value
    pub fn with_value(mut self, value: String) -> Self {
        self.value = value;
        self
    }

    /// Add character to input
    pub fn add_char(&mut self, c: char) {
        self.value.push(c);
    }

    /// Remove last character
    pub fn backspace(&mut self) {
        self.value.pop();
    }

    /// Clear input
    pub fn clear(&mut self) {
        self.value.clear();
    }

    /// Get current value
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Build and render the dialog
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let mut dialog = self.dialog.clone();
        dialog.content.push(format!(
            "{}{}",
            self.value,
            if self.value.is_empty() { &self.placeholder } else { "" }
        ));
        dialog.render(frame, area);
    }

    /// Get the underlying dialog for navigation
    pub fn dialog_mut(&mut self) -> &mut DialogWidget {
        &mut self.dialog
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dialog_button_creation() {
        let button = DialogButton::new("OK".to_string(), "ok".to_string());
        assert_eq!(button.label, "OK");
        assert_eq!(button.action, "ok");
        assert!(!button.is_primary);
    }

    #[test]
    fn test_dialog_button_primary() {
        let button = DialogButton::new("OK".to_string(), "ok".to_string()).primary();
        assert!(button.is_primary);
    }

    #[test]
    fn test_dialog_button_dangerous() {
        let button = DialogButton::new("Delete".to_string(), "delete".to_string()).dangerous();
        assert!(button.is_dangerous);
    }

    #[test]
    fn test_dialog_creation() {
        let dialog = DialogWidget::new("Test".to_string());
        assert_eq!(dialog.title, "Test");
        assert!(dialog.closable);
        assert!(dialog.buttons.is_empty());
    }

    #[test]
    fn test_dialog_with_buttons() {
        let dialog = DialogWidget::new("Test".to_string())
            .with_button(DialogButton::new("OK".to_string(), "ok".to_string()))
            .with_button(DialogButton::new("Cancel".to_string(), "cancel".to_string()));

        assert_eq!(dialog.buttons.len(), 2);
        assert_eq!(dialog.selected_button, 0);
    }

    #[test]
    fn test_dialog_navigation() {
        let mut dialog = DialogWidget::new("Test".to_string())
            .with_button(DialogButton::new("OK".to_string(), "ok".to_string()))
            .with_button(DialogButton::new("Cancel".to_string(), "cancel".to_string()));

        dialog.next_button();
        assert_eq!(dialog.selected_button, 1);

        dialog.next_button();
        assert_eq!(dialog.selected_button, 0);

        dialog.prev_button();
        assert_eq!(dialog.selected_button, 1);
    }

    #[test]
    fn test_confirm_dialog() {
        let dialog = ConfirmDialog::new("Delete File?".to_string())
            .with_message("This action cannot be undone.".to_string())
            .build();

        assert_eq!(dialog.title, "Delete File?");
        assert_eq!(dialog.buttons.len(), 2);
        assert!(dialog.buttons[1].is_primary);
    }

    #[test]
    fn test_alert_dialog() {
        let dialog = AlertDialog::new("Error".to_string())
            .with_message("Something went wrong".to_string())
            .build();

        assert_eq!(dialog.title, "Error");
        assert_eq!(dialog.buttons.len(), 1);
    }

    #[test]
    fn test_input_dialog() {
        let mut input = InputDialog::new("Enter Name".to_string())
            .with_placeholder("Name...".to_string());

        input.add_char('H');
        input.add_char('e');
        input.add_char('l');
        input.add_char('l');
        input.add_char('o');

        assert_eq!(input.value(), "Hello");

        input.backspace();
        assert_eq!(input.value(), "Hell");

        input.clear();
        assert_eq!(input.value(), "");
    }
}
