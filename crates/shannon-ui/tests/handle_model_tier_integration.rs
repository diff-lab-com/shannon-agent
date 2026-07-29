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