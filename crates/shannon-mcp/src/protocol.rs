// Core MCP protocol type definitions
//
// This module defines the types used in the Model Context Protocol,
// including JSON-RPC messages, requests, responses, and notifications.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// JSON-RPC message envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcMessage {
    Request(JsonRpcRequest),
    Response(JsonRpcResponse),
    Notification(JsonRpcNotification),
}

impl JsonRpcMessage {
    pub fn id(&self) -> Option<&str> {
        match self {
            JsonRpcMessage::Request(req) => Some(&req.id),
            JsonRpcMessage::Response(res) => Some(&res.id),
            JsonRpcMessage::Notification(_) => None,
        }
    }
}

/// JSON-RPC request message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl JsonRpcRequest {
    pub fn new(method: impl Into<String>, params: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Uuid::new_v4().to_string(),
            method: method.into(),
            params,
        }
    }

    pub fn with_id(
        id: impl Into<String>,
        method: impl Into<String>,
        params: Option<serde_json::Value>,
    ) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: id.into(),
            method: method.into(),
            params,
        }
    }
}

/// JSON-RPC response message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn ok(id: impl Into<String>, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: id.into(),
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: impl Into<String>, error: JsonRpcError) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: id.into(),
            result: None,
            error: Some(error),
        }
    }

    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }
}

/// JSON-RPC error object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    pub fn with_data(code: i32, message: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            code,
            message: message.into(),
            data: Some(data),
        }
    }

    // Standard error codes
    pub fn parse_error() -> Self {
        Self::new(-32700, "Parse error")
    }

    pub fn invalid_request() -> Self {
        Self::new(-32600, "Invalid Request")
    }

    pub fn method_not_found() -> Self {
        Self::new(-32601, "Method not found")
    }

    pub fn invalid_params() -> Self {
        Self::new(-32602, "Invalid params")
    }

    pub fn internal_error() -> Self {
        Self::new(-32603, "Internal error")
    }
}

/// JSON-RPC notification message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl JsonRpcNotification {
    pub fn new(method: impl Into<String>, params: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: method.into(),
            params,
        }
    }
}

/// MCP-specific request methods
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum RequestMethod {
    Initialize,
    ToolsList,
    ToolsCall,
    ResourcesList,
    ResourcesRead,
    ResourcesSubscribe,
    ResourcesUnsubscribe,
    ResourcesTemplatesList,
    PromptsList,
    PromptsGet,
    PromptsArgumentsList,
}

/// MCP-specific response methods (for type safety)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResponseMethod {
    Initialize,
    ToolsList,
    ToolsCall,
    ResourcesList,
    ResourcesRead,
    ResourcesSubscribe,
    ResourcesUnsubscribe,
    ResourcesTemplatesList,
    PromptsList,
    PromptsGet,
    PromptsArgumentsList,
}

/// MCP-specific notification methods
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NotificationMethod {
    NotificationsMessage,
    NotificationsResourcesUpdated,
    NotificationsResourcesListChanged,
    NotificationsToolsListChanged,
    NotificationsPromptsListChanged,
    LoggingMessage,
    Progress,
}

/// Typed MCP request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRequest {
    pub method: RequestMethod,
    pub params: serde_json::Value,
}

/// Typed MCP response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResponse {
    pub method: ResponseMethod,
    pub result: serde_json::Value,
}

/// Typed MCP notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpNotification {
    pub method: NotificationMethod,
    pub params: serde_json::Value,
}

/// Tool annotations providing behavioral hints about a tool.
///
/// Servers use annotations to communicate how a tool behaves so that clients
/// can make smarter decisions about permissions, batching, and UI presentation.
/// All fields default to `false` when absent.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolAnnotations {
    /// If true, the tool only performs read operations (no side effects).
    #[serde(default)]
    pub read_only_hint: bool,
    /// If true, the tool may perform destructive (irreversible) operations.
    #[serde(default)]
    pub destructive_hint: bool,
    /// If true, calling the tool multiple times with the same arguments
    /// produces the same result.
    #[serde(default)]
    pub idempotent_hint: bool,
    /// If true, the tool may interact with external entities (network, APIs)
    /// beyond the server's own resources.
    #[serde(default)]
    pub open_world_hint: bool,
}

/// Tool definition
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<ToolAnnotations>,
}

/// Resource definition
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resource {
    pub uri: String,
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// Prompt definition
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Vec<PromptArgument>>,
}

/// Prompt argument
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptArgument {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
}

/// Tool call result content
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolContent {
    pub content: Vec<ContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

/// Content block (text, image, or embedded resource)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Image {
        data: String,
        mime_type: String,
    },
    Resource {
        uri: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },
}

/// Resource content
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceContent {
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    pub contents: Vec<ContentBlock>,
}

/// MCP server capabilities
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServerCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolsCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourcesCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<PromptsCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logging: Option<LoggingCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completions: Option<CompletionsCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CompletionsCapability {}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolsCapability {
    pub list_changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ResourcesCapability {
    pub subscribe: bool,
    pub list_changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PromptsCapability {
    pub list_changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LoggingCapability {
    pub level: String,
}

/// MCP client capabilities (advertised during initialization)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sampling: Option<SamplingCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourcesCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roots: Option<RootsCapability>,
}

/// Client capability for exposing filesystem roots to servers.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RootsCapability {
    /// Whether the list of roots may change dynamically.
    #[serde(default)]
    pub list_changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SamplingCapability {}

// ---------------------------------------------------------------------------
// Sampling (server→client LLM requests)
// ---------------------------------------------------------------------------

/// Role of a message author in a sampling conversation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SamplingMessageRole {
    User,
    Assistant,
}

/// A message in a sampling conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SamplingMessage {
    pub role: SamplingMessageRole,
    pub content: SamplingContent,
}

/// Content of a sampling message (text or image).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum SamplingContent {
    Text { text: String },
    Image { data: String, mime_type: String },
}

/// Hint about the model priority for a sampling request.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModelHint {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Server request to create a message via the client's LLM.
///
/// Spec: <https://spec.modelcontextprotocol.io/specification/2024-11-05/basic/utilities/sampling/>
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateMessageRequest {
    /// Conversation messages provided by the server.
    pub messages: Vec<SamplingMessage>,
    /// Optional model selection hints.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_preferences: Option<ModelPreferences>,
    /// System prompt the server wants to use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    /// Requested context window (in tokens).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Sampling parameters.
    #[serde(flatten)]
    pub sampling_params: SamplingParams,
}

/// Model selection preferences.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPreferences {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hints: Option<Vec<ModelHint>>,
    /// Cost priority (0–1, higher = prioritize cost).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_priority: Option<f64>,
    /// Speed priority (0–1, higher = prioritize speed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_priority: Option<f64>,
    /// Intelligence priority (0–1, higher = prioritize quality).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intelligence_priority: Option<f64>,
}

/// Common sampling parameters for LLM requests.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SamplingParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
}

/// Reason why sampling stopped.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum StopReason {
    EndTurn,
    StopSequence,
    MaxTokens,
}

/// Result of a `sampling/createMessage` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateMessageResult {
    pub role: SamplingMessageRole,
    pub model: String,
    pub content: SamplingContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<StopReason>,
}

/// Combined capabilities type for convenience
pub type McpCapabilities = ServerCapabilities;

/// Initialize request parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub protocol_version: String,
    pub capabilities: ClientCapabilities,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_info: Option<ClientInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

/// Initialize result
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: String,
    pub capabilities: ServerCapabilities,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_info: Option<ServerInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

/// Tool call parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallParams {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<serde_json::Value>,
}

/// List tools result
pub type ListToolsResult = Vec<Tool>;

/// List resources result
pub type ListResourcesResult = Vec<Resource>;

/// List prompts result
pub type ListPromptsResult = Vec<Prompt>;

/// Resource template
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceTemplate {
    pub uri_template: String,
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// List resource templates result
pub type ListResourceTemplatesResult = Vec<ResourceTemplate>;

/// Completion request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionRequest {
    #[serde(rename = "ref")]
    pub reference: CompletionRef,
    pub argument: PromptArgument,
}

/// Reference to a completion
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionRef {
    #[serde(rename = "type")]
    pub ref_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Completion result
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionResult {
    pub completion: Completion,
}

/// Completion values
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Completion {
    pub values: Vec<CompletionValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_more: Option<bool>,
}

/// A completion value suggestion
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionValue {
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Set level for logging
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum LoggingLevel {
    Debug,
    Info,
    Notice,
    Warning,
    Error,
    Critical,
    Alert,
    Emergency,
}

/// Set logging level request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetLevelRequest {
    pub level: LoggingLevel,
}

/// Subscribe to resource request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscribeRequest {
    pub uri: String,
}

/// Unsubscribe from resource request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsubscribeRequest {
    pub uri: String,
}

/// Subscription result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscribeResult {
    pub subscribed: bool,
}

/// Notification sent by the server when a subscribed resource changes.
///
/// Spec: <https://spec.modelcontextprotocol.io/specification/2024-11-05/server/resources/#notificationsresourcesupdated>
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourcesUpdatedNotification {
    /// URI of the resource that was updated.
    pub uri: String,
    /// Optional updated content or metadata about the change.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<serde_json::Value>,
}

/// Progress token used to correlate progress notifications with requests.
///
/// The client includes this in `_meta.progressToken` of a request. The server
/// then sends `notifications/progress` using the same token.
pub type ProgressToken = serde_json::Value;

/// Progress notification params sent by the server during long-running operations.
///
/// Spec: <https://spec.modelcontextprotocol.io/specification/2024-11-05/basic/utilities/progress/>
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressNotification {
    /// The progress token matching the one sent in the request `_meta`.
    pub progress_token: ProgressToken,
    /// Current progress value (monotonically increasing).
    pub progress: f64,
    /// Optional total value; when present, `progress / total` gives a fraction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<f64>,
}

/// A filesystem root that the client exposes to the server.
///
/// Servers can request the list of roots via `roots/list` to understand the
/// workspace layout. Each root has a URI and an optional human-readable name.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Root {
    /// URI for the root directory (e.g. `file:///home/user/project`).
    pub uri: String,
    /// Optional human-readable name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Result of a `roots/list` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRootsResult {
    pub roots: Vec<Root>,
}

// ---------------------------------------------------------------------------
// Elicitation (server → client user prompts)
// ---------------------------------------------------------------------------

/// Request from server to elicit information from the user.
///
/// Spec: <https://spec.modelcontextprotocol.io/specification/2024-11-05/basic/utilities/elicitation/>
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElicitationRequest {
    /// Human-readable message to present to the user.
    pub message: String,
    /// Optional JSON Schema describing the requested input structure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_schema: Option<serde_json::Value>,
}

/// Result of an elicitation request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElicitationResult {
    /// The action the user took.
    pub action: ElicitationAction,
    /// The user's input, present only when action is `Accept`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<serde_json::Value>,
}

/// User response to an elicitation prompt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ElicitationAction {
    /// User accepted and provided input.
    Accept,
    /// User declined to provide input.
    Decline,
    /// Request was cancelled (e.g. timeout, dismiss).
    Cancel,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── JsonRpcRequest ─────────────────────────────────────────────────

    #[test]
    fn jsonrpc_request_new() {
        let req = JsonRpcRequest::new("tools/list", None);
        assert_eq!(req.jsonrpc, "2.0");
        assert_eq!(req.method, "tools/list");
        assert!(req.params.is_none());
        assert!(!req.id.is_empty());
    }

    #[test]
    fn jsonrpc_request_with_id() {
        let req =
            JsonRpcRequest::with_id("42", "tools/call", Some(serde_json::json!({"name": "x"})));
        assert_eq!(req.id, "42");
        assert_eq!(req.method, "tools/call");
        assert!(req.params.is_some());
    }

    #[test]
    fn jsonrpc_request_roundtrip() {
        let req = JsonRpcRequest::with_id("1", "ping", None);
        let json = serde_json::to_string(&req).unwrap();
        let de: JsonRpcRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(de.id, "1");
        assert_eq!(de.method, "ping");
    }

    // ── JsonRpcResponse ────────────────────────────────────────────────

    #[test]
    fn jsonrpc_response_ok() {
        let res = JsonRpcResponse::ok("1", serde_json::json!({"tools": []}));
        assert!(!res.is_error());
        assert!(res.result.is_some());
        assert!(res.error.is_none());
    }

    #[test]
    fn jsonrpc_response_error() {
        let res = JsonRpcResponse::error("2", JsonRpcError::method_not_found());
        assert!(res.is_error());
        assert!(res.error.is_some());
        assert!(res.result.is_none());
    }

    #[test]
    fn jsonrpc_response_roundtrip() {
        let res = JsonRpcResponse::ok("1", serde_json::json!(true));
        let json = serde_json::to_string(&res).unwrap();
        let de: JsonRpcResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(de.id, "1");
        assert!(!de.is_error());
    }

    // ── JsonRpcError ───────────────────────────────────────────────────

    #[test]
    fn jsonrpc_error_standard_codes() {
        assert_eq!(JsonRpcError::parse_error().code, -32700);
        assert_eq!(JsonRpcError::invalid_request().code, -32600);
        assert_eq!(JsonRpcError::method_not_found().code, -32601);
        assert_eq!(JsonRpcError::invalid_params().code, -32602);
        assert_eq!(JsonRpcError::internal_error().code, -32603);
    }

    #[test]
    fn jsonrpc_error_with_data() {
        let err = JsonRpcError::with_data(-32000, "custom", serde_json::json!({"detail": "x"}));
        assert_eq!(err.code, -32000);
        assert!(err.data.is_some());
    }

    #[test]
    fn jsonrpc_error_roundtrip() {
        let err = JsonRpcError::new(-1, "test error");
        let json = serde_json::to_string(&err).unwrap();
        let de: JsonRpcError = serde_json::from_str(&json).unwrap();
        assert_eq!(de.code, -1);
        assert_eq!(de.message, "test error");
    }

    // ── JsonRpcNotification ────────────────────────────────────────────

    #[test]
    fn jsonrpc_notification_new() {
        let n = JsonRpcNotification::new("progress", Some(serde_json::json!({"p": 0.5})));
        assert_eq!(n.jsonrpc, "2.0");
        assert_eq!(n.method, "progress");
        assert!(n.params.is_some());
    }

    #[test]
    fn jsonrpc_notification_no_id() {
        let msg = JsonRpcMessage::Notification(JsonRpcNotification::new("ping", None));
        assert!(msg.id().is_none());
    }

    // ── JsonRpcMessage id() ────────────────────────────────────────────

    #[test]
    fn jsonrpc_message_request_id() {
        let msg = JsonRpcMessage::Request(JsonRpcRequest::with_id("abc", "test", None));
        assert_eq!(msg.id(), Some("abc"));
    }

    #[test]
    fn jsonrpc_message_response_id() {
        let msg = JsonRpcMessage::Response(JsonRpcResponse::ok("xyz", serde_json::json!(null)));
        assert_eq!(msg.id(), Some("xyz"));
    }

    // ── RequestMethod camelCase serde ──────────────────────────────────

    #[test]
    fn request_method_serde() {
        let methods = vec![
            RequestMethod::Initialize,
            RequestMethod::ToolsList,
            RequestMethod::ToolsCall,
            RequestMethod::ResourcesList,
            RequestMethod::ResourcesRead,
        ];
        let json = serde_json::to_string(&methods).unwrap();
        assert!(json.contains("toolsList"));
        assert!(json.contains("toolsCall"));
        assert!(json.contains("resourcesList"));
        let de: Vec<RequestMethod> = serde_json::from_str(&json).unwrap();
        assert_eq!(de, methods);
    }

    #[test]
    fn response_method_camelcase() {
        let json = serde_json::to_string(&ResponseMethod::ToolsList).unwrap();
        assert_eq!(json, "\"toolsList\"");
    }

    #[test]
    fn notification_method_camelcase() {
        let json = serde_json::to_string(&NotificationMethod::Progress).unwrap();
        assert_eq!(json, "\"progress\"");
        let json =
            serde_json::to_string(&NotificationMethod::NotificationsToolsListChanged).unwrap();
        assert!(json.contains("ToolsListChanged"));
    }

    // ── ToolAnnotations ────────────────────────────────────────────────

    #[test]
    fn tool_annotations_default_all_false() {
        let ann = ToolAnnotations::default();
        assert!(!ann.read_only_hint);
        assert!(!ann.destructive_hint);
        assert!(!ann.idempotent_hint);
        assert!(!ann.open_world_hint);
    }

    #[test]
    fn tool_annotations_equality() {
        let a = ToolAnnotations {
            read_only_hint: true,
            ..Default::default()
        };
        let b = ToolAnnotations {
            read_only_hint: true,
            ..Default::default()
        };
        assert_eq!(a, b);
    }

    #[test]
    fn tool_annotations_roundtrip() {
        let ann = ToolAnnotations {
            read_only_hint: true,
            destructive_hint: false,
            idempotent_hint: true,
            open_world_hint: false,
        };
        let json = serde_json::to_string(&ann).unwrap();
        assert!(json.contains("readOnlyHint"));
        assert!(json.contains("idempotentHint"));
        let de: ToolAnnotations = serde_json::from_str(&json).unwrap();
        assert_eq!(de, ann);
    }

    // ── Tool / Resource / Prompt ───────────────────────────────────────

    #[test]
    fn tool_roundtrip() {
        let tool = Tool {
            name: "fetch".to_string(),
            description: "Fetch URL".to_string(),
            input_schema: Some(serde_json::json!({"type": "object"})),
            annotations: Some(ToolAnnotations {
                read_only_hint: true,
                ..Default::default()
            }),
        };
        let json = serde_json::to_string(&tool).unwrap();
        let de: Tool = serde_json::from_str(&json).unwrap();
        assert_eq!(de.name, "fetch");
        assert!(de.annotations.unwrap().read_only_hint);
    }

    #[test]
    fn resource_roundtrip() {
        let res = Resource {
            uri: "file:///x".to_string(),
            name: "x".to_string(),
            description: "desc".to_string(),
            mime_type: Some("text/plain".to_string()),
        };
        let json = serde_json::to_string(&res).unwrap();
        let de: Resource = serde_json::from_str(&json).unwrap();
        assert_eq!(de.uri, "file:///x");
    }

    #[test]
    fn prompt_roundtrip() {
        let prompt = Prompt {
            name: "review".to_string(),
            description: "Code review".to_string(),
            arguments: Some(vec![PromptArgument {
                name: "file".to_string(),
                description: "File to review".to_string(),
                required: Some(true),
            }]),
        };
        let json = serde_json::to_string(&prompt).unwrap();
        let de: Prompt = serde_json::from_str(&json).unwrap();
        assert_eq!(de.arguments.unwrap().len(), 1);
    }

    // ── ContentBlock ───────────────────────────────────────────────────

    #[test]
    fn content_block_text() {
        let block = ContentBlock::Text {
            text: "hello".to_string(),
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("\"type\":\"text\""));
        let de: ContentBlock = serde_json::from_str(&json).unwrap();
        if let ContentBlock::Text { text } = de {
            assert_eq!(text, "hello");
        } else {
            panic!("Expected Text variant");
        }
    }

    #[test]
    fn content_block_image() {
        let block = ContentBlock::Image {
            data: "abc".to_string(),
            mime_type: "image/png".to_string(),
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("\"type\":\"image\""));
    }

    // ── Capabilities ───────────────────────────────────────────────────

    #[test]
    fn server_capabilities_default() {
        let caps = ServerCapabilities::default();
        assert!(caps.tools.is_none());
        assert!(caps.resources.is_none());
    }

    #[test]
    fn client_capabilities_default() {
        let caps = ClientCapabilities::default();
        assert!(caps.experimental.is_none());
        assert!(caps.sampling.is_none());
    }

    #[test]
    fn server_capabilities_with_tools() {
        let caps = ServerCapabilities {
            tools: Some(ToolsCapability { list_changed: true }),
            ..Default::default()
        };
        let json = serde_json::to_string(&caps).unwrap();
        assert!(json.contains("listChanged"));
    }

    // ── InitializeParams / InitializeResult ────────────────────────────

    #[test]
    fn initialize_params_roundtrip() {
        let params = InitializeParams {
            protocol_version: "2024-11-05".to_string(),
            capabilities: ClientCapabilities::default(),
            client_info: Some(ClientInfo {
                name: "shannon".to_string(),
                version: "0.1".to_string(),
            }),
        };
        let json = serde_json::to_string(&params).unwrap();
        let de: InitializeParams = serde_json::from_str(&json).unwrap();
        assert_eq!(de.protocol_version, "2024-11-05");
        assert_eq!(de.client_info.unwrap().name, "shannon");
    }

    #[test]
    fn initialize_result_roundtrip() {
        let result = InitializeResult {
            protocol_version: "2024-11-05".to_string(),
            capabilities: ServerCapabilities::default(),
            server_info: Some(ServerInfo {
                name: "test-server".to_string(),
                version: "1.0".to_string(),
            }),
        };
        let json = serde_json::to_string(&result).unwrap();
        let de: InitializeResult = serde_json::from_str(&json).unwrap();
        assert_eq!(de.server_info.unwrap().name, "test-server");
    }

    // ── Sampling types ─────────────────────────────────────────────────

    #[test]
    fn sampling_message_role_lowercase() {
        let json = serde_json::to_string(&SamplingMessageRole::User).unwrap();
        assert_eq!(json, "\"user\"");
        let de: SamplingMessageRole = serde_json::from_str("\"assistant\"").unwrap();
        assert_eq!(de, SamplingMessageRole::Assistant);
    }

    #[test]
    fn sampling_content_text_roundtrip() {
        let content = SamplingContent::Text {
            text: "hi".to_string(),
        };
        let json = serde_json::to_string(&content).unwrap();
        let de: SamplingContent = serde_json::from_str(&json).unwrap();
        if let SamplingContent::Text { text } = de {
            assert_eq!(text, "hi");
        } else {
            panic!("Expected Text");
        }
    }

    #[test]
    fn stop_reason_roundtrip() {
        let reasons = vec![
            StopReason::EndTurn,
            StopReason::StopSequence,
            StopReason::MaxTokens,
        ];
        let json = serde_json::to_string(&reasons).unwrap();
        let de: Vec<StopReason> = serde_json::from_str(&json).unwrap();
        assert_eq!(de, reasons);
    }

    // ── LoggingLevel ───────────────────────────────────────────────────

    #[test]
    fn logging_level_lowercase() {
        let json = serde_json::to_string(&LoggingLevel::Error).unwrap();
        assert_eq!(json, "\"error\"");
        let de: LoggingLevel = serde_json::from_str("\"debug\"").unwrap();
        assert_eq!(de, LoggingLevel::Debug);
    }

    // ── ToolCallParams ─────────────────────────────────────────────────

    #[test]
    fn tool_call_params_roundtrip() {
        let params = ToolCallParams {
            name: "fetch".to_string(),
            arguments: Some(serde_json::json!({"url": "http://example.com"})),
        };
        let json = serde_json::to_string(&params).unwrap();
        let de: ToolCallParams = serde_json::from_str(&json).unwrap();
        assert_eq!(de.name, "fetch");
    }

    // ── Elicitation ────────────────────────────────────────────────────

    #[test]
    fn elicitation_action_serde() {
        let json = serde_json::to_string(&ElicitationAction::Accept).unwrap();
        assert_eq!(json, "\"accept\"");
        let de: ElicitationAction = serde_json::from_str("\"decline\"").unwrap();
        assert_eq!(de, ElicitationAction::Decline);
        let de: ElicitationAction = serde_json::from_str("\"cancel\"").unwrap();
        assert_eq!(de, ElicitationAction::Cancel);
    }

    #[test]
    fn elicitation_result_roundtrip() {
        let result = ElicitationResult {
            action: ElicitationAction::Accept,
            content: Some(serde_json::json!({"name": "value"})),
        };
        let json = serde_json::to_string(&result).unwrap();
        let de: ElicitationResult = serde_json::from_str(&json).unwrap();
        assert_eq!(de.action, ElicitationAction::Accept);
    }

    // ── Progress ───────────────────────────────────────────────────────

    #[test]
    fn progress_notification_roundtrip() {
        let notif = ProgressNotification {
            progress_token: serde_json::json!("tok-1"),
            progress: 0.5,
            total: Some(1.0),
        };
        let json = serde_json::to_string(&notif).unwrap();
        let de: ProgressNotification = serde_json::from_str(&json).unwrap();
        assert_eq!(de.progress, 0.5);
        assert_eq!(de.total, Some(1.0));
    }

    // ── Roots ──────────────────────────────────────────────────────────

    #[test]
    fn list_roots_result_roundtrip() {
        let result = ListRootsResult {
            roots: vec![Root {
                uri: "file:///home".to_string(),
                name: Some("home".to_string()),
            }],
        };
        let json = serde_json::to_string(&result).unwrap();
        let de: ListRootsResult = serde_json::from_str(&json).unwrap();
        assert_eq!(de.roots.len(), 1);
    }

    // ── McpRequest / McpResponse / McpNotification ─────────────────────

    #[test]
    fn mcp_request_roundtrip() {
        let req = McpRequest {
            method: RequestMethod::ToolsList,
            params: serde_json::json!({}),
        };
        let json = serde_json::to_string(&req).unwrap();
        let de: McpRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(de.method, RequestMethod::ToolsList));
    }

    // ── Send+Sync ──────────────────────────────────────────────────────

    #[test]
    fn send_sync_types() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<JsonRpcRequest>();
        assert_send_sync::<JsonRpcResponse>();
        assert_send_sync::<JsonRpcError>();
        assert_send_sync::<JsonRpcNotification>();
        assert_send_sync::<Tool>();
        assert_send_sync::<ToolAnnotations>();
        assert_send_sync::<ContentBlock>();
        assert_send_sync::<ServerCapabilities>();
        assert_send_sync::<ClientCapabilities>();
        assert_send_sync::<InitializeParams>();
        assert_send_sync::<InitializeResult>();
        assert_send_sync::<ElicitationAction>();
        assert_send_sync::<ProgressNotification>();
    }
}
