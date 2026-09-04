//! `goal_get` / `goal_update` tools (P2.5 — Codex-style goal contract).
//!
//! These tools let the model read the current goal state and mark it as
//! completed or blocked via structured tool calls, in addition to the
//! marker-on-last-line contract. Per design R9/R14 in
//! `docs/plans/2026-09-04-goal-phase2-guardrails-plan.md`, the model can
//! only set `complete` or `blocked`:
//!
//!   * `update_goal` is only allowed to set `complete` or `blocked`. The
//!     pause path stays user-only (`/goal pause`).
//!   * `blocked` requires a non-empty `reason`.
//!
//! The tools do not own goal state — they delegate to a `GoalStateAccess`
//! trait object the caller (the REPL) supplies. This keeps `shannon-tools`
//! dependency-free of `shannon-ui` (no cycle).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use shannon_tool_interface::{Tool, ToolOutput, ToolResult};

use crate::ToolRegistry;

/// Snapshot of the active goal the model can observe. Mirrors `GoalState`
/// but lives here so this crate stays independent of `shannon-ui`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalSnapshot {
    pub objective: String,
    pub status: String, // "active" | "paused" | "complete"
    pub iterations: usize,
    pub max_iterations: usize,
    pub max_budget_usd: Option<f64>,
}

/// Outcome of `update_goal`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GoalUpdateOutcome {
    /// Goal transitioned to Complete (final).
    Completed,
    /// Goal transitioned to Paused (blocked or any other model-reported
    /// reason). Always user-clearable with `/goal clear`.
    Paused(String),
    /// Tool refused: model attempted a forbidden transition (e.g. trying
    /// to `pause` or `update max_budget_usd`). The state is unchanged.
    Rejected(String),
}

/// State accessor the REPL injects at tool-registration time. Implementations
/// must be cheap (a function pointer or a single mutex acquisition); the
/// tools are called inside the agent loop, on every model reply.
pub trait GoalStateAccess: Send + Sync {
    fn snapshot(&self) -> Option<GoalSnapshot>;
    /// Returns the outcome; `None` indicates the goal is no longer present
    /// (e.g. user cleared it while a tool call was in flight).
    fn apply_update(&self, outcome: GoalUpdateOutcome) -> Option<()>;
}

/// `goal_get` — returns the current goal snapshot or a "no goal set" stub.
pub struct GoalGetTool {
    pub access: std::sync::Arc<dyn GoalStateAccess>,
}

#[async_trait]
impl Tool for GoalGetTool {
    fn name(&self) -> &str {
        "goal_get"
    }
    fn description(&self) -> &str {
        "Return the current session goal (objective, status, iteration counter, ".into()
    }
    fn input_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    async fn execute(&self, _input: Value) -> ToolResult<ToolOutput> {
        let snap = self.access.snapshot();
        let body = match snap {
            Some(s) => serde_json::to_string(&s).unwrap_or_default(),
            None => "{\"status\":\"none\",\"objective\":\"\"}".to_string(),
        };
        Ok(ToolOutput::success(body))
    }
    fn category(&self) -> &str {
        "goal"
    }
}

/// `goal_update` — model may only set complete or blocked (per Codex spec).
pub struct GoalUpdateTool {
    pub access: std::sync::Arc<dyn GoalStateAccess>,
}

#[async_trait]
impl Tool for GoalUpdateTool {
    fn name(&self) -> &str {
        "goal_update"
    }
    fn description(&self) -> &str {
        "Mark the current session goal as complete or blocked. \
         Allowed transitions: active/paused -> complete (with status=complete); \
         active/paused -> paused (with status=blocked, reason required). \
         The goal stays paused when blocked; /goal resume re-arms it."
            .into()
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "status": { "type": "string", "enum": ["complete", "blocked"] },
                "reason": { "type": "string" }
            },
            "required": ["status"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        #[derive(Deserialize)]
        struct Args {
            status: String,
            reason: Option<String>,
        }
        let args: Args = match serde_json::from_value(input) {
            Ok(a) => a,
            Err(e) => {
                return Ok(ToolOutput::success(format!("Invalid input: {e}")));
            }
        };
        let outcome = match args.status.as_str() {
            "complete" => GoalUpdateOutcome::Completed,
            "blocked" => {
                let reason = args
                    .reason
                    .as_deref()
                    .map(str::trim)
                    .filter(|r| !r.is_empty())
                    .map(str::to_string);
                match reason {
                    Some(r) => GoalUpdateOutcome::Paused(r),
                    None => {
                        return Ok(ToolOutput::success(
                            "Blocked requires a non-empty `reason`.".to_string(),
                        ));
                    }
                }
            }
            other => {
                return Ok(ToolOutput::success(format!(
                    "Rejected: '{other}' is not a permitted transition. Use status=\"complete\" or status=\"blocked\" (with reason)."
                )));
            }
        };
        match self.access.apply_update(outcome) {
            Some(()) => Ok(ToolOutput::success(format!("ok: {}", args.status))),
            None => Ok(ToolOutput::success(
                "Goal is no longer active (cleared by user).".to_string(),
            )),
        }
    }
    fn category(&self) -> &str {
        "goal"
    }
}

/// Register both tools. The REPL must call this at startup, after
/// `register_default_tools`, supplying the goal-state accessor it owns.
pub fn register_goal_tools(
    registry: &mut ToolRegistry,
    access: std::sync::Arc<dyn GoalStateAccess>,
) -> Result<(), crate::ToolError> {
    registry.register(Box::new(GoalGetTool {
        access: access.clone(),
    }))?;
    registry.register(Box::new(GoalUpdateTool { access }))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Minimal in-memory accessor the tool tests can drive directly.
    struct InMemory {
        state: Mutex<Option<GoalSnapshot>>,
        applied: Mutex<Vec<GoalUpdateOutcome>>,
    }
    impl GoalStateAccess for InMemory {
        fn snapshot(&self) -> Option<GoalSnapshot> {
            self.state.lock().unwrap().clone()
        }
        fn apply_update(&self, outcome: GoalUpdateOutcome) -> Option<()> {
            self.applied.lock().unwrap().push(outcome.clone());
            let mut g = self.state.lock().unwrap();
            let s = g.as_mut()?;
            match outcome {
                GoalUpdateOutcome::Completed => s.status = "complete".into(),
                GoalUpdateOutcome::Paused(_) => s.status = "paused".into(),
                GoalUpdateOutcome::Rejected(_) => {}
            }
            Some(())
        }
    }

    #[test]
    fn goal_get_returns_snapshot_when_active() {
        let mem = std::sync::Arc::new(InMemory {
            state: Mutex::new(Some(GoalSnapshot {
                objective: "ship it".into(),
                status: "active".into(),
                iterations: 1,
                max_iterations: 25,
                max_budget_usd: None,
            })),
            applied: Mutex::new(vec![]),
        });
        let tool = GoalGetTool { access: mem };
        let out = futures::executor::block_on(tool.execute(json!({}))).unwrap();
        assert!(out.content.contains("ship it"));
        assert!(out.content.contains("active"));
    }

    #[test]
    fn goal_get_handles_no_goal() {
        let mem = std::sync::Arc::new(InMemory {
            state: Mutex::new(None),
            applied: Mutex::new(vec![]),
        });
        let tool = GoalGetTool { access: mem };
        let out = futures::executor::block_on(tool.execute(json!({}))).unwrap();
        assert_eq!(out.content, "{\"status\":\"none\",\"objective\":\"\"}");
    }

    #[test]
    fn goal_update_complete_records_outcome() {
        let mem = std::sync::Arc::new(InMemory {
            state: Mutex::new(Some(GoalSnapshot {
                objective: "x".into(),
                status: "active".into(),
                iterations: 0,
                max_iterations: 25,
                max_budget_usd: None,
            })),
            applied: Mutex::new(vec![]),
        });
        let tool = GoalUpdateTool {
            access: mem.clone(),
        };
        let out = futures::executor::block_on(tool.execute(json!({"status": "complete"}))).unwrap();
        assert!(out.content.contains("ok"));
        assert_eq!(mem.snapshot().unwrap().status, "complete");
    }

    #[test]
    fn goal_update_blocked_requires_reason() {
        let mem = std::sync::Arc::new(InMemory {
            state: Mutex::new(Some(GoalSnapshot {
                objective: "x".into(),
                status: "active".into(),
                iterations: 0,
                max_iterations: 25,
                max_budget_usd: None,
            })),
            applied: Mutex::new(vec![]),
        });
        let tool = GoalUpdateTool {
            access: mem.clone(),
        };
        let out = futures::executor::block_on(tool.execute(json!({"status": "blocked"}))).unwrap();
        assert!(out.content.contains("requires"), "got: {}", out.content);
        assert_eq!(mem.applied.lock().unwrap().len(), 0, "no transition");

        // With reason: accepted.
        let out = futures::executor::block_on(
            tool.execute(json!({"status": "blocked", "reason": "no creds"})),
        )
        .unwrap();
        assert!(out.content.contains("ok"));
        assert_eq!(mem.snapshot().unwrap().status, "paused");
    }

    #[test]
    fn goal_update_rejects_unknown_status() {
        let mem = std::sync::Arc::new(InMemory {
            state: Mutex::new(Some(GoalSnapshot {
                objective: "x".into(),
                status: "active".into(),
                iterations: 0,
                max_iterations: 25,
                max_budget_usd: None,
            })),
            applied: Mutex::new(vec![]),
        });
        let tool = GoalUpdateTool {
            access: mem.clone(),
        };
        let out = futures::executor::block_on(tool.execute(json!({"status": "pause"}))).unwrap();
        assert!(out.content.contains("Rejected"));
        assert_eq!(
            mem.snapshot().unwrap().status,
            "active",
            "state must not transition on reject"
        );
    }

    #[test]
    fn goal_update_handles_removed_goal() {
        let mem = std::sync::Arc::new(InMemory {
            state: Mutex::new(None),
            applied: Mutex::new(vec![]),
        });
        let tool = GoalUpdateTool {
            access: mem.clone(),
        };
        let out = futures::executor::block_on(tool.execute(json!({"status": "complete"}))).unwrap();
        assert!(out.content.contains("no longer active"));
    }
}
