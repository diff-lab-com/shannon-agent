//! Engine API server discovery (Q4-A).
//!
//! Before hosting its own loopback API server, the desktop probes
//! `127.0.0.1:33420` to see whether another process (typically the `shannon`
//! CLI REPL, or another desktop instance) is already serving the engine
//! protocol. If something is listening and answers an HTTP request, we
//! connect as a client and skip hosting our own server — the two
//! processes share the same loopback port without colliding.
//!
//! The probe is bounded by a 250 ms timeout so a non-responsive listener
//! cannot delay desktop startup noticeably.

use std::time::Duration;

use serde::Serialize;

/// How the desktop obtains its engine connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EngineMode {
    /// No other engine was reachable; desktop hosts `loopback_api` on
    /// `127.0.0.1:33420`.
    Hosted,
    /// Another engine is already listening; desktop connects as a client
    /// to the existing endpoint.
    External,
}

/// Bounded probe of the loopback engine port. Returns `External` if a TCP
/// connection completes within 250 ms AND the listener answers an HTTP
/// `OPTIONS /api/ws` request (any HTTP status proves a server is here —
/// the engine doesn't need to grant OPTIONS to be authoritative). Returns
/// `Hosted` on connect failure, timeout, or non-HTTP listener.
///
/// `probe_at` is the testable seam: tests pass a known-free port to assert
/// `Hosted` and a listener-bound port serving canned HTTP to assert
/// `External`. Production callers use [`probe_existing_engine`].
pub async fn probe_at(host: &str, port: u16) -> EngineMode {
    let url = format!("http://{host}:{port}/api/ws");
    let result = tokio::time::timeout(
        Duration::from_millis(250),
        reqwest::Client::new()
            .request(reqwest::Method::OPTIONS, &url)
            .send(),
    )
    .await;
    match result {
        Ok(Ok(_response)) => EngineMode::External,
        _ => EngineMode::Hosted,
    }
}

/// Probe the canonical loopback engine port. See [`probe_at`].
pub async fn probe_existing_engine() -> EngineMode {
    probe_at("127.0.0.1", crate::loopback_api::LOOPBACK_PORT).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Reserve a free OS-assigned port and return it. The port is released
    /// before the test returns; tests that need an actual listener must
    /// re-bind it.
    async fn free_port() -> u16 {
        let probe = TcpListener::bind("127.0.0.1:0").await.expect("bind probe");
        let port = probe.local_addr().expect("addr").port();
        drop(probe);
        port
    }

    #[tokio::test]
    async fn probe_at_returns_hosted_when_port_is_free() {
        let port = free_port().await;
        // Sleep briefly so the OS releases the port — in practice the
        // bind + drop is synchronous and the OS never reuses the port
        // before our probe connects.
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(probe_at("127.0.0.1", port).await, EngineMode::Hosted);
    }

    #[tokio::test]
    async fn probe_at_returns_external_when_http_responds() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr: SocketAddr = listener.local_addr().expect("addr");

        // Background task: accept one connection, read until \r\n\r\n
        // (end of HTTP request line + headers), reply with HTTP 200 OK,
        // close. That's all the probe needs.
        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf).await;
                let reply = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";
                let _ = stream.write_all(reply).await;
                let _ = stream.shutdown().await;
            }
        });

        assert_eq!(
            probe_at("127.0.0.1", addr.port()).await,
            EngineMode::External
        );
    }

    #[tokio::test]
    async fn probe_at_times_out_against_unresponsive_listener() {
        // Bind a listener that accepts but never writes. The probe must
        // bail out at 250 ms (we assert a faster ceiling to avoid CI
        // flake on loaded runners).
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                // Hold the stream open until dropped.
                tokio::time::sleep(Duration::from_secs(5)).await;
                drop(stream);
            }
        });
        let start = std::time::Instant::now();
        let mode = probe_at("127.0.0.1", addr.port()).await;
        let elapsed = start.elapsed();
        assert_eq!(mode, EngineMode::Hosted);
        assert!(
            elapsed < Duration::from_millis(800),
            "probe took {elapsed:?} — expected < 800 ms (250 ms timeout + headroom)"
        );
    }
}
