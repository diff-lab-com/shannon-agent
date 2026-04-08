//! OAuth authentication

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// OAuth configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub auth_url: String,
    pub token_url: String,
    pub scopes: Vec<String>,
}

/// OAuth token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub token_type: String,
}

/// OAuth service
pub struct OAuthService {
    configs: HashMap<String, OAuthConfig>,
    tokens: HashMap<String, OAuthToken>,
}

impl OAuthService {
    pub fn new() -> Self {
        Self {
            configs: HashMap::new(),
            tokens: HashMap::new(),
        }
    }

    /// Register an OAuth provider
    pub fn register_provider(&mut self, name: String, config: OAuthConfig) {
        self.configs.insert(name, config);
    }

    /// Get authorization URL
    pub fn get_auth_url(&self, provider: &str, state: &str) -> Result<String, OAuthError> {
        let config = self.configs.get(provider)
            .ok_or_else(|| OAuthError::ProviderNotFound(provider.to_string()))?;

        let scope = config.scopes.join(" ");
        let auth_url = format!(
            "{}?client_id={}&redirect_uri={}&scope={}&response_type=code&state={}",
            config.auth_url, config.client_id, config.redirect_uri, scope, state
        );

        Ok(auth_url)
    }

    /// Exchange code for token
    pub async fn exchange_code_for_token(
        &mut self,
        provider: &str,
        code: &str,
    ) -> Result<OAuthToken, OAuthError> {
        let config = self.configs.get(provider)
            .ok_or_else(|| OAuthError::ProviderNotFound(provider.to_string()))?;

        // TODO: Implement actual token exchange
        let token = OAuthToken {
            access_token: "placeholder_token".to_string(),
            refresh_token: None,
            expires_at: None,
            token_type: "Bearer".to_string(),
        };

        self.tokens.insert(provider.to_string(), token.clone());
        Ok(token)
    }

    /// Refresh token
    pub async fn refresh_token(&mut self, provider: &str) -> Result<OAuthToken, OAuthError> {
        if let Some(token) = self.tokens.get(provider) {
            if let Some(refresh_token) = &token.refresh_token {
                // TODO: Implement actual refresh
                Ok(token.clone())
            } else {
                Err(OAuthError::NoRefreshToken)
            }
        } else {
            Err(OAuthError::NotAuthenticated(provider.to_string()))
        }
    }

    /// Get stored token
    pub fn get_token(&self, provider: &str) -> Option<&OAuthToken> {
        self.tokens.get(provider)
    }
}

impl Default for OAuthService {
    fn default() -> Self {
        Self::new()
    }
}

/// OAuth errors
#[derive(Debug, thiserror::Error)]
pub enum OAuthError {
    #[error("Provider not found: {0}")]
    ProviderNotFound(String),

    #[error("Not authenticated: {0}")]
    NotAuthenticated(String),

    #[error("No refresh token available")]
    NoRefreshToken,

    #[error("Exchange failed: {0}")]
    ExchangeFailed(String),

    #[error("Network error: {0}")]
    NetworkError(String),
}
