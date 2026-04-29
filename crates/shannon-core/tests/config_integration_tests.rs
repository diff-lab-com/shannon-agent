//! Integration tests for credential_manager, settings, and unified_config modules.
//!
//! Covers:
//! - CredentialManager: store/retrieve/delete, overwrite, list, import/export, persistence
//! - Settings: defaults, validation, serialization round-trip, get/set/merge, env parsing
//! - UnifiedConfig: creation, merging, priority chain, ConfigBuilder, file loading

// ============================================================================
// CredentialManager Integration Tests
// ============================================================================

mod credential_manager_tests {
    use shannon_core::credential_manager::{
        Credential, CredentialError, CredentialFileFormat, CredentialManager,
        PortableCredential, PortableCredentialBundle,
    };
    use std::collections::HashMap;
    use tempfile::TempDir;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    /// Helper: create a CredentialManager backed by a temp directory.
    fn make_mgr() -> (CredentialManager, TempDir) {
        let dir = TempDir::new().unwrap();
        let mgr = CredentialManager::with_dir(dir.path().to_path_buf()).unwrap();
        (mgr, dir)
    }

    // -- Store & Retrieve ----------------------------------------------------

    #[test]
    fn test_store_and_retrieve_single_credential() {
        let (mut mgr, _dir) = make_mgr();
        let cred = Credential::new("Anthropic Key", "anthropic", "sk-ant-test123");
        mgr.store(cred).unwrap();

        let retrieved = mgr.retrieve("anthropic").unwrap();
        assert_eq!(retrieved.name, "Anthropic Key");
        assert_eq!(retrieved.service, "anthropic");
        assert_eq!(retrieved.value, "sk-ant-test123");
    }

    #[test]
    fn test_store_multiple_credentials_and_retrieve_each() {
        let (mut mgr, _dir) = make_mgr();
        mgr.store(Credential::new("Anthropic", "anthropic", "key-a")).unwrap();
        mgr.store(Credential::new("GitHub", "github", "key-g")).unwrap();
        mgr.store(Credential::new("OpenAI", "openai", "key-o")).unwrap();

        assert_eq!(mgr.retrieve("anthropic").unwrap().value, "key-a");
        assert_eq!(mgr.retrieve("github").unwrap().value, "key-g");
        assert_eq!(mgr.retrieve("openai").unwrap().value, "key-o");
    }

    // -- Retrieve Non-Existent ------------------------------------------------

    #[test]
    fn test_retrieve_nonexistent_returns_not_found_error() {
        let (mgr, _dir) = make_mgr();
        let result = mgr.retrieve("nonexistent_service");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, CredentialError::NotFound(ref s) if s.contains("nonexistent_service")));
    }

    #[test]
    fn test_retrieve_from_empty_manager_returns_error() {
        let (mgr, _dir) = make_mgr();
        assert!(mgr.retrieve("anything").is_err());
    }

    // -- Store Duplicate -------------------------------------------------------

    #[test]
    fn test_store_duplicate_service_rejected() {
        let (mut mgr, _dir) = make_mgr();
        mgr.store(Credential::new("First", "anthropic", "key-1")).unwrap();
        let result = mgr.store(Credential::new("Second", "anthropic", "key-2"));
        assert!(result.is_err());
        match result.unwrap_err() {
            CredentialError::AlreadyExists(msg) => {
                assert!(msg.contains("anthropic"));
            }
            other => panic!("Expected AlreadyExists, got {other:?}"),
        }
    }

    // -- Overwrite via store_or_update -----------------------------------------

    #[test]
    fn test_store_or_update_overwrites_existing() {
        let (mut mgr, _dir) = make_mgr();
        mgr.store(Credential::new("Original", "anthropic", "old-key")).unwrap();
        assert_eq!(mgr.retrieve("anthropic").unwrap().value, "old-key");

        mgr.store_or_update(Credential::new("Updated", "anthropic", "new-key")).unwrap();
        let retrieved = mgr.retrieve("anthropic").unwrap();
        assert_eq!(retrieved.name, "Updated");
        assert_eq!(retrieved.value, "new-key");
    }

    #[test]
    fn test_store_or_update_creates_if_not_exists() {
        let (mut mgr, _dir) = make_mgr();
        mgr.store_or_update(Credential::new("Fresh", "openai", "fresh-key")).unwrap();
        assert_eq!(mgr.retrieve("openai").unwrap().value, "fresh-key");
    }

    // -- Delete ---------------------------------------------------------------

    #[test]
    fn test_delete_existing_credential() {
        let (mut mgr, _dir) = make_mgr();
        mgr.store(Credential::new("Test", "anthropic", "key")).unwrap();
        assert!(mgr.exists("anthropic"));

        let deleted = mgr.delete("anthropic").unwrap();
        assert_eq!(deleted.service, "anthropic");
        assert!(!mgr.exists("anthropic"));
    }

    #[test]
    fn test_delete_nonexistent_returns_error() {
        let (mut mgr, _dir) = make_mgr();
        let result = mgr.delete("ghost");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CredentialError::NotFound(_)));
    }

    #[test]
    fn test_delete_removes_file_from_disk() {
        let (mut mgr, dir) = make_mgr();
        mgr.store(Credential::new("Test", "anthropic", "key")).unwrap();
        let file_path = dir.path().join("anthropic.json");
        assert!(file_path.exists());

        mgr.delete("anthropic").unwrap();
        assert!(!file_path.exists());
    }

    // -- List -----------------------------------------------------------------

    #[test]
    fn test_list_empty_manager() {
        let (mgr, _dir) = make_mgr();
        let list = mgr.list();
        assert!(list.is_empty());
    }

    #[test]
    fn test_list_returns_all_stored_services() {
        let (mut mgr, _dir) = make_mgr();
        mgr.store(Credential::new("A", "svc-a", "val-a")).unwrap();
        mgr.store(Credential::new("B", "svc-b", "val-b")).unwrap();
        mgr.store(Credential::new("C", "svc-c", "val-c")).unwrap();

        let list = mgr.list();
        assert_eq!(list.len(), 3);

        let services: Vec<&str> = list.iter().map(|s| s.service.as_str()).collect();
        assert!(services.contains(&"svc-a"));
        assert!(services.contains(&"svc-b"));
        assert!(services.contains(&"svc-c"));
    }

    #[test]
    fn test_list_does_not_contain_sensitive_values() {
        let (mut mgr, _dir) = make_mgr();
        mgr.store(Credential::new("Test", "anthropic", "super-secret-key")).unwrap();

        let list = mgr.list();
        assert_eq!(list.len(), 1);
        // CredentialSummary does not have a `value` field
        assert_eq!(list[0].service, "anthropic");
        assert_eq!(list[0].name, "Test");
    }

    // -- Count & Exists -------------------------------------------------------

    #[test]
    fn test_count_tracks_credentials() {
        let (mut mgr, _dir) = make_mgr();
        assert_eq!(mgr.count(), 0);

        mgr.store(Credential::new("A", "a", "1")).unwrap();
        assert_eq!(mgr.count(), 1);

        mgr.store(Credential::new("B", "b", "2")).unwrap();
        assert_eq!(mgr.count(), 2);

        mgr.delete("a").unwrap();
        assert_eq!(mgr.count(), 1);
    }

    #[test]
    fn test_exists_checks_by_service() {
        let (mut mgr, _dir) = make_mgr();
        assert!(!mgr.exists("anthropic"));

        mgr.store(Credential::new("Anthropic", "anthropic", "key")).unwrap();
        assert!(mgr.exists("anthropic"));
    }

    // -- Validation -----------------------------------------------------------

    #[test]
    fn test_store_empty_name_rejected() {
        let (mut mgr, _dir) = make_mgr();
        let mut cred = Credential::new("X", "anthropic", "key");
        cred.name = String::new();
        let result = mgr.store(cred);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("name cannot be empty"));
    }

    #[test]
    fn test_store_empty_service_rejected() {
        let (mut mgr, _dir) = make_mgr();
        let mut cred = Credential::new("Test", "svc", "key");
        cred.service = String::new();
        let result = mgr.store(cred);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("service cannot be empty"));
    }

    // -- Persistence -----------------------------------------------------------

    #[test]
    fn test_credentials_persist_across_manager_instances() {
        let dir = TempDir::new().unwrap();
        let dir_path = dir.path().to_path_buf();

        // Store in first instance
        {
            let mut mgr = CredentialManager::with_dir(dir_path.clone()).unwrap();
            mgr.store(Credential::new("Persistent", "anthropic", "persisted-key")).unwrap();
        }

        // Load in second instance
        {
            let mut mgr = CredentialManager::with_dir(dir_path.clone()).unwrap();
            mgr.load().unwrap();
            let cred = mgr.retrieve("anthropic").unwrap();
            assert_eq!(cred.value, "persisted-key");
            assert_eq!(cred.name, "Persistent");
        }
    }

    #[test]
    fn test_persist_creates_json_file_on_disk() {
        let (mut mgr, dir) = make_mgr();
        mgr.store(Credential::new("Test", "openai", "key-123")).unwrap();

        let file_path = dir.path().join("openai.json");
        assert!(file_path.exists());

        let content = std::fs::read_to_string(&file_path).unwrap();
        assert!(content.contains("openai"));
        assert!(content.contains("key-123"));
    }

    // -- File Descriptor -------------------------------------------------------

    #[test]
    fn test_file_descriptor_nonexistent_file() {
        let (mgr, _dir) = make_mgr();
        let desc = mgr.file_descriptor("ghost");
        assert!(!desc.exists);
        assert_eq!(desc.size, 0);
        assert_eq!(desc.format, CredentialFileFormat::Json);
    }

    #[test]
    fn test_file_descriptor_existing_file() {
        let (mut mgr, _dir) = make_mgr();
        mgr.store(Credential::new("Test", "anthropic", "key")).unwrap();

        let desc = mgr.file_descriptor("anthropic");
        assert!(desc.exists);
        assert!(desc.size > 0);
        assert_eq!(desc.format, CredentialFileFormat::Json);
    }

    // -- Portable Export/Import -----------------------------------------------

    #[test]
    fn test_export_portable_bundle_structure() {
        let (mut mgr, _dir) = make_mgr();
        mgr.store(Credential::new("A", "svc-a", "val-a")).unwrap();
        mgr.store(Credential::new("B", "svc-b", "val-b")).unwrap();

        let bundle = mgr.export_portable().unwrap();
        assert_eq!(bundle.version, 1);
        assert_eq!(bundle.credentials.len(), 2);
        assert!(bundle.exported_at <= chrono::Utc::now());
    }

    #[test]
    fn test_import_portable_creates_credentials() {
        let (mut mgr, _dir) = make_mgr();
        let mut bundle = PortableCredentialBundle::new();
        bundle.credentials.push(PortableCredential {
            name: "Imported".into(),
            service: "imported".into(),
            value: "imported-val".into(),
            metadata: HashMap::new(),
            exported_at: chrono::Utc::now(),
        });

        let result = mgr.import_portable(bundle, false).unwrap();
        assert_eq!(result.imported, 1);
        assert_eq!(result.skipped, 0);
        assert_eq!(mgr.retrieve("imported").unwrap().value, "imported-val");
    }

    #[test]
    fn test_import_portable_skip_existing_preserves_original() {
        let (mut mgr, _dir) = make_mgr();
        mgr.store(Credential::new("Original", "anthropic", "original-key")).unwrap();

        let mut bundle = PortableCredentialBundle::new();
        bundle.credentials.push(PortableCredential {
            name: "New".into(),
            service: "anthropic".into(),
            value: "new-key".into(),
            metadata: HashMap::new(),
            exported_at: chrono::Utc::now(),
        });

        let result = mgr.import_portable(bundle, true).unwrap();
        assert_eq!(result.imported, 0);
        assert_eq!(result.skipped, 1);
        // Original preserved
        assert_eq!(mgr.retrieve("anthropic").unwrap().value, "original-key");
    }

    #[test]
    fn test_import_portable_overwrite_replaces_existing() {
        let (mut mgr, _dir) = make_mgr();
        mgr.store(Credential::new("Original", "anthropic", "old-key")).unwrap();

        let mut bundle = PortableCredentialBundle::new();
        bundle.credentials.push(PortableCredential {
            name: "Replacement".into(),
            service: "anthropic".into(),
            value: "new-key".into(),
            metadata: HashMap::new(),
            exported_at: chrono::Utc::now(),
        });

        let result = mgr.import_portable(bundle, false).unwrap();
        assert_eq!(result.imported, 1);
        assert_eq!(result.skipped, 0);
        assert_eq!(mgr.retrieve("anthropic").unwrap().value, "new-key");
    }

    #[test]
    fn test_import_empty_bundle() {
        let (mut mgr, _dir) = make_mgr();
        let bundle = PortableCredentialBundle::new();
        let result = mgr.import_portable(bundle, false).unwrap();
        assert_eq!(result.imported, 0);
        assert_eq!(result.skipped, 0);
    }

    #[test]
    fn test_roundtrip_export_then_import() {
        let dir = TempDir::new().unwrap();
        let dir_path = dir.path().to_path_buf();

        // Store credentials in first manager, export
        let bundle = {
            let mut mgr = CredentialManager::with_dir(dir_path.clone()).unwrap();
            mgr.store(Credential::new("A", "svc-a", "val-a")).unwrap();
            mgr.store(Credential::new("B", "svc-b", "val-b")).unwrap();
            mgr.export_portable().unwrap()
        };

        // Import into a second manager in a different directory
        let dir2 = TempDir::new().unwrap();
        let mut mgr2 = CredentialManager::with_dir(dir2.path().to_path_buf()).unwrap();
        let result = mgr2.import_portable(bundle, false).unwrap();
        assert_eq!(result.imported, 2);

        assert_eq!(mgr2.retrieve("svc-a").unwrap().value, "val-a");
        assert_eq!(mgr2.retrieve("svc-b").unwrap().value, "val-b");
    }

    // -- Credential Serialization ---------------------------------------------

    #[test]
    fn test_credential_json_roundtrip() {
        let mut cred = Credential::new("Test Cred", "test-service", "secret-value");
        cred.metadata.insert("env".to_string(), "prod".to_string());

        let json = serde_json::to_string(&cred).unwrap();
        let parsed: Credential = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.name, cred.name);
        assert_eq!(parsed.service, cred.service);
        assert_eq!(parsed.value, cred.value);
        assert_eq!(parsed.metadata.get("env").unwrap(), "prod");
    }

    #[test]
    fn test_portable_bundle_json_roundtrip() {
        let mut bundle = PortableCredentialBundle::new();
        bundle.machine_id = Some("test-machine".to_string());
        bundle.credentials.push(PortableCredential {
            name: "Key".into(),
            service: "svc".into(),
            value: "val".into(),
            metadata: HashMap::new(),
            exported_at: chrono::Utc::now(),
        });

        let json = serde_json::to_string(&bundle).unwrap();
        let parsed: PortableCredentialBundle = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.credentials.len(), 1);
        assert_eq!(parsed.machine_id, Some("test-machine".to_string()));
    }

    // -- Secure Permissions ----------------------------------------------------

    #[test]
    #[cfg(unix)]
    fn test_set_secure_permissions() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.json");
        std::fs::write(&file_path, "{}").unwrap();

        let mgr = CredentialManager::with_dir(dir.path().to_path_buf()).unwrap();
        mgr.set_secure_permissions(&file_path).unwrap();

        let mode = std::fs::metadata(&file_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    // -- Load from Empty Directory ---------------------------------------------

    #[test]
    fn test_load_from_empty_directory() {
        let (mut mgr, _dir) = make_mgr();
        mgr.load().unwrap();
        assert_eq!(mgr.count(), 0);
    }

    #[test]
    fn test_load_from_nonexistent_directory() {
        let dir = TempDir::new().unwrap();
        let nonexistent = dir.path().join("does_not_exist");
        // with_dir creates the directory, so this should succeed
        let mut mgr = CredentialManager::with_dir(nonexistent.clone()).unwrap();
        mgr.load().unwrap();
        assert_eq!(mgr.count(), 0);
    }

    // -- Full CRUD Lifecycle ---------------------------------------------------

    #[test]
    fn test_full_crud_lifecycle() {
        let (mut mgr, _dir) = make_mgr();

        // Create
        mgr.store(Credential::new("Test", "svc", "initial")).unwrap();
        assert_eq!(mgr.count(), 1);

        // Read
        let cred = mgr.retrieve("svc").unwrap();
        assert_eq!(cred.value, "initial");

        // Update
        mgr.store_or_update(Credential::new("Updated", "svc", "updated")).unwrap();
        assert_eq!(mgr.retrieve("svc").unwrap().value, "updated");
        assert_eq!(mgr.count(), 1); // still one, not two

        // Delete
        let deleted = mgr.delete("svc").unwrap();
        assert_eq!(deleted.service, "svc");
        assert_eq!(mgr.count(), 0);
        assert!(mgr.retrieve("svc").is_err());
    }
}

// ============================================================================
// Settings Integration Tests
// ============================================================================

mod settings_tests {
    use serde_json::json;
    use shannon_core::settings::{Settings, SettingsError, SettingsManager};

    // -- Default Values --------------------------------------------------------

    #[test]
    fn test_default_settings_version() {
        let settings = Settings::default();
        assert_eq!(settings.version, "1.0");
    }

    #[test]
    fn test_default_settings_optional_fields_are_none() {
        let settings = Settings::default();
        assert!(settings.model.is_none());
        assert!(settings.temperature.is_none());
        assert!(settings.max_tokens.is_none());
    }

    #[test]
    fn test_default_settings_has_sensible_defaults() {
        let settings = Settings::default();
        assert!(settings.tools_enabled);
        assert_eq!(settings.permissions_mode, "ask");
        assert!(settings.auto_memory);
        assert_eq!(settings.theme, "dark");
    }

    #[test]
    fn test_settings_new_equals_default() {
        let new_settings = Settings::new();
        let default_settings = Settings::default();
        assert_eq!(new_settings, default_settings);
    }

    // -- Validation ------------------------------------------------------------

    #[test]
    fn test_valid_default_settings() {
        assert!(Settings::default().validate().is_ok());
    }

    #[test]
    fn test_valid_custom_settings() {
        let settings = Settings {
            version: "1.0".to_string(),
            model: Some("claude-opus-4-6".to_string()),
            temperature: Some(0.5),
            max_tokens: Some(4096),
            tools_enabled: true,
            permissions_mode: "auto".to_string(),
            auto_memory: false,
            theme: "light".to_string(),
            ..Default::default()
        };
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn test_invalid_temperature_too_high() {
        let mut settings = Settings::default();
        settings.temperature = Some(1.5);
        let err = settings.validate().unwrap_err();
        assert!(matches!(err, SettingsError::InvalidValue { ref key, .. } if key == "temperature"));
    }

    #[test]
    fn test_invalid_temperature_negative() {
        let mut settings = Settings::default();
        settings.temperature = Some(-0.1);
        let err = settings.validate().unwrap_err();
        assert!(matches!(err, SettingsError::InvalidValue { ref key, .. } if key == "temperature"));
    }

    #[test]
    fn test_invalid_temperature_boundary() {
        let mut settings = Settings::default();
        // Boundary values should be valid
        settings.temperature = Some(0.0);
        assert!(settings.validate().is_ok());
        settings.temperature = Some(1.0);
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn test_invalid_max_tokens_zero() {
        let mut settings = Settings::default();
        settings.max_tokens = Some(0);
        let err = settings.validate().unwrap_err();
        assert!(matches!(err, SettingsError::InvalidValue { ref key, .. } if key == "max_tokens"));
    }

    #[test]
    fn test_valid_max_tokens_positive() {
        let mut settings = Settings::default();
        settings.max_tokens = Some(1);
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn test_invalid_permissions_mode() {
        let mut settings = Settings::default();
        for bad in &["unknown", "admin", "readwrite", ""] {
            settings.permissions_mode = bad.to_string();
            assert!(settings.validate().is_err(), "Expected error for permissions_mode={bad}");
        }
    }

    #[test]
    fn test_valid_permissions_modes() {
        for mode in &["ask", "auto", "readonly"] {
            let mut settings = Settings::default();
            settings.permissions_mode = mode.to_string();
            assert!(settings.validate().is_ok(), "Expected ok for permissions_mode={mode}");
        }
    }

    #[test]
    fn test_invalid_theme() {
        let mut settings = Settings::default();
        settings.theme = "neon".to_string();
        assert!(settings.validate().is_err());
    }

    #[test]
    fn test_valid_themes() {
        for theme in &["dark", "light", "auto"] {
            let mut settings = Settings::default();
            settings.theme = theme.to_string();
            assert!(settings.validate().is_ok());
        }
    }

    // -- Serialization / Deserialization Round-Trip ----------------------------

    #[test]
    fn test_settings_json_roundtrip_default() {
        let original = Settings::default();
        let json = serde_json::to_string(&original).unwrap();
        let parsed: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(original, parsed);
    }

    #[test]
    fn test_settings_json_roundtrip_custom() {
        let original = Settings {
            version: "1.0".to_string(),
            model: Some("gpt-4o".to_string()),
            temperature: Some(0.7),
            max_tokens: Some(8192),
            tools_enabled: false,
            permissions_mode: "auto".to_string(),
            auto_memory: false,
            theme: "light".to_string(),
            ..Default::default()
        };
        let json = serde_json::to_string(&original).unwrap();
        let parsed: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(original, parsed);
    }

    #[test]
    fn test_settings_json_uses_camel_case() {
        let settings = Settings::default();
        let json = serde_json::to_string(&settings).unwrap();
        // Verify camelCase field names in JSON output
        assert!(json.contains("toolsEnabled"));
        assert!(json.contains("permissionsMode"));
        assert!(json.contains("autoMemory"));
    }

    #[test]
    fn test_settings_deserialize_from_camel_case_json() {
        let json_str = r#"{
            "version": "1.0",
            "model": "claude-opus-4-6",
            "temperature": 0.3,
            "maxTokens": 4096,
            "toolsEnabled": false,
            "permissionsMode": "readonly",
            "autoMemory": true,
            "theme": "light"
        }"#;
        let parsed: Settings = serde_json::from_str(json_str).unwrap();
        assert_eq!(parsed.model, Some("claude-opus-4-6".to_string()));
        assert_eq!(parsed.temperature, Some(0.3));
        assert_eq!(parsed.max_tokens, Some(4096));
        assert!(!parsed.tools_enabled);
        assert_eq!(parsed.permissions_mode, "readonly");
        assert!(parsed.auto_memory);
        assert_eq!(parsed.theme, "light");
    }

    #[test]
    fn test_settings_optional_fields_omitted_when_none() {
        let settings = Settings::default();
        let json = serde_json::to_string(&settings).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        // Optional fields with skip_serializing_if should not appear
        assert!(parsed.get("model").is_none());
        assert!(parsed.get("temperature").is_none());
        assert!(parsed.get("maxTokens").is_none());
        // Non-optional fields should appear
        assert!(parsed.get("toolsEnabled").is_some());
    }

    #[test]
    fn test_settings_skip_serializing_if_none() {
        let settings = Settings {
            version: "1.0".to_string(),
            model: None,
            temperature: None,
            max_tokens: None,
            tools_enabled: true,
            permissions_mode: "ask".to_string(),
            auto_memory: true,
            theme: "dark".to_string(),
            ..Default::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert!(!json.contains("model"));
        assert!(!json.contains("temperature"));
        assert!(!json.contains("maxTokens"));
    }

    // -- Get / Set Value -------------------------------------------------------

    #[test]
    fn test_get_value_all_fields() {
        let settings = Settings {
            version: "1.0".to_string(),
            model: Some("test-model".to_string()),
            temperature: Some(0.5),
            max_tokens: Some(2048),
            tools_enabled: true,
            permissions_mode: "auto".to_string(),
            auto_memory: false,
            theme: "light".to_string(),
            ..Default::default()
        };

        assert_eq!(settings.get_value("version"), Some(json!("1.0")));
        assert_eq!(settings.get_value("model"), Some(json!("test-model")));
        assert_eq!(settings.get_value("tools_enabled"), Some(json!(true)));
        assert_eq!(settings.get_value("permissions_mode"), Some(json!("auto")));
        assert_eq!(settings.get_value("auto_memory"), Some(json!(false)));
        assert_eq!(settings.get_value("theme"), Some(json!("light")));
        assert_eq!(settings.get_value("nonexistent"), None);
    }

    #[test]
    fn test_set_value_model() {
        let mut settings = Settings::default();
        settings.set_value("model", json!("gpt-4o")).unwrap();
        assert_eq!(settings.model, Some("gpt-4o".to_string()));
    }

    #[test]
    fn test_set_value_null_clears_optional() {
        let mut settings = Settings::default();
        settings.model = Some("test".to_string());
        settings.set_value("model", json!(null)).unwrap();
        assert!(settings.model.is_none());
    }

    #[test]
    fn test_set_value_empty_string_clears_model() {
        let mut settings = Settings::default();
        settings.model = Some("test".to_string());
        settings.set_value("model", json!("")).unwrap();
        assert!(settings.model.is_none());
    }

    #[test]
    fn test_set_value_invalid_type_rejected() {
        let mut settings = Settings::default();
        let result = settings.set_value("model", json!(true));
        assert!(result.is_err());
    }

    #[test]
    fn test_set_value_unknown_key_rejected() {
        let mut settings = Settings::default();
        let result = settings.set_value("nonexistent", json!("value"));
        assert!(matches!(result.unwrap_err(), SettingsError::KeyNotFound(_)));
    }

    // -- Merge -----------------------------------------------------------------

    #[test]
    fn test_merge_override_takes_precedence() {
        let mut base = Settings {
            version: "1.0".to_string(),
            model: Some("base-model".to_string()),
            temperature: Some(0.5),
            max_tokens: Some(4096),
            tools_enabled: true,
            permissions_mode: "ask".to_string(),
            auto_memory: true,
            theme: "dark".to_string(),
            ..Default::default()
        };

        let override_settings = Settings {
            version: "1.0".to_string(),
            model: Some("override-model".to_string()),
            temperature: None, // None should NOT override
            max_tokens: Some(8192),
            tools_enabled: false,
            permissions_mode: "auto".to_string(),
            auto_memory: false,
            theme: "light".to_string(),
            ..Default::default()
        };

        base.merge(override_settings);

        assert_eq!(base.model, Some("override-model".to_string()));
        assert_eq!(base.temperature, Some(0.5)); // kept from base
        assert_eq!(base.max_tokens, Some(8192));
        assert!(!base.tools_enabled);
        assert_eq!(base.permissions_mode, "auto");
        assert!(!base.auto_memory);
        assert_eq!(base.theme, "light");
    }

    #[test]
    fn test_merge_none_fields_preserve_base() {
        let mut base = Settings {
            version: "1.0".to_string(),
            model: Some("base".to_string()),
            temperature: Some(0.3),
            max_tokens: Some(100),
            ..Default::default()
        };

        let other = Settings {
            model: None,
            temperature: None,
            max_tokens: None,
            ..Default::default()
        };

        base.merge(other);
        assert_eq!(base.model, Some("base".to_string()));
        assert_eq!(base.temperature, Some(0.3));
        assert_eq!(base.max_tokens, Some(100));
    }

    // -- SettingsManager File I/O via save_to_path / load_from_path ------------

    #[test]
    fn test_save_and_load_roundtrip_via_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_settings.json");

        let mut mgr = SettingsManager::new();
        mgr.settings_mut().model = Some("test-model".to_string());
        mgr.settings_mut().temperature = Some(0.8);
        mgr.settings_mut().max_tokens = Some(2048);
        mgr.settings_mut().permissions_mode = "auto".to_string();
        mgr.settings_mut().theme = "light".to_string();

        mgr.save_to_path(&path).unwrap();
        assert!(path.exists());

        let mut mgr2 = SettingsManager::new();
        mgr2.load_from_path(&path).unwrap();

        assert_eq!(mgr2.settings().model, Some("test-model".to_string()));
        assert_eq!(mgr2.settings().temperature, Some(0.8));
        assert_eq!(mgr2.settings().max_tokens, Some(2048));
        assert_eq!(mgr2.settings().permissions_mode, "auto");
        assert_eq!(mgr2.settings().theme, "light");
    }

    #[test]
    fn test_save_to_path_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("dir").join("settings.json");

        let mgr = SettingsManager::new();
        mgr.save_to_path(&path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_load_from_path_nonexistent_file_uses_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");

        let mut mgr = SettingsManager::new();
        mgr.load_from_path(&path).unwrap();
        assert_eq!(mgr.settings().version, "1.0");
        assert!(mgr.settings().tools_enabled);
    }

    #[test]
    fn test_load_from_path_invalid_version_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad_version.json");

        let bad_json = r#"{"version": "0.5", "toolsEnabled": true}"#;
        std::fs::write(&path, bad_json).unwrap();

        let mut mgr = SettingsManager::new();
        let result = mgr.load_from_path(&path);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SettingsError::InvalidVersion { .. }));
    }

    #[test]
    fn test_save_rejects_invalid_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");

        let mut mgr = SettingsManager::new();
        mgr.settings_mut().temperature = Some(5.0); // invalid

        let result = mgr.save_to_path(&path);
        assert!(result.is_err());
    }

    // -- Manager Get/Set via string API ----------------------------------------

    #[test]
    fn test_manager_set_model_string() {
        let mut mgr = SettingsManager::new();
        mgr.set("model", "claude-opus-4-6").unwrap();
        assert_eq!(mgr.settings().model, Some("claude-opus-4-6".to_string()));
    }

    #[test]
    fn test_manager_set_temperature_string() {
        let mut mgr = SettingsManager::new();
        mgr.set("temperature", "0.7").unwrap();
        assert_eq!(mgr.settings().temperature, Some(0.7));
    }

    #[test]
    fn test_manager_set_max_tokens_string() {
        let mut mgr = SettingsManager::new();
        mgr.set("max_tokens", "8192").unwrap();
        assert_eq!(mgr.settings().max_tokens, Some(8192));
    }

    #[test]
    fn test_manager_set_boolean_string() {
        let mut mgr = SettingsManager::new();
        mgr.set("tools_enabled", "false").unwrap();
        assert!(!mgr.settings().tools_enabled);
    }

    // -- Env Override Application -----------------------------------------------

    #[test]
    fn test_apply_env_overrides_sets_values() {
        let mut mgr = SettingsManager::new();
        let overrides = vec![
            "SHANNON_MODEL=test-model".to_string(),
            "SHANNON_MAX_TOKENS=4096".to_string(),
            "SHANNON_TEMPERATURE=0.5".to_string(),
        ];
        mgr.apply_env_overrides(&overrides).unwrap();
        assert_eq!(mgr.settings().model, Some("test-model".to_string()));
        assert_eq!(mgr.settings().max_tokens, Some(4096));
        assert_eq!(mgr.settings().temperature, Some(0.5));
    }

    #[test]
    fn test_apply_env_overrides_validates_before_accepting() {
        let mut mgr = SettingsManager::new();
        let overrides = vec![
            "SHANNON_TEMPERATURE=5.0".to_string(), // invalid
        ];
        let result = mgr.apply_env_overrides(&overrides);
        assert!(result.is_err());
    }

    // -- Equality --------------------------------------------------------------

    #[test]
    fn test_settings_equality() {
        let a = Settings::default();
        let b = Settings::default();
        assert_eq!(a, b);
    }

    #[test]
    fn test_settings_inequality() {
        let mut a = Settings::default();
        a.model = Some("different".to_string());
        assert_ne!(a, Settings::default());
    }
}

// ============================================================================
// UnifiedConfig Integration Tests
// ============================================================================

mod unified_config_tests {
    use shannon_core::unified_config::{ConfigBuilder, ShannonConfig};

    // -- ShannonConfig Creation ------------------------------------------------

    #[test]
    fn test_empty_config_all_fields_none() {
        let config = ShannonConfig::empty();
        assert!(config.model.is_none());
        assert!(config.provider.is_none());
        assert!(config.api_key.is_none());
        assert!(config.base_url.is_none());
        assert!(config.max_tokens.is_none());
        assert!(config.temperature.is_none());
        assert!(config.timeout.is_none());
        assert!(!config.debug);
    }

    #[test]
    fn test_default_config_equals_empty() {
        let empty = ShannonConfig::empty();
        let default = ShannonConfig::default();
        assert!(empty.model.is_none());
        assert!(default.model.is_none());
        assert!(!default.debug);
        assert!(!empty.debug);
    }

    #[test]
    fn test_config_with_all_fields_set() {
        let config = ShannonConfig {
            model: Some("claude-opus-4-6".to_string()),
            provider: Some("anthropic".to_string()),
            api_key: Some("sk-test".to_string()),
            base_url: Some("https://api.anthropic.com".to_string()),
            max_tokens: Some(4096),
            temperature: Some(0.7),
            timeout: Some(30),
            debug: true,
        };
        assert_eq!(config.model.as_deref(), Some("claude-opus-4-6"));
        assert_eq!(config.provider.as_deref(), Some("anthropic"));
        assert_eq!(config.api_key.as_deref(), Some("sk-test"));
        assert_eq!(config.base_url.as_deref(), Some("https://api.anthropic.com"));
        assert_eq!(config.max_tokens, Some(4096));
        assert_eq!(config.temperature, Some(0.7));
        assert_eq!(config.timeout, Some(30));
        assert!(config.debug);
    }

    // -- Merge -----------------------------------------------------------------

    #[test]
    fn test_merge_other_overrides_self() {
        let base = ShannonConfig {
            model: Some("base-model".to_string()),
            provider: Some("anthropic".to_string()),
            api_key: Some("base-key".to_string()),
            max_tokens: Some(2048),
            debug: false,
            ..Default::default()
        };

        let other = ShannonConfig {
            model: Some("override-model".to_string()),
            provider: None,
            api_key: None,
            base_url: Some("http://custom".to_string()),
            max_tokens: None,
            temperature: Some(0.5),
            debug: true,
            ..Default::default()
        };

        let merged = base.merge(&other);
        assert_eq!(merged.model, Some("override-model".to_string()));
        assert_eq!(merged.provider, Some("anthropic".to_string())); // kept from base
        assert_eq!(merged.api_key, Some("base-key".to_string())); // kept from base
        assert_eq!(merged.base_url, Some("http://custom".to_string())); // from override
        assert_eq!(merged.max_tokens, Some(2048)); // kept from base
        assert_eq!(merged.temperature, Some(0.5)); // from override
        assert!(merged.debug); // from override
    }

    #[test]
    fn test_merge_none_in_other_preserves_base() {
        let base = ShannonConfig {
            model: Some("kept".to_string()),
            temperature: Some(0.3),
            max_tokens: Some(100),
            ..Default::default()
        };
        let other = ShannonConfig {
            model: None,
            temperature: None,
            max_tokens: None,
            ..Default::default()
        };

        let merged = base.merge(&other);
        assert_eq!(merged.model, Some("kept".to_string()));
        assert_eq!(merged.temperature, Some(0.3));
        assert_eq!(merged.max_tokens, Some(100));
    }

    #[test]
    fn test_merge_both_none_stays_none() {
        let a = ShannonConfig::empty();
        let b = ShannonConfig::empty();
        let merged = a.merge(&b);
        assert!(merged.model.is_none());
        assert!(merged.provider.is_none());
        assert!(merged.api_key.is_none());
        assert!(merged.base_url.is_none());
        assert!(merged.max_tokens.is_none());
        assert!(merged.temperature.is_none());
        assert!(merged.timeout.is_none());
    }

    #[test]
    fn test_merge_debug_is_or_logic() {
        // If either side has debug=true, result should be true
        let a = ShannonConfig { debug: true, ..Default::default() };
        let b = ShannonConfig { debug: false, ..Default::default() };
        let merged = a.merge(&b);
        assert!(merged.debug);

        let a2 = ShannonConfig { debug: false, ..Default::default() };
        let b2 = ShannonConfig { debug: true, ..Default::default() };
        let merged2 = a2.merge(&b2);
        assert!(merged2.debug);

        let a3 = ShannonConfig { debug: false, ..Default::default() };
        let b3 = ShannonConfig { debug: false, ..Default::default() };
        let merged3 = a3.merge(&b3);
        assert!(!merged3.debug);
    }

    #[test]
    fn test_merge_chains_correctly() {
        let global = ShannonConfig {
            model: Some("global".to_string()),
            provider: Some("anthropic".to_string()),
            max_tokens: Some(1024),
            ..Default::default()
        };
        let local = ShannonConfig {
            model: Some("local".to_string()),
            temperature: Some(0.7),
            ..Default::default()
        };
        let env = ShannonConfig {
            api_key: Some("env-key".to_string()),
            max_tokens: Some(2048),
            ..Default::default()
        };

        // global -> local -> env
        let merged = global.merge(&local).merge(&env);
        assert_eq!(merged.model, Some("local".to_string())); // local overrides global
        assert_eq!(merged.provider, Some("anthropic".to_string())); // from global
        assert_eq!(merged.api_key, Some("env-key".to_string())); // from env
        assert_eq!(merged.max_tokens, Some(2048)); // env overrides global
        assert_eq!(merged.temperature, Some(0.7)); // from local
    }

    // -- Serialization Round-Trip -----------------------------------------------

    #[test]
    fn test_config_json_roundtrip_empty() {
        let config = ShannonConfig::empty();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: ShannonConfig = serde_json::from_str(&json).unwrap();
        assert!(parsed.model.is_none());
        assert!(!parsed.debug);
    }

    #[test]
    fn test_config_json_roundtrip_full() {
        let config = ShannonConfig {
            model: Some("test-model".to_string()),
            provider: Some("test-provider".to_string()),
            api_key: Some("test-key".to_string()),
            base_url: Some("http://test".to_string()),
            max_tokens: Some(8192),
            temperature: Some(0.9),
            timeout: Some(60),
            debug: true,
        };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: ShannonConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.model, config.model);
        assert_eq!(parsed.provider, config.provider);
        assert_eq!(parsed.api_key, config.api_key);
        assert_eq!(parsed.base_url, config.base_url);
        assert_eq!(parsed.max_tokens, config.max_tokens);
        assert_eq!(parsed.temperature, config.temperature);
        assert_eq!(parsed.timeout, config.timeout);
        assert_eq!(parsed.debug, config.debug);
    }

    // -- ConfigBuilder ---------------------------------------------------------

    #[test]
    fn test_builder_new_starts_empty() {
        let builder = ConfigBuilder::new();
        let config = builder.build();
        assert!(config.model.is_none());
        assert!(config.provider.is_none());
        assert!(!config.debug);
    }

    #[test]
    fn test_builder_default_trait() {
        let builder = ConfigBuilder::default();
        let config = builder.build();
        assert!(config.model.is_none());
    }

    #[test]
    fn test_builder_with_cli_overrides() {
        let mut builder = ConfigBuilder::new();
        builder.set_cli_overrides(ShannonConfig {
            model: Some("cli-model".to_string()),
            debug: true,
            ..Default::default()
        });
        let config = builder.build();
        assert_eq!(config.model, Some("cli-model".to_string()));
        assert!(config.debug);
    }

    #[test]
    fn test_builder_empty_sources_produces_empty_config() {
        let config = ConfigBuilder::new().build();
        assert!(config.model.is_none());
        assert!(config.provider.is_none());
        assert!(config.api_key.is_none());
        assert!(config.base_url.is_none());
        assert!(config.max_tokens.is_none());
        assert!(config.temperature.is_none());
        assert!(config.timeout.is_none());
    }

    // -- ConfigBuilder with File Loading ----------------------------------------

    #[test]
    fn test_builder_load_local_toml_from_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join(".shannon.toml");

        // Write a simple key=value config
        std::fs::write(
            &toml_path,
            r#"model = "file-model"
provider = "openai"
max_tokens = 4096
temperature = 0.3
"#,
        )
        .unwrap();

        // ConfigBuilder::load_local_toml reads from ".shannon.toml" in CWD,
        // so we need to change directory temporarily
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let mut builder = ConfigBuilder::new();
        builder.load_local_toml();
        let config = builder.build();

        std::env::set_current_dir(&original_dir).unwrap();

        assert_eq!(config.model, Some("file-model".to_string()));
        assert_eq!(config.provider, Some("openai".to_string()));
        assert_eq!(config.max_tokens, Some(4096));
        assert_eq!(config.temperature, Some(0.3));
    }

    #[test]
    fn test_builder_load_local_toml_missing_file_uses_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let mut builder = ConfigBuilder::new();
        builder.load_local_toml();
        let config = builder.build();

        std::env::set_current_dir(&original_dir).unwrap();

        assert!(config.model.is_none());
    }

    #[test]
    fn test_builder_load_local_toml_json_file() {
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join(".shannon.toml");

        // Write JSON format (the loader tries JSON first)
        let json_content = serde_json::to_string(&ShannonConfig {
            model: Some("json-model".to_string()),
            provider: Some("anthropic".to_string()),
            ..Default::default()
        })
        .unwrap();
        std::fs::write(&toml_path, &json_content).unwrap();

        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let mut builder = ConfigBuilder::new();
        builder.load_local_toml();
        let config = builder.build();

        std::env::set_current_dir(&original_dir).unwrap();

        assert_eq!(config.model, Some("json-model".to_string()));
        assert_eq!(config.provider, Some("anthropic".to_string()));
    }

    #[test]
    fn test_builder_load_local_toml_with_comments_and_blanks() {
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join(".shannon.toml");

        std::fs::write(
            &toml_path,
            r#"# This is a comment
model = "commented-model"

# Another comment
provider = "ollama"
"#,
        )
        .unwrap();

        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let mut builder = ConfigBuilder::new();
        builder.load_local_toml();
        let config = builder.build();

        std::env::set_current_dir(&original_dir).unwrap();

        assert_eq!(config.model, Some("commented-model".to_string()));
        assert_eq!(config.provider, Some("ollama".to_string()));
    }

    #[test]
    fn test_builder_load_local_toml_quoted_values() {
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join(".shannon.toml");

        std::fs::write(
            &toml_path,
            r#"model = "quoted-model"
debug = true
"#,
        )
        .unwrap();

        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let mut builder = ConfigBuilder::new();
        builder.load_local_toml();
        let config = builder.build();

        std::env::set_current_dir(&original_dir).unwrap();

        assert_eq!(config.model, Some("quoted-model".to_string()));
        assert!(config.debug);
    }

    // -- Env Vars via Builder ---------------------------------------------------

    #[test]
    fn test_builder_load_env_vars_reads_shannon_vars() {
        // Set env vars
        let cleanup = vec!["SHANNON_MODEL", "SHANNON_PROVIDER", "SHANNON_MAX_TOKENS"];
        for var in &cleanup {
            unsafe { std::env::remove_var(var); }
        }

        unsafe {
            std::env::set_var("SHANNON_MODEL", "env-model");
            std::env::set_var("SHANNON_PROVIDER", "env-provider");
            std::env::set_var("SHANNON_MAX_TOKENS", "9999");
        }

        let mut builder = ConfigBuilder::new();
        builder.load_env_vars();
        let config = builder.build();

        // Clean up before assertions so they don't leak
        for var in &cleanup {
            unsafe { std::env::remove_var(var); }
        }

        assert_eq!(config.model, Some("env-model".to_string()));
        assert_eq!(config.provider, Some("env-provider".to_string()));
        assert_eq!(config.max_tokens, Some(9999));
    }

    #[test]
    fn test_builder_env_vars_with_debug() {
        unsafe { std::env::remove_var("SHANNON_DEBUG"); }
        unsafe { std::env::set_var("SHANNON_DEBUG", "true"); }

        let mut builder = ConfigBuilder::new();
        builder.load_env_vars();
        let config = builder.build();

        unsafe { std::env::remove_var("SHANNON_DEBUG"); }

        assert!(config.debug);
    }

    #[test]
    fn test_builder_env_vars_missing_vars_produce_none() {
        // Remove all SHANNON_ vars
        let vars = [
            "SHANNON_MODEL", "SHANNON_PROVIDER", "SHANNON_API_KEY",
            "SHANNON_BASE_URL", "SHANNON_MAX_TOKENS", "SHANNON_TEMPERATURE",
            "SHANNON_TIMEOUT", "SHANNON_DEBUG",
        ];
        for var in &vars {
            unsafe { std::env::remove_var(var); }
        }

        let mut builder = ConfigBuilder::new();
        builder.load_env_vars();
        let config = builder.build();

        assert!(config.model.is_none());
        assert!(config.provider.is_none());
        assert!(config.api_key.is_none());
        assert!(config.base_url.is_none());
        assert!(config.max_tokens.is_none());
        assert!(config.temperature.is_none());
        assert!(config.timeout.is_none());
        assert!(!config.debug);
    }

    // -- Full Builder Priority: TOML + Env + CLI --------------------------------

    #[test]
    fn test_full_priority_toml_then_env_then_cli() {
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join(".shannon.toml");

        // Write TOML
        std::fs::write(
            &toml_path,
            r#"model = "toml-model"
provider = "toml-provider"
max_tokens = 1024
"#,
        )
        .unwrap();

        // Set env
        unsafe {
            std::env::remove_var("SHANNON_MODEL");
            std::env::remove_var("SHANNON_MAX_TOKENS");
            std::env::set_var("SHANNON_MODEL", "env-model");
            std::env::set_var("SHANNON_MAX_TOKENS", "2048");
        }

        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let mut builder = ConfigBuilder::new();
        builder.load_local_toml();
        builder.load_env_vars();
        builder.set_cli_overrides(ShannonConfig {
            model: Some("cli-model".to_string()),
            debug: true,
            ..Default::default()
        });
        let config = builder.build();

        std::env::set_current_dir(&original_dir).unwrap();
        unsafe {
            std::env::remove_var("SHANNON_MODEL");
            std::env::remove_var("SHANNON_MAX_TOKENS");
        }

        // CLI wins model, TOML provides provider, env wins max_tokens
        assert_eq!(config.model, Some("cli-model".to_string()));
        assert_eq!(config.provider, Some("toml-provider".to_string()));
        assert_eq!(config.max_tokens, Some(2048));
        assert!(config.debug);
    }

    // -- Clone & Debug ----------------------------------------------------------

    #[test]
    fn test_config_is_cloneable() {
        let config = ShannonConfig {
            model: Some("clone-test".to_string()),
            ..Default::default()
        };
        let cloned = config.clone();
        assert_eq!(cloned.model, config.model);
    }

    #[test]
    fn test_config_debug_format() {
        let config = ShannonConfig {
            model: Some("debug-test".to_string()),
            ..Default::default()
        };
        let debug_str = format!("{config:?}");
        assert!(debug_str.contains("debug-test"));
    }
}
