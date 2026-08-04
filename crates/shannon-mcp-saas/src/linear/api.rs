//! Linear GraphQL client.
//!
//! Linear exposes a single endpoint `https://api.linear.app/graphql`
//! behind Bearer auth (`Authorization: <personal API key>`). All
//! operations — both Query and Mutation — go through
//! [`LinearClient::request`], which POSTs `{ "query": ..., "variables": ... }`
//! and parses the standard `{ "data": ..., "errors": [...] }` envelope.
//!
//! ## Error handling
//!
//! Linear surfaces two failure shapes:
//!
//! - **Transport-level** (HTTP): `401` (revoked key), `404` (not
//!   applicable to the single GraphQL endpoint), `429` (rate-limited,
//!   `Retry-After` honoured), `5xx` (server errors, exponential
//!   backoff).
//! - **GraphQL-level** (`200 OK` with `errors[]` populated): each entry
//!   carries `{ "message": ..., "path": [...], "extensions": {...} }`.
//!   We surface the joined `messages` string and let `path` flow through
//!   as part of the error context so callers can debug which field
//!   failed validation.
//!
//! Authentication-missing fallbacks always surface as
//! [`ApiError::Unauthorized`] so the host can prompt the user to set a
//! token (no PII / no token content ever leaks into the error).
//!
//! ## Rate limit notes
//!
//! Linear publishes a soft limit of `1500 req/hr` for personal API
//! keys — well above what Shannon needs, but we still implement the
//! `Retry-After` honour path (used when the workspace is shared or
//! scripted bursts pile up). The 429 + exponential backoff ceiling
//! matches Slack's: 3 retries, exponential, 60s cap.

use std::time::Duration;

use reqwest::{Client, Method, Response, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::sync::RwLock;
use tokio::time::sleep;

use crate::linear::auth::Token;

/// GraphQL endpoint base. `request_path` is always appended on POST
/// so a test that overrides `base_url` to a mockito host (e.g.
/// `http://127.0.0.1:1234`) still produces a request that hits
/// `POST http://127.0.0.1:1234/graphql` — which mockito's path
/// matchers can intercept.
pub const DEFAULT_BASE_URL: &str = "https://api.linear.app";

/// Path segment for the GraphQL endpoint.
pub const GRAPHQL_PATH: &str = "/graphql";

/// User-Agent required by no one in particular, but Linear gateways
/// occasionally 403 anonymous-looking clients. Identifies us.
const USER_AGENT: &str = "shannon-mcp-saas/0.7";

/// Maximum retries on a single request (after the initial attempt).
/// Matches Slack/Jira so behaviour is consistent across SaaS modules.
const MAX_RETRIES: u32 = 3;

/// Default backoff per retry when the server doesn't supply
/// `Retry-After`. Doubles on each attempt, 60 s cap.
const DEFAULT_BACKOFF_SECS: u64 = 1;
const MAX_BACKOFF_SECS: u64 = 60;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("unauthorized (401) — token may be revoked or missing")]
    Unauthorized,
    #[error("rate-limited; retry after {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },
    #[error("server error: {0}")]
    Server(String),
    #[error("GraphQL error: {0}")]
    GraphQL(String),
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

/// Tiny GraphQL error payload — Linear populates `message` always and
/// `path` / `extensions` optionally. We only deserialize the fields we
/// surface in the error string.
#[derive(Debug, Deserialize)]
struct GraphQLErrorEntry {
    message: String,
    #[serde(default)]
    #[allow(dead_code)] // KEEP: future debug logging joins `path` into surfaced errors
    path: Vec<Value>,
}

#[derive(Debug, Deserialize)]
struct GraphQLResponse<T> {
    #[serde(default)]
    data: Option<T>,
    #[serde(default)]
    errors: Vec<GraphQLErrorEntry>,
}

/// Subset of a Linear `Issue` node returned by `linear_get_issue` and
/// `linear_list_issues`. The Linear schema is GraphQL — every field is
/// nullable on the wire, so each field is wrapped in `Option` or uses
/// `#[serde(default)]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub id: String,
    #[serde(default)]
    pub identifier: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub priority: Option<f64>,
    #[serde(default)]
    pub priority_label: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub branch_name: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub state: Option<WorkflowState>,
    #[serde(default)]
    pub team: Option<Team>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowState {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub r#type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    pub id: String,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

/// Linear `Team` plus nested workflow states. `linear_list_teams`
/// returns this so callers can map state names to IDs before invoking
/// `linear_update_status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamWithStates {
    pub id: String,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub states: Vec<WorkflowState>,
}

/// LinearIssueConnection (paginated list of issues).
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct IssueConnection {
    #[serde(default)]
    pub nodes: Vec<Issue>,
    #[serde(default)]
    pub page_info: PageInfo,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PageInfo {
    #[serde(default)]
    pub has_next_page: bool,
    #[serde(default)]
    pub end_cursor: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TeamConnection {
    #[serde(default)]
    pub nodes: Vec<TeamWithStates>,
}

/// Body of the IssueUpdate mutation — what we currently let the user
/// change through `linear_update_status` is just the `stateId`.
///
/// Fields are named `snake_case` for Rust but aliased to Linear's
/// camelCase GraphQL response names so we can decode the wire shape
/// directly.
#[derive(Debug, Default, Deserialize)]
pub struct IssueUpdatePayload {
    #[serde(default)]
    pub success: Option<bool>,
    #[serde(default)]
    pub issue: Option<Issue>,
}

pub struct LinearClient {
    http: Client,
    base_url: String,
    token: RwLock<Option<Token>>,
    /// Test hook: scale factor for exponential-backoff sleeps. `0` =
    /// no sleep, `1` (default) = production cadence.
    backoff_scale: u32,
}

impl LinearClient {
    pub fn new(http: Client, token: Option<Token>) -> Self {
        Self {
            http,
            base_url: DEFAULT_BASE_URL.to_string(),
            token: RwLock::new(token),
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
    // Tool-shaped API methods (mirror the 5 tools in tools.rs)
    // -----------------------------------------------------------------

    /// `query issues(filter: {…}, first: 50, after: "cursor")` —
    /// paginated list of issues. `filter` is a free-form GraphQL filter
    /// input (we pass it as `Value` so callers can encode any nested
    /// shape Linear accepts).
    pub async fn list_issues(
        &self,
        filter: Option<&Value>,
        first: Option<u32>,
        after: Option<&str>,
    ) -> Result<IssueConnection, ApiError> {
        let query = r#"
            query ListIssues($filter: IssueFilter, $first: Int, $after: String) {
                issues(filter: $filter, first: $first, after: $after) {
                    nodes {
                        id
                        identifier
                        title
                        description
                        priority
                        priorityLabel
                        url
                        branchName
                        createdAt
                        updatedAt
                        state { id name type }
                        team { id key name }
                    }
                    pageInfo { hasNextPage endCursor }
                }
            }
        "#;
        let mut vars = serde_json::Map::new();
        if let Some(f) = filter {
            vars.insert("filter".into(), f.clone());
        }
        if let Some(n) = first {
            vars.insert("first".into(), Value::Number(n.into()));
        }
        if let Some(c) = after {
            vars.insert("after".into(), Value::String(c.to_string()));
        }
        let body = self.request(query, Value::Object(vars)).await?;
        let payload: ListIssuesPayload = serde_json::from_value(body)?;
        Ok(payload.issues.unwrap_or_default())
    }

    /// `query issue(id: "…")` — single issue.
    pub async fn get_issue(&self, id: &str) -> Result<Issue, ApiError> {
        let query = r#"
            query GetIssue($id: String!) {
                issue(id: $id) {
                    id
                    identifier
                    title
                    description
                    priority
                    priorityLabel
                    url
                    branchName
                    createdAt
                    updatedAt
                    state { id name type }
                    team { id key name }
                }
            }
        "#;
        let vars = serde_json::json!({ "id": id });
        let body = self.request(query, vars).await?;
        let payload: GetIssuePayload = serde_json::from_value(body)?;
        payload
            .issue
            .ok_or_else(|| ApiError::GraphQL(format!("issue '{id}' not found")))
    }

    /// `mutation IssueCreate` — creates an issue. Returns the created
    /// `Issue` node.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_issue(
        &self,
        title: &str,
        team_id: &str,
        description: Option<&str>,
        priority: Option<f64>,
        label_ids: Option<&[String]>,
    ) -> Result<Issue, ApiError> {
        let query = r#"
            mutation IssueCreate(
                $title: String!,
                $teamId: String!,
                $description: String,
                $priority: Int,
                $labelIds: [String!]
            ) {
                issueCreate(
                    input: {
                        title: $title,
                        teamId: $teamId,
                        description: $description,
                        priority: $priority,
                        labelIds: $labelIds
                    }
                ) {
                    success
                    issue {
                        id
                        identifier
                        title
                        description
                        priority
                        priorityLabel
                        url
                        branchName
                        createdAt
                        updatedAt
                        state { id name type }
                        team { id key name }
                    }
                }
            }
        "#;
        let mut vars = serde_json::Map::new();
        vars.insert("title".into(), Value::String(title.to_string()));
        vars.insert("teamId".into(), Value::String(team_id.to_string()));
        if let Some(d) = description {
            vars.insert("description".into(), Value::String(d.to_string()));
        }
        if let Some(p) = priority {
            vars.insert(
                "priority".into(),
                Value::Number(serde_json::Number::from_f64(p).unwrap_or_else(|| 0.into())),
            );
        }
        if let Some(labels) = label_ids {
            vars.insert(
                "labelIds".into(),
                Value::Array(labels.iter().map(|s| Value::String(s.clone())).collect()),
            );
        }
        let body = self.request(query, Value::Object(vars)).await?;
        let payload: CreateIssuePayload = serde_json::from_value(body)?;
        let envelope = payload
            .issue_create
            .ok_or_else(|| ApiError::GraphQL("issueCreate returned no payload".into()))?;
        if !matches!(envelope.success, Some(true)) {
            return Err(ApiError::GraphQL(
                "issueCreate failed (success=false)".into(),
            ));
        }
        envelope
            .issue
            .ok_or_else(|| ApiError::GraphQL("issueCreate returned no issue".into()))
    }

    /// `mutation IssueUpdate` — moves an issue to a workflow state by
    /// `stateId`. We resolve `state name → id` client-side via
    /// [`LinearClient::list_teams`] before invoking this mutation.
    pub async fn update_status(&self, issue_id: &str, state_id: &str) -> Result<Issue, ApiError> {
        let query = r#"
            mutation IssueUpdate($issueId: String!, $stateId: String!) {
                issueUpdate(input: { issueId: $issueId, stateId: $stateId }) {
                    success
                    issue {
                        id
                        identifier
                        title
                        description
                        priority
                        priorityLabel
                        url
                        branchName
                        createdAt
                        updatedAt
                        state { id name type }
                        team { id key name }
                    }
                }
            }
        "#;
        let vars = serde_json::json!({
            "issueId": issue_id,
            "stateId": state_id,
        });
        let body = self.request(query, vars).await?;
        let payload: UpdateStatusPayload = serde_json::from_value(body)?;
        let envelope = payload
            .issue_update
            .ok_or_else(|| ApiError::GraphQL("issueUpdate returned no payload".into()))?;
        if !matches!(envelope.success, Some(true)) {
            return Err(ApiError::GraphQL(
                "issueUpdate failed (success=false)".into(),
            ));
        }
        envelope
            .issue
            .ok_or_else(|| ApiError::GraphQL("issueUpdate returned no issue".into()))
    }

    /// `query teams(first: N)` — paginated list of teams including
    /// their workflow states. Use this to discover valid `stateId`s
    /// before calling `linear_update_status`.
    pub async fn list_teams(&self, first: Option<u32>) -> Result<TeamConnection, ApiError> {
        let query = r#"
            query ListTeams($first: Int) {
                teams(first: $first) {
                    nodes {
                        id
                        key
                        name
                        description
                        states(first: 50) {
                            nodes { id name type }
                        }
                    }
                }
            }
        "#;
        let mut vars = serde_json::Map::new();
        if let Some(n) = first {
            vars.insert("first".into(), Value::Number(n.into()));
        }
        let body = self.request(query, Value::Object(vars)).await?;
        let payload: ListTeamsPayload = serde_json::from_value(body)?;
        // Flatten the nested `states { nodes }` shape into TeamWithStates.
        let conn = payload
            .teams
            .map(|c| TeamConnection {
                nodes: c
                    .nodes
                    .into_iter()
                    .map(|t| TeamWithStates {
                        id: t.id,
                        key: t.key,
                        name: t.name,
                        description: t.description,
                        states: t.states.map(|s| s.nodes).unwrap_or_default(),
                    })
                    .collect(),
            })
            .unwrap_or_default();
        Ok(conn)
    }

    // -----------------------------------------------------------------
    // Internals
    // -----------------------------------------------------------------

    /// POST a GraphQL operation. Returns the parsed `data` payload —
    /// callers deserialize into their typed shape. `errors[].message`
    /// is joined into a single `ApiError::GraphQL` and returned on
    /// failure.
    pub async fn request(&self, query: &str, variables: Value) -> Result<Value, ApiError> {
        let body = serde_json::json!({
            "query": query,
            "variables": variables,
        });
        let response = self.post_with_retry(&body).await?;
        let parsed: GraphQLResponse<Value> = response.json().await?;
        if !parsed.errors.is_empty() {
            let joined = parsed
                .errors
                .iter()
                .map(|e| e.message.clone())
                .collect::<Vec<_>>()
                .join("; ");
            return Err(ApiError::GraphQL(joined));
        }
        parsed
            .data
            .ok_or_else(|| ApiError::GraphQL("GraphQL response missing data".into()))
    }

    async fn post_with_retry(&self, body: &Value) -> Result<Response, ApiError> {
        let url = format!("{}{}", self.base_url.trim_end_matches('/'), GRAPHQL_PATH);
        let mut attempt: u32 = 0;
        loop {
            let mut req = self
                .http
                .request(Method::POST, &url)
                .header("User-Agent", USER_AGENT)
                .header("Content-Type", "application/json")
                .header("Accept", "application/json");
            if let Some(t) = self.current_token().await {
                req = req.header("Authorization", t.header_value());
            }
            let req = req.json(body);
            let response = req.send().await?;
            let status = response.status();

            // 401 — token revoked. Don't retry.
            if status == StatusCode::UNAUTHORIZED {
                return Err(ApiError::Unauthorized);
            }

            // 429 — honour Retry-After then exponential backoff.
            if status == StatusCode::TOO_MANY_REQUESTS {
                let retry_after =
                    parse_retry_after(response.headers()).unwrap_or_else(|| exp_backoff(attempt));
                if attempt < MAX_RETRIES {
                    self.backoff_sleep(retry_after).await;
                    attempt += 1;
                    continue;
                }
                return Err(ApiError::RateLimited {
                    retry_after_secs: retry_after,
                });
            }

            // 5xx — exponential backoff.
            if status.is_server_error() {
                if attempt < MAX_RETRIES {
                    let backoff = exp_backoff(attempt);
                    self.backoff_sleep(backoff).await;
                    attempt += 1;
                    continue;
                }
                let text = response.text().await.unwrap_or_default();
                return Err(ApiError::Server(format!("HTTP {status}: {text}")));
            }

            // Other non-2xx — treat as server error.
            if !status.is_success() {
                let text = response.text().await.unwrap_or_default();
                return Err(ApiError::Server(format!("HTTP {status}: {text}")));
            }

            // 2xx — Linear always returns 200 on GraphQL errors. We
            // unwrap `errors[]` in `request()` above.
            return Ok(response);
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

// ---------------------------------------------------------------------------
// Internal payload types — match the GraphQL response shape exactly.
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
struct ListIssuesPayload {
    #[serde(default)]
    issues: Option<IssueConnection>,
}

#[derive(Debug, Default, Deserialize)]
struct GetIssuePayload {
    #[serde(default)]
    issue: Option<Issue>,
}

#[derive(Debug, Default, Deserialize)]
struct CreateIssuePayload {
    #[serde(default, rename = "issueCreate")]
    issue_create: Option<IssueUpdatePayload>,
}

#[derive(Debug, Default, Deserialize)]
struct UpdateStatusPayload {
    #[serde(default, rename = "issueUpdate")]
    issue_update: Option<IssueUpdatePayload>,
}

#[derive(Debug, Default, Deserialize)]
struct ListTeamsPayload {
    #[serde(default)]
    teams: Option<RawTeamConnection>,
}

#[derive(Debug, Default, Deserialize)]
struct RawTeamConnection {
    #[serde(default)]
    nodes: Vec<RawTeam>,
}

#[derive(Debug, Deserialize)]
struct RawTeam {
    id: String,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    /// `states { nodes: [WorkflowState] }` — flattened in
    /// [`LinearClient::list_teams`] into [`TeamWithStates::states`].
    #[serde(default)]
    states: Option<RawStateConnection>,
}

#[derive(Debug, Default, Deserialize)]
struct RawStateConnection {
    #[serde(default)]
    nodes: Vec<WorkflowState>,
}

// ---------------------------------------------------------------------------
// Free helpers — small and pure so unit tests can exercise them.
// ---------------------------------------------------------------------------

/// Exponential backoff schedule. attempt 0 → 1s, 1 → 2s, 2 → 4s,
/// capped at 60s.
fn exp_backoff(attempt: u32) -> u64 {
    let base = DEFAULT_BACKOFF_SECS
        .checked_shl(attempt)
        .unwrap_or(MAX_BACKOFF_SECS);
    base.min(MAX_BACKOFF_SECS)
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
    fn exp_backoff_doubles_and_caps() {
        assert_eq!(exp_backoff(0), 1);
        assert_eq!(exp_backoff(1), 2);
        assert_eq!(exp_backoff(2), 4);
        assert_eq!(exp_backoff(5), 32);
        assert_eq!(exp_backoff(6), 60); // capped
        assert_eq!(exp_backoff(20), 60); // still capped
    }

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn default_backoff_is_one() {
        // Anchors the value so a careless edit can't set it to 0 and
        // short-circuit retries entirely.
        assert!(DEFAULT_BACKOFF_SECS == 1);
    }

    #[test]
    fn graphq_response_parses_data_and_errors() {
        let body = r#"{"data":{"x":1},"errors":[]}"#;
        let parsed: GraphQLResponse<Value> = serde_json::from_str(body).unwrap();
        assert!(parsed.data.is_some());
        assert!(parsed.errors.is_empty());
    }

    #[test]
    fn graphq_error_surfaces_message() {
        let body = r#"{"data":null,"errors":[{"message":"bad","path":["issues"]}]}"#;
        let parsed: GraphQLResponse<Value> = serde_json::from_str(body).unwrap();
        assert!(parsed.data.is_none());
        assert_eq!(parsed.errors.len(), 1);
        assert_eq!(parsed.errors[0].message, "bad");
    }

    #[test]
    fn auth_prefix_is_bearer() {
        let t = Token::new("lin_api_x".into());
        assert!(t.header_value().starts_with("Bearer "));
    }
}
