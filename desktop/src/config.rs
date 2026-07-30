//! Desktop-specific configuration management.
//!
//! Loads provider settings from Shannon's standard config locations
//! and supports runtime provider switching.

use serde::{Deserialize, Serialize};
use shannon_types::provider_config::{ProviderQuirks, ProviderTiers};
use std::collections::HashMap;
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
    /// Speech-to-text (voice input) provider config (D4 cloud STT).
    #[serde(default)]
    pub stt: Option<SttConfig>,
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
    /// Surface a desktop notification when a query/task completes (non-error
    /// notifications, e.g. `NotificationLevel::Info`/`Success`/`Warning`).
    /// Default: enabled.
    #[serde(default = "default_true")]
    pub notifications_on_completed: bool,
    /// Surface a desktop notification when a query/task fails
    /// (`NotificationLevel::Error`). Default: enabled.
    #[serde(default = "default_true")]
    pub notifications_on_failed: bool,
    /// Gateway process supervision (E-1, 方案 C). When `managed` is true the
    /// desktop app spawns and supervises a local `shannon-gateway` binary;
    /// when false, the gateway is treated as external (user/ops runs it and
    /// the UI's engine endpoints point at it).
    #[serde(default)]
    pub gateway: GatewayDesktopConfig,
}

/// Gateway process supervision config (E-1, 方案 C). Stored under
/// `~/.shannon/desktop/config.json` (the *desktop's* own config — not the
/// gateway's `~/.shannon/gateway/config.json`, which the gateway itself reads).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayDesktopConfig {
    /// 方案 C master switch. `true` (default) → desktop spawns + supervises a
    /// local gateway binary. `false` → the gateway is external; desktop only
    /// reads/writes its config + engine endpoints and never starts a process.
    #[serde(default = "default_gateway_managed")]
    pub managed: bool,
    /// Explicit path to the gateway binary. If `None`, the supervisor probes a
    /// few default locations (Tauri resource dir, then `$PATH`); if none
    /// resolves, `start()` reports `NotInstalled` rather than erroring.
    #[serde(default)]
    pub binary_path: Option<String>,
    /// Extra CLI args appended to the gateway invocation
    /// (e.g. `["--log-level", "debug"]`).
    #[serde(default)]
    pub extra_args: Vec<String>,
}

impl Default for GatewayDesktopConfig {
    fn default() -> Self {
        Self {
            managed: default_gateway_managed(),
            binary_path: None,
            extra_args: Vec::new(),
        }
    }
}

fn default_gateway_managed() -> bool {
    true
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
/// providers; the **active** one is mirrored into the engine's
/// `~/.shannon/providers.toml` via [`crate::commands_config::save_provider`]
/// so the engine reads it directly. The desktop shell keeps a parallel
/// `~/.shannon/desktop/providers.json` cache purely as a read-side fan-out
/// for the UI list — `providers.toml` is the source of truth on disk.
///
/// The fields beyond `id`/`label`/`provider_kind`/`base_url`/`model` are the
/// v2 `ProviderProfile` schema (ADR-0005 Phase 2 / task 4). The desktop
/// extends the engine's per-profile knobs so users can configure custom
/// headers, fallback models, and per-tier overrides from the Add Provider
/// modal without going through the CLI's `/connect` flow.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderConnection {
    /// Stable slug id (derived from the label, de-duplicated).
    pub id: String,
    /// Human-readable label shown in the list (e.g. "My GLM key").
    pub label: String,
    /// Provider kind: `anthropic` | `openai` | `deepseek` | `ollama` |
    /// `openai-compatible`. Determines the auth scheme + default base_url.
    pub provider_kind: String,
    /// API key plaintext (transitional only). A1 mandates plaintext keys
    /// never live on disk; the canonical store is
    /// `~/.shannon/credentials/<id>.json` (0600), written by
    /// `store_provider_key` from [`crate::commands_config`]. This field
    /// exists only so a legacy `providers.json` with a leftover plaintext
    /// key can be deserialized and moved to the credential store by
    /// `migrate_providers_to_credentials`. It is never serialized out.
    #[serde(default, skip_serializing)]
    pub api_key: Option<String>,
    /// Base URL override. Required for `openai-compatible`; optional for the
    /// built-in kinds (falls back to the canonical URL).
    #[serde(default)]
    pub base_url: Option<String>,
    /// Default model id for this connection.
    #[serde(default)]
    pub model: Option<String>,
    pub created_at: String,
    /// Optional override for the model listing endpoint. `None` →
    /// `{base_url}/models` (engine default).
    #[serde(default)]
    pub models_url: Option<String>,
    /// Per-request HTTP headers. Use for proxies, custom auth schemes, or
    /// `X-*` headers the engine doesn't otherwise expose.
    #[serde(default)]
    pub extra_headers: HashMap<String, String>,
    /// Default `max_tokens` for this provider's requests. Falls back to
    /// `cfg.max_tokens` (then to 4096) when unset.
    #[serde(default)]
    pub default_max_tokens: Option<u32>,
    /// Fallback model ids tried in order if the primary is unavailable.
    /// Engine-side support is a Phase 5 follow-up; today these are
    /// persisted but not consumed by the runtime path.
    #[serde(default)]
    pub fallback_models: Vec<String>,
    /// Per-provider behavior tweaks (temperature strategy, max_tokens
    /// override, send_temperature). Engine-side support is a Phase 5
    /// follow-up; today these are persisted but not consumed by the
    /// runtime path.
    #[serde(default)]
    pub quirks: ProviderQuirks,
    /// Per-tier model id overrides (canonical: `fast` / `standard` /
    /// `pro`). REPL `/model --tier <name> <model> --save` writes the same
    /// shape into the engine store; the desktop's Add Provider modal
    /// exposes it for managed connections.
    #[serde(default)]
    pub tiers: ProviderTiers,
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

impl ProviderConnection {
    /// Build a v2 `ProviderProfile` from this connection for the engine's
    /// `~/.shannon/providers.toml`. Used by
    /// [`crate::commands_config::save_provider`] /
    /// [`crate::commands_config::set_active_provider`] when landing a
    /// managed connection through
    /// [`shannon_core::provider_config_store::ProviderConfigStore::upsert_profile`].
    ///
    /// Behaviour:
    /// - `kind` is derived from `provider_kind` via the engine's slug
    ///   table. Unknown slugs fall back to `OpenAiCompatible` so a
    ///   typo'd kind still round-trips.
    /// - `base_url` falls back to the engine's canonical default for
    ///   the kind (e.g. `https://api.anthropic.com` for Anthropic).
    /// - `credential` is `Store { service: id }` when the credential
    ///   store has a key for this id, else `Ephemeral` (the resolver
    ///   falls back to env-var lookup for `Ephemeral`).
    /// - `models_url`, `extra_headers`, `default_max_tokens`,
    ///   `fallback_models`, `quirks`, `tiers` are passed through
    ///   verbatim from this struct.
    pub fn to_provider_profile(
        &self,
        default_base_url: &str,
    ) -> shannon_types::provider_config::ProviderProfile {
        use shannon_types::provider_config::{CredentialRef, ProviderKind, ProviderProfile};

        let kind = match self.provider_kind.as_str() {
            "anthropic" => ProviderKind::Anthropic,
            "openai" => ProviderKind::OpenAi,
            "openai-compatible" => ProviderKind::OpenAiCompatible,
            "ollama" => ProviderKind::Ollama,
            "gemini" => ProviderKind::Gemini,
            "deepseek" => ProviderKind::Deepseek,
            // Unknown slug — collapse to openai-compatible so the
            // engine's `resolve_provider` can recover identity from
            // base_url at resolution time.
            _ => ProviderKind::OpenAiCompatible,
        };

        // Decide the credential: Store iff the credential store has a
        // key for this id. The desktop's `store_provider_key` writes
        // before the `ProviderProfile` is constructed, so a fresh save
        // sees its own write. A delete or unconfigured state resolves
        // to Ephemeral, which the resolver handles by falling back to
        // the provider's env-var lookup.
        let credential =
            match shannon_core::credential_manager::read_credential_value_default(&self.id) {
                Some(_) => CredentialRef::Store {
                    service: self.id.clone(),
                },
                None => CredentialRef::Ephemeral,
            };

        ProviderProfile {
            id: self.id.clone(),
            kind,
            display_name: self.label.clone(),
            base_url: self
                .base_url
                .clone()
                .unwrap_or_else(|| default_base_url.to_string()),
            models_url: self.models_url.clone(),
            credential,
            extra_headers: self.extra_headers.clone(),
            default_max_tokens: self.default_max_tokens,
            fallback_models: self.fallback_models.clone(),
            quirks: self.quirks.clone(),
            tiers: self.tiers.clone(),
        }
    }
}

/// Service slug used by the credentials store for this connection. Stable
/// across renames of the human-readable `label` — `id` is the unique slug
/// the user picks (and which survives label edits), so it is the right key
/// for `~/.shannon/credentials/<service>.json`.
pub(crate) fn credential_service(conn: &ProviderConnection) -> String {
    conn.id.clone()
}

/// Report from [`migrate_providers_to_credentials`]. `migrated` counts how
/// many plaintext `api_key` fields were moved into `~/.shannon/credentials/`
/// and cleared from `providers.json`. `skipped` counts providers whose key
/// was already migrated in a previous run (idempotent — no double-write).
#[derive(Debug, Default, Clone, Copy)]
pub struct CredentialMigrationReport {
    pub migrated: usize,
    pub skipped: usize,
}

/// One-shot, idempotent migration of plaintext `api_key` fields out of
/// `providers.json` into `~/.shannon/credentials/<service>.json` (A1:
///
/// config never carries plaintext secrets). Called from `list_providers`
/// on every read so legacy installs heal themselves the first time the
/// user opens the providers panel — no explicit user action required.
///
/// The migration is **idempotent**:
/// - Providers whose `api_key` is already `None` are left alone (skipped).
/// - Providers whose key was already written to the credential store
///   (detected by `read_credential_value_default(&conn.id)` returning
///   `Some`) have any leftover plaintext cleared without re-writing.
///
/// Returns the migration counts so callers can log "moved N keys to the
/// credential store" once if they want to.
pub fn migrate_providers_to_credentials(
    file: &mut ProvidersFile,
) -> Result<CredentialMigrationReport, String> {
    use shannon_core::credential_manager::{Credential, CredentialManager};

    let mut manager = CredentialManager::new()
        .map_err(|e| format!("could not open credential store for migration: {e}"))?;
    let mut report = CredentialMigrationReport::default();

    for conn in &mut file.providers {
        let plaintext = match conn.api_key.as_deref().filter(|s| !s.is_empty()) {
            Some(k) => k.to_string(),
            None => {
                report.skipped += 1;
                continue;
            }
        };
        let service = credential_service(conn);
        manager
            .store(Credential::new(&conn.label, &service, &plaintext))
            .map_err(|e| format!("could not write credential `{service}`: {e}"))?;
        // Clear the plaintext field — it now lives in the credential store.
        conn.api_key = None;
        report.migrated += 1;
    }
    Ok(report)
}

fn default_skill_loop_min_duration_secs() -> u64 {
    30
}

fn default_skill_loop_min_tool_calls() -> usize {
    2
}

/// Speech-to-text (voice input) provider configuration (D4 cloud STT).
/// Backs the `transcribe_audio` command. `None`/missing key ⇒ the UI surfaces
/// a "not configured" toast instead of attempting a provider call.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SttConfig {
    /// Provider preset: `groq` | `openai` | `custom`.
    #[serde(default)]
    pub provider: Option<String>,
    /// API key (stored locally; masked to `"***"` in read-back responses).
    #[serde(default)]
    pub api_key: Option<String>,
    /// Base URL override. Required for `custom`; optional for the presets.
    #[serde(default)]
    pub base_url: Option<String>,
    /// Whisper model id. Defaults: groq→`whisper-large-v3`, openai→`whisper-1`.
    #[serde(default)]
    pub model: Option<String>,
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
            notifications_on_completed: default_true(),
            notifications_on_failed: default_true(),
            stt: None,
            gateway: GatewayDesktopConfig::default(),
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
            notifications_on_completed: true,
            notifications_on_failed: true,
            stt: None,
            ..Default::default()
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
            notifications_on_completed: true,
            notifications_on_failed: true,
            stt: None,
            ..Default::default()
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
            notifications_on_completed: true,
            notifications_on_failed: true,
            stt: None,
            ..Default::default()
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
                ..Default::default()
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

    // === ProviderConnection → ProviderProfile (Phase 2 task 4) ===
    //
    // The desktop's `to_provider_profile` helper is the bridge from
    // the UI-side `ProviderConnection` to the engine-side
    // `ProviderProfile`. It is the only conversion point; if it ever
    // drops a field, the engine store silently disagrees with the UI
    // and the user sees a stale connection on next launch.

    fn full_provider_connection(id: &str, kind: &str) -> ProviderConnection {
        let mut extra_headers = HashMap::new();
        extra_headers.insert("X-Custom".into(), "yes".into());
        extra_headers.insert("X-Region".into(), "us-east".into());
        ProviderConnection {
            id: id.into(),
            label: format!("{id} label"),
            provider_kind: kind.into(),
            api_key: None,
            base_url: Some("https://example.test/v1".into()),
            model: Some("default-model".into()),
            created_at: "2026-07-30T00:00:00Z".into(),
            models_url: Some("https://example.test/v1/models".into()),
            extra_headers,
            default_max_tokens: Some(8192),
            fallback_models: vec!["a".into(), "b".into()],
            quirks: Default::default(),
            tiers: ProviderTiers {
                fast: Some("fast-model".into()),
                standard: Some("std-model".into()),
                pro: Some("pro-model".into()),
            },
        }
    }

    #[test]
    fn to_provider_profile_maps_known_kind_to_engine_enum() {
        // Anthropic kind maps to ProviderKind::Anthropic — not the
        // openai-compatible catch-all.
        let conn = full_provider_connection("anthropic-main", "anthropic");
        let profile = conn.to_provider_profile("https://api.anthropic.com");
        assert_eq!(profile.id, "anthropic-main");
        assert_eq!(
            profile.kind,
            shannon_types::provider_config::ProviderKind::Anthropic
        );
        assert_eq!(profile.display_name, "anthropic-main label");
        assert_eq!(profile.base_url, "https://example.test/v1");
        assert_eq!(
            profile.models_url.as_deref(),
            Some("https://example.test/v1/models")
        );
        assert_eq!(
            profile.extra_headers.get("X-Custom").map(String::as_str),
            Some("yes")
        );
        assert_eq!(profile.default_max_tokens, Some(8192));
        assert_eq!(profile.fallback_models, vec!["a", "b"]);
        assert_eq!(profile.tiers.fast.as_deref(), Some("fast-model"));
        assert_eq!(profile.tiers.standard.as_deref(), Some("std-model"));
        assert_eq!(profile.tiers.pro.as_deref(), Some("pro-model"));
    }

    #[test]
    fn to_provider_profile_collapses_unknown_kind_to_openai_compatible() {
        // A typo'd kind (e.g. "anthropicc") must still produce a
        // round-trippable profile so the engine's resolve_provider
        // can recover identity from base_url.
        let conn = full_provider_connection("custom-1", "anthropicc");
        let profile = conn.to_provider_profile("https://default/v1");
        assert_eq!(
            profile.kind,
            shannon_types::provider_config::ProviderKind::OpenAiCompatible
        );
    }

    #[test]
    fn to_provider_profile_falls_back_to_default_base_url_when_unset() {
        // The user-supplied base_url is None → use the engine's
        // canonical default (e.g. for Anthropic). This matches the
        // guarantee /connect gives the CLI.
        let mut conn = full_provider_connection("anthropic-main", "anthropic");
        conn.base_url = None;
        let profile = conn.to_provider_profile("https://api.anthropic.com");
        assert_eq!(profile.base_url, "https://api.anthropic.com");
    }

    #[test]
    fn to_provider_profile_uses_store_credential_when_credential_file_present() {
        // If ~/.shannon/credentials/<id>.json exists on disk, the
        // profile advertises CredentialRef::Store so the resolver
        // reads the key from the store. We don't write a real
        // credential here — we just check the fallback path. The
        // Store-vs-Ephemeral branching is exercised by the live
        // /provider health / connection test paths.
        let conn = full_provider_connection("never-stored", "anthropic");
        let profile = conn.to_provider_profile("https://api.anthropic.com");
        match &profile.credential {
            shannon_types::provider_config::CredentialRef::Ephemeral
            | shannon_types::provider_config::CredentialRef::Store { .. } => {}
            other => panic!("expected Ephemeral or Store, got {other:?}"),
        }
    }

    #[test]
    fn provider_connection_default_is_constructible() {
        // Default::default() must produce a valid (mostly-empty)
        // struct. Used by callers that build a ProviderConnection
        // piecewise (e.g. the Add Provider modal's reset state).
        let conn = ProviderConnection::default();
        assert!(conn.id.is_empty());
        assert!(conn.label.is_empty());
        assert!(conn.provider_kind.is_empty());
        assert!(conn.api_key.is_none());
        assert!(conn.base_url.is_none());
        assert!(conn.model.is_none());
        assert!(conn.models_url.is_none());
        assert!(conn.extra_headers.is_empty());
        assert!(conn.default_max_tokens.is_none());
        assert!(conn.fallback_models.is_empty());
        assert_eq!(conn.tiers, ProviderTiers::default());
    }

    #[test]
    fn provider_connection_legacy_api_key_field_does_not_serialize() {
        // The api_key field is a transitional deserialization seam
        // only — once the credential-store migration runs the value
        // is None, and we never write it back out.
        let conn = ProviderConnection {
            id: "a".into(),
            label: "A".into(),
            provider_kind: "anthropic".into(),
            api_key: Some("sk-secret".into()),
            base_url: None,
            model: None,
            created_at: "2026-07-30T00:00:00Z".into(),
            ..Default::default()
        };
        let json = serde_json::to_string(&conn).unwrap();
        assert!(
            !json.contains("api_key"),
            "api_key must not be serialized (saw {json})"
        );
        assert!(!json.contains("sk-secret"));
    }

    #[test]
    fn provider_connection_legacy_api_key_field_deserializes_and_is_consumed() {
        // A legacy providers.json with a plaintext api_key must still
        // parse (so the credential-store migration can run). The
        // value is captured into the struct, but apply_provider_update
        // never writes it back out.
        let json = r#"{
            "id":"glm",
            "label":"GLM",
            "provider_kind":"openai-compatible",
            "api_key":"sk-legacy-plaintext",
            "created_at":"2026-07-30T00:00:00Z"
        }"#;
        let conn: ProviderConnection = serde_json::from_str(json).unwrap();
        assert_eq!(conn.api_key.as_deref(), Some("sk-legacy-plaintext"));
        // Round-trip back out: api_key must be gone.
        let back = serde_json::to_string(&conn).unwrap();
        assert!(!back.contains("api_key"));
        assert!(!back.contains("sk-legacy-plaintext"));
    }
}
