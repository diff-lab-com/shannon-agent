//! Attachment bar — shows attached files/images above the input area

use crate::theme::Theme;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Type of attached resource
#[derive(Debug, Clone, PartialEq)]
pub enum AttachmentKind {
    File,
    Image,
    Url,
}

impl AttachmentKind {
    /// Icon used when rendering this attachment kind
    fn icon(&self) -> &'static str {
        match self {
            AttachmentKind::File => "\u{1f4ce}",   // paperclip
            AttachmentKind::Image => "\u{1f5bc}",   // framed picture
            AttachmentKind::Url => "\u{1f517}",     // link symbol
        }
    }
}

/// An attached file or resource
#[derive(Debug, Clone)]
pub struct Attachment {
    pub path: String,
    pub kind: AttachmentKind,
    pub size_bytes: Option<u64>,
}

impl Attachment {
    /// Extract the filename portion from the path for display
    fn display_name(&self, max_len: usize) -> String {
        let name = self
            .path
            .rsplit('/')
            .next()
            .unwrap_or(&self.path);
        if name.chars().count() > max_len {
            let truncated: String = name.chars().take(max_len.saturating_sub(3)).collect();
            format!("{truncated}...")
        } else {
            name.to_string()
        }
    }

    /// Human-readable size string
    fn size_str(&self) -> Option<String> {
        self.size_bytes.map(|b| {
            if b < 1024 {
                format!("{b}B")
            } else if b < 1024 * 1024 {
                format!("{:.1}K", b as f64 / 1024.0)
            } else {
                format!("{:.1}M", b as f64 / (1024.0 * 1024.0))
            }
        })
    }
}

/// Single-line bar that shows attached files/images above the input area
#[derive(Debug, Clone)]
pub struct AttachmentBarWidget {
    pub attachments: Vec<Attachment>,
    pub max_attachments: usize,
    pub delete_mode: bool,
}

impl AttachmentBarWidget {
    /// Create a new attachment bar with the given maximum number of attachments
    pub fn new(max: usize) -> Self {
        Self {
            attachments: Vec::new(),
            max_attachments: max,
            delete_mode: false,
        }
    }

    /// Add an attachment; returns an error string if the limit has been reached
    pub fn add(&mut self, attachment: Attachment) -> Result<(), String> {
        if self.attachments.len() >= self.max_attachments {
            return Err(format!(
                "Maximum of {} attachments reached",
                self.max_attachments
            ));
        }
        self.attachments.push(attachment);
        Ok(())
    }

    /// Remove an attachment by index
    pub fn remove(&mut self, index: usize) {
        if index < self.attachments.len() {
            self.attachments.remove(index);
        }
    }

    /// Toggle delete mode on/off (activated by Ctrl+R)
    pub fn toggle_delete_mode(&mut self) {
        self.delete_mode = !self.delete_mode;
    }

    /// Number of attachments currently held
    pub fn len(&self) -> usize {
        self.attachments.len()
    }

    /// Whether there are no attachments
    pub fn is_empty(&self) -> bool {
        self.attachments.is_empty()
    }

    /// Render the attachment bar. Renders nothing when there are no attachments.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if self.attachments.is_empty() {
            return;
        }

        let inner_width = (area.width as usize).saturating_sub(2); // subtract borders
        let mut spans: Vec<Span<'static>> = Vec::new();

        if self.delete_mode {
            // In delete mode, prefix with a hint and use error styling
            spans.push(Span::styled(
                "Ctrl+R: delete mode \u{2014} press number to remove  ",
                Style::default()
                    .fg(theme.error)
                    .add_modifier(Modifier::BOLD),
            ));
        }

        for (i, att) in self.attachments.iter().enumerate() {
            // Each chip takes ~8 chars for brackets + icon + space, plus name + optional size
            let overhead = if self.delete_mode { 5 } else { 4 }; // "[1\u{1f4ce} ]" vs "[\u{1f4ce} ]"
            let size_width = att.size_str().map_or(0, |s| s.len() + 1); // " 1.2K"
            let name_budget = inner_width.saturating_sub(overhead + size_width);
            let name = att.display_name(name_budget.max(4));
            let icon = att.kind.icon();

            let style = if self.delete_mode {
                Style::default()
                    .fg(theme.error)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.primary)
            };

            if self.delete_mode {
                spans.push(Span::styled(format!("[{i}{icon} {name}]"), style));
            } else {
                spans.push(Span::styled(format!("[{icon} {name}]"), style));
            }

            if let Some(size) = att.size_str() {
                spans.push(Span::styled(
                    format!(" {size}"),
                    Style::default().fg(theme.text_dim),
                ));
            }

            // Separator between chips
            if i < self.attachments.len() - 1 {
                spans.push(Span::raw(" "));
            }
        }

        let paragraph = Paragraph::new(Line::from(spans)).block(
            Block::default()
                .borders(Borders::NONE)
                .style(Style::default().bg(theme.context_bar_bg)),
        );

        frame.render_widget(paragraph, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_attachment(path: &str, kind: AttachmentKind) -> Attachment {
        Attachment {
            path: path.to_string(),
            kind,
            size_bytes: None,
        }
    }

    #[test]
    fn test_new_bar_is_empty() {
        let bar = AttachmentBarWidget::new(5);
        assert!(bar.is_empty());
        assert_eq!(bar.len(), 0);
        assert!(!bar.delete_mode);
    }

    #[test]
    fn test_add_attachment() {
        let mut bar = AttachmentBarWidget::new(5);
        bar.add(make_attachment("/tmp/file.rs", AttachmentKind::File))
            .unwrap();
        assert_eq!(bar.len(), 1);
    }

    #[test]
    fn test_add_respects_limit() {
        let mut bar = AttachmentBarWidget::new(2);
        bar.add(make_attachment("/a", AttachmentKind::File)).unwrap();
        bar.add(make_attachment("/b", AttachmentKind::File)).unwrap();
        let result = bar.add(make_attachment("/c", AttachmentKind::File));
        assert!(result.is_err());
        assert_eq!(bar.len(), 2);
    }

    #[test]
    fn test_remove_attachment() {
        let mut bar = AttachmentBarWidget::new(5);
        bar.add(make_attachment("/a", AttachmentKind::File)).unwrap();
        bar.add(make_attachment("/b", AttachmentKind::Image)).unwrap();
        bar.remove(0);
        assert_eq!(bar.len(), 1);
        assert_eq!(bar.attachments[0].path, "/b");
    }

    #[test]
    fn test_remove_out_of_bounds_is_noop() {
        let mut bar = AttachmentBarWidget::new(5);
        bar.add(make_attachment("/a", AttachmentKind::File)).unwrap();
        bar.remove(5); // out of bounds
        assert_eq!(bar.len(), 1);
    }

    #[test]
    fn test_toggle_delete_mode() {
        let mut bar = AttachmentBarWidget::new(5);
        assert!(!bar.delete_mode);
        bar.toggle_delete_mode();
        assert!(bar.delete_mode);
        bar.toggle_delete_mode();
        assert!(!bar.delete_mode);
    }

    #[test]
    fn test_display_name_truncation() {
        let att = Attachment {
            path: "/very/long/path/to/some/really_long_filename_that_goes_on_and_on.rs".into(),
            kind: AttachmentKind::File,
            size_bytes: None,
        };
        let name = att.display_name(10);
        assert!(name.chars().count() <= 10);
        assert!(name.ends_with("..."));
    }

    #[test]
    fn test_display_name_short_path() {
        let att = make_attachment("/tmp/main.rs", AttachmentKind::File);
        let name = att.display_name(30);
        assert_eq!(name, "main.rs");
    }

    #[test]
    fn test_size_str() {
        let att = Attachment {
            path: "x".into(),
            kind: AttachmentKind::File,
            size_bytes: Some(512),
        };
        assert_eq!(att.size_str(), Some("512B".to_string()));

        let att = Attachment {
            path: "x".into(),
            kind: AttachmentKind::File,
            size_bytes: Some(2048),
        };
        assert_eq!(att.size_str(), Some("2.0K".to_string()));

        let att = Attachment {
            path: "x".into(),
            kind: AttachmentKind::File,
            size_bytes: Some(3 * 1024 * 1024),
        };
        assert_eq!(att.size_str(), Some("3.0M".to_string()));

        let att = make_attachment("x", AttachmentKind::File);
        assert_eq!(att.size_str(), None);
    }

    #[test]
    fn test_attachment_kind_icons() {
        assert!(!AttachmentKind::File.icon().is_empty());
        assert!(!AttachmentKind::Image.icon().is_empty());
        assert!(!AttachmentKind::Url.icon().is_empty());
    }
}
