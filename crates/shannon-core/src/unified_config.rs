//! Unified configuration system with priority-based merging.
//!
//! Configuration sources (highest to lowest priority):
//! 1. CLI arguments (explicit overrides)
//! 2. Environment variables (`SHANNON_*`)
//! 3. Project-local config (`.shannon.toml`)
//! 4. Global config (`~/.shannon/config.toml`)
//! 5. Default values
//!
//! ## v2-native (N1 / C-fields)
//! As of N1, [`ShannonConfig`] carries only the multi-provider/model
//! [`ProviderModelConfig`](shannon_types::provider_config::ProviderModelConfig)
//! in its `provider_model` field. The pre-N1 flat fields
//! (`model`/`provider`/`api_key`/`base_url`/`[providers.*]`) have been
//! removed under the no-compat policy (shannon-code/desktop were unreleased —
//! see [[no-public-release-no-compat]]). Configuration previously set on the
//! flat fields is now expressed as a default [`ProviderProfile`] inside
//! `provider_model`, synthesized from CLI/TOML/env inputs by
//! [`crate::provider_resolver::synthesize_default_profile`]. Credentials are
//! A1-strict: only [`CredentialRef::Env`](shannon_types::provider_config::CredentialRef::Env)
//! references, never plaintext in the config (plaintext values live in
//! `~/.shannon/secrets.env` via [`crate::config_migration::persist_secrets`]).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::notifier::NotificationsConfig;
use shannon_engine::api::types::LlmProvider;

/// A conversation preset with pre-configured settings.
/// Duplicated here to avoid a circular dependency on shannon-commands.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PresetEntry {
    /// Custom system prompt addition.
    pub system_prompt: Option<String>,
    /// Initial message to inject.
    pub initial_message: Option<String>,
    /// Model override.
    pub model: Option<String>,
    /// Temperature override.
    pub temperature: Option<f32>,
    /// Max tokens override.
    pub max_tokens: Option<usize>,
    /// Tools whitelist.
    pub tools: Option<Vec<String>>,
    /// Description for display.
    pub description: Option<String>,
}

/// Unified Shannon configuration.
///
/// Carries multi-provider/model v2 config in [`Self::provider_model`] — see
/// the module docs for the flat→profile mapping.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ShannonConfig {
    pub max_tokens: Option<usize>,
    pub temperature: Option<f32>,
    pub timeout: Option<u64>,
    #[serde(default)]
    pub debug: bool,
    /// Override tool calling: Some(true) = force tools on, Some(false) = force off, None = auto.
    pub enable_tools: Option<bool>,
    /// Maximum context tokens before compression. Overrides model registry defaults.
    /// Priority: user config > Ollama num_ctx > model registry > fallback (128K).
    pub max_context_tokens: Option<usize>,
    /// User-defined conversation presets from config files.
    #[serde(default)]
    pub presets: Option<HashMap<String, PresetEntry>>,
    /// Permission profile name: "strict", "balanced", "permissive", or "custom:<name>".
    #[serde(default)]
    pub permission_profile: Option<String>,
    /// `[notifications]` section for system-level notification behavior.
    #[serde(default)]
    pub notifications: Option<NotificationsConfig>,
    /// v2 multi-provider/model config. The `"default"` profile's active
    /// target, when present, drives the engine `LlmClientConfig`. CLI / TOML
    /// / env inputs feed this through
    /// [`crate::provider_resolver::synthesize_default_profile`].
    #[serde(default)]
    pub provider_model: shannon_types::provider_config::ProviderModelConfig,
}

impl ShannonConfig {
    /// Create an empty config with all fields set to None.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Merge another config on top of this one.
    /// Values from `other` take precedence if they are `Some`.
    pub fn merge(&self, other: &ShannonConfig) -> ShannonConfig {
        // Merge presets: other's entries overlay on top of self's.
        let presets = match (&self.presets, &other.presets) {
            (None, None) => None,
            (Some(a), None) => Some(a.clone()),
            (None, Some(b)) => Some(b.clone()),
            (Some(a), Some(b)) => {
                let mut merged = a.clone();
                for (k, v) in b {
                    merged.insert(k.clone(), v.clone());
                }
                Some(merged)
            }
        };

        // v2 provider_model — first-non-empty wins (CLI > env > TOML > global).
        let provider_model = if !other.provider_model.profiles.is_empty() {
            other.provider_model.clone()
        } else {
            self.provider_model.clone()
        };

        ShannonConfig {
            max_tokens: other.max_tokens.or(self.max_tokens),
            temperature: other.temperature.or(self.temperature),
            timeout: other.timeout.or(self.timeout),
            debug: other.debug || self.debug,
            enable_tools: other.enable_tools.or(self.enable_tools),
            max_context_tokens: other.max_context_tokens.or(self.max_context_tokens),
            presets,
            permission_profile: other
                .permission_profile
                .clone()
                .or_else(|| self.permission_profile.clone()),
            notifications: other
                .notifications
                .clone()
                .or_else(|| self.notifications.clone()),
            provider_model,
        }
    }

    /// Parse the `permission_profile` field into a [`PermissionProfile`].
    ///
    /// Returns `None` if the field is unset or contains an unrecognised value.
    pub fn resolve_permission_profile(
        &self,
    ) -> Option<shannon_engine::permission_profile::PermissionProfile> {
        self.permission_profile
            .as_deref()
            .and_then(shannon_engine::permission_profile::PermissionProfile::from_str_lossy)
    }

    /// Resolve an API key for the given provider against the v2 active target.
    ///
    /// N1/C-fields: the v1 flat `api_key`/`[providers.*]` fields are gone.
    /// If the v2 active target resolves to the same provider, its
    /// [`CredentialRef`](shannon_types::provider_config::CredentialRef) is
    /// consulted (decision A1: env-only for now). Otherwise the provider's
    /// own env chain is consulted.
    pub fn resolve_api_key_for_provider(&self, provider: &LlmProvider) -> String {
        if let Some(rt) = crate::provider_resolver::resolve_active_target(&self.provider_model) {
            if rt.provider == *provider {
                let resolved = crate::provider_resolver::resolve_credential(&rt.profile.credential);
                if !resolved.is_empty() {
                    return resolved;
                }
            }
        }
        provider.resolve_api_key_from_env()
    }
}

/// Builder for constructing a merged configuration from multiple sources.
pub struct ConfigBuilder {
    global_toml: ShannonConfig,
    local_toml: ShannonConfig,
    env_vars: ShannonConfig,
    cli_overrides: ShannonConfig,
}

impl ConfigBuilder {
    /// Create a new config builder.
    pub fn new() -> Self {
        Self {
            global_toml: ShannonConfig::empty(),
            local_toml: ShannonConfig::empty(),
            env_vars: ShannonConfig::empty(),
            cli_overrides: ShannonConfig::empty(),
        }
    }

    /// Load global TOML config from `~/.shannon/config.toml`.
    pub fn load_global_toml(&mut self) -> &mut Self {
        if let Some(home) = dirs::home_dir() {
            let path = home.join(".shannon").join("config.toml");
            self.global_toml = load_config_file(&path);
        }
        self
    }

    /// Load project-local TOML config from `.shannon.toml`.
    pub fn load_local_toml(&mut self) -> &mut Self {
        let path = std::path::Path::new(".shannon.toml");
        let local = load_config_file(path);
        self.local_toml = local;
        self
    }

    /// Load configuration from environment variables (`SHANNON_*`).
    ///
    /// N1/C-fields: `SHANNON_MODEL` / `SHANNON_PROVIDER` / `SHANNON_BASE_URL`
    /// (plus the env-fallback chain inside
    /// [`crate::provider_resolver::synthesize_default_profile`]) populate
    /// a default v2 profile inside `provider_model`. The pre-N1 flat fields
    /// are gone.
    pub fn load_env_vars(&mut self) -> &mut Self {
        let provider_model = crate::provider_resolver::synthesize_default_profile(
            std::env::var("SHANNON_MODEL").ok().as_deref(),
            std::env::var("SHANNON_PROVIDER").ok().as_deref(),
            std::env::var("SHANNON_BASE_URL").ok().as_deref(),
            None,
        )
        .unwrap_or_default();
        self.env_vars = ShannonConfig {
            max_tokens: std::env::var("SHANNON_MAX_TOKENS")
                .ok()
                .and_then(|v| v.parse().ok()),
            temperature: std::env::var("SHANNON_TEMPERATURE")
                .ok()
                .and_then(|v| v.parse().ok()),
            timeout: std::env::var("SHANNON_TIMEOUT")
                .ok()
                .and_then(|v| v.parse().ok()),
            debug: std::env::var("SHANNON_DEBUG").is_ok(),
            enable_tools: std::env::var("SHANNON_ENABLE_TOOLS")
                .ok()
                .and_then(|v| v.parse().ok()),
            max_context_tokens: std::env::var("SHANNON_MAX_CONTEXT_TOKENS")
                .ok()
                .and_then(|v| v.parse().ok()),
            permission_profile: std::env::var("SHANNON_PERMISSION_PROFILE").ok(),
            presets: None,
            notifications: None,
            provider_model,
        };
        self
    }

    /// Set CLI argument overrides (highest priority).
    pub fn set_cli_overrides(&mut self, config: ShannonConfig) -> &mut Self {
        self.cli_overrides = config;
        self
    }

    /// Build the final merged configuration.
    ///
    /// Priority (highest to lowest):
    /// CLI overrides > env vars > local TOML > global TOML
    pub fn build(&self) -> ShannonConfig {
        let mut config = self
            .global_toml
            .merge(&self.local_toml)
            .merge(&self.env_vars)
            .merge(&self.cli_overrides);

        // Clamp temperature to valid range for all LLM providers.
        if let Some(t) = config.temperature {
            config.temperature = Some(t.clamp(0.0, 2.0));
        }
        // Ensure max_tokens is within reasonable bounds.
        if let Some(mt) = config.max_tokens {
            if mt == 0 {
                config.max_tokens = None;
            } else {
                config.max_tokens = Some(mt.min(128_000));
            }
        }

        config
    }
}

impl Default for ConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Load a config file (TOML or JSON), returning an empty config if the file doesn't exist or is invalid.
///
/// Note: This uses serde_json for parsing. For TOML files in the CLI crate,
/// use the dedicated TOML parser there and pass the result via `set_cli_overrides`.
fn load_config_file(path: &std::path::Path) -> ShannonConfig {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return ShannonConfig::empty(),
    };

    // Try JSON first
    if let Ok(config) = serde_json::from_str::<ShannonConfig>(&content) {
        return config;
    }

    // If it's a TOML file, try simple key=value parsing for common fields
    // (Full TOML support requires the `toml` crate, available in shannon-cli).
    // N1/C-fields: `model`/`provider`/`base_url` populate
    // `provider_model` via [`crate::provider_resolver::synthesize_default_profile`].
    let mut model: Option<String> = None;
    let mut provider: Option<String> = None;
    let mut base_url: Option<String> = None;
    let mut max_tokens: Option<usize> = None;
    let mut temperature: Option<f32> = None;
    let mut timeout: Option<u64> = None;
    let mut max_context_tokens: Option<usize> = None;
    let mut debug: bool = false;
    let mut permission_profile: Option<String> = None;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim().trim_matches('"');
            match key {
                "model" => model = Some(value.to_string()),
                "provider" => provider = Some(value.to_string()),
                "base_url" => base_url = Some(value.to_string()),
                "max_tokens" => {
                    if let Ok(v) = value.parse() {
                        max_tokens = Some(v);
                    } else {
                        tracing::warn!("Invalid max_tokens value in config: {value}");
                    }
                }
                "temperature" => {
                    if let Ok(v) = value.parse() {
                        temperature = Some(v);
                    } else {
                        tracing::warn!("Invalid temperature value in config: {value}");
                    }
                }
                "timeout" => {
                    if let Ok(v) = value.parse() {
                        timeout = Some(v);
                    } else {
                        tracing::warn!("Invalid timeout value in config: {value}");
                    }
                }
                "debug" => debug = value.parse().unwrap_or(false),
                "max_context_tokens" => {
                    if let Ok(v) = value.parse() {
                        max_context_tokens = Some(v);
                    } else {
                        tracing::warn!("Invalid max_context_tokens value in config: {value}");
                    }
                }
                "permission_profile" => {
                    permission_profile = Some(value.to_string());
                }
                _ => {}
            }
        }
    }
    let provider_model = crate::provider_resolver::synthesize_default_profile(
        model.as_deref(),
        provider.as_deref(),
        base_url.as_deref(),
        None,
    )
    .unwrap_or_default();
    ShannonConfig {
        max_tokens,
        temperature,
        timeout,
        debug,
        enable_tools: None,
        max_context_tokens,
        presets: None,
        permission_profile,
        notifications: None,
        provider_model,
    }
}

// Implement `ApiKeyResolver` (defined in `shannon-engine::api::types`) for
// `ShannonConfig` so `LlmClient::set_model_for_provider_with_config` can
// accept a `ShannonConfig` without `shannon-engine` depending on
// `shannon-core`.
impl shannon_engine::api::types::ApiKeyResolver for ShannonConfig {
    fn resolve_api_key_for_provider(&self, provider: &shannon_engine::api::LlmProvider) -> String {
        ShannonConfig::resolve_api_key_for_provider(self, provider)
    }
}

// Moved from `api/types.rs` during D1 Phase 2 PR-B extraction.
// `ShannonConfig` is defined here in `shannon-core`, and `LlmClientConfig`
// now lives in `shannon-engine`. Rust permits a `From` impl in either the
// type's crate or the trait's crate. Keeping it here avoids a cyclic
// dependency (`shannon-engine → shannon-core` for `ShannonConfig`).
//
// N1/C-fields: the v2 path (default profile resolves an active target) is
// preferred. When `provider_model` is empty (e.g. a direct
// `ShannonConfig::default()` for a test, or no CLI/TOML/env config at all),
// [`crate::provider_resolver::synthesize_default_profile`] is invoked with
// no CLI/TOML inputs — its Ollama auto-default kicks in when no credential
// and no base_url are configured, preserving the pre-N1 "no key → Ollama
// localhost" behaviour. The original 80-line env-pile / string→provider /
// Ollama-branch body was relocated into `synthesize_default_profile`.
impl From<ShannonConfig> for shannon_engine::api::LlmClientConfig {
    fn from(cfg: ShannonConfig) -> Self {
        // v2 path: synthesize (or use existing) default profile, then build.
        let pm = if crate::provider_resolver::resolve_active_target(&cfg.provider_model).is_some() {
            cfg.provider_model.clone()
        } else {
            crate::provider_resolver::synthesize_default_profile(None, None, None, None)
                .unwrap_or_default()
        };
        // synthesize_default_profile always returns Some (Ollama branch on
        // empty inputs), so resolve_active_target should succeed here. If
        // it doesn't, fall back to a hardcoded Ollama localhost config so
        // we never panic.
        if let Some(rt) = crate::provider_resolver::resolve_active_target(&pm) {
            return build_client_from_resolved(&cfg, rt);
        }
        use shannon_engine::api::{LlmProvider, RetryConfig};
        use std::collections::HashMap;
        tracing::warn!("No v2 provider resolved — defaulting to Ollama localhost:11434");
        Self {
            api_key: String::new(),
            base_url: "http://localhost:11434".to_string(),
            model: "llama3".to_string(),
            max_tokens: cfg.max_tokens.map(|v| v as u32).unwrap_or(4096),
            timeout_seconds: cfg.timeout.unwrap_or(300),
            api_version: String::new(),
            provider: LlmProvider::Ollama,
            extra_headers: HashMap::new(),
            retry_config: RetryConfig::default(),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 3,
            budget_tokens: None,
            reasoning_effort: None,
        }
    }
}

/// N1: build a [`LlmClientConfig`] from a resolved v2 active target (the v2
/// path). The active profile drives provider identity, base_url, model and
/// credential; the flat v1 fields are consulted only for behavioural overrides
/// (`max_tokens`, `timeout`). Credentials are resolved strictly per A1 — only
/// the profile's own [`CredentialRef`] is consulted.
fn build_client_from_resolved(
    cfg: &ShannonConfig,
    rt: crate::provider_resolver::ResolvedTarget<'_>,
) -> shannon_engine::api::LlmClientConfig {
    use shannon_engine::api::{LlmClientConfig, LlmProvider, RetryConfig};

    let provider = rt.provider;
    let base_url = rt.profile.base_url.clone();
    let model = rt.model_id.to_string();
    let api_key = crate::provider_resolver::resolve_credential(&rt.profile.credential);

    // Decision: explicit config override > profile default > engine fallback.
    let max_tokens = cfg
        .max_tokens
        .map(|v| v as u32)
        .or(rt.profile.default_max_tokens)
        .unwrap_or(4096);
    let timeout_seconds = cfg.timeout.unwrap_or(if provider == LlmProvider::Ollama {
        300
    } else {
        120
    });
    let api_version = match provider {
        LlmProvider::Anthropic => {
            std::env::var("ANTHROPIC_API_VERSION").unwrap_or_else(|_| "2023-06-01".to_string())
        }
        _ => String::new(),
    };

    LlmClientConfig {
        api_key,
        base_url,
        model,
        max_tokens,
        timeout_seconds,
        api_version,
        provider,
        extra_headers: rt.profile.extra_headers.clone(),
        retry_config: RetryConfig::default(),
        fallback_provider: None,
        fallback_base_url: None,
        max_stream_reconnects: 3,
        budget_tokens: None,
        reasoning_effort: None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use shannon_engine::api::{LlmClientConfig, LlmProvider};
    use shannon_types::provider_config::{
        ActiveTarget, CredentialRef, CredentialScope, ModelProfile, ProviderKind,
        ProviderModelConfig, ProviderProfile, Scope,
    };
    use std::collections::HashMap;

    /// Build a v2 `ProviderModelConfig` with a single `"default"` profile whose
    /// active target is the given provider profile + model.
    fn v2_default_profile(provider: ProviderProfile, model: &str) -> ProviderModelConfig {
        let mut profiles = HashMap::new();
        profiles.insert(
            "default".to_string(),
            ModelProfile {
                name: "default".to_string(),
                active_target: ActiveTarget {
                    provider_id: provider.id.clone(),
                    model_id: model.to_string(),
                    scope: Scope::Global,
                },
                providers: vec![provider],
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

    fn anthropic_profile(cred_var: &str) -> ProviderProfile {
        ProviderProfile {
            id: "anthropic".to_string(),
            kind: ProviderKind::Anthropic,
            display_name: "Anthropic".to_string(),
            base_url: "https://api.anthropic.com".to_string(),
            models_url: None,
            credential: CredentialRef::Env {
                var: cred_var.to_string(),
            },
            extra_headers: HashMap::new(),
            default_max_tokens: None,
            fallback_models: Vec::new(),
            quirks: Default::default(),
        }
    }

    #[test]
    fn test_empty_config() {
        let config = ShannonConfig::empty();
        assert!(config.provider_model.profiles.is_empty());
        assert!(!config.debug);
        assert!(config.max_tokens.is_none());
        assert!(config.temperature.is_none());
    }

    #[test]
    fn test_merge_other_overrides_self() {
        let base = ShannonConfig {
            max_tokens: Some(4096),
            temperature: None,
            timeout: None,
            debug: false,
            enable_tools: None,
            max_context_tokens: None,
            provider_model: v2_default_profile(anthropic_profile("BASE_KEY"), "base-model"),
            ..Default::default()
        };
        let override_config = ShannonConfig {
            max_tokens: None,
            temperature: Some(0.5),
            timeout: None,
            debug: true,
            enable_tools: None,
            max_context_tokens: None,
            provider_model: v2_default_profile(anthropic_profile("OVERRIDE_KEY"), "over-model"),
            ..Default::default()
        };

        let merged = base.merge(&override_config);
        // v2 provider_model: other wins (first-non-empty).
        assert_eq!(
            merged.provider_model.profiles["default"]
                .active_target
                .model_id,
            "over-model"
        );
        assert_eq!(merged.max_tokens, Some(4096)); // kept from base
        assert_eq!(merged.temperature, Some(0.5)); // from override
        assert!(merged.debug); // from override
    }

    #[test]
    fn test_merge_other_overrides_self_empty_other_keeps_self() {
        // N1: when `other.provider_model` is empty, `self.provider_model` is
        // preserved (CLI/empty TOML doesn't clobber the user's profile).
        let base = ShannonConfig {
            provider_model: v2_default_profile(anthropic_profile("K"), "a-model"),
            ..Default::default()
        };
        let override_config = ShannonConfig::empty();
        let merged = base.merge(&override_config);
        assert_eq!(
            merged.provider_model.profiles["default"]
                .active_target
                .model_id,
            "a-model"
        );
    }

    #[test]
    fn test_builder_priority_chain() {
        // 4-layer merge: global < local < env < cli (when each carries
        // provider_model). The scalar fields (max_tokens/temperature/debug)
        // merge independently: highest-priority Some wins.
        let mut builder = ConfigBuilder::new();

        // Global TOML: profile + max_tokens
        builder.global_toml = ShannonConfig {
            max_tokens: Some(2048),
            provider_model: v2_default_profile(anthropic_profile("G"), "global-model"),
            ..Default::default()
        };

        // Local TOML overrides global: profile + temperature
        builder.local_toml = ShannonConfig {
            temperature: Some(0.7),
            provider_model: v2_default_profile(anthropic_profile("L"), "local-model"),
            ..Default::default()
        };

        // Env layer: empty provider_model (no SHANNON_* in tests by default) +
        // max_tokens override.
        builder.env_vars = ShannonConfig {
            max_tokens: Some(8192),
            provider_model: Default::default(),
            ..Default::default()
        };

        // CLI overrides highest priority: provider_model + debug.
        builder.cli_overrides = ShannonConfig {
            debug: true,
            provider_model: v2_default_profile(anthropic_profile("C"), "cli-model"),
            ..Default::default()
        };

        let config = builder.build();

        // CLI's provider_model is the only non-empty one in the merge chain
        // (local/global have profiles but env is empty → CLI wins on
        // first-non-empty-wins). Re-derive: cli_overrides has profiles;
        // it merges on top of global/local; its non-empty wins. env is empty
        // so does NOT clobber. The merged result's profile has cli-model.
        assert_eq!(
            config.provider_model.profiles["default"]
                .active_target
                .model_id,
            "cli-model"
        );
        // env's max_tokens: max(2048, 8192) — env wins over global.
        assert_eq!(config.max_tokens, Some(8192));
        // local's temperature survives (none of env/cli override it).
        assert_eq!(config.temperature, Some(0.7));
        // CLI's debug is true.
        assert!(config.debug);
    }

    #[test]
    fn test_builder_empty_sources() {
        let config = ConfigBuilder::new().build();
        assert!(config.provider_model.profiles.is_empty());
        assert!(config.max_tokens.is_none());
    }

    #[test]
    fn test_merge_both_none_stays_none() {
        let a = ShannonConfig::empty();
        let b = ShannonConfig::empty();
        let merged = a.merge(&b);
        assert!(merged.max_tokens.is_none());
        assert!(merged.provider_model.profiles.is_empty());
    }

    #[test]
    fn test_merge_debug_or_logic() {
        let a = ShannonConfig {
            debug: true,
            ..Default::default()
        };
        let b = ShannonConfig {
            debug: false,
            ..Default::default()
        };
        // a.debug || b.debug when b overrides
        let merged = a.merge(&b);
        // b.debug is false, but a.debug was true — since merge uses `other.debug || self.debug`
        assert!(merged.debug);
    }

    #[test]
    fn test_v2_profile_in_config_drives_client() {
        // N1/C-fields: the v1 flat fields are gone. The `default` profile in
        // `provider_model` is now the *only* way to express provider/base_url/
        // model/api_key. A ShannonConfig with a populated `provider_model`
        // produces an `LlmClientConfig` from that profile.
        let cfg = ShannonConfig {
            provider_model: v2_default_profile(
                anthropic_profile("SHANNON_ANTHROPIC_API_KEY"),
                "claude-sonnet-4-20250514",
            ),
            ..Default::default()
        };
        let cc: LlmClientConfig = cfg.into();
        assert_eq!(cc.provider, LlmProvider::Anthropic);
        assert_eq!(cc.base_url, "https://api.anthropic.com");
        assert_eq!(cc.model, "claude-sonnet-4-20250514");
    }

    #[test]
    fn test_v2_credential_env_resolved() {
        // SAFETY: unique key read only by this test thread; removed at the end.
        unsafe { std::env::set_var("N1_V2_TEST_KEY", "v2-secret") };
        let provider = ProviderProfile {
            id: "zhipu".to_string(),
            kind: ProviderKind::OpenAiCompatible,
            display_name: "Zhipu".to_string(),
            base_url: "https://open.bigmodel.cn".to_string(),
            models_url: None,
            credential: CredentialRef::Env {
                var: "N1_V2_TEST_KEY".to_string(),
            },
            extra_headers: HashMap::new(),
            default_max_tokens: None,
            fallback_models: Vec::new(),
            quirks: Default::default(),
        };
        let cfg = ShannonConfig {
            provider_model: v2_default_profile(provider, "glm-4"),
            ..Default::default()
        };
        let cc: LlmClientConfig = cfg.into();
        // base_url detection → Zhipu provider; credential from the env var.
        assert_eq!(cc.provider, LlmProvider::Zhipu);
        assert_eq!(cc.api_key, "v2-secret");
        // SAFETY: see above.
        unsafe { std::env::remove_var("N1_V2_TEST_KEY") };
    }

    #[test]
    fn test_v2_max_tokens_priority() {
        fn build(max_tokens: Option<usize>, profile_max: Option<u32>) -> LlmClientConfig {
            let mut provider = anthropic_profile("N1_V2_UNUSED");
            provider.default_max_tokens = profile_max;
            let cfg = ShannonConfig {
                max_tokens,
                provider_model: v2_default_profile(provider, "claude"),
                ..Default::default()
            };
            cfg.into()
        }
        // profile default wins when there is no config override
        assert_eq!(build(None, Some(8000)).max_tokens, 8000);
        // config override beats profile default
        assert_eq!(build(Some(1000), Some(8000)).max_tokens, 1000);
        // engine fallback when neither is set
        assert_eq!(build(None, None).max_tokens, 4096);
    }

    #[test]
    fn test_v2_empty_falls_back_to_legacy_path() {
        // No v2 default profile → legacy v1 path runs. max_tokens is
        // deterministic regardless of env (4096 fallback) and is set by the
        // v1 branch, proving the v2 branch was skipped.
        let cfg = ShannonConfig {
            provider_model: ProviderModelConfig::default(),
            ..Default::default()
        };
        let cc: LlmClientConfig = cfg.into();
        assert_eq!(cc.max_tokens, 4096);
    }
}
