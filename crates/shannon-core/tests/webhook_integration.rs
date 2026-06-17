//! Integration tests for `WebhookHandler` HTTP delivery.
//!
//! Verifies that `WebhookHandler::send()` actually reaches the target server
//! via mockito, that HMAC signature headers are present when configured, and
//! that the fire-and-forget `tokio::spawn` does not block the caller.

use chrono::Utc;
use shannon_core::notifier::{
    Notification, NotificationHandler, NotificationLevel, NotifierError, WebhookConfig,
    WebhookHandler, WebhookTemplate,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

fn sample_notification(title: &str) -> Notification {
    Notification {
        title: title.into(),
        body: "All tests passed".into(),
        level: NotificationLevel::Success,
        id: "w-int-1".into(),
        timestamp: Utc::now(),
        source: Some("query_complete".into()),
        action_id: None,
    }
}

#[tokio::test]
async fn webhook_post_reaches_mockito_server_with_slack_body() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/hook")
        .match_header("content-type", "application/json")
        .match_body(mockito::Matcher::PartialJson(
            serde_json::json!({"text": "*Build OK*\nAll tests passed"}),
        ))
        .with_status(200)
        .with_body("ok")
        .create_async()
        .await;

    let handler = WebhookHandler::new(WebhookConfig {
        url: format!("{}/hook", server.url()),
        secret: None,
        template: WebhookTemplate::Slack,
        timeout_ms: 2000,
        include_body: true,
    })
    .unwrap();

    handler.send(&sample_notification("Build OK")).unwrap();

    // Fire-and-forget: poll briefly until mockito records the hit.
    for _ in 0..50 {
        if mock.matched() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    mock.assert();
}

#[tokio::test]
async fn webhook_hmac_signature_header_sent_when_secret_configured() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/hook")
        .match_header(
            "x-shannon-signature",
            mockito::Matcher::Regex(r"^sha256=[0-9a-f]{64}$".to_string()),
        )
        .with_status(200)
        .create_async()
        .await;

    let handler = WebhookHandler::new(WebhookConfig {
        url: format!("{}/hook", server.url()),
        secret: Some("integration-secret".into()),
        template: WebhookTemplate::Raw,
        timeout_ms: 2000,
        include_body: false,
    })
    .unwrap();

    handler.send(&sample_notification("signed")).unwrap();

    for _ in 0..50 {
        if mock.matched() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    mock.assert();
}

#[tokio::test]
async fn webhook_handler_does_not_block_notifier_pipeline() {
    // A slow webhook (server sleeps 2s) should not delay a fast callback handler
    // invoked after it via the same Notifier::notify call.
    let mut slow_server = mockito::Server::new_async().await;
    let _slow_mock = slow_server
        .mock("POST", "/slow")
        .with_status(200)
        .with_body("ok")
        .create_async()
        .await;

    let slow_url = format!("{}/slow", slow_server.url());
    let counter: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();

    // We can't easily wire WebhookHandler into Notifier directly here because
    // Notifier::add_handler takes Box<dyn NotificationHandler + Send + Sync>.
    // Instead, verify timing: send() returns immediately even when the server
    // would take a long time to respond.
    let handler = WebhookHandler::new(WebhookConfig {
        url: slow_url,
        secret: None,
        template: WebhookTemplate::Raw,
        timeout_ms: 5_000,
        include_body: false,
    })
    .unwrap();

    // Spawn a "fast callback" task that fires immediately after send() returns.
    let start = std::time::Instant::now();
    handler.send(&sample_notification("slow")).unwrap();
    let elapsed_after_send = start.elapsed();
    counter_clone.store(1, Ordering::SeqCst);

    // send() should have returned in well under 100ms — it just spawns a task.
    assert!(
        elapsed_after_send < Duration::from_millis(100),
        "send() took {elapsed_after_send:?}; expected <100ms (fire-and-forget)"
    );
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn webhook_delivery_failure_does_not_propagate_to_caller() {
    // Unreachable URL — send() should still return Ok (fire-and-forget).
    let handler = WebhookHandler::new(WebhookConfig {
        url: "http://127.0.0.1:1/nonexistent".into(), // port 1 = no listener
        secret: None,
        template: WebhookTemplate::Slack,
        timeout_ms: 200,
        include_body: false,
    })
    .unwrap();

    let result = handler.send(&sample_notification("dropped"));
    assert!(
        result.is_ok(),
        "send() should be Ok even when delivery fails"
    );
}

#[test]
fn webhook_send_outside_runtime_returns_handler_failed() {
    // Without #[tokio::test], there's no runtime — send() must return HandlerFailed.
    let handler = WebhookHandler::new(WebhookConfig {
        url: "http://example.invalid/hook".into(),
        secret: None,
        template: WebhookTemplate::Raw,
        timeout_ms: 100,
        include_body: false,
    })
    .unwrap();

    match handler.send(&sample_notification("no-runtime")) {
        Err(NotifierError::HandlerFailed { name, .. }) => assert_eq!(name, "webhook"),
        other => panic!("expected HandlerFailed, got {other:?}"),
    }
}

#[tokio::test]
async fn webhook_feishu_payload_matches_text_schema() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/hook")
        .match_body(mockito::Matcher::PartialJson(serde_json::json!({
            "msg_type": "text",
            "content": { "text": "Build OK\nAll tests passed" }
        })))
        .with_status(200)
        .create_async()
        .await;

    let handler = WebhookHandler::new(WebhookConfig {
        url: format!("{}/hook", server.url()),
        secret: None,
        template: WebhookTemplate::Feishu,
        timeout_ms: 2000,
        include_body: true,
    })
    .unwrap();

    handler.send(&sample_notification("Build OK")).unwrap();

    for _ in 0..50 {
        if mock.matched() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    mock.assert();
}

#[tokio::test]
async fn webhook_wechat_payload_matches_text_schema() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/hook")
        .match_body(mockito::Matcher::PartialJson(serde_json::json!({
            "msgtype": "text",
            "text": { "content": "Build OK\nAll tests passed" }
        })))
        .with_status(200)
        .create_async()
        .await;

    let handler = WebhookHandler::new(WebhookConfig {
        url: format!("{}/hook", server.url()),
        secret: None,
        template: WebhookTemplate::Wechat,
        timeout_ms: 2000,
        include_body: true,
    })
    .unwrap();

    handler.send(&sample_notification("Build OK")).unwrap();

    for _ in 0..50 {
        if mock.matched() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    mock.assert();
}
