//! OAuth 2.1 PKCE utilities for Tier-2 remote MCP installs.
//!
//! PKCE (RFC 7636) replaces the client secret with a code_challenge derived
//! from a random code_verifier. This is mandatory for OAuth 2.1 and the only
//! safe way to do OAuth in a desktop app where the binary ships without a
//! secret.
//!
//! Flow:
//! 1. Generate code_verifier (43-128 random url-safe chars).
//! 2. Derive code_challenge = BASE64URL_NOPAD(SHA256(code_verifier)).
//! 3. Open browser to `{authorize_url}?...&code_challenge=...&redirect_uri=http://localhost:{port}/callback`.
//! 4. Local TcpListener on `127.0.0.1:{port}` receives `?code=...`.
//! 5. POST to token endpoint with code + code_verifier → access_token.
//! 6. Store token in keychain, return to caller.

use base64::Engine;
use rand::Rng;
use sha2::{Digest, Sha256};

// Note: we implement Display/Error manually (not thiserror) to match the
// style of InstallError in installer.rs and avoid adding a dep.

/// Random URL-safe string, 43-128 chars per RFC 7636.
///
/// We use 64 chars from `[A-Za-z0-9-._~]` (the RFC's unreserved set).
/// 64 chars of base64-ish entropy ≈ 384 bits, well above the 256-bit floor.
pub fn generate_code_verifier() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    let mut rng = rand::thread_rng();
    (0..64)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

/// S256 code_challenge: BASE64URL_NOPAD(SHA256(verifier)).
///
/// Returns a string safe to embed in a URL query parameter.
pub fn code_challenge_s256(verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let digest = hasher.finalize();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

/// Generate a cryptographically random `state` parameter for CSRF protection.
pub fn generate_state() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..32)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

/// Build the full authorization URL the user's browser should visit.
///
/// Caller passes the vendor's authorize endpoint + PKCE values + redirect URI
/// + desired scopes. This function does the percent-encoding.
pub fn build_authorize_url(
    authorize_endpoint: &str,
    client_id: &str,
    redirect_uri: &str,
    code_challenge: &str,
    state: &str,
    scopes: &[String],
) -> Result<String, url::ParseError> {
    use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
    use url::Url;

    let mut url = Url::parse(authorize_endpoint)?;
    let mut q = url.query_pairs_mut();
    q.append_pair("response_type", "code");
    q.append_pair("client_id", client_id);
    q.append_pair("redirect_uri", redirect_uri);
    q.append_pair("code_challenge", code_challenge);
    q.append_pair("code_challenge_method", "S256");
    q.append_pair("state", state);
    if !scopes.is_empty() {
        q.append_pair("scope", &scopes.join(" "));
    }
    drop(q);

    // Some vendors require PKCE values to also survive URL escaping round-trips;
    // the safe path is to leave the URL untouched by append_pair (already
    // percent-encoded by url crate). The utf8_percent_encode call is for safety
    // when re-rendering the final URL string for logging.
    let _ = utf8_percent_encode(url.as_ref(), NON_ALPHANUMERIC);
    Ok(url.to_string())
}

/// PKCE bundle the installer needs to keep around between auth and token steps.
#[derive(Debug, Clone)]
pub struct PkceContext {
    pub verifier: String,
    pub challenge: String,
    pub state: String,
}

impl PkceContext {
    /// Generate a fresh context with a new verifier, challenge, and state.
    pub fn new() -> Self {
        let verifier = generate_code_verifier();
        let challenge = code_challenge_s256(&verifier);
        let state = generate_state();
        Self {
            verifier,
            challenge,
            state,
        }
    }
}

impl Default for PkceContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse the `?code=...&state=...` from the loopback redirect.
///
/// Returns an error if state doesn't match (CSRF defense) or if the vendor
/// returned an `error=...` parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthCallback {
    Success {
        code: String,
        state: String,
    },
    Error {
        error: String,
        description: Option<String>,
    },
}

pub fn parse_callback_query(query: &str, expected_state: &str) -> Result<String, OAuthError> {
    let parsed: Vec<(String, String)> = serde_urlencoded::parse(query.as_bytes()).collect();

    // Check for OAuth error response first.
    for (k, v) in &parsed {
        if k == "error" {
            let desc = parsed
                .iter()
                .find(|(k2, _)| k2 == "error_description")
                .map(|(_, v)| v.clone());
            return Err(OAuthError::VendorError(v.clone(), desc));
        }
    }

    let code = parsed
        .iter()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.clone());
    let state = parsed
        .iter()
        .find(|(k, _)| k == "state")
        .map(|(_, v)| v.clone());

    let code = code.ok_or(OAuthError::MissingCode)?;
    let state = state.ok_or(OAuthError::MissingState)?;

    if state != expected_state {
        return Err(OAuthError::StateMismatch);
    }

    Ok(code)
}

/// Errors that can occur during the OAuth flow.
#[derive(Debug)]
pub enum OAuthError {
    /// Vendor returned `?error=...&error_description=...`.
    VendorError(String, Option<String>),
    /// Callback missing the `code` parameter.
    MissingCode,
    /// Callback missing the `state` parameter.
    MissingState,
    /// `state` didn't match — possible CSRF attempt.
    StateMismatch,
    /// User explicitly denied the auth request.
    UserDenied,
    /// Local loopback server failed to bind or accept.
    Server(String),
    /// No callback received within the timeout window.
    Timeout,
    /// Token exchange HTTP request failed or returned non-200.
    TokenExchange(String),
}

impl std::fmt::Display for OAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OAuthError::VendorError(code, desc) => {
                write!(f, "vendor OAuth error: {code}")?;
                if let Some(d) = desc {
                    write!(f, " ({d})")?;
                }
                Ok(())
            }
            OAuthError::MissingCode => write!(f, "callback missing code parameter"),
            OAuthError::MissingState => write!(f, "callback missing state parameter"),
            OAuthError::StateMismatch => write!(f, "state mismatch — possible CSRF attempt"),
            OAuthError::UserDenied => write!(f, "user denied authorization"),
            OAuthError::Server(msg) => write!(f, "loopback server error: {msg}"),
            OAuthError::Timeout => write!(f, "timeout waiting for OAuth callback"),
            OAuthError::TokenExchange(msg) => write!(f, "token exchange failed: {msg}"),
        }
    }
}

impl std::error::Error for OAuthError {}

// Use serde_urlencoded via a tiny inline dep — it's already in reqwest's tree.
mod serde_urlencoded {
    pub fn parse(bytes: &[u8]) -> impl Iterator<Item = (String, String)> + '_ {
        url::form_urlencoded::parse(bytes).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_verifier_is_correct_length() {
        let v = generate_code_verifier();
        assert!(
            (43..=128).contains(&v.len()),
            "len {} not in [43,128]",
            v.len()
        );
    }

    #[test]
    fn code_verifier_only_uses_unreserved_chars() {
        let v = generate_code_verifier();
        for c in v.chars() {
            let ok = c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~');
            assert!(ok, "char {c:?} not in unreserved set");
        }
    }

    #[test]
    fn code_verifier_is_unique_each_call() {
        let a = generate_code_verifier();
        let b = generate_code_verifier();
        assert_ne!(a, b);
    }

    #[test]
    fn code_challenge_is_base64url_no_pad() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = code_challenge_s256(verifier);
        // Known-good vector from RFC 7636 §B (worked example).
        assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn code_challenge_has_no_padding() {
        let v = generate_code_verifier();
        let c = code_challenge_s256(&v);
        assert!(!c.contains('='), "should not contain padding =");
    }

    #[test]
    fn state_is_32_chars() {
        let s = generate_state();
        assert_eq!(s.len(), 32);
    }

    #[test]
    fn pkce_context_produces_matching_challenge() {
        let ctx = PkceContext::new();
        assert_eq!(ctx.challenge, code_challenge_s256(&ctx.verifier));
        assert_eq!(ctx.state.len(), 32);
    }

    #[test]
    fn build_authorize_url_contains_required_params() {
        let url = build_authorize_url(
            "https://api.notion.com/v1/oauth/authorize",
            "shannon-desktop",
            "http://localhost:1738/callback",
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
            "abc123",
            &["openid".to_string()],
        )
        .expect("url");
        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=shannon-desktop"));
        assert!(url.contains("code_challenge=E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=abc123"));
        assert!(url.contains("scope=openid"));
    }

    #[test]
    fn build_authorize_url_handles_empty_scopes() {
        let url = build_authorize_url(
            "https://auth.example.com/authorize",
            "cid",
            "http://localhost:1/cb",
            "challenge",
            "state",
            &[],
        )
        .expect("url");
        assert!(!url.contains("scope="));
    }

    #[test]
    fn parse_callback_success_returns_code() {
        let code = parse_callback_query("code=abc123&state=mystate", "mystate").expect("code");
        assert_eq!(code, "abc123");
    }

    #[test]
    fn parse_callback_state_mismatch_errors() {
        let err = parse_callback_query("code=abc&state=wrong", "expected").unwrap_err();
        assert!(matches!(err, OAuthError::StateMismatch));
    }

    #[test]
    fn parse_callback_missing_code_errors() {
        let err = parse_callback_query("state=ok", "ok").unwrap_err();
        assert!(matches!(err, OAuthError::MissingCode));
    }

    #[test]
    fn parse_callback_missing_state_errors() {
        let err = parse_callback_query("code=abc", "expected").unwrap_err();
        assert!(matches!(err, OAuthError::MissingState));
    }

    #[test]
    fn parse_callback_vendor_error_surfaces_description() {
        let err = parse_callback_query(
            "error=access_denied&error_description=user+cancelled",
            "any",
        )
        .unwrap_err();
        match err {
            OAuthError::VendorError(code, desc) => {
                assert_eq!(code, "access_denied");
                assert_eq!(desc.as_deref(), Some("user cancelled"));
            }
            _ => panic!("expected VendorError"),
        }
    }

    #[test]
    fn parse_callback_vendor_error_without_description() {
        let err = parse_callback_query("error=invalid_request", "any").unwrap_err();
        match err {
            OAuthError::VendorError(code, desc) => {
                assert_eq!(code, "invalid_request");
                assert!(desc.is_none());
            }
            _ => panic!("expected VendorError"),
        }
    }
}
