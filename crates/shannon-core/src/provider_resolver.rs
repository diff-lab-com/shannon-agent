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
use shannon_types::provider_config::{
    CredentialRef, ProviderKind, ProviderModelConfig, ProviderProfile,
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
pub fn resolve_active_target(pm: &ProviderModelConfig) -> Option<ResolvedTarget<'_>> {
    let profile = pm.profiles.get("default")?;
    let active = profile
        .providers
        .iter()
        .find(|p| p.id == profile.active_target.provider_id)?;
    Some(ResolvedTarget {
        provider: resolve_provider(&active.kind, &active.base_url),
        profile: active,
        model_id: &profile.active_target.model_id,
    })
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
/// N1 resolves env vars only. N2 will additionally load `~/.shannon/secrets.env`
/// (chmod 0600) before consulting the process env.
///
/// - `Env { var }` → `std::env::var(var)`, empty string on unset.
/// - `InlineLegacy { masked }` → the transition-only value, as-is.
/// - `Keyring` / `Ephemeral` → empty here (opportunistic / session-injected; #9).
pub fn resolve_credential(cred: &CredentialRef) -> String {
    match cred {
        CredentialRef::Env { var } => std::env::var(var).unwrap_or_default(),
        CredentialRef::InlineLegacy { masked } => masked.clone(),
        CredentialRef::Keyring { .. } | CredentialRef::Ephemeral => String::new(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use shannon_types::provider_config::{
        ActiveTarget, CredentialRef, CredentialScope, ModelProfile, ProviderKind,
        ProviderModelConfig, ProviderProfile, Scope,
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
}
