//! Slack MCP tool implementations.
//!
//! ## Permission gating
//!
//! Write tools (`slack_conversations_reply`, `slack_files_upload`,
//! `slack_reactions_add`) require the host to call
//! `tools/grant { name, scope: "write" }` before the `tools/call`
//! arrives. The previous `args.permission` self-attested gate has been
//! removed (the field is stripped at the JSON-RPC boundary in
//! `server::handle_tools_call` and the real check lives in
//! `server::SessionGrants`). The OAuth bot token remains the actual
//! authority for upstream calls.

use crate::slack::api::{ApiError, SlackClient};
use async_trait::async_trait;
use serde_json::{Value, json};
use shannon_mcp::McpError;
use std::sync::Arc;
use thiserror::Error;

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
pub type SharedClient = Arc<SlackClient>;

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
fn require_write(args: &Value) -> Result<(), ToolError> {
    // UX hint only — the real gate is server::SessionGrants. The
    // `permission` field is stripped before reaching the tool, so this
    // is effectively a no-op. Kept for backwards compatibility with
    // any host that still passes the field; will be removed in a
    // future cleanup once the host-side migration is complete.
    let _ = args;
    Ok(())
}

pub fn all_tools(client: SharedClient) -> Vec<Box<dyn McpTool>> {
    vec![
        Box::new(ListChannelsTool {
            client: client.clone(),
        }),
        Box::new(HistoryTool {
            client: client.clone(),
        }),
        Box::new(ReplyTool {
            client: client.clone(),
        }),
        Box::new(UsersListTool {
            client: client.clone(),
        }),
        Box::new(UploadFileTool {
            client: client.clone(),
        }),
        Box::new(AddReactionTool { client }),
    ]
}
pub fn all_tools_unauth() -> Vec<Box<dyn McpTool>> {
    let http = reqwest::Client::builder()
        .user_agent("shannon-mcp-saas/0.7")
        .build()
        .expect("build reqwest client");
    all_tools(Arc::new(SlackClient::new(http, None)))
}

/// Erase the per-SaaS `McpTool` trait into `server::ServerTool`.
pub fn as_server_tool(tools: Vec<Box<dyn McpTool>>) -> Vec<Box<dyn crate::server::ServerTool>> {
    tools
        .into_iter()
        .map(|t| Box::new(SlackServerTool(t)) as Box<dyn crate::server::ServerTool>)
        .collect()
}

/// Adapter from `Box<dyn McpTool>` to `Box<dyn ServerTool>`.
pub struct SlackServerTool(pub Box<dyn McpTool>);

#[async_trait]
impl crate::server::ServerTool for SlackServerTool {
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

pub struct ListChannelsTool {
    pub(crate) client: SharedClient,
}
#[async_trait]
impl McpTool for ListChannelsTool {
    fn name(&self) -> &'static str {
        "slack_list_channels"
    }
    fn description(&self) -> &'static str {
        "List Slack channels."
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"cursor":{"type":"string"},"limit":{"type":"integer"}}})
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
        let channels = self
            .client
            .list_channels(
                optional_string(&args, "cursor"),
                optional_u32(&args, "limit"),
            )
            .await?;
        Ok(json!({ "channels": channels }))
    }
}

pub struct HistoryTool {
    pub(crate) client: SharedClient,
}
#[async_trait]
impl McpTool for HistoryTool {
    fn name(&self) -> &'static str {
        "slack_conversations_history"
    }
    fn description(&self) -> &'static str {
        "Read messages from a Slack channel."
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"channel":{"type":"string"},"cursor":{"type":"string"},"limit":{"type":"integer"}},"required":["channel"]})
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
        let channel = require_string(&args, "channel")?;
        let messages = self
            .client
            .history(
                channel,
                optional_string(&args, "cursor"),
                optional_u32(&args, "limit"),
            )
            .await?;
        Ok(json!({ "messages": messages }))
    }
}

pub struct ReplyTool {
    pub(crate) client: SharedClient,
}
#[async_trait]
impl McpTool for ReplyTool {
    fn name(&self) -> &'static str {
        "slack_conversations_reply"
    }
    fn description(&self) -> &'static str {
        "Reply in a Slack thread."
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"channel":{"type":"string"},"thread_ts":{"type":"string"},"text":{"type":"string"}},"required":["channel","thread_ts","text"]})
    }
    fn required_permission(&self) -> &'static str {
        "write"
    }
    async fn execute(&self, args: Value) -> Result<Value, McpError> {
        require_write(&args)?;
        let result = self
            .client
            .reply(
                require_string(&args, "channel")?,
                require_string(&args, "thread_ts")?,
                require_string(&args, "text")?,
            )
            .await?;
        Ok(json!({ "message": result, "created": true }))
    }
}

pub struct UsersListTool {
    pub(crate) client: SharedClient,
}
#[async_trait]
impl McpTool for UsersListTool {
    fn name(&self) -> &'static str {
        "slack_users_list"
    }
    fn description(&self) -> &'static str {
        "List Slack workspace users."
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"cursor":{"type":"string"},"limit":{"type":"integer"}}})
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
        let users = self
            .client
            .users_list(
                optional_string(&args, "cursor"),
                optional_u32(&args, "limit"),
            )
            .await?;
        Ok(json!({ "users": users }))
    }
}

pub struct UploadFileTool {
    pub(crate) client: SharedClient,
}
#[async_trait]
impl McpTool for UploadFileTool {
    fn name(&self) -> &'static str {
        "slack_files_upload"
    }
    fn description(&self) -> &'static str {
        "Upload a file to Slack."
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"channels":{"type":"string"},"content":{"type":"string"},"filename":{"type":"string"}},"required":["channels","content"]})
    }
    fn required_permission(&self) -> &'static str {
        "write"
    }
    async fn execute(&self, args: Value) -> Result<Value, McpError> {
        require_write(&args)?;
        let file = self
            .client
            .upload_file(
                require_string(&args, "channels")?,
                require_string(&args, "content")?,
                optional_string(&args, "filename"),
            )
            .await?;
        Ok(json!({ "file": file, "uploaded": true }))
    }
}

pub struct AddReactionTool {
    pub(crate) client: SharedClient,
}
#[async_trait]
impl McpTool for AddReactionTool {
    fn name(&self) -> &'static str {
        "slack_reactions_add"
    }
    fn description(&self) -> &'static str {
        "Add an emoji reaction to a Slack message."
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"channel":{"type":"string"},"timestamp":{"type":"string"},"name":{"type":"string"}},"required":["channel","timestamp","name"]})
    }
    fn required_permission(&self) -> &'static str {
        "write"
    }
    async fn execute(&self, args: Value) -> Result<Value, McpError> {
        require_write(&args)?;
        self.client
            .add_reaction(
                require_string(&args, "channel")?,
                require_string(&args, "timestamp")?,
                require_string(&args, "name")?,
            )
            .await?;
        Ok(json!({ "added": true }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_tools_returns_six() {
        assert_eq!(all_tools_unauth().len(), 6);
    }
}
