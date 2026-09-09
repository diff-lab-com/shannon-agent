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
