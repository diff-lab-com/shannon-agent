use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Types of events that can be emitted by the MCP system.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum McpEventType {
    /// A tool call has started execution.
    ToolCallStarted,
    /// A tool call has completed (success or failure).
    ToolCallCompleted,
    /// An MCP server has connected and initialized successfully.
    ServerConnected,
    /// An MCP server has disconnected or failed.
    ServerDisconnected,
    /// An MCP notification was received from a server.
    NotificationReceived,
}

/// An event emitted by the MCP system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpEvent {
    /// The type of event.
    pub event_type: McpEventType,
    /// Name of the MCP server that generated the event.
    pub server_name: String,
    /// Name of the tool involved (for tool events).
    pub tool_name: Option<String>,
    /// Event-specific payload data.
    pub payload: Value,
    /// When this event was created.
    pub timestamp: DateTime<Utc>,
}

impl McpEvent {
    /// Create a new event with the current timestamp.
    pub fn new(
        event_type: McpEventType,
        server_name: String,
        tool_name: Option<String>,
        payload: Value,
    ) -> Self {
        Self {
            event_type,
            server_name,
            tool_name,
            payload,
            timestamp: Utc::now(),
        }
    }
}

/// Configuration for a single webhook endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    /// The URL to deliver events to (HTTPS recommended).
    pub url: String,
    /// HMAC-SHA256 secret for signing payloads.
    pub secret: String,
    /// Event types this webhook subscribes to. Empty = all events.
    pub event_types: Vec<McpEventType>,
    /// Whether this webhook is active.
    pub enabled: bool,
}

impl WebhookConfig {
    /// Create a new webhook config that receives all event types.
    pub fn new(url: String, secret: String) -> Self {
        Self {
            url,
            secret,
            event_types: Vec::new(),
            enabled: true,
        }
    }

    /// Create a webhook config filtered to specific event types.
    pub fn with_event_types(url: String, secret: String, event_types: Vec<McpEventType>) -> Self {
        Self {
            url,
            secret,
            event_types,
            enabled: true,
        }
    }

    /// Check if this webhook should receive the given event type.
    pub fn matches_event(&self, event_type: &McpEventType) -> bool {
        self.enabled && (self.event_types.is_empty() || self.event_types.contains(event_type))
    }
}

/// Record of a single webhook delivery attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookDelivery {
    /// Unique delivery ID.
    pub id: String,
    /// The webhook URL that was targeted.
    pub webhook_url: String,
    /// The event being delivered.
    pub event: McpEvent,
    /// Delivery status: "pending", "success", "failed".
    pub status: String,
    /// Number of delivery attempts so far.
    pub attempts: u32,
    /// Timestamp of the last delivery attempt.
    pub last_attempt: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_type_serde_roundtrip() {
        let types = vec![
            McpEventType::ToolCallStarted,
            McpEventType::ToolCallCompleted,
            McpEventType::ServerConnected,
            McpEventType::ServerDisconnected,
            McpEventType::NotificationReceived,
        ];
        let json = serde_json::to_string(&types).unwrap();
        let de: Vec<McpEventType> = serde_json::from_str(&json).unwrap();
        assert_eq!(de, types);
    }

    #[test]
    fn event_type_snake_case_serialization() {
        let json = serde_json::to_string(&McpEventType::ToolCallStarted).unwrap();
        assert_eq!(json, "\"tool_call_started\"");
    }

    #[test]
    fn mcp_event_new_has_timestamp() {
        let event = McpEvent::new(
            McpEventType::ServerConnected,
            "test".to_string(),
            None,
            serde_json::json!({}),
        );
        assert_eq!(event.event_type, McpEventType::ServerConnected);
        assert_eq!(event.server_name, "test");
        assert!(event.tool_name.is_none());
    }

    #[test]
    fn webhook_config_new_receives_all_events() {
        let config =
            WebhookConfig::new("https://example.com/hook".to_string(), "secret".to_string());
        assert!(config.matches_event(&McpEventType::ToolCallStarted));
        assert!(config.matches_event(&McpEventType::ServerConnected));
        assert!(config.enabled);
        assert!(config.event_types.is_empty());
    }

    #[test]
    fn webhook_config_filtered_events() {
        let config = WebhookConfig::with_event_types(
            "https://example.com/hook".to_string(),
            "secret".to_string(),
            vec![
                McpEventType::ToolCallStarted,
                McpEventType::ToolCallCompleted,
            ],
        );
        assert!(config.matches_event(&McpEventType::ToolCallStarted));
        assert!(config.matches_event(&McpEventType::ToolCallCompleted));
        assert!(!config.matches_event(&McpEventType::ServerConnected));
    }

    #[test]
    fn webhook_config_disabled_matches_nothing() {
        let mut config =
            WebhookConfig::new("https://example.com/hook".to_string(), "secret".to_string());
        config.enabled = false;
        assert!(!config.matches_event(&McpEventType::ToolCallStarted));
    }

    #[test]
    fn webhook_config_serialization_roundtrip() {
        let config = WebhookConfig::with_event_types(
            "https://example.com/hook".to_string(),
            "my_secret".to_string(),
            vec![McpEventType::ServerConnected],
        );
        let json = serde_json::to_string(&config).unwrap();
        let de: WebhookConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(de.url, config.url);
        assert_eq!(de.secret, config.secret);
        assert_eq!(de.event_types, config.event_types);
        assert!(de.enabled);
    }

    #[test]
    fn webhook_delivery_serialization() {
        let delivery = WebhookDelivery {
            id: "del_123".to_string(),
            webhook_url: "https://example.com/hook".to_string(),
            event: McpEvent::new(
                McpEventType::ServerConnected,
                "test".to_string(),
                None,
                serde_json::json!({"status": "ok"}),
            ),
            status: "success".to_string(),
            attempts: 1,
            last_attempt: None,
        };
        let json = serde_json::to_string(&delivery).unwrap();
        let de: WebhookDelivery = serde_json::from_str(&json).unwrap();
        assert_eq!(de.id, "del_123");
        assert_eq!(de.status, "success");
        assert_eq!(de.attempts, 1);
    }

    #[test]
    fn send_sync_types() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<McpEventType>();
        assert_send_sync::<McpEvent>();
        assert_send_sync::<WebhookConfig>();
        assert_send_sync::<WebhookDelivery>();
    }
}
