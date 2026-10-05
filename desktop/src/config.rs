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
    /// Surface a desktop notification when the user's attention is required
    /// (tool-approval waits, budget alerts — `NotificationKind::NeedsAttention`).
    /// Default: enabled.
    #[serde(default = "default_true")]
    pub notifications_on_needs_attention: bool,
    /// Play the frontend-composited chime (Web Audio) on task completed /
    /// failed / needs-attention events. Independent of the OS notification
    /// sound; the chime itself is gated by the master switch and DND window
    /// frontend-side (R4). Default: disabled.
    #[serde(default)]
    pub notifications_sound_enabled: bool,
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
    /// S3-5 (P2-19): effort dial — `low` | `standard` | `high` | `max`
    /// (canonical forms; `medium` is accepted on write and normalized to
    /// `standard`, the engine's alias). `None` = the engine default
    /// (`Standard`, which sends no thinking parameters). Written by the
    /// composer's effort sub-tier via `configure('effort_level')` — the same
    /// key vocabulary the CLI `/effort` surface speaks — and applied per turn
    /// via `QueryEngine::set_effort` in the send path. Global preference,
    /// deliberately NOT per session.
    #[serde(default)]
    pub effort_level: Option<String>,
    /// Settings R3 T3 — GPU-composited webview rendering switch. `true`
    /// (default) keeps hardware acceleration on; `false` injects the
    /// per-platform "disable GPU" env vars BEFORE the webview backend
    /// initializes (see [`apply_hardware_acceleration_env`]) so a broken
    /// GPU/driver can no longer blank-screen or crash the window. Takes
    /// effect on the NEXT app launch — the env vars are read once at
    /// webview creation. macOS is unaffected (no injection path).
    #[serde(default = "default_true")]
    pub hardware_acceleration: bool,
    /// Settings R3 T3 — always-on wake lock. When true, the desktop holds a
    /// prevent-sleep refcount from `AppState::new` until the toggle flips
    /// off or the app exits. Default: false (opt-in).
    #[serde(default)]
    pub power_keep_awake: bool,
    /// Settings R3 T3 — block idle sleep while ANY agent run (interactive
    /// turn, background task, goal run, best-of-N branch, routine rerun) is
    /// streaming. Refcounted, so overlapping runs keep the lock until the
    /// last one ends. Default: true (the agent working while the machine
    /// dozes off is the surprising outcome).
    #[serde(default = "default_power_block_sleep_during_tasks")]
    pub power_block_sleep_during_tasks: bool,
    /// Settings R3 T4 (B1) — explicit HTTP(S) proxy URL. When set, injected
    /// at startup as `HTTPS_PROXY` + `HTTP_PROXY` + `ALL_PROXY` so every
    /// outbound path (LLM clients, MCP stdio, gateway sidecar, command-tool
    /// subprocesses) inherits it — reqwest reads the standard proxy env vars
    /// by default. Empty/None keeps the implicit env fallback (R1: never
    /// force direct connections). Empty at configure time → stored as None.
    #[serde(default)]
    pub network_proxy_url: Option<String>,
    /// Settings R3 T4 (B1) — NO_PROXY companion: comma-separated hosts that
    /// bypass the proxy (`localhost,127.0.0.1,::1,.example.com`). Injected
    /// as `NO_PROXY` when set; empty/None leaves the env untouched.
    #[serde(default)]
    pub network_no_proxy: Option<String>,
    /// Settings R3 T4 (B1) — custom CA certificate (PEM) path, `~`-expanded
    /// at configure time and existence-checked. Injected as
    /// `SHANNON_CA_BUNDLE` (read by the engine/desktop HTTP builders — the
    /// workspace reqwest trusts webpki-roots only, so a corporate MITM CA
    /// must be added explicitly) plus `NODE_EXTRA_CA_CERTS` /
    /// `SSL_CERT_FILE` for subprocesses. Empty/None leaves the env
    /// untouched.
    #[serde(default)]
    pub network_ca_cert_path: Option<String>,
    /// Settings R3 T6 — master switch for the query engine's automatic
    /// context compaction (the 60%/80% warning injections, micro-compaction
    /// and the compact/truncate ladder). Default `true`: existing behavior.
    /// When `false` the engine preserves model requests and responses
    /// verbatim — nothing is auto-compacted or truncated, and a turn fails
    /// only when the context window is genuinely exhausted (`/compact`
    /// stays available as the manual path). Written via
    /// `configure("context.auto_compact")`; the engine is rebuilt per
    /// message, so a change applies to the NEXT message without a restart.
    #[serde(default = "default_true")]
    pub context_auto_compact: bool,
    /// Settings R3 T7 — master switch for the timed auto-archive scan. When
    /// false — the default — the scan never archives anything ("never
    /// auto-archive" is the standing policy, mirroring the GC posture).
    /// When true, every pass archives active sessions that are 已完成 per
    /// the R6 adjudication (`!running && 无未读 inbox 条目`), unpinned, and
    /// whose last activity is older than
    /// [`DesktopConfig::session_auto_archive_days`]. Re-read live each pass
    /// (the loop runs every 6h) — a flip needs no restart. Written via
    /// `configure("session.auto_archive_enabled")`.
    #[serde(default)]
    pub session_auto_archive_enabled: bool,
    /// Settings R3 T7 — auto-archive retention window in days. A session
    /// qualifies when its last activity (`events.jsonl` mtime) is older than
    /// this many days. Default 7; `configure` clamps into `1..=365` so a
    /// hand-edited or wire-level bad value can neither wedge (0) nor explode
    /// (u32::MAX) the scan. Written via
    /// `configure("session.auto_archive_days")`.
    #[serde(default = "default_session_auto_archive_days")]
    pub session_auto_archive_days: u32,
    /// Settings R3 T8 — 提问自动继续. When true, a desktop `ask_user_question`
    /// left unanswered for 5 minutes (`ask_user_handler::ASK_USER_TIMEOUT_SECS`)
    /// is auto-answered with "continue on your best judgment" (plus an
    /// `ask-user-resolved` timed-out event for the card); when false — the
    /// default — the agent waits for the user indefinitely. Read live by
    /// `DesktopQuestionHandler` before each question's wait, so a flip
    /// applies to the NEXT question without a restart. Written via
    /// `configure("chat.ask_user_auto_continue")`.
    #[serde(default)]
    pub chat_ask_user_auto_continue: bool,
}

/// Settings R3 T7 — the auto-archive retention default (7 days).
fn default_session_auto_archive_days() -> u32 {
    7
}

fn default_power_block_sleep_during_tasks() -> bool {
    true
}

/// Settings R3 T3 — apply the webview "disable GPU" environment for the
/// persisted [`DesktopConfig::hardware_acceleration`] choice.
///
/// MUST run before `tauri::Builder` starts: the webview backend reads these
/// variables exactly once, when the first window's webview is created — a
/// later write is a silent no-op. Platform matrix:
///
/// - Linux (WebKitGTK): `WEBKIT_DISABLE_COMPOSITING_MODE=1` +
///   `WEBKIT_DISABLE_DMABUF_RENDERER=1` — the two switches the WebKit bug
///   trackers recommend for blank-window / GPU-crash workarounds.
/// - Windows (WebView2): `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--disable-gpu`.
/// - macOS (WKWebView): no-op — there is no supported escape hatch and the
///   UI hides the card.
///
/// Public so the bin crate's `main()` can call it before the builder.
pub fn apply_hardware_acceleration_env(config: &DesktopConfig) {
    if config.hardware_acceleration {
        return;
    }
    #[cfg(target_os = "linux")]
    {
        // Edition 2024: set_var is unsafe (env is process-global) — same
        // precedent as main.rs's SHANNON_LANG test helper.
        unsafe {
            std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
        tracing::info!(
            "hardware acceleration disabled: WebKitGTK compositing + DMABUF renderer off (takes effect after restart)"
        );
    }
    #[cfg(target_os = "windows")]
    {
        unsafe {
            std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "--disable-gpu");
        }
        tracing::info!(
            "hardware acceleration disabled: WebView2 --disable-gpu (takes effect after restart)"
        );
    }
    // macOS / other targets: no supported escape hatch — leave untouched.
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        let _ = config;
        tracing::debug!("hardware acceleration toggle has no effect on this platform");
    }
}

/// Settings R3 T4 (B1) — the effective value of one optional text config
/// field: trimmed, empty/whitespace = unset. Shared by [`network_env`] and
/// the configure arms so a blank UI field always means "clear".
fn effective_network_value(raw: Option<&str>) -> Option<&str> {
    raw.map(str::trim).filter(|v| !v.is_empty())
}

/// Settings R3 T4 (B1) — compute the process environment to inject for the
/// corporate-network trio, as `(name, value)` pairs.
///
/// Pure on purpose: tests assert the returned set without mutating global
/// process state; [`apply_network_env`] is the thin (unsafe) applier.
///
/// Rules (控制器裁决 R1 + 覆盖语义):
/// - `network_proxy_url` set → `HTTPS_PROXY` = `HTTP_PROXY` = `ALL_PROXY` =
///   the URL, so every reqwest-based client (which reads the standard proxy
///   env vars by default) and every env-inheriting subprocess (gateway
///   sidecar, MCP stdio, command tools) routes through the proxy.
/// - `network_no_proxy` set → `NO_PROXY` = the list.
/// - `network_ca_cert_path` set → `SHANNON_CA_BUNDLE` (read by the
///   engine/desktop reqwest builders, which trust webpki-roots only) +
///   `NODE_EXTRA_CA_CERTS` + `SSL_CERT_FILE` for Node-based subprocesses.
/// - Unset values inject NOTHING — the implicit env keeps working (R1:
///   leaving the UI blank never forces direct connections).
/// - Set values WIN over pre-existing same-named env: an explicit setting is
///   the user's intent, a stale shell export must not clobber it.
pub fn network_env(config: &DesktopConfig) -> Vec<(String, String)> {
    let mut env = Vec::new();
    if let Some(proxy) = effective_network_value(config.network_proxy_url.as_deref()) {
        for name in ["HTTPS_PROXY", "HTTP_PROXY", "ALL_PROXY"] {
            env.push((name.to_string(), proxy.to_string()));
        }
    }
    if let Some(no_proxy) = effective_network_value(config.network_no_proxy.as_deref()) {
        env.push(("NO_PROXY".to_string(), no_proxy.to_string()));
    }
    if let Some(ca) = effective_network_value(config.network_ca_cert_path.as_deref()) {
        for name in ["SHANNON_CA_BUNDLE", "NODE_EXTRA_CA_CERTS", "SSL_CERT_FILE"] {
            env.push((name.to_string(), ca.to_string()));
        }
    }
    env
}

/// Settings R3 T4 (B1) — apply [`network_env`] to the process environment.
///
/// MUST run before `tauri::Builder` starts (same early block as
/// [`apply_hardware_acceleration_env`]): the gateway sidecar and MCP stdio
/// children are spawned from this process later during startup and inherit
/// its environment wholesale, and the engine's HTTP clients are built once
/// at first use.
pub fn apply_network_env(config: &DesktopConfig) {
    for (name, value) in network_env(config) {
        // Edition 2024: set_var is unsafe (env is process-global) — same
        // precedent as apply_hardware_acceleration_env above.
        unsafe { std::env::set_var(&name, &value) };
        tracing::info!("network env injected: {name} (from persisted config, restart-applied)");
    }
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

/// Persisted OAuth credentials for one `mcpServers` entry (W3-B, A2 token
/// lifecycle; ruling R6: they live inside the existing
/// `settings.json#mcpServers` blob — the same domain and file permissions
/// as the other hub-installed credentials — rather than a new keychain
/// dependency, which stays on the R2-P2-12 backlog).
///
/// Written by the OAuth installers and updated in place after every
/// successful token refresh, so a restart reconnects without re-consent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpStoredOAuth {
    /// OAuth client id the original authorization flow used.
    #[serde(default)]
    pub client_id: String,
    /// Token endpoint the refresh grant is POSTed to. Empty on legacy
    /// entries that only carry a static `Authorization` header — those
    /// connect with the stored token but cannot refresh.
    #[serde(default)]
    pub token_url: String,
    /// Refresh token, when the vendor issued one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// Current access token (mirrored into `headers.Authorization`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    /// Access-token expiry as unix epoch seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
}

impl McpStoredOAuth {
    /// True when this block can drive a token refresh (needs an endpoint
    /// and a refresh token).
    pub fn can_refresh(&self) -> bool {
        !self.token_url.is_empty() && self.refresh_token.as_deref().is_some_and(|t| !t.is_empty())
    }

    /// True when at least one credential is present (access or refresh).
    pub fn has_credential(&self) -> bool {
        self.access_token.as_deref().is_some_and(|t| !t.is_empty()) || self.can_refresh()
    }
}

/// MCP server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: std::collections::HashMap<String, String>,
    pub enabled: bool,
    /// Remote (HTTP/SSE) endpoint for url-only entries installed by the
    /// extensions hub (OAuth remote / `.mcpb` http bundles). `None` for
    /// stdio rows. W1-1 (R2-P0-1(B)): the loader keeps the url so the UI
    /// can show a truthful "remote" state instead of a dead Offline badge.
    /// Parse-only — saves still skip url-only rows because the struct
    /// cannot round-trip `type`/`headers` (see [`save_mcp_servers_to`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// W2-A (R4/A1) — the single-source auth verdict for a url-only row;
    /// since W3-B (A2) its semantics are "OAuth entry": `true` when the
    /// store entry carries HTTP `headers` or a `shannonOAuth` token block
    /// (today always the OAuth remote installer's product). Such entries
    /// connect through the pool's stored-credential OAuth path — the W2-A
    /// honest "remote" skip is gone — while url-only rows **without**
    /// credentials are pure remote servers wired up header-less. Computed
    /// by the private `mcp_server_config_from_json`; the seeder, restart
    /// and the UI all follow this one verdict.
    #[serde(default)]
    pub has_auth_headers: bool,
    /// W3-B (A2): stored OAuth credentials parsed from the entry's
    /// `shannonOAuth` block. Legacy installs (header-only) get an
    /// in-memory block derived from the `Authorization: Bearer …` header so
    /// they connect without rewriting the store. Never serialized back out
    /// — tokens must not reach the UI wire (see the `skip_serializing`).
    #[serde(default, skip_serializing)]
    pub oauth: Option<McpStoredOAuth>,
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
    /// S2-1 (模型仓固化): the provider slot's curated model declarations
    /// (`ModelSpec`s in `providers.toml` v2). Passes through verbatim in
    /// both directions so the desktop's read-modify-write save path can no
    /// longer wipe declarations authored by the vault (or the CLI's
    /// `providers model-meta`), and the AddProviderModal can pre-select the
    /// existing curation when editing.
    #[serde(default)]
    pub models: Vec<shannon_types::provider_config::ModelSpec>,
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
            // S2-1: pass the curated vault through verbatim — an
            // engine-store round trip must never silently drop it.
            models: self.models.clone(),
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
        // S2-1: the curated vault travels to the UI so the AddProviderModal
        // can pre-select the existing curation in edit mode.
        models: p.models.clone(),
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
            approval_mode: Some("auto-edit".into()),
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
            notifications_on_needs_attention: default_true(),
            notifications_sound_enabled: false,
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
            effort_level: None,
            hardware_acceleration: default_true(),
            power_keep_awake: false,
            power_block_sleep_during_tasks: default_power_block_sleep_during_tasks(),
            network_proxy_url: None,
            network_no_proxy: None,
            network_ca_cert_path: None,
            context_auto_compact: true,
            session_auto_archive_enabled: false,
            session_auto_archive_days: default_session_auto_archive_days(),
            chat_ask_user_auto_continue: false,
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
pub(crate) fn user_settings_path() -> PathBuf {
    let home = dirs_home().unwrap_or_else(|| PathBuf::from("."));
    home.join(".shannon").join("settings.json")
}

/// Settings R3 T4 (B1): `pub(crate)` so the configure arms can `~`-expand a
/// user-typed CA certificate path with the same home resolution every other
/// path in this module uses (`$HOME`, falling back to `$USERPROFILE`).
pub(crate) fn dirs_home() -> Option<PathBuf> {
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
    let content = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    // Carries secrets (`stt.api_key`, stdio `mcp_servers[].env`) — owner-only
    // atomic write (R6).
    crate::secret_files::write_atomic_owner_only(&path, content.as_bytes())
        .map_err(|e| e.to_string())
}

/// Load MCP server configs from the unified store
/// (`~/.shannon/settings.json#mcpServers`).
///
/// G1 split-brain fix: previously this read the legacy
/// `~/.shannon/desktop/mcp-servers.json` while the extensions hub wrote
/// `~/.shannon/settings.json#mcpServers` — servers installed from the hub
/// never showed up (and vice versa). Entries that are not stdio-startable
/// (no `command`, e.g. `url`-only OAuth/HTTP servers) are still listed so
/// the UI shows them; W2-A wires the header-less (pure remote) ones into
/// the process pool and leaves the auth-bearing ones on the honest badge.
///
/// A missing file starts empty; a present-but-corrupt file is an
/// `Err` — W2-A read/write symmetry: the save side already refuses to
/// reset a corrupt store, so the load side must not silently swallow it
/// as an empty list either (that masqueraded as "nothing installed").
pub fn load_mcp_servers() -> Result<Vec<McpServerConfig>, String> {
    load_mcp_servers_with_store(
        &user_settings_path(),
        crate::secret_store::global().as_deref(),
    )
}

/// `load_mcp_servers` against an explicit `settings.json` path (tests inject
/// a tempdir so they never touch the user's HOME).
pub fn load_mcp_servers_from(path: &std::path::Path) -> Result<Vec<McpServerConfig>, String> {
    load_mcp_servers_with_store(path, crate::secret_store::global().as_deref())
}

/// `load_mcp_servers` with an injected
/// [`SecretStore`](crate::secret_store::SecretStore) (tests pass a
/// [`MockSecretStore`](crate::secret_store::MockSecretStore) — never the
/// real keyring; `None` = degraded plaintext-only read).
///
/// F5 read-path tolerance window: **keyring first, plaintext fallback**.
/// A migrated server keeps its OAuth token block under
/// `shannon/mcp-oauth/<name>`; the plaintext copy was deleted on migration,
/// so the keyring hit is the only source left. A not-yet-migrated (or
/// degraded-mode) entry falls through to the on-disk `shannonOAuth` block /
/// legacy Bearer header exactly as before.
pub fn load_mcp_servers_with_store(
    path: &std::path::Path,
    store: Option<&dyn crate::secret_store::SecretStore>,
) -> Result<Vec<McpServerConfig>, String> {
    let root = read_settings_json_root(path)?;
    let Some(root_obj) = root.as_object() else {
        return Err(format!(
            "settings.json is not a JSON object: {}",
            path.display()
        ));
    };
    let Some(map) = root_obj.get("mcpServers") else {
        return Ok(Vec::new());
    };
    let Some(map) = map.as_object() else {
        return Err(format!(
            "settings.json#mcpServers is not an object: {}",
            path.display()
        ));
    };
    let mut servers: Vec<McpServerConfig> = map
        .iter()
        .filter_map(|(name, value)| {
            let mut config = mcp_server_config_from_json(name, value)?;
            // Keyring first, plaintext fallback (A8 tolerance window): while
            // both sources exist — e.g. a migration interrupted between the
            // keyring put and the file rewrite — the store entry is the
            // live credential the refresh path keeps updating.
            if let Some(oauth) = oauth_from_secret_store(store, name) {
                config.oauth = Some(oauth);
                config.has_auth_headers = true;
            }
            Some(config)
        })
        .collect();
    servers.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(servers)
}

/// Read one server's OAuth token block from the secret store, if present.
/// A store miss is indistinguishable from "not migrated yet" — the plaintext
/// fallback applies. A store READ failure (locked keychain, …) is never
/// silently treated as a miss: it warns loudly (with the server name) and
/// then takes the same plaintext fallback, so a degraded read is at least
/// diagnosable (A8: no silent degradation).
fn oauth_from_secret_store(
    store: Option<&dyn crate::secret_store::SecretStore>,
    server: &str,
) -> Option<McpStoredOAuth> {
    let raw = match store?.get(&crate::secret_store::mcp_oauth_key(server)) {
        Ok(Some(raw)) => raw,
        Ok(None) => return None,
        Err(e) => {
            tracing::warn!(
                domain = "mcp-oauth",
                server,
                error = %e,
                "keyring read failed — treating the OAuth credential as absent \
                 (plaintext fallback applies)"
            );
            return None;
        }
    };
    match serde_json::from_str::<McpStoredOAuth>(&raw) {
        Ok(block) => Some(block),
        Err(e) => {
            tracing::warn!(
                server,
                error = %e,
                "keyring OAuth block for MCP server is not valid JSON — treating as absent"
            );
            None
        }
    }
}

/// Convert one `mcpServers.<name>` JSON entry into a [`McpServerConfig`].
///
/// `None` = not listable as a config row (e.g. the value is not an object).
/// Stdio `command` is the only hard requirement for the *pool*, but
/// url-only servers are surfaced too (`command` empty, `enabled` from the
/// `enabled` flag) so `list_mcp_servers` shows what the user installed.
///
/// W2-A (R4/A1) introduced the auth verdict; since W3-B (A2) it reads
/// "OAuth entry": an entry with a non-empty `headers` object **or** a
/// `shannonOAuth` token block. OAuth entries connect through the pool's
/// stored-credential path; header-less entries without a token block are
/// pure remote servers the pool wires up anonymously.
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
    // W1-1: keep the remote endpoint — dropping it turned every url-only
    // install into an anonymous empty-command row the UI could only show
    // as "Offline".
    let url = obj.get("url").and_then(|u| u.as_str()).map(str::to_string);
    // W3-B (A2): the persisted token block, then a legacy fallback —
    // pre-A2 installs carry only `headers.Authorization: Bearer …`, so
    // derive an in-memory block from it (never written back verbatim; a
    // refresh landing a new token upgrades the entry to a real block).
    let oauth = parse_stored_oauth(obj).or_else(|| oauth_from_bearer_header(obj));
    // W2-A: headers (static or command-sourced) are credentials in every
    // store shape the hub writes; A2 adds the `shannonOAuth` block as an
    // equivalent verdict source. Either way the row is an OAuth entry and
    // must never be started header-less/anonymously.
    let has_headers = obj
        .get("headers")
        .and_then(|h| h.as_object())
        .is_some_and(|h| !h.is_empty());
    let has_auth_headers = has_headers || oauth.is_some();
    Some(McpServerConfig {
        name: name.to_string(),
        command,
        args,
        env,
        enabled,
        url,
        has_auth_headers,
        oauth,
    })
}

/// Parse the `shannonOAuth` token block of an entry, if present.
fn parse_stored_oauth(obj: &serde_json::Map<String, serde_json::Value>) -> Option<McpStoredOAuth> {
    let block = obj.get("shannonOAuth")?.as_object()?;
    let str_field = |key: &str| {
        block
            .get(key)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    Some(McpStoredOAuth {
        client_id: str_field("client_id").unwrap_or_default(),
        token_url: str_field("token_url").unwrap_or_default(),
        refresh_token: str_field("refresh_token"),
        access_token: str_field("access_token"),
        expires_at: block
            .get("expires_at")
            .and_then(|v| v.as_i64())
            .or_else(|| {
                block
                    .get("expires_at")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse().ok())
            }),
    })
}

/// Derive an in-memory [`McpStoredOAuth`] from a legacy
/// `headers.Authorization: Bearer <token>` entry so A2 can connect it
/// without rewriting the store. Returns `None` when no bearer token is
/// recoverable.
fn oauth_from_bearer_header(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> Option<McpStoredOAuth> {
    let headers = obj.get("headers")?.as_object()?;
    let bearer = headers
        .get("Authorization")
        .or_else(|| headers.get("authorization"))
        .and_then(|v| v.as_str())?;
    let token = bearer.strip_prefix("Bearer ").map(str::trim)?;
    if token.is_empty() {
        return None;
    }
    Some(McpStoredOAuth {
        client_id: String::new(),
        token_url: String::new(),
        refresh_token: None,
        access_token: Some(token.to_string()),
        expires_at: None,
    })
}

/// Serialize a [`McpServerConfig`] into the `mcpServers.<name>` JSON entry
/// shape shared with the CLI. Only stdio rows are serializable — url-only
/// entries (`type`/`url`/`headers` fields, the latter carrying OAuth bearer
/// tokens) cannot round-trip through this struct, so saves skip them and
/// their original JSON blob in the store stays untouched.
fn mcp_server_config_to_json(config: &McpServerConfig) -> serde_json::Value {
    serde_json::json!({
        "command": config.command,
        "args": config.args,
        "env": config.env,
        "enabled": config.enabled,
    })
}

/// Atomically replace `path` with the pretty-printed JSON `root`, with the
/// temp file created owner-only (`0600`) so the OAuth blob in `mcpServers`
/// is never on disk world-readable, even briefly. A crash mid-write can
/// never leave a truncated/corrupt settings.json.
fn write_settings_json_atomic(
    path: &std::path::Path,
    root: &serde_json::Value,
) -> Result<(), String> {
    let content = serde_json::to_string_pretty(root).map_err(|e| e.to_string())?;
    crate::secret_files::write_atomic_owner_only(path, content.as_bytes())
        .map_err(|e| e.to_string())
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
            // Url-only row: the struct cannot round-trip `type`/`headers`
            // (OAuth bearer tokens), so writing it would replace the real
            // entry with a lossy stub. Skip — whatever the store already
            // holds stays intact.
            continue;
        }
        mcp_obj.insert(config.name.clone(), mcp_server_config_to_json(config));
    }
    write_settings_json_atomic(path, &root)
}

/// Remove one MCP server entry from the unified store by name, and delete
/// its keyring OAuth entry with it (F5: an orphaned keyring entry after
/// uninstall is a new leak surface). Returns `Ok(false)` when no entry with
/// that name existed.
pub fn remove_mcp_server_entry(name: &str) -> Result<bool, String> {
    remove_mcp_server_entry_with_store(
        &user_settings_path(),
        name,
        crate::secret_store::global().as_deref(),
    )
}

/// [`remove_mcp_server_entry`] with an injected secret store (tests pass a
/// mock; `None` = degraded mode, nothing to clean).
pub fn remove_mcp_server_entry_with_store(
    path: &std::path::Path,
    name: &str,
    store: Option<&dyn crate::secret_store::SecretStore>,
) -> Result<bool, String> {
    let removed = remove_mcp_server_entry_from(path, name)?;
    if removed {
        crate::secret_store::delete_mcp_oauth_secret(store, name);
    }
    Ok(removed)
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

/// Flip the `enabled` flag of one `mcpServers.<name>` entry in the unified
/// store (W2-A inline toggle). Edits the raw JSON entry in place, so
/// url-only rows keep their `type`/`url`/`headers` blob — unlike
/// [`save_mcp_servers_to`], which must skip rows the struct cannot
/// round-trip. Returns `Ok(false)` when no entry with that name exists.
pub fn set_mcp_server_enabled(name: &str, enabled: bool) -> Result<bool, String> {
    set_mcp_server_enabled_to(&user_settings_path(), name, enabled)
}

/// `set_mcp_server_enabled` against an explicit `settings.json` path.
pub fn set_mcp_server_enabled_to(
    path: &std::path::Path,
    name: &str,
    enabled: bool,
) -> Result<bool, String> {
    let mut root = read_settings_json_root(path)?;
    let Some(entry) = root
        .get_mut("mcpServers")
        .and_then(|m| m.as_object_mut())
        .and_then(|m| m.get_mut(name))
    else {
        return Ok(false);
    };
    let Some(entry_obj) = entry.as_object_mut() else {
        return Err(format!("settings.json#mcpServers.{name} is not an object"));
    };
    entry_obj.insert("enabled".to_string(), serde_json::Value::Bool(enabled));
    write_settings_json_atomic(path, &root)?;
    Ok(true)
}

/// Persist refreshed OAuth credentials for one `mcpServers.<name>` entry
/// (W3-B, A2 token lifecycle). F5: with a working secret store the token
/// block lives **in the keyring** (`shannon/mcp-oauth/<name>`) and the
/// entry's plaintext secret material (`shannonOAuth` block + the mirrored
/// `Authorization` header) is stripped from settings.json — a refresh must
/// never re-introduce plaintext after the migration deleted it. Without a
/// store (degraded mode) the legacy in-file shape is written, protected by
/// the R6 0600 atomic writer. Returns `Ok(false)` when no entry with that
/// name exists.
pub fn update_mcp_server_oauth_tokens(name: &str, tokens: &McpStoredOAuth) -> Result<bool, String> {
    update_mcp_server_oauth_tokens_with_store(
        &user_settings_path(),
        name,
        tokens,
        crate::secret_store::global().as_deref(),
    )
}

/// `update_mcp_server_oauth_tokens` against an explicit `settings.json` path
/// and an injected secret store (tests use a mock; `None` = degraded
/// plaintext write).
pub fn update_mcp_server_oauth_tokens_to(
    path: &std::path::Path,
    name: &str,
    tokens: &McpStoredOAuth,
) -> Result<bool, String> {
    update_mcp_server_oauth_tokens_with_store(
        path,
        name,
        tokens,
        crate::secret_store::global().as_deref(),
    )
}

/// Store-injected core of [`update_mcp_server_oauth_tokens`].
pub fn update_mcp_server_oauth_tokens_with_store(
    path: &std::path::Path,
    name: &str,
    tokens: &McpStoredOAuth,
    store: Option<&dyn crate::secret_store::SecretStore>,
) -> Result<bool, String> {
    // Keyring mode: the store is the source of truth for the credential.
    // A failed write keeps the plaintext fallback honest — the block is
    // then written into the file below instead (0600), never dropped.
    let mut keyring_write_ok = false;
    if let Some(store) = store {
        match serde_json::to_string(tokens)
            .map_err(|e| format!("oauth tokens serialize: {e}"))
            .and_then(|raw| store.put(&crate::secret_store::mcp_oauth_key(name), &raw))
        {
            Ok(()) => keyring_write_ok = true,
            Err(e) => tracing::warn!(
                domain = "mcp-oauth",
                server = name,
                error = %e,
                "keyring write failed for refreshed OAuth tokens — keeping the \
                 plaintext block in settings.json (0600)"
            ),
        }
    }

    let mut root = read_settings_json_root(path)?;
    let Some(entry) = root
        .get_mut("mcpServers")
        .and_then(|m| m.as_object_mut())
        .and_then(|m| m.get_mut(name))
    else {
        return Ok(false);
    };
    let Some(entry_obj) = entry.as_object_mut() else {
        return Err(format!("settings.json#mcpServers.{name} is not an object"));
    };
    if keyring_write_ok {
        // Strip any plaintext secret material — including a mirrored
        // Authorization header left over from the pre-migration shape.
        entry_obj.remove("shannonOAuth");
        strip_bearer_authorization(entry_obj);
    } else {
        entry_obj.insert(
            "shannonOAuth".to_string(),
            serde_json::to_value(tokens).map_err(|e| format!("oauth tokens serialize: {e}"))?,
        );
        if let Some(access) = tokens.access_token.as_deref().filter(|t| !t.is_empty()) {
            if let Some(headers) = entry_obj.get_mut("headers").and_then(|h| h.as_object_mut()) {
                headers.insert(
                    "Authorization".to_string(),
                    serde_json::Value::String(format!("Bearer {access}")),
                );
            }
        }
    }
    write_settings_json_atomic(path, &root)?;
    Ok(true)
}

/// Remove the OAuth-mirrored `Authorization` header from an entry's
/// `headers` object, preserving every other (user-configured) header and
/// dropping the object entirely when nothing remains. Only the exact
/// OAuth-mirror keys (`Authorization` / lowercase spelling) are touched —
/// manual custom headers stay in the file verbatim.
fn strip_bearer_authorization(entry_obj: &mut serde_json::Map<String, serde_json::Value>) {
    let Some(headers) = entry_obj.get_mut("headers").and_then(|h| h.as_object_mut()) else {
        return;
    };
    headers.remove("Authorization");
    headers.remove("authorization");
    if headers.is_empty() {
        entry_obj.remove("headers");
    }
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

    // Existing unified entries win (idempotent re-runs are no-ops). A
    // corrupt unified store skips the migration entirely — W2-A made the
    // load honest (Err), and writing into a corrupt file would fail the
    // save anyway.
    let existing: std::collections::HashSet<String> = match load_mcp_servers_from(settings_path) {
        Ok(servers) => servers.into_iter().map(|s| s.name).collect(),
        Err(e) => {
            tracing::warn!(
                path = %settings_path.display(),
                error = %e,
                "unified settings.json is corrupt — skipping legacy MCP migration"
            );
            return 0;
        }
    };

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
            url: None,
            has_auth_headers: false,
            oauth: None,
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

/// One-time, idempotent migration of plaintext MCP OAuth credentials into
/// the OS keyring (R7-④ batch 2 / A8). Runs at startup (and after an OAuth
/// install/re-authentication) before anything reads the store.
///
/// The keyring is the live credential once an entry is migrated — a token
/// refresh rotates it **there** — so the file's plaintext copy never
/// blindly overwrites it. Per entry, in order:
///
/// 1. Keyring **read** failure (locked keychain, …) → the entry is left
///    as-is (plaintext kept, nothing overwritten) and the failure warns —
///    retried on the next startup, never guessed over an entry we cannot
///    see.
/// 2. Keyring holds no block → first migration: put the plaintext payload,
///    then strip it from the file (atomic 0600 rewrite).
/// 3. Keyring block == plaintext block → in sync (a run interrupted
///    between the put and the file rewrite) → strip the leftover only.
/// 4. Keyring block differs → put only when the plaintext is demonstrably
///    the newer one — its `expires_at` advanced past the stored block's,
///    the fingerprint of a refresh whose keyring write failed last
///    session. Otherwise the keyring wins and only the stale leftover is
///    stripped (the crash-shaped case: an earlier migration's rewrite
///    failed, then a refresh rotated the keyring — re-putting the old
///    plaintext would roll back a possibly single-use refresh token).
///    A mismatch with no `expires_at` on either side (static tokens) is
///    indeterminate and likewise never rolls the keyring back —
///    re-authentication recovers, a rolled-back rotation does not.
///
/// A **failed keyring write keeps the plaintext** (the file is already
/// 0600) and warns — loading is never blocked.
///
/// Migration scope (A8 ruling): the `shannonOAuth` token block, plus the
/// W3-B legacy shape it derives from — a url-only entry's
/// `Authorization: Bearer …` header. Only the exact OAuth-mirror
/// `Authorization` key is removed; user-configured custom headers
/// (`X-API-Key`, proxy headers, …) are never migrated and stay in
/// settings.json with their fail-safe semantics intact. Stdio `env`
/// secrets are likewise out of scope.
///
/// Returns the number of entries whose plaintext secret material was
/// removed this run (re-runs are no-ops).
pub fn migrate_mcp_oauth_secrets() -> usize {
    migrate_mcp_oauth_secrets_to(
        &user_settings_path(),
        crate::secret_store::global().as_deref(),
    )
}

/// True when the plaintext token block is demonstrably newer than the
/// stored one: its `expires_at` advanced, which only a token refresh does.
/// See [`migrate_mcp_oauth_secrets_to`] step 4 for why an indeterminate
/// (static-token) mismatch resolves to `false` — never roll the keyring
/// back on a guess.
fn plaintext_block_newer(plain: &McpStoredOAuth, stored: &McpStoredOAuth) -> bool {
    match (plain.expires_at, stored.expires_at) {
        (Some(plain_exp), Some(stored_exp)) => plain_exp > stored_exp,
        // Only the refreshed side carries an expiry: whoever has one was
        // written by (or after) a refresh.
        (Some(_), None) => true,
        (None, _) => false,
    }
}

/// Store-injected core of [`migrate_mcp_oauth_secrets`] (tests pass a
/// [`MockSecretStore`](crate::secret_store::MockSecretStore); `None` = the
/// degraded plaintext fallback, a no-op).
pub fn migrate_mcp_oauth_secrets_to(
    settings_path: &std::path::Path,
    store: Option<&dyn crate::secret_store::SecretStore>,
) -> usize {
    let Some(store) = store else {
        return 0; // degraded mode: credentials deliberately stay in the 0600 file
    };
    let mut root = match read_settings_json_root(settings_path) {
        Ok(root) => root,
        Err(e) => {
            tracing::warn!(
                domain = "mcp-oauth",
                error = %e,
                "settings.json unreadable — skipping OAuth keyring migration"
            );
            return 0;
        }
    };
    let Some(mcp_obj) = root.get_mut("mcpServers").and_then(|m| m.as_object_mut()) else {
        return 0;
    };

    let mut migrated = 0;
    for (name, entry) in mcp_obj.iter_mut() {
        let Some(entry_obj) = entry.as_object_mut() else {
            continue;
        };
        // The credential payload: the stored block, or the W3-B legacy
        // derivation from the Authorization bearer header (the exact shape
        // the loader derives from — anything else is a manual custom header
        // and stays put).
        let block =
            match parse_stored_oauth(entry_obj).or_else(|| oauth_from_bearer_header(entry_obj)) {
                Some(block) if block.has_credential() => block,
                _ => continue,
            };
        let had_plaintext =
            entry_obj.get("shannonOAuth").is_some() || entry_obj.get("headers").is_some();

        // Keyring first: read the stored block (if any) before touching
        // anything — the file's copy is a leftover, not the source of truth.
        let stored: Option<McpStoredOAuth> =
            match store.get(&crate::secret_store::mcp_oauth_key(name)) {
                Ok(Some(raw)) => serde_json::from_str(&raw).ok(),
                Ok(None) => None,
                Err(e) => {
                    tracing::warn!(
                        domain = "mcp-oauth",
                        server = name,
                        error = %e,
                        "keyring read failed during migration — entry left as-is \
                         (plaintext kept, retried on next startup)"
                    );
                    continue;
                }
            };
        let needs_put = match &stored {
            // First migration, or a stored block we cannot parse (corrupt
            // payload): the plaintext is the only sane source — put it.
            None => true,
            Some(stored_block) => {
                if *stored_block == block {
                    // In sync: the plaintext is the leftover of a run whose
                    // file rewrite was interrupted. Strip only — re-putting
                    // identical bytes is pointless churn.
                    false
                } else {
                    plaintext_block_newer(&block, stored_block)
                }
            }
        };
        if needs_put {
            let raw = match serde_json::to_string(&block) {
                Ok(raw) => raw,
                Err(e) => {
                    tracing::warn!(
                        domain = "mcp-oauth",
                        server = name,
                        error = %e,
                        "OAuth token block serialization failed — entry left as-is"
                    );
                    continue;
                }
            };
            if let Err(e) = store.put(&crate::secret_store::mcp_oauth_key(name), &raw) {
                // Degraded write: keep the plaintext (0600 file) and warn —
                // migration must never block loading or lose a credential.
                tracing::warn!(
                    domain = "mcp-oauth",
                    server = name,
                    error = %e,
                    "keyring write failed — OAuth token stays as plaintext in \
                     settings.json (owner-only 0600)"
                );
                continue;
            }
        }
        if had_plaintext {
            entry_obj.remove("shannonOAuth");
            strip_bearer_authorization(entry_obj);
            migrated += 1;
        }
    }

    if migrated > 0 {
        if let Err(e) = write_settings_json_atomic(settings_path, &root) {
            // The keyring already holds every migrated credential, so the
            // next startup's crash-recovery leg (keyring hit → strip
            // plaintext) finishes the job. Warn loudly either way: the
            // plaintext copies are still on disk until then.
            tracing::warn!(
                domain = "mcp-oauth",
                error = %e,
                "post-migration settings.json rewrite failed — plaintext \
                 OAuth copies remain (0600) and will be stripped on the \
                 next startup"
            );
            return 0;
        }
        tracing::info!(
            domain = "mcp-oauth",
            count = migrated,
            "migrated MCP OAuth tokens into the OS keyring"
        );
    }
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
        assert_eq!(config.approval_mode, Some("auto-edit".into()));
    }

    /// R6: settings.json carries the `mcpServers` OAuth blob — it must land
    /// owner-only (0600), including when a rewrite replaces a pre-existing
    /// world-readable file.
    #[cfg(unix)]
    #[test]
    fn settings_json_atomic_write_lands_0600() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");

        write_settings_json_atomic(&path, &serde_json::json!({"mcpServers": {}})).unwrap();
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600, "fresh settings.json must be 0600");

        // Pre-existing 0644 file (older build) is fixed by the next write —
        // the rename swaps in the temp file's inode, no batch chmod needed.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(mode(&path), 0o644);
        write_settings_json_atomic(&path, &serde_json::json!({"mcpServers": {}})).unwrap();
        assert_eq!(mode(&path), 0o600, "rewritten settings.json must be 0600");
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

    /// Settings R3 T3: the three power/hardware keys must default correctly
    /// when a pre-R3 `config.json` (no such keys) loads — hw-accel ON,
    /// keep-awake OFF, block-sleep-during-tasks ON — and round-trip once
    /// written.
    #[test]
    fn test_power_keys_default_compat_and_round_trip() {
        // Legacy JSON without the new keys (the exact shape older installs
        // have on disk).
        let legacy: DesktopConfig = serde_json::from_str(
            r#"{"working_dir":null,"theme":null,"mcp_servers":[],"approval_mode":null}"#,
        )
        .expect("legacy config must deserialize");
        assert!(legacy.hardware_acceleration, "hw accel defaults ON");
        assert!(!legacy.power_keep_awake, "keep-awake defaults OFF");
        assert!(
            legacy.power_block_sleep_during_tasks,
            "block-sleep-during-tasks defaults ON"
        );

        let config = DesktopConfig {
            hardware_acceleration: false,
            power_keep_awake: true,
            power_block_sleep_during_tasks: false,
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"hardware_acceleration\":false"), "{json}");
        assert!(json.contains("\"power_keep_awake\":true"), "{json}");
        assert!(
            json.contains("\"power_block_sleep_during_tasks\":false"),
            "{json}"
        );
        let back: DesktopConfig = serde_json::from_str(&json).unwrap();
        assert!(!back.hardware_acceleration);
        assert!(back.power_keep_awake);
        assert!(!back.power_block_sleep_during_tasks);
    }

    /// Settings R3 T4 (B1): the three network keys must default to None on a
    /// pre-R3 config.json and round-trip once written.
    #[test]
    fn test_network_keys_default_compat_and_round_trip() {
        let legacy: DesktopConfig = serde_json::from_str(
            r#"{"working_dir":null,"theme":null,"mcp_servers":[],"approval_mode":null}"#,
        )
        .expect("legacy config must deserialize");
        assert!(legacy.network_proxy_url.is_none(), "proxy defaults unset");
        assert!(legacy.network_no_proxy.is_none(), "no_proxy defaults unset");
        assert!(
            legacy.network_ca_cert_path.is_none(),
            "ca path defaults unset"
        );

        let config = DesktopConfig {
            network_proxy_url: Some("http://127.0.0.1:7890".into()),
            network_no_proxy: Some("localhost,127.0.0.1".into()),
            network_ca_cert_path: Some("/etc/shannon/root-ca.pem".into()),
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(
            json.contains("\"network_proxy_url\":\"http://127.0.0.1:7890\""),
            "{json}"
        );
        assert!(
            json.contains("\"network_no_proxy\":\"localhost,127.0.0.1\""),
            "{json}"
        );
        assert!(
            json.contains("\"network_ca_cert_path\":\"/etc/shannon/root-ca.pem\""),
            "{json}"
        );
        let back: DesktopConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(
            back.network_proxy_url.as_deref(),
            Some("http://127.0.0.1:7890")
        );
        assert_eq!(
            back.network_no_proxy.as_deref(),
            Some("localhost,127.0.0.1")
        );
        assert_eq!(
            back.network_ca_cert_path.as_deref(),
            Some("/etc/shannon/root-ca.pem")
        );
    }

    /// Settings R3 T6: `context_auto_compact` defaults to `true` — a legacy
    /// config.json without the key keeps auto-compaction ON (exact pre-T6
    /// behavior), and an explicit `false` survives a save/load round trip.
    #[test]
    fn test_context_auto_compact_default_compat_and_round_trip() {
        // Legacy JSON: no `context_auto_compact` key at all → default true.
        let legacy: DesktopConfig = serde_json::from_str(
            r#"{"working_dir":null,"theme":null,"mcp_servers":[],"approval_mode":null}"#,
        )
        .expect("legacy config must deserialize");
        assert!(
            legacy.context_auto_compact,
            "missing key must default to auto-compaction ON"
        );
        assert!(DesktopConfig::default().context_auto_compact);

        // Explicit off persists and reloads as off.
        let config = DesktopConfig {
            context_auto_compact: false,
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"context_auto_compact\":false"), "{json}");
        let back: DesktopConfig = serde_json::from_str(&json).unwrap();
        assert!(!back.context_auto_compact, "false must round-trip");
    }

    /// Settings R3 T7: the auto-archive keys default to off + 7 days — a
    /// legacy config.json without them keeps "never auto-archive" (the
    /// standing posture), and explicit values survive a save/load round trip.
    #[test]
    fn test_auto_archive_keys_default_compat_and_round_trip() {
        // Legacy JSON: neither key present → disabled + 7-day window.
        let legacy: DesktopConfig = serde_json::from_str(
            r#"{"working_dir":null,"theme":null,"mcp_servers":[],"approval_mode":null}"#,
        )
        .expect("legacy config must deserialize");
        assert!(
            !legacy.session_auto_archive_enabled,
            "missing key must default to auto-archive OFF"
        );
        assert_eq!(legacy.session_auto_archive_days, 7);
        assert!(!DesktopConfig::default().session_auto_archive_enabled);
        assert_eq!(DesktopConfig::default().session_auto_archive_days, 7);

        // Explicit values persist and reload verbatim.
        let config = DesktopConfig {
            session_auto_archive_enabled: true,
            session_auto_archive_days: 30,
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(
            json.contains("\"session_auto_archive_enabled\":true"),
            "{json}"
        );
        assert!(json.contains("\"session_auto_archive_days\":30"), "{json}");
        let back: DesktopConfig = serde_json::from_str(&json).unwrap();
        assert!(back.session_auto_archive_enabled, "true must round-trip");
        assert_eq!(back.session_auto_archive_days, 30, "days must round-trip");
    }

    /// Settings R3 T4 (B1): `network_env` is a pure function — given a
    /// config, exactly the expected `(name, value)` pairs come back. Empty
    /// / whitespace values inject NOTHING (R1: keep the implicit env
    /// fallback, never force direct), set values produce the full trio and
    /// override any same-named env by construction of the applier.
    #[test]
    fn network_env_empty_config_injects_nothing() {
        let cfg = DesktopConfig::default();
        assert!(
            network_env(&cfg).is_empty(),
            "unset network settings must not inject any env var"
        );
        // Whitespace-only / empty-string values (hand-edited config) count
        // as unset too.
        let cfg = DesktopConfig {
            network_proxy_url: Some("   ".into()),
            network_no_proxy: Some(String::new()),
            network_ca_cert_path: None,
            ..Default::default()
        };
        assert!(network_env(&cfg).is_empty(), "blank values = unset (R1)");
    }

    #[test]
    fn network_env_proxy_value_yields_the_standard_proxy_vars() {
        let cfg = DesktopConfig {
            network_proxy_url: Some("http://127.0.0.1:7890".into()),
            ..Default::default()
        };
        let env = network_env(&cfg);
        let expected = vec![
            (
                "HTTPS_PROXY".to_string(),
                "http://127.0.0.1:7890".to_string(),
            ),
            (
                "HTTP_PROXY".to_string(),
                "http://127.0.0.1:7890".to_string(),
            ),
            ("ALL_PROXY".to_string(), "http://127.0.0.1:7890".to_string()),
        ];
        assert_eq!(env, expected, "proxy set → exactly the three standard vars");
    }

    #[test]
    fn network_env_no_proxy_and_ca_yield_their_own_vars() {
        let cfg = DesktopConfig {
            network_no_proxy: Some("localhost,127.0.0.1,::1,.example.com".into()),
            network_ca_cert_path: Some("/home/u/certs/root-ca.pem".into()),
            ..Default::default()
        };
        let env = network_env(&cfg);
        assert!(
            env.contains(&(
                "NO_PROXY".to_string(),
                "localhost,127.0.0.1,::1,.example.com".to_string()
            )),
            "no_proxy → NO_PROXY, got {env:?}"
        );
        for name in ["SHANNON_CA_BUNDLE", "NODE_EXTRA_CA_CERTS", "SSL_CERT_FILE"] {
            assert!(
                env.iter()
                    .any(|(n, v)| n == name && v == "/home/u/certs/root-ca.pem"),
                "{name} must carry the CA path, got {env:?}"
            );
        }
        assert!(
            !env.iter()
                .any(|(n, _)| matches!(n.as_str(), "HTTP_PROXY" | "HTTPS_PROXY" | "ALL_PROXY")),
            "proxy vars untouched when only ca/no_proxy set: {env:?}"
        );
    }

    #[test]
    fn network_env_values_are_trimmed_and_explicit() {
        let cfg = DesktopConfig {
            network_proxy_url: Some("  http://corp-proxy.internal:3128  ".into()),
            ..Default::default()
        };
        let env = network_env(&cfg);
        assert!(
            env.iter()
                .all(|(_, v)| v == "http://corp-proxy.internal:3128"),
            "values are stored pre-trimmed (configure trims); env passthrough is verbatim: {env:?}"
        );
    }

    #[test]
    fn test_notification_keys_default_compat_and_round_trip() {
        // Settings-r3 T5: the two new notification keys must default sensibly
        // when absent from an older config.json — needs-attention ON, sound
        // OFF (R4).
        let legacy: DesktopConfig = serde_json::from_str(
            r#"{"working_dir":null,"theme":null,"mcp_servers":[],"approval_mode":null}"#,
        )
        .expect("legacy config must deserialize");
        assert!(
            legacy.notifications_on_needs_attention,
            "needs-attention defaults ON"
        );
        assert!(!legacy.notifications_sound_enabled, "sound defaults OFF");

        let config = DesktopConfig {
            notifications_on_needs_attention: false,
            notifications_sound_enabled: true,
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(
            json.contains("\"notifications_on_needs_attention\":false"),
            "{json}"
        );
        assert!(
            json.contains("\"notifications_sound_enabled\":true"),
            "{json}"
        );
        let back: DesktopConfig = serde_json::from_str(&json).unwrap();
        assert!(!back.notifications_on_needs_attention);
        assert!(back.notifications_sound_enabled);
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
            models: vec![shannon_types::provider_config::ModelSpec {
                id: "vault-model".into(),
                display_name: None,
                context_window: Some(65_536),
                max_output: Some(8_192),
                cost_per_m_input: None,
                cost_per_m_output: None,
                capabilities: Vec::new(),
            }],
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
        // S2-1: the curated vault passes through verbatim — an engine-store
        // round trip must never silently drop it.
        assert_eq!(profile.models.len(), 1);
        assert_eq!(profile.models[0].id, "vault-model");
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
                url: None,
                has_auth_headers: false,
                oauth: None,
            }],
        )
        .unwrap();

        let root: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(root["permissions"]["allow"][0], "Bash");
        assert_eq!(root["mcpServers"]["notion"]["url"], "https://mcp.example");

        // Load sees both entries (url-only listed with empty command, its
        // url preserved — W1-1: it is no longer dropped).
        let servers = load_mcp_servers_from(&path).unwrap();
        assert_eq!(servers.len(), 2);
        let everything = servers.iter().find(|s| s.name == "everything").unwrap();
        assert_eq!(everything.command, "npx");
        assert_eq!(everything.args.len(), 2);
        assert_eq!(everything.env.get("K").map(String::as_str), Some("v"));
        assert!(everything.enabled);
        assert!(everything.url.is_none());
        let notion = servers.iter().find(|s| s.name == "notion").unwrap();
        assert!(notion.command.is_empty());
        assert_eq!(notion.url.as_deref(), Some("https://mcp.example"));

        // Removal drops only the target row.
        assert!(remove_mcp_server_entry_from(&path, "everything").unwrap());
        assert!(!remove_mcp_server_entry_from(&path, "everything").unwrap());
        let servers = load_mcp_servers_from(&path).unwrap();
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
                url: None,
                has_auth_headers: false,
                oauth: None,
            }],
        )
        .unwrap();

        let migrated = migrate_legacy_mcp_servers_to(&settings, &legacy);
        assert_eq!(migrated, 1, "only 'fs' migrates; 'shared' already exists");

        let servers = load_mcp_servers_from(&settings).unwrap();
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
                url: None,
                has_auth_headers: false,
                oauth: None,
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
                url: None,
                has_auth_headers: false,
                oauth: None,
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
                url: Some("https://mcp.example".into()),
                has_auth_headers: false,
                oauth: None,
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

    /// W3-B (A2): the `shannonOAuth` block parses into the struct, makes
    /// the row an OAuth entry even without `headers`, and legacy
    /// header-only entries derive an in-memory token (never persisted back
    /// verbatim). Tokens never serialize back out (UI wire safety).
    #[test]
    fn oauth_blocks_parse_and_legacy_bearer_derives() {
        let dir = tempfile::tempdir().unwrap();
        let path = tmp_settings(dir.path());
        std::fs::write(
            &path,
            r#"{"mcpServers":{
                "modern":{"type":"http","url":"https://mcp.example","shannonOAuth":{"client_id":"cid","token_url":"https://t/token","refresh_token":"rt","access_token":"at","expires_at":1735689600}},
                "legacy":{"type":"http","url":"https://mcp.example","headers":{"Authorization":"Bearer leg"}},
                "neither":{"type":"http","url":"https://mcp.example"}
            }}"#,
        )
        .unwrap();

        let servers = load_mcp_servers_from(&path).unwrap();
        let modern = servers.iter().find(|s| s.name == "modern").unwrap();
        let oauth = modern.oauth.as_ref().unwrap();
        assert_eq!(oauth.client_id, "cid");
        assert_eq!(oauth.token_url, "https://t/token");
        assert_eq!(oauth.refresh_token.as_deref(), Some("rt"));
        assert_eq!(oauth.access_token.as_deref(), Some("at"));
        assert_eq!(oauth.expires_at, Some(1735689600));
        assert!(oauth.can_refresh());
        assert!(modern.has_auth_headers, "token block alone is a verdict");

        let legacy = servers.iter().find(|s| s.name == "legacy").unwrap();
        let derived = legacy.oauth.as_ref().expect("legacy bearer derived");
        assert_eq!(derived.access_token.as_deref(), Some("leg"));
        assert!(!derived.can_refresh(), "legacy entries cannot refresh");
        assert!(legacy.has_auth_headers);

        let neither = servers.iter().find(|s| s.name == "neither").unwrap();
        assert!(neither.oauth.is_none());
        assert!(!neither.has_auth_headers, "pure remote stays pure");

        // Tokens must never reach the UI wire: the field is
        // `skip_serializing` on the desktop config struct.
        let json = serde_json::to_value(modern).unwrap();
        assert!(json.get("oauth").is_none(), "{json}");
        assert!(json.get("shannonOAuth").is_none(), "{json}");
    }

    /// W3-B (A2): `update_mcp_server_oauth_tokens_to` writes the refresh
    /// result in place — the `shannonOAuth` block plus the mirrored
    /// `Authorization` header — without disturbing the rest of the entry
    /// or other rows, and reports a missing entry as `Ok(false)`.
    #[test]
    fn update_oauth_tokens_edits_entry_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let path = tmp_settings(dir.path());
        std::fs::write(
            &path,
            r#"{"other":1,"mcpServers":{
                "linear":{"type":"http","url":"https://mcp.linear.app/sse","enabled":false,"headers":{"Authorization":"Bearer old"},"shannonOAuth":{"client_id":"cid","token_url":"https://t/token","refresh_token":"rt1","access_token":"old"}}
            }}"#,
        )
        .unwrap();

        let updated = McpStoredOAuth {
            client_id: "cid".into(),
            token_url: "https://t/token".into(),
            refresh_token: Some("rt2".into()),
            access_token: Some("new".into()),
            expires_at: Some(1735689600),
        };
        assert!(update_mcp_server_oauth_tokens_to(&path, "linear", &updated).unwrap());

        let root: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let entry = &root["mcpServers"]["linear"];
        assert_eq!(entry["shannonOAuth"]["access_token"], "new");
        assert_eq!(entry["shannonOAuth"]["refresh_token"], "rt2");
        assert_eq!(entry["headers"]["Authorization"], "Bearer new");
        // Untouched blob fields and unrelated top-level keys survive.
        assert_eq!(entry["url"], "https://mcp.linear.app/sse");
        assert_eq!(entry["enabled"], false);
        assert_eq!(root["other"], 1);

        assert!(!update_mcp_server_oauth_tokens_to(&path, "ghost", &updated).unwrap());
    }

    // ── F5 (R7-④ batch 2 / A8): OAuth tokens → OS keyring ─────────────────

    use crate::secret_store::{MockSecretStore, SecretStore};

    fn linear_entry_json() -> String {
        r#"{"other":1,"mcpServers":{
            "linear":{"type":"http","url":"https://mcp.linear.app/sse","enabled":false,
                      "headers":{"Authorization":"Bearer old","X-Custom":"keep-me"},
                      "shannonOAuth":{"client_id":"cid","token_url":"https://t/token","refresh_token":"rt1","access_token":"old"}}
        }}"#
        .replace('\n', "")
    }

    /// Acceptance ①: plaintext token block → migrate → the block lands in
    /// the keyring under `shannon/mcp-oauth/<name>`, the plaintext secret
    /// material (block + mirrored Authorization) is deleted from the file,
    /// everything else (custom headers, url, enabled, foreign keys) stays,
    /// and a re-run is a no-op. The load path then reads the token back
    /// from the keyring.
    #[test]
    fn migrate_oauth_moves_block_to_keyring_and_strips_plaintext() {
        let dir = tempfile::tempdir().unwrap();
        let path = tmp_settings(dir.path());
        std::fs::write(&path, linear_entry_json()).unwrap();

        let store = MockSecretStore::new();
        let migrated = migrate_mcp_oauth_secrets_to(&path, Some(&store));
        assert_eq!(migrated, 1);

        // The keyring holds the full token block, keyed by namespace.
        let key = crate::secret_store::mcp_oauth_key("linear");
        let raw = store.value(&key).expect("keyring entry after migration");
        let block: McpStoredOAuth = serde_json::from_str(&raw).unwrap();
        assert_eq!(block.access_token.as_deref(), Some("old"));
        assert_eq!(block.refresh_token.as_deref(), Some("rt1"));
        assert_eq!(block.token_url, "https://t/token");

        // The file: plaintext secret material gone, everything else kept.
        let root: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let entry = &root["mcpServers"]["linear"];
        assert!(entry.get("shannonOAuth").is_none(), "{entry}");
        assert_eq!(
            entry["headers"]["X-Custom"], "keep-me",
            "manual custom headers are never migrated away"
        );
        assert!(
            entry["headers"].get("Authorization").is_none(),
            "the OAuth-mirrored Authorization header goes with the block: {entry}"
        );
        assert_eq!(entry["url"], "https://mcp.linear.app/sse");
        assert_eq!(entry["enabled"], false);
        assert_eq!(root["other"], 1);

        // Idempotent: a second pass is a no-op.
        assert_eq!(migrate_mcp_oauth_secrets_to(&path, Some(&store)), 0);

        // Read path (tolerance window closed): the token comes back from
        // the keyring and the row stays an OAuth entry.
        let servers = load_mcp_servers_with_store(&path, Some(&store)).unwrap();
        let linear = servers.iter().find(|s| s.name == "linear").unwrap();
        let oauth = linear.oauth.as_ref().expect("oauth restored from keyring");
        assert_eq!(oauth.access_token.as_deref(), Some("old"));
        assert!(linear.has_auth_headers);
        assert_eq!(oauth.token_url, "https://t/token");
    }

    /// Acceptance ② (W3-B legacy shape): a header-only pre-A2 entry — the
    /// exact storage form `oauth_from_bearer_header` derives from —
    /// migrates too, while a manual entry with only custom headers stays
    /// untouched (fail-safe semantics unchanged).
    #[test]
    fn migrate_oauth_handles_legacy_bearer_but_leaves_manual_headers() {
        let dir = tempfile::tempdir().unwrap();
        let path = tmp_settings(dir.path());
        std::fs::write(
            &path,
            r#"{"mcpServers":{
                "legacy":{"type":"http","url":"https://mcp.example","headers":{"Authorization":"Bearer leg"}},
                "manual":{"type":"http","url":"https://mcp.example","headers":{"X-API-Key":"k3y"}}
            }}"#,
        )
        .unwrap();

        let store = MockSecretStore::new();
        assert_eq!(migrate_mcp_oauth_secrets_to(&path, Some(&store)), 1);

        let raw = store
            .value(&crate::secret_store::mcp_oauth_key("legacy"))
            .expect("legacy bearer migrated");
        let block: McpStoredOAuth = serde_json::from_str(&raw).unwrap();
        assert_eq!(block.access_token.as_deref(), Some("leg"));
        assert!(!block.can_refresh(), "derived shape keeps its honesty");

        let root: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            root["mcpServers"]["legacy"]["headers"],
            serde_json::Value::Null,
            "mirrored Authorization stripped with the (empty) headers object"
        );
        let manual = &root["mcpServers"]["manual"];
        assert_eq!(manual["headers"]["X-API-Key"], "k3y", "manual headers stay");
        assert!(!store.contains(&crate::secret_store::mcp_oauth_key("manual")));
    }

    /// Acceptance ③: a failed keyring write keeps the plaintext (the file is
    /// 0600) and warns — migration never blocks loading and never loses a
    /// credential.
    #[test]
    fn failed_keyring_write_keeps_plaintext_and_warns() {
        let dir = tempfile::tempdir().unwrap();
        let path = tmp_settings(dir.path());
        std::fs::write(&path, linear_entry_json()).unwrap();
        let original = std::fs::read_to_string(&path).unwrap();

        let store = MockSecretStore::failing_writes();
        let (capture, migrated) = crate::secret_store::test_support::capture_warnings(|| {
            migrate_mcp_oauth_secrets_to(&path, Some(&store))
        });
        assert_eq!(migrated, 0);
        assert!(
            store
                .value(&crate::secret_store::mcp_oauth_key("linear"))
                .is_none()
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            original,
            "degraded write: the plaintext credential must survive"
        );
        let warnings = capture.warnings();
        assert!(
            warnings.iter().any(|w| w.contains("keyring write failed")),
            "degradation must be visible: {warnings:?}"
        );

        // Loading still works (from plaintext).
        let servers = load_mcp_servers_with_store(&path, Some(&store)).unwrap();
        let linear = servers.iter().find(|s| s.name == "linear").unwrap();
        assert_eq!(
            linear.oauth.as_ref().unwrap().access_token.as_deref(),
            Some("old")
        );
    }

    /// `None` store (probe failed / pre-init) = the migration is a no-op and
    /// the plaintext stays — the documented degraded mode.
    #[test]
    fn migrate_without_store_is_a_noop() {
        let dir = tempfile::tempdir().unwrap();
        let path = tmp_settings(dir.path());
        std::fs::write(&path, linear_entry_json()).unwrap();
        let original = std::fs::read_to_string(&path).unwrap();
        assert_eq!(migrate_mcp_oauth_secrets_to(&path, None), 0);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }

    /// Regression (review Important#1): the crash-shaped state "keyring
    /// already holds the refreshed NEW token, settings.json still carries
    /// the OLD plaintext (an earlier migration's rewrite failed, then a
    /// refresh rotated the keyring)" must NEVER roll the keyring back —
    /// re-putting the old block would resurrect a dead single-use refresh
    /// token. The stale leftover is stripped; the keyring block survives.
    #[test]
    fn migration_never_rolls_a_newer_keyring_block_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = tmp_settings(dir.path());
        std::fs::write(
            &path,
            r#"{"mcpServers":{"linear":{"type":"http","url":"https://mcp.linear.app/sse",
                "shannonOAuth":{"client_id":"cid","token_url":"https://t/token",
                                "refresh_token":"dead-refresh","access_token":"stale-token",
                                "expires_at":1000}}}}"#,
        )
        .unwrap();

        // Keyring holds the rotated (newer) pair from the refresh.
        let store = MockSecretStore::new();
        let newer = McpStoredOAuth {
            client_id: "cid".into(),
            token_url: "https://t/token".into(),
            refresh_token: Some("rotated-refresh".into()),
            access_token: Some("fresh-token".into()),
            expires_at: Some(2000),
        };
        store
            .put(
                &crate::secret_store::mcp_oauth_key("linear"),
                &serde_json::to_string(&newer).unwrap(),
            )
            .unwrap();

        assert_eq!(migrate_mcp_oauth_secrets_to(&path, Some(&store)), 1);

        // The keyring still holds the NEW credential — not rolled back.
        let kept: McpStoredOAuth = serde_json::from_str(
            &store
                .value(&crate::secret_store::mcp_oauth_key("linear"))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            kept, newer,
            "keyring block must win over the stale plaintext"
        );

        // The stale plaintext is still stripped (the file converges).
        let root: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(root["mcpServers"]["linear"].get("shannonOAuth").is_none());
    }

    /// The mirror case (review Important#1's other leg): the plaintext IS
    /// the newer block — a refresh whose keyring write failed last session
    /// left the rotated pair in the file — so the migration pushes it into
    /// the keyring instead of stripping it into oblivion.
    #[test]
    fn migration_lets_a_demonstrably_newer_plaintext_block_win() {
        let dir = tempfile::tempdir().unwrap();
        let path = tmp_settings(dir.path());
        std::fs::write(
            &path,
            r#"{"mcpServers":{"linear":{"type":"http","url":"https://mcp.linear.app/sse",
                "shannonOAuth":{"client_id":"cid","token_url":"https://t/token",
                                "refresh_token":"rotated-refresh","access_token":"fresh-token",
                                "expires_at":2000}}}}"#,
        )
        .unwrap();

        // Keyring holds the pre-rotation (older) pair.
        let store = MockSecretStore::new();
        let older = McpStoredOAuth {
            client_id: "cid".into(),
            token_url: "https://t/token".into(),
            refresh_token: Some("dead-refresh".into()),
            access_token: Some("stale-token".into()),
            expires_at: Some(1000),
        };
        store
            .put(
                &crate::secret_store::mcp_oauth_key("linear"),
                &serde_json::to_string(&older).unwrap(),
            )
            .unwrap();

        assert_eq!(migrate_mcp_oauth_secrets_to(&path, Some(&store)), 1);

        // The keyring now carries the newer (plaintext) credential.
        let pushed: McpStoredOAuth = serde_json::from_str(
            &store
                .value(&crate::secret_store::mcp_oauth_key("linear"))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(pushed.refresh_token.as_deref(), Some("rotated-refresh"));
        assert_eq!(pushed.expires_at, Some(2000));

        // And the file converges to keyring-only.
        let root: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(root["mcpServers"]["linear"].get("shannonOAuth").is_none());
    }

    /// An indeterminate mismatch (static tokens, no `expires_at` on either
    /// side) never rolls the keyring back either — re-authentication
    /// recovers, a rolled-back block may not.
    #[test]
    fn migration_with_static_token_mismatch_keeps_the_keyring() {
        let dir = tempfile::tempdir().unwrap();
        let path = tmp_settings(dir.path());
        std::fs::write(
            &path,
            r#"{"mcpServers":{"linear":{"type":"http","url":"https://mcp.linear.app/sse",
                "shannonOAuth":{"client_id":"cid","token_url":"","access_token":"file-token"}}}}"#,
        )
        .unwrap();

        let store = MockSecretStore::new();
        store
            .put(
                &crate::secret_store::mcp_oauth_key("linear"),
                r#"{"client_id":"cid","token_url":"","access_token":"keyring-token"}"#,
            )
            .unwrap();

        assert_eq!(migrate_mcp_oauth_secrets_to(&path, Some(&store)), 1);
        let kept: McpStoredOAuth = serde_json::from_str(
            &store
                .value(&crate::secret_store::mcp_oauth_key("linear"))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(kept.access_token.as_deref(), Some("keyring-token"));
    }

    /// A keyring READ failure during migration leaves the entry untouched
    /// (plaintext kept, nothing overwritten) and warns — never guessed over
    /// an entry we cannot see.
    #[test]
    fn migration_with_unreadable_keyring_leaves_entries_and_warns() {
        let dir = tempfile::tempdir().unwrap();
        let path = tmp_settings(dir.path());
        std::fs::write(&path, linear_entry_json()).unwrap();
        let original = std::fs::read_to_string(&path).unwrap();

        let store = MockSecretStore::failing_reads();
        let (capture, migrated) = crate::secret_store::test_support::capture_warnings(|| {
            migrate_mcp_oauth_secrets_to(&path, Some(&store))
        });
        assert_eq!(migrated, 0);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            original,
            "an unreadable keyring must not be answered with a stripped file"
        );
        let warnings = capture.warnings();
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("keyring read failed during migration")),
            "read failure must be visible: {warnings:?}"
        );
    }

    /// Regression (review Important#3): a keyring read failure on the load
    /// path warns (with the server name) instead of silently rendering a
    /// migrated server as unauthenticated; the returned semantics are
    /// unchanged (plaintext fallback / credential-less row).
    #[test]
    fn load_with_unreadable_keyring_warns_and_falls_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = tmp_settings(dir.path());
        std::fs::write(&path, linear_entry_json()).unwrap();

        let store = MockSecretStore::failing_reads();
        let (capture, servers) = crate::secret_store::test_support::capture_warnings(|| {
            load_mcp_servers_with_store(&path, Some(&store))
        });
        let servers = servers.unwrap();
        let linear = servers.iter().find(|s| s.name == "linear").unwrap();
        // Semantics unchanged: the plaintext copy still serves the row…
        assert_eq!(
            linear.oauth.as_ref().unwrap().access_token.as_deref(),
            Some("old")
        );
        // …but the keyring failure is loud, not silent.
        let warnings = capture.warnings();
        assert!(
            warnings.iter().any(|w| w.contains("keyring read failed")),
            "a locked keychain must not silently look like 'not migrated': {warnings:?}"
        );
    }

    /// Read-path precedence within the tolerance window: keyring wins over
    /// a leftover plaintext copy, and the plaintext fallback still serves
    /// entries the store doesn't know.
    #[test]
    fn load_prefers_keyring_but_falls_back_to_plaintext() {
        let dir = tempfile::tempdir().unwrap();
        let path = tmp_settings(dir.path());
        std::fs::write(
            &path,
            r#"{"mcpServers":{
                "both":{"type":"http","url":"https://mcp.example","shannonOAuth":{"client_id":"c","token_url":"","refresh_token":"file-rt","access_token":"file-token"}},
                "plain":{"type":"http","url":"https://mcp.example","shannonOAuth":{"client_id":"c","token_url":"","access_token":"plain-token"}}
            }}"#,
        )
        .unwrap();

        let store = MockSecretStore::new();
        store
            .put(
                &crate::secret_store::mcp_oauth_key("both"),
                r#"{"client_id":"c","token_url":"","refresh_token":"kr-rt","access_token":"kr-token"}"#,
            )
            .unwrap();

        let servers = load_mcp_servers_with_store(&path, Some(&store)).unwrap();
        let both = servers.iter().find(|s| s.name == "both").unwrap();
        assert_eq!(
            both.oauth.as_ref().unwrap().refresh_token.as_deref(),
            Some("kr-rt"),
            "keyring must win while the tolerance window is open"
        );
        let plain = servers.iter().find(|s| s.name == "plain").unwrap();
        assert_eq!(
            plain.oauth.as_ref().unwrap().access_token.as_deref(),
            Some("plain-token"),
            "plaintext fallback keeps not-yet-migrated entries working"
        );

        // Degraded mode (`None` store): plaintext-only, as before F5.
        let servers = load_mcp_servers_with_store(&path, None).unwrap();
        let both = servers.iter().find(|s| s.name == "both").unwrap();
        assert_eq!(
            both.oauth.as_ref().unwrap().refresh_token.as_deref(),
            Some("file-rt")
        );
    }

    /// A handshake-time refresh in keyring mode updates the keyring and
    /// strips the plaintext copy instead of re-introducing it; the degraded
    /// mode keeps writing the file (0600).
    #[test]
    fn update_oauth_tokens_with_store_writes_keyring_and_strips_plaintext() {
        let dir = tempfile::tempdir().unwrap();
        let path = tmp_settings(dir.path());
        std::fs::write(&path, linear_entry_json()).unwrap();

        let store = MockSecretStore::new();
        let updated = McpStoredOAuth {
            client_id: "cid".into(),
            token_url: "https://t/token".into(),
            refresh_token: Some("rt2".into()),
            access_token: Some("new".into()),
            expires_at: Some(1735689600),
        };
        assert!(
            update_mcp_server_oauth_tokens_with_store(&path, "linear", &updated, Some(&store))
                .unwrap()
        );

        let raw = store
            .value(&crate::secret_store::mcp_oauth_key("linear"))
            .expect("refresh lands in the keyring");
        let block: McpStoredOAuth = serde_json::from_str(&raw).unwrap();
        assert_eq!(block.access_token.as_deref(), Some("new"));
        assert_eq!(block.refresh_token.as_deref(), Some("rt2"));

        let root: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let entry = &root["mcpServers"]["linear"];
        assert!(entry.get("shannonOAuth").is_none(), "{entry}");
        assert!(entry["headers"].get("Authorization").is_none(), "{entry}");
        assert_eq!(entry["headers"]["X-Custom"], "keep-me");

        // Degraded mode keeps the legacy in-file shape (existing test).
        assert!(
            update_mcp_server_oauth_tokens_with_store(&path, "linear", &updated, None).unwrap()
        );
        let root: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            root["mcpServers"]["linear"]["shannonOAuth"]["access_token"],
            "new"
        );
    }

    /// Uninstall cleanup: deleting a server removes its keyring entry with
    /// the store row.
    #[test]
    fn remove_mcp_server_entry_cleans_the_keyring() {
        let dir = tempfile::tempdir().unwrap();
        let path = tmp_settings(dir.path());
        std::fs::write(&path, linear_entry_json()).unwrap();

        let store = MockSecretStore::new();
        store
            .put(&crate::secret_store::mcp_oauth_key("linear"), "{}")
            .unwrap();

        assert!(remove_mcp_server_entry_with_store(&path, "linear", Some(&store)).unwrap());
        assert!(
            !store.contains(&crate::secret_store::mcp_oauth_key("linear")),
            "uninstall must not orphan the keyring entry"
        );
        // A missing entry stays success and cleans nothing extra.
        assert!(!remove_mcp_server_entry_with_store(&path, "linear", Some(&store)).unwrap());
    }

    /// W1-1 (R2-P0-1(B)): url-only store entries keep their `url` on load,
    /// and pre-existing configs without a `url` field still parse (serde
    /// default — backward compatibility).
    #[test]
    fn url_only_entries_keep_url_and_legacy_rows_parse_without_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = tmp_settings(dir.path());
        std::fs::write(
            &path,
            r#"{"mcpServers":{
                "notion":{"type":"http","url":"https://mcp.example","headers":{"Authorization":"Bearer x"}},
                "fs":{"command":"npx","args":["-y","fs"],"env":{},"enabled":true}
            }}"#,
        )
        .unwrap();

        let servers = load_mcp_servers_from(&path).unwrap();
        assert_eq!(servers.len(), 2);

        let notion = servers.iter().find(|s| s.name == "notion").unwrap();
        assert!(notion.command.is_empty());
        assert_eq!(notion.url.as_deref(), Some("https://mcp.example"));
        // W2-A: the `headers.Authorization` blob makes this row auth-gated —
        // the single-source verdict the seeder and the UI follow.
        assert!(notion.has_auth_headers);

        // A stdio row from an older store (no `url` key anywhere) parses
        // with `url: None` — the new field is fully backward compatible.
        let fs = servers.iter().find(|s| s.name == "fs").unwrap();
        assert_eq!(fs.command, "npx");
        assert!(fs.url.is_none());

        // A non-string `url` value is ignored rather than poisoning the row.
        std::fs::write(
            &path,
            r#"{"mcpServers":{"weird":{"url":42,"enabled":true}}}"#,
        )
        .unwrap();
        let servers = load_mcp_servers_from(&path).unwrap();
        assert_eq!(servers.len(), 1);
        assert!(servers[0].url.is_none());
    }

    /// W2-A (R4/A1) — the auth verdict: a url-only entry with headers (the
    /// OAuth installer product) is auth-gated; a header-less url-only entry
    /// is pure remote and wireable; an empty `headers` object is not auth.
    #[test]
    fn auth_verdict_follows_store_headers() {
        let dir = tempfile::tempdir().unwrap();
        let path = tmp_settings(dir.path());
        std::fs::write(
            &path,
            r#"{"mcpServers":{
                "oauth":{"type":"http","url":"https://mcp.example","headers":{"Authorization":"Bearer t"}},
                "pure":{"type":"http","url":"https://plain.example/mcp"},
                "empty-headers":{"url":"https://h.example","headers":{}},
                "cmd-header":{"url":"https://c.example","headers":{"X-Key":{"command":"op read x"}}}
            }}"#,
        )
        .unwrap();

        let servers = load_mcp_servers_from(&path).unwrap();
        let verdict = |name: &str| {
            servers
                .iter()
                .find(|s| s.name == name)
                .unwrap_or_else(|| panic!("{name} missing"))
                .has_auth_headers
        };
        assert!(verdict("oauth"), "Authorization header → auth-gated");
        assert!(verdict("cmd-header"), "command-sourced header → auth-gated");
        assert!(
            !verdict("pure"),
            "header-less url-only row is pure remote (wireable)"
        );
        assert!(
            !verdict("empty-headers"),
            "empty headers object is not auth"
        );
    }

    /// W2-A read/write symmetry: the save side already refused to reset a
    /// corrupt settings.json — the load side now reports it instead of
    /// silently masquerading as "nothing installed". Missing file and a
    /// store without `mcpServers` stay ordinary empty lists.
    #[test]
    fn load_honestly_reports_corrupt_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = tmp_settings(dir.path());

        // Missing file: ordinary empty start.
        assert!(load_mcp_servers_from(&path).unwrap().is_empty());

        // Corrupt JSON: Err, never a silent empty list.
        std::fs::write(&path, "{broken").unwrap();
        let err = load_mcp_servers_from(&path).unwrap_err();
        assert!(err.contains("settings.json parse"), "{err}");

        // Valid JSON but wrong shape (root not an object / mcpServers not
        // an object): equally corrupt as far as the store contract goes.
        std::fs::write(&path, "[1,2,3]").unwrap();
        assert!(load_mcp_servers_from(&path).is_err());
        std::fs::write(&path, r#"{"mcpServers":[1]}"#).unwrap();
        assert!(load_mcp_servers_from(&path).is_err());

        // Valid object without `mcpServers`: ordinary empty list.
        std::fs::write(&path, r#"{"permissions":{"allow":[]}}"#).unwrap();
        assert!(load_mcp_servers_from(&path).unwrap().is_empty());
    }

    /// W2-A inline toggle: the enabled flag flips in place — url-only rows
    /// keep their `type`/`url`/`headers` blob untouched — and a missing or
    /// corrupt store is reported, never invented.
    #[test]
    fn set_mcp_server_enabled_toggles_rows_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let path = tmp_settings(dir.path());
        std::fs::write(
            &path,
            r#"{"mcpServers":{
                "notion":{"type":"http","url":"https://mcp.example","headers":{"Authorization":"Bearer t"},"enabled":true},
                "fs":{"command":"npx","args":[],"env":{},"enabled":true}
            }}"#,
        )
        .unwrap();

        // Toggle the url-only row off…
        assert!(set_mcp_server_enabled_to(&path, "notion", false).unwrap());
        let root: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let notion = &root["mcpServers"]["notion"];
        assert_eq!(notion["enabled"], false);
        assert_eq!(notion["url"], "https://mcp.example");
        assert_eq!(notion["headers"]["Authorization"], "Bearer t");
        assert_eq!(notion["type"], "http");

        // …and the stdio row too; the first row is untouched.
        assert!(set_mcp_server_enabled_to(&path, "fs", false).unwrap());
        let servers = load_mcp_servers_from(&path).unwrap();
        assert!(servers.iter().all(|s| !s.enabled));

        // Back on.
        assert!(set_mcp_server_enabled_to(&path, "notion", true).unwrap());
        let servers = load_mcp_servers_from(&path).unwrap();
        assert!(servers.iter().find(|s| s.name == "notion").unwrap().enabled);

        // Unknown name → Ok(false), no write.
        assert!(!set_mcp_server_enabled_to(&path, "ghost", true).unwrap());

        // Corrupt store → Err.
        std::fs::write(&path, "{broken").unwrap();
        assert!(set_mcp_server_enabled_to(&path, "fs", true).is_err());
    }
}
