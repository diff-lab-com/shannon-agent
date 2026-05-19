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

    /// Auto-restore is disabled by default. Users expect a fresh session on startup.
    /// Use `/resume` or `--resume` to explicitly continue a previous session.
    pub(crate) fn auto_restore_last_session(&mut self) {
        // Disabled: auto-restore was confusing — users expect a fresh session on startup.
        // Use /resume or --resume to continue a previous session.
    }
}
