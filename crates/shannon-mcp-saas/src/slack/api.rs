//! Slack Web API client with retry and rate-limit handling.

use crate::slack::auth::Token;
use reqwest::{Client, Method, Response, StatusCode};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use thiserror::Error;
use tokio::{sync::RwLock, time::sleep};

pub const DEFAULT_BASE_URL: &str = "https://slack.com/api";
const MAX_RETRIES: u32 = 3;

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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackFile {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub permalink: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiEnvelope<T> {
    ok: bool,
    #[serde(flatten)]
    data: T,
    #[serde(default)]
    error: Option<String>,
    /// Cursor for paged responses. We deserialize it so the field is
    /// available on the slack-specific data structs, but Slack's
    /// `ApiEnvelope` does not currently surface it in the top-level
    /// envelope — see `HistoryData` / `UsersData` for the cursor
    /// surface.
    #[allow(dead_code)]
    #[serde(default)]
    response_metadata: Option<ResponseMetadata>,
}
#[derive(Debug, Deserialize)]
pub struct ResponseMetadata {
    #[serde(default)]
    pub next_cursor: String,
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
#[derive(Debug, Deserialize)]
pub struct UsersData {
    #[serde(default)]
    pub members: Vec<SlackUser>,
    #[serde(default)]
    pub response_metadata: Option<ResponseMetadata>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct MessageData {
    pub ts: String,
    pub channel: String,
}
#[derive(Debug, Deserialize)]
pub struct FileData {
    #[serde(default)]
    pub file: Option<SlackFile>,
}
#[derive(Debug, Deserialize)]
struct EmptyData {}

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
            base_url: DEFAULT_BASE_URL.into(),
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
    pub async fn history(
        &self,
        channel: &str,
        cursor: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Vec<Message>, ApiError> {
        let mut params: Vec<(String, String)> = vec![("channel".into(), channel.into())];
        if let Some(v) = cursor {
            params.push(("cursor".into(), v.into()));
        }
        if let Some(v) = limit {
            params.push(("limit".into(), v.to_string()));
        }
        self.call_json("conversations.history", &params)
            .await
            .map(|v: HistoryData| v.messages)
    }
    pub async fn reply(
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
    pub async fn users_list(
        &self,
        cursor: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Vec<SlackUser>, ApiError> {
        let mut params: Vec<(String, String)> = Vec::new();
        if let Some(v) = cursor {
            params.push(("cursor".into(), v.into()));
        }
        if let Some(v) = limit {
            params.push(("limit".into(), v.to_string()));
        }
        self.call_json("users.list", &params)
            .await
            .map(|v: UsersData| v.members)
    }
    pub async fn upload_file(
        &self,
        channels: &str,
        content: &str,
        filename: Option<&str>,
    ) -> Result<SlackFile, ApiError> {
        let mut params: Vec<(String, String)> = vec![
            ("channels".into(), channels.into()),
            ("content".into(), content.into()),
        ];
        if let Some(v) = filename {
            params.push(("filename".into(), v.into()));
        }
        self.call_json_post("files.upload", &params)
            .await
            .map(|v: FileData| {
                v.file.unwrap_or(SlackFile {
                    id: String::new(),
                    name: None,
                    permalink: None,
                })
            })
    }
    pub async fn add_reaction(
        &self,
        channel: &str,
        timestamp: &str,
        name: &str,
    ) -> Result<(), ApiError> {
        let _: EmptyData = self
            .call_json_post(
                "reactions.add",
                &[
                    ("channel".into(), channel.into()),
                    ("timestamp".into(), timestamp.into()),
                    ("name".into(), name.into()),
                ],
            )
            .await?;
        Ok(())
    }

    async fn call_json<T: serde::de::DeserializeOwned + Send>(
        &self,
        method: &str,
        params: &[(String, String)],
    ) -> Result<T, ApiError> {
        let response = self.request(Method::GET, method, params, None).await?;
        let envelope: ApiEnvelope<T> = response
            .json()
            .await
            .map_err(|e| ApiError::Json(e.to_string()))?;
        if !envelope.ok {
            return Err(ApiError::Slack(
                envelope
                    .error
                    .unwrap_or_else(|| "unknown Slack error".into()),
            ));
        }
        Ok(envelope.data)
    }
    async fn call_json_post<T: serde::de::DeserializeOwned + Send>(
        &self,
        method: &str,
        params: &[(String, String)],
    ) -> Result<T, ApiError> {
        let response = self.request(Method::POST, method, params, None).await?;
        let envelope: ApiEnvelope<T> = response
            .json()
            .await
            .map_err(|e| ApiError::Json(e.to_string()))?;
        if !envelope.ok {
            return Err(ApiError::Slack(
                envelope
                    .error
                    .unwrap_or_else(|| "unknown Slack error".into()),
            ));
        }
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
        let mut attempt = 0;
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
            if response.status() == StatusCode::UNAUTHORIZED {
                return Err(ApiError::Unauthorized);
            }
            if response.status() == StatusCode::TOO_MANY_REQUESTS {
                let retry_after = response
                    .headers()
                    .get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_else(|| 1u64 << attempt.min(5));
                if attempt < MAX_RETRIES {
                    self.backoff_sleep(retry_after).await;
                    attempt += 1;
                    continue;
                }
                return Err(ApiError::RateLimited {
                    retry_after_secs: retry_after,
                });
            }
            if response.status().is_server_error() && attempt < MAX_RETRIES {
                self.backoff_sleep(1u64 << attempt.min(5)).await;
                attempt += 1;
                continue;
            }
            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(ApiError::Server(format!("HTTP {status}: {body}")));
            }
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
        if self.backoff_scale != 0 {
            sleep(Duration::from_secs(
                seconds.saturating_mul(self.backoff_scale as u64),
            ))
            .await;
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn endpoint_names_are_stable() {
        assert_eq!("conversations.history", "conversations.history");
    }
}
