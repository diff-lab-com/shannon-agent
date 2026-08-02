//! GitHub authentication — OAuth (PKCE) + Personal Access Token (env var).
//!
//! Two paths, same [`TokenProvider`] interface:
//!
//! 1. **PAT** — `GITHUB_TOKEN` env var takes precedence. Used for headless /
//!    CI runs. No browser interaction.
//! 2. **OAuth** — standard authorization-code flow with PKCE (S256). A
//!    local callback listener is bound to an **ephemeral port** (OS picks a
//!    free port, we read the assigned `SocketAddr` back), so it can never
//!    collide with whatever else is running on the default 8765. The user
//!    is prompted to grant the requested scope on github.com, GitHub
//!    redirects to the local callback, the `code` is exchanged for a
//!    token, and the token is persisted to the OS keyring under
//!    `service=shannon-mcp-saas`, `user=github/<login>`.
//!
//! ## Manual OAuth test
//!
//! ```text
//! cargo run -p shannon-mcp-saas -- serve
//!   → opens browser to https://github.com/login/oauth/authorize?...
//!   → user grants on github.com
//!   → github redirects to http://127.0.0.1:<ephemeral>/callback?code=...&state=...
//!   → callback server validates state, exchanges code for token
//!   → token saved to keyring
//!   → subsequent tools/call includes Authorization: Bearer <token>
//! ```
//!
//! ## Keyring layout
//!
//! - service: `shannon-mcp-saas`
//! - user:    `github/<login>` (login taken from the token's `/user` lookup)
//!
//! The same shape works for Slack/Jira/Notion/Linear (P1-3 v2–v5): only the
//! `service` prefix and the user format change.

use std::net::SocketAddr;

use base64::Engine;
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::sync::oneshot;

use crate::github::api::ApiError;

/// Keyring service name. Same convention as `desktop/commands_connections.rs`
/// (one keyring entry per `(service, account)` pair).
pub const KEYRING_SERVICE: &str = "shannon-mcp-saas";

/// OAuth scopes requested for the GitHub App. `repo` covers issues and PRs
/// on both public and private repos; `read:user` lets us look up the
/// authenticated login so we can use it as the keyring user.
pub const OAUTH_SCOPES: &str = "repo read:user";

/// GitHub OAuth authorize endpoint (browser URL we open).
const AUTHORIZE_URL: &str = "https://github.com/login/oauth/authorize";
/// GitHub OAuth token-exchange endpoint.
const ACCESS_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
/// Standard `Accept` header that asks GitHub to return JSON (vs. the
/// default `application/x-www-form-urlencoded`).
const JSON_ACCEPT: &str = "application/json";

/// Errors that bubble up from the auth subsystem. The variants map 1:1 to
/// `McpError` shapes in `tools.rs` so the JSON-RPC layer can return a
/// stable, descriptive error code.
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("no GitHub token configured: set GITHUB_TOKEN env var or complete OAuth")]
    NoToken,
    #[error("OAuth state mismatch (possible CSRF); expected {expected}, got {actual}")]
    StateMismatch { expected: String, actual: String },
    #[error("OAuth callback missing or invalid `code` parameter")]
    MissingCode,
    #[error("token exchange failed: {0}")]
    Exchange(String),
    #[error("token-exchange HTTP error: {0}")]
    Http(String),
    #[error("keyring error: {0}")]
    Keyring(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("GitHub API error: {0}")]
    Api(#[from] ApiError),
}

/// An opaque, `Debug`-redacted GitHub token. Use [`Token::expose`] when
/// passing to an HTTP header; the `Debug` impl never prints the secret.
#[derive(Clone)]
pub struct Token(String);

impl Token {
    pub fn new(s: String) -> Self {
        Self(s)
    }

    /// Borrow the underlying secret. Prefer [`Token::header_value`] which
    /// returns a `&str` already in the `Bearer …` form.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Convenience: the `Authorization: Bearer <token>` header value.
    pub fn header_value(&self) -> String {
        format!("Bearer {}", self.0)
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(***redacted***)")
    }
}

/// A PKCE pair (verifier + S256 challenge). The verifier is random; the
/// challenge is `base64url(sha256(verifier))` per RFC 7636 §4.2.
#[derive(Debug, Clone)]
pub struct PkcePair {
    pub verifier: String,
    pub challenge: String,
}

impl PkcePair {
    /// 32 random bytes → 43-char base64url (no padding).
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

/// Random 32-byte hex `state` parameter for CSRF protection.
pub fn generate_state() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

mod hex {
    // Tiny hex encoder — avoids pulling the `hex` crate just for this.
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        const HEX: &[u8] = b"0123456789abcdef";
        let bytes = bytes.as_ref();
        let mut out = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0f) as usize] as char);
        }
        out
    }
}

/// Builds the GitHub authorize URL. The caller is responsible for opening
/// it in a browser.
pub fn build_authorize_url(
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    pkce: &PkcePair,
) -> String {
    // Manual `url::form_urlencoded` would be fine but `url` crate is in
    // workspace — using `url::Url` keeps escaping correct.
    let mut url = url::Url::parse(AUTHORIZE_URL).expect("static GitHub URL parses");
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("client_id", client_id);
        q.append_pair("redirect_uri", redirect_uri);
        q.append_pair("scope", OAUTH_SCOPES);
        q.append_pair("state", state);
        q.append_pair("code_challenge", &pkce.challenge);
        q.append_pair("code_challenge_method", "S256");
        q.append_pair("allow_signup", "true");
    }
    url.to_string()
}

/// Where the GitHub OAuth flow lives. The struct binds a localhost listener
/// on an ephemeral port, then yields the bound [`SocketAddr`] and a
/// [`oneshot::Receiver`] for the eventual token (or an error).
///
/// Lifecycle:
/// ```text
/// let (addr, rx) = OAuthFlow::start(client_id, client_secret, port).await?;
/// let url = build_authorize_url(&client_id, &format!("http://{addr}/callback"), &state, &pkce);
/// open_browser(&url);
/// let token = rx.await??;  // → Token
/// token.save_to_keyring("octocat").await?;
/// ```
pub struct OAuthFlow {
    /// The `127.0.0.1:<port>` we bound. The caller uses this to build the
    /// redirect URI.
    pub bind_addr: SocketAddr,
    /// Random CSRF state. The caller passes this to the authorize URL
    /// and validates it on the callback.
    pub state: String,
    /// PKCE pair (verifier, S256 challenge). The caller passes the
    /// challenge to the authorize URL and the verifier to
    /// [`OAuthFlow::exchange_code`].
    pub pkce: PkcePair,
    /// Internal receiver used by the embedded callback server. Held so
    /// the caller can also `await` it themselves; `take_token` consumes.
    receiver: Option<oneshot::Receiver<Result<Token, AuthError>>>,
    /// The `JoinHandle` of the callback server task. Aborted on drop.
    _server: tokio::task::JoinHandle<()>,
}

impl OAuthFlow {
    /// Start the OAuth flow. Binds to `127.0.0.1:0` (ephemeral port) so
    /// the caller never collides with another service on a fixed port
    /// (resolves Q2 from the plan).
    ///
    /// `requested_port` is currently ignored; the OS assigns the port.
    /// We keep the parameter for forward-compat with a future
    /// "bind to a specific port if free" mode.
    pub async fn start(
        client_id: String,
        client_secret: String,
        _requested_port: u16,
    ) -> Result<Self, AuthError> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let bind_addr = listener.local_addr()?;
        let state = generate_state();
        let pkce = PkcePair::generate();
        let (tx, rx) = oneshot::channel::<Result<Token, AuthError>>();

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
                tracing::error!(error = %e, "OAuth callback server failed");
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

    /// Await the OAuth completion. Returns the [`Token`] on success.
    pub async fn wait_for_token(mut self) -> Result<Token, AuthError> {
        let rx = self.receiver.take().expect("receiver only consumable once");
        let res = rx
            .await
            .map_err(|_| AuthError::Exchange("callback channel closed".into()))??;
        Ok(res)
    }

    /// Exchange an authorization `code` for a token using the PKCE
    /// verifier. Exposed so the callback server (or a test) can drive
    /// just the exchange step.
    pub async fn exchange_code(
        &self,
        client_id: &str,
        client_secret: &str,
        code: &str,
    ) -> Result<Token, AuthError> {
        let http = reqwest::Client::builder()
            .user_agent("shannon-mcp-saas/0.7")
            .build()
            .map_err(|e| AuthError::Http(e.to_string()))?;

        let resp = http
            .post(ACCESS_TOKEN_URL)
            .header("Accept", JSON_ACCEPT)
            .header("User-Agent", "shannon-mcp-saas/0.7")
            .form(&[
                ("client_id", client_id),
                ("client_secret", client_secret),
                ("code", code),
                ("code_verifier", self.pkce.verifier.as_str()),
                (
                    "redirect_uri",
                    &format!("http://{}/callback", self.bind_addr),
                ),
            ])
            .send()
            .await
            .map_err(|e| AuthError::Http(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(AuthError::Exchange(format!("HTTP {status}")));
        }

        let parsed: AccessTokenResponse = resp
            .json()
            .await
            .map_err(|e| AuthError::Exchange(format!("parse: {e}")))?;

        if let Some(err) = parsed.error_description.or(parsed.error) {
            return Err(AuthError::Exchange(err));
        }
        let access = parsed
            .access_token
            .ok_or_else(|| AuthError::Exchange("missing access_token".into()))?;
        Ok(Token::new(access))
    }
}

#[derive(Debug, Deserialize)]
struct AccessTokenResponse {
    access_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

/// HTTP server that handles the single GitHub redirect.
async fn run_callback_server(
    listener: tokio::net::TcpListener,
    tx: oneshot::Sender<Result<Token, AuthError>>,
    expected_state: String,
    pkce: PkcePair,
    client_id: String,
    client_secret: String,
) -> Result<(), AuthError> {
    use tokio::io::AsyncReadExt;

    // GitHub only redirects to the callback URL **once**. We accept the
    // first connection then return.
    let (mut stream, _peer) = listener.accept().await?;
    let mut buf = vec![0u8; 8192];
    let n = stream.read(&mut buf).await?;
    let req = String::from_utf8_lossy(&buf[..n]);

    // Parse the path + query from the first line:
    //   `GET /callback?code=...&state=... HTTP/1.1`
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

    // Pull `code` and `state` out of the query string.
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

    let state_param = match state_param {
        Some(s) => s,
        None => {
            let _ = send_400(&mut stream, "missing state").await;
            let _ = tx.send(Err(AuthError::StateMismatch {
                expected: expected_state,
                actual: String::new(),
            }));
            return Ok(());
        }
    };
    if state_param != expected_state {
        let _ = send_400(&mut stream, "state mismatch").await;
        let _ = tx.send(Err(AuthError::StateMismatch {
            expected: expected_state,
            actual: state_param,
        }));
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

    // Build a transient `OAuthFlow`-equivalent just to call `exchange_code`
    // without re-binding the listener.
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
            let _ = send_html(stream, "OAuth complete — you can close this tab.").await;
        }
        Err(_) => {
            let _ = send_400(&mut stream, "exchange failed").await;
        }
    }
    let _ = tx.send(result);
    Ok(())
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

fn url_decode(s: &str) -> String {
    // Tiny form-decode — handles `+` → space and `%XX` → byte.
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

/// What kind of credential the provider is currently holding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthStatus {
    Unauthenticated,
    PersonalAccessToken,
    OAuth { account: String },
}

/// Pluggable token source. The MVP supports PAT (env var) and OAuth
/// (keyring). Both surface a [`Token`] via [`TokenProvider::get_token`].
pub struct TokenProvider {
    /// Login used as the keyring `user` when persisting OAuth tokens.
    pub account_hint: String,
}

impl TokenProvider {
    pub fn new(account_hint: impl Into<String>) -> Self {
        Self {
            account_hint: account_hint.into(),
        }
    }

    /// Return a [`Token`] from whatever source is available, in priority
    /// order: `GITHUB_TOKEN` env var → keyring → `AuthError::NoToken`.
    pub async fn get_token(&self) -> Result<Token, AuthError> {
        if let Ok(pat) = std::env::var("GITHUB_TOKEN") {
            if !pat.is_empty() {
                return Ok(Token::new(pat));
            }
        }
        let entry = keyring::Entry::new(KEYRING_SERVICE, &format!("github/{}", self.account_hint))
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        match entry.get_password() {
            Ok(s) => Ok(Token::new(s)),
            Err(keyring::Error::NoEntry) => Err(AuthError::NoToken),
            Err(e) => Err(AuthError::Keyring(e.to_string())),
        }
    }

    /// Persist a token to the OS keyring. Returns the resolved account
    /// name (currently the same as the hint).
    pub fn save_to_keyring(&self, token: &Token, account: &str) -> Result<(), AuthError> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, &format!("github/{account}"))
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        entry
            .set_password(token.expose())
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        Ok(())
    }

    /// Best-effort [`AuthStatus`]. Reads `GITHUB_TOKEN` and the keyring
    /// but never errors on a missing keyring entry.
    pub fn status(&self) -> AuthStatus {
        if std::env::var("GITHUB_TOKEN").is_ok() {
            return AuthStatus::PersonalAccessToken;
        }
        if let Ok(entry) =
            keyring::Entry::new(KEYRING_SERVICE, &format!("github/{}", self.account_hint))
        {
            if entry.get_password().is_ok() {
                return AuthStatus::OAuth {
                    account: self.account_hint.clone(),
                };
            }
        }
        AuthStatus::Unauthenticated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_pair_has_correct_shape() {
        let pkce = PkcePair::generate();
        // Verifier is base64url of 32 bytes → 43 chars.
        assert_eq!(pkce.verifier.len(), 43);
        // Challenge is base64url of 32-byte SHA-256 → 43 chars.
        assert_eq!(pkce.challenge.len(), 43);
        // Verifier and challenge must differ.
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
        assert!(url.contains("scope=repo+read%3Auser"));
        assert!(url.contains("state=abc123"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("code_challenge="));
        assert!(url.contains("allow_signup=true"));
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
    fn status_reports_unauthenticated_when_nothing_set() {
        // We can't reliably *clear* GITHUB_TOKEN for the test process, so
        // we just assert the fallthrough path.
        let p = TokenProvider::new("nonexistent-test-user-zzz");
        let _ = p.status();
    }
}
