//! Subagent context isolation
//!
//! Provides an isolated context window for subagents. Each subagent runs with
//! its own message history that is not shared with the parent session. When the
//! subagent finishes, only a structured summary is returned to the caller,
//! preventing context pollution.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Message types for isolated conversation turns
// ---------------------------------------------------------------------------

/// Role of a speaker in an isolated context conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextRole {
    /// System-level instructions (e.g. agent system prompt)
    System,
    /// User / task input
    User,
    /// Assistant response
    Assistant,
    /// Tool call result
    Tool,
}

/// A single message inside an isolated subagent context.
///
/// This is distinct from the inter-agent [`AgentMessage`](crate::message::AgentMessage):
/// it represents a conversation turn within a subagent's private LLM session,
/// not a message sent between agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextMessage {
    /// Role of the speaker
    pub role: ContextRole,
    /// Text content of the message
    pub content: String,
    /// Unix-epoch timestamp (seconds)
    pub timestamp: i64,
}

impl ContextMessage {
    /// Create a new context message with the current timestamp.
    pub fn new(role: ContextRole, content: String) -> Self {
        Self {
            role,
            content,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for creating an [`IsolatedContext`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsolationConfig {
    /// Maximum context tokens for the subagent.
    pub max_tokens: usize,
    /// Whether to include the parent's system prompt.
    #[serde(default = "default_true")]
    pub inherit_system_prompt: bool,
    /// Whether to include the parent's project instructions.
    #[serde(default = "default_true")]
    pub inherit_project_instructions: bool,
    /// Maximum turns the subagent can take (`None` = unlimited).
    pub max_turns: Option<usize>,
}

fn default_true() -> bool {
    true
}

impl Default for IsolationConfig {
    fn default() -> Self {
        Self {
            max_tokens: 100_000,
            inherit_system_prompt: true,
            inherit_project_instructions: true,
            max_turns: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Summary returned from a completed subagent execution
// ---------------------------------------------------------------------------

/// Structured summary returned from an isolated subagent execution.
///
/// Only this data crosses the isolation boundary back to the parent session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentSummary {
    /// The subagent's context ID.
    pub context_id: String,
    /// Brief result summary (returned to parent).
    pub summary: String,
    /// Whether the task succeeded.
    pub success: bool,
    /// Files modified during execution (if any).
    #[serde(default)]
    pub files_modified: Vec<String>,
    /// Key findings (useful for research agents).
    #[serde(default)]
    pub findings: Vec<String>,
    /// Token usage within the subagent's context.
    pub tokens_used: usize,
}

// ---------------------------------------------------------------------------
// IsolatedContext
// ---------------------------------------------------------------------------

/// An isolated context window for a subagent.
///
/// The subagent runs with its own message history that is not shared with the
/// parent session. When execution finishes, call [`to_summary`](IsolatedContext::to_summary)
/// to produce a [`SubagentSummary`] that is the only data returned to the caller.
pub struct IsolatedContext {
    /// Unique context ID.
    id: String,
    /// The subagent's own message history (not shared with parent).
    messages: Vec<ContextMessage>,
    /// Maximum tokens for this context.
    max_tokens: usize,
    /// Current estimated token usage.
    current_tokens: usize,
    /// Whether the context is still active (accepting messages).
    active: bool,
    /// Maximum turns allowed.
    max_turns: Option<usize>,
}

/// Threshold (fraction of `max_tokens`) at which pressure is considered high.
const PRESSURE_THRESHOLD: f64 = 0.75;

impl IsolatedContext {
    /// Create a new isolated context from the given configuration.
    pub fn new(config: IsolationConfig) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            messages: Vec::new(),
            max_tokens: config.max_tokens,
            current_tokens: 0,
            active: true,
            max_turns: config.max_turns,
        }
    }

    /// Add a message to the isolated history.
    ///
    /// Updates the estimated token count. If the context has been deactivated
    /// (via [`deactivate`](Self::deactivate)), the message is silently dropped.
    pub fn add_message(&mut self, message: ContextMessage) {
        if !self.active {
            return;
        }

        let estimated = Self::estimate_tokens_for_str(&message.content);
        self.current_tokens += estimated;
        self.messages.push(message);
    }

    /// Rough token estimation for the entire message history.
    ///
    /// Uses the standard approximation of ~4 characters per token.
    pub fn estimate_tokens(&self) -> usize {
        self.current_tokens
    }

    /// Returns `true` when token usage exceeds 75% of `max_tokens`.
    pub fn is_pressure_high(&self) -> bool {
        if self.max_tokens == 0 {
            return true;
        }
        let ratio = self.current_tokens as f64 / self.max_tokens as f64;
        ratio > PRESSURE_THRESHOLD
    }

    /// Returns `true` if the context is still active.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Deactivate the context, preventing further messages from being added.
    pub fn deactivate(&mut self) {
        self.active = false;
    }

    /// Check whether the subagent has remaining turns.
    ///
    /// Returns `true` when `max_turns` is `None` (unlimited) or when the
    /// current message count (user + assistant turns only) is below the limit.
    pub fn has_turns_remaining(&self) -> bool {
        match self.max_turns {
            None => true,
            Some(max) => {
                // Count only User+Assistant messages as "turns"
                let turns = self
                    .messages
                    .iter()
                    .filter(|m| m.role == ContextRole::User || m.role == ContextRole::Assistant)
                    .count();
                turns < max
            }
        }
    }

    /// Convert the final context state into a summary for the parent session.
    ///
    /// This is the **only** data that crosses the isolation boundary.
    pub fn to_summary(&self, task_result: &str, success: bool) -> SubagentSummary {
        SubagentSummary {
            context_id: self.id.clone(),
            summary: task_result.to_string(),
            success,
            files_modified: Vec::new(),
            findings: Vec::new(),
            tokens_used: self.current_tokens,
        }
    }

    /// Convert the final context state into a summary with extra metadata.
    ///
    /// Use this overload when the caller can supply file paths and findings.
    pub fn to_summary_with(
        &self,
        task_result: &str,
        success: bool,
        files_modified: Vec<String>,
        findings: Vec<String>,
    ) -> SubagentSummary {
        SubagentSummary {
            context_id: self.id.clone(),
            summary: task_result.to_string(),
            success,
            files_modified,
            findings,
            tokens_used: self.current_tokens,
        }
    }

    /// Read-only access to the isolated message history.
    pub fn messages(&self) -> &[ContextMessage] {
        &self.messages
    }

    /// The unique context ID.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Maximum tokens configured for this context.
    pub fn max_tokens(&self) -> usize {
        self.max_tokens
    }

    // -- helpers --

    /// Estimate tokens for a string using the chars/4 heuristic.
    fn estimate_tokens_for_str(s: &str) -> usize {
        // Standard approximation: ~4 characters per token for English text.
        // Use `.ceil()` via integer arithmetic to avoid pulling in floats
        // everywhere: (len + 3) / 4.
        (s.len() + 3) / 4
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Lifecycle tests --

    #[test]
    fn new_context_is_active_with_defaults() {
        let config = IsolationConfig::default();
        let ctx = IsolatedContext::new(config);

        assert!(!ctx.id().is_empty());
        assert!(ctx.is_active());
        assert!(ctx.messages().is_empty());
        assert_eq!(ctx.estimate_tokens(), 0);
        assert!(!ctx.is_pressure_high());
        assert!(ctx.has_turns_remaining());
    }

    #[test]
    fn lifecycle_add_messages_then_summary() {
        let config = IsolationConfig {
            max_tokens: 10_000,
            ..Default::default()
        };
        let mut ctx = IsolatedContext::new(config);

        ctx.add_message(ContextMessage::new(ContextRole::System, "You are a helper.".into()));
        ctx.add_message(ContextMessage::new(ContextRole::User, "Do the thing.".into()));
        ctx.add_message(ContextMessage::new(ContextRole::Assistant, "Done!".into()));

        assert_eq!(ctx.messages().len(), 3);
        assert!(ctx.estimate_tokens() > 0);
        assert!(!ctx.is_pressure_high());

        let summary = ctx.to_summary("Task completed", true);
        assert_eq!(summary.context_id, ctx.id());
        assert!(summary.success);
        assert_eq!(summary.summary, "Task completed");
        assert_eq!(summary.tokens_used, ctx.estimate_tokens());
        assert!(summary.files_modified.is_empty());
        assert!(summary.findings.is_empty());
    }

    #[test]
    fn deactivated_context_rejects_messages() {
        let config = IsolationConfig::default();
        let mut ctx = IsolatedContext::new(config);

        ctx.add_message(ContextMessage::new(ContextRole::User, "first".into()));
        assert_eq!(ctx.messages().len(), 1);

        ctx.deactivate();
        assert!(!ctx.is_active());

        ctx.add_message(ContextMessage::new(ContextRole::User, "should be dropped".into()));
        assert_eq!(ctx.messages().len(), 1);
    }

    // -- Token estimation --

    #[test]
    fn token_estimation_accuracy() {
        // Empty string -> 0 tokens
        assert_eq!(IsolatedContext::estimate_tokens_for_str(""), 0);

        // 4 chars -> 1 token
        assert_eq!(IsolatedContext::estimate_tokens_for_str("abcd"), 1);

        // 5 chars -> 2 tokens (ceiling)
        assert_eq!(IsolatedContext::estimate_tokens_for_str("abcde"), 2);

        // 100 chars -> 25 tokens
        assert_eq!(IsolatedContext::estimate_tokens_for_str(&"x".repeat(100)), 25);

        // 1 char -> 1 token
        assert_eq!(IsolatedContext::estimate_tokens_for_str("a"), 1);

        // 8 chars -> 2 tokens (exact division)
        assert_eq!(IsolatedContext::estimate_tokens_for_str("abcdefgh"), 2);
    }

    #[test]
    fn token_estimation_accumulates_across_messages() {
        let config = IsolationConfig::default();
        let mut ctx = IsolatedContext::new(config);

        // "abcd" = 1 token
        ctx.add_message(ContextMessage::new(ContextRole::User, "abcd".into()));
        assert_eq!(ctx.estimate_tokens(), 1);

        // "abcdefgh" = 2 tokens, total = 3
        ctx.add_message(ContextMessage::new(ContextRole::Assistant, "abcdefgh".into()));
        assert_eq!(ctx.estimate_tokens(), 3);
    }

    // -- Pressure detection --

    #[test]
    fn pressure_threshold_at_75_percent() {
        let config = IsolationConfig {
            max_tokens: 100,
            ..Default::default()
        };
        let mut ctx = IsolatedContext::new(config);

        // Add messages until just under 75 tokens
        // Each char adds 0.25 tokens, so 296 chars = 74 tokens (296/4 = 74)
        ctx.add_message(ContextMessage::new(
            ContextRole::User,
            "a".repeat(296),
        ));
        assert_eq!(ctx.estimate_tokens(), 74);
        assert!(!ctx.is_pressure_high(), "74/100 should not be high pressure");

        // Add one more token to cross 75
        ctx.add_message(ContextMessage::new(
            ContextRole::Assistant,
            "a".repeat(8), // 8 chars = 2 tokens, total = 76
        ));
        assert_eq!(ctx.estimate_tokens(), 76);
        assert!(ctx.is_pressure_high(), "76/100 should be high pressure");
    }

    #[test]
    fn zero_max_tokens_always_high_pressure() {
        let config = IsolationConfig {
            max_tokens: 0,
            ..Default::default()
        };
        let ctx = IsolatedContext::new(config);
        assert!(ctx.is_pressure_high());
    }

    // -- Isolation guarantee --

    #[test]
    fn messages_do_not_leak_between_contexts() {
        let config = IsolationConfig::default();

        let mut ctx_a = IsolatedContext::new(config.clone());
        let mut ctx_b = IsolatedContext::new(config);

        ctx_a.add_message(ContextMessage::new(ContextRole::User, "secret for A".into()));
        ctx_b.add_message(ContextMessage::new(ContextRole::User, "secret for B".into()));
        ctx_a.add_message(ContextMessage::new(ContextRole::Assistant, "A response".into()));

        // Each context only sees its own messages
        assert_eq!(ctx_a.messages().len(), 2);
        assert_eq!(ctx_b.messages().len(), 1);

        assert_eq!(ctx_a.messages()[0].content, "secret for A");
        assert_eq!(ctx_a.messages()[1].content, "A response");
        assert_eq!(ctx_b.messages()[0].content, "secret for B");

        // Different IDs
        assert_ne!(ctx_a.id(), ctx_b.id());
    }

    // -- Turn limiting --

    #[test]
    fn max_turns_enforcement() {
        let config = IsolationConfig {
            max_turns: Some(2),
            ..Default::default()
        };
        let mut ctx = IsolatedContext::new(config);

        // System messages don't count as turns
        ctx.add_message(ContextMessage::new(ContextRole::System, "sys".into()));
        assert!(ctx.has_turns_remaining());

        // After 1 user + 1 assistant message = 2 turns (individual messages counted)
        ctx.add_message(ContextMessage::new(ContextRole::User, "u1".into()));
        assert!(ctx.has_turns_remaining()); // 1 turn used, limit is 2
        ctx.add_message(ContextMessage::new(ContextRole::Assistant, "a1".into()));
        assert!(!ctx.has_turns_remaining()); // 2 turns used, limit is 2 -> at limit
    }

    #[test]
    fn unlimited_turns_when_none() {
        let config = IsolationConfig {
            max_turns: None,
            ..Default::default()
        };
        let mut ctx = IsolatedContext::new(config);

        for i in 0..50 {
            ctx.add_message(ContextMessage::new(ContextRole::User, format!("u{i}")));
            ctx.add_message(ContextMessage::new(ContextRole::Assistant, format!("a{i}")));
        }
        assert!(ctx.has_turns_remaining());
    }

    // -- Summary generation --

    #[test]
    fn summary_with_files_and_findings() {
        let config = IsolationConfig {
            max_tokens: 50_000,
            ..Default::default()
        };
        let mut ctx = IsolatedContext::new(config);

        ctx.add_message(ContextMessage::new(ContextRole::User, "research X".into()));

        let files = vec!["src/main.rs".into(), "src/lib.rs".into()];
        let findings = vec!["X is related to Y".into(), "Z needs attention".into()];

        let summary = ctx.to_summary_with(
            "Researched X",
            true,
            files.clone(),
            findings.clone(),
        );

        assert_eq!(summary.context_id, ctx.id());
        assert!(summary.success);
        assert_eq!(summary.summary, "Researched X");
        assert_eq!(summary.files_modified, files);
        assert_eq!(summary.findings, findings);
        assert!(summary.tokens_used > 0);
    }

    #[test]
    fn summary_after_deactivation() {
        let config = IsolationConfig::default();
        let mut ctx = IsolatedContext::new(config);

        ctx.add_message(ContextMessage::new(ContextRole::User, "task".into()));
        ctx.deactivate();

        let summary = ctx.to_summary("finished", true);
        assert!(summary.success);
        assert_eq!(summary.summary, "finished");
    }

    // -- Serialization round-trip --

    #[test]
    fn context_message_serde_roundtrip() {
        let msg = ContextMessage::new(ContextRole::Assistant, "hello world".into());
        let json = serde_json::to_string(&msg).unwrap();
        let decoded: ContextMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.role, ContextRole::Assistant);
        assert_eq!(decoded.content, "hello world");
    }

    #[test]
    fn isolation_config_serde_roundtrip() {
        let config = IsolationConfig {
            max_tokens: 200_000,
            inherit_system_prompt: false,
            inherit_project_instructions: true,
            max_turns: Some(25),
        };
        let json = serde_json::to_string(&config).unwrap();
        let decoded: IsolationConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.max_tokens, 200_000);
        assert!(!decoded.inherit_system_prompt);
        assert!(decoded.inherit_project_instructions);
        assert_eq!(decoded.max_turns, Some(25));
    }

    #[test]
    fn subagent_summary_serde_roundtrip() {
        let summary = SubagentSummary {
            context_id: "test-id".into(),
            summary: "did work".into(),
            success: true,
            files_modified: vec!["a.rs".into()],
            findings: vec!["found X".into()],
            tokens_used: 1234,
        };
        let json = serde_json::to_string(&summary).unwrap();
        let decoded: SubagentSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.context_id, summary.context_id);
        assert_eq!(decoded.files_modified, summary.files_modified);
        assert_eq!(decoded.findings, summary.findings);
        assert_eq!(decoded.tokens_used, 1234);
    }

    #[test]
    fn subagent_summary_default_collections() {
        // Ensure Vec fields default to empty when deserialized from minimal JSON
        let json = r#"{"context_id":"c","summary":"s","success":false,"tokens_used":0}"#;
        let decoded: SubagentSummary = serde_json::from_str(json).unwrap();
        assert!(decoded.files_modified.is_empty());
        assert!(decoded.findings.is_empty());
    }
}
