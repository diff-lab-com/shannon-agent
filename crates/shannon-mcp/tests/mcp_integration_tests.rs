//! Additional integration tests for shannon-mcp.
//!
//! Tests cover:
//! - ResourceSubscriptionManager lifecycle and concurrent access
//! - WebhookRegistry HMAC-SHA256 signing, persistence, and event filtering
//! - Protocol types: ResourcesUpdatedNotification, subscribe/unsubscribe roundtrips
//! - Sampling and elicitation types
//! - Progress notification and roots

use shannon_mcp::protocol::*;
use shannon_mcp::resource_subscription::ResourceSubscriptionManager;
use shannon_mcp::webhook::{
    McpEvent, McpEventType, WebhookConfig, WebhookDelivery, WebhookRegistry,
};
use std::sync::{Arc, Mutex};

// =========================================================================
// 1. ResourceSubscriptionManager Integration
// =========================================================================

mod resource_subscription_integration {
    use super::*;

    #[test]
    fn test_concurrent_subscribe_unsubscribe() {
        let manager = Arc::new(ResourceSubscriptionManager::new());
        let mut handles = Vec::new();

        // Spawn 10 threads each subscribing to a unique URI
        for i in 0..10 {
            let mgr = Arc::clone(&manager);
            handles.push(std::thread::spawn(move || {
                let uri = format!("file:///resource/{i}");
                mgr.subscribe("srv-concurrent", &uri);
                assert!(mgr.is_subscribed(&uri));
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(manager.subscription_count(), 10);

        // Unsubscribe half from separate threads
        let mut handles = Vec::new();
        for i in 0..5 {
            let mgr = Arc::clone(&manager);
            handles.push(std::thread::spawn(move || {
                let uri = format!("file:///resource/{i}");
                mgr.unsubscribe(&uri);
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(manager.subscription_count(), 5);
    }

    #[test]
    fn test_notification_with_updated_content() {
        let manager = ResourceSubscriptionManager::new();
        let updates: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let updates_clone = updates.clone();

        manager.set_on_update(Arc::new(move |update| {
            updates_clone
                .lock()
                .unwrap()
                .push(update.resource_uri.clone());
        }));

        manager.subscribe("data-srv", "file:///log.txt");

        // First notification with content
        manager.handle_notification(
            "data-srv",
            &serde_json::json!({
                "uri": "file:///log.txt",
                "updated": {"lines": 42, "size": 1024}
            }),
        );

        // Second notification with different content
        manager.handle_notification(
            "data-srv",
            &serde_json::json!({
                "uri": "file:///log.txt",
                "updated": {"lines": 55, "size": 1400}
            }),
        );

        let received = updates.lock().unwrap();
        assert_eq!(received.len(), 2);
        assert_eq!(received[0], "file:///log.txt");
        assert_eq!(received[1], "file:///log.txt");

        // last_updated should be set
        let info = manager.get_subscription("file:///log.txt").unwrap();
        assert!(info.last_updated.is_some());
    }

    #[test]
    fn test_notification_multiple_servers_same_uri() {
        let manager = ResourceSubscriptionManager::new();

        // Subscribe to same URI from two different servers
        manager.subscribe("server-a", "file:///shared.md");
        manager.subscribe("server-b", "file:///shared.md");

        // The second subscribe replaces the first
        assert_eq!(manager.subscription_count(), 1);
        let info = manager.get_subscription("file:///shared.md").unwrap();
        assert_eq!(info.server_name, "server-b");
    }
}

// =========================================================================
// 2. Webhook Registry Integration
// =========================================================================

mod webhook_integration {
    use super::*;

    #[test]
    fn test_sign_payload_verification() {
        let secret = "my-hmac-secret-key";
        let payload = br#"{"event_type":"tool_call_started","server_name":"test"}"#;

        let sig1 = WebhookRegistry::sign_payload(secret, payload);
        let sig2 = WebhookRegistry::sign_payload(secret, payload);

        // Same inputs must produce same signature
        assert_eq!(sig1, sig2);

        // Different payload must produce different signature
        let sig3 = WebhookRegistry::sign_payload(secret, b"different payload");
        assert_ne!(sig1, sig3);

        // Different secret must produce different signature
        let sig4 = WebhookRegistry::sign_payload("other-secret", payload);
        assert_ne!(sig1, sig4);
    }

    #[test]
    fn test_registry_event_filtering_integration() {
        let registry = WebhookRegistry::new();

        // Register a webhook that only receives ToolCall events
        let tool_webhook = WebhookConfig::with_event_types(
            "https://example.com/tools".to_string(),
            "secret1".to_string(),
            vec![
                McpEventType::ToolCallStarted,
                McpEventType::ToolCallCompleted,
            ],
        );
        registry.register(tool_webhook);

        // Register a webhook that receives all events
        let all_webhook =
            WebhookConfig::new("https://example.com/all".to_string(), "secret2".to_string());
        registry.register(all_webhook);

        // ToolCallStarted: both webhooks should match
        let tool_matches = registry.get_matching(&McpEventType::ToolCallStarted);
        assert_eq!(tool_matches.len(), 2);

        // ServerConnected: only the "all" webhook should match
        let server_matches = registry.get_matching(&McpEventType::ServerConnected);
        assert_eq!(server_matches.len(), 1);
        assert_eq!(server_matches[0].1.url, "https://example.com/all");
    }

    #[test]
    fn test_persist_load_multiple_webhooks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("webhooks.json");

        let registry = WebhookRegistry::new();
        let wh1 = WebhookConfig::with_event_types(
            "https://a.com".to_string(),
            "secret-a".to_string(),
            vec![McpEventType::ToolCallStarted],
        );
        let wh2 = WebhookConfig::new("https://b.com".to_string(), "secret-b".to_string());
        let id1 = registry.register(wh1);
        let id2 = registry.register(wh2);
        registry.persist(&path).unwrap();

        let loaded = WebhookRegistry::new();
        loaded.load(&path).unwrap();
        assert_eq!(loaded.len(), 2);

        let c1 = loaded.get(&id1).unwrap();
        assert_eq!(c1.url, "https://a.com");
        assert_eq!(c1.event_types.len(), 1);

        let c2 = loaded.get(&id2).unwrap();
        assert_eq!(c2.url, "https://b.com");
        assert!(c2.event_types.is_empty());
    }

    #[test]
    fn test_webhook_delivery_serialization_roundtrip() {
        let delivery = WebhookDelivery {
            id: "del_integration".to_string(),
            webhook_url: "https://example.com/hook".to_string(),
            event: McpEvent::new(
                McpEventType::ToolCallStarted,
                "test-server".to_string(),
                Some("my_tool".to_string()),
                serde_json::json!({"duration_ms": 150}),
            ),
            status: "success".to_string(),
            attempts: 2,
            last_attempt: Some(chrono::Utc::now()),
        };

        let json = serde_json::to_string(&delivery).unwrap();
        let parsed: WebhookDelivery = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "del_integration");
        assert_eq!(parsed.attempts, 2);
        assert!(parsed.last_attempt.is_some());
    }
}

// =========================================================================
// 3. Protocol Type Roundtrips
// =========================================================================

mod protocol_type_tests {
    use super::*;

    #[test]
    fn test_resources_updated_notification_roundtrip() {
        let notif = ResourcesUpdatedNotification {
            uri: "file:///docs/readme.md".to_string(),
            updated: Some(serde_json::json!({"size": 2048, "hash": "abc123"})),
        };

        let json = serde_json::to_string(&notif).unwrap();
        assert!(json.contains("uri"));
        assert!(json.contains("updated"));

        let parsed: ResourcesUpdatedNotification = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.uri, "file:///docs/readme.md");
        assert!(parsed.updated.is_some());
    }

    #[test]
    fn test_resources_updated_notification_without_updated() {
        let notif = ResourcesUpdatedNotification {
            uri: "file:///data.csv".to_string(),
            updated: None,
        };

        let json = serde_json::to_string(&notif).unwrap();
        assert!(!json.contains("updated"));

        let parsed: ResourcesUpdatedNotification = serde_json::from_str(&json).unwrap();
        assert!(parsed.updated.is_none());
    }

    #[test]
    fn test_request_method_resources_subscribe_roundtrip() {
        let method = RequestMethod::ResourcesSubscribe;
        let json = serde_json::to_string(&method).unwrap();
        assert!(json.contains("resourcesSubscribe"));

        let parsed: RequestMethod = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, RequestMethod::ResourcesSubscribe));
    }

    #[test]
    fn test_response_method_resources_subscribe_roundtrip() {
        let method = ResponseMethod::ResourcesSubscribe;
        let json = serde_json::to_string(&method).unwrap();
        assert!(json.contains("resourcesSubscribe"));

        let parsed: ResponseMethod = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, ResponseMethod::ResourcesSubscribe));
    }

    #[test]
    fn test_request_method_resources_unsubscribe_roundtrip() {
        let method = RequestMethod::ResourcesUnsubscribe;
        let json = serde_json::to_string(&method).unwrap();
        let parsed: RequestMethod = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, RequestMethod::ResourcesUnsubscribe));
    }

    #[test]
    fn test_subscribe_request_roundtrip() {
        let req = SubscribeRequest {
            uri: "file:///config.toml".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: SubscribeRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.uri, "file:///config.toml");
    }

    #[test]
    fn test_unsubscribe_request_roundtrip() {
        let req = UnsubscribeRequest {
            uri: "file:///old.md".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: UnsubscribeRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.uri, "file:///old.md");
    }

    #[test]
    fn test_sampling_message_roundtrip() {
        let msg = SamplingMessage {
            role: SamplingMessageRole::User,
            content: SamplingContent::Text {
                text: "Analyze this data".to_string(),
            },
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: SamplingMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.role, SamplingMessageRole::User);
    }

    #[test]
    fn test_create_message_request_roundtrip() {
        let req = CreateMessageRequest {
            messages: vec![SamplingMessage {
                role: SamplingMessageRole::User,
                content: SamplingContent::Text {
                    text: "Hello".to_string(),
                },
            }],
            model_preferences: Some(ModelPreferences {
                hints: Some(vec![ModelHint {
                    name: Some("claude-sonnet".to_string()),
                }]),
                cost_priority: Some(0.5),
                speed_priority: None,
                intelligence_priority: Some(0.9),
            }),
            system_prompt: Some("Be helpful".to_string()),
            max_tokens: Some(1024),
            sampling_params: SamplingParams {
                temperature: Some(0.7),
                top_p: None,
                stop_sequences: Some(vec!["\n".to_string()]),
            },
        };

        let json = serde_json::to_string(&req).unwrap();
        let parsed: CreateMessageRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.messages.len(), 1);
        assert_eq!(parsed.max_tokens, Some(1024));
    }

    #[test]
    fn test_elicitation_request_result_roundtrip() {
        let request = ElicitationRequest {
            message: "Enter your API key".to_string(),
            requested_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "api_key": {"type": "string", "description": "Your API key"}
                },
                "required": ["api_key"]
            })),
        };

        let json = serde_json::to_string(&request).unwrap();
        let parsed: ElicitationRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.message, "Enter your API key");
        assert!(parsed.requested_schema.is_some());
    }

    #[test]
    fn test_progress_notification_roundtrip() {
        let notif = ProgressNotification {
            progress_token: serde_json::json!("tok-42"),
            progress: 0.75,
            total: Some(1.0),
        };
        let json = serde_json::to_string(&notif).unwrap();
        let parsed: ProgressNotification = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.progress, 0.75);
        assert_eq!(parsed.total, Some(1.0));
    }

    #[test]
    fn test_list_roots_result_roundtrip() {
        let result = ListRootsResult {
            roots: vec![
                Root {
                    uri: "file:///home/user/project-a".to_string(),
                    name: Some("Project A".to_string()),
                },
                Root {
                    uri: "file:///home/user/project-b".to_string(),
                    name: None,
                },
            ],
        };

        let json = serde_json::to_string(&result).unwrap();
        let parsed: ListRootsResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.roots.len(), 2);
        assert_eq!(parsed.roots[0].name, Some("Project A".to_string()));
        assert!(parsed.roots[1].name.is_none());
    }

    #[test]
    fn test_mcp_notification_roundtrip() {
        let notif = McpNotification {
            method: NotificationMethod::NotificationsResourcesUpdated,
            params: serde_json::json!({"uri": "file:///x"}),
        };
        let json = serde_json::to_string(&notif).unwrap();
        let parsed: McpNotification = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            parsed.method,
            NotificationMethod::NotificationsResourcesUpdated
        ));
    }
}

// =========================================================================
// 4. MCP Event Type Integration
// =========================================================================

mod event_type_tests {
    use super::*;

    #[test]
    fn test_mcp_event_creation_and_serialization() {
        let event = McpEvent::new(
            McpEventType::ServerDisconnected,
            "my-server".to_string(),
            None,
            serde_json::json!({"reason": "timeout", "reconnect_attempts": 3}),
        );

        assert_eq!(event.event_type, McpEventType::ServerDisconnected);
        assert_eq!(event.server_name, "my-server");
        assert!(event.tool_name.is_none());
        assert!(event.timestamp <= chrono::Utc::now());

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("server_disconnected"));
        assert!(json.contains("my-server"));
    }

    #[test]
    fn test_all_event_types_serialize_to_snake_case() {
        let all_types = vec![
            McpEventType::ToolCallStarted,
            McpEventType::ToolCallCompleted,
            McpEventType::ServerConnected,
            McpEventType::ServerDisconnected,
            McpEventType::NotificationReceived,
        ];

        for et in &all_types {
            let json = serde_json::to_string(et).unwrap();
            // Should be snake_case (lowercase with underscores)
            let trimmed = json.trim_matches('"');
            assert_eq!(
                trimmed,
                trimmed.to_lowercase(),
                "Event type should serialize to lowercase snake_case"
            );
            assert!(
                !trimmed.contains(|c: char| c.is_uppercase()),
                "Event type should not contain uppercase: {trimmed}"
            );
        }
    }
}
