//! # Dynamic model catalog (models.dev)
//!
//! Phase D (ADR-0005): augments the built-in [`super::MODEL_CATALOG`] with
//! live model data fetched from <https://models.dev/models.json>. The dynamic
//! layer is **strictly additive and offline-safe**:
//!
//! - The static catalog always wins: a dynamic entry whose model id already
//!   exists for a provider is dropped, so curated metadata (pricing, aliases,
//!   Phase B beta headers living in the engine) is never clobbered.
//! - Network is opt-in. `/model refresh` performs an async fetch; the picker
//!   only reads the on-disk cache (no network) so the UI works fully offline.
//! - Any error — DNS, timeout, non-200, malformed JSON, missing home dir —
//!   silently falls back to the static catalog. No panic, no crash.
//!
//! models.dev's JSON is a flat map of `"<provider-slug>/<model-id>"` → entry.
//! Notably it carries **no pricing**, so dynamic entries only fill context
//! window, max output, and capability flags; pricing stays `0.0` (unknown) and
//! is inherited from the static catalog where one exists.
//!
//! Only providers Shannon ships as first-class [`LlmProvider`] variants are
//! surfaced; unknown slugs (`meta`, `nvidia`, `xiaomi`, …) are filtered out.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use shannon_engine::api::LlmProvider;

use super::{ModelCapabilities, ModelInfo, TierLabel};

/// Canonical models.dev model registry endpoint.
pub const MODELS_DEV_URL: &str = "https://models.dev/models.json";

/// Cache freshness window (24h).
const TTL_SECS: u64 = 24 * 60 * 60;

/// Fetch timeout for `/model refresh`.
pub const DEFAULT_FETCH_TIMEOUT: Duration = Duration::from_secs(15);

// ── Overlay store ──────────────────────────────────────────────────

/// Global dynamic overlay. Empty until populated from cache or a refresh —
/// which means "no overlay" == "static catalog only" (the offline default).
static DYNAMIC_OVERLAY: OnceLock<Mutex<Vec<ModelInfo>>> = OnceLock::new();

/// Guards the one-shot lazy cache read so the picker does not touch disk on
/// every open. `/model refresh` populates the overlay directly and sets this.
static OVERLAY_INITIALIZED: OnceLock<()> = OnceLock::new();

fn overlay() -> &'static Mutex<Vec<ModelInfo>> {
    DYNAMIC_OVERLAY.get_or_init(|| Mutex::new(Vec::new()))
}

/// Snapshot the overlay for merge. Empty on lock poisoning (fail-open to
/// static).
pub fn overlay_snapshot() -> Vec<ModelInfo> {
    overlay().lock().map(|g| g.clone()).unwrap_or_default()
}

/// Best-effort, idempotent population of the overlay from the on-disk cache.
/// Never touches the network. Called lazily by
/// [`super::merged_models_for_provider`].
pub fn ensure_overlay_loaded() {
    if OVERLAY_INITIALIZED.get().is_some() {
        return;
    }
    if let Some(payload) = load_cached_payload() {
        let models = build_overlay_from_payload(&payload);
        if let Ok(mut g) = overlay().lock() {
            *g = models;
        }
    }
    let _ = OVERLAY_INITIALIZED.set(());
}

// ── models.dev schema ──────────────────────────────────────────────

/// A single entry in the models.dev registry. All optional fields use
/// `#[serde(default)]` and unknown fields are ignored, so schema drift degrades
/// gracefully rather than failing the whole parse.
#[derive(Debug, Default, Deserialize)]
pub struct ModelsDevEntry {
    /// Full id, always `"<provider-slug>/<model-id>"`.
    pub id: String,
    /// Human-readable name (falls back to the model id when absent).
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub tool_call: Option<bool>,
    #[serde(default)]
    pub reasoning: Option<bool>,
    #[serde(default)]
    pub modalities: Option<Modalities>,
    #[serde(default)]
    pub limit: Option<Limit>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Modalities {
    #[serde(default)]
    pub input: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Limit {
    #[serde(default)]
    pub context: Option<usize>,
    #[serde(default)]
    pub output: Option<usize>,
}

/// Parse the raw models.dev payload into entries. The map is keyed by
/// `provider/model-id`; values are collected, order determined by the map.
pub fn parse_models_dev(payload: &str) -> Result<Vec<ModelsDevEntry>, DynamicCatalogError> {
    let map: BTreeMap<String, ModelsDevEntry> =
        serde_json::from_str(payload).map_err(|e| DynamicCatalogError::Parse(e.to_string()))?;
    Ok(map.into_values().collect())
}

/// Map a models.dev provider slug to a first-class Shannon provider.
///
/// Returns `None` for providers Shannon does not ship (Meta, NVIDIA, Xiaomi,
/// Tencent, StepFun, …) — those are reachable only via an aggregator
/// (OpenRouter / Together / Bedrock), never as a native tab.
pub fn slug_to_provider(slug: &str) -> Option<LlmProvider> {
    match slug {
        "anthropic" => Some(LlmProvider::Anthropic),
        "openai" => Some(LlmProvider::OpenAI),
        "google" => Some(LlmProvider::Gemini),
        "deepseek" => Some(LlmProvider::DeepSeek),
        "mistral" => Some(LlmProvider::Mistral),
        "xai" => Some(LlmProvider::Xai),
        "cohere" => Some(LlmProvider::Cohere),
        "moonshotai" => Some(LlmProvider::Moonshot),
        "perplexity" => Some(LlmProvider::Perplexity),
        "zhipuai" => Some(LlmProvider::Zhipu),
        "minimax" => Some(LlmProvider::Minimax),
        "alibaba" => Some(LlmProvider::DashScope),
        _ => None,
    }
}

/// Leaked `&'static str` from an owned string. Matches the [`super::detect_local_models`]
/// precedent — bounded by the finite universe of model ids (~hundreds) and a CLI
/// process lifetime, never a long-running server.
fn leak_str(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

/// Convert a parsed entry into a catalog [`ModelInfo`], or `None` if the
/// provider slug is not a first-class Shannon provider.
pub fn entry_to_model_info(entry: &ModelsDevEntry) -> Option<ModelInfo> {
    let (slug, model_id) = entry.id.split_once('/')?;
    let provider = slug_to_provider(slug)?;
    if model_id.is_empty() {
        return None;
    }

    let id = leak_str(model_id);
    let display = leak_str(entry.name.as_deref().unwrap_or(model_id));
    let limit = entry.limit.as_ref();
    let context_window = limit.and_then(|l| l.context).unwrap_or(200_000);
    let max_output = limit.and_then(|l| l.output).unwrap_or(8_192);

    let mut caps = ModelCapabilities::empty();
    if entry.reasoning.unwrap_or(false) {
        caps = caps.or(ModelCapabilities::reasoning());
    }
    // Tool-calling is the best available proxy for "coding-capable".
    if entry.tool_call.unwrap_or(false) {
        caps = caps.or(ModelCapabilities::coding());
    }
    let lower = model_id.to_lowercase();
    if ["flash", "mini", "nano", "haiku", "turbo", "lite", "air"]
        .iter()
        .any(|k| lower.contains(k))
    {
        caps = caps
            .or(ModelCapabilities::speed())
            .or(ModelCapabilities::cheap());
    }
    if entry
        .modalities
        .as_ref()
        .is_some_and(|m| m.input.iter().any(|x| x == "image"))
    {
        caps = caps.or(ModelCapabilities::vision());
    }

    let mut info = ModelInfo {
        id,
        display_name: display,
        aliases: &[],
        provider,
        context_window,
        max_output,
        cost_per_m_input: 0.0,
        cost_per_m_output: 0.0,
        capabilities: caps,
    };

    // The picker filters models by tier (Fast/Standard/Pro); `Unknown`-tier
    // models would be hidden. Floor anything that did not classify into
    // Standard so freshly discovered models remain selectable.
    if info.tier_label() == TierLabel::Unknown {
        info.capabilities = caps.or(ModelCapabilities::coding());
    }
    Some(info)
}

/// Parse + convert a payload into the dynamic overlay set.
fn build_overlay_from_payload(payload: &str) -> Vec<ModelInfo> {
    match parse_models_dev(payload) {
        Ok(entries) => entries.iter().filter_map(entry_to_model_info).collect(),
        Err(_) => Vec::new(),
    }
}

// ── Cache ──────────────────────────────────────────────────────────

/// On-disk cache envelope: raw payload + fetch timestamp.
#[derive(Debug, Serialize, Deserialize)]
struct CacheEnvelope {
    fetched_at: u64,
    payload: String,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `~/.shannon/cache/models-dev.json` (the same `cache/` dir used by
/// housekeeping). `None` if the home directory cannot be determined.
fn cache_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".shannon").join("cache").join("models-dev.json"))
}

/// Whether a cached envelope is still fresh relative to `now`.
fn is_fresh(fetched_at: u64, now: u64) -> bool {
    now.saturating_sub(fetched_at) <= TTL_SECS
}

fn load_cached_payload_at(path: &Path, now: u64) -> Option<String> {
    let data = std::fs::read(path).ok()?;
    let env: CacheEnvelope = serde_json::from_slice(&data).ok()?;
    if !is_fresh(env.fetched_at, now) {
        return None;
    }
    Some(env.payload)
}

fn save_cache_at(path: &Path, payload: &str, now: u64) -> Result<(), DynamicCatalogError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| DynamicCatalogError::Io(e.to_string()))?;
    }
    let env = CacheEnvelope {
        fetched_at: now,
        payload: payload.to_string(),
    };
    let bytes = serde_json::to_vec(&env).map_err(|e| DynamicCatalogError::Io(e.to_string()))?;
    std::fs::write(path, bytes).map_err(|e| DynamicCatalogError::Io(e.to_string()))
}

/// Load a fresh cached payload from the default path, if any.
fn load_cached_payload() -> Option<String> {
    let path = cache_path()?;
    load_cached_payload_at(&path, now_secs())
}

// ── Network ────────────────────────────────────────────────────────

/// Fetch the models.dev payload over HTTP. Driven by the caller's runtime
/// (e.g. `repl.runtime.block_on(...)`); the module never constructs its own
/// tokio runtime, so it cannot trigger the "runtime within runtime" panic.
pub async fn fetch_models_dev(timeout: Duration) -> Result<String, DynamicCatalogError> {
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| DynamicCatalogError::Network(e.to_string()))?;
    let resp = client
        .get(MODELS_DEV_URL)
        .send()
        .await
        .map_err(|e| DynamicCatalogError::Network(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(DynamicCatalogError::Status(resp.status().as_u16()));
    }
    resp.text()
        .await
        .map_err(|e| DynamicCatalogError::Network(e.to_string()))
}

/// Fetch → persist cache → rebuild overlay. Returns the number of models loaded
/// into the overlay. On error the existing overlay/static catalog is left
/// untouched (fail-open).
pub async fn refresh_overlay_async(timeout: Duration) -> Result<usize, DynamicCatalogError> {
    let payload = fetch_models_dev(timeout).await?;
    if let Some(path) = cache_path() {
        let _ = save_cache_at(&path, &payload, now_secs());
    }
    let models = build_overlay_from_payload(&payload);
    let count = models.len();
    if let Ok(mut g) = overlay().lock() {
        *g = models;
    }
    // Cache is now authoritative; prevent a later lazy read from overriding it.
    let _ = OVERLAY_INITIALIZED.set(());
    Ok(count)
}

// ── Errors ─────────────────────────────────────────────────────────

/// Failures from the dynamic catalog pipeline. All variants are non-fatal —
/// callers surface a message and fall back to the static catalog.
#[derive(Debug)]
pub enum DynamicCatalogError {
    Network(String),
    Status(u16),
    Parse(String),
    Io(String),
}

impl std::fmt::Display for DynamicCatalogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Network(m) => write!(f, "network error: {m}"),
            Self::Status(c) => write!(f, "HTTP {c}"),
            Self::Parse(m) => write!(f, "parse error: {m}"),
            Self::Io(m) => write!(f, "io error: {m}"),
        }
    }
}

impl std::error::Error for DynamicCatalogError {}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn parse_handles_minimal_and_unknown_fields() {
        let payload = r#"{
            "anthropic/claude-sonnet-4-6": {
                "id": "anthropic/claude-sonnet-4-6",
                "name": "Claude Sonnet 4.6",
                "tool_call": true,
                "reasoning": true,
                "limit": {"context": 1000000, "output": 64000},
                "modalities": {"input": ["text", "image"], "output": ["text"]},
                "benchmarks": [{"name": "x", "score": 1}]
            },
            "nvidia/nemotron-nano": {
                "id": "nvidia/nemotron-nano",
                "name": "Nemotron Nano"
            }
        }"#;
        let entries = parse_models_dev(payload).expect("parses");
        assert_eq!(entries.len(), 2);
        let claude = entries
            .iter()
            .find(|e| e.id == "anthropic/claude-sonnet-4-6")
            .unwrap();
        assert_eq!(claude.limit.as_ref().unwrap().context, Some(1_000_000));
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(parse_models_dev("not json").is_err());
    }

    #[test]
    fn slug_mapping_covers_supported_drops_unknown() {
        assert_eq!(slug_to_provider("anthropic"), Some(LlmProvider::Anthropic));
        assert_eq!(slug_to_provider("google"), Some(LlmProvider::Gemini));
        assert_eq!(slug_to_provider("alibaba"), Some(LlmProvider::DashScope));
        assert_eq!(slug_to_provider("nvidia"), None);
        assert_eq!(slug_to_provider("meta"), None);
        assert_eq!(slug_to_provider("xiaomi"), None);
    }

    #[test]
    fn entry_classifies_fast_and_standard_and_drops_unknown_provider() {
        let flash = ModelsDevEntry {
            id: "google/gemini-3.5-flash".into(),
            name: Some("Gemini 3.5 Flash".into()),
            tool_call: Some(true),
            reasoning: Some(true),
            modalities: Some(Modalities {
                input: vec!["image".into()],
            }),
            limit: Some(Limit {
                context: Some(1_048_576),
                output: Some(65_536),
            }),
        };
        let info = entry_to_model_info(&flash).expect("flash maps to Gemini");
        assert_eq!(info.provider, LlmProvider::Gemini);
        assert_eq!(info.context_window, 1_048_576);
        assert_eq!(info.tier_label(), TierLabel::Fast);
        assert!(info.capabilities.has(ModelCapabilities::vision()));

        let glm = ModelsDevEntry {
            id: "zhipuai/glm-5".into(),
            name: Some("GLM-5".into()),
            tool_call: Some(true),
            reasoning: Some(true),
            ..Default::default()
        };
        let glm_info = entry_to_model_info(&glm).expect("glm maps to Zhipu");
        assert_eq!(glm_info.tier_label(), TierLabel::Standard);

        // Unknown provider slug → filtered out.
        let nemo = ModelsDevEntry {
            id: "nvidia/nemotron-nano".into(),
            name: None,
            ..Default::default()
        };
        assert!(entry_to_model_info(&nemo).is_none());
    }

    #[test]
    fn unknown_tier_is_floored_to_standard() {
        // No reasoning, no tool_call, no flash/mini marker, no pro marker →
        // would be Unknown; the floor must lift it to Standard so the picker
        // tier filter does not hide it.
        let e = ModelsDevEntry {
            id: "perplexity/sonar".into(),
            name: Some("Sonar".into()),
            tool_call: Some(false),
            reasoning: Some(false),
            ..Default::default()
        };
        let info = entry_to_model_info(&e).expect("maps");
        assert_eq!(info.tier_label(), TierLabel::Standard);
    }

    #[test]
    fn cache_roundtrip_and_freshness() {
        let tmp = NamedTempFile::new().unwrap();
        let now = now_secs();
        save_cache_at(tmp.path(), r#"{"x":1}"#, now).expect("save");
        let loaded = load_cached_payload_at(tmp.path(), now).expect("fresh load");
        assert!(loaded.contains(r#""x":1"#));

        // Stale (>24h) → None.
        assert_eq!(load_cached_payload_at(tmp.path(), now + TTL_SECS + 1), None);
    }

    #[test]
    fn build_overlay_skips_unparseable() {
        // Garbage payload → empty overlay, never a panic.
        assert!(build_overlay_from_payload("not json").is_empty());
    }

    #[test]
    fn merge_static_priority_dedup_by_id() {
        // Pure merge logic lives in the parent module; exercise it via the
        // public API with a known static provider. The static catalog has at
        // least one Anthropic model, so the merge must keep it exactly once
        // and never panic on an empty overlay.
        let merged = crate::model_registry::merge_static_and_dynamic(LlmProvider::Anthropic, &[]);
        assert!(merged.iter().any(|m| m.provider == LlmProvider::Anthropic));
        // Simulate a dynamic duplicate of a static id + a brand-new id.
        let dup = entry_to_model_info(&ModelsDevEntry {
            id: "anthropic/claude-sonnet-4-6".into(),
            name: Some("dup".into()),
            tool_call: Some(true),
            reasoning: Some(true),
            ..Default::default()
        })
        .unwrap();
        let fresh = entry_to_model_info(&ModelsDevEntry {
            id: "anthropic/claude-future-9".into(),
            name: Some("Future".into()),
            tool_call: Some(true),
            reasoning: Some(true),
            ..Default::default()
        })
        .unwrap();
        let merged =
            crate::model_registry::merge_static_and_dynamic(LlmProvider::Anthropic, &[dup, fresh]);
        let sonnet_count = merged
            .iter()
            .filter(|m| m.id == "claude-sonnet-4-6")
            .count();
        assert_eq!(sonnet_count, 1, "static dedups dynamic duplicate");
        assert!(merged.iter().any(|m| m.id == "claude-future-9"));
    }
}
