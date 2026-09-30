//! R3-3 — Plan/Act dual-tier model preference (roadmap 2026-09-30, item R3-3).
//!
//! Cline-style plan/act models mapped onto Shannon's tier system: the user
//! pins a model **tier** (`fast` | `standard` | `pro`) per chat phase —
//! planning vs execution — and each phase may also be `inherit`, meaning
//! "no phase override; use the global default".
//!
//! Storage is the global desktop config (`DesktopConfig.plan_tier` /
//! `DesktopConfig.act_tier`), NOT per session — a phase tier is a routing
//! posture, not a conversation property.
//!
//! Precedence (roadmap R3-3, explicit):
//!
//! ```text
//! session model override (R2-1)  >  phase tier  >  global default
//! ```
//!
//! The phase is derived from the session's approval mode, which the desktop
//! already tracks as the global `approval_mode` config key (`plan` when the
//! composer's plan mode is active): when `approval_mode == "plan"` the
//! **plan** tier applies, otherwise the **act** tier does. An `inherit`
//! tier (or an unset/invalid value, which normalizes to `inherit`) means
//! the phase contributes no override.
//!
//! Tier → concrete-model resolution delegates to
//! `shannon_core::model_registry::resolve_tier` — the same resolution the
//! TUI's `/model --tier` uses — so a user-configured `providers.toml`
//! `[tiers]` override wins first, then catalog-capability inference with the
//! documented tie-break (cheapest candidate for fast/standard, most capable
//! = priciest for pro). The pure functions here only decide WHICH tier (if
//! any) applies to a query; the concrete model never comes from this module.

/// The wire/config value meaning "no phase override — inherit the global
/// default". This is the default dropdown value in both the chat-header
/// control and Settings → Models.
pub const TIER_INHERIT: &str = "inherit";

/// Canonical tier names accepted as phase-tier preference values.
pub const TIER_VALUES: [&str; 3] = ["fast", "standard", "pro"];

/// A resolved phase-tier preference. `inherit` is intentionally not a
/// variant here — the Option-shaped APIs below treat it (and any junk
/// value) as "no override".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseTier {
    Fast,
    Standard,
    Pro,
}

impl PhaseTier {
    /// Canonical lowercase name — the same vocabulary `resolve_tier` and
    /// the `ProviderTiers` fields speak.
    pub fn as_str(self) -> &'static str {
        match self {
            PhaseTier::Fast => "fast",
            PhaseTier::Standard => "standard",
            PhaseTier::Pro => "pro",
        }
    }

    /// Parse a canonical tier name. Aliases (`haiku`, `opus`, …) are
    /// deliberately NOT accepted here: the phase preference is written by
    /// the desktop UI's dropdown, which only offers canonical names, and
    /// keeping the parser strict keeps `configure('plan_tier')` honest.
    pub fn from_pref(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "fast" => Some(PhaseTier::Fast),
            "standard" => Some(PhaseTier::Standard),
            "pro" => Some(PhaseTier::Pro),
            _ => None,
        }
    }
}

/// Normalize a persisted phase-tier preference to the canonical wire value:
/// `None` for inherit/unset/legacy junk (older configs, hand-edited JSON),
/// `Some(canonical)` for a real tier. The configure write path rejects
/// invalid values upfront; this lenient reader exists so a bad value can
/// never wedge query routing — it degrades to `inherit`.
pub fn normalize_tier_pref(raw: Option<&str>) -> Option<String> {
    raw.and_then(PhaseTier::from_pref)
        .map(|t| t.as_str().to_string())
}

/// Validate a `configure('plan_tier' | 'act_tier')` value.
///
/// `""` and `inherit` clear the preference (`Ok(None)`); a canonical tier
/// name stores canonically (`Ok(Some(..))`); anything else is a hard error
/// so a typo can never silently disable the feature.
pub fn validate_tier_pref_value(value: &str) -> Result<Option<String>, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case(TIER_INHERIT) {
        return Ok(None);
    }
    PhaseTier::from_pref(trimmed)
        .map(|t| Some(t.as_str().to_string()))
        .ok_or_else(|| {
            format!(
                "invalid phase tier `{trimmed}` — expected one of: {TIER_INHERIT}, {}",
                TIER_VALUES.join(", ")
            )
        })
}

/// Which phase tier applies to a query, if any.
///
/// `approval_mode` is the desktop config's global approval mode; the chat
/// composer's plan mode writes `"plan"` into it (and back to `"suggest"` on
/// exit), so `plan` is the plan-phase signal. Anything else (suggest /
/// readonly / auto / full_auto / confirm / unset) counts as the act phase.
///
/// Returns `None` when the applicable phase is `inherit` (or unset/invalid,
/// which normalizes to inherit) — the caller keeps the global default.
pub fn effective_phase_tier(
    approval_mode: Option<&str>,
    plan_tier: Option<&str>,
    act_tier: Option<&str>,
) -> Option<PhaseTier> {
    let in_plan_phase = matches!(approval_mode.map(str::trim), Some("plan"));
    let pref = if in_plan_phase { plan_tier } else { act_tier };
    normalize_tier_pref(pref).and_then(|t| PhaseTier::from_pref(&t))
}

/// Resolve a phase tier to a concrete model id for `provider` via the
/// engine's tier resolution (`shannon_core::model_registry::resolve_tier` —
/// the exact resolution the TUI `/model --tier` command performs):
///
/// 1. the profile's persisted `tiers.<canonical>` override (providers.toml),
/// 2. catalog-capability inference (Fast ⇒ speed|cheap, Standard ⇒ coding,
///    Pro ⇒ reasoning) with the documented cost tie-break (cheapest for
///    fast/standard, priciest for pro),
/// 3. the internal alias fallback.
///
/// `None` = the tier does not resolve for this provider (no catalog match,
/// no override) — the caller keeps the global default model rather than
/// failing the send.
pub fn resolve_tier_model(
    tier: PhaseTier,
    provider: shannon_engine::api::LlmProvider,
    profile_tiers: &shannon_types::provider_config::ProviderTiers,
) -> Option<String> {
    shannon_core::model_registry::resolve_tier(tier.as_str(), &provider, profile_tiers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_pref_accepts_canonical_names_only() {
        assert_eq!(PhaseTier::from_pref("fast"), Some(PhaseTier::Fast));
        assert_eq!(
            PhaseTier::from_pref(" Standard "),
            Some(PhaseTier::Standard)
        );
        assert_eq!(PhaseTier::from_pref("PRO"), Some(PhaseTier::Pro));
        // TUI aliases must NOT round-trip through the desktop preference.
        assert_eq!(PhaseTier::from_pref("haiku"), None);
        assert_eq!(PhaseTier::from_pref("opus"), None);
        assert_eq!(PhaseTier::from_pref(""), None);
        assert_eq!(PhaseTier::from_pref("auto"), None);
    }

    #[test]
    fn normalize_tier_pref_degrades_junk_to_inherit() {
        assert_eq!(normalize_tier_pref(None), None);
        assert_eq!(normalize_tier_pref(Some("inherit")), None);
        assert_eq!(normalize_tier_pref(Some("inherit")), None);
        assert_eq!(normalize_tier_pref(Some("fast")), Some("fast".into()));
        // Legacy junk (hand-edited config.json) never wedges routing.
        assert_eq!(normalize_tier_pref(Some("ultra")), None);
    }

    #[test]
    fn validate_tier_pref_value_round_trips() {
        assert_eq!(validate_tier_pref_value(""), Ok(None));
        assert_eq!(validate_tier_pref_value("inherit"), Ok(None));
        assert_eq!(validate_tier_pref_value("  fast "), Ok(Some("fast".into())));
        assert_eq!(
            validate_tier_pref_value("standard"),
            Ok(Some("standard".into()))
        );
        assert!(validate_tier_pref_value("ultra").is_err());
        assert!(validate_tier_pref_value("haiku").is_err());
    }

    #[test]
    fn effective_phase_tier_follows_approval_mode() {
        // Plan phase (approval_mode == "plan") uses the plan tier.
        assert_eq!(
            effective_phase_tier(Some("plan"), Some("pro"), Some("fast")),
            Some(PhaseTier::Pro)
        );
        // Any non-plan mode uses the act tier.
        for mode in ["suggest", "readonly", "auto", "full_auto", "confirm"] {
            assert_eq!(
                effective_phase_tier(Some(mode), Some("pro"), Some("fast")),
                Some(PhaseTier::Fast),
                "mode `{mode}` should resolve through the act tier"
            );
        }
        // Unset mode = act phase.
        assert_eq!(
            effective_phase_tier(None, Some("pro"), Some("standard")),
            Some(PhaseTier::Standard)
        );
    }

    #[test]
    fn effective_phase_tier_inherits_by_default_and_per_phase() {
        // Both inherit → no override at all.
        assert_eq!(effective_phase_tier(Some("plan"), None, None), None);
        assert_eq!(effective_phase_tier(Some("suggest"), None, None), None);
        // Explicit inherit strings behave like unset.
        assert_eq!(
            effective_phase_tier(Some("plan"), Some("inherit"), Some("fast")),
            None
        );
        assert_eq!(
            effective_phase_tier(Some("plan"), Some("pro"), Some("inherit")),
            Some(PhaseTier::Pro)
        );
        // Junk values degrade to inherit rather than erroring a send.
        assert_eq!(
            effective_phase_tier(Some("plan"), Some("ultra"), None),
            None
        );
    }

    #[test]
    fn resolve_tier_model_uses_engine_resolution() {
        use shannon_types::provider_config::ProviderTiers;
        // Explicit per-tier override always wins (same contract as the TUI
        // `/model --tier --save` write-back).
        let tiers = ProviderTiers {
            fast: Some("my-custom-fast-model".into()),
            standard: None,
            pro: None,
        };
        assert_eq!(
            resolve_tier_model(
                PhaseTier::Fast,
                shannon_engine::api::LlmProvider::Anthropic,
                &tiers
            ),
            Some("my-custom-fast-model".into())
        );
        // Without an override the catalog classifies: Anthropic's fast tier
        // resolves to a haiku-family model.
        let empty = ProviderTiers::default();
        let fast = resolve_tier_model(
            PhaseTier::Fast,
            shannon_engine::api::LlmProvider::Anthropic,
            &empty,
        )
        .expect("anthropic has a fast-tier catalog entry");
        assert!(
            fast.contains("haiku"),
            "fast tier resolves into the haiku family, got {fast}"
        );
    }

    #[test]
    fn resolve_tier_model_can_return_none_for_unknown_provider_catalog() {
        use shannon_engine::api::LlmProvider;
        // Ollama has no static catalog entries — no crash, just "no override".
        let empty = shannon_types::provider_config::ProviderTiers::default();
        assert_eq!(
            resolve_tier_model(PhaseTier::Pro, LlmProvider::Ollama, &empty),
            None
        );
    }
}
