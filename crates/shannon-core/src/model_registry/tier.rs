//! Tier + alias resolution and the lightweight model router — split out of the
//! parent registry (ADR-0008 P2-8, the "tier 解析" half).
//!
//! Alias → model-id resolution ([`resolve_model_alias`] / [`resolve_model`]),
//! canonical-tier → model-id resolution backed by catalog capabilities +
//! persisted overrides ([`resolve_tier`] / [`resolve_auto_tier`]), and the
//! task-type [`ModelRouter`]. Everything public here is re-exported by the
//! parent, so `model_registry::resolve_tier` / `::ModelRouter` / etc. continue
//! to resolve for every caller.

use super::{MODEL_CATALOG, ModelCapabilities, ModelInfo, model_info_for};
use serde::{Deserialize, Serialize};
use shannon_engine::api::LlmProvider;
use shannon_types::provider_config::{ProviderTiers, TierName};

// ============================================================================
// Model Aliases
// ============================================================================

/// Tier names that resolve to the best matching model per provider.
const TIER_OPUS: &[&str] = &["opus"];
const TIER_SONNET: &[&str] = &["sonnet"];
const TIER_HAIKU: &[&str] = &["haiku", "fast", "mini"];

/// Resolve a model alias (tier name) to an actual model ID.
///
/// Recognized aliases:
/// - `"opus"` → most capable reasoning model for the given provider
/// - `"sonnet"` → mid-tier coding model for the given provider
/// - `"haiku"`, `"fast"`, `"mini"` → cheapest/fastest model for the given provider
///
/// If `alias` is not a recognized alias, returns `None` (caller should use it as-is).
/// If `provider` is `None`, returns the best match across all providers.
pub fn resolve_model_alias(alias: &str, provider: Option<&LlmProvider>) -> Option<&'static str> {
    let tier = if TIER_OPUS.contains(&alias) {
        ModelTier::Opus
    } else if TIER_SONNET.contains(&alias) {
        ModelTier::Sonnet
    } else if TIER_HAIKU.contains(&alias) {
        ModelTier::Haiku
    } else {
        return None;
    };

    let candidates: Vec<&ModelInfo> = MODEL_CATALOG
        .iter()
        .filter(|m| match provider {
            Some(p) => m.provider == *p,
            None => true,
        })
        .filter(|m| m.capabilities.has(tier.required_capability()))
        .collect();

    if candidates.is_empty() {
        return None;
    }

    Some(tier.select(&candidates).id)
}

/// Resolve a model string that might be an alias or a literal model ID.
///
/// If the string is a recognized alias, resolves it. Otherwise returns it as-is.
pub fn resolve_model(model: &str, provider: Option<&LlmProvider>) -> String {
    resolve_model_alias(model, provider)
        .map(|s| s.to_string())
        .unwrap_or_else(|| model.to_string())
}

/// Model tier for alias resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModelTier {
    Opus,
    Sonnet,
    Haiku,
}

impl ModelTier {
    fn required_capability(self) -> ModelCapabilities {
        match self {
            Self::Opus => ModelCapabilities::reasoning(),
            Self::Sonnet => ModelCapabilities::coding(),
            Self::Haiku => ModelCapabilities::cheap().or(ModelCapabilities::speed()),
        }
    }

    fn select<'a>(self, candidates: &[&'a ModelInfo]) -> &'a ModelInfo {
        if candidates.is_empty() {
            tracing::warn!(
                "ModelTier::select called with no candidates; falling back to first catalog entry"
            );
            return &MODEL_CATALOG[0];
        }
        match self {
            // Opus: pick most expensive (most capable)
            Self::Opus => candidates
                .iter()
                .max_by(|a, b| {
                    (a.cost_per_m_input + a.cost_per_m_output)
                        .partial_cmp(&(b.cost_per_m_input + b.cost_per_m_output))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map_or(&MODEL_CATALOG[0], |v| *v),
            // Sonnet: pick mid-range cost
            Self::Sonnet => {
                let mut sorted: Vec<&ModelInfo> = candidates.to_vec();
                sorted.sort_by(|a, b| {
                    (a.cost_per_m_input + a.cost_per_m_output)
                        .partial_cmp(&(b.cost_per_m_input + b.cost_per_m_output))
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                let idx = (sorted.len() - 1) / 2;
                sorted[idx]
            }
            // Haiku: pick cheapest
            Self::Haiku => candidates
                .iter()
                .min_by(|a, b| {
                    (a.cost_per_m_input + a.cost_per_m_output)
                        .partial_cmp(&(b.cost_per_m_input + b.cost_per_m_output))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map_or(&MODEL_CATALOG[0], |v| *v),
        }
    }
}

/// Return all recognized alias names (for tab completion).
pub fn model_aliases() -> &'static [&'static str] {
    TierName::suggestions()
}

/// Check if a string is a recognized model alias.
pub fn is_model_alias(s: &str) -> bool {
    resolve_model_alias(s, None).is_some()
}

fn resolve_model_id_alias(model_id: &'static str) -> &'static str {
    match model_id {
        "claude-haiku-4-5-20251001" => "claude-haiku-4-5",
        "claude-opus-4-20250115" => "claude-opus-4",
        _ => model_id,
    }
}

/// Resolve a tier (canonical or alias) to a concrete model id for a provider.
///
/// Resolution order:
///   1. User-configured `profile_tiers.<canonical>` override (from providers.toml)
///   2. Catalog match using `ModelCapabilities` (Fast ⇒ SPEED|CHEAP, etc.)
///   3. Internal `ModelTier` enum fallback (Opus/Sonnet/Haiku)
///   4. None (caller should display "tier not available for this provider")
///
/// `tier_input` accepts both canonical names ("fast") and aliases ("haiku", "flash", etc.).
/// Returns None for unrecognized input or for the reserved `Auto` tier.
pub fn resolve_tier(
    tier_input: &str,
    provider: &LlmProvider,
    profile_tiers: &ProviderTiers,
) -> Option<String> {
    let tier = TierName::from_user_input(tier_input)?;
    if matches!(tier, TierName::Auto) {
        return None;
    }

    // 1. Explicit user override
    let explicit = match tier {
        TierName::Fast => &profile_tiers.fast,
        TierName::Standard => &profile_tiers.standard,
        TierName::Pro => &profile_tiers.pro,
        TierName::Auto => return None,
    };
    if let Some(id) = explicit {
        return Some(id.clone());
    }

    // 2. Catalog-based inference using ModelCapabilities
    let wanted = match tier {
        TierName::Fast => ModelCapabilities::speed().or(ModelCapabilities::cheap()),
        TierName::Standard => ModelCapabilities::coding(),
        TierName::Pro => ModelCapabilities::reasoning(),
        TierName::Auto => return None,
    };
    if let Some(model) = MODEL_CATALOG
        .iter()
        .filter(|model| &model.provider == provider)
        .filter(|model| model.capabilities.has(wanted))
        .reduce(|selected, candidate| match tier {
            TierName::Pro => {
                let selected_cost = selected.cost_per_m_input + selected.cost_per_m_output;
                let candidate_cost = candidate.cost_per_m_input + candidate.cost_per_m_output;
                if candidate_cost > selected_cost {
                    candidate
                } else {
                    selected
                }
            }
            TierName::Fast | TierName::Standard => {
                let selected_cost = selected.cost_per_m_input + selected.cost_per_m_output;
                let candidate_cost = candidate.cost_per_m_input + candidate.cost_per_m_output;
                if candidate_cost < selected_cost {
                    candidate
                } else {
                    selected
                }
            }
            TierName::Auto => selected,
        })
    {
        return Some(resolve_model_id_alias(model.id).to_string());
    }

    // 3. Internal ModelTier enum fallback
    let model_tier = match tier {
        TierName::Fast => ModelTier::Haiku,
        TierName::Standard => ModelTier::Sonnet,
        TierName::Pro => ModelTier::Opus,
        TierName::Auto => return None,
    };
    let alias = match model_tier {
        ModelTier::Haiku => "haiku",
        ModelTier::Sonnet => "sonnet",
        ModelTier::Opus => "opus",
    };
    resolve_model_alias(alias, Some(provider)).map(str::to_string)
}

/// Resolve the `Auto` tier to a concrete `(tier, model_id)` for a provider.
///
/// This is the **lightweight heuristic** agreed for `/model --tier auto`
/// (ADR-0005 decision ②) — deliberately *not* the full task-type
/// [`ModelRouter`] (spec §11 keeps that unwired). The rule is simple and
/// deterministic: pick the best-cost/capability **default** the provider
/// actually offers, trying tiers in preference order **standard → pro → fast**
/// and returning the first that resolves:
///
/// - **Standard** is the workhorse default (best cost/capability for coding).
/// - Escalate to **pro** only when the provider has no standard-tier model
///   (a provider without a standard tier usually wants its flagship as the
///   default).
/// - Fall back to **fast** as the last resort.
///
/// Each candidate is resolved through [`resolve_tier`], so persisted
/// `providers.toml` overrides and catalog inference both apply. `Auto` itself
/// is never persisted (only the resolved concrete tier is), per the tier-naming
/// rule.
pub fn resolve_auto_tier(
    provider: &LlmProvider,
    profile_tiers: &ProviderTiers,
) -> Option<(TierName, String)> {
    for tier in [TierName::Standard, TierName::Pro, TierName::Fast] {
        if let Some(id) = resolve_tier(tier.canonical(), provider, profile_tiers) {
            return Some((tier, id));
        }
    }
    None
}

// ============================================================================
// Model Router
// ============================================================================

/// Effort level controlling reasoning depth and token budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffortLevel {
    Low,
    Medium,
    High,
}

impl Default for EffortLevel {
    fn default() -> Self {
        Self::Medium
    }
}

impl EffortLevel {
    /// Parse from string (case-insensitive). Returns None for unrecognized values.
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "low" => Some(Self::Low),
            "medium" | "med" => Some(Self::Medium),
            "high" | "max" => Some(Self::High),
            _ => None,
        }
    }

    /// Suggested `thinking_budget` (extended thinking tokens) for this effort level.
    pub fn thinking_budget(self) -> Option<usize> {
        match self {
            Self::Low => None,
            Self::Medium => Some(10_000),
            Self::High => Some(32_000),
        }
    }

    /// Suggested `max_tokens` multiplier relative to the model's default.
    pub fn max_tokens_factor(self) -> f64 {
        match self {
            Self::Low => 0.5,
            Self::Medium => 1.0,
            Self::High => 1.5,
        }
    }
}

/// Task type hint for model routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskType {
    /// Simple question, quick lookup — prefer cheap/fast models
    QuickQuery,
    /// Code generation, editing, debugging — prefer coding models
    CodeGeneration,
    /// Architecture design, complex reasoning — prefer reasoning models
    ArchitectureDesign,
    /// Multi-step workflow — prefer coding + reasoning
    ComplexWorkflow,
}

/// Recommends a model based on task type and preferences.
pub struct ModelRouter;

impl ModelRouter {
    /// Recommend the best model ID for a given task type.
    ///
    /// Falls back to the first model in the catalog if no match is found.
    pub fn recommend(task: TaskType) -> &'static str {
        let required = match task {
            TaskType::QuickQuery => ModelCapabilities::cheap(),
            TaskType::CodeGeneration => ModelCapabilities::coding(),
            TaskType::ArchitectureDesign => ModelCapabilities::reasoning(),
            TaskType::ComplexWorkflow => {
                ModelCapabilities::coding().or(ModelCapabilities::reasoning())
            }
        };

        // Find cheapest model that has the required capabilities
        let mut best: Option<&'static ModelInfo> = None;
        let mut best_cost = f64::MAX;

        for model in MODEL_CATALOG {
            if model.capabilities.has(required) {
                let cost = model.cost_per_m_input + model.cost_per_m_output;
                if cost < best_cost {
                    best_cost = cost;
                    best = Some(model);
                }
            }
        }

        match best {
            Some(m) => m.id,
            None => MODEL_CATALOG[0].id,
        }
    }

    /// Recommend a model for the given task, with a preference for speed.
    pub fn recommend_fast(task: TaskType) -> &'static str {
        let required = match task {
            TaskType::QuickQuery => ModelCapabilities::cheap().or(ModelCapabilities::speed()),
            TaskType::CodeGeneration => ModelCapabilities::coding().or(ModelCapabilities::speed()),
            TaskType::ArchitectureDesign => ModelCapabilities::reasoning(),
            TaskType::ComplexWorkflow => {
                ModelCapabilities::coding().or(ModelCapabilities::reasoning())
            }
        };

        for model in MODEL_CATALOG {
            if model.capabilities.has(required)
                && model.capabilities.has(ModelCapabilities::speed())
            {
                return model.id;
            }
        }

        Self::recommend(task)
    }

    /// Estimate cost for a request with the given model and token counts.
    pub fn estimate_cost(model_id: &str, input_tokens: usize, output_tokens: usize) -> f64 {
        if let Some(info) = model_info_for(model_id) {
            let input_cost = (input_tokens as f64 / 1_000_000.0) * info.cost_per_m_input;
            let output_cost = (output_tokens as f64 / 1_000_000.0) * info.cost_per_m_output;
            input_cost + output_cost
        } else {
            0.0
        }
    }
}
