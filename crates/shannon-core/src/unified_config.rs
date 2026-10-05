//! Unified configuration system with priority-based merging.
//!
//! Configuration layers (highest to lowest priority; the six-source merge
//! [`ConfigBuilder::build`] performs and `config_dump` prints per-layer):
//! 1. CLI arguments (the per-invocation overlay)
//! 2. Connected profile (`~/.shannon/providers.toml`, written by `/connect`)
//! 3. Environment variables (`SHANNON_*`)
//! 4. Project-local config (`.shannon.toml`)
//! 5. Global config (`~/.shannon/config.toml`)
//! 6. Built-in default values
//!
//! ## v2-native (N1 / C-fields)
//! As of N1, [`ShannonConfig`] carries only the multi-provider/model
//! [`ProviderModelConfig`](shannon_types::provider_config::ProviderModelConfig)
//! in its `provider_model` field. The pre-N1 flat fields
//! (`model`/`provider`/`api_key`/`base_url`/`[providers.*]`) have been
//! removed under the no-compat policy (shannon-code/desktop were unreleased —
//! see [[no-public-release-no-compat]]). Configuration previously set on the
//! flat fields is now expressed as a default `ProviderProfile` inside
//! `provider_model`, synthesized from CLI/TOML/env inputs by
//! [`crate::provider_resolver::synthesize_default_profile`]. Credentials are
//! A1-strict: only [`CredentialRef::Env`](shannon_types::provider_config::CredentialRef::Env)
//! references, never plaintext in the config (plaintext values live in the
//! process environment, resolved by `resolve_credential`).

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
    /// Permission profile name: "strict", "balanced", "permissive", or "custom:\<name\>".
    #[serde(default)]
    pub permission_profile: Option<String>,
    /// `[notifications]` section for system-level notification behavior.
    #[serde(default)]
    pub notifications: Option<NotificationsConfig>,
    /// `[hooks]` section for inbound webhook endpoints (P2-7:
    /// `[hooks.github] secret` guards `POST /hooks/github` on shannon-server).
    #[serde(default)]
    pub hooks: Option<HooksConfig>,
    /// v2 multi-provider/model config. The `"default"` profile's active
    /// target, when present, drives the engine `LlmClientConfig`. CLI / TOML
    /// / env inputs feed this through
    /// [`crate::provider_resolver::synthesize_default_profile`].
    #[serde(default)]
    pub provider_model: shannon_types::provider_config::ProviderModelConfig,
}

/// `[secret_guard]` config section (blueprint artifact c, Phase 2).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct SecretGuardSection {
    /// `"audit"` (default since v0.11.0 when unset) | `"redact"` | `"off"`
    /// to disable entirely.
    #[serde(default)]
    pub mode: Option<String>,
    /// Plugin failure behavior — reserved for Phase 3: `"open"` (default) |
    /// `"closed"`.
    #[serde(default)]
    pub fail_mode: Option<String>,
}

impl SecretGuardSection {
    /// Load this section from the same config files [`ConfigBuilder`] reads
    /// (`~/.shannon/config.toml`, then a `.shannon.toml` override). Kept
    /// independent of `ShannonConfig` so the secret-guard's process-wide
    /// one-shot init does not depend on the merged-config call path.
    pub fn load() -> Self {
        Self::load_from(&[
            dirs::home_dir()
                .map(|home| home.join(".shannon").join("config.toml"))
                .unwrap_or_default(),
            std::path::PathBuf::from(".shannon.toml"),
        ])
    }

    /// Layered parse: later files override earlier ones; broken files are
    /// skipped (same degrade-not-fail posture as the rest of the loader).
    fn load_from(paths: &[std::path::PathBuf]) -> Self {
        let mut section = Self::default();
        for path in paths {
            let Ok(text) = std::fs::read_to_string(path) else {
                continue;
            };
            let Ok(value) = toml::from_str::<toml::Value>(&text) else {
                continue;
            };
            if let Ok(parsed) = value
                .get("secret_guard")
                .cloned()
                .unwrap_or(toml::Value::Boolean(false))
                .try_into::<Self>()
            {
                section = parsed;
            }
        }
        section
    }
}

/// `[hooks]` config section: inbound webhook endpoints (P2-7).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HooksConfig {
    /// GitHub webhook endpoint (`POST /hooks/github` on shannon-server).
    #[serde(default)]
    pub github: Option<GitHubHooksConfig>,
}

/// `[hooks.github]` config section.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GitHubHooksConfig {
    /// HMAC-SHA256 webhook secret. GitHub sends the same secret in the
    /// webhook settings UI; deliveries carry `X-Hub-Signature-256:
    /// sha256=<hex>` over the raw body. When unset, the endpoint answers
    /// **503** (disabled — safe default).
    #[serde(default)]
    pub secret: Option<String>,
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
            hooks: other.hooks.clone().or_else(|| self.hooks.clone()),
            provider_model,
        }
    }

    /// P1-3: the configured `permission_profile` string.
    ///
    /// Precedence: `SHANNON_PERMISSION_PROFILE` env override, then the
    /// `permission_profile` key from `~/.shannon/config.toml` / `.shannon.toml`.
    pub fn configured_permission_profile() -> Option<String> {
        if let Ok(v) = std::env::var("SHANNON_PERMISSION_PROFILE") {
            if !v.trim().is_empty() {
                return Some(v.trim().to_string());
            }
        }
        ConfigBuilder::new()
            .load_global_toml()
            .load_local_toml()
            .build()
            .permission_profile
            .filter(|s| !s.trim().is_empty())
    }

    /// Parse the `permission_profile` field into a `PermissionProfile`.
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
    /// `CredentialRef`(shannon_types::provider_config::CredentialRef) is
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

/// Provenance-annotated snapshot of one configuration layer.
///
/// Produced by [`ConfigBuilder::layer_snapshots`] and consumed by
/// [`crate::config_dump`] (`shannon --dump-config`, §4.10 W3-2).
pub struct LayerSnapshot {
    /// Stable label, one of: `builtin`, `user-global`, `project`,
    /// `env-vars`, `connected`, `cli-overlay` (ordered lowest → highest
    /// precedence; this is the engine's true merge order).
    pub source: &'static str,
    /// Backing file when the layer is file-backed.
    pub path: Option<std::path::PathBuf>,
    /// Did the layer actually contribute bytes? (file present / env set /
    /// profile connected)
    pub present: bool,
    /// The parsed layer content. Empty layer ⇒ all-unset defaults.
    pub config: ShannonConfig,
}

impl LayerSnapshot {
    /// The never-overridden floor every other layer overlays: the engine's
    /// built-in baseline (all fields unset — downstream runtime defaults
    /// apply after the merge chain).
    pub fn builtin() -> Self {
        Self {
            source: "builtin",
            path: None,
            present: true,
            config: ShannonConfig::empty(),
        }
    }
}

/// Builder for constructing a merged configuration from multiple sources.
pub struct ConfigBuilder {
    global_toml: ShannonConfig,
    local_toml: ShannonConfig,
    env_vars: ShannonConfig,
    // §4.10 provenance bookkeeping for --dump-config.
    global_present: bool,
    local_present: bool,
    connected_present: bool,
    /// The connected provider profile (`~/.shannon/providers.toml`, written by
    /// `/connect`). Merged between env vars and CLI overrides so a connected
    /// provider wins over ambient `SHANNON_*` env vars (the `/connect`
    /// "works without env vars" contract) while `--provider`/`--model` still
    /// override a single invocation. See ADR-0005 Phase 4.
    connected: ShannonConfig,
    cli_overrides: ShannonConfig,
}

impl ConfigBuilder {
    /// Create a new config builder.
    pub fn new() -> Self {
        Self {
            global_toml: ShannonConfig::empty(),
            local_toml: ShannonConfig::empty(),
            env_vars: ShannonConfig::empty(),
            global_present: false,
            local_present: false,
            connected_present: false,
            connected: ShannonConfig::empty(),
            cli_overrides: ShannonConfig::empty(),
        }
    }

    /// Load global TOML config from `~/.shannon/config.toml`.
    pub fn load_global_toml(&mut self) -> &mut Self {
        if let Some(home) = dirs::home_dir() {
            let path = home.join(".shannon").join("config.toml");
            self.global_present = path.exists();
            self.global_toml = load_config_file(&path);
            crate::substitute::substitute_config(&mut self.global_toml);
        }
        self
    }

    /// Load project-local TOML config from `.shannon.toml`.
    pub fn load_local_toml(&mut self) -> &mut Self {
        let path = std::path::Path::new(".shannon.toml");
        self.local_present = path.exists();
        let local = load_config_file(path);
        self.local_toml = local;
        crate::substitute::substitute_config(&mut self.local_toml);
        self
    }

    /// Start watching `.shannon.toml` and emit `HookEvent::ConfigChange`
    /// whenever the file changes on disk (P1-2c).
    ///
    /// Returns `None` when the parent directory is missing or the platform's
    /// filesystem watcher is unavailable (e.g. inside a sandbox); callers
    /// must treat both cases as "no watcher" and continue normally.
    ///
    /// The reload itself is *not* performed here — `on_change` is the
    /// hook-emit point only. Callers that want live reload should
    /// re-invoke [`Self::load_local_toml`] from inside the callback.
    pub fn watch_local_toml<F>(&self, on_change: F) -> Option<crate::config_watcher::ConfigWatcher>
    where
        F: FnMut(crate::config_watcher::ConfigChange) + Send + 'static,
    {
        crate::config_watcher::ConfigWatcher::start(
            std::path::PathBuf::from(".shannon.toml"),
            on_change,
        )
    }

    /// Load the connected provider profile from `~/.shannon/providers.toml`
    /// (ADR-0005 Phase 4). The file is written by `/connect` and carries a
    /// `CredentialRef::Store` credential, so a connected provider activates on
    /// the next launch with no environment variable. A missing or unparseable
    /// file leaves the layer empty (synthesis takes over) — launch never fails.
    pub fn load_connected_profile(&mut self) -> &mut Self {
        if let Some(pm) = crate::provider_config_store::load(None) {
            self.connected_present = true;
            self.connected = ShannonConfig {
                max_tokens: None,
                temperature: None,
                timeout: None,
                debug: false,
                enable_tools: None,
                max_context_tokens: None,
                presets: None,
                permission_profile: None,
                notifications: None,
                hooks: None,
                provider_model: pm,
            };
            crate::substitute::substitute_config(&mut self.connected);
        }
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
            hooks: None,
            provider_model,
        };
        self
    }

    /// Set CLI argument overrides (highest priority).
    pub fn set_cli_overrides(&mut self, config: ShannonConfig) -> &mut Self {
        self.cli_overrides = config;
        self
    }

    /// Ordered (lowest → highest precedence) provenance snapshots of every
    /// loaded layer — the data behind `shannon --dump-config` (§4.10).
    ///
    /// Call this *after* the loaders you care about; unloaded layers show as
    /// absent empties so the dump can still render the full ladder.
    pub fn layer_snapshots(&self) -> Vec<LayerSnapshot> {
        let global_path = dirs::home_dir().map(|home| home.join(".shannon").join("config.toml"));
        vec![
            LayerSnapshot::builtin(),
            LayerSnapshot {
                source: "user-global",
                path: global_path,
                present: self.global_present,
                config: self.global_toml.clone(),
            },
            LayerSnapshot {
                source: "project",
                path: Some(std::path::PathBuf::from(".shannon.toml")),
                present: self.local_present,
                config: self.local_toml.clone(),
            },
            LayerSnapshot {
                source: "env-vars",
                path: None,
                present: true,
                config: self.env_vars.clone(),
            },
            LayerSnapshot {
                source: "connected",
                path: crate::provider_config_store::default_path(),
                present: self.connected_present,
                config: self.connected.clone(),
            },
            LayerSnapshot {
                source: "cli-overlay",
                path: None,
                present: true,
                config: self.cli_overrides.clone(),
            },
        ]
    }

    /// Build the final merged configuration.
    ///
    /// Priority (highest to lowest):
    /// CLI overrides > connected profile (`providers.toml`) > env vars >
    /// local TOML > global TOML
    ///
    /// The connected layer (ADR-0005 Phase 4) lets `/connect` win over ambient
    /// `SHANNON_*` env vars, while a per-invocation `--provider`/`--model`
    /// still overrides it.
    pub fn build(&self) -> ShannonConfig {
        let mut config = self
            .global_toml
            .merge(&self.local_toml)
            .merge(&self.env_vars)
            .merge(&self.connected)
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
        hooks: None,
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
            alternate_api_keys: Vec::new(),
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
            enable_anthropic_toolsets: shannon_engine::api::toolsets::anthropic_toolsets_from_env(),
            thinking_type: shannon_engine::api::types::thinking_type_from_env(),
        }
    }
}

/// N1: build a `LlmClientConfig` from a resolved v2 active target (the v2
/// path). The active profile drives provider identity, base_url, model and
/// credential; the flat v1 fields are consulted only for behavioural overrides
/// (`max_tokens`, `timeout`). Credentials are resolved strictly per A1 — only
/// the profile's own `CredentialRef` is consulted.
///
/// `pub` so the desktop shell's `AppState::build_client_config` (and any other
/// downstream crate that has a `ShannonConfig` + resolved active target) can
/// share this exact construction — T1 (ADR-0005 P1.1). Note: callers must
/// have already obtained a [`crate::provider_resolver::ResolvedTarget`] via
/// [`crate::provider_resolver::resolve_active_target`]; this function does not
/// synthesize a fallback (synthesis lives in the `From<ShannonConfig>` impl).
pub fn build_client_from_resolved(
    cfg: &ShannonConfig,
    rt: crate::provider_resolver::ResolvedTarget<'_>,
) -> shannon_engine::api::LlmClientConfig {
    use shannon_engine::api::{LlmClientConfig, LlmProvider, RetryConfig};

    // R2-4: whichever profile actually drove this client construction owns
    // the per-model metadata registry (pricing / context / tier overrides).
    // Replacement, not accumulation — a provider switch re-registers here.
    // S2-5: the declarations bind to the active provider slot so the tier
    // scan can attribute them correctly.
    crate::declared_models::replace_for_provider(&rt.provider, &rt.profile.models);

    let provider = rt.provider;
    let base_url = rt.profile.base_url.clone();
    let model = rt.model_id.to_string();
    // R4-3: resolve ALL keys the profile's credential stands for, in
    // rotation order. `api_key` (slot 0) is the active key — the exact value
    // the single-key `resolve_credential` used to return, so single-key
    // behavior is unchanged; the remainder feed the engine's key-rotation
    // walk. Failover targets below deliberately keep resolving the ACTIVE
    // key only: rotation is a primary-provider concern.
    let resolved_keys = crate::provider_resolver::resolve_credential_keys(&rt.profile.credential);
    let api_key = resolved_keys.first().cloned().unwrap_or_default();
    let alternate_api_keys = resolved_keys.into_iter().skip(1).collect::<Vec<String>>();

    // Decision: explicit config override > profile default > engine fallback.
    let mut max_tokens = cfg
        .max_tokens
        .map(|v| v as u32)
        .or(rt.profile.default_max_tokens)
        .unwrap_or(4096);
    // S2-3 (裁定⑩): a declared `max_output` on the ACTIVE model is a real
    // endpoint constraint — clamp the request ceiling down to it. Clamp only:
    // a declaration never *raises* the ceiling (a model without a configured
    // default keeps the 4096 fallback even when it could take more).
    if let Some(spec) = rt
        .profile
        .models
        .iter()
        .find(|m| m.id == rt.model_id)
        .and_then(|m| m.max_output)
    {
        max_tokens = max_tokens.min(spec);
    }
    let timeout_seconds = cfg.timeout.unwrap_or(if provider == LlmProvider::Ollama {
        300
    } else {
        120
    });
    let api_version = match provider {
        LlmProvider::Anthropic => {
            std::env::var("ANTHROPIC_API_VERSION").unwrap_or_else(|_| "2023-06-01".to_string())
        }
        // S2-6: Azure's deployments route requires an explicit versioned
        // query on every request (`endpoint_url` appends it); leave empty and
        // the client falls back to `AZURE_DEFAULT_API_VERSION` — seeding the
        // env override here keeps the value visible in config dumps/debug.
        LlmProvider::Azure => std::env::var("AZURE_OPENAI_API_VERSION")
            .unwrap_or_else(|_| shannon_engine::api::types::AZURE_DEFAULT_API_VERSION.to_string()),
        _ => String::new(),
    };

    // R3-1: resolve the profile's explicit `fallback_models` into the
    // client's failover chain. Empty list → `RetryConfig::default()` exactly
    // as before (failover stays opt-in; Shannon does not route).
    let fallbacks = resolve_failover_chain(&cfg.provider_model, &rt.profile.id, rt.model_id);
    let retry_config = if fallbacks.is_empty() {
        RetryConfig::default()
    } else {
        RetryConfig {
            fallbacks,
            ..RetryConfig::default()
        }
    };

    LlmClientConfig {
        api_key,
        alternate_api_keys,
        base_url,
        model,
        max_tokens,
        timeout_seconds,
        api_version,
        provider,
        extra_headers: rt.profile.extra_headers.clone(),
        retry_config,
        fallback_provider: None,
        fallback_base_url: None,
        max_stream_reconnects: 3,
        budget_tokens: None,
        reasoning_effort: None,
        enable_anthropic_toolsets: shannon_engine::api::toolsets::anthropic_toolsets_from_env(),
        thinking_type: shannon_engine::api::types::thinking_type_from_env(),
    }
}

/// R3-1: resolve the active provider profile's explicit `fallback_models`
/// list into concrete failover targets for the client config.
///
/// Resolution rules (per item, in list order):
/// - **Bare id** (`"glm-5-flash"`) → same provider: the active provider
///   profile's provider/base_url/credential with just the model swapped.
/// - **Qualified id** (`"deepseek/deepseek-v4-flash"`) → switch provider:
///   the named provider's profile in the SAME named `ModelProfile`
///   (resolved via [`shannon_types::provider_config::ProviderModelConfig::active_profile_key`],
///   R3-2-aware) supplies kind/base_url/credential. "Connected" means
///   exactly that — the provider must be in the roster; an unknown or
///   unconnected provider slug is **skipped with a warning**, never an
///   error (config hygiene must not block the session).
/// - The active target itself, empty ids, and anything beyond
///   [`shannon_engine::api::retry::MAX_FAILOVER_TARGETS`] entries are
///   skipped (with a log line) — the chain can never exceed the engine cap.
///
/// `provider_model` is the config `rt` was resolved from; passing a
/// mismatched pair simply resolves against the named roster (pure function,
/// unit-tested in isolation).
pub fn resolve_failover_chain(
    provider_model: &shannon_types::provider_config::ProviderModelConfig,
    active_provider_id: &str,
    active_model_id: &str,
) -> Vec<shannon_engine::api::FailoverTarget> {
    use crate::provider_resolver::{
        llm_provider_from_id, llm_provider_from_slug, resolve_provider,
    };
    use shannon_engine::api::retry::MAX_FAILOVER_TARGETS;

    let Some(mp) = provider_model
        .profiles
        .get(provider_model.active_profile_key())
    else {
        return Vec::new();
    };
    let Some(active) = mp.providers.iter().find(|p| p.id == active_provider_id) else {
        return Vec::new();
    };
    if active.fallback_models.is_empty() {
        return Vec::new();
    }

    let active_provider = llm_provider_from_id(&active.id)
        .unwrap_or_else(|| resolve_provider(&active.kind, &active.base_url));

    let mut targets = Vec::new();
    for raw in &active.fallback_models {
        let id = raw.trim();
        if id.is_empty() {
            continue;
        }
        if targets.len() >= MAX_FAILOVER_TARGETS {
            tracing::debug!(
                skipped = %id,
                cap = MAX_FAILOVER_TARGETS,
                "failover chain capped — remaining fallback_models ignored"
            );
            break;
        }

        // Qualified `provider/model` switches provider; bare stays.
        let (owner, model_id) = match id.split_once('/') {
            Some((provider_part, model_part)) => {
                let slug = provider_part.trim();
                // Exact profile id first, then any profile that canonicalizes
                // to the same LlmProvider ("glm" ↔ "zhipu" vocabularies).
                let resolved = mp.providers.iter().find(|p| p.id == slug).or_else(|| {
                    let want = llm_provider_from_slug(slug)?;
                    mp.providers
                        .iter()
                        .find(|p| llm_provider_from_id(&p.id).is_some_and(|pp| pp == want))
                });
                (resolved, model_part.trim())
            }
            None => (Some(active), id),
        };

        let Some(owner_profile) = owner else {
            tracing::warn!(
                fallback = %id,
                active_provider = %active.id,
                "fallback_models entry names an unconnected provider — skipped \
                 (add the provider to the profile roster to enable this failover)"
            );
            continue;
        };
        if model_id.is_empty() {
            tracing::warn!(fallback = %id, "fallback_models entry has an empty model — skipped");
            continue;
        }

        let owner_provider = llm_provider_from_id(&owner_profile.id)
            .unwrap_or_else(|| resolve_provider(&owner_profile.kind, &owner_profile.base_url));
        if owner_provider == active_provider && model_id == active_model_id.trim() {
            tracing::debug!(fallback = %id, "fallback_models entry is the active target — skipped");
            continue;
        }

        targets.push(shannon_engine::api::FailoverTarget {
            model: model_id.to_string(),
            provider: owner_provider,
            base_url: owner_profile.base_url.clone(),
            api_key: crate::provider_resolver::resolve_credential(&owner_profile.credential),
        });
    }
    targets
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {

    #[test]
    fn secret_guard_section_loads_layered_and_degrades() {
        let dir = std::env::temp_dir().join(format!(
            "sg-section-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let global = dir.join("config.toml");
        let local = dir.join(".shannon.toml");
        std::fs::write(&global, "[secret_guard]\nmode = \"audit\"\n").unwrap();
        std::fs::write(&local, "[secret_guard]\nmode = \"redact\"\n").unwrap();
        let section = SecretGuardSection::load_from(&[global.clone(), local.clone()]);
        assert_eq!(
            section.mode.as_deref(),
            Some("redact"),
            "local overrides global"
        );

        // A type-broken local section is skipped; the global value survives.
        std::fs::write(&local, "[secret_guard]\nmode = 7\n").unwrap();
        let section = SecretGuardSection::load_from(&[global, local]);
        assert_eq!(
            section.mode.as_deref(),
            Some("audit"),
            "degrade keeps global"
        );

        std::fs::remove_dir_all(dir).ok();
    }
    use super::*;
    use shannon_engine::api::{LlmClientConfig, LlmProvider};
    use shannon_types::provider_config::{
        ActiveTarget, CredentialRef, CredentialScope, ModelProfile, ProviderKind,
        ProviderModelConfig, ProviderProfile, ProviderTiers, Scope,
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
            active_profile: String::new(),
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
            tiers: ProviderTiers::default(),
            models: Vec::new(),
        }
    }

    /// R2-4: building a client from a resolved target must (re)register that
    /// profile's per-model declarations — this is the binding that makes the
    /// declared pricing/context values authoritative at the engine's lookup
    /// boundaries for whichever provider actually drives the session.
    #[test]
    fn build_client_from_resolved_registers_declared_models() {
        use shannon_types::provider_config::ModelSpec;
        let mut profile = anthropic_profile("K");
        profile.models.push(ModelSpec {
            id: "shannon-binding-test-model".to_string(),
            display_name: None,
            context_window: Some(77_777),
            max_output: None,
            cost_per_m_input: Some(1.25),
            cost_per_m_output: Some(10.0),
            capabilities: vec![],
        });
        let cfg = ShannonConfig {
            provider_model: v2_default_profile(profile, "shannon-binding-test-model"),
            ..Default::default()
        };
        let rt = crate::provider_resolver::resolve_active_target(&cfg.provider_model)
            .expect("active target resolves");
        crate::declared_models::clear();
        let _client = build_client_from_resolved(&cfg, rt);
        // Registered: pricing + context lookups now see the declaration.
        assert_eq!(
            crate::declared_models::context_window_for("shannon-binding-test-model"),
            Some(77_777)
        );
        assert!(
            crate::declared_models::pricing_for("shannon-binding-test-model").is_some(),
            "declared pricing must be registered by client construction"
        );
        // A model absent from the declaration is not registered.
        assert_eq!(crate::declared_models::lookup("other-model"), None);
        crate::declared_models::clear();
    }

    /// S2-3 (裁定⑩): a declared `max_output` on the ACTIVE model is a real
    /// endpoint constraint — the request `max_tokens` ceiling is clamped
    /// DOWN to it, never raised.
    #[test]
    fn build_client_from_resolved_clamps_max_tokens_to_declared_max_output() {
        use shannon_types::provider_config::ModelSpec;

        let declared = |max_output: Option<u32>| {
            let mut profile = anthropic_profile("K");
            profile.models.push(ModelSpec {
                id: "clamped-model".to_string(),
                display_name: None,
                context_window: None,
                max_output,
                cost_per_m_input: None,
                cost_per_m_output: None,
                capabilities: vec![],
            });
            ShannonConfig {
                provider_model: v2_default_profile(profile, "clamped-model"),
                ..Default::default()
            }
        };

        // 1. The declared value beats the engine fallback (4096 → 1024).
        let cfg = declared(Some(1_024));
        let rt = crate::provider_resolver::resolve_active_target(&cfg.provider_model)
            .expect("active target resolves");
        let client = build_client_from_resolved(&cfg, rt);
        assert_eq!(
            client.max_tokens, 1_024,
            "fallback 4096 must clamp down to the declared cap"
        );

        // 2. The declared value beats the desktop/config override too.
        let cfg = ShannonConfig {
            max_tokens: Some(8_192),
            ..declared(Some(2_048))
        };
        let rt = crate::provider_resolver::resolve_active_target(&cfg.provider_model)
            .expect("active target resolves");
        let client = build_client_from_resolved(&cfg, rt);
        assert_eq!(client.max_tokens, 2_048, "config override must clamp down");

        // 3. Clamp only — a declaration NEVER raises the ceiling: a model
        // with a large declared cap but no configured default keeps 4096.
        let cfg = declared(Some(65_536));
        let rt = crate::provider_resolver::resolve_active_target(&cfg.provider_model)
            .expect("active target resolves");
        let client = build_client_from_resolved(&cfg, rt);
        assert_eq!(
            client.max_tokens, 4096,
            "declaration must not raise the default"
        );
    }

    /// S2-5: client construction binds the declarations to the resolved
    /// provider, so the tier scan can attribute declared models correctly
    /// (the profile-level `replace_from_specs` legacy path is
    /// provider-agnostic; this is the provider-aware upgrade).
    #[test]
    fn build_client_from_resolved_binds_declarations_to_the_provider() {
        use shannon_engine::api::LlmProvider;
        use shannon_types::provider_config::ModelSpec;

        let mut profile = anthropic_profile("K");
        profile.models.push(ModelSpec {
            id: "bound-declared-model".to_string(),
            display_name: None,
            context_window: None,
            max_output: None,
            cost_per_m_input: Some(9.0),
            cost_per_m_output: Some(9.0),
            capabilities: vec![shannon_types::provider_config::ModelCapability::Reasoning],
        });
        let cfg = ShannonConfig {
            provider_model: v2_default_profile(profile, "bound-declared-model"),
            ..Default::default()
        };
        crate::declared_models::clear();
        let rt = crate::provider_resolver::resolve_active_target(&cfg.provider_model)
            .expect("active target resolves");
        assert_eq!(rt.provider, LlmProvider::Anthropic);
        let _client = build_client_from_resolved(&cfg, rt);

        // Bound to Anthropic → the Anthropic tier scan sees it.
        let candidates = crate::declared_models::tier_candidates(&LlmProvider::Anthropic);
        assert!(
            candidates.iter().any(|c| c.id == "bound-declared-model"),
            "declared model must be a tier candidate for its own provider"
        );
        // ...and no other provider's scan does.
        assert!(
            !crate::declared_models::tier_candidates(&LlmProvider::OpenAI)
                .iter()
                .any(|c| c.id == "bound-declared-model"),
            "declared models must not leak across providers"
        );
        crate::declared_models::clear();
    }

    // ── R3-1: fallback_models resolution ─────────────────────────────────

    /// A deepseek-style profile with an inline (env-free) credential so the
    /// tests never touch process env.
    fn deepseek_profile() -> ProviderProfile {
        ProviderProfile {
            id: "deepseek".to_string(),
            kind: ProviderKind::Deepseek,
            display_name: "DeepSeek".to_string(),
            base_url: "https://api.deepseek.com".to_string(),
            models_url: None,
            credential: CredentialRef::InlineLegacy {
                masked: "sk-ds-test".to_string(),
            },
            extra_headers: HashMap::new(),
            default_max_tokens: None,
            fallback_models: Vec::new(),
            quirks: Default::default(),
            tiers: ProviderTiers::default(),
            models: Vec::new(),
        }
    }

    /// A v2 config whose `"default"` profile carries `providers` with the
    /// named provider slot active at `(provider_id, model_id)`.
    fn v2_multi_provider_config(
        providers: Vec<ProviderProfile>,
        active_provider_id: &str,
        model_id: &str,
    ) -> ProviderModelConfig {
        assert!(
            providers.iter().any(|p| p.id == active_provider_id),
            "active provider must be in the roster"
        );
        ProviderModelConfig {
            version: ProviderModelConfig::VERSION,
            active_profile: String::new(),
            profiles: HashMap::from([(
                "default".to_string(),
                ModelProfile {
                    name: "default".to_string(),
                    active_target: ActiveTarget {
                        provider_id: active_provider_id.to_string(),
                        model_id: model_id.to_string(),
                        scope: Scope::Global,
                    },
                    providers,
                    auxiliary: HashMap::new(),
                    credential_scope: CredentialScope::Shared,
                },
            )]),
            gateway: Default::default(),
        }
    }

    fn profile_with_fallbacks(mut p: ProviderProfile, fallbacks: &[&str]) -> ProviderProfile {
        p.fallback_models = fallbacks.iter().map(|s| s.to_string()).collect();
        p
    }

    #[test]
    fn resolve_failover_chain_bare_id_stays_on_active_provider() {
        let active = profile_with_fallbacks(anthropic_profile("K"), &["claude-haiku-4-5"]);
        let pm = v2_multi_provider_config(vec![active], "anthropic", "claude-sonnet-4-6");
        let chain = resolve_failover_chain(&pm, "anthropic", "claude-sonnet-4-6");
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].model, "claude-haiku-4-5");
        assert_eq!(chain[0].provider, LlmProvider::Anthropic);
        assert_eq!(chain[0].base_url, "https://api.anthropic.com");
        assert_eq!(
            chain[0].api_key, "",
            "env credential var K is unset in tests — resolved empty is fine, the walk inherits"
        );
    }

    #[test]
    fn resolve_failover_chain_qualified_id_switches_provider() {
        let active = profile_with_fallbacks(
            anthropic_profile("K"),
            &["deepseek/deepseek-v4-flash", "glm-5-flash"],
        );
        let ds = deepseek_profile();
        let pm = v2_multi_provider_config(vec![active, ds], "anthropic", "claude-sonnet-4-6");
        let chain = resolve_failover_chain(&pm, "anthropic", "claude-sonnet-4-6");
        assert_eq!(chain.len(), 2, "bare + qualified both resolve");
        assert_eq!(chain[0].model, "deepseek-v4-flash");
        assert_eq!(chain[0].provider, LlmProvider::DeepSeek);
        assert_eq!(chain[0].base_url, "https://api.deepseek.com");
        assert_eq!(
            chain[0].api_key, "sk-ds-test",
            "qualified hops resolve the OWNING profile's credential"
        );
        // Bare id keeps the active provider's identity + endpoint.
        assert_eq!(chain[1].model, "glm-5-flash");
        assert_eq!(chain[1].provider, LlmProvider::Anthropic);
        assert_eq!(chain[1].base_url, "https://api.anthropic.com");
    }

    #[test]
    fn resolve_failover_chain_skips_unconnected_provider_with_warning() {
        let active = profile_with_fallbacks(
            anthropic_profile("K"),
            &["nosuch/some-model", "glm-5-flash"],
        );
        let pm = v2_multi_provider_config(vec![active], "anthropic", "claude-sonnet-4-6");
        let chain = resolve_failover_chain(&pm, "anthropic", "claude-sonnet-4-6");
        // The unknown qualified id is skipped (warning, not error); the bare
        // id still resolves.
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].model, "glm-5-flash");
    }

    #[test]
    fn resolve_failover_chain_caps_at_engine_limit() {
        let active =
            profile_with_fallbacks(anthropic_profile("K"), &["m1", "m2", "m3", "m4", "m5"]);
        let pm = v2_multi_provider_config(vec![active], "anthropic", "claude-sonnet-4-6");
        let chain = resolve_failover_chain(&pm, "anthropic", "claude-sonnet-4-6");
        assert_eq!(
            chain.len(),
            shannon_engine::api::retry::MAX_FAILOVER_TARGETS,
            "the chain can never exceed the engine cap"
        );
        assert_eq!(
            chain.iter().map(|t| t.model.as_str()).collect::<Vec<_>>(),
            vec!["m1", "m2", "m3"],
            "order is the author's list order"
        );
    }

    #[test]
    fn resolve_failover_chain_skips_empty_and_self_entries() {
        let active = profile_with_fallbacks(
            anthropic_profile("K"),
            &["", "  ", "claude-sonnet-4-6", "anthropic/claude-sonnet-4-6"],
        );
        let pm = v2_multi_provider_config(vec![active], "anthropic", "claude-sonnet-4-6");
        let chain = resolve_failover_chain(&pm, "anthropic", "claude-sonnet-4-6");
        assert!(
            chain.is_empty(),
            "empty ids and the active target itself must not become hops"
        );
    }

    #[test]
    fn resolve_failover_chain_accepts_provider_slug_vocabulary() {
        // The profile is stored under the slug "glm" but the fallback names
        // "zhipu/model" — both canonicalize to LlmProvider::Zhipu.
        let mut glm = deepseek_profile();
        glm.id = "glm".to_string();
        glm.kind = ProviderKind::OpenAiCompatible;
        glm.base_url = "https://open.bigmodel.cn/api/paas/v4".to_string();
        let active = profile_with_fallbacks(anthropic_profile("K"), &["zhipu/glm-5-flash"]);
        let pm = v2_multi_provider_config(vec![active, glm], "anthropic", "claude-sonnet-4-6");
        let chain = resolve_failover_chain(&pm, "anthropic", "claude-sonnet-4-6");
        assert_eq!(chain.len(), 1, "slug vocabularies canonicalize");
        assert_eq!(chain[0].model, "glm-5-flash");
        assert_eq!(chain[0].base_url, "https://open.bigmodel.cn/api/paas/v4");
    }

    /// End-to-end: `build_client_from_resolved` carries the resolved chain
    /// on the client's retry config; an empty `fallback_models` keeps the
    /// exact `RetryConfig::default()` (failover stays opt-in).
    #[test]
    fn build_client_from_resolved_wires_failover_chain() {
        let active =
            profile_with_fallbacks(anthropic_profile("K"), &["deepseek/deepseek-v4-flash"]);
        let ds = deepseek_profile();
        let pm = v2_multi_provider_config(vec![active, ds], "anthropic", "claude-sonnet-4-6");
        let cfg = ShannonConfig {
            provider_model: pm,
            ..Default::default()
        };
        let rt = crate::provider_resolver::resolve_active_target(&cfg.provider_model)
            .expect("active target resolves");
        let client = build_client_from_resolved(&cfg, rt);
        let chain = &client.retry_config.fallbacks;
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].provider, LlmProvider::DeepSeek);
        assert_eq!(chain[0].model, "deepseek-v4-flash");
        assert!(!client.retry_config.suppress_failover);

        // Empty list → byte-default retry config (no failover).
        let pm2 = v2_multi_provider_config(
            vec![anthropic_profile("K")],
            "anthropic",
            "claude-sonnet-4-6",
        );
        let cfg2 = ShannonConfig {
            provider_model: pm2,
            ..Default::default()
        };
        let rt2 = crate::provider_resolver::resolve_active_target(&cfg2.provider_model)
            .expect("active target resolves");
        let client2 = build_client_from_resolved(&cfg2, rt2);
        assert!(client2.retry_config.fallbacks.is_empty());
        assert_eq!(
            client2.retry_config.max_retries,
            shannon_engine::api::RetryConfig::default().max_retries
        );
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
            tiers: ProviderTiers::default(),
            models: Vec::new(),
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
