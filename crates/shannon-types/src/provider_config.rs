use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    Anthropic,
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
}

#[derive(Debug, Clone, PartialEq, JsonSchema, Serialize, Deserialize)]
pub struct ProviderModelConfig {
    pub version: u32, // = 2
    pub profiles: HashMap<String, ModelProfile>,
}

impl ProviderModelConfig {
    pub fn version() -> u32 {
        2
    }
}
