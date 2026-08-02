//! Six GitHub tool implementations backed by [`GitHubClient`].
//!
//! Each tool:
//! 1. Parses + validates the input JSON.
//! 2. For *write* tools, asserts `args.permission == "write"` and
//!    otherwise returns `McpError::PermissionRequired("write")`.
//! 3. Calls the matching [`GitHubClient`] method, mapping
//!    [`ApiError`] to MCP error codes (see the table at the bottom of
//!    this file).
//! 4. Returns a `serde_json::Value` shaped to match the GitHub REST
//!    response (the same shape the legacy stub returned, so existing
//!    clients don't break).
//!
//! ## Permission gating
//!
//! Write tools (create_issue, comment, review_pr) require the caller
//! to pass `"permission": "write"` in the arguments. The MCP host
//! (Shannon's `PermissionRuleChecker`) is expected to inject that arg
//! once the user has granted the write scope; we never accept it as
//! proof of authorization on its own — the keyring/OAuth token in
//! `GitHubClient` is the actual authority.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use thiserror::Error;

use shannon_mcp::McpError;

use crate::github::api::{ApiError, GitHubClient, Issue, IssueState, ReviewEvent};

/// The single source of truth for an unauthenticated client (used by
/// the spike-style tools/list check + tests that don't care about
/// auth). Real call paths construct their own via
/// [`crate::github::auth::TokenProvider`].
fn empty_client() -> GitHubClient {
    GitHubClient::new(
        reqwest::Client::builder()
            .user_agent("shannon-mcp-saas/0.7")
            .build()
            .expect("build reqwest client"),
        None,
    )
}

// ---------------------------------------------------------------------------
// Error mapping
// ---------------------------------------------------------------------------

/// Wraps `ApiError` with the MCP error code so the stdio server can
/// return a structured response. The variant strings show up in the
/// error message for `PermissionRequired` so the client can prompt
/// the user.
#[derive(Debug, Error)]
pub enum ToolError {
    #[error("invalid arguments: {0}")]
    InvalidArgs(String),
    #[error("missing required argument: {0}")]
    MissingArg(&'static str),
    #[error("permission required: {0}")]
    PermissionRequired(&'static str),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("upstream API error: {0}")]
    Api(String),
}

impl From<ApiError> for ToolError {
    fn from(e: ApiError) -> Self {
        match e {
            ApiError::NotFound(owner, repo, n) => {
                ToolError::NotFound(format!("{owner}/{repo}#{n}"))
            }
            other => ToolError::Api(other.to_string()),
        }
    }
}

impl From<ToolError> for McpError {
    fn from(e: ToolError) -> Self {
        // We use the `InvalidRequest` variant for shape errors and
        // `Server` for everything else. PermissionRequired is a
        // sub-class of `InvalidRequest` so it surfaces with the
        // standard JSON-RPC `-32600` code; the user-facing message
        // names the missing scope.
        match e {
            ToolError::InvalidArgs(_)
            | ToolError::MissingArg(_)
            | ToolError::PermissionRequired(_) => McpError::InvalidRequest(e.to_string()),
            other => McpError::Server(other.to_string()),
        }
    }
}

impl From<ApiError> for McpError {
    fn from(e: ApiError) -> Self {
        ToolError::from(e).into()
    }
}

// ---------------------------------------------------------------------------
// McpTool trait
// ---------------------------------------------------------------------------

/// Behavior shared by every tool. Lets the stdio server dispatch by name
/// without each tool being a separate type registration.
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
    /// `permission` metadata advertised in `tools/list` so the
    /// `PermissionRuleChecker` on the Shannon side can map it to an
    /// `ApprovalMode` without parsing the schema.
    fn required_permission(&self) -> &'static str;
    async fn execute(&self, args: Value) -> Result<Value, McpError>;
}

// ---------------------------------------------------------------------------
// Permission helpers
// ---------------------------------------------------------------------------

/// Returns `Ok(())` when the caller passed `"permission": "write"`,
/// else `Err(ToolError::PermissionRequired("write"))`.
fn require_write(args: &Value) -> Result<(), ToolError> {
    match args.get("permission").and_then(|v| v.as_str()) {
        Some("write") => Ok(()),
        _ => Err(ToolError::PermissionRequired("write")),
    }
}

fn require_string<'a>(args: &'a Value, key: &'static str) -> Result<&'a str, ToolError> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or(ToolError::MissingArg(key))
}

fn require_u64(args: &Value, key: &'static str) -> Result<u64, ToolError> {
    args.get(key)
        .and_then(|v| v.as_u64())
        .ok_or(ToolError::MissingArg(key))
}

fn optional_string<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|v| v.as_str())
}

fn optional_u8(args: &Value, key: &str) -> Option<u8> {
    args.get(key)
        .and_then(|v| v.as_u64())
        .and_then(|n| u8::try_from(n).ok())
}

fn optional_u32(args: &Value, key: &str) -> Option<u32> {
    args.get(key)
        .and_then(|v| v.as_u64())
        .and_then(|n| u32::try_from(n).ok())
}

fn optional_string_vec(args: &Value, key: &str) -> Option<Vec<String>> {
    args.get(key).and_then(|v| v.as_array()).map(|arr| {
        arr.iter()
            .filter_map(|i| i.as_str().map(|s| s.to_string()))
            .collect()
    })
}

fn parse_issue_state(s: Option<&str>) -> Result<IssueState, ToolError> {
    match s.unwrap_or("open") {
        "open" => Ok(IssueState::Open),
        "closed" => Ok(IssueState::Closed),
        "all" => Ok(IssueState::All),
        other => Err(ToolError::InvalidArgs(format!(
            "state must be open|closed|all (got {other:?})"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Shared client state
// ---------------------------------------------------------------------------

/// A handle to the live `GitHubClient` shared across all 6 tools.
pub type SharedClient = Arc<GitHubClient>;

/// Build the default list of six tools, each sharing the given client.
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
        Box::new(CommentTool {
            client: client.clone(),
        }),
        Box::new(ListPrsTool {
            client: client.clone(),
        }),
        Box::new(ReviewPrTool { client }),
    ]
}

/// Construct a list of unauthenticated tools (no token set). Used by
/// the stdio server's startup path and by the existing spike tests
/// that only exercise `tools/list`.
pub fn all_tools_unauth() -> Vec<Box<dyn McpTool>> {
    all_tools(Arc::new(empty_client()))
}

// ---------------------------------------------------------------------------
// ListIssuesTool
// ---------------------------------------------------------------------------

pub struct ListIssuesTool {
    pub(crate) client: SharedClient,
}

#[async_trait]
impl McpTool for ListIssuesTool {
    fn name(&self) -> &'static str {
        "github_list_issues"
    }
    fn description(&self) -> &'static str {
        "List issues in a GitHub repository. Args: owner (string), repo (string), state (string, optional: open|closed|all), since (string ISO 8601, optional), per_page (u8, optional, default 30), page (u32, optional, default 1)."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "owner": { "type": "string" },
                "repo": { "type": "string" },
                "state": { "type": "string", "enum": ["open", "closed", "all"] },
                "since": { "type": "string", "format": "date-time" },
                "per_page": { "type": "integer", "minimum": 1, "maximum": 100 },
                "page": { "type": "integer", "minimum": 1 }
            },
            "required": ["owner", "repo"]
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
        let owner = require_string(&args, "owner")?;
        let repo = require_string(&args, "repo")?;
        let state = parse_issue_state(optional_string(&args, "state"))?;
        let since = optional_string(&args, "since");
        let per_page = optional_u8(&args, "per_page");
        let page = optional_u32(&args, "page");
        let issues = self
            .client
            .list_issues(owner, repo, state, since, per_page, page)
            .await?;
        Ok(issues_to_value(&issues))
    }
}

fn issues_to_value(issues: &[Issue]) -> Value {
    json!({
        "issues": issues,
        "total_count": issues.len(),
    })
}

// ---------------------------------------------------------------------------
// GetIssueTool
// ---------------------------------------------------------------------------

pub struct GetIssueTool {
    pub(crate) client: SharedClient,
}

#[async_trait]
impl McpTool for GetIssueTool {
    fn name(&self) -> &'static str {
        "github_get_issue"
    }
    fn description(&self) -> &'static str {
        "Get a single issue by number. Args: owner (string), repo (string), number (u32)."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "owner": { "type": "string" },
                "repo": { "type": "string" },
                "number": { "type": "integer", "minimum": 1 }
            },
            "required": ["owner", "repo", "number"]
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
        let owner = require_string(&args, "owner")?;
        let repo = require_string(&args, "repo")?;
        let number = require_u64(&args, "number")?;
        let issue = self.client.get_issue(owner, repo, number).await?;
        Ok(json!({ "issue": issue }))
    }
}

// ---------------------------------------------------------------------------
// CreateIssueTool
// ---------------------------------------------------------------------------

pub struct CreateIssueTool {
    pub(crate) client: SharedClient,
}

#[async_trait]
impl McpTool for CreateIssueTool {
    fn name(&self) -> &'static str {
        "github_create_issue"
    }
    fn description(&self) -> &'static str {
        "Create a new issue. Args: owner (string), repo (string), title (string), body (string, optional), labels (array<string>, optional), assignees (array<string>, optional), permission (string, required: must be \"write\")."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "owner": { "type": "string" },
                "repo": { "type": "string" },
                "title": { "type": "string" },
                "body": { "type": "string" },
                "labels": { "type": "array", "items": { "type": "string" } },
                "assignees": { "type": "array", "items": { "type": "string" } },
                "permission": { "type": "string", "enum": ["write"] }
            },
            "required": ["owner", "repo", "title", "permission"]
        })
    }
    fn is_read_only(&self) -> bool {
        false
    }
    fn required_permission(&self) -> &'static str {
        "write"
    }
    async fn execute(&self, args: Value) -> Result<Value, McpError> {
        require_write(&args)?;
        let owner = require_string(&args, "owner")?;
        let repo = require_string(&args, "repo")?;
        let title = require_string(&args, "title")?;
        let body = optional_string(&args, "body");
        let labels = optional_string_vec(&args, "labels");
        let assignees = optional_string_vec(&args, "assignees");
        let issue = self
            .client
            .create_issue(
                owner,
                repo,
                title,
                body,
                labels.as_deref(),
                assignees.as_deref(),
            )
            .await?;
        Ok(json!({ "issue": issue, "created": true }))
    }
}

// ---------------------------------------------------------------------------
// CommentTool
// ---------------------------------------------------------------------------

pub struct CommentTool {
    pub(crate) client: SharedClient,
}

#[async_trait]
impl McpTool for CommentTool {
    fn name(&self) -> &'static str {
        "github_comment"
    }
    fn description(&self) -> &'static str {
        "Add a comment to an issue or PR. Args: owner (string), repo (string), number (u32), body (string), permission (string, required: must be \"write\")."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "owner": { "type": "string" },
                "repo": { "type": "string" },
                "number": { "type": "integer", "minimum": 1 },
                "body": { "type": "string" },
                "permission": { "type": "string", "enum": ["write"] }
            },
            "required": ["owner", "repo", "number", "body", "permission"]
        })
    }
    fn required_permission(&self) -> &'static str {
        "write"
    }
    async fn execute(&self, args: Value) -> Result<Value, McpError> {
        require_write(&args)?;
        let owner = require_string(&args, "owner")?;
        let repo = require_string(&args, "repo")?;
        let number = require_u64(&args, "number")?;
        let body = require_string(&args, "body")?;
        let comment = self.client.comment(owner, repo, number, body).await?;
        Ok(json!({ "comment": comment, "created": true }))
    }
}

// ---------------------------------------------------------------------------
// ListPrsTool
// ---------------------------------------------------------------------------

pub struct ListPrsTool {
    pub(crate) client: SharedClient,
}

#[async_trait]
impl McpTool for ListPrsTool {
    fn name(&self) -> &'static str {
        "github_list_prs"
    }
    fn description(&self) -> &'static str {
        "List pull requests. Args: owner (string), repo (string), state (string, optional: open|closed|all), per_page (u8, optional), page (u32, optional)."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "owner": { "type": "string" },
                "repo": { "type": "string" },
                "state": { "type": "string", "enum": ["open", "closed", "all"] },
                "per_page": { "type": "integer", "minimum": 1, "maximum": 100 },
                "page": { "type": "integer", "minimum": 1 }
            },
            "required": ["owner", "repo"]
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
        let owner = require_string(&args, "owner")?;
        let repo = require_string(&args, "repo")?;
        let state = parse_issue_state(optional_string(&args, "state"))?;
        let per_page = optional_u8(&args, "per_page");
        let page = optional_u32(&args, "page");
        let prs = self
            .client
            .list_prs(owner, repo, state, per_page, page)
            .await?;
        Ok(json!({ "pulls": prs, "total_count": prs.len() }))
    }
}

// ---------------------------------------------------------------------------
// ReviewPrTool
// ---------------------------------------------------------------------------

pub struct ReviewPrTool {
    pub(crate) client: SharedClient,
}

#[async_trait]
impl McpTool for ReviewPrTool {
    fn name(&self) -> &'static str {
        "github_review_pr"
    }
    fn description(&self) -> &'static str {
        "Submit a pull request review. Args: owner (string), repo (string), number (u32), event (string: APPROVE|REQUEST_CHANGES|COMMENT), body (string, optional), commit_id (string, optional), permission (string, required: must be \"write\")."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "owner": { "type": "string" },
                "repo": { "type": "string" },
                "number": { "type": "integer", "minimum": 1 },
                "event": { "type": "string", "enum": ["APPROVE", "REQUEST_CHANGES", "COMMENT"] },
                "body": { "type": "string" },
                "commit_id": { "type": "string" },
                "permission": { "type": "string", "enum": ["write"] }
            },
            "required": ["owner", "repo", "number", "event", "permission"]
        })
    }
    fn required_permission(&self) -> &'static str {
        "write"
    }
    async fn execute(&self, args: Value) -> Result<Value, McpError> {
        require_write(&args)?;
        let owner = require_string(&args, "owner")?;
        let repo = require_string(&args, "repo")?;
        let number = require_u64(&args, "number")?;
        let event_str = require_string(&args, "event")?;
        let event = ReviewEvent::parse(event_str).ok_or_else(|| {
            ToolError::InvalidArgs(format!(
                "event must be APPROVE|REQUEST_CHANGES|COMMENT (got {event_str:?})"
            ))
        })?;
        let body = optional_string(&args, "body");
        let commit_id = optional_string(&args, "commit_id");
        let review = self
            .client
            .review_pr(owner, repo, number, event, body, commit_id)
            .await?;
        Ok(json!({ "review": review, "submitted": true }))
    }
}
