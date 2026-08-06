//! Jira MCP tool implementations.
//!
//! Four tools backed by [`JiraClient`]:
//! 1. `jira_search_issues` — JQL search (read-only).
//! 2. `jira_get_issue`     — fetch full issue body by key (read-only).
//! 3. `jira_create_issue`  — create issue (write).
//! 4. `jira_transition`    — move issue along workflow (write).
//!
//! ## Permission gating
//!
//! Write tools require the host to have called `tools/grant { name,
//! scope: "write" }` before the `tools/call` arrives. The previous
//! self-attested `args.permission` check (which an LLM could forge)
//! is gone — that field is stripped at the JSON-RPC boundary in
//! `crate::server::handle_tools_call` and the real capability check
//! lives in [`crate::server::SessionGrants`]. These tool
//! implementations therefore do **not** re-attest write scope.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use thiserror::Error;

use shannon_mcp::McpError;

use crate::jira::api::{ApiError, JiraClient};

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

pub type SharedClient = Arc<JiraClient>;

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

/// UX hint only — the real gate is `server::SessionGrants`. The
/// `permission` field is stripped before reaching the tool, so this is
/// a no-op retained for symmetry with slack.
fn require_write(_args: &Value) -> Result<(), ToolError> {
    Ok(())
}

pub fn all_tools(client: SharedClient) -> Vec<Box<dyn McpTool>> {
    vec![
        Box::new(SearchIssuesTool {
            client: client.clone(),
        }),
        Box::new(GetIssueTool {
            client: client.clone(),
        }),
        Box::new(CreateIssueTool {
            client: client.clone(),
        }),
        Box::new(TransitionTool { client }),
    ]
}

pub fn all_tools_unauth() -> Vec<Box<dyn McpTool>> {
    let http = reqwest::Client::builder()
        .user_agent("shannon-mcp-saas/0.7")
        .build()
        .expect("build reqwest client");
    all_tools(Arc::new(JiraClient::new(http, None)))
}

/// Erase the per-SaaS `McpTool` trait into `server::ServerTool`.
pub fn as_server_tool(tools: Vec<Box<dyn McpTool>>) -> Vec<Box<dyn crate::server::ServerTool>> {
    tools
        .into_iter()
        .map(|t| Box::new(JiraServerTool(t)) as Box<dyn crate::server::ServerTool>)
        .collect()
}

/// Adapter from `Box<dyn McpTool>` to `Box<dyn ServerTool>`.
pub struct JiraServerTool(pub Box<dyn McpTool>);

#[async_trait]
impl crate::server::ServerTool for JiraServerTool {
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

pub struct SearchIssuesTool {
    pub(crate) client: SharedClient,
}

#[async_trait]
impl McpTool for SearchIssuesTool {
    fn name(&self) -> &'static str {
        "jira_search_issues"
    }
    fn description(&self) -> &'static str {
        "Search Jira issues with JQL."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type":"object",
            "properties":{
                "jql":{"type":"string","description":"JQL query string, e.g. 'project = ENG AND status != Done'"},
                "max_results":{"type":"integer"},
                "start_at":{"type":"integer"}
            },
            "required":["jql"]
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
        let jql = require_string(&args, "jql")?;
        let max_results = optional_u32(&args, "max_results");
        let start_at = optional_u32(&args, "start_at");
        let (issues, total) = self
            .client
            .search_issues(jql, max_results, start_at)
            .await?;
        Ok(json!({ "issues": issues, "total": total }))
    }
}

pub struct GetIssueTool {
    pub(crate) client: SharedClient,
}

#[async_trait]
impl McpTool for GetIssueTool {
    fn name(&self) -> &'static str {
        "jira_get_issue"
    }
    fn description(&self) -> &'static str {
        "Fetch a Jira issue by key (e.g. 'ENG-123')."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type":"object",
            "properties":{
                "key":{"type":"string","description":"Issue key like 'ENG-123'"}
            },
            "required":["key"]
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
        let key = require_string(&args, "key")?;
        let issue = self.client.get_issue(key).await?;
        // The raw `Issue` v3 payload is large; return it whole so the
        // LLM picks the fields it cares about. Wrap in a top-level
        // `issue` key for consistency with `create_issue` / `transition`.
        Ok(json!({ "issue": issue }))
    }
}

pub struct CreateIssueTool {
    pub(crate) client: SharedClient,
}

#[async_trait]
impl McpTool for CreateIssueTool {
    fn name(&self) -> &'static str {
        "jira_create_issue"
    }
    fn description(&self) -> &'static str {
        "Create a new Jira issue."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type":"object",
            "properties":{
                "project":{"type":"string","description":"Project key, e.g. 'ENG'"},
                "summary":{"type":"string","description":"One-line title"},
                "issue_type":{"type":"string","description":"Type name (e.g. 'Task', 'Bug', 'Story')"},
                "description":{"type":"string","description":"Optional plain-text body"}
            },
            "required":["project","summary","issue_type"]
        })
    }
    fn required_permission(&self) -> &'static str {
        "write"
    }
    async fn execute(&self, args: Value) -> Result<Value, McpError> {
        require_write(&args)?;
        let project = require_string(&args, "project")?;
        let summary = require_string(&args, "summary")?;
        let issue_type = require_string(&args, "issue_type")?;
        let description = optional_string(&args, "description");
        let issue = self
            .client
            .create_issue(project, summary, issue_type, description)
            .await?;
        Ok(json!({ "issue": issue, "created": true }))
    }
}

pub struct TransitionTool {
    pub(crate) client: SharedClient,
}

#[async_trait]
impl McpTool for TransitionTool {
    fn name(&self) -> &'static str {
        "jira_transition"
    }
    fn description(&self) -> &'static str {
        "Move a Jira issue to a new workflow status."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type":"object",
            "properties":{
                "key":{"type":"string","description":"Issue key like 'ENG-123'"},
                "target_status":{"type":"string","description":"Target status name, e.g. 'In Progress'"}
            },
            "required":["key","target_status"]
        })
    }
    fn required_permission(&self) -> &'static str {
        "write"
    }
    async fn execute(&self, args: Value) -> Result<Value, McpError> {
        require_write(&args)?;
        let key = require_string(&args, "key")?;
        let target = require_string(&args, "target_status")?;
        let issue = self.client.transition(key, target).await?;
        Ok(json!({ "issue": issue, "transitioned": true }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_tools_returns_four() {
        assert_eq!(all_tools_unauth().len(), 4);
    }

    #[test]
    fn four_tool_names_are_stable() {
        let names: Vec<_> = all_tools_unauth().iter().map(|t| t.name()).collect();
        assert_eq!(
            names,
            vec![
                "jira_search_issues",
                "jira_get_issue",
                "jira_create_issue",
                "jira_transition",
            ]
        );
    }

    #[test]
    fn required_permission_matches_role() {
        let tools = all_tools_unauth();
        assert_eq!(tools[0].required_permission(), "read"); // search
        assert_eq!(tools[1].required_permission(), "read"); // get
        assert_eq!(tools[2].required_permission(), "write"); // create
        assert_eq!(tools[3].required_permission(), "write"); // transition
    }

    #[test]
    fn read_only_hints_match_required_permission() {
        let tools = all_tools_unauth();
        assert!(tools[0].is_read_only());
        assert!(tools[1].is_read_only());
        assert!(!tools[2].is_read_only());
        assert!(!tools[3].is_read_only());
    }
}
