//! Plan mode tools
//!
//! Provides implementations for:
//! - EnterPlanMode: Switch to read-only planning mode
//! - ExitPlanMode: Exit plan mode, return to normal editing
//! - GetPlanStatus: Query current plan mode state and plan content
//!
//! Plan mode disables file modifications to support read-only analysis
//! and planning workflows. Plans are tracked with rich state including
//! content, approval status, and file-based persistence.

use crate::{Tool, ToolError, ToolResult, ToolOutput};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Plan data types
// ---------------------------------------------------------------------------

/// A single plan entry with content and lifecycle tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanEntry {
    /// Unique identifier for this plan.
    pub id: String,
    /// The plan content / description.
    pub content: String,
    /// When this plan was created.
    pub created_at: DateTime<Utc>,
    /// Whether the user has approved this plan.
    pub approved: bool,
    /// When the plan was approved, if it was.
    pub approved_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// PlanState — internal mutable state behind Arc<RwLock<..>>
// ---------------------------------------------------------------------------

/// Internal state for the plan mode system.
#[derive(Debug, Clone)]
#[derive(Default)]
struct PlanState {
    active: bool,
    current_plan: Option<PlanEntry>,
    plan_history: Vec<PlanEntry>,
    plan_file_path: Option<PathBuf>,
}


// ---------------------------------------------------------------------------
// PlanManager — public API wrapping Arc<RwLock<PlanState>>
// ---------------------------------------------------------------------------

/// Thread-safe manager for plan mode state and plan content.
///
/// Wraps `Arc<RwLock<PlanState>>` to provide safe concurrent access to plan
/// mode state, plan content tracking, approval flow, and file persistence.
#[derive(Debug, Clone)]
pub struct PlanManager {
    state: Arc<RwLock<PlanState>>,
}

impl PlanManager {
    /// Create a new `PlanManager` with inactive plan mode and no current plan.
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(PlanState::default())),
        }
    }

    /// Activate plan mode (disables file modifications).
    pub fn enter_plan_mode(&self) -> Result<(), ToolError> {
        let mut state = self.state.write().map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to acquire plan mode lock: {e}"))
        })?;
        state.active = true;
        Ok(())
    }

    /// Deactivate plan mode (re-enables file modifications).
    pub fn exit_plan_mode(&self) -> Result<(), ToolError> {
        let mut state = self.state.write().map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to acquire plan mode lock: {e}"))
        })?;
        state.active = false;
        Ok(())
    }

    /// Check whether plan mode is currently active.
    pub fn is_active(&self) -> Result<bool, ToolError> {
        let state = self.state.read().map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to read plan mode state: {e}"))
        })?;
        Ok(state.active)
    }

    /// Set the current plan content. Returns the plan ID.
    ///
    /// If there is an existing unapproved plan it is moved to history as rejected
    /// before the new plan is created.
    pub fn set_plan(&self, content: &str) -> Result<String, ToolError> {
        let mut state = self.state.write().map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to acquire plan mode lock: {e}"))
        })?;

        // Move any existing unapproved plan to history as rejected.
        if let Some(existing) = state.current_plan.take() {
            if !existing.approved {
                state.plan_history.push(existing);
            } else {
                // Keep the approved plan in current; push new one separately.
                state.plan_history.push(existing);
            }
        }

        let id = Uuid::new_v4().to_string();
        let entry = PlanEntry {
            id: id.clone(),
            content: content.to_string(),
            created_at: Utc::now(),
            approved: false,
            approved_at: None,
        };
        state.current_plan = Some(entry);
        Ok(id)
    }

    /// Get a clone of the current plan, if any.
    pub fn get_plan(&self) -> Result<Option<PlanEntry>, ToolError> {
        let state = self.state.read().map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to read plan mode state: {e}"))
        })?;
        Ok(state.current_plan.clone())
    }

    /// Mark the current plan as approved. Returns the approved plan.
    ///
    /// Returns an error if there is no current plan.
    pub fn approve_plan(&self) -> Result<PlanEntry, ToolError> {
        let mut state = self.state.write().map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to acquire plan mode lock: {e}"))
        })?;

        let plan = state.current_plan.as_mut().ok_or_else(|| {
            ToolError::ExecutionFailed("No current plan to approve".to_string())
        })?;

        plan.approved = true;
        plan.approved_at = Some(Utc::now());

        Ok(state.current_plan.clone().unwrap())
    }

    /// Reject the current plan, moving it to history as a rejected entry.
    ///
    /// Returns an error if there is no current plan.
    pub fn reject_plan(&self) -> Result<PlanEntry, ToolError> {
        let mut state = self.state.write().map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to acquire plan mode lock: {e}"))
        })?;

        let plan = state.current_plan.take().ok_or_else(|| {
            ToolError::ExecutionFailed("No current plan to reject".to_string())
        })?;

        // The plan remains in history as not-approved (i.e. rejected).
        state.plan_history.push(plan.clone());
        Ok(plan)
    }

    /// Return a read-only snapshot of plan history.
    pub fn plan_history(&self) -> Result<Vec<PlanEntry>, ToolError> {
        let state = self.state.read().map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to read plan mode state: {e}"))
        })?;
        Ok(state.plan_history.clone())
    }

    /// Set the directory where plans should be persisted.
    pub fn set_plan_file_path(&self, path: PathBuf) -> Result<(), ToolError> {
        let mut state = self.state.write().map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to acquire plan mode lock: {e}"))
        })?;
        state.plan_file_path = Some(path);
        Ok(())
    }

    /// Persist the current plan to the `.shannon/plans/` directory under `dir`.
    ///
    /// The file is named `{plan_id}.md` and contains a markdown header with
    /// metadata followed by the plan content.
    pub fn save_plan_to_file(&self, dir: &Path) -> Result<PathBuf, ToolError> {
        let state = self.state.read().map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to read plan mode state: {e}"))
        })?;

        let plan = state
            .current_plan
            .as_ref()
            .ok_or_else(|| ToolError::ExecutionFailed("No current plan to save".to_string()))?;

        let plans_dir = dir.join(".shannon").join("plans");
        fs::create_dir_all(&plans_dir).map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to create plans directory: {e}"))
        })?;

        let title = plan
            .content
            .lines()
            .next()
            .unwrap_or("Untitled Plan")
            .trim_start_matches('#')
            .trim();

        let status = if plan.approved {
            "approved"
        } else {
            "pending"
        };

        let file_content = format!(
            "# Plan: {}\nCreated: {}\nStatus: {}\n\n{}",
            title,
            plan.created_at.to_rfc3339(),
            status,
            plan.content
        );

        let file_path = plans_dir.join(format!("{}.md", plan.id));
        fs::write(&file_path, file_content).map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to write plan file: {e}"))
        })?;

        Ok(file_path)
    }

    /// Load a plan from a markdown file on disk.
    ///
    /// Parses the file header for title, creation timestamp, and status, then
    /// reconstructs a `PlanEntry`.
    pub fn load_plan_from_file(&self, path: &Path) -> Result<PlanEntry, ToolError> {
        let content = fs::read_to_string(path).map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to read plan file: {e}"))
        })?;

        let mut lines = content.lines();

        // Parse "# Plan: {title}"
        let _title = lines
            .next()
            .and_then(|l| l.strip_prefix("# Plan: "))
            .unwrap_or("Untitled")
            .to_string();

        // Parse "Created: {timestamp}"
        let created_at = lines
            .next()
            .and_then(|l| l.strip_prefix("Created: "))
            .and_then(|ts| DateTime::parse_from_rfc3339(ts).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(Utc::now);

        // Parse "Status: {approved|pending|rejected}"
        let status_str = lines
            .next()
            .and_then(|l| l.strip_prefix("Status: "))
            .unwrap_or("pending");
        let approved = status_str == "approved";

        // Skip the blank line after header
        let _blank = lines.next();

        // Remaining lines are the plan content
        let plan_content = lines.collect::<Vec<_>>().join("\n");

        // Derive an ID from the file stem
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let approved_at = if approved { Some(Utc::now()) } else { None };

        Ok(PlanEntry {
            id,
            content: plan_content,
            created_at,
            approved,
            approved_at,
        })
    }

    /// List all saved plan files in the `.shannon/plans/` directory under `dir`.
    ///
    /// Returns a list of `(filename, PlanEntry)` pairs sorted by creation time
    /// (newest first).
    pub fn list_plans(&self, dir: &Path) -> Result<Vec<(String, PlanEntry)>, ToolError> {
        let plans_dir = dir.join(".shannon").join("plans");
        if !plans_dir.exists() {
            return Ok(Vec::new());
        }

        let entries = fs::read_dir(&plans_dir).map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to read plans directory: {e}"))
        })?;

        let mut plans: Vec<(String, PlanEntry)> = Vec::new();

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("md") {
                let filename = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                match self.load_plan_from_file(&path) {
                    Ok(plan) => plans.push((filename, plan)),
                    Err(_) => continue, // skip malformed files
                }
            }
        }

        // Sort newest first
        plans.sort_by(|a, b| b.1.created_at.cmp(&a.1.created_at));
        Ok(plans)
    }

    /// Format the current plan for inclusion in an LLM prompt.
    ///
    /// Returns a structured string describing plan mode state and current plan
    /// content, or a brief notice if there is no plan.
    pub fn get_plan_content_for_prompt(&self) -> Result<String, ToolError> {
        let state = self.state.read().map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to read plan mode state: {e}"))
        })?;

        if !state.active {
            return Ok("Plan mode is not active.".to_string());
        }

        match &state.current_plan {
            None => Ok("Plan mode is active. No plan has been created yet.".to_string()),
            Some(plan) => {
                let status = if plan.approved { "APPROVED" } else { "PENDING APPROVAL" };
                Ok(format!(
                    "Plan mode is active.\n\
                     Plan ID: {}\n\
                     Status: {}\n\
                     Created: {}\n\
                     \n\
                     ## Plan Content\n\
                     {}",
                    plan.id,
                    status,
                    plan.created_at.to_rfc3339(),
                    plan.content
                ))
            }
        }
    }
}

impl Default for PlanManager {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Legacy compatibility — PlanModeState = Arc<RwLock<bool>>
// ---------------------------------------------------------------------------

/// Shared plan mode state. `true` means plan mode is active (file modifications disabled).
///
/// Kept for backward compatibility. New code should use `PlanManager`.
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
        .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read plan mode state: {e}")))
}

// ---------------------------------------------------------------------------
// EnterPlanModeTool
// ---------------------------------------------------------------------------

/// Enter plan mode - switches to read-only analysis.
///
/// While in plan mode, file modifications (write, edit, bash write operations)
/// should be blocked by the query engine.
pub struct EnterPlanModeTool {
    plan_mode: PlanModeState,
    plan_manager: Option<PlanManager>,
}

impl EnterPlanModeTool {
    /// Create a new `EnterPlanModeTool` with the given legacy shared state.
    pub fn new(plan_mode: PlanModeState) -> Self {
        Self {
            plan_mode,
            plan_manager: None,
        }
    }

    /// Create with a `PlanManager` for rich plan content tracking.
    pub fn with_manager(plan_manager: PlanManager) -> Self {
        let state = plan_manager.is_active()
            .ok()
            .map(|active| Arc::new(RwLock::new(active)))
            .unwrap_or_else(new_plan_mode_state);
        Self {
            plan_mode: state,
            plan_manager: Some(plan_manager),
        }
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
            "properties": {
                "reason": {
                    "type": "string",
                    "description": "Optional reason for entering plan mode"
                }
            }
        })
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        {
            let mut state = self.plan_mode.write().map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to acquire plan mode lock: {e}"))
            })?;
            *state = true;
        }

        // Also activate via PlanManager if available.
        if let Some(ref manager) = self.plan_manager {
            manager.enter_plan_mode()?;
        }

        let reason = input
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let content = if reason.is_empty() {
            "Entered plan mode. File modifications are disabled.".to_string()
        } else {
            format!(
                "Entered plan mode. File modifications are disabled. Reason: {reason}"
            )
        };

        Ok(ToolOutput {
            content,
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("plan_mode_active".to_string(), json!(true));
                if !reason.is_empty() {
                    map.insert("reason".to_string(), json!(reason));
                }
                map
            },
        })
    }

    fn category(&self) -> &str {
        "mode"
    }

    fn is_read_only(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// ExitPlanModeTool
// ---------------------------------------------------------------------------

/// Exit plan mode - returns to normal editing mode.
pub struct ExitPlanModeTool {
    plan_mode: PlanModeState,
    plan_manager: Option<PlanManager>,
}

impl ExitPlanModeTool {
    /// Create a new `ExitPlanModeTool` with the given legacy shared state.
    pub fn new(plan_mode: PlanModeState) -> Self {
        Self {
            plan_mode,
            plan_manager: None,
        }
    }

    /// Create with a `PlanManager` for rich plan content tracking.
    pub fn with_manager(plan_manager: PlanManager) -> Self {
        let state = plan_manager.is_active()
            .ok()
            .map(|active| Arc::new(RwLock::new(active)))
            .unwrap_or_else(new_plan_mode_state);
        Self {
            plan_mode: state,
            plan_manager: Some(plan_manager),
        }
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
            "properties": {
                "plan_content": {
                    "type": "string",
                    "description": "Optional plan content to save before exiting plan mode"
                },
                "approved": {
                    "type": "boolean",
                    "description": "Whether the plan was approved by the user (default: false)"
                }
            }
        })
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        // Optionally set plan content via PlanManager before exiting.
        let plan_id = if let Some(ref manager) = self.plan_manager {
            let plan_content = input.get("plan_content").and_then(|v| v.as_str());
            let approved = input
                .get("approved")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            if let Some(content) = plan_content {
                if !content.is_empty() {
                    let id = manager.set_plan(content)?;
                    if approved {
                        manager.approve_plan()?;
                    }
                    Some(id)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        {
            let mut state = self.plan_mode.write().map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to acquire plan mode lock: {e}"))
            })?;
            *state = false;
        }

        if let Some(ref manager) = self.plan_manager {
            manager.exit_plan_mode()?;
        }

        let content = match plan_id {
            Some(ref id) => format!(
                "Exited plan mode. Plan saved with ID: {id}"
            ),
            None => "Exited plan mode.".to_string(),
        };

        Ok(ToolOutput {
            content,
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("plan_mode_active".to_string(), json!(false));
                if let Some(ref id) = plan_id {
                    map.insert("plan_id".to_string(), json!(id));
                }
                map
            },
        })
    }

    fn category(&self) -> &str {
        "mode"
    }

    fn is_read_only(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// GetPlanStatusTool
// ---------------------------------------------------------------------------

/// Query the current plan mode state and plan content.
pub struct GetPlanStatusTool {
    plan_manager: PlanManager,
}

impl GetPlanStatusTool {
    /// Create a new `GetPlanStatusTool` backed by the given `PlanManager`.
    pub fn new(plan_manager: PlanManager) -> Self {
        Self { plan_manager }
    }
}

#[async_trait]
impl Tool for GetPlanStatusTool {
    fn name(&self) -> &str {
        "get_plan_status"
    }

    fn description(&self) -> &str {
        "Get the current plan mode status, including active state and any plan content."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _input: serde_json::Value) -> ToolResult<ToolOutput> {
        let active = self.plan_manager.is_active()?;
        let plan = self.plan_manager.get_plan()?;
        let history = self.plan_manager.plan_history()?;

        let mut metadata = HashMap::new();
        metadata.insert("plan_mode_active".to_string(), json!(active));

        match &plan {
            Some(p) => {
                metadata.insert("plan_id".to_string(), json!(p.id));
                metadata.insert("plan_approved".to_string(), json!(p.approved));
                metadata.insert("plan_created_at".to_string(), json!(p.created_at.to_rfc3339()));
                if let Some(approved_at) = &p.approved_at {
                    metadata.insert(
                        "plan_approved_at".to_string(),
                        json!(approved_at.to_rfc3339()),
                    );
                }
                metadata.insert("plan_history_count".to_string(), json!(history.len()));

                let status_label = if p.approved { "approved" } else { "pending" };
                let content = format!(
                    "Plan mode is active.\n\
                     Current plan [{}]:\n\
                     ID: {}\n\
                     Status: {}\n\
                     Created: {}\n\
                     \n\
                     {}\n\
                     \n\
                     History contains {} previous plan(s).",
                    status_label,
                    p.id,
                    status_label,
                    p.created_at.to_rfc3339(),
                    p.content,
                    history.len()
                );

                Ok(ToolOutput {
                    content,
                    is_error: false,
                    metadata,
                })
            }
            None => {
                let content = if active {
                    "Plan mode is active. No plan has been created yet.".to_string()
                } else {
                    "Plan mode is not active.".to_string()
                };
                Ok(ToolOutput {
                    content,
                    is_error: false,
                    metadata,
                })
            }
        }
    }

    fn category(&self) -> &str {
        "mode"
    }

    fn is_read_only(&self) -> bool {
        true
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── Legacy compatibility tests ─────────────────────────────────────────

    #[test]
    fn test_new_plan_mode_state() {
        let state = new_plan_mode_state();
        assert!(!(*state.read().unwrap()));
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
        assert!(properties.contains_key("reason"));
    }

    #[test]
    fn test_exit_plan_mode_input_schema() {
        let state = new_plan_mode_state();
        let tool = ExitPlanModeTool::new(state);
        let schema = tool.input_schema();

        assert_eq!(schema.get("type").unwrap().as_str().unwrap(), "object");
        let properties = schema.get("properties").unwrap().as_object().unwrap();
        assert!(properties.contains_key("plan_content"));
        assert!(properties.contains_key("approved"));
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
        assert!(output.metadata.get("plan_mode_active").unwrap().as_bool().unwrap());
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
        assert!(!output.metadata.get("plan_mode_active").unwrap().as_bool().unwrap());
    }

    #[tokio::test]
    async fn test_enter_exit_cycle() {
        let state = new_plan_mode_state();
        let enter = EnterPlanModeTool::new(state.clone());
        let exit = ExitPlanModeTool::new(state.clone());

        assert!(!is_plan_mode_active(&state).unwrap());

        enter.execute(json!({})).await.unwrap();
        assert!(is_plan_mode_active(&state).unwrap());

        exit.execute(json!({})).await.unwrap();
        assert!(!is_plan_mode_active(&state).unwrap());

        enter.execute(json!({})).await.unwrap();
        assert!(is_plan_mode_active(&state).unwrap());
    }

    #[tokio::test]
    async fn test_shared_state_across_tools() {
        let state = new_plan_mode_state();
        let enter = EnterPlanModeTool::new(state.clone());
        let exit = ExitPlanModeTool::new(state.clone());

        enter.execute(json!({})).await.unwrap();
        assert!(is_plan_mode_active(&state).unwrap());

        exit.execute(json!({})).await.unwrap();
        assert!(!is_plan_mode_active(&state).unwrap());
    }

    #[tokio::test]
    async fn test_enter_plan_mode_idempotent() {
        let state = new_plan_mode_state();
        let tool = EnterPlanModeTool::new(state.clone());

        tool.execute(json!({})).await.unwrap();
        tool.execute(json!({})).await.unwrap();

        assert!(is_plan_mode_active(&state).unwrap());
    }

    #[tokio::test]
    async fn test_exit_plan_mode_idempotent() {
        let state = new_plan_mode_state();
        let tool = ExitPlanModeTool::new(state.clone());

        tool.execute(json!({})).await.unwrap();
        tool.execute(json!({})).await.unwrap();

        assert!(!is_plan_mode_active(&state).unwrap());
    }

    #[tokio::test]
    async fn test_enter_plan_mode_accepts_any_input() {
        let state = new_plan_mode_state();
        let tool = EnterPlanModeTool::new(state.clone());

        let result = tool.execute(json!({"reason": "planning a feature"})).await;
        assert!(result.is_ok());
        assert!(is_plan_mode_active(&state).unwrap());
    }

    #[tokio::test]
    async fn test_enter_plan_mode_with_reason() {
        let state = new_plan_mode_state();
        let tool = EnterPlanModeTool::new(state.clone());

        let result = tool
            .execute(json!({"reason": "Analyzing architecture before refactoring"}))
            .await
            .unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("Analyzing architecture before refactoring"));
        assert_eq!(
            result.metadata.get("reason").unwrap().as_str().unwrap(),
            "Analyzing architecture before refactoring"
        );
    }

    // ── PlanManager lifecycle tests ────────────────────────────────────────

    #[test]
    fn test_plan_manager_new() {
        let manager = PlanManager::new();
        assert!(!manager.is_active().unwrap());
        assert!(manager.get_plan().unwrap().is_none());
        assert!(manager.plan_history().unwrap().is_empty());
    }

    #[test]
    fn test_plan_manager_enter_exit() {
        let manager = PlanManager::new();

        manager.enter_plan_mode().unwrap();
        assert!(manager.is_active().unwrap());

        manager.exit_plan_mode().unwrap();
        assert!(!manager.is_active().unwrap());
    }

    #[test]
    fn test_plan_manager_set_plan() {
        let manager = PlanManager::new();
        let id = manager.set_plan("Refactor authentication module").unwrap();

        assert!(!id.is_empty());
        let plan = manager.get_plan().unwrap().unwrap();
        assert_eq!(plan.id, id);
        assert_eq!(plan.content, "Refactor authentication module");
        assert!(!plan.approved);
        assert!(plan.approved_at.is_none());
    }

    #[test]
    fn test_plan_manager_approve_plan() {
        let manager = PlanManager::new();
        manager.set_plan("Some plan").unwrap();

        let approved = manager.approve_plan().unwrap();
        assert!(approved.approved);
        assert!(approved.approved_at.is_some());

        // Verify the stored plan is also approved.
        let plan = manager.get_plan().unwrap().unwrap();
        assert!(plan.approved);
    }

    #[test]
    fn test_plan_manager_approve_no_plan_errors() {
        let manager = PlanManager::new();
        let result = manager.approve_plan();
        assert!(result.is_err());
    }

    #[test]
    fn test_plan_manager_reject_plan() {
        let manager = PlanManager::new();
        let id = manager.set_plan("Plan to reject").unwrap();

        let rejected = manager.reject_plan().unwrap();
        assert_eq!(rejected.id, id);
        assert!(!rejected.approved);

        // Current plan should be cleared.
        assert!(manager.get_plan().unwrap().is_none());

        // Rejected plan should appear in history.
        let history = manager.plan_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, id);
    }

    #[test]
    fn test_plan_manager_reject_no_plan_errors() {
        let manager = PlanManager::new();
        let result = manager.reject_plan();
        assert!(result.is_err());
    }

    #[test]
    fn test_plan_manager_set_plan_replaces_previous() {
        let manager = PlanManager::new();

        let id1 = manager.set_plan("First plan").unwrap();
        let id2 = manager.set_plan("Second plan").unwrap();

        assert_ne!(id1, id2);
        let plan = manager.get_plan().unwrap().unwrap();
        assert_eq!(plan.content, "Second plan");

        // First plan should be in history.
        let history = manager.plan_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, id1);
        assert_eq!(history[0].content, "First plan");
    }

    #[test]
    fn test_plan_manager_full_lifecycle() {
        let manager = PlanManager::new();

        // Enter plan mode
        manager.enter_plan_mode().unwrap();
        assert!(manager.is_active().unwrap());

        // Set a plan
        let id = manager.set_plan("# Architecture Plan\n\n1. Step one\n2. Step two").unwrap();
        let plan = manager.get_plan().unwrap().unwrap();
        assert_eq!(plan.id, id);
        assert!(!plan.approved);

        // Approve the plan
        let approved = manager.approve_plan().unwrap();
        assert!(approved.approved);
        assert!(approved.approved_at.is_some());

        // Get prompt content
        let prompt = manager.get_plan_content_for_prompt().unwrap();
        assert!(prompt.contains("APPROVED"));
        assert!(prompt.contains("Architecture Plan"));

        // Exit plan mode
        manager.exit_plan_mode().unwrap();
        assert!(!manager.is_active().unwrap());

        // Prompt should say not active.
        let prompt = manager.get_plan_content_for_prompt().unwrap();
        assert!(prompt.contains("not active"));
    }

    #[test]
    fn test_plan_manager_history_multiple_entries() {
        let manager = PlanManager::new();

        manager.set_plan("Plan A").unwrap();
        manager.set_plan("Plan B").unwrap();
        manager.set_plan("Plan C").unwrap();

        let history = manager.plan_history().unwrap();
        assert_eq!(history.len(), 2); // A and B pushed to history, C is current

        let current = manager.get_plan().unwrap().unwrap();
        assert_eq!(current.content, "Plan C");
    }

    #[test]
    fn test_plan_manager_get_plan_content_not_active() {
        let manager = PlanManager::new();
        let content = manager.get_plan_content_for_prompt().unwrap();
        assert_eq!(content, "Plan mode is not active.");
    }

    #[test]
    fn test_plan_manager_get_plan_content_active_no_plan() {
        let manager = PlanManager::new();
        manager.enter_plan_mode().unwrap();
        let content = manager.get_plan_content_for_prompt().unwrap();
        assert!(content.contains("No plan has been created"));
    }

    #[test]
    fn test_plan_manager_get_plan_content_with_plan() {
        let manager = PlanManager::new();
        manager.enter_plan_mode().unwrap();
        manager.set_plan("My detailed plan").unwrap();

        let content = manager.get_plan_content_for_prompt().unwrap();
        assert!(content.contains("PENDING APPROVAL"));
        assert!(content.contains("My detailed plan"));
    }

    // ── Plan file persistence tests ────────────────────────────────────────

    #[test]
    fn test_save_plan_to_file() {
        let tmp = TempDir::new().unwrap();
        let manager = PlanManager::new();
        let id = manager.set_plan("# My Plan\n\nStep 1: Do things").unwrap();

        let path = manager.save_plan_to_file(tmp.path()).unwrap();

        assert!(path.exists());
        assert!(path.to_str().unwrap().contains(&id));
        assert!(path.to_str().unwrap().ends_with(".md"));

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.starts_with("# Plan: My Plan"));
        assert!(content.contains("Status: pending"));
        assert!(content.contains("Step 1: Do things"));
    }

    #[test]
    fn test_save_approved_plan_to_file() {
        let tmp = TempDir::new().unwrap();
        let manager = PlanManager::new();
        manager.set_plan("# Approved Plan\n\nDo the thing").unwrap();
        manager.approve_plan().unwrap();

        let path = manager.save_plan_to_file(tmp.path()).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("Status: approved"));
    }

    #[test]
    fn test_save_plan_no_current_plan_errors() {
        let tmp = TempDir::new().unwrap();
        let manager = PlanManager::new();
        let result = manager.save_plan_to_file(tmp.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_load_plan_from_file() {
        let tmp = TempDir::new().unwrap();
        let manager = PlanManager::new();

        // Write a plan file manually.
        let plans_dir = tmp.path().join(".shannon").join("plans");
        std::fs::create_dir_all(&plans_dir).unwrap();
        let file_path = plans_dir.join("test-plan-id.md");
        let content = "# Plan: Test Plan\nCreated: 2025-01-15T10:30:00+00:00\nStatus: pending\n\nMy plan content here.";
        std::fs::write(&file_path, content).unwrap();

        let plan = manager.load_plan_from_file(&file_path).unwrap();
        assert_eq!(plan.id, "test-plan-id");
        assert_eq!(plan.content, "My plan content here.");
        assert!(!plan.approved);
        assert_eq!(plan.approved_at, None);
    }

    #[test]
    fn test_load_approved_plan_from_file() {
        let tmp = TempDir::new().unwrap();
        let manager = PlanManager::new();

        let plans_dir = tmp.path().join(".shannon").join("plans");
        std::fs::create_dir_all(&plans_dir).unwrap();
        let file_path = plans_dir.join("approved-plan.md");
        let content = "# Plan: Approved\nCreated: 2025-01-15T10:30:00+00:00\nStatus: approved\n\nApproved content.";
        std::fs::write(&file_path, content).unwrap();

        let plan = manager.load_plan_from_file(&file_path).unwrap();
        assert!(plan.approved);
        assert!(plan.approved_at.is_some());
    }

    #[test]
    fn test_save_load_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let manager = PlanManager::new();

        manager.set_plan("# Roundtrip Plan\n\n- Step 1\n- Step 2").unwrap();

        let path = manager.save_plan_to_file(tmp.path()).unwrap();
        let loaded = manager.load_plan_from_file(&path).unwrap();

        let original = manager.get_plan().unwrap().unwrap();
        assert_eq!(loaded.id, original.id);
        // Content comparison: the saved file includes the "# Plan: Roundtrip Plan"
        // header line as part of the body, so check the plan content is in there.
        assert!(loaded.content.contains("Step 1"));
        assert!(loaded.content.contains("Step 2"));
        assert_eq!(loaded.approved, original.approved);
    }

    #[test]
    fn test_list_plans_empty() {
        let tmp = TempDir::new().unwrap();
        let manager = PlanManager::new();

        let plans = manager.list_plans(tmp.path()).unwrap();
        assert!(plans.is_empty());
    }

    #[test]
    fn test_list_plans_multiple() {
        let tmp = TempDir::new().unwrap();
        let manager = PlanManager::new();

        let plans_dir = tmp.path().join(".shannon").join("plans");
        std::fs::create_dir_all(&plans_dir).unwrap();

        // Create two plan files with different timestamps.
        std::fs::write(
            plans_dir.join("plan-older.md"),
            "# Plan: Older\nCreated: 2025-01-01T00:00:00+00:00\nStatus: pending\n\nOlder plan.",
        )
        .unwrap();
        std::fs::write(
            plans_dir.join("plan-newer.md"),
            "# Plan: Newer\nCreated: 2025-06-01T00:00:00+00:00\nStatus: approved\n\nNewer plan.",
        )
        .unwrap();

        let plans = manager.list_plans(tmp.path()).unwrap();
        assert_eq!(plans.len(), 2);

        // Sorted newest first.
        assert_eq!(plans[0].1.content, "Newer plan.");
        assert_eq!(plans[1].1.content, "Older plan.");
    }

    #[test]
    fn test_list_plans_ignores_non_markdown() {
        let tmp = TempDir::new().unwrap();
        let manager = PlanManager::new();

        let plans_dir = tmp.path().join(".shannon").join("plans");
        std::fs::create_dir_all(&plans_dir).unwrap();

        std::fs::write(
            plans_dir.join("plan.md"),
            "# Plan: Valid\nCreated: 2025-01-01T00:00:00+00:00\nStatus: pending\n\nValid.",
        )
        .unwrap();
        std::fs::write(plans_dir.join("notes.txt"), "Not a plan").unwrap();

        let plans = manager.list_plans(tmp.path()).unwrap();
        assert_eq!(plans.len(), 1);
    }

    #[test]
    fn test_list_plans_no_directory() {
        let tmp = TempDir::new().unwrap();
        let manager = PlanManager::new();

        // Directory doesn't exist yet — should return empty, not error.
        let plans = manager.list_plans(tmp.path().join("nonexistent").as_path()).unwrap();
        assert!(plans.is_empty());
    }

    // ── Thread safety tests ────────────────────────────────────────────────

    #[test]
    fn test_plan_manager_thread_safety() {
        use std::thread;

        let manager = PlanManager::new();
        let manager_clone = manager.clone();

        // Write from main thread.
        manager.enter_plan_mode().unwrap();
        manager.set_plan("Main thread plan").unwrap();

        // Read from another thread.
        let handle = thread::spawn(move || {
            let active = manager_clone.is_active().unwrap();
            let plan = manager_clone.get_plan().unwrap();
            (active, plan.map(|p| p.content))
        });

        let (active, content) = handle.join().unwrap();
        assert!(active);
        assert_eq!(content, Some("Main thread plan".to_string()));
    }

    #[test]
    fn test_plan_manager_concurrent_set_plan() {
        use std::thread;

        let manager = PlanManager::new();
        manager.enter_plan_mode().unwrap();

        let mut handles = Vec::new();
        for i in 0..5 {
            let mgr = manager.clone();
            handles.push(thread::spawn(move || {
                mgr.set_plan(&format!("Plan from thread {i}"))
            }));
        }

        // All set_plan calls should succeed.
        for handle in handles {
            assert!(handle.join().unwrap().is_ok());
        }

        // There should be a current plan and history.
        let current = manager.get_plan().unwrap();
        assert!(current.is_some());

        let history = manager.plan_history().unwrap();
        // 5 plans set, 1 is current, so 4 in history.
        assert_eq!(history.len(), 4);
    }

    // ── PlanManager-backed tool tests ──────────────────────────────────────

    #[tokio::test]
    async fn test_enter_plan_mode_with_manager() {
        let manager = PlanManager::new();
        let tool = EnterPlanModeTool::with_manager(manager.clone());

        let result = tool.execute(json!({"reason": "Testing"})).await.unwrap();
        assert!(!result.is_error);
        assert!(manager.is_active().unwrap());
        assert!(result.content.contains("Testing"));
    }

    #[tokio::test]
    async fn test_exit_plan_mode_saves_plan_via_manager() {
        let manager = PlanManager::new();
        manager.enter_plan_mode().unwrap();

        let tool = ExitPlanModeTool::with_manager(manager.clone());
        let result = tool
            .execute(json!({
                "plan_content": "My final plan",
                "approved": true
            }))
            .await
            .unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("Plan saved with ID"));
        assert!(!manager.is_active().unwrap());

        // Plan was saved and approved before exit.
        let history = manager.plan_history().unwrap();
        assert_eq!(history.len(), 0); // approved plan stays as current, not history

        // Current plan should still be available (even though mode exited).
        let plan = manager.get_plan().unwrap().unwrap();
        assert!(plan.approved);
        assert_eq!(plan.content, "My final plan");
    }

    #[tokio::test]
    async fn test_get_plan_status_tool() {
        let manager = PlanManager::new();
        let tool = GetPlanStatusTool::new(manager.clone());

        // Initially not active.
        let result = tool.execute(json!({})).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("not active"));
        assert!(
            !result.metadata.get("plan_mode_active").unwrap().as_bool().unwrap()
        );

        // Activate and set a plan.
        manager.enter_plan_mode().unwrap();
        manager.set_plan("Test plan content").unwrap();

        let result = tool.execute(json!({})).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("Test plan content"));
        assert!(result.content.contains("pending"));
        assert!(
            result.metadata.get("plan_mode_active").unwrap().as_bool().unwrap()
        );
        assert!(result.metadata.contains_key("plan_id"));
    }

    #[tokio::test]
    async fn test_get_plan_status_tool_name() {
        let manager = PlanManager::new();
        let tool = GetPlanStatusTool::new(manager);
        assert_eq!(tool.name(), "get_plan_status");
    }

    #[test]
    fn test_get_plan_status_tool_is_read_only() {
        let manager = PlanManager::new();
        let tool = GetPlanStatusTool::new(manager);
        assert!(tool.is_read_only());
    }

    #[test]
    fn test_enter_plan_mode_tool_is_read_only() {
        let state = new_plan_mode_state();
        let tool = EnterPlanModeTool::new(state);
        assert!(tool.is_read_only());
    }

    #[test]
    fn test_exit_plan_mode_tool_is_read_only() {
        let state = new_plan_mode_state();
        let tool = ExitPlanModeTool::new(state);
        assert!(tool.is_read_only());
    }
}
