//! Plan mode tools
//!
//! Provides implementations for:
//! - EnterPlanMode: Switch to read-only planning mode
//! - ExitPlanMode: Exit plan mode, return to normal editing
//!
//! Plan mode disables file modifications to support read-only analysis
//! and planning workflows.

use crate::{Tool, ToolError, ToolResult, ToolOutput};
use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Shared plan mode state. `true` means plan mode is active (file modifications disabled).
pub type PlanModeState = Arc<RwLock<bool>>;

/// Create a new shared plan mode state, initially inactive.
pub fn new_plan_mode_state() -> PlanModeState {
    Arc::new(RwLock::new(false))
}

/// Check whether plan mode is currently active.
pub fn is_plan_mode_active(state: &PlanModeState) -> Result<bool, ToolError> {
    state
        .read()
        .map(|guard| *guard)
        .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read plan mode state: {}", e)))
}

/// Enter plan mode - switches to read-only analysis.
///
/// While in plan mode, file modifications (write, edit, bash write operations)
/// should be blocked by the query engine.
pub struct EnterPlanModeTool {
    plan_mode: PlanModeState,
}

impl EnterPlanModeTool {
    /// Create a new EnterPlanModeTool with the given shared state.
    pub fn new(plan_mode: PlanModeState) -> Self {
        Self { plan_mode }
    }
}

#[async_trait]
impl Tool for EnterPlanModeTool {
    fn name(&self) -> &str {
        "enter_plan_mode"
    }

    fn description(&self) -> &str {
        "Enter plan mode for read-only analysis and planning. File modifications will be disabled."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _input: serde_json::Value) -> ToolResult<ToolOutput> {
        {
            let mut state = self.plan_mode.write().map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to acquire plan mode lock: {}", e))
            })?;
            *state = true;
        }

        Ok(ToolOutput {
            content: "Entered plan mode. File modifications are disabled.".to_string(),
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("plan_mode_active".to_string(), json!(true));
                map
            },
        })
    }

    fn category(&self) -> &str {
        "mode"
    }
}

/// Exit plan mode - returns to normal editing mode.
pub struct ExitPlanModeTool {
    plan_mode: PlanModeState,
}

impl ExitPlanModeTool {
    /// Create a new ExitPlanModeTool with the given shared state.
    pub fn new(plan_mode: PlanModeState) -> Self {
        Self { plan_mode }
    }
}

#[async_trait]
impl Tool for ExitPlanModeTool {
    fn name(&self) -> &str {
        "exit_plan_mode"
    }

    fn description(&self) -> &str {
        "Exit plan mode and return to normal editing mode."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _input: serde_json::Value) -> ToolResult<ToolOutput> {
        {
            let mut state = self.plan_mode.write().map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to acquire plan mode lock: {}", e))
            })?;
            *state = false;
        }

        Ok(ToolOutput {
            content: "Exited plan mode.".to_string(),
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("plan_mode_active".to_string(), json!(false));
                map
            },
        })
    }

    fn category(&self) -> &str {
        "mode"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_plan_mode_state() {
        let state = new_plan_mode_state();
        assert_eq!(*state.read().unwrap(), false);
    }

    #[test]
    fn test_is_plan_mode_active_initial() {
        let state = new_plan_mode_state();
        assert!(!is_plan_mode_active(&state).unwrap());
    }

    #[test]
    fn test_enter_plan_mode_tool_name() {
        let state = new_plan_mode_state();
        let tool = EnterPlanModeTool::new(state);
        assert_eq!(tool.name(), "enter_plan_mode");
    }

    #[test]
    fn test_enter_plan_mode_tool_description() {
        let state = new_plan_mode_state();
        let tool = EnterPlanModeTool::new(state);
        assert!(tool.description().contains("read-only"));
        assert!(tool.description().contains("plan mode"));
    }

    #[test]
    fn test_enter_plan_mode_tool_category() {
        let state = new_plan_mode_state();
        let tool = EnterPlanModeTool::new(state);
        assert_eq!(tool.category(), "mode");
    }

    #[test]
    fn test_exit_plan_mode_tool_name() {
        let state = new_plan_mode_state();
        let tool = ExitPlanModeTool::new(state);
        assert_eq!(tool.name(), "exit_plan_mode");
    }

    #[test]
    fn test_exit_plan_mode_tool_description() {
        let state = new_plan_mode_state();
        let tool = ExitPlanModeTool::new(state);
        assert!(tool.description().to_lowercase().contains("exit"));
        assert!(tool.description().contains("plan mode"));
    }

    #[test]
    fn test_exit_plan_mode_tool_category() {
        let state = new_plan_mode_state();
        let tool = ExitPlanModeTool::new(state);
        assert_eq!(tool.category(), "mode");
    }

    #[test]
    fn test_enter_plan_mode_input_schema() {
        let state = new_plan_mode_state();
        let tool = EnterPlanModeTool::new(state);
        let schema = tool.input_schema();

        assert_eq!(schema.get("type").unwrap().as_str().unwrap(), "object");
        let properties = schema.get("properties").unwrap().as_object().unwrap();
        assert!(properties.is_empty());
    }

    #[test]
    fn test_exit_plan_mode_input_schema() {
        let state = new_plan_mode_state();
        let tool = ExitPlanModeTool::new(state);
        let schema = tool.input_schema();

        assert_eq!(schema.get("type").unwrap().as_str().unwrap(), "object");
        let properties = schema.get("properties").unwrap().as_object().unwrap();
        assert!(properties.is_empty());
    }

    #[tokio::test]
    async fn test_enter_plan_mode_activates_state() {
        let state = new_plan_mode_state();
        let tool = EnterPlanModeTool::new(state.clone());

        assert!(!is_plan_mode_active(&state).unwrap());

        let result = tool.execute(json!({})).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(!output.is_error);
        assert!(output.content.contains("Entered plan mode"));

        assert!(is_plan_mode_active(&state).unwrap());
        assert_eq!(
            output.metadata.get("plan_mode_active").unwrap().as_bool().unwrap(),
            true
        );
    }

    #[tokio::test]
    async fn test_exit_plan_mode_deactivates_state() {
        let state = new_plan_mode_state();

        // Pre-set plan mode to active
        {
            let mut lock = state.write().unwrap();
            *lock = true;
        }
        assert!(is_plan_mode_active(&state).unwrap());

        let tool = ExitPlanModeTool::new(state.clone());
        let result = tool.execute(json!({})).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(!output.is_error);
        assert!(output.content.contains("Exited plan mode"));

        assert!(!is_plan_mode_active(&state).unwrap());
        assert_eq!(
            output.metadata.get("plan_mode_active").unwrap().as_bool().unwrap(),
            false
        );
    }

    #[tokio::test]
    async fn test_enter_exit_cycle() {
        let state = new_plan_mode_state();
        let enter = EnterPlanModeTool::new(state.clone());
        let exit = ExitPlanModeTool::new(state.clone());

        // Initial state: not in plan mode
        assert!(!is_plan_mode_active(&state).unwrap());

        // Enter plan mode
        enter.execute(json!({})).await.unwrap();
        assert!(is_plan_mode_active(&state).unwrap());

        // Exit plan mode
        exit.execute(json!({})).await.unwrap();
        assert!(!is_plan_mode_active(&state).unwrap());

        // Enter again
        enter.execute(json!({})).await.unwrap();
        assert!(is_plan_mode_active(&state).unwrap());
    }

    #[tokio::test]
    async fn test_shared_state_across_tools() {
        let state = new_plan_mode_state();
        let enter = EnterPlanModeTool::new(state.clone());
        let exit = ExitPlanModeTool::new(state.clone());

        // Enter via one tool, verify via the state accessor
        enter.execute(json!({})).await.unwrap();
        assert!(is_plan_mode_active(&state).unwrap());

        // Exit via the other tool
        exit.execute(json!({})).await.unwrap();
        assert!(!is_plan_mode_active(&state).unwrap());
    }

    #[tokio::test]
    async fn test_enter_plan_mode_idempotent() {
        let state = new_plan_mode_state();
        let tool = EnterPlanModeTool::new(state.clone());

        // Enter twice
        tool.execute(json!({})).await.unwrap();
        tool.execute(json!({})).await.unwrap();

        // Should still be active
        assert!(is_plan_mode_active(&state).unwrap());
    }

    #[tokio::test]
    async fn test_exit_plan_mode_idempotent() {
        let state = new_plan_mode_state();
        let tool = ExitPlanModeTool::new(state.clone());

        // Exit when already inactive
        tool.execute(json!({})).await.unwrap();
        tool.execute(json!({})).await.unwrap();

        // Should still be inactive
        assert!(!is_plan_mode_active(&state).unwrap());
    }

    #[tokio::test]
    async fn test_enter_plan_mode_accepts_any_input() {
        let state = new_plan_mode_state();
        let tool = EnterPlanModeTool::new(state.clone());

        // Execute with extra/irrelevant input -- should succeed since schema has no required fields
        let result = tool.execute(json!({"reason": "planning a feature"})).await;
        assert!(result.is_ok());
        assert!(is_plan_mode_active(&state).unwrap());
    }
}
