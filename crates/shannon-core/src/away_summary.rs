//! Away Summary
//!
//! Generates a short session recap when the user returns after being away.
//! Uses recent conversation messages to create a concise "where we left off" summary.

use serde::{Deserialize, Serialize};

/// Recent message window for away summary generation
const RECENT_MESSAGE_WINDOW: usize = 30;

/// A conversation message for summary generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub role: String,
    pub content: String,
    pub timestamp: Option<String>,
}

/// Away summary generator
pub struct AwaySummaryGenerator;

impl Default for AwaySummaryGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl AwaySummaryGenerator {
    pub fn new() -> Self {
        Self
    }

    /// Generate a short "where we left off" summary from recent messages
    pub fn generate(
        &self,
        messages: &[ConversationMessage],
        session_memory: Option<&str>,
    ) -> Option<String> {
        if messages.is_empty() {
            return None;
        }

        let recent: Vec<&ConversationMessage> = messages
            .iter()
            .rev()
            .take(RECENT_MESSAGE_WINDOW)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        // Build summary from recent messages -- only user messages are actionable context
        let last_user_msg = recent
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(|m| m.content.as_str());

        let task_hint = last_user_msg
            .map(|s| {
                let truncated = s.chars().take(200).collect::<String>();
                format!("Recent context: {truncated}")
            })
            .unwrap_or_default();

        let memory_hint = session_memory
            .map(|m| format!("Session memory: {m}"))
            .unwrap_or_default();

        if task_hint.is_empty() && memory_hint.is_empty() {
            return None;
        }

        Some(format!("{memory_hint}\n{task_hint}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_messages(msgs: Vec<(&str, &str)>) -> Vec<ConversationMessage> {
        msgs.into_iter()
            .map(|(role, content)| ConversationMessage {
                role: role.to_string(),
                content: content.to_string(),
                timestamp: None,
            })
            .collect()
    }

    #[test]
    fn test_empty_messages() {
        let generator = AwaySummaryGenerator::new();
        assert!(generator.generate(&[], None).is_none());
    }

    #[test]
    fn test_with_user_message() {
        let generator = AwaySummaryGenerator::new();
        let msgs = make_messages(vec![
            ("user", "Fix the auth bug in login.rs"),
            ("assistant", "I'll investigate the auth bug now."),
        ]);
        let summary = generator.generate(&msgs, None);
        assert!(summary.is_some());
        let s = summary.unwrap();
        assert!(s.contains("auth bug"));
    }

    #[test]
    fn test_with_memory() {
        let generator = AwaySummaryGenerator::new();
        let msgs = make_messages(vec![("user", "What is the project structure?")]);
        let summary = generator.generate(&msgs, Some("Building a Rust CLI tool"));
        assert!(summary.is_some());
        let s = summary.unwrap();
        assert!(s.contains("Building a Rust CLI tool"));
    }

    #[test]
    fn test_recent_window() {
        let generator = AwaySummaryGenerator::new();
        let mut msgs = Vec::new();
        for i in 0..50 {
            msgs.push(ConversationMessage {
                role: if i % 2 == 0 {
                    "user".to_string()
                } else {
                    "assistant".to_string()
                },
                content: format!("Message {i}"),
                timestamp: None,
            });
        }
        let summary = generator.generate(&msgs, None);
        assert!(summary.is_some());
    }

    #[test]
    fn test_no_actionable_content() {
        // Only assistant messages (no user message) means no actionable content
        let generator = AwaySummaryGenerator::new();
        let msgs = make_messages(vec![("assistant", "Okay.")]);
        let summary = generator.generate(&msgs, None);
        assert!(summary.is_none());
    }
}
