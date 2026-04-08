//! Hook system for extension points

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Hook errors
#[derive(Error, Debug)]
pub enum HookError {
    #[error("Hook execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Hook not found: {0}")]
    NotFound(String),

    #[error("Hook already registered: {0}")]
    AlreadyRegistered(String),
}

/// Hook event types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HookEventType {
    BeforeQuery,
    AfterQuery,
    BeforeTool,
    AfterTool,
}

/// Hook event data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookEvent {
    pub event_type: HookEventType,
    pub data: serde_json::Value,
}

/// Hook decision (allow/deny)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum HookDecision {
    Allow,
    Deny { reason: String },
}

/// Hook result
#[derive(Debug, Clone)]
pub struct HookResult {
    pub decision: HookDecision,
    pub data: Option<serde_json::Value>,
}

/// Hook manager (placeholder - to be implemented)
pub struct HookManager;

impl HookManager {
    pub fn new() -> Self {
        Self
    }

    pub fn register(&mut self, _event_type: HookEventType, _hook: Box<dyn Fn(&HookEvent) -> HookResult + Send + Sync>) -> Result<(), HookError> {
        Ok(())
    }

    pub fn trigger(&self, _event: HookEvent) -> HookResult {
        HookResult {
            decision: HookDecision::Allow,
            data: None,
        }
    }
}
