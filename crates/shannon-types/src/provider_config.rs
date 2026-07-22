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
pub enum Scope {
    Process,
    Session,
    Project,
    Global,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, JsonSchema, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ModelSource {
    Catalog,
    Discovered,
    #[default]
    UserDeclared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
}

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

#[derive(Debug, Clone, PartialEq, JsonSchema, Serialize, Deserialize)]
pub struct ProviderModelConfig {
    pub version: u32, // = 2
    pub profiles: HashMap<String, ModelProfile>,
    /// B3 契约：网关多 profile 路由（默认 off，字节级等同单 profile）
    #[serde(default)]
    pub gateway: GatewayConfig,
}

impl ProviderModelConfig {
    pub fn version() -> u32 {
        2
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
#[derive(Debug, Clone, PartialEq, Eq, JsonSchema, Serialize, Deserialize)]
pub struct GatewayConfig {
    #[serde(default)]
    pub multiplex_profiles: bool,
    #[serde(default)]
    pub profile_routes: Vec<ProfileRoute>,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            multiplex_profiles: false,
            profile_routes: Vec::new(),
        }
    }
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
