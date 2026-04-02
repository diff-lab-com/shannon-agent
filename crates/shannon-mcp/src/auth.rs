// Authentication mechanisms for MCP servers
//
// This module provides authentication implementations including
// OAuth 2.0 PKCE and API key authentication.

use crate::{McpError, McpResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use tracing::{debug, info, warn};

/// Authentication error types
#[derive(Error, Debug)]
pub enum AuthError {
    #[error("OAuth error: {0}")]
    OAuth(String),

    #[error("invalid token: {0}")]
    InvalidToken(String),

    #[error("authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("token expired")]
    TokenExpired,

    #[error("configuration error: {0}")]
    Configuration(String),
}

/// Authentication provider trait
#[async_trait]
pub trait AuthProvider: Send + Sync {
    /// Get the current authentication token
    async fn get_token(&self) -> Result<String, AuthError>;

    /// Refresh the authentication token if possible
    async fn refresh_token(&self) -> Result<(), AuthError>;

    /// Check if the current token is valid
    async fn is_valid(&self) -> bool;

    /// Add authentication headers to a request
    async fn add_auth_headers(&self, headers: &mut HashMap<String, String>) -> Result<(), AuthError>;
}

/// OAuth 2.0 PKCE authentication provider (stub implementation)
pub struct OAuth2Provider {
    client_id: String,
    client_secret: Option<String>,
    auth_url: String,
    token_url: String,
    redirect_url: String,
    scopes: Vec<String>,
    access_token: Option<String>,
    refresh_token_val: Option<String>,
}

impl OAuth2Provider {
    /// Create a new OAuth 2.0 provider
    pub fn new(
        client_id: impl Into<String>,
        auth_url: impl Into<String>,
        token_url: impl Into<String>,
        redirect_url: impl Into<String>,
    ) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: None,
            auth_url: auth_url.into(),
            token_url: token_url.into(),
            redirect_url: redirect_url.into(),
            scopes: Vec::new(),
            access_token: None,
            refresh_token_val: None,
        }
    }

    /// Set the client secret (for confidential clients)
    pub fn with_client_secret(mut self, secret: impl Into<String>) -> Self {
        self.client_secret = Some(secret.into());
        self
    }

    /// Add OAuth scopes
    pub fn with_scopes(mut self, scopes: Vec<String>) -> Self {
        self.scopes = scopes;
        self
    }

    /// Generate the authorization URL with PKCE
    pub fn get_authorization_url(&mut self) -> Result<(String, String), AuthError> {
        // Stub implementation - returns a mock URL
        let auth_url = format!("{}?client_id={}&redirect_uri={}",
            self.auth_url, self.client_id, self.redirect_url);
        let csrf_token = "mock_csrf_token".to_string();
        Ok((auth_url, csrf_token))
    }

    /// Exchange authorization code for access token
    pub async fn exchange_code(&mut self, _code: &str, _state: &str) -> Result<String, AuthError> {
        // Stub implementation - returns mock token
        self.access_token = Some("mock_access_token".to_string());
        Ok("mock_access_token".to_string())
    }

    /// Refresh the access token using refresh token
    pub async fn refresh_access_token(&mut self) -> Result<String, AuthError> {
        // Stub implementation - returns mock token
        self.access_token = Some("mock_refreshed_token".to_string());
        Ok("mock_refreshed_token".to_string())
    }
}

#[async_trait]
impl AuthProvider for OAuth2Provider {
    async fn get_token(&self) -> Result<String, AuthError> {
        self.access_token
            .clone()
            .ok_or_else(|| AuthError::TokenExpired)
    }

    async fn refresh_token(&self) -> Result<(), AuthError> {
        warn!("Token refresh requested in stub provider");
        Ok(())
    }

    async fn is_valid(&self) -> bool {
        self.access_token.is_some()
    }

    async fn add_auth_headers(&self, headers: &mut HashMap<String, String>) -> Result<(), AuthError> {
        if let Some(token) = &self.access_token {
            headers.insert("Authorization".to_string(), format!("Bearer {}", token));
            Ok(())
        } else {
            Err(AuthError::InvalidToken("No token available".to_string()))
        }
    }
}

/// API Key authentication provider
pub struct ApiKeyProvider {
    api_key: String,
    header_name: Option<String>,
    prefix: Option<String>,
}

impl ApiKeyProvider {
    /// Create a new API key provider
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            header_name: None,
            prefix: None,
        }
    }

    /// Set the header name (default: "X-API-Key")
    pub fn with_header_name(mut self, name: impl Into<String>) -> Self {
        self.header_name = Some(name.into());
        self
    }

    /// Set the key prefix (e.g., "Bearer")
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }
}

#[async_trait]
impl AuthProvider for ApiKeyProvider {
    async fn get_token(&self) -> Result<String, AuthError> {
        Ok(self.api_key.clone())
    }

    async fn refresh_token(&self) -> Result<(), AuthError> {
        Ok(())
    }

    async fn is_valid(&self) -> bool {
        !self.api_key.is_empty()
    }

    async fn add_auth_headers(&self, headers: &mut HashMap<String, String>) -> Result<(), AuthError> {
        let header_name = self.header_name.as_deref().unwrap_or("X-API-Key");
        let value = if let Some(prefix) = &self.prefix {
            format!("{} {}", prefix, self.api_key)
        } else {
            self.api_key.clone()
        };
        headers.insert(header_name.to_string(), value);
        Ok(())
    }
}

/// Token storage for persisting authentication tokens
#[async_trait]
pub trait TokenStorage: Send + Sync {
    /// Save a token
    async fn save_token(&self, key: &str, token: &str) -> Result<(), AuthError>;

    /// Load a token
    async fn load_token(&self, key: &str) -> Result<Option<String>, AuthError>;

    /// Delete a token
    async fn delete_token(&self, key: &str) -> Result<(), AuthError>;
}

/// In-memory token storage (for testing)
pub struct MemoryTokenStorage {
    tokens: std::sync::Arc<std::sync::Mutex<HashMap<String, String>>>,
}

impl MemoryTokenStorage {
    pub fn new() -> Self {
        Self {
            tokens: std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl TokenStorage for MemoryTokenStorage {
    async fn save_token(&self, key: &str, token: &str) -> Result<(), AuthError> {
        let mut tokens = self.tokens.lock().map_err(|e| AuthError::Configuration(e.to_string()))?;
        tokens.insert(key.to_string(), token.to_string());
        Ok(())
    }

    async fn load_token(&self, key: &str) -> Result<Option<String>, AuthError> {
        let tokens = self.tokens.lock().map_err(|e| AuthError::Configuration(e.to_string()))?;
        Ok(tokens.get(key).cloned())
    }

    async fn delete_token(&self, key: &str) -> Result<(), AuthError> {
        let mut tokens = self.tokens.lock().map_err(|e| AuthError::Configuration(e.to_string()))?;
        tokens.remove(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_key_provider() {
        let provider = ApiKeyProvider::new("test_key")
            .with_header_name("X-Custom-Key")
            .with_prefix("Bearer");

        assert!(provider.is_valid());
    }

    #[test]
    fn test_oauth_provider_creation() {
        let provider = OAuth2Provider::new(
            "client_id",
            "https://auth.example.com/authorize",
            "https://auth.example.com/token",
            "https://app.example.com/callback",
        );

        assert_eq!(provider.client_id, "client_id");
    }
}
