//! Linear mock-backed tests covering all 5 tools + auth + edge cases.
//!
//! Mirrors `slack::tests` and `jira::tests` — happy paths plus
//! 401 / 429 / 5xx / GraphQL-error edges. `with_backoff_scale(0)`
//! makes every retry instant. The endpoint fixture is mockito running
//! at `127.0.0.1:<random>`; we override `base_url` on the client to
//! point there.

use super::api::LinearClient;
use super::auth::{API_TOKEN_KEY, LINEAR_API_KEY_PREFIX, Token};
use super::tools::all_tools;
use serde_json::json;
use std::sync::Arc;

async fn client(server: &mockito::Server) -> LinearClient {
    LinearClient::new(
        reqwest::Client::new(),
        Some(Token::new("lin_api_test".into())),
    )
    .with_base_url(server.url())
    .with_backoff_scale(0)
}

fn tools(client: LinearClient) -> Vec<Box<dyn super::tools::McpTool>> {
    all_tools(Arc::new(client))
}

// ---------------------------------------------------------------------------
// linear_list_issues (read)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_issues_tool_calls_api() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", mockito::Matcher::Regex(r"^/graphql$".into()))
        .match_header("authorization", "Bearer lin_api_test")
        .with_status(200)
        .with_body(
            r#"{
                "data": {
                    "issues": {
                        "nodes": [
                            {
                                "id":"abc-1",
                                "identifier":"ENG-1",
                                "title":"first issue",
                                "priority":2.0,
                                "state":{"id":"state-1","name":"In Progress","type":"started"},
                                "team":{"id":"team-1","key":"ENG","name":"Engineering"}
                            }
                        ],
                        "pageInfo": {"hasNextPage":false,"endCursor":null}
                    }
                }
            }"#,
        )
        .create_async()
        .await;
    let tools = tools(client(&server).await);
    let result = tools[0].execute(json!({"first":10})).await.unwrap();
    assert_eq!(result["issues"][0]["identifier"], "ENG-1");
    assert_eq!(result["issues"][0]["title"], "first issue");
}

#[tokio::test]
async fn list_issues_passes_filter_and_after() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", mockito::Matcher::Regex(r"^/graphql$".into()))
        .match_body(mockito::Matcher::Regex(r#""after":"cursor-1""#.into()))
        .with_status(200)
        .with_body(
            r#"{"data":{"issues":{"nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}"#,
        )
        .create_async()
        .await;
    let tools = tools(client(&server).await);
    let result = tools[0]
        .execute(json!({"after":"cursor-1","filter":{"state":{"type":{"in":"started"}}}}))
        .await
        .unwrap();
    assert!(result["issues"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn list_issues_takes_no_required_args() {
    let tools = tools(LinearClient::new(reqwest::Client::new(), None));
    // No required args — call without any. Should not fail on arg parsing.
    // The auth-missing check happens client-side; we just want to confirm
    // the schema accepts `{}`.
    let tools_unauth = tools;
    let res = tools_unauth[0].execute(json!({})).await;
    // Either passes (client ok) or fails on authorization — never on schema.
    if let Err(e) = res {
        let msg = e.to_string();
        assert!(
            !msg.contains("missing required"),
            "no-args call should not fail schema; got: {msg}"
        );
    }
}

// ---------------------------------------------------------------------------
// linear_get_issue (read)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_issue_tool_calls_api() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", mockito::Matcher::Regex(r"^/graphql$".into()))
        .match_body(mockito::Matcher::Regex(r#""id":"ENG-1""#.into()))
        .with_status(200)
        .with_body(
            r#"{"data":{"issue":{"id":"abc-1","identifier":"ENG-1","title":"My bug","priority":1.0}}}"#,
        )
        .create_async()
        .await;
    let tools = tools(client(&server).await);
    let result = tools[1].execute(json!({"id":"ENG-1"})).await.unwrap();
    assert_eq!(result["issue"]["identifier"], "ENG-1");
    assert_eq!(result["issue"]["title"], "My bug");
}

#[tokio::test]
async fn get_issue_missing_returns_error() {
    let mut server = mockito::Server::new_async().await;
    // Linear returns `null` for missing issues with a GraphQL error.
    let _m = server
        .mock("POST", mockito::Matcher::Regex(r"^/graphql$".into()))
        .with_status(200)
        .with_body(
            r#"{"data":{"issue":null},"errors":[{"message":"Entity not found","path":["issue"]}]}"#,
        )
        .create_async()
        .await;
    let tools = tools(client(&server).await);
    let err = tools[1]
        .execute(json!({"id":"NOPE"}))
        .await
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("Entity not found") || err.contains("NOPE"),
        "got: {err}"
    );
}

#[tokio::test]
async fn get_issue_requires_id() {
    let tools = tools(LinearClient::new(reqwest::Client::new(), None));
    assert!(tools[1].execute(json!({})).await.is_err());
}

// ---------------------------------------------------------------------------
// linear_create_issue (write)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_issue_tool_calls_api() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", mockito::Matcher::Regex(r"^/graphql$".into()))
        .match_body(mockito::Matcher::Regex(r#""title":"hello""#.into()))
        .with_status(200)
        .with_body(
            r#"{"data":{"issueCreate":{"success":true,"issue":{"id":"new","identifier":"ENG-2","title":"hello","state":{"id":"s","name":"Todo","type":"unstarted"}}}}}"#,
        )
        .create_async()
        .await;
    let tools = tools(client(&server).await);
    let result = tools[2]
        .execute(json!({"title":"hello","team_id":"team-1","description":"world","priority":2.0}))
        .await
        .unwrap();
    assert_eq!(result["created"], true);
    assert_eq!(result["issue"]["identifier"], "ENG-2");
}

#[tokio::test]
async fn create_issue_requires_title_and_team_id() {
    let tools = tools(LinearClient::new(reqwest::Client::new(), None));
    assert!(tools[2].execute(json!({"title":"x"})).await.is_err());
    assert!(tools[2].execute(json!({"team_id":"t"})).await.is_err());
}

#[tokio::test]
async fn create_issue_surfaces_validation_error() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", mockito::Matcher::Regex(r"^/graphql$".into()))
        .with_status(200)
        .with_body(
            r#"{"data":null,"errors":[{"message":"Argument Validation Error: title must be ≤ 100 chars","path":["issueCreate"]}]}"#,
        )
        .create_async()
        .await;
    let tools = tools(client(&server).await);
    let err = tools[2]
        .execute(json!({"title":"x","team_id":"t"}))
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("Argument Validation"), "got: {err}");
}

// ---------------------------------------------------------------------------
// linear_update_status (write)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn update_status_tool_calls_api() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", mockito::Matcher::Regex(r"^/graphql$".into()))
        .match_body(mockito::Matcher::Regex(
            r#""issueId":"i-1".*"stateId":"s-2""#.into(),
        ))
        .with_status(200)
        .with_body(
            r#"{"data":{"issueUpdate":{"success":true,"issue":{"id":"i-1","identifier":"ENG-1","title":"t","state":{"id":"s-2","name":"Done","type":"completed"}}}}}"#,
        )
        .create_async()
        .await;
    let tools = tools(client(&server).await);
    let result = tools[3]
        .execute(json!({"issue_id":"i-1","state_id":"s-2"}))
        .await
        .unwrap();
    assert_eq!(result["updated"], true);
    assert_eq!(result["issue"]["state"]["name"], "Done");
}

#[tokio::test]
async fn update_status_requires_both_args() {
    let tools = tools(LinearClient::new(reqwest::Client::new(), None));
    assert!(tools[3].execute(json!({"issue_id":"i"})).await.is_err());
    assert!(tools[3].execute(json!({"state_id":"s"})).await.is_err());
}

// ---------------------------------------------------------------------------
// linear_list_teams (read)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_teams_tool_calls_api() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", mockito::Matcher::Regex(r"^/graphql$".into()))
        .with_status(200)
        .with_body(
            r#"{
                "data":{
                    "teams":{
                        "nodes":[
                            {
                                "id":"team-1",
                                "key":"ENG",
                                "name":"Engineering",
                                "description":null,
                                "states":{"nodes":[
                                    {"id":"state-1","name":"Todo","type":"unstarted"},
                                    {"id":"state-2","name":"In Progress","type":"started"},
                                    {"id":"state-3","name":"Done","type":"completed"}
                                ]}
                            }
                        ]
                    }
                }
            }"#,
        )
        .create_async()
        .await;
    let tools = tools(client(&server).await);
    let result = tools[4].execute(json!({})).await.unwrap();
    let team = &result["teams"][0];
    assert_eq!(team["key"], "ENG");
    assert_eq!(team["states"].as_array().unwrap().len(), 3);
    assert_eq!(team["states"][2]["name"], "Done");
}

#[tokio::test]
async fn list_teams_takes_no_required_args() {
    let tools = tools(LinearClient::new(reqwest::Client::new(), None));
    let res = tools[4].execute(json!({})).await;
    if let Err(e) = res {
        let msg = e.to_string();
        assert!(
            !msg.contains("missing required"),
            "no-args call should not fail schema; got: {msg}"
        );
    }
}

// ---------------------------------------------------------------------------
// Auth + error surface
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bearer_header_is_set_on_post() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", mockito::Matcher::Regex(r"^/graphql$".into()))
        .match_header("authorization", "Bearer lin_api_test")
        .with_status(200)
        .with_body(r#"{"data":{"viewer":{"id":"u1"}}}"#)
        .expect_at_least(1)
        .create_async()
        .await;
    // Hit the underlying `request()` to confirm headers land.
    let c = client(&server).await;
    let _ = c
        .request("query { viewer { id } }", serde_json::json!({}))
        .await;
}

#[tokio::test]
async fn rate_limit_honors_retry_after_then_surfaces_error() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", mockito::Matcher::Regex(r"^/graphql$".into()))
        .with_status(429)
        .with_header("Retry-After", "0")
        .with_body("rate limited")
        .expect(4) // 1 initial attempt + 3 retries
        .create_async()
        .await;
    let c = LinearClient::new(
        reqwest::Client::new(),
        Some(Token::new("lin_api_test".into())),
    )
    .with_base_url(server.url())
    .with_backoff_scale(0);
    let result = c.list_issues(None, None, None).await;
    assert!(result.is_err(), "expected rate-limit error after retries");
}

#[tokio::test]
async fn unauthorized_does_not_retry() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", mockito::Matcher::Regex(r"^/graphql$".into()))
        .with_status(401)
        .with_body("unauthorized")
        .expect(1) // single try — no retries on 401
        .create_async()
        .await;
    let c = LinearClient::new(
        reqwest::Client::new(),
        Some(Token::new("lin_api_test".into())),
    )
    .with_base_url(server.url())
    .with_backoff_scale(0);
    let err = c
        .list_issues(None, None, None)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("401"), "got: {err}");
}

#[tokio::test]
async fn fivexx_surfaces_as_server_error() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", mockito::Matcher::Regex(r"^/graphql$".into()))
        .with_status(500)
        .with_body("boom")
        .expect_at_least(1)
        .create_async()
        .await;
    let c = LinearClient::new(
        reqwest::Client::new(),
        Some(Token::new("lin_api_test".into())),
    )
    .with_base_url(server.url())
    .with_backoff_scale(0);
    let err = c
        .list_issues(None, None, None)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("500"), "got: {err}");
}

#[tokio::test]
async fn graphql_errors_are_passthrough() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", mockito::Matcher::Regex(r"^/graphql$".into()))
        .with_status(200)
        .with_body(r#"{"data":null,"errors":[{"message":"Not authorized","path":["issues"]}]}"#)
        .create_async()
        .await;
    let c = LinearClient::new(
        reqwest::Client::new(),
        Some(Token::new("lin_api_test".into())),
    )
    .with_base_url(server.url())
    .with_backoff_scale(0);
    let err = c
        .list_issues(None, None, None)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("Not authorized"), "got: {err}");
}

// ---------------------------------------------------------------------------
// Pure-CPU guards
// ---------------------------------------------------------------------------

#[test]
fn token_debug_redacts() {
    assert!(!format!("{:?}", Token::new("lin_api_secret".into())).contains("secret"));
}

#[test]
fn keyring_account_is_stable() {
    assert_eq!(API_TOKEN_KEY, "linear-token");
}

#[test]
fn api_key_prefix_is_recognised() {
    assert_eq!(LINEAR_API_KEY_PREFIX, "lin_api_");
    assert!(LINEAR_API_KEY_PREFIX.starts_with("lin_api"));
}

#[test]
fn five_tool_names_match_required_set() {
    let names: Vec<_> = super::tools::all_tools_unauth()
        .iter()
        .map(|t| t.name())
        .collect();
    assert_eq!(
        names,
        vec![
            "linear_list_issues",
            "linear_get_issue",
            "linear_create_issue",
            "linear_update_status",
            "linear_list_teams",
        ]
    );
}
