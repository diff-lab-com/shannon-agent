//! Linear personal-API-key credential handling.
//!
//! Linear has no OAuth flow for personal use — users paste a personal
//! API key from `Settings → API → Personal API keys` (the token starts
//! with `lin_api_…`). The Shannon adapter therefore skips the OAuth +
//! callback server dance used by `slack/auth.rs` and `jira/auth.rs`
//! and persists the token directly to the keyring.
//!
//! ## Resolution order
//!
//! 1. `LINEAR_TOKEN` env var (headless / CI / quick test runs).
//! 2. Keyring `shannon-mcp-saas` / `linear-token` (interactive / desktop).
//!
//! ## Keyring layout
//!
//! - service: `shannon-mcp-saas`
//! - account: `linear-token` — the raw `lin_api_…` string
//!
//! `Token` redacts in `Debug` and supports the shared `set_token` /
//! `current_token` async surface used by `LinearClient` (see
//! `linear/api.rs`).

use thiserror::Error;

use crate::linear::api::ApiError;

/// Keyring service name. Matches `slack::auth::KEYRING_SERVICE` and
/// `jira::auth::KEYRING_SERVICE` so all Shannon SaaS credentials share
/// one vault under the desktop `connections` UX.
pub const KEYRING_SERVICE: &str = "shannon-mcp-saas";

/// Keyring account holding the Linear personal API key (`lin_api_…`).
pub const API_TOKEN_KEY: &str = "linear-token";

/// Bearer header prefix — the value is sent verbatim as
/// `Authorization: <header_value>()`.
const BEARER_PREFIX: &str = "Bearer ";

/// Marker substring that every Linear personal API key starts with.
/// Not exhaustive (Linear could mint new prefixes), but useful in
/// tests and for early misconfiguration detection.
pub const LINEAR_API_KEY_PREFIX: &str = "lin_api_";

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("no Linear token configured: set LINEAR_TOKEN or save one via TokenProvider")]
    NoToken,
    #[error("keyring error: {0}")]
    Keyring(String),
    #[error("Linear API error: {0}")]
    Api(#[from] ApiError),
}

/// Opaque redacted Linear API key. Wrap via [`Token::new`] / read via
/// [`Token::expose`] / [`Token::header_value`].
#[derive(Clone)]
pub struct Token(String);

impl Token {
    pub fn new(value: String) -> Self {
        Self(value)
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    /// `Bearer <key>` — the value Linear expects in the `Authorization`
    /// header on `https://api.linear.app/graphql`.
    pub fn header_value(&self) -> String {
        format!("{BEARER_PREFIX}{}", self.0)
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(***redacted***)")
    }
}

/// Read / write the Linear API key.
///
/// Resolution order:
/// 1. `LINEAR_TOKEN` env var (if non-empty).
/// 2. Keyring `KEYRING_SERVICE` / `API_TOKEN_KEY`.
///
/// `account_hint` is retained for API symmetry with
/// `slack::auth::TokenProvider` (multi-account support is a future
/// feature — multiple Linear workspaces — and never read today).
pub struct TokenProvider {
    pub account_hint: String,
}

impl TokenProvider {
    pub fn new(account_hint: impl Into<String>) -> Self {
        Self {
            account_hint: account_hint.into(),
        }
    }

    /// Async for symmetry with the other SaaS providers; current
    /// resolution only touches env / keyring which are sync, so the
    /// body is `async` but trivially resolved.
    pub async fn get_token(&self) -> Result<Token, AuthError> {
        if let Ok(token) = std::env::var("LINEAR_TOKEN") {
            if !token.is_empty() {
                return Ok(Token::new(token));
            }
        }
        let entry = keyring::Entry::new(KEYRING_SERVICE, API_TOKEN_KEY)
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        match entry.get_password() {
            Ok(t) => Ok(Token::new(t)),
            Err(keyring::Error::NoEntry) => Err(AuthError::NoToken),
            Err(e) => Err(AuthError::Keyring(e.to_string())),
        }
    }

    /// Persist a personal API key to the keyring. Returns the password
    /// line unchanged so the caller can stash it alongside other
    /// payload metadata.
    pub fn save_to_keyring(&self, token: &Token) -> Result<(), AuthError> {
        keyring::Entry::new(KEYRING_SERVICE, API_TOKEN_KEY)
            .map_err(|e| AuthError::Keyring(e.to_string()))?
            .set_password(token.expose())
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        Ok(())
    }

    /// Remove the keyring entry (best-effort: missing entries are not
    /// an error). Used by the desktop `connections` UX on "Sign out".
    pub fn clear_keyring(&self) -> Result<(), AuthError> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, API_TOKEN_KEY)
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(AuthError::Keyring(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_debug_redacts_secret() {
        let t = Token::new("lin_api_supersecret123".into());
        let dbg = format!("{t:?}");
        assert!(!dbg.contains("supersecret"));
        assert!(
            dbg.contains("redacted"),
            "Debug impl should say redacted; got: {dbg}"
        );
    }

    #[test]
    fn token_header_value_format() {
        let t = Token::new("lin_api_abc".into());
        assert_eq!(t.header_value(), "Bearer lin_api_abc");
    }

    #[test]
    fn account_hint_round_trip() {
        assert_eq!(TokenProvider::new("ws-acme").account_hint, "ws-acme");
    }

    #[test]
    fn prefix_is_stable() {
        // Anchors the prefix string so a careless rename doesn't slip in.
        assert_eq!(LINEAR_API_KEY_PREFIX, "lin_api_");
    }

    #[test]
    fn keyring_account_is_stable() {
        assert_eq!(API_TOKEN_KEY, "linear-token");
        assert_eq!(KEYRING_SERVICE, "shannon-mcp-saas");
    }
}
