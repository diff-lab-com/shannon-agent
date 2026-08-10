//! Desktop-specific configuration management.
//!
//! Loads provider settings from Shannon's standard config locations
//! and supports runtime provider switching.

use serde::{Deserialize, Serialize};
use shannon_types::provider_config::{ProviderQuirks, ProviderTiers};
use std::collections::HashMap;
use std::path::PathBuf;

/// Desktop app configuration persisted across sessions.
///
/// P1.2-B (ADR-0005): the legacy singular `provider` / `api_key` /
/// `base_url` / `model` fields were removed — the engine
/// `ProviderConfigStore` is now the single source of truth for those
/// values (see `crate::commands::AppState::provider_store` and
/// `crate::commands::AppState::build_client_config`). Persisted
/// `desktop/config.json` from older installs may still carry them, but
/// they are silently ignored on load (no field to deserialize into) and
/// never written back out.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopConfig {
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
    /// Local-only STT config (P2-5e whisper-rs). Independent of the cloud
    /// `stt` config so the user can keep a cloud key for fallback while
    /// the local provider is the primary. `enabled = false` is the
    /// default — cloud is still the path most users land on first.
    #[serde(default)]
    pub voice_local: VoiceLocalConfig,
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
    /// Provider allowlist — restricts the model catalog to the listed kinds
    /// (`anthropic` / `openai` / `ollama` / `gemini` / `deepseek` /
    /// `openai-compatible`). Drives the desktop Settings' "Provider
    /// visibility" panel (ADR-0005 P4.9). Semantics:
    ///
    /// - `None` (default) — no desktop override; the engine's
    ///   `SHANNON_ENABLED_PROVIDERS` / `SHANNON_DISABLED_PROVIDERS` env vars
    ///   decide. If neither is set, every provider is visible.
    /// - `Some(vec![])` — user toggled every provider off in the desktop UI;
    ///   the picker shows nothing regardless of env-var state.
    /// - `Some(non_empty)` — user-set allowlist; beats the env vars
    ///   (`SHANNON_*_PROVIDERS`) so a stale shell export can't clobber the
    ///   persisted choice.
    ///
    /// New field — defaults to `None` (legacy "use engine env vars") for
    /// backward compatibility.
    #[serde(default)]
    pub enabled_providers: Option<Vec<String>>,
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
/// **TD-4** (ADR-0009 Phase 2 / tech-debt TD-4): this wire type now
/// faithfully mirrors the engine `ProviderProfile` (+ a derived
/// `has_api_key`, − the backend-only `credential` field). It is an
/// internal desktop type — not on any `shannon-*` public stable surface.
/// See `docs/plans/td-4-retire-provider-connection.md`.
///
/// The fields beyond `id`/`display_name`/`kind`/`base_url` are the v2
/// `ProviderProfile` schema (ADR-0005 Phase 2 / task 4). The desktop
/// extends the engine's per-profile knobs so users can configure custom
/// headers, fallback models, and per-tier overrides from the Add Provider
/// modal without going through the CLI's `/connect` flow.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderConnection {
    /// Stable slug id (derived from the display name, de-duplicated).
    pub id: String,
    /// Human-readable display name shown in the list (e.g. "My GLM key").
    pub display_name: String,
    /// Provider kind slug: `anthropic` | `openai` | `deepseek` | `ollama` |
    /// `openai-compatible`. Determines the auth scheme + default base_url.
    pub kind: String,
    /// True when the credential store has a key for this id. Replaces the
    /// dead `api_key: Option<String>` (which was always `None` +
    /// `skip_serializing`, so consumers never saw it on the wire). Derived
    /// from `credential_manager::read_credential_value_default(id)`.
    #[serde(default)]
    pub has_api_key: bool,
    /// Base URL override. Required for `openai-compatible`; optional for the
    /// built-in kinds (falls back to the canonical URL).
    #[serde(default)]
    pub base_url: Option<String>,
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

/// Legacy on-disk shape of historical `~/.shannon/desktop/providers.json`
/// (pre-TD-4). Used ONLY by the one-shot `migrate_providers_to_toml`. The
/// live wire type is `ProviderConnection`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct LegacyProviderConnection {
    pub id: String,
    pub label: String,
    pub provider_kind: String,
    // KEEP: read by `migrate_providers_to_credentials` when migrating
    // legacy providers.json files that carry plaintext keys. The field
    // is deserialized but clippy flags it as dead because the migration
    // function itself has no callers in the current codebase.
    #[serde(default, skip_serializing)]
    #[allow(dead_code)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub models_url: Option<String>,
    #[serde(default)]
    pub extra_headers: HashMap<String, String>,
    #[serde(default)]
    pub default_max_tokens: Option<u32>,
    #[serde(default)]
    pub fallback_models: Vec<String>,
    #[serde(default)]
    pub quirks: ProviderQuirks,
    #[serde(default)]
    pub tiers: ProviderTiers,
}

/// Legacy on-disk shape of historical `~/.shannon/desktop/providers.json`
/// (pre-TD-4). Used ONLY by the one-shot `migrate_providers_to_toml`. The
/// live wire type is `ProvidersFile`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct LegacyProvidersFile {
    #[serde(default)]
    pub active_provider_id: Option<String>,
    #[serde(default)]
    pub providers: Vec<LegacyProviderConnection>,
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

        let kind = match self.kind.as_str() {
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
            display_name: self.display_name.clone(),
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

/// Build a [`ProviderConnection`] from a v2 engine [`shannon_types::provider_config::ProviderProfile`]
/// for the UI side. Reverse of [`ProviderConnection::to_provider_profile`].
///
/// This is the read-side companion to the engine-write path that
/// `ProviderConfigStore::upsert_profile` populates (see also
/// `crate::commands_config::list_providers` — ADR-0005 Phase 2 task 5):
/// the engine store is the source of truth, and the UI list is just a
/// fan-out of `models/profiles["default"].providers`.
///
/// Mapping notes:
/// - `id` is the profile's `id` (the desktop slug), not `display_name`.
/// - `display_name` falls back to `id` when the engine-side profile has an
///   empty `display_name` (defense-in-depth — engine profiles are
///   expected to always carry a non-empty display name).
/// - `kind` is the UI's slug string via [`kind_engine_to_slug`]
///   (reverse of [`kind_slug_to_engine`]).
/// - `has_api_key` is derived from the credential store — true when
///   `credential_manager::read_credential_value_default(id)` returns a
///   value. This replaces the dead `api_key: Option<String>` field (which
///   was always `None` + `skip_serializing`, so consumers never saw it).
pub(crate) fn from_provider_profile(
    id: &str,
    p: &shannon_types::provider_config::ProviderProfile,
) -> ProviderConnection {
    let display_name = if p.display_name.is_empty() {
        id.to_string()
    } else {
        p.display_name.clone()
    };
    let has_api_key = shannon_core::credential_manager::read_credential_value_default(id).is_some();

    ProviderConnection {
        id: id.to_string(),
        display_name,
        kind: kind_engine_to_slug(&p.kind).to_string(),
        has_api_key,
        base_url: Some(p.base_url.clone()),
        models_url: p.models_url.clone(),
        extra_headers: p.extra_headers.clone(),
        default_max_tokens: p.default_max_tokens,
        fallback_models: p.fallback_models.clone(),
        quirks: p.quirks.clone(),
        tiers: p.tiers.clone(),
    }
}

/// Map the engine's `ProviderKind` enum back to the desktop's wire slug.
/// Inverse of [`kind_slug_to_engine`]. Round-trips for every kind the
/// engine knows about today; an unknown arm — `ProviderKind` is
/// `non_exhaustive` so the enum may grow — falls back to the
/// `openai-compatible` slug, which matches the existing collapse
/// convention (engine resolvers recover fine-grained identity from
/// `base_url` at resolution time).
fn kind_engine_to_slug(kind: &shannon_types::provider_config::ProviderKind) -> &'static str {
    use shannon_types::provider_config::ProviderKind;
    match kind {
        ProviderKind::Anthropic => "anthropic",
        ProviderKind::OpenAi => "openai",
        ProviderKind::OpenAiCompatible => "openai-compatible",
        ProviderKind::Ollama => "ollama",
        ProviderKind::Gemini => "gemini",
        ProviderKind::Deepseek => "deepseek",
        // non_exhaustive: future kinds collapse to the
        // user-supplied-URL catch-all so the wire stays
        // forward-compatible.
        _ => "openai-compatible",
    }
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

/// Local-only STT config (P2-5e). Drives the `transcribe_audio_local` Tauri
/// command and the Settings → Voice local-provider card. The local provider
/// is opt-in and lives behind the `voice-local` Cargo feature at compile
/// time; this struct is always present in the config so the Settings UI
/// can render the card (disabled) on builds that don't have whisper-rs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceLocalConfig {
    /// Master switch. When `true` and the user picks `local` in
    /// `useVoice`, recordings go through `transcribe_audio_local`
    /// instead of the cloud command. Default: `false`.
    #[serde(default)]
    pub enabled: bool,
    /// Model slug. One of `tiny.en` | `base` | `small`. `None` means
    /// "use the smallest available downloaded model" — the command
    /// picks at call time so adding a downloaded model automatically
    /// upgrades the active model.
    #[serde(default)]
    pub model: Option<String>,
    /// BCP-47 language hint passed to whisper-rs (`en`, `zh`, `auto`,
    /// etc.). `None` ⇒ auto-detect.
    #[serde(default)]
    pub language: Option<String>,
    /// When `true` (default), a missing model is auto-downloaded on
    /// first use. When `false`, the command returns
    /// `STT_MODEL_NOT_FOUND` and the UI prompts the user to download
    /// from Settings → Voice.
    #[serde(default = "default_true")]
    pub auto_download: bool,
}

impl Default for VoiceLocalConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: None,
            language: None,
            auto_download: true,
        }
    }
}

impl Default for DesktopConfig {
    fn default() -> Self {
        Self {
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
            voice_local: VoiceLocalConfig::default(),
            gateway: GatewayDesktopConfig::default(),
            enabled_providers: None,
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

/// Load the historical (pre-TD-4) `~/.shannon/desktop/providers.json` from
/// disk, returning an empty file if not found. Used only by the one-shot
/// `migrate_providers_to_toml`. The live read path is
/// `ProviderReadSnapshot::to_providers_file`.
pub(crate) fn load_legacy_providers() -> LegacyProvidersFile {
    let path = providers_path();
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => LegacyProvidersFile::default(),
    }
}

/// One-shot migration: lift entries from
/// `~/.shannon/desktop/providers.json` into the engine's
/// `~/.shannon/providers.toml` (built as a `ProviderModelConfig`) and
/// remove the legacy file.
///
/// Idempotent:
/// - Returns `None` if `providers.json` doesn't exist OR has zero
///   providers (nothing to migrate). The file is left in place
///   when there's nothing to do — no false-positive deletion.
/// - Returns `None` if the engine store already has providers
///   loaded from disk. The legacy file is still removed (it's
///   stale — the engine store is the source of truth and the JSON
///   is just a UI cache that the engine-write path will rebuild).
/// - Skips entries with no resolvable base_url (logs a warn;
///   doesn't bail the whole migration on one bad row).
///
/// Called from `AppState::new()` on first launch after this code
/// lands. Legacy installs heal themselves. After C2, no desktop command
/// writes `providers.json` — `list_providers` reads the engine store
/// exclusively, and the write commands (`save_provider` /
/// `delete_provider` / `set_active_provider`) R-M-W against it directly.
/// This migration is the only code that ever reads the legacy file; it
/// lifts then deletes it, and nothing recreates it.
pub fn migrate_providers_to_toml() -> Option<shannon_types::provider_config::ProviderModelConfig> {
    let path = providers_path();
    if !path.exists() {
        return None;
    }
    let file = load_legacy_providers();
    if file.providers.is_empty() {
        return None;
    }

    // Build the v2 model from the legacy entries. The engine
    // store's idempotence (it loads existing data on startup, so
    // repeated runs are safe) is what protects us from
    // re-applying a partial migration: the AppState startup calls
    // this once, then the in-memory store owns subsequent writes.
    let pm = build_migrated_provider_model(&file);

    // Always remove the legacy file when it has data — the engine
    // store is the source of truth and a stale JSON cache can
    // only confuse the user (e.g. show a "broken" connection
    // that the engine store has already removed).
    if let Err(e) = std::fs::remove_file(&path) {
        tracing::warn!(path = %path.display(), error = %e, "could not remove legacy providers.json after migration");
    }

    pm
}

/// Pure helper that converts a `ProvidersFile` into a
/// `ProviderModelConfig` for the engine store. Extracted from
/// [`migrate_providers_to_toml`] so the migration's data transformation
/// can be unit-tested in isolation (the surrounding function reads
/// from the user's home dir, which tests can't mock without
/// process-wide env mutation).
pub(crate) fn build_migrated_provider_model(
    file: &LegacyProvidersFile,
) -> Option<shannon_types::provider_config::ProviderModelConfig> {
    use shannon_types::provider_config::{
        ActiveTarget, CredentialRef, CredentialScope, ModelProfile, ProviderModelConfig,
        ProviderProfile, Scope,
    };
    use std::collections::HashMap;

    let providers: Vec<ProviderProfile> = file
        .providers
        .iter()
        .filter_map(|conn| {
            let default_url = default_base_url_for_kind(&conn.provider_kind);
            let base_url = conn
                .base_url
                .clone()
                .or_else(|| default_url.map(String::from))
                .unwrap_or_default();
            if base_url.is_empty() {
                tracing::warn!(
                    provider_id = %conn.id,
                    kind = %conn.provider_kind,
                    "skipping migration: empty base_url"
                );
                return None;
            }
            // Decide credential: prefer Store iff the credential
            // store has a key for this id, else Ephemeral.
            let credential =
                match shannon_core::credential_manager::read_credential_value_default(&conn.id) {
                    Some(_) => CredentialRef::Store {
                        service: conn.id.clone(),
                    },
                    None => CredentialRef::Ephemeral,
                };
            Some(ProviderProfile {
                id: conn.id.clone(),
                kind: kind_slug_to_engine(&conn.provider_kind),
                display_name: conn.label.clone(),
                base_url,
                models_url: conn.models_url.clone(),
                credential,
                extra_headers: conn.extra_headers.clone(),
                default_max_tokens: conn.default_max_tokens,
                fallback_models: conn.fallback_models.clone(),
                quirks: conn.quirks.clone(),
                tiers: conn.tiers.clone(),
            })
        })
        .collect();

    if providers.is_empty() {
        return None;
    }

    // Build the ModelProfile, with active_target pointing at the
    // active_provider_id (or the first provider as a fallback).
    let active_id = file
        .active_provider_id
        .clone()
        .or_else(|| providers.first().map(|p| p.id.clone()));
    let active_model_id = active_id
        .as_ref()
        .and_then(|aid| file.providers.iter().find(|c| &c.id == aid))
        .and_then(|c| c.model.clone())
        .unwrap_or_else(|| "default".into());

    let active_target = ActiveTarget {
        provider_id: active_id.unwrap_or_default(),
        model_id: active_model_id,
        scope: Scope::Global,
    };

    let model_profile = ModelProfile {
        name: "default".to_string(),
        active_target,
        providers,
        auxiliary: HashMap::new(),
        credential_scope: CredentialScope::Shared,
    };

    let mut pm = ProviderModelConfig::default();
    pm.profiles.insert("default".to_string(), model_profile);
    Some(pm)
}

/// Map a desktop kind slug to the engine's `ProviderKind`. Unknown
/// slugs collapse to `OpenAiCompatible` (same convention as
/// `ProviderConnection::to_provider_profile`).
fn kind_slug_to_engine(slug: &str) -> shannon_types::provider_config::ProviderKind {
    use shannon_types::provider_config::ProviderKind;
    match slug {
        "anthropic" => ProviderKind::Anthropic,
        "openai" => ProviderKind::OpenAi,
        "openai-compatible" => ProviderKind::OpenAiCompatible,
        "ollama" => ProviderKind::Ollama,
        "gemini" => ProviderKind::Gemini,
        "deepseek" => ProviderKind::Deepseek,
        _ => ProviderKind::OpenAiCompatible,
    }
}

/// Default base URL for a kind slug (used when the legacy
/// `providers.json` entry didn't have one and the kind has a
/// canonical default). Mirrors
/// `commands_config::default_base_url_for_kind` so a migration
/// without the user's base_url still lands a usable profile.
fn default_base_url_for_kind(kind: &str) -> Option<&'static str> {
    if kind == "openai-compatible" {
        return None;
    }
    match kind {
        "anthropic" => Some("https://api.anthropic.com"),
        "openai" => Some("https://api.openai.com"),
        "ollama" => Some("http://localhost:11434"),
        "gemini" => Some("https://generativelanguage.googleapis.com"),
        "deepseek" => Some("https://api.deepseek.com"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RAII guard that points `HOME` at an isolated tempdir for the duration
    /// of a test. Every desktop + engine config path (`~/.shannon/...`) then
    /// resolves under the tempdir, so disk-touching tests never mutate the
    /// real user home and don't race on a shared `providers.json`.
    ///
    /// `HOME` is process-global, so a static mutex serializes all
    /// `IsolatedHome` instances — the same lesson the extensions installers
    /// learned after a lock-guarded env override raced with unrelated
    /// `dirs::home_dir()` readers (see `extensions/agent_installers.rs`).
    struct IsolatedHome {
        _guard: std::sync::MutexGuard<'static, ()>,
        old_home: Option<std::ffi::OsString>,
        _tmp: tempfile::TempDir,
    }

    static HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    impl IsolatedHome {
        fn new() -> Self {
            let guard = HOME_LOCK.lock().expect("HOME_LOCK poisoned");
            let tmp = tempfile::tempdir().expect("tempdir for isolated HOME");
            let old_home = std::env::var_os("HOME");
            // SAFETY: `HOME_LOCK` is held for the entire lifetime of this
            // guard (including drop), so no other thread reads or writes HOME
            // concurrently.
            unsafe { std::env::set_var("HOME", tmp.path()) };
            Self {
                _guard: guard,
                old_home,
                _tmp: tmp,
            }
        }
    }

    impl Drop for IsolatedHome {
        fn drop(&mut self) {
            // SAFETY: `_guard` is still held while `drop` runs.
            unsafe {
                match &self.old_home {
                    Some(h) => std::env::set_var("HOME", h),
                    None => std::env::remove_var("HOME"),
                }
            }
        }
    }

    #[test]
    fn test_default_config() {
        let config = DesktopConfig::default();
        assert!(config.working_dir.is_none());
        assert!(config.theme.is_none());
        assert_eq!(config.approval_mode, Some("confirm".into()));
    }

    #[test]
    fn test_config_serialization_roundtrip() {
        let config = DesktopConfig {
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
        assert_eq!(parsed.approval_mode, None);
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
                display_name: "My GLM".into(),
                kind: "openai-compatible".into(),
                has_api_key: false,
                base_url: Some("https://open.bigmodel.cn/api/paas/v4".into()),
                ..Default::default()
            }],
        };
        let json = serde_json::to_string(&file).unwrap();
        let back: ProvidersFile = serde_json::from_str(&json).unwrap();
        assert_eq!(back.active_provider_id, Some("glm".into()));
        assert_eq!(back.providers.len(), 1);
        assert_eq!(back.providers[0].kind, "openai-compatible");
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
        // base_url/has_api_key are all #[serde(default)]-Optional/default —
        // a hand-written entry omitting them must still parse.
        let json = r#"{
            "id":"anthropic",
            "display_name":"Anthropic",
            "kind":"anthropic"
        }"#;
        let conn: ProviderConnection = serde_json::from_str(json).unwrap();
        assert_eq!(conn.id, "anthropic");
        assert!(!conn.has_api_key);
        assert!(conn.base_url.is_none());
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
            display_name: format!("{id} label"),
            kind: kind.into(),
            has_api_key: false,
            base_url: Some("https://example.test/v1".into()),
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
        assert!(conn.display_name.is_empty());
        assert!(conn.kind.is_empty());
        assert!(!conn.has_api_key);
        assert!(conn.base_url.is_none());
        assert!(conn.models_url.is_none());
        assert!(conn.extra_headers.is_empty());
        assert!(conn.default_max_tokens.is_none());
        assert!(conn.fallback_models.is_empty());
        assert_eq!(conn.tiers, ProviderTiers::default());
    }

    #[test]
    fn provider_connection_does_not_serialize_dead_fields() {
        // TD-4: api_key/model/created_at/label/provider_kind are gone
        // from the wire. The serialized JSON must not contain any of
        // them. has_api_key is the new presence signal.
        let conn = ProviderConnection {
            id: "a".into(),
            display_name: "A".into(),
            kind: "anthropic".into(),
            has_api_key: true,
            base_url: None,
            ..Default::default()
        };
        let json = serde_json::to_string(&conn).unwrap();
        assert!(!json.contains("\"api_key\""), "saw api_key in {json}");
        assert!(!json.contains("\"model\""), "saw model in {json}");
        assert!(!json.contains("\"created_at\""), "saw created_at in {json}");
        assert!(!json.contains("\"label\""), "saw label in {json}");
        assert!(
            !json.contains("\"provider_kind\""),
            "saw provider_kind in {json}"
        );
        assert!(json.contains("\"has_api_key\""));
        assert!(json.contains("\"display_name\""));
        assert!(json.contains("\"kind\""));
    }

    #[test]
    fn provider_connection_legacy_api_key_field_deserializes_via_legacy_struct() {
        // A legacy providers.json with a plaintext api_key must still
        // parse via LegacyProviderConnection (so the credential-store
        // migration can run). The live wire type no longer has api_key.
        let json = r#"{
            "id":"glm",
            "label":"GLM",
            "provider_kind":"openai-compatible",
            "api_key":"sk-legacy-plaintext",
            "created_at":"2026-07-30T00:00:00Z"
        }"#;
        let conn: LegacyProviderConnection = serde_json::from_str(json).unwrap();
        assert_eq!(conn.api_key.as_deref(), Some("sk-legacy-plaintext"));
        // Round-trip back out: api_key must be gone (skip_serializing).
        let back = serde_json::to_string(&conn).unwrap();
        assert!(!back.contains("api_key"));
        assert!(!back.contains("sk-legacy-plaintext"));
    }

    // === One-shot providers.json → providers.toml migration (Phase 2 task 4) ===
    //
    // `migrate_providers_to_toml` runs on AppState startup. It is
    // idempotent — re-runs on a clean store are a no-op — and
    // tolerant — one bad row (empty base_url) doesn't bail the whole
    // migration.

    /// Write a `LegacyProvidersFile` to the canonical path so the migration
    /// helper picks it up. Cleans up afterwards.
    fn seed_legacy_providers_file(file: &LegacyProvidersFile) {
        let path = providers_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, serde_json::to_string_pretty(file).unwrap()).unwrap();
    }

    #[test]
    fn migrate_returns_none_when_legacy_file_absent() {
        // The migration is a one-shot heal — when there's nothing to
        // migrate, return None so the caller falls through to
        // load_or_default (which reads the engine store). Isolated HOME so
        // a sibling test's seeded file can't make this spuriously Some.
        let _home = IsolatedHome::new();
        assert!(migrate_providers_to_toml().is_none());
    }

    #[test]
    fn migrate_returns_none_when_legacy_file_empty() {
        let _home = IsolatedHome::new();
        seed_legacy_providers_file(&LegacyProvidersFile::default());
        assert!(migrate_providers_to_toml().is_none());
    }

    #[test]
    fn build_migrated_provider_model_lifts_entries() {
        // Two connections, both with the user-supplied or engine-
        // default base_url. The pure helper should:
        // - produce a ProviderModelConfig with one ModelProfile
        //   named "default"
        // - keep both provider slots distinct (no OpenAI collapse
        //   for the openai-compatible slot)
        // - set the active target to the file's active_provider_id
        let file = LegacyProvidersFile {
            active_provider_id: Some("glm".into()),
            providers: vec![
                LegacyProviderConnection {
                    id: "anthropic-main".into(),
                    label: "Anthropic".into(),
                    provider_kind: "anthropic".into(),
                    api_key: None,
                    base_url: None,
                    model: Some("claude-sonnet-4-6".into()),
                    created_at: "2026-07-30T00:00:00Z".into(),
                    ..Default::default()
                },
                LegacyProviderConnection {
                    id: "glm".into(),
                    label: "GLM".into(),
                    provider_kind: "openai-compatible".into(),
                    api_key: None,
                    base_url: Some("https://open.bigmodel.cn/api/paas/v4".into()),
                    model: Some("glm-4.6".into()),
                    created_at: "2026-07-30T00:00:00Z".into(),
                    ..Default::default()
                },
            ],
        };

        let pm = build_migrated_provider_model(&file).expect("migration should return Some");
        let default = pm.profiles.get("default").expect("default model profile");
        assert_eq!(default.providers.len(), 2);

        let ids: Vec<&str> = default.providers.iter().map(|p| p.id.as_str()).collect();
        assert!(ids.contains(&"anthropic-main"));
        assert!(ids.contains(&"glm"));
        // Active target follows the legacy active_provider_id.
        assert_eq!(default.active_target.provider_id, "glm");
        assert_eq!(default.active_target.model_id, "glm-4.6");

        // Anthropic got the engine canonical default base_url.
        let anthropic = default
            .providers
            .iter()
            .find(|p| p.id == "anthropic-main")
            .unwrap();
        assert_eq!(anthropic.base_url, "https://api.anthropic.com");

        // GLM kept the user-supplied base_url.
        let glm = default.providers.iter().find(|p| p.id == "glm").unwrap();
        assert_eq!(glm.base_url, "https://open.bigmodel.cn/api/paas/v4");
        assert_eq!(
            glm.kind,
            shannon_types::provider_config::ProviderKind::OpenAiCompatible
        );
    }

    #[test]
    fn build_migrated_provider_model_uses_first_entry_when_active_unset() {
        let file = LegacyProvidersFile {
            active_provider_id: None,
            providers: vec![LegacyProviderConnection {
                id: "anthropic-main".into(),
                label: "Anthropic".into(),
                provider_kind: "anthropic".into(),
                api_key: None,
                base_url: None,
                model: Some("claude-sonnet-4-6".into()),
                created_at: "2026-07-30T00:00:00Z".into(),
                ..Default::default()
            }],
        };
        let pm = build_migrated_provider_model(&file).expect("migration should return Some");
        let default = pm.profiles.get("default").expect("default model profile");
        assert_eq!(default.active_target.provider_id, "anthropic-main");
        assert_eq!(default.active_target.model_id, "claude-sonnet-4-6");
    }

    #[test]
    fn build_migrated_provider_model_skips_entries_with_empty_base_url() {
        // An openai-compatible entry without a base_url has no
        // canonical default and no user-supplied URL — the helper
        // must skip it rather than land a profile with
        // base_url="". The other entries still migrate.
        let file = LegacyProvidersFile {
            active_provider_id: Some("anthropic-main".into()),
            providers: vec![
                LegacyProviderConnection {
                    id: "anthropic-main".into(),
                    label: "Anthropic".into(),
                    provider_kind: "anthropic".into(),
                    api_key: None,
                    base_url: None,
                    model: Some("claude-sonnet-4-6".into()),
                    created_at: "2026-07-30T00:00:00Z".into(),
                    ..Default::default()
                },
                LegacyProviderConnection {
                    id: "broken-glm".into(),
                    label: "Broken GLM".into(),
                    provider_kind: "openai-compatible".into(),
                    api_key: None,
                    base_url: None,
                    model: None,
                    created_at: "2026-07-30T00:00:00Z".into(),
                    ..Default::default()
                },
            ],
        };
        let pm = build_migrated_provider_model(&file).expect("migration should still return Some");
        let default = pm.profiles.get("default").expect("default model profile");
        // The good entry migrated; the broken one was skipped.
        assert_eq!(default.providers.len(), 1);
        assert_eq!(default.providers[0].id, "anthropic-main");
    }

    #[test]
    fn build_migrated_provider_model_returns_none_when_all_entries_skipped() {
        // If every entry is broken (e.g. all openai-compatible
        // with no base_url), the helper returns None — the
        // caller treats that as "no migration needed".
        let file = LegacyProvidersFile {
            active_provider_id: Some("broken-1".into()),
            providers: vec![LegacyProviderConnection {
                id: "broken-1".into(),
                label: "Broken".into(),
                provider_kind: "openai-compatible".into(),
                api_key: None,
                base_url: None,
                model: None,
                created_at: "2026-07-30T00:00:00Z".into(),
                ..Default::default()
            }],
        };
        assert!(build_migrated_provider_model(&file).is_none());
    }

    #[test]
    fn migrate_lifts_and_removes_legacy_file_end_to_end() {
        // Full-disk round-trip under an isolated HOME: seed a legacy file,
        // run the I/O wrapper, verify the model was produced AND the legacy
        // file was removed. With a fresh tempdir the engine store is empty,
        // so migration always runs — the previous `match None` skip branch
        // masked an ordering flake where a sibling test's
        // `cleanup_legacy_providers_file()` deleted this test's shared file
        // mid-run.
        let _home = IsolatedHome::new();
        let file = LegacyProvidersFile {
            active_provider_id: Some("anthropic-main".into()),
            providers: vec![LegacyProviderConnection {
                id: "anthropic-main".into(),
                label: "Anthropic".into(),
                provider_kind: "anthropic".into(),
                api_key: None,
                base_url: None,
                model: Some("claude-sonnet-4-6".into()),
                created_at: "2026-07-30T00:00:00Z".into(),
                ..Default::default()
            }],
        };
        seed_legacy_providers_file(&file);

        let pm = migrate_providers_to_toml().expect("fresh home → migration runs");
        let default = pm.profiles.get("default").expect("default model profile");
        assert_eq!(default.providers[0].id, "anthropic-main");
        // The legacy file is removed after migration.
        assert!(!providers_path().exists());
    }

    // === Provider allowlist (ADR-0005 P4.9) ===
    //
    // `DesktopConfig::enabled_providers` is the desktop-side authoring
    // surface for the engine's `SHANNON_*_PROVIDERS` env allowlist. The
    // tests below pin the three documented states (None / Some(empty) /
    // Some(non_empty)) so the wire shape doesn't silently drift.

    #[test]
    fn enabled_providers_defaults_to_none() {
        // New field — default `None` so legacy installs keep engine
        // env-var behaviour.
        let cfg = DesktopConfig::default();
        assert!(cfg.enabled_providers.is_none());
    }

    #[test]
    fn enabled_providers_round_trips_through_serde() {
        let json = r#"{
            "mcp_servers":[],
            "enabled_providers":["anthropic","openai"]
        }"#;
        let cfg: DesktopConfig = serde_json::from_str(json).unwrap();
        let slugs = cfg
            .enabled_providers
            .clone()
            .expect("Some(non-empty) round-trips");
        assert_eq!(slugs, vec!["anthropic", "openai"]);
        // And back out — the wire shape is preserved.
        let back = serde_json::to_string(&cfg).unwrap();
        assert!(back.contains("\"enabled_providers\":[\"anthropic\",\"openai\"]"));
    }

    #[test]
    fn enabled_providers_distinguishes_none_from_some_empty() {
        // Critical: `None` (use engine env vars) and `Some(vec![])`
        // (user toggled every provider off) look the same after serde
        // deserialisation if the field defaults to `[]`. The default
        // MUST be `None` so the two states stay distinguishable on the
        // wire and in memory.
        let cfg_none = DesktopConfig::default();
        assert!(cfg_none.enabled_providers.is_none());

        let cfg_empty: DesktopConfig =
            serde_json::from_str(r#"{"mcp_servers":[],"enabled_providers":[]}"#).unwrap();
        assert_eq!(cfg_empty.enabled_providers, Some(vec![]));
    }
}
