//! Jira API mock tests covering all four tools, auth, and rate limits.

use super::api::JiraClient;
use super::auth::{API_TOKEN_KEY, OAUTH_SCOPES, OAUTH_TOKEN_KEY, Token};
use super::tools::all_tools;
use base64::Engine;
use serde_json::json;
use std::sync::Arc;

fn basic_credential() -> crate::jira::auth::CredentialKind {
    crate::jira::auth::CredentialKind::ApiToken {
        email: "alice@example.com".into(),
        token: "apitoken".into(),
    }
}

async fn client(server: &mockito::Server) -> JiraClient {
    JiraClient::new(reqwest::Client::new(), Some(basic_credential()))
        .with_base_url(server.url())
        .with_backoff_scale(0)
}

#[tokio::test]
async fn search_issues_calls_api() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("GET", mockito::Matcher::Regex("/search.*".into()))
        .match_header("authorization", mockito::Matcher::Regex("Basic .*".into()))
        .with_status(200)
        .with_body(r#"{"issues":[{"id":"1","key":"ENG-1"}],"startAt":0,"maxResults":50,"total":1}"#)
        .create_async()
        .await;
    let tools = all_tools(Arc::new(client(&server).await));
    let result = tools[0]
        .execute(json!({"jql":"project = ENG"}))
        .await
        .unwrap();
    assert_eq!(result["issues"][0]["key"], "ENG-1");
}

#[tokio::test]
async fn search_requires_jql() {
    let tools = all_tools(Arc::new(JiraClient::new(reqwest::Client::new(), None)));
    assert!(tools[0].execute(json!({})).await.is_err());
}

#[tokio::test]
async fn get_issue_calls_api() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock(
            "GET",
            mockito::Matcher::Regex(r"^/rest/api/3/issue/ENG-1$".into()),
        )
        .with_status(200)
        .with_body(
            r#"{"id":"1","key":"ENG-1","fields":{"summary":"Hello","status":{"name":"Open"}}}"#,
        )
        .create_async()
        .await;
    let tools = all_tools(Arc::new(client(&server).await));
    let result = tools[1].execute(json!({"key":"ENG-1"})).await.unwrap();
    assert_eq!(result["issue"]["key"], "ENG-1");
    assert_eq!(result["issue"]["fields"]["summary"], "Hello");
}

#[tokio::test]
async fn get_issue_requires_key() {
    let tools = all_tools(Arc::new(JiraClient::new(reqwest::Client::new(), None)));
    assert!(tools[1].execute(json!({})).await.is_err());
}

#[tokio::test]
async fn create_issue_calls_api() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock(
            "POST",
            mockito::Matcher::Regex(r"^/rest/api/3/issue$".into()),
        )
        .with_status(201)
        .with_body(r#"{"id":"10000","key":"ENG-42"}"#)
        .create_async()
        .await;
    let tools = all_tools(Arc::new(client(&server).await));
    let result = tools[2]
        .execute(json!({
            "project":"ENG",
            "summary":"My first ticket",
            "issue_type":"Task",
            "description":"Body text",
            "permission":"write"
        }))
        .await
        .unwrap();
    assert_eq!(result["issue"]["key"], "ENG-42");
    assert_eq!(result["created"], true);
}

#[tokio::test]
async fn create_issue_validates_required_fields() {
    let tools = all_tools(Arc::new(JiraClient::new(reqwest::Client::new(), None)));
    // missing `project`
    assert!(
        tools[2]
            .execute(json!({"summary":"x","issue_type":"Task"}))
            .await
            .is_err()
    );
    // missing `summary`
    assert!(
        tools[2]
            .execute(json!({"project":"ENG","issue_type":"Task"}))
            .await
            .is_err()
    );
    // missing `issue_type`
    assert!(
        tools[2]
            .execute(json!({"project":"ENG","summary":"x"}))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn transition_resolves_status_name_to_id() {
    let mut server = mockito::Server::new_async().await;
    let _list = server
        .mock(
            "GET",
            mockito::Matcher::Regex(r"^/rest/api/3/issue/ENG-1/transitions$".into()),
        )
        .with_status(200)
        .with_body(
            r#"{"transitions":[{"id":"11","name":"To Do"},{"id":"21","name":"In Progress"},{"id":"31","name":"Done"}]}"#,
        )
        .expect(1)
        .create_async()
        .await;
    let _post = server
        .mock(
            "POST",
            mockito::Matcher::Regex(r"^/rest/api/3/issue/ENG-1/transitions$".into()),
        )
        .match_body(mockito::Matcher::Regex(r#""id":"21""#.into()))
        .with_status(204)
        .expect(1)
        .create_async()
        .await;
    let tools = all_tools(Arc::new(client(&server).await));
    let result = tools[3]
        .execute(json!({"key":"ENG-1","target_status":"In Progress","permission":"write"}))
        .await
        .unwrap();
    assert_eq!(result["transitioned"], true);
    assert_eq!(result["issue"]["key"], "ENG-1");
}

#[tokio::test]
async fn transition_rejects_unknown_status() {
    let mut server = mockito::Server::new_async().await;
    let _list = server
        .mock(
            "GET",
            mockito::Matcher::Regex(r"^/rest/api/3/issue/ENG-1/transitions$".into()),
        )
        .with_status(200)
        .with_body(r#"{"transitions":[{"id":"11","name":"To Do"}]}"#)
        .expect(1)
        .create_async()
        .await;
    let tools = all_tools(Arc::new(client(&server).await));
    let err = tools[3]
        .execute(json!({"key":"ENG-1","target_status":"Hovering","permission":"write"}))
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("Hovering"), "err should mention status: {err}");
}

#[tokio::test]
async fn rate_limit_honors_retry_after_on_429() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("GET", mockito::Matcher::Regex("/search.*".into()))
        .with_status(429)
        .with_header("Retry-After", "0")
        .with_body("rate limited")
        .expect_at_least(1)
        .create_async()
        .await;
    let result = client(&server)
        .await
        .search_issues("project = ENG", None, None)
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn x_ratelimit_remaining_zero_with_429_is_handled() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("GET", mockito::Matcher::Regex("/search.*".into()))
        .with_status(403)
        .with_header("X-RateLimit-Remaining", "0")
        .with_header("Retry-After", "0")
        .with_body("rate-limited via header")
        .expect_at_least(1)
        .create_async()
        .await;
    let result = client(&server)
        .await
        .search_issues("project = ENG", None, None)
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn api_token_basic_auth_header_is_set() {
    let mut server = mockito::Server::new_async().await;
    // The email:token "alice@example.com:apitoken" base64-encodes to a
    // stable value; assert the header matches so the test fails loudly
    // if the auth wiring changes.
    let expected = base64::engine::general_purpose::STANDARD.encode("alice@example.com:apitoken");
    let _m = server
        .mock("GET", mockito::Matcher::Regex("/search.*".into()))
        .match_header("authorization", format!("Basic {expected}").as_str())
        .with_status(200)
        .with_body(r#"{"issues":[]}"#)
        .expect(1)
        .create_async()
        .await;
    let result = client(&server)
        .await
        .search_issues("project = ENG", None, None)
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn oauth_bearer_header_is_set() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("GET", mockito::Matcher::Regex("/search.*".into()))
        .match_header("authorization", "Bearer oauth-tok")
        .with_status(200)
        .with_body(r#"{"issues":[]}"#)
        .expect(1)
        .create_async()
        .await;
    let client = JiraClient::new(
        reqwest::Client::new(),
        Some(crate::jira::auth::CredentialKind::OAuth {
            access_token: "oauth-tok".into(),
            cloudid: None,
        }),
    )
    .with_base_url(server.url())
    .with_backoff_scale(0);
    let result = client.search_issues("project = ENG", None, None).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn not_found_maps_to_clear_error() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock(
            "GET",
            mockito::Matcher::Regex(r"^/rest/api/3/issue/ENG-404$".into()),
        )
        .with_status(404)
        .create_async()
        .await;
    let tools = all_tools(Arc::new(client(&server).await));
    let err = tools[1]
        .execute(json!({"key":"ENG-404"}))
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("404"), "got: {err}");
}

#[tokio::test]
async fn unauthorized_does_not_retry() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("GET", mockito::Matcher::Regex("/search.*".into()))
        .with_status(401)
        .expect(1)
        .create_async()
        .await;
    let err = client(&server)
        .await
        .search_issues("project = ENG", None, None)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("401"), "err: {err}");
}

#[test]
fn token_debug_redacts() {
    assert!(!format!("{:?}", Token::new("secret".into())).contains("secret"));
}

#[test]
fn keyring_names_are_stable() {
    assert_eq!(API_TOKEN_KEY, "jira/api-token");
    assert_eq!(OAUTH_TOKEN_KEY, "jira/oauth");
}

#[test]
fn oauth_scope_contains_all_permissions() {
    for scope in [
        "read:jira-work",
        "read:jira-user",
        "write:jira-work",
        "offline_access",
    ] {
        assert!(OAUTH_SCOPES.contains(scope), "missing scope {scope}");
    }
}

#[test]
fn four_tool_names_are_stable() {
    let names: Vec<_> = super::tools::all_tools_unauth()
        .iter()
        .map(|t| t.name())
        .collect();
    assert_eq!(
        names,
        vec![
            "jira_search_issues",
            "jira_get_issue",
            "jira_create_issue",
            "jira_transition",
        ]
    );
}
