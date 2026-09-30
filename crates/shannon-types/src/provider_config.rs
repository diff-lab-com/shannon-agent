//! v2 multi-provider/model protocol-schema vocabulary for shannon-agent.
//!
//! Defines the cross-sibling protocol contract (Rust → JSON Schema → consumed by
//! shannon-desktop + shannon-gateway). Encodes decisions A1 (env-default credentials,
//! no plaintext in v2), B3 (phased: profile + multiplex routing, default off), and C1
//! (one-shot v1→v2 migration). The emitted schema lives at
//! `crates/shannon-types/schema/provider-model-config.schema.json`.
//!
//! ⚠ If you change types in this file, you MUST also update the redeclaration block
//! in `build.rs` (`build.rs:~356–557`) — `schemars::schema_for!` only sees the build.rs
//! stubs. Drift = schema silently diverges from Rust types. See ledger note.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ProviderKind {
    Anthropic,
    #[serde(rename = "openai")]
    OpenAi,
    #[serde(rename = "openai-compatible")]
    OpenAiCompatible,
    Ollama,
    Gemini,
    Deepseek,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Scope {
    Process,
    Session,
    Project,
    Global,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, JsonSchema, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ModelSource {
    Catalog,
    Discovered,
    #[default]
    UserDeclared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AuxRole {
    Vision,
    WebExtract,
    Compression,
    TitleGeneration,
    SessionSearch,
}

/// 凭据引用。A1 决议：Env 是默认/可用性下界；Keyring 机会性可选（探测失败静默降级）。
/// v2 结构化配置永不存明文——InlineLegacy 仅迁移过渡期，迁移后转 Env/Keyring。
#[derive(Debug, Clone, PartialEq, Eq, JsonSchema, Serialize, Deserialize)]
#[serde(tag = "backend", rename_all = "snake_case")]
pub enum CredentialRef {
    /// 默认后端：环境变量（CI / ~/.shannon/secrets.env chmod 0600）
    Env { var: String },
    /// Shannon 凭据存储后端：值落在 `~/.shannon/credentials/<service>.json`
    /// （0600）。这是 `/connect`、`/credentials` 写入、请求路径读取的统一
    /// 后端（ADR-0005 Phase 1）。读取由 provider_resolver 完成；缺失时返回
    /// 空，调用方自然回退到 provider 的 env 链。
    Store { service: String },
    /// 机会性可选：仅探测到 D-Bus secret-service 可用时启用
    Keyring { service: String, account: String },
    /// 迁移过渡期：已 mask 的旧明文，迁移完成后清除
    InlineLegacy { masked: String },
    /// 会话内临时注入，不落盘
    Ephemeral,
}

/// 原子切换单元：provider+model+scope 同组切换，杜绝半切换不一致（P3）。
#[derive(Debug, Clone, PartialEq, Eq, JsonSchema, Serialize, Deserialize)]
pub struct ActiveTarget {
    pub provider_id: String,
    pub model_id: String,
    pub scope: Scope,
}

/// 温度发送策略：None=用调用方默认；Omit=完全不发（如 Kimi 服务端自管）
#[derive(Debug, Clone, Copy, PartialEq, JsonSchema, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TemperatureStrategy {
    #[default]
    Default,
    Omit,
}

/// 首期最小集（避免 Hermes 20+ 布尔标志反模式）
#[derive(Debug, Clone, PartialEq, JsonSchema, Serialize, Deserialize)]
pub struct ProviderQuirks {
    pub temperature_strategy: TemperatureStrategy,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub max_tokens_override: Option<u32>,
    #[serde(default = "default_true")]
    pub send_temperature: bool,
}

impl Default for ProviderQuirks {
    fn default() -> Self {
        Self {
            temperature_strategy: TemperatureStrategy::default(),
            max_tokens_override: None,
            send_temperature: default_true(),
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, JsonSchema)]
pub struct ProviderTiers {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fast: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub standard: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pro: Option<String>,
}

/// R2-4: a single per-model capability flag. Same bit semantics as the
/// engine's `ModelCapabilities` catalog bitset, expressed as named variants so
/// the TOML/JSON schema stays human-editable. An unknown name is a schema
/// error (serde rejects it with the full accepted list).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ModelCapability {
    Reasoning,
    Coding,
    Speed,
    Cheap,
    Vision,
}

/// R2-4: user-declared metadata for one model on a provider profile.
///
/// Declared values are **authoritative** for pricing, context window and tier
/// classification: the engine consults them before the catalog, the built-in
/// pricing tables and the LiteLLM overlay. Every field is optional — declare
/// only what the endpoint actually documents. Costs are USD per million
/// tokens; a pricing override takes effect when *both* input and output
/// prices are declared (a lone half is ignored so billing never mixes a
/// declared price with a guessed one).
#[derive(Debug, Clone, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSpec {
    /// The model id exactly as sent to the API (matching is exact — this is
    /// what kills the substring-collision class of pricing drift).
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Total context window in tokens (drives compaction budgets).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
    /// Maximum output tokens per request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output: Option<u32>,
    /// Input price in USD per million tokens (≥ 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_per_m_input: Option<f64>,
    /// Output price in USD per million tokens (≥ 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_per_m_output: Option<f64>,
    /// Capability flags (fed to tier classification and capability gating).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<ModelCapability>,
}

impl ModelSpec {
    /// Semantic validation beyond the serde schema: non-empty id, positive
    /// token limits, finite non-negative prices. Returns a human-readable
    /// error naming the offending field.
    pub fn validate(&self) -> Result<(), String> {
        if self.id.trim().is_empty() {
            return Err("model declaration is missing `id`".to_string());
        }
        if self.context_window == Some(0) {
            return Err(format!("model '{}': `context_window` must be > 0", self.id));
        }
        if self.max_output == Some(0) {
            return Err(format!("model '{}': `max_output` must be > 0", self.id));
        }
        for (field, value) in [
            ("cost_per_m_input", self.cost_per_m_input),
            ("cost_per_m_output", self.cost_per_m_output),
        ] {
            if let Some(v) = value {
                if !v.is_finite() || v < 0.0 {
                    return Err(format!(
                        "model '{}': `{field}` must be a finite number ≥ 0 (got {v})",
                        self.id
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderProfile {
    pub id: String,
    pub kind: ProviderKind,
    pub display_name: String,
    pub base_url: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub models_url: Option<String>, // None → {base_url}/models
    pub credential: CredentialRef,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub extra_headers: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub default_max_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback_models: Vec<String>,
    #[serde(default)]
    pub quirks: ProviderQuirks,
    #[serde(default)]
    pub tiers: ProviderTiers,
    /// R2-4: per-model metadata declarations (pricing / context window /
    /// max output / capabilities). Authoritative over catalog + pricing
    /// overlays for the declared ids. Absent = no declarations (the
    /// historical behavior).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<ModelSpec>,
}

impl ProviderProfile {
    /// Validate the profile's `models` declarations: every spec individually
    /// valid, ids unique within the profile. Returns a human-readable error
    /// naming the first offender.
    pub fn validate_models(&self) -> Result<(), String> {
        let mut seen = std::collections::HashSet::new();
        for spec in &self.models {
            spec.validate()
                .map_err(|e| format!("provider '{}': {e}", self.id))?;
            if !seen.insert(spec.id.as_str()) {
                return Err(format!(
                    "provider '{}': duplicate model declaration for id '{}'",
                    self.id, spec.id
                ));
            }
        }
        Ok(())
    }
}

/// Provider 注册的模型目录条目（context 限制、工具支持、来源标签）。
/// 写权限在 catalog；运行期只读。
#[derive(Debug, Clone, PartialEq, Eq, JsonSchema, Serialize, Deserialize)]
pub struct ModelDescriptor {
    pub id: String,
    pub provider_id: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub context_limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub output_limit: Option<u32>,
    #[serde(default)]
    pub supports_tools: bool,
    #[serde(default)]
    pub supports_vision: bool,
    #[serde(default)]
    pub source: ModelSource,
    #[serde(default)]
    pub available: bool,
}

/// 命名 profile（providers + active target + credential scope）。
/// 同一 v2 config 可承载多个 profile（gateway multiplex 路由按
/// `ProfileRoute.specificity_weight` 选路），单 profile 场景下
/// gateway 默认 off，字节级等同 v1 行为。
#[derive(Debug, Clone, PartialEq, JsonSchema, Serialize, Deserialize)]
pub struct ModelProfile {
    #[serde(default)]
    pub name: String,
    pub active_target: ActiveTarget,
    #[serde(default)]
    pub providers: Vec<ProviderProfile>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub auxiliary: HashMap<AuxRole, ActiveTarget>,
    /// C1 两层凭据解析（默认 Shared；isolated 时独立解析，互不影响）
    #[serde(default)]
    pub credential_scope: CredentialScope,
}

/// v2 多 provider/model 协议 schema 顶层文档。
///
/// 承载 A1（env-default credentials, 永不存明文）/ B3（phased profile +
/// multiplex routing, 默认 off）/ C1（v1→v2 one-shot 迁移前置 version
/// 字段）。`version` 必须 = `VERSION`；迁移逻辑见 Φ1。
#[derive(Debug, Clone, PartialEq, JsonSchema, Serialize, Deserialize)]
pub struct ProviderModelConfig {
    pub version: u32, // = VERSION
    /// R3-2: which named profile (a key into `profiles`) in-process
    /// resolution uses (engine launch, `/model`, `/connect`, credentials).
    /// Empty (or absent) means [`Self::DEFAULT_PROFILE`] — B3 phase-1 files
    /// round-trip byte-identically because the key is skipped when default.
    /// Must appear **before** `profiles` in field order: TOML requires
    /// scalar values ahead of the `[profiles.*]` tables.
    #[serde(default, skip_serializing_if = "is_default_active_profile")]
    pub active_profile: String,
    pub profiles: HashMap<String, ModelProfile>,
    /// B3 契约：网关多 profile 路由（默认 off，字节级等同单 profile）
    #[serde(default)]
    pub gateway: GatewayConfig,
}

/// `skip_serializing_if` predicate for [`ProviderModelConfig::active_profile`]:
/// the key is omitted when unset or pointed at `"default"`, so v2 files
/// written before R3-2 (and single-profile users) keep their exact shape.
fn is_default_active_profile(s: &String) -> bool {
    s.is_empty() || s == ProviderModelConfig::DEFAULT_PROFILE
}

/// Validate a user-supplied profile name (R3-2 `/profiles new|rename`). The
/// name is the `profiles` map key (a TOML table key and the `/profiles use`
/// argument), so it must be a single friendly token. Returns the trimmed
/// name or a human-readable error. Shared by the service, the REPL command
/// and the desktop so every front-end rejects the same input.
pub fn validate_profile_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("profile name must not be empty".to_string());
    }
    if trimmed.len() > 64 {
        return Err(format!(
            "profile name is too long ({} chars; max 64): '{}'",
            trimmed.len(),
            trimmed
        ));
    }
    if trimmed.chars().any(char::is_whitespace) {
        return Err(format!(
            "profile name must not contain whitespace: '{trimmed}'"
        ));
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return Err(format!(
            "profile name must not contain control characters: '{trimmed}'"
        ));
    }
    Ok(trimmed.to_string())
}

impl ProviderModelConfig {
    /// Current schema version. `ProviderModelConfig::version` 字段必须等于此常量。
    pub const VERSION: u32 = 2;

    /// The profile key used when [`Self::active_profile`] is empty — the B3
    /// phase-1 single-profile name every pre-R3-2 file implicitly targets.
    pub const DEFAULT_PROFILE: &'static str = "default";

    /// The profile key in-process resolution uses: `active_profile` when
    /// set, else [`Self::DEFAULT_PROFILE`]. Never empty — callers can index
    /// `profiles` with it directly.
    pub fn active_profile_key(&self) -> &str {
        if self.active_profile.is_empty() {
            Self::DEFAULT_PROFILE
        } else {
            &self.active_profile
        }
    }

    /// Borrow the active [`ModelProfile`] (per [`Self::active_profile_key`]).
    /// `None` when the pointer dangles (the named profile was deleted by a
    /// hand edit / another writer) — callers fall back to synthesis.
    pub fn active_model_profile(&self) -> Option<&ModelProfile> {
        self.profiles.get(self.active_profile_key())
    }

    /// Mutable twin of [`Self::active_model_profile`].
    pub fn active_model_profile_mut(&mut self) -> Option<&mut ModelProfile> {
        let key = self.active_profile_key().to_string();
        self.profiles.get_mut(&key)
    }

    /// All profile names, sorted. HashMap iteration order is nondeterministic,
    /// so every listing surface (REPL `/profiles`, CLI, desktop) goes through
    /// this to render a stable order.
    pub fn profile_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.profiles.keys().cloned().collect();
        names.sort();
        names
    }

    /// Validate every profile's per-model declarations (R2-4). The store's
    /// `load` refuses files that fail this — same graceful-degradation
    /// contract as a parse error.
    pub fn validate_models(&self) -> Result<(), String> {
        for (name, profile) in &self.profiles {
            for provider in &profile.providers {
                provider
                    .validate_models()
                    .map_err(|e| format!("profile '{name}': {e}"))?;
            }
        }
        Ok(())
    }
}

/// N1: a `Default` so `ShannonConfig` (which embeds this via `#[serde(default)]`)
/// can keep its `#[derive(Default)]`. Empty profiles → no active target → the
/// legacy v1 flat fields / Ollama default are used. `version` is pinned to
/// `VERSION`; no schema impact (T5 stays green).
impl Default for ProviderModelConfig {
    fn default() -> Self {
        Self {
            version: Self::VERSION,
            active_profile: String::new(),
            profiles: HashMap::new(),
            gateway: Default::default(),
        }
    }
}

/// C1 两层凭据解析：默认 Shared（沿用旧单 profile 语义）；
/// Isolated 表示该 profile 独立解析凭据，互不影响。
#[derive(Debug, Clone, Copy, PartialEq, Eq, JsonSchema, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CredentialScope {
    #[default]
    Shared,
    Isolated,
}

/// B3 契约：profile 路由条目。specificity 由 `specificity_weight` 计算：
/// session(8) > project(4) > tenant(2)，client_id 不参与评分。
#[derive(Debug, Clone, PartialEq, Eq, JsonSchema, Serialize, Deserialize)]
pub struct ProfileRoute {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tenant_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub project_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub client_id: Option<String>,
    pub profile: String,
    #[serde(default = "default_route_enabled")]
    pub enabled: bool,
}

fn default_route_enabled() -> bool {
    true
}

/// B3 契约：网关级 multiplex 路由配置。`multiplex_profiles=false`（默认）时
/// `profile_routes` 完全被忽略，行为字节级等同单 profile。
#[derive(Debug, Clone, Default, PartialEq, Eq, JsonSchema, Serialize, Deserialize)]
pub struct GatewayConfig {
    #[serde(default)]
    pub multiplex_profiles: bool,
    #[serde(default)]
    pub profile_routes: Vec<ProfileRoute>,
}

/// 计算路由条目的 specificity 加权值。
/// 规则：session=8 / project=4 / tenant=2，按字段是否设置累加；未设置=0。
/// client_id 不参与评分（仅用于 audit / 标识，不影响选路）。
pub fn specificity_weight(r: &ProfileRoute) -> u32 {
    let mut w: u32 = 0;
    if r.session_id.is_some() {
        w += 8;
    }
    if r.project_path.is_some() {
        w += 4;
    }
    if r.tenant_id.is_some() {
        w += 2;
    }
    w
}

/// Model tier. Canonical names are `fast`/`standard`/`pro`/`auto`.
/// Aliases (input-only) include Anthropic's `haiku`/`sonnet`/`opus`
/// and provider-native names (`flash`/`mini`/`plus`/`ultra`/`max`).
///
/// `Auto` is an input-only tier: `/model --tier auto` resolves it to a
/// concrete tier via a lightweight best-default heuristic (standard → pro →
/// fast; ADR-0005 decision ②) — not the full task-type router (spec §11).
/// `Auto` is never persisted; only the resolved concrete tier is stored.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TierName {
    Fast,
    Standard,
    Pro,
    Auto,
}

impl TierName {
    /// Canonical lowercase name (used in toml, logs, status pills).
    pub fn canonical(self) -> &'static str {
        match self {
            TierName::Fast => "fast",
            TierName::Standard => "standard",
            TierName::Pro => "pro",
            TierName::Auto => "auto",
        }
    }

    /// Human-readable display label (capitalized, used in UI).
    pub fn display(self) -> &'static str {
        match self {
            TierName::Fast => "Fast",
            TierName::Standard => "Standard",
            TierName::Pro => "Pro",
            TierName::Auto => "Auto",
        }
    }

    /// Normalize any accepted user input to canonical TierName.
    /// Accepts canonical names + Anthropic aliases + other provider-native
    /// aliases. Case-insensitive. Returns None for unrecognized input.
    pub fn from_user_input(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            // Canonical
            "fast" => Some(TierName::Fast),
            "standard" => Some(TierName::Standard),
            "pro" => Some(TierName::Pro),
            "auto" => Some(TierName::Auto),
            // Aliases → Fast
            "flash" | "mini" | "nano" | "haiku" => Some(TierName::Fast),
            // Aliases → Standard
            "plus" | "sonnet" | "medium" | "turbo" => Some(TierName::Standard),
            // Aliases → Pro
            "opus" | "ultra" | "max" | "large" => Some(TierName::Pro),
            _ => None,
        }
    }

    /// Tab-completion suggestions shown to the user.
    /// Order: canonical first, then Anthropic aliases, then other aliases.
    pub fn suggestions() -> &'static [&'static str] {
        &[
            "fast", "standard", "pro", "auto", "haiku", "sonnet", "opus", "flash", "mini", "plus",
            "ultra", "max",
        ]
    }
}

#[cfg(test)]
mod tier_name_tests {
    use super::*;

    #[test]
    fn canonical_is_lowercase() {
        assert_eq!(TierName::Fast.canonical(), "fast");
        assert_eq!(TierName::Standard.canonical(), "standard");
        assert_eq!(TierName::Pro.canonical(), "pro");
        assert_eq!(TierName::Auto.canonical(), "auto");
    }

    #[test]
    fn from_user_input_accepts_canonical() {
        assert_eq!(TierName::from_user_input("fast"), Some(TierName::Fast));
        assert_eq!(
            TierName::from_user_input("standard"),
            Some(TierName::Standard)
        );
        assert_eq!(TierName::from_user_input("pro"), Some(TierName::Pro));
        assert_eq!(TierName::from_user_input("auto"), Some(TierName::Auto));
    }

    #[test]
    fn from_user_input_accepts_anthropic_aliases() {
        assert_eq!(TierName::from_user_input("haiku"), Some(TierName::Fast));
        assert_eq!(
            TierName::from_user_input("sonnet"),
            Some(TierName::Standard)
        );
        assert_eq!(TierName::from_user_input("opus"), Some(TierName::Pro));
    }

    #[test]
    fn from_user_input_accepts_other_provider_aliases() {
        assert_eq!(TierName::from_user_input("flash"), Some(TierName::Fast));
        assert_eq!(TierName::from_user_input("mini"), Some(TierName::Fast));
        assert_eq!(TierName::from_user_input("plus"), Some(TierName::Standard));
        assert_eq!(TierName::from_user_input("ultra"), Some(TierName::Pro));
        assert_eq!(TierName::from_user_input("max"), Some(TierName::Pro));
    }

    #[test]
    fn from_user_input_is_case_insensitive() {
        assert_eq!(TierName::from_user_input("FAST"), Some(TierName::Fast));
        assert_eq!(TierName::from_user_input("Haiku"), Some(TierName::Fast));
        assert_eq!(TierName::from_user_input("oPuS"), Some(TierName::Pro));
    }

    #[test]
    fn from_user_input_rejects_unknown() {
        assert_eq!(TierName::from_user_input(""), None);
        assert_eq!(TierName::from_user_input("xyz"), None);
        assert_eq!(TierName::from_user_input("turbo-xl"), None);
    }

    #[test]
    fn canonical_round_trips_through_from_user_input() {
        for tier in [
            TierName::Fast,
            TierName::Standard,
            TierName::Pro,
            TierName::Auto,
        ] {
            assert_eq!(TierName::from_user_input(tier.canonical()), Some(tier));
        }
    }

    #[test]
    fn suggestions_starts_with_canonical() {
        let s = TierName::suggestions();
        assert_eq!(s[0], "fast");
        assert_eq!(s[1], "standard");
        assert_eq!(s[2], "pro");
        assert_eq!(s[3], "auto");
        // Anthropic aliases present
        assert!(s.contains(&"haiku"));
        assert!(s.contains(&"sonnet"));
        assert!(s.contains(&"opus"));
    }

    #[test]
    fn provider_profile_round_trip_with_tiers() {
        let profile = ProviderProfile {
            id: "anthropic".to_string(),
            kind: ProviderKind::Anthropic,
            display_name: "Anthropic".to_string(),
            base_url: "https://api.anthropic.com".to_string(),
            models_url: None,
            credential: CredentialRef::Env {
                var: "ANTHROPIC_API_KEY".to_string(),
            },
            extra_headers: Default::default(),
            default_max_tokens: None,
            fallback_models: vec![],
            quirks: ProviderQuirks::default(),
            tiers: ProviderTiers {
                fast: Some("claude-haiku-4-5".to_string()),
                standard: Some("claude-sonnet-4-20250514".to_string()),
                pro: Some("claude-opus-4".to_string()),
            },
            models: Vec::new(),
        };

        let toml_str = toml::to_string(&profile).expect("serialize");
        assert!(toml_str.contains("fast = \"claude-haiku-4-5\""));
        assert!(toml_str.contains("standard = \"claude-sonnet-4-20250514\""));
        assert!(toml_str.contains("pro = \"claude-opus-4\""));

        let parsed: ProviderProfile = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(parsed.tiers.fast, profile.tiers.fast);
        assert_eq!(parsed.tiers.standard, profile.tiers.standard);
        assert_eq!(parsed.tiers.pro, profile.tiers.pro);
    }

    #[test]
    fn provider_profile_round_trip_without_tiers_uses_default() {
        // Existing toml files without `tiers` should still parse
        let minimal_toml = r#"
            id = "anthropic"
            kind = "anthropic"
            display_name = "Anthropic"
            base_url = "https://api.anthropic.com"
            credential = { backend = "env", var = "ANTHROPIC_API_KEY" }
        "#;
        let parsed: ProviderProfile = toml::from_str(minimal_toml).expect("deserialize");
        assert_eq!(parsed.tiers, ProviderTiers::default());
    }

    // ── R2-4: per-model metadata declarations ────────────────────────────

    fn openai_compat_profile_with(models: Vec<ModelSpec>) -> ProviderProfile {
        ProviderProfile {
            id: "glm".to_string(),
            kind: ProviderKind::OpenAiCompatible,
            display_name: "GLM".to_string(),
            base_url: "https://open.bigmodel.cn/api/paas/v4".to_string(),
            models_url: None,
            credential: CredentialRef::Env {
                var: "ZHIPU_API_KEY".to_string(),
            },
            extra_headers: HashMap::new(),
            default_max_tokens: None,
            fallback_models: vec![],
            quirks: ProviderQuirks::default(),
            tiers: ProviderTiers::default(),
            models,
        }
    }

    #[test]
    fn model_spec_round_trips_through_toml() {
        let profile = openai_compat_profile_with(vec![ModelSpec {
            id: "glm-5.3-flash".to_string(),
            display_name: Some("GLM-5.3 Flash".to_string()),
            context_window: Some(198_000),
            max_output: Some(32_768),
            cost_per_m_input: Some(0.5),
            cost_per_m_output: Some(2.0),
            capabilities: vec![ModelCapability::Vision, ModelCapability::Reasoning],
        }]);

        let toml_str = toml::to_string(&profile).expect("serialize");
        assert!(
            toml_str.contains("[[models]]"),
            "array-of-tables form:\n{toml_str}"
        );
        for needle in [
            "id = \"glm-5.3-flash\"",
            "context_window = 198000",
            "max_output = 32768",
            "cost_per_m_input = 0.5",
            "cost_per_m_output = 2.0",
        ] {
            assert!(
                toml_str.contains(needle),
                "missing `{needle}` in:\n{toml_str}"
            );
        }

        let parsed: ProviderProfile = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(parsed.models, profile.models);
        assert_eq!(
            parsed.models[0].capabilities,
            vec![ModelCapability::Vision, ModelCapability::Reasoning]
        );
    }

    #[test]
    fn profile_without_models_omits_field_and_parses() {
        // Backward compat: existing providers.toml files (and the canonical
        // writer, which skips the empty Vec) must keep parsing.
        let minimal_toml = r#"
            id = "glm"
            kind = "openai-compatible"
            display_name = "GLM"
            base_url = "https://open.bigmodel.cn/api/paas/v4"
            credential = { backend = "env", var = "ZHIPU_API_KEY" }
        "#;
        let parsed: ProviderProfile = toml::from_str(minimal_toml).expect("deserialize");
        assert!(parsed.models.is_empty());
        // And the round-trip omits the key entirely.
        let out = toml::to_string(&parsed).expect("serialize");
        assert!(
            !out.contains("models"),
            "empty models must be skipped:\n{out}"
        );
    }

    #[test]
    fn hand_written_models_block_parses() {
        let toml_str = r#"
            id = "glm"
            kind = "openai-compatible"
            display_name = "GLM"
            base_url = "https://open.bigmodel.cn/api/paas/v4"
            credential = { backend = "env", var = "ZHIPU_API_KEY" }

            [[models]]
            id = "glm-5.3-flash"
            context_window = 198000
            cost_per_m_input = 0.5
            cost_per_m_output = 2.0
            capabilities = ["vision", "reasoning"]

            [[models]]
            id = "glm-4.5-air"
        "#;
        let parsed: ProviderProfile = toml::from_str(toml_str).expect("deserialize");
        assert_eq!(parsed.models.len(), 2);
        assert_eq!(parsed.models[0].id, "glm-5.3-flash");
        assert_eq!(parsed.models[0].context_window, Some(198_000));
        assert!(parsed.models[1].context_window.is_none());
    }

    #[test]
    fn unknown_capability_name_is_rejected() {
        let toml_str = r#"
            id = "glm"
            kind = "openai-compatible"
            display_name = "GLM"
            base_url = "https://open.bigmodel.cn/api/paas/v4"
            credential = { backend = "env", var = "ZHIPU_API_KEY" }

            [[models]]
            id = "glm-5.3-flash"
            capabilities = ["visionn"]
        "#;
        let err = toml::from_str::<ProviderProfile>(toml_str)
            .expect_err("unknown capability must be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("visionn") && msg.contains("unknown variant"),
            "error must name the bad capability and the accepted set: {msg}"
        );
    }

    #[test]
    fn unknown_field_in_model_spec_is_rejected() {
        let toml_str = r#"
            id = "glm"
            kind = "openai-compatible"
            display_name = "GLM"
            base_url = "https://open.bigmodel.cn/api/paas/v4"
            credential = { backend = "env", var = "ZHIPU_API_KEY" }

            [[models]]
            id = "glm-5.3-flash"
            env_key = "X"
        "#;
        assert!(toml::from_str::<ProviderProfile>(toml_str).is_err());
    }

    #[test]
    fn validate_models_accepts_valid_specs() {
        let profile = openai_compat_profile_with(vec![ModelSpec {
            id: "m".to_string(),
            display_name: None,
            context_window: Some(1),
            max_output: None,
            cost_per_m_input: Some(0.0), // free local models are legal
            cost_per_m_output: Some(0.0),
            capabilities: vec![],
        }]);
        assert!(profile.validate_models().is_ok());
    }

    #[test]
    fn validate_models_rejects_duplicate_ids() {
        let dup = || ModelSpec {
            id: "same".to_string(),
            display_name: None,
            context_window: None,
            max_output: None,
            cost_per_m_input: None,
            cost_per_m_output: None,
            capabilities: vec![],
        };
        let profile = openai_compat_profile_with(vec![dup(), dup()]);
        let err = profile.validate_models().expect_err("dup must fail");
        assert!(err.contains("duplicate") && err.contains("same"), "{err}");
    }

    // ── R3-2: active_profile key + helpers ──────────────────────────────

    fn pm_config_with(profiles: &[(&str, ModelProfile)]) -> ProviderModelConfig {
        let mut map = HashMap::new();
        for (name, mp) in profiles {
            map.insert((*name).to_string(), mp.clone());
        }
        ProviderModelConfig {
            version: ProviderModelConfig::VERSION,
            active_profile: String::new(),
            profiles: map,
            gateway: Default::default(),
        }
    }

    fn empty_model_profile(name: &str) -> ModelProfile {
        ModelProfile {
            name: name.to_string(),
            active_target: ActiveTarget {
                provider_id: String::new(),
                model_id: String::new(),
                scope: Scope::Global,
            },
            providers: Vec::new(),
            auxiliary: HashMap::new(),
            credential_scope: CredentialScope::Shared,
        }
    }

    #[test]
    fn active_profile_key_defaults_to_default_when_unset() {
        let pm = pm_config_with(&[]);
        assert_eq!(pm.active_profile_key(), "default");
        assert!(pm.active_model_profile().is_none());
    }

    #[test]
    fn active_profile_key_honors_explicit_pointer() {
        let mut pm = pm_config_with(&[("work", empty_model_profile("work"))]);
        pm.active_profile = "work".to_string();
        assert_eq!(pm.active_profile_key(), "work");
        assert!(pm.active_model_profile().is_some());
        assert_eq!(pm.active_model_profile().unwrap().name, "work");
        // Mutable twin sees the same entry.
        pm.active_model_profile_mut().unwrap().name = "renamed".into();
        assert_eq!(pm.profiles["work"].name, "renamed");
    }

    #[test]
    fn active_profile_key_dangling_pointer_is_none() {
        let mut pm = pm_config_with(&[]);
        pm.active_profile = "ghost".to_string();
        assert_eq!(pm.active_profile_key(), "ghost");
        assert!(pm.active_model_profile().is_none());
    }

    #[test]
    fn profile_names_are_sorted_and_stable() {
        let pm = pm_config_with(&[
            ("zeta", empty_model_profile("zeta")),
            ("alpha", empty_model_profile("alpha")),
            ("mid", empty_model_profile("mid")),
        ]);
        assert_eq!(pm.profile_names(), vec!["alpha", "mid", "zeta"]);
    }

    #[test]
    fn active_profile_toml_round_trips_and_is_skipped_when_default() {
        let mut pm = pm_config_with(&[("default", empty_model_profile("default"))]);
        // Unset → the key is omitted entirely (byte-compat with pre-R3-2 files).
        let toml_str = toml::to_string(&pm).expect("serialize");
        assert!(
            !toml_str.contains("active_profile"),
            "default/unset active_profile must be skipped:\n{toml_str}"
        );
        let parsed: ProviderModelConfig = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(parsed.active_profile, "");
        assert_eq!(parsed, pm);

        // Explicit non-default → the key survives a round-trip, and an old
        // reader-free hand-written file without the key still parses.
        pm.active_profile = "work".to_string();
        let toml_str = toml::to_string(&pm).expect("serialize");
        assert!(toml_str.contains("active_profile = \"work\""), "{toml_str}");
        let parsed: ProviderModelConfig = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(parsed.active_profile_key(), "work");

        let legacy = r#"version = 2

[profiles.default]
name = "default"

[profiles.default.active_target]
provider_id = "anthropic"
model_id = "claude-sonnet-4-20250514"
scope = "global"
"#;
        let parsed: ProviderModelConfig = toml::from_str(legacy).expect("legacy file parses");
        assert_eq!(parsed.active_profile_key(), "default");
    }

    #[test]
    fn validate_profile_name_trims_and_rejects_bad_input() {
        assert_eq!(
            validate_profile_name("  work  ").unwrap(),
            "work".to_string()
        );
        assert!(validate_profile_name("").is_err());
        assert!(validate_profile_name("   ").is_err());
        assert!(validate_profile_name("two words").is_err());
        assert!(validate_profile_name("tab\tname").is_err());
        assert!(validate_profile_name(&"x".repeat(65)).is_err());
        assert_eq!(validate_profile_name(&"x".repeat(64)).unwrap().len(), 64);
    }

    #[test]
    fn validate_models_rejects_zero_context_and_bad_prices() {
        let zero_ctx = ModelSpec {
            id: "m".to_string(),
            display_name: None,
            context_window: Some(0),
            max_output: None,
            cost_per_m_input: None,
            cost_per_m_output: None,
            capabilities: vec![],
        };
        assert!(
            openai_compat_profile_with(vec![zero_ctx])
                .validate_models()
                .expect_err("zero context must fail")
                .contains("context_window")
        );

        let negative = ModelSpec {
            id: "m".to_string(),
            display_name: None,
            context_window: None,
            max_output: None,
            cost_per_m_input: Some(-0.5),
            cost_per_m_output: None,
            capabilities: vec![],
        };
        assert!(
            openai_compat_profile_with(vec![negative])
                .validate_models()
                .expect_err("negative price must fail")
                .contains("cost_per_m_input")
        );

        let empty_id = ModelSpec {
            id: "  ".to_string(),
            display_name: None,
            context_window: None,
            max_output: None,
            cost_per_m_input: None,
            cost_per_m_output: None,
            capabilities: vec![],
        };
        assert!(
            openai_compat_profile_with(vec![empty_id])
                .validate_models()
                .is_err()
        );
    }
}
