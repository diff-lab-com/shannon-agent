//! Desktop-specific configuration management.
//!
//! Loads provider settings from Shannon's standard config locations
//! and supports runtime provider switching.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Desktop app configuration persisted across sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopConfig {
    pub provider: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub working_dir: Option<String>,
    pub theme: Option<String>,
    pub mcp_servers: Vec<McpServerConfig>,
    pub approval_mode: Option<String>,
    /// OPC strategic focus statement.
    pub strategic_focus: Option<String>,
    /// Model selection strategy: `speed` | `balanced` | `high-quality`.
    pub performance_strategy: Option<String>,
    /// Long-term memory toggle.
    pub memory_enabled: Option<bool>,
    /// Anonymous usage telemetry toggle.
    pub telemetry_enabled: Option<bool>,
    /// Local data encryption toggle.
    pub encryption_enabled: Option<bool>,
    /// Debug console toggle.
    pub debug_console: Option<bool>,
    /// Default sampling temperature.
    pub temperature: Option<f32>,
    /// Default max tokens for generation.
    pub max_tokens: Option<u32>,
    /// Billing plan name (local-app echo of provider plan).
    pub plan: Option<String>,
    /// Skill loop evaluation enabled (default: false).
    #[serde(default)]
    pub skill_loop_enabled: bool,
    /// Minimum task duration (seconds) to trigger skill evaluation.
    #[serde(default = "default_skill_loop_min_duration_secs")]
    pub skill_loop_min_duration_secs: u64,
    /// Minimum tool call count to trigger skill evaluation.
    #[serde(default = "default_skill_loop_min_tool_calls")]
    pub skill_loop_min_tool_calls: usize,
    /// Enable the recurring-pattern skill-candidate detector (D6 Phase 1).
    /// When false, trigger_skill_pattern_detection returns 0 without
    /// scanning sessions. Default: true.
    #[serde(default = "default_skill_detection_enabled")]
    pub skill_detection_enabled: bool,
    /// Master switch for desktop (OS) notifications. When false, the
    /// `TauriNotificationHandler` silently drops every notification.
    /// Default: enabled (existing users keep notifications on upgrade).
    #[serde(default = "default_true")]
    pub notifications_master_enabled: bool,
    /// Do-Not-Disturb / quiet-hours switch. When true, desktop notifications
    /// are suppressed while the current local time is inside the window
    /// [`notifications_dnd_start`, `notifications_dnd_end`). Webhook delivery
    /// is unaffected.
    #[serde(default)]
    pub notifications_dnd_enabled: bool,
    /// DND window start, `"HH:MM"` (24h, system-local). Parsed leniently.
    #[serde(default)]
    pub notifications_dnd_start: Option<String>,
    /// DND window end, `"HH:MM"` (24h, system-local).
    #[serde(default)]
    pub notifications_dnd_end: Option<String>,
}

fn default_skill_detection_enabled() -> bool {
    true
}

fn default_true() -> bool {
    true
}

/// MCP server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: std::collections::HashMap<String, String>,
    pub enabled: bool,
}

/// A managed LLM provider connection (Models P2). Users may configure several
/// providers; the **active** one is mirrored into `DesktopConfig`'s singular
/// fields, which is what the engine actually reads. The full roster lives in
/// `~/.shannon/desktop/providers.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConnection {
    /// Stable slug id (derived from the label, de-duplicated).
    pub id: String,
    /// Human-readable label shown in the list (e.g. "My GLM key").
    pub label: String,
    /// Provider kind: `anthropic` | `openai` | `deepseek` | `ollama` |
    /// `openai-compatible`. Determines the auth scheme + default base_url.
    pub provider_kind: String,
    /// API key (stored locally; masked to `"***"` in list responses).
    #[serde(default)]
    pub api_key: Option<String>,
    /// Base URL override. Required for `openai-compatible`; optional for the
    /// built-in kinds (falls back to the canonical URL).
    #[serde(default)]
    pub base_url: Option<String>,
    /// Default model id for this connection.
    #[serde(default)]
    pub model: Option<String>,
    pub created_at: String,
}

/// Container persisted to `~/.shannon/desktop/providers.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProvidersFile {
    /// Id of the provider whose fields are mirrored into `DesktopConfig`.
    #[serde(default)]
    pub active_provider_id: Option<String>,
    #[serde(default)]
    pub providers: Vec<ProviderConnection>,
}

fn default_skill_loop_min_duration_secs() -> u64 {
    30
}

fn default_skill_loop_min_tool_calls() -> usize {
    2
}

impl Default for DesktopConfig {
    fn default() -> Self {
        Self {
            provider: Some("anthropic".into()),
            api_key: None,
            base_url: None,
            model: Some("claude-sonnet-4-6".into()),
            working_dir: None,
            theme: None,
            mcp_servers: Vec::new(),
            approval_mode: Some("confirm".into()),
            strategic_focus: None,
            performance_strategy: None,
            memory_enabled: None,
            telemetry_enabled: None,
            encryption_enabled: None,
            debug_console: None,
            temperature: None,
            max_tokens: None,
            plan: None,
            skill_loop_enabled: false,
            skill_loop_min_duration_secs: default_skill_loop_min_duration_secs(),
            skill_loop_min_tool_calls: default_skill_loop_min_tool_calls(),
            skill_detection_enabled: default_skill_detection_enabled(),
            notifications_master_enabled: default_true(),
            notifications_dnd_enabled: false,
            notifications_dnd_start: None,
            notifications_dnd_end: None,
        }
    }
}

/// Resolve the config file path: `~/.shannon/desktop/config.json`
fn config_path() -> PathBuf {
    let home = dirs_home().unwrap_or_else(|| PathBuf::from("."));
    home.join(".shannon").join("desktop").join("config.json")
}

/// Resolve the MCP servers config file path: `~/.shannon/desktop/mcp-servers.json`
fn mcp_servers_path() -> PathBuf {
    let home = dirs_home().unwrap_or_else(|| PathBuf::from("."));
    home.join(".shannon")
        .join("desktop")
        .join("mcp-servers.json")
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
}

/// Load desktop config from disk, returning default if not found.
pub fn load_config() -> DesktopConfig {
    let path = config_path();
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => DesktopConfig::default(),
    }
}

/// Save desktop config to disk.
pub fn save_config(config: &DesktopConfig) -> Result<(), String> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    std::fs::write(&path, content).map_err(|e| e.to_string())?;
    crate::file_permissions::restrict_to_owner(&path);
    Ok(())
}

/// Load MCP server configs from disk.
pub fn load_mcp_servers() -> Vec<McpServerConfig> {
    let path = mcp_servers_path();
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// Save MCP server configs to disk.
pub fn save_mcp_servers(servers: &[McpServerConfig]) -> Result<(), String> {
    let path = mcp_servers_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content = serde_json::to_string_pretty(servers).map_err(|e| e.to_string())?;
    std::fs::write(&path, content).map_err(|e| e.to_string())?;
    crate::file_permissions::restrict_to_owner(&path);
    Ok(())
}

/// Resolve the managed-providers file path: `~/.shannon/desktop/providers.json`
pub fn providers_path() -> PathBuf {
    let home = dirs_home().unwrap_or_else(|| PathBuf::from("."));
    home.join(".shannon").join("desktop").join("providers.json")
}

/// Load managed providers from disk, returning an empty file if not found.
pub fn load_providers() -> ProvidersFile {
    let path = providers_path();
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => ProvidersFile::default(),
    }
}

/// Save managed providers to disk.
pub fn save_providers(file: &ProvidersFile) -> Result<(), String> {
    let path = providers_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content = serde_json::to_string_pretty(file).map_err(|e| e.to_string())?;
    std::fs::write(&path, content).map_err(|e| e.to_string())?;
    crate::file_permissions::restrict_to_owner(&path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = DesktopConfig::default();
        assert_eq!(config.provider, Some("anthropic".into()));
        assert!(config.api_key.is_none());
        assert_eq!(config.model, Some("claude-sonnet-4-6".into()));
        assert!(config.working_dir.is_none());
        assert!(config.theme.is_none());
        assert_eq!(config.approval_mode, Some("confirm".into()));
    }

    #[test]
    fn test_config_serialization_roundtrip() {
        let config = DesktopConfig {
            provider: Some("openai".into()),
            api_key: Some("sk-test".into()),
            base_url: Some("https://api.openai.com".into()),
            model: Some("gpt-4.1".into()),
            working_dir: None,
            theme: None,
            mcp_servers: vec![],
            approval_mode: None,
            strategic_focus: None,
            performance_strategy: None,
            memory_enabled: None,
            telemetry_enabled: None,
            encryption_enabled: None,
            debug_console: None,
            temperature: None,
            max_tokens: None,
            plan: None,
            skill_loop_enabled: false,
            skill_loop_min_duration_secs: 30,
            skill_loop_min_tool_calls: 2,
            skill_detection_enabled: true,
            notifications_master_enabled: true,
            notifications_dnd_enabled: false,
            notifications_dnd_start: None,
            notifications_dnd_end: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: DesktopConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.provider, Some("openai".into()));
        assert_eq!(parsed.api_key, Some("sk-test".into()));
        assert_eq!(parsed.model, Some("gpt-4.1".into()));
    }

    #[test]
    fn test_skill_loop_config_defaults() {
        let config = DesktopConfig::default();
        assert!(!config.skill_loop_enabled);
        assert_eq!(config.skill_loop_min_duration_secs, 30);
        assert_eq!(config.skill_loop_min_tool_calls, 2);
    }

    #[test]
    fn test_config_path_is_under_shannon_dir() {
        let path = config_path();
        assert!(path.to_string_lossy().contains(".shannon"));
        assert!(path.to_string_lossy().contains("desktop"));
        assert!(path.to_string_lossy().contains("config.json"));
    }

    #[test]
    fn test_approval_mode_serialization() {
        let config = DesktopConfig {
            provider: Some("anthropic".into()),
            api_key: None,
            base_url: None,
            model: Some("claude-sonnet-4-6".into()),
            working_dir: None,
            theme: None,
            mcp_servers: vec![],
            approval_mode: Some("auto".into()),
            strategic_focus: None,
            performance_strategy: None,
            memory_enabled: None,
            telemetry_enabled: None,
            encryption_enabled: None,
            debug_console: None,
            temperature: None,
            max_tokens: None,
            plan: None,
            skill_loop_enabled: false,
            skill_loop_min_duration_secs: 30,
            skill_loop_min_tool_calls: 2,
            skill_detection_enabled: true,
            notifications_master_enabled: true,
            notifications_dnd_enabled: false,
            notifications_dnd_start: None,
            notifications_dnd_end: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: DesktopConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.approval_mode, Some("auto".into()));
    }

    #[test]
    fn test_approval_mode_persistence() {
        let config = DesktopConfig {
            provider: Some("anthropic".into()),
            api_key: None,
            base_url: None,
            model: Some("claude-sonnet-4-6".into()),
            working_dir: None,
            theme: None,
            mcp_servers: vec![],
            approval_mode: Some("full_auto".into()),
            strategic_focus: None,
            performance_strategy: None,
            memory_enabled: None,
            telemetry_enabled: None,
            encryption_enabled: None,
            debug_console: None,
            temperature: None,
            max_tokens: None,
            plan: None,
            skill_loop_enabled: false,
            skill_loop_min_duration_secs: 30,
            skill_loop_min_tool_calls: 2,
            skill_detection_enabled: true,
            notifications_master_enabled: true,
            notifications_dnd_enabled: false,
            notifications_dnd_start: None,
            notifications_dnd_end: None,
        };

        // Test serialization preserves approval_mode
        let json = serde_json::to_string_pretty(&config).unwrap();
        assert!(json.contains("approval_mode"));
        assert!(json.contains("full_auto"));

        // Test deserialization
        let parsed: DesktopConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.approval_mode, Some("full_auto".into()));
    }

    #[test]
    fn providers_file_round_trip() {
        let file = ProvidersFile {
            active_provider_id: Some("glm".into()),
            providers: vec![ProviderConnection {
                id: "glm".into(),
                label: "My GLM".into(),
                provider_kind: "openai-compatible".into(),
                api_key: Some("sk-x".into()),
                base_url: Some("https://open.bigmodel.cn/api/paas/v4".into()),
                model: Some("glm-4.6".into()),
                created_at: "2026-06-27T00:00:00Z".into(),
            }],
        };
        let json = serde_json::to_string(&file).unwrap();
        let back: ProvidersFile = serde_json::from_str(&json).unwrap();
        assert_eq!(back.active_provider_id, Some("glm".into()));
        assert_eq!(back.providers.len(), 1);
        assert_eq!(back.providers[0].provider_kind, "openai-compatible");
        assert_eq!(
            back.providers[0].base_url.as_deref(),
            Some("https://open.bigmodel.cn/api/paas/v4")
        );
    }

    #[test]
    fn providers_file_defaults_empty() {
        let file = ProvidersFile::default();
        assert!(file.active_provider_id.is_none());
        assert!(file.providers.is_empty());
    }

    #[test]
    fn provider_connection_without_optional_fields_deserializes() {
        // api_key/base_url/model are all #[serde(default)]-Optional — a legacy
        // or hand-written entry omitting them must still parse.
        let json = r#"{
            "id":"anthropic",
            "label":"Anthropic",
            "provider_kind":"anthropic",
            "created_at":"2026-06-27T00:00:00Z"
        }"#;
        let conn: ProviderConnection = serde_json::from_str(json).unwrap();
        assert_eq!(conn.id, "anthropic");
        assert!(conn.api_key.is_none());
        assert!(conn.base_url.is_none());
        assert!(conn.model.is_none());
    }

    #[test]
    fn test_providers_path_is_under_shannon_dir() {
        let path = providers_path();
        assert!(path.to_string_lossy().contains(".shannon"));
        assert!(path.to_string_lossy().contains("desktop"));
        assert!(path.to_string_lossy().contains("providers.json"));
    }
}
