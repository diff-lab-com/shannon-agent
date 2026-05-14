//! Tests for MCP authentication module
//!
//! Covers OAuth2Provider, ApiKeyProvider, token storage, PKCE helpers,
//! auth error variants, and header injection.

use shannon_mcp::auth::{AuthProvider, ApiKeyProvider, OAuth2Provider, MemoryTokenStorage, TokenStorage, AuthError};
use std::collections::HashMap;

// ============================================================================
// ApiKeyProvider Tests
// ============================================================================

#[tokio::test]
async fn test_api_key_provider_is_valid_with_nonempty_key() {
    let provider = ApiKeyProvider::new("sk-test-12345");
    assert!(provider.is_valid().await);
}

#[tokio::test]
async fn test_api_key_provider_is_invalid_with_empty_key() {
    let provider = ApiKeyProvider::new("");
    assert!(!provider.is_valid().await);
}

#[tokio::test]
async fn test_api_key_provider_default_header() {
    let provider = ApiKeyProvider::new("my-secret");
    let mut headers = HashMap::new();
    provider.add_auth_headers(&mut headers).await.unwrap();

    assert_eq!(headers.get("X-API-Key").unwrap(), "my-secret");
    assert_eq!(headers.len(), 1);
}

#[tokio::test]
async fn test_api_key_provider_custom_header_with_prefix() {
    let provider = ApiKeyProvider::new("tok-abc")
        .with_header_name("Authorization")
        .with_prefix("Bearer");

    let mut headers = HashMap::new();
    provider.add_auth_headers(&mut headers).await.unwrap();

    assert_eq!(headers.get("Authorization").unwrap(), "Bearer tok-abc");
    assert_eq!(headers.len(), 1);
}

#[tokio::test]
async fn test_api_key_provider_get_token_returns_key() {
    let provider = ApiKeyProvider::new("key-xyz");
    let token = provider.get_token().await.unwrap();
    assert_eq!(token, "key-xyz");
}

#[tokio::test]
async fn test_api_key_provider_refresh_is_noop() {
    let provider = ApiKeyProvider::new("key-xyz");
    assert!(provider.refresh_token().await.is_ok());
}

// ============================================================================
// OAuth2Provider Tests
// ============================================================================

#[tokio::test]
async fn test_oauth2_provider_construction() {
    let provider = OAuth2Provider::new(
        "test-client-id",
        "https://auth.example.com/authorize",
        "https://auth.example.com/token",
        "https://app.example.com/callback",
    );

    // Verify construction doesn't panic and defaults are sensible.
    assert!(!provider.is_valid().await, "no token set yet, should be invalid");
    assert!(provider.get_token().await.is_err(), "no token, get_token should fail");
}

#[tokio::test]
async fn test_oauth2_provider_with_scopes_and_secret() {
    let provider = OAuth2Provider::new(
        "cid",
        "https://auth.example.com/authorize",
        "https://auth.example.com/token",
        "https://app.example.com/callback",
    )
    .with_client_secret("s3cret")
    .with_scopes(vec!["read".to_string(), "write".to_string()]);

    // Should not be valid yet (no tokens set).
    assert!(!provider.is_valid().await);
}

#[tokio::test]
async fn test_oauth2_authorization_url_contains_pkce_params() {
    let provider = OAuth2Provider::new(
        "my-client",
        "https://auth.example.com/authorize",
        "https://auth.example.com/token",
        "https://app.example.com/callback",
    )
    .with_scopes(vec!["openid".to_string()]);

    let (url, _state) = provider.get_authorization_url().await.unwrap();

    assert!(url.starts_with("https://auth.example.com/authorize?"));
    assert!(url.contains("response_type=code"));
    assert!(url.contains("client_id=my-client"));
    assert!(url.contains("code_challenge="));
    assert!(url.contains("code_challenge_method=S256"));
    assert!(url.contains("state="));
    assert!(url.contains("scope=openid"));
}

#[tokio::test]
async fn test_oauth2_authorization_url_without_scopes() {
    let provider = OAuth2Provider::new(
        "cid",
        "https://auth.example.com/authorize",
        "https://auth.example.com/token",
        "https://app.example.com/callback",
    );

    let (url, _state) = provider.get_authorization_url().await.unwrap();
    assert!(!url.contains("&scope="));
}

#[tokio::test]
async fn test_oauth2_no_token_means_not_expired() {
    // When no token has ever been set, is_expired returns false
    // (there is no expiry to check against).
    let provider = OAuth2Provider::new(
        "cid",
        "https://auth.example.com/authorize",
        "https://auth.example.com/token",
        "https://app.example.com/callback",
    );
    assert!(!provider.is_expired().await, "no token set, so not expired");
    assert!(!provider.is_valid().await, "no token set, so not valid");
}

#[tokio::test]
async fn test_oauth2_no_token_get_token_fails() {
    let provider = OAuth2Provider::new(
        "cid",
        "https://auth.example.com/authorize",
        "https://auth.example.com/token",
        "https://app.example.com/callback",
    );
    let err = provider.get_token().await.unwrap_err();
    assert!(matches!(err, AuthError::TokenExpired));
}

#[tokio::test]
async fn test_oauth2_add_auth_headers_fails_without_token() {
    let provider = OAuth2Provider::new(
        "cid",
        "https://auth.example.com/authorize",
        "https://auth.example.com/token",
        "https://app.example.com/callback",
    );
    let mut headers = HashMap::new();
    let result = provider.add_auth_headers(&mut headers).await;
    assert!(result.is_err(), "Should fail because no token is set");
    assert!(headers.is_empty(), "No headers should be injected without a token");
}

#[tokio::test]
async fn test_oauth2_exchange_code_without_verifier_fails() {
    let provider = OAuth2Provider::new(
        "cid",
        "https://auth.example.com/authorize",
        "https://auth.example.com/token",
        "https://app.example.com/callback",
    );

    // Don't call get_authorization_url first, so no verifier is stored.
    let result = provider.exchange_code("some-code", "some-state").await;
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("code verifier") || msg.contains("verifier") || msg.contains("state"));
}

#[tokio::test]
async fn test_oauth2_refresh_without_refresh_token_fails() {
    let provider = OAuth2Provider::new(
        "cid",
        "https://auth.example.com/authorize",
        "https://auth.example.com/token",
        "https://app.example.com/callback",
    );

    let result = provider.refresh_access_token().await;
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.to_lowercase().contains("refresh token"));
}

// ============================================================================
// AuthError variant tests
// ============================================================================

#[test]
fn test_auth_error_variants_format() {
    let e1 = AuthError::OAuth("bad request".to_string());
    assert!(e1.to_string().contains("bad request"));

    let e2 = AuthError::InvalidToken("not bearer".to_string());
    assert!(e2.to_string().contains("not bearer"));

    let e3 = AuthError::AuthenticationFailed("denied".to_string());
    assert!(e3.to_string().contains("denied"));

    let e4 = AuthError::TokenExpired;
    assert!(e4.to_string().to_lowercase().contains("expired"));

    let e5 = AuthError::Configuration("missing field".to_string());
    assert!(e5.to_string().contains("missing field"));
}

// ============================================================================
// MemoryTokenStorage Tests
// ============================================================================

#[tokio::test]
async fn test_memory_token_storage_save_load_delete() {
    let storage = MemoryTokenStorage::new();

    // Nothing stored yet.
    assert!(storage.load_token("key1").await.unwrap().is_none());

    // Save and load.
    storage.save_token("key1", "value1").await.unwrap();
    assert_eq!(storage.load_token("key1").await.unwrap().as_deref(), Some("value1"));

    // Overwrite.
    storage.save_token("key1", "value2").await.unwrap();
    assert_eq!(storage.load_token("key1").await.unwrap().as_deref(), Some("value2"));

    // Delete.
    storage.delete_token("key1").await.unwrap();
    assert!(storage.load_token("key1").await.unwrap().is_none());
}

#[tokio::test]
async fn test_memory_token_storage_independent_keys() {
    let storage = MemoryTokenStorage::new();
    storage.save_token("a", "1").await.unwrap();
    storage.save_token("b", "2").await.unwrap();

    assert_eq!(storage.load_token("a").await.unwrap().as_deref(), Some("1"));
    assert_eq!(storage.load_token("b").await.unwrap().as_deref(), Some("2"));

    storage.delete_token("a").await.unwrap();
    assert!(storage.load_token("a").await.unwrap().is_none());
    assert_eq!(storage.load_token("b").await.unwrap().as_deref(), Some("2"));
}

#[tokio::test]
async fn test_memory_token_storage_delete_nonexistent_is_ok() {
    let storage = MemoryTokenStorage::new();
    assert!(storage.delete_token("nope").await.is_ok());
}
