//! MCP resource subscription management.
//!
//! Manages active resource subscriptions across connected MCP servers. Tracks
//! which resources are subscribed, their server provenance, and processes
//! `notifications/resources/updated` messages from servers.
//!
//! ## Architecture
//!
//! `ResourceSubscriptionManager` uses a `DashMap` for lock-free concurrent
//! access from the notification handler and public API methods. Each
//! subscription is keyed by its canonical resource URI.

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, warn};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Metadata about an active resource subscription.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionInfo {
    /// Name of the MCP server that owns the resource.
    pub server_name: String,
    /// The resource URI being subscribed to.
    pub resource_uri: String,
    /// When the subscription was established.
    pub subscribed_at: DateTime<Utc>,
    /// When the server last sent an update for this resource.
    pub last_updated: Option<DateTime<Utc>>,
}

/// An update received via `notifications/resources/updated`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceUpdate {
    /// The resource URI that was updated.
    pub resource_uri: String,
    /// Name of the MCP server that sent the notification.
    pub server_name: String,
    /// Optional updated content included in the notification.
    pub content: Option<Value>,
    /// When this update was received.
    pub updated_at: DateTime<Utc>,
}

/// Callback type invoked when a subscribed resource receives an update.
///
/// Receives `(&ResourceUpdate)` so the caller can react (e.g. re-read the
/// resource, update a cache, notify the UI).
pub type ResourceUpdateCallback =
    std::sync::Arc<dyn Fn(&ResourceUpdate) + Send + Sync>;

// ---------------------------------------------------------------------------
// ResourceSubscriptionManager
// ---------------------------------------------------------------------------

/// Manages MCP resource subscriptions across all connected servers.
///
/// Thread-safe via `DashMap`; cheaply cloneable if needed (wrap in `Arc`).
pub struct ResourceSubscriptionManager {
    /// Active subscriptions: resource_uri -> SubscriptionInfo
    subscriptions: DashMap<String, SubscriptionInfo>,
    /// Optional callback invoked on each `notifications/resources/updated`.
    on_update: std::sync::Mutex<Option<ResourceUpdateCallback>>,
}

impl ResourceSubscriptionManager {
    /// Create a new, empty subscription manager.
    pub fn new() -> Self {
        Self {
            subscriptions: DashMap::new(),
            on_update: std::sync::Mutex::new(None),
        }
    }

    /// Set a callback that fires when a subscribed resource is updated.
    pub fn set_on_update(&self, callback: ResourceUpdateCallback) {
        if let Ok(mut guard) = self.on_update.lock() {
            *guard = Some(callback);
        }
    }

    // -----------------------------------------------------------------------
    // Subscribe / Unsubscribe
    // -----------------------------------------------------------------------

    /// Record a new subscription for `resource_uri` on `server_name`.
    ///
    /// Call this **after** the MCP server has accepted the subscribe request.
    /// If a subscription already exists for the URI it is replaced.
    pub fn subscribe(&self, server_name: &str, resource_uri: &str) {
        debug!(
            server = %server_name,
            uri = %resource_uri,
            "Recording resource subscription"
        );
        self.subscriptions.insert(
            resource_uri.to_string(),
            SubscriptionInfo {
                server_name: server_name.to_string(),
                resource_uri: resource_uri.to_string(),
                subscribed_at: Utc::now(),
                last_updated: None,
            },
        );
    }

    /// Remove a subscription. Returns `true` if the subscription existed.
    pub fn unsubscribe(&self, resource_uri: &str) -> bool {
        debug!(uri = %resource_uri, "Removing resource subscription");
        self.subscriptions.remove(resource_uri).is_some()
    }

    /// Remove all subscriptions for a given server (e.g. on disconnect).
    pub fn unsubscribe_all_for_server(&self, server_name: &str) {
        self.subscriptions
            .retain(|_, info| info.server_name != server_name);
    }

    // -----------------------------------------------------------------------
    // Notification handling
    // -----------------------------------------------------------------------

    /// Process an incoming `notifications/resources/updated` notification.
    ///
    /// If the resource is subscribed, updates `last_updated` and fires the
    /// optional `on_update` callback. Ignores notifications for unknown
    /// (unsubscribed) resources.
    ///
    /// `notification` is the raw JSON value of the notification params, which
    /// should contain `uri` and optionally updated content.
    pub fn handle_notification(&self, server_name: &str, notification: &Value) {
        // Extract the URI from the notification params.
        let uri = match notification
            .get("uri")
            .and_then(|v| v.as_str())
        {
            Some(u) => u.to_string(),
            None => {
                warn!(
                    server = %server_name,
                    "Received resources/updated notification without URI, ignoring"
                );
                return;
            }
        };

        let mut updated_info: Option<ResourceUpdate> = None;

        if let Some(mut entry) = self.subscriptions.get_mut(&uri) {
            let now = Utc::now();
            debug!(
                server = %server_name,
                uri = %uri,
                "Processing resource update for subscribed resource"
            );
            entry.last_updated = Some(now);

            updated_info = Some(ResourceUpdate {
                resource_uri: uri.clone(),
                server_name: server_name.to_string(),
                content: notification.get("updated").cloned(),
                updated_at: now,
            });
        } else {
            debug!(
                server = %server_name,
                uri = %uri,
                "Ignoring resource update for unsubscribed resource"
            );
        }

        // Fire the callback outside the DashMap guard to avoid deadlocks.
        if let Some(update) = updated_info {
            if let Ok(guard) = self.on_update.lock() {
                if let Some(ref cb) = *guard {
                    cb(&update);
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    /// Return references to all active subscriptions.
    pub fn list_subscriptions(&self) -> Vec<SubscriptionInfo> {
        self.subscriptions
            .iter()
            .map(|e| e.value().clone())
            .collect()
    }

    /// Check whether a resource URI is currently subscribed.
    pub fn is_subscribed(&self, resource_uri: &str) -> bool {
        self.subscriptions.contains_key(resource_uri)
    }

    /// Return the number of active subscriptions.
    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }

    /// Look up the subscription info for a specific URI.
    pub fn get_subscription(&self, resource_uri: &str) -> Option<SubscriptionInfo> {
        self.subscriptions
            .get(resource_uri)
            .map(|e| e.value().clone())
    }
}

impl Default for ResourceSubscriptionManager {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_subscribe_adds_subscription() {
        let manager = ResourceSubscriptionManager::new();
        assert!(!manager.is_subscribed("file:///readme.md"));

        manager.subscribe("my-server", "file:///readme.md");

        assert!(manager.is_subscribed("file:///readme.md"));
        let info = manager.get_subscription("file:///readme.md").unwrap();
        assert_eq!(info.server_name, "my-server");
        assert_eq!(info.resource_uri, "file:///readme.md");
        assert!(info.last_updated.is_none());
    }

    #[test]
    fn test_unsubscribe_removes_subscription() {
        let manager = ResourceSubscriptionManager::new();
        manager.subscribe("srv", "file:///a");
        assert!(manager.is_subscribed("file:///a"));

        let removed = manager.unsubscribe("file:///a");
        assert!(removed);
        assert!(!manager.is_subscribed("file:///a"));
    }

    #[test]
    fn test_unsubscribe_nonexistent_returns_false() {
        let manager = ResourceSubscriptionManager::new();
        assert!(!manager.unsubscribe("file:///nope"));
    }

    #[test]
    fn test_is_subscribed() {
        let manager = ResourceSubscriptionManager::new();
        manager.subscribe("srv", "file:///x");
        assert!(manager.is_subscribed("file:///x"));
        assert!(!manager.is_subscribed("file:///y"));
    }

    #[test]
    fn test_list_subscriptions() {
        let manager = ResourceSubscriptionManager::new();
        manager.subscribe("srv-a", "file:///a");
        manager.subscribe("srv-b", "file:///b");

        let subs = manager.list_subscriptions();
        assert_eq!(subs.len(), 2);

        let uris: Vec<&str> = subs.iter().map(|s| s.resource_uri.as_str()).collect();
        assert!(uris.contains(&"file:///a"));
        assert!(uris.contains(&"file:///b"));
    }

    #[test]
    fn test_subscription_count() {
        let manager = ResourceSubscriptionManager::new();
        assert_eq!(manager.subscription_count(), 0);
        manager.subscribe("srv", "file:///a");
        assert_eq!(manager.subscription_count(), 1);
        manager.subscribe("srv", "file:///b");
        assert_eq!(manager.subscription_count(), 2);
        manager.unsubscribe("file:///a");
        assert_eq!(manager.subscription_count(), 1);
    }

    #[test]
    fn test_handle_notification_updates_timestamp() {
        let manager = ResourceSubscriptionManager::new();
        manager.subscribe("my-server", "file:///readme.md");

        // Before notification, last_updated is None.
        let info = manager.get_subscription("file:///readme.md").unwrap();
        assert!(info.last_updated.is_none());

        // Send a notification.
        let notification = serde_json::json!({
            "uri": "file:///readme.md"
        });
        manager.handle_notification("my-server", &notification);

        // After notification, last_updated is set.
        let info = manager.get_subscription("file:///readme.md").unwrap();
        assert!(info.last_updated.is_some());
    }

    #[test]
    fn test_handle_notification_ignores_unknown_resource() {
        let manager = ResourceSubscriptionManager::new();
        manager.subscribe("srv", "file:///known");

        let notification = serde_json::json!({
            "uri": "file:///unknown"
        });
        // Should not panic or create a new subscription.
        manager.handle_notification("srv", &notification);

        assert!(manager.is_subscribed("file:///known"));
        assert!(!manager.is_subscribed("file:///unknown"));
        assert_eq!(manager.subscription_count(), 1);
    }

    #[test]
    fn test_handle_notification_without_uri_is_ignored() {
        let manager = ResourceSubscriptionManager::new();
        manager.subscribe("srv", "file:///x");

        let notification = serde_json::json!({"noUri": true});
        manager.handle_notification("srv", &notification);

        // Existing subscription untouched, no new entries.
        assert_eq!(manager.subscription_count(), 1);
        let info = manager.get_subscription("file:///x").unwrap();
        assert!(info.last_updated.is_none());
    }

    #[test]
    fn test_handle_notification_fires_callback() {
        let manager = ResourceSubscriptionManager::new();
        let updates: Arc<Mutex<Vec<ResourceUpdate>>> = Arc::new(Mutex::new(Vec::new()));
        let updates_clone = updates.clone();

        manager.set_on_update(Arc::new(move |update| {
            updates_clone.lock().unwrap().push(update.clone());
        }));

        manager.subscribe("srv", "file:///data");

        let notification = serde_json::json!({
            "uri": "file:///data",
            "updated": {"key": "value"}
        });
        manager.handle_notification("srv", &notification);

        let received = updates.lock().unwrap();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].resource_uri, "file:///data");
        assert_eq!(received[0].server_name, "srv");
        assert!(received[0].content.is_some());
    }

    #[test]
    fn test_callback_not_fired_for_unsubscribed() {
        let manager = ResourceSubscriptionManager::new();
        let updates: Arc<Mutex<Vec<ResourceUpdate>>> = Arc::new(Mutex::new(Vec::new()));
        let updates_clone = updates.clone();

        manager.set_on_update(Arc::new(move |update| {
            updates_clone.lock().unwrap().push(update.clone());
        }));

        // No subscription for this URI.
        let notification = serde_json::json!({"uri": "file:///unknown"});
        manager.handle_notification("srv", &notification);

        let received = updates.lock().unwrap();
        assert!(received.is_empty());
    }

    #[test]
    fn test_unsubscribe_all_for_server() {
        let manager = ResourceSubscriptionManager::new();
        manager.subscribe("srv-a", "file:///a1");
        manager.subscribe("srv-a", "file:///a2");
        manager.subscribe("srv-b", "file:///b1");

        manager.unsubscribe_all_for_server("srv-a");

        assert!(!manager.is_subscribed("file:///a1"));
        assert!(!manager.is_subscribed("file:///a2"));
        assert!(manager.is_subscribed("file:///b1"));
    }

    #[test]
    fn test_subscribe_replaces_existing() {
        let manager = ResourceSubscriptionManager::new();
        manager.subscribe("srv-a", "file:///x");
        let first_at = manager.get_subscription("file:///x").unwrap().subscribed_at;

        // Small sleep to get a different timestamp.
        std::thread::sleep(std::time::Duration::from_millis(10));

        manager.subscribe("srv-b", "file:///x");
        let info = manager.get_subscription("file:///x").unwrap();
        assert_eq!(info.server_name, "srv-b");
        assert!(info.subscribed_at > first_at);
    }

    #[test]
    fn test_get_subscription_nonexistent() {
        let manager = ResourceSubscriptionManager::new();
        assert!(manager.get_subscription("file:///nope").is_none());
    }

    #[test]
    fn test_default_trait() {
        let manager = ResourceSubscriptionManager::default();
        assert_eq!(manager.subscription_count(), 0);
    }

    #[test]
    fn test_subscription_info_serialization() {
        let info = SubscriptionInfo {
            server_name: "srv".to_string(),
            resource_uri: "file:///x".to_string(),
            subscribed_at: Utc::now(),
            last_updated: Some(Utc::now()),
        };
        let json = serde_json::to_string(&info).unwrap();
        let parsed: SubscriptionInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.server_name, "srv");
        assert_eq!(parsed.resource_uri, "file:///x");
    }

    #[test]
    fn test_resource_update_serialization() {
        let update = ResourceUpdate {
            resource_uri: "file:///x".to_string(),
            server_name: "srv".to_string(),
            content: Some(serde_json::json!({"key": "val"})),
            updated_at: Utc::now(),
        };
        let json = serde_json::to_string(&update).unwrap();
        let parsed: ResourceUpdate = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.resource_uri, "file:///x");
        assert!(parsed.content.is_some());
    }

    #[test]
    fn test_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ResourceSubscriptionManager>();
        assert_send_sync::<SubscriptionInfo>();
        assert_send_sync::<ResourceUpdate>();
    }
}
