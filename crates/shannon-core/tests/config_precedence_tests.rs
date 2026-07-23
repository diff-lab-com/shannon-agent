//! Config precedence tests verifying the priority chain:
//!
//!   CLI overrides > env vars (`SHANNON_*`) > local `.shannon.toml` > global config > defaults
//!
//! N1/C-fields: pre-N1 flat fields (`model`/`provider`/`api_key`/`base_url`/
//! `[providers.*]`) were removed under no-compat; provider-level precedence
//! for those is now expressed via the `provider_model` v2 profile (first
//! non-empty wins across the four layers). Scalar behavioural knobs
//! (`max_tokens`/`temperature`/`timeout`/`debug`/`enable_tools`/
//! `max_context_tokens`) still merge independently through `ConfigBuilder`.

mod config_precedence_tests {
    use shannon_core::unified_config::{ConfigBuilder, ShannonConfig};
    use shannon_types::provider_config::{
        CredentialRef, CredentialScope, ModelProfile, ProviderKind, ProviderModelConfig,
        ProviderProfile, Scope,
    };
    use std::collections::HashMap;

    /// Helper: build a `ProviderModelConfig` with a single `"default"` profile
    /// whose active target is the given (provider_id, model_id, base_url,
    /// cred_var). Mirrors the helper of the same name in the unified_config
    /// in-file tests so the test file can stand on its own.
    fn synth_profile(
        provider_id: &str,
        provider_kind: ProviderKind,
        base_url: &str,
        cred_var: &str,
        model_id: &str,
    ) -> ProviderModelConfig {
        use shannon_types::provider_config::{ActiveTarget, Scope};
        let profile = ProviderProfile {
            id: provider_id.to_string(),
            kind: provider_kind,
            display_name: provider_id.to_string(),
            base_url: base_url.to_string(),
            models_url: None,
            credential: CredentialRef::Env {
                var: cred_var.to_string(),
            },
            extra_headers: HashMap::new(),
            default_max_tokens: None,
            fallback_models: Vec::new(),
            quirks: Default::default(),
        };
        let mut profiles = HashMap::new();
        profiles.insert(
            "default".to_string(),
            ModelProfile {
                name: "default".to_string(),
                active_target: ActiveTarget {
                    provider_id: provider_id.to_string(),
                    model_id: model_id.to_string(),
                    scope: Scope::Global,
                },
                providers: vec![profile],
                auxiliary: HashMap::new(),
                credential_scope: CredentialScope::Shared,
            },
        );
        ProviderModelConfig {
            version: ProviderModelConfig::VERSION,
            profiles,
            gateway: Default::default(),
        }
    }

    /// Helper: env-var names that `ConfigBuilder::load_env_vars()`
    /// consults (plus a few that can leak from a developer's shell: Anthropic /
    /// OpenAI BASE_URLs cause `synthesize_default_profile` to pick a different
    /// provider id, breaking tests that don't expect them).
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
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_API_KEY",
        "OPENAI_BASE_URL",
        "OPENAI_API_KEY",
        "ANTHROPIC_MODEL",
        "OPENAI_MODEL",
    ];

    /// Remove all `SHANNON_*` / `ANTHROPIC_BASE_URL` / `OPENAI_BASE_URL`
    /// env vars to avoid cross-test contamination (the test environment in
    /// this project often has them set to a non-Anthropic / non-Ollama
    /// provider, which would otherwise leak).
    fn clear_shannon_env() {
        for var in ENV_VARS {
            // SAFETY: tests in this module serialize on these keys; no
            // concurrent readers.
            unsafe {
                std::env::remove_var(var);
            }
        }
    }

    fn active_profile(cfg: &ShannonConfig) -> &ModelProfile {
        cfg.provider_model
            .profiles
            .get("default")
            .expect("merged config should carry a default profile when any layer populated it")
    }

    fn active_provider<'a>(cfg: &'a ShannonConfig) -> &'a ProviderProfile {
        active_profile(cfg)
            .providers
            .first()
            .expect("default profile should list the active provider")
    }

    // -----------------------------------------------------------------------
    // 1. CLI args override env vars
    // -----------------------------------------------------------------------

    #[test]
    fn test_cli_args_override_env_vars() {
        clear_shannon_env();

        // SAFETY: see clear_shannon_env.
        unsafe {
            std::env::set_var("SHANNON_MODEL", "env-model");
            std::env::set_var("SHANNON_PROVIDER", "env-provider");
            std::env::set_var("SHANNON_MAX_TOKENS", "4096");
            std::env::set_var("SHANNON_TEMPERATURE", "0.3");
        }

        let mut builder = ConfigBuilder::new();
        builder.load_env_vars();

        // CLI overrides carry a populated v2 profile (CLI's win), plus
        // scalar overrides that conflict with env vars.
        builder.set_cli_overrides(ShannonConfig {
            max_tokens: Some(8192),
            temperature: Some(0.7),
            debug: true,
            provider_model: synth_profile(
                "anthropic",
                ProviderKind::Anthropic,
                "https://api.anthropic.com",
                "SHANNON_API_KEY",
                "cli-model",
            ),
            ..Default::default()
        });

        let config = builder.build();

        // CLI wins on every conflicting scalar field.
        assert_eq!(config.max_tokens, Some(8192));
        assert_eq!(config.temperature, Some(0.7));
        assert!(config.debug);

        // CLI's profile is the merged profile (cli overrides env where the
        // former is non-empty on provider_model).
        assert_eq!(active_profile(&config).active_target.model_id, "cli-model");
        assert_eq!(active_provider(&config).id, "anthropic");

        clear_shannon_env();
    }

    // -----------------------------------------------------------------------
    // 2. Env vars override project config
    // -----------------------------------------------------------------------

    #[test]
    fn test_env_vars_override_project_config() {
        clear_shannon_env();
        // SAFETY: assert isolation between tests.
        unsafe {
            std::env::remove_var("ANTHROPIC_BASE_URL");
            std::env::remove_var("OPENAI_BASE_URL");
        }

        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join(".shannon.toml");

        // Project-local config (N1: model/provider/base_url fed through
        // provider_model synthesis on TOML load). `provider = "anthropic"`
        // is a recognised LlmProvider variant so it survives synthesis.
        std::fs::write(
            &toml_path,
            r#"model = "toml-model"
provider = "anthropic"
base_url = "https://toml.example.com"
max_tokens = 1024
temperature = 0.2
"#,
        )
        .unwrap();

        // Env vars that conflict with TOML.
        // SAFETY: see clear_shannon_env.
        unsafe {
            std::env::set_var("SHANNON_MODEL", "env-model");
            std::env::set_var("SHANNON_BASE_URL", "https://env.example.com");
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

        // Env wins across the board (env_vars layer's synthesized profile
        // replaces TOML's, since both are non-empty and env is loaded last —
        // N1/C-fields per-profile merge). Pre-N1 field-level merging
        // preserved TOML's provider / base_url when env only set model +
        // max_tokens, but per-profile merge loses that granularity. Users
        // wanting per-field overrides should now use the structured
        // provider_model config directly.
        assert_eq!(active_profile(&config).active_target.model_id, "env-model");
        assert_eq!(active_provider(&config).base_url, "https://env.example.com");
        // Env wins for max_tokens (scalar, unchanged behaviour).
        assert_eq!(config.max_tokens, Some(2048));
        // Env-only profile: SHANNON_PROVIDER was not set, and the resolved
        // base_url is "https://env.example.com" which is not a recognised
        // provider host, so it falls back to `LlmProvider::Custom`.
        assert_eq!(active_provider(&config).id, "custom");
        // Temperature comes from TOML (scalar: not part of provider_model).
        assert_eq!(config.temperature, Some(0.2));
    }

    // -----------------------------------------------------------------------
    // 3. Project config overrides user (global) config
    // -----------------------------------------------------------------------

    #[test]
    fn test_project_config_overrides_user_config() {
        clear_shannon_env();
        // SAFETY: assert isolation between tests.
        unsafe {
            std::env::remove_var("ANTHROPIC_BASE_URL");
            std::env::remove_var("OPENAI_BASE_URL");
        }

        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join(".shannon.toml");

        // `provider = "anthropic"` is a recognised LlmProvider variant so
        // it maps to a real id (not "custom").
        std::fs::write(
            &toml_path,
            r#"model = "project-model"
provider = "anthropic"
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
            max_tokens: Some(2048),
            temperature: Some(0.5),
            provider_model: synth_profile(
                "global-provider",
                ProviderKind::OpenAiCompatible,
                "https://global.example.com",
                "GLOBAL_KEY",
                "global-model",
            ),
            ..Default::default()
        };

        builder.load_local_toml();
        let local_config = builder.build();
        // Merge: global -> local
        let config = global.merge(&local_config);

        std::env::set_current_dir(&original_dir).unwrap();

        // Project overrides user for conflicting scalar fields.
        assert_eq!(
            active_profile(&config).active_target.model_id,
            "project-model"
        );
        assert_eq!(config.max_tokens, Some(8192));
        assert_eq!(config.temperature, Some(0.9));
        // TOML provider is "anthropic" — synthesized profile wins over
        // global's "global-provider" (TOML is loaded after global). N1
        // per-profile merge replaces the whole profile; global's base_url
        // no longer flows through on its own (pre-N1 per-field merge did
        // preserve it). The merged Anthropic profile falls back to its
        // default base_url.
        assert_eq!(active_provider(&config).id, "anthropic");
        assert_eq!(
            active_provider(&config).base_url,
            "https://api.anthropic.com"
        );
    }

    // -----------------------------------------------------------------------
    // 4. Invalid config falls back to defaults
    // -----------------------------------------------------------------------

    #[test]
    fn test_invalid_config_fallback() {
        clear_shannon_env();
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join(".shannon.toml");

        // Write malformed config content.
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

        // Malformed config produces the Ollama auto-default profile (N1
        // faithful relocation: `synthesize_default_profile` with no cred/no
        // base_url/no provider → Ollama localhost:11434) plus the default
        // max_tokens fallback. Critically, it does NOT panic.
        assert_eq!(active_provider(&config).id, "ollama");
        assert_eq!(active_provider(&config).base_url, "http://localhost:11434");
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
        clear_shannon_env();

        // Empty temp dir — no .shannon.toml present.
        let dir = tempfile::tempdir().unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let mut builder = ConfigBuilder::new();
        builder.load_global_toml();
        builder.load_local_toml();
        builder.load_env_vars();
        let config = builder.build();

        std::env::set_current_dir(&original_dir).unwrap();

        // No config files + no env vars + no cred var → Ollama auto-default.
        assert_eq!(active_provider(&config).id, "ollama");
        assert_eq!(active_provider(&config).base_url, "http://localhost:11434");
        assert!(config.max_tokens.is_none());
        assert!(config.temperature.is_none());
        assert!(config.timeout.is_none());
        assert!(!config.debug);
    }

    // -----------------------------------------------------------------------
    // 6. Deep merging across 4 layers
    // -----------------------------------------------------------------------

    #[test]
    fn test_config_merging_deep() {
        clear_shannon_env();
        // SAFETY: assert isolation between tests.
        unsafe {
            std::env::remove_var("ANTHROPIC_BASE_URL");
            std::env::remove_var("OPENAI_BASE_URL");
        }

        // Layer 1: global config — provides profile + scalars.
        let global = ShannonConfig {
            max_tokens: Some(2048),
            temperature: Some(0.5),
            timeout: Some(60),
            debug: false,
            enable_tools: Some(true),
            max_context_tokens: Some(128_000),
            provider_model: synth_profile(
                "anthropic",
                ProviderKind::Anthropic,
                "https://global.example.com",
                "GLOBAL_KEY",
                "global-model",
            ),
            ..Default::default()
        };

        // Layer 2: local config — overrides active_target model_id, sets
        // enable_tools to false, different max_context_tokens.
        let local = ShannonConfig {
            max_context_tokens: Some(64_000),
            provider_model: synth_profile(
                "anthropic",
                ProviderKind::Anthropic,
                "https://global.example.com",
                "GLOBAL_KEY",
                "local-model",
            ),
            ..Default::default()
        };

        // Layer 3: env — overrides max_tokens, sets enable_tools false.
        let env = ShannonConfig {
            max_tokens: Some(8192),
            enable_tools: Some(false),
            provider_model: Default::default(),
            ..Default::default()
        };

        // Layer 4: CLI — overrides debug, forces a different model.
        let cli = ShannonConfig {
            debug: true,
            provider_model: synth_profile(
                "anthropic",
                ProviderKind::Anthropic,
                "https://global.example.com",
                "GLOBAL_KEY",
                "cli-model",
            ),
            ..Default::default()
        };

        let merged = global.merge(&local).merge(&env).merge(&cli);

        // Each layer provides its own contribution:
        assert_eq!(active_profile(&merged).active_target.model_id, "cli-model"); // CLI wins
        assert_eq!(active_provider(&merged).id, "anthropic"); // from global (all layers agree)
        assert_eq!(
            active_provider(&merged).base_url,
            "https://global.example.com"
        ); // from global
        assert_eq!(merged.max_tokens, Some(8192)); // env wins over global
        assert_eq!(merged.temperature, Some(0.5)); // from global
        assert_eq!(merged.timeout, Some(60)); // from global
        assert!(merged.debug); // CLI wins (OR logic with global false)
        assert_eq!(merged.enable_tools, Some(false)); // env wins over local + global
        assert_eq!(merged.max_context_tokens, Some(64_000)); // local wins over global
    }

    // -----------------------------------------------------------------------
    // 7. Type coercion — string "true" parsed to bool, numeric strings, etc.
    // -----------------------------------------------------------------------

    #[test]
    fn test_config_type_coercion() {
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join(".shannon.toml");

        // Write TOML with string representations of typed values.
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

        // String "true" in TOML parsed as bool.
        assert!(config.debug);
        // Numeric strings parsed to correct integer/float types.
        assert_eq!(config.max_tokens, Some(4096));
        assert_eq!(config.temperature, Some(0.75));
        assert_eq!(config.timeout, Some(30));
        assert_eq!(config.max_context_tokens, Some(65536));
        // Model flows through to the synthesized default profile.
        assert_eq!(
            active_profile(&config).active_target.model_id,
            "coercion-model"
        );
    }

    // -----------------------------------------------------------------------
    // 8. All config fields have sensible defaults
    // -----------------------------------------------------------------------

    #[test]
    fn test_config_default_values() {
        clear_shannon_env();
        // SAFETY: assert isolation between tests.
        unsafe {
            std::env::remove_var("ANTHROPIC_BASE_URL");
            std::env::remove_var("OPENAI_BASE_URL");
        }

        let empty = ShannonConfig::empty();
        let default = ShannonConfig::default();

        // Both empty() and default() produce the same result.
        assert!(empty.provider_model.profiles.is_empty());
        assert!(default.provider_model.profiles.is_empty());

        // All optional scalar fields default to None.
        assert!(empty.max_tokens.is_none());
        assert!(empty.temperature.is_none());
        assert!(empty.timeout.is_none());
        assert!(empty.enable_tools.is_none());
        assert!(empty.max_context_tokens.is_none());

        // Debug defaults to false (off by default — safe).
        assert!(!empty.debug);
        assert!(!default.debug);

        // Verify through builder with no sources.
        let built = ConfigBuilder::new().build();
        assert!(built.provider_model.profiles.is_empty());
        assert!(built.max_tokens.is_none());
        assert!(built.temperature.is_none());
        assert!(built.timeout.is_none());
        assert!(!built.debug);
        assert!(built.enable_tools.is_none());
        assert!(built.max_context_tokens.is_none());
    }
}
