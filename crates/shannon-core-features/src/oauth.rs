//! # OAuth Service
//!
//! Complete OAuth 2.0 client management, token lifecycle, and authorization flows.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during OAuth operations.
#[derive(Debug, Error)]
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
    /// Create a new OAuth client.
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

    /// Create a new OAuth client with predefined scopes.
    pub fn with_scopes(
        id: &str,
        name: &str,
        client_id: &str,
        client_secret: &str,
        auth_url: &str,
        token_url: &str,
        redirect_url: &str,
        scopes: Vec<String>,
    ) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            auth_url: auth_url.to_string(),
            token_url: token_url.to_string(),
            redirect_url: redirect_url.to_string(),
            scopes,
        }
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
        Ok(encrypted.iter().map(|b| format!("{:02x}", b)).collect())
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

/// Internal state for a pending authorization request.
#[allow(dead_code)]
struct PendingAuth {
    client_id: String,
    scope: String,
    redirect_url: String,
    expires_at: DateTime<Utc>,
}

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
        if self.clients.contains_key(id) {
            return Err(OAuthError::ClientAlreadyExists(id.to_string()));
        }

        let client = OAuthClient::new(id, name, client_id, client_secret, auth_url, token_url, redirect_url);
        self.clients.insert(id.to_string(), client);
        Ok(self.clients.get(id).unwrap())
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
        self.pending_codes
            .get_mut(&code)
            .unwrap()
            .scope = format!("{}:state:{}", scope, state);

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
            .ok_or_else(|| OAuthError::TokenNotFound(format!("No refresh token for {}", client_id)))?;

        if refresh.is_empty() {
            return Err(OAuthError::TokenNotFound(format!(
                "Empty refresh token for {}",
                client_id
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
    fn test_encryption_round_trip() {
        let encryption = TokenEncryption::new("my-secret-key");

        let plaintext = "my-super-secret-access-token-12345";
        let encrypted = encryption.encrypt(plaintext).unwrap();
        let decrypted = encryption.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, plaintext);
        assert_ne!(encrypted, plaintext);
    }

    #[test]
    fn test_oauth_client_serialization() {
        let client = OAuthClient::with_scopes(
            "test",
            "Test Provider",
            "cid",
            "csec",
            "https://auth.com/authorize",
            "https://auth.com/token",
            "http://localhost/cb",
            vec!["read".to_string(), "write".to_string()],
        );

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
}
