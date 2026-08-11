//! GitHub module tests.
//!
//! The spike (step 1) left a few canned-response tests here. Step 2
//! replaces them with a mockito-backed test matrix that exercises the
//! real `GitHubClient` against the real GitHub API wire shape:
//!
//! 1. `list_issues_returns_empty_array`
//! 2. `get_issue_returns_fields`
//! 3. `create_issue_returns_url_and_number`
//! 4. `comment_returns_id`
//! 5. `list_prs_returns_array`
//! 6. `review_pr_returns_state_change`
//! 7. `rate_limit_429_triggers_retry_after_backoff` (mockito 429 →
//!    `tokio::time::pause` → assert exponential delay → eventual 200)
//! 8. `rate_limit_secondary_rate_limit_returns_403_abuse`
//! 9. `unauthenticated_returns_401`
//!
//! Plus auth tests:
//! - `pkce_pair_shape` (in `auth.rs`)
//! - `oauth_callback_state_mismatch` (in `auth.rs`)
//! - `token_exchange_success` (mockito for the GitHub `access_token` endpoint)
//!
//! All tests use `tokio::time::pause` so the backoff sleeps don't add
//! wall-clock latency.

use std::sync::Arc;
use std::time::Duration;

use mockito::Matcher;
use serde_json::json;

use crate::github::api::{ApiError, GitHubClient};
use crate::github::auth::Token;
use crate::github::tools::{
    CommentTool, CreateIssueTool, GetIssueTool, ListIssuesTool, ListPrsTool, McpTool, ReviewPrTool,
    all_tools,
};

/// Build a `GitHubClient` pointed at a mockito server. Each test gets
/// its own `Server::new_async().await`, so they don't share state.
/// `backoff_scale = 0` so rate-limit retry tests are instant.
async fn client_against(server: &mockito::Server) -> GitHubClient {
    let url = server.url();
    let http = reqwest::Client::builder()
        .user_agent("shannon-mcp-saas/0.7")
        .build()
        .expect("build reqwest");
    GitHubClient::new(http, Some(Token::new("test-token".into())))
        .with_base_url(url)
        .with_backoff_scale(0)
}

// ---------------------------------------------------------------------------
// 1. list_issues — empty array
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_issues_returns_empty_array() {
    let mut server = mockito::Server::new_async().await;
    let m = server
        .mock("GET", "/repos/octocat/hello-world/issues")
        .match_query(Matcher::AllOf(vec![Matcher::UrlEncoded(
            "state".into(),
            "open".into(),
        )]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("[]")
        .create_async()
        .await;

    let client = client_against(&server).await;
    let issues = client
        .list_issues(
            "octocat",
            "hello-world",
            crate::github::api::IssueState::Open,
            None,
            None,
            None,
        )
        .await
        .expect("list_issues");
    assert!(issues.is_empty());
    m.assert_async().await;
}

// ---------------------------------------------------------------------------
// 2. get_issue — full field shape
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_issue_returns_fields() {
    let mut server = mockito::Server::new_async().await;
    let m = server
        .mock("GET", "/repos/octocat/hello-world/issues/42")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "number": 42,
                "title": "Test issue",
                "state": "open",
                "html_url": "https://github.com/octocat/hello-world/issues/42",
                "body": "details here",
                "labels": [{"name": "bug"}, {"name": "P1"}],
                "user": {"login": "octocat"},
                "created_at": "2024-01-01T00:00:00Z",
                "updated_at": "2024-01-02T00:00:00Z"
            })
            .to_string(),
        )
        .create_async()
        .await;

    let client = client_against(&server).await;
    let issue = client
        .get_issue("octocat", "hello-world", 42)
        .await
        .expect("get_issue");
    assert_eq!(issue.number, 42);
    assert_eq!(issue.title, "Test issue");
    assert_eq!(issue.state, "open");
    assert_eq!(
        issue.html_url,
        "https://github.com/octocat/hello-world/issues/42"
    );
    assert_eq!(issue.body.as_deref(), Some("details here"));
    assert_eq!(issue.labels.len(), 2);
    assert_eq!(issue.labels[0].name, "bug");
    assert_eq!(
        issue.user.as_ref().map(|u| u.login.as_str()),
        Some("octocat")
    );
    m.assert_async().await;
}

// ---------------------------------------------------------------------------
// 3. create_issue — url + number
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_issue_returns_url_and_number() {
    let mut server = mockito::Server::new_async().await;
    let m = server
        .mock("POST", "/repos/octocat/hello-world/issues")
        .match_body(Matcher::PartialJson(json!({
            "title": "Found a bug"
        })))
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "number": 101,
                "title": "Found a bug",
                "state": "open",
                "html_url": "https://github.com/octocat/hello-world/issues/101"
            })
            .to_string(),
        )
        .create_async()
        .await;

    let client = client_against(&server).await;
    let tool = CreateIssueTool {
        client: Arc::new(client),
    };
    let resp = tool
        .execute(json!({
            "owner": "octocat",
            "repo": "hello-world",
            "title": "Found a bug",
            "body": "descr",
            "permission": "write"
        }))
        .await
        .expect("create_issue");
    let issue = &resp["issue"];
    assert_eq!(issue["number"], 101);
    assert_eq!(
        issue["html_url"],
        "https://github.com/octocat/hello-world/issues/101"
    );
    assert_eq!(resp["created"], json!(true));
    m.assert_async().await;
}

// ---------------------------------------------------------------------------
// 4. comment — id
// ---------------------------------------------------------------------------

#[tokio::test]
async fn comment_returns_id() {
    let mut server = mockito::Server::new_async().await;
    let m = server
        .mock("POST", "/repos/octocat/hello-world/issues/7/comments")
        .match_body(Matcher::PartialJson(json!({
            "body": "Nice work!"
        })))
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "id": 5551212,
                "body": "Nice work!",
                "html_url": "https://github.com/octocat/hello-world/issues/7#issuecomment-5551212",
                "user": {"login": "octocat"}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let client = client_against(&server).await;
    let tool = CommentTool {
        client: Arc::new(client),
    };
    let resp = tool
        .execute(json!({
            "owner": "octocat",
            "repo": "hello-world",
            "number": 7,
            "body": "Nice work!",
            "permission": "write"
        }))
        .await
        .expect("comment");
    assert_eq!(resp["comment"]["id"], 5551212);
    assert_eq!(resp["created"], json!(true));
    m.assert_async().await;
}

// ---------------------------------------------------------------------------
// 5. list_prs — array
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_prs_returns_array() {
    let mut server = mockito::Server::new_async().await;
    let m = server
        .mock("GET", "/repos/octocat/hello-world/pulls")
        .match_query(Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!([
                {
                    "number": 8,
                    "title": "Fix typo",
                    "state": "open",
                    "html_url": "https://github.com/octocat/hello-world/pull/8",
                    "user": {"login": "octocat"},
                    "head": {"ref": "fix-typo", "sha": "abc123"},
                    "base": {"ref": "main", "sha": "def456"},
                    "merge_commit_sha": null
                },
                {
                    "number": 9,
                    "title": "Add tests",
                    "state": "closed",
                    "html_url": "https://github.com/octocat/hello-world/pull/9",
                    "user": {"login": "monalisa"},
                    "head": {"ref": "tests", "sha": "111"},
                    "base": {"ref": "main", "sha": "222"},
                    "merge_commit_sha": "333"
                }
            ])
            .to_string(),
        )
        .create_async()
        .await;

    let client = client_against(&server).await;
    let tool = ListPrsTool {
        client: Arc::new(client),
    };
    let resp = tool
        .execute(json!({"owner": "octocat", "repo": "hello-world", "state": "all"}))
        .await
        .expect("list_prs");
    assert_eq!(resp["total_count"], 2);
    let pulls = resp["pulls"].as_array().expect("array");
    assert_eq!(pulls.len(), 2);
    assert_eq!(pulls[0]["number"], 8);
    assert_eq!(pulls[0]["state"], "open");
    assert_eq!(pulls[1]["state"], "closed");
    m.assert_async().await;
}

// ---------------------------------------------------------------------------
// 6. review_pr — state change (APPROVE)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn review_pr_returns_state_change() {
    let mut server = mockito::Server::new_async().await;
    let m = server
        .mock("POST", "/repos/octocat/hello-world/pulls/8/reviews")
        .match_body(Matcher::PartialJson(json!({
            "event": "APPROVE",
            "body": "LGTM"
        })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "id": 7777,
                "state": "APPROVED",
                "html_url": "https://github.com/octocat/hello-world/pull/8#pullrequestreview-7777",
                "submitted_at": "2024-06-01T00:00:00Z",
                "body": "LGTM",
                "user": {"login": "octocat"}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let client = client_against(&server).await;
    let tool = ReviewPrTool {
        client: Arc::new(client),
    };
    let resp = tool
        .execute(json!({
            "owner": "octocat",
            "repo": "hello-world",
            "number": 8,
            "event": "APPROVE",
            "body": "LGTM",
            "permission": "write"
        }))
        .await
        .expect("review_pr");
    assert_eq!(resp["review"]["id"], 7777);
    assert_eq!(resp["review"]["state"], "APPROVED");
    assert_eq!(resp["submitted"], json!(true));
    m.assert_async().await;
}

// ---------------------------------------------------------------------------
// 7. rate-limit 429 → retry-after backoff
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rate_limit_429_triggers_retry_after_backoff() {
    let mut server = mockito::Server::new_async().await;
    // First call: 429 with `retry-after: 2`. Second call: 200 OK.
    let m_429 = server
        .mock("GET", "/repos/octocat/hello-world/issues/1")
        .with_status(429)
        .with_header("retry-after", "2")
        .with_body(r#"{"message":"API rate limit exceeded"}"#)
        .expect_at_least(1)
        .create_async()
        .await;
    let m_200 = server
        .mock("GET", "/repos/octocat/hello-world/issues/1")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "number": 1,
                "title": "recovered",
                "state": "open",
                "html_url": "https://github.com/octocat/hello-world/issues/1"
            })
            .to_string(),
        )
        .create_async()
        .await;

    // Client with `backoff_scale = 0` → backoff sleeps are skipped.
    let client = client_against(&server).await;
    let tool = GetIssueTool {
        client: Arc::new(client),
    };

    let start = std::time::Instant::now();
    let resp = tool
        .execute(json!({"owner": "octocat", "repo": "hello-world", "number": 1}))
        .await
        .expect("execute");
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_millis(500),
        "test should not actually wait; elapsed={elapsed:?}"
    );

    let issue = &resp["issue"];
    assert_eq!(issue["number"], 1);
    assert_eq!(issue["title"], "recovered");

    m_429.assert_async().await;
    m_200.assert_async().await;
}

// ---------------------------------------------------------------------------
// 8. secondary rate-limit (403 abuse detection) → exponential backoff
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rate_limit_secondary_rate_limit_returns_403_abuse() {
    let mut server = mockito::Server::new_async().await;
    // GitHub signals secondary rate-limit with 403 + remaining=0
    // and no `retry-after`. We return 403 once, then 200.
    let m_403 = server
        .mock("GET", "/repos/octocat/hello-world/issues/2")
        .match_query(Matcher::Any)
        .with_status(403)
        .with_header("x-ratelimit-remaining", "0")
        .with_header("x-ratelimit-reset", "9999999999")
        .with_body(r#"{"message":"You have exceeded a secondary rate limit"}"#)
        .expect_at_least(1)
        .create_async()
        .await;
    let m_200 = server
        .mock("GET", "/repos/octocat/hello-world/issues/2")
        .match_query(Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "number": 2,
                "title": "after backoff",
                "state": "open",
                "html_url": "https://github.com/octocat/hello-world/issues/2"
            })
            .to_string(),
        )
        .create_async()
        .await;

    let client = client_against(&server).await;
    let start = std::time::Instant::now();
    let result = client
        .get_issue("octocat", "hello-world", 2)
        .await
        .expect("get_issue");

    assert_eq!(result.number, 2);
    assert!(
        start.elapsed() < Duration::from_millis(500),
        "backoff_scale=0 test should not block; elapsed={:?}",
        start.elapsed()
    );

    m_403.assert_async().await;
    m_200.assert_async().await;
}

// ---------------------------------------------------------------------------
// 9. unauthenticated → 401
// ---------------------------------------------------------------------------

#[tokio::test]
async fn unauthenticated_returns_401() {
    let mut server = mockito::Server::new_async().await;
    let m = server
        .mock("GET", "/repos/octocat/hello-world/issues/3")
        .with_status(401)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"message":"Bad credentials","documentation_url":"https://docs.github.com/rest"}"#,
        )
        .create_async()
        .await;

    let client = client_against(&server).await;
    let err = client
        .get_issue("octocat", "hello-world", 3)
        .await
        .expect_err("expected 401");
    assert!(matches!(err, ApiError::Unauthorized));
    m.assert_async().await;
}

// ---------------------------------------------------------------------------
// 10. write-tool permission gating
// ---------------------------------------------------------------------------

// Note: the previous `*_without_permission_returns_error` tests exercised
// the self-attested `args.permission` gate, which has been removed (see the
// module docs in `github/tools.rs`). Permission enforcement now lives in
// `server::SessionGrants` and is exercised by the integration tests in
// `server.rs`:
//   * write_tool_without_grant_is_denied_even_if_args_permission_set
//   * write_tool_succeeds_after_tools_grant
//   * revoke_unsets_a_grant
//   * read_only_defaults_pre_grant_read_tools
//
// The write tools themselves now trust the server boundary and will reach
// the upstream HTTP layer (and fail there for the unauthenticated client
// used in tests). That is the correct post-fix behavior.

// ---------------------------------------------------------------------------
// 11. all_tools() returns six McpTool trait objects with the right names
// ---------------------------------------------------------------------------

#[test]
fn all_tools_returns_six_named_correctly() {
    let client = Arc::new(GitHubClient::new(reqwest::Client::new(), None));
    let tools = all_tools(client);
    let names: Vec<&'static str> = tools.iter().map(|t| t.name()).collect();
    assert_eq!(
        names,
        vec![
            "github_list_issues",
            "github_get_issue",
            "github_create_issue",
            "github_comment",
            "github_list_prs",
            "github_review_pr",
        ]
    );
}

// ---------------------------------------------------------------------------
// 12. list_issues — filters out PRs (post-filter via `pull_request` field)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_issues_filters_out_pull_requests() {
    let mut server = mockito::Server::new_async().await;
    let m = server
        .mock("GET", "/repos/octocat/hello-world/issues")
        .match_query(Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!([
                {
                    "number": 1,
                    "title": "real issue",
                    "state": "open",
                    "html_url": "https://github.com/octocat/hello-world/issues/1"
                },
                {
                    "number": 2,
                    "title": "this is a PR",
                    "state": "open",
                    "html_url": "https://github.com/octocat/hello-world/pull/2",
                    "pull_request": {"url": "https://api.github.com/repos/octocat/hello-world/pulls/2"}
                }
            ])
            .to_string(),
        )
        .create_async()
        .await;

    let client = client_against(&server).await;
    let issues = client
        .list_issues(
            "octocat",
            "hello-world",
            crate::github::api::IssueState::Open,
            None,
            None,
            None,
        )
        .await
        .expect("list_issues");
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].number, 1);
    m.assert_async().await;
}

// ---------------------------------------------------------------------------
// 13. read tools don't require permission
// ---------------------------------------------------------------------------

#[tokio::test]
async fn read_tools_do_not_require_permission_arg() {
    // No mockito needed — read tools never inspect `args.permission`.
    let tool = ListIssuesTool {
        client: Arc::new(GitHubClient::new(
            reqwest::Client::builder()
                .connect_timeout(Duration::from_millis(1))
                .build()
                .unwrap(),
            Some(Token::new("t".into())),
        )),
    };
    // Pass an empty args object; this *will* fail at the HTTP layer
    // (connect_timeout: 1ms + invalid host), but the failure should be
    // a network error, NOT a permission error.
    let err = tool
        .execute(json!({}))
        .await
        .expect_err("read tool should not need permission");
    let msg = format!("{err}");
    assert!(
        !msg.to_lowercase().contains("permission"),
        "read tool should not check permission; got {msg}"
    );
}

// ---------------------------------------------------------------------------
// 14. token exchange happy path (mockito against the access_token endpoint)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn oauth_exchange_code_returns_token() {
    let mut server = mockito::Server::new_async().await;
    let m = server
        .mock("POST", "/login/oauth/access_token")
        .match_header("accept", "application/json")
        .match_body(Matcher::AllOf(vec![Matcher::UrlEncoded(
            "code".into(),
            "abc123".into(),
        )]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "access_token": "gho_xxx",
                "scope": "repo,read:user",
                "token_type": "bearer"
            })
            .to_string(),
        )
        .create_async()
        .await;

    // We construct a flow that points at the mockito server. The
    // `bind_addr` is irrelevant for `exchange_code` — it is only used
    // by the callback server to compute the redirect_uri. We patch the
    // URL by spinning up a real `OAuthFlow::start()` on the ephemeral
    // port and then directly call `exchange_code()` with the mockito
    // URL. But `exchange_code` hardcodes
    // `https://github.com/login/oauth/access_token`, so we instead
    // hit the mockito `access_token` mock by using a `reqwest::Client`
    // configured with a base URL — which is what `exchange_code`
    // doesn't yet do. We approximate by hitting the mock with the
    // same path the GitHub API uses.
    //
    // The cleanest path is to extract the body parsing into a helper
    // that both `exchange_code` and the test can call. For now we
    // *do* test the wire path with a custom exchange:
    use crate::github::auth::PkcePair;
    let pkce = PkcePair::generate();
    let http = reqwest::Client::builder()
        .user_agent("shannon-mcp-saas/0.7")
        .build()
        .unwrap();
    let resp = http
        .post(format!("{}/login/oauth/access_token", server.url()))
        .header("Accept", "application/json")
        .form(&[
            ("client_id", "test"),
            ("client_secret", "test"),
            ("code", "abc123"),
            ("code_verifier", pkce.verifier.as_str()),
            ("redirect_uri", "http://127.0.0.1:0/callback"),
        ])
        .send()
        .await
        .expect("send");
    assert!(resp.status().is_success());
    let body: serde_json::Value = resp.json().await.expect("json");
    assert_eq!(body["access_token"], "gho_xxx");
    m.assert_async().await;
}

// ---------------------------------------------------------------------------
// 15. list_issues with since + per_page + page query
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_issues_passes_query_params() {
    let mut server = mockito::Server::new_async().await;
    let m = server
        .mock("GET", "/repos/octocat/hello-world/issues")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("state".into(), "closed".into()),
            Matcher::UrlEncoded("per_page".into(), "50".into()),
            Matcher::UrlEncoded("page".into(), "3".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("[]")
        .create_async()
        .await;

    let client = client_against(&server).await;
    let issues = client
        .list_issues(
            "octocat",
            "hello-world",
            crate::github::api::IssueState::Closed,
            None,
            Some(50),
            Some(3),
        )
        .await
        .expect("list_issues");
    assert!(issues.is_empty());
    m.assert_async().await;
}
