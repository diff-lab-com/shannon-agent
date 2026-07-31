//! N1 — v2-native provider/model resolution.
//!
//! Bridges the v2 [`ProviderModelConfig`] (Φ0 schema) to the engine's runtime
//! types ([`LlmProvider`] + credential values), **without** the v1 flat config
//! fields. This is the resolution core the `ShannonConfig → LlmClientConfig`
//! adapter (N1 commit B) will call; it is additive and unit-tested in isolation.
//!
//! # Provider identity (the `ProviderKind` ↔ `LlmProvider` bridge)
//! The v2 [`ProviderKind`] is a coarse wire-protocol discriminator (~6 kinds);
//! the engine's [`LlmProvider`] names ~30 concrete providers. Identity is
//! derived from the profile's `base_url` via [`LlmProvider::from_base_url`]
//! **first** (preserves every known provider — Zhipu, Moonshot, DashScope, …),
//! falling back to the [`ProviderKind`] only when the host is unrecognized — so
//! an unknown OpenAI-compatible endpoint still gets the correct wire format.

use shannon_engine::api::LlmProvider;
use shannon_types::model_ref::ModelRef;
use shannon_types::provider_config::{
    ActiveTarget, CredentialRef, CredentialScope, ModelProfile, ProviderKind, ProviderModelConfig,
    ProviderProfile, ProviderTiers, Scope,
};

/// A resolved active target: the concrete engine provider, its source profile,
/// and the active model id. Borrows from the originating [`ProviderModelConfig`].
#[derive(Debug, Clone)]
pub struct ResolvedTarget<'a> {
    /// Concrete engine provider (drives wire format + endpoint path).
    pub provider: LlmProvider,
    /// The active provider profile (`base_url`, `credential`, `quirks`, …).
    pub profile: &'a ProviderProfile,
    /// The active model id from the profile's `active_target`.
    pub model_id: &'a str,
}

/// Resolve the active target of the `"default"` profile (B3 phase-1:
/// single-active-profile).
///
/// Returns `None` when there is no `"default"` profile, or its `active_target`
/// points at a provider id absent from `providers`.
///
/// The provider identity is recovered in this order (preserving the
/// pre-N1 explicit-name behaviour, where a user `--provider groq` flows
/// through to `LlmProvider::Groq` even when the base_url is a localhost
/// mock):
/// 1. **`profile.id` as an `LlmProvider` name** — recognised variants win
///    unconditionally (preserves the user's explicit intent).
/// 2. **fall back to [`resolve_provider`]** — base_url-driven detection
///    for unknown ids, then the coarse `ProviderKind` fallback.
pub fn resolve_active_target(pm: &ProviderModelConfig) -> Option<ResolvedTarget<'_>> {
    let profile = pm.profiles.get("default")?;
    let active = profile
        .providers
        .iter()
        .find(|p| p.id == profile.active_target.provider_id)?;
    let provider = llm_provider_from_id(&active.id)
        .unwrap_or_else(|| resolve_provider(&active.kind, &active.base_url));
    Some(ResolvedTarget {
        provider,
        profile: active,
        model_id: &profile.active_target.model_id,
    })
}

/// Reverse of [`llm_provider_id`]: map a profile id slug (e.g. `"groq"`,
/// `"zhipu"`, `"anthropic"`) back to its [`LlmProvider`] enum value. Returns
/// `None` for unknown slugs — callers should fall back to
/// [`resolve_provider`].
///
/// Thin delegate over [`llm_provider_from_slug`] (ADR-0008 Decision 1 / P2-3):
/// profile ids are always canonical lower-case Debug names produced by
/// [`llm_provider_id`], so the alias arms in `llm_provider_from_slug` never
/// fire for a real profile id — the delegation is behaviour-preserving while
/// collapsing three parallel provider-name tables into one.
pub(crate) fn llm_provider_from_id(id: &str) -> Option<LlmProvider> {
    llm_provider_from_slug(id)
}

/// The single source of truth for mapping a provider name slug (canonical or
/// alias) to an [`LlmProvider`] (ADR-0008 Decision 1 / P2-3).
///
/// This is the union of every provider-name table that previously existed
/// independently: the REPL's `parse_provider_name`, the config-loading
/// `provider_str_to_llm`, and the strict `llm_provider_from_id`. Adding a new
/// alias now means editing one match instead of three, and the exhaustive
/// `from_slug_*` tests guard against drift.
///
/// Input is trimmed and lower-cased, so `"Anthropic"`, `"  anthropic  "`, and
/// `"anthropic"` all resolve the same. Returns `None` for unknown names;
/// callers decide whether to fall back to [`LlmProvider::from_base_url`] or
/// surface an error.
pub fn llm_provider_from_slug(slug: &str) -> Option<LlmProvider> {
    use LlmProvider::*;
    match slug.trim().to_lowercase().as_str() {
        "anthropic" | "claude" => Some(Anthropic),
        "openai" | "gpt" | "chatgpt" => Some(OpenAI),
        "ollama" | "local" => Some(Ollama),
        "gemini" | "google" => Some(Gemini),
        "azure" | "azure-openai" => Some(Azure),
        "bedrock" | "aws" => Some(Bedrock),
        "mistral" | "mistral-ai" => Some(Mistral),
        "deepseek" | "ds" => Some(DeepSeek),
        "groq" => Some(Groq),
        "together" | "together-ai" => Some(Together),
        "openrouter" => Some(OpenRouter),
        "cohere" => Some(Cohere),
        "fireworks" => Some(Fireworks),
        "perplexity" => Some(Perplexity),
        "xai" | "grok" => Some(Xai),
        "ai21" => Some(Ai21),
        "siliconflow" | "sf" => Some(SiliconFlow),
        "zhipu" | "zhipu-cn" | "glm" => Some(Zhipu),
        "zhipuinternational" | "zhipu-international" | "zhipu-intl" | "glm-intl" => {
            Some(ZhipuInternational)
        }
        "zhipucoding" | "zhipu-coding" | "zhipu-anthropic" => Some(ZhipuCoding),
        "moonshot" | "kimi" => Some(Moonshot),
        "minimax" | "mm" => Some(Minimax),
        "dashscope" | "qwen" | "aliyun" => Some(DashScope),
        "cloudflare" | "cf" => Some(Cloudflare),
        "replicate" => Some(Replicate),
        _ => None,
    }
}

/// Map a v2 ([`ProviderKind`], `base_url`) pair to the engine's [`LlmProvider`].
///
/// `from_base_url` wins when it recognizes the host (preserves Zhipu/Moonshot/
/// DashScope/…). For unrecognized hosts the [`ProviderKind`] picks the wire
/// format: `OpenAi`/`OpenAiCompatible`/`Deepseek` → [`LlmProvider::OpenAI`] so a
/// custom proxy uses the OpenAI wire format against its own `base_url`;
/// `Anthropic`/`Ollama`/`Gemini` map to their native providers.
pub fn resolve_provider(kind: &ProviderKind, base_url: &str) -> LlmProvider {
    let detected = LlmProvider::from_base_url(base_url);
    if !matches!(detected, LlmProvider::Custom) {
        return detected;
    }
    match kind {
        ProviderKind::Anthropic => LlmProvider::Anthropic,
        ProviderKind::OpenAi | ProviderKind::OpenAiCompatible | ProviderKind::Deepseek => {
            LlmProvider::OpenAI
        }
        ProviderKind::Ollama => LlmProvider::Ollama,
        ProviderKind::Gemini => LlmProvider::Gemini,
        // ProviderKind is #[non_exhaustive]; future kinds default to OpenAI wire.
        _ => LlmProvider::OpenAI,
    }
}

/// Resolve a credential value from a [`CredentialRef`] (decision A1: env is the
/// default availability floor).
///
/// - `Env { var }` → `std::env::var(var)`, empty string on unset.
/// - `Store { service }` → reads `~/.shannon/credentials/<service>.json`
///   (ADR-0005 Phase 1). Empty when absent — callers like
///   [`resolve_api_key_for_provider`] then fall back to the provider's env
///   chain, so a missing store entry degrades gracefully to env.
/// - `InlineLegacy { masked }` → the transition-only value, as-is.
/// - `Keyring` / `Ephemeral` → empty here (opportunistic / session-injected; #9).
pub fn resolve_credential(cred: &CredentialRef) -> String {
    match cred {
        CredentialRef::Env { var } => std::env::var(var).unwrap_or_default(),
        CredentialRef::Store { service } => {
            crate::credential_manager::read_credential_value_default(service).unwrap_or_default()
        }
        CredentialRef::InlineLegacy { masked } => masked.clone(),
        CredentialRef::Keyring { .. } | CredentialRef::Ephemeral => String::new(),
    }
}

/// A resolved [`ModelRef`]: the concrete engine provider plus the (possibly
/// alias-expanded) model id, with catalog metadata for display/UI.
///
/// Produced by [`resolve_model_ref`]. This is the Phase 0 (ADR-0005) bridge
/// from the user-facing `provider/model` spelling to the engine's runtime
/// types — used by `/model`, `--model`, and the desktop provider switcher.
#[derive(Debug, Clone)]
pub struct ResolvedModelRef {
    /// Concrete engine provider (drives wire format + endpoint path).
    pub provider: LlmProvider,
    /// Model id after alias resolution (e.g. `sonnet` → `claude-sonnet-4-…`).
    /// For custom/unknown models this is the ref's model verbatim.
    pub model_id: String,
    /// True when `model_id` (post-resolution) is in the built-in catalog.
    pub known: bool,
    /// Context window from the catalog, or the `200_000` default for unknowns.
    pub context_window: usize,
}

/// Resolve a `provider/model` [`ModelRef`] into a concrete provider + model id,
/// applying alias resolution against the built-in catalog.
///
/// **Provider slug** resolution reuses the same identity bridge as
/// [`resolve_active_target`]: a recognised slug (`anthropic`, `ollama`,
/// `zhipu`, `moonshot`, `dashscope`, …) maps directly to its [`LlmProvider`],
/// preserving every known provider. An unrecognised slug is treated as an
/// OpenAI-compatible endpoint and falls back to the OpenAI wire format —
/// consistent with [`resolve_provider`].
///
/// **Model** resolution is intentionally lenient: a model absent from the
/// catalog (an Ollama model, a proxy's custom id, a newly-released model) is
/// returned verbatim with `known = false`. Tier aliases (`sonnet`, `opus`,
/// `haiku`) are expanded against the resolved provider when applicable.
pub fn resolve_model_ref(mref: &ModelRef) -> ResolvedModelRef {
    let provider = llm_provider_from_id(&mref.provider)
        .unwrap_or_else(|| resolve_provider(&ProviderKind::OpenAiCompatible, ""));
    // Alias-expand against the resolved provider first; unknowns pass through.
    let model_id = crate::model_registry::resolve_model(&mref.model, Some(&provider));
    let known = crate::model_registry::model_info_for(&model_id).is_some();
    let context_window = crate::model_registry::context_window_for(&model_id);
    ResolvedModelRef {
        provider,
        model_id,
        known,
        context_window,
    }
}

/// Build a default [`ProviderModelConfig`] from raw CLI / TOML inputs plus a
/// faithful relocation of the pre-N1 env-fallback chain.
///
/// N1/C-fields removes the pre-N1 flat `ShannonConfig` fields
/// (`model`/`provider`/`api_key`/`base_url`/`[providers.*]`). The resolution
/// that used to live in `impl From<ShannonConfig> for LlmClientConfig` is
/// relocated here so the [`crate::unified_config::ConfigBuilder`], the CLI
/// (`build_llm_config_from_builder`) and that `From` impl all go through one
/// fallback chain.
///
/// Resolution order (faithful, no behavior change vs. pre-N1):
/// 1. **Cred var**: `explicit_cred_var` (caller override, e.g. CLI injecting
///    a `--api-key` value into a side-channel var name) → first-set of
///    `SHANNON_API_KEY` → `ANTHROPIC_API_KEY` → `OPENAI_API_KEY`. None set
///    → `CredentialRef::Env { var: "SHANNON_API_KEY" }` (resolves empty).
/// 2. **`base_url`**: input → `SHANNON_BASE_URL` → `ANTHROPIC_BASE_URL` →
///    `OPENAI_BASE_URL` → provider's [`LlmProvider::default_base_url`].
/// 3. **`model`**: input → `SHANNON_MODEL` → `ANTHROPIC_MODEL` →
///    `OPENAI_MODEL` → `"claude-sonnet-4-20250514"`.
/// 4. **`provider`**: input → `SHANNON_PROVIDER` → infer from cred var
///    (`ANTHROPIC_API_KEY`-family → Anthropic, `OPENAI_API_KEY` → OpenAI) →
///    infer from `base_url` via [`LlmProvider::from_base_url`] → Anthropic
///    default.
/// 5. **Ollama auto-default**: when no credential AND no `base_url` (CLI,
///    TOML, or env) AND no provider are set, the default is Ollama on
///    `http://localhost:11434` with model `input → SHANNON_MODEL → "llama3"`.
///
/// Always returns `Some` (the empty case falls through to the Ollama branch).
/// Callers that have nothing to default (e.g. unit tests constructing a
/// `ShannonConfig` literal with a pre-populated `provider_model`) can ignore
/// the returned `ProviderModelConfig`.
pub fn synthesize_default_profile(
    model: Option<&str>,
    provider_input: Option<&str>,
    base_url_input: Option<&str>,
    explicit_cred_var: Option<&str>,
) -> Option<ProviderModelConfig> {
    use std::collections::HashMap;

    // (1) Credential var: explicit override wins, else first-set in canonical chain.
    let cred_var = if let Some(v) = explicit_cred_var {
        Some(v.to_string())
    } else {
        ["SHANNON_API_KEY", "ANTHROPIC_API_KEY", "OPENAI_API_KEY"]
            .iter()
            .find(|v| std::env::var(v).is_ok())
            .map(|s| s.to_string())
    };
    let cred_present = cred_var.is_some();

    // (2) base_url chain (input + 3 env vars).
    let base_url_env_first = std::env::var("SHANNON_BASE_URL")
        .ok()
        .or_else(|| std::env::var("ANTHROPIC_BASE_URL").ok())
        .or_else(|| std::env::var("OPENAI_BASE_URL").ok());
    let base_url = base_url_input.map(|s| s.to_string()).or(base_url_env_first);
    let base_url_present = base_url.is_some();

    // (4) Provider name chain.
    let provider_input = provider_input
        .map(|s| s.to_string())
        .or_else(|| std::env::var("SHANNON_PROVIDER").ok());
    let provider_explicit = provider_input.is_some();

    // (5) Ollama auto-default: no cred, no base_url, no provider.
    if !cred_present && !base_url_present && !provider_explicit {
        let ollama_model = model
            .map(|s| s.to_string())
            .or_else(|| std::env::var("SHANNON_MODEL").ok())
            .unwrap_or_else(|| "llama3".to_string());
        return Some(ollama_default_profile(&ollama_model));
    }

    // Resolve LlmProvider: explicit name wins, else cred var family,
    // else from base_url, else Anthropic default.
    let provider = if let Some(ref p) = provider_input {
        provider_str_to_llm(p, base_url.as_deref())
    } else {
        match cred_var.as_deref() {
            Some("SHANNON_API_KEY") | Some("ANTHROPIC_API_KEY") => LlmProvider::Anthropic,
            Some("OPENAI_API_KEY") => LlmProvider::OpenAI,
            _ => base_url
                .as_deref()
                .map(LlmProvider::from_base_url)
                .unwrap_or(LlmProvider::Anthropic),
        }
    };

    // Fill base_url with the provider's default if still unset (post-Ollama branch).
    let mut base_url = base_url.unwrap_or_else(|| provider.default_base_url().to_string());

    // Pre-N1 behaviour (faithfully preserved): when base_url was NOT
    // explicitly set by the user (no input, no SHANNON_BASE_URL) but came
    // from a foreign provider's env var (ANTHROPIC_BASE_URL /
    // OPENAI_BASE_URL) and doesn't match the resolved provider, override to
    // the provider's default base_url. This handles the common case where
    // a user has ANTHROPIC_BASE_URL set in their environment for a
    // different tool (e.g. Claude Code) but asks Shannon for `--provider
    // ollama` via the CLI.
    let user_explicit_base_url =
        base_url_input.is_some() || std::env::var("SHANNON_BASE_URL").is_ok();
    if !user_explicit_base_url {
        let came_from_anthropic =
            std::env::var("ANTHROPIC_BASE_URL").ok().as_deref() == Some(base_url.as_str());
        let came_from_openai =
            std::env::var("OPENAI_BASE_URL").ok().as_deref() == Some(base_url.as_str());
        let is_anthropic_provider = matches!(provider, LlmProvider::Anthropic);
        let is_openai_provider = matches!(provider, LlmProvider::OpenAI);
        let no_foreign_env = std::env::var("ANTHROPIC_BASE_URL").is_err()
            && std::env::var("OPENAI_BASE_URL").is_err();
        let conflicts = (came_from_anthropic && !is_anthropic_provider)
            || (came_from_openai && !is_openai_provider)
            || (no_foreign_env && base_url != provider.default_base_url());
        if conflicts {
            base_url = provider.default_base_url().to_string();
        }
    }
    let kind = llm_provider_to_kind(&provider);
    let provider_id = llm_provider_id(&provider);

    // (3) Model chain.
    let model_id = model
        .map(|s| s.to_string())
        .or_else(|| std::env::var("SHANNON_MODEL").ok())
        .or_else(|| std::env::var("ANTHROPIC_MODEL").ok())
        .or_else(|| std::env::var("OPENAI_MODEL").ok())
        .unwrap_or_else(|| "claude-sonnet-4-20250514".to_string());

    // A1-strict: the credential is the chosen env var (the plaintext value
    // never enters the config). When no cred var was resolved we still need
    // a `CredentialRef` field — point it at `SHANNON_API_KEY` so it resolves
    // to an empty string and the provider just sees "no key".
    let cred = cred_var
        .map(|v| CredentialRef::Env { var: v })
        .unwrap_or(CredentialRef::Env {
            var: "SHANNON_API_KEY".to_string(),
        });

    let profile = ProviderProfile {
        id: provider_id.clone(),
        kind,
        display_name: provider_id.clone(),
        base_url,
        models_url: None,
        credential: cred,
        extra_headers: HashMap::new(),
        default_max_tokens: None,
        fallback_models: Vec::new(),
        quirks: Default::default(),
        tiers: ProviderTiers::default(),
    };

    Some(ProviderModelConfig {
        version: ProviderModelConfig::VERSION,
        profiles: build_default_profiles_map(profile, &model_id),
        gateway: Default::default(),
    })
}

/// Construct a single-provider Ollama profile at `http://localhost:11434`.
fn ollama_default_profile(model_id: &str) -> ProviderModelConfig {
    use std::collections::HashMap;
    let profile = ProviderProfile {
        id: "ollama".to_string(),
        kind: ProviderKind::Ollama,
        display_name: "Ollama".to_string(),
        base_url: "http://localhost:11434".to_string(),
        models_url: None,
        // Ollama doesn't require auth, but `credential` is required — point
        // at `SHANNON_API_KEY` (resolves empty when unset).
        credential: CredentialRef::Env {
            var: "SHANNON_API_KEY".to_string(),
        },
        extra_headers: HashMap::new(),
        default_max_tokens: None,
        fallback_models: Vec::new(),
        quirks: Default::default(),
        tiers: ProviderTiers::default(),
    };
    ProviderModelConfig {
        version: ProviderModelConfig::VERSION,
        profiles: build_default_profiles_map(profile, model_id),
        gateway: Default::default(),
    }
}

/// Wrap a single provider profile into the `"default"` [`ModelProfile`] map.
fn build_default_profiles_map(
    profile: ProviderProfile,
    model_id: &str,
) -> std::collections::HashMap<String, ModelProfile> {
    use std::collections::HashMap;
    let provider_id = profile.id.clone();
    let mut profiles = HashMap::new();
    profiles.insert(
        "default".to_string(),
        ModelProfile {
            name: "default".to_string(),
            active_target: ActiveTarget {
                provider_id,
                model_id: model_id.to_string(),
                scope: Scope::Global,
            },
            providers: vec![profile],
            auxiliary: HashMap::new(),
            credential_scope: CredentialScope::Shared,
        },
    );
    profiles
}

/// Map an LlmProvider-name string to an [`LlmProvider`] enum value, falling
/// back to the resolved `base_url` for unrecognized names. Mirrors the
/// string→provider table that used to live in the v1 `From<ShannonConfig>` impl.
///
/// The name→variant arms now delegate to [`llm_provider_from_slug`] (the single
/// alias table, ADR-0008 Decision 1); only the `base_url` fallback for truly
/// unknown names remains here, since that is config-loading-specific behaviour.
fn provider_str_to_llm(p: &str, base_url: Option<&str>) -> LlmProvider {
    llm_provider_from_slug(p).unwrap_or_else(|| {
        base_url
            .map(LlmProvider::from_base_url)
            .unwrap_or(LlmProvider::Custom)
    })
}

/// Coarse wire-protocol discriminator for a provider profile's `kind` field.
pub(crate) fn llm_provider_to_kind(p: &LlmProvider) -> ProviderKind {
    match p {
        LlmProvider::Anthropic => ProviderKind::Anthropic,
        LlmProvider::OpenAI => ProviderKind::OpenAi,
        LlmProvider::Ollama => ProviderKind::Ollama,
        LlmProvider::Gemini => ProviderKind::Gemini,
        LlmProvider::DeepSeek => ProviderKind::Deepseek,
        // Others all use OpenAI-compatible wire format; the fine-grained
        // provider identity is recovered from `base_url` at resolution time.
        _ => ProviderKind::OpenAiCompatible,
    }
}

/// Profile-id slug for a provider (matches the provider's `Debug` name
/// lower-cased so `ProviderProfile.id` and `ActiveTarget.provider_id` line up).
///
/// Public so the REPL (`/connect`, `/provider`) can compute the same service
/// slug a connected profile / credential-store entry is keyed under.
pub fn llm_provider_id(p: &LlmProvider) -> String {
    format!("{p:?}").to_lowercase()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use shannon_types::provider_config::{
        ActiveTarget, CredentialRef, CredentialScope, ModelProfile, ProviderKind,
        ProviderModelConfig, ProviderProfile, ProviderTiers, Scope,
    };
    use std::collections::HashMap;

    fn profile(id: &str, kind: ProviderKind, base_url: &str, var: &str) -> ProviderProfile {
        ProviderProfile {
            id: id.to_string(),
            kind,
            display_name: id.to_string(),
            base_url: base_url.to_string(),
            models_url: None,
            credential: CredentialRef::Env {
                var: var.to_string(),
            },
            extra_headers: HashMap::new(),
            default_max_tokens: None,
            fallback_models: Vec::new(),
            quirks: Default::default(),
            tiers: ProviderTiers::default(),
        }
    }

    fn config_with(provider: ProviderProfile, model: &str) -> ProviderModelConfig {
        let mut profiles = HashMap::new();
        profiles.insert(
            "default".to_string(),
            ModelProfile {
                name: "default".to_string(),
                active_target: ActiveTarget {
                    provider_id: provider.id.clone(),
                    model_id: model.to_string(),
                    scope: Scope::Global,
                },
                providers: vec![provider],
                auxiliary: HashMap::new(),
                credential_scope: CredentialScope::Shared,
            },
        );
        ProviderModelConfig {
            version: ProviderModelConfig::VERSION,
            profiles,
            gateway: Default::default(),
        }
    }

    // ── llm_provider_from_slug (ADR-0008 Decision 1 / P2-3) ──────────────
    //
    // Exhaustive guard over the single alias table. Every alias that previously
    // lived in `parse_provider_name`, `provider_str_to_llm`, or
    // `llm_provider_from_id` must resolve here, so a future edit can't silently
    // drop a name and regress `/provider <alias>` or config loading.

    #[test]
    fn from_slug_round_trips_every_catalog_provider_slug() {
        // Every canonical slug emitted by `llm_provider_id` for a catalog
        // provider must round-trip through `from_slug`. Guards against a
        // slug/parser rename drift. (`Custom` has no catalog presence and is
        // intentionally a `None` — covered by `from_slug_returns_none_for_unknown_and_custom`.)
        for p in crate::model_registry::all_providers() {
            let slug = llm_provider_id(&p);
            assert_eq!(
                llm_provider_from_slug(&slug),
                Some(p.clone()),
                "canonical slug '{slug}' did not resolve to {p:?}"
            );
        }
    }

    #[test]
    fn from_slug_resolves_all_known_aliases() {
        // The union of every alias arm from the three former tables.
        let cases: &[(&str, LlmProvider)] = &[
            ("anthropic", LlmProvider::Anthropic),
            ("claude", LlmProvider::Anthropic),
            ("openai", LlmProvider::OpenAI),
            ("gpt", LlmProvider::OpenAI),
            ("chatgpt", LlmProvider::OpenAI),
            ("ollama", LlmProvider::Ollama),
            ("local", LlmProvider::Ollama),
            ("gemini", LlmProvider::Gemini),
            ("google", LlmProvider::Gemini),
            ("azure", LlmProvider::Azure),
            ("azure-openai", LlmProvider::Azure),
            ("bedrock", LlmProvider::Bedrock),
            ("aws", LlmProvider::Bedrock),
            ("mistral", LlmProvider::Mistral),
            ("mistral-ai", LlmProvider::Mistral),
            ("deepseek", LlmProvider::DeepSeek),
            ("ds", LlmProvider::DeepSeek),
            ("groq", LlmProvider::Groq),
            ("together", LlmProvider::Together),
            ("together-ai", LlmProvider::Together),
            ("openrouter", LlmProvider::OpenRouter),
            ("cohere", LlmProvider::Cohere),
            ("fireworks", LlmProvider::Fireworks),
            ("perplexity", LlmProvider::Perplexity),
            ("xai", LlmProvider::Xai),
            ("grok", LlmProvider::Xai),
            ("ai21", LlmProvider::Ai21),
            ("siliconflow", LlmProvider::SiliconFlow),
            ("sf", LlmProvider::SiliconFlow),
            ("zhipu", LlmProvider::Zhipu),
            ("zhipu-cn", LlmProvider::Zhipu),
            ("glm", LlmProvider::Zhipu),
            ("zhipuinternational", LlmProvider::ZhipuInternational),
            ("zhipu-international", LlmProvider::ZhipuInternational),
            ("zhipu-intl", LlmProvider::ZhipuInternational),
            ("glm-intl", LlmProvider::ZhipuInternational),
            ("zhipucoding", LlmProvider::ZhipuCoding),
            ("zhipu-coding", LlmProvider::ZhipuCoding),
            ("zhipu-anthropic", LlmProvider::ZhipuCoding),
            ("moonshot", LlmProvider::Moonshot),
            ("kimi", LlmProvider::Moonshot),
            ("minimax", LlmProvider::Minimax),
            ("mm", LlmProvider::Minimax),
            ("dashscope", LlmProvider::DashScope),
            ("qwen", LlmProvider::DashScope),
            ("aliyun", LlmProvider::DashScope),
            ("cloudflare", LlmProvider::Cloudflare),
            ("cf", LlmProvider::Cloudflare),
            ("replicate", LlmProvider::Replicate),
        ];
        for (alias, expected) in cases {
            assert_eq!(
                llm_provider_from_slug(alias),
                Some(expected.clone()),
                "alias '{alias}' should resolve to {expected:?}"
            );
        }
    }

    #[test]
    fn from_slug_is_case_insensitive_and_trim_tolerant() {
        assert_eq!(
            llm_provider_from_slug("Anthropic"),
            Some(LlmProvider::Anthropic)
        );
        assert_eq!(
            llm_provider_from_slug("  CLAUDE  "),
            Some(LlmProvider::Anthropic)
        );
        assert_eq!(llm_provider_from_slug("Qwen"), Some(LlmProvider::DashScope));
    }

    #[test]
    fn from_slug_returns_none_for_unknown_and_custom() {
        assert!(llm_provider_from_slug("unknown-provider").is_none());
        assert!(llm_provider_from_slug("custom").is_none());
        assert!(llm_provider_from_slug("").is_none());
    }

    #[test]
    fn none_when_no_default_profile() {
        let pm = ProviderModelConfig {
            version: ProviderModelConfig::VERSION,
            profiles: HashMap::new(),
            gateway: Default::default(),
        };
        assert!(resolve_active_target(&pm).is_none());
    }

    #[test]
    fn none_when_active_target_provider_missing() {
        let pm = config_with(
            profile(
                "anthropic",
                ProviderKind::Anthropic,
                "https://api.anthropic.com",
                "K",
            ),
            "claude",
        );
        let mut pm = pm;
        pm.profiles
            .get_mut("default")
            .unwrap()
            .active_target
            .provider_id = "ghost".to_string();
        assert!(resolve_active_target(&pm).is_none());
    }

    #[test]
    fn resolves_known_anthropic_via_base_url() {
        let pm = config_with(
            profile(
                "anthropic",
                ProviderKind::Anthropic,
                "https://api.anthropic.com",
                "SHANNON_ANTHROPIC_API_KEY",
            ),
            "claude-sonnet-4-20250514",
        );
        let r = resolve_active_target(&pm).unwrap();
        assert_eq!(r.provider, LlmProvider::Anthropic);
        assert_eq!(r.model_id, "claude-sonnet-4-20250514");
        assert_eq!(r.profile.base_url, "https://api.anthropic.com");
    }

    #[test]
    fn resolves_zhipu_via_base_url_detection() {
        // base_url drives a specific provider even though kind is coarse.
        let pm = config_with(
            profile(
                "zhipu",
                ProviderKind::OpenAiCompatible,
                "https://open.bigmodel.cn",
                "ZHIPU_API_KEY",
            ),
            "glm-4",
        );
        let r = resolve_active_target(&pm).unwrap();
        assert_eq!(r.provider, LlmProvider::Zhipu);
    }

    #[test]
    fn unknown_url_openai_compatible_uses_openai_wire() {
        let pm = config_with(
            profile(
                "proxy",
                ProviderKind::OpenAiCompatible,
                "https://my-proxy.example.com/v1",
                "PROXY_KEY",
            ),
            "gpt-4o",
        );
        let r = resolve_active_target(&pm).unwrap();
        // unrecognized host → kind fallback → OpenAI wire format
        assert_eq!(r.provider, LlmProvider::OpenAI);
        assert!(r.provider.is_openai_compatible());
    }

    #[test]
    fn resolve_provider_kind_fallbacks_for_unrecognized_url() {
        // "http://x" is unrecognized by from_base_url → kind decides.
        assert_eq!(
            resolve_provider(&ProviderKind::Ollama, "http://x"),
            LlmProvider::Ollama
        );
        assert_eq!(
            resolve_provider(&ProviderKind::Gemini, "http://x"),
            LlmProvider::Gemini
        );
        assert_eq!(
            resolve_provider(&ProviderKind::Anthropic, "http://x"),
            LlmProvider::Anthropic
        );
        assert_eq!(
            resolve_provider(&ProviderKind::OpenAi, "http://x"),
            LlmProvider::OpenAI
        );
        assert_eq!(
            resolve_provider(&ProviderKind::Deepseek, "http://x"),
            LlmProvider::OpenAI
        );
        assert_eq!(
            resolve_provider(&ProviderKind::OpenAiCompatible, "http://x"),
            LlmProvider::OpenAI
        );
    }

    #[test]
    fn resolve_credential_env_present_and_absent() {
        // SAFETY: unique key read only by this test thread; no concurrent
        // set/remove of the same key elsewhere.
        unsafe { std::env::set_var("N1_TEST_CRED", "secret-value") };
        assert_eq!(
            resolve_credential(&CredentialRef::Env {
                var: "N1_TEST_CRED".to_string()
            }),
            "secret-value"
        );
        // SAFETY: see above.
        unsafe { std::env::remove_var("N1_TEST_CRED") };
        assert_eq!(
            resolve_credential(&CredentialRef::Env {
                var: "N1_DEFINITELY_UNSET".to_string()
            }),
            ""
        );
    }

    #[test]
    fn resolve_credential_inline_legacy_passthrough() {
        assert_eq!(
            resolve_credential(&CredentialRef::InlineLegacy {
                masked: "m".to_string()
            }),
            "m"
        );
    }

    #[test]
    fn resolve_credential_keyring_and_ephemeral_empty_for_now() {
        assert_eq!(
            resolve_credential(&CredentialRef::Keyring {
                service: "s".to_string(),
                account: "a".to_string()
            }),
            ""
        );
        assert_eq!(resolve_credential(&CredentialRef::Ephemeral), "");
    }

    #[test]
    fn resolve_credential_store_missing_service_is_empty() {
        // A store entry that definitely does not exist resolves to empty —
        // this read-only check never writes to the home credentials dir. The
        // store-read primitive itself is covered by credential_manager tests.
        assert_eq!(
            resolve_credential(&CredentialRef::Store {
                service: "shannon-definitely-not-a-real-service-9f3a".to_string()
            }),
            ""
        );
    }

    // ── resolve_model_ref (Phase 0, ADR-0005) ──────────────────────────

    #[test]
    fn resolve_model_ref_known_anthropic_full_id() {
        let m = ModelRef::new("anthropic", "claude-sonnet-4-20250514");
        let r = resolve_model_ref(&m);
        assert_eq!(r.provider, LlmProvider::Anthropic);
        assert_eq!(r.model_id, "claude-sonnet-4-20250514");
        assert!(r.known);
        assert_eq!(r.context_window, 200_000);
    }

    #[test]
    fn resolve_model_ref_expands_sonnet_alias_against_provider() {
        let m = ModelRef::new("anthropic", "sonnet");
        let r = resolve_model_ref(&m);
        assert_eq!(r.provider, LlmProvider::Anthropic);
        // The alias must be expanded to a concrete catalog id (not left as
        // "sonnet"); the exact pick is the registry's tier heuristic.
        assert_ne!(r.model_id, "sonnet");
        assert!(r.model_id.starts_with("claude-"));
        assert!(r.known);
    }

    #[test]
    fn resolve_model_ref_preserves_zhipu_slug() {
        let m = ModelRef::new("zhipu", "glm-4.6");
        let r = resolve_model_ref(&m);
        // recognised slug → Zhipu (not the OpenAI fallback)
        assert_eq!(r.provider, LlmProvider::Zhipu);
        assert_eq!(r.model_id, "glm-4.6");
        // glm-4.6 may or may not be in the catalog; both are acceptable, just
        // assert context window is sane.
        assert!(r.context_window > 0);
    }

    #[test]
    fn resolve_model_ref_ollama_custom_model_is_unknown_but_kept() {
        let m = ModelRef::new("ollama", "llama3");
        let r = resolve_model_ref(&m);
        assert_eq!(r.provider, LlmProvider::Ollama);
        assert_eq!(r.model_id, "llama3");
        // Runtime-detected models aren't in the static catalog.
        assert!(!r.known);
        assert_eq!(r.context_window, 200_000);
    }

    #[test]
    fn resolve_model_ref_unknown_provider_slug_falls_back_to_openai_wire() {
        let m = ModelRef::new("my-custom-proxy", "some-model");
        let r = resolve_model_ref(&m);
        // Unknown slug → OpenAI-compatible wire format.
        assert!(r.provider.is_openai_compatible());
        assert_eq!(r.model_id, "some-model");
        assert!(!r.known);
    }

    #[test]
    fn resolve_model_ref_from_input_bare_uses_fallback_provider() {
        // Simulates `--model opus` with the active provider being Anthropic.
        let m = ModelRef::from_input("opus", Some("anthropic")).unwrap();
        let r = resolve_model_ref(&m);
        assert_eq!(r.provider, LlmProvider::Anthropic);
        // Alias expanded to a known catalog id for Anthropic.
        assert_ne!(r.model_id, "opus");
        assert!(r.model_id.starts_with("claude-"));
        assert!(r.known);
    }

    // ── /connect profile shape (ADR-0005 Phase 4) ─────────────────────
    // The former `build_connect_profile` unit tests moved to
    // `provider_config_service::tests` once `/connect` started routing through
    // `ProviderConfigService::connect` (ADR-0008 P2-5 step 4): the service is
    // now the only producer of the on-disk profile shape, so the shape + A1
    // (no-plaintext) assertions live next to it.
}
