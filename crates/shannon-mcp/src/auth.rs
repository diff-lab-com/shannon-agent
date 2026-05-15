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
    /// Stored CSRF state for validation during token exchange
    csrf_state: std::sync::Arc<tokio::sync::RwLock<Option<String>>>,
}

/// Internal token storage with expiry
#[derive(Debug, Clone)]
#[derive(Default)]
struct OAuth2Tokens {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
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
            csrf_state: std::sync::Arc::new(tokio::sync::RwLock::new(None)),
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

        // Store state for CSRF validation during token exchange
        *self.csrf_state.write().await = Some(state.clone());

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
    pub async fn exchange_code(&self, code: &str, state: &str) -> Result<String, AuthError> {
        // Validate CSRF state
        {
            let stored_state = self.csrf_state.read().await;
            let expected = stored_state.as_ref()
                .ok_or_else(|| AuthError::OAuth("No state stored. Call get_authorization_url first.".to_string()))?;
            if state != expected {
                return Err(AuthError::OAuth(
                    format!("CSRF state mismatch: expected {expected}, got {state}")
                ));
            }
        }
        // Clear state after validation (one-time use)
        *self.csrf_state.write().await = None;

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

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| AuthError::OAuth(format!("Failed to create HTTP client: {e}")))?;
        let response = client
            .post(&self.token_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| AuthError::OAuth(format!("Token request failed: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AuthError::OAuth(format!(
                "Token exchange failed: {status} - {body}"
            )));
        }

        let token_response: OAuth2TokenResponse = response
            .json()
            .await
            .map_err(|e| AuthError::OAuth(format!("Failed to parse token response: {e}")))?;

        // Validate token type (must be "Bearer" per RFC 6749)
        if let Some(ref token_type) = token_response.token_type {
            if token_type.to_lowercase() != "bearer" {
                return Err(AuthError::InvalidToken(format!(
                    "Unsupported token type: {token_type} (expected 'Bearer')"
                )));
            }
        }

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

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| AuthError::OAuth(format!("Failed to create HTTP client: {e}")))?;
        let response = client
            .post(&self.token_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| AuthError::OAuth(format!("Refresh request failed: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AuthError::OAuth(format!(
                "Token refresh failed: {status} - {body}"
            )));
        }

        let token_response: OAuth2TokenResponse = response
            .json()
            .await
            .map_err(|e| AuthError::OAuth(format!("Failed to parse refresh response: {e}")))?;

        // Validate token type
        if let Some(ref token_type) = token_response.token_type {
            if token_type.to_lowercase() != "bearer" {
                return Err(AuthError::InvalidToken(format!(
                    "Unsupported token type: {token_type} (expected 'Bearer')"
                )));
            }
        }

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

    /// Check if token is expired (public for external validation)
    /// Applies a 60-second margin before actual expiry to avoid race conditions
    pub async fn is_expired(&self) -> bool {
        const EXPIRY_MARGIN_SECS: i64 = 60;
        let tokens = self.tokens.read().await;
        if let Some(expires_at) = tokens.expires_at {
            chrono::Utc::now() >= (expires_at - chrono::Duration::seconds(EXPIRY_MARGIN_SECS))
        } else {
            false
        }
    }
}

#[async_trait]
impl AuthProvider for OAuth2Provider {
    async fn get_token(&self) -> Result<String, AuthError> {
        // Auto-refresh if expired
        if self.is_expired().await {
            if self.tokens.read().await.refresh_token.is_some() {
                info!("OAuth2 token expired, refreshing...");
                self.refresh_access_token().await?;
            } else {
                return Err(AuthError::TokenExpired);
            }
        }

        let tokens = self.tokens.read().await;
        tokens.access_token.clone().ok_or(AuthError::TokenExpired)
    }

    async fn refresh_token(&self) -> Result<(), AuthError> {
        self.refresh_access_token().await?;
        Ok(())
    }

    async fn is_valid(&self) -> bool {
        const EXPIRY_MARGIN_SECS: i64 = 60;
        let tokens = self.tokens.read().await;
        if tokens.access_token.is_none() {
            return false;
        }
        if let Some(expires_at) = tokens.expires_at {
            chrono::Utc::now() < (expires_at - chrono::Duration::seconds(EXPIRY_MARGIN_SECS))
        } else {
            true
        }
    }

    async fn add_auth_headers(&self, headers: &mut HashMap<String, String>) -> Result<(), AuthError> {
        let token = self.get_token().await?;
        headers.insert("Authorization".to_string(), format!("Bearer {token}"));
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

impl Default for MemoryTokenStorage {
    fn default() -> Self {
        Self::new()
    }
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

// ---------------------------------------------------------------------------
// OAuth Discovery Chain (RFC 9728 + RFC 8414)
// ---------------------------------------------------------------------------

/// Result of OAuth metadata discovery.
#[derive(Debug, Clone)]
pub struct OAuthDiscoveryResult {
    /// Authorization endpoint URL (RFC 6749).
    pub authorization_endpoint: String,
    /// Token endpoint URL (RFC 6749).
    pub token_endpoint: String,
    /// Scopes supported by the authorization server.
    pub scopes_supported: Vec<String>,
    /// Registration endpoint URL (RFC 7591 DCR), if advertised.
    pub registration_endpoint: Option<String>,
}

/// Extract the origin from a URL (scheme + host + port).
fn extract_origin(raw_url: &str) -> Result<String, AuthError> {
    // Minimal URL parsing: find scheme://, then host[:port], up to next /
    if !raw_url.contains("://") {
        return Err(AuthError::Configuration(
            format!("Invalid URL (no scheme): {raw_url}")
        ));
    }
    let scheme_end = raw_url.find("://").ok_or_else(|| {
        AuthError::Configuration(format!("Invalid URL (malformed scheme): {raw_url}"))
    })?;
    let after_scheme = &raw_url[scheme_end + 3..];
    let origin_end = after_scheme.find('/').unwrap_or(after_scheme.len());
    let scheme = &raw_url[..scheme_end + 3];
    Ok(format!("{}{}", scheme, &after_scheme[..origin_end]))
}

/// Fetch JSON metadata from a URL. Returns None on any failure.
async fn fetch_metadata_json(url: &str) -> Option<serde_json::Value> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?;

    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.json::<serde_json::Value>().await.ok()
}

/// Try RFC 8414 Authorization Server Metadata at a given origin.
fn parse_auth_server_metadata(metadata: &serde_json::Value) -> Option<OAuthDiscoveryResult> {
    let auth_endpoint = metadata.get("authorization_endpoint")?.as_str()?;
    let token_endpoint = metadata.get("token_endpoint")?.as_str()?;

    // RFC 8414 §3: authorization_endpoint MUST be HTTPS for native apps
    if !auth_endpoint.starts_with("https://") && !auth_endpoint.starts_with("http://localhost") {
        return None;
    }

    Some(OAuthDiscoveryResult {
        authorization_endpoint: auth_endpoint.to_string(),
        token_endpoint: token_endpoint.to_string(),
        scopes_supported: metadata
            .get("scopes_supported")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        registration_endpoint: metadata
            .get("registration_endpoint")
            .and_then(|v| v.as_str())
            .map(String::from),
    })
}

/// Discover OAuth endpoints using RFC 9728 Protected Resource Metadata
/// and RFC 8414 Authorization Server Metadata.
///
/// Discovery chain:
/// 1. RFC 9728: Fetch `{origin}/.well-known/oauth-protected-resource`
///    to find the authorization server(s).
/// 2. RFC 8414: For each authorization server, fetch
///    `{auth_origin}/.well-known/oauth-authorization-server` for endpoints.
/// 3. RFC 8414 direct: Try `{resource_origin}/.well-known/oauth-authorization-server`
///    in case the resource server is also the authorization server.
/// 4. Fallback: return an error — caller should use explicit config.
pub async fn discover_oauth_endpoints(
    server_url: &str,
) -> Result<OAuthDiscoveryResult, AuthError> {
    let origin = extract_origin(server_url)?;

    info!("Starting OAuth discovery for server: {server_url}");

    // Step 1: RFC 9728 Protected Resource Metadata
    let resource_metadata_url =
        format!("{origin}/.well-known/oauth-protected-resource");

    if let Some(metadata) = fetch_metadata_json(&resource_metadata_url).await {
        info!("Found RFC 9728 Protected Resource Metadata");

        // authorization_servers is an array of authorization server URLs
        if let Some(auth_servers) = metadata
            .get("authorization_servers")
            .and_then(|v| v.as_array())
        {
            for server_value in auth_servers {
                if let Some(auth_server_url) = server_value.as_str() {
                    // Step 2: RFC 8414 Authorization Server Metadata
                    let auth_origin = extract_origin(auth_server_url)?;
                    let auth_metadata_url =
                        format!("{auth_origin}/.well-known/oauth-authorization-server");

                    if let Some(auth_metadata) = fetch_metadata_json(&auth_metadata_url).await {
                        if let Some(result) = parse_auth_server_metadata(&auth_metadata) {
                            info!(
                                "Discovered OAuth endpoints via RFC 9728→8414: auth={}",
                                result.authorization_endpoint
                            );
                            return Ok(result);
                        }
                    }
                }
            }
        }
    }

    // Step 3: RFC 8414 direct at the resource origin
    let direct_metadata_url =
        format!("{origin}/.well-known/oauth-authorization-server");

    if let Some(metadata) = fetch_metadata_json(&direct_metadata_url).await {
        if let Some(result) = parse_auth_server_metadata(&metadata) {
            info!(
                "Discovered OAuth endpoints via RFC 8414 direct: auth={}",
                result.authorization_endpoint
            );
            return Ok(result);
        }
    }

    Err(AuthError::Configuration(format!(
        "OAuth discovery failed for '{server_url}': no metadata found at well-known endpoints"
    )))
}

// ---------------------------------------------------------------------------
// Dynamic Client Registration (RFC 7591)
// ---------------------------------------------------------------------------

/// Result of a successful dynamic client registration.
#[derive(Debug, Clone)]
pub struct DcrRegistrationResult {
    /// The registered client ID.
    pub client_id: String,
    /// The client secret (optional, not all flows use it).
    pub client_secret: Option<String>,
}

/// Register a new OAuth client dynamically using RFC 7591.
///
/// This is used when no `client_id` is configured — the client registers
/// itself with the authorization server at runtime.
///
/// # Arguments
/// * `registration_endpoint` — URL from `OAuthDiscoveryResult::registration_endpoint`
/// * `redirect_uris` — URIs the authorization server can redirect to after auth
/// * `scopes` — OAuth scopes to request
pub async fn register_client(
    registration_endpoint: &str,
    redirect_uris: &[String],
    scopes: &[String],
) -> Result<DcrRegistrationResult, AuthError> {
    info!("Attempting dynamic client registration at {registration_endpoint}");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| AuthError::OAuth(format!("Failed to create HTTP client: {e}")))?;

    let mut body = serde_json::json!({
        "client_name": "shannon-code",
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
        "application_type": "native",
    });

    if !redirect_uris.is_empty() {
        body["redirect_uris"] = serde_json::json!(redirect_uris);
    }

    if !scopes.is_empty() {
        body["scope"] = serde_json::json!(scopes.join(" "));
    }

    let response = client
        .post(registration_endpoint)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| AuthError::OAuth(format!("DCR request failed: {e}")))?;

    if !response.status().is_success() {
        let status = response.status();
        let resp_body = response.text().await.unwrap_or_default();
        return Err(AuthError::OAuth(format!(
            "Dynamic client registration failed: {status} - {resp_body}"
        )));
    }

    let result: serde_json::Value = response
        .json()
        .await
        .map_err(|e| AuthError::OAuth(format!("Failed to parse DCR response: {e}")))?;

    let client_id = result
        .get("client_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AuthError::OAuth("DCR response missing client_id".to_string()))?
        .to_string();

    let client_secret = result
        .get("client_secret")
        .and_then(|v| v.as_str())
        .map(String::from);

    info!("Dynamic client registration successful: client_id={client_id}");
    Ok(DcrRegistrationResult {
        client_id,
        client_secret,
    })
}

/// Attempt OAuth auto-registration when no client_id is configured.
///
/// Uses the DCR endpoint from discovery (if available) to register a new client,
/// then constructs an `OAuth2Provider` with the registered credentials.
pub async fn auto_register_oauth(
    server_url: &str,
    redirect_url: &str,
    scopes: Vec<String>,
) -> Result<OAuth2Provider, AuthError> {
    let discovery = discover_oauth_endpoints(server_url).await?;

    let registration_endpoint = discovery
        .registration_endpoint
        .as_ref()
        .ok_or_else(|| {
            AuthError::Configuration(
                "Server does not advertise a registration_endpoint for DCR".to_string(),
            )
        })?;

    let redirect_uris = vec![redirect_url.to_string()];
    let dcr = register_client(registration_endpoint, &redirect_uris, &scopes).await?;

    let mut provider = OAuth2Provider::new(
        dcr.client_id,
        discovery.authorization_endpoint,
        discovery.token_endpoint,
        redirect_url,
    )
    .with_scopes(scopes);

    if let Some(secret) = dcr.client_secret {
        provider = provider.with_client_secret(secret);
    }

    Ok(provider)
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

    #[tokio::test]
    async fn test_api_key_provider_headers() {
        let provider = ApiKeyProvider::new("test_key")
            .with_header_name("X-Custom-Key")
            .with_prefix("Bearer");

        let mut headers = HashMap::new();
        provider.add_auth_headers(&mut headers).await.unwrap();
        assert_eq!(headers.get("X-Custom-Key").unwrap(), "Bearer test_key");
    }

    #[tokio::test]
    async fn test_oauth_expired_token_with_no_refresh() {
        let provider = OAuth2Provider::new(
            "client_id",
            "https://auth.example.com/authorize",
            "https://auth.example.com/token",
            "https://app.example.com/callback",
        );

        // Set expired token directly
        {
            let mut tokens = provider.tokens.write().await;
            tokens.access_token = Some("expired_token".to_string());
            tokens.expires_at = Some(chrono::Utc::now() - chrono::Duration::seconds(60));
            // No refresh token
        }

        assert!(provider.is_expired().await);
        // get_token should fail because there's no refresh token
        let result = provider.get_token().await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AuthError::TokenExpired));
    }

    #[tokio::test]
    async fn test_oauth_valid_token() {
        let provider = OAuth2Provider::new(
            "client_id",
            "https://auth.example.com/authorize",
            "https://auth.example.com/token",
            "https://app.example.com/callback",
        );

        {
            let mut tokens = provider.tokens.write().await;
            tokens.access_token = Some("valid_token".to_string());
            tokens.expires_at = Some(chrono::Utc::now() + chrono::Duration::seconds(3600));
        }

        assert!(!provider.is_expired().await);
        assert!(provider.is_valid().await);
        let token = provider.get_token().await.unwrap();
        assert_eq!(token, "valid_token");
    }

    // ── OAuth Discovery Tests ──────────────────────────────────────────
    #[test]
    fn test_extract_origin() {
        assert_eq!(
            super::extract_origin("https://api.example.com/mcp/sse").unwrap(),
            "https://api.example.com"
        );
        assert_eq!(
            super::extract_origin("https://api.example.com:8443/path").unwrap(),
            "https://api.example.com:8443"
        );
        assert_eq!(
            super::extract_origin("http://localhost:3000/mcp").unwrap(),
            "http://localhost:3000"
        );
        assert_eq!(
            super::extract_origin("https://api.example.com").unwrap(),
            "https://api.example.com"
        );
        assert!(super::extract_origin("not-a-url").is_err());
    }

    #[test]
    fn test_parse_auth_server_metadata() {
        let metadata = serde_json::json!({
            "authorization_endpoint": "https://auth.example.com/authorize",
            "token_endpoint": "https://auth.example.com/token",
            "scopes_supported": ["read", "write"]
        });
        let result = super::parse_auth_server_metadata(&metadata).unwrap();
        assert_eq!(result.authorization_endpoint, "https://auth.example.com/authorize");
        assert_eq!(result.token_endpoint, "https://auth.example.com/token");
        assert_eq!(result.scopes_supported, vec!["read", "write"]);
        assert!(result.registration_endpoint.is_none());
    }

    #[test]
    fn test_parse_auth_server_metadata_missing_fields() {
        let metadata = serde_json::json!({
            "token_endpoint": "https://auth.example.com/token"
        });
        assert!(super::parse_auth_server_metadata(&metadata).is_none());
    }

    #[test]
    fn test_parse_auth_server_metadata_with_registration() {
        let metadata = serde_json::json!({
            "authorization_endpoint": "https://auth.example.com/authorize",
            "token_endpoint": "https://auth.example.com/token",
            "registration_endpoint": "https://auth.example.com/register"
        });
        let result = super::parse_auth_server_metadata(&metadata).unwrap();
        assert_eq!(result.registration_endpoint.as_deref(), Some("https://auth.example.com/register"));
    }

    #[test]
    fn test_parse_auth_server_metadata_non_https_rejected() {
        let metadata = serde_json::json!({
            "authorization_endpoint": "http://auth.example.com/authorize",
            "token_endpoint": "https://auth.example.com/token"
        });
        // Non-HTTPS, non-localhost should be rejected
        assert!(super::parse_auth_server_metadata(&metadata).is_none());
    }

    #[test]
    fn test_parse_auth_server_metadata_localhost_allowed() {
        let metadata = serde_json::json!({
            "authorization_endpoint": "http://localhost:8080/authorize",
            "token_endpoint": "http://localhost:8080/token"
        });
        let result = super::parse_auth_server_metadata(&metadata).unwrap();
        assert_eq!(result.authorization_endpoint, "http://localhost:8080/authorize");
    }

    #[tokio::test]
    async fn test_discover_oauth_endpoints_invalid_url() {
        let result = super::discover_oauth_endpoints("not-a-url").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_discover_oauth_endpoints_no_server() {
        // Use a URL that won't have any OAuth metadata
        let result = super::discover_oauth_endpoints("https://127.0.0.1:1/mcp").await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("no metadata found") || msg.contains("connection refused") || msg.contains("discovery failed"));
    }
}
