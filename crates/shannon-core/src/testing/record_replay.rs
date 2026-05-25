//! Record/Replay system for zero-cost CI testing against real API responses.
//!
//! **Record mode**: Run tests against real LLM APIs, save request→response
//! pairs as JSON fixture files. Requires API keys.
//!
//! **Replay mode**: Load saved fixtures, create mockito mocks from them.
//! No API keys needed — runs entirely offline, deterministic, free.
//!
//! # Usage
//!
//! ## Recording fixtures
//!
//! ```ignore
//! // Run locally with API key:
//! SHANNON_RECORD_DIR=./tests/fixtures cargo test --test my_test -- --ignored
//! ```
//!
//! ## Replaying fixtures in CI
//!
//! ```rust
//! use shannon_core::testing::record_replay::ReplayHarness;
//!
//! let harness = ReplayHarness::from_dir("./tests/fixtures/my_test");
//! let mut server = mockito::Server::new_async().await;
//! for fixture in &harness.fixtures {
//!     fixture.mount(&mut server);
//! }
//! // Now point SHANNON_BASE_URL to server.url()
//! ```

use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// A single recorded request→response exchange.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RecordedExchange {
    /// Hash of the request body for matching.
    pub request_hash: String,
    /// The provider that was used (anthropic, openai, ollama).
    pub provider: String,
    /// The model that was used.
    pub model: String,
    /// The HTTP request details.
    pub request: RecordedRequest,
    /// The HTTP response details.
    pub response: RecordedResponse,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RecordedRequest {
    /// HTTP method (always POST for LLM APIs).
    pub method: String,
    /// URL path (e.g., "/v1/messages").
    pub path: String,
    /// Request body as raw string.
    pub body: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RecordedResponse {
    /// HTTP status code.
    pub status: u16,
    /// Response headers.
    pub headers: Vec<(String, String)>,
    /// Response body as raw string.
    pub body: String,
}

impl RecordedExchange {
    /// Compute a stable hash from the request body.
    pub fn hash_body(body: &str) -> String {
        let mut hasher = DefaultHasher::new();
        body.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    /// Load a fixture from a JSON file.
    pub fn load(path: &Path) -> Result<Self, String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        serde_json::from_str(&content).map_err(|e| format!("parse {}: {e}", path.display()))
    }

    /// Save a fixture to a JSON file.
    pub fn save(&self, dir: &Path) -> Result<PathBuf, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("create dir {}: {e}", dir.display()))?;
        let filename = format!("{}_{}.json", self.provider, self.request_hash);
        let path = dir.join(&filename);
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| format!("serialize {}: {e}", path.display()))?;
        std::fs::write(&path, content).map_err(|e| format!("write {}: {e}", path.display()))?;
        Ok(path)
    }

    /// Create a mockito mock configuration from this exchange.
    /// Callers use this with their own mockito ServerGuard.
    ///
    /// Example:
    /// ```ignore
    /// let mut server = mockito::Server::new();
    /// let m = server.mock("POST", exchange.request.path.as_str())
    ///     .with_status(exchange.response.status as usize)
    ///     .with_body(&exchange.response.body)
    ///     .create();
    /// ```
    pub fn response_status_usize(&self) -> usize {
        self.response.status as usize
    }
}

/// Harness for loading and replaying recorded fixtures.
pub struct ReplayHarness {
    pub fixtures: Vec<RecordedExchange>,
    pub fixture_dir: PathBuf,
}

impl ReplayHarness {
    /// Load all fixtures from a directory.
    pub fn from_dir(dir: impl AsRef<Path>) -> Self {
        let dir = dir.as_ref().to_path_buf();
        let mut fixtures = Vec::new();

        if dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|ext| ext == "json") {
                        if let Ok(exchange) = RecordedExchange::load(&path) {
                            fixtures.push(exchange);
                        }
                    }
                }
            }
        }

        // Sort by provider + hash for deterministic ordering
        fixtures.sort_by(|a, b| {
            a.provider
                .cmp(&b.provider)
                .then(a.request_hash.cmp(&b.request_hash))
        });

        ReplayHarness {
            fixtures,
            fixture_dir: dir,
        }
    }

    /// Find a fixture matching a request body hash.
    pub fn find_by_hash(&self, hash: &str) -> Option<&RecordedExchange> {
        self.fixtures.iter().find(|f| f.request_hash == hash)
    }

    /// Find a fixture matching a provider name.
    pub fn find_by_provider(&self, provider: &str) -> Option<&RecordedExchange> {
        self.fixtures.iter().find(|f| f.provider == provider)
    }
}

/// Recording session for capturing API exchanges.
pub struct RecordingSession {
    pub fixture_dir: PathBuf,
    pub provider: String,
    pub model: String,
}

impl RecordingSession {
    /// Create a new recording session.
    pub fn new(fixture_dir: impl AsRef<Path>, provider: &str, model: &str) -> Self {
        RecordingSession {
            fixture_dir: fixture_dir.as_ref().to_path_buf(),
            provider: provider.to_string(),
            model: model.to_string(),
        }
    }

    /// Record an API exchange.
    pub fn record(
        &self,
        request_path: &str,
        request_body: &str,
        response_status: u16,
        response_headers: Vec<(String, String)>,
        response_body: &str,
    ) -> Result<PathBuf, String> {
        let exchange = RecordedExchange {
            request_hash: RecordedExchange::hash_body(request_body),
            provider: self.provider.clone(),
            model: self.model.clone(),
            request: RecordedRequest {
                method: "POST".to_string(),
                path: request_path.to_string(),
                body: request_body.to_string(),
            },
            response: RecordedResponse {
                status: response_status,
                headers: response_headers,
                body: response_body.to_string(),
            },
        };
        exchange.save(&self.fixture_dir)
    }

    /// Check if recording is enabled (SHANNON_RECORD_DIR is set).
    pub fn is_recording_enabled() -> Option<PathBuf> {
        std::env::var("SHANNON_RECORD_DIR").ok().map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_body_stability() {
        let body = r#"{"model":"claude-3","messages":[{"role":"user","content":"hello"}]}"#;
        let hash1 = RecordedExchange::hash_body(body);
        let hash2 = RecordedExchange::hash_body(body);
        assert_eq!(hash1, hash2, "Same input should produce same hash");
        assert_eq!(hash1.len(), 16, "Hash should be 16 hex chars");
    }

    #[test]
    fn test_hash_body_uniqueness() {
        let body1 = r#"{"messages":[{"role":"user","content":"hello"}]}"#;
        let body2 = r#"{"messages":[{"role":"user","content":"world"}]}"#;
        let hash1 = RecordedExchange::hash_body(body1);
        let hash2 = RecordedExchange::hash_body(body2);
        assert_ne!(
            hash1, hash2,
            "Different inputs should produce different hashes"
        );
    }

    #[test]
    fn test_fixture_roundtrip() {
        let dir = std::env::temp_dir().join("shannon-rr-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let exchange = RecordedExchange {
            request_hash: RecordedExchange::hash_body(
                r#"{"model":"test","messages":[{"role":"user","content":"hi"}]}"#,
            ),
            provider: "anthropic".to_string(),
            model: "claude-3".to_string(),
            request: RecordedRequest {
                method: "POST".to_string(),
                path: "/v1/messages".to_string(),
                body: r#"{"model":"test","messages":[{"role":"user","content":"hi"}]}"#.to_string(),
            },
            response: RecordedResponse {
                status: 200,
                headers: vec![("content-type".to_string(), "text/event-stream".to_string())],
                body: "data: {\"type\":\"message_start\"}\n\ndata: [DONE]\n\n".to_string(),
            },
        };

        let saved_path = exchange.save(&dir).unwrap();
        assert!(saved_path.exists());

        let loaded = RecordedExchange::load(&saved_path).unwrap();
        assert_eq!(loaded.provider, "anthropic");
        assert_eq!(loaded.response.status, 200);
        assert_eq!(loaded.request.path, "/v1/messages");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_replay_harness_empty_dir() {
        let dir = std::env::temp_dir().join("shannon-rr-empty");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let harness = ReplayHarness::from_dir(&dir);
        assert!(harness.fixtures.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_replay_harness_find() {
        let dir = std::env::temp_dir().join("shannon-rr-find");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let exchange = RecordedExchange {
            request_hash: "abcd1234".to_string(),
            provider: "openai".to_string(),
            model: "gpt-4".to_string(),
            request: RecordedRequest {
                method: "POST".to_string(),
                path: "/v1/chat/completions".to_string(),
                body: "{}".to_string(),
            },
            response: RecordedResponse {
                status: 200,
                headers: vec![],
                body: "data: {}\n\ndata: [DONE]\n\n".to_string(),
            },
        };
        exchange.save(&dir).unwrap();

        let harness = ReplayHarness::from_dir(&dir);
        assert_eq!(harness.fixtures.len(), 1);
        assert!(harness.find_by_hash("abcd1234").is_some());
        assert!(harness.find_by_hash("nonexistent").is_none());
        assert!(harness.find_by_provider("openai").is_some());
        assert!(harness.find_by_provider("anthropic").is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
