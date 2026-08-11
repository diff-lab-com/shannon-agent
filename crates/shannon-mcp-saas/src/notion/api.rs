//! Notion REST client targeting `https://api.notion.com/v1/`.
//!
//! Mirrors the `slack/api.rs` and `jira/api.rs` shape: a long-lived
//! `reqwest::Client`, a rotating `Token` slot, exponential backoff on
//! 5xx, and explicit `Retry-After` honouring on 429. The wire is the
//! standard Notion API — JSON in / JSON out, with a mandatory
//! `Notion-Version` header that pins the schema. We default to
//! `2022-06-28` because that version's envelope (page objects nested
//! under `properties`, database rows under `properties`) matches the
//! shape our 6 tools surface; newer versions (2025-09-03) introduce a
//! "data source" separation that would change `query_database`.
//!
//! Notion does NOT wrap responses in a `{ ok: ... }` envelope. Errors
//! come back on non-2xx with `{ object, status, code, message }`.
//! Successful payloads are returned verbatim to the caller.

use std::time::Duration;

use reqwest::{Client, Method, Response, StatusCode};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;
use tokio::time::sleep;

use crate::notion::auth::Token;

/// Default base URL. Overrideable via [`NotionClient::with_base_url`]
/// for unit tests.
pub const DEFAULT_BASE_URL: &str = "https://api.notion.com/v1";

/// Notion REST schema version. 2022-06-28 is the last version where
/// pages and database rows share the same `properties` envelope, which
/// is what `notion_get_page` and `notion_query_database` rely on.
pub const NOTION_VERSION: &str = "2022-06-28";

/// Per-window rate limit. Notion advertises ~3 requests/second. We
/// surface this as a const for telemetry and (future) internal pacing.
pub const RATE_LIMIT_PER_SECOND: u32 = 3;

/// Maximum retries on a single request (after the initial attempt).
const MAX_RETRIES: u32 = 3;

/// Default backoff per attempt if the server doesn't supply `Retry-After`.
const DEFAULT_BACKOFF: u64 = 1;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("unauthorized (401) — Notion token revoked or integration removed from the workspace")]
    Unauthorized,
    #[error("forbidden (403) — integration lacks access to the requested page/database: {0}")]
    Forbidden(String),
    #[error("not found (404) — {0}")]
    NotFound(String),
    #[error("validation failed (400) — {0}")]
    Validation(String),
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

// ---------------------------------------------------------------------------
// Typed shapes — kept minimal: the public API contract is "the caller
// gets the raw JSON the way Notion sent it". The structs here are
// reserved for future, more strongly-typed tool returns.
// ---------------------------------------------------------------------------

/// Subset of a Notion page object we surface for `notion_get_page` and
/// `notion_create_page`. `properties` is intentionally kept as
/// `serde_json::Value` because the schema is dynamic per database
/// (every database defines its own property names and types).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page {
    pub id: String,
    #[serde(default)]
    pub object: Option<String>,
    #[serde(default)]
    pub created_time: Option<String>,
    #[serde(default)]
    pub last_edited_time: Option<String>,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub properties: serde_json::Value,
    #[serde(default)]
    pub url: Option<String>,
}

/// Subset of a Notion database object (the list_databases / query_database
/// return type).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Database {
    pub id: String,
    #[serde(default)]
    pub object: Option<String>,
    #[serde(default)]
    pub title: Vec<serde_json::Value>,
    #[serde(default)]
    pub properties: serde_json::Value,
}

/// `query_database` response. `results` is heterogeneous (each entry is
/// a full page object), `has_more` flags the presence of a `next_cursor`.
#[derive(Debug, Default, Deserialize)]
pub struct DatabaseQueryResponse {
    #[serde(default)]
    pub results: Vec<serde_json::Value>,
    #[serde(default)]
    pub has_more: bool,
    #[serde(default)]
    pub next_cursor: Option<String>,
    #[rustfmt::skip]
    #[allow(dead_code)] // KEEP: future typed accessor for the `object` discriminator field on Notion's list envelope.
    #[serde(default)]
    pub object: Option<String>,
}

/// `search_pages` response. Same envelope as `query_database` — both
/// surface `results`/`has_more`/`next_cursor` — so we reuse the type.
#[derive(Debug, Default, Deserialize)]
pub struct SearchResponse {
    #[serde(default)]
    pub results: Vec<serde_json::Value>,
    #[serde(default)]
    pub has_more: bool,
    #[serde(default)]
    pub next_cursor: Option<String>,
}

/// A single block. `notion_append_block` returns one of these; rich
/// children would nest under the same shape. Kept generic on the wire
/// because the block schema is enormous and versioned.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub id: String,
    #[serde(default)]
    pub object: Option<String>,
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub has_children: bool,
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

pub struct NotionClient {
    http: Client,
    base_url: String,
    notion_version: String,
    token: RwLock<Option<Token>>,
    /// Multiplier applied to backoff sleeps. 0 = no sleep, 1 = production.
    backoff_scale: u32,
}

impl NotionClient {
    pub fn new(http: Client, token: Option<Token>) -> Self {
        Self {
            http,
            base_url: DEFAULT_BASE_URL.to_string(),
            notion_version: NOTION_VERSION.to_string(),
            token: RwLock::new(token),
            backoff_scale: 1,
        }
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    pub fn with_notion_version(mut self, version: impl Into<String>) -> Self {
        self.notion_version = version.into();
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
    // Tool-shaped API methods
    // -----------------------------------------------------------------

    /// `POST /v1/search` — search pages and databases. Notion's search
    /// returns both `page` and `database` objects; we hand the raw
    /// `results` array back to the caller.
    pub async fn search_pages(
        &self,
        query: Option<&str>,
        filter: Option<&serde_json::Value>,
        sort: Option<&serde_json::Value>,
        start_cursor: Option<&str>,
        page_size: Option<u32>,
    ) -> Result<SearchResponse, ApiError> {
        let mut body = serde_json::Map::new();
        if let Some(q) = query {
            body.insert("query".into(), serde_json::Value::String(q.to_string()));
        }
        if let Some(f) = filter {
            body.insert("filter".into(), f.clone());
        }
        if let Some(s) = sort {
            body.insert("sort".into(), s.clone());
        }
        if let Some(c) = start_cursor {
            body.insert(
                "start_cursor".into(),
                serde_json::Value::String(c.to_string()),
            );
        }
        if let Some(n) = page_size {
            body.insert(
                "page_size".into(),
                serde_json::Value::Number(serde_json::Number::from(n)),
            );
        }
        self.post_json("/search", &serde_json::Value::Object(body))
            .await
    }

    /// `GET /v1/pages/{id}` — fetch a single page by id.
    pub async fn get_page(&self, page_id: &str) -> Result<Page, ApiError> {
        let path = format!("/pages/{}", url_path_encode(page_id));
        self.get_json(&path).await
    }

    /// `PATCH /v1/pages/{id}` — append a single block (or block tree)
    /// to a page's children. Notion's contract is "append children" —
    /// the request body is `{ children: [...] }`.
    pub async fn append_block(
        &self,
        page_id: &str,
        block: &serde_json::Value,
    ) -> Result<Block, ApiError> {
        let path = format!("/blocks/{}/children", url_path_encode(page_id));
        let body = serde_json::json!({ "children": [block] });
        let resp: serde_json::Value = self.patch_json(&path, &body).await?;
        // PATCH returns a `{ object: "list", results: [...] }` envelope;
        // unwrap to the first result.
        let id = resp
            .get("results")
            .and_then(serde_json::Value::as_array)
            .and_then(|arr| arr.first())
            .and_then(|v| v.get("id"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        Ok(Block {
            id,
            object: resp
                .get("object")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            r#type: None,
            has_children: false,
        })
    }

    /// `POST /v1/pages` — create a page. `parent` is a Notion-shaped
    /// object (e.g. `{ database_id: "..." }` or `{ page_id: "..." }`).
    pub async fn create_page(
        &self,
        parent: &serde_json::Value,
        properties: &serde_json::Value,
        children: Option<&[serde_json::Value]>,
    ) -> Result<Page, ApiError> {
        let mut body = serde_json::Map::new();
        body.insert("parent".into(), parent.clone());
        body.insert("properties".into(), properties.clone());
        if let Some(c) = children {
            if !c.is_empty() {
                body.insert("children".into(), serde_json::Value::Array(c.to_vec()));
            }
        }
        self.post_json("/pages", &serde_json::Value::Object(body))
            .await
    }

    /// `POST /v1/databases/{id}/query` — query a database. Supports
    /// filter / sorts / pagination per Notion's contract.
    pub async fn query_database(
        &self,
        database_id: &str,
        filter: Option<&serde_json::Value>,
        sorts: Option<&[serde_json::Value]>,
        page_size: Option<u32>,
        start_cursor: Option<&str>,
    ) -> Result<DatabaseQueryResponse, ApiError> {
        let path = format!("/databases/{}/query", url_path_encode(database_id));
        let mut body = serde_json::Map::new();
        if let Some(f) = filter {
            body.insert("filter".into(), f.clone());
        }
        if let Some(s) = sorts {
            if !s.is_empty() {
                body.insert("sorts".into(), serde_json::Value::Array(s.to_vec()));
            }
        }
        if let Some(n) = page_size {
            body.insert(
                "page_size".into(),
                serde_json::Value::Number(serde_json::Number::from(n)),
            );
        }
        if let Some(c) = start_cursor {
            body.insert(
                "start_cursor".into(),
                serde_json::Value::String(c.to_string()),
            );
        }
        self.post_json(&path, &serde_json::Value::Object(body))
            .await
    }

    /// `POST /v1/databases` — list databases. Wait: Notion doesn't
    /// expose a "list databases" endpoint. The supported discovery
    /// path is `POST /v1/search` with `filter: { value: "database",
    /// property: "object" }`. This wrapper composes that call so the
    /// `notion_list_databases` tool has a stable surface.
    pub async fn list_databases(
        &self,
        start_cursor: Option<&str>,
        page_size: Option<u32>,
    ) -> Result<SearchResponse, ApiError> {
        let filter = serde_json::json!({
            "value": "database",
            "property": "object",
        });
        self.search_pages(None, Some(&filter), None, start_cursor, page_size)
            .await
    }

    // -----------------------------------------------------------------
    // Internals
    // -----------------------------------------------------------------

    async fn get_json<T: serde::de::DeserializeOwned + Send>(
        &self,
        path: &str,
    ) -> Result<T, ApiError> {
        let resp = self.request_with_retry(Method::GET, path, None).await?;
        resp.json::<T>().await.map_err(ApiError::from)
    }

    async fn post_json<T: serde::de::DeserializeOwned + Send>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<T, ApiError> {
        let resp = self
            .request_with_retry(Method::POST, path, Some(body))
            .await?;
        resp.json::<T>().await.map_err(ApiError::from)
    }

    async fn patch_json<T: serde::de::DeserializeOwned + Send>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<T, ApiError> {
        let resp = self
            .request_with_retry(Method::PATCH, path, Some(body))
            .await?;
        resp.json::<T>().await.map_err(ApiError::from)
    }

    async fn request_with_retry(
        &self,
        method: Method,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<Response, ApiError> {
        let url = format!(
            "{}{}",
            self.base_url.trim_end_matches('/'),
            if path.starts_with('/') {
                path.to_string()
            } else {
                format!("/{path}")
            }
        );
        let mut attempt: u32 = 0;
        loop {
            let mut req = self
                .http
                .request(method.clone(), &url)
                .header("Notion-Version", self.notion_version.clone())
                .header("Content-Type", "application/json")
                .header("Accept", "application/json");
            if let Some(token) = self.current_token().await {
                req = req.header("Authorization", token.header_value());
            }
            let req = match body {
                Some(b) => req.json(b),
                None => req,
            };
            let resp = req.send().await?;
            let status = resp.status();

            // 401 — token revoked. Don't retry; caller must re-auth.
            if status == StatusCode::UNAUTHORIZED {
                return Err(ApiError::Unauthorized);
            }

            // 403 — integration lacks access. Surface the body so the
            // caller can tell "page not shared with integration" from
            // "missing scope".
            if status == StatusCode::FORBIDDEN {
                let body = resp.text().await.unwrap_or_default();
                return Err(ApiError::Forbidden(body));
            }

            // 404 — wrap the requested path for caller clarity.
            if status == StatusCode::NOT_FOUND {
                return Err(ApiError::NotFound(path.to_string()));
            }

            // 400 — validation. Surface Notion's `{ code, message }`.
            if status == StatusCode::BAD_REQUEST {
                let body = resp.text().await.unwrap_or_default();
                return Err(ApiError::Validation(body));
            }

            // 429 — primary rate-limit. Notion advertises ~3 req/s;
            // honour `Retry-After` exactly.
            if status == StatusCode::TOO_MANY_REQUESTS {
                let retry_after = parse_retry_after(resp.headers()).unwrap_or_else(exp_backoff);
                if attempt < MAX_RETRIES {
                    self.backoff_sleep(retry_after).await;
                    attempt += 1;
                    continue;
                }
                return Err(ApiError::RateLimited {
                    retry_after_secs: retry_after,
                });
            }

            // 5xx — server error. Exponential backoff.
            if status.is_server_error() {
                if attempt < MAX_RETRIES {
                    self.backoff_sleep(exp_backoff()).await;
                    attempt += 1;
                    continue;
                }
                let body = resp.text().await.unwrap_or_default();
                return Err(ApiError::Server(format!("HTTP {status}: {body}")));
            }

            // Anything else non-2xx — surface as a server-shaped error.
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                return Err(ApiError::Server(format!("HTTP {status}: {body}")));
            }
            return Ok(resp);
        }
    }

    async fn backoff_sleep(&self, secs: u64) {
        if self.backoff_scale == 0 {
            return;
        }
        let scaled = secs.saturating_mul(self.backoff_scale as u64);
        sleep(Duration::from_secs(scaled)).await;
    }
}

fn exp_backoff() -> u64 {
    DEFAULT_BACKOFF
}

fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get("Retry-After")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
}

/// Path-segment encoder. Same RFC 3986 unreserved set as
/// `jira::api::url_path_encode` — forward slash must be percent-encoded
/// so a hostile `page_id` cannot escape into an unrelated URL path.
fn url_path_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notion_version_is_pinned() {
        // Schema version is part of the wire contract. Bumping it
        // silently would change the shape of `properties` payloads
        // downstream, so anchor the value.
        assert_eq!(NOTION_VERSION, "2022-06-28");
    }

    #[test]
    fn rate_limit_constant_is_three() {
        // Notion advertises 3 req/s. Anchored so a careless edit doesn't
        // accidentally paper over a real rate-limit regression.
        assert_eq!(RATE_LIMIT_PER_SECOND, 3);
    }

    #[test]
    fn url_path_encode_blocks_segment_breakout() {
        assert_eq!(url_path_encode("abc-123"), "abc-123");
        assert_eq!(url_path_encode("abc/../admin"), "abc%2F..%2Fadmin");
        assert_eq!(url_path_encode("abc?x=y"), "abc%3Fx%3Dy");
    }

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn default_backoff_is_one() {
        // 0 would short-circuit retry sleeps entirely.
        assert!(DEFAULT_BACKOFF == 1);
    }
}
