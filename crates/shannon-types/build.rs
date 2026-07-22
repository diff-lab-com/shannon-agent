use std::env;
use std::fs;
use std::path::Path;

// Type definitions with JsonSchema derives for build.rs
// These must match the types in src/events.rs and src/provider_config.rs exactly
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

fn main() {
    println!("cargo:rerun-if-changed=src/events.rs");
    println!("cargo:rerun-if-changed=src/provider_config.rs");

    // Generate schemas for all 23 payload structs + EventEnvelope
    let mut schemas = schemars::Map::new();

    // Query lifecycle events
    schemas.insert(
        "QueryTextPayload".to_string(),
        schemars::schema_for!(QueryTextPayload),
    );
    schemas.insert(
        "ToolStartPayload".to_string(),
        schemars::schema_for!(ToolStartPayload),
    );
    schemas.insert(
        "ToolResultPayload".to_string(),
        schemars::schema_for!(ToolResultPayload),
    );
    schemas.insert(
        "ToolProgressPayload".to_string(),
        schemars::schema_for!(ToolProgressPayload),
    );
    schemas.insert(
        "ThinkingPayload".to_string(),
        schemars::schema_for!(ThinkingPayload),
    );
    schemas.insert(
        "UsagePayload".to_string(),
        schemars::schema_for!(UsagePayload),
    );
    schemas.insert(
        "QueryCompletedPayload".to_string(),
        schemars::schema_for!(QueryCompletedPayload),
    );
    schemas.insert(
        "QueryFailedPayload".to_string(),
        schemars::schema_for!(QueryFailedPayload),
    );
    schemas.insert(
        "QueryCancelledPayload".to_string(),
        schemars::schema_for!(QueryCancelledPayload),
    );

    // Permission and session events
    schemas.insert(
        "PermissionRequest".to_string(),
        schemars::schema_for!(PermissionRequest),
    );
    schemas.insert(
        "SessionInfo".to_string(),
        schemars::schema_for!(SessionInfo),
    );
    schemas.insert(
        "SessionLoaded".to_string(),
        schemars::schema_for!(SessionLoaded),
    );
    schemas.insert(
        "ChatMessage".to_string(),
        schemars::schema_for!(ChatMessage),
    );

    // Background task events
    schemas.insert(
        "BackgroundTaskUpdate".to_string(),
        schemars::schema_for!(BackgroundTaskUpdate),
    );
    schemas.insert(
        "BackgroundTaskInfo".to_string(),
        schemars::schema_for!(BackgroundTaskInfo),
    );

    // Config and update events
    schemas.insert(
        "ConfigUpdatedPayload".to_string(),
        schemars::schema_for!(ConfigUpdatedPayload),
    );
    schemas.insert(
        "UpdateAvailablePayload".to_string(),
        schemars::schema_for!(UpdateAvailablePayload),
    );
    schemas.insert(
        "UpdateProgressPayload".to_string(),
        schemars::schema_for!(UpdateProgressPayload),
    );

    // Diff review events
    schemas.insert("HunkAction".to_string(), schemars::schema_for!(HunkAction));
    schemas.insert(
        "DiffFileInfo".to_string(),
        schemars::schema_for!(DiffFileInfo),
    );
    schemas.insert("DiffHunk".to_string(), schemars::schema_for!(DiffHunk));

    // Workflow streaming events
    schemas.insert(
        "TaskStepPayload".to_string(),
        schemars::schema_for!(TaskStepPayload),
    );
    schemas.insert(
        "TaskRetryPayload".to_string(),
        schemars::schema_for!(TaskRetryPayload),
    );

    // EventEnvelope (generic, using serde_json::Value as concrete type)
    schemas.insert(
        "EventEnvelope".to_string(),
        schemars::schema_for!(EventEnvelope<serde_json::Value>),
    );

    // Normalize through serde_json::Value so the emitted JSON is deterministic.
    // schemars::Map may iterate non-deterministically (HashMap), which churned the
    // committed schema file on every build. serde_json::Value is BTreeMap-backed
    // (no `preserve_order` feature), so re-serializing sorts keys at every level.
    let value: serde_json::Value = serde_json::to_value(&schemas).expect("schema to_value failed");
    let schema_json = serde_json::to_string_pretty(&value).expect("schema serialization failed");

    // Write to OUT_DIR (build-time location)
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set");
    let out_path = Path::new(&out_dir).join("events.schema.json");
    fs::write(&out_path, &schema_json).expect("failed to write schema to OUT_DIR");

    // Also write to source tree (committed, for consumers without build)
    // Use CARGO_MANIFEST_DIR to get the crate directory
    let crate_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let src_path = Path::new(&crate_dir).join("schema/events.schema.json");
    fs::create_dir_all(src_path.parent().unwrap()).expect("failed to create schema directory");
    fs::write(&src_path, &schema_json).expect("failed to write schema to source tree");

    println!(
        "cargo:warning=Generated JSON Schema at: {}",
        src_path.display()
    );

    // ── ProviderModelConfig schema (Φ0.5: cross-end contract) ─────────
    // Mirrors the events pattern: generate via schemars, write to OUT_DIR
    // for build-time consumers, then copy to schema/ for committed-file
    // consumers (desktop/gateway without a build step).
    let pm_schema = schemars::schema_for!(ProviderModelConfig);
    let pm_value: serde_json::Value =
        serde_json::to_value(&pm_schema).expect("pm schema to_value failed");
    let pm_json = serde_json::to_string_pretty(&pm_value).expect("pm schema serialization failed");

    let pm_out_path = Path::new(&out_dir).join("provider-model-config.schema.json");
    fs::write(&pm_out_path, &pm_json).expect("failed to write pm schema to OUT_DIR");

    let pm_src_path = Path::new(&crate_dir).join("schema/provider-model-config.schema.json");
    fs::write(&pm_src_path, &pm_json).expect("failed to write pm schema to source tree");

    println!(
        "cargo:warning=Generated ProviderModelConfig JSON Schema at: {}",
        pm_src_path.display()
    );
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryTextPayload {
    pub query_id: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolStartPayload {
    pub query_id: String,
    pub tool_use_id: String,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolResultPayload {
    pub query_id: String,
    pub tool_use_id: String,
    pub tool_name: String,
    pub result: String,
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolProgressPayload {
    pub query_id: String,
    pub tool_use_id: String,
    pub tool_name: String,
    pub progress: f32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ThinkingPayload {
    pub query_id: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BackgroundTaskUpdate {
    pub task_id: String,
    pub status: String,
    pub prompt: String,
    pub output: String,
    pub started_at: i64,
    pub completed_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BackgroundTaskInfo {
    pub task_id: String,
    pub prompt: String,
    pub status: String,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    pub output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UsagePayload {
    pub query_id: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryCompletedPayload {
    pub query_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryFailedPayload {
    pub query_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PermissionRequest {
    pub tool: String,
    pub input: serde_json::Value,
    pub risk: String,
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionInfo {
    pub id: String,
    pub title: String,
    pub created_at: i64,
    pub message_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch_point: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionLoaded {
    pub messages: Vec<ChatMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryCancelledPayload {
    pub query_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConfigUpdatedPayload {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HunkAction {
    pub line_start: u32,
    pub line_end: u32,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UpdateAvailablePayload {
    pub version: String,
    pub date: Option<String>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UpdateProgressPayload {
    pub progress: f32,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DiffFileInfo {
    pub path: String,
    pub status: String,
    pub hunks: Vec<DiffHunk>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiffHunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskStepPayload {
    pub task_id: String,
    pub run_id: String,
    pub step_index: usize,
    pub step_total: usize,
    pub step_label: String,
    pub status: String,
    pub error: Option<String>,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskRetryPayload {
    pub task_id: String,
    pub run_id: String,
    pub attempt: usize,
    pub max_attempts: usize,
    pub delay_ms: u64,
    pub last_error: String,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EventEnvelope<T> {
    pub schema_version: u32,
    pub event: String,
    pub payload: T,
}

// ── ProviderModelConfig redeclarations (Φ0.5) ───────────────────────
// These MUST match src/provider_config.rs exactly. build.rs is a separate
// translation unit, so types are duplicated here to drive schemars generation.
// Drift between these and src/ is detected only at build time — if Cargo.toml
// depends on this schema, keep both files in sync.

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

#[derive(Debug, Clone, PartialEq, Eq, JsonSchema, Serialize, Deserialize)]
#[serde(tag = "backend", rename_all = "snake_case")]
pub enum CredentialRef {
    Env { var: String },
    Keyring { service: String, account: String },
    InlineLegacy { masked: String },
    Ephemeral,
}

#[derive(Debug, Clone, PartialEq, Eq, JsonSchema, Serialize, Deserialize)]
pub struct ActiveTarget {
    pub provider_id: String,
    pub model_id: String,
    pub scope: Scope,
}

#[derive(Debug, Clone, Copy, PartialEq, JsonSchema, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TemperatureStrategy {
    #[default]
    Default,
    Omit,
}

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
    pub models_url: Option<String>,
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
    pub version: u32,
    pub profiles: HashMap<String, ModelProfile>,
    /// B3 契约：网关多 profile 路由（默认 off，字节级等同单 profile）
    #[serde(default)]
    pub gateway: GatewayConfig,
}

/// C1 两层凭据解析：默认 Shared（沿用旧单 profile 语义）
#[derive(Debug, Clone, Copy, PartialEq, Eq, JsonSchema, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CredentialScope {
    #[default]
    Shared,
    Isolated,
}

/// B3 契约：profile 路由条目
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

/// B3 契约：网关级 multiplex 路由配置
#[derive(Debug, Clone, Default, PartialEq, Eq, JsonSchema, Serialize, Deserialize)]
pub struct GatewayConfig {
    #[serde(default)]
    pub multiplex_profiles: bool,
    #[serde(default)]
    pub profile_routes: Vec<ProfileRoute>,
}
