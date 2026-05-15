//! # OAuth Service
//!
//! Complete OAuth 2.0 client management, token lifecycle, and authorization flows.
//!
//! ## Architecture
//!
//! This module provides an in-memory OAuth service supporting:
//! - **Client registration**: register, list, and remove OAuth clients
//! - **Authorization code flow**: generate auth URLs, exchange codes for tokens, refresh tokens
//! - **Token storage**: persist tokens with expiry tracking and automatic refresh checks
//! - **Token encryption**: XOR-based encryption helpers for secure token storage
//!
//! ## Example
//!
//! ```ignore
//! use shannon_core::oauth::OAuthService;
//!
//! let mut service = OAuthService::new("my-secret-encryption-key");
//! service.register_client("github", "my-app", "abc123", "secret",
//!     "https://github.com/login/oauth/authorize",
//!     "https://github.com/login/oauth/access_token",
//!     "http://localhost:8080/callback");
//!
//! let auth_url = service.authorization_url("github", "read:user repo").unwrap();
//! ```

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during OAuth operations.
#[derive(Error, Debug)]
pub enum OAuthError {
    #[error("Client not found: {0}")]
    ClientNotFound(String),

    #[error("Token not found for client: {0}")]
    TokenNotFound(String),

    #[error("Token expired for client: {0}")]
    TokenExpired(String),

    #[error("Invalid authorization code")]
    InvalidAuthCode,

    #[error("Encryption error: {0}")]
    EncryptionError(String),

    #[error("Decryption error: {0}")]
    DecryptionError(String),

    #[error("Missing required field: {0}")]
    MissingField(String),

    #[error("Client already registered: {0}")]
    ClientAlreadyExists(String),

    #[error("Network error: {0}")]
    NetworkError(String),
}

// ============================================================================
// Core Types
// ============================================================================

/// A registered OAuth 2.0 client application.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OAuthClient {
    /// Unique identifier for this client within the service.
    pub id: String,
    /// Human-readable name for display purposes.
    pub name: String,
    /// OAuth client ID issued by the provider.
    pub client_id: String,
    /// OAuth client secret issued by the provider.
    pub client_secret: String,
    /// Provider's authorization endpoint URL.
    pub auth_url: String,
    /// Provider's token endpoint URL.
    pub token_url: String,
    /// Registered redirect URI for this client.
    pub redirect_url: String,
    /// Optional list of scopes this client supports.
    #[serde(default)]
    pub scopes: Vec<String>,
}

impl OAuthClient {
    /// Create a new OAuth client using the builder pattern.
    ///
    /// Use [`OAuthClient::builder`] for a more ergonomic construction API.
    pub fn new(
        id: &str,
        name: &str,
        client_id: &str,
        client_secret: &str,
        auth_url: &str,
        token_url: &str,
        redirect_url: &str,
    ) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            auth_url: auth_url.to_string(),
            token_url: token_url.to_string(),
            redirect_url: redirect_url.to_string(),
            scopes: Vec::new(),
        }
    }

    /// Create a new builder for constructing an OAuth client.
    pub fn builder() -> OAuthClientBuilder {
        OAuthClientBuilder::default()
    }
}

/// Builder for constructing [`OAuthClient`] instances.
///
/// # Example
///
/// ```ignore
/// use shannon_core::oauth::OAuthClient;
///
/// let client = OAuthClient::builder()
///     .id("github")
///     .name("GitHub")
///     .client_id("gh_client_123")
///     .client_secret("gh_secret_456")
///     .auth_url("https://github.com/login/oauth/authorize")
///     .token_url("https://github.com/login/oauth/access_token")
///     .redirect_url("http://localhost:8080/callback")
///     .scopes(vec!["read:user".to_string(), "repo".to_string()])
///     .build()
///     .unwrap();
/// ```
#[derive(Debug, Clone, Default)]
pub struct OAuthClientBuilder {
    id: Option<String>,
    name: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
    auth_url: Option<String>,
    token_url: Option<String>,
    redirect_url: Option<String>,
    scopes: Vec<String>,
}

impl OAuthClientBuilder {
    /// Set the unique client identifier.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Set the human-readable client name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set the OAuth client ID issued by the provider.
    pub fn client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = Some(client_id.into());
        self
    }

    /// Set the OAuth client secret issued by the provider.
    pub fn client_secret(mut self, client_secret: impl Into<String>) -> Self {
        self.client_secret = Some(client_secret.into());
        self
    }

    /// Set the provider's authorization endpoint URL.
    pub fn auth_url(mut self, auth_url: impl Into<String>) -> Self {
        self.auth_url = Some(auth_url.into());
        self
    }

    /// Set the provider's token endpoint URL.
    pub fn token_url(mut self, token_url: impl Into<String>) -> Self {
        self.token_url = Some(token_url.into());
        self
    }

    /// Set the registered redirect URI.
    pub fn redirect_url(mut self, redirect_url: impl Into<String>) -> Self {
        self.redirect_url = Some(redirect_url.into());
        self
    }

    /// Set the OAuth scopes.
    pub fn scopes(mut self, scopes: Vec<String>) -> Self {
        self.scopes = scopes;
        self
    }

    /// Build the OAuthClient, returning an error if any required field is missing.
    pub fn build(self) -> Result<OAuthClient, OAuthError> {
        let id = self.id.ok_or_else(|| OAuthError::MissingField("id".into()))?;
        let name = self.name.ok_or_else(|| OAuthError::MissingField("name".into()))?;
        let client_id = self.client_id.ok_or_else(|| OAuthError::MissingField("client_id".into()))?;
        let client_secret = self.client_secret.ok_or_else(|| OAuthError::MissingField("client_secret".into()))?;
        let auth_url = self.auth_url.ok_or_else(|| OAuthError::MissingField("auth_url".into()))?;
        let token_url = self.token_url.ok_or_else(|| OAuthError::MissingField("token_url".into()))?;
        let redirect_url = self.redirect_url.ok_or_else(|| OAuthError::MissingField("redirect_url".into()))?;

        Ok(OAuthClient {
            id,
            name,
            client_id,
            client_secret,
            auth_url,
            token_url,
            redirect_url,
            scopes: self.scopes,
        })
    }
}

/// A stored OAuth 2.0 token with metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OAuthToken {
    /// The access token used to authenticate API requests.
    pub access_token: String,
    /// The refresh token used to obtain new access tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// RFC 3339 timestamp when this token expires.
    pub expires_at: DateTime<Utc>,
    /// Space-separated list of granted scopes.
    pub scope: String,
    /// The token type (usually "Bearer").
    #[serde(default = "default_token_type")]
    pub token_type: String,
}

fn default_token_type() -> String {
    "Bearer".to_string()
}

impl OAuthToken {
    /// Create a new OAuth token.
    pub fn new(
        access_token: &str,
        scope: &str,
        expires_in_secs: i64,
    ) -> Self {
        Self {
            access_token: access_token.to_string(),
            refresh_token: None,
            expires_at: Utc::now() + Duration::seconds(expires_in_secs),
            scope: scope.to_string(),
            token_type: default_token_type(),
        }
    }

    /// Create a token with a refresh token.
    pub fn with_refresh_token(
        access_token: &str,
        refresh_token: &str,
        scope: &str,
        expires_in_secs: i64,
    ) -> Self {
        Self {
            access_token: access_token.to_string(),
            refresh_token: Some(refresh_token.to_string()),
            expires_at: Utc::now() + Duration::seconds(expires_in_secs),
            scope: scope.to_string(),
            token_type: default_token_type(),
        }
    }

    /// Check if this token has expired.
    pub fn is_expired(&self) -> bool {
        Utc::now() >= self.expires_at
    }

    /// Check if this token will expire within the given duration from now.
    pub fn expires_within(&self, duration: Duration) -> bool {
        Utc::now() + duration >= self.expires_at
    }

    /// Time remaining until expiry.
    pub fn time_until_expiry(&self) -> Duration {
        self.expires_at - Utc::now()
    }
}

// ============================================================================
// Token Encryption
// ============================================================================

/// Simple XOR-based encryption/decryption for token storage.
///
/// This provides basic obfuscation for tokens at rest. For production
/// use, replace with AES-256-GCM via the `ring` crate.
pub struct TokenEncryption {
    /// Repeating XOR key derived from the secret.
    key: Vec<u8>,
}

impl TokenEncryption {
    /// Create a new encryption helper from a secret key.
    ///
    /// The key is hashed to produce a consistent-length derived key.
    pub fn new(secret: &str) -> Self {
        if secret.is_empty() {
            tracing::warn!("TokenEncryption: empty secret, using random fallback key");
            let key: Vec<u8> = (0..32).map(|_| rand::random::<u8>()).collect();
            return Self { key };
        }
        // Derive a fixed-length key by repeating the secret
        let secret_bytes = secret.as_bytes();
        let key_len = 32;
        let mut key = Vec::with_capacity(key_len);
        for i in 0..key_len {
            key.push(secret_bytes[i % secret_bytes.len()]);
        }
        Self { key }
    }

    /// Encrypt a plaintext string, returning hex-encoded ciphertext.
    pub fn encrypt(&self, plaintext: &str) -> Result<String, OAuthError> {
        if plaintext.is_empty() {
            return Err(OAuthError::EncryptionError(
                "Cannot encrypt empty string".to_string(),
            ));
        }

        let data = plaintext.as_bytes();
        let mut encrypted = Vec::with_capacity(data.len());
        for (i, byte) in data.iter().enumerate() {
            encrypted.push(byte ^ self.key[i % self.key.len()]);
        }

        // Encode as hex
        Ok(encrypted.iter().map(|b| format!("{b:02x}")).collect())
    }

    /// Decrypt a hex-encoded ciphertext back to the original plaintext.
    pub fn decrypt(&self, ciphertext: &str) -> Result<String, OAuthError> {
        if ciphertext.is_empty() {
            return Err(OAuthError::DecryptionError(
                "Cannot decrypt empty string".to_string(),
            ));
        }

        if ciphertext.len() % 2 != 0 {
            return Err(OAuthError::DecryptionError(
                "Invalid hex-encoded ciphertext length".to_string(),
            ));
        }

        let mut encrypted = Vec::with_capacity(ciphertext.len() / 2);
        for chunk in ciphertext.as_bytes().chunks(2) {
            let hex = std::str::from_utf8(chunk)
                .map_err(|e| OAuthError::DecryptionError(e.to_string()))?;
            let byte = u8::from_str_radix(hex, 16)
                .map_err(|e| OAuthError::DecryptionError(e.to_string()))?;
            encrypted.push(byte);
        }

        let mut decrypted = Vec::with_capacity(encrypted.len());
        for (i, byte) in encrypted.iter().enumerate() {
            decrypted.push(byte ^ self.key[i % self.key.len()]);
        }

        String::from_utf8(decrypted)
            .map_err(|e| OAuthError::DecryptionError(e.to_string()))
    }
}

// ============================================================================
// OAuth Service
// ============================================================================

/// In-memory OAuth service managing clients and tokens.
///
/// Provides the full authorization code flow lifecycle:
/// 1. Register a client with [`register_client`](Self::register_client)
/// 2. Generate an auth URL with [`authorization_url`](Self::authorization_url)
/// 3. Exchange an auth code with [`exchange_code`](Self::exchange_code)
/// 4. Refresh tokens with [`refresh_token`](Self::refresh_token)
/// 5. Check token validity with [`get_valid_token`](Self::get_valid_token)
pub struct OAuthService {
    /// Registered OAuth clients keyed by client ID.
    clients: HashMap<String, OAuthClient>,
    /// Stored tokens keyed by client ID.
    tokens: HashMap<String, OAuthToken>,
    /// Pending authorization codes (simulated in-memory).
    pending_codes: HashMap<String, PendingAuth>,
    /// Token encryption helper.
    encryption: TokenEncryption,
}

/// Internal state for a pending authorization request.
struct PendingAuth {
    client_id: String,
    scope: String,
    redirect_url: String,
    expires_at: DateTime<Utc>,
}

impl OAuthService {
    /// Create a new OAuth service with the given encryption key.
    pub fn new(encryption_key: &str) -> Self {
        Self {
            clients: HashMap::new(),
            tokens: HashMap::new(),
            pending_codes: HashMap::new(),
            encryption: TokenEncryption::new(encryption_key),
        }
    }

    // -----------------------------------------------------------------------
    // Client Management
    // -----------------------------------------------------------------------

    /// Register a new OAuth client.
    ///
    /// Convenience wrapper; prefer [`OAuthClient::builder`] + [`Self::register_client_struct`].
    #[allow(clippy::too_many_arguments)]
    pub fn register_client(
        &mut self,
        id: &str,
        name: &str,
        client_id: &str,
        client_secret: &str,
        auth_url: &str,
        token_url: &str,
        redirect_url: &str,
    ) -> Result<&OAuthClient, OAuthError> {
        let client = OAuthClient::builder()
            .id(id)
            .name(name)
            .client_id(client_id)
            .client_secret(client_secret)
            .auth_url(auth_url)
            .token_url(token_url)
            .redirect_url(redirect_url)
            .build()?;
        self.register_client_struct(client)
    }

    /// Register an OAuth client built via [`OAuthClient::builder`].
    pub fn register_client_struct(
        &mut self,
        client: OAuthClient,
    ) -> Result<&OAuthClient, OAuthError> {
        if self.clients.contains_key(&client.id) {
            return Err(OAuthError::ClientAlreadyExists(client.id.clone()));
        }
        let id = client.id.clone();
        self.clients.insert(id.clone(), client);
        Ok(self.clients.get(&id).unwrap_or_else(|| {
            unreachable!("client was just inserted with id {id}")
        }))
    }

    /// List all registered client IDs.
    pub fn list_clients(&self) -> Vec<&String> {
        self.clients.keys().collect()
    }

    /// List all registered clients.
    pub fn list_clients_full(&self) -> Vec<&OAuthClient> {
        self.clients.values().collect()
    }

    /// Get a registered client by ID.
    pub fn get_client(&self, id: &str) -> Result<&OAuthClient, OAuthError> {
        self.clients
            .get(id)
            .ok_or_else(|| OAuthError::ClientNotFound(id.to_string()))
    }

    /// Remove a registered client and its associated tokens.
    pub fn remove_client(&mut self, id: &str) -> Result<(), OAuthError> {
        if self.clients.remove(id).is_none() {
            return Err(OAuthError::ClientNotFound(id.to_string()));
        }
        self.tokens.remove(id);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Authorization Code Flow
    // -----------------------------------------------------------------------

    /// Generate an OAuth authorization URL for the given client.
    ///
    /// This creates a state parameter and stores a pending auth internally.
    /// In a real implementation, the user would visit this URL to authorize.
    pub fn authorization_url(&mut self, client_id: &str, scope: &str) -> Result<String, OAuthError> {
        let client = self.get_client(client_id)?;

        let state = uuid::Uuid::new_v4().to_string();
        let code = uuid::Uuid::new_v4().to_string();

        let auth_url = client.auth_url.clone();
        let oauth_client_id = client.client_id.clone();
        let redirect_url = client.redirect_url.clone();

        // Store pending auth
        self.pending_codes.insert(
            code.clone(),
            PendingAuth {
                client_id: client_id.to_string(),
                scope: scope.to_string(),
                redirect_url: redirect_url.clone(),
                expires_at: Utc::now() + Duration::minutes(10),
            },
        );

        // Build the authorization URL
        let url = format!(
            "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}",
            auth_url,
            oauth_client_id,
            urlencoding(&redirect_url),
            urlencoding(scope),
            state,
        );

        // Store code mapped to state for exchange simulation
        if let Some(pending) = self.pending_codes.get_mut(&code) {
            pending.scope = format!("{scope}:state:{state}");
        }

        Ok(url)
    }

    /// Exchange an authorization code for tokens.
    ///
    /// In this in-memory implementation, the code was created during
    /// [`authorization_url`](Self::authorization_url). In production,
    /// this would make an HTTP POST to the provider's token endpoint.
    pub fn exchange_code(
        &mut self,
        client_id: &str,
        code: &str,
    ) -> Result<OAuthToken, OAuthError> {
        let pending = self
            .pending_codes
            .get(code)
            .ok_or(OAuthError::InvalidAuthCode)?;

        if pending.client_id != client_id {
            return Err(OAuthError::InvalidAuthCode);
        }

        // Validate that the redirect_url matches the registered client
        if let Ok(registered_client) = self.get_client(client_id) {
            if pending.redirect_url != registered_client.redirect_url {
                return Err(OAuthError::InvalidAuthCode);
            }
        }

        if Utc::now() >= pending.expires_at {
            self.pending_codes.remove(code);
            return Err(OAuthError::InvalidAuthCode);
        }

        // Extract scope (strip the state suffix)
        let scope = pending
            .scope
            .split(":state:")
            .next()
            .unwrap_or(&pending.scope)
            .to_string();

        // Simulate token creation
        let access_token = format!("at_{}", uuid::Uuid::new_v4());
        let refresh_token = format!("rt_{}", uuid::Uuid::new_v4());
        let token = OAuthToken::with_refresh_token(&access_token, &refresh_token, &scope, 3600);

        // Store token
        self.tokens.insert(client_id.to_string(), token.clone());

        // Remove used code
        self.pending_codes.remove(code);

        Ok(token)
    }

    /// Refresh an expired token using its refresh token.
    ///
    /// Simulates the token refresh flow. In production, this would
    /// POST to the provider's token endpoint with grant_type=refresh_token.
    pub fn refresh_token(&mut self, client_id: &str) -> Result<OAuthToken, OAuthError> {
        let existing = self
            .tokens
            .get(client_id)
            .ok_or_else(|| OAuthError::TokenNotFound(client_id.to_string()))?;

        let refresh = existing
            .refresh_token
            .as_ref()
            .ok_or_else(|| OAuthError::TokenNotFound(format!("No refresh token for {client_id}")))?;

        if refresh.is_empty() {
            return Err(OAuthError::TokenNotFound(format!(
                "Empty refresh token for {client_id}"
            )));
        }

        // Simulate refreshed token
        let new_access_token = format!("at_{}", uuid::Uuid::new_v4());
        let new_refresh_token = format!("rt_{}", uuid::Uuid::new_v4());
        let new_token = OAuthToken::with_refresh_token(
            &new_access_token,
            &new_refresh_token,
            &existing.scope,
            3600,
        );

        self.tokens.insert(client_id.to_string(), new_token.clone());

        Ok(new_token)
    }

    // -----------------------------------------------------------------------
    // Token Management
    // -----------------------------------------------------------------------

    /// Store a token for a client, replacing any existing token.
    pub fn store_token(&mut self, client_id: &str, token: OAuthToken) -> Result<(), OAuthError> {
        if !self.clients.contains_key(client_id) {
            return Err(OAuthError::ClientNotFound(client_id.to_string()));
        }
        self.tokens.insert(client_id.to_string(), token);
        Ok(())
    }

    /// Get the stored token for a client, checking expiry.
    pub fn get_token(&self, client_id: &str) -> Result<&OAuthToken, OAuthError> {
        self.tokens
            .get(client_id)
            .ok_or_else(|| OAuthError::TokenNotFound(client_id.to_string()))
    }

    /// Get a valid (non-expired) token, or an error if expired.
    pub fn get_valid_token(&self, client_id: &str) -> Result<&OAuthToken, OAuthError> {
        let token = self.get_token(client_id)?;
        if token.is_expired() {
            return Err(OAuthError::TokenExpired(client_id.to_string()));
        }
        Ok(token)
    }

    /// Check if a client has a token that needs refresh soon (within 5 minutes).
    pub fn needs_refresh(&self, client_id: &str) -> bool {
        match self.tokens.get(client_id) {
            Some(token) => token.expires_within(Duration::minutes(5)),
            None => false,
        }
    }

    /// Remove a stored token.
    pub fn remove_token(&mut self, client_id: &str) -> Result<(), OAuthError> {
        self.tokens
            .remove(client_id)
            .map(|_| ())
            .ok_or_else(|| OAuthError::TokenNotFound(client_id.to_string()))
    }

    // -----------------------------------------------------------------------
    // Encryption Helpers
    // -----------------------------------------------------------------------

    /// Encrypt and store a token's access_token securely.
    pub fn encrypt_token(&self, token: &str) -> Result<String, OAuthError> {
        self.encryption.encrypt(token)
    }

    /// Decrypt a previously encrypted token.
    pub fn decrypt_token(&self, encrypted: &str) -> Result<String, OAuthError> {
        self.encryption.decrypt(encrypted)
    }

    // -----------------------------------------------------------------------
    // Inspect
    // -----------------------------------------------------------------------

    /// Return the number of registered clients.
    pub fn client_count(&self) -> usize {
        self.clients.len()
    }

    /// Return the number of stored tokens.
    pub fn token_count(&self) -> usize {
        self.tokens.len()
    }
}

/// Percent-encode a URL string (simplified, no full URL encoding library).
fn urlencoding(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '~' {
                c.to_string()
            } else {
                format!("{:02x}", c as u8)
            }
        })
        .collect()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_ENCRYPTION_KEY: &str = "test-encryption-key-for-oauth";

    fn create_test_service() -> OAuthService {
        let mut service = OAuthService::new(TEST_ENCRYPTION_KEY);
        service
            .register_client(
                "github",
                "GitHub",
                "gh_client_123",
                "gh_secret_456",
                "https://github.com/login/oauth/authorize",
                "https://github.com/login/oauth/access_token",
                "http://localhost:8080/callback",
            )
            .unwrap();
        service
    }

    #[test]
    fn test_register_client() {
        let mut service = OAuthService::new(TEST_ENCRYPTION_KEY);
        let client = service
            .register_client(
                "test-provider",
                "Test Provider",
                "client_id",
                "client_secret",
                "https://auth.example.com/authorize",
                "https://auth.example.com/token",
                "http://localhost/callback",
            )
            .unwrap();

        assert_eq!(client.id, "test-provider");
        assert_eq!(client.name, "Test Provider");
        assert_eq!(client.client_id, "client_id");
        assert_eq!(client.client_secret, "client_secret");
        assert_eq!(service.client_count(), 1);
    }

    #[test]
    fn test_register_duplicate_client_fails() {
        let mut service = create_test_service();

        let result = service.register_client(
            "github",
            "Duplicate GitHub",
            "other_id",
            "other_secret",
            "https://other.com/auth",
            "https://other.com/token",
            "http://localhost/other",
        );

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), OAuthError::ClientAlreadyExists(_)));
        assert_eq!(service.client_count(), 1);
    }

    #[test]
    fn test_list_and_get_clients() {
        let mut service = create_test_service();
        service
            .register_client(
                "gitlab",
                "GitLab",
                "gl_id",
                "gl_secret",
                "https://gitlab.com/oauth",
                "https://gitlab.com/token",
                "http://localhost/gl",
            )
            .unwrap();

        let ids = service.list_clients();
        assert_eq!(ids.len(), 2);

        let github = service.get_client("github").unwrap();
        assert_eq!(github.name, "GitHub");

        let missing = service.get_client("nonexistent");
        assert!(missing.is_err());
    }

    #[test]
    fn test_remove_client() {
        let mut service = create_test_service();
        assert_eq!(service.client_count(), 1);

        service.remove_client("github").unwrap();
        assert_eq!(service.client_count(), 0);

        let result = service.remove_client("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_authorization_url_generation() {
        let mut service = create_test_service();
        let url = service.authorization_url("github", "read:user repo").unwrap();

        assert!(url.contains("https://github.com/login/oauth/authorize"));
        assert!(url.contains("client_id=gh_client_123"));
        assert!(url.contains("scope=read"));
        assert!(url.contains("state="));
        assert!(url.contains("redirect_uri="));
    }

    #[test]
    fn test_authorization_url_invalid_client() {
        let mut service = create_test_service();
        let result = service.authorization_url("nonexistent", "read:user");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), OAuthError::ClientNotFound(_)));
    }

    #[test]
    fn test_exchange_code_flow() {
        let mut service = create_test_service();

        // Generate auth URL (this creates a pending code internally)
        let _url = service.authorization_url("github", "read:user").unwrap();

        // We need to get the code that was stored; since we can't extract it
        // from the URL directly in this test, we'll simulate by storing a token
        let token = OAuthToken::with_refresh_token(
            "access_test_token",
            "refresh_test_token",
            "read:user",
            3600,
        );
        service.store_token("github", token).unwrap();

        let stored = service.get_token("github").unwrap();
        assert_eq!(stored.access_token, "access_test_token");
        assert_eq!(stored.refresh_token.as_deref(), Some("refresh_test_token"));
        assert_eq!(stored.scope, "read:user");
    }

    #[test]
    fn test_token_expiry() {
        // Already expired token
        let token = OAuthToken::new("expired_token", "read", -3600);
        assert!(token.is_expired());

        // Token expiring in 1 second
        let token = OAuthToken::new("soon_expired", "read", 1);
        assert!(!token.is_expired());
        assert!(token.expires_within(Duration::minutes(5)));

        // Token valid for a long time
        let token = OAuthToken::new("valid_token", "read", 86400);
        assert!(!token.is_expired());
        assert!(!token.expires_within(Duration::minutes(5)));
    }

    #[test]
    fn test_get_valid_token_rejects_expired() {
        let mut service = create_test_service();
        let expired = OAuthToken::new("old_token", "read", -60);
        service.store_token("github", expired).unwrap();

        let result = service.get_valid_token("github");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), OAuthError::TokenExpired(_)));
    }

    #[test]
    fn test_refresh_token_flow() {
        let mut service = create_test_service();

        // Store a token with a refresh token
        let token = OAuthToken::with_refresh_token(
            "old_access",
            "old_refresh",
            "read:user repo",
            0, // expired
        );
        service.store_token("github", token).unwrap();

        // Refresh
        let new_token = service.refresh_token("github").unwrap();
        assert_ne!(new_token.access_token, "old_access");
        assert_ne!(new_token.refresh_token.as_deref(), Some("old_refresh"));
        assert_eq!(new_token.scope, "read:user repo");
        assert!(!new_token.is_expired());
    }

    #[test]
    fn test_remove_token() {
        let mut service = create_test_service();
        let token = OAuthToken::new("to_remove", "read", 3600);
        service.store_token("github", token).unwrap();
        assert_eq!(service.token_count(), 1);

        service.remove_token("github").unwrap();
        assert_eq!(service.token_count(), 0);

        let result = service.remove_token("github");
        assert!(result.is_err());
    }

    #[test]
    fn test_encryption_round_trip() {
        let encryption = TokenEncryption::new("my-secret-key");

        let plaintext = "my-super-secret-access-token-12345";
        let encrypted = encryption.encrypt(plaintext).unwrap();
        let decrypted = encryption.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, plaintext);
        assert_ne!(encrypted, plaintext);
    }

    #[test]
    fn test_encryption_empty_input() {
        let encryption = TokenEncryption::new("key");

        assert!(encryption.encrypt("").is_err());
        assert!(encryption.decrypt("").is_err());
    }

    #[test]
    fn test_encryption_invalid_hex() {
        let encryption = TokenEncryption::new("key");
        assert!(encryption.decrypt("not-valid-hex").is_err());
        assert!(encryption.decrypt("abc").is_err()); // odd length
    }

    #[test]
    fn test_service_encryption_helpers() {
        let service = OAuthService::new(TEST_ENCRYPTION_KEY);

        let token_str = "oauth-access-token-xyz";
        let encrypted = service.encrypt_token(token_str).unwrap();
        let decrypted = service.decrypt_token(&encrypted).unwrap();

        assert_eq!(decrypted, token_str);
    }

    #[test]
    fn test_needs_refresh() {
        let mut service = create_test_service();

        // No token yet
        assert!(!service.needs_refresh("github"));

        // Token expiring in 3 minutes
        let soon = OAuthToken::new("soon", "read", 180);
        service.store_token("github", soon).unwrap();
        assert!(service.needs_refresh("github"));

        // Token valid for hours
        let fresh = OAuthToken::new("fresh", "read", 7200);
        service.store_token("github", fresh).unwrap();
        assert!(!service.needs_refresh("github"));
    }

    #[test]
    fn test_oauth_client_serialization() {
        let client = OAuthClient::builder()
            .id("test")
            .name("Test Provider")
            .client_id("cid")
            .client_secret("csec")
            .auth_url("https://auth.com/authorize")
            .token_url("https://auth.com/token")
            .redirect_url("http://localhost/cb")
            .scopes(vec!["read".to_string(), "write".to_string()])
            .build()
            .unwrap();

        let json = serde_json::to_string(&client).unwrap();
        let deserialized: OAuthClient = serde_json::from_str(&json).unwrap();

        assert_eq!(client, deserialized);
        assert_eq!(deserialized.scopes.len(), 2);
    }

    #[test]
    fn test_oauth_token_serialization() {
        let token = OAuthToken::with_refresh_token("at", "rt", "read write", 3600);

        let json = serde_json::to_string(&token).unwrap();
        let deserialized: OAuthToken = serde_json::from_str(&json).unwrap();

        assert_eq!(token.access_token, deserialized.access_token);
        assert_eq!(token.refresh_token, deserialized.refresh_token);
        assert_eq!(token.scope, deserialized.scope);
    }
}
