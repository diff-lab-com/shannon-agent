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
//! ```ignore
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
    /// Tokens written to the prompt cache (Anthropic-specific).
    #[serde(default)]
    pub cache_creation_input_tokens: u32,
    /// Tokens read from the prompt cache (Anthropic-specific).
    #[serde(default)]
    pub cache_read_input_tokens: u32,
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

    /// Header names that should be stripped from recordings to avoid leaking secrets.
    const SENSITIVE_HEADERS: &'static [&'static str] = &[
        "authorization",
        "x-api-key",
        "api-key",
        "cookie",
        "set-cookie",
        "anthropic-api-key", // Alias used by some SDKs
    ];

    /// Extract cache metrics from the response body JSON.
    /// Looks for Anthropic-style `usage.cache_creation_input_tokens` and
    /// `usage.cache_read_input_tokens`, or OpenAI-style equivalents in SSE data lines.
    pub fn extract_cache_metrics(response_body: &str) -> (u32, u32) {
        // Try parsing as a single JSON object first (non-streaming response)
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(response_body) {
            if let Some(usage) = v.get("usage") {
                let created = usage
                    .get("cache_creation_input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                let read = usage
                    .get("cache_read_input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                return (created, read);
            }
        }

        // For streaming responses, scan SSE data lines for message_start/message_delta
        let mut created: u32 = 0;
        let mut read: u32 = 0;
        for line in response_body.lines() {
            let trimmed = line.trim();
            if !trimmed.starts_with("data: ") {
                continue;
            }
            let data = &trimmed[6..];
            if data == "[DONE]" {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                if let Some(usage) = v
                    .get("message")
                    .and_then(|m| m.get("usage"))
                    .or_else(|| v.get("usage"))
                {
                    created = usage
                        .get("cache_creation_input_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32;
                    read = usage
                        .get("cache_read_input_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32;
                }
            }
        }
        (created, read)
    }

    /// Return a copy with sensitive headers redacted.
    pub fn strip_secrets(mut self) -> Self {
        self.response.headers = self
            .response
            .headers
            .into_iter()
            .map(|(name, value)| {
                let lower = name.to_lowercase();
                let is_sensitive = Self::SENSITIVE_HEADERS.iter().any(|h| *h == lower)
                    || lower.contains("token")
                    || lower.contains("secret");
                if is_sensitive {
                    (name, "***REDACTED***".to_string())
                } else {
                    (name, value)
                }
            })
            .collect();
        self
    }

    /// Append this exchange as a single JSON line to a JSONL file.
    /// Creates the file (and parent directories) if they don't exist.
    pub fn save_jsonl(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create dir {}: {e}", parent.display()))?;
        }
        let mut line =
            serde_json::to_string(self).map_err(|e| format!("serialize for jsonl: {e}"))?;
        line.push('\n');
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| format!("open jsonl {}: {e}", path.display()))?;
        file.write_all(line.as_bytes())
            .map_err(|e| format!("write jsonl {}: {e}", path.display()))?;
        Ok(())
    }

    /// Load all exchanges from a JSONL file (one JSON object per line).
    pub fn load_jsonl(path: &Path) -> Result<Vec<Self>, String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        content
            .lines()
            .filter(|l| !l.trim().is_empty())
            .enumerate()
            .map(|(i, line)| {
                serde_json::from_str(line)
                    .map_err(|e| format!("parse {} line {}: {e}", path.display(), i + 1))
            })
            .collect()
    }
}

/// Harness for loading and replaying recorded fixtures.
pub struct ReplayHarness {
    pub fixtures: Vec<RecordedExchange>,
    pub fixture_dir: PathBuf,
}

/// Aggregate cache statistics from a set of recorded exchanges.
#[derive(Debug, Default)]
pub struct CacheStats {
    pub total_exchanges: usize,
    pub exchanges_with_cache_hit: usize,
    pub total_cache_creation_tokens: u64,
    pub total_cache_read_tokens: u64,
}

impl CacheStats {
    /// Compute stats from a slice of recorded exchanges.
    pub fn from_exchanges(exchanges: &[RecordedExchange]) -> Self {
        let mut stats = CacheStats {
            total_exchanges: exchanges.len(),
            ..CacheStats::default()
        };
        for ex in exchanges {
            stats.total_cache_creation_tokens += ex.cache_creation_input_tokens as u64;
            stats.total_cache_read_tokens += ex.cache_read_input_tokens as u64;
            if ex.cache_read_input_tokens > 0 {
                stats.exchanges_with_cache_hit += 1;
            }
        }
        stats
    }

    /// Cache hit rate as a fraction (0.0–1.0).
    pub fn hit_rate(&self) -> f64 {
        if self.total_exchanges == 0 {
            return 0.0;
        }
        self.exchanges_with_cache_hit as f64 / self.total_exchanges as f64
    }

    /// Print a human-readable summary.
    pub fn print_summary(&self) {
        println!("Cache Statistics");
        println!("================");
        println!("Exchanges:           {}", self.total_exchanges);
        println!(
            "Cache hits:          {} ({:.1}%)",
            self.exchanges_with_cache_hit,
            self.hit_rate() * 100.0
        );
        println!("Tokens written:      {}", self.total_cache_creation_tokens);
        println!("Tokens read (saved): {}", self.total_cache_read_tokens);
    }
}

impl ReplayHarness {
    /// Load all fixtures from a directory (supports both `.json` and `.jsonl` files).
    pub fn from_dir(dir: impl AsRef<Path>) -> Self {
        let dir = dir.as_ref().to_path_buf();
        let mut fixtures = Vec::new();

        if dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                    match ext {
                        "json" => {
                            if let Ok(exchange) = RecordedExchange::load(&path) {
                                fixtures.push(exchange);
                            }
                        }
                        "jsonl" => {
                            if let Ok(exchanges) = RecordedExchange::load_jsonl(&path) {
                                fixtures.extend(exchanges);
                            }
                        }
                        _ => {}
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
    /// Optional session name for JSONL mode. When set, exchanges are appended
    /// to `{fixture_dir}/{session_name}.jsonl` instead of individual files.
    pub session_name: Option<String>,
}

impl RecordingSession {
    /// Create a new recording session.
    pub fn new(fixture_dir: impl AsRef<Path>, provider: &str, model: &str) -> Self {
        RecordingSession {
            fixture_dir: fixture_dir.as_ref().to_path_buf(),
            provider: provider.to_string(),
            model: model.to_string(),
            session_name: None,
        }
    }

    /// Create a session that writes to a JSONL file.
    pub fn new_jsonl(
        fixture_dir: impl AsRef<Path>,
        provider: &str,
        model: &str,
        session_name: &str,
    ) -> Self {
        RecordingSession {
            fixture_dir: fixture_dir.as_ref().to_path_buf(),
            provider: provider.to_string(),
            model: model.to_string(),
            session_name: Some(session_name.to_string()),
        }
    }

    /// Create from environment variables. Uses JSONL mode when
    /// `SHANNON_RECORD_SESSION` is set, otherwise falls back to per-file mode.
    pub fn from_env() -> Option<Self> {
        let dir = std::env::var("SHANNON_RECORD_DIR")
            .ok()
            .map(PathBuf::from)?;
        let provider = std::env::var("SHANNON_PROVIDER")
            .or_else(|_| std::env::var("ANTHROPIC_PROVIDER"))
            .unwrap_or_else(|_| "anthropic".to_string());
        let model = std::env::var("SHANNON_MODEL")
            .or_else(|_| std::env::var("ANTHROPIC_MODEL"))
            .unwrap_or_else(|_| "unknown".to_string());
        let session_name = std::env::var("SHANNON_RECORD_SESSION").ok();
        Some(RecordingSession {
            fixture_dir: dir,
            provider,
            model,
            session_name,
        })
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
        let (cache_creation, cache_read) = RecordedExchange::extract_cache_metrics(response_body);
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
            cache_creation_input_tokens: cache_creation,
            cache_read_input_tokens: cache_read,
        }
        .strip_secrets();

        if let Some(ref name) = self.session_name {
            let path = self.fixture_dir.join(format!("{name}.jsonl"));
            exchange.save_jsonl(&path)?;
            Ok(path)
        } else {
            exchange.save(&self.fixture_dir)
        }
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
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
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
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
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

    #[test]
    fn test_jsonl_roundtrip() {
        let dir = std::env::temp_dir().join("shannon-rr-jsonl");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.jsonl");

        let exchange1 = RecordedExchange {
            request_hash: "hash1111".to_string(),
            provider: "anthropic".to_string(),
            model: "glm-5.1".to_string(),
            request: RecordedRequest {
                method: "POST".to_string(),
                path: "/v1/messages".to_string(),
                body: r#"{"model":"glm-5.1"}"#.to_string(),
            },
            response: RecordedResponse {
                status: 200,
                headers: vec![("content-type".to_string(), "text/event-stream".to_string())],
                body: "data: {\"type\":\"message_start\"}\n\n".to_string(),
            },
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
        };

        let exchange2 = RecordedExchange {
            request_hash: "hash2222".to_string(),
            provider: "anthropic".to_string(),
            model: "glm-5.1".to_string(),
            request: RecordedRequest {
                method: "POST".to_string(),
                path: "/v1/messages".to_string(),
                body: r#"{"model":"glm-5.1","messages":[...]}"#.to_string(),
            },
            response: RecordedResponse {
                status: 200,
                headers: vec![],
                body: "data: {\"type\":\"content_block_delta\"}\n\n".to_string(),
            },
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
        };

        exchange1.save_jsonl(&path).unwrap();
        exchange2.save_jsonl(&path).unwrap();

        let loaded = RecordedExchange::load_jsonl(&path).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].request_hash, "hash1111");
        assert_eq!(loaded[1].request_hash, "hash2222");
        assert_eq!(loaded[0].model, "glm-5.1");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_strip_secrets() {
        let exchange = RecordedExchange {
            request_hash: "test".to_string(),
            provider: "anthropic".to_string(),
            model: "test".to_string(),
            request: RecordedRequest {
                method: "POST".to_string(),
                path: "/v1/messages".to_string(),
                body: "{}".to_string(),
            },
            response: RecordedResponse {
                status: 200,
                headers: vec![
                    ("content-type".to_string(), "text/event-stream".to_string()),
                    (
                        "authorization".to_string(),
                        "Bearer sk-secret-key".to_string(),
                    ),
                    ("x-api-key".to_string(), "my-api-key".to_string()),
                    ("x-request-id".to_string(), "req-123".to_string()),
                    ("x-token-refresh".to_string(), "token-value".to_string()),
                ],
                body: "{}".to_string(),
            },
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
        };

        let stripped = exchange.strip_secrets();

        // Safe headers preserved
        assert_eq!(
            stripped.response.headers[0],
            ("content-type".to_string(), "text/event-stream".to_string())
        );
        assert_eq!(
            stripped.response.headers[3],
            ("x-request-id".to_string(), "req-123".to_string())
        );

        // Sensitive headers redacted
        assert_eq!(
            stripped.response.headers[1],
            ("authorization".to_string(), "***REDACTED***".to_string())
        );
        assert_eq!(
            stripped.response.headers[2],
            ("x-api-key".to_string(), "***REDACTED***".to_string())
        );
        // "token" in header name triggers redaction
        assert_eq!(
            stripped.response.headers[4],
            ("x-token-refresh".to_string(), "***REDACTED***".to_string())
        );
    }

    #[test]
    fn test_replay_harness_loads_jsonl() {
        let dir = std::env::temp_dir().join("shannon-rr-jsonl-harness");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let exchange = RecordedExchange {
            request_hash: "jsonl_test".to_string(),
            provider: "anthropic".to_string(),
            model: "glm-5.1".to_string(),
            request: RecordedRequest {
                method: "POST".to_string(),
                path: "/v1/messages".to_string(),
                body: "{}".to_string(),
            },
            response: RecordedResponse {
                status: 200,
                headers: vec![],
                body: "data: {}\n\n".to_string(),
            },
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
        };

        exchange
            .save_jsonl(&dir.join("test_session.jsonl"))
            .unwrap();

        let harness = ReplayHarness::from_dir(&dir);
        assert_eq!(harness.fixtures.len(), 1);
        assert_eq!(harness.fixtures[0].request_hash, "jsonl_test");
        assert_eq!(harness.fixtures[0].model, "glm-5.1");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_session_from_env_fallback() {
        // Without SHANNON_RECORD_DIR set, returns None.
        // We can't safely remove env vars in edition 2024, so just verify
        // the method exists and handles the "not set" case.
        // In practice, CI won't have SHANNON_RECORD_DIR set.
        if std::env::var("SHANNON_RECORD_DIR").is_err() {
            assert!(RecordingSession::from_env().is_none());
        }
    }

    #[test]
    fn test_extract_cache_metrics_non_streaming() {
        let body = r#"{"id":"msg_1","usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":200,"cache_read_input_tokens":300}}"#;
        let (created, read) = RecordedExchange::extract_cache_metrics(body);
        assert_eq!(created, 200);
        assert_eq!(read, 300);
    }

    #[test]
    fn test_extract_cache_metrics_streaming() {
        let body = "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":10,\"cache_creation_input_tokens\":0,\"cache_read_input_tokens\":0}}}\n\ndata: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":50,\"cache_creation_input_tokens\":500,\"cache_read_input_tokens\":1000}}\n\ndata: [DONE]\n\n";
        let (created, read) = RecordedExchange::extract_cache_metrics(body);
        assert_eq!(created, 500);
        assert_eq!(read, 1000);
    }

    #[test]
    fn test_extract_cache_metrics_no_usage() {
        let body = r#"{"id":"msg_1","content":"hello"}"#;
        let (created, read) = RecordedExchange::extract_cache_metrics(body);
        assert_eq!(created, 0);
        assert_eq!(read, 0);
    }

    #[test]
    fn test_cache_stats() {
        let exchanges = vec![
            RecordedExchange {
                request_hash: "h1".to_string(),
                provider: "anthropic".to_string(),
                model: "test".to_string(),
                request: RecordedRequest {
                    method: "POST".to_string(),
                    path: "/v1/messages".to_string(),
                    body: "{}".to_string(),
                },
                response: RecordedResponse {
                    status: 200,
                    headers: vec![],
                    body: "{}".to_string(),
                },
                cache_creation_input_tokens: 100,
                cache_read_input_tokens: 200,
            },
            RecordedExchange {
                request_hash: "h2".to_string(),
                provider: "anthropic".to_string(),
                model: "test".to_string(),
                request: RecordedRequest {
                    method: "POST".to_string(),
                    path: "/v1/messages".to_string(),
                    body: "{}".to_string(),
                },
                response: RecordedResponse {
                    status: 200,
                    headers: vec![],
                    body: "{}".to_string(),
                },
                cache_creation_input_tokens: 50,
                cache_read_input_tokens: 0,
            },
        ];
        let stats = CacheStats::from_exchanges(&exchanges);
        assert_eq!(stats.total_exchanges, 2);
        assert_eq!(stats.exchanges_with_cache_hit, 1);
        assert_eq!(stats.total_cache_creation_tokens, 150);
        assert_eq!(stats.total_cache_read_tokens, 200);
        assert!((stats.hit_rate() - 0.5).abs() < 0.001);
    }
}
