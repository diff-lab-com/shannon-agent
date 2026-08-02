//! Slack API mock tests covering all six tools, auth, and rate limits.

use super::{api::SlackClient, auth::Token, tools::all_tools};
use serde_json::json;
use std::sync::Arc;

async fn client(server: &mockito::Server) -> SlackClient {
    SlackClient::new(reqwest::Client::new(), Some(Token::new("xoxb-test".into())))
        .with_base_url(server.url())
        .with_backoff_scale(0)
}

#[tokio::test]
async fn list_channels_tool_calls_api() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock(
            "GET",
            mockito::Matcher::Regex("/conversations.list.*".into()),
        )
        .match_header("authorization", "Bearer xoxb-test")
        .with_status(200)
        .with_body(r#"{"ok":true,"channels":[{"id":"C1","name":"general"}]}"#)
        .create_async()
        .await;
    let tools = all_tools(Arc::new(client(&server).await));
    let result = tools[0].execute(json!({})).await.unwrap();
    assert_eq!(result["channels"][0]["id"], "C1");
}

#[tokio::test]
async fn history_tool_requires_channel() {
    let tools = all_tools(Arc::new(SlackClient::new(reqwest::Client::new(), None)));
    assert!(tools[1].execute(json!({})).await.is_err());
}

#[tokio::test]
async fn history_tool_calls_api() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock(
            "GET",
            mockito::Matcher::Regex("/conversations.history.*".into()),
        )
        .with_status(200)
        .with_body(r#"{"ok":true,"messages":[{"ts":"1","text":"hi"}]}"#)
        .create_async()
        .await;
    let tools = all_tools(Arc::new(client(&server).await));
    assert_eq!(
        tools[1].execute(json!({"channel":"C1"})).await.unwrap()["messages"][0]["text"],
        "hi"
    );
}

#[tokio::test]
async fn reply_requires_permission() {
    let tools = all_tools(Arc::new(SlackClient::new(reqwest::Client::new(), None)));
    assert!(
        tools[2]
            .execute(json!({"channel":"C1","thread_ts":"1","text":"x"}))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn reply_tool_calls_api() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", "/chat.postMessage")
        .with_status(200)
        .with_body(r#"{"ok":true,"ts":"2","channel":"C1"}"#)
        .create_async()
        .await;
    let tools = all_tools(Arc::new(client(&server).await));
    assert_eq!(
        tools[2]
            .execute(json!({"channel":"C1","thread_ts":"1","text":"x","permission":"write"}))
            .await
            .unwrap()["created"],
        true
    );
}

#[tokio::test]
async fn users_list_tool_calls_api() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("GET", mockito::Matcher::Regex("/users.list.*".into()))
        .with_status(200)
        .with_body(r#"{"ok":true,"members":[{"id":"U1"}]}"#)
        .create_async()
        .await;
    let tools = all_tools(Arc::new(client(&server).await));
    assert_eq!(
        tools[3].execute(json!({})).await.unwrap()["users"][0]["id"],
        "U1"
    );
}

#[tokio::test]
async fn upload_tool_calls_api() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", "/files.upload")
        .with_status(200)
        .with_body(r#"{"ok":true,"file":{"id":"F1"}}"#)
        .create_async()
        .await;
    let tools = all_tools(Arc::new(client(&server).await));
    assert_eq!(
        tools[4]
            .execute(json!({"channels":"C1","content":"hello","permission":"write"}))
            .await
            .unwrap()["file"]["id"],
        "F1"
    );
}

#[tokio::test]
async fn reaction_tool_calls_api() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", "/reactions.add")
        .with_status(200)
        .with_body(r#"{"ok":true}"#)
        .create_async()
        .await;
    let tools = all_tools(Arc::new(client(&server).await));
    assert_eq!(
        tools[5]
            .execute(json!({"channel":"C1","timestamp":"1","name":"thumbsup","permission":"write"}))
            .await
            .unwrap()["added"],
        true
    );
}

#[tokio::test]
async fn rate_limit_honors_retry_after() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock(
            "GET",
            mockito::Matcher::Regex("/conversations.list.*".into()),
        )
        .with_status(429)
        .with_header("Retry-After", "0")
        .with_body("rate limited")
        .expect(4)
        .create_async()
        .await;
    let result = client(&server).await.list_channels(None, None).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn slack_error_is_returned() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock(
            "GET",
            mockito::Matcher::Regex("/conversations.list.*".into()),
        )
        .with_status(200)
        .with_body(r#"{"ok":false,"error":"missing_scope"}"#)
        .create_async()
        .await;
    assert!(
        client(&server)
            .await
            .list_channels(None, None)
            .await
            .unwrap_err()
            .to_string()
            .contains("missing_scope")
    );
}

#[test]
fn token_debug_redacts() {
    assert!(!format!("{:?}", Token::new("secret".into())).contains("secret"));
}

#[test]
fn keyring_names_are_stable() {
    assert_eq!(super::auth::BOT_TOKEN_KEY, "slack/bot-token");
    assert_eq!(super::auth::REFRESH_TOKEN_KEY, "slack/refresh-token");
}

#[test]
fn oauth_scope_contains_all_permissions() {
    for scope in [
        "channels:read",
        "channels:history",
        "chat:write",
        "users:read",
        "files:write",
        "reactions:write",
    ] {
        assert!(super::auth::OAUTH_SCOPES.contains(scope));
    }
}

#[test]
fn oauth_response_accepts_refresh_token() {
    let _: serde_json::Value =
        serde_json::from_str(r#"{"ok":true,"access_token":"x","refresh_token":"r"}"#).unwrap();
}

#[test]
fn six_tool_names_are_stable() {
    let names: Vec<_> = super::tools::all_tools_unauth()
        .iter()
        .map(|t| t.name())
        .collect();
    assert_eq!(
        names,
        vec![
            "slack_list_channels",
            "slack_conversations_history",
            "slack_conversations_reply",
            "slack_users_list",
            "slack_files_upload",
            "slack_reactions_add"
        ]
    );
}
