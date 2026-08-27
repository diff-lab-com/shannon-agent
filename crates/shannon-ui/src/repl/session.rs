//! Session restore and auto-restore functionality.

use crate::widgets::ChatRole;
use shannon_core::{ContentBlock, MessageContent};

impl super::Repl {
    /// Restore conversation history from an L0-projected session (§4.6).
    ///
    /// Takes the projected [`StoredSession`] and injects it into the query
    /// engine so the next user message continues the prior conversation.
    /// Also populates the chat widget so the user can see the restored
    /// history. Returns the number of messages restored.
    pub fn restore_session(
        &mut self,
        session_data: shannon_core::session_log::StoredSession,
    ) -> usize {
        let msg_count = session_data.messages.len();
        if msg_count == 0 {
            return 0;
        }

        // Populate chat widget with restored messages so the user can see them.
        // Tool-use / tool-result blocks are rendered (not dropped) so history is legible.
        for msg in &session_data.messages {
            let role = match msg.role.as_str() {
                "user" => ChatRole::User,
                "assistant" => ChatRole::Assistant,
                "system" => ChatRole::System,
                _ => ChatRole::Tool, // "tool" and any unknown roles
            };
            let text = render_message_content(&msg.content);
            if !text.trim().is_empty() {
                self.chat.add_message(role, text);
            }
        }

        if let Some(ref mut engine) = self.query_engine {
            let preview = session_data.metadata.title.clone();
            engine.replace_conversation(session_data.messages);
            tracing::info!(
                "Resumed session {} ({} messages, title: {preview:?})",
                session_data.session_id,
                msg_count,
            );
        }
        msg_count
    }

    /// Open the interactive session picker, scoped to the current project.
    ///
    /// Used by `/resume` (no-arg) in the REPL and by bare `shannon --resume`
    /// in interactive mode. Rows show "first prompt → last prompt · recency ·
    /// turns" (with a `⤵` prefix for branches) so sessions are identifiable
    /// without AI-generated titles. Press Tab inside the picker to toggle
    /// between the current project and all projects.
    pub fn open_session_picker(&mut self) -> crate::Result<()> {
        // Always start scoped to the current project; Tab toggles to all.
        self.state.session_picker_show_all = false;
        self.refresh_session_picker_scope()
    }

    /// Rebuild the session picker's items and title from the current scope
    /// (`session_picker_show_all`). Called on open and on Tab toggle. Keeps
    /// `last_session_list` in sync with whatever scope is visible so
    /// `/resume <number>` resolves against the list the user actually saw.
    pub fn refresh_session_picker_scope(&mut self) -> crate::Result<()> {
        let project = self.state.working_directory.clone();
        let show_all = self.state.session_picker_show_all;

        let mut sessions = match self.l0_store().list() {
            Ok(s) => s,
            Err(e) => {
                self.chat
                    .add_message(ChatRole::System, format!("Error listing sessions: {e}"));
                self.state.fuzzy_picker = None;
                self.state.session_picker_active = false;
                return Ok(());
            }
        };
        if !show_all {
            sessions.retain(|s| s.project_path.as_deref() == Some(project.as_str()));
        }

        if sessions.is_empty() {
            let msg = if show_all {
                "No saved sessions found.".to_string()
            } else {
                format!(
                    "No sessions found for this project ({project}). Press Tab to browse all sessions, or use /resume <uuid>."
                )
            };
            self.chat.add_message(ChatRole::System, msg);
            self.last_session_list.clear();
            // If the picker is already on screen (toggled into an empty
            // scope), keep it open but empty so the user can Tab back.
            let title = self.session_picker_title();
            if let Some(ref mut picker) = self.state.fuzzy_picker {
                picker.set_items(Vec::new());
                picker.set_title(title);
            } else {
                self.state.session_picker_active = false;
            }
            return Ok(());
        }

        self.last_session_list = sessions.clone();
        let items: Vec<crate::widgets::select::SelectItem<String>> = sessions
            .iter()
            .map(|s| {
                let label = format_session_picker_row(s);
                crate::widgets::select::SelectItem::new(label, s.session_id.to_string())
            })
            .collect();

        let title = self.session_picker_title();
        if let Some(ref mut picker) = self.state.fuzzy_picker {
            picker.set_items(items);
            picker.set_title(title);
        } else {
            let mut picker = crate::widgets::select::FuzzyPickerWidget::new(title);
            picker.set_items(items);
            picker.start_search();
            self.state.fuzzy_picker = Some(picker);
            self.state.session_picker_active = true;
        }
        Ok(())
    }

    /// Border title for the session picker, reflecting the active scope so the
    /// user always knows whether they are browsing the current project or all.
    fn session_picker_title(&self) -> String {
        if self.state.session_picker_show_all {
            "Resume session · all projects (Tab: current)".to_string()
        } else {
            let basename = std::path::Path::new(&self.state.working_directory)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("current");
            format!("Resume session · {basename} (Tab: all)")
        }
    }

    /// Auto-restore is disabled by default. Users expect a fresh session on startup.
    /// Use `/resume` or `--resume` to explicitly continue a previous session.
    pub(crate) fn auto_restore_last_session(&mut self) {
        // Disabled: auto-restore was confusing — users expect a fresh session on startup.
        // Use /resume or --resume to continue a previous session.
    }
}

/// Render message content to display text, preserving tool-use / tool-result
/// structure instead of dropping non-text blocks.
///
/// Used when replaying a resumed session into the chat widget so the user can
/// see what tools were invoked and what they returned.
pub(crate) fn render_message_content(content: &MessageContent) -> String {
    match content {
        MessageContent::Text(t) => t.clone(),
        MessageContent::Blocks(blocks) => {
            let mut parts: Vec<String> = Vec::new();
            for b in blocks {
                match b {
                    ContentBlock::Text { text } => parts.push(text.clone()),
                    ContentBlock::ToolUse { name, input, .. } => {
                        let input_str = input
                            .as_str()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| input.to_string());
                        parts.push(format!(
                            "⚙ {name}: {}",
                            truncate_for_display(&input_str, 200)
                        ));
                    }
                    ContentBlock::ToolResult {
                        content, is_error, ..
                    } => {
                        let body = match content {
                            Some(shannon_engine::api::ToolResultContent::Single(s)) => s.clone(),
                            Some(shannon_engine::api::ToolResultContent::Multiple(_)) => {
                                "<multi-block result>".to_string()
                            }
                            None => String::new(),
                        };
                        let prefix = if is_error.unwrap_or(false) {
                            "✗ "
                        } else {
                            "↳ "
                        };
                        parts.push(format!("{prefix}{}", truncate_for_display(&body, 300)));
                    }
                    ContentBlock::Thinking { thinking } => {
                        parts.push(format!("💭 {}", truncate_for_display(thinking, 160)));
                    }
                    ContentBlock::Image { .. } => parts.push("[image]".to_string()),
                }
            }
            parts.join("\n")
        }
    }
}

/// Char-boundary-safe truncation with ellipsis; collapses newlines for single-line display.
fn truncate_for_display(s: &str, max_len: usize) -> String {
    let truncated = if s.len() <= max_len {
        s.to_string()
    } else {
        let mut end = max_len.saturating_sub(3);
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    };
    truncated.replace('\n', " ⏎ ")
}

/// Format a session as a picker row:
/// `[⤵] first-prompt → last-prompt · recency · N turns`
///
/// The `⤵ ` prefix marks a branch (session with a parent) so branches are
/// visually distinguishable from roots.
fn format_session_picker_row(s: &shannon_core::session_log::StoredSessionInfo) -> String {
    let first = s
        .title
        .clone()
        .or_else(|| s.preview.clone())
        .unwrap_or_else(|| "(no prompt)".to_string());
    let last = s
        .last_user_preview
        .clone()
        .unwrap_or_else(|| "—".to_string());
    let rel = format_relative_time(s.updated_at);
    let branch = if s.parent_session_id.is_some() {
        "⤵ "
    } else {
        ""
    };
    format!(
        "{branch}{} → {} · {} · {} turns",
        truncate_for_display(&first, 48),
        truncate_for_display(&last, 48),
        rel,
        s.turn_count
    )
}

/// Format a timestamp as a short relative duration for the picker.
fn format_relative_time(ts: chrono::DateTime<chrono::Utc>) -> String {
    let now = chrono::Utc::now();
    let delta = now.signed_duration_since(ts);
    if delta.num_seconds() < 60 {
        "just now".to_string()
    } else if delta.num_minutes() < 60 {
        format!("{}m ago", delta.num_minutes())
    } else if delta.num_hours() < 24 {
        format!("{}h ago", delta.num_hours())
    } else if delta.num_days() < 7 {
        format!("{}d ago", delta.num_days())
    } else {
        ts.format("%Y-%m-%d").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use shannon_core::session_log::StoredSessionInfo;
    use shannon_core::{ContentBlock, MessageContent};
    use shannon_engine::api::ToolResultContent;

    #[test]
    fn test_render_message_content_text() {
        let c = MessageContent::Text("hello world".to_string());
        assert_eq!(render_message_content(&c), "hello world");
    }

    #[test]
    fn test_render_message_content_text_block() {
        let c = MessageContent::Blocks(vec![ContentBlock::Text {
            text: "plain".to_string(),
        }]);
        assert_eq!(render_message_content(&c), "plain");
    }

    #[test]
    fn test_render_message_content_tool_use() {
        let c = MessageContent::Blocks(vec![
            ContentBlock::Text {
                text: "running".to_string(),
            },
            ContentBlock::ToolUse {
                id: "tu_1".to_string(),
                name: "bash".to_string(),
                input: json!({"command": "ls -la"}),
            },
        ]);
        let out = render_message_content(&c);
        assert!(out.contains("running"));
        assert!(out.contains("⚙ bash:"));
        assert!(out.contains("ls -la"));
    }

    #[test]
    fn test_render_message_content_tool_result_ok_and_error() {
        let ok = MessageContent::Blocks(vec![ContentBlock::ToolResult {
            tool_use_id: "tu_1".to_string(),
            content: Some(ToolResultContent::Single("ok-output".to_string())),
            is_error: Some(false),
        }]);
        let rendered_ok = render_message_content(&ok);
        assert!(rendered_ok.contains("↳ ok-output"));
        assert!(!rendered_ok.contains("✗"));

        let err = MessageContent::Blocks(vec![ContentBlock::ToolResult {
            tool_use_id: "tu_2".to_string(),
            content: Some(ToolResultContent::Single("boom".to_string())),
            is_error: Some(true),
        }]);
        assert!(render_message_content(&err).contains("✗ boom"));
    }

    #[test]
    fn test_render_message_content_tool_result_multi_and_none() {
        let multi = MessageContent::Blocks(vec![ContentBlock::ToolResult {
            tool_use_id: "tu_3".to_string(),
            content: Some(ToolResultContent::Multiple(vec![])),
            is_error: None,
        }]);
        assert!(render_message_content(&multi).contains("<multi-block result>"));

        let none = MessageContent::Blocks(vec![ContentBlock::ToolResult {
            tool_use_id: "tu_4".to_string(),
            content: None,
            is_error: None,
        }]);
        // Empty body, non-error prefix.
        assert_eq!(render_message_content(&none), "↳ ");
    }

    #[test]
    fn test_render_message_content_thinking() {
        let c = MessageContent::Blocks(vec![ContentBlock::Thinking {
            thinking: "let me consider".to_string(),
        }]);
        let out = render_message_content(&c);
        assert!(out.contains("💭"));
        assert!(out.contains("let me consider"));
    }

    #[test]
    fn test_truncate_for_display_short_unchanged() {
        assert_eq!(truncate_for_display("hi", 10), "hi");
    }

    #[test]
    fn test_truncate_for_display_long_truncates_with_ellipsis() {
        let out = truncate_for_display("abcdefghij", 5);
        assert!(out.ends_with("..."));
        assert!(out.starts_with("ab"));
    }

    #[test]
    fn test_truncate_for_display_collapses_newlines() {
        assert_eq!(truncate_for_display("a\nb\nc", 100), "a ⏎ b ⏎ c");
    }

    #[test]
    fn test_format_relative_time_buckets() {
        let now = chrono::Utc::now();
        // "just now" — the function's internal `now` is captured within microseconds.
        assert_eq!(format_relative_time(now), "just now");
        assert_eq!(
            format_relative_time(now - chrono::Duration::minutes(5)),
            "5m ago"
        );
        assert_eq!(
            format_relative_time(now - chrono::Duration::hours(3)),
            "3h ago"
        );
        assert_eq!(
            format_relative_time(now - chrono::Duration::days(2)),
            "2d ago"
        );
        let old = now - chrono::Duration::days(30);
        assert_eq!(
            format_relative_time(old),
            old.format("%Y-%m-%d").to_string()
        );
    }

    #[test]
    fn test_format_session_picker_row_shape() {
        let info = StoredSessionInfo {
            session_id: uuid::Uuid::new_v4(),
            title: None,
            preview: Some("How do I parse JSON?".to_string()),
            last_user_preview: Some("Thanks, that worked".to_string()),
            model: "test-model".to_string(),
            created_at: chrono::Utc::now() - chrono::Duration::days(1),
            updated_at: chrono::Utc::now() - chrono::Duration::minutes(10),
            turn_count: 4,
            total_input_tokens: 0,
            total_output_tokens: 0,
            parent_session_id: None,
            branch_point_message_index: None,
            project_path: None,
        };
        let row = format_session_picker_row(&info);
        assert!(row.contains("How do I parse JSON?"), "row = {row}");
        assert!(row.contains("Thanks, that worked"), "row = {row}");
        assert!(row.contains('→'), "row = {row}");
        assert!(row.contains("10m ago"), "row = {row}");
        assert!(row.contains("4 turns"), "row = {row}");
        // Root session → no branch marker.
        assert!(!row.contains("⤵"), "row = {row}");
    }

    #[test]
    fn test_format_session_picker_row_branch_marker() {
        // A branch session (parent_session_id set) is prefixed with ⤵.
        let info = StoredSessionInfo {
            session_id: uuid::Uuid::new_v4(),
            title: None,
            preview: Some("Root idea".to_string()),
            last_user_preview: Some("Branch exploration".to_string()),
            model: "test-model".to_string(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            turn_count: 1,
            total_input_tokens: 0,
            total_output_tokens: 0,
            parent_session_id: Some(uuid::Uuid::new_v4()),
            branch_point_message_index: Some(3),
            project_path: None,
        };
        let row = format_session_picker_row(&info);
        assert!(row.starts_with("⤵ "), "row = {row}");
        assert!(row.contains("Root idea"), "row = {row}");
    }

    #[test]
    fn test_format_session_picker_row_falls_back_when_empty() {
        let info = StoredSessionInfo {
            session_id: uuid::Uuid::new_v4(),
            title: None,
            preview: None,
            last_user_preview: None,
            model: String::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            turn_count: 0,
            total_input_tokens: 0,
            total_output_tokens: 0,
            parent_session_id: None,
            branch_point_message_index: None,
            project_path: None,
        };
        let row = format_session_picker_row(&info);
        assert!(row.contains("(no prompt)"), "row = {row}");
        assert!(row.contains("—"), "row = {row}"); // last-prompt fallback dash
    }
}
