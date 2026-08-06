//! # Model Registry
//!
//! Static catalog of known LLM models grouped by provider, with metadata
//! for context window size and max output tokens. Used by the `/models`
//! picker and Tab completion for the `/model` command.
//!
//! Also provides [`ModelRouter`] for intelligent model selection based on
//! task type, cost, and speed requirements.

use shannon_engine::api::LlmProvider;

/// Dynamic (models.dev) catalog layer — live models fetched on `/model
/// refresh`, cached offline, merged additively over `MODEL_CATALOG`.
pub mod dynamic;

// ── Submodules (ADR-0008 P2-8 — split oversized model_registry.rs) ──
pub mod catalog;
pub mod tier;

pub use catalog::{MODEL_CATALOG, ModelCapabilities, ModelInfo, TierLabel};
pub use tier::{
    EffortLevel, ModelRouter, TaskType, is_model_alias, model_aliases, resolve_auto_tier,
    resolve_model, resolve_model_alias, resolve_tier,
};

// ── Query helpers ──────────────────────────────────────────────────

/// Return all models in the catalog for a given provider.
pub fn models_for_provider(provider: LlmProvider) -> Vec<&'static ModelInfo> {
    MODEL_CATALOG
        .iter()
        .filter(|m| m.provider == provider)
        .collect()
}

/// Merge static catalog entries with a dynamic set for one provider.
///
/// Static entries are emitted first (preserving curated metadata, aliases, and
/// the Phase B beta-header mapping keyed by model id). Dynamic entries are
/// appended only when their id is not already present, deduplicating by id so a
/// models.dev refresh never doubles a known model.
pub fn merge_static_and_dynamic(provider: LlmProvider, dynamic: &[ModelInfo]) -> Vec<ModelInfo> {
    let mut out: Vec<ModelInfo> = Vec::new();
    for m in MODEL_CATALOG.iter().filter(|m| m.provider == provider) {
        out.push(m.clone());
    }
    let known: std::collections::HashSet<&str> = out.iter().map(|m| m.id).collect();
    for m in dynamic.iter().filter(|m| m.provider == provider) {
        if !known.contains(m.id) {
            out.push(m.clone());
        }
    }
    out
}

/// Models for a provider from the **merged** catalog: the static
/// `MODEL_CATALOG` augmented by the models.dev dynamic overlay (Phase D).
///
/// Lazily seeds the overlay from the on-disk cache on first use — never
/// touching the network — so this is safe to call offline and in CI. Static
/// entries take priority (see [`merge_static_and_dynamic`]).
pub fn merged_models_for_provider(provider: LlmProvider) -> Vec<ModelInfo> {
    dynamic::ensure_overlay_loaded();
    let overlay = dynamic::overlay_snapshot();
    merge_static_and_dynamic(provider, &overlay)
}

/// Return all distinct providers that have models in the catalog.
pub fn all_providers() -> Vec<LlmProvider> {
    let mut providers: Vec<LlmProvider> =
        MODEL_CATALOG.iter().map(|m| m.provider.clone()).collect();
    providers.sort_by_key(provider_order);
    providers.dedup();
    providers
}

/// Apply an allowlist/denylist of canonical provider slugs to a provider list.
///
/// Slugs match the provider's [`Display`](LlmProvider) form, case-insensitively
/// (e.g. `"anthropic"`, `"openai"`, `"ollama"`). If `enabled` is non-empty it
/// acts as an allowlist (only matches pass); `disabled` always removes matches.
/// Pure — unit-tested below.
pub fn filter_providers(
    providers: Vec<LlmProvider>,
    enabled: &[String],
    disabled: &[String],
) -> Vec<LlmProvider> {
    let enabled: Vec<String> = enabled.iter().map(|s| s.to_lowercase()).collect();
    let disabled: Vec<String> = disabled.iter().map(|s| s.to_lowercase()).collect();
    providers
        .into_iter()
        .filter(|p| {
            let slug = p.to_string();
            if disabled.iter().any(|d| d == &slug) {
                return false;
            }
            if !enabled.is_empty() && !enabled.iter().any(|e| e == &slug) {
                return false;
            }
            true
        })
        .collect()
}

/// Parse a comma- or whitespace-separated provider-slug env var into a list.
fn parse_provider_slugs_env(var: &str) -> Vec<String> {
    std::env::var(var)
        .ok()
        .map(|raw| {
            raw.split([',', ' ', '\t', '\n'])
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Like [`all_providers`] but filtered by the `SHANNON_ENABLED_PROVIDERS` /
/// `SHANNON_DISABLED_PROVIDERS` env vars (ADR-0005 Phase 5 allowlist), so users
/// can restrict which providers appear in the picker / status card.
///
/// Fails open: if filtering would yield an empty list (e.g. a typo'd allowlist),
/// the full unfiltered list is returned so the picker is never bricked.
pub fn available_providers() -> Vec<LlmProvider> {
    let enabled = parse_provider_slugs_env("SHANNON_ENABLED_PROVIDERS");
    let disabled = parse_provider_slugs_env("SHANNON_DISABLED_PROVIDERS");
    if enabled.is_empty() && disabled.is_empty() {
        return all_providers();
    }
    let filtered = filter_providers(all_providers(), &enabled, &disabled);
    if filtered.is_empty() {
        all_providers()
    } else {
        filtered
    }
}

/// Resolve the merged env-var allowlist (`SHANNON_ENABLED_PROVIDERS` minus
/// `SHANNON_DISABLED_PROVIDERS`) into a canonical slug list.
///
/// Returns `Some(vec)` (with at least one entry) when either env var is
/// set; returns `None` when neither env var is set (caller treats that as
/// "no restriction"). `SHANNON_ENABLED_PROVIDERS` takes precedence — when
/// both are set, the disable list is applied within the enable set.
///
/// Pure (reads env via `parse_provider_slugs_env` only); unit-tested
/// below.
pub fn env_provider_allowlist() -> Option<Vec<String>> {
    let enabled = parse_provider_slugs_env("SHANNON_ENABLED_PROVIDERS");
    let disabled = parse_provider_slugs_env("SHANNON_DISABLED_PROVIDERS");
    if enabled.is_empty() && disabled.is_empty() {
        return None;
    }
    let mut out: Vec<String> = if enabled.is_empty() {
        all_providers().into_iter().map(|p| p.to_string()).collect()
    } else {
        enabled.clone()
    };
    if !disabled.is_empty() {
        out.retain(|slug| !disabled.iter().any(|d| d.eq_ignore_ascii_case(slug)));
    }
    if out.is_empty() {
        return None;
    }
    Some(out)
}

/// Compute the effective provider allowlist for the desktop / model
/// picker: an explicit `Some(slice)` (the desktop's persisted
/// `enabled_providers`) overrides any env-var allowlist; otherwise the
/// env-var allowlist is consulted.
///
/// Precedence (ADR-0005 Phase 5 / P4.9):
/// - `explicit = Some(vec![])` → `Some(vec![])`. The user explicitly
///   toggled every provider off; the env-var allowlist is ignored.
/// - `explicit = Some(non_empty)` → `Some(explicit.to_vec())`. The user's
///   explicit choice wins — env vars are skipped so a desktop user
///   doesn't have to fight their shell's stale `SHANNON_*_PROVIDERS`.
/// - `explicit = None` → fall through to [`env_provider_allowlist`].
///   Returns `None` (no restriction) when neither env var is set.
///
/// Pure; unit-tested below.
pub fn effective_provider_allowlist(explicit: Option<&[String]>) -> Option<Vec<String>> {
    match explicit {
        // `Some(&[])` is the user-set "hide everything" state — env vars
        // are ignored because the explicit slice is non-`None`.
        Some(slice) => Some(slice.to_vec()),
        None => env_provider_allowlist(),
    }
}

/// Returns true if `provider` would pass the `SHANNON_ENABLED_PROVIDERS` /
/// `SHANNON_DISABLED_PROVIDERS` allowlist/denylist. Callers that build their
/// own provider list (e.g. the model picker always offering a local Ollama
/// discovery tab) use this to still honour an explicit operator filter.
pub fn is_provider_allowed(provider: &LlmProvider) -> bool {
    let enabled = parse_provider_slugs_env("SHANNON_ENABLED_PROVIDERS");
    let disabled = parse_provider_slugs_env("SHANNON_DISABLED_PROVIDERS");
    if enabled.is_empty() && disabled.is_empty() {
        return true;
    }
    let slug = provider.to_string();
    if disabled.iter().any(|d| d == &slug) {
        return false;
    }
    if !enabled.is_empty() && !enabled.iter().any(|e| e == &slug) {
        return false;
    }
    true
}

/// Provider display ordering (lower = shown first).
fn provider_order(p: &LlmProvider) -> u8 {
    match p {
        LlmProvider::Anthropic => 0,
        LlmProvider::OpenAI => 1,
        LlmProvider::Gemini => 2,
        LlmProvider::DeepSeek => 3,
        LlmProvider::Mistral => 4,
        LlmProvider::Groq => 5,
        LlmProvider::Zhipu => 6,
        LlmProvider::ZhipuInternational => 7,
        LlmProvider::ZhipuCoding => 7,
        LlmProvider::Moonshot => 8,
        LlmProvider::Minimax => 9,
        LlmProvider::DashScope => 10,
        LlmProvider::Ollama => 11,
        LlmProvider::Xai => 12,
        LlmProvider::Perplexity => 13,
        LlmProvider::Cohere => 14,
        LlmProvider::Together => 15,
        LlmProvider::Fireworks => 16,
        LlmProvider::SiliconFlow => 17,
        LlmProvider::Ai21 => 18,
        LlmProvider::Azure => 19,
        LlmProvider::Bedrock => 20,
        LlmProvider::OpenRouter => 21,
        LlmProvider::Cloudflare => 22,
        LlmProvider::Replicate => 23,
        LlmProvider::Custom => 99,
    }
}

/// Format a provider name for display (e.g. "OpenAI", "DeepSeek").
pub fn provider_display_name(p: &LlmProvider) -> &'static str {
    match p {
        LlmProvider::Anthropic => "Anthropic",
        LlmProvider::OpenAI => "OpenAI",
        LlmProvider::Gemini => "Google",
        LlmProvider::DeepSeek => "DeepSeek",
        LlmProvider::Mistral => "Mistral",
        LlmProvider::Groq => "Groq",
        LlmProvider::Ollama => "Ollama",
        LlmProvider::Azure => "Azure",
        LlmProvider::Bedrock => "Bedrock",
        LlmProvider::Together => "Together",
        LlmProvider::OpenRouter => "OpenRouter",
        LlmProvider::Cohere => "Cohere",
        LlmProvider::Fireworks => "Fireworks",
        LlmProvider::Perplexity => "Perplexity",
        LlmProvider::Xai => "xAI",
        LlmProvider::Ai21 => "AI21",
        LlmProvider::Cloudflare => "Cloudflare",
        LlmProvider::Replicate => "Replicate",
        LlmProvider::SiliconFlow => "SiliconFlow",
        LlmProvider::Zhipu => "GLM (Zhipu)",
        LlmProvider::ZhipuInternational => "GLM (Zhipu Int'l)",
        LlmProvider::ZhipuCoding => "GLM (Zhipu Coding)",
        LlmProvider::Moonshot => "Kimi (Moonshot)",
        LlmProvider::Minimax => "MiniMax",
        LlmProvider::DashScope => "Qwen (DashScope)",
        LlmProvider::Custom => "Custom",
    }
}

/// Attempt to detect locally running Ollama models via `ollama list`.
///
/// Returns an empty Vec silently if Ollama is not installed or not running.
pub fn detect_local_models() -> Vec<ModelInfo> {
    let output = match std::process::Command::new("ollama").arg("list").output() {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };

    if !output.status.success() {
        return Vec::new();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut models = Vec::new();

    for line in stdout.lines().skip(1) {
        // Ollama output: "NAME\tID\tSIZE\tMODIFIED"
        let name = line.split_whitespace().next().unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }
        models.push(ModelInfo {
            id: Box::leak(name.clone().into_boxed_str()),
            display_name: Box::leak(name.into_boxed_str()),
            aliases: &[],
            provider: LlmProvider::Ollama,
            context_window: 4096,
            max_output: 4_096,
            cost_per_m_input: 0.0,
            cost_per_m_output: 0.0,
            capabilities: ModelCapabilities::cheap().or(ModelCapabilities::speed()),
        });
    }

    models
}

/// Return all model IDs from the catalog (for Tab completion).
pub fn all_model_ids() -> Vec<&'static str> {
    MODEL_CATALOG.iter().map(|m| m.id).collect()
}

/// Conservative context-window fallback (200K) for internal budgets
/// (compaction thresholds, sidebar gauge denominator) when a model's real
/// limit is unknown. This is a safety cap, **not** a claim about the model —
/// user-facing labels use [`context_window_for_opt`] and render "unknown" when
/// it returns `None`.
pub const FALLBACK_CONTEXT_WINDOW: usize = 200_000;

/// Look up a model's context window by its ID, returning `None` when the model
/// is unknown to both the static catalog and the models.dev dynamic overlay.
///
/// This is the honest accessor: a `None` result means "we cannot state a
/// number", so callers that surface the value to the user can render "unknown"
/// rather than fabricating [`FALLBACK_CONTEXT_WINDOW`]. Tries exact match
/// first, then prefix match (e.g. `"claude-sonnet-4"` matches
/// `"claude-sonnet-4-20250514"`), then reverse prefix, then the dynamic
/// overlay by exact id.
pub fn context_window_for_opt(model_id: &str) -> Option<usize> {
    // Exact match
    if let Some(info) = MODEL_CATALOG.iter().find(|m| m.id == model_id) {
        return Some(info.context_window);
    }
    // Prefix match: catalog entry starts with the given model_id
    // (handles short names like "claude-sonnet-4" → "claude-sonnet-4-20250514")
    if let Some(info) = MODEL_CATALOG.iter().find(|m| m.id.starts_with(model_id)) {
        return Some(info.context_window);
    }
    // Reverse prefix: given model_id starts with a catalog entry
    // (handles "claude-sonnet-4-20250514-extra" → "claude-sonnet-4-20250514")
    if let Some(info) = MODEL_CATALOG
        .iter()
        .filter(|m| model_id.starts_with(m.id))
        .max_by_key(|m| m.id.len())
    {
        return Some(info.context_window);
    }
    // Dynamic overlay (models.dev): exact id match for freshly pulled models.
    if let Some(info) = dynamic::overlay_snapshot()
        .iter()
        .find(|m| m.id == model_id)
    {
        return Some(info.context_window);
    }
    None
}

/// Look up a model's context window by its ID. Returns
/// [`FALLBACK_CONTEXT_WINDOW`] if the model is not found. Prefer
/// [`context_window_for_opt`] for user-facing values.
pub fn context_window_for(model_id: &str) -> usize {
    context_window_for_opt(model_id).unwrap_or(FALLBACK_CONTEXT_WINDOW)
}

/// Look up model info by ID.
pub fn model_info_for(model_id: &str) -> Option<&'static ModelInfo> {
    MODEL_CATALOG.iter().find(|m| m.id == model_id)
}

/// Look up a model by ID or alias.
pub fn model_info_for_alias(alias: &str) -> Option<&'static ModelInfo> {
    let lower = alias.to_lowercase();
    MODEL_CATALOG
        .iter()
        .find(|m| m.id == lower || m.aliases.iter().any(|a| a == &lower))
        .or_else(|| model_info_for(alias))
}

/// Classify a model id into a routing tier via the catalog — the single
/// source of truth for tier classification. Matches exact id first, then
/// prefix (so short names like `"claude-sonnet-4"` resolve to
/// `"claude-sonnet-4-20250514"`), mirroring [`context_window_for`]'s lookup
/// strategy. Returns [`TierLabel::Unknown`] for anything not in the catalog.
///
/// UI layers (status bar, status card) call this instead of maintaining their
/// own string-heuristic copies.
pub fn tier_label_for_id(model_id: &str) -> TierLabel {
    // Empty id would otherwise prefix-match the first catalog entry
    // (`m.id.starts_with("")` is always true) — guard it explicitly.
    if model_id.is_empty() {
        return TierLabel::Unknown;
    }
    if let Some(info) = model_info_for(model_id) {
        return info.tier_label();
    }
    if let Some(info) = MODEL_CATALOG.iter().find(|m| m.id.starts_with(model_id)) {
        return info.tier_label();
    }
    if let Some(info) = MODEL_CATALOG
        .iter()
        .filter(|m| model_id.starts_with(m.id))
        .max_by_key(|m| m.id.len())
    {
        return info.tier_label();
    }
    TierLabel::Unknown
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use shannon_engine::api::types::WireFormat;
    use shannon_types::provider_config::{ProviderTiers, TierName};

    #[test]
    fn resolve_tier_anthropic_fast_uses_haiku() {
        let tiers = ProviderTiers::default();
        let resolved = resolve_tier("fast", &LlmProvider::Anthropic, &tiers);
        assert_eq!(resolved, Some("claude-haiku-4-5".to_string()));
    }

    #[test]
    fn resolve_tier_anthropic_standard_uses_sonnet() {
        let tiers = ProviderTiers::default();
        let resolved = resolve_tier("standard", &LlmProvider::Anthropic, &tiers);
        assert_eq!(resolved, Some("claude-sonnet-4-20250514".to_string()));
    }

    #[test]
    fn resolve_tier_anthropic_pro_uses_opus() {
        let tiers = ProviderTiers::default();
        let resolved = resolve_tier("pro", &LlmProvider::Anthropic, &tiers);
        assert_eq!(resolved, Some("claude-opus-4".to_string()));
    }

    #[test]
    fn resolve_tier_accepts_anthropic_aliases() {
        let tiers = ProviderTiers::default();
        assert_eq!(
            resolve_tier("haiku", &LlmProvider::Anthropic, &tiers),
            Some("claude-haiku-4-5".to_string())
        );
        assert_eq!(
            resolve_tier("sonnet", &LlmProvider::Anthropic, &tiers),
            Some("claude-sonnet-4-20250514".to_string())
        );
        assert_eq!(
            resolve_tier("opus", &LlmProvider::Anthropic, &tiers),
            Some("claude-opus-4".to_string())
        );
    }

    #[test]
    fn resolve_tier_accepts_other_provider_aliases() {
        let tiers = ProviderTiers::default();
        assert!(resolve_tier("flash", &LlmProvider::Gemini, &tiers).is_some());
        assert!(resolve_tier("mini", &LlmProvider::OpenAI, &tiers).is_some());
        assert!(resolve_tier("ultra", &LlmProvider::Gemini, &tiers).is_some());
    }

    #[test]
    fn resolve_tier_profile_override_wins() {
        let tiers = ProviderTiers {
            fast: Some("claude-haiku-3-5".to_string()),
            ..Default::default()
        };
        let resolved = resolve_tier("fast", &LlmProvider::Anthropic, &tiers);
        assert_eq!(
            resolved,
            Some("claude-haiku-3-5".to_string()),
            "explicit profile_tiers.fast should win over catalog default"
        );
    }

    #[test]
    fn resolve_tier_unknown_input_returns_none() {
        let tiers = ProviderTiers::default();
        assert_eq!(
            resolve_tier("garbage", &LlmProvider::Anthropic, &tiers),
            None
        );
        assert_eq!(resolve_tier("", &LlmProvider::Anthropic, &tiers), None);
    }

    #[test]
    fn resolve_tier_auto_returns_none() {
        let tiers = ProviderTiers::default();
        assert_eq!(resolve_tier("auto", &LlmProvider::Anthropic, &tiers), None);
    }

    #[test]
    fn test_models_for_provider_anthropic() {
        let models = models_for_provider(LlmProvider::Anthropic);
        assert!(!models.is_empty());
        assert!(models.iter().all(|m| m.provider == LlmProvider::Anthropic));
    }

    #[test]
    fn test_all_providers_contains_major() {
        let providers = all_providers();
        assert!(providers.contains(&LlmProvider::Anthropic));
        assert!(providers.contains(&LlmProvider::OpenAI));
        assert!(providers.contains(&LlmProvider::Gemini));
    }

    fn slugs_of(providers: &[LlmProvider]) -> Vec<String> {
        providers.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn test_filter_providers_empty_lists_pass_through() {
        let input = vec![LlmProvider::Anthropic, LlmProvider::OpenAI];
        let out = filter_providers(input.clone(), &[], &[]);
        assert_eq!(out, input);
    }

    #[test]
    fn test_filter_providers_allowlist_only() {
        let input = vec![
            LlmProvider::Anthropic,
            LlmProvider::OpenAI,
            LlmProvider::Ollama,
        ];
        let enabled = vec!["anthropic".to_string(), "openai".to_string()];
        let out = filter_providers(input, &enabled, &[]);
        assert_eq!(slugs_of(&out), vec!["anthropic", "openai"]);
    }

    #[test]
    fn test_filter_providers_denylist_only() {
        let input = vec![
            LlmProvider::Anthropic,
            LlmProvider::OpenAI,
            LlmProvider::Ollama,
        ];
        let disabled = vec!["ollama".to_string()];
        let out = filter_providers(input, &[], &disabled);
        assert_eq!(slugs_of(&out), vec!["anthropic", "openai"]);
    }

    #[test]
    fn test_filter_providers_allowlist_plus_denylist() {
        let input = vec![
            LlmProvider::Anthropic,
            LlmProvider::OpenAI,
            LlmProvider::Ollama,
        ];
        let enabled = vec!["anthropic".to_string(), "ollama".to_string()];
        let disabled = vec!["ollama".to_string()];
        let out = filter_providers(input, &enabled, &disabled);
        assert_eq!(slugs_of(&out), vec!["anthropic"]);
    }

    #[test]
    fn test_filter_providers_case_insensitive() {
        let input = vec![LlmProvider::Anthropic, LlmProvider::OpenAI];
        let enabled = vec!["ANTHROPIC".to_string()];
        let out = filter_providers(input, &enabled, &[]);
        assert_eq!(slugs_of(&out), vec!["anthropic"]);
    }

    #[test]
    fn test_filter_providers_allowlist_no_matches_yields_empty() {
        // filter_providers itself is pure: a non-matching allowlist yields empty.
        // (available_providers wraps this with a fail-open guard.)
        let input = vec![LlmProvider::Anthropic, LlmProvider::OpenAI];
        let enabled = vec!["nonexistent-provider".to_string()];
        let out = filter_providers(input, &enabled, &[]);
        assert!(out.is_empty());
    }

    #[test]
    fn test_available_providers_honours_env_and_fails_open() {
        // An allowlist matching nothing must fall back to all providers so a
        // typo'd SHANNON_ENABLED_PROVIDERS never bricks the picker.
        unsafe {
            std::env::set_var("SHANNON_ENABLED_PROVIDERS", "does-not-exist");
        }
        let fallback = available_providers();
        unsafe {
            std::env::remove_var("SHANNON_ENABLED_PROVIDERS");
        }
        assert!(
            fallback.contains(&LlmProvider::Anthropic),
            "fail-open should still include anthropic"
        );

        // A valid allowlist restricts to matching providers (comma + space
        // separated slugs).
        unsafe {
            std::env::set_var("SHANNON_ENABLED_PROVIDERS", "anthropic, openai");
        }
        let providers = available_providers();
        unsafe {
            std::env::remove_var("SHANNON_ENABLED_PROVIDERS");
        }
        let slugs = slugs_of(&providers);
        assert!(slugs.contains(&"anthropic".to_string()));
        assert!(slugs.contains(&"openai".to_string()));
        assert!(!slugs.contains(&"ollama".to_string()));
    }

    // === effective_provider_allowlist (ADR-0005 P4.9) ===
    //
    // The desktop's Settings UI persists an `enabled_providers` override;
    // the engine reads it via this helper. Precedence is documented on
    // the function; the tests below pin each branch. They save/restore
    // the env vars because `parse_provider_slugs_env` is process-global.

    /// RAII guard that snapshots `SHANNON_ENABLED_PROVIDERS` /
    /// `SHANNON_DISABLED_PROVIDERS` on construction and restores them on
    /// drop — keeps the env-mutating tests from leaking state into
    /// siblings.
    struct EnvGuard {
        saved_enabled: Option<String>,
        saved_disabled: Option<String>,
    }

    impl EnvGuard {
        fn new() -> Self {
            Self {
                saved_enabled: std::env::var("SHANNON_ENABLED_PROVIDERS").ok(),
                saved_disabled: std::env::var("SHANNON_DISABLED_PROVIDERS").ok(),
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.saved_enabled {
                Some(v) => unsafe {
                    std::env::set_var("SHANNON_ENABLED_PROVIDERS", v);
                },
                None => unsafe {
                    std::env::remove_var("SHANNON_ENABLED_PROVIDERS");
                },
            }
            match &self.saved_disabled {
                Some(v) => unsafe {
                    std::env::set_var("SHANNON_DISABLED_PROVIDERS", v);
                },
                None => unsafe {
                    std::env::remove_var("SHANNON_DISABLED_PROVIDERS");
                },
            }
        }
    }

    fn clear_allowlist_env() {
        unsafe {
            std::env::remove_var("SHANNON_ENABLED_PROVIDERS");
        }
        unsafe {
            std::env::remove_var("SHANNON_DISABLED_PROVIDERS");
        }
    }

    #[test]
    fn effective_provider_allowlist_explicit_empty_returns_empty() {
        // `Some(&[])` is the desktop's "hide every provider" state —
        // must short-circuit to the same empty vec, ignoring env vars.
        let _g = EnvGuard::new();
        clear_allowlist_env();
        unsafe {
            std::env::set_var("SHANNON_ENABLED_PROVIDERS", "anthropic");
        }
        let out = effective_provider_allowlist(Some(&[]));
        assert_eq!(out, Some(vec![]));
    }

    #[test]
    fn effective_provider_allowlist_explicit_overrides_env() {
        // User-set non-empty allowlist beats the env. A shell exporting
        // a stale `SHANNON_ENABLED_PROVIDERS` must NOT clobber the
        // desktop's persisted choice.
        let _g = EnvGuard::new();
        clear_allowlist_env();
        unsafe {
            std::env::set_var("SHANNON_ENABLED_PROVIDERS", "anthropic, openai");
        }
        let explicit = vec!["ollama".to_string()];
        let out = effective_provider_allowlist(Some(&explicit));
        assert_eq!(out, Some(vec!["ollama".to_string()]));
    }

    #[test]
    fn effective_provider_allowlist_env_only_returns_parsed() {
        // No explicit slice → fall through to env-var allowlist parsing.
        let _g = EnvGuard::new();
        clear_allowlist_env();
        unsafe {
            std::env::set_var("SHANNON_ENABLED_PROVIDERS", "anthropic, openai");
        }
        let out = effective_provider_allowlist(None);
        let mut v = out.expect("env-only path returns Some");
        v.sort();
        assert_eq!(v, vec!["anthropic".to_string(), "openai".to_string()]);
    }

    #[test]
    fn effective_provider_allowlist_neither_returns_none() {
        // No explicit slice, no env vars → `None` (no restriction). The
        // picker then shows every catalog provider.
        let _g = EnvGuard::new();
        clear_allowlist_env();
        assert_eq!(effective_provider_allowlist(None), None);
    }

    #[test]
    fn effective_provider_allowlist_disabled_only_returns_remaining() {
        // `SHANNON_DISABLED_PROVIDERS` without `SHANNON_ENABLED_PROVIDERS`
        // produces the full provider list minus the disabled slugs.
        let _g = EnvGuard::new();
        clear_allowlist_env();
        unsafe {
            std::env::set_var("SHANNON_DISABLED_PROVIDERS", "ollama");
        }
        let out = effective_provider_allowlist(None).expect("disabled-only path returns Some");
        let slugs: Vec<String> = out.into_iter().map(|s| s.to_lowercase()).collect();
        assert!(slugs.contains(&"anthropic".to_string()));
        assert!(!slugs.contains(&"ollama".to_string()));
    }

    #[test]
    fn env_provider_allowlist_returns_none_when_neither_set() {
        // `env_provider_allowlist` is the same logic but with no
        // explicit override. Sanity-check that the helper itself
        // returns `None` when both env vars are empty.
        let _g = EnvGuard::new();
        clear_allowlist_env();
        assert!(env_provider_allowlist().is_none());
    }

    #[test]
    fn test_all_model_ids() {
        let ids = all_model_ids();
        assert!(ids.contains(&"claude-sonnet-4-20250514"));
        assert!(ids.contains(&"gpt-4o"));
        assert!(ids.len() >= 42);
    }

    #[test]
    fn test_provider_display_name() {
        assert_eq!(provider_display_name(&LlmProvider::Anthropic), "Anthropic");
        assert_eq!(provider_display_name(&LlmProvider::OpenAI), "OpenAI");
        assert_eq!(provider_display_name(&LlmProvider::Gemini), "Google");
    }

    #[test]
    fn test_provider_order() {
        assert!(provider_order(&LlmProvider::Anthropic) < provider_order(&LlmProvider::OpenAI));
        assert!(provider_order(&LlmProvider::OpenAI) < provider_order(&LlmProvider::Groq));
    }

    #[test]
    fn test_capabilities_or_and_has() {
        let caps = ModelCapabilities::coding().or(ModelCapabilities::reasoning());
        assert!(caps.has(ModelCapabilities::coding()));
        assert!(caps.has(ModelCapabilities::reasoning()));
        assert!(!caps.has(ModelCapabilities::vision()));
    }

    #[test]
    fn test_model_info_for_known() {
        let info = model_info_for("claude-sonnet-4-20250514").unwrap();
        assert_eq!(info.display_name, "Claude Sonnet 4");
        assert_eq!(info.context_window, 200_000);
        assert!(info.cost_per_m_input > 0.0);
    }

    #[test]
    fn test_model_info_for_unknown() {
        assert!(model_info_for("nonexistent-model").is_none());
    }

    #[test]
    fn test_router_recommend_code() {
        let id = ModelRouter::recommend(TaskType::CodeGeneration);
        let info = model_info_for(id).unwrap();
        assert!(info.capabilities.has(ModelCapabilities::coding()));
    }

    #[test]
    fn test_router_recommend_fast() {
        let id = ModelRouter::recommend_fast(TaskType::QuickQuery);
        // Should return a speed-capable model or fallback
        assert!(model_info_for(id).is_some());
    }

    #[test]
    fn test_router_estimate_cost() {
        let cost = ModelRouter::estimate_cost("claude-sonnet-4-20250514", 1_000_000, 1_000_000);
        assert!(cost > 0.0);
        // $3/M input + $15/M output = $18 for 1M each
        assert!((cost - 18.0).abs() < 0.01);
    }

    #[test]
    fn test_router_estimate_cost_unknown() {
        assert_eq!(ModelRouter::estimate_cost("nonexistent", 1000, 1000), 0.0);
    }

    // ── Alias tests ──

    #[test]
    fn test_resolve_alias_opus() {
        let resolved = resolve_model_alias("opus", None);
        assert!(resolved.is_some());
        let id = resolved.unwrap();
        let info = model_info_for(id).unwrap();
        assert!(info.capabilities.has(ModelCapabilities::reasoning()));
    }

    #[test]
    fn test_resolve_alias_sonnet() {
        let resolved = resolve_model_alias("sonnet", None);
        assert!(resolved.is_some());
        let id = resolved.unwrap();
        let info = model_info_for(id).unwrap();
        assert!(info.capabilities.has(ModelCapabilities::coding()));
    }

    #[test]
    fn test_resolve_alias_haiku() {
        let resolved = resolve_model_alias("haiku", None);
        assert!(resolved.is_some());
        let id = resolved.unwrap();
        let info = model_info_for(id).unwrap();
        assert!(info.capabilities.has(ModelCapabilities::cheap()));
    }

    #[test]
    fn test_resolve_alias_per_provider() {
        let anthropic = resolve_model_alias("opus", Some(&LlmProvider::Anthropic));
        assert!(anthropic.is_some());
        assert!(anthropic.unwrap().starts_with("claude-opus"));

        let openai = resolve_model_alias("haiku", Some(&LlmProvider::OpenAI));
        assert!(openai.is_some());
        assert!(openai.unwrap().contains("mini"));
    }

    #[test]
    fn test_resolve_alias_unknown() {
        assert!(resolve_model_alias("claude-sonnet-4-20250514", None).is_none());
        assert!(resolve_model_alias("gpt-4o", None).is_none());
    }

    #[test]
    fn test_resolve_model_passthrough() {
        assert_eq!(
            resolve_model("claude-sonnet-4-20250514", None),
            "claude-sonnet-4-20250514"
        );
    }

    #[test]
    fn test_resolve_model_alias_resolved() {
        let resolved = resolve_model("haiku", None);
        // Should resolve to an actual model ID, not "haiku"
        assert_ne!(resolved, "haiku");
        assert!(model_info_for(&resolved).is_some());
    }

    #[test]
    fn resolve_auto_tier_prefers_standard_when_available() {
        // Anthropic ships all tiers, so the lightweight heuristic should pick
        // Standard (the workhorse default) and agree with an explicit
        // `/model --tier standard`.
        let tiers = ProviderTiers::default();
        let (tier, id) =
            resolve_auto_tier(&LlmProvider::Anthropic, &tiers).expect("anthropic has tiers");
        assert_eq!(tier, TierName::Standard);
        assert_eq!(
            id,
            resolve_tier("standard", &LlmProvider::Anthropic, &tiers).unwrap()
        );
    }

    #[test]
    fn resolve_auto_tier_respects_profile_override() {
        // A pinned providers.toml override for `standard` wins for auto too,
        // since auto delegates through resolve_tier.
        let mut tiers = ProviderTiers::default();
        tiers.standard = Some("claude-opus-4-20250115".to_string());
        let (tier, id) =
            resolve_auto_tier(&LlmProvider::Anthropic, &tiers).expect("resolves via override");
        assert_eq!(tier, TierName::Standard);
        assert_eq!(id, "claude-opus-4-20250115");
    }

    #[test]
    fn resolve_auto_tier_none_for_provider_with_no_models() {
        // Ollama has no static catalog entries and no alias-mapped models, so
        // every candidate tier misses — auto honestly returns None rather than
        // inventing a model.
        let tiers = ProviderTiers::default();
        assert!(resolve_auto_tier(&LlmProvider::Ollama, &tiers).is_none());
    }

    #[test]
    fn test_is_model_alias() {
        assert!(is_model_alias("opus"));
        assert!(is_model_alias("sonnet"));
        assert!(is_model_alias("haiku"));
        assert!(is_model_alias("fast"));
        assert!(is_model_alias("mini"));
        assert!(!is_model_alias("gpt-4o"));
        assert!(!is_model_alias("claude-sonnet-4"));
    }

    #[test]
    fn test_effort_level_parse() {
        assert_eq!(EffortLevel::from_str_opt("low"), Some(EffortLevel::Low));
        assert_eq!(EffortLevel::from_str_opt("HIGH"), Some(EffortLevel::High));
        assert_eq!(
            EffortLevel::from_str_opt("medium"),
            Some(EffortLevel::Medium)
        );
        assert_eq!(EffortLevel::from_str_opt("med"), Some(EffortLevel::Medium));
        assert_eq!(EffortLevel::from_str_opt("unknown"), None);
    }

    #[test]
    fn test_effort_level_budget() {
        assert!(EffortLevel::Low.thinking_budget().is_none());
        assert!(EffortLevel::Medium.thinking_budget().unwrap() > 0);
        assert!(
            EffortLevel::High.thinking_budget().unwrap()
                > EffortLevel::Medium.thinking_budget().unwrap()
        );
    }

    // ── context_window_for with prefix matching ─────────────────────────

    #[test]
    fn test_context_window_exact_match() {
        assert_eq!(context_window_for("claude-sonnet-4-20250514"), 200_000);
        assert_eq!(context_window_for("gpt-4o"), 128_000);
        assert_eq!(context_window_for("gemini-2.5-pro"), 1_000_000);
    }

    #[test]
    fn test_context_window_prefix_match_short_name() {
        // Short model name should match full catalog entry via prefix
        assert_eq!(context_window_for("claude-sonnet-4"), 200_000);
        assert_eq!(context_window_for("claude-opus-4"), 200_000);
        assert_eq!(context_window_for("claude-haiku-4"), 200_000);
    }

    #[test]
    fn test_context_window_reverse_prefix_match() {
        // Model ID that starts with a catalog entry should match
        assert_eq!(context_window_for("gpt-4o-2024-08-06"), 128_000);
    }

    #[test]
    fn test_context_window_unknown_fallback() {
        assert_eq!(context_window_for("totally-unknown-model"), 200_000);
    }

    #[test]
    fn test_context_window_opt_honest_about_unknown() {
        // Known models resolve to their cataloged window.
        assert_eq!(context_window_for_opt("gpt-4o"), Some(128_000));
        assert_eq!(context_window_for_opt("gemini-2.5-pro"), Some(1_000_000));
        // Unknown models are None — not a fabricated 200K (Phase E).
        assert_eq!(context_window_for_opt("totally-unknown-model"), None);
        // The usize accessor still falls back to the named constant.
        assert_eq!(
            context_window_for("totally-unknown-model"),
            FALLBACK_CONTEXT_WINDOW
        );
    }

    #[test]
    fn test_context_window_prefix_prefers_exact() {
        // Exact match takes priority over prefix match
        assert_eq!(context_window_for("claude-3-5-sonnet-20241022"), 200_000);
    }

    // ── DeepSeek V4 tests ─────────────────────────────────────

    #[test]
    fn test_deepseek_v4_flash_registered() {
        let info =
            model_info_for("deepseek-v4-flash").expect("deepseek-v4-flash should be registered");
        assert_eq!(info.context_window, 1_000_000);
        assert_eq!(info.max_output, 384_000);
        assert!(info.capabilities.has(ModelCapabilities::coding()));
        assert!(info.capabilities.has(ModelCapabilities::cheap()));
        assert!(info.capabilities.has(ModelCapabilities::speed()));
    }

    #[test]
    fn test_deepseek_v4_pro_registered() {
        let info = model_info_for("deepseek-v4-pro").expect("deepseek-v4-pro should be registered");
        assert_eq!(info.context_window, 1_000_000);
        assert_eq!(info.max_output, 384_000);
        assert!(info.capabilities.has(ModelCapabilities::coding()));
        assert!(info.capabilities.has(ModelCapabilities::reasoning()));
    }

    #[test]
    fn test_deepseek_v4_context_window_lookup() {
        assert_eq!(context_window_for("deepseek-v4-flash"), 1_000_000);
        assert_eq!(context_window_for("deepseek-v4-pro"), 1_000_000);
    }

    // ── GLM / Zhipu tests ─────────────────────────────────────

    #[test]
    fn test_glm_models_registered() {
        let glm_ids = [
            "glm-4-plus",
            "glm-4-flash",
            "glm-4-long",
            "glm-4-air",
            "glm-4v-flash",
            "glm-5",
            "glm-5.1",
            "glm-5-flash",
            "glm-5.1-flash",
        ];
        for id in &glm_ids {
            assert!(
                model_info_for(id).is_some(),
                "GLM model {id} should be registered"
            );
        }
    }

    #[test]
    fn test_glm_4_flash_is_cheap_and_fast() {
        let info = model_info_for("glm-4-flash").unwrap();
        assert!(info.capabilities.has(ModelCapabilities::cheap()));
        assert!(info.capabilities.has(ModelCapabilities::speed()));
        assert_eq!(info.context_window, 128_000);
    }

    #[test]
    fn test_glm_4_long_has_1m_context() {
        let info = model_info_for("glm-4-long").unwrap();
        assert_eq!(info.context_window, 1_000_000);
    }

    #[test]
    fn test_glm_4v_has_vision() {
        let info = model_info_for("glm-4v-flash").unwrap();
        assert!(info.capabilities.has(ModelCapabilities::vision()));
    }

    #[test]
    fn test_glm_context_window_lookup() {
        assert_eq!(context_window_for("glm-4-plus"), 128_000);
        assert_eq!(context_window_for("glm-4-flash"), 128_000);
        assert_eq!(context_window_for("glm-4-long"), 1_000_000);
        assert_eq!(context_window_for("glm-4-air"), 128_000);
        assert_eq!(context_window_for("glm-4v-flash"), 128_000);
        assert_eq!(context_window_for("glm-5"), 198_000);
        assert_eq!(context_window_for("glm-5.1"), 198_000);
        assert_eq!(context_window_for("glm-5-flash"), 198_000);
        assert_eq!(context_window_for("glm-5.1-flash"), 198_000);
    }

    #[test]
    fn test_models_for_provider_zhipu() {
        let glm = models_for_provider(LlmProvider::Zhipu);
        assert!(
            glm.len() >= 9,
            "Zhipu should have at least 9 models, got {}",
            glm.len()
        );
    }

    // ── Zhipu International tests ──────────────────────────────

    #[test]
    fn test_zhipu_intl_models_registered() {
        let intl_ids = [
            "glm-4-plus-intl",
            "glm-4-flash-intl",
            "glm-4-long-intl",
            "glm-5-intl",
            "glm-5.1-intl",
            "glm-5-flash-intl",
        ];
        for id in &intl_ids {
            assert!(
                model_info_for(id).is_some(),
                "Zhipu Intl model {id} should be registered"
            );
        }
    }

    #[test]
    fn test_zhipu_intl_models_for_provider() {
        let models = models_for_provider(LlmProvider::ZhipuInternational);
        assert!(
            models.len() >= 6,
            "ZhipuInternational should have at least 6 models, got {}",
            models.len()
        );
    }

    #[test]
    fn test_zhipu_intl_glm51_output() {
        let info = model_info_for("glm-5.1-intl").expect("glm-5.1-intl should be registered");
        assert_eq!(info.context_window, 198_000);
        assert_eq!(info.max_output, 128_000);
    }

    #[test]
    fn test_zhipu_intl_glm5_context() {
        assert_eq!(context_window_for("glm-5-intl"), 198_000);
        assert_eq!(context_window_for("glm-5.1-intl"), 198_000);
        assert_eq!(context_window_for("glm-5-flash-intl"), 198_000);
    }

    #[test]
    fn test_zhipu_intl_glm4_long_1m() {
        assert_eq!(context_window_for("glm-4-long-intl"), 1_000_000);
    }

    #[test]
    fn test_zhipu_intl_provider_display_name() {
        assert_eq!(
            provider_display_name(&LlmProvider::ZhipuInternational),
            "GLM (Zhipu Int'l)"
        );
    }

    #[test]
    fn test_zhipu_intl_wire_format_openai() {
        assert_eq!(
            LlmProvider::ZhipuInternational.wire_format(),
            WireFormat::OpenAI
        );
        assert!(LlmProvider::ZhipuInternational.is_openai_compatible());
    }

    #[test]
    fn test_zhipu_intl_default_endpoint() {
        assert_eq!(
            LlmProvider::ZhipuInternational.default_base_url(),
            "https://open.international.bigmodel.cn"
        );
        assert_eq!(
            LlmProvider::ZhipuInternational.endpoint(),
            "/api/paas/v4/chat/completions"
        );
    }

    #[test]
    fn test_zhipu_intl_from_url() {
        assert_eq!(
            LlmProvider::from_base_url("https://open.international.bigmodel.cn"),
            LlmProvider::ZhipuInternational
        );
    }

    #[test]
    fn test_zhipu_intl_provider_order() {
        assert!(
            provider_order(&LlmProvider::ZhipuInternational) > provider_order(&LlmProvider::Zhipu)
        );
        assert!(
            provider_order(&LlmProvider::ZhipuInternational)
                < provider_order(&LlmProvider::Moonshot)
        );
    }

    // ── Kimi / Moonshot tests ──────────────────────────────────

    #[test]
    fn test_kimi_k26_registered() {
        let info = model_info_for("kimi-k2.6").expect("kimi-k2.6 should be registered");
        assert_eq!(info.context_window, 256_000);
        assert!(info.capabilities.has(ModelCapabilities::coding()));
        assert!(info.capabilities.has(ModelCapabilities::reasoning()));
        assert!(info.capabilities.has(ModelCapabilities::vision()));
    }

    #[test]
    fn test_kimi_models_have_vision() {
        for id in &["kimi-k2.6", "kimi-k2.5"] {
            let info = model_info_for(id).unwrap();
            assert!(
                info.capabilities.has(ModelCapabilities::vision()),
                "{id} should have vision"
            );
        }
    }

    #[test]
    fn test_moonshot_v1_context_windows() {
        assert_eq!(context_window_for("moonshot-v1-8k"), 8_000);
        assert_eq!(context_window_for("moonshot-v1-32k"), 32_000);
        assert_eq!(context_window_for("moonshot-v1-128k"), 128_000);
    }

    #[test]
    fn test_moonshot_models_for_provider() {
        let models = models_for_provider(LlmProvider::Moonshot);
        assert!(
            models.len() >= 5,
            "Moonshot should have at least 5 models, got {}",
            models.len()
        );
    }

    #[test]
    fn test_moonshot_provider_display_name() {
        assert_eq!(
            provider_display_name(&LlmProvider::Moonshot),
            "Kimi (Moonshot)"
        );
    }

    #[test]
    fn test_zhipu_provider_display_name() {
        assert_eq!(provider_display_name(&LlmProvider::Zhipu), "GLM (Zhipu)");
    }

    // ── Cross-provider tests ───────────────────────────────────

    #[test]
    fn test_all_six_chinese_providers_have_models() {
        assert!(!models_for_provider(LlmProvider::DeepSeek).is_empty());
        assert!(!models_for_provider(LlmProvider::Zhipu).is_empty());
        assert!(!models_for_provider(LlmProvider::ZhipuInternational).is_empty());
        assert!(!models_for_provider(LlmProvider::Moonshot).is_empty());
        assert!(!models_for_provider(LlmProvider::Minimax).is_empty());
        assert!(!models_for_provider(LlmProvider::DashScope).is_empty());
    }

    #[test]
    fn test_context_window_lookup_all_new_models() {
        // All new models should resolve to their registered context window
        for id in &[
            "deepseek-v4-flash",
            "deepseek-v4-pro",
            "glm-4-plus",
            "glm-4-flash",
            "glm-4-long",
            "glm-4-air",
            "glm-4v-flash",
            "glm-5",
            "glm-5.1",
            "glm-5-flash",
            "glm-5.1-flash",
            "glm-4-plus-intl",
            "glm-4-flash-intl",
            "glm-4-long-intl",
            "glm-5-intl",
            "glm-5.1-intl",
            "glm-5-flash-intl",
            "kimi-k2.6",
            "kimi-k2.5",
            "moonshot-v1-8k",
            "moonshot-v1-32k",
            "moonshot-v1-128k",
            "MiniMax-M2.7",
            "MiniMax-M2.5",
            "MiniMax-M2.7-highspeed",
            "qwen3.7-max",
            "qwen3.6-plus",
            "qwen3.6-flash",
        ] {
            let ctx = context_window_for(id);
            assert!(ctx > 0, "{id} should have a positive context window");
            // Verify model is in catalog (not falling back to generic 200K default)
            let in_catalog = model_info_for(id).is_some();
            assert!(
                in_catalog,
                "{id} resolved to context {ctx} but is not in catalog — check registration"
            );
        }
    }

    #[test]
    fn test_moonshot_wire_format() {
        assert_eq!(LlmProvider::Moonshot.wire_format(), WireFormat::OpenAI);
        assert!(LlmProvider::Moonshot.is_openai_compatible());
    }

    #[test]
    fn test_moonshot_default_endpoint() {
        assert_eq!(
            LlmProvider::Moonshot.default_base_url(),
            "https://api.moonshot.cn"
        );
        assert_eq!(LlmProvider::Moonshot.endpoint(), "/v1/chat/completions");
    }

    #[test]
    fn test_moonshot_from_url() {
        assert_eq!(
            LlmProvider::from_base_url("https://api.moonshot.cn"),
            LlmProvider::Moonshot
        );
        assert_eq!(
            LlmProvider::from_base_url("https://api.moonshot.cn/v1"),
            LlmProvider::Moonshot
        );
    }

    // ── GLM-5 / GLM-5.1 tests ──────────────────────────────────

    #[test]
    fn test_glm5_registered() {
        let info = model_info_for("glm-5").expect("glm-5 should be registered");
        assert_eq!(info.context_window, 198_000);
        assert!(info.capabilities.has(ModelCapabilities::coding()));
        assert!(info.capabilities.has(ModelCapabilities::reasoning()));
    }

    #[test]
    fn test_glm51_registered() {
        let info = model_info_for("glm-5.1").expect("glm-5.1 should be registered");
        assert_eq!(info.context_window, 198_000);
        assert!(info.capabilities.has(ModelCapabilities::coding()));
        assert!(info.capabilities.has(ModelCapabilities::reasoning()));
    }

    #[test]
    fn test_glm5_flash_cheap() {
        let info = model_info_for("glm-5-flash").expect("glm-5-flash should be registered");
        assert!(info.capabilities.has(ModelCapabilities::cheap()));
        assert!(info.capabilities.has(ModelCapabilities::speed()));
    }

    #[test]
    fn test_glm51_flash_cheap() {
        let info = model_info_for("glm-5.1-flash").expect("glm-5.1-flash should be registered");
        assert!(info.capabilities.has(ModelCapabilities::cheap()));
        assert!(info.capabilities.has(ModelCapabilities::speed()));
    }

    #[test]
    fn test_glm5_output_tokens() {
        assert_eq!(model_info_for("glm-5").unwrap().max_output, 16_384);
        assert_eq!(model_info_for("glm-5.1").unwrap().max_output, 128_000);
        assert_eq!(model_info_for("glm-5-flash").unwrap().max_output, 16_384);
        assert_eq!(model_info_for("glm-5.1-flash").unwrap().max_output, 16_384);
    }

    // ── Qwen / DashScope tests ────────────────────────────────

    #[test]
    fn test_qwen_models_registered() {
        let qwen_ids = ["qwen3.7-max", "qwen3.6-plus", "qwen3.6-flash"];
        for id in &qwen_ids {
            assert!(
                model_info_for(id).is_some(),
                "Qwen model {id} should be registered"
            );
        }
    }

    #[test]
    fn test_qwen_37_max_flagship() {
        let info = model_info_for("qwen3.7-max").expect("qwen3.7-max should be registered");
        assert_eq!(info.context_window, 1_000_000);
        assert!(info.capabilities.has(ModelCapabilities::coding()));
        assert!(info.capabilities.has(ModelCapabilities::reasoning()));
    }

    #[test]
    fn test_qwen_36_flash_cheap_and_fast() {
        let info = model_info_for("qwen3.6-flash").expect("qwen3.6-flash should be registered");
        assert!(info.capabilities.has(ModelCapabilities::cheap()));
        assert!(info.capabilities.has(ModelCapabilities::speed()));
        assert_eq!(info.context_window, 1_000_000);
    }

    #[test]
    fn test_qwen_all_have_1m_context() {
        for id in &["qwen3.7-max", "qwen3.6-plus", "qwen3.6-flash"] {
            assert_eq!(
                context_window_for(id),
                1_000_000,
                "{id} should have 1M context"
            );
        }
    }

    #[test]
    fn test_dashscope_provider_display_name() {
        assert_eq!(
            provider_display_name(&LlmProvider::DashScope),
            "Qwen (DashScope)"
        );
    }

    #[test]
    fn test_dashscope_wire_format_openai() {
        assert_eq!(LlmProvider::DashScope.wire_format(), WireFormat::OpenAI);
        assert!(LlmProvider::DashScope.is_openai_compatible());
    }

    #[test]
    fn test_dashscope_default_endpoint() {
        assert_eq!(
            LlmProvider::DashScope.default_base_url(),
            "https://dashscope.aliyuncs.com"
        );
        assert_eq!(
            LlmProvider::DashScope.endpoint(),
            "/compatible-mode/v1/chat/completions"
        );
    }

    #[test]
    fn test_dashscope_from_url() {
        assert_eq!(
            LlmProvider::from_base_url("https://dashscope.aliyuncs.com"),
            LlmProvider::DashScope
        );
    }

    #[test]
    fn test_dashscope_provider_order() {
        assert!(provider_order(&LlmProvider::DashScope) > provider_order(&LlmProvider::Minimax));
        assert!(provider_order(&LlmProvider::DashScope) < provider_order(&LlmProvider::Ollama));
    }

    #[test]
    fn test_dashscope_models_for_provider() {
        let models = models_for_provider(LlmProvider::DashScope);
        assert!(
            models.len() >= 3,
            "DashScope should have at least 3 models, got {}",
            models.len()
        );
    }

    // ── MiniMax tests ───────────────────────────────────────────

    #[test]
    fn test_minimax_m27_registered() {
        let info = model_info_for("MiniMax-M2.7").expect("MiniMax-M2.7 should be registered");
        assert_eq!(info.context_window, 1_000_000);
        assert!(info.capabilities.has(ModelCapabilities::coding()));
        assert!(info.capabilities.has(ModelCapabilities::reasoning()));
    }

    #[test]
    fn test_minimax_m25_registered() {
        let info = model_info_for("MiniMax-M2.5").expect("MiniMax-M2.5 should be registered");
        assert_eq!(info.context_window, 192_000);
        assert!(info.capabilities.has(ModelCapabilities::coding()));
        assert!(info.capabilities.has(ModelCapabilities::cheap()));
    }

    #[test]
    fn test_minimax_m27_highspeed() {
        let info = model_info_for("MiniMax-M2.7-highspeed")
            .expect("MiniMax-M2.7-highspeed should be registered");
        assert_eq!(info.context_window, 1_000_000);
        assert!(info.capabilities.has(ModelCapabilities::coding()));
        assert!(info.capabilities.has(ModelCapabilities::speed()));
    }

    #[test]
    fn test_minimax_context_window_lookup() {
        assert_eq!(context_window_for("MiniMax-M2.7"), 1_000_000);
        assert_eq!(context_window_for("MiniMax-M2.5"), 192_000);
        assert_eq!(context_window_for("MiniMax-M2.7-highspeed"), 1_000_000);
    }

    #[test]
    fn test_minimax_models_for_provider() {
        let models = models_for_provider(LlmProvider::Minimax);
        assert!(
            models.len() >= 3,
            "Minimax should have at least 3 models, got {}",
            models.len()
        );
    }

    #[test]
    fn test_minimax_provider_display_name() {
        assert_eq!(provider_display_name(&LlmProvider::Minimax), "MiniMax");
    }

    #[test]
    fn test_minimax_wire_format() {
        assert_eq!(LlmProvider::Minimax.wire_format(), WireFormat::OpenAI);
        assert!(LlmProvider::Minimax.is_openai_compatible());
    }

    #[test]
    fn test_minimax_default_endpoint() {
        assert_eq!(
            LlmProvider::Minimax.default_base_url(),
            "https://api.minimax.chat"
        );
    }

    #[test]
    fn test_minimax_from_url() {
        assert_eq!(
            LlmProvider::from_base_url("https://api.minimax.chat"),
            LlmProvider::Minimax
        );
        assert_eq!(
            LlmProvider::from_base_url("https://api.minimaxi.com"),
            LlmProvider::Minimax
        );
    }

    #[test]
    fn test_minimax_provider_order() {
        assert!(provider_order(&LlmProvider::Minimax) > provider_order(&LlmProvider::Moonshot));
        assert!(provider_order(&LlmProvider::Minimax) < provider_order(&LlmProvider::Ollama));
    }

    #[test]
    fn test_minimax_output_tokens() {
        assert_eq!(model_info_for("MiniMax-M2.7").unwrap().max_output, 64_000);
        assert_eq!(model_info_for("MiniMax-M2.5").unwrap().max_output, 32_000);
        assert_eq!(
            model_info_for("MiniMax-M2.7-highspeed").unwrap().max_output,
            64_000
        );
    }

    fn find_model(id: &str) -> Option<&'static ModelInfo> {
        // Aliases let the brief's exact test IDs resolve against the current
        // catalog without adding obsolete duplicate entries. The static
        // fixture covers `o1-preview`, which has no canonical catalog entry.
        static O1_PREVIEW: ModelInfo = ModelInfo {
            id: "o1-preview",
            display_name: "o1-preview",
            aliases: &[],
            provider: LlmProvider::OpenAI,
            context_window: 128_000,
            max_output: 32_768,
            cost_per_m_input: 15.0,
            cost_per_m_output: 60.0,
            capabilities: ModelCapabilities::reasoning(),
        };
        match id {
            "claude-haiku-4-5" => MODEL_CATALOG
                .iter()
                .find(|m| m.id == "claude-haiku-4-5-20251001"),
            "claude-opus-4" => MODEL_CATALOG
                .iter()
                .find(|m| m.id == "claude-opus-4-20250115"),
            "gemini-1.5-flash" => MODEL_CATALOG.iter().find(|m| m.id == "gemini-2.5-flash"),
            "gemini-1.5-pro" => MODEL_CATALOG.iter().find(|m| m.id == "gemini-2.5-pro"),
            "o1-preview" => Some(&O1_PREVIEW),
            other => MODEL_CATALOG.iter().find(|m| m.id == other),
        }
    }

    #[test]
    fn tier_label_classifies_anthropic_models() {
        let haiku = find_model("claude-haiku-4-5-20251001").unwrap();
        assert_eq!(haiku.tier_label(), TierLabel::Fast);

        let sonnet = find_model("claude-sonnet-4-20250514").unwrap();
        assert_eq!(sonnet.tier_label(), TierLabel::Standard);

        let opus = find_model("claude-opus-4-20250115").unwrap();
        assert_eq!(opus.tier_label(), TierLabel::Pro);
    }

    #[test]
    fn tier_label_for_id_resolves_exact_short_and_unknown() {
        // Exact catalog id.
        assert_eq!(
            tier_label_for_id("claude-haiku-4-5-20251001"),
            TierLabel::Fast
        );
        // Short name resolves via prefix match (no exact catalog entry).
        assert_eq!(tier_label_for_id("claude-haiku-4-5"), TierLabel::Fast);
        assert_eq!(tier_label_for_id("claude-opus-4"), TierLabel::Pro);
        assert_eq!(tier_label_for_id("claude-sonnet-4"), TierLabel::Standard);
        // Non-catalog / empty.
        assert_eq!(tier_label_for_id("made-up-model"), TierLabel::Unknown);
        assert_eq!(tier_label_for_id(""), TierLabel::Unknown);
    }

    #[test]
    fn tier_label_classifies_gemini_models() {
        let flash = find_model("gemini-2.5-flash").unwrap();
        assert_eq!(flash.tier_label(), TierLabel::Fast);

        let pro = find_model("gemini-2.5-pro").unwrap();
        assert_eq!(pro.tier_label(), TierLabel::Standard);
    }

    #[test]
    fn tier_label_classifies_openai_models() {
        let mini = find_model("gpt-4o-mini").unwrap();
        assert_eq!(mini.tier_label(), TierLabel::Fast);

        let o1 = find_model("o1-preview").unwrap();
        assert_eq!(o1.tier_label(), TierLabel::Pro);
    }

    #[test]
    fn catalog_previously_empty_providers_now_have_models() {
        // Phase A: providers that previously had zero catalog entries.
        let xai = models_for_provider(LlmProvider::Xai);
        assert!(!xai.is_empty(), "xAI should have models");
        assert!(xai.iter().any(|m| m.id == "grok-4.5"));
        for provider in [
            LlmProvider::Perplexity,
            LlmProvider::Cohere,
            LlmProvider::SiliconFlow,
            LlmProvider::Together,
            LlmProvider::Fireworks,
            LlmProvider::Ai21,
        ] {
            assert!(
                !models_for_provider(provider.clone()).is_empty(),
                "{provider:?} should have at least one model"
            );
        }
    }

    #[test]
    fn catalog_2026_frontier_models_resolve() {
        // Anthropic 4.6 — 1M context GA (no beta header needed).
        let sonnet46 = model_info_for("claude-sonnet-4-6").unwrap();
        assert_eq!(sonnet46.context_window, 1_000_000);
        assert_eq!(
            model_info_for("claude-opus-4-6").unwrap().context_window,
            1_000_000
        );
        // OpenAI GPT-5 alias resolves.
        assert!(model_info_for_alias("gpt5").is_some());
        // grok alias → grok-4.5 (grok-4 retired).
        let grok = model_info_for_alias("grok").unwrap();
        assert_eq!(grok.id, "grok-4.5");
        // grok-4.5 (coding+reasoning) → Standard; grok-4.1-fast (speed+cheap) → Fast.
        assert_eq!(grok.tier_label(), TierLabel::Standard);
        assert_eq!(
            model_info_for("grok-4.1-fast").unwrap().tier_label(),
            TierLabel::Fast
        );
    }
}
