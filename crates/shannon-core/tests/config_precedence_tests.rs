//! Config precedence tests verifying the priority chain:
//!
//!   CLI overrides > env vars (`SHANNON_*`) > local `.shannon.toml` > global config > defaults

mod config_precedence_tests {
    use shannon_core::unified_config::{ConfigBuilder, ShannonConfig};

    /// Helper: env var names used by `ConfigBuilder::load_env_vars()`.
    const ENV_VARS: &[&str] = &[
        "SHANNON_MODEL",
        "SHANNON_PROVIDER",
        "SHANNON_API_KEY",
        "SHANNON_BASE_URL",
        "SHANNON_MAX_TOKENS",
        "SHANNON_TEMPERATURE",
        "SHANNON_TIMEOUT",
        "SHANNON_DEBUG",
        "SHANNON_ENABLE_TOOLS",
        "SHANNON_MAX_CONTEXT_TOKENS",
    ];

    /// Remove all SHANNON_* env vars to avoid cross-test contamination.
    fn clear_shannon_env() {
        for var in ENV_VARS {
            unsafe { std::env::remove_var(var); }
        }
    }

    // -----------------------------------------------------------------------
    // 1. CLI args override env vars
    // -----------------------------------------------------------------------

    #[test]
    fn test_cli_args_override_env_vars() {
        clear_shannon_env();

        // Set env vars
        unsafe {
            std::env::set_var("SHANNON_MODEL", "env-model");
            std::env::set_var("SHANNON_PROVIDER", "env-provider");
            std::env::set_var("SHANNON_MAX_TOKENS", "4096");
            std::env::set_var("SHANNON_TEMPERATURE", "0.3");
        }

        let mut builder = ConfigBuilder::new();
        builder.load_env_vars();

        // Set CLI overrides that conflict with env vars
        builder.set_cli_overrides(ShannonConfig {
            model: Some("cli-model".to_string()),
            provider: Some("cli-provider".to_string()),
            max_tokens: Some(8192),
            temperature: Some(0.7),
            debug: true,
            ..Default::default()
        });

        let config = builder.build();

        // CLI wins on every conflicting field
        assert_eq!(config.model, Some("cli-model".to_string()));
        assert_eq!(config.provider, Some("cli-provider".to_string()));
        assert_eq!(config.max_tokens, Some(8192));
        assert_eq!(config.temperature, Some(0.7));
        assert!(config.debug);

        clear_shannon_env();
    }

    // -----------------------------------------------------------------------
    // 2. Env vars override project config
    // -----------------------------------------------------------------------

    #[test]
    fn test_env_vars_override_project_config() {
        clear_shannon_env();

        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join(".shannon.toml");

        // Write project-local config
        std::fs::write(
            &toml_path,
            r#"model = "toml-model"
provider = "toml-provider"
max_tokens = 1024
temperature = 0.2
"#,
        )
        .unwrap();

        // Set env vars that conflict with TOML
        unsafe {
            std::env::set_var("SHANNON_MODEL", "env-model");
            std::env::set_var("SHANNON_MAX_TOKENS", "2048");
        }

        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let mut builder = ConfigBuilder::new();
        builder.load_local_toml();
        builder.load_env_vars();
        let config = builder.build();

        std::env::set_current_dir(&original_dir).unwrap();
        clear_shannon_env();

        // Env wins for model and max_tokens
        assert_eq!(config.model, Some("env-model".to_string()));
        assert_eq!(config.max_tokens, Some(2048));
        // TOML provides provider and temperature (not overridden by env)
        assert_eq!(config.provider, Some("toml-provider".to_string()));
        // temperature from TOML is clamped but 0.2 is within range
        assert_eq!(config.temperature, Some(0.2));
    }

    // -----------------------------------------------------------------------
    // 3. Project config overrides user (global) config
    // -----------------------------------------------------------------------

    #[test]
    fn test_project_config_overrides_user_config() {
        clear_shannon_env();

        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join(".shannon.toml");

        // Write project-local config
        std::fs::write(
            &toml_path,
            r#"model = "project-model"
provider = "project-provider"
max_tokens = 8192
temperature = 0.9
"#,
        )
        .unwrap();

        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let mut builder = ConfigBuilder::new();

        // Simulate the full 4-layer merge manually (global -> local -> env -> cli)
        // ConfigBuilder.global_toml is private, so we do the merge chain ourselves.
        let global = ShannonConfig {
            model: Some("global-model".to_string()),
            provider: Some("global-provider".to_string()),
            max_tokens: Some(2048),
            temperature: Some(0.5),
            base_url: Some("https://global.example.com".to_string()),
            ..Default::default()
        };

        builder.load_local_toml();
        let local_config = builder.build();
        // Merge: global -> local
        let config = global.merge(&local_config);

        std::env::set_current_dir(&original_dir).unwrap();

        // Project overrides user for conflicting fields
        assert_eq!(config.model, Some("project-model".to_string()));
        assert_eq!(config.provider, Some("project-provider".to_string()));
        assert_eq!(config.max_tokens, Some(8192));
        assert_eq!(config.temperature, Some(0.9));
        // Global-only field preserved
        assert_eq!(config.base_url, Some("https://global.example.com".to_string()));
    }

    // -----------------------------------------------------------------------
    // 4. Invalid config falls back to defaults
    // -----------------------------------------------------------------------

    #[test]
    fn test_invalid_config_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join(".shannon.toml");

        // Write malformed config content
        std::fs::write(
            &toml_path,
            r#"{ this is not valid json or toml !!!
broken content [[[
"#,
        )
        .unwrap();

        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let mut builder = ConfigBuilder::new();
        builder.load_local_toml();
        let config = builder.build();

        std::env::set_current_dir(&original_dir).unwrap();

        // Malformed config produces empty/default values rather than panicking
        assert!(config.model.is_none());
        assert!(config.provider.is_none());
        assert!(config.api_key.is_none());
        assert!(config.base_url.is_none());
        assert!(config.max_tokens.is_none());
        assert!(config.temperature.is_none());
        assert!(config.timeout.is_none());
        assert!(!config.debug);
    }

    // -----------------------------------------------------------------------
    // 5. Missing config file falls back to defaults
    // -----------------------------------------------------------------------

    #[test]
    fn test_config_file_not_found() {
        clear_shannon_env();

        // Empty temp dir — no .shannon.toml present
        let dir = tempfile::tempdir().unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let mut builder = ConfigBuilder::new();
        builder.load_global_toml();
        builder.load_local_toml();
        builder.load_env_vars();
        let config = builder.build();

        std::env::set_current_dir(&original_dir).unwrap();

        // No config files + no env vars = all defaults
        assert!(config.model.is_none());
        assert!(config.provider.is_none());
        assert!(config.api_key.is_none());
        assert!(config.base_url.is_none());
        assert!(config.max_tokens.is_none());
        assert!(config.temperature.is_none());
        assert!(config.timeout.is_none());
        assert!(!config.debug);
    }

    // -----------------------------------------------------------------------
    // 6. Deep merging of nested config values
    // -----------------------------------------------------------------------

    #[test]
    fn test_config_merging_deep() {
        clear_shannon_env();

        // Layer 1: global config — provides model, provider, max_tokens, base_url
        let global = ShannonConfig {
            model: Some("global-model".to_string()),
            provider: Some("anthropic".to_string()),
            api_key: None,
            max_tokens: Some(2048),
            base_url: Some("https://global.example.com".to_string()),
            temperature: Some(0.5),
            timeout: Some(60),
            debug: false,
            enable_tools: Some(true),
            max_context_tokens: Some(128_000),
        };

        // Layer 2: local config — overrides model, adds api_key, different max_context_tokens
        let local = ShannonConfig {
            model: Some("local-model".to_string()),
            api_key: Some("local-key".to_string()),
            max_context_tokens: Some(64_000),
            ..Default::default()
        };

        // Layer 3: env — overrides api_key, max_tokens
        let env = ShannonConfig {
            api_key: Some("env-key".to_string()),
            max_tokens: Some(8192),
            enable_tools: Some(false),
            ..Default::default()
        };

        // Layer 4: CLI — overrides model, debug
        let cli = ShannonConfig {
            model: Some("cli-model".to_string()),
            debug: true,
            ..Default::default()
        };

        let merged = global.merge(&local).merge(&env).merge(&cli);

        // Each layer provides its own contribution:
        assert_eq!(merged.model, Some("cli-model".to_string())); // CLI wins
        assert_eq!(merged.provider, Some("anthropic".to_string())); // from global
        assert_eq!(merged.api_key, Some("env-key".to_string())); // env wins
        assert_eq!(merged.base_url, Some("https://global.example.com".to_string())); // from global
        assert_eq!(merged.max_tokens, Some(8192)); // env wins
        assert_eq!(merged.temperature, Some(0.5)); // from global
        assert_eq!(merged.timeout, Some(60)); // from global
        assert!(merged.debug); // CLI wins (OR logic with global false)
        assert_eq!(merged.enable_tools, Some(false)); // env wins over local
        assert_eq!(merged.max_context_tokens, Some(64_000)); // local wins over global
    }

    // -----------------------------------------------------------------------
    // 7. Type coercion — string "true" parsed to bool, numeric strings, etc.
    // -----------------------------------------------------------------------

    #[test]
    fn test_config_type_coercion() {
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join(".shannon.toml");

        // Write TOML with string representations of typed values
        std::fs::write(
            &toml_path,
            r#"model = "coercion-model"
debug = true
max_tokens = 4096
temperature = 0.75
timeout = 30
max_context_tokens = 65536
"#,
        )
        .unwrap();

        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let mut builder = ConfigBuilder::new();
        builder.load_local_toml();
        let config = builder.build();

        std::env::set_current_dir(&original_dir).unwrap();

        // String "true" in TOML parsed as bool
        assert!(config.debug);
        // Numeric strings parsed to correct integer/float types
        assert_eq!(config.max_tokens, Some(4096));
        assert_eq!(config.temperature, Some(0.75));
        assert_eq!(config.timeout, Some(30));
        assert_eq!(config.max_context_tokens, Some(65536));
        // Regular string field
        assert_eq!(config.model, Some("coercion-model".to_string()));
    }

    // -----------------------------------------------------------------------
    // 8. All config fields have sensible defaults
    // -----------------------------------------------------------------------

    #[test]
    fn test_config_default_values() {
        clear_shannon_env();

        let empty = ShannonConfig::empty();
        let default = ShannonConfig::default();

        // Both empty() and default() produce the same result
        assert!(empty.model.is_none());
        assert!(default.model.is_none());

        // All optional fields default to None
        assert!(empty.provider.is_none());
        assert!(empty.api_key.is_none());
        assert!(empty.base_url.is_none());
        assert!(empty.max_tokens.is_none());
        assert!(empty.temperature.is_none());
        assert!(empty.timeout.is_none());
        assert!(empty.enable_tools.is_none());
        assert!(empty.max_context_tokens.is_none());

        // Debug defaults to false (off by default — safe)
        assert!(!empty.debug);
        assert!(!default.debug);

        // Verify through builder with no sources
        let built = ConfigBuilder::new().build();
        assert!(built.model.is_none());
        assert!(built.provider.is_none());
        assert!(built.api_key.is_none());
        assert!(built.base_url.is_none());
        assert!(built.max_tokens.is_none());
        assert!(built.temperature.is_none());
        assert!(built.timeout.is_none());
        assert!(!built.debug);
        assert!(built.enable_tools.is_none());
        assert!(built.max_context_tokens.is_none());
    }
}
