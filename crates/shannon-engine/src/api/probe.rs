//! Per-provider lightweight endpoint probe.
//!
//! Sits beside the chat-completion client (`client.rs`) and the streaming
//! adapter (`streaming.rs`). Hits a provider's "list models" endpoint
//! (or Ollama's `/api/tags`) to validate reachability + credential without
//! a billable chat token. Mirrors the desktop shell's former
//! `provider_probe_url` / `ping_provider` pair so both front-ends share one
//! implementation (ADR-0005 task 5).
//!
//! Status code → [`ApiError`] mapping:
//! - 200..=299 → `Ok(())`
//! - 401 / 403 → [`ApiError::AuthenticationFailed`]
//! - 429       → [`ApiError::RateLimitExceeded`] (no `Retry-After` parsed)
//! - 5xx, other → [`ApiError::ApiError`] with the status code
//! - network / timeout → [`ApiError::HttpError`] / [`ApiError::Timeout`]

use crate::api::error::ApiError;
use crate::api::types::LlmProvider;
use std::time::Duration;

/// Timeout for the probe HTTP round-trip. Matches the desktop shell's prior
/// `reqwest::Client::builder().timeout(10s)` so behaviour is preserved.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Map an `LlmProvider` to the probe slug used by [`probe_provider_endpoint`.
///
/// Returns `None` for providers that have no shared list-models endpoint we
/// can probe (Gemini, Bedrock, Azure, Replicate, … — they all speak bespoke
/// list-models APIs). Used by `QueryEngine::probe_all_health` to fan out a
/// health check over every allowed provider in a single pass.
pub fn probe_kind_for_provider(p: &LlmProvider) -> Option<&'static str> {
    match p {
        LlmProvider::Anthropic => Some("anthropic"),
        LlmProvider::OpenAI => Some("openai"),
        LlmProvider::DeepSeek => Some("deepseek"),
        LlmProvider::Ollama => Some("ollama"),
        // Every other OpenAI-wire-format provider (Zhipu / Moonshot / Groq /
        // Together / OpenRouter / Cohere / Fireworks / Perplexity / Xai /
        // Ai21 / Cloudflare / SiliconFlow / Minimax / DashScope) shares the
        // openai-compatible `/models` endpoint.
        p if p.is_openai_compatible() => Some("openai-compatible"),
        _ => None,
    }
}

/// Lightweight, non-billable probe. `provider_kind` is the canonical slug
/// (`anthropic` / `openai` / `deepseek` / `ollama` / `openai-compatible`).
/// `base_url` overrides the canonical endpoint and is **required** for
/// `openai-compatible` providers (GLM / Zhipu / Moonshot / …).
///
/// Does not mutate any state. Used by `/connect`, `/provider health`, and
/// (after this task) the desktop shell's `test_provider_connection`.
pub async fn probe_provider_endpoint(
    provider_kind: &str,
    api_key: &str,
    base_url: Option<&str>,
) -> Result<(), ApiError> {
    match provider_kind {
        "anthropic" | "openai" | "deepseek" | "openai-compatible" => {
            let (url, auth_header) =
                build_authenticated_probe_url(provider_kind, api_key, base_url)?;
            execute_probe(&url, auth_header.as_deref()).await
        }
        "ollama" => {
            // Ollama needs no auth and uses its bespoke tags endpoint.
            // Default to `localhost:11434` so the bare-call case matches the
            // desktop shell's `OLLAMA_HOST` fallback semantics.
            let base = base_url.unwrap_or("http://localhost:11434");
            ensure_http_scheme(base)?;
            execute_probe(&format!("{base}/api/tags"), None).await
        }
        other => Err(ApiError::UnsupportedProvider(other.to_string())),
    }
}

/// Build the `(url, auth_header)` pair for an authenticated provider. The
/// `auth_header` is the raw `"Name: value"` form because the executor splits
/// it before applying via `reqwest::RequestBuilder::header`.
fn build_authenticated_probe_url(
    provider_kind: &str,
    api_key: &str,
    base_url: Option<&str>,
) -> Result<(String, Option<String>), ApiError> {
    match provider_kind {
        "anthropic" => {
            let base = base_url.unwrap_or("https://api.anthropic.com");
            ensure_http_scheme(base)?;
            Ok((
                format!("{base}/v1/models?limit=1"),
                Some(format!("x-api-key: {api_key}")),
            ))
        }
        "openai" => {
            let base = base_url.unwrap_or("https://api.openai.com");
            ensure_http_scheme(base)?;
            Ok((
                format!("{base}/v1/models"),
                Some(format!("Authorization: Bearer {api_key}")),
            ))
        }
        "deepseek" => {
            let base = base_url.unwrap_or("https://api.deepseek.com");
            ensure_http_scheme(base)?;
            Ok((
                format!("{base}/models"),
                Some(format!("Authorization: Bearer {api_key}")),
            ))
        }
        "openai-compatible" => {
            // openai-compatible is the catch-all (GLM / Zhipu / Moonshot /
            // Together / Groq / …) — every one of them needs an explicit
            // base_url to know which endpoint to probe.
            let base = base_url.ok_or_else(|| ApiError::ApiError {
                status: 0,
                message: "openai-compatible provider requires a base_url".to_string(),
            })?;
            ensure_http_scheme(base)?;
            Ok((
                format!("{base}/models"),
                Some(format!("Authorization: Bearer {api_key}")),
            ))
        }
        _ => unreachable!("validated by caller match"),
    }
}

/// Reject non-http(s) schemes (defeats `javascript:` / `file://` injection).
/// Lightweight: the desktop shell runs a stricter `validate_base_url` that
/// also forbids embedded credentials; this is a defence-in-depth check that
/// keeps the engine safe even when called directly.
fn ensure_http_scheme(raw: &str) -> Result<&str, ApiError> {
    if raw.starts_with("http://") || raw.starts_with("https://") {
        Ok(raw)
    } else {
        Err(ApiError::ApiError {
            status: 0,
            message: format!("base_url must use http or https: `{raw}`"),
        })
    }
}

async fn execute_probe(url: &str, auth_header: Option<&str>) -> Result<(), ApiError> {
    let client = reqwest::Client::builder().timeout(PROBE_TIMEOUT).build()?;
    let mut req = client.get(url);
    if let Some(auth) = auth_header {
        let (name, value) = auth.split_once(": ").ok_or_else(|| ApiError::ApiError {
            status: 0,
            message: "malformed auth header".to_string(),
        })?;
        req = req.header(name, value);
    }
    // Anthropic requires the version header alongside the key.
    if auth_header.is_some_and(|s| s.starts_with("x-api-key:")) {
        req = req.header("anthropic-version", "2023-06-01");
    }

    let send_fut = req.send();
    let resp = tokio::time::timeout(PROBE_TIMEOUT, send_fut)
        .await
        .map_err(|_| ApiError::Timeout)??;
    let status = resp.status().as_u16();
    match status {
        200..=299 => Ok(()),
        401 | 403 => Err(ApiError::AuthenticationFailed),
        429 => Err(ApiError::RateLimitExceeded {
            retry_after_secs: None,
        }),
        other => Err(ApiError::ApiError {
            status: other,
            message: format!("HTTP {other}"),
        }),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_http_scheme() {
        assert!(ensure_http_scheme("javascript:alert(1)").is_err());
        assert!(ensure_http_scheme("file:///etc/passwd").is_err());
        assert!(ensure_http_scheme("ftp://example.com").is_err());
        assert!(ensure_http_scheme("https://api.example.com").is_ok());
        assert!(ensure_http_scheme("http://localhost:11434").is_ok());
    }

    #[test]
    fn anthropic_uses_x_api_key_and_default_base() {
        let (url, auth) = build_authenticated_probe_url("anthropic", "sk-test", None).unwrap();
        assert_eq!(url, "https://api.anthropic.com/v1/models?limit=1");
        assert_eq!(auth.as_deref(), Some("x-api-key: sk-test"));
    }

    #[test]
    fn openai_uses_bearer_and_default_base() {
        let (url, auth) = build_authenticated_probe_url("openai", "sk-test", None).unwrap();
        assert_eq!(url, "https://api.openai.com/v1/models");
        assert_eq!(auth.as_deref(), Some("Authorization: Bearer sk-test"));
    }

    #[test]
    fn deepseek_uses_models_path_and_default_base() {
        let (url, auth) = build_authenticated_probe_url("deepseek", "sk-test", None).unwrap();
        assert_eq!(url, "https://api.deepseek.com/models");
        assert_eq!(auth.as_deref(), Some("Authorization: Bearer sk-test"));
    }

    #[test]
    fn openai_compatible_requires_base_url() {
        let err = build_authenticated_probe_url("openai-compatible", "k", None).unwrap_err();
        assert!(
            matches!(err, ApiError::ApiError { .. }),
            "expected ApiError with status 0, got {err:?}"
        );
        assert!(err.to_string().contains("requires a base_url"));
    }

    #[test]
    fn openai_compatible_uses_provided_base() {
        let (url, auth) = build_authenticated_probe_url(
            "openai-compatible",
            "k",
            Some("https://open.bigmodel.cn"),
        )
        .unwrap();
        assert_eq!(url, "https://open.bigmodel.cn/models");
        assert_eq!(auth.as_deref(), Some("Authorization: Bearer k"));
    }

    #[test]
    fn base_url_overrides_default_for_built_in_kinds() {
        let (url, _) =
            build_authenticated_probe_url("anthropic", "k", Some("https://proxy.example.com"))
                .unwrap();
        assert_eq!(url, "https://proxy.example.com/v1/models?limit=1");
    }

    #[test]
    fn non_http_base_url_rejected() {
        let err =
            build_authenticated_probe_url("openai-compatible", "k", Some("javascript:alert(1)"))
                .unwrap_err();
        assert!(err.to_string().contains("http or https"));
    }

    #[test]
    fn probe_kind_canonical_providers() {
        use crate::api::types::LlmProvider;
        assert_eq!(
            probe_kind_for_provider(&LlmProvider::Anthropic),
            Some("anthropic")
        );
        assert_eq!(
            probe_kind_for_provider(&LlmProvider::OpenAI),
            Some("openai")
        );
        assert_eq!(
            probe_kind_for_provider(&LlmProvider::DeepSeek),
            Some("deepseek")
        );
        assert_eq!(
            probe_kind_for_provider(&LlmProvider::Ollama),
            Some("ollama")
        );
    }

    #[test]
    fn probe_kind_openai_compatible_collapses_to_openai_compatible() {
        use crate::api::types::LlmProvider;
        // Every other OpenAI-wire-format provider shares the openai-compatible
        // /models endpoint, so they all map to that probe slug regardless of
        // their specific default base_url.
        assert_eq!(
            probe_kind_for_provider(&LlmProvider::Zhipu),
            Some("openai-compatible")
        );
        assert_eq!(
            probe_kind_for_provider(&LlmProvider::Moonshot),
            Some("openai-compatible")
        );
        assert_eq!(
            probe_kind_for_provider(&LlmProvider::Groq),
            Some("openai-compatible")
        );
        assert_eq!(
            probe_kind_for_provider(&LlmProvider::OpenRouter),
            Some("openai-compatible")
        );
    }

    #[test]
    fn probe_kind_unsupported_returns_none() {
        use crate::api::types::LlmProvider;
        // Gemini uses a bespoke list-models API (WireFormat::Gemini), so the
        // shared openai-compatible probe slug does not apply.
        assert_eq!(probe_kind_for_provider(&LlmProvider::Gemini), None);
    }
}
