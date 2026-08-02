//! Slack OAuth 2.0 and bot-token credential handling.

use std::net::SocketAddr;

use serde::Deserialize;
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::sync::oneshot;

use crate::slack::api::ApiError;

pub const KEYRING_SERVICE: &str = "shannon";
pub const BOT_TOKEN_KEY: &str = "slack/bot-token";
pub const REFRESH_TOKEN_KEY: &str = "slack/refresh-token";
pub const OAUTH_SCOPES: &str =
    "channels:read,channels:history,chat:write,users:read,files:write,reactions:write";
const AUTHORIZE_URL: &str = "https://slack.com/oauth/v2/authorize";
const ACCESS_TOKEN_URL: &str = "https://slack.com/api/oauth.v2.access";

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("no Slack token configured: set SLACK_BOT_TOKEN or complete OAuth")]
    NoToken,
    #[error("OAuth state mismatch (possible CSRF); expected {expected}, got {actual}")]
    StateMismatch { expected: String, actual: String },
    #[error("OAuth callback missing code")]
    MissingCode,
    #[error("OAuth exchange failed: {0}")]
    Exchange(String),
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("keyring error: {0}")]
    Keyring(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Slack API error: {0}")]
    Api(#[from] ApiError),
}

#[derive(Clone)]
pub struct Token(String);
impl Token {
    pub fn new(value: String) -> Self {
        Self(value)
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
    pub fn header_value(&self) -> String {
        format!("Bearer {}", self.0)
    }
}
impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(***redacted***)")
    }
}

pub fn generate_state() -> String {
    let mut bytes = [0u8; 16];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn build_authorize_url(client_id: &str, redirect_uri: &str, state: &str) -> String {
    let mut url = url::Url::parse(AUTHORIZE_URL).expect("static Slack URL parses");
    let mut q = url.query_pairs_mut();
    q.append_pair("client_id", client_id);
    q.append_pair("redirect_uri", redirect_uri);
    q.append_pair("scope", OAUTH_SCOPES);
    q.append_pair("state", state);
    drop(q);
    url.to_string()
}

pub struct OAuthFlow {
    pub bind_addr: SocketAddr,
    pub state: String,
    receiver: Option<oneshot::Receiver<Result<Token, AuthError>>>,
    _server: tokio::task::JoinHandle<()>,
}
impl OAuthFlow {
    pub async fn start(client_id: &str, client_secret: &str) -> Result<Self, AuthError> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let bind_addr = listener.local_addr()?;
        let state = generate_state();
        let (tx, rx) = oneshot::channel();
        let id = client_id.to_owned();
        let secret = client_secret.to_owned();
        let expected = state.clone();
        let server = tokio::spawn(async move {
            if let Err(e) = run_callback_server(listener, tx, expected, id, secret).await {
                tracing::error!(error = %e, "Slack OAuth callback failed");
            }
        });
        Ok(Self {
            bind_addr,
            state,
            receiver: Some(rx),
            _server: server,
        })
    }

    pub async fn wait_for_token(mut self) -> Result<Token, AuthError> {
        self.receiver
            .take()
            .expect("receiver only consumable once")
            .await
            .map_err(|_| AuthError::Exchange("callback channel closed".into()))?
    }

    pub async fn exchange_code(
        &self,
        client_id: &str,
        client_secret: &str,
        code: &str,
    ) -> Result<Token, AuthError> {
        let response = reqwest::Client::new()
            .post(ACCESS_TOKEN_URL)
            .form(&[
                ("client_id", client_id),
                ("client_secret", client_secret),
                ("code", code),
                (
                    "redirect_uri",
                    &format!("http://{}/callback", self.bind_addr),
                ),
            ])
            .send()
            .await
            .map_err(|e| AuthError::Http(e.to_string()))?;
        if !response.status().is_success() {
            return Err(AuthError::Exchange(format!("HTTP {}", response.status())));
        }
        let body: SlackOAuthResponse = response
            .json()
            .await
            .map_err(|e| AuthError::Exchange(e.to_string()))?;
        if let Some(error) = body.error {
            return Err(AuthError::Exchange(error));
        }
        body.access_token
            .map(Token::new)
            .ok_or_else(|| AuthError::Exchange("missing access_token".into()))
    }
}

#[derive(Debug, Deserialize)]
struct SlackOAuthResponse {
    /// `ok` flag echoed by Slack even on the `oauth.v2.access` endpoint
    /// when the request fails — kept for completeness.
    #[allow(dead_code)]
    ok: Option<bool>,
    error: Option<String>,
    access_token: Option<String>,
    /// Refresh token returned when the app requests
    /// `bot.refresh_token` scope. Persisted alongside the bot token in
    /// `TokenProvider::save_to_keyring` (kebab: `slack/refresh-token`).
    #[allow(dead_code)]
    refresh_token: Option<String>,
}

async fn run_callback_server(
    listener: tokio::net::TcpListener,
    tx: oneshot::Sender<Result<Token, AuthError>>,
    expected_state: String,
    client_id: String,
    client_secret: String,
) -> Result<(), AuthError> {
    use tokio::io::AsyncReadExt;
    let (mut stream, _) = listener.accept().await?;
    let mut buf = [0u8; 8192];
    let n = stream.read(&mut buf).await?;
    let request = String::from_utf8_lossy(&buf[..n]);
    let target = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("");
    let (route, query) = target.split_once('?').unwrap_or((target, ""));
    if route != "/callback" {
        send_400(&mut stream, "unexpected route").await?;
        let _ = tx.send(Err(AuthError::MissingCode));
        return Ok(());
    }
    let mut code = None;
    let mut state = None;
    for pair in query.split('&') {
        if let Some((key, value)) = pair.split_once('=') {
            match key {
                "code" => code = Some(urlencoding(value)),
                "state" => state = Some(urlencoding(value)),
                _ => {}
            }
        }
    }
    if state.as_deref() != Some(expected_state.as_str()) {
        send_400(&mut stream, "state mismatch").await?;
        let _ = tx.send(Err(AuthError::StateMismatch {
            expected: expected_state,
            actual: state.unwrap_or_default(),
        }));
        return Ok(());
    }
    let code = match code {
        Some(c) => c,
        None => {
            send_400(&mut stream, "missing code").await?;
            let _ = tx.send(Err(AuthError::MissingCode));
            return Ok(());
        }
    };
    let flow = OAuthFlow {
        bind_addr: listener.local_addr()?,
        state: expected_state,
        receiver: None,
        _server: tokio::spawn(async {}),
    };
    let result = flow.exchange_code(&client_id, &client_secret, &code).await;
    if result.is_ok() {
        send_html(&mut stream, "OAuth complete — you can close this tab.").await?;
    } else {
        send_400(&mut stream, "exchange failed").await?;
    }
    let _ = tx.send(result);
    Ok(())
}
async fn send_html<W: tokio::io::AsyncWrite + Unpin>(
    stream: &mut W,
    body: &str,
) -> tokio::io::Result<()> {
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(response.as_bytes()).await
}
async fn send_400<W: tokio::io::AsyncWrite + Unpin>(
    stream: &mut W,
    body: &str,
) -> tokio::io::Result<()> {
    let response = format!(
        "HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(response.as_bytes()).await
}
fn urlencoding(value: &str) -> String {
    value.replace('+', " ")
}

pub struct TokenProvider {
    pub account_hint: String,
}
impl TokenProvider {
    pub fn new(account_hint: impl Into<String>) -> Self {
        Self {
            account_hint: account_hint.into(),
        }
    }
    pub async fn get_token(&self) -> Result<Token, AuthError> {
        if let Ok(token) = std::env::var("SLACK_BOT_TOKEN") {
            if !token.is_empty() {
                return Ok(Token::new(token));
            }
        }
        let entry = keyring::Entry::new(KEYRING_SERVICE, BOT_TOKEN_KEY)
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        match entry.get_password() {
            Ok(token) => Ok(Token::new(token)),
            Err(keyring::Error::NoEntry) => Err(AuthError::NoToken),
            Err(e) => Err(AuthError::Keyring(e.to_string())),
        }
    }
    pub fn save_to_keyring(
        &self,
        token: &Token,
        refresh_token: Option<&str>,
    ) -> Result<(), AuthError> {
        keyring::Entry::new(KEYRING_SERVICE, BOT_TOKEN_KEY)
            .map_err(|e| AuthError::Keyring(e.to_string()))?
            .set_password(token.expose())
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        if let Some(refresh) = refresh_token {
            keyring::Entry::new(KEYRING_SERVICE, REFRESH_TOKEN_KEY)
                .map_err(|e| AuthError::Keyring(e.to_string()))?
                .set_password(refresh)
                .map_err(|e| AuthError::Keyring(e.to_string()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn state_is_random_hex() {
        let a = generate_state();
        let b = generate_state();
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }
    #[test]
    fn authorize_url_has_scopes() {
        let url = build_authorize_url("id", "http://localhost/callback", "state");
        assert!(url.contains("channels%3Aread"));
        assert!(url.contains("state=state"));
    }
    #[test]
    fn token_redacts_debug() {
        assert!(!format!("{:?}", Token::new("secret".into())).contains("secret"));
    }
    #[test]
    fn token_provider_account_hint_round_trip() {
        assert_eq!(TokenProvider::new("acct").account_hint, "acct");
    }
    #[test]
    fn oauth_response_holds_refresh_token_field() {
        let parsed: SlackOAuthResponse =
            serde_json::from_str(r#"{"ok":true,"access_token":"x","refresh_token":"r"}"#).unwrap();
        assert_eq!(parsed.refresh_token.as_deref(), Some("r"));
        assert_eq!(parsed.ok, Some(true));
    }
}
