//! # Feature Flags System
//!
//! Lightweight feature flag management with multi-source resolution.
//!
//! ## Flag Resolution Order (highest priority first)
//!
//! 1. **Environment variable**: `SHANNON_FEATURE_<FLAG_NAME>=1/0`
//! 2. **Runtime override**: `FeatureFlagManager::set_override(flag, enabled)`
//! 3. **Config file**: `settings.json` → `"features": { "agent_teams": true }`
//! 4. **Default**: each flag has a built-in default value
//!
//! ## Example
//!
//! ```no_run
//! use shannon_core::feature_flags::{FeatureFlagManager, flags};
//!
//! let manager = FeatureFlagManager::new();
//! if manager.is_enabled(&flags::AGENT_TEAMS) {
//!     println!("Agent teams are enabled");
//! }
//! ```

use serde::{Deserialize, Serialize};
use shannon_types::recover_lock;
use std::collections::HashMap;
use std::path::PathBuf;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur during feature flag operations.
#[derive(Error, Debug)]
pub enum FlagError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Unknown feature flag: {0}")]
    UnknownFlag(String),

    #[error("Home directory not found")]
    HomeNotFound,
}

// ---------------------------------------------------------------------------
// Well-known flag definitions
// ---------------------------------------------------------------------------

/// Static metadata for a well-known feature flag.
struct FlagDef {
    name: &'static str,
    default: bool,
    description: &'static str,
}

/// Registry of all well-known feature flags.
///
/// Add new flags here to make them available via `flags::*` constants and the
/// `all_flags()` listing.
static FLAG_DEFINITIONS: &[FlagDef] = &[
    FlagDef {
        name: "agent_teams",
        default: false,
        description: "Multi-agent team orchestration",
    },
    FlagDef {
        name: "custom_agents",
        default: false,
        description: "User-defined custom agent creation",
    },
    FlagDef {
        name: "progressive_skills",
        default: false,
        description: "Progressive skill loading and discovery",
    },
    FlagDef {
        name: "voice_mode",
        default: false,
        description: "Voice input/output mode",
    },
    FlagDef {
        name: "auto_memory",
        default: true,
        description: "Automatic memory extraction from conversations",
    },
    FlagDef {
        name: "otlp_telemetry",
        default: false,
        description: "OpenTelemetry protocol telemetry export",
    },
];

fn find_def(name: &str) -> Option<&'static FlagDef> {
    FLAG_DEFINITIONS.iter().find(|d| d.name == name)
}

fn default_value(name: &str) -> bool {
    find_def(name).map(|d| d.default).unwrap_or(false)
}

fn is_known_flag(name: &str) -> bool {
    find_def(name).is_some()
}

// ---------------------------------------------------------------------------
// FeatureFlag identifier
// ---------------------------------------------------------------------------

/// Feature flag identifier.
///
/// Create via the constants in [`flags`] or from a string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FeatureFlag(&'static str);

impl FeatureFlag {
    /// Create a new feature flag identifier from a static string.
    ///
    /// This is const-compatible and used for well-known flag constants.
    pub const fn new(name: &'static str) -> Self {
        Self(name)
    }

    /// Create a new feature flag identifier from a runtime-owned string.
    ///
    /// The string is leaked to obtain a `'static` reference. Only use this
    /// for long-lived flag names (e.g. from static definitions).
    pub fn from_name(name: impl Into<String>) -> Self {
        Self(Box::leak(name.into().into_boxed_str()))
    }

    /// The flag name (e.g. `"agent_teams"`).
    pub fn name(&self) -> &str {
        self.0
    }
}

impl std::fmt::Display for FeatureFlag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl AsRef<str> for FeatureFlag {
    fn as_ref(&self) -> &str {
        self.0
    }
}

/// Well-known feature flag constants.
///
/// These correspond to the flags defined in the internal registry and provide
/// convenient typed access to the most common flags.
pub mod flags {
    use super::FeatureFlag;

    pub const AGENT_TEAMS: FeatureFlag = FeatureFlag::new("agent_teams");
    pub const CUSTOM_AGENTS: FeatureFlag = FeatureFlag::new("custom_agents");
    pub const PROGRESSIVE_SKILLS: FeatureFlag = FeatureFlag::new("progressive_skills");
    pub const VOICE_MODE: FeatureFlag = FeatureFlag::new("voice_mode");
    pub const AUTO_MEMORY: FeatureFlag = FeatureFlag::new("auto_memory");
    pub const OTLP_TELEMETRY: FeatureFlag = FeatureFlag::new("otlp_telemetry");
}

// ---------------------------------------------------------------------------
// Flag source / status reporting
// ---------------------------------------------------------------------------

/// Where a flag value was resolved from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FlagSource {
    /// Environment variable `SHANNON_FEATURE_*`.
    Environment,
    /// Runtime override set via `set_override`.
    Override,
    /// Value read from `settings.json`.
    ConfigFile,
    /// Built-in default.
    Default,
}

impl std::fmt::Display for FlagSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FlagSource::Environment => f.write_str("environment"),
            FlagSource::Override => f.write_str("override"),
            FlagSource::ConfigFile => f.write_str("config"),
            FlagSource::Default => f.write_str("default"),
        }
    }
}

/// The resolved state of a single flag.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlagStatus {
    pub name: String,
    pub enabled: bool,
    pub source: FlagSource,
    pub description: String,
}

// ---------------------------------------------------------------------------
// Config file representation (subset of settings.json)
// ---------------------------------------------------------------------------

/// Minimal serde shape we expect from `settings.json` for the `"features"` key.
#[derive(Debug, Default, Deserialize)]
struct FeaturesConfig {
    #[serde(default)]
    features: HashMap<String, bool>,
}

// ---------------------------------------------------------------------------
// FeatureFlagManager
// ---------------------------------------------------------------------------

/// Manages feature flag resolution from multiple sources.
///
/// Thread-safe: `FeatureFlagManager` is `Send + Sync` because it only uses
/// `std::sync::RwLock` for interior mutability of runtime overrides and
/// immutable reads for all other sources.
pub struct FeatureFlagManager {
    /// Runtime overrides set via `set_override`.
    overrides: std::sync::RwLock<HashMap<String, bool>>,
    /// Path to `settings.json` (may not exist).
    config_file: Option<PathBuf>,
    /// Cached config file contents (loaded lazily).
    config_cache: std::sync::RwLock<Option<HashMap<String, bool>>>,
}

impl std::fmt::Debug for FeatureFlagManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FeatureFlagManager")
            .field("config_file", &self.config_file)
            .finish_non_exhaustive()
    }
}

impl Default for FeatureFlagManager {
    fn default() -> Self {
        Self::new()
    }
}

impl FeatureFlagManager {
    // -- Construction -------------------------------------------------------

    /// Create a new manager that reads from the default user config path
    /// (`~/.shannon/settings.json`).
    pub fn new() -> Self {
        let config_file = dirs::home_dir().map(|h| h.join(".shannon").join("settings.json"));
        Self {
            overrides: std::sync::RwLock::new(HashMap::new()),
            config_file,
            config_cache: std::sync::RwLock::new(None),
        }
    }

    /// Create a manager that reads from an explicit config file path.
    pub fn with_config_file(path: PathBuf) -> Self {
        Self {
            overrides: std::sync::RwLock::new(HashMap::new()),
            config_file: Some(path),
            config_cache: std::sync::RwLock::new(None),
        }
    }

    // -- Resolution ---------------------------------------------------------

    /// Check whether a feature flag is enabled.
    ///
    /// Resolution order:
    /// 1. Environment variable
    /// 2. Runtime override
    /// 3. Config file (`settings.json`)
    /// 4. Built-in default
    pub fn is_enabled(&self, flag: &FeatureFlag) -> bool {
        self.resolve(flag).0
    }

    /// Resolve a flag, returning `(enabled, source)`.
    fn resolve(&self, flag: &FeatureFlag) -> (bool, FlagSource) {
        let name = flag.name();

        // 1. Environment variable
        let env_key = format!("SHANNON_FEATURE_{}", name.to_uppercase());
        if let Ok(val) = std::env::var(&env_key) {
            match val.as_str() {
                "1" | "true" | "yes" | "on" => return (true, FlagSource::Environment),
                "0" | "false" | "no" | "off" => return (false, FlagSource::Environment),
                _ => {
                    // Unrecognised value — fall through to next source.
                    tracing::warn!(
                        "Unrecognised value '{val}' for env var {env_key}, expected 1/0/true/false"
                    );
                }
            }
        }

        // 2. Runtime override
        {
            let overrides = recover_lock(self.overrides.read());
            if let Some(&enabled) = overrides.get(name) {
                return (enabled, FlagSource::Override);
            }
        }

        // 3. Config file
        if let Some(enabled) = self.read_config_flag(name) {
            return (enabled, FlagSource::ConfigFile);
        }

        // 4. Default
        (default_value(name), FlagSource::Default)
    }

    // -- Runtime overrides --------------------------------------------------

    /// Set (or clear) a runtime override for a flag.
    ///
    /// Runtime overrides take priority over config-file values but are
    /// lower priority than environment variables.
    pub fn set_override(&self, flag: &str, enabled: bool) {
        let mut overrides = recover_lock(self.overrides.write());
        overrides.insert(flag.to_lowercase(), enabled);
    }

    /// Remove a runtime override, reverting to the next source in the
    /// resolution chain.
    #[allow(dead_code)]
    pub fn clear_override(&self, flag: &str) {
        let mut overrides = recover_lock(self.overrides.write());
        overrides.remove(&flag.to_lowercase());
    }

    // -- Listing ------------------------------------------------------------

    /// Return the status of every well-known flag.
    pub fn all_flags(&self) -> Vec<FlagStatus> {
        FLAG_DEFINITIONS
            .iter()
            .map(|def| {
                let flag = FeatureFlag::from_name(def.name);
                let (enabled, source) = self.resolve(&flag);
                FlagStatus {
                    name: def.name.to_string(),
                    enabled,
                    source,
                    description: def.description.to_string(),
                }
            })
            .collect()
    }

    // -- CLI helpers (enable / disable) -------------------------------------

    /// Enable a well-known flag by writing it into `settings.json`.
    ///
    /// Returns an error for unknown flag names or when the config path cannot
    /// be resolved.
    pub fn enable(&self, flag_name: &str) -> Result<(), FlagError> {
        self.write_config_flag(flag_name, true)
    }

    /// Disable a well-known flag by writing it into `settings.json`.
    pub fn disable(&self, flag_name: &str) -> Result<(), FlagError> {
        self.write_config_flag(flag_name, false)
    }

    // -- Config file I/O ----------------------------------------------------

    /// Read a single flag from the config file (with caching).
    fn read_config_flag(&self, name: &str) -> Option<bool> {
        let cache = recover_lock(self.config_cache.read());
        if let Some(ref map) = *cache {
            return map.get(name).copied();
        }
        drop(cache);

        // Lazy-load the config file.
        let loaded = self.load_config_features();
        let mut cache = recover_lock(self.config_cache.write());
        *cache = Some(loaded.clone());
        loaded.get(name).copied()
    }

    /// Parse `"features"` map from the config file.
    fn load_config_features(&self) -> HashMap<String, bool> {
        let path = match &self.config_file {
            Some(p) => p,
            None => return HashMap::new(),
        };

        if !path.exists() {
            return HashMap::new();
        }

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to read feature flags from {}: {e}", path.display());
                return HashMap::new();
            }
        };

        let config: FeaturesConfig = match serde_json::from_str(&content) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
                    "Failed to parse feature flags from {}: {e}",
                    path.display()
                );
                return HashMap::new();
            }
        };

        config.features
    }

    /// Write a flag value into the `"features"` object of `settings.json`.
    ///
    /// If the file does not exist it will be created. The function preserves
    /// any existing top-level keys in the JSON file.
    fn write_config_flag(&self, flag_name: &str, enabled: bool) -> Result<(), FlagError> {
        let flag_name = flag_name.to_lowercase();

        if !is_known_flag(&flag_name) {
            return Err(FlagError::UnknownFlag(flag_name));
        }

        let path = self
            .config_file
            .as_ref()
            .ok_or(FlagError::HomeNotFound)?;

        // Ensure parent directory exists.
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Read existing content or start fresh.
        let mut root: serde_json::Value = if path.exists() {
            let content = std::fs::read_to_string(path)?;
            serde_json::from_str(&content).unwrap_or(serde_json::Value::Object(Default::default()))
        } else {
            serde_json::Value::Object(Default::default())
        };

        // Insert into the "features" object.
        let features = root
            .as_object_mut()
            .expect("root is always an Object")
            .entry("features")
            .or_insert_with(|| serde_json::Value::Object(Default::default()));

        if let Some(map) = features.as_object_mut() {
            map.insert(
                flag_name.clone(),
                serde_json::Value::Bool(enabled),
            );
        }

        // Write back.
        let json = serde_json::to_string_pretty(&root)?;
        std::fs::write(path, json)?;

        // Invalidate cache so next read picks up the change.
        let mut cache = recover_lock(self.config_cache.write());
        *cache = None;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to build a manager with an isolated temp-dir config file.
    fn manager_with_temp() -> (FeatureFlagManager, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("settings.json");
        let manager = FeatureFlagManager::with_config_file(path);
        (manager, dir)
    }

    /// Helper to set an env var for the duration of a test, restoring it on drop.
    struct EnvGuard {
        key: String,
        original: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &str, value: &str) -> Self {
            let original = std::env::var(key).ok();
            // SAFETY: test-only code, single-threaded with --test-threads=1
            unsafe {
                std::env::set_var(key, value);
            }
            Self {
                key: key.to_string(),
                original,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // SAFETY: test-only code, single-threaded with --test-threads=1
            unsafe {
                match &self.original {
                    Some(v) => std::env::set_var(&self.key, v),
                    None => std::env::remove_var(&self.key),
                }
            }
        }
    }

    // -- Default values -----------------------------------------------------

    #[test]
    fn test_default_values() {
        let (manager, _dir) = manager_with_temp();

        assert!(!manager.is_enabled(&flags::AGENT_TEAMS));
        assert!(!manager.is_enabled(&flags::CUSTOM_AGENTS));
        assert!(!manager.is_enabled(&flags::PROGRESSIVE_SKILLS));
        assert!(!manager.is_enabled(&flags::VOICE_MODE));
        assert!(manager.is_enabled(&flags::AUTO_MEMORY));
        assert!(!manager.is_enabled(&flags::OTLP_TELEMETRY));
    }

    #[test]
    fn test_default_for_unknown_flag() {
        let (manager, _dir) = manager_with_temp();
        let unknown = FeatureFlag::new("nonexistent_flag");
        // Unknown flags default to false.
        assert!(!manager.is_enabled(&unknown));
    }

    // -- Environment variable override --------------------------------------

    #[test]
    fn test_env_override_enables_flag() {
        let (manager, _dir) = manager_with_temp();
        let _guard = EnvGuard::set("SHANNON_FEATURE_AGENT_TEAMS", "1");
        assert!(manager.is_enabled(&flags::AGENT_TEAMS));
    }

    #[test]
    fn test_env_override_disables_flag() {
        let (manager, _dir) = manager_with_temp();
        // AUTO_MEMORY defaults to true; env should disable it.
        let _guard = EnvGuard::set("SHANNON_FEATURE_AUTO_MEMORY", "0");
        assert!(!manager.is_enabled(&flags::AUTO_MEMORY));
    }

    #[test]
    fn test_env_override_various_truthy_values() {
        let (manager, _dir) = manager_with_temp();
        {
            let _g = EnvGuard::set("SHANNON_FEATURE_VOICE_MODE", "true");
            assert!(manager.is_enabled(&flags::VOICE_MODE));
        }
        {
            let _g = EnvGuard::set("SHANNON_FEATURE_VOICE_MODE", "yes");
            assert!(manager.is_enabled(&flags::VOICE_MODE));
        }
        {
            let _g = EnvGuard::set("SHANNON_FEATURE_VOICE_MODE", "on");
            assert!(manager.is_enabled(&flags::VOICE_MODE));
        }
    }

    #[test]
    fn test_env_override_various_falsy_values() {
        let (manager, _dir) = manager_with_temp();
        {
            let _g = EnvGuard::set("SHANNON_FEATURE_AUTO_MEMORY", "false");
            assert!(!manager.is_enabled(&flags::AUTO_MEMORY));
        }
        {
            let _g = EnvGuard::set("SHANNON_FEATURE_AUTO_MEMORY", "no");
            assert!(!manager.is_enabled(&flags::AUTO_MEMORY));
        }
        {
            let _g = EnvGuard::set("SHANNON_FEATURE_AUTO_MEMORY", "off");
            assert!(!manager.is_enabled(&flags::AUTO_MEMORY));
        }
    }

    // -- Runtime override ---------------------------------------------------

    #[test]
    fn test_runtime_override() {
        let (manager, _dir) = manager_with_temp();
        assert!(!manager.is_enabled(&flags::AGENT_TEAMS));

        manager.set_override("agent_teams", true);
        assert!(manager.is_enabled(&flags::AGENT_TEAMS));
    }

    #[test]
    fn test_runtime_override_case_insensitive() {
        let (manager, _dir) = manager_with_temp();
        manager.set_override("AGENT_TEAMS", true);
        assert!(manager.is_enabled(&flags::AGENT_TEAMS));
    }

    #[test]
    fn test_clear_override() {
        let (manager, _dir) = manager_with_temp();
        manager.set_override("agent_teams", true);
        assert!(manager.is_enabled(&flags::AGENT_TEAMS));

        manager.clear_override("agent_teams");
        assert!(!manager.is_enabled(&flags::AGENT_TEAMS));
    }

    // -- Config file --------------------------------------------------------

    #[test]
    fn test_config_file_loading() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("settings.json");
        let json = r#"{"features": {"agent_teams": true, "voice_mode": true}}"#;
        std::fs::write(&path, json).unwrap();

        let manager = FeatureFlagManager::with_config_file(path);
        assert!(manager.is_enabled(&flags::AGENT_TEAMS));
        assert!(manager.is_enabled(&flags::VOICE_MODE));
        assert!(!manager.is_enabled(&flags::CUSTOM_AGENTS));
    }

    #[test]
    fn test_config_file_missing_features_key() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"model": "opus"}"#).unwrap();

        let manager = FeatureFlagManager::with_config_file(path);
        // Should fall through to defaults.
        assert!(!manager.is_enabled(&flags::AGENT_TEAMS));
    }

    #[test]
    fn test_config_file_missing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("nonexistent").join("settings.json");

        let manager = FeatureFlagManager::with_config_file(path);
        assert!(!manager.is_enabled(&flags::AGENT_TEAMS));
    }

    #[test]
    fn test_config_file_invalid_json() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "not json").unwrap();

        let manager = FeatureFlagManager::with_config_file(path);
        // Should gracefully fall back to defaults.
        assert!(!manager.is_enabled(&flags::AGENT_TEAMS));
    }

    // -- Priority order -----------------------------------------------------

    #[test]
    fn test_env_overrides_runtime() {
        let (manager, _dir) = manager_with_temp();
        manager.set_override("agent_teams", true);

        // Env says disabled → should win.
        let _guard = EnvGuard::set("SHANNON_FEATURE_AGENT_TEAMS", "0");
        assert!(!manager.is_enabled(&flags::AGENT_TEAMS));
    }

    #[test]
    fn test_runtime_overrides_config() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"features": {"agent_teams": true}}"#).unwrap();

        let manager = FeatureFlagManager::with_config_file(path);
        assert!(manager.is_enabled(&flags::AGENT_TEAMS));

        // Runtime override says disabled → should win over config.
        manager.set_override("agent_teams", false);
        assert!(!manager.is_enabled(&flags::AGENT_TEAMS));
    }

    #[test]
    fn test_config_overrides_default() {
        let (manager, _dir) = manager_with_temp();
        // AUTO_MEMORY defaults to true; with no config file it stays true.
        assert!(manager.is_enabled(&flags::AUTO_MEMORY));

        // Now write a config file that disables it.
        let path = manager.config_file.as_ref().unwrap().clone();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"features": {"auto_memory": false}}"#).unwrap();

        // New manager reads the config.
        let manager2 = FeatureFlagManager::with_config_file(path);
        assert!(!manager2.is_enabled(&flags::AUTO_MEMORY));
    }

    #[test]
    fn test_full_priority_chain() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("settings.json");
        // Config enables it.
        std::fs::write(&path, r#"{"features": {"agent_teams": true}}"#).unwrap();

        let manager = FeatureFlagManager::with_config_file(path);

        // 1. Config says true.
        assert!(manager.is_enabled(&flags::AGENT_TEAMS));

        // 2. Runtime override says false.
        manager.set_override("agent_teams", false);
        assert!(!manager.is_enabled(&flags::AGENT_TEAMS));

        // 3. Env says true → wins over everything.
        let _guard = EnvGuard::set("SHANNON_FEATURE_AGENT_TEAMS", "1");
        assert!(manager.is_enabled(&flags::AGENT_TEAMS));
    }

    // -- Enable / disable writes to config ----------------------------------

    #[test]
    fn test_enable_writes_to_config() {
        let (manager, dir) = manager_with_temp();
        manager.enable("agent_teams").unwrap();

        let path = dir.path().join("settings.json");
        let content = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["features"]["agent_teams"], serde_json::Value::Bool(true));

        // Verify the manager now sees it enabled (from fresh cache).
        let manager2 = FeatureFlagManager::with_config_file(path);
        assert!(manager2.is_enabled(&flags::AGENT_TEAMS));
    }

    #[test]
    fn test_disable_writes_to_config() {
        let (manager, dir) = manager_with_temp();
        // First enable, then disable.
        manager.enable("auto_memory").unwrap();
        manager.disable("auto_memory").unwrap();

        let path = dir.path().join("settings.json");
        let content = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["features"]["auto_memory"], serde_json::Value::Bool(false));
    }

    #[test]
    fn test_enable_preserves_existing_config() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"model": "opus", "features": {"voice_mode": true}}"#).unwrap();

        let manager = FeatureFlagManager::with_config_file(path.clone());
        manager.enable("agent_teams").unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        // Original keys preserved.
        assert_eq!(parsed["model"], serde_json::Value::String("opus".to_string()));
        assert_eq!(parsed["features"]["voice_mode"], serde_json::Value::Bool(true));
        // New flag added.
        assert_eq!(parsed["features"]["agent_teams"], serde_json::Value::Bool(true));
    }

    #[test]
    fn test_enable_unknown_flag_returns_error() {
        let (manager, _dir) = manager_with_temp();
        let result = manager.enable("totally_unknown_flag");
        assert!(result.is_err());
        match result.unwrap_err() {
            FlagError::UnknownFlag(name) => assert_eq!(name, "totally_unknown_flag"),
            other => panic!("expected UnknownFlag, got {other}"),
        }
    }

    #[test]
    fn test_disable_unknown_flag_returns_error() {
        let (manager, _dir) = manager_with_temp();
        let result = manager.disable("no_such_flag");
        assert!(result.is_err());
    }

    // -- all_flags ----------------------------------------------------------

    #[test]
    fn test_all_flags_returns_all_definitions() {
        let (manager, _dir) = manager_with_temp();
        let statuses = manager.all_flags();
        assert_eq!(statuses.len(), FLAG_DEFINITIONS.len());
    }

    #[test]
    fn test_all_flags_includes_source() {
        let (manager, _dir) = manager_with_temp();
        let statuses = manager.all_flags();
        for status in &statuses {
            if status.name == "auto_memory" {
                assert!(status.enabled);
                assert_eq!(status.source, FlagSource::Default);
            } else {
                assert!(!status.enabled);
                assert_eq!(status.source, FlagSource::Default);
            }
        }
    }

    // -- Thread safety ------------------------------------------------------

    #[test]
    fn test_manager_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<FeatureFlagManager>();
    }

    #[test]
    fn test_concurrent_access() {
        let (manager, _dir) = manager_with_temp();
        let manager = std::sync::Arc::new(manager);

        let flags_list: Vec<&FeatureFlag> = vec![
            &flags::AGENT_TEAMS,
            &flags::CUSTOM_AGENTS,
            &flags::PROGRESSIVE_SKILLS,
            &flags::VOICE_MODE,
        ];

        let mut handles = Vec::new();
        for (i, flag) in flags_list.into_iter().enumerate() {
            let m = std::sync::Arc::clone(&manager);
            let flag_name = flag.name().to_string();
            handles.push(std::thread::spawn(move || {
                let enabled = i % 2 == 0;
                m.set_override(&flag_name, enabled);
                assert_eq!(m.is_enabled(flag), enabled);
            }));
        }

        for handle in handles {
            handle.join().expect("thread panicked");
        }
    }

    // -- FeatureFlag basics -------------------------------------------------

    #[test]
    fn test_feature_flag_display() {
        assert_eq!(flags::AGENT_TEAMS.to_string(), "agent_teams");
    }

    #[test]
    fn test_feature_flag_equality() {
        let a = FeatureFlag::new("agent_teams");
        assert_eq!(a, flags::AGENT_TEAMS);
    }

    #[test]
    fn test_flag_source_display() {
        assert_eq!(FlagSource::Environment.to_string(), "environment");
        assert_eq!(FlagSource::Override.to_string(), "override");
        assert_eq!(FlagSource::ConfigFile.to_string(), "config");
        assert_eq!(FlagSource::Default.to_string(), "default");
    }
}
