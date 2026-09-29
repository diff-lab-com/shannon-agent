//! R2-4 — runtime registry of per-model metadata declarations.
//!
//! `providers.toml` v2 [`ProviderProfile`]s can carry a `models` list
//! ([`ModelSpec`]): per-model pricing, context window, max output and
//! capability declarations, authored for exactly the openai-compatible /
//! proxy endpoints whose models are absent (or wrong) in the curated
//! catalog. This module is the in-memory layer that makes those
//! declarations **authoritative** at the engine's existing lookup
//! boundaries:
//!
//! - **Pricing** — [`query_engine::types::find_pricing`] consults
//!   [`pricing_for`] first, so a declared price beats the catalog, the
//!   built-in gap-fillers, the `.shannon-pricing.json` /
//!   `SHANNON_PRICING_JSON` overlays and the LiteLLM feed. Matching is
//!   exact-id only, which retires the substring-collision failure class
//!   (glm-5.3-flash / openai/gpt-5-mini precedents) and with it the
//!   pricing dual-table drift (review 2026-09-29 §2 item 22).
//! - **Context window** — [`QueryEngine::resolve_max_context_tokens`]
//!   consults [`context_window_for`] between the user override and the
//!   model registry, so compaction budgets follow the declared limit.
//! - **Tier classification** — [`model_registry::tier_label_for_id`]
//!   consults [`tier_label_for`], feeding declared capabilities through
//!   the same heuristic the catalog entries use.
//!
//! ## Lifecycle
//!
//! The registry is process-global (one active profile per process, B3
//! phase-1). [`replace_from_specs`] is called from
//! [`crate::unified_config::build_client_from_resolved`] — every host
//! (CLI, REPL, desktop) builds its client through that funnel, so a
//! provider/model switch re-registers the declarations of whichever
//! profile actually drove the client. Registration is *replacement*, not
//! accumulation: stale declarations never outlive their profile.
//!
//! [`QueryEngine::resolve_max_context_tokens`]:
//!     crate::query_engine::engine::QueryEngine::resolve_max_context_tokens

use std::collections::BTreeMap;
use std::sync::RwLock;

use shannon_types::provider_config::{ModelCapability, ModelSpec, ProviderModelConfig};

use crate::model_registry::{ModelCapabilities, TierLabel};

/// Snapshot of one model's declared metadata. Field-for-field the
/// [`ModelSpec`] payload minus the id (which keys the registry).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeclaredModelMeta {
    pub display_name: Option<String>,
    pub context_window: Option<u32>,
    pub max_output: Option<u32>,
    pub cost_per_m_input: Option<f64>,
    pub cost_per_m_output: Option<f64>,
    pub capabilities: Vec<ModelCapability>,
}

impl DeclaredModelMeta {
    /// Project a schema [`ModelSpec`] onto the runtime snapshot.
    pub fn from_spec(spec: &ModelSpec) -> Self {
        Self {
            display_name: spec.display_name.clone(),
            context_window: spec.context_window,
            max_output: spec.max_output,
            cost_per_m_input: spec.cost_per_m_input,
            cost_per_m_output: spec.cost_per_m_output,
            capabilities: spec.capabilities.clone(),
        }
    }

    /// The declared pricing entry, when the declaration prices **both**
    /// directions. A lone half is ignored so billing never mixes a declared
    /// price with a guessed one. Cache rates stay `None` (cache tokens bill
    /// at the declared input rate), matching the built-in table's behavior
    /// for non-Anthropic models.
    pub fn pricing(&self) -> Option<crate::query_engine::ModelPricing> {
        Some(crate::query_engine::ModelPricing {
            input_price_per_mtok: self.cost_per_m_input?,
            output_price_per_mtok: self.cost_per_m_output?,
            cache_read_per_mtok: None,
            cache_write_per_mtok: None,
        })
    }
}

static DECLARED: RwLock<BTreeMap<String, DeclaredModelMeta>> = RwLock::new(BTreeMap::new());

/// Replace the whole registry with `specs` (keyed by `ModelSpec::id`).
/// Registration is replacement — call this, not an append, whenever the
/// active profile changes so stale declarations never outlive their
/// profile. Invalid specs (unvalidatable here — the store validates on
/// load) are skipped defensively rather than poisoning the registry.
pub fn replace_from_specs(specs: &[ModelSpec]) {
    let mut map = BTreeMap::new();
    for spec in specs {
        if spec.validate().is_err() {
            tracing::warn!(
                model = %spec.id,
                "skipping invalid per-model declaration (store validation should have caught this)"
            );
            continue;
        }
        map.insert(spec.id.clone(), DeclaredModelMeta::from_spec(spec));
    }
    let count = map.len();
    *registry_mut() = map;
    if count > 0 {
        tracing::debug!(count, "registered per-model metadata declarations");
    }
}

/// Replace the registry from a v2 config: the **active** provider profile's
/// declarations become authoritative (B3 phase-1: single active profile).
/// No active target → the registry is cleared.
pub fn replace_from_config(pm: &ProviderModelConfig) {
    let active = crate::provider_resolver::resolve_active_target(pm);
    match active {
        Some(rt) => replace_from_specs(&rt.profile.models),
        None => clear(),
    }
}

/// Drop every declaration (no active profile / synthesis fallback paths).
pub fn clear() {
    *registry_mut() = BTreeMap::new();
}

/// Re-register declarations for the provider slot with the raw stored id
/// `slug` (e.g. `"glm"`), from the on-disk `providers.toml`. Falls back to
/// the slot whose id canonicalizes to the same [`LlmProvider`], so a REPL
/// switch to `zhipu` still finds a slot stored as `glm`-style custom ids
/// only when they genuinely resolve to the same provider. No matching slot
/// (or no readable file) clears the registry. Read-only on the store.
pub fn replace_for_provider_slug(slug: &str) {
    let Some(cfg) = crate::provider_config_store::load(None) else {
        clear();
        return;
    };
    replace_for_provider_in(slug, &cfg);
}

/// [`replace_for_provider_slug`] against an already-loaded config (the
/// hermetic seam tests use).
pub fn replace_for_provider_in(slug: &str, cfg: &ProviderModelConfig) {
    let Some(mp) = cfg.profiles.get("default") else {
        clear();
        return;
    };
    let want = crate::provider_resolver::llm_provider_from_slug(slug);
    let slot = mp.providers.iter().find(|p| p.id == slug).or_else(|| {
        want.as_ref().and_then(|w| {
            mp.providers.iter().find(|p| {
                crate::provider_resolver::llm_provider_from_slug(&p.id).is_some_and(|pp| &pp == w)
            })
        })
    });
    match slot {
        Some(p) => replace_from_specs(&p.models),
        None => clear(),
    }
}

/// Look up the declared metadata for `model` (exact id match).
pub fn lookup(model: &str) -> Option<DeclaredModelMeta> {
    registry().get(model).cloned()
}

/// Declared pricing for `model`, when both directions are declared.
/// See [`DeclaredModelMeta::pricing`] for the partial-price rule.
pub fn pricing_for(model: &str) -> Option<crate::query_engine::ModelPricing> {
    lookup(model).and_then(|meta| meta.pricing())
}

/// Declared context window for `model`, when declared.
pub fn context_window_for(model: &str) -> Option<usize> {
    lookup(model)
        .and_then(|meta| meta.context_window)
        .map(|v| v as usize)
}

/// Declared max output tokens for `model`, when declared.
pub fn max_output_for(model: &str) -> Option<usize> {
    lookup(model)
        .and_then(|meta| meta.max_output)
        .map(|v| v as usize)
}

/// Tier label derived from the declaration's capabilities, run through the
/// same heuristic as [`crate::model_registry::catalog::ModelInfo::tier_label`]
/// (cheap/speed → Fast, flagship id markers → Pro, reasoning/coding →
/// Standard). `None` when the model has no declaration or the declaration
/// carries no capabilities — the caller falls through to the catalog.
pub fn tier_label_for(model_id: &str) -> Option<TierLabel> {
    let meta = lookup(model_id)?;
    if meta.capabilities.is_empty() {
        return None;
    }
    let mut caps = ModelCapabilities::empty();
    for cap in &meta.capabilities {
        caps = caps.or(match cap {
            ModelCapability::Reasoning => ModelCapabilities::reasoning(),
            ModelCapability::Coding => ModelCapabilities::coding(),
            ModelCapability::Speed => ModelCapabilities::speed(),
            ModelCapability::Cheap => ModelCapabilities::cheap(),
            ModelCapability::Vision => ModelCapabilities::vision(),
            // ModelCapability is #[non_exhaustive]; a future flag contributes
            // no catalog bit until the mapping learns about it.
            _ => ModelCapabilities::empty(),
        });
    }
    // Mirror of ModelInfo::tier_label — keep in sync with the catalog
    // heuristic (cheap/speed, flagship id markers, reasoning/coding).
    let id = model_id.to_lowercase();
    Some(
        if caps.has(ModelCapabilities::cheap()) || caps.has(ModelCapabilities::speed()) {
            TierLabel::Fast
        } else if id.contains("opus")
            || id.contains("o1")
            || id.contains("ultra")
            || id.contains("max")
        {
            TierLabel::Pro
        } else if caps.has(ModelCapabilities::reasoning()) || caps.has(ModelCapabilities::coding())
        {
            TierLabel::Standard
        } else {
            TierLabel::Unknown
        },
    )
}

fn registry() -> std::sync::RwLockReadGuard<'static, BTreeMap<String, DeclaredModelMeta>> {
    DECLARED.read().unwrap_or_else(|e| e.into_inner())
}

fn registry_mut() -> std::sync::RwLockWriteGuard<'static, BTreeMap<String, DeclaredModelMeta>> {
    DECLARED.write().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn spec(id: &str) -> ModelSpec {
        ModelSpec {
            id: id.to_string(),
            display_name: None,
            context_window: None,
            max_output: None,
            cost_per_m_input: None,
            cost_per_m_output: None,
            capabilities: vec![],
        }
    }

    /// Serializes registry mutations across tests in one process
    /// (plain `cargo test` shares the process; nextest isolates anyway).
    fn with_registry<T>(specs: &[ModelSpec], f: impl FnOnce() -> T) -> T {
        replace_from_specs(specs);
        let out = f();
        clear();
        out
    }

    #[test]
    fn lookup_is_exact_id_match() {
        let mut s = spec("glm-5.3-flash");
        s.context_window = Some(198_000);
        with_registry(&[s], || {
            assert_eq!(context_window_for("glm-5.3-flash"), Some(198_000));
            // No substring matching — declared ids are exact by contract.
            assert_eq!(context_window_for("prefix-glm-5.3-flash"), None);
            assert_eq!(context_window_for("glm-5.3-flashy"), None);
            assert_eq!(context_window_for("unknown"), None);
        });
    }

    #[test]
    fn pricing_requires_both_directions() {
        let mut half = spec("half-priced");
        half.cost_per_m_input = Some(1.0);
        let mut full = spec("full-priced");
        full.cost_per_m_input = Some(0.5);
        full.cost_per_m_output = Some(2.0);
        with_registry(&[half, full], || {
            assert!(pricing_for("half-priced").is_none());
            let p = pricing_for("full-priced").expect("both declared");
            assert_eq!(p.input_price_per_mtok, 0.5);
            assert_eq!(p.output_price_per_mtok, 2.0);
            assert_eq!(p.cache_read_per_mtok, None);
            assert_eq!(p.cache_write_per_mtok, None);
        });
    }

    #[test]
    fn replace_drops_previous_registrations() {
        let mut a = spec("old-model");
        a.context_window = Some(1_000);
        let mut b = spec("new-model");
        b.max_output = Some(4_096);
        replace_from_specs(&[a]);
        with_registry(&[b], || {
            assert_eq!(lookup("old-model"), None, "stale entry must not survive");
            assert_eq!(max_output_for("new-model"), Some(4_096));
        });
    }

    #[test]
    fn clear_empties_the_registry() {
        let mut s = spec("m");
        s.cost_per_m_input = Some(1.0);
        s.cost_per_m_output = Some(1.0);
        with_registry(&[s], || {
            assert!(pricing_for("m").is_some());
            clear();
            assert!(pricing_for("m").is_none());
        });
    }

    #[test]
    fn tier_label_maps_capabilities_like_the_catalog() {
        let mut fast = spec("quick-mini");
        fast.capabilities = vec![ModelCapability::Cheap];
        let mut pro_by_id = spec("some-ultra-thing");
        pro_by_id.capabilities = vec![ModelCapability::Reasoning];
        let mut standard = spec("workhorse");
        standard.capabilities = vec![ModelCapability::Coding];
        let mut vision_only = spec("seer");
        vision_only.capabilities = vec![ModelCapability::Vision];
        let mut no_caps = spec("quiet");
        no_caps.context_window = Some(8_192);

        with_registry(&[fast, pro_by_id, standard, vision_only, no_caps], || {
            assert_eq!(tier_label_for("quick-mini"), Some(TierLabel::Fast));
            // Reasoning + "ultra" in the id → Pro (catalog heuristic order:
            // cheap/speed first, then id markers, then reasoning/coding).
            assert_eq!(tier_label_for("some-ultra-thing"), Some(TierLabel::Pro));
            assert_eq!(tier_label_for("workhorse"), Some(TierLabel::Standard));
            // Vision alone matches nothing → Unknown (still Some: declared).
            assert_eq!(tier_label_for("seer"), Some(TierLabel::Unknown));
            // No capabilities → defer to the catalog (None).
            assert_eq!(tier_label_for("quiet"), None);
            assert_eq!(tier_label_for("undeclared"), None);
        });
    }

    #[test]
    fn replace_from_config_uses_the_active_profile() {
        use shannon_types::provider_config::{
            ActiveTarget, CredentialRef, CredentialScope, ModelProfile, ProviderKind,
            ProviderProfile, ProviderTiers, Scope,
        };
        use std::collections::HashMap;

        fn profile_with_models(id: &str, models: Vec<ModelSpec>) -> ProviderProfile {
            ProviderProfile {
                id: id.to_string(),
                kind: ProviderKind::OpenAiCompatible,
                display_name: id.to_string(),
                base_url: format!("https://{id}.example.com/v1"),
                models_url: None,
                credential: CredentialRef::Env {
                    var: "K".to_string(),
                },
                extra_headers: HashMap::new(),
                default_max_tokens: None,
                fallback_models: Vec::new(),
                quirks: Default::default(),
                tiers: ProviderTiers::default(),
                models,
            }
        }

        let mut declared = spec("declared-a");
        declared.context_window = Some(65_536);
        let mut other = spec("declared-b");
        other.context_window = Some(32_768);

        let mut profiles = HashMap::new();
        profiles.insert(
            "default".to_string(),
            ModelProfile {
                name: "default".to_string(),
                active_target: ActiveTarget {
                    provider_id: "glm".to_string(),
                    model_id: "declared-a".to_string(),
                    scope: Scope::Global,
                },
                providers: vec![
                    profile_with_models("glm", vec![declared]),
                    profile_with_models("kimi", vec![other]),
                ],
                auxiliary: HashMap::new(),
                credential_scope: CredentialScope::Shared,
            },
        );
        let pm = ProviderModelConfig {
            version: ProviderModelConfig::VERSION,
            profiles,
            gateway: Default::default(),
        };

        with_registry(&[], || {
            replace_from_config(&pm);
            // Active provider's declarations are registered...
            assert_eq!(context_window_for("declared-a"), Some(65_536));
            // ...the non-active provider's are not...
            assert_eq!(context_window_for("declared-b"), None);
            // ...and an empty/absent active target clears the registry.
            clear();
            replace_from_specs(&[spec("leftover")]);
            let mut empty = pm.clone();
            empty
                .profiles
                .get_mut("default")
                .unwrap()
                .active_target
                .provider_id = "ghost".to_string();
            replace_from_config(&empty);
            assert_eq!(lookup("leftover"), None);
        });
    }

    #[test]
    fn replace_for_provider_matches_raw_id_then_canonical_slug() {
        use shannon_types::provider_config::{
            ActiveTarget, CredentialRef, CredentialScope, ModelProfile, ProviderKind,
            ProviderProfile, ProviderTiers, Scope,
        };
        use std::collections::HashMap;

        let mut declared = spec("by-raw-id");
        declared.context_window = Some(1_000);
        let mut zhipu_declared = spec("by-canonical-slug");
        zhipu_declared.context_window = Some(2_000);

        let mk = |id: &str, models: Vec<ModelSpec>| ProviderProfile {
            id: id.to_string(),
            kind: ProviderKind::OpenAiCompatible,
            display_name: id.to_string(),
            base_url: "https://x.example/v1".to_string(),
            models_url: None,
            credential: CredentialRef::Env {
                var: "K".to_string(),
            },
            extra_headers: HashMap::new(),
            default_max_tokens: None,
            fallback_models: Vec::new(),
            quirks: Default::default(),
            tiers: ProviderTiers::default(),
            models,
        };
        let mut profiles = HashMap::new();
        profiles.insert(
            "default".to_string(),
            ModelProfile {
                name: "default".to_string(),
                active_target: ActiveTarget {
                    provider_id: "glm".to_string(),
                    model_id: "m".to_string(),
                    scope: Scope::Global,
                },
                providers: vec![mk("glm", vec![declared]), mk("zhipu", vec![zhipu_declared])],
                auxiliary: HashMap::new(),
                credential_scope: CredentialScope::Shared,
            },
        );
        let pm = ProviderModelConfig {
            version: ProviderModelConfig::VERSION,
            profiles,
            gateway: Default::default(),
        };

        with_registry(&[], || {
            // Exact raw id wins.
            replace_for_provider_in("glm", &pm);
            assert_eq!(context_window_for("by-raw-id"), Some(1_000));
            assert_eq!(lookup("by-canonical-slug"), None);
            // Alias slug canonicalizes to a slot whose id resolves the same.
            replace_for_provider_in("zhipu", &pm);
            assert_eq!(context_window_for("by-canonical-slug"), Some(2_000));
            assert_eq!(lookup("by-raw-id"), None);
            // Unknown slug clears.
            replace_for_provider_in("ghost", &pm);
            assert_eq!(lookup("by-canonical-slug"), None);
        });
    }
}
