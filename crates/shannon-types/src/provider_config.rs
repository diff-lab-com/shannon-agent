use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
