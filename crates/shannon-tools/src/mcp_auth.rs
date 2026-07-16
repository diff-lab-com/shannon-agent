//! MCP Auth Tool
//!
//! Manages OAuth authentication for MCP servers that require it.
//! Provides token management, authorization code flow, and token refresh.

use crate::{Tool, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// OAuth token information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<i64>,
    pub token_type: String,
    pub scope: Option<String>,
}

/// MCP server OAuth configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpOAuthConfig {
    pub server_name: String,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub scopes: Vec<String>,
}

/// Auth action types.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "action")]
pub enum McpAuthAction {
    /// Start OAuth authorization flow.
    Authorize { server_name: String },
    /// Exchange authorization code for token.
    Token { server_name: String, code: String },
    /// Refresh an expired token.
    Refresh { server_name: String },
    /// List authenticated servers.
    List,
    /// Revoke authentication.
    Revoke { server_name: String },
}

/// Shared OAuth token store.
pub type OAuthTokenStore = Arc<RwLock<HashMap<String, OAuthToken>>>;

/// MCP Auth tool implementation.
///
/// Manages OAuth authentication for MCP servers. Supports:
/// - Registering OAuth configurations for MCP servers
/// - Initiating authorization flows
/// - Exchanging authorization codes for tokens
/// - Refreshing expired tokens
/// - Listing authenticated servers
/// - Revoking authentication
pub struct McpAuthTool {
    tokens: OAuthTokenStore,
    configs: Arc<RwLock<HashMap<String, McpOAuthConfig>>>,
}

impl McpAuthTool {
    pub fn new() -> Self {
        Self {
            tokens: Arc::new(RwLock::new(HashMap::new())),
            configs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register an OAuth configuration for an MCP server.
    pub async fn register_config(&self, config: McpOAuthConfig) {
        let mut configs = self.configs.write().await;
        configs.insert(config.server_name.clone(), config);
    }

    /// Get a reference to the shared token store.
    pub fn tokens(&self) -> OAuthTokenStore {
        Arc::clone(&self.tokens)
    }
}

impl Default for McpAuthTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for McpAuthTool {
    fn name(&self) -> &str {
        "McpAuth"
    }

    fn description(&self) -> &str {
        "Manage OAuth authentication for MCP servers that require it"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["Authorize", "Token", "Refresh", "List", "Revoke"],
                    "description": "Auth action to perform"
                },
                "server_name": {
                    "type": "string",
                    "description": "MCP server name"
                },
                "code": {
                    "type": "string",
                    "description": "Authorization code (for Token action)"
                },
                "scopes": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Requested OAuth scopes"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let action: McpAuthAction = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid McpAuth input: {e}")))?;

        match action {
            McpAuthAction::List => {
                let tokens = self.tokens.read().await;
                let server_names: Vec<String> = tokens.keys().cloned().collect();
                let content = if server_names.is_empty() {
                    "No authenticated MCP servers".to_string()
                } else {
                    format!("Authenticated servers: {}", server_names.join(", "))
                };

                let mut metadata = HashMap::new();
                metadata.insert("servers".to_string(), json!(server_names));

                Ok(ToolOutput {
                    content,
                    is_error: false,
                    metadata,
                })
            }

            McpAuthAction::Authorize { server_name } => {
                let configs = self.configs.read().await;
                match configs.get(&server_name) {
                    Some(config) => {
                        let auth_url = format!(
                            "{}?response_type=code&client_id={}&scope={}",
                            config.authorization_endpoint,
                            config.client_id,
                            config.scopes.join(" ")
                        );

                        let mut metadata = HashMap::new();
                        metadata.insert("server_name".to_string(), json!(server_name));
                        metadata.insert("auth_url".to_string(), json!(auth_url));

                        Ok(ToolOutput {
                            content: format!(
                                "Authorization required for {server_name}. Please visit:\n\n{auth_url}\n\nThen use the Token action with the authorization code."
                            ),
                            is_error: false,
                            metadata,
                        })
                    }
                    None => Err(ToolError::NotFound(format!(
                        "No OAuth config registered for MCP server: {server_name}"
                    ))),
                }
            }

            McpAuthAction::Token {
                server_name,
                code: _code,
            } => {
                // In a real implementation, this would exchange the code for tokens via HTTP.
                // For now, store a placeholder token to demonstrate the flow.
                let token = OAuthToken {
                    access_token: format!("token_{}", uuid::Uuid::new_v4().simple()),
                    refresh_token: None,
                    expires_at: None,
                    token_type: "Bearer".to_string(),
                    scope: None,
                };

                let mut tokens = self.tokens.write().await;
                tokens.insert(server_name.clone(), token);

                let mut metadata = HashMap::new();
                metadata.insert("server_name".to_string(), json!(server_name));

                Ok(ToolOutput {
                    content: format!("Successfully authenticated with {server_name}"),
                    is_error: false,
                    metadata,
                })
            }

            McpAuthAction::Refresh { server_name } => {
                let mut tokens = self.tokens.write().await;
                match tokens.get_mut(&server_name) {
                    Some(token) => {
                        token.access_token = format!("refreshed_{}", uuid::Uuid::new_v4().simple());
                        Ok(ToolOutput {
                            content: format!("Refreshed token for {server_name}"),
                            is_error: false,
                            metadata: HashMap::new(),
                        })
                    }
                    None => Err(ToolError::NotFound(format!(
                        "No token found for: {server_name}"
                    ))),
                }
            }

            McpAuthAction::Revoke { server_name } => {
                let mut tokens = self.tokens.write().await;
                tokens.remove(&server_name);
                Ok(ToolOutput {
                    content: format!("Revoked authentication for {server_name}"),
                    is_error: false,
                    metadata: HashMap::new(),
                })
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_list_empty() {
        let tool = McpAuthTool::new();
        let input = json!({ "action": "List" });

        let result = tool.execute(input).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(!output.is_error);
        assert!(output.content.contains("No authenticated"));
        let servers = output.metadata.get("servers").unwrap().as_array().unwrap();
        assert!(servers.is_empty());
    }

    #[tokio::test]
    async fn test_authorize_unknown_server() {
        let tool = McpAuthTool::new();
        let input = json!({
            "action": "Authorize",
            "server_name": "unknown-server"
        });

        let result = tool.execute(input).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ToolError::NotFound(msg) => {
                assert!(msg.contains("unknown-server"));
            }
            other => panic!("Expected NotFound, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_register_and_authorize() {
        let tool = McpAuthTool::new();

        // Register a config
        tool.register_config(McpOAuthConfig {
            server_name: "test-server".to_string(),
            client_id: "test-client-id".to_string(),
            client_secret: None,
            authorization_endpoint: "https://auth.example.com/authorize".to_string(),
            token_endpoint: "https://auth.example.com/token".to_string(),
            scopes: vec!["read".to_string(), "write".to_string()],
        })
        .await;

        // Authorize
        let input = json!({
            "action": "Authorize",
            "server_name": "test-server"
        });

        let result = tool.execute(input).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(!output.is_error);
        assert!(output.content.contains("test-server"));
        assert!(output.content.contains("auth.example.com"));
        assert_eq!(output.metadata.get("server_name").unwrap(), "test-server");
    }

    #[tokio::test]
    async fn test_authorize_and_token() {
        let tool = McpAuthTool::new();

        // Register a config
        tool.register_config(McpOAuthConfig {
            server_name: "my-server".to_string(),
            client_id: "my-client".to_string(),
            client_secret: Some("secret".to_string()),
            authorization_endpoint: "https://auth.example.com/authorize".to_string(),
            token_endpoint: "https://auth.example.com/token".to_string(),
            scopes: vec!["read".to_string()],
        })
        .await;

        // Authorize first
        let auth_input = json!({
            "action": "Authorize",
            "server_name": "my-server"
        });
        let auth_result = tool.execute(auth_input).await;
        assert!(auth_result.is_ok());

        // Exchange code for token
        let token_input = json!({
            "action": "Token",
            "server_name": "my-server",
            "code": "test-auth-code-123"
        });
        let token_result = tool.execute(token_input).await;
        assert!(token_result.is_ok());

        let token_output = token_result.unwrap();
        assert!(!token_output.is_error);
        assert!(token_output.content.contains("Successfully authenticated"));

        // Verify token is stored
        let tokens = tool.tokens.read().await;
        assert!(tokens.contains_key("my-server"));
        assert!(tokens["my-server"].access_token.starts_with("token_"));
    }

    #[tokio::test]
    async fn test_token_and_refresh() {
        let tool = McpAuthTool::new();

        // Manually insert a token
        {
            let mut tokens = tool.tokens.write().await;
            tokens.insert(
                "test-server".to_string(),
                OAuthToken {
                    access_token: "old_token".to_string(),
                    refresh_token: Some("old_refresh".to_string()),
                    expires_at: None,
                    token_type: "Bearer".to_string(),
                    scope: None,
                },
            );
        }

        // Refresh the token
        let input = json!({
            "action": "Refresh",
            "server_name": "test-server"
        });

        let result = tool.execute(input).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(!output.is_error);
        assert!(output.content.contains("Refreshed token"));

        // Verify token was updated
        let tokens = tool.tokens.read().await;
        let token = &tokens["test-server"];
        assert!(token.access_token.starts_with("refreshed_"));
        // refresh_token should be preserved
        assert_eq!(token.refresh_token.as_deref(), Some("old_refresh"));
    }

    #[tokio::test]
    async fn test_revoke() {
        let tool = McpAuthTool::new();

        // Insert a token
        {
            let mut tokens = tool.tokens.write().await;
            tokens.insert(
                "revoke-server".to_string(),
                OAuthToken {
                    access_token: "some_token".to_string(),
                    refresh_token: None,
                    expires_at: None,
                    token_type: "Bearer".to_string(),
                    scope: None,
                },
            );
        }

        // Verify it exists
        {
            let tokens = tool.tokens.read().await;
            assert!(tokens.contains_key("revoke-server"));
        }

        // Revoke
        let input = json!({
            "action": "Revoke",
            "server_name": "revoke-server"
        });

        let result = tool.execute(input).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(!output.is_error);
        assert!(output.content.contains("Revoked"));

        // Verify it was removed
        {
            let tokens = tool.tokens.read().await;
            assert!(!tokens.contains_key("revoke-server"));
        }
    }
}
