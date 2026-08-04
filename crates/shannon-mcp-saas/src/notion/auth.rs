//! Notion authentication — internal-integration token (Bearer) + keyring.
//!
//! Notion's official API is auth'd by an "internal integration" secret,
//! not OAuth. The token is a `secret_…` or `ntn_…` opaque string that the
//! user pastes into the Notion UI (`Settings → Integrations`) and is
//! exchanged as `Authorization: Bearer <token>` against
//! `https://api.notion.com/v1/`. We mirror the Slack/Jira shape: an
//! opaque redacted [`Token`], a [`TokenProvider`] that resolves the
//! credential from env or keyring, and `set_token`/`current_token` for
//! in-memory rotation.
//!
//! ## Keyring layout
//!
//! - service: `shannon-mcp-saas`
//! - account: `notion-token`
//!
//! The service is `shannon-mcp-saas` (matches the crate name) rather
//! than `shannon` (Slack/Jira) so a rebrand or a future split between
//! the desktop binary and the server-side SaaS runner can re-target
//! without colliding. Slack and Jira are not affected — they keep
//! their own `KEYRING_SERVICE` constants.

use serde::Deserialize;
use thiserror::Error;

use crate::notion::api::ApiError;

/// Keyring service name. Distinct from the desktop `shannon` so the
/// SaaS crate can run in headless / CI contexts without colliding
/// with the desktop credential store.
pub const KEYRING_SERVICE: &str = "shannon-mcp-saas";

/// Keyring account for the Notion internal-integration secret.
pub const TOKEN_KEY: &str = "notion-token";

/// Environment variable used as a headless override. Read first by
/// [`TokenProvider::get_token`] so CI / unit tests can supply a token
/// without touching the keyring.
pub const ENV_TOKEN: &str = "NOTION_TOKEN";

/// Stable identifier for the Notion SaaS, used as the prefix in tool
/// names (`notion_search_pages` etc.) and the public-facing module
/// label. Mirrors `slack::auth` and `jira::auth` having stable key
/// constants in the same place.
pub const NOTION: &str = "notion";

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("no Notion token configured: set NOTION_TOKEN or store it via TokenProvider::save")]
    NoToken,
    #[error("keyring error: {0}")]
    Keyring(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Notion API error: {0}")]
    Api(#[from] ApiError),
}

/// Opaque redacted token. Use [`Token::expose`] / [`Token::header_value`].
#[derive(Clone)]
pub struct Token(String);

impl Token {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
    /// `Bearer <token>` — Notion's auth scheme.
    pub fn header_value(&self) -> String {
        format!("Bearer {}", self.0)
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Mirror the Slack/Jira redactor so logs/traces cannot leak the
        // secret even when a `Token` is unintentionally formatted.
        f.write_str("Token(***redacted***)")
    }
}

/// Holder of a single Notion workspace credential. Created with an
/// `account_hint` (e.g. the workspace name) so error messages and keyring
/// operations can disambiguate when more than one workspace is
/// configured.
pub struct TokenProvider {
    pub account_hint: String,
}

impl TokenProvider {
    pub fn new(account_hint: impl Into<String>) -> Self {
        Self {
            account_hint: account_hint.into(),
        }
    }

    /// Look up a token. `NOTION_TOKEN` env var takes precedence (so
    /// headless runs and tests can override without keyring access),
    /// then the keyring entry at `shannon-mcp-saas/notion-token`.
    pub async fn get_token(&self) -> Result<Token, AuthError> {
        if let Ok(token) = std::env::var(ENV_TOKEN) {
            if !token.is_empty() {
                return Ok(Token::new(token));
            }
        }
        let entry = keyring::Entry::new(KEYRING_SERVICE, TOKEN_KEY)
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        match entry.get_password() {
            Ok(token) => Ok(Token::new(token)),
            Err(keyring::Error::NoEntry) => Err(AuthError::NoToken),
            Err(e) => Err(AuthError::Keyring(e.to_string())),
        }
    }

    /// Persist the integration secret to the keyring. The value is
    /// stored verbatim (no envelope) — Notion tokens are already
    /// opaque and self-authenticating.
    pub fn save_to_keyring(&self, token: &Token) -> Result<(), AuthError> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, TOKEN_KEY)
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        entry
            .set_password(token.expose())
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        Ok(())
    }

    /// Best-effort delete — surfaces keyring errors as `AuthError::Keyring`
    /// but ignores `NoEntry` so the operation is idempotent.
    pub fn clear(&self) -> Result<(), AuthError> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, TOKEN_KEY)
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(AuthError::Keyring(e.to_string())),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct NotionTokenShape {
    /// Kept for documentation / introspection helpers that may parse
    /// the raw value to confirm it is well-formed. Notion tokens are
    /// prefixed `secret_` (legacy) or `ntn_` (current). Field is never
    /// read in production code paths.
    #[rustfmt::skip]
    #[allow(dead_code)] // KEEP: shape-validation helper for future prefix checks / telemetry.
    pub prefix: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_debug_redacts_secret() {
        let t = Token::new("secret_super_secret_value");
        let dbg = format!("{t:?}");
        assert!(!dbg.contains("secret_super_secret_value"));
        assert!(dbg.contains("redacted"));
    }

    #[test]
    fn token_header_value_is_bearer() {
        let t = Token::new("ntn_xyz");
        assert_eq!(t.header_value(), "Bearer ntn_xyz");
    }

    #[test]
    fn keyring_constants_are_stable() {
        assert_eq!(KEYRING_SERVICE, "shannon-mcp-saas");
        assert_eq!(TOKEN_KEY, "notion-token");
        assert_eq!(ENV_TOKEN, "NOTION_TOKEN");
    }

    #[test]
    fn account_hint_round_trip() {
        let p = TokenProvider::new("acme");
        assert_eq!(p.account_hint, "acme");
    }
}
