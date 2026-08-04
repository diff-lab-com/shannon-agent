//! Notion MCP tool implementations.
//!
//! Six tools backed by [`NotionClient`]:
//!
//! 1. `notion_search_pages`     — search across the workspace (read).
//! 2. `notion_get_page`         — fetch a single page (read).
//! 3. `notion_append_block`     — append a block to a page (write).
//! 4. `notion_create_page`      — create a page in a database or under
//!    another page (write).
//! 5. `notion_list_databases`   — list databases visible to the
//!    integration (read).
//! 6. `notion_query_database`   — query a database with filter / sorts
//!    (read).
//!
//! ## Permission gating
//!
//! Write tools (`notion_append_block`, `notion_create_page`) require
//! the host to have called `tools/grant { name, scope: "write" }`
//! before the `tools/call` arrives. The previous self-attested
//! `args.permission` gate is gone — that field is stripped at the
//! JSON-RPC boundary in `server::handle_tools_call` and the real
//! capability check lives in `server::SessionGrants`. These tool
//! implementations therefore do **not** re-attest write scope.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use thiserror::Error;

use shannon_mcp::McpError;

use crate::notion::api::{ApiError, NotionClient};

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

pub type SharedClient = Arc<NotionClient>;

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

fn optional_object(args: &Value, key: &str) -> Option<Value> {
    args.get(key).cloned()
}

fn optional_array(args: &Value, key: &str) -> Option<Vec<Value>> {
    args.get(key).and_then(Value::as_array).cloned()
}

/// UX hint only — the real gate is `server::SessionGrants`. The
/// `permission` field is stripped before reaching the tool, so this is
/// a no-op retained for symmetry with `slack::tools` / `jira::tools`.
fn require_write(_args: &Value) -> Result<(), ToolError> {
    Ok(())
}

pub fn all_tools(client: SharedClient) -> Vec<Box<dyn McpTool>> {
    vec![
        Box::new(SearchPagesTool {
            client: client.clone(),
        }),
        Box::new(GetPageTool {
            client: client.clone(),
        }),
        Box::new(AppendBlockTool {
            client: client.clone(),
        }),
        Box::new(CreatePageTool {
            client: client.clone(),
        }),
        Box::new(ListDatabasesTool {
            client: client.clone(),
        }),
        Box::new(QueryDatabaseTool { client }),
    ]
}

pub fn all_tools_unauth() -> Vec<Box<dyn McpTool>> {
    let http = reqwest::Client::builder()
        .user_agent("shannon-mcp-saas/0.7")
        .build()
        .expect("build reqwest client");
    all_tools(Arc::new(NotionClient::new(http, None)))
}

/// Erase the per-SaaS `McpTool` trait into `server::ServerTool`.
pub fn as_server_tool(tools: Vec<Box<dyn McpTool>>) -> Vec<Box<dyn crate::server::ServerTool>> {
    tools
        .into_iter()
        .map(|t| Box::new(NotionServerTool(t)) as Box<dyn crate::server::ServerTool>)
        .collect()
}

/// Adapter from `Box<dyn McpTool>` to `Box<dyn ServerTool>`.
pub struct NotionServerTool(pub Box<dyn McpTool>);

#[async_trait]
impl crate::server::ServerTool for NotionServerTool {
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
// SearchPagesTool
// ---------------------------------------------------------------------------

pub struct SearchPagesTool {
    pub(crate) client: SharedClient,
}

#[async_trait]
impl McpTool for SearchPagesTool {
    fn name(&self) -> &'static str {
        "notion_search_pages"
    }
    fn description(&self) -> &'static str {
        "Search Notion pages and databases by query string."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query":        { "type": "string",  "description": "Search query; omit or send empty string to list everything visible to the integration" },
                "filter":       { "type": "object",  "description": "Notion search filter, e.g. { value: 'page', property: 'object' }" },
                "sort":         { "type": "object",  "description": "Notion sort directive, e.g. { direction: 'descending', timestamp: 'last_edited_time' }" },
                "start_cursor": { "type": "string",  "description": "Pagination cursor from a prior response" },
                "page_size":    { "type": "integer", "minimum": 1, "maximum": 100 }
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
        let query = optional_string(&args, "query");
        let filter = optional_object(&args, "filter");
        let sort = optional_object(&args, "sort");
        let start_cursor = optional_string(&args, "start_cursor");
        let page_size = optional_u32(&args, "page_size");
        let resp = self
            .client
            .search_pages(
                query,
                filter.as_ref(),
                sort.as_ref(),
                start_cursor,
                page_size,
            )
            .await?;
        Ok(json!({
            "results": resp.results,
            "has_more": resp.has_more,
            "next_cursor": resp.next_cursor,
        }))
    }
}

// ---------------------------------------------------------------------------
// GetPageTool
// ---------------------------------------------------------------------------

pub struct GetPageTool {
    pub(crate) client: SharedClient,
}

#[async_trait]
impl McpTool for GetPageTool {
    fn name(&self) -> &'static str {
        "notion_get_page"
    }
    fn description(&self) -> &'static str {
        "Fetch a single Notion page by id. Returns the page object with properties."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "page_id": {
                    "type": "string",
                    "description": "Page id (hyphenated UUID, e.g. '00000000-0000-0000-0000-000000000000')"
                }
            },
            "required": ["page_id"]
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
        let page_id = require_string(&args, "page_id")?;
        let page = self.client.get_page(page_id).await?;
        Ok(json!({ "page": page }))
    }
}

// ---------------------------------------------------------------------------
// AppendBlockTool
// ---------------------------------------------------------------------------

pub struct AppendBlockTool {
    pub(crate) client: SharedClient,
}

#[async_trait]
impl McpTool for AppendBlockTool {
    fn name(&self) -> &'static str {
        "notion_append_block"
    }
    fn description(&self) -> &'static str {
        "Append a block (or block tree) to a Notion page's children."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "page_id": { "type": "string",  "description": "Page id to append to" },
                "block":   { "type": "object",  "description": "Block object, e.g. { object: 'block', type: 'paragraph', paragraph: { rich_text: [...] } }" }
            },
            "required": ["page_id", "block"]
        })
    }
    fn required_permission(&self) -> &'static str {
        "write"
    }
    async fn execute(&self, args: Value) -> Result<Value, McpError> {
        require_write(&args)?;
        let page_id = require_string(&args, "page_id")?;
        let block = optional_object(&args, "block").ok_or(ToolError::MissingArg("block"))?;
        let appended = self.client.append_block(page_id, &block).await?;
        Ok(json!({ "block": appended, "appended": true }))
    }
}

// ---------------------------------------------------------------------------
// CreatePageTool
// ---------------------------------------------------------------------------

pub struct CreatePageTool {
    pub(crate) client: SharedClient,
}

#[async_trait]
impl McpTool for CreatePageTool {
    fn name(&self) -> &'static str {
        "notion_create_page"
    }
    fn description(&self) -> &'static str {
        "Create a new Notion page in a database or under another page."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "parent":     { "type": "object",  "description": "Notion parent, e.g. { database_id: '...' } or { page_id: '...' }" },
                "properties": { "type": "object",  "description": "Notion page properties keyed by column name" },
                "children":   { "type": "array",   "description": "Optional initial block children", "items": { "type": "object" } }
            },
            "required": ["parent", "properties"]
        })
    }
    fn required_permission(&self) -> &'static str {
        "write"
    }
    async fn execute(&self, args: Value) -> Result<Value, McpError> {
        require_write(&args)?;
        let parent = optional_object(&args, "parent").ok_or(ToolError::MissingArg("parent"))?;
        let properties =
            optional_object(&args, "properties").ok_or(ToolError::MissingArg("properties"))?;
        let children = optional_array(&args, "children");
        let page = self
            .client
            .create_page(&parent, &properties, children.as_deref())
            .await?;
        Ok(json!({ "page": page, "created": true }))
    }
}

// ---------------------------------------------------------------------------
// ListDatabasesTool
// ---------------------------------------------------------------------------

pub struct ListDatabasesTool {
    pub(crate) client: SharedClient,
}

#[async_trait]
impl McpTool for ListDatabasesTool {
    fn name(&self) -> &'static str {
        "notion_list_databases"
    }
    fn description(&self) -> &'static str {
        "List Notion databases visible to the integration."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "start_cursor": { "type": "string",  "description": "Pagination cursor" },
                "page_size":    { "type": "integer", "minimum": 1, "maximum": 100 }
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
        let start_cursor = optional_string(&args, "start_cursor");
        let page_size = optional_u32(&args, "page_size");
        let resp = self.client.list_databases(start_cursor, page_size).await?;
        Ok(json!({
            "databases": resp.results,
            "has_more": resp.has_more,
            "next_cursor": resp.next_cursor,
        }))
    }
}

// ---------------------------------------------------------------------------
// QueryDatabaseTool
// ---------------------------------------------------------------------------

pub struct QueryDatabaseTool {
    pub(crate) client: SharedClient,
}

#[async_trait]
impl McpTool for QueryDatabaseTool {
    fn name(&self) -> &'static str {
        "notion_query_database"
    }
    fn description(&self) -> &'static str {
        "Query a Notion database with optional filter, sorts, and pagination."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "database_id": { "type": "string",  "description": "Database id" },
                "filter":      { "type": "object",  "description": "Notion filter object" },
                "sorts":       { "type": "array",   "description": "Array of sort directives", "items": { "type": "object" } },
                "start_cursor":{ "type": "string",  "description": "Pagination cursor" },
                "page_size":   { "type": "integer", "minimum": 1, "maximum": 100 }
            },
            "required": ["database_id"]
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
        let database_id = require_string(&args, "database_id")?;
        let filter = optional_object(&args, "filter");
        let sorts = optional_array(&args, "sorts");
        let start_cursor = optional_string(&args, "start_cursor");
        let page_size = optional_u32(&args, "page_size");
        let resp = self
            .client
            .query_database(
                database_id,
                filter.as_ref(),
                sorts.as_deref(),
                page_size,
                start_cursor,
            )
            .await?;
        Ok(json!({
            "rows": resp.results,
            "has_more": resp.has_more,
            "next_cursor": resp.next_cursor,
        }))
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
                "notion_search_pages",
                "notion_get_page",
                "notion_append_block",
                "notion_create_page",
                "notion_list_databases",
                "notion_query_database",
            ]
        );
    }

    #[test]
    fn required_permission_matches_role() {
        let tools = all_tools_unauth();
        // read tools
        assert_eq!(tools[0].required_permission(), "read"); // search_pages
        assert_eq!(tools[1].required_permission(), "read"); // get_page
        assert_eq!(tools[4].required_permission(), "read"); // list_databases
        assert_eq!(tools[5].required_permission(), "read"); // query_database
        // write tools
        assert_eq!(tools[2].required_permission(), "write"); // append_block
        assert_eq!(tools[3].required_permission(), "write"); // create_page
    }

    #[test]
    fn read_only_hints_match_required_permission() {
        let tools = all_tools_unauth();
        assert!(tools[0].is_read_only());
        assert!(tools[1].is_read_only());
        assert!(!tools[2].is_read_only());
        assert!(!tools[3].is_read_only());
        assert!(tools[4].is_read_only());
        assert!(tools[5].is_read_only());
    }
}
