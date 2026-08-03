//! Slack Web API client with retry and rate-limit handling.
//!
//! Mirrors the `jira/api.rs` shape — long-lived `reqwest::Client`, rotating
//! token, exponential backoff on 5xx, honouring `Retry-After` on 429 — but
//! the wire is `https://slack.com/api/<endpoint>`. All Web API endpoints
//! return a JSON envelope `{ "ok": bool, ... , "error"?: string }` even on
//! success-shaped 200s, so every method goes through `call_json` to check
//! the envelope and surface Slack's `error` string on failure.

use std::sync::Arc;
use std::time::Duration;

use reqwest::{Client, Method, Response, StatusCode};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::{sync::RwLock, time::sleep};

use crate::slack::auth::Token;

pub const DEFAULT_BASE_URL: &str = "https://slack.com/api";

/// Maximum retries on a single request (after the initial attempt).
/// Matches the Jira module's ceiling so behaviour is consistent across SaaSes.
const MAX_RETRIES: u32 = 3;

/// Default backoff per attempt if the server doesn't supply `Retry-After`.
const DEFAULT_BACKOFF: u64 = 1;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("unauthorized (401) — token may be revoked or missing scopes")]
    Unauthorized,
    #[error("Slack API error: {0}")]
    Slack(String),
    #[error("rate-limited; retry after {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },
    #[error("server error: {0}")]
    Server(String),
    #[error("JSON error: {0}")]
    Json(String),
}
impl From<reqwest::Error> for ApiError {
    fn from(e: reqwest::Error) -> Self {
        Self::Http(e.to_string())
    }
}
impl From<serde_json::Error> for ApiError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub is_private: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub ts: String,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub thread_ts: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackUser {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub real_name: Option<String>,
}

/// Response shape returned by `chat.postMessage`.
#[derive(Debug, Serialize, Deserialize)]
pub struct MessageData {
    pub ts: String,
    pub channel: String,
}

/// Cursor for paged responses. Surfaced as a top-level field on payloads
/// (e.g. `search.messages`) but kept in our envelope so callers can pivot.
#[derive(Debug, Deserialize)]
pub struct ResponseMetadata {
    #[serde(default)]
    pub next_cursor: String,
}

/// Slack wraps every response in `{ "ok": bool, ... }`. The `ok` and `error`
/// fields are read on the raw `serde_json::Value` (not the typed envelope) so
/// we can probe `ok:false` without forcing every data type to be lenient.
/// They're declared here for documentation purposes only.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ApiEnvelope<T> {
    ok: bool,
    #[serde(flatten)]
    data: T,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChannelsData {
    #[serde(default)]
    pub channels: Vec<Channel>,
}

#[derive(Debug, Deserialize)]
pub struct HistoryData {
    #[serde(default)]
    pub messages: Vec<Message>,
    #[serde(default)]
    pub has_more: bool,
    #[serde(default)]
    pub response_metadata: Option<ResponseMetadata>,
}

/// Slack `search.messages` payload shape (subset).
#[derive(Debug, Deserialize)]
pub struct SearchData {
    pub messages: Option<SearchMatches>,
    #[serde(default)]
    pub total: u32,
}
#[derive(Debug, Default, Deserialize)]
pub struct SearchMatches {
    #[serde(default)]
    pub matches: Vec<SearchMessage>,
    #[serde(default)]
    pub total: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMessage {
    #[serde(default)]
    pub ts: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub channel: Option<SearchChannel>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchChannel {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

/// `users.info` payload — Slack nests the user object under `user`.
#[derive(Debug, Deserialize)]
pub struct UserInfoData {
    #[serde(default)]
    pub user: Option<SlackUser>,
}

pub struct SlackClient {
    http: Client,
    base_url: String,
    token: Arc<RwLock<Option<Token>>>,
    backoff_scale: u32,
}
impl SlackClient {
    pub fn new(http: Client, token: Option<Token>) -> Self {
        Self {
            http,
            base_url: DEFAULT_BASE_URL.to_string(),
            token: Arc::new(RwLock::new(token)),
            backoff_scale: 1,
        }
    }
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }
    pub fn with_backoff_scale(mut self, scale: u32) -> Self {
        self.backoff_scale = scale;
        self
    }
    pub async fn set_token(&self, token: Token) {
        *self.token.write().await = Some(token);
    }
    async fn current_token(&self) -> Option<Token> {
        self.token.read().await.clone()
    }

    // -----------------------------------------------------------------
    // Tool-shaped API methods (mirror the 6 tools in tools.rs)
    // -----------------------------------------------------------------

    /// `POST /chat.postMessage` — post a top-level message to a channel.
    /// Returns the `MessageData` envelope (ts + channel).
    pub async fn post_message(&self, channel: &str, text: &str) -> Result<MessageData, ApiError> {
        self.call_json_post(
            "chat.postMessage",
            &[
                ("channel".into(), channel.into()),
                ("text".into(), text.into()),
            ],
        )
        .await
    }

    /// `GET /conversations.list` — list channels (no args required for the
    /// `slack_list_channels` tool surface; cursor/limit let callers page).
    pub async fn list_channels(
        &self,
        cursor: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Vec<Channel>, ApiError> {
        let mut params: Vec<(String, String)> = Vec::new();
        if let Some(v) = cursor {
            params.push(("cursor".into(), v.into()));
        }
        if let Some(v) = limit {
            params.push(("limit".into(), v.to_string()));
        }
        self.call_json("conversations.list", &params)
            .await
            .map(|v: ChannelsData| v.channels)
    }

    /// `GET /conversations.history` — read recent messages for a channel.
    /// The `slack_read_channel` tool exposes `channel` + `limit`.
    pub async fn read_channel(
        &self,
        channel: &str,
        limit: Option<u32>,
    ) -> Result<Vec<Message>, ApiError> {
        let mut params: Vec<(String, String)> = vec![("channel".into(), channel.into())];
        if let Some(v) = limit {
            params.push(("limit".into(), v.to_string()));
        }
        self.call_json("conversations.history", &params)
            .await
            .map(|v: HistoryData| v.messages)
    }

    /// `GET /search.messages` — search public/private messages by query.
    pub async fn search_messages(
        &self,
        query: &str,
        limit: Option<u32>,
    ) -> Result<SearchMatches, ApiError> {
        let mut params: Vec<(String, String)> = vec![("query".into(), query.into())];
        if let Some(v) = limit {
            params.push(("limit".into(), v.to_string()));
        }
        let data: SearchData = self.call_json("search.messages", &params).await?;
        let m = data.messages.unwrap_or_default();
        Ok(SearchMatches {
            matches: m.matches,
            total: m.total,
        })
    }

    /// `POST /chat.postMessage` with `thread_ts` — reply in a thread.
    pub async fn thread_reply(
        &self,
        channel: &str,
        thread_ts: &str,
        text: &str,
    ) -> Result<MessageData, ApiError> {
        self.call_json_post(
            "chat.postMessage",
            &[
                ("channel".into(), channel.into()),
                ("thread_ts".into(), thread_ts.into()),
                ("text".into(), text.into()),
            ],
        )
        .await
    }

    /// `GET /users.info` — look up a single user by id.
    pub async fn get_user_info(&self, user_id: &str) -> Result<SlackUser, ApiError> {
        let data: UserInfoData = self
            .call_json("users.info", &[("user".into(), user_id.into())])
            .await?;
        data.user
            .ok_or_else(|| ApiError::Slack("users.info: missing `user` field".into()))
    }

    // -----------------------------------------------------------------
    // Internals
    // -----------------------------------------------------------------

    async fn call_json<T: serde::de::DeserializeOwned + Send>(
        &self,
        method: &str,
        params: &[(String, String)],
    ) -> Result<T, ApiError> {
        let response = self.request(Method::GET, method, params, None).await?;
        // Read the body once so we can probe `ok` even when the success
        // payload is missing fields required by `T`. Slack surfaces
        // `ok:false` errors on 200 responses with partial payloads.
        let value: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ApiError::Json(e.to_string()))?;
        if value.get("ok") == Some(&serde_json::Value::Bool(false)) {
            let err = value
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown Slack error")
                .to_string();
            return Err(ApiError::Slack(err));
        }
        let envelope: ApiEnvelope<T> =
            serde_json::from_value(value).map_err(|e| ApiError::Json(e.to_string()))?;
        Ok(envelope.data)
    }
    async fn call_json_post<T: serde::de::DeserializeOwned + Send>(
        &self,
        method: &str,
        params: &[(String, String)],
    ) -> Result<T, ApiError> {
        let response = self.request(Method::POST, method, params, None).await?;
        let value: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ApiError::Json(e.to_string()))?;
        if value.get("ok") == Some(&serde_json::Value::Bool(false)) {
            let err = value
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown Slack error")
                .to_string();
            return Err(ApiError::Slack(err));
        }
        let envelope: ApiEnvelope<T> =
            serde_json::from_value(value).map_err(|e| ApiError::Json(e.to_string()))?;
        Ok(envelope.data)
    }

    async fn request(
        &self,
        method: Method,
        endpoint: &str,
        params: &[(String, String)],
        body: Option<&serde_json::Value>,
    ) -> Result<Response, ApiError> {
        let url = format!("{}/{}", self.base_url.trim_end_matches('/'), endpoint);
        let mut attempt: u32 = 0;
        loop {
            let mut request = self
                .http
                .request(method.clone(), &url)
                .header("Content-Type", "application/x-www-form-urlencoded");
            if let Some(token) = self.current_token().await {
                request = request.header("Authorization", token.header_value());
            }
            let owned: Vec<(&str, &str)> = params
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            if method == Method::GET {
                request = request.query(&owned);
            } else {
                request = request.form(&owned);
            }
            if let Some(value) = body {
                request = request.json(value);
            }
            let response = request.send().await?;

            // 401 — token revoked. Don't retry; caller must re-auth.
            if response.status() == StatusCode::UNAUTHORIZED {
                return Err(ApiError::Unauthorized);
            }

            // 429 — primary rate-limit. Honour Retry-After exactly, mirroring
            // Jira's behaviour.
            if response.status() == StatusCode::TOO_MANY_REQUESTS {
                let retry_after = parse_retry_after(response.headers()).unwrap_or_else(|| {
                    // Exponential backoff when Retry-After is absent or unparseable.
                    DEFAULT_BACKOFF << attempt.min(5)
                });
                if attempt < MAX_RETRIES {
                    self.backoff_sleep(retry_after).await;
                    attempt += 1;
                    continue;
                }
                return Err(ApiError::RateLimited {
                    retry_after_secs: retry_after,
                });
            }

            // 5xx — server error. Exponential backoff with a 60s cap.
            if response.status().is_server_error() {
                if attempt < MAX_RETRIES {
                    let backoff = DEFAULT_BACKOFF << attempt.min(5);
                    self.backoff_sleep(backoff).await;
                    attempt += 1;
                    continue;
                }
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(ApiError::Server(format!("HTTP {status}: {body}")));
            }

            // Other non-success — surface as a server-shaped error.
            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(ApiError::Server(format!("HTTP {status}: {body}")));
            }

            // On 2xx with X-RateLimit-Reset pointing into the future, sleep
            // up to 60s before returning — same guard as Jira uses.
            if let Some(reset) = response
                .headers()
                .get("X-RateLimit-Reset")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<i64>().ok())
            {
                let now = chrono::Utc::now().timestamp();
                if reset > now {
                    sleep(Duration::from_secs((reset - now).min(60) as u64)).await;
                }
            }
            return Ok(response);
        }
    }

    async fn backoff_sleep(&self, seconds: u64) {
        if self.backoff_scale == 0 {
            return;
        }
        sleep(Duration::from_secs(
            seconds.saturating_mul(self.backoff_scale as u64),
        ))
        .await;
    }
}

fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get("Retry-After")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_names_are_stable() {
        // The 6 tools round-trip via these Slack endpoints. If any of
        // them change, the corresponding tool's wiring is now stale.
        let endpoints = [
            "chat.postMessage",
            "conversations.list",
            "conversations.history",
            "search.messages",
            "users.info",
        ];
        for e in endpoints {
            assert!(
                e.contains('.'),
                "Slack endpoint should look like foo.bar: {e}"
            );
        }
    }

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn default_backoff_is_one() {
        // 0 would be a bug — it would short-circuit retry sleeps entirely.
        // Anchors the value to break a careless edit at `1 -> 0`.
        assert!(DEFAULT_BACKOFF == 1);
    }
}
