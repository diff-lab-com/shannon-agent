// Authentication mechanisms for MCP servers
//
// This module provides authentication implementations including
// OAuth 2.0 PKCE and API key authentication.

use async_trait::async_trait;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use thiserror::Error;
use tracing::info;

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

/// OAuth 2.0 PKCE authentication provider
///
/// Uses RFC 7636 PKCE (Proof Key for Code Exchange) for secure
/// authentication without requiring a client secret.
pub struct OAuth2Provider {
    client_id: String,
    client_secret: Option<String>,
    auth_url: String,
    token_url: String,
    redirect_url: String,
    scopes: Vec<String>,
    /// Interior mutability for token storage (needed for &self methods)
    tokens: std::sync::Arc<tokio::sync::RwLock<OAuth2Tokens>>,
    /// PKCE verifier storage (needed for token exchange)
    code_verifier: std::sync::Arc<tokio::sync::RwLock<Option<String>>>,
}

/// Internal token storage with expiry
#[derive(Debug, Clone)]
struct OAuth2Tokens {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl Default for OAuth2Tokens {
    fn default() -> Self {
        Self {
            access_token: None,
            refresh_token: None,
            expires_at: None,
        }
    }
}

/// OAuth 2.0 token response from server
#[derive(Debug, Deserialize)]
struct OAuth2TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    token_type: Option<String>,
}

/// Helper function to generate base64url encoding without padding
fn base64url_encode(data: &[u8]) -> String {
    use base64::prelude::*;
    BASE64_URL_SAFE_NO_PAD.encode(data)
}

/// Helper function to generate a random PKCE code verifier
///
/// Generates 43-128 random bytes (we use 32 for 256-bit security)
/// and encodes as base64url without padding
fn generate_code_verifier() -> String {
    use rand::RngCore;
    let mut rng = rand::thread_rng();
    let mut bytes = [0u8; 32];
    rng.fill_bytes(&mut bytes);
    base64url_encode(&bytes)
}

/// Compute PKCE code challenge from verifier
///
/// challenge = BASE64URL(SHA256(ASCII(code_verifier)))
fn compute_code_challenge(verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let hash = hasher.finalize();
    base64url_encode(&hash)
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
            tokens: std::sync::Arc::new(tokio::sync::RwLock::new(OAuth2Tokens::default())),
            code_verifier: std::sync::Arc::new(tokio::sync::RwLock::new(None)),
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
    ///
    /// Returns (authorization_url, state)
    /// The user should visit the URL and the callback will include the state
    pub async fn get_authorization_url(&self) -> Result<(String, String), AuthError> {
        let verifier = generate_code_verifier();
        let challenge = compute_code_challenge(&verifier);

        // Store verifier for later token exchange
        *self.code_verifier.write().await = Some(verifier.clone());

        // Generate random state for CSRF protection
        let state = {
            use rand::RngCore;
            let mut rng = rand::thread_rng();
            let mut bytes = [0u8; 16];
            rng.fill_bytes(&mut bytes);
            base64url_encode(&bytes)
        };

        let scope = self.scopes.join(" ");
        let mut url = format!(
            "{}?response_type=code&client_id={}&redirect_uri={}&code_challenge={}&code_challenge_method=S256&state={}",
            self.auth_url,
            urlencoding::Encoded::new(&self.client_id),
            urlencoding::Encoded::new(&self.redirect_url),
            urlencoding::Encoded::new(&challenge),
            urlencoding::Encoded::new(&state)
        );

        if !scope.is_empty() {
            url.push_str(&format!("&scope={}", urlencoding::Encoded::new(&scope)));
        }

        info!("Generated OAuth2 authorization URL with PKCE");
        Ok((url, state))
    }

    /// Exchange authorization code for access token
    pub async fn exchange_code(&self, code: &str, _state: &str) -> Result<String, AuthError> {
        // Retrieve the stored code verifier
        let verifier_guard = self.code_verifier.read().await;
        let verifier = verifier_guard.as_ref()
            .ok_or_else(|| AuthError::OAuth("No code verifier stored. Call get_authorization_url first.".to_string()))?
            .clone();
        drop(verifier_guard);

        // Build token request
        let mut params = HashMap::new();
        params.insert("grant_type", "authorization_code");
        params.insert("code", code);
        params.insert("redirect_uri", &self.redirect_url);
        params.insert("client_id", &self.client_id);
        params.insert("code_verifier", &verifier);

        if let Some(secret) = &self.client_secret {
            params.insert("client_secret", secret);
        }

        let client = reqwest::Client::new();
        let response = client
            .post(&self.token_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| AuthError::OAuth(format!("Token request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AuthError::OAuth(format!(
                "Token exchange failed: {} - {}",
                status, body
            )));
        }

        let token_response: OAuth2TokenResponse = response
            .json()
            .await
            .map_err(|e| AuthError::OAuth(format!("Failed to parse token response: {}", e)))?;

        // Calculate expiry time
        let expires_at = token_response.expires_in.map(|seconds| {
            chrono::Utc::now() + chrono::Duration::seconds(seconds as i64)
        });

        // Store tokens
        let mut tokens = self.tokens.write().await;
        tokens.access_token = Some(token_response.access_token.clone());
        tokens.refresh_token = token_response.refresh_token;
        tokens.expires_at = expires_at;

        // Clear the code verifier after use
        *self.code_verifier.write().await = None;

        info!("OAuth2 token exchanged successfully");
        Ok(token_response.access_token)
    }

    /// Refresh the access token using refresh token
    pub async fn refresh_access_token(&self) -> Result<String, AuthError> {
        let tokens_guard = self.tokens.read().await;
        let refresh_token = tokens_guard.refresh_token.as_ref()
            .ok_or_else(|| AuthError::OAuth("No refresh token available".to_string()))?
            .clone();
        drop(tokens_guard);

        // Build refresh request
        let mut params = HashMap::new();
        params.insert("grant_type", "refresh_token");
        params.insert("refresh_token", &refresh_token);
        params.insert("client_id", &self.client_id);

        if let Some(secret) = &self.client_secret {
            params.insert("client_secret", secret);
        }

        let client = reqwest::Client::new();
        let response = client
            .post(&self.token_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| AuthError::OAuth(format!("Refresh request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AuthError::OAuth(format!(
                "Token refresh failed: {} - {}",
                status, body
            )));
        }

        let token_response: OAuth2TokenResponse = response
            .json()
            .await
            .map_err(|e| AuthError::OAuth(format!("Failed to parse refresh response: {}", e)))?;

        // Update tokens
        let mut tokens = self.tokens.write().await;
        tokens.access_token = Some(token_response.access_token.clone());
        tokens.refresh_token = token_response.refresh_token.or(tokens.refresh_token.clone());

        let expires_at = token_response.expires_in.map(|seconds| {
            chrono::Utc::now() + chrono::Duration::seconds(seconds as i64)
        });
        tokens.expires_at = expires_at;

        info!("OAuth2 token refreshed successfully");
        Ok(token_response.access_token)
    }

    /// Check if token is expired
    async fn is_expired(&self) -> bool {
        let tokens = self.tokens.read().await;
        if let Some(expires_at) = tokens.expires_at {
            chrono::Utc::now() >= expires_at
        } else {
            false
        }
    }
}

#[async_trait]
impl AuthProvider for OAuth2Provider {
    async fn get_token(&self) -> Result<String, AuthError> {
        let tokens = self.tokens.read().await;
        tokens.access_token.clone().ok_or_else(|| AuthError::TokenExpired)
    }

    async fn refresh_token(&self) -> Result<(), AuthError> {
        self.refresh_access_token().await?;
        Ok(())
    }

    async fn is_valid(&self) -> bool {
        let tokens = self.tokens.read().await;
        if tokens.access_token.is_none() {
            return false;
        }
        if let Some(expires_at) = tokens.expires_at {
            chrono::Utc::now() < expires_at
        } else {
            true
        }
    }

    async fn add_auth_headers(&self, headers: &mut HashMap<String, String>) -> Result<(), AuthError> {
        let token = self.get_token().await?;
        headers.insert("Authorization".to_string(), format!("Bearer {}", token));
        Ok(())
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

    #[tokio::test]
    async fn test_api_key_provider() {
        let provider = ApiKeyProvider::new("test_key")
            .with_header_name("X-Custom-Key")
            .with_prefix("Bearer");

        assert!(provider.is_valid().await);
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
