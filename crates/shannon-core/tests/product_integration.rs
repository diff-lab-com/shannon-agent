//! Product integration tests for Shannon Code.
//!
//! Verifies end-to-end correctness of core subsystems:
//! - AutoUpdater: Instant overflow fix, interval gating, version comparison
//! - OAuth: Builder pattern, full lifecycle, encryption, serialization
//! - ToolRegistry: Registration, lookup, duplicate handling
//! - State management: Session persistence, listing, restoration
//! - Config: Defaults, validation, serialization round-trip
//! - Permissions: Manager creation, level ordering

// ============================================================================
// Updater Integration Tests
// ============================================================================

mod updater_tests {
    use shannon_core::updater::{AutoUpdater, UpdateStatus, UpdaterConfig};
    use std::time::Duration;

    fn make_updater(interval: Duration, enabled: bool) -> AutoUpdater {
        let config = UpdaterConfig {
            repo: "shannon-code/shannon".to_string(),
            check_interval: interval,
            enabled,
            include_prereleases: false,
        };
        AutoUpdater::new(config)
    }

    // -- Instant overflow regression ----------------------------------------

    #[test]
    fn test_updater_creation_no_panic() {
        // The old code used `Instant::now() - Duration::from_secs(u64::MAX)` which
        // panicked. This test verifies the fix (Option<Instant> with None).
        let _updater = make_updater(Duration::from_secs(86400), true);
        // If we reach here, no panic occurred during construction.
    }

    #[test]
    fn test_updater_first_check_proceeds_immediately() {
        // Even with a 1-year interval, first check should proceed (last_check is None)
        let mut updater = make_updater(Duration::from_secs(365 * 86400), true);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let status = rt.block_on(updater.force_check());
        // Should get a valid status (not panic)
        match status {
            UpdateStatus::UpToDate { .. } | UpdateStatus::UpdateAvailable { .. } | UpdateStatus::CheckFailed { .. } => {}
        }
    }

    #[test]
    fn test_updater_interval_gating_after_check() {
        let mut updater = make_updater(Duration::from_secs(365 * 86400), true);
        let rt = tokio::runtime::Runtime::new().unwrap();
        // First force_check sets last_check
        let _ = rt.block_on(updater.force_check());

        // Now check_for_update should skip (interval not elapsed)
        let status = rt.block_on(updater.check_for_update());
        match status {
            UpdateStatus::UpToDate { current } => assert!(!current.is_empty()),
            UpdateStatus::CheckFailed { .. } => {} // cached failure is also ok
            other => panic!("Expected cached result, got {:?}", other),
        }
    }

    #[test]
    fn test_updater_disabled_returns_up_to_date() {
        let mut updater = make_updater(Duration::from_secs(86400), false);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let status = rt.block_on(updater.check_for_update());
        match status {
            UpdateStatus::UpToDate { current } => assert!(!current.is_empty()),
            _ => panic!("Expected UpToDate when disabled"),
        }
    }

    // -- Version comparison --------------------------------------------------

    #[test]
    fn test_version_comparison_comprehensive() {
        use std::cmp::Ordering;

        assert_eq!(AutoUpdater::compare_versions("1.0.0", "2.0.0"), Ordering::Less);
        assert_eq!(AutoUpdater::compare_versions("3.0.0", "2.0.0"), Ordering::Greater);
        assert_eq!(AutoUpdater::compare_versions("1.1.0", "1.2.0"), Ordering::Less);
        assert_eq!(AutoUpdater::compare_versions("1.5.0", "1.3.0"), Ordering::Greater);
        assert_eq!(AutoUpdater::compare_versions("1.0.1", "1.0.2"), Ordering::Less);
        assert_eq!(AutoUpdater::compare_versions("1.0.9", "1.0.3"), Ordering::Greater);
        assert_eq!(AutoUpdater::compare_versions("1.2.3", "1.2.3"), Ordering::Equal);
        assert_eq!(AutoUpdater::compare_versions("v0.1.0", "v0.2.0"), Ordering::Less);
        assert_eq!(AutoUpdater::compare_versions("1.0.0", "v1.0.0"), Ordering::Equal);
        assert_eq!(AutoUpdater::compare_versions("1.0.0-alpha", "1.0.0-beta"), Ordering::Equal);
    }

    #[test]
    fn test_version_parsing_edge_cases() {
        assert_eq!(AutoUpdater::parse_version("0.0.0"), Some((0, 0, 0)));
        assert_eq!(AutoUpdater::parse_version("999.999.999"), Some((999, 999, 999)));
        assert_eq!(AutoUpdater::parse_version(""), None);
        assert_eq!(AutoUpdater::parse_version("1"), None);
        assert_eq!(AutoUpdater::parse_version("1.2"), None);
    }

    // -- Cached status -------------------------------------------------------

    #[test]
    fn test_cached_status_initially_none() {
        let updater = make_updater(Duration::from_secs(86400), true);
        assert!(updater.cached_status().is_none());
    }

    #[test]
    fn test_cached_status_after_force_check() {
        let mut updater = make_updater(Duration::from_secs(86400), true);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _ = rt.block_on(updater.force_check());
        assert!(updater.cached_status().is_some());
    }

    // -- UpdateStatus formatting --------------------------------------------

    #[test]
    fn test_format_update_message_available() {
        let status = UpdateStatus::UpdateAvailable {
            current: "0.1.0".to_string(),
            latest: "0.2.0".to_string(),
            release: shannon_core::updater::ReleaseInfo {
                tag_name: "v0.2.0".to_string(),
                name: Some("Shannon v0.2.0".to_string()),
                body: None,
                published_at: "2026-01-01T00:00:00Z".to_string(),
                html_url: "https://github.com/shannon-code/shannon/releases/tag/v0.2.0".to_string(),
                prerelease: false,
            },
        };
        let msg = AutoUpdater::format_update_message(&status).unwrap();
        assert!(msg.contains("0.1.0"));
        assert!(msg.contains("0.2.0"));
        assert!(msg.contains("github.com"));
    }

    #[test]
    fn test_format_update_message_no_update() {
        assert!(AutoUpdater::format_update_message(&UpdateStatus::UpToDate {
            current: "1.0.0".to_string(),
        }).is_none());

        assert!(AutoUpdater::format_update_message(&UpdateStatus::CheckFailed {
            error: "network error".to_string(),
        }).is_none());
    }

    // -- Config serialization round-trip ------------------------------------

    #[test]
    fn test_updater_config_roundtrip() {
        let config = UpdaterConfig {
            repo: "test/repo".to_string(),
            check_interval: Duration::from_secs(3600),
            enabled: false,
            include_prereleases: true,
        };
        let json = serde_json::to_string(&config).unwrap();
        let loaded: UpdaterConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.repo, "test/repo");
        assert_eq!(loaded.check_interval, Duration::from_secs(3600));
        assert!(!loaded.enabled);
        assert!(loaded.include_prereleases);
    }
}

// ============================================================================
// OAuth Integration Tests
// ============================================================================

mod oauth_tests {
    use shannon_core::oauth::{OAuthService, OAuthClient, OAuthToken, OAuthError, TokenEncryption};

    const KEY: &str = "test-integration-key";

    fn service_with_github() -> OAuthService {
        let mut svc = OAuthService::new(KEY);
        svc.register_client(
            "github", "GitHub", "gh_id", "gh_secret",
            "https://github.com/login/oauth/authorize",
            "https://github.com/login/oauth/access_token",
            "http://localhost:8080/callback",
        ).unwrap();
        svc
    }

    // -- Builder pattern ----------------------------------------------------

    #[test]
    fn test_builder_minimal_required_fields() {
        let client = OAuthClient::builder()
            .id("test").name("Test").client_id("cid").client_secret("csec")
            .auth_url("https://auth.example.com/authorize")
            .token_url("https://auth.example.com/token")
            .redirect_url("http://localhost/cb")
            .build().unwrap();
        assert_eq!(client.id, "test");
        assert!(client.scopes.is_empty());
    }

    #[test]
    fn test_builder_with_scopes() {
        let client = OAuthClient::builder()
            .id("gitlab").name("GitLab").client_id("gl_id").client_secret("gl_sec")
            .auth_url("https://gitlab.com/oauth/authorize")
            .token_url("https://gitlab.com/oauth/token")
            .redirect_url("http://localhost/gl")
            .scopes(vec!["read_user".to_string(), "api".to_string()])
            .build().unwrap();
        assert_eq!(client.scopes.len(), 2);
    }

    #[test]
    fn test_builder_missing_field_returns_error() {
        let result = OAuthClient::builder().id("test").name("Test").build();
        assert!(result.is_err());
        match result.unwrap_err() {
            OAuthError::MissingField(field) => assert_eq!(field, "client_id"),
            other => panic!("Expected MissingField, got {:?}", other),
        }
    }

    #[test]
    fn test_builder_missing_multiple_fields_reports_first() {
        let result = OAuthClient::builder().build();
        assert!(result.is_err());
        match result.unwrap_err() {
            OAuthError::MissingField(field) => assert_eq!(field, "id"),
            other => panic!("Expected MissingField, got {:?}", other),
        }
    }

    #[test]
    fn test_register_client_struct_via_builder() {
        let mut svc = OAuthService::new(KEY);
        let client = OAuthClient::builder()
            .id("custom").name("Custom Provider").client_id("cp_id").client_secret("cp_secret")
            .auth_url("https://custom.auth/authorize").token_url("https://custom.auth/token")
            .redirect_url("http://localhost/custom").scopes(vec!["read".to_string()])
            .build().unwrap();

        svc.register_client_struct(client).unwrap();
        assert_eq!(svc.client_count(), 1);
        let registered = svc.get_client("custom").unwrap();
        assert_eq!(registered.name, "Custom Provider");
    }

    // -- Full lifecycle: register → auth URL → store → verify → refresh -----

    #[test]
    fn test_full_oauth_lifecycle() {
        let mut svc = service_with_github();
        assert_eq!(svc.client_count(), 1);
        assert_eq!(svc.token_count(), 0);

        // Generate authorization URL
        let auth_url = svc.authorization_url("github", "read:user repo").unwrap();
        assert!(auth_url.contains("github.com"));
        assert!(auth_url.contains("client_id=gh_id"));
        assert!(auth_url.contains("state="));

        // Store token (simulating exchange)
        let token = OAuthToken::with_refresh_token("at_123", "rt_456", "read:user repo", 3600);
        svc.store_token("github", token).unwrap();
        assert_eq!(svc.token_count(), 1);

        // Verify token validity
        let valid = svc.get_valid_token("github").unwrap();
        assert_eq!(valid.access_token, "at_123");
        assert!(!valid.is_expired());
        assert!(!svc.needs_refresh("github"));

        // Refresh token
        let new_token = svc.refresh_token("github").unwrap();
        assert_ne!(new_token.access_token, "at_123");
        assert_eq!(new_token.scope, "read:user repo");

        // Remove token
        svc.remove_token("github").unwrap();
        assert_eq!(svc.token_count(), 0);
    }

    // -- Token lifecycle edge cases -----------------------------------------

    #[test]
    fn test_expired_token_rejected() {
        let mut svc = service_with_github();
        let expired = OAuthToken::new("old_token", "read", -3600);
        svc.store_token("github", expired).unwrap();
        let result = svc.get_valid_token("github");
        assert!(matches!(result.unwrap_err(), OAuthError::TokenExpired(_)));
    }

    #[test]
    fn test_token_expiry_within_threshold() {
        let soon = OAuthToken::new("soon", "read", 180);
        assert!(!soon.is_expired());
        assert!(soon.expires_within(chrono::Duration::minutes(5)));

        let fresh = OAuthToken::new("fresh", "read", 7200);
        assert!(!fresh.expires_within(chrono::Duration::minutes(5)));
    }

    #[test]
    fn test_needs_refresh_accuracy() {
        let mut svc = service_with_github();
        assert!(!svc.needs_refresh("github"));

        let soon = OAuthToken::new("soon", "read", 180);
        svc.store_token("github", soon).unwrap();
        assert!(svc.needs_refresh("github"));

        let fresh = OAuthToken::new("fresh", "read", 7200);
        svc.store_token("github", fresh).unwrap();
        assert!(!svc.needs_refresh("github"));
    }

    // -- Encryption ----------------------------------------------------------

    #[test]
    fn test_encryption_round_trip_various_inputs() {
        let enc = TokenEncryption::new("my-secret-key");

        let plain_tokens: Vec<String> = vec![
            "simple-token".to_string(),
            "at_550e8400-e29b-41d4-a716-446655440000".to_string(),
            "token-with-special-chars".to_string(),
            "a".repeat(1000),
        ];

        for plaintext in plain_tokens {
            let encrypted = enc.encrypt(&plaintext).unwrap();
            let decrypted = enc.decrypt(&encrypted).unwrap();
            assert_eq!(decrypted, plaintext);
        }
    }

    #[test]
    fn test_encryption_empty_input_rejected() {
        let enc = TokenEncryption::new("key");
        assert!(enc.encrypt("").is_err());
        assert!(enc.decrypt("").is_err());
    }

    #[test]
    fn test_encryption_invalid_hex_rejected() {
        let enc = TokenEncryption::new("key");
        assert!(enc.decrypt("not-hex").is_err());
        assert!(enc.decrypt("abc").is_err());
    }

    #[test]
    fn test_different_keys_produce_different_ciphertext() {
        let enc1 = TokenEncryption::new("key-a");
        let enc2 = TokenEncryption::new("key-b");
        let c1 = enc1.encrypt("same-input").unwrap();
        let c2 = enc2.encrypt("same-input").unwrap();
        assert_ne!(c1, c2);
    }

    // -- Client management ---------------------------------------------------

    #[test]
    fn test_duplicate_client_rejected() {
        let mut svc = service_with_github();
        let result = svc.register_client(
            "github", "Dup", "x", "y", "http://a", "http://b", "http://c",
        );
        assert!(matches!(result.unwrap_err(), OAuthError::ClientAlreadyExists(_)));
    }

    #[test]
    fn test_remove_client_cascades_to_token() {
        let mut svc = service_with_github();
        svc.store_token("github", OAuthToken::new("tok", "read", 3600)).unwrap();
        assert_eq!(svc.token_count(), 1);
        svc.remove_client("github").unwrap();
        assert_eq!(svc.client_count(), 0);
        assert_eq!(svc.token_count(), 0);
    }

    #[test]
    fn test_operations_on_nonexistent_client() {
        let mut svc = OAuthService::new(KEY);
        assert!(svc.get_client("ghost").is_err());
        assert!(svc.remove_client("ghost").is_err());
        assert!(svc.store_token("ghost", OAuthToken::new("t", "r", 60)).is_err());
        assert!(svc.get_token("ghost").is_err());
    }

    // -- Serialization -------------------------------------------------------

    #[test]
    fn test_oauth_client_json_roundtrip() {
        let client = OAuthClient::builder()
            .id("test").name("Test").client_id("cid").client_secret("csec")
            .auth_url("https://a.com/auth").token_url("https://a.com/token")
            .redirect_url("http://localhost/cb")
            .scopes(vec!["read".to_string(), "write".to_string()])
            .build().unwrap();
        let json = serde_json::to_string_pretty(&client).unwrap();
        let parsed: OAuthClient = serde_json::from_str(&json).unwrap();
        assert_eq!(client, parsed);
    }

    #[test]
    fn test_oauth_token_json_roundtrip() {
        let token = OAuthToken::with_refresh_token("at_x", "rt_y", "read write", 3600);
        let json = serde_json::to_string(&token).unwrap();
        let parsed: OAuthToken = serde_json::from_str(&json).unwrap();
        assert_eq!(token.access_token, parsed.access_token);
        assert_eq!(token.refresh_token, parsed.refresh_token);
        assert_eq!(token.scope, parsed.scope);
        assert_eq!(token.token_type, "Bearer");
    }
}

// ============================================================================
// ToolRegistry Integration Tests
// ============================================================================

mod tool_registry_tests {
    use shannon_core::tools::{ToolRegistry, Tool, ToolOutput, ToolResult};
    use async_trait::async_trait;
    use serde_json::json;

    struct DummyTool { name: String }

    #[async_trait]
    impl Tool for DummyTool {
        async fn execute(&self, _input: serde_json::Value) -> ToolResult<ToolOutput> {
            Ok(ToolOutput {
                content: format!("executed {}", self.name),
                is_error: false,
                metadata: Default::default(),
            })
        }
        fn name(&self) -> &str { &self.name }
        fn description(&self) -> &str { "dummy tool for testing" }
        fn input_schema(&self) -> serde_json::Value {
            json!({"type": "object", "properties": {}})
        }
    }

    #[test]
    fn test_register_and_lookup() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(DummyTool { name: "foo".into() })).unwrap();
        let tool = registry.get("foo").unwrap();
        assert_eq!(tool.name(), "foo");
    }

    #[test]
    fn test_register_duplicate_rejected() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(DummyTool { name: "dup".into() })).unwrap();
        let result = registry.register(Box::new(DummyTool { name: "dup".into() }));
        assert!(result.is_err());
    }

    #[test]
    fn test_lookup_nonexistent() {
        let registry = ToolRegistry::new();
        assert!(registry.get("nope").is_none());
    }

    #[test]
    fn test_list_registered_tools() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(DummyTool { name: "a".into() })).unwrap();
        registry.register(Box::new(DummyTool { name: "b".into() })).unwrap();

        let tools = registry.list_tools_info();
        assert_eq!(tools.len(), 2);
        let names: Vec<String> = tools.iter().map(|t| t.name.clone()).collect();
        assert!(names.contains(&"a".to_string()));
        assert!(names.contains(&"b".to_string()));
    }

    #[tokio::test]
    async fn test_execute_tool_through_registry() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(DummyTool { name: "echo".into() })).unwrap();
        let tool = registry.get("echo").unwrap();
        let result = tool.execute(json!({})).await.unwrap();
        assert_eq!(result.content, "executed echo");
        assert!(!result.is_error);
    }
}

// ============================================================================
// Config & LLM Client Integration Tests
// ============================================================================

mod config_tests {
    use shannon_core::api::LlmClientConfig;

    #[test]
    fn test_default_config_has_provider() {
        let config = LlmClientConfig::default();
        assert!(!config.provider.to_string().is_empty());
    }

    #[test]
    fn test_default_config_has_max_tokens() {
        let config = LlmClientConfig::default();
        assert!(config.max_tokens > 0);
    }

    #[test]
    fn test_config_describe_does_not_leak_secrets() {
        let config = LlmClientConfig::default();
        let desc = config.describe();
        assert!(!desc.contains("sk-"));
        assert!(!desc.contains("secret"));
    }
}

// ============================================================================
// Permissions Integration Tests
// ============================================================================

mod permissions_tests {
    use shannon_core::permissions::{PermissionManager, PermissionLevel};

    #[test]
    fn test_permission_manager_creation() {
        let _mgr = PermissionManager::new();
    }

    #[test]
    fn test_permission_level_ordering() {
        assert!(PermissionLevel::Admin as u8 > PermissionLevel::Write as u8);
        assert!(PermissionLevel::Write as u8 > PermissionLevel::Read as u8);
        assert!(PermissionLevel::Read as u8 > PermissionLevel::None as u8);
    }

    #[test]
    fn test_permission_level_values() {
        assert_eq!(PermissionLevel::None as u8, 0);
        assert_eq!(PermissionLevel::Read as u8, 1);
        assert_eq!(PermissionLevel::Write as u8, 2);
        assert_eq!(PermissionLevel::Admin as u8, 3);
    }
}

// ============================================================================
// State Management Integration Tests (supplement existing)
// ============================================================================

mod state_extra_tests {
    use shannon_core::state::StateManager;
    use shannon_core::api::{Message, MessageContent};
    use uuid::Uuid;

    fn make_mgr() -> (StateManager, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let mgr = StateManager::with_sessions_dir(dir.path().to_path_buf()).unwrap();
        (mgr, dir)
    }

    #[test]
    fn test_delete_session() {
        let (mgr, _dir) = make_mgr();
        let session = mgr.create_session(None, "test-model".to_string()).unwrap();
        let id = session.session_id;
        // Verify it exists in memory
        assert!(mgr.get_session(id).is_ok());
        mgr.delete_session(id).unwrap();
        assert!(mgr.get_session(id).is_err());
    }

    #[test]
    fn test_delete_nonexistent_session_errors() {
        let (mgr, _dir) = make_mgr();
        let result = mgr.delete_session(Uuid::new_v4());
        assert!(result.is_err());
    }

    #[test]
    fn test_list_sessions_empty() {
        let (mgr, _dir) = make_mgr();
        let sessions = mgr.list_persisted_sessions().unwrap();
        assert!(sessions.is_empty());
    }

    #[test]
    fn test_large_session_messages() {
        let (mgr, _dir) = make_mgr();
        let id = Uuid::new_v4();
        let msgs: Vec<Message> = (0..100)
            .map(|i| Message {
                role: if i % 2 == 0 { "user" } else { "assistant" }.to_string(),
                content: MessageContent::Text(format!("Message {}", i)),
            })
            .collect();
        mgr.save_session(&id, &msgs, &Default::default()).unwrap();
        let loaded = mgr.load_session(&id).unwrap().unwrap();
        assert_eq!(loaded.messages.len(), 100);
        assert_eq!(loaded.metadata.turn_count, 50);
    }
}
