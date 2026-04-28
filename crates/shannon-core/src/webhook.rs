//! Webhook receiver for external event injection.
//!
//! Runs a lightweight HTTP server that accepts incoming webhook payloads
//! (e.g. GitHub PR comments) and queues them for the REPL to consume
//! as injected messages.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tower_http::cors::{Any, CorsLayer};
use tracing::{debug, info, warn};

// ── Error type ──────────────────────────────────────────────────────────

/// Errors from webhook operations.
#[derive(Debug, thiserror::Error)]
pub enum WebhookError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Channel closed")]
    ChannelClosed,
    #[error("Server error: {0}")]
    Server(String),
}

// ── Event types ─────────────────────────────────────────────────────────

/// Source of an external webhook event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WebhookSource {
    /// GitHub webhook (issue comment, PR review, etc.)
    GitHub,
    /// Generic HTTP POST webhook.
    Custom(String),
    /// Slack webhook.
    Slack,
}

impl std::fmt::Display for WebhookSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GitHub => write!(f, "github"),
            Self::Custom(name) => write!(f, "custom:{name}"),
            Self::Slack => write!(f, "slack"),
        }
    }
}

/// A single external event received via webhook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookEvent {
    /// Unique event ID.
    pub id: String,
    /// Event source.
    pub source: WebhookSource,
    /// Human-readable title (e.g. "PR comment on #42").
    pub title: String,
    /// Full body text to inject into the session.
    pub body: String,
    /// When the event was received.
    pub timestamp: DateTime<Utc>,
    /// Optional URL for context (e.g. PR link).
    #[serde(default)]
    pub url: Option<String>,
    /// Raw payload for advanced consumers.
    #[serde(default)]
    pub raw_payload: Option<serde_json::Value>,
}

// ── GitHub webhook payloads ─────────────────────────────────────────────

/// Minimal GitHub `issue_comment` webhook payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubIssueCommentPayload {
    pub action: String,
    pub comment: GitHubComment,
    #[serde(default)]
    pub issue: Option<GitHubIssue>,
    #[serde(default)]
    pub repository: Option<GitHubRepository>,
    #[serde(default)]
    pub sender: Option<GitHubUser>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubComment {
    pub id: u64,
    pub body: String,
    pub user: GitHubUser,
    #[serde(default)]
    pub html_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubIssue {
    pub number: u64,
    pub title: String,
    #[serde(default)]
    pub html_url: Option<String>,
    #[serde(default)]
    pub pull_request: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubRepository {
    pub full_name: String,
    #[serde(default)]
    pub html_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubUser {
    pub login: String,
}

/// Generic webhook POST body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenericWebhookPayload {
    /// Title for the injected event.
    #[serde(default)]
    pub title: Option<String>,
    /// Body text to inject.
    #[serde(default)]
    pub body: Option<String>,
    /// Optional source identifier.
    #[serde(default)]
    pub source: Option<String>,
    /// Optional URL.
    #[serde(default)]
    pub url: Option<String>,
    /// Pass-through raw JSON.
    #[serde(default)]
    pub payload: Option<serde_json::Value>,
}

// ── Shared state ────────────────────────────────────────────────────────

#[derive(Clone)]
struct WebhookState {
    tx: mpsc::UnboundedSender<WebhookEvent>,
    secret: Option<String>,
}

// ── Webhook receiver ────────────────────────────────────────────────────

/// Configuration for the webhook receiver.
#[derive(Debug, Clone)]
pub struct WebhookConfig {
    /// Bind host (default: "127.0.0.1").
    pub host: String,
    /// Bind port (default: 3789).
    pub port: u16,
    /// Optional HMAC secret for signature verification.
    pub secret: Option<String>,
}

impl Default for WebhookConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 3789,
            secret: None,
        }
    }
}

/// A running webhook receiver server.
///
/// Call [`WebhookReceiver::events()`] to get a stream of incoming events.
pub struct WebhookReceiver {
    config: WebhookConfig,
    tx: mpsc::UnboundedSender<WebhookEvent>,
    rx: mpsc::UnboundedReceiver<WebhookEvent>,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl WebhookReceiver {
    /// Create a new receiver with the given configuration.
    pub fn new(config: WebhookConfig) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            config,
            tx,
            rx,
            shutdown_tx: None,
        }
    }

    /// Start the HTTP server in the background.
    ///
    /// Returns `Ok(())` once the server is bound and listening.
    /// Call `stop()` to shut down.
    pub async fn start(&mut self) -> Result<(), WebhookError> {
        let state = WebhookState {
            tx: self.tx.clone(),
            secret: self.config.secret.clone(),
        };

        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);

        let app = axum::Router::new()
            .route("/webhook/github", post(github_handler))
            .route("/webhook/generic", post(generic_handler))
            .route("/webhook/health", post(health_handler))
            .layer(cors)
            .with_state(state);

        let addr = format!("{}:{}", self.config.host, self.config.port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        info!("Webhook receiver listening on {addr}");

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        self.shutdown_tx = Some(shutdown_tx);

        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .ok();
        });

        Ok(())
    }

    /// Start the HTTP server from a synchronous context.
    ///
    /// Spawns a background tokio runtime thread to run the server.
    pub fn try_start(&mut self) -> Result<(), WebhookError> {
        let state = WebhookState {
            tx: self.tx.clone(),
            secret: self.config.secret.clone(),
        };

        let host = self.config.host.clone();
        let port = self.config.port;
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        self.shutdown_tx = Some(shutdown_tx);

        std::thread::Builder::new()
            .name("shannon-webhook".to_string())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        let _ = ready_tx.send(Err(WebhookError::Server(e.to_string())));
                        return;
                    }
                };

                let cors = CorsLayer::new()
                    .allow_origin(Any)
                    .allow_methods(Any)
                    .allow_headers(Any);

                let app = axum::Router::new()
                    .route("/webhook/github", post(github_handler))
                    .route("/webhook/generic", post(generic_handler))
                    .route("/webhook/health", post(health_handler))
                    .layer(cors)
                    .with_state(state);

                let addr = format!("{host}:{port}");

                rt.block_on(async {
                    let listener = match tokio::net::TcpListener::bind(&addr).await {
                        Ok(l) => l,
                        Err(e) => {
                            let _ = ready_tx.send(Err(WebhookError::Io(e)));
                            return;
                        }
                    };
                    let _ = ready_tx.send(Ok(()));
                    info!("Webhook receiver listening on {addr}");
                    axum::serve(listener, app)
                        .with_graceful_shutdown(async {
                            let _ = shutdown_rx.await;
                        })
                        .await
                        .ok();
                });
            })
            .map_err(|e| WebhookError::Server(e.to_string()))?;

        ready_rx.recv().map_err(|_| WebhookError::ChannelClosed)?
    }

    /// Stop the webhook server.
    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }

    /// Receive the next event (blocking).
    pub async fn recv(&mut self) -> Option<WebhookEvent> {
        self.rx.recv().await
    }

    /// Check if there are pending events without blocking.
    pub fn try_recv(&mut self) -> Option<WebhookEvent> {
        self.rx.try_recv().ok()
    }

    /// The bind address in use.
    pub fn address(&self) -> String {
        format!("{}:{}", self.config.host, self.config.port)
    }
}

impl Drop for WebhookReceiver {
    fn drop(&mut self) {
        self.stop();
    }
}

// ── Route handlers ──────────────────────────────────────────────────────

async fn github_handler(
    State(state): State<WebhookState>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // Validate signature if secret is configured.
    if let Some(ref secret) = state.secret {
        if let Some(sig) = headers.get("X-Hub-Signature-256") {
            let sig = sig.to_str().unwrap_or("");
            if !verify_hmac(&payload, secret, sig) {
                warn!("GitHub webhook signature verification failed");
                return Err((
                    StatusCode::UNAUTHORIZED,
                    "Invalid signature".to_string(),
                ));
            }
        }
    }

    let event_type = headers
        .get("X-GitHub-Event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");

    debug!("GitHub webhook received: event={event_type}");

    let webhook_event = match event_type {
        "issue_comment" => parse_github_comment(&payload),
        "pull_request_review_comment" => parse_github_comment(&payload),
        "pull_request_review" => parse_github_review(&payload),
        _ => {
            // Generic fallback: stringify the payload.
            Some(WebhookEvent {
                id: uuid::Uuid::new_v4().to_string(),
                source: WebhookSource::GitHub,
                title: format!("GitHub event: {event_type}"),
                body: payload.to_string(),
                timestamp: Utc::now(),
                url: None,
                raw_payload: Some(payload),
            })
        }
    };

    if let Some(event) = webhook_event {
        if state.tx.send(event).is_err() {
            warn!("Webhook event channel closed, dropping event");
        }
    }

    Ok(StatusCode::OK)
}

async fn generic_handler(
    State(state): State<WebhookState>,
    Json(payload): Json<GenericWebhookPayload>,
) -> impl IntoResponse {
    let event = WebhookEvent {
        id: uuid::Uuid::new_v4().to_string(),
        source: payload
            .source
            .as_deref()
            .map(|s| WebhookSource::Custom(s.to_string()))
            .unwrap_or(WebhookSource::Custom("generic".to_string())),
        title: payload.title.unwrap_or_else(|| "Webhook event".to_string()),
        body: payload.body.unwrap_or_default(),
        timestamp: Utc::now(),
        url: payload.url,
        raw_payload: payload.payload,
    };

    if state.tx.send(event).is_err() {
        warn!("Webhook event channel closed, dropping event");
        return StatusCode::SERVICE_UNAVAILABLE;
    }

    StatusCode::OK
}

async fn health_handler() -> StatusCode {
    StatusCode::OK
}

// ── Payload parsing ─────────────────────────────────────────────────────

fn parse_github_comment(payload: &serde_json::Value) -> Option<WebhookEvent> {
    let action = payload.get("action")?.as_str()?;
    if action != "created" && action != "edited" {
        return None;
    }

    let comment = payload.get("comment")?;
    let body = comment.get("body")?.as_str()?.to_string();
    let user = comment
        .get("user")
        .and_then(|u| u.get("login"))
        .and_then(|l| l.as_str())
        .unwrap_or("unknown");
    let comment_url = comment
        .get("html_url")
        .and_then(|u| u.as_str())
        .map(String::from);

    let is_pr = payload
        .get("issue")
        .and_then(|i| i.get("pull_request"))
        .is_some();

    let issue_number = payload
        .get("issue")
        .and_then(|i| i.get("number"))
        .and_then(|n| n.as_u64());

    let repo = payload
        .get("repository")
        .and_then(|r| r.get("full_name"))
        .and_then(|n| n.as_str())
        .unwrap_or("unknown/repo");

    let title = if is_pr {
        format!("PR comment by @{user} on {repo}#{n}", n = issue_number.unwrap_or(0))
    } else {
        format!("Issue comment by @{user} on {repo}#{n}", n = issue_number.unwrap_or(0))
    };

    Some(WebhookEvent {
        id: uuid::Uuid::new_v4().to_string(),
        source: WebhookSource::GitHub,
        title,
        body,
        timestamp: Utc::now(),
        url: comment_url,
        raw_payload: Some(payload.clone()),
    })
}

fn parse_github_review(payload: &serde_json::Value) -> Option<WebhookEvent> {
    let action = payload.get("action")?.as_str()?;
    if action != "submitted" && action != "edited" {
        return None;
    }

    let review = payload.get("review")?;
    let body = review
        .get("body")
        .and_then(|b| b.as_str())
        .unwrap_or("(no body)");
    let user = review
        .get("user")
        .and_then(|u| u.get("login"))
        .and_then(|l| l.as_str())
        .unwrap_or("unknown");
    let state = review
        .get("state")
        .and_then(|s| s.as_str())
        .unwrap_or("pending");

    let pr_number = payload
        .get("pull_request")
        .and_then(|pr| pr.get("number"))
        .and_then(|n| n.as_u64());

    let repo = payload
        .get("repository")
        .and_then(|r| r.get("full_name"))
        .and_then(|n| n.as_str())
        .unwrap_or("unknown/repo");

    Some(WebhookEvent {
        id: uuid::Uuid::new_v4().to_string(),
        source: WebhookSource::GitHub,
        title: format!(
            "PR review ({state}) by @{user} on {repo}#{n}",
            n = pr_number.unwrap_or(0)
        ),
        body: body.to_string(),
        timestamp: Utc::now(),
        url: review
            .get("html_url")
            .and_then(|u| u.as_str())
            .map(String::from),
        raw_payload: Some(payload.clone()),
    })
}

/// Verify HMAC-SHA256 signature for GitHub webhooks.
fn verify_hmac(payload: &serde_json::Value, secret: &str, signature: &str) -> bool {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    type HmacSha256 = Hmac<Sha256>;

    let body = match serde_json::to_vec(payload) {
        Ok(b) => b,
        Err(_) => return false,
    };

    let mut mac = match HmacSha256::new_from_slice(secret.as_bytes()) {
        Ok(m) => m,
        Err(_) => return false,
    };
    mac.update(&body);
    let result = mac.finalize();
    let code_bytes = result.into_bytes();
    let expected = format!("sha256={}", hex::encode(code_bytes));

    // Constant-time comparison.
    let expected_bytes = expected.as_bytes();
    let sig_bytes = signature.as_bytes();

    if expected_bytes.len() != sig_bytes.len() {
        return false;
    }

    let mut diff: u8 = 0;
    for (a, b) in expected_bytes.iter().zip(sig_bytes.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_webhook_source_display() {
        assert_eq!(WebhookSource::GitHub.to_string(), "github");
        assert_eq!(
            WebhookSource::Custom("ci".to_string()).to_string(),
            "custom:ci"
        );
        assert_eq!(WebhookSource::Slack.to_string(), "slack");
    }

    #[test]
    fn test_webhook_config_default() {
        let config = WebhookConfig::default();
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 3789);
        assert!(config.secret.is_none());
    }

    #[test]
    fn test_parse_github_comment_created() {
        let payload = serde_json::json!({
            "action": "created",
            "comment": {
                "id": 123,
                "body": "Please fix the typo in the README",
                "html_url": "https://github.com/org/repo/issues/1#issuecomment-123",
                "user": { "login": "reviewer" }
            },
            "issue": {
                "number": 1,
                "title": "Bug fix",
                "pull_request": {},
                "html_url": "https://github.com/org/repo/pull/1"
            },
            "repository": {
                "full_name": "org/repo"
            }
        });

        let event = parse_github_comment(&payload).unwrap();
        assert!(event.title.contains("@reviewer"));
        assert!(event.title.contains("PR comment"));
        assert_eq!(event.body, "Please fix the typo in the README");
        assert_eq!(event.source, WebhookSource::GitHub);
        assert!(event.url.is_some());
    }

    #[test]
    fn test_parse_github_comment_ignores_deleted() {
        let payload = serde_json::json!({
            "action": "deleted",
            "comment": { "id": 1, "body": "gone", "user": { "login": "u" } }
        });
        assert!(parse_github_comment(&payload).is_none());
    }

    #[test]
    fn test_parse_github_review_submitted() {
        let payload = serde_json::json!({
            "action": "submitted",
            "review": {
                "body": "Looks good overall, minor nits",
                "state": "changes_requested",
                "user": { "login": "reviewer" },
                "html_url": "https://github.com/org/repo/pull/5#pullrequestreview-99"
            },
            "pull_request": {
                "number": 5
            },
            "repository": {
                "full_name": "org/repo"
            }
        });

        let event = parse_github_review(&payload).unwrap();
        assert!(event.title.contains("changes_requested"));
        assert!(event.title.contains("@reviewer"));
        assert_eq!(event.body, "Looks good overall, minor nits");
    }

    #[test]
    fn test_parse_github_review_ignores_dismissed() {
        let payload = serde_json::json!({
            "action": "dismissed",
            "review": { "body": "", "state": "pending", "user": { "login": "u" } }
        });
        assert!(parse_github_review(&payload).is_none());
    }

    #[test]
    fn test_generic_webhook_payload_deserialization() {
        let json = r#"{
            "title": "Build failed",
            "body": "CI pipeline failed on main branch",
            "source": "ci",
            "url": "https://ci.example.com/build/42"
        }"#;
        let payload: GenericWebhookPayload = serde_json::from_str(json).unwrap();
        assert_eq!(payload.title.as_deref(), Some("Build failed"));
        assert_eq!(payload.source.as_deref(), Some("ci"));
    }

    #[test]
    fn test_webhook_event_serialization() {
        let event = WebhookEvent {
            id: "test-id".to_string(),
            source: WebhookSource::GitHub,
            title: "Test".to_string(),
            body: "Hello".to_string(),
            timestamp: Utc::now(),
            url: None,
            raw_payload: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: WebhookEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "test-id");
        assert_eq!(back.source, WebhookSource::GitHub);
    }

    #[tokio::test]
    async fn test_webhook_receiver_start_stop() {
        let config = WebhookConfig {
            port: 19389,
            ..Default::default()
        };
        let mut receiver = WebhookReceiver::new(config);
        let result = receiver.start().await;
        if result.is_ok() {
            receiver.stop();
        }
    }
}
