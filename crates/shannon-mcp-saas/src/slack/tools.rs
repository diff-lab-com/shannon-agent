//! Slack MCP tool implementations.
//!
//! Six tools backed by [`SlackClient`]:
//!
//! 1. `slack_post_message`   — send a message to a channel (write).
//! 2. `slack_search_messages`— search messages across the workspace (read).
//! 3. `slack_read_channel`   — read recent history for a channel (read).
//! 4. `slack_thread_reply`   — reply to a thread (write).
//! 5. `slack_list_channels`  — list channels visible to the bot (read).
//! 6. `slack_get_user_info`  — look up a user by id (read).
//!
//! ## Permission gating
//!
//! Write tools (`slack_post_message`, `slack_thread_reply`) require the
//! host to have called `tools/grant { name, scope: "write" }` before
//! the `tools/call` arrives. The previous self-attested `args.permission`
//! gate is gone — that field is stripped at the JSON-RPC boundary in
//! `server::handle_tools_call` and the real capability check lives in
//! `server::SessionGrants`. These tool implementations therefore do **not**
//! re-attest write scope. The bot token remains the actual authority for
//! what Slack's API will accept.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use thiserror::Error;

use shannon_mcp::McpError;

use crate::slack::api::{ApiError, SlackClient};

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

/// UX hint only — the real gate is `server::SessionGrants`. The
/// `permission` field is stripped before reaching the tool, so this is a
/// no-op retained for symmetry with `jira::tools`.
fn require_write(_args: &Value) -> Result<(), ToolError> {
    Ok(())
}

pub fn all_tools(client: SharedClient) -> Vec<Box<dyn McpTool>> {
    vec![
        Box::new(PostMessageTool {
            client: client.clone(),
        }),
        Box::new(SearchMessagesTool {
            client: client.clone(),
        }),
        Box::new(ReadChannelTool {
            client: client.clone(),
        }),
        Box::new(ThreadReplyTool {
            client: client.clone(),
        }),
        Box::new(ListChannelsTool {
            client: client.clone(),
        }),
        Box::new(GetUserInfoTool { client }),
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

// ---------------------------------------------------------------------------
// PostMessageTool
// ---------------------------------------------------------------------------

pub struct PostMessageTool {
    pub(crate) client: SharedClient,
}

#[async_trait]
impl McpTool for PostMessageTool {
    fn name(&self) -> &'static str {
        "slack_post_message"
    }
    fn description(&self) -> &'static str {
        "Post a message to a Slack channel. Args: channel (string), text (string)."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "channel": { "type": "string", "description": "Channel id (C…) or name (#general)" },
                "text": { "type": "string", "description": "Message body. Supports Slack mrkdwn." }
            },
            "required": ["channel", "text"]
        })
    }
    fn required_permission(&self) -> &'static str {
        "write"
    }
    async fn execute(&self, args: Value) -> Result<Value, McpError> {
        require_write(&args)?;
        let channel = require_string(&args, "channel")?;
        let text = require_string(&args, "text")?;
        let result = self.client.post_message(channel, text).await?;
        Ok(json!({ "message": result, "posted": true }))
    }
}

// ---------------------------------------------------------------------------
// SearchMessagesTool
// ---------------------------------------------------------------------------

pub struct SearchMessagesTool {
    pub(crate) client: SharedClient,
}

#[async_trait]
impl McpTool for SearchMessagesTool {
    fn name(&self) -> &'static str {
        "slack_search_messages"
    }
    fn description(&self) -> &'static str {
        "Search messages across the workspace. Args: query (string), limit (u32, optional)."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query string" },
                "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
            },
            "required": ["query"]
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
        let query = require_string(&args, "query")?;
        let limit = optional_u32(&args, "limit");
        let matches = self.client.search_messages(query, limit).await?;
        Ok(json!({ "matches": matches.matches, "total": matches.total }))
    }
}

// ---------------------------------------------------------------------------
// ReadChannelTool
// ---------------------------------------------------------------------------

pub struct ReadChannelTool {
    pub(crate) client: SharedClient,
}

#[async_trait]
impl McpTool for ReadChannelTool {
    fn name(&self) -> &'static str {
        "slack_read_channel"
    }
    fn description(&self) -> &'static str {
        "Read recent messages from a channel. Args: channel (string), limit (u32, optional)."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "channel": { "type": "string" },
                "limit": { "type": "integer", "minimum": 1, "maximum": 200 }
            },
            "required": ["channel"]
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
        let channel = require_string(&args, "channel")?;
        let limit = optional_u32(&args, "limit");
        let messages = self.client.read_channel(channel, limit).await?;
        Ok(json!({ "messages": messages }))
    }
}

// ---------------------------------------------------------------------------
// ThreadReplyTool
// ---------------------------------------------------------------------------

pub struct ThreadReplyTool {
    pub(crate) client: SharedClient,
}

#[async_trait]
impl McpTool for ThreadReplyTool {
    fn name(&self) -> &'static str {
        "slack_thread_reply"
    }
    fn description(&self) -> &'static str {
        "Reply in a Slack thread. Args: channel (string), thread_ts (string), text (string)."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "channel": { "type": "string" },
                "thread_ts": { "type": "string", "description": "Parent message ts" },
                "text": { "type": "string" }
            },
            "required": ["channel", "thread_ts", "text"]
        })
    }
    fn required_permission(&self) -> &'static str {
        "write"
    }
    async fn execute(&self, args: Value) -> Result<Value, McpError> {
        require_write(&args)?;
        let channel = require_string(&args, "channel")?;
        let thread_ts = require_string(&args, "thread_ts")?;
        let text = require_string(&args, "text")?;
        let result = self.client.thread_reply(channel, thread_ts, text).await?;
        Ok(json!({ "message": result, "posted": true }))
    }
}

// ---------------------------------------------------------------------------
// ListChannelsTool
// ---------------------------------------------------------------------------

pub struct ListChannelsTool {
    pub(crate) client: SharedClient,
}

#[async_trait]
impl McpTool for ListChannelsTool {
    fn name(&self) -> &'static str {
        "slack_list_channels"
    }
    fn description(&self) -> &'static str {
        "List Slack channels visible to the bot."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "cursor": { "type": "string" },
                "limit": { "type": "integer", "minimum": 1, "maximum": 200 }
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

// ---------------------------------------------------------------------------
// GetUserInfoTool
// ---------------------------------------------------------------------------

pub struct GetUserInfoTool {
    pub(crate) client: SharedClient,
}

#[async_trait]
impl McpTool for GetUserInfoTool {
    fn name(&self) -> &'static str {
        "slack_get_user_info"
    }
    fn description(&self) -> &'static str {
        "Look up a Slack user by id. Args: user_id (string)."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "user_id": { "type": "string", "description": "User id like 'U…'" }
            },
            "required": ["user_id"]
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
        let user_id = require_string(&args, "user_id")?;
        let user = self.client.get_user_info(user_id).await?;
        Ok(json!({ "user": user }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_tools_returns_six() {
        assert_eq!(all_tools_unauth().len(), 6);
    }

    #[test]
    fn six_tool_names_are_stable() {
        let names: Vec<_> = all_tools_unauth().iter().map(|t| t.name()).collect();
        assert_eq!(
            names,
            vec![
                "slack_post_message",
                "slack_search_messages",
                "slack_read_channel",
                "slack_thread_reply",
                "slack_list_channels",
                "slack_get_user_info",
            ]
        );
    }

    #[test]
    fn required_permission_matches_role() {
        let tools = all_tools_unauth();
        // write tools
        assert_eq!(tools[0].required_permission(), "write"); // post_message
        assert_eq!(tools[3].required_permission(), "write"); // thread_reply
        // read tools
        assert_eq!(tools[1].required_permission(), "read"); // search
        assert_eq!(tools[2].required_permission(), "read"); // read_channel
        assert_eq!(tools[4].required_permission(), "read"); // list_channels
        assert_eq!(tools[5].required_permission(), "read"); // get_user_info
    }

    #[test]
    fn read_only_hints_match_required_permission() {
        let tools = all_tools_unauth();
        assert!(!tools[0].is_read_only()); // post_message
        assert!(tools[1].is_read_only()); // search
        assert!(tools[2].is_read_only()); // read_channel
        assert!(!tools[3].is_read_only()); // thread_reply
        assert!(tools[4].is_read_only()); // list_channels
        assert!(tools[5].is_read_only()); // get_user_info
    }

    #[tokio::test]
    async fn post_message_requires_channel_and_text() {
        // Missing text.
        let http = reqwest::Client::new();
        let tools = all_tools_unauth_with(http);
        assert!(tools[0].execute(json!({"channel":"C1"})).await.is_err());
        // Missing channel.
        assert!(tools[0].execute(json!({"text":"hi"})).await.is_err());
    }

    fn all_tools_unauth_with(http: reqwest::Client) -> Vec<Box<dyn McpTool>> {
        all_tools(Arc::new(SlackClient::new(http, None)))
    }
}
