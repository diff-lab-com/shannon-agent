//! GitHub REST client.
//!
//! Six methods backing the MCP tools: `list_issues`, `get_issue`,
//! `create_issue`, `comment`, `list_prs`, `review_pr`. We hand-roll the
//! HTTP via `reqwest` (already in the workspace) to keep the dependency
//! footprint light — see Q1 in `docs/plans/saas-mcp-github.md`.
//!
//! ## Rate-limit handling
//!
//! - Every response is inspected for `x-ratelimit-remaining` and
//!   `x-ratelimit-reset`. When the remaining budget is `0` we sleep
//!   until the reset epoch before returning the response.
//! - On HTTP 429 (primary rate limit) we read `retry-after` and sleep
//!   that long. If `retry-after` is missing we fall back to the
//!   exponential schedule below.
//! - On HTTP 403 with `x-ratelimit-remaining: 0` (GitHub's "secondary
//!   rate limit / abuse detection" — see
//!   <https://docs.github.com/en/rest/overview/resources-in-the-rest-api#secondary-rate-limits>)
//!   we apply exponential backoff starting at 1s.
//! - On 5xx we apply the same exponential backoff.
//!
//! ### Exponential backoff
//!
//! 1 s → 2 s → 4 s → 8 s, then capped at 30 s, max 3 retries (so a
//! single request can wait at most `1 + 2 + 4 = 7 s` of backoff across
//! 3 retries; the 4th attempt is terminal). 429s honour the server's
//! `retry-after` even when it exceeds the backoff cap.

use std::sync::Arc;
use std::time::Duration;

use reqwest::{Client, Response, StatusCode};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;
use tokio::time::sleep;

use crate::github::auth::Token;

/// API base for `api.github.com` REST. Overrideable for tests via
/// [`GitHubClient::with_base_url`].
pub const DEFAULT_BASE_URL: &str = "https://api.github.com";

/// User-Agent the client sends. Required by GitHub; missing it can
/// cause 403s.
const USER_AGENT: &str = "shannon-mcp-saas/0.7";

/// Maximum retries for a single request (after the initial attempt).
const MAX_RETRIES: u32 = 3;

/// Sleep budget for exponential backoff: 1s, 2s, 4s, then capped.
const MAX_BACKOFF_SECS: u64 = 30;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("unauthorized (401) — token may be revoked or missing scopes")]
    Unauthorized,
    #[error("forbidden (403) — {0}")]
    Forbidden(String),
    #[error("not found (404) — {0}/{1} #{2}")]
    NotFound(String, String, u64),
    #[error("validation failed (422) — {0}")]
    Validation(String),
    #[error("rate-limited; retry after {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },
    #[error("GitHub secondary rate-limit / abuse detection; backoff applied")]
    SecondaryRateLimit,
    #[error("server error (5xx): {0}")]
    Server(String),
    #[error("JSON deserialization: {0}")]
    Json(String),
    #[error("URL parse: {0}")]
    Url(String),
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

/// GitHub Issue (subset we care about for the 6 tools).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub html_url: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub labels: Vec<Label>,
    #[serde(default)]
    pub user: Option<User>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// GitHub Pull Request. Mirrors the issue shape for the read paths;
/// `merge_commit_sha` is needed for `review_pr` to anchor to a commit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub html_url: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub user: Option<User>,
    #[serde(default)]
    pub head: Option<PrRef>,
    #[serde(default)]
    pub base: Option<PrRef>,
    #[serde(default)]
    pub merge_commit_sha: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrRef {
    #[serde(default)]
    pub ref_: Option<String>,
    #[serde(default)]
    pub sha: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Label {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub login: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub id: u64,
    pub body: String,
    pub html_url: String,
    pub user: Option<User>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    pub id: u64,
    pub state: String,
    pub html_url: String,
    pub submitted_at: Option<String>,
    pub body: Option<String>,
    pub user: Option<User>,
}

/// State filter for `list_issues` and `list_prs`.
#[derive(Debug, Clone, Copy)]
pub enum IssueState {
    Open,
    Closed,
    All,
}

impl IssueState {
    fn as_str(&self) -> &'static str {
        match self {
            IssueState::Open => "open",
            IssueState::Closed => "closed",
            IssueState::All => "all",
        }
    }
}

/// Review event for `review_pr`.
#[derive(Debug, Clone, Copy)]
pub enum ReviewEvent {
    Approve,
    RequestChanges,
    Comment,
}

impl ReviewEvent {
    fn as_str(&self) -> &'static str {
        match self {
            ReviewEvent::Approve => "APPROVE",
            ReviewEvent::RequestChanges => "REQUEST_CHANGES",
            ReviewEvent::Comment => "COMMENT",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "APPROVE" => Some(ReviewEvent::Approve),
            "REQUEST_CHANGES" => Some(ReviewEvent::RequestChanges),
            "COMMENT" => Some(ReviewEvent::Comment),
            _ => None,
        }
    }
}

/// Async REST client. Carries a long-lived `reqwest::Client` plus the
/// current token (which can rotate without rebuilding the pool).
pub struct GitHubClient {
    http: Client,
    base_url: String,
    token: Arc<RwLock<Option<Token>>>,
    /// Test hook: a multiplier applied to the exponential-backoff
    /// sleeps. 0 makes the test instant; 1 (default) preserves the
    /// production 1s/2s/4s schedule. `#[cfg(test)]` is not used so
    /// integration tests outside this crate can also set it via
    /// `with_backoff_scale`.
    backoff_scale: u32,
}

impl GitHubClient {
    /// Construct with a pre-built `reqwest::Client` and an initial
    /// token. Pass `None` for unauthenticated calls (e.g. unauthed
    /// `/user` probe).
    pub fn new(http: Client, token: Option<Token>) -> Self {
        Self {
            http,
            base_url: DEFAULT_BASE_URL.to_string(),
            token: Arc::new(RwLock::new(token)),
            backoff_scale: 1,
        }
    }

    /// Override the base URL (used in mockito-backed tests).
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    /// Scale the exponential-backoff sleeps. Used by tests to make
    /// rate-limit retries instant. `0` = no sleep, `1` = production
    /// schedule, `2` = twice as slow.
    pub fn with_backoff_scale(mut self, scale: u32) -> Self {
        self.backoff_scale = scale;
        self
    }

    /// Set / replace the active token. Write-locks for the briefest
    /// possible moment.
    pub async fn set_token(&self, token: Token) {
        *self.token.write().await = Some(token);
    }

    async fn current_token(&self) -> Option<Token> {
        self.token.read().await.clone()
    }

    // -----------------------------------------------------------------
    // Read endpoints
    // -----------------------------------------------------------------

    pub async fn list_issues(
        &self,
        owner: &str,
        repo: &str,
        state: IssueState,
        since: Option<&str>,
        per_page: Option<u8>,
        page: Option<u32>,
    ) -> Result<Vec<Issue>, ApiError> {
        let mut path = format!("/repos/{owner}/{repo}/issues?state={}", state.as_str());
        if let Some(s) = since {
            path.push_str("&since=");
            path.push_str(&url_encode(s));
        }
        if let Some(p) = per_page {
            path.push_str(&format!("&per_page={p}"));
        }
        if let Some(p) = page {
            path.push_str(&format!("&page={p}"));
        }
        // `/repos/{}/{}/issues` returns PRs as well as issues. The 6-tool
        // contract says "issues", so we deserialize into `IssueRaw` (which
        // carries the `pull_request` discriminator) and drop PRs.
        let raw: Vec<IssueRaw> = self.get_json(&path).await?;
        Ok(raw
            .into_iter()
            .filter(|i| i.pull_request.is_none())
            .map(Into::into)
            .collect())
    }

    pub async fn get_issue(&self, owner: &str, repo: &str, number: u64) -> Result<Issue, ApiError> {
        let path = format!("/repos/{owner}/{repo}/issues/{number}");
        // /issues/{n} returns a PR-shaped object for PRs. We use the raw
        // shape to detect that and surface a clearer error in the tools
        // layer (it should call `list_prs` instead).
        let raw: IssueRaw = self.get_json(&path).await?;
        if raw.pull_request.is_some() {
            return Err(ApiError::Validation(format!(
                "#{number} is a pull request, not an issue"
            )));
        }
        Ok(raw.into())
    }

    pub async fn list_prs(
        &self,
        owner: &str,
        repo: &str,
        state: IssueState,
        per_page: Option<u8>,
        page: Option<u32>,
    ) -> Result<Vec<PullRequest>, ApiError> {
        let mut path = format!("/repos/{owner}/{repo}/pulls?state={}", state.as_str());
        if let Some(p) = per_page {
            path.push_str(&format!("&per_page={p}"));
        }
        if let Some(p) = page {
            path.push_str(&format!("&page={p}"));
        }
        let body: Vec<PullRequest> = self.get_json(&path).await?;
        Ok(body)
    }

    // -----------------------------------------------------------------
    // Write endpoints
    // -----------------------------------------------------------------

    pub async fn create_issue(
        &self,
        owner: &str,
        repo: &str,
        title: &str,
        body: Option<&str>,
        labels: Option<&[String]>,
        assignees: Option<&[String]>,
    ) -> Result<Issue, ApiError> {
        let path = format!("/repos/{owner}/{repo}/issues");
        let mut payload = serde_json::json!({ "title": title });
        if let Some(b) = body {
            payload["body"] = serde_json::Value::String(b.to_string());
        }
        if let Some(ls) = labels {
            payload["labels"] = serde_json::json!(ls);
        }
        if let Some(ass) = assignees {
            payload["assignees"] = serde_json::json!(ass);
        }
        let raw: IssueRaw = self.post_json(&path, &payload).await?;
        Ok(raw.into())
    }

    pub async fn comment(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        body: &str,
    ) -> Result<Comment, ApiError> {
        let path = format!("/repos/{owner}/{repo}/issues/{number}/comments");
        let payload = serde_json::json!({ "body": body });
        self.post_json(&path, &payload).await
    }

    pub async fn review_pr(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        event: ReviewEvent,
        body: Option<&str>,
        commit_id: Option<&str>,
    ) -> Result<Review, ApiError> {
        let path = format!("/repos/{owner}/{repo}/pulls/{number}/reviews");
        let mut payload = serde_json::json!({ "event": event.as_str() });
        if let Some(b) = body {
            payload["body"] = serde_json::Value::String(b.to_string());
        }
        if let Some(c) = commit_id {
            payload["commit_id"] = serde_json::Value::String(c.to_string());
        }
        self.post_json(&path, &payload).await
    }

    // -----------------------------------------------------------------
    // Internals
    // -----------------------------------------------------------------

    async fn get_json<T: serde::de::DeserializeOwned + Send>(
        &self,
        path: &str,
    ) -> Result<T, ApiError> {
        let resp = self
            .request_with_retry(reqwest::Method::GET, path, None)
            .await?;
        resp.json::<T>().await.map_err(ApiError::from)
    }

    async fn post_json<T: serde::de::DeserializeOwned + Send>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<T, ApiError> {
        let resp = self
            .request_with_retry(reqwest::Method::POST, path, Some(body))
            .await?;
        resp.json::<T>().await.map_err(ApiError::from)
    }

    /// Build + send + retry. Backoff schedule lives here.
    async fn request_with_retry(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<Response, ApiError> {
        let url = format!("{}{path}", self.base_url);
        let mut attempt: u32 = 0;
        loop {
            let token = self.current_token().await;
            let mut req = self
                .http
                .request(method.clone(), &url)
                .header("User-Agent", USER_AGENT)
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28");
            if let Some(t) = token.as_ref() {
                req = req.header("Authorization", t.header_value());
            }
            let req = match body {
                Some(b) => req.json(b),
                None => req,
            };

            let resp = req.send().await?;
            let status = resp.status();

            // 401 — token revoked / missing scope. Do not retry; the
            // caller should re-authorize.
            if status == StatusCode::UNAUTHORIZED {
                return Err(ApiError::Unauthorized);
            }

            // 403 — could be forbidden, could be secondary rate-limit.
            // GitHub signals the latter with `x-ratelimit-remaining: 0`
            // (and no `retry-after`); the former does not touch the
            // rate-limit headers.
            if status == StatusCode::FORBIDDEN {
                let remaining = parse_rate_remaining(resp.headers());
                if matches!(remaining, Some(0)) {
                    if attempt < MAX_RETRIES {
                        let backoff = exp_backoff(attempt);
                        tracing::warn!(
                            attempt = attempt + 1,
                            backoff_secs = backoff,
                            "GitHub secondary rate-limit / abuse detection; backing off"
                        );
                        self.backoff_sleep(backoff).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(ApiError::SecondaryRateLimit);
                }
                let body = resp.text().await.unwrap_or_default();
                return Err(ApiError::Forbidden(body));
            }

            // 404 — record owner/repo/number for the error message.
            if status == StatusCode::NOT_FOUND {
                let (owner, repo, n) = parse_owner_repo_number(path);
                return Err(ApiError::NotFound(owner, repo, n));
            }

            // 422 — validation. Read body for the GitHub error message
            // and surface it.
            if status == StatusCode::UNPROCESSABLE_ENTITY {
                let body = resp.text().await.unwrap_or_default();
                return Err(ApiError::Validation(body));
            }

            // 429 — primary rate-limit. Honour `retry-after` if present.
            if status == StatusCode::TOO_MANY_REQUESTS {
                let retry_after = parse_retry_after(resp.headers());
                let sleep_for = retry_after.unwrap_or_else(|| exp_backoff(attempt));
                if attempt < MAX_RETRIES {
                    tracing::warn!(
                        attempt = attempt + 1,
                        retry_after_secs = sleep_for,
                        "HTTP 429 from GitHub; sleeping then retrying"
                    );
                    self.backoff_sleep(sleep_for).await;
                    attempt += 1;
                    continue;
                }
                return Err(ApiError::RateLimited {
                    retry_after_secs: sleep_for,
                });
            }

            // 5xx — server error. Exponential backoff.
            if status.is_server_error() {
                if attempt < MAX_RETRIES {
                    let backoff = exp_backoff(attempt);
                    tracing::warn!(
                        attempt = attempt + 1,
                        status = status.as_u16(),
                        backoff_secs = backoff,
                        "GitHub 5xx; backing off"
                    );
                    self.backoff_sleep(backoff).await;
                    attempt += 1;
                    continue;
                }
                let body = resp.text().await.unwrap_or_default();
                return Err(ApiError::Server(format!("HTTP {status}: {body}")));
            }

            // 2xx — but we may still be near the limit. Pre-emptively
            // sleep if `x-ratelimit-remaining` is 0.
            if status.is_success() {
                let remaining = parse_rate_remaining(resp.headers());
                if matches!(remaining, Some(0)) {
                    if let Some(reset_at) = parse_rate_reset(resp.headers()) {
                        let now = chrono::Utc::now().timestamp() as u64;
                        if reset_at > now {
                            let wait = (reset_at - now).min(60);
                            tracing::warn!(
                                wait_secs = wait,
                                "x-ratelimit-remaining=0; sleeping until reset"
                            );
                            // Do **not** retry — just return this
                            // response after waiting. The remaining
                            // budget is for the *next* call.
                            sleep(Duration::from_secs(wait)).await;
                        }
                    }
                }
                return Ok(resp);
            }

            // Other 4xx — treat as a server error for caller clarity.
            let body = resp.text().await.unwrap_or_default();
            return Err(ApiError::Server(format!("HTTP {status}: {body}")));
        }
    }
}

/// `Issue` as it comes back from the API plus a `pull_request` field
/// that GitHub sets when the issue is actually a PR. We use it to
/// post-filter the list and to surface a clearer error from `get_issue`.
#[derive(Debug, Deserialize)]
struct IssueRaw {
    number: u64,
    title: String,
    state: String,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    labels: Vec<Label>,
    #[serde(default)]
    user: Option<User>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default, rename = "pull_request")]
    pull_request: Option<serde_json::Value>,
}

impl From<IssueRaw> for Issue {
    fn from(r: IssueRaw) -> Self {
        Issue {
            number: r.number,
            title: r.title,
            state: r.state,
            html_url: r.html_url,
            body: r.body,
            labels: r.labels,
            user: r.user,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

fn exp_backoff(attempt: u32) -> u64 {
    // 2^attempt seconds, capped. attempt 0 → 1s, 1 → 2s, 2 → 4s.
    let base = 1u64.checked_shl(attempt).unwrap_or(MAX_BACKOFF_SECS);
    base.min(MAX_BACKOFF_SECS)
}

impl GitHubClient {
    /// Apply the configured backoff scale. Tests pass `0` to skip
    /// sleeps entirely; production always uses `1`.
    async fn backoff_sleep(&self, secs: u64) {
        if self.backoff_scale == 0 {
            return;
        }
        let scaled = secs.saturating_mul(self.backoff_scale as u64);
        sleep(Duration::from_secs(scaled)).await;
    }
}

fn parse_rate_remaining(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get("x-ratelimit-remaining")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
}

fn parse_rate_reset(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get("x-ratelimit-reset")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
}

fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    let v = headers.get("retry-after")?.to_str().ok()?;
    // RFC 7231: delta-seconds OR HTTP-date. We only handle the former
    // (GitHub emits it).
    v.parse().ok()
}

/// Naive owner/repo/number extraction for error messages. We accept
/// any of the four `/repos/.../.../issues[N]` shapes:
///   `/repos/{owner}/{repo}/issues/{n}`     → (owner, repo, n)
///   `/repos/{owner}/{repo}/issues/{n}/comments` → (owner, repo, n)
///   `/repos/{owner}/{repo}/pulls/{n}/reviews`   → (owner, repo, n)
/// Other shapes return `("", "", 0)`.
fn parse_owner_repo_number(path: &str) -> (String, String, u64) {
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    if parts.len() < 5 || parts[0] != "repos" {
        return (String::new(), String::new(), 0);
    }
    let owner = parts[1].to_string();
    let repo = parts[2].to_string();
    let n: u64 = parts.get(4).and_then(|p| p.parse().ok()).unwrap_or(0);
    (owner, repo, n)
}

fn url_encode(s: &str) -> String {
    // Minimal URL-component encoder. Sufficient for ISO 8601 strings.
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    out
}

// Public re-export of the ReviewEvent parser so the tools layer can
// decode its string arg without reimplementing the match.
pub fn parse_review_event(s: &str) -> Option<ReviewEvent> {
    ReviewEvent::parse(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exp_backoff_doubles() {
        assert_eq!(exp_backoff(0), 1);
        assert_eq!(exp_backoff(1), 2);
        assert_eq!(exp_backoff(2), 4);
        assert_eq!(exp_backoff(3), 8);
        assert_eq!(exp_backoff(4), 16);
        assert_eq!(exp_backoff(5), 30);
        assert_eq!(exp_backoff(20), 30);
    }

    #[test]
    fn parse_owner_repo_number_handles_all_shapes() {
        assert_eq!(
            parse_owner_repo_number("/repos/o/r/issues/7"),
            ("o".into(), "r".into(), 7)
        );
        assert_eq!(
            parse_owner_repo_number("/repos/o/r/issues/7/comments"),
            ("o".into(), "r".into(), 7)
        );
        assert_eq!(
            parse_owner_repo_number("/repos/o/r/pulls/42/reviews"),
            ("o".into(), "r".into(), 42)
        );
        assert_eq!(
            parse_owner_repo_number("/user"),
            (String::new(), String::new(), 0)
        );
    }

    #[test]
    fn review_event_round_trip() {
        for s in ["APPROVE", "REQUEST_CHANGES", "COMMENT"] {
            let e = parse_review_event(s).expect("parses");
            assert_eq!(e.as_str(), s);
        }
        assert!(parse_review_event("bogus").is_none());
    }

    #[test]
    fn url_encode_handles_iso8601() {
        assert_eq!(
            url_encode("2024-01-01T00:00:00Z"),
            "2024-01-01T00%3A00%3A00Z"
        );
        assert_eq!(url_encode("plain"), "plain");
    }
}
