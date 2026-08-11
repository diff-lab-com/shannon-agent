//! Notion API mock tests covering all six tools, auth, and rate limits.
//!
//! Mirrors `slack::tests` / `jira::tests` — happy paths plus the
//! 401/403/404/429/5xx edges. `with_backoff_scale(0)` makes every
//! retry immediate, so the test suite stays fast.

use super::api::NotionClient;
use super::auth::{ENV_TOKEN, KEYRING_SERVICE, TOKEN_KEY, Token};
use super::tools::all_tools;
use serde_json::json;
use std::sync::Arc;

async fn client(server: &mockito::Server) -> NotionClient {
    NotionClient::new(reqwest::Client::new(), Some(Token::new("ntn-test")))
        .with_base_url(server.url())
        .with_backoff_scale(0)
}

fn tools(client: NotionClient) -> Vec<Box<dyn super::tools::McpTool>> {
    all_tools(Arc::new(client))
}

// ---------------------------------------------------------------------------
// search_pages (read)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn search_pages_tool_calls_api() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", "/search")
        .match_body(mockito::Matcher::PartialJson(json!({"query":"deploy"})))
        .with_status(200)
        .with_body(
            r#"{"object":"list","results":[{"object":"page","id":"p1","properties":{}}],"has_more":false,"next_cursor":null}"#,
        )
        .create_async()
        .await;
    let tools = tools(client(&server).await);
    let result = tools[0].execute(json!({"query":"deploy"})).await.unwrap();
    assert_eq!(result["results"][0]["id"], "p1");
    assert_eq!(result["has_more"], false);
}

#[tokio::test]
async fn search_pages_filter_databases_uses_search_endpoint() {
    // list_databases wraps a search with the database filter — confirm
    // the request body actually contains the filter so the search
    // endpoint returns only databases, not pages.
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", "/search")
        .match_body(mockito::Matcher::PartialJson(json!({
            "filter": {"value": "database", "property": "object"}
        })))
        .with_status(200)
        .with_body(
            r#"{"object":"list","results":[{"object":"database","id":"d1","title":[]}],"has_more":false}"#,
        )
        .create_async()
        .await;
    let tools = tools(client(&server).await);
    let result = tools[4].execute(json!({"page_size": 10})).await.unwrap();
    assert_eq!(result["databases"][0]["id"], "d1");
}

// ---------------------------------------------------------------------------
// get_page (read)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_page_tool_calls_api() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("GET", "/pages/00000000-0000-0000-0000-000000000000")
        .with_status(200)
        .with_body(
            r#"{"object":"page","id":"00000000-0000-0000-0000-000000000000","properties":{"Name":{"title":[{"text":{"content":"Hello"}}]}}}"#,
        )
        .create_async()
        .await;
    let tools = tools(client(&server).await);
    let result = tools[1]
        .execute(json!({"page_id":"00000000-0000-0000-0000-000000000000"}))
        .await
        .unwrap();
    assert_eq!(result["page"]["id"], "00000000-0000-0000-0000-000000000000");
    assert_eq!(
        result["page"]["properties"]["Name"]["title"][0]["text"]["content"],
        "Hello"
    );
}

#[tokio::test]
async fn get_page_requires_page_id() {
    let tools = tools(NotionClient::new(reqwest::Client::new(), None));
    assert!(tools[1].execute(json!({})).await.is_err());
}

#[tokio::test]
async fn get_page_encodes_path_segment_to_block_injection() {
    // A naive `format!("/pages/{}", id)` would let `?x=y` leak into
    // the query string; the test confirms the slash and `?` get
    // percent-encoded so the request lands on the intended path.
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock(
            "GET",
            mockito::Matcher::Regex(r"^/pages/p1%3Fx%3Dy$".into()),
        )
        .with_status(200)
        .with_body(r#"{"object":"page","id":"p1","properties":{}}"#)
        .create_async()
        .await;
    let tools = tools(client(&server).await);
    let result = tools[1].execute(json!({"page_id":"p1?x=y"})).await.unwrap();
    assert_eq!(result["page"]["id"], "p1");
}

// ---------------------------------------------------------------------------
// append_block (write)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn append_block_tool_calls_api() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("PATCH", "/blocks/p1/children")
        .match_body(mockito::Matcher::PartialJson(json!({
            "children": [{"object":"block","type":"paragraph","paragraph":{}}]
        })))
        .with_status(200)
        .with_body(
            r#"{"object":"list","results":[{"object":"block","id":"b1","type":"paragraph","has_children":false}]}"#,
        )
        .create_async()
        .await;
    let tools = tools(client(&server).await);
    let result = tools[2]
        .execute(json!({
            "page_id": "p1",
            "block": { "object":"block", "type":"paragraph", "paragraph": {} }
        }))
        .await
        .unwrap();
    assert_eq!(result["appended"], true);
    assert_eq!(result["block"]["id"], "b1");
}

#[tokio::test]
async fn append_block_requires_page_id_and_block() {
    let tools = tools(NotionClient::new(reqwest::Client::new(), None));
    assert!(tools[2].execute(json!({"page_id":"p1"})).await.is_err());
    assert!(tools[2].execute(json!({"block":{}})).await.is_err());
}

// ---------------------------------------------------------------------------
// create_page (write)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_page_tool_calls_api() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", "/pages")
        .match_body(mockito::Matcher::PartialJson(json!({
            "parent": {"database_id": "d1"},
            "properties": {"Name": {"title": []}}
        })))
        .with_status(200)
        .with_body(r#"{"object":"page","id":"new-page","properties":{}}"#)
        .create_async()
        .await;
    let tools = tools(client(&server).await);
    let result = tools[3]
        .execute(json!({
            "parent":     {"database_id": "d1"},
            "properties": {"Name": {"title": []}}
        }))
        .await
        .unwrap();
    assert_eq!(result["created"], true);
    assert_eq!(result["page"]["id"], "new-page");
}

#[tokio::test]
async fn create_page_includes_children_when_supplied() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", "/pages")
        .match_body(mockito::Matcher::PartialJson(json!({
            "children": [{"object":"block","type":"paragraph","paragraph":{}}]
        })))
        .with_status(200)
        .with_body(r#"{"object":"page","id":"new-page","properties":{}}"#)
        .create_async()
        .await;
    let tools = tools(client(&server).await);
    let result = tools[3]
        .execute(json!({
            "parent":     {"page_id": "p-parent"},
            "properties": {"title": []},
            "children":   [{"object":"block","type":"paragraph","paragraph":{}}]
        }))
        .await
        .unwrap();
    assert_eq!(result["page"]["id"], "new-page");
}

#[tokio::test]
async fn create_page_requires_parent_and_properties() {
    let tools = tools(NotionClient::new(reqwest::Client::new(), None));
    assert!(tools[3].execute(json!({})).await.is_err());
    assert!(
        tools[3]
            .execute(json!({"parent":{"database_id":"d1"}}))
            .await
            .is_err()
    );
    assert!(tools[3].execute(json!({"properties":{}})).await.is_err());
}

// ---------------------------------------------------------------------------
// list_databases (read)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_databases_tool_calls_api() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", "/search")
        .match_body(mockito::Matcher::PartialJson(json!({
            "page_size": 50
        })))
        .with_status(200)
        .with_body(
            r#"{"object":"list","results":[{"object":"database","id":"d1","title":[]},{"object":"database","id":"d2","title":[]}],"has_more":false}"#,
        )
        .create_async()
        .await;
    let tools = tools(client(&server).await);
    let result = tools[4].execute(json!({"page_size": 50})).await.unwrap();
    assert_eq!(result["databases"][0]["id"], "d1");
    assert_eq!(result["databases"][1]["id"], "d2");
    assert_eq!(result["has_more"], false);
}

// ---------------------------------------------------------------------------
// query_database (read)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn query_database_tool_calls_api() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", "/databases/d1/query")
        .match_body(mockito::Matcher::PartialJson(json!({
            "page_size": 25
        })))
        .with_status(200)
        .with_body(
            r#"{"object":"list","results":[{"object":"page","id":"row1","properties":{}}],"has_more":true,"next_cursor":"abc"}"#,
        )
        .create_async()
        .await;
    let tools = tools(client(&server).await);
    let result = tools[5]
        .execute(json!({"database_id":"d1","page_size":25}))
        .await
        .unwrap();
    assert_eq!(result["rows"][0]["id"], "row1");
    assert_eq!(result["has_more"], true);
    assert_eq!(result["next_cursor"], "abc");
}

#[tokio::test]
async fn query_database_requires_database_id() {
    let tools = tools(NotionClient::new(reqwest::Client::new(), None));
    assert!(tools[5].execute(json!({})).await.is_err());
}

// ---------------------------------------------------------------------------
// Auth + error surface
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bearer_and_notion_version_headers_are_set() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", "/search")
        .match_header("authorization", "Bearer ntn-test")
        .match_header("Notion-Version", "2022-06-28")
        .with_status(200)
        .with_body(r#"{"object":"list","results":[],"has_more":false}"#)
        .expect(1)
        .create_async()
        .await;
    let tools = tools(client(&server).await);
    tools[0].execute(json!({"query":"x"})).await.unwrap();
}

#[tokio::test]
async fn unauthorized_does_not_retry() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", "/search")
        .with_status(401)
        .expect(1) // single try — no retries on 401
        .create_async()
        .await;
    let err = client(&server)
        .await
        .search_pages(Some("x"), None, None, None, None)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("401"), "got: {err}");
}

#[tokio::test]
async fn forbidden_is_surfaced_with_body() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", "/search")
        .with_status(403)
        .with_body(r#"{"object":"error","code":"unauthorized","message":"page not shared"}"#)
        .create_async()
        .await;
    let err = client(&server)
        .await
        .search_pages(Some("x"), None, None, None, None)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("403"), "got: {err}");
    assert!(err.contains("page not shared"), "got: {err}");
}

#[tokio::test]
async fn not_found_is_surfaced_with_path() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("GET", mockito::Matcher::Regex(r"^/pages/.*$".into()))
        .with_status(404)
        .create_async()
        .await;
    let err = client(&server)
        .await
        .get_page("00000000-0000-0000-0000-000000000000")
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("404"), "got: {err}");
}

#[tokio::test]
async fn rate_limit_honors_retry_after_then_surfaces_error() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", "/search")
        .with_status(429)
        .with_header("Retry-After", "0")
        .with_body("rate limited")
        .expect_at_least(1) // 1 initial + retries
        .create_async()
        .await;
    let result = client(&server)
        .await
        .search_pages(Some("x"), None, None, None, None)
        .await;
    assert!(result.is_err(), "expected rate-limit error after retries");
}

#[tokio::test]
async fn fivexx_surfaces_as_server_error() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", "/search")
        .with_status(500)
        .with_body("boom")
        .expect_at_least(1)
        .create_async()
        .await;
    let err = client(&server)
        .await
        .search_pages(Some("x"), None, None, None, None)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("500"), "got: {err}");
}

#[tokio::test]
async fn unauthenticated_call_without_token_surfaces() {
    // `all_tools_unauth` produces a client with no token, so the
    // outbound request has no Authorization header. The mock here
    // asserts that the call still goes out (and surfaces a 4xx from
    // the server rather than panicking), because the auth flow lives
    // outside the API client — `TokenProvider::get_token` is what
    // should be called first.
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", "/search")
        .match_header("authorization", mockito::Matcher::Missing)
        .with_status(401)
        .expect(1)
        .create_async()
        .await;
    let tools = all_tools(Arc::new(
        NotionClient::new(reqwest::Client::new(), None)
            .with_base_url(server.url())
            .with_backoff_scale(0),
    ));
    let err = tools[0]
        .execute(json!({"query":"x"}))
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("401"), "got: {err}");
}

// ---------------------------------------------------------------------------
// Pure-CPU guards
// ---------------------------------------------------------------------------

#[test]
fn token_debug_redacts() {
    assert!(!format!("{:?}", Token::new("ntn_super_secret")).contains("ntn_super_secret"));
}

#[test]
fn keyring_and_env_constants_are_stable() {
    assert_eq!(KEYRING_SERVICE, "shannon-mcp-saas");
    assert_eq!(TOKEN_KEY, "notion-token");
    assert_eq!(ENV_TOKEN, "NOTION_TOKEN");
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
            "notion_search_pages",
            "notion_get_page",
            "notion_append_block",
            "notion_create_page",
            "notion_list_databases",
            "notion_query_database",
        ]
    );
}
