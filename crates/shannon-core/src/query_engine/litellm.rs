//! # LiteLLM community model pricing
//!
//! Live per-model pricing from LiteLLM's
//! [`model_prices_and_context_window.json`](LITELLM_PRICES_URL), cached at
//! `~/.shannon/cache/litellm-prices.json` with a 24h TTL. This prices
//! **dynamic/custom models** that are absent from the curated catalog so the
//! model picker and cost estimates show real numbers instead of the blind
//! `$3/$15` `super::types` fallback.
//!
//! The layer is **strictly below** the curated sources: catalog ids/aliases,
//! `DEFAULT_PRICING`, and file/env overrides all win first (see
//! [`super::types::pricing_for_model_opt`]). LiteLLM only fills genuinely
//! unknown models.
//!
//! Like the models.dev overlay, this module never constructs its own runtime:
//! [`refresh_async`] is driven by the caller's runtime (e.g. `/model refresh`),
//! while every other entry point is sync and reads only the on-disk cache, so
//! headless/CI never block on the network. Any error — DNS, timeout, non-200,
//! malformed JSON, missing home dir — silently falls back to "no LiteLLM data"
//! (the existing fallback then applies). No panic, no crash.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::types::ModelPricing;

/// Canonical LiteLLM model-pricing endpoint (per-token costs).
pub const LITELLM_PRICES_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";

/// Cache freshness window (24h) — matches the models.dev overlay.
const TTL_SECS: u64 = 24 * 60 * 60;

/// Fetch timeout for `/model refresh`.
pub const DEFAULT_FETCH_TIMEOUT: Duration = Duration::from_secs(15);

// ── Overlay store ──────────────────────────────────────────────────

/// Global pricing overlay. Empty until populated from cache or a refresh —
/// "empty" means "no LiteLLM data; use the existing fallback".
static PRICING_OVERLAY: OnceLock<Mutex<HashMap<String, ModelPricing>>> = OnceLock::new();

/// Guards the one-shot lazy cache read so lookups do not touch disk on every
/// call. [`refresh_async`] populates the overlay directly and sets this.
static OVERLAY_INITIALIZED: OnceLock<()> = OnceLock::new();

fn overlay() -> &'static Mutex<HashMap<String, ModelPricing>> {
    PRICING_OVERLAY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Best-effort, idempotent population of the overlay from the on-disk cache.
/// Never touches the network. Called lazily by [`lookup_pricing`].
pub fn ensure_overlay_loaded() {
    if OVERLAY_INITIALIZED.get().is_some() {
        return;
    }
    if let Some(payload) = load_cached_payload() {
        if let Ok(table) = parse_litellm(&payload) {
            if let Ok(mut g) = overlay().lock() {
                *g = table;
            }
        }
    }
    let _ = OVERLAY_INITIALIZED.set(());
}

// ── LiteLLM schema ─────────────────────────────────────────────────

/// One LiteLLM pricing entry. Costs are **per-token**; we convert to
/// per-million-token during parse. All fields optional + `#[serde(default)]`
/// so non-model keys (e.g. `sample_spec`, metadata) are skipped when they lack
/// cost fields.
#[derive(Default, Deserialize)]
struct LitellmEntry {
    #[serde(default)]
    input_cost_per_token: Option<f64>,
    #[serde(default)]
    output_cost_per_token: Option<f64>,
}

/// Parse the raw LiteLLM payload into a `model-key → ModelPricing` map.
///
/// Entries missing either cost (the `sample_spec` sentinel, provider-only
/// rows, …) are dropped. Per-token costs are scaled to per-million-token to
/// match Shannon's `ModelPricing` units. Negative or non-finite costs are
/// rejected (a corrupted feed must not bypass budget limits).
pub fn parse_litellm(payload: &str) -> Result<HashMap<String, ModelPricing>, LitellmPricingError> {
    let raw: HashMap<String, LitellmEntry> =
        serde_json::from_str(payload).map_err(|e| LitellmPricingError::Parse(e.to_string()))?;
    let mut table = HashMap::with_capacity(raw.len());
    for (key, entry) in raw {
        let (Some(input_per_token), Some(output_per_token)) =
            (entry.input_cost_per_token, entry.output_cost_per_token)
        else {
            continue;
        };
        if !input_per_token.is_finite()
            || !output_per_token.is_finite()
            || input_per_token < 0.0
            || output_per_token < 0.0
        {
            continue;
        }
        table.insert(
            key,
            ModelPricing {
                input_price_per_mtok: input_per_token * 1_000_000.0,
                output_price_per_mtok: output_per_token * 1_000_000.0,
            },
        );
    }
    Ok(table)
}

// ── Lookup ─────────────────────────────────────────────────────────

/// Resolve a model id to LiteLLM-sourced pricing, if known.
///
/// Matching order: exact key → the id after any `<provider>/` prefix (LiteLLM
/// keys are sometimes provider-prefixed) → substring (our id contains a known
/// key). Best-effort; returns `None` when nothing matches so the caller's
/// existing fallback applies.
pub fn lookup_pricing(model: &str) -> Option<ModelPricing> {
    ensure_overlay_loaded();
    let guard = overlay().lock().ok()?;
    let table = &*guard;

    if let Some(p) = table.get(model) {
        return Some(p.clone());
    }
    // LiteLLM keys are occasionally "<provider>/<model>"; try the bare tail.
    if let Some((_provider, tail)) = model.split_once('/') {
        if let Some(p) = table.get(tail) {
            return Some(p.clone());
        }
    }
    // Substring fallback (our id contains a known LiteLLM key).
    for (key, pricing) in table.iter() {
        if model.contains(key.as_str()) {
            return Some(pricing.clone());
        }
    }
    None
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

/// `~/.shannon/cache/litellm-prices.json` (same `cache/` dir as models.dev).
/// `None` if the home directory cannot be determined.
fn cache_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".shannon").join("cache").join("litellm-prices.json"))
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

fn save_cache_at(path: &Path, payload: &str, now: u64) -> Result<(), LitellmPricingError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| LitellmPricingError::Io(e.to_string()))?;
    }
    let env = CacheEnvelope {
        fetched_at: now,
        payload: payload.to_string(),
    };
    let bytes = serde_json::to_vec(&env).map_err(|e| LitellmPricingError::Io(e.to_string()))?;
    std::fs::write(path, bytes).map_err(|e| LitellmPricingError::Io(e.to_string()))
}

/// Load a fresh cached payload from the default path, if any.
fn load_cached_payload() -> Option<String> {
    let path = cache_path()?;
    load_cached_payload_at(&path, now_secs())
}

// ── Network ────────────────────────────────────────────────────────

/// Fetch the LiteLLM payload over HTTP. Driven by the caller's runtime
/// (e.g. `repl.runtime.block_on(...)`); the module never constructs its own
/// tokio runtime.
pub async fn fetch_litellm(timeout: Duration) -> Result<String, LitellmPricingError> {
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| LitellmPricingError::Network(e.to_string()))?;
    let resp = client
        .get(LITELLM_PRICES_URL)
        .send()
        .await
        .map_err(|e| LitellmPricingError::Network(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(LitellmPricingError::Status(resp.status().as_u16()));
    }
    resp.text()
        .await
        .map_err(|e| LitellmPricingError::Network(e.to_string()))
}

/// Fetch → persist cache → rebuild overlay. Returns the number of priced models
/// loaded. On error the existing overlay is left untouched (fail-open).
pub async fn refresh_async(timeout: Duration) -> Result<usize, LitellmPricingError> {
    let payload = fetch_litellm(timeout).await?;
    let table = parse_litellm(&payload)?;
    if let Some(path) = cache_path() {
        let _ = save_cache_at(&path, &payload, now_secs());
    }
    let count = table.len();
    if let Ok(mut g) = overlay().lock() {
        *g = table;
    }
    // Cache is now authoritative; prevent a later lazy read from overriding it.
    let _ = OVERLAY_INITIALIZED.set(());
    Ok(count)
}

// ── Errors ─────────────────────────────────────────────────────────

/// Failures from the LiteLLM pricing pipeline. All variants are non-fatal —
/// callers surface a message and fall back to the existing pricing estimate.
#[derive(Debug)]
pub enum LitellmPricingError {
    Network(String),
    Status(u16),
    Parse(String),
    Io(String),
}

impl std::fmt::Display for LitellmPricingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Network(m) => write!(f, "network error: {m}"),
            Self::Status(c) => write!(f, "HTTP {c}"),
            Self::Parse(m) => write!(f, "parse error: {m}"),
            Self::Io(m) => write!(f, "io error: {m}"),
        }
    }
}

impl std::error::Error for LitellmPricingError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_converts_per_token_to_per_mtok_and_drops_costless() {
        // Anthropic Sonnet-ish: $0.000003/token in, $0.000015/token out.
        let payload = r#"{
            "claude-sonnet-4-20250514": {
                "input_cost_per_token": 0.000003,
                "output_cost_per_token": 0.000015
            },
            "anthropic/claude-opus-4": {
                "input_cost_per_token": 0.000015,
                "output_cost_per_token": 0.000075
            },
            "sample_spec": { "litellm_provider": "anthropic" },
            "free-local-model": {}
        }"#;
        let table = parse_litellm(payload).expect("parses");
        // Costless entries (sample_spec, free-local-model) dropped.
        assert_eq!(table.len(), 2);
        let sonnet = table.get("claude-sonnet-4-20250514").unwrap();
        assert!((sonnet.input_price_per_mtok - 3.0).abs() < 1e-6);
        assert!((sonnet.output_price_per_mtok - 15.0).abs() < 1e-6);
        let opus = table.get("anthropic/claude-opus-4").unwrap();
        assert!((opus.input_price_per_mtok - 15.0).abs() < 1e-6);
    }

    #[test]
    fn parse_rejects_garbage_and_negative_costs() {
        assert!(parse_litellm("not json").is_err());
        let payload = r#"{
            "bad-negative": { "input_cost_per_token": -1.0, "output_cost_per_token": 0.0 },
            "nan-input": { "input_cost_per_token": null, "output_cost_per_token": 0.0 }
        }"#;
        let table = parse_litellm(payload).expect("parses");
        assert!(table.is_empty(), "negative/null costs dropped");
    }

    /// Populate the in-memory overlay directly (bypasses disk), then restore.
    fn with_overlay<F: FnOnce()>(table: HashMap<String, ModelPricing>, f: F) {
        // Force-init the OnceLocks so the test overlay is authoritative.
        let _ = OVERLAY_INITIALIZED.set(());
        if let Ok(mut g) = overlay().lock() {
            *g = table;
        }
        f();
        if let Ok(mut g) = overlay().lock() {
            g.clear();
        }
    }

    #[test]
    fn lookup_exact_and_strips_provider_prefix() {
        let mut table = HashMap::new();
        table.insert(
            "claude-sonnet-4-20250514".to_string(),
            ModelPricing {
                input_price_per_mtok: 3.0,
                output_price_per_mtok: 15.0,
            },
        );
        table.insert(
            "anthropic/claude-opus-4".to_string(),
            ModelPricing {
                input_price_per_mtok: 15.0,
                output_price_per_mtok: 75.0,
            },
        );
        with_overlay(table, || {
            // Exact bare id.
            let p = lookup_pricing("claude-sonnet-4-20250514").unwrap();
            assert!((p.input_price_per_mtok - 3.0).abs() < 1e-9);
            // Prefixed id resolves via the bare tail.
            let p = lookup_pricing("anthropic/claude-opus-4").unwrap();
            assert!((p.input_price_per_mtok - 15.0).abs() < 1e-9);
            // Unknown model → None (caller falls back).
            assert!(lookup_pricing("totally-unknown-model").is_none());
        });
    }
}
