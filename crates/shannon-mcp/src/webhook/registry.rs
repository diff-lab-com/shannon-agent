use dashmap::DashMap;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::path::Path;

use super::types::{McpEventType, WebhookConfig};

type HmacSha256 = Hmac<Sha256>;

/// Registry of webhook endpoints, keyed by ID.
///
/// Thread-safe via `DashMap`. Supports registration, unregistration,
/// event-type filtering, persistence, and HMAC signing.
pub struct WebhookRegistry {
    webhooks: DashMap<String, WebhookConfig>,
}

impl WebhookRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            webhooks: DashMap::new(),
        }
    }

    /// Register a new webhook. Returns the generated ID.
    pub fn register(&self, config: WebhookConfig) -> String {
        let id = format!("wh_{}", uuid::Uuid::new_v4().as_simple());
        self.webhooks.insert(id.clone(), config);
        id
    }

    /// Register a webhook with a specific ID (for loading from persistence).
    pub fn register_with_id(&self, id: String, config: WebhookConfig) {
        self.webhooks.insert(id, config);
    }

    /// Remove a webhook by ID. Returns the config if found.
    pub fn unregister(&self, id: &str) -> Option<WebhookConfig> {
        self.webhooks.remove(id).map(|(_, v)| v)
    }

    /// List all registered webhook configs with their IDs.
    pub fn list(&self) -> Vec<(String, WebhookConfig)> {
        self.webhooks
            .iter()
            .map(|e| (e.key().clone(), e.value().clone()))
            .collect()
    }

    /// Get webhooks that should receive the given event type.
    pub fn get_matching(&self, event_type: &McpEventType) -> Vec<(String, WebhookConfig)> {
        self.webhooks
            .iter()
            .filter(|e| e.value().matches_event(event_type))
            .map(|e| (e.key().clone(), e.value().clone()))
            .collect()
    }

    /// Get a webhook by ID.
    pub fn get(&self, id: &str) -> Option<WebhookConfig> {
        self.webhooks.get(id).map(|v| v.value().clone())
    }

    /// Number of registered webhooks.
    pub fn len(&self) -> usize {
        self.webhooks.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.webhooks.is_empty()
    }

    /// Sign a payload with HMAC-SHA256 using the given secret.
    ///
    /// Returns the hex-encoded signature.
    pub fn sign_payload(secret: &str, payload: &[u8]) -> String {
        let mut mac =
            HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
        mac.update(payload);
        let result = mac.finalize();
        let code_bytes = result.into_bytes();
        format!("{code_bytes:x}")
    }

    /// Persist all webhooks to a JSON file.
    pub fn persist(&self, path: &Path) -> Result<(), std::io::Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let entries: Vec<(String, WebhookConfig)> = self.list();
        let json = serde_json::to_string_pretty(&entries)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load webhooks from a JSON file, replacing current contents.
    pub fn load(&self, path: &Path) -> Result<(), std::io::Error> {
        let data = std::fs::read_to_string(path)?;
        let entries: Vec<(String, WebhookConfig)> = serde_json::from_str(&data)?;
        self.webhooks.clear();
        for (id, config) in entries {
            self.webhooks.insert(id, config);
        }
        Ok(())
    }
}

impl Default for WebhookRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_config(url: &str) -> WebhookConfig {
        WebhookConfig::new(url.to_string(), "test_secret".to_string())
    }

    #[test]
    fn register_returns_id_with_prefix() {
        let registry = WebhookRegistry::new();
        let id = registry.register(sample_config("https://example.com"));
        assert!(id.starts_with("wh_"));
    }

    #[test]
    fn unregister_removes_webhook() {
        let registry = WebhookRegistry::new();
        let id = registry.register(sample_config("https://example.com"));
        let removed = registry.unregister(&id);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().url, "https://example.com");
        assert!(registry.is_empty());
    }

    #[test]
    fn unregister_nonexistent_returns_none() {
        let registry = WebhookRegistry::new();
        assert!(registry.unregister("wh_nonexistent").is_none());
    }

    #[test]
    fn list_returns_all_webhooks() {
        let registry = WebhookRegistry::new();
        registry.register(sample_config("https://a.com"));
        registry.register(sample_config("https://b.com"));
        let list = registry.list();
        assert_eq!(list.len(), 2);
        let urls: Vec<&str> = list.iter().map(|(_, c)| c.url.as_str()).collect();
        assert!(urls.contains(&"https://a.com"));
        assert!(urls.contains(&"https://b.com"));
    }

    #[test]
    fn get_matching_filters_by_event_type() {
        let registry = WebhookRegistry::new();
        let config = WebhookConfig::with_event_types(
            "https://example.com".to_string(),
            "secret".to_string(),
            vec![McpEventType::ToolCallStarted],
        );
        registry.register(config);

        let matching = registry.get_matching(&McpEventType::ToolCallStarted);
        assert_eq!(matching.len(), 1);

        let not_matching = registry.get_matching(&McpEventType::ServerConnected);
        assert!(not_matching.is_empty());
    }

    #[test]
    fn get_matching_empty_event_types_receives_all() {
        let registry = WebhookRegistry::new();
        registry.register(sample_config("https://example.com"));

        assert_eq!(
            registry.get_matching(&McpEventType::ToolCallStarted).len(),
            1
        );
        assert_eq!(
            registry.get_matching(&McpEventType::ServerConnected).len(),
            1
        );
    }

    #[test]
    fn sign_payload_produces_deterministic_hex() {
        let sig1 = WebhookRegistry::sign_payload("secret", b"hello world");
        let sig2 = WebhookRegistry::sign_payload("secret", b"hello world");
        assert_eq!(sig1, sig2);
        // HMAC-SHA256 produces 32 bytes = 64 hex chars
        assert_eq!(sig1.len(), 64);
    }

    #[test]
    fn sign_payload_differs_with_different_secret() {
        let sig1 = WebhookRegistry::sign_payload("secret1", b"hello");
        let sig2 = WebhookRegistry::sign_payload("secret2", b"hello");
        assert_ne!(sig1, sig2);
    }

    #[test]
    fn persist_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("webhooks.json");

        let registry = WebhookRegistry::new();
        let config = WebhookConfig::with_event_types(
            "https://example.com".to_string(),
            "my_secret".to_string(),
            vec![McpEventType::ServerConnected],
        );
        let id = registry.register(config);
        registry.persist(&path).unwrap();

        let loaded = WebhookRegistry::new();
        loaded.load(&path).unwrap();
        assert_eq!(loaded.len(), 1);

        let retrieved = loaded.get(&id).unwrap();
        assert_eq!(retrieved.url, "https://example.com");
        assert_eq!(retrieved.secret, "my_secret");
        assert_eq!(retrieved.event_types, vec![McpEventType::ServerConnected]);
    }

    #[test]
    fn persist_creates_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("dir").join("webhooks.json");
        let registry = WebhookRegistry::new();
        registry.register(sample_config("https://example.com"));
        assert!(registry.persist(&path).is_ok());
        assert!(path.exists());
    }

    #[test]
    fn load_nonexistent_file_errors() {
        let registry = WebhookRegistry::new();
        assert!(registry.load(Path::new("/nonexistent/path.json")).is_err());
    }

    #[test]
    fn get_by_id() {
        let registry = WebhookRegistry::new();
        let id = registry.register(sample_config("https://example.com"));
        let config = registry.get(&id).unwrap();
        assert_eq!(config.url, "https://example.com");
    }

    #[test]
    fn get_nonexistent_returns_none() {
        let registry = WebhookRegistry::new();
        assert!(registry.get("wh_nonexistent").is_none());
    }

    #[test]
    fn register_with_id() {
        let registry = WebhookRegistry::new();
        registry.register_with_id(
            "wh_custom".to_string(),
            sample_config("https://example.com"),
        );
        assert_eq!(registry.len(), 1);
        let config = registry.get("wh_custom").unwrap();
        assert_eq!(config.url, "https://example.com");
    }

    #[test]
    fn len_and_is_empty() {
        let registry = WebhookRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
        registry.register(sample_config("https://example.com"));
        assert!(!registry.is_empty());
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn load_replaces_existing_contents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("webhooks.json");

        let registry = WebhookRegistry::new();
        registry.register(sample_config("https://old.com"));
        registry.persist(&path).unwrap();

        // Load into a registry that already has data
        let loaded = WebhookRegistry::new();
        loaded.register(sample_config("https://preexisting.com"));
        loaded.load(&path).unwrap();
        // preexisting entry should be gone
        let list = loaded.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].1.url, "https://old.com");
    }

    #[test]
    fn send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<WebhookRegistry>();
    }
}
