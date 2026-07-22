//! C1 v1→v2 provider/model migration — Φ1a (pure structural mapping).
//!
//! Maps the v1 legacy provider/model configuration shape into the v2
//! [`crate::provider_config::ProviderModelConfig`]. This is the migration
//! contract for decision **C1** (one-shot v1→v2 migration); the v2 schema
//! carries a top-level `version` field precisely so this can run once.
//!
//! # Purity
//! No I/O, no secret resolution. Per decision **A1**, v1 plaintext `api_key`
//! is **never** carried into v2: the migrator emits a [`CredentialRef::Env`]
//! reference (`SHANNON_<PROVIDER>_API_KEY`, or the legacy `api_key_env` when
//! one was set). Writing the legacy value into `~/.shannon/secrets.env`
//! (chmod 0600) so the env ref resolves is the job of the Φ1b runtime adapter.
//!
//! # Placement
//! Lives in `shannon-types` (the protocol crate) so the contract sits next to
//! the v2 type it targets, with **no dependency on `shannon-core`**. The Φ1b
//! runtime adapter constructs a [`LegacyV1Config`] from
//! `shannon_core::ShannonConfig` by a trivial field copy.
//!
//! # Scope
//! Only the provider/model-relevant v1 fields migrate. Settings orthogonal to
//! provider/model (`temperature`, `timeout`, `debug`, `presets`,
//! `notifications`, `permission_profile`, …) are **not** part of the
//! [`ProviderModelConfig`] contract and survive migration unchanged in their
//! own config surface — hence their absence from [`LegacyV1Config`].
//!
//! # Single-profile milestone (B3 phase 1)
//! Emits exactly one profile named `"default"`. Gateway multiplex routing
//! stays at its default (off), so the result is byte-equivalent to
//! single-profile behaviour — the v2 schema carries the multiplex fields from
//! Φ0 for forward use only.

use crate::provider_config::{
    ActiveTarget, CredentialRef, CredentialScope, GatewayConfig, ModelProfile, ProviderKind,
    ProviderModelConfig, ProviderProfile, ProviderQuirks, Scope,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// The v1 legacy provider/model configuration shape — migration input.
///
/// Mirrors the provider/model-relevant subset of `shannon_core::ShannonConfig`.
/// See the [module docs](self) for why orthogonal fields are absent and why
/// `api_key` is captured but never carried into v2.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LegacyV1Config {
    pub model: Option<String>,
    pub provider: Option<String>,
    /// Captured for fidelity; **never** copied into v2 (decision A1).
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub providers: HashMap<String, LegacyV1ProviderEntry>,
}

/// A v1 `[providers.<name>]` entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LegacyV1ProviderEntry {
    /// Captured for fidelity; **never** copied into v2 (decision A1).
    pub api_key: Option<String>,
    /// If set, becomes the [`CredentialRef::Env`] variable name (preferred).
    pub api_key_env: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
}

/// Non-recoverable migration failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MigrationError {
    /// The v1 config names no provider and defines no `[providers.*]` entries.
    #[error("v1 config has no provider and no [providers.*] entries — nothing to migrate")]
    NoProvider,
    /// An unknown (OpenAI-compatible) provider was given no `base_url`.
    #[error("provider {provider:?} is OpenAI-compatible but has no base_url; v2 requires one")]
    OpenAiCompatibleNeedsBaseUrl { provider: String },
    /// An unknown (OpenAI-compatible) provider was given no model.
    #[error("provider {provider:?} is OpenAI-compatible but has no model; v2 requires one")]
    OpenAiCompatibleNeedsModel { provider: String },
}

/// Migrate a v1 legacy config into a v2 [`ProviderModelConfig`].
///
/// See the [module docs](self) for the full contract. Returns
/// [`MigrationError::NoProvider`] when there is nothing to migrate.
pub fn migrate_v1(legacy: &LegacyV1Config) -> Result<ProviderModelConfig, MigrationError> {
    // ── 1. Pick the active provider + an ordered list of all providers ─────
    // Deterministic ordering: BTreeMap over the (unordered) HashMap. The
    // active provider is the explicit top-level `provider`, else the
    // lexicographically smallest `[providers.*]` key.
    let sorted: BTreeMap<&str, &LegacyV1ProviderEntry> = legacy
        .providers
        .iter()
        .map(|(k, v)| (k.as_str(), v))
        .collect();

    let active_name = legacy
        .provider
        .as_deref()
        .or_else(|| sorted.keys().next().copied())
        .ok_or(MigrationError::NoProvider)?
        .to_string();

    // Active first, then the rest in sorted order (active never repeated).
    let mut ordered_names: Vec<String> = vec![active_name.clone()];
    for k in sorted.keys() {
        if *k != active_name.as_str() {
            ordered_names.push((*k).to_string());
        }
    }

    // ── 2. Resolve every provider into a fully-populated definition ────────
    let mut defs: Vec<ProviderDef> = Vec::with_capacity(ordered_names.len());
    for name in &ordered_names {
        defs.push(resolve_def(legacy, name)?);
    }
    // Invariant: `active_name` was pushed first, so defs[0] is the active def.
    let active = &defs[0];

    // ── 3. Build ProviderProfiles (active gets max_tokens) ────────────────
    let profiles: Vec<ProviderProfile> = defs
        .iter()
        .map(|d| {
            let default_max_tokens = if d.name == active.name {
                legacy.max_tokens
            } else {
                None
            };
            ProviderProfile {
                id: d.name.clone(),
                kind: d.kind.clone(),
                display_name: d.name.clone(),
                base_url: d.base_url.clone(),
                models_url: None,
                credential: CredentialRef::Env {
                    var: d.env_var.clone(),
                },
                extra_headers: HashMap::new(),
                default_max_tokens,
                fallback_models: Vec::new(),
                quirks: ProviderQuirks::default(),
            }
        })
        .collect();

    // ── 4. Assemble the single "default" profile + top-level doc ──────────
    let default_profile = ModelProfile {
        name: "default".to_string(),
        active_target: ActiveTarget {
            provider_id: active.name.clone(),
            model_id: active.model.clone(),
            scope: Scope::Global,
        },
        providers: profiles,
        auxiliary: HashMap::new(),
        credential_scope: CredentialScope::Shared,
    };

    let mut profile_map = HashMap::new();
    profile_map.insert("default".to_string(), default_profile);

    Ok(ProviderModelConfig {
        version: ProviderModelConfig::VERSION,
        profiles: profile_map,
        gateway: GatewayConfig::default(),
    })
}

/// A resolved, fully-populated provider definition (v2-ready).
struct ProviderDef {
    name: String,
    kind: ProviderKind,
    base_url: String,
    model: String,
    env_var: String,
}

/// Resolve one provider name (plus its map entry, plus top-level fallbacks
/// when it is the provider named by the top-level `provider` field) into a
/// complete [`ProviderDef`], filling kind-based defaults and erroring when an
/// OpenAI-compatible provider is underspecified.
fn resolve_def(legacy: &LegacyV1Config, name: &str) -> Result<ProviderDef, MigrationError> {
    let entry = legacy.providers.get(name);
    let kind = parse_kind(name);
    // Top-level base_url/model apply only to the provider the top-level
    // `provider` field actually names (otherwise they are ambiguous).
    let uses_top_level = legacy.provider.as_deref() == Some(name);

    let base_url = entry
        .and_then(|e| e.base_url.clone())
        .or_else(|| uses_top_level.then(|| legacy.base_url.clone()).flatten())
        .or_else(|| default_base_url(&kind).map(str::to_string))
        .ok_or(MigrationError::OpenAiCompatibleNeedsBaseUrl {
            provider: name.to_string(),
        })?;

    let model = entry
        .and_then(|e| e.model.clone())
        .or_else(|| uses_top_level.then(|| legacy.model.clone()).flatten())
        .or_else(|| default_model(&kind).map(str::to_string))
        .ok_or(MigrationError::OpenAiCompatibleNeedsModel {
            provider: name.to_string(),
        })?;

    let env_var = entry
        .and_then(|e| e.api_key_env.clone())
        .unwrap_or_else(|| default_env_var(name));

    Ok(ProviderDef {
        name: name.to_string(),
        kind,
        base_url,
        model,
        env_var,
    })
}

/// Parse a v1 provider name into a v2 [`ProviderKind`].
///
/// Unknown names collapse to [`ProviderKind::OpenAiCompatible`] (the realistic
/// case for a custom-named provider pointed at an OpenAI-compatible endpoint),
/// which [`resolve_def`] then requires to carry an explicit `base_url` + model.
fn parse_kind(name: &str) -> ProviderKind {
    match name.to_ascii_lowercase().as_str() {
        "anthropic" => ProviderKind::Anthropic,
        "openai" => ProviderKind::OpenAi,
        "openai-compatible" | "openai_compatible" => ProviderKind::OpenAiCompatible,
        "ollama" => ProviderKind::Ollama,
        "gemini" | "google" => ProviderKind::Gemini,
        "deepseek" => ProviderKind::Deepseek,
        _ => ProviderKind::OpenAiCompatible,
    }
}

/// Canonical base URL for a known [`ProviderKind`]; `None` for
/// [`ProviderKind::OpenAiCompatible`] (user-defined — must be supplied).
///
/// Consolidate with `shannon_engine::LlmProvider::default_base_url` into a
/// shared provider-metadata crate in a follow-up.
fn default_base_url(kind: &ProviderKind) -> Option<&'static str> {
    match kind {
        ProviderKind::Anthropic => Some("https://api.anthropic.com"),
        ProviderKind::OpenAi => Some("https://api.openai.com/v1"),
        ProviderKind::Ollama => Some("http://localhost:11434"),
        ProviderKind::Gemini => Some("https://generativelanguage.googleapis.com"),
        ProviderKind::Deepseek => Some("https://api.deepseek.com"),
        ProviderKind::OpenAiCompatible => None,
    }
}

/// Last-resort model id for a known [`ProviderKind`]; `None` for
/// [`ProviderKind::OpenAiCompatible`].
///
/// Values mirror `shannon_core`'s v1 runtime defaults to preserve behaviour;
/// consolidate into shared provider metadata in a follow-up.
fn default_model(kind: &ProviderKind) -> Option<&'static str> {
    match kind {
        ProviderKind::Anthropic => Some("claude-sonnet-4-20250514"),
        ProviderKind::OpenAi => Some("gpt-4o"),
        ProviderKind::Ollama => Some("llama3"),
        ProviderKind::Gemini => Some("gemini-1.5-pro"),
        ProviderKind::Deepseek => Some("deepseek-chat"),
        ProviderKind::OpenAiCompatible => None,
    }
}

/// Default credential env-var name for a provider: `SHANNON_<NAME>_API_KEY`
/// (ASCII-upper-cased, `-`/space → `_`).
fn default_env_var(name: &str) -> String {
    let stem: String = name
        .chars()
        .map(|c| match c {
            '-' | ' ' => '_',
            _ => c.to_ascii_uppercase(),
        })
        .collect();
    format!("SHANNON_{stem}_API_KEY")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_env_var_cases() {
        assert_eq!(default_env_var("anthropic"), "SHANNON_ANTHROPIC_API_KEY");
        assert_eq!(default_env_var("openai"), "SHANNON_OPENAI_API_KEY");
        assert_eq!(
            default_env_var("my-provider"),
            "SHANNON_MY_PROVIDER_API_KEY"
        );
    }

    #[test]
    fn parse_kind_known_and_unknown() {
        assert_eq!(parse_kind("Anthropic"), ProviderKind::Anthropic);
        assert_eq!(parse_kind("OpenAI"), ProviderKind::OpenAi);
        assert_eq!(parse_kind("ollama"), ProviderKind::Ollama);
        assert_eq!(parse_kind("google"), ProviderKind::Gemini);
        assert_eq!(parse_kind("deepseek"), ProviderKind::Deepseek);
        assert_eq!(parse_kind("zhipu"), ProviderKind::OpenAiCompatible);
        assert_eq!(
            parse_kind("openai-compatible"),
            ProviderKind::OpenAiCompatible
        );
    }
}
