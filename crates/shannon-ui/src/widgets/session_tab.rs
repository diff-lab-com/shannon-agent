//! Session tab bar for managing multiple conversations
//!
//! Shows tabs at the top of the terminal for switching between sessions.
//! Ctrl+T: New session | Ctrl+W: Close session | Ctrl+Tab: Next session

use crate::theme::Theme;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

/// Session metadata
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub title: String,
    pub message_count: usize,
}

/// Tab bar widget for multiple sessions
#[derive(Debug, Clone)]
pub struct SessionTabWidget {
    pub sessions: Vec<SessionInfo>,
    pub active_index: usize,
    pub visible: bool,
}

impl Default for SessionTabWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionTabWidget {
    pub fn new() -> Self {
        let default_session = SessionInfo {
            id: uuid::Uuid::new_v4().to_string(),
            title: "New Chat".to_string(),
            message_count: 0,
        };
        Self {
            sessions: vec![default_session],
            active_index: 0,
            visible: false,
        }
    }

    pub fn height() -> u16 {
        1
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if !self.visible || area.width < 10 {
            return;
        }

        let mut spans: Vec<Span<'static>> = Vec::new();
        let tab_width = 16usize;
        let max_visible = (area.width as usize).saturating_sub(4) / (tab_width + 1);
        let start = if self.active_index >= max_visible {
            self.active_index - max_visible + 1
        } else {
            0
        };

        let visible_sessions = &self.sessions[start..self.sessions.len().min(start + max_visible)];

        for (i, session) in visible_sessions.iter().enumerate() {
            let actual_index = start + i;
            let is_active = actual_index == self.active_index;

            let title = if session.title.len() > tab_width - 2 {
                format!(" {}… ", &session.title[..tab_width - 3])
            } else {
                format!(" {} ", session.title)
            };

            let style = if is_active {
                Style::default()
                    .fg(theme.context_bar_bg)
                    .bg(theme.primary)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text_dim).bg(theme.context_bar_bg)
            };

            spans.push(Span::styled(title, style));

            if i < visible_sessions.len() - 1 {
                spans.push(Span::styled(crate::a11y::separator().to_string(), Style::default().fg(theme.muted)));
            }
        }

        // New tab hint
        if area.width as usize > spans.iter().map(|s| unicode_width::UnicodeWidthStr::width(s.content.as_ref())).sum::<usize>() + 6 {
            spans.push(Span::styled(" +", Style::default().fg(theme.primary)));
        }

        let paragraph = Paragraph::new(Line::from(spans))
            .style(Style::default().bg(theme.context_bar_bg));
        frame.render_widget(paragraph, area);
    }

    pub fn new_session(&mut self) -> &str {
        let session = SessionInfo {
            id: uuid::Uuid::new_v4().to_string(),
            title: "New Chat".to_string(),
            message_count: 0,
        };
        self.sessions.push(session);
        self.active_index = self.sessions.len() - 1;
        &self.sessions[self.active_index].id
    }

    pub fn close_session(&mut self) -> Option<String> {
        if self.sessions.len() <= 1 {
            return None;
        }
        let removed = self.sessions.remove(self.active_index);
        if self.active_index >= self.sessions.len() {
            self.active_index = self.sessions.len() - 1;
        }
        Some(removed.id)
    }

    pub fn next_session(&mut self) {
        if self.sessions.len() > 1 {
            self.active_index = (self.active_index + 1) % self.sessions.len();
        }
    }

    pub fn prev_session(&mut self) {
        if self.sessions.len() > 1 {
            self.active_index = if self.active_index == 0 {
                self.sessions.len() - 1
            } else {
                self.active_index - 1
            };
        }
    }

    pub fn toggle_visibility(&mut self) {
        self.visible = !self.visible;
    }

    pub fn update_title(&mut self, session_id: &str, title: String) {
        if let Some(session) = self.sessions.iter_mut().find(|s| s.id == session_id) {
            session.title = title;
        }
    }

    pub fn active_session_id(&self) -> &str {
        &self.sessions[self.active_index].id
    }
}
