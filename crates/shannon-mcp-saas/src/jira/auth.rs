//! Jira authentication — OAuth 2.0 (3LO, PKCE S256) + API token (Basic auth).
//!
//! Two paths, same [`TokenProvider`] interface:
//!
//! 1. **OAuth** — authorization-code flow with PKCE S256. Local callback
//!    on `127.0.0.1:0` (ephemeral port). The user grants on
//!    `auth.atlassian.com`, Atlassian redirects to the local callback,
//!    `code` is exchanged for an access token + refresh token, both
//!    written to the OS keyring under `service=shannon`, accounts
//!    `jira/api-token` and `jira/oauth`.
//! 2. **API token** — `JIRA_API_TOKEN` env var (or keyring fallback).
//!    Jira Cloud accepts HTTP Basic with `email:api_token`. We store the
//!    combined `email:token` string in the keyring so the basic-auth
//!    header is reconstructable.
//!
//! ## Atlassian OAuth endpoints
//!
//! - Authorize: `https://auth.atlassian.com/authorize`
//! - Token:     `https://auth.atlassian.com/oauth/token`
//! - Accessible-resources: `https://api.atlassian.com/oauth/token/accessible-resources`
//!   (returns the cloudid the API needs in the `Authorization: Bearer` header path
//!   `https://api.atlassian.com/ex/jira/<cloudid>/rest/api/3/...`).
//!
//! ## Keyring layout
//!
//! - service: `shannon`
//! - account: `jira/api-token` — `email:token` for API-token basic auth
//!   `jira/oauth` — access token JSON (and `refresh_token`)
//!   `jira/cloudid` — accessible-resources cloud id (OAuth only)

use std::net::SocketAddr;

use base64::Engine;
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::sync::oneshot;

use crate::jira::api::ApiError;

/// Keyring service name. Matches Slack's constant so the same desktop
/// binary can address both SaaSes with the same credentials store.
pub const KEYRING_SERVICE: &str = "shannon";

/// Keyring account for the API-token path (`email:token` basic auth).
pub const API_TOKEN_KEY: &str = "jira/api-token";
/// Keyring account for the OAuth access/refresh blob.
pub const OAUTH_TOKEN_KEY: &str = "jira/oauth";
/// Keyring account for the OAuth cloud id (so we can address the
/// tenant-specific REST endpoint without re-discovering every boot).
pub const CLOUD_ID_KEY: &str = "jira/cloudid";

/// OAuth scopes for Jira Cloud REST API v3.
/// - `read:jira-work`   — search + get issue
/// - `read:jira-user`   — user lookup (handy for transitions' assignee)
/// - `write:jira-work`  — create + transition issue
pub const OAUTH_SCOPES: &str = "read:jira-work read:jira-user write:jira-work offline_access";

const AUTHORIZE_URL: &str = "https://auth.atlassian.com/authorize";
const ACCESS_TOKEN_URL: &str = "https://auth.atlassian.com/oauth/token";
const ACCESSIBLE_RESOURCES_URL: &str = "https://api.atlassian.com/oauth/token/accessible-resources";

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("no Jira token configured: set JIRA_API_TOKEN or complete OAuth")]
    NoToken,
    // CSRF state — never log expected/actual. See slack/auth.rs for the
    // timing-oracle rationale (constant-time comparison + no logging).
    #[error("OAuth state mismatch (possible CSRF)")]
    StateMismatch,
    #[error("OAuth callback missing code")]
    MissingCode,
    #[error("token exchange failed: {0}")]
    Exchange(String),
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("keyring error: {0}")]
    Keyring(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Jira API error: {0}")]
    Api(#[from] ApiError),
}

/// Opaque redacted token. Use [`Token::expose`] / [`Token::header_value`].
#[derive(Clone)]
pub struct Token(String);

impl Token {
    pub fn new(value: String) -> Self {
        Self(value)
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
    /// `Bearer <token>` — Atlassian 3LO format.
    pub fn header_value(&self) -> String {
        format!("Bearer {}", self.0)
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(***redacted***)")
    }
}

/// PKCE S256 pair per RFC 7636.
#[derive(Debug, Clone)]
pub struct PkcePair {
    pub verifier: String,
    pub challenge: String,
}

impl PkcePair {
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        let digest = hasher.finalize();
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
        Self {
            verifier,
            challenge,
        }
    }
}

/// Random 32-char hex `state` parameter for CSRF protection.
pub fn generate_state() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    let mut out = String::with_capacity(32);
    const HEX: &[u8] = b"0123456789abcdef";
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Build the `/authorize` URL.
pub fn build_authorize_url(
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    pkce: &PkcePair,
) -> String {
    let mut url = url::Url::parse(AUTHORIZE_URL).expect("static Atlassian URL parses");
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("audience", "api.atlassian.com");
        q.append_pair("client_id", client_id);
        q.append_pair("scope", OAUTH_SCOPES);
        q.append_pair("redirect_uri", redirect_uri);
        q.append_pair("state", state);
        q.append_pair("response_type", "code");
        q.append_pair("prompt", "consent");
        q.append_pair("code_challenge", &pkce.challenge);
        q.append_pair("code_challenge_method", "S256");
    }
    url.to_string()
}

/// OAuth flow runner. Same lifecycle as `slack::auth::OAuthFlow`.
pub struct OAuthFlow {
    pub bind_addr: SocketAddr,
    pub state: String,
    pub pkce: PkcePair,
    receiver: Option<oneshot::Receiver<Result<OAuthTokens, AuthError>>>,
    _server: tokio::task::JoinHandle<()>,
}

/// Bundle of values the callback server hands back to the caller.
#[derive(Debug, Clone)]
pub struct OAuthTokens {
    pub access_token: Token,
    pub refresh_token: Option<String>,
    pub expires_in: u64,
    pub cloudid: String,
}

impl OAuthFlow {
    /// Bind to `127.0.0.1:0` (ephemeral). `requested_port` is ignored,
    /// retained for API parity with the github flow.
    pub async fn start(
        client_id: String,
        client_secret: String,
        _requested_port: u16,
    ) -> Result<Self, AuthError> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let bind_addr = listener.local_addr()?;
        let state = generate_state();
        let pkce = PkcePair::generate();
        let (tx, rx) = oneshot::channel::<Result<OAuthTokens, AuthError>>();

        let server_state = state.clone();
        let server_pkce = pkce.clone();
        let server = tokio::spawn(async move {
            if let Err(e) = run_callback_server(
                listener,
                tx,
                server_state,
                server_pkce,
                client_id,
                client_secret,
            )
            .await
            {
                tracing::error!(error = %e, "Jira OAuth callback failed");
            }
        });

        Ok(Self {
            bind_addr,
            state,
            pkce,
            receiver: Some(rx),
            _server: server,
        })
    }

    pub async fn wait_for_tokens(mut self) -> Result<OAuthTokens, AuthError> {
        let rx = self.receiver.take().expect("receiver only consumable once");
        rx.await
            .map_err(|_| AuthError::Exchange("callback channel closed".into()))?
    }

    /// Exchange the authorization code for tokens, then resolve the
    /// cloudid via the accessible-resources endpoint. Exposed so tests
    /// can drive just the exchange step.
    pub async fn exchange_code(
        &self,
        client_id: &str,
        client_secret: &str,
        code: &str,
    ) -> Result<OAuthTokens, AuthError> {
        let http = reqwest::Client::builder()
            .user_agent("shannon-mcp-saas/0.7")
            .build()
            .map_err(|e| AuthError::Http(e.to_string()))?;

        let resp = http
            .post(ACCESS_TOKEN_URL)
            .header("User-Agent", "shannon-mcp-saas/0.7")
            .form(&[
                ("grant_type", "authorization_code"),
                ("client_id", client_id),
                ("client_secret", client_secret),
                ("code", code),
                (
                    "redirect_uri",
                    &format!("http://{}/callback", self.bind_addr),
                ),
                ("code_verifier", self.pkce.verifier.as_str()),
            ])
            .send()
            .await
            .map_err(|e| AuthError::Http(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AuthError::Exchange(format!("HTTP {status}: {body}")));
        }
        let parsed: AccessTokenResponse = resp
            .json()
            .await
            .map_err(|e| AuthError::Exchange(format!("parse: {e}")))?;
        let access = parsed
            .access_token
            .ok_or_else(|| AuthError::Exchange("missing access_token".into()))?;
        let expires_in = parsed.expires_in.unwrap_or(3600);
        let refresh = parsed.refresh_token;

        // Resolve cloudid via accessible-resources. Atlassian issues
        // tokens that may bind to multiple resources; we use the first.
        let cloudid = resolve_cloudid(&http, &access).await?;

        Ok(OAuthTokens {
            access_token: Token::new(access),
            refresh_token: refresh,
            expires_in,
            cloudid,
        })
    }
}

#[derive(Debug, Deserialize)]
struct AccessTokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    // KEEP: Atlassian surfaces `error`/`error_description` on token
    // endpoint failures. We surface them via `Exchange(err)` instead,
    // but the serde fields are kept so future logging / error shaping
    // can leverage them without re-deriving.
    #[serde(default)]
    #[allow(dead_code)]
    error: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AccessibleResource {
    id: String,
}

async fn resolve_cloudid(http: &reqwest::Client, access_token: &str) -> Result<String, AuthError> {
    let resp = http
        .get(ACCESSIBLE_RESOURCES_URL)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("User-Agent", "shannon-mcp-saas/0.7")
        .send()
        .await
        .map_err(|e| AuthError::Http(e.to_string()))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(AuthError::Exchange(format!(
            "accessible-resources HTTP {status}: {body}"
        )));
    }
    let resources: Vec<AccessibleResource> = resp
        .json()
        .await
        .map_err(|e| AuthError::Exchange(format!("parse accessible-resources: {e}")))?;
    resources
        .into_iter()
        .next()
        .map(|r| r.id)
        .ok_or_else(|| AuthError::Exchange("no accessible resources for token".into()))
}

/// Callback HTTP server. Identical pattern to `slack/auth.rs`:
/// constant-time state comparison, no `expected_state` logging,
/// first valid request wins, channel reserved for the valid request.
async fn run_callback_server(
    listener: tokio::net::TcpListener,
    tx: oneshot::Sender<Result<OAuthTokens, AuthError>>,
    expected_state: String,
    pkce: PkcePair,
    client_id: String,
    client_secret: String,
) -> Result<(), AuthError> {
    use tokio::io::AsyncReadExt;

    let (mut stream, _peer) = listener.accept().await?;
    let mut buf = vec![0u8; 8192];
    let n = stream.read(&mut buf).await?;
    let req = String::from_utf8_lossy(&buf[..n]);
    let first_line = req.lines().next().unwrap_or("");
    let (path, _rest) = match first_line.split_once(' ') {
        Some((m, rest)) => (m, rest),
        None => {
            let _ = send_400(&mut stream, "malformed request").await;
            let _ = tx.send(Err(AuthError::MissingCode));
            return Ok(());
        }
    };
    let (route, query) = match path.split_once('?') {
        Some((r, q)) => (r, q),
        None => (path, ""),
    };
    if route != "/callback" {
        let _ = send_400(&mut stream, "unexpected route").await;
        let _ = tx.send(Err(AuthError::MissingCode));
        return Ok(());
    }
    let mut code: Option<String> = None;
    let mut state_param: Option<String> = None;
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            let v = url_decode(v);
            match k {
                "code" => code = Some(v),
                "state" => state_param = Some(v),
                _ => {}
            }
        }
    }
    let state_bytes = expected_state.as_bytes();
    let state_ok = state_param
        .as_deref()
        .map(|s| constant_time_eq(s.as_bytes(), state_bytes))
        .unwrap_or(false);
    if !state_ok {
        let _ = send_400(&mut stream, "state mismatch").await;
        let _ = tx.send(Err(AuthError::StateMismatch));
        return Ok(());
    }
    let code = match code {
        Some(c) => c,
        None => {
            let _ = send_400(&mut stream, "missing code").await;
            let _ = tx.send(Err(AuthError::MissingCode));
            return Ok(());
        }
    };
    let flow = OAuthFlow {
        bind_addr: listener.local_addr()?,
        state: expected_state,
        pkce,
        receiver: None,
        _server: tokio::spawn(async {}),
    };
    let result = flow.exchange_code(&client_id, &client_secret, &code).await;
    match &result {
        Ok(_) => {
            let _ = send_html(&mut stream, "OAuth complete — you can close this tab.").await;
        }
        Err(_) => {
            let _ = send_400(&mut stream, "exchange failed").await;
        }
    }
    let _ = tx.send(result);
    Ok(())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

async fn send_html<W: tokio::io::AsyncWrite + Unpin + Send>(
    mut stream: W,
    body: &str,
) -> tokio::io::Result<()> {
    let body = body.replace('\n', "<br/>");
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body,
    );
    stream.write_all(resp.as_bytes()).await?;
    stream.shutdown().await?;
    Ok(())
}

async fn send_400<W: tokio::io::AsyncWrite + Unpin + Send>(
    mut stream: W,
    reason: &str,
) -> tokio::io::Result<()> {
    let body = format!("OAuth error: {reason}");
    let resp = format!(
        "HTTP/1.1 400 Bad Request\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body,
    );
    stream.write_all(resp.as_bytes()).await?;
    stream.shutdown().await?;
    Ok(())
}

/// Tiny form-decode: handles `+` → space and `%XX`.
fn url_decode(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = hex_value(bytes[i + 1]);
                let lo = hex_value(bytes[i + 2]);
                match (hi, lo) {
                    (Some(h), Some(l)) => {
                        out.push((h << 4) | l);
                        i += 3;
                    }
                    _ => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Credential source. Resolves a token via API-token env → keyring → OAuth keyring.
pub struct TokenProvider {
    pub account_hint: String,
}

impl TokenProvider {
    pub fn new(account_hint: impl Into<String>) -> Self {
        Self {
            account_hint: account_hint.into(),
        }
    }

    /// Look up a token. `JIRA_API_TOKEN` env var takes precedence (Basic
    /// auth path), then the keyring `jira/api-token`, then the OAuth
    /// entry `jira/oauth`. Whichever is found is wrapped in a [`Token`]
    /// — at the API layer we route either to Basic or Bearer based on
    /// the kind returned.
    pub async fn get_token(&self) -> Result<CredentialKind, AuthError> {
        if let Ok(pat) = std::env::var("JIRA_API_TOKEN") {
            if !pat.is_empty() {
                let email = std::env::var("JIRA_EMAIL").unwrap_or_default();
                return Ok(CredentialKind::ApiToken { email, token: pat });
            }
        }
        let entry = keyring::Entry::new(KEYRING_SERVICE, API_TOKEN_KEY)
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        match entry.get_password() {
            Ok(s) => {
                let (email, token) = match s.split_once(':') {
                    Some((e, t)) => (e.to_string(), t.to_string()),
                    None => (String::new(), s),
                };
                Ok(CredentialKind::ApiToken { email, token })
            }
            Err(keyring::Error::NoEntry) => {
                // Try OAuth entry.
                let oauth = keyring::Entry::new(KEYRING_SERVICE, OAUTH_TOKEN_KEY)
                    .map_err(|e| AuthError::Keyring(e.to_string()))?;
                match oauth.get_password() {
                    Ok(s) => Ok(CredentialKind::OAuth {
                        access_token: s,
                        cloudid: None,
                    }),
                    Err(keyring::Error::NoEntry) => Err(AuthError::NoToken),
                    Err(e) => Err(AuthError::Keyring(e.to_string())),
                }
            }
            Err(e) => Err(AuthError::Keyring(e.to_string())),
        }
    }

    /// Persist an API-token `email:token` pair.
    pub fn save_api_token(&self, email: &str, token: &str) -> Result<(), AuthError> {
        let combined = format!("{email}:{token}");
        let entry = keyring::Entry::new(KEYRING_SERVICE, API_TOKEN_KEY)
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        entry
            .set_password(&combined)
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        Ok(())
    }

    /// Persist a set of OAuth tokens. Also stores the cloudid so the
    /// next process boot skips the accessible-resources round-trip.
    pub fn save_oauth(&self, tokens: &OAuthTokens) -> Result<(), AuthError> {
        let oauth = keyring::Entry::new(KEYRING_SERVICE, OAUTH_TOKEN_KEY)
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        oauth
            .set_password(tokens.access_token.expose())
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        let cid = keyring::Entry::new(KEYRING_SERVICE, CLOUD_ID_KEY)
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        cid.set_password(&tokens.cloudid)
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        Ok(())
    }
}

/// What the credential resolves to. API tokens use Basic auth; OAuth
/// uses Bearer with the resolved cloudid.
#[derive(Debug, Clone)]
pub enum CredentialKind {
    ApiToken {
        email: String,
        token: String,
    },
    OAuth {
        access_token: String,
        cloudid: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_pair_has_correct_shape() {
        let pkce = PkcePair::generate();
        assert_eq!(pkce.verifier.len(), 43);
        assert_eq!(pkce.challenge.len(), 43);
        assert_ne!(pkce.verifier, pkce.challenge);
    }

    #[test]
    fn generate_state_is_random_and_hex() {
        let a = generate_state();
        let b = generate_state();
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn build_authorize_url_contains_required_params() {
        let pkce = PkcePair::generate();
        let state = "abc123";
        let url = build_authorize_url("client_id", "http://127.0.0.1:12345/callback", state, &pkce);
        assert!(url.contains("client_id=client_id"));
        assert!(url.contains("state=abc123"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("audience=api.atlassian.com"));
    }

    #[test]
    fn url_decode_handles_plus_and_percent() {
        assert_eq!(url_decode("hello+world"), "hello world");
        assert_eq!(url_decode("a%20b"), "a b");
        assert_eq!(url_decode("plain"), "plain");
    }

    #[test]
    fn token_debug_redacts_secret() {
        let t = Token::new("super-secret-123".into());
        let dbg = format!("{t:?}");
        assert!(!dbg.contains("super-secret-123"));
        assert!(dbg.contains("redacted"));
    }

    #[test]
    fn token_header_value_format() {
        let t = Token::new("abc".into());
        assert_eq!(t.header_value(), "Bearer abc");
    }

    #[test]
    fn scopes_contain_read_and_write() {
        for s in [
            "read:jira-work",
            "read:jira-user",
            "write:jira-work",
            "offline_access",
        ] {
            assert!(OAUTH_SCOPES.contains(s), "missing scope {s}");
        }
    }

    #[test]
    fn account_hint_round_trip() {
        assert_eq!(
            TokenProvider::new("user@example.com").account_hint,
            "user@example.com"
        );
    }
}
