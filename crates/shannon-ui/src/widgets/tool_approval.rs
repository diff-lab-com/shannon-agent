//! Tool approval overlay — shows before executing tools that need user confirmation

use crate::theme::Theme;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
    Frame,
};

/// User's decision on a tool approval request
#[derive(Debug, Clone, PartialEq)]
pub enum ApprovalDecision {
    Pending,
    AllowOnce,
    AllowSession,
    Deny,
}

/// Risk level for a tool invocation
#[derive(Debug, Clone, PartialEq)]
pub enum RiskLevel {
    /// Read-only operations: reads, searches
    Low,
    /// File modifications: writes
    Medium,
    /// Arbitrary execution: bash commands
    High,
}

impl RiskLevel {
    /// Returns a unicode indicator and the theme color key for this risk level
    fn indicator(&self) -> (&'static str, fn(&Theme) -> ratatui::style::Color) {
        match self {
            RiskLevel::Low => ("\u{1f7e2} Low", |t| t.success),
            RiskLevel::Medium => ("\u{1f7e1} Medium", |t| t.warning),
            RiskLevel::High => ("\u{1f534} High", |t| t.error),
        }
    }
}

/// A tool approval request with context
#[derive(Debug, Clone)]
pub struct ToolApprovalRequest {
    pub tool_name: String,
    pub description: String,
    pub risk_level: RiskLevel,
    pub detail: Option<String>,
}

/// Auto-approval rule
#[derive(Debug, Clone)]
pub struct AutoApproveRule {
    pub tool_name: String,
    pub pattern: String,
    pub approved: bool,
}

/// Overlay widget that prompts the user to approve or deny a tool invocation
#[derive(Debug, Clone)]
pub struct ToolApprovalWidget {
    pub request: Option<ToolApprovalRequest>,
    pub decision: ApprovalDecision,
    pub selected_option: usize,
    pub auto_approve_rules: Vec<AutoApproveRule>,
}

/// Number of options presented to the user
const NUM_OPTIONS: usize = 3;

impl ToolApprovalWidget {
    /// Create a new widget with no pending request
    pub fn new() -> Self {
        Self {
            request: None,
            decision: ApprovalDecision::Pending,
            selected_option: 0,
            auto_approve_rules: Vec::new(),
        }
    }

    /// Show a new approval request
    pub fn show_request(&mut self, request: ToolApprovalRequest) {
        self.request = Some(request);
        self.decision = ApprovalDecision::Pending;
        self.selected_option = 0;
    }

    /// Dismiss the current request
    pub fn dismiss(&mut self) {
        self.request = None;
        self.decision = ApprovalDecision::Pending;
        self.selected_option = 0;
    }

    /// Whether a request is currently being shown
    pub fn is_active(&self) -> bool {
        self.request.is_some() && self.decision == ApprovalDecision::Pending
    }

    /// Check whether a tool/command pair is auto-approved by any rule
    pub fn is_auto_approved(&self, tool_name: &str, command: &str) -> bool {
        self.auto_approve_rules.iter().any(|rule| {
            rule.approved
                && rule.tool_name == tool_name
                && (rule.pattern == "*" || command.contains(&rule.pattern))
        })
    }

    /// Process a keyboard event and return a decision if one was made
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<ApprovalDecision> {
        if !self.is_active() {
            return None;
        }

        match key.code {
            KeyCode::Char('1') => {
                self.selected_option = 0;
                self.decision = ApprovalDecision::AllowOnce;
                Some(ApprovalDecision::AllowOnce)
            }
            KeyCode::Char('2') => {
                self.selected_option = 1;
                self.decision = ApprovalDecision::AllowSession;
                Some(ApprovalDecision::AllowSession)
            }
            KeyCode::Char('3') | KeyCode::Char('q') | KeyCode::Esc => {
                self.selected_option = 2;
                self.decision = ApprovalDecision::Deny;
                Some(ApprovalDecision::Deny)
            }
            KeyCode::Left => {
                if self.selected_option > 0 {
                    self.selected_option -= 1;
                }
                None
            }
            KeyCode::Right => {
                if self.selected_option < NUM_OPTIONS - 1 {
                    self.selected_option += 1;
                }
                None
            }
            KeyCode::Enter => {
                let decision = match self.selected_option {
                    0 => ApprovalDecision::AllowOnce,
                    1 => ApprovalDecision::AllowSession,
                    _ => ApprovalDecision::Deny,
                };
                self.decision = decision.clone();
                Some(decision)
            }
            _ => None,
        }
    }

    /// Render the overlay centered in the given area
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let req = match &self.request {
            Some(r) => r,
            None => return,
        };

        // Compute centered overlay rect
        let overlay_w = 60.min(area.width);
        let overlay_h = 12.min(area.height);
        let x = area.x + (area.width.saturating_sub(overlay_w)) / 2;
        let y = area.y + (area.height.saturating_sub(overlay_h)) / 2;
        let overlay_area = Rect {
            x,
            y,
            width: overlay_w,
            height: overlay_h,
        };

        // Clear behind the overlay for modal effect
        frame.render_widget(Clear, overlay_area);

        let mut lines: Vec<Line<'static>> = Vec::new();

        // Risk level indicator line
        let (risk_text, risk_color_fn) = req.risk_level.indicator();
        lines.push(Line::from(vec![
            Span::styled("Risk: ", Style::default().fg(theme.text_dim)),
            Span::styled(
                risk_text.to_string(),
                Style::default().fg(risk_color_fn(theme)).add_modifier(Modifier::BOLD),
            ),
        ]));

        lines.push(Line::from(""));

        // Tool name
        lines.push(Line::from(vec![
            Span::styled("Tool: ", Style::default().fg(theme.text_dim)),
            Span::styled(
                req.tool_name.clone(),
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));

        // Description (wrap to overlay width minus borders/padding)
        let content_width = (overlay_w as usize).saturating_sub(4);
        let desc = &req.description;
        let wrapped = wrap_text(desc, content_width);
        for line in &wrapped {
            lines.push(Line::from(vec![Span::styled(
                line.clone(),
                Style::default().fg(theme.text),
            )]));
        }

        // Detail (if present)
        if let Some(ref detail) = req.detail {
            lines.push(Line::from(""));
            for line in wrap_text(detail, content_width) {
                lines.push(Line::from(vec![Span::styled(
                    line,
                    Style::default().fg(theme.text_dim),
                )]));
            }
        }

        lines.push(Line::from(""));

        // Options row
        let options = [
            ("1", "Allow Once"),
            ("2", "Allow Session"),
            ("3", "Deny"),
        ];
        let mut option_spans: Vec<Span<'static>> = Vec::new();
        for (i, (key, label)) in options.iter().enumerate() {
            let is_selected = i == self.selected_option;
            let style = if i == 2 {
                // Deny uses error color
                Style::default()
                    .fg(if is_selected { theme.error } else { theme.text_dim })
                    .add_modifier(if is_selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    })
            } else if is_selected {
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text_dim)
            };

            let prefix = if is_selected { " > " } else { "   " };
            let suffix = if is_selected { "<" } else { " " };

            option_spans.push(Span::styled(
                format!("{prefix}[{key}] {label}{suffix}"),
                style,
            ));
        }
        lines.push(Line::from(option_spans));

        // Keyboard hints
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                "1/2/3 or \u{2190}/\u{2192} to select, Enter to confirm, Esc to deny",
                Style::default().fg(theme.text_dim),
            ),
        ]));

        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border))
                    .border_type(BorderType::Rounded)
                    .title(" Tool Approval Required "),
            )
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, overlay_area);
    }
}

impl Default for ToolApprovalWidget {
    fn default() -> Self {
        Self::new()
    }
}

/// Simple word-wrap helper (word-level, preserves words)
fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut len = 0usize;

    for word in text.split_whitespace() {
        let wlen = word.chars().count();
        if len == 0 {
            current.push_str(word);
            len = wlen;
        } else if len + 1 + wlen <= max_width {
            current.push(' ');
            current.push_str(word);
            len += 1 + wlen;
        } else {
            lines.push(std::mem::take(&mut current));
            current.push_str(word);
            len = wlen;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_widget_has_no_request() {
        let w = ToolApprovalWidget::new();
        assert!(w.request.is_none());
        assert_eq!(w.decision, ApprovalDecision::Pending);
        assert_eq!(w.selected_option, 0);
        assert!(!w.is_active());
    }

    #[test]
    fn test_show_request_sets_active() {
        let mut w = ToolApprovalWidget::new();
        w.show_request(ToolApprovalRequest {
            tool_name: "bash".into(),
            description: "rm -rf /".into(),
            risk_level: RiskLevel::High,
            detail: None,
        });
        assert!(w.request.is_some());
        assert!(w.is_active());
        assert_eq!(w.selected_option, 0);
    }

    #[test]
    fn test_handle_key_number_selection() {
        let mut w = ToolApprovalWidget::new();
        w.show_request(ToolApprovalRequest {
            tool_name: "bash".into(),
            description: "ls".into(),
            risk_level: RiskLevel::Low,
            detail: None,
        });

        let decision = w.handle_key(KeyEvent::new(KeyCode::Char('1'), crossterm::event::KeyModifiers::NONE));
        assert_eq!(decision, Some(ApprovalDecision::AllowOnce));

        // After a decision is made the widget is no longer active
        assert!(!w.is_active());
    }

    #[test]
    fn test_handle_key_deny_via_esc() {
        let mut w = ToolApprovalWidget::new();
        w.show_request(ToolApprovalRequest {
            tool_name: "bash".into(),
            description: "ls".into(),
            risk_level: RiskLevel::Low,
            detail: None,
        });

        let decision = w.handle_key(KeyEvent::new(KeyCode::Esc, crossterm::event::KeyModifiers::NONE));
        assert_eq!(decision, Some(ApprovalDecision::Deny));
    }

    #[test]
    fn test_handle_key_arrows_then_enter() {
        let mut w = ToolApprovalWidget::new();
        w.show_request(ToolApprovalRequest {
            tool_name: "write".into(),
            description: "save file".into(),
            risk_level: RiskLevel::Medium,
            detail: None,
        });

        // Move right twice to reach "Deny"
        assert_eq!(w.handle_key(KeyEvent::new(KeyCode::Right, crossterm::event::KeyModifiers::NONE)), None);
        assert_eq!(w.selected_option, 1);
        assert_eq!(w.handle_key(KeyEvent::new(KeyCode::Right, crossterm::event::KeyModifiers::NONE)), None);
        assert_eq!(w.selected_option, 2);

        let decision = w.handle_key(KeyEvent::new(KeyCode::Enter, crossterm::event::KeyModifiers::NONE));
        assert_eq!(decision, Some(ApprovalDecision::Deny));
    }

    #[test]
    fn test_auto_approve_rules() {
        let w = ToolApprovalWidget {
            request: None,
            decision: ApprovalDecision::Pending,
            selected_option: 0,
            auto_approve_rules: vec![
                AutoApproveRule {
                    tool_name: "bash".into(),
                    pattern: "ls".into(),
                    approved: true,
                },
                AutoApproveRule {
                    tool_name: "bash".into(),
                    pattern: "*".into(),
                    approved: false,
                },
            ],
        };

        assert!(w.is_auto_approved("bash", "ls -la"));
        assert!(!w.is_auto_approved("bash", "rm -rf /"));
        assert!(!w.is_auto_approved("write", "file.txt"));
    }

    #[test]
    fn test_dismiss_clears_state() {
        let mut w = ToolApprovalWidget::new();
        w.show_request(ToolApprovalRequest {
            tool_name: "bash".into(),
            description: "ls".into(),
            risk_level: RiskLevel::Low,
            detail: None,
        });
        assert!(w.is_active());
        w.dismiss();
        assert!(!w.is_active());
        assert!(w.request.is_none());
    }

    #[test]
    fn test_risk_level_indicators() {
        let theme = Theme::default_dark();
        let (_low_txt, low_fn) = RiskLevel::Low.indicator();
        assert_eq!(low_fn(&theme), theme.success);

        let (_med_txt, med_fn) = RiskLevel::Medium.indicator();
        assert_eq!(med_fn(&theme), theme.warning);

        let (_high_txt, high_fn) = RiskLevel::High.indicator();
        assert_eq!(high_fn(&theme), theme.error);
    }

    #[test]
    fn test_wrap_text() {
        assert_eq!(wrap_text("hello world", 20), vec!["hello world"]);
        assert_eq!(wrap_text("hello world", 8), vec!["hello", "world"]);
        assert_eq!(wrap_text("a b c d", 5), vec!["a b c", "d"]);
        assert_eq!(wrap_text("", 10), vec![""]);
    }

    #[test]
    fn test_handle_key_ignored_when_not_active() {
        let mut w = ToolApprovalWidget::new();
        let result = w.handle_key(KeyEvent::new(KeyCode::Enter, crossterm::event::KeyModifiers::NONE));
        assert_eq!(result, None);
    }
}
