// shannon-mcp: MCP (Model Context Protocol) implementation for Shannon
//
// This crate provides a complete implementation of the Model Context Protocol,
// enabling AI assistants to interact with local servers, tools, and resources.
//
// ## Architecture
//
// The crate is organized into several modules:
//
// - **protocol**: Core MCP protocol types and message definitions
// - **transport**: Transport layer abstractions (stdio, SSE, HTTP, WebSocket)
// - **client**: MCP client implementation for connecting to servers
// - **auth**: Authentication mechanisms (OAuth 2.0 PKCE, API keys)
//
// ## Example Usage
//
// ```rust,no_run
// use shannon_mcp::{client::McpClient, transport::StdioTransport};
//
// #[tokio::main]
// async fn main() -> Result<(), Box<dyn std::error::Error>> {
//     let transport = StdioTransport::new("/path/to/server");
//     let mut client = McpClient::connect(transport).await?;
//
//     // Initialize connection
//     let capabilities = client.initialize().await?;
//
//     // List available tools
//     let tools = client.list_tools().await?;
//
//     // Call a tool
//     let result = client.call_tool("my_tool", vec![]).await?;
//
//     Ok(())
// }
// ```

pub mod client;
pub mod protocol;
pub mod transport;
pub mod auth;
pub mod resources;

pub use protocol::{
    JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, JsonRpcNotification,
    McpRequest, McpResponse, McpNotification,
    RequestMethod, ResponseMethod, NotificationMethod,
    Tool, Resource, ResourceTemplate, Prompt, PromptArgument,
    McpCapabilities, ServerCapabilities, ClientCapabilities,
    ToolContent, ResourceContent, ContentBlock,
    CompletionRequest, CompletionRef, CompletionResult, Completion, CompletionValue,
    LoggingLevel,
    InitializeParams, InitializeResult, ClientInfo, ServerInfo,
    ListToolsResult, ListResourcesResult, ListPromptsResult, ListResourceTemplatesResult,
    SubscribeRequest, UnsubscribeRequest, SubscribeResult,
};
pub use transport::{Transport, TransportError, StdioTransport, SseTransport, HttpTransport, WebSocketTransport};
pub use client::{McpClient, McpClientError};
pub use auth::{AuthProvider, OAuth2Provider, ApiKeyProvider};
pub use resources::{
    ResourceDescriptor, ListResourcesInput, ListResourcesOutput,
    ReadResourceInput, ReadResourceOutput, ResourceReadContent,
    McpResourceManager, McpResourceClient, McpClientAdapter,
};

/// Current MCP protocol version supported by this implementation
pub const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

/// Result type alias for MCP operations
pub type McpResult<T> = Result<T, McpError>;

/// Error type for MCP operations
#[derive(thiserror::Error, Debug)]
pub enum McpError {
    #[error("transport error: {0}")]
    Transport(#[from] transport::TransportError),

    #[error("client error: {0}")]
    Client(#[from] client::McpClientError),

    #[error("authentication error: {0}")]
    Auth(String),

    #[error("protocol error: {0}")]
    Protocol(String),

    #[error("JSON-RPC error: code={0}, message={1}")]
    JsonRpc(i32, String),

    #[error("server error: {0}")]
    Server(String),

    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("timeout: operation took longer than {0:?}")]
    Timeout(std::time::Duration),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

impl From<reqwest::Error> for McpError {
    fn from(err: reqwest::Error) -> Self {
        McpError::Transport(transport::TransportError::Http(err.to_string()))
    }
}
