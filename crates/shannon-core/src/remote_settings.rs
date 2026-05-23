//! # Remote Managed Settings
//!
//! Organization-level remote settings with a hierarchical override system.
//!
//! ## Architecture
//!
//! Settings follow a priority hierarchy: **local < org < remote**
//!
//! - **Local settings**: User-defined settings on the device
//! - **Organization settings**: Managed by the organization admin
//! - **Remote settings**: Fetched from a centralized management service
//!
//! Higher-priority sources override lower-priority ones. Each override
//! tracks its source, priority, and optional expiration.
//!
//! ## Example
//!
//! ```ignore
//! use shannon_core::remote_settings::{RemoteSettingsProvider, RemoteManagedSettings};
//!
//! let mut settings = RemoteManagedSettings::new();
//! settings.set_local("model", "claude-opus-4-6");
//! settings.set_org("permissions_mode", "readonly");
//! settings.set_remote("allowed_tools", "read,search");
//!
//! // Effective value for "model" is "claude-opus-4-6" (local)
//! // Effective value for "permissions_mode" is "readonly" (org overrides local)
//! // Effective value for "allowed_tools" is "read,search" (remote overrides all)
//! ```

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during remote settings operations.
#[derive(Error, Debug)]
pub enum RemoteSettingsError {
    #[error("Setting not found: {0}")]
    NotFound(String),

    #[error("Setting has expired: {0}")]
    Expired(String),

    #[error("Fetch error: {0}")]
    FetchError(String),

    #[error("Apply error: {0}")]
    ApplyError(String),

    #[error("Invalid priority: {0}. Must be between 0 and 100")]
    InvalidPriority(i32),

    #[error("Serialization error: {0}")]
    Serialization(String),
}

// ============================================================================
// Core Types
// ============================================================================

/// Source of a setting override.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum SettingSource {
    /// User-defined on the local device.
    Local,
    /// Managed by the organization admin.
    Org,
    /// Fetched from a centralized remote service.
    Remote,
}

impl std::fmt::Display for SettingSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SettingSource::Local => write!(f, "local"),
            SettingSource::Org => write!(f, "org"),
            SettingSource::Remote => write!(f, "remote"),
        }
    }
}

impl SettingSource {
    /// Return the default priority for this source.
    pub fn default_priority(&self) -> i32 {
        match self {
            SettingSource::Local => 10,
            SettingSource::Org => 50,
            SettingSource::Remote => 90,
        }
    }
}

/// A setting override with metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SettingOverride {
    /// The setting key.
    pub key: String,
    /// The setting value.
    pub value: String,
    /// Where this override comes from.
    pub source: SettingSource,
    /// Priority level (0-100). Higher values take precedence.
    pub priority: i32,
    /// Optional expiration time for time-limited overrides.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
}

impl SettingOverride {
    /// Create a new setting override.
    pub fn new(
        key: &str,
        value: &str,
        source: SettingSource,
        priority: i32,
    ) -> Result<Self, RemoteSettingsError> {
        if !(0..=100).contains(&priority) {
            return Err(RemoteSettingsError::InvalidPriority(priority));
        }
        Ok(Self {
            key: key.to_string(),
            value: value.to_string(),
            source,
            priority,
            expires_at: None,
        })
    }

    /// Create a new setting override with default priority for the source.
    pub fn with_defaults(key: &str, value: &str, source: SettingSource) -> Self {
        Self {
            key: key.to_string(),
            value: value.to_string(),
            source,
            priority: source.default_priority(),
            expires_at: None,
        }
    }

    /// Create a time-limited override that expires at the given time.
    pub fn with_expiry(
        key: &str,
        value: &str,
        source: SettingSource,
        priority: i32,
        expires_at: DateTime<Utc>,
    ) -> Result<Self, RemoteSettingsError> {
        let mut override_val = Self::new(key, value, source, priority)?;
        override_val.expires_at = Some(expires_at);
        Ok(override_val)
    }

    /// Check if this override has expired.
    pub fn is_expired(&self) -> bool {
        match self.expires_at {
            Some(expiry) => Utc::now() >= expiry,
            None => false,
        }
    }
}

// ============================================================================
// Remote Settings Provider Trait
// ============================================================================

/// Trait for fetching and applying remote settings from a centralized service.
#[async_trait]
pub trait RemoteSettingsProvider: Send + Sync {
    /// Fetch the current remote settings from the management service.
    ///
    /// Returns a list of setting overrides to apply.
    async fn fetch(&self) -> Result<Vec<SettingOverride>, RemoteSettingsError>;

    /// Apply the given settings overrides to the managed settings store.
    async fn apply(&self, overrides: Vec<SettingOverride>) -> Result<(), RemoteSettingsError>;

    /// Check if the provider is available and reachable.
    async fn is_available(&self) -> bool;
}

// ============================================================================
// Remote Managed Settings
// ============================================================================

/// Organization-level remote managed settings with hierarchical overrides.
///
/// Settings are resolved using the priority hierarchy: **local < org < remote**.
/// For each key, the override with the highest priority wins. If two overrides
/// have the same priority, the most recently added one wins.
///
/// Expired overrides are automatically excluded from resolution.
pub struct RemoteManagedSettings {
    /// All setting overrides, keyed by setting key.
    /// Multiple overrides per key can exist (from different sources).
    overrides: HashMap<String, Vec<SettingOverride>>,
}

impl Default for RemoteManagedSettings {
    fn default() -> Self {
        Self::new()
    }
}

impl RemoteManagedSettings {
    /// Create a new empty managed settings store.
    pub fn new() -> Self {
        Self {
            overrides: HashMap::new(),
        }
    }

    // -----------------------------------------------------------------------
    // Setting Operations
    // -----------------------------------------------------------------------

    /// Set a local setting override.
    pub fn set_local(&mut self, key: &str, value: &str) {
        self.insert_override(SettingOverride::with_defaults(
            key,
            value,
            SettingSource::Local,
        ));
    }

    /// Set an organization-level setting override.
    pub fn set_org(&mut self, key: &str, value: &str) {
        self.insert_override(SettingOverride::with_defaults(
            key,
            value,
            SettingSource::Org,
        ));
    }

    /// Set a remote setting override.
    pub fn set_remote(&mut self, key: &str, value: &str) {
        self.insert_override(SettingOverride::with_defaults(
            key,
            value,
            SettingSource::Remote,
        ));
    }

    /// Set a setting override with explicit priority and optional expiry.
    pub fn set_override(&mut self, override_val: SettingOverride) {
        self.insert_override(override_val);
    }

    /// Get the effective value for a setting key.
    ///
    /// Returns the value from the highest-priority, non-expired override.
    /// If no override exists for the key, returns `None`.
    pub fn get(&self, key: &str) -> Option<&str> {
        let active = self.active_overrides(key);
        active.first().map(|o| o.value.as_str())
    }

    /// Get the effective value and its source for a setting key.
    pub fn get_with_source(&self, key: &str) -> Option<(&str, SettingSource)> {
        let active = self.active_overrides(key);
        active.first().map(|o| (o.value.as_str(), o.source))
    }

    /// Remove all overrides for a setting key.
    pub fn remove(&mut self, key: &str) {
        self.overrides.remove(key);
    }

    /// Remove overrides for a key from a specific source.
    pub fn remove_by_source(&mut self, key: &str, source: SettingSource) {
        if let Some(overrides) = self.overrides.get_mut(key) {
            overrides.retain(|o| o.source != source);
        }
    }

    /// Get all setting keys that have at least one override.
    pub fn keys(&self) -> Vec<&String> {
        self.overrides.keys().collect()
    }

    /// Check if a setting key has any overrides.
    pub fn contains(&self, key: &str) -> bool {
        self.overrides.contains_key(key)
    }

    /// Get all active overrides for a key, sorted by priority descending.
    pub fn active_overrides(&self, key: &str) -> Vec<&SettingOverride> {
        match self.overrides.get(key) {
            Some(all) => {
                let mut active: Vec<&SettingOverride> =
                    all.iter().filter(|o| !o.is_expired()).collect();
                active.sort_by(|a, b| b.priority.cmp(&a.priority));
                active
            }
            None => Vec::new(),
        }
    }

    /// Get all overrides for a key, including expired ones.
    pub fn all_overrides(&self, key: &str) -> Vec<&SettingOverride> {
        match self.overrides.get(key) {
            Some(all) => {
                let mut sorted: Vec<&SettingOverride> = all.iter().collect();
                sorted.sort_by(|a, b| b.priority.cmp(&a.priority));
                sorted
            }
            None => Vec::new(),
        }
    }

    /// Get all settings with their effective values and sources.
    pub fn all_settings(&self) -> HashMap<String, (String, SettingSource)> {
        let mut result = HashMap::new();
        for key in self.overrides.keys() {
            if let Some((value, source)) = self.get_with_source(key) {
                result.insert(key.clone(), (value.to_string(), source));
            }
        }
        result
    }

    /// Clean up all expired overrides.
    ///
    /// Returns the number of expired overrides removed.
    pub fn cleanup_expired(&mut self) -> usize {
        let mut removed = 0;
        for overrides in self.overrides.values_mut() {
            let before = overrides.len();
            overrides.retain(|o| !o.is_expired());
            removed += before - overrides.len();
        }

        // Remove empty key entries
        self.overrides.retain(|_, v| !v.is_empty());

        removed
    }

    /// Get the count of all overrides (including expired).
    pub fn override_count(&self) -> usize {
        self.overrides.values().map(|v| v.len()).sum()
    }

    /// Get the count of active (non-expired) overrides.
    pub fn active_override_count(&self) -> usize {
        self.overrides
            .values()
            .map(|v| v.iter().filter(|o| !o.is_expired()).count())
            .sum()
    }

    // -----------------------------------------------------------------------
    // Batch Operations
    // -----------------------------------------------------------------------

    /// Apply a batch of overrides.
    pub fn apply_overrides(&mut self, overrides: Vec<SettingOverride>) {
        for o in overrides {
            self.insert_override(o);
        }
    }

    /// Merge another `RemoteManagedSettings` into this one.
    ///
    /// Overrides from `other` are added alongside existing ones.
    /// Resolution follows normal priority rules.
    pub fn merge(&mut self, other: RemoteManagedSettings) {
        for (_, overrides) in other.overrides {
            for o in overrides {
                self.insert_override(o);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Serialization
    // -----------------------------------------------------------------------

    /// Export all overrides as a JSON string.
    pub fn export(&self) -> Result<String, RemoteSettingsError> {
        let all: Vec<&SettingOverride> = self.overrides.values().flatten().collect();
        serde_json::to_string(&all).map_err(|e| RemoteSettingsError::Serialization(e.to_string()))
    }

    /// Import overrides from a JSON string.
    pub fn import(&mut self, json: &str) -> Result<(), RemoteSettingsError> {
        let overrides: Vec<SettingOverride> = serde_json::from_str(json)
            .map_err(|e| RemoteSettingsError::Serialization(e.to_string()))?;
        self.apply_overrides(overrides);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Internal
    // -----------------------------------------------------------------------

    fn insert_override(&mut self, override_val: SettingOverride) {
        self.overrides
            .entry(override_val.key.clone())
            .or_default()
            .push(override_val);
    }
}

// ============================================================================
// Mock Provider for Testing
// ============================================================================

/// A mock remote settings provider for testing.
#[cfg(test)]
pub struct MockRemoteSettingsProvider {
    pub available: bool,
    pub settings: Vec<SettingOverride>,
    pub fetch_count: std::sync::atomic::AtomicUsize,
    pub apply_count: std::sync::atomic::AtomicUsize,
}

#[cfg(test)]
impl MockRemoteSettingsProvider {
    pub fn new(available: bool, settings: Vec<SettingOverride>) -> Self {
        Self {
            available,
            settings,
            fetch_count: std::sync::atomic::AtomicUsize::new(0),
            apply_count: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

#[cfg(test)]
#[async_trait]
impl RemoteSettingsProvider for MockRemoteSettingsProvider {
    async fn fetch(&self) -> Result<Vec<SettingOverride>, RemoteSettingsError> {
        self.fetch_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if !self.available {
            return Err(RemoteSettingsError::FetchError(
                "Provider unavailable".to_string(),
            ));
        }
        Ok(self.settings.clone())
    }

    async fn apply(&self, _overrides: Vec<SettingOverride>) -> Result<(), RemoteSettingsError> {
        self.apply_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    async fn is_available(&self) -> bool {
        self.available
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_local_setting() {
        let mut settings = RemoteManagedSettings::new();
        settings.set_local("model", "claude-opus-4-6");

        assert_eq!(settings.get("model"), Some("claude-opus-4-6"));
        let (value, source) = settings.get_with_source("model").unwrap();
        assert_eq!(value, "claude-opus-4-6");
        assert_eq!(source, SettingSource::Local);
    }

    #[test]
    fn test_org_overrides_local() {
        let mut settings = RemoteManagedSettings::new();
        settings.set_local("permissions_mode", "ask");
        settings.set_org("permissions_mode", "readonly");

        // Org has higher priority than local
        assert_eq!(settings.get("permissions_mode"), Some("readonly"));
        let (_, source) = settings.get_with_source("permissions_mode").unwrap();
        assert_eq!(source, SettingSource::Org);
    }

    #[test]
    fn test_remote_overrides_org() {
        let mut settings = RemoteManagedSettings::new();
        settings.set_local("theme", "dark");
        settings.set_org("theme", "light");
        settings.set_remote("theme", "auto");

        // Remote has highest priority
        assert_eq!(settings.get("theme"), Some("auto"));
        let (_, source) = settings.get_with_source("theme").unwrap();
        assert_eq!(source, SettingSource::Remote);
    }

    #[test]
    fn test_priority_hierarchy() {
        let mut settings = RemoteManagedSettings::new();

        // Same key, different sources with default priorities
        settings.set_local("key1", "local_val");
        settings.set_org("key1", "org_val");
        settings.set_remote("key1", "remote_val");

        // Remote should win
        assert_eq!(settings.get("key1"), Some("remote_val"));

        // Active overrides should be sorted by priority descending
        let active = settings.active_overrides("key1");
        assert_eq!(active.len(), 3);
        assert_eq!(active[0].source, SettingSource::Remote);
        assert_eq!(active[1].source, SettingSource::Org);
        assert_eq!(active[2].source, SettingSource::Local);
    }

    #[test]
    fn test_custom_priority() {
        let mut settings = RemoteManagedSettings::new();

        // Local with high custom priority should beat org default
        let high_local =
            SettingOverride::new("key", "high_local", SettingSource::Local, 80).unwrap();
        settings.set_override(high_local);
        settings.set_org("key", "org_val");

        assert_eq!(settings.get("key"), Some("high_local"));
    }

    #[test]
    fn test_invalid_priority() {
        let result = SettingOverride::new("key", "val", SettingSource::Local, -1);
        assert!(result.is_err());

        let result = SettingOverride::new("key", "val", SettingSource::Local, 101);
        assert!(result.is_err());

        let result = SettingOverride::new("key", "val", SettingSource::Local, 50);
        assert!(result.is_ok());
    }

    #[test]
    fn test_expiry() {
        let mut settings = RemoteManagedSettings::new();

        // Expired override
        let expired = SettingOverride::with_expiry(
            "model",
            "expired-model",
            SettingSource::Remote,
            90,
            Utc::now() - Duration::seconds(1),
        )
        .unwrap();
        settings.set_override(expired);

        // Valid override
        settings.set_local("model", "local-model");

        // Expired override should be skipped; local wins
        assert_eq!(settings.get("model"), Some("local-model"));
    }

    #[test]
    fn test_cleanup_expired() {
        let mut settings = RemoteManagedSettings::new();

        // Add an expired override
        let expired = SettingOverride::with_expiry(
            "key1",
            "expired",
            SettingSource::Remote,
            90,
            Utc::now() - Duration::seconds(1),
        )
        .unwrap();
        settings.set_override(expired);

        // Add a valid override
        settings.set_local("key2", "valid");

        // Add another expired one
        let expired2 = SettingOverride::with_expiry(
            "key3",
            "also-expired",
            SettingSource::Org,
            50,
            Utc::now() - Duration::minutes(1),
        )
        .unwrap();
        settings.set_override(expired2);

        let removed = settings.cleanup_expired();
        assert_eq!(removed, 2);
        assert_eq!(settings.active_override_count(), 1);
        assert!(!settings.contains("key1"));
        assert!(settings.contains("key2"));
        assert!(!settings.contains("key3"));
    }

    #[test]
    fn test_remove_by_source() {
        let mut settings = RemoteManagedSettings::new();
        settings.set_local("key", "local");
        settings.set_org("key", "org");
        settings.set_remote("key", "remote");

        // Remove just org
        settings.remove_by_source("key", SettingSource::Org);

        let active = settings.active_overrides("key");
        assert_eq!(active.len(), 2);
        assert!(!active.iter().any(|o| o.source == SettingSource::Org));
    }

    #[test]
    fn test_all_settings() {
        let mut settings = RemoteManagedSettings::new();
        settings.set_local("model", "claude-opus-4-6");
        settings.set_org("permissions", "readonly");

        let all = settings.all_settings();
        assert_eq!(all.len(), 2);
        assert_eq!(all.get("model").unwrap().0, "claude-opus-4-6");
        assert_eq!(all.get("permissions").unwrap().1, SettingSource::Org);
    }

    #[test]
    fn test_export_import_roundtrip() {
        let mut settings = RemoteManagedSettings::new();
        settings.set_local("model", "claude-opus-4-6");
        settings.set_org("theme", "dark");

        let exported = settings.export().unwrap();

        let mut imported = RemoteManagedSettings::new();
        imported.import(&exported).unwrap();

        assert_eq!(imported.get("model"), Some("claude-opus-4-6"));
        assert_eq!(imported.get("theme"), Some("dark"));
    }

    #[test]
    fn test_merge() {
        let mut settings_a = RemoteManagedSettings::new();
        settings_a.set_local("model", "claude-opus-4-6");

        let mut settings_b = RemoteManagedSettings::new();
        settings_b.set_org("theme", "dark");

        settings_a.merge(settings_b);

        assert_eq!(settings_a.get("model"), Some("claude-opus-4-6"));
        assert_eq!(settings_a.get("theme"), Some("dark"));
    }

    #[test]
    fn test_setting_override_serialization() {
        let override_val = SettingOverride::with_expiry(
            "model",
            "claude-opus-4-6",
            SettingSource::Remote,
            90,
            Utc::now() + Duration::hours(1),
        )
        .unwrap();

        let json = serde_json::to_string(&override_val).unwrap();
        let deserialized: SettingOverride = serde_json::from_str(&json).unwrap();

        assert_eq!(override_val, deserialized);
    }

    #[tokio::test]
    async fn test_mock_provider() {
        let provider = MockRemoteSettingsProvider::new(
            true,
            vec![SettingOverride::with_defaults(
                "model",
                "claude-opus-4-6",
                SettingSource::Remote,
            )],
        );

        assert!(provider.is_available().await);

        let fetched = provider.fetch().await.unwrap();
        assert_eq!(fetched.len(), 1);
        assert_eq!(fetched[0].key, "model");

        provider.apply(vec![]).await.unwrap();
    }

    #[tokio::test]
    async fn test_mock_provider_unavailable() {
        let provider = MockRemoteSettingsProvider::new(false, vec![]);

        assert!(!provider.is_available().await);
        assert!(provider.fetch().await.is_err());
    }
}
