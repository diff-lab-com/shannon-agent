//! Session restore and auto-restore functionality.

use crate::widgets::ChatRole;
use shannon_core::{ContentBlock, MessageContent};

impl super::Repl {
    /// Restore conversation history from a previously persisted session.
    ///
    /// Loads messages from the given `SessionData` and injects them into the
    /// query engine so the next user message continues the prior conversation.
    /// Also populates the chat widget so the user can see the restored history.
    /// Returns the number of messages restored.
    pub fn restore_session(&mut self, session_data: shannon_core::state::SessionData) -> usize {
        let msg_count = session_data.messages.len();
        if msg_count == 0 {
            return 0;
        }

        // Populate chat widget with restored messages so the user can see them
        for msg in &session_data.messages {
            let role = match msg.role.as_str() {
                "user" => ChatRole::User,
                "assistant" => ChatRole::Assistant,
                "system" => ChatRole::System,
                _ => ChatRole::Tool, // "tool" and any unknown roles
            };
            let text = match &msg.content {
                MessageContent::Text(t) => t.clone(),
                MessageContent::Blocks(blocks) => blocks
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            };
            self.chat.add_message(role, text);
        }

        if let Some(ref mut engine) = self.query_engine {
            let preview = session_data.first_user_message_preview(60);
            engine.replace_conversation(session_data.messages);
            tracing::info!(
                "Resumed session {} ({} messages, preview: {:?})",
                session_data.session_id,
                msg_count,
                preview,
            );
        }
        msg_count
    }

    /// Check for the most recent session and auto-restore it if it was
    /// active within the last 2 hours. Shows a system message to inform
    /// the user; they can start fresh with `/clear` if unwanted.
    pub(crate) fn auto_restore_last_session(&mut self) {
        let sessions = match self.state_manager.list_persisted_sessions() {
            Ok(s) => s,
            Err(_) => return,
        };
        if sessions.is_empty() {
            return;
        }

        // Find the most recently updated session
        let most_recent = match sessions
            .iter()
            .max_by_key(|s| s.updated_at)
        {
            Some(s) => s,
            None => return,
        };

        // Only auto-restore if updated within the last 2 hours
        let two_hours_ago = chrono::Utc::now() - chrono::Duration::hours(2);
        if most_recent.updated_at < two_hours_ago {
            return;
        }

        // Skip sessions with no turns (empty/stub sessions)
        if most_recent.turn_count == 0 {
            return;
        }

        let session_id = most_recent.session_id;
        let title = most_recent.title.as_deref()
            .or(most_recent.preview.as_deref())
            .unwrap_or("Untitled");

        if let Ok(Some(data)) = self.state_manager.load_session(&session_id) {
            let msg_count = data.messages.len();
            if msg_count == 0 {
                return;
            }

            // Show notice before restoring messages
            self.chat.add_message(ChatRole::System, format!(
                "Auto-restored session: \"{}\" ({} messages, {})\nType /clear to start fresh.",
                title, msg_count, most_recent.model,
            ));

            // Populate chat widget with restored messages
            for msg in &data.messages {
                let role = match msg.role.as_str() {
                    "user" => ChatRole::User,
                    "assistant" => ChatRole::Assistant,
                    "system" => ChatRole::System,
                    _ => ChatRole::Tool,
                };
                let text = match &msg.content {
                    MessageContent::Text(t) => t.clone(),
                    MessageContent::Blocks(blocks) => blocks
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                };
                self.chat.add_message(role, text);
            }

            // Restore in query engine
            if let Some(ref mut engine) = self.query_engine {
                engine.replace_conversation(data.messages);
                if let Err(e) = engine.restore_session(session_id) {
                    tracing::warn!("Auto-restore engine session failed: {e}");
                }
            }

            self.state.tokens_used = most_recent.total_input_tokens + most_recent.total_output_tokens;
            if !most_recent.model.is_empty() {
                self.state.model = Some(most_recent.model.clone());
            }

            tracing::info!(
                "Auto-restored session {} (\"{}\", {} msgs)",
                session_id, title, msg_count,
            );
        }
    }
}
