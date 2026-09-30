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
    /// Dream distillation (L2) master switch. When false, `run_dream_pass`
    /// returns a skipped result without reading a single session or memory
    /// file — the pass is opt-in and its output review-gated. Default:
    /// false. Gated additionally by `skill_detection_enabled` (the privacy
    /// main switch).
    #[serde(default)]
    pub dream_enabled: bool,
    /// L3 skill-distillation leg of the dream pass. When true, a running
    /// dream pass additionally runs the skill-pattern detector over the same
    /// session window and LLM-refines each newly appended candidate into the
    /// review queue (`refined=true`) — never auto-promoted. Requires both
    /// `dream_enabled` and `skill_detection_enabled`; default false.
    #[serde(default)]
    pub dream_skill_distill_enabled: bool,
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
    /// Session ids with an open dedicated window (P1-1 multi-window).
    /// Mirrored from the in-memory window registry; replayed at app startup
    /// to restore the previous session-window set. `#[serde(default)]` keeps
    /// older config files loadable.
    #[serde(default)]
    pub open_session_windows: Vec<String>,
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
    /// P1-3: the persisted **active permission profile** (`strict` /
    /// `balanced` / `permissive` / a custom profile name from
    /// `.shannon/profiles/*.toml`). `None` = no profile — the plain
    /// `approval_mode` above drives the engine, exactly as before this
    /// field existed. Written by `activate_permission_profile`.
    #[serde(default)]
    pub active_permission_profile: Option<String>,
    /// P1-3: command-sandbox configuration. `None` = the key was never set
    /// (older configs) → engine default `off`. The frozen config key path
    /// is `sandbox.mode`; see [`SandboxConfig`].
    #[serde(default)]
    pub sandbox: Option<SandboxConfig>,
    /// P2-5: off-peak execution settings. The frozen config key path is
    /// `offpeak.model_override`; see [`OffpeakConfig`]. `#[serde(default)]`
    /// keeps older `config.json` files loadable.
    #[serde(default)]
    pub offpeak: OffpeakConfig,
    /// B2: master switch for real sub-agent execution. When true,
    /// `agent_spawn` runs a real in-process sub-agent QueryEngine (real LLM
    /// spend) and the registry lifecycle is bridged to the frontend as
    /// `subagent:start` / `subagent:stop`. Default: false — the tool keeps
    /// its zero-cost placeholder behavior until opted in. The frozen config
    /// key is `agent_teams_enabled`; see `crate::agent_teams`.
    #[serde(default)]
    pub agent_teams_enabled: bool,
    /// Master switch for the session GC (卡0). When false — the default —
    /// the GC never deletes anything ("never auto-delete" is the standing
    /// policy; adversarial review F10/F13). Since Task 2 (archive MVP, 卡A)
    /// the enabled GC prunes only **archived** sessions past the
    /// [`DesktopConfig::session_retention_days`] window; the
    /// `SHANNON_SESSION_GC_ENABLED` env var can only *force-disable* it
    /// (an env var must never switch deletion on) — see
    /// `commands_sessions::effective_gc_retention_days`.
    #[serde(default)]
    pub session_gc_enabled: bool,
    /// Session GC retention window in days (卡A). Deletion candidates are
    /// only sessions that are **archived** (their `<id>/curation.json`
    /// sidecar says so) **and** whose last activity is older than this many
    /// days. `None` — the default — means **never delete**, even with
    /// `session_gc_enabled = true`: "never auto-delete" stays the standing
    /// posture until the user configures an explicit window.
    #[serde(default)]
    pub session_retention_days: Option<u32>,
    /// R3-3: plan-phase model tier — `fast` | `standard` | `pro`. `None`
    /// (and any legacy junk value, which the reader normalizes away) means
    /// **inherit**: the plan phase uses the global default model. Written by
    /// `configure('plan_tier')`; consulted at query time by
    /// `commands_chat::resolve_client_config_for_session` under the
    /// precedence **session override (R2-1) > phase tier > global default**.
    /// Global preference, deliberately NOT per session.
    #[serde(default)]
    pub plan_tier: Option<String>,
    /// R3-3: act-phase model tier — same contract as [`DesktopConfig::plan_tier`]
    /// for the execution phase (every approval mode except `plan`).
    #[serde(default)]
    pub act_tier: Option<String>,
    /// P2-1: user-set monthly spend budget in USD, covering every source the
    /// Usage page counts (chat + scheduled routines). `None` = unset: the
    /// sidebar falls back to the trailing 7-day cost and no threshold
    /// alert fires. Written via `configure('monthly_budget_usd')`; consumed
    /// by `usage_governance::get_usage_governance`.
    #[serde(default)]
    pub monthly_budget_usd: Option<f64>,
}

/// P2-5: payload of the desktop `offpeak.model_override` config key.
///
/// When a routine executes **inside its execution window** and this key is
/// non-empty, the run uses the named model instead of the active one
/// (downgrade-to-cheaper-model pattern). Empty/None = disabled — the
/// configured model override is ignored and routines use the active model.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OffpeakConfig {
    /// Model id for in-window routine executions. `None`/empty = disabled.
    #[serde(default)]
    pub model_override: Option<String>,
}

impl OffpeakConfig {
    /// The effective override: `None` when unset or empty/whitespace
    /// (brief contract: empty = disabled).
    pub fn effective_model_override(&self) -> Option<&str> {
        self.model_override
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }
}

/// P1-3: payload of the desktop `sandbox.mode` config key.
///
/// `mode` uses the same vocabulary as the engine's `[sandbox]` TOML table /
/// `SHANNON_SANDBOX` env var (`off` | `local` | `landlock`, see
/// `shannon_tool_interface::SandboxMode`) so there is exactly one tier
/// naming across TUI and desktop:
///
/// | mode       | desktop UI tier      | enforcement                                        |
/// |------------|----------------------|----------------------------------------------------|
/// | `off`      | 关闭 (off)           | legacy passthrough, byte-identical                 |
/// | `local`    | 只读文件系统 (readonly fs) | user-space policy mirror on the in-process fs tools |
/// | `landlock` | 完全 (full, experimental) | kernel-enforced child world + user-space fs mirror |
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxConfig {
    /// `off` | `local` | `landlock`. `None` behaves like `off`.
    #[serde(default)]
    pub mode: Option<String>,
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
            // R2-4 (engine): per-model metadata declarations. The desktop's
            // legacy `ProviderConnection` cache carries none — the engine
            // store's reload-merge is where declared models enter.
            models: Vec::new(),
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
/// - `kind` is the UI's slug string via [`kind_engine_to_slug`].
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
/// Round-trips for every kind the
/// engine knows about today; an unknown arm — `ProviderKind` is
/// `non_exhaustive` so the enum may grow — falls back to the
/// `openai-compatible` slug, which matches the existing collapse
/// convention (engine resolvers recover fine-grained identity from
/// `base_url` at resolution time).
pub(crate) fn kind_engine_to_slug(
    kind: &shannon_types::provider_config::ProviderKind,
) -> &'static str {
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
            dream_enabled: false,
            dream_skill_distill_enabled: false,
            notifications_master_enabled: default_true(),
            notifications_dnd_enabled: false,
            notifications_dnd_start: None,
            notifications_dnd_end: None,
            notifications_on_completed: default_true(),
            notifications_on_failed: default_true(),
            stt: None,
            voice_local: VoiceLocalConfig::default(),
            gateway: GatewayDesktopConfig::default(),
            open_session_windows: Vec::new(),
            enabled_providers: None,
            active_permission_profile: None,
            sandbox: None,
            offpeak: OffpeakConfig::default(),
            agent_teams_enabled: false,
            session_gc_enabled: false,
            session_retention_days: None,
            plan_tier: None,
            act_tier: None,
            monthly_budget_usd: None,
        }
    }
}

/// Resolve the config file path: `~/.shannon/desktop/config.json`
fn config_path() -> PathBuf {
    let home = dirs_home().unwrap_or_else(|| PathBuf::from("."));
    home.join(".shannon").join("desktop").join("config.json")
}

/// Resolve the legacy MCP servers store path:
/// `~/.shannon/desktop/mcp-servers.json`.
///
/// G1 split-brain fix: this file is legacy-only. The single source of truth
/// for desktop-installed MCP servers is `~/.shannon/settings.json#mcpServers`
/// (the same blob the CLI and the extensions-hub installers read/write). At
/// startup `migrate_legacy_mcp_servers` copies any entries still living here
/// into settings.json — the file itself is never rewritten or deleted.
fn mcp_servers_path() -> PathBuf {
    let home = dirs_home().unwrap_or_else(|| PathBuf::from("."));
    home.join(".shannon")
        .join("desktop")
        .join("mcp-servers.json")
}

/// Resolve the unified MCP store path: `~/.shannon/settings.json`.
fn user_settings_path() -> PathBuf {
    let home = dirs_home().unwrap_or_else(|| PathBuf::from("."));
    home.join(".shannon").join("settings.json")
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

/// Load MCP server configs from the unified store
/// (`~/.shannon/settings.json#mcpServers`).
///
/// G1 split-brain fix: previously this read the legacy
/// `~/.shannon/desktop/mcp-servers.json` while the extensions hub wrote
/// `~/.shannon/settings.json#mcpServers` — servers installed from the hub
/// never showed up (and vice versa). Entries that are not stdio-startable
/// (no `command`, e.g. `url`-only OAuth/HTTP servers) are still listed so
/// the UI shows them; the process pool skips them.
pub fn load_mcp_servers() -> Vec<McpServerConfig> {
    load_mcp_servers_from(&user_settings_path())
}

/// `load_mcp_servers` against an explicit `settings.json` path (tests inject
/// a tempdir so they never touch the user's HOME).
pub fn load_mcp_servers_from(path: &std::path::Path) -> Vec<McpServerConfig> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(root) = serde_json::from_str::<serde_json::Value>(&content) else {
        return Vec::new();
    };
    let Some(map) = root.get("mcpServers").and_then(|m| m.as_object()) else {
        return Vec::new();
    };
    let mut servers: Vec<McpServerConfig> = map
        .iter()
        .filter_map(|(name, value)| mcp_server_config_from_json(name, value))
        .collect();
    servers.sort_by(|a, b| a.name.cmp(&b.name));
    servers
}

/// Convert one `mcpServers.<name>` JSON entry into a [`McpServerConfig`].
///
/// `None` = not listable as a config row (e.g. the value is not an object).
/// Stdio `command` is the only hard requirement for the *pool*, but
/// url-only servers are surfaced too (`command` empty, `enabled` from the
/// `enabled` flag) so `list_mcp_servers` shows what the user installed.
fn mcp_server_config_from_json(name: &str, value: &serde_json::Value) -> Option<McpServerConfig> {
    let obj = value.as_object()?;
    let command = obj
        .get("command")
        .and_then(|c| c.as_str())
        .unwrap_or_default()
        .to_string();
    let args = obj
        .get("args")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let env = obj
        .get("env")
        .and_then(|e| e.as_object())
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();
    let enabled = obj.get("enabled").and_then(|e| e.as_bool()).unwrap_or(true);
    Some(McpServerConfig {
        name: name.to_string(),
        command,
        args,
        env,
        enabled,
    })
}

/// Serialize a [`McpServerConfig`] into the `mcpServers.<name>` JSON entry
/// shape shared with the CLI. Only stdio rows are serializable — url-only
/// entries (`url`/`type` fields) never enter this lossy struct, so saves
/// skip them and their original JSON blob in the store stays untouched.
fn mcp_server_config_to_json(config: &McpServerConfig) -> serde_json::Value {
    serde_json::json!({
        "command": config.command,
        "args": config.args,
        "env": config.env,
        "enabled": config.enabled,
    })
}

/// Atomically replace `path` with the pretty-printed JSON `root`:
/// write a temp file in the same directory, then rename over the target.
/// A crash mid-write can never leave a truncated/corrupt settings.json.
fn write_settings_json_atomic(
    path: &std::path::Path,
    root: &serde_json::Value,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content = serde_json::to_string_pretty(root).map_err(|e| e.to_string())?;
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "settings.json".to_string());
    let tmp = path.with_file_name(format!(".{file_name}.tmp"));
    std::fs::write(&tmp, content).map_err(|e| e.to_string())?;
    // Best-effort restrictive perms before the rename becomes visible.
    crate::file_permissions::restrict_to_owner(&tmp);
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })?;
    crate::file_permissions::restrict_to_owner(path);
    Ok(())
}

/// Read and parse the settings.json root. A missing file starts empty
/// (`Ok(None)`-style via `json!({})`); a present-but-corrupt file is an
/// error — silently starting from `{}` here would make the next save reset
/// the whole file (permissions, provider mirrors, everything) to just
/// `mcpServers`.
fn read_settings_json_root(path: &std::path::Path) -> Result<serde_json::Value, String> {
    match std::fs::read_to_string(path) {
        Ok(content) => serde_json::from_str(&content)
            .map_err(|e| format!("settings.json parse ({}): {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(serde_json::json!({})),
        Err(e) => Err(format!("settings.json read ({}): {e}", path.display())),
    }
}

/// Save MCP server configs into the unified store
/// (`~/.shannon/settings.json#mcpServers`), preserving every other
/// top-level key in the file. Url-only rows (empty `command`) cannot be
/// represented by [`McpServerConfig`] and are skipped — their existing
/// store entry (if any) is left untouched. Writes are atomic
/// (temp-file + rename); a corrupt existing file is an error, never a
/// silent reset.
pub fn save_mcp_servers(servers: &[McpServerConfig]) -> Result<(), String> {
    save_mcp_servers_to(&user_settings_path(), servers)
}

/// `save_mcp_servers` against an explicit `settings.json` path (see
/// [`load_mcp_servers_from`]).
pub fn save_mcp_servers_to(
    path: &std::path::Path,
    servers: &[McpServerConfig],
) -> Result<(), String> {
    let mut root = read_settings_json_root(path)?;
    let map = root
        .as_object_mut()
        .ok_or_else(|| format!("settings.json is not a JSON object: {}", path.display()))?;
    let mcp = map
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}));
    let Some(mcp_obj) = mcp.as_object_mut() else {
        return Err("settings.json#mcpServers is not an object".to_string());
    };
    for config in servers {
        if config.command.is_empty() {
            // Url-only row: the struct cannot round-trip `url`/`type`, so
            // writing it would replace the real entry with a lossy stub.
            // Skip — whatever the store already holds stays intact.
            continue;
        }
        mcp_obj.insert(config.name.clone(), mcp_server_config_to_json(config));
    }
    write_settings_json_atomic(path, &root)
}

/// Remove one MCP server entry from the unified store by name. Returns
/// `Ok(false)` when no entry with that name existed.
pub fn remove_mcp_server_entry(name: &str) -> Result<bool, String> {
    remove_mcp_server_entry_from(&user_settings_path(), name)
}

/// `remove_mcp_server_entry` against an explicit `settings.json` path.
pub fn remove_mcp_server_entry_from(path: &std::path::Path, name: &str) -> Result<bool, String> {
    let mut root = read_settings_json_root(path)?;
    let removed = root
        .get_mut("mcpServers")
        .and_then(|m| m.as_object_mut())
        .and_then(|m| m.remove(name))
        .is_some();
    if removed {
        write_settings_json_atomic(path, &root)?;
    }
    Ok(removed)
}

/// One-time, idempotent migration of the legacy
/// `~/.shannon/desktop/mcp-servers.json` store into the unified
/// `~/.shannon/settings.json#mcpServers`. Entries already present in the
/// unified store (by name) are left untouched; the legacy file is never
/// modified or deleted. Returns the number of migrated entries.
pub fn migrate_legacy_mcp_servers() -> usize {
    migrate_legacy_mcp_servers_to(&user_settings_path(), &mcp_servers_path())
}

/// `migrate_legacy_mcp_servers` against explicit paths (tests inject
/// tempdir paths so they never touch the user's HOME).
pub fn migrate_legacy_mcp_servers_to(
    settings_path: &std::path::Path,
    legacy_path: &std::path::Path,
) -> usize {
    let Ok(content) = std::fs::read_to_string(legacy_path) else {
        return 0;
    };
    let legacy: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                path = %legacy_path.display(),
                error = %e,
                "legacy mcp-servers.json is not valid JSON — skipping migration"
            );
            return 0;
        }
    };
    let Some(rows) = legacy.as_array() else {
        return 0;
    };

    // Existing unified entries win (idempotent re-runs are no-ops).
    let existing: std::collections::HashSet<String> = load_mcp_servers_from(settings_path)
        .into_iter()
        .map(|s| s.name)
        .collect();

    let mut configs = Vec::new();
    for row in rows {
        let Some(name) = row.get("name").and_then(|n| n.as_str()) else {
            continue;
        };
        if existing.contains(name) {
            continue;
        }
        configs.push(McpServerConfig {
            name: name.to_string(),
            command: row
                .get("command")
                .and_then(|c| c.as_str())
                .unwrap_or_default()
                .to_string(),
            args: row
                .get("args")
                .map(|a| serde_json::from_value(a.clone()).unwrap_or_default())
                .unwrap_or_default(),
            env: row
                .get("env")
                .map(|e| serde_json::from_value(e.clone()).unwrap_or_default())
                .unwrap_or_default(),
            enabled: row.get("enabled").and_then(|e| e.as_bool()).unwrap_or(true),
        });
    }
    if configs.is_empty() {
        return 0;
    }
    let migrated = configs.len();
    if let Err(e) = save_mcp_servers_to(settings_path, &configs) {
        tracing::warn!(error = %e, "legacy MCP server migration write failed");
        return 0;
    }
    tracing::info!(
        count = migrated,
        "migrated legacy MCP servers into settings.json"
    );
    migrated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = DesktopConfig::default();
        assert!(config.working_dir.is_none());
        assert!(config.theme.is_none());
        assert_eq!(config.approval_mode, Some("confirm".into()));
    }

    #[test]
    fn test_p1_3_profile_and_sandbox_defaults_and_round_trip() {
        // P1-3: config files written before `active_permission_profile` /
        // `sandbox` existed must keep loading (both default to "unset").
        let legacy: DesktopConfig = serde_json::from_str(
            r#"{"working_dir":null,"theme":null,"mcp_servers":[],"approval_mode":null}"#,
        )
        .expect("legacy config must deserialize");
        assert!(legacy.active_permission_profile.is_none());
        assert!(legacy.sandbox.is_none());

        let config = DesktopConfig {
            active_permission_profile: Some("strict".into()),
            sandbox: Some(SandboxConfig {
                mode: Some("local".into()),
            }),
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(
            json.contains("\"active_permission_profile\":\"strict\""),
            "{json}"
        );
        assert!(json.contains("\"sandbox\":{\"mode\":\"local\"}"), "{json}");
        let back: DesktopConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.active_permission_profile.as_deref(), Some("strict"));
        assert_eq!(back.sandbox.and_then(|s| s.mode), Some("local".into()));
    }

    #[test]
    fn test_open_session_windows_field_defaults_and_round_trips() {
        // P1-1: config files written before the field existed must load with
        // an empty restore list.
        let legacy: DesktopConfig = serde_json::from_str(
            r#"{"working_dir":null,"theme":null,"mcp_servers":[],"approval_mode":null}"#,
        )
        .expect("legacy config must deserialize");
        assert!(legacy.open_session_windows.is_empty());

        let config = DesktopConfig {
            open_session_windows: vec!["7e6c3f18-4a2e-4f6a-9a52-6d1c1a0f83f1".into()],
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let back: DesktopConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(
            back.open_session_windows,
            vec!["7e6c3f18-4a2e-4f6a-9a52-6d1c1a0f83f1".to_string()]
        );
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
    fn session_gc_defaults_to_disabled_and_legacy_configs_load() {
        // 卡0: "never auto-delete" is the standing policy — the GC flag must
        // default to false, and config.json files written before the key
        // existed must load with the GC still disabled.
        assert!(!DesktopConfig::default().session_gc_enabled);
        let legacy: DesktopConfig = serde_json::from_str(
            r#"{"working_dir":null,"theme":null,"mcp_servers":[],"approval_mode":null}"#,
        )
        .expect("legacy config must deserialize");
        assert!(!legacy.session_gc_enabled);

        // An explicit opt-in round-trips.
        let cfg: DesktopConfig =
            serde_json::from_str(r#"{"mcp_servers":[],"session_gc_enabled":true}"#).unwrap();
        assert!(cfg.session_gc_enabled);
        let back = serde_json::to_string(&cfg).unwrap();
        assert!(back.contains("\"session_gc_enabled\":true"), "{back}");
    }

    #[test]
    fn session_retention_days_defaults_to_never_and_round_trips() {
        // 卡A: the retention window defaults to None = never delete, even
        // with the GC enabled; legacy config.json files (key absent) must
        // load as None.
        assert_eq!(DesktopConfig::default().session_retention_days, None);
        let legacy: DesktopConfig = serde_json::from_str(
            r#"{"working_dir":null,"theme":null,"mcp_servers":[],"approval_mode":null}"#,
        )
        .expect("legacy config must deserialize");
        assert_eq!(legacy.session_retention_days, None);

        let cfg: DesktopConfig = serde_json::from_str(
            r#"{"mcp_servers":[],"session_gc_enabled":true,"session_retention_days":90}"#,
        )
        .unwrap();
        assert_eq!(cfg.session_retention_days, Some(90));
        let back = serde_json::to_string(&cfg).unwrap();
        assert!(back.contains("\"session_retention_days\":90"), "{back}");
    }

    #[test]
    fn phase_tier_fields_default_and_round_trip() {
        // R3-3: the phase-tier pair is new keys — config.json files written
        // before them must load with both unset (inherit).
        assert_eq!(DesktopConfig::default().plan_tier, None);
        assert_eq!(DesktopConfig::default().act_tier, None);
        let legacy: DesktopConfig = serde_json::from_str(
            r#"{"working_dir":null,"theme":null,"mcp_servers":[],"approval_mode":null}"#,
        )
        .expect("legacy config must deserialize");
        assert_eq!(legacy.plan_tier, None);
        assert_eq!(legacy.act_tier, None);

        // Explicit values round-trip.
        let cfg: DesktopConfig =
            serde_json::from_str(r#"{"mcp_servers":[],"plan_tier":"fast","act_tier":"pro"}"#)
                .unwrap();
        assert_eq!(cfg.plan_tier.as_deref(), Some("fast"));
        assert_eq!(cfg.act_tier.as_deref(), Some("pro"));
        let back = serde_json::to_string(&cfg).unwrap();
        assert!(back.contains("\"plan_tier\":\"fast\""), "{back}");
        assert!(back.contains("\"act_tier\":\"pro\""), "{back}");
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

    // === Off-peak model override (P2-5, frozen key `offpeak.model_override`) ===

    #[test]
    fn offpeak_defaults_to_disabled() {
        let cfg = DesktopConfig::default();
        assert!(cfg.offpeak.model_override.is_none());
        assert!(cfg.offpeak.effective_model_override().is_none());
    }

    #[test]
    fn offpeak_legacy_config_without_key_loads() {
        // config.json written before the `offpeak` key existed must keep
        // loading, with the override disabled.
        let legacy: DesktopConfig = serde_json::from_str(
            r#"{"working_dir":null,"theme":null,"mcp_servers":[],"approval_mode":null}"#,
        )
        .expect("legacy config must deserialize");
        assert!(legacy.offpeak.model_override.is_none());
    }

    #[test]
    fn offpeak_model_override_round_trips_and_normalizes_empty_to_disabled() {
        let json = r#"{"mcp_servers":[],"offpeak":{"model_override":"glm-4-flash"}}"#;
        let cfg: DesktopConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.offpeak.effective_model_override(), Some("glm-4-flash"));

        let written = serde_json::to_string(&cfg).unwrap();
        assert!(
            written.contains("\"offpeak\":{\"model_override\":\"glm-4-flash\"}"),
            "{written}"
        );

        // Brief contract: empty (and whitespace-only) = disabled.
        let mut empty = DesktopConfig::default();
        empty.offpeak.model_override = Some(String::new());
        assert!(empty.offpeak.effective_model_override().is_none());
        let mut blank = DesktopConfig::default();
        blank.offpeak.model_override = Some("   ".into());
        assert!(blank.offpeak.effective_model_override().is_none());
        // Whitespace around a real value is trimmed on read.
        let mut padded = DesktopConfig::default();
        padded.offpeak.model_override = Some("  glm-4-flash ".into());
        assert_eq!(
            padded.offpeak.effective_model_override(),
            Some("glm-4-flash")
        );
    }

    // ---- G1: unified MCP store (settings.json#mcpServers) ----

    fn tmp_settings(dir: &std::path::Path) -> PathBuf {
        dir.join("settings.json")
    }

    #[test]
    fn unified_mcp_store_round_trips_and_preserves_foreign_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = tmp_settings(dir.path());

        // Start with a settings.json carrying an unrelated key + an OAuth
        // (url-only) server installed by the hub.
        std::fs::write(
            &path,
            r#"{"permissions":{"allow":["Bash"]},"mcpServers":{"notion":{"type":"http","url":"https://mcp.example"}}}"#,
        )
        .unwrap();

        // Upsert a stdio row — foreign keys and the url entry survive.
        save_mcp_servers_to(
            &path,
            &[McpServerConfig {
                name: "everything".into(),
                command: "npx".into(),
                args: vec![
                    "-y".into(),
                    "@modelcontextprotocol/server-everything".into(),
                ],
                env: [("K".to_string(), "v".to_string())].into_iter().collect(),
                enabled: true,
            }],
        )
        .unwrap();

        let root: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(root["permissions"]["allow"][0], "Bash");
        assert_eq!(root["mcpServers"]["notion"]["url"], "https://mcp.example");

        // Load sees both entries (url-only listed with empty command).
        let servers = load_mcp_servers_from(&path);
        assert_eq!(servers.len(), 2);
        let everything = servers.iter().find(|s| s.name == "everything").unwrap();
        assert_eq!(everything.command, "npx");
        assert_eq!(everything.args.len(), 2);
        assert_eq!(everything.env.get("K").map(String::as_str), Some("v"));
        assert!(everything.enabled);
        let notion = servers.iter().find(|s| s.name == "notion").unwrap();
        assert!(notion.command.is_empty());

        // Removal drops only the target row.
        assert!(remove_mcp_server_entry_from(&path, "everything").unwrap());
        assert!(!remove_mcp_server_entry_from(&path, "everything").unwrap());
        let servers = load_mcp_servers_from(&path);
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "notion");
        // Foreign key still intact after removal.
        let root: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(root["permissions"].is_object());
    }

    #[test]
    fn legacy_mcp_migration_is_idempotent_and_existing_entries_win() {
        let dir = tempfile::tempdir().unwrap();
        let settings = tmp_settings(dir.path());
        let legacy_dir = dir.path().join("desktop");
        std::fs::create_dir_all(&legacy_dir).unwrap();
        let legacy = legacy_dir.join("mcp-servers.json");
        std::fs::write(
            &legacy,
            r#"[
                {"name":"fs","command":"npx","args":["-y","server-fs"],"env":{},"enabled":true},
                {"name":"shared","command":"legacy-cmd","args":[],"env":{},"enabled":true}
            ]"#,
        )
        .unwrap();

        // A pre-existing unified entry with the same name must win.
        save_mcp_servers_to(
            &settings,
            &[McpServerConfig {
                name: "shared".into(),
                command: "unified-cmd".into(),
                args: vec![],
                env: Default::default(),
                enabled: true,
            }],
        )
        .unwrap();

        let migrated = migrate_legacy_mcp_servers_to(&settings, &legacy);
        assert_eq!(migrated, 1, "only 'fs' migrates; 'shared' already exists");

        let servers = load_mcp_servers_from(&settings);
        assert_eq!(servers.len(), 2);
        let shared = servers.iter().find(|s| s.name == "shared").unwrap();
        assert_eq!(shared.command, "unified-cmd");
        assert!(servers.iter().any(|s| s.name == "fs" && s.command == "npx"));

        // Idempotent: a second run migrates nothing.
        assert_eq!(migrate_legacy_mcp_servers_to(&settings, &legacy), 0);
        // The legacy file is untouched.
        assert!(legacy.exists());
    }

    #[test]
    fn legacy_mcp_migration_handles_missing_and_corrupt_files() {
        let dir = tempfile::tempdir().unwrap();
        let settings = tmp_settings(dir.path());
        let missing = dir.path().join("does-not-exist.json");
        assert_eq!(migrate_legacy_mcp_servers_to(&settings, &missing), 0);

        let corrupt = dir.path().join("corrupt.json");
        std::fs::write(&corrupt, "{not json").unwrap();
        assert_eq!(migrate_legacy_mcp_servers_to(&settings, &corrupt), 0);
        // Corrupt legacy file did not create a settings.json.
        assert!(!settings.exists());
    }

    /// Imp-1: a corrupt settings.json must NEVER be silently reset by a
    /// save — the error propagates and the original bytes stay untouched
    /// (permissions & co. survive a transient parse failure).
    #[test]
    fn save_on_corrupt_settings_returns_err_and_leaves_file_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = tmp_settings(dir.path());
        let original = r#"{"permissions":{"deny":"oops"},,,"mcpServers":{"keep":{"command":"x"}}}"#;
        std::fs::write(&path, original).unwrap();

        let result = save_mcp_servers_to(
            &path,
            &[McpServerConfig {
                name: "new".into(),
                command: "npx".into(),
                args: vec![],
                env: Default::default(),
                enabled: true,
            }],
        );
        assert!(result.is_err(), "corrupt settings.json must fail the save");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            original,
            "failed save must not touch the original file"
        );
        // Remove is equally honest (parse error → Err, file untouched).
        assert!(remove_mcp_server_entry_from(&path, "keep").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }

    /// Imp-1: saves are atomic — no `.tmp` sibling is left behind, and a
    /// successful save keeps foreign keys intact.
    #[test]
    fn save_leaves_no_temp_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = tmp_settings(dir.path());
        save_mcp_servers_to(
            &path,
            &[McpServerConfig {
                name: "fs".into(),
                command: "npx".into(),
                args: vec![],
                env: Default::default(),
                enabled: true,
            }],
        )
        .unwrap();
        assert!(path.exists());
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp file leaked: {leftovers:?}");
    }

    /// Minor-5: saving never writes a lossy marker over a url-only entry —
    /// the original blob (url/type/enabled) survives untouched.
    #[test]
    fn save_skips_url_only_rows_instead_of_clobbering_them() {
        let dir = tempfile::tempdir().unwrap();
        let path = tmp_settings(dir.path());
        std::fs::write(
            &path,
            r#"{"mcpServers":{"notion":{"type":"http","url":"https://mcp.example","enabled":true}}}"#,
        )
        .unwrap();

        // A save that re-lists the url-only row (empty command) must not
        // replace it with a marker stub.
        save_mcp_servers_to(
            &path,
            &[McpServerConfig {
                name: "notion".into(),
                command: String::new(),
                args: vec![],
                env: Default::default(),
                enabled: true,
            }],
        )
        .unwrap();
        let root: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(root["mcpServers"]["notion"]["url"], "https://mcp.example");
        assert_eq!(root["mcpServers"]["notion"]["type"], "http");
        assert!(
            root["mcpServers"]["notion"]
                .get("shannon:list_only")
                .is_none(),
            "private list_only marker must not exist"
        );
    }
}
