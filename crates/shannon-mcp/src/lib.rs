// shannon-mcp: MCP (Model Context Protocol) implementation for Shannon
#![allow(clippy::type_complexity)]
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

pub mod auth;
pub mod client;
pub mod config;
pub mod process_pool;
pub mod protocol;
pub mod resource_subscription;
pub mod resources;
pub mod server_manager;
pub mod transport;
pub mod webhook;

pub use auth::{
    ApiKeyProvider, AuthProvider, DcrRegistrationResult, OAuth2Provider, OAuthDiscoveryResult,
    auto_register_oauth, discover_oauth_endpoints, register_client,
};
pub use client::{McpClient, McpClientError};
pub use config::{
    ConfigError, HeaderSource, McpAuthConfig, McpConfig, McpServerConfig, config_search_paths,
    discover_config, expand_env_vars, expand_server_config,
};
pub use process_pool::{
    ChunkResult, McpProcessPool, PooledDiscoveryResult, PooledMcpToolAdapter, ServerState,
    ServerStatus, UserPromptCallback, discover_pooled_remote_tools, discover_pooled_tools,
    make_elicitation_provider, make_sampling_provider,
};
pub use protocol::{
    ClientCapabilities, ClientInfo, Completion, CompletionRef, CompletionRequest, CompletionResult,
    CompletionValue, CompletionsCapability, ContentBlock, CreateMessageRequest,
    CreateMessageResult, ElicitationAction, ElicitationRequest, ElicitationResult,
    InitializeParams, InitializeResult, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, ListPromptsResult, ListResourceTemplatesResult, ListResourcesResult,
    ListRootsResult, ListToolsResult, LoggingLevel, McpCapabilities, McpNotification, McpRequest,
    McpResponse, ModelHint, ModelPreferences, NotificationMethod, ProgressNotification,
    ProgressToken, Prompt, PromptArgument, RequestMethod, Resource, ResourceContent,
    ResourceTemplate, ResourcesUpdatedNotification, ResponseMethod, Root, RootsCapability,
    SamplingCapability, SamplingContent, SamplingMessage, SamplingMessageRole, SamplingParams,
    ServerCapabilities, ServerInfo, StopReason, SubscribeRequest, SubscribeResult, Tool,
    ToolAnnotations, ToolContent, UnsubscribeRequest,
};
pub use resource_subscription::{ResourceSubscriptionManager, ResourceUpdate, SubscriptionInfo};
pub use resources::{
    ListResourcesInput, ListResourcesOutput, McpClientAdapter, McpResourceClient,
    McpResourceManager, ReadResourceInput, ReadResourceOutput, ResourceDescriptor,
    ResourceReadContent, SubscribeResourceInput, SubscribeResourceOutput, UnsubscribeResourceInput,
    UnsubscribeResourceOutput,
};
pub use server_manager::{
    McpDiscoveryResult, PooledMcpDiscoveryResult, discover_all_servers,
    discover_all_servers_pooled, discover_all_servers_pooled_nonblocking,
};
pub use transport::{
    HttpTransport, SseTransport, StdioTransport, Transport, TransportError, WebSocketTransport,
};
pub use webhook::{
    EventPublisher, McpEvent, McpEventType, WebhookConfig, WebhookDelivery, WebhookRegistry,
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
