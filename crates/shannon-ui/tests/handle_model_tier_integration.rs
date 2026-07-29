//! Integration test: `/model --tier <name> [provider] [--save]` resolves user
//! input to canonical tier names, and `--save` persists the override under
//! the canonical key (never the alias). Lives in `shannon-ui/tests/` so it
//! can pull from the public `shannon-ui` API surface + `shannon-types`.

use shannon_types::provider_config::TierName;

/// Anthropic-specific aliases (`haiku` / `sonnet` / `opus`) and provider-
/// native names (`flash` / `mini` / `plus` / `ultra` / `max` / `large`) all
/// resolve to the same three canonical tiers. This is the contract the
/// `/model --tier <name>` parser relies on (see `TierName::from_user_input`).
#[test]
fn tier_name_from_user_input_resolves_anthropic_aliases() {
    assert_eq!(TierName::from_user_input("haiku"), Some(TierName::Fast));
    assert_eq!(TierName::from_user_input("sonnet"), Some(TierName::Standard));
    assert_eq!(TierName::from_user_input("opus"), Some(TierName::Pro));

    // Provider-native aliases from non-Anthropic providers also collapse to
    // the same canonical buckets (decision: tier names are about *role*, not
    // *provider brand*).
    assert_eq!(TierName::from_user_input("flash"), Some(TierName::Fast));
    assert_eq!(TierName::from_user_input("mini"), Some(TierName::Fast));
    assert_eq!(TierName::from_user_input("nano"), Some(TierName::Fast));
    assert_eq!(TierName::from_user_input("plus"), Some(TierName::Standard));
    assert_eq!(TierName::from_user_input("medium"), Some(TierName::Standard));
    assert_eq!(TierName::from_user_input("turbo"), Some(TierName::Standard));
    assert_eq!(TierName::from_user_input("ultra"), Some(TierName::Pro));
    assert_eq!(TierName::from_user_input("max"), Some(TierName::Pro));
    assert_eq!(TierName::from_user_input("large"), Some(TierName::Pro));
}

/// After `from_user_input` normalizes an alias, `canonical()` returns the
/// stable storage key — the key `persist_model_to_providers_toml` writes
/// under, and the key the engine layer reads back. **Aliases must never
/// appear in `providers.toml`** (decision: storage is canonical, input is
/// permissive).
#[test]
fn tier_name_persistence_uses_canonical_form() {
    // "haiku" → TierName::Fast → canonical "fast"
    let tier = TierName::from_user_input("haiku").unwrap();
    assert_eq!(tier, TierName::Fast);
    assert_eq!(tier.canonical(), "fast");
    assert_ne!(tier.canonical(), "haiku");

    // "sonnet" → TierName::Standard → canonical "standard"
    let tier = TierName::from_user_input("sonnet").unwrap();
    assert_eq!(tier, TierName::Standard);
    assert_eq!(tier.canonical(), "standard");
    assert_ne!(tier.canonical(), "sonnet");

    // "opus" → TierName::Pro → canonical "pro"
    let tier = TierName::from_user_input("opus").unwrap();
    assert_eq!(tier, TierName::Pro);
    assert_eq!(tier.canonical(), "pro");
    assert_ne!(tier.canonical(), "opus");

    // Case insensitivity: "Haiku" / "oPuS" still normalize to canonical.
    let tier = TierName::from_user_input("Haiku").unwrap();
    assert_eq!(tier.canonical(), "fast");
    let tier = TierName::from_user_input("oPuS").unwrap();
    assert_eq!(tier.canonical(), "pro");

    // Invalid input must return None so the REPL can show suggestions.
    assert_eq!(TierName::from_user_input(""), None);
    assert_eq!(TierName::from_user_input("xyz"), None);
    assert_eq!(TierName::from_user_input("turbo-xl"), None);
}

// =============================================================================
// End-to-end /model --tier flow tests
// =============================================================================
//
// These tests exercise the *full* `/model --tier <alias> [provider]` pipeline:
// user input → `TierName::from_user_input` (alias normalization) →
// `resolve_tier(alias, provider, &tiers)` (model resolution against a real
// `ProviderTiers` config). The alias-only tests above only check
// `from_user_input`; these verify the end-to-end contract the REPL actually
// depends on.

#[test]
fn full_flow_haiku_alias_resolves_to_anthropic_fast() {
    use shannon_core::model_registry::resolve_tier;
    use shannon_engine::api::LlmProvider;
    use shannon_types::provider_config::ProviderTiers;

    // Step 1: alias normalization (REPL parser path).
    let tier = TierName::from_user_input("haiku").expect("haiku is valid alias");
    assert_eq!(tier, TierName::Fast);

    // Step 2: end-to-end resolution — alias + provider → concrete model id.
    let tiers = ProviderTiers::default();
    let resolved = resolve_tier("haiku", &LlmProvider::Anthropic, &tiers);
    assert!(
        resolved.is_some(),
        "haiku alias should resolve to claude-haiku-4-5 for anthropic"
    );
    let resolved = resolved.unwrap();
    assert!(resolved.contains("haiku"), "got: {}", resolved);
}

#[test]
fn full_flow_flash_alias_resolves_to_gemini_fast() {
    use shannon_core::model_registry::resolve_tier;
    use shannon_engine::api::LlmProvider;
    use shannon_types::provider_config::ProviderTiers;

    // Gemini's provider-native `flash` alias resolves to a Gemini *Fast*
    // tier model id under the unified tier system.
    let tiers = ProviderTiers::default();
    let resolved = resolve_tier("flash", &LlmProvider::Gemini, &tiers);
    assert!(
        resolved.is_some(),
        "flash alias should resolve to a Gemini fast model"
    );
    let id = resolved.unwrap();
    assert!(id.contains("flash"), "got: {}", id);
}

#[test]
fn full_flow_unknown_tier_input_suggests_canonical_names() {
    use shannon_types::provider_config::TierName;

    // Unknown alias → None (REPL prints suggestions in this branch).
    let result = TierName::from_user_input("turbo-xl");
    assert!(result.is_none());

    // The error message in handle_model would include suggestions().
    let suggestions = TierName::suggestions();
    assert!(suggestions.contains(&"fast"));
    assert!(suggestions.contains(&"haiku"));
}
