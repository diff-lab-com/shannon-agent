//! Slack API mock tests covering all six tools, auth, and rate limits.
//!
//! Mirrors `jira::tests` — happy paths plus the 401/404/429/5xx
//! edges. `with_backoff_scale(0)` makes every retry immediate, so the
//! test suite stays fast.

use super::api::SlackClient;
use super::auth::{BOT_TOKEN_KEY, OAUTH_SCOPES, REFRESH_TOKEN_KEY, Token};
use super::tools::all_tools;
use serde_json::json;
use std::sync::Arc;

async fn client(server: &mockito::Server) -> SlackClient {
    SlackClient::new(reqwest::Client::new(), Some(Token::new("xoxb-test".into())))
        .with_base_url(server.url())
        .with_backoff_scale(0)
}

fn tools(client: SlackClient) -> Vec<Box<dyn super::tools::McpTool>> {
    all_tools(Arc::new(client))
}

// ---------------------------------------------------------------------------
// post_message (write)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn post_message_tool_calls_api() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", "/chat.postMessage")
        .match_query(mockito::Matcher::Missing)
        .with_status(200)
        .with_body(r#"{"ok":true,"channel":"C1","ts":"1700000000.000100"}"#)
        .create_async()
        .await;
    let tools = tools(client(&server).await);
    let result = tools[0]
        .execute(json!({"channel":"C1","text":"hello"}))
        .await
        .unwrap();
    assert_eq!(result["posted"], true);
    assert_eq!(result["message"]["channel"], "C1");
}

#[tokio::test]
async fn post_message_requires_channel_and_text() {
    let tools = tools(SlackClient::new(reqwest::Client::new(), None));
    // missing channel
    assert!(tools[0].execute(json!({"text":"hi"})).await.is_err());
    // missing text
    assert!(tools[0].execute(json!({"channel":"C1"})).await.is_err());
}

#[tokio::test]
async fn post_message_surfaces_slack_error() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", "/chat.postMessage")
        .match_body(mockito::Matcher::AnyOf(vec![
            mockito::Matcher::Regex("channel=missing".into()),
            mockito::Matcher::Regex("text=x".into()),
        ]))
        .with_status(200)
        .with_body(r#"{"ok":false,"error":"channel_not_found"}"#)
        .create_async()
        .await;
    let err = tools(client(&server).await)[0]
        .execute(json!({"channel":"missing","text":"x"}))
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("channel_not_found"), "got: {err}");
}

// ---------------------------------------------------------------------------
// search_messages (read)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn search_messages_tool_calls_api() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock(
            "GET",
            mockito::Matcher::Regex("/search.messages.*".into()),
        )
        .match_query(mockito::Matcher::Regex("query=deploy".into()))
        .with_status(200)
        .with_body(
            r#"{"ok":true,"messages":{"total":1,"matches":[{"ts":"1","text":"deploy failed","channel":{"id":"C1","name":"eng"}}]}}"#,
        )
        .create_async()
        .await;
    let tools = tools(client(&server).await);
    let result = tools[1].execute(json!({"query":"deploy"})).await.unwrap();
    assert_eq!(result["total"], 1);
    assert_eq!(result["matches"][0]["text"], "deploy failed");
}

#[tokio::test]
async fn search_messages_requires_query() {
    let tools = tools(SlackClient::new(reqwest::Client::new(), None));
    assert!(tools[1].execute(json!({})).await.is_err());
}

// ---------------------------------------------------------------------------
// read_channel (read)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn read_channel_tool_calls_api() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock(
            "GET",
            mockito::Matcher::Regex("/conversations.history.*".into()),
        )
        .match_query(mockito::Matcher::Regex("channel=C1".into()))
        .with_status(200)
        .with_body(r#"{"ok":true,"messages":[{"ts":"1","text":"hi"}]}"#)
        .create_async()
        .await;
    let tools = tools(client(&server).await);
    let result = tools[2]
        .execute(json!({"channel":"C1","limit":10}))
        .await
        .unwrap();
    assert_eq!(result["messages"][0]["text"], "hi");
}

#[tokio::test]
async fn read_channel_requires_channel() {
    let tools = tools(SlackClient::new(reqwest::Client::new(), None));
    assert!(tools[2].execute(json!({})).await.is_err());
}

// ---------------------------------------------------------------------------
// thread_reply (write)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn thread_reply_tool_calls_api() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", "/chat.postMessage")
        .match_body(mockito::Matcher::Regex("thread_ts=1700.*".into()))
        .with_status(200)
        .with_body(r#"{"ok":true,"ts":"1700000001.000200","channel":"C1"}"#)
        .create_async()
        .await;
    let tools = tools(client(&server).await);
    let result = tools[3]
        .execute(json!({"channel":"C1","thread_ts":"1700000000.000100","text":"reply"}))
        .await
        .unwrap();
    assert_eq!(result["posted"], true);
    assert_eq!(result["message"]["channel"], "C1");
}

#[tokio::test]
async fn thread_reply_requires_channel_thread_ts_text() {
    let tools = tools(SlackClient::new(reqwest::Client::new(), None));
    assert!(
        tools[3]
            .execute(json!({"channel":"C1","text":"x"}))
            .await
            .is_err()
    );
    assert!(
        tools[3]
            .execute(json!({"thread_ts":"1","text":"x"}))
            .await
            .is_err()
    );
    assert!(
        tools[3]
            .execute(json!({"channel":"C1","thread_ts":"1"}))
            .await
            .is_err()
    );
}

// ---------------------------------------------------------------------------
// list_channels (read)
// ---------------------------------------------------------------------------

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
    let tools = tools(client(&server).await);
    let result = tools[4].execute(json!({})).await.unwrap();
    assert_eq!(result["channels"][0]["id"], "C1");
    assert_eq!(result["channels"][0]["name"], "general");
}

#[tokio::test]
async fn list_channels_takes_no_required_args() {
    let tools = tools(SlackClient::new(reqwest::Client::new(), None));
    // No args — should not error on arg parsing; only on missing token.
    let res = tools[4].execute(json!({})).await;
    assert!(res.is_err(), "got: {res:?}");
    // Make sure the error is about missing auth, not a bad schema.
    assert!(
        !res.unwrap_err().to_string().contains("missing required"),
        "err should not be about missing args; got an arg-shaped error"
    );
}

// ---------------------------------------------------------------------------
// get_user_info (read)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_user_info_tool_calls_api() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("GET", mockito::Matcher::Regex("/users.info.*".into()))
        .match_query(mockito::Matcher::Regex("user=U1".into()))
        .with_status(200)
        .with_body(r#"{"ok":true,"user":{"id":"U1","name":"alice","real_name":"Alice A"}}"#)
        .create_async()
        .await;
    let tools = tools(client(&server).await);
    let result = tools[5].execute(json!({"user_id":"U1"})).await.unwrap();
    assert_eq!(result["user"]["id"], "U1");
    assert_eq!(result["user"]["name"], "alice");
}

#[tokio::test]
async fn get_user_info_requires_user_id() {
    let tools = tools(SlackClient::new(reqwest::Client::new(), None));
    assert!(tools[5].execute(json!({})).await.is_err());
}

// ---------------------------------------------------------------------------
// Auth + error surface
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bearer_header_is_set_on_post() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", "/chat.postMessage")
        .match_header("authorization", "Bearer xoxb-test")
        .with_status(200)
        .with_body(r#"{"ok":true,"ts":"1","channel":"C1"}"#)
        .expect(1)
        .create_async()
        .await;
    let tools = tools(client(&server).await);
    assert!(
        tools[0]
            .execute(json!({"channel":"C1","text":"x"}))
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn rate_limit_honors_retry_after_then_surfaces_error() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock(
            "GET",
            mockito::Matcher::Regex("/conversations.list.*".into()),
        )
        .with_status(429)
        .with_header("Retry-After", "0")
        .with_body("rate limited")
        .expect(4) // 1 initial attempt + 3 retries
        .create_async()
        .await;
    let client = SlackClient::new(reqwest::Client::new(), Some(Token::new("xoxb-test".into())))
        .with_base_url(server.url())
        .with_backoff_scale(0);
    let result = client.list_channels(None, None).await;
    assert!(result.is_err(), "expected rate-limit error after retries");
}

#[tokio::test]
async fn unauthorized_does_not_retry() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock(
            "GET",
            mockito::Matcher::Regex("/conversations.list.*".into()),
        )
        .with_status(401)
        .expect(1) // single try — no retries on 401
        .create_async()
        .await;
    let client = SlackClient::new(reqwest::Client::new(), Some(Token::new("xoxb-test".into())))
        .with_base_url(server.url())
        .with_backoff_scale(0);
    let err = client
        .list_channels(None, None)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("401"), "got: {err}");
}

#[tokio::test]
async fn fivexx_surfaces_as_server_error() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock(
            "GET",
            mockito::Matcher::Regex("/conversations.list.*".into()),
        )
        .with_status(500)
        .with_body("boom")
        .expect_at_least(1)
        .create_async()
        .await;
    let client = SlackClient::new(reqwest::Client::new(), Some(Token::new("xoxb-test".into())))
        .with_base_url(server.url())
        .with_backoff_scale(0);
    let err = client
        .list_channels(None, None)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("500"), "got: {err}");
}

// ---------------------------------------------------------------------------
// Pure-CPU guards (auth + keyring surface)
// ---------------------------------------------------------------------------

#[test]
fn token_debug_redacts() {
    assert!(!format!("{:?}", Token::new("secret".into())).contains("secret"));
}

#[test]
fn keyring_names_are_stable() {
    assert_eq!(BOT_TOKEN_KEY, "slack/bot-token");
    assert_eq!(REFRESH_TOKEN_KEY, "slack/refresh-token");
}

#[test]
fn oauth_scope_contains_required_scopes() {
    for scope in [
        "channels:read",
        "channels:history",
        "chat:write",
        "users:read",
    ] {
        assert!(OAUTH_SCOPES.contains(scope), "missing scope {scope}");
    }
}

#[test]
fn six_tool_names_match_required_set() {
    let names: Vec<_> = super::tools::all_tools_unauth()
        .iter()
        .map(|t| t.name())
        .collect();
    assert_eq!(
        names,
        vec![
            "slack_post_message",
            "slack_search_messages",
            "slack_read_channel",
            "slack_thread_reply",
            "slack_list_channels",
            "slack_get_user_info",
        ]
    );
}
