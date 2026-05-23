//! Unified configuration system with priority-based merging.
//!
//! Configuration sources (highest to lowest priority):
//! 1. CLI arguments (explicit overrides)
//! 2. Environment variables (`SHANNON_*`)
//! 3. Project-local config (`.shannon.toml`)
//! 4. Global config (`~/.shannon/config.toml`)
//! 5. Default values

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::api::types::LlmProvider;

/// Per-provider configuration entry for `[providers.<name>]` sections.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderEntry {
    pub api_key: Option<String>,
    pub api_key_env: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
}

/// Unified Shannon configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ShannonConfig {
    pub model: Option<String>,
    pub provider: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
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
    /// Per-provider configuration: `[providers.deepseek]`, `[providers.zhipu]`, etc.
    #[serde(default)]
    pub providers: Option<HashMap<String, ProviderEntry>>,
}

impl ShannonConfig {
    /// Create an empty config with all fields set to None.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Merge another config on top of this one.
    /// Values from `other` take precedence if they are `Some`.
    pub fn merge(&self, other: &ShannonConfig) -> ShannonConfig {
        // Merge providers: other's entries overlay on top of self's.
        let providers = match (&self.providers, &other.providers) {
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

        ShannonConfig {
            model: other.model.clone().or_else(|| self.model.clone()),
            provider: other.provider.clone().or_else(|| self.provider.clone()),
            api_key: other.api_key.clone().or_else(|| self.api_key.clone()),
            base_url: other.base_url.clone().or_else(|| self.base_url.clone()),
            max_tokens: other.max_tokens.or(self.max_tokens),
            temperature: other.temperature.or(self.temperature),
            timeout: other.timeout.or(self.timeout),
            debug: other.debug || self.debug,
            enable_tools: other.enable_tools.or(self.enable_tools),
            max_context_tokens: other.max_context_tokens.or(self.max_context_tokens),
            providers,
        }
    }

    /// Resolve the API key for a given provider from config + env.
    pub fn resolve_api_key_for_provider(&self, provider: &LlmProvider) -> String {
        let display = provider.to_string();

        // 1. Top-level api_key in config (if provider matches)
        if let Some(ref key) = self.api_key {
            // If config has a top-level api_key and no provider filter, use it
            if self.provider.is_none() || self.provider.as_deref() == Some(&display) {
                return key.clone();
            }
        }

        // 2. Check [providers.<name>] section in config
        if let Some(ref providers) = self.providers {
            if let Some(entry) = providers.get(&display) {
                if let Some(ref key) = entry.api_key {
                    return key.clone();
                }
                if let Some(ref env_name) = entry.api_key_env {
                    if let Ok(key) = std::env::var(env_name) {
                        return key;
                    }
                }
            }
        }

        // 3. Provider's own env resolution chain
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
    pub fn load_env_vars(&mut self) -> &mut Self {
        self.env_vars = ShannonConfig {
            model: std::env::var("SHANNON_MODEL").ok(),
            provider: std::env::var("SHANNON_PROVIDER").ok(),
            api_key: std::env::var("SHANNON_API_KEY").ok(),
            base_url: std::env::var("SHANNON_BASE_URL").ok(),
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
            ..Default::default()
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
    // (Full TOML support requires the `toml` crate, available in shannon-cli)
    let mut config = ShannonConfig::empty();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim().trim_matches('"');
            match key {
                "model" => config.model = Some(value.to_string()),
                "provider" => config.provider = Some(value.to_string()),
                "api_key" => config.api_key = Some(value.to_string()),
                "base_url" => config.base_url = Some(value.to_string()),
                "max_tokens" => {
                    if let Ok(v) = value.parse() {
                        config.max_tokens = Some(v);
                    } else {
                        tracing::warn!("Invalid max_tokens value in config: {value}");
                    }
                }
                "temperature" => {
                    if let Ok(v) = value.parse() {
                        config.temperature = Some(v);
                    } else {
                        tracing::warn!("Invalid temperature value in config: {value}");
                    }
                }
                "timeout" => {
                    if let Ok(v) = value.parse() {
                        config.timeout = Some(v);
                    } else {
                        tracing::warn!("Invalid timeout value in config: {value}");
                    }
                }
                "debug" => config.debug = value.parse().unwrap_or(false),
                "max_context_tokens" => {
                    if let Ok(v) = value.parse() {
                        config.max_context_tokens = Some(v);
                    } else {
                        tracing::warn!("Invalid max_context_tokens value in config: {value}");
                    }
                }
                _ => {}
            }
        }
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_config() {
        let config = ShannonConfig::empty();
        assert!(config.model.is_none());
        assert!(config.provider.is_none());
        assert!(config.api_key.is_none());
        assert!(!config.debug);
    }

    #[test]
    fn test_merge_other_overrides_self() {
        let base = ShannonConfig {
            model: Some("base-model".to_string()),
            provider: Some("anthropic".to_string()),
            api_key: Some("base-key".to_string()),
            base_url: None,
            max_tokens: Some(4096),
            temperature: None,
            timeout: None,
            debug: false,
            enable_tools: None,
            max_context_tokens: None,
            ..Default::default()
        };
        let override_config = ShannonConfig {
            model: Some("override-model".to_string()),
            provider: None, // Don't override
            api_key: None,
            base_url: Some("http://custom".to_string()),
            max_tokens: None,
            temperature: Some(0.5),
            timeout: None,
            debug: true,
            enable_tools: None,
            max_context_tokens: None,
            ..Default::default()
        };

        let merged = base.merge(&override_config);
        assert_eq!(merged.model, Some("override-model".to_string()));
        assert_eq!(merged.provider, Some("anthropic".to_string())); // kept from base
        assert_eq!(merged.api_key, Some("base-key".to_string())); // kept from base
        assert_eq!(merged.base_url, Some("http://custom".to_string())); // from override
        assert_eq!(merged.max_tokens, Some(4096)); // kept from base
        assert_eq!(merged.temperature, Some(0.5)); // from override
        assert!(merged.debug); // from override
    }

    #[test]
    fn test_builder_priority_chain() {
        let mut builder = ConfigBuilder::new();

        // Simulate global TOML
        builder.global_toml = ShannonConfig {
            model: Some("global-model".to_string()),
            provider: Some("anthropic".to_string()),
            max_tokens: Some(2048),
            ..Default::default()
        };

        // Simulate local TOML (overrides global)
        builder.local_toml = ShannonConfig {
            model: Some("local-model".to_string()),
            temperature: Some(0.7),
            ..Default::default()
        };

        // Simulate env vars (overrides TOML)
        builder.env_vars = ShannonConfig {
            api_key: Some("env-key".to_string()),
            max_tokens: Some(8192),
            ..Default::default()
        };

        // Simulate CLI overrides (highest priority)
        builder.cli_overrides = ShannonConfig {
            model: Some("cli-model".to_string()),
            debug: true,
            ..Default::default()
        };

        let config = builder.build();

        // CLI wins for model
        assert_eq!(config.model, Some("cli-model".to_string()));
        // Local TOML provides provider (not overridden by env or CLI)
        assert_eq!(config.provider, Some("anthropic".to_string()));
        // Env provides api_key
        assert_eq!(config.api_key, Some("env-key".to_string()));
        // Env overrides global max_tokens
        assert_eq!(config.max_tokens, Some(8192));
        // Local TOML provides temperature
        assert_eq!(config.temperature, Some(0.7));
        // CLI sets debug
        assert!(config.debug);
    }

    #[test]
    fn test_builder_empty_sources() {
        let config = ConfigBuilder::new().build();
        assert!(config.model.is_none());
        assert!(config.provider.is_none());
    }

    #[test]
    fn test_merge_both_none_stays_none() {
        let a = ShannonConfig::empty();
        let b = ShannonConfig::empty();
        let merged = a.merge(&b);
        assert!(merged.model.is_none());
        assert!(merged.max_tokens.is_none());
    }

    #[test]
    fn test_merge_debug_or_logic() {
        let a = ShannonConfig { debug: true, ..Default::default() };
        let b = ShannonConfig { debug: false, ..Default::default() };
        // a.debug || b.debug when b overrides
        let merged = a.merge(&b);
        // b.debug is false, but a.debug was true — since merge uses `other.debug || self.debug`
        assert!(merged.debug);
    }
}
