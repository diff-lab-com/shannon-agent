//! N1 regression tests: reqwest Kind::Request failures ("error sending
//! request") must classify as retryable. Measured in the TB2.1 sweep: 18
//! first-call deaths from this exact error class (DNS flaps / resets while
//! the request is in flight), none of which the old classification
//! (`is_timeout() || is_connect()`) ever retried — Kind::Request is neither.
//!
//! Deterministic reproduction: a localhost listener that accepts the
//! connection and closes it cleanly (FIN) after a beat. The client finishes
//! sending into the socket buffer, then hits the EOF before any response —
//! yielding Kind::Request with is_connect() == false, the precise shape the
//! old classifier missed.

use shannon_engine::api::error::ApiError;
use shannon_engine::api::retry::RetryPolicy;
use std::time::Duration;

/// Bind a listener that accepts one connection, holds it briefly, and closes.
fn fin_close_listener() -> std::net::SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            std::thread::sleep(Duration::from_millis(50));
            drop(stream);
        }
    });
    addr
}

/// Capture the actual reqwest error a client sees against the FIN-closing
/// listener, so the test cannot drift from reality.
async fn kind_request_error() -> reqwest::Error {
    let addr = fin_close_listener();
    let client = reqwest::Client::new();
    client
        .get(format!("http://{addr}/x"))
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .err()
        .expect("client should fail against a closing listener")
}

#[tokio::test]
async fn kind_request_failure_is_retryable() {
    let err = kind_request_error().await;
    assert!(
        err.is_request(),
        "expected Kind::Request, got a different error kind: {err}"
    );
    assert!(!err.is_connect(), "precondition: not a connect-phase failure");

    let policy = RetryPolicy::default();
    assert!(
        policy.is_retryable(&ApiError::HttpError(err)),
        "Kind::Request network failures must classify as retryable (N1)"
    );
}

#[tokio::test]
async fn kind_request_retry_policy_survives_rst_variant_too() {
    // RST variant: is_connect() is ALSO true here, so this class was always
    // retryable — kept as a guard that the fix didn't narrow the old paths.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            use std::os::fd::AsRawFd;
            let linger = libc::linger {
                l_onoff: 1,
                l_linger: 0,
            };
            unsafe {
                libc::setsockopt(
                    stream.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_LINGER,
                    &linger as *const libc::linger as *const libc::c_void,
                    std::mem::size_of::<libc::linger>() as u32,
                );
            }
            drop(stream);
        }
    });
    let client = reqwest::Client::new();
    let err = client
        .get(format!("http://{addr}/x"))
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .err()
        .expect("RST listener should produce an error");

    let policy = RetryPolicy::default();
    assert!(
        policy.is_retryable(&ApiError::HttpError(err)),
        "RST failures were always retryable and must stay so"
    );
}

// ── N1 end-to-end at the client layer ───────────────────────────────────────
//
// Full-path proof: two connections are injected to fail with the exact
// TB2.1-failure shape (accept → hold → FIN close → Kind::Request,
// is_connect() == false), the third returns a canned Anthropic response.
// `send_message_with_retry` must recover — proving Kind::Request failures
// flow through the classification (N1) and the bounded retry loop.
//
// The mini-server deliberately bypasses mockito: mockito stubs always
// respond properly and cannot produce a mid-request connection drop.

mod support {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Connections to fail with a FIN before any response (Kind::Request
    /// per the probe: is_request=true, is_connect=false).
    pub const INJECTED_FAILURES: usize = 2;

    pub static CONN_COUNT: AtomicUsize = AtomicUsize::new(0);

    pub fn spawn_injecting_server() -> std::net::SocketAddr {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let failures_left = Arc::new(AtomicUsize::new(INJECTED_FAILURES));
        let served = Arc::new(AtomicUsize::new(0));
        let f = failures_left.clone();
        let s = served.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                s.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf); // consume the request head
                if f.load(Ordering::SeqCst) > 0 {
                    f.fetch_sub(1, Ordering::SeqCst);
                    // Hold briefly so the client is mid-request, then FIN.
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    drop(stream);
                    continue;
                }
                let body = r#"{"id":"msg_n1","role":"assistant","content":[{"type":"text","text":"ok"}],"model":"claude-3","stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.flush();
                let _ = stream.shutdown(std::net::Shutdown::Both);
            }
        });
        let _ = s;
        addr
    }

    pub fn served_count() -> usize {
        // The server thread owns the counter; approximate via the listener
        // port's connection count is not exposed, so tests assert on
        // send_message outcomes instead. Kept for symmetry.
        0
    }
}

#[tokio::test]
async fn transient_kind_request_failures_are_retried_to_success() {
    use shannon_engine::api::retry::RetryConfig;
    use shannon_engine::api::{LlmClient, LlmClientConfig, LlmProvider, Message};

    let addr = support::spawn_injecting_server();

    let config = LlmClientConfig {
        provider: LlmProvider::Anthropic,
        api_key: "test-key".to_string(),
        model: "claude-3".to_string(),
        base_url: format!("http://{addr}"),
        max_tokens: 64,
        timeout_seconds: 10,
        retry_config: RetryConfig {
            max_retries: 3,
            initial_backoff_ms: 10,
            max_backoff_ms: 50,
            ..Default::default()
        },
        ..Default::default()
    };
    let client = LlmClient::new(config);

    let blocks = client
        .send_message_with_retry(vec![Message {
            role: "user".to_string(),
            content: shannon_engine::api::MessageContent::Text("hi".to_string()),
        }], None, None)
        .await
        .expect("retry must recover from injected Kind::Request failures");

    let text = blocks
        .iter()
        .filter_map(|b| match b {
            shannon_engine::api::ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<String>();
    assert_eq!(text, "ok");
}

#[tokio::test]
async fn permanent_failures_surface_immediately_when_retries_disabled() {
    use shannon_engine::api::{LlmClient, LlmClientConfig, LlmProvider, Message};
    use shannon_engine::api::retry::RetryConfig;

    // No listener at all: connect-phase errors with retries disabled must
    // surface as an error after a single attempt (bounded behavior guard).
    let config = LlmClientConfig {
        provider: LlmProvider::Anthropic,
        api_key: "test-key".to_string(),
        model: "claude-3".to_string(),
        base_url: "http://127.0.0.1:1".to_string(), // port 1: nothing listens
        max_tokens: 64,
        timeout_seconds: 2,
        retry_config: RetryConfig {
            max_retries: 0,
            initial_backoff_ms: 1,
            max_backoff_ms: 1,
            ..Default::default()
        },
        ..Default::default()
    };
    let client = LlmClient::new(config);
    let err = client
        .send_message_with_retry(
            vec![Message {
                role: "user".to_string(),
                content: shannon_engine::api::MessageContent::Text("hi".to_string()),
            }],
            None,
            None,
        )
        .await
        .expect_err("retries disabled + dead endpoint must error");
    let _ = err; // shape asserted by classification tests elsewhere
}
