//! Φ1a — integration tests for the C1 v1→v2 provider/model migrator
//! (`shannon_types::migration::migrate_v1`).
//!
//! Covers the migration contract end to end: default-filling for known
//! providers, explicit-field passthrough, decision **A1** (plaintext
//! `api_key` is never carried into v2), `api_key_env` precedence,
//! multi-provider maps, OpenAI-compatible underspecification errors,
//! top-level/entry overlay, v2 structural validity (serde round-trip), and a
//! TOML end-to-end case.

#![allow(clippy::unwrap_used)]

use shannon_types::migration::{LegacyV1Config, LegacyV1ProviderEntry, MigrationError, migrate_v1};
use shannon_types::provider_config::{
    CredentialRef, CredentialScope, ModelProfile, ProviderKind, ProviderModelConfig,
    ProviderProfile, Scope,
};
use std::collections::HashMap;

/// The single `"default"` profile every Φ1a migration emits.
fn default_profile(cfg: &ProviderModelConfig) -> &ModelProfile {
    cfg.profiles
        .get("default")
        .expect("Φ1a always emits one profile named \"default\"")
}

/// The `ProviderProfile` whose id matches the active target.
fn active_provider(cfg: &ProviderModelConfig) -> &ProviderProfile {
    let p = default_profile(cfg);
    p.providers
        .iter()
        .find(|pp| pp.id == p.active_target.provider_id)
        .expect("active target points at a real provider profile")
}

fn anthropic_legacy() -> LegacyV1Config {
    LegacyV1Config {
        provider: Some("anthropic".to_string()),
        ..Default::default()
    }
}

#[test]
fn empty_config_is_no_provider_error() {
    assert_eq!(
        migrate_v1(&LegacyV1Config::default()),
        Err(MigrationError::NoProvider)
    );
}

#[test]
fn top_level_known_provider_fills_defaults() {
    let cfg = migrate_v1(&anthropic_legacy()).unwrap();

    assert_eq!(cfg.version, ProviderModelConfig::VERSION);
    assert!(!cfg.gateway.multiplex_profiles);
    assert!(cfg.gateway.profile_routes.is_empty());

    let p = default_profile(&cfg);
    assert_eq!(p.name, "default");
    assert_eq!(p.active_target.provider_id, "anthropic");
    assert_eq!(p.active_target.model_id, "claude-sonnet-4-20250514");
    assert_eq!(p.active_target.scope, Scope::Global);
    assert_eq!(p.credential_scope, CredentialScope::Shared);
    assert_eq!(p.providers.len(), 1);

    let active = active_provider(&cfg);
    assert_eq!(active.id, "anthropic");
    assert_eq!(active.kind, ProviderKind::Anthropic);
    assert_eq!(active.base_url, "https://api.anthropic.com");
    assert_eq!(active.models_url, None);
    assert_eq!(active.default_max_tokens, None);
    assert!(active.fallback_models.is_empty());
    assert_eq!(
        active.credential,
        CredentialRef::Env {
            var: "SHANNON_ANTHROPIC_API_KEY".to_string()
        }
    );
}

#[test]
fn explicit_fields_pass_through() {
    let legacy = LegacyV1Config {
        provider: Some("openai".to_string()),
        base_url: Some("https://proxy.example.com/v1".to_string()),
        model: Some("gpt-4o-mini".to_string()),
        max_tokens: Some(2048),
        ..Default::default()
    };
    let cfg = migrate_v1(&legacy).unwrap();
    let active = active_provider(&cfg);

    assert_eq!(active.base_url, "https://proxy.example.com/v1");
    assert_eq!(
        cfg.profiles["default"].active_target.model_id,
        "gpt-4o-mini"
    );
    assert_eq!(active.default_max_tokens, Some(2048));
}

#[test]
fn plaintext_api_key_is_never_carried_into_v2() {
    // Decision A1: the legacy plaintext key must not appear anywhere in the
    // migrated v2 config; the credential becomes an Env reference only.
    let legacy = LegacyV1Config {
        provider: Some("anthropic".to_string()),
        api_key: Some("sk-SECRET-12345".to_string()),
        ..Default::default()
    };
    let cfg = migrate_v1(&legacy).unwrap();

    let json = serde_json::to_string(&cfg).unwrap();
    assert!(
        !json.contains("sk-SECRET-12345"),
        "plaintext api_key leaked into v2 config: {json}"
    );

    let active = active_provider(&cfg);
    assert!(
        matches!(active.credential, CredentialRef::Env { .. }),
        "credential must be the Env backend, not InlineLegacy"
    );
    assert_eq!(
        active.credential,
        CredentialRef::Env {
            var: "SHANNON_ANTHROPIC_API_KEY".to_string()
        }
    );
}

#[test]
fn explicit_api_key_env_is_respected() {
    let mut providers = HashMap::new();
    providers.insert(
        "openai".to_string(),
        LegacyV1ProviderEntry {
            api_key_env: Some("MY_OPENAI_KEY".to_string()),
            ..Default::default()
        },
    );
    let cfg = migrate_v1(&LegacyV1Config {
        providers,
        ..Default::default()
    })
    .unwrap();

    let active = active_provider(&cfg);
    assert_eq!(
        active.credential,
        CredentialRef::Env {
            var: "MY_OPENAI_KEY".to_string()
        }
    );
}

#[test]
fn multi_provider_map_picks_smallest_as_active() {
    // No top-level provider → active = lexicographically smallest key
    // (deterministic despite HashMap's unordered iteration).
    let mut providers = HashMap::new();
    providers.insert("openai".to_string(), LegacyV1ProviderEntry::default());
    providers.insert("anthropic".to_string(), LegacyV1ProviderEntry::default());
    let cfg = migrate_v1(&LegacyV1Config {
        providers,
        ..Default::default()
    })
    .unwrap();

    let p = default_profile(&cfg);
    assert_eq!(p.active_target.provider_id, "anthropic");
    assert_eq!(p.providers.len(), 2);
    let ids: Vec<&str> = p.providers.iter().map(|pp| pp.id.as_str()).collect();
    assert!(ids.contains(&"anthropic"));
    assert!(ids.contains(&"openai"));
}

#[test]
fn openai_compatible_requires_base_url() {
    let mut providers = HashMap::new();
    providers.insert(
        "custom".to_string(),
        LegacyV1ProviderEntry {
            model: Some("m".to_string()), // model present, base_url absent
            ..Default::default()
        },
    );
    assert_eq!(
        migrate_v1(&LegacyV1Config {
            providers,
            ..Default::default()
        }),
        Err(MigrationError::OpenAiCompatibleNeedsBaseUrl {
            provider: "custom".to_string()
        })
    );
}

#[test]
fn openai_compatible_requires_model() {
    let mut providers = HashMap::new();
    providers.insert(
        "custom".to_string(),
        LegacyV1ProviderEntry {
            base_url: Some("https://api.custom.example.com".to_string()),
            ..Default::default() // base_url present, model absent
        },
    );
    assert_eq!(
        migrate_v1(&LegacyV1Config {
            providers,
            ..Default::default()
        }),
        Err(MigrationError::OpenAiCompatibleNeedsModel {
            provider: "custom".to_string()
        })
    );
}

#[test]
fn openai_compatible_with_explicit_fields_migrates() {
    let mut providers = HashMap::new();
    providers.insert(
        "custom".to_string(),
        LegacyV1ProviderEntry {
            base_url: Some("https://api.custom.example.com".to_string()),
            model: Some("custom-model".to_string()),
            ..Default::default()
        },
    );
    let cfg = migrate_v1(&LegacyV1Config {
        providers,
        ..Default::default()
    })
    .unwrap();

    let active = active_provider(&cfg);
    assert_eq!(active.kind, ProviderKind::OpenAiCompatible);
    assert_eq!(active.base_url, "https://api.custom.example.com");
    assert_eq!(
        cfg.profiles["default"].active_target.model_id,
        "custom-model"
    );
    assert_eq!(
        active.credential,
        CredentialRef::Env {
            var: "SHANNON_CUSTOM_API_KEY".to_string()
        }
    );
}

#[test]
fn top_level_overlays_matching_map_entry() {
    let mut providers = HashMap::new();
    providers.insert(
        "anthropic".to_string(),
        LegacyV1ProviderEntry {
            base_url: Some("https://proxy.anthropic.example.com".to_string()),
            ..Default::default()
        },
    );
    let cfg = migrate_v1(&LegacyV1Config {
        provider: Some("anthropic".to_string()),
        providers,
        ..Default::default()
    })
    .unwrap();

    let p = default_profile(&cfg);
    // Entry wins over the kind default; the top-level provider is not duplicated.
    assert_eq!(p.providers.len(), 1);
    assert_eq!(
        active_provider(&cfg).base_url,
        "https://proxy.anthropic.example.com"
    );
}

#[test]
fn migrated_config_roundtrips_through_serde_json() {
    // Structural validity: the migrated value is a well-formed v2 document.
    let cfg = migrate_v1(&anthropic_legacy()).unwrap();
    let json = serde_json::to_string(&cfg).unwrap();
    let back: ProviderModelConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn migrate_from_toml_snippet_end_to_end() {
    let toml = r#"
provider = "deepseek"
model = "deepseek-chat"

[providers.openai]
base_url = "https://api.openai.com/v1"
model = "gpt-4o"
"#;
    let legacy: LegacyV1Config = toml::from_str(toml).unwrap();
    let cfg = migrate_v1(&legacy).unwrap();

    assert_eq!(cfg.version, ProviderModelConfig::VERSION);
    let p = default_profile(&cfg);
    assert_eq!(p.active_target.provider_id, "deepseek");
    assert_eq!(p.active_target.model_id, "deepseek-chat");
    assert_eq!(p.providers.len(), 2);

    let openai = p
        .providers
        .iter()
        .find(|pp| pp.id == "openai")
        .expect("openai provider present");
    assert_eq!(openai.kind, ProviderKind::OpenAi);
    assert_eq!(openai.base_url, "https://api.openai.com/v1");
}
