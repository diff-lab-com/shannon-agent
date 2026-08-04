//! Linear MCP tool implementations.
//!
//! Five tools backed by [`LinearClient`]:
//!
//! 1. `linear_list_issues`   — paginated issue list (read).
//! 2. `linear_get_issue`     — fetch one issue (read).
//! 3. `linear_create_issue`  — create issue (write).
//! 4. `linear_update_status` — transition issue by state id (write).
//! 5. `linear_list_teams`    — list teams + workflow states (read).
//!
//! `linear_update_status` requires a `state_id` UUID — callers must
//! resolve from team metadata via `linear_list_teams` first. The docs
//! in `docs/integrations/linear.md` call out this discoverability step.
//!
//! ## Permission gating
//!
//! Write tools require the host to call `tools/grant` (server-side
//! check in `server::SessionGrants`). The previous self-attested
//! `args.permission` gate is gone; tool implementations no longer
//! re-attest write scope.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use thiserror::Error;

use shannon_mcp::McpError;

use crate::linear::api::{ApiError, LinearClient};

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("missing required argument: {0}")]
    MissingArg(&'static str),
    #[error("invalid arguments: {0}")]
    InvalidArgs(String),
    #[error("upstream API error: {0}")]
    Api(String),
}

impl From<ApiError> for ToolError {
    fn from(e: ApiError) -> Self {
        Self::Api(e.to_string())
    }
}

impl From<ToolError> for McpError {
    fn from(e: ToolError) -> Self {
        match e {
            ToolError::MissingArg(_) | ToolError::InvalidArgs(_) => {
                McpError::InvalidRequest(e.to_string())
            }
            ToolError::Api(_) => McpError::Server(e.to_string()),
        }
    }
}

impl From<ApiError> for McpError {
    fn from(e: ApiError) -> Self {
        ToolError::from(e).into()
    }
}

#[async_trait]
pub trait McpTool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn input_schema(&self) -> Value;
    fn is_read_only(&self) -> bool {
        false
    }
    fn is_destructive(&self) -> bool {
        false
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn is_open_world(&self) -> bool {
        true
    }
    fn required_permission(&self) -> &'static str;
    async fn execute(&self, args: Value) -> Result<Value, McpError>;
}

pub type SharedClient = Arc<LinearClient>;

fn require_string<'a>(args: &'a Value, key: &'static str) -> Result<&'a str, ToolError> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or(ToolError::MissingArg(key))
}
fn optional_string<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}
fn optional_u32(args: &Value, key: &str) -> Option<u32> {
    args.get(key)
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
}
fn optional_f64(args: &Value, key: &str) -> Option<f64> {
    args.get(key).and_then(Value::as_f64)
}
fn optional_string_array(args: &Value, key: &str) -> Option<Vec<String>> {
    args.get(key).and_then(Value::as_array).map(|arr| {
        arr.iter()
            .filter_map(Value::as_str)
            .map(|s| s.to_string())
            .collect()
    })
}
fn optional_filter(args: &Value, key: &str) -> Option<Value> {
    args.get(key).cloned()
}

/// UX hint only — the real gate is `server::SessionGrants`. The
/// `permission` field is stripped before reaching the tool.
/// Kept for symmetry with Slack/Jira tools.
#[allow(dead_code)] // KEEP: parity with slack::tools::require_write and jira::tools::require_write; UX-only helper retained for future re-attestation hooks
fn require_write(_args: &Value) -> Result<(), ToolError> {
    Ok(())
}

pub fn all_tools(client: SharedClient) -> Vec<Box<dyn McpTool>> {
    vec![
        Box::new(ListIssuesTool {
            client: client.clone(),
        }),
        Box::new(GetIssueTool {
            client: client.clone(),
        }),
        Box::new(CreateIssueTool {
            client: client.clone(),
        }),
        Box::new(UpdateStatusTool {
            client: client.clone(),
        }),
        Box::new(ListTeamsTool { client }),
    ]
}

pub fn all_tools_unauth() -> Vec<Box<dyn McpTool>> {
    let http = reqwest::Client::builder()
        .user_agent("shannon-mcp-saas/0.7")
        .build()
        .expect("build reqwest client");
    all_tools(Arc::new(LinearClient::new(http, None)))
}

/// Erase the per-SaaS `McpTool` trait into `server::ServerTool`.
pub fn as_server_tool(tools: Vec<Box<dyn McpTool>>) -> Vec<Box<dyn crate::server::ServerTool>> {
    tools
        .into_iter()
        .map(|t| Box::new(LinearServerTool(t)) as Box<dyn crate::server::ServerTool>)
        .collect()
}

/// Adapter from `Box<dyn McpTool>` to `Box<dyn ServerTool>`.
pub struct LinearServerTool(pub Box<dyn McpTool>);

#[async_trait]
impl crate::server::ServerTool for LinearServerTool {
    fn name(&self) -> &'static str {
        self.0.name()
    }
    fn description(&self) -> &'static str {
        self.0.description()
    }
    fn input_schema(&self) -> Value {
        self.0.input_schema()
    }
    fn is_read_only(&self) -> bool {
        self.0.is_read_only()
    }
    fn is_destructive(&self) -> bool {
        self.0.is_destructive()
    }
    fn is_idempotent(&self) -> bool {
        self.0.is_idempotent()
    }
    fn is_open_world(&self) -> bool {
        self.0.is_open_world()
    }
    fn required_permission(&self) -> &'static str {
        self.0.required_permission()
    }
    async fn execute(&self, args: Value) -> Result<Value, McpError> {
        self.0.execute(args).await
    }
}

// ---------------------------------------------------------------------------
// ListIssuesTool (read)
// ---------------------------------------------------------------------------

pub struct ListIssuesTool {
    pub(crate) client: SharedClient,
}

#[async_trait]
impl McpTool for ListIssuesTool {
    fn name(&self) -> &'static str {
        "linear_list_issues"
    }
    fn description(&self) -> &'static str {
        "List Linear issues with optional GraphQL filter and pagination."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type":"object",
            "properties":{
                "filter":{
                    "type":"object",
                    "description":"GraphQL IssueFilter object, e.g. {\"state\":{\"type\":\"inProgress\"},\"team\":{\"id\":{\"eq\":\"TEAM-ID\"}}}",
                },
                "first":{"type":"integer","minimum":1,"maximum":100,"description":"Page size (default 50)"},
                "after":{"type":"string","description":"Pagination cursor returned by previous call"}
            }
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_idempotent(&self) -> bool {
        true
    }
    fn required_permission(&self) -> &'static str {
        "read"
    }
    async fn execute(&self, args: Value) -> Result<Value, McpError> {
        let filter = optional_filter(&args, "filter");
        let first = optional_u32(&args, "first");
        let after = optional_string(&args, "after");
        let conn = self
            .client
            .list_issues(filter.as_ref(), first, after)
            .await?;
        Ok(json!({
            "issues": conn.nodes,
            "page_info": conn.page_info,
        }))
    }
}

// ---------------------------------------------------------------------------
// GetIssueTool (read)
// ---------------------------------------------------------------------------

pub struct GetIssueTool {
    pub(crate) client: SharedClient,
}

#[async_trait]
impl McpTool for GetIssueTool {
    fn name(&self) -> &'static str {
        "linear_get_issue"
    }
    fn description(&self) -> &'static str {
        "Fetch a single Linear issue by id or identifier (e.g. 'ENG-123')."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type":"object",
            "properties":{
                "id":{
                    "type":"string",
                    "description":"Issue UUID or human-readable identifier like 'ENG-123'",
                }
            },
            "required":["id"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_idempotent(&self) -> bool {
        true
    }
    fn required_permission(&self) -> &'static str {
        "read"
    }
    async fn execute(&self, args: Value) -> Result<Value, McpError> {
        let id = require_string(&args, "id")?;
        let issue = self.client.get_issue(id).await?;
        Ok(json!({ "issue": issue }))
    }
}

// ---------------------------------------------------------------------------
// CreateIssueTool (write)
// ---------------------------------------------------------------------------

pub struct CreateIssueTool {
    pub(crate) client: SharedClient,
}

#[async_trait]
impl McpTool for CreateIssueTool {
    fn name(&self) -> &'static str {
        "linear_create_issue"
    }
    fn description(&self) -> &'static str {
        "Create a Linear issue in the given team. Use linear_list_teams to discover team ids."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type":"object",
            "properties":{
                "title":{"type":"string","description":"One-line title"},
                "team_id":{"type":"string","description":"Team UUID (resolve via linear_list_teams)"},
                "description":{"type":"string","description":"Optional markdown body"},
                "priority":{"type":"number","description":"Linear priority 0..4 (0=No priority, 1=Urgent, 2=High, 3=Medium, 4=Low)"},
                "label_ids":{
                    "type":"array",
                    "items":{"type":"string"},
                    "description":"Optional list of label UUIDs to attach"
                }
            },
            "required":["title","team_id"]
        })
    }
    fn required_permission(&self) -> &'static str {
        "write"
    }
    async fn execute(&self, args: Value) -> Result<Value, McpError> {
        let _ = require_write(&args);
        let title = require_string(&args, "title")?;
        let team_id = require_string(&args, "team_id")?;
        let description = optional_string(&args, "description");
        let priority = optional_f64(&args, "priority");
        let label_ids = optional_string_array(&args, "label_ids");
        let labels_ref = label_ids.as_deref();
        let issue = self
            .client
            .create_issue(title, team_id, description, priority, labels_ref)
            .await?;
        Ok(json!({ "issue": issue, "created": true }))
    }
}

// ---------------------------------------------------------------------------
// UpdateStatusTool (write)
// ---------------------------------------------------------------------------

pub struct UpdateStatusTool {
    pub(crate) client: SharedClient,
}

#[async_trait]
impl McpTool for UpdateStatusTool {
    fn name(&self) -> &'static str {
        "linear_update_status"
    }
    fn description(&self) -> &'static str {
        "Move a Linear issue to a workflow state. Resolve the state id via linear_list_teams first."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type":"object",
            "properties":{
                "issue_id":{"type":"string","description":"Issue UUID"},
                "state_id":{"type":"string","description":"Workflow state UUID (resolve via linear_list_teams)"}
            },
            "required":["issue_id","state_id"]
        })
    }
    fn required_permission(&self) -> &'static str {
        "write"
    }
    async fn execute(&self, args: Value) -> Result<Value, McpError> {
        let _ = require_write(&args);
        let issue_id = require_string(&args, "issue_id")?;
        let state_id = require_string(&args, "state_id")?;
        let issue = self.client.update_status(issue_id, state_id).await?;
        Ok(json!({ "issue": issue, "updated": true }))
    }
}

// ---------------------------------------------------------------------------
// ListTeamsTool (read)
// ---------------------------------------------------------------------------

pub struct ListTeamsTool {
    pub(crate) client: SharedClient,
}

#[async_trait]
impl McpTool for ListTeamsTool {
    fn name(&self) -> &'static str {
        "linear_list_teams"
    }
    fn description(&self) -> &'static str {
        "List Linear teams and their workflow states. Use to resolve state names to ids before linear_update_status."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type":"object",
            "properties":{
                "first":{"type":"integer","minimum":1,"maximum":100,"description":"Max teams to return (default 50)"}
            }
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_idempotent(&self) -> bool {
        true
    }
    fn required_permission(&self) -> &'static str {
        "read"
    }
    async fn execute(&self, args: Value) -> Result<Value, McpError> {
        let first = optional_u32(&args, "first");
        let conn = self.client.list_teams(first).await?;
        Ok(json!({ "teams": conn.nodes }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_tools_returns_five() {
        assert_eq!(all_tools_unauth().len(), 5);
    }

    #[test]
    fn five_tool_names_are_stable() {
        let names: Vec<_> = all_tools_unauth().iter().map(|t| t.name()).collect();
        assert_eq!(
            names,
            vec![
                "linear_list_issues",
                "linear_get_issue",
                "linear_create_issue",
                "linear_update_status",
                "linear_list_teams",
            ]
        );
    }

    #[test]
    fn required_permission_matches_role() {
        let tools = all_tools_unauth();
        // read tools
        assert_eq!(tools[0].required_permission(), "read"); // list_issues
        assert_eq!(tools[1].required_permission(), "read"); // get_issue
        assert_eq!(tools[4].required_permission(), "read"); // list_teams
        // write tools
        assert_eq!(tools[2].required_permission(), "write"); // create_issue
        assert_eq!(tools[3].required_permission(), "write"); // update_status
    }

    #[test]
    fn read_only_hints_match_required_permission() {
        let tools = all_tools_unauth();
        assert!(tools[0].is_read_only()); // list_issues
        assert!(tools[1].is_read_only()); // get_issue
        assert!(!tools[2].is_read_only()); // create_issue
        assert!(!tools[3].is_read_only()); // update_status
        assert!(tools[4].is_read_only()); // list_teams
    }

    #[tokio::test]
    async fn create_issue_requires_title_and_team_id() {
        let tools = all_tools_unauth();
        // missing team_id
        assert!(tools[2].execute(json!({"title":"x"})).await.is_err());
        // missing title
        assert!(tools[2].execute(json!({"team_id":"abc"})).await.is_err());
    }

    #[tokio::test]
    async fn get_issue_requires_id() {
        let tools = all_tools_unauth();
        assert!(tools[1].execute(json!({})).await.is_err());
    }

    #[tokio::test]
    async fn update_status_requires_issue_and_state() {
        let tools = all_tools_unauth();
        assert!(tools[3].execute(json!({"issue_id":"i"})).await.is_err());
        assert!(tools[3].execute(json!({"state_id":"s"})).await.is_err());
    }
}
