//! Jira Cloud REST API v3 client with rate-limit handling.

use std::time::Duration;

use base64::Engine;
use reqwest::{Client, Method, Response, StatusCode};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;
use tokio::time::sleep;

use crate::jira::auth::{CredentialKind, Token};

/// Cloud REST base. Overrideable via [`JiraClient::with_base_url`] —
/// for OAuth the real base is `https://api.atlassian.com/ex/jira/<cloudid>/rest/api/3`.
pub const DEFAULT_BASE_URL: &str = "https://api.atlassian.com";

/// Maximum retries on a single request (after the initial attempt).
const MAX_RETRIES: u32 = 3;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("unauthorized (401) — token may be revoked or missing scope")]
    Unauthorized,
    #[error("forbidden (403) — {0}")]
    Forbidden(String),
    #[error("not found (404) — {0}")]
    NotFound(String),
    #[error("validation failed (400) — {0}")]
    Validation(String),
    #[error("rate-limited; retry after {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },
    #[error("server error (5xx): {0}")]
    Server(String),
    #[error("JSON deserialization: {0}")]
    Json(String),
}

impl From<reqwest::Error> for ApiError {
    fn from(e: reqwest::Error) -> Self {
        ApiError::Http(e.to_string())
    }
}
impl From<serde_json::Error> for ApiError {
    fn from(e: serde_json::Error) -> Self {
        ApiError::Json(e.to_string())
    }
}

/// Subset of a Jira issue we return for `get_issue` / `search_issues`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub id: String,
    pub key: String,
    #[serde(default)]
    pub fields: Option<serde_json::Value>,
}

/// Result wrapper for `search_issues`.
#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    issues: Vec<Issue>,
    // KEEP: Atlassian returns these for pagination bookkeeping; clients
    // occasionally want them for "next page" UX even though our 4-tool
    // contract exposes only `start_at`/`max_results` as inputs.
    #[serde(default)]
    #[allow(dead_code)]
    start_at: Option<u64>,
    #[serde(default)]
    #[allow(dead_code)]
    max_results: Option<u64>,
    #[serde(default)]
    total: Option<u64>,
}

/// Result wrapper for `create_issue` / `get_issue` / `transition`.
#[derive(Debug, Deserialize)]
struct IssueResponse {
    id: String,
    key: String,
}

#[derive(Debug, Deserialize)]
struct TransitionsResponse {
    #[serde(default)]
    transitions: Vec<Transition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transition {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub to: Option<serde_json::Value>,
}

/// Async REST client. Carries a long-lived `reqwest::Client` plus the
/// current credentials (which can rotate without rebuilding the pool).
pub struct JiraClient {
    http: Client,
    base_url: String,
    credential: RwLock<Option<CredentialKind>>,
    /// Multiplier applied to backoff sleeps. 0 = no sleep, 1 = production.
    backoff_scale: u32,
}

impl JiraClient {
    pub fn new(http: Client, credential: Option<CredentialKind>) -> Self {
        Self {
            http,
            base_url: DEFAULT_BASE_URL.to_string(),
            credential: RwLock::new(credential),
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
    pub async fn set_credential(&self, kind: CredentialKind) {
        *self.credential.write().await = Some(kind);
    }
    async fn current(&self) -> Option<CredentialKind> {
        self.credential.read().await.clone()
    }

    /// Convenience helper: build the base URL for a tenant-scoped
    /// request when the credential carries a cloudid.
    pub async fn tenant_base(&self) -> String {
        match self.current().await {
            Some(CredentialKind::OAuth {
                cloudid: Some(cid), ..
            }) => {
                // Percent-encode the cloudid so a malicious or malformed
                // credential cannot inject path/query segments.
                let cid_enc = url_path_encode(&cid);
                format!(
                    "{}/ex/jira/{}/rest/api/3",
                    self.base_url.trim_end_matches('/'),
                    cid_enc
                )
            }
            _ => format!("{}/rest/api/3", self.base_url.trim_end_matches('/')),
        }
    }

    /// `GET /search` — JQL search.
    pub async fn search_issues(
        &self,
        jql: &str,
        max_results: Option<u32>,
        start_at: Option<u32>,
    ) -> Result<(Vec<Issue>, Option<u64>), ApiError> {
        let mut path = String::from("/search?jql=");
        path.push_str(&url_encode(jql));
        if let Some(n) = max_results {
            path.push_str(&format!("&maxResults={n}"));
        }
        if let Some(o) = start_at {
            path.push_str(&format!("&startAt={o}"));
        }
        let parsed: SearchResponse = self.get_json(&path).await?;
        Ok((parsed.issues, parsed.total))
    }

    /// `GET /issue/{key}` — full issue body. Returns the raw JSON so the
    /// caller can pick fields; the schema is large and varies by project.
    pub async fn get_issue(&self, key: &str) -> Result<serde_json::Value, ApiError> {
        validate_issue_key(key)?;
        let path = format!("/issue/{}", url_path_encode(key));
        let v: serde_json::Value = self.get_json(&path).await?;
        Ok(v)
    }

    /// `POST /issue` — create issue. The Atlassian v3 schema requires
    /// ADF for body; we accept a plain string and wrap it into a single
    /// paragraph node. The returned `Issue` carries the new key.
    pub async fn create_issue(
        &self,
        project_key: &str,
        summary: &str,
        issue_type: &str,
        description: Option<&str>,
    ) -> Result<Issue, ApiError> {
        let path = "/issue".to_string();
        let payload = build_create_payload(project_key, summary, issue_type, description);
        let resp: IssueResponse = self.post_json(&path, &payload).await?;
        Ok(Issue {
            id: resp.id,
            key: resp.key,
            fields: None,
        })
    }

    /// `GET /issue/{key}/transitions` — list available workflow transitions.
    pub async fn list_transitions(&self, key: &str) -> Result<Vec<Transition>, ApiError> {
        validate_issue_key(key)?;
        let path = format!("/issue/{}/transitions", url_path_encode(key));
        let parsed: TransitionsResponse = self.get_json(&path).await?;
        Ok(parsed.transitions)
    }

    /// `POST /issue/{key}/transitions` — move issue to a new status.
    /// Jira returns 204 No Content on success; we ignore the body.
    pub async fn transition(&self, key: &str, target_status: &str) -> Result<Issue, ApiError> {
        validate_issue_key(key)?;
        let transitions = self.list_transitions(key).await?;
        let id = transitions
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(target_status))
            .map(|t| t.id.clone())
            .ok_or_else(|| {
                ApiError::Validation(format!("no transition named '{target_status}' for {key}"))
            })?;
        let path = format!("/issue/{}/transitions", url_path_encode(key));
        let payload = serde_json::json!({ "transition": { "id": id } });
        self.post_no_response(&path, &payload).await?;
        Ok(Issue {
            id: String::new(),
            key: key.to_string(),
            fields: None,
        })
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

    /// POST that ignores the response body (e.g. transitions which
    /// return 204 No Content). Same retry / rate-limit handling as
    /// `post_json`.
    async fn post_no_response(&self, path: &str, body: &serde_json::Value) -> Result<(), ApiError> {
        let _ = self
            .request_with_retry(Method::POST, path, Some(body))
            .await?;
        Ok(())
    }

    async fn request_with_retry(
        &self,
        method: Method,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<Response, ApiError> {
        let base = self.tenant_base().await;
        let url = format!("{base}{path}");
        let mut attempt: u32 = 0;
        loop {
            let mut req = self
                .http
                .request(method.clone(), &url)
                .header("User-Agent", "shannon-mcp-saas/0.7")
                .header("Accept", "application/json");
            if let Some(cred) = self.current().await.as_ref() {
                match cred {
                    CredentialKind::ApiToken { email, token } => {
                        let raw = format!("{email}:{token}");
                        let encoded = base64::engine::general_purpose::STANDARD.encode(raw);
                        req = req.header("Authorization", format!("Basic {encoded}"));
                    }
                    CredentialKind::OAuth { access_token, .. } => {
                        req = req.header("Authorization", format!("Bearer {access_token}"));
                    }
                }
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

            // 403 — forbidden. Could be scope or rate. Atlassian emits
            // `X-RateLimit-Remaining` near zero on rate-limited calls.
            if status == StatusCode::FORBIDDEN {
                if matches!(parse_rate_remaining(resp.headers()), Some(0)) {
                    if attempt < MAX_RETRIES {
                        let retry = parse_retry_after(resp.headers()).unwrap_or_else(exp_backoff);
                        self.backoff_sleep(retry).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(ApiError::RateLimited {
                        retry_after_secs: parse_retry_after(resp.headers())
                            .unwrap_or(DEFAULT_BACKOFF),
                    });
                }
                let body = resp.text().await.unwrap_or_default();
                return Err(ApiError::Forbidden(body));
            }

            // 404 — wrap path for caller.
            if status == StatusCode::NOT_FOUND {
                return Err(ApiError::NotFound(path.to_string()));
            }

            // 400 — validation. Surface Atlassian's error message list.
            if status == StatusCode::BAD_REQUEST {
                let body = resp.text().await.unwrap_or_default();
                return Err(ApiError::Validation(body));
            }

            // 429 — primary rate-limit. Honour Retry-After.
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
                    let backoff = exp_backoff();
                    self.backoff_sleep(backoff).await;
                    attempt += 1;
                    continue;
                }
                let body = resp.text().await.unwrap_or_default();
                return Err(ApiError::Server(format!("HTTP {status}: {body}")));
            }

            // 2xx. If the rate-limit budget is zero and a reset timestamp
            // is in the future, sleep until reset before returning. We
            // do NOT retry here — the budget is for the next call.
            if status.is_success() {
                let remaining = parse_rate_remaining(resp.headers());
                if matches!(remaining, Some(0)) {
                    if let Some(reset_at) = parse_rate_reset(resp.headers()) {
                        let now = chrono::Utc::now().timestamp() as u64;
                        if reset_at > now {
                            let wait = (reset_at - now).min(60);
                            sleep(Duration::from_secs(wait)).await;
                        }
                    }
                }
                return Ok(resp);
            }

            // Anything else — treat as server error for caller clarity.
            let body = resp.text().await.unwrap_or_default();
            return Err(ApiError::Server(format!("HTTP {status}: {body}")));
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

/// Translate a token instance (without its credential kind) into a header
/// value — kept for callers that want the plain `Bearer` form.
#[allow(dead_code)]
pub fn token_header(token: &Token) -> String {
    token.header_value()
}

fn build_create_payload(
    project_key: &str,
    summary: &str,
    issue_type: &str,
    description: Option<&str>,
) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "fields": {
            "project": { "key": project_key },
            "summary": summary,
            "issuetype": { "name": issue_type },
        }
    });
    if let Some(desc) = description {
        payload["fields"]["description"] = serde_json::json!({
            "type": "doc",
            "version": 1,
            "content": [{
                "type": "paragraph",
                "content": [{ "type": "text", "text": desc }]
            }]
        });
    }
    payload
}

const DEFAULT_BACKOFF: u64 = 1;
fn exp_backoff() -> u64 {
    DEFAULT_BACKOFF
}

fn parse_rate_remaining(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get("X-RateLimit-Remaining")
        .or_else(|| headers.get("x-ratelimit-remaining"))
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
}

fn parse_rate_reset(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get("X-RateLimit-Reset")
        .or_else(|| headers.get("x-ratelimit-reset"))
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
}

fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    let v = headers.get("Retry-After")?.to_str().ok()?;
    v.parse().ok()
}

fn url_encode(s: &str) -> String {
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

/// Path-segment encoder. Same RFC 3986 unreserved set as `url_encode`,
/// but reserved for path segments: forward slash is NOT allowed inside
/// a single segment, so it is also percent-encoded.
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

/// Validate a Jira issue key shape before splicing it into a URL path.
/// Jira keys follow `<PROJECTKEY>-<NUMBER>` where PROJECTKEY is uppercase
/// alphanumeric + underscore and NUMBER is digits. Anything else is
/// either malformed or an injection attempt.
fn validate_issue_key(key: &str) -> Result<(), ApiError> {
    let mut parts = key.split('-');
    let project = parts.next().unwrap_or("");
    let number = parts.next().unwrap_or("");
    if parts.next().is_some() {
        return Err(ApiError::Validation(format!(
            "invalid issue key '{key}': expected PROJECT-NUMBER"
        )));
    }
    if project.is_empty()
        || !project
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        || project.len() > 32
    {
        return Err(ApiError::Validation(format!(
            "invalid issue key '{key}': bad project segment"
        )));
    }
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) || number.len() > 9 {
        return Err(ApiError::Validation(format!(
            "invalid issue key '{key}': bad number segment"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_create_payload_wraps_description_in_adf() {
        let v = build_create_payload("ENG", "Hello", "Task", Some("world"));
        assert_eq!(v["fields"]["project"]["key"], "ENG");
        assert_eq!(v["fields"]["summary"], "Hello");
        assert_eq!(v["fields"]["issuetype"]["name"], "Task");
        assert_eq!(v["fields"]["description"]["type"], "doc");
        assert_eq!(v["fields"]["description"]["version"], 1);
    }

    #[test]
    fn build_create_payload_omits_description_when_none() {
        let v = build_create_payload("ENG", "Hello", "Task", None);
        assert!(v["fields"]["description"].is_null());
    }

    #[test]
    fn url_encode_handles_jql_special_chars() {
        // JQL typically uses spaces, = and quotes; we must percent-encode
        // at least the space to keep the query string parseable.
        assert_eq!(url_encode("project = ENG"), "project%20%3D%20ENG");
    }

    #[test]
    fn url_path_encode_blocks_segment_breakout() {
        // A naive f-string concat would put `/admin` into the same
        // segment. url_path_encode must percent-encode the slash so
        // `get_json` cannot escape into an unrelated URL path.
        assert_eq!(url_path_encode("ENG-1/../admin"), "ENG-1%2F..%2Fadmin");
        assert_eq!(url_path_encode("ENG-1?x=y"), "ENG-1%3Fx%3Dy");
        // Existing shape must round-trip.
        assert_eq!(url_path_encode("ENG-1"), "ENG-1");
    }

    #[test]
    fn validate_issue_key_accepts_well_formed_and_rejects_injection() {
        // Well-formed
        for k in ["ENG-1", "ABC_DEF-42", "X-999999999"] {
            assert!(validate_issue_key(k).is_ok(), "{k} should be valid");
        }
        // Injection / malformed
        for bad in [
            "ENG-1/admin",
            "ENG-1?x=y",
            "../admin",
            "eng-1",        // lowercase project
            "ENG",          // no number
            "ENG-",         // empty number
            "ENG-1-2",      // extra segment
            "ENG_$admin-1", // disallowed char
            "",             // empty
        ] {
            assert!(validate_issue_key(bad).is_err(), "{bad} should be invalid");
        }
    }
}
