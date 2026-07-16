//! # Hooks System
//!
//! A hook system that allows executing shell commands at various lifecycle
//! points, similar to Claude Code's hook mechanism.
//!
//! ## Hook Events
//!
//! Hooks can be triggered on these events:
//! - [`HookEvent::PreToolUse`]: Before a tool is executed
//! - [`HookEvent::PostToolUse`]: After a tool completes successfully
//! - [`HookEvent::PostToolUseFailure`]: After a tool fails
//! - [`HookEvent::SessionStart`]: When a session begins
//! - [`HookEvent::SessionEnd`]: When a session ends
//! - [`HookEvent::Notification`]: When a notification is emitted
//! - [`HookEvent::UserPromptSubmit`]: When the user submits a prompt
//! - [`HookEvent::Stop`]: When the model stops generating
//! - [`HookEvent::StopFailure`]: When the model stops due to an error
//! - [`HookEvent::PreCompact`]: Before context compaction
//! - [`HookEvent::PostCompact`]: After context compaction completes
//! - [`HookEvent::SubagentStart`]: When a subagent is spawned
//! - [`HookEvent::SubagentStop`]: When a subagent finishes
//! - [`HookEvent::PermissionRequest`]: When a permission is requested
//! - [`HookEvent::PermissionDenied`]: When a permission is denied
//! - [`HookEvent::FileChanged`]: When a file is modified on disk
//! - [`HookEvent::CwdChanged`]: When the working directory changes
//!
//! ## Hook Types
//!
//! Each hook definition supports a `type` field controlling execution:
//! - `command` (default): Shell command via stdin/stdout protocol
//! - `http`: POST JSON to a URL
//! - `llm`: LLM-based evaluation with prompt template substitution
//! - `prompt`: Single-turn LLM evaluation
//!
//! ## Configuration
//!
//! Hooks are loaded from multiple locations (later files override earlier ones).
//! Claude Code's `settings.json` format is fully compatible — serde ignores
//! non-hook fields like `mcpServers`.
//!
//! **User-level** (lower priority):
//! - `~/.claude/settings.json`
//! - `~/.shannon/settings.json`
//! - `~/.shannon/hooks.json`
//!
//! **Project-level** (higher priority):
//! - `.claude/settings.json`
//! - `.claude/settings.local.json`
//! - `.shannon/settings.json`
//! - `.shannon/settings.local.json`
//! - `.shannon/hooks.json`
//!
//! ## Example hooks.json
//!
//! ```json
//! {
//!   "hooks": {
//!     "PreToolUse": [
//!       {
//!         "matcher": "Bash",
//!         "hooks": [
//!           { "command": "echo 'About to run bash'", "timeout": 5, "blocking": false }
//!         ]
//!       }
//!     ]
//!   }
//! }
//! ```

mod config;
mod events;
mod manager;
mod types;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;

// Re-export all public types to maintain the same public API
pub use config::{HookConfig, HookDecision, HookDef, HookResult, HookType, HooksFile};
pub use events::{HookEvent, HookEventType};
pub use manager::HookManager;
pub use types::HookError;
