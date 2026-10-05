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

/// The probe's reqwest client: `PROBE_TIMEOUT` total budget plus the
/// `SHANNON_CA_BUNDLE` custom roots (Settings R3 T4, B1). The chat client
/// (`client.rs::apply_custom_root_certificates`) trusts the same bundle —
/// without it a corporate MITM CA would surface as "Test connection failed"
/// while the very same endpoint works for actual chat traffic.
fn probe_client() -> Result<reqwest::Client, ApiError> {
    Ok(
        crate::api::client::apply_custom_root_certificates(reqwest::Client::builder())
            .timeout(PROBE_TIMEOUT)
            .build()?,
    )
}

async fn execute_probe(url: &str, auth_header: Option<&str>) -> Result<(), ApiError> {
    let client = probe_client()?;
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
            // Honor the server's Retry-After hint when present (C-3).
            retry_after_secs: resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok()),
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

    // ── Settings R3 T4 (B1): probe client carries the custom CA roots ──

    /// A real root certificate (ACCVRAIZ1, a public ES root), embedded
    /// verbatim — same constant shape the client.rs tests use.
    const VALID_PEM: &str = "-----BEGIN CERTIFICATE-----
MIIH0zCCBbugAwIBAgIIXsO3pkN/pOAwDQYJKoZIhvcNAQEFBQAwQjESMBAGA1UE
AwwJQUNDVlJBSVoxMRAwDgYDVQQLDAdQS0lBQ0NWMQ0wCwYDVQQKDARBQ0NWMQsw
CQYDVQQGEwJFUzAeFw0xMTA1MDUwOTM3MzdaFw0zMDEyMzEwOTM3MzdaMEIxEjAQ
BgNVBAMMCUFDQ1ZSQUlaMTEQMA4GA1UECwwHUEtJQUNDVjENMAsGA1UECgwEQUND
VjELMAkGA1UEBhMCRVMwggIiMA0GCSqGSIb3DQEBAQUAA4ICDwAwggIKAoICAQCb
qau/YUqXry+XZpp0X9DZlv3P4uRm7x8fRzPCRKPfmt4ftVTdFXxpNRFvu8gMjmoY
HtiP2Ra8EEg2XPBjs5BaXCQ316PWywlxufEBcoSwfdtNgM3802/J+Nq2DoLSRYWo
G2ioPej0RGy9ocLLA76MPhMAhN9KSMDjIgro6TenGEyxCQ0jVn8ETdkXhBilyNpA
lHPrzg5XPAOBOp0KoVdDaaxXbXmQeOW1tDvYvEyNKKGno6e6Ak4l0Squ7a4DIrhr
IA8wKFSVf+DuzgpmndFALW4ir50awQUZ0m/A8p/4e7MCQvtQqR0tkw8jq8bBD5L/
0KIV9VMJcRz/RROE5iZe+OCIHAr8Fraocwa48GOEAqDGWuzndN9wrqODJerWx5eH
k6fGioozl2A3ED6XPm4pFdahD9GILBKfb6qkxkLrQaLjlUPTAYVtjrs78yM2x/47
4KElB0iryYl0/wiPgL/AlmXz7uxLaL2diMMxs0Dx6M/2OLuc5NF/1OVYm3z61PMO
m3WR5LpSLhl+0fXNWhn8ugb2+1KoS5kE3fj5tItQo05iifCHJPqDQsGH+tUtKSpa
cXpkatcnYGMN285J9Y0fkIkyF/hzQ7jSWpOGYdbhdQrqeWZ2iE9x6wQl1gpaepPl
uUsXQA+xtrn13k/c4LOsOxFwYIRKQ26ZIMApcQrAZQIDAQABo4ICyzCCAscwfQYI
KwYBBQUHAQEEcTBvMEwGCCsGAQUFBzAChkBodHRwOi8vd3d3LmFjY3YuZXMvZmls
ZWFkbWluL0FyY2hpdm9zL2NlcnRpZmljYWRvcy9yYWl6YWNjdjEuY3J0MB8GCCsG
AQUFBzABhhNodHRwOi8vb2NzcC5hY2N2LmVzMB0GA1UdDgQWBBTSh7Tj3zcnk1X2
VuqB5TbMjB4/vTAPBgNVHRMBAf8EBTADAQH/MB8GA1UdIwQYMBaAFNKHtOPfNyeT
VfZW6oHlNsyMHj+9MIIBcwYDVR0gBIIBajCCAWYwggFiBgRVHSAAMIIBWDCCASIG
CCsGAQUFBwICMIIBFB6CARAAQQB1AHQAbwByAGkAZABhAGQAIABkAGUAIABDAGUA
cgB0AGkAZgBpAGMAYQBjAGkA8wBuACAAUgBhAO0AegAgAGQAZQAgAGwAYQAgAEEA
QwBDAFYAIAAoAEEAZwBlAG4AYwBpAGEAIABkAGUAIABUAGUAYwBuAG8AbABvAGcA
7QBhACAAeQAgAEMAZQByAHQAaQBmAGkAYwBhAGMAaQDzAG4AIABFAGwAZQBjAHQA
cgDzAG4AaQBjAGEALAAgAEMASQBGACAAUQA0ADYAMAAxADEANQA2AEUAKQAuACAA
QwBQAFMAIABlAG4AIABoAHQAdABwADoALwAvAHcAdwB3AC4AYQBjAGMAdgAuAGUA
czAwBggrBgEFBQcCARYkaHR0cDovL3d3dy5hY2N2LmVzL2xlZ2lzbGFjaW9uX2Mu
aHRtMFUGA1UdHwROMEwwSqBIoEaGRGh0dHA6Ly93d3cuYWNjdi5lcy9maWxlYWRt
aW4vQXJjaGl2b3MvY2VydGlmaWNhZG9zL3JhaXphY2N2MV9kZXIuY3JsMA4GA1Ud
DwEB/wQEAwIBBjAXBgNVHREEEDAOgQxhY2N2QGFjY3YuZXMwDQYJKoZIhvcNAQEF
BQADggIBAJcxAp/n/UNnSEQU5CmH7UwoZtCPNdpNYbdKl02125DgBS4OxnnQ8pdp
D70ER9m+27Up2pvZrqmZ1dM8MJP1jaGo/AaNRPTKFpV8M9xii6g3+CfYCS0b78gU
JyCpZET/LtZ1qmxNYEAZSUNUY9rizLpm5U9EelvZaoErQNV/+QEnWCzI7UiRfD+m
AM/EKXMRNt6GGT6d7hmKG9Ww7Y49nCrADdg9ZuM8Db3VlFzi4qc1GwQA9j9ajepD
vV+JHanBsMyZ4k0ACtrJJ1vnE5Bc5PUzolVt3OAJTS+xJlsndQAJxGJ3KQhfnlms
tn6tn1QwIgPBHnFk/vk4CpYY3QIUrCPLBhwepH2NDd4nQeit2hW3sCPdK6jT2iWH
7ehVRE2I9DZ+hJp4rPcOVkkO1jMl1oRQQmwgEh0q1b688nCBpHBgvgW1m54ERL5h
I6zppSSMEYCUWqKiuUnSwdzRp+0xESyeGabu4VXhwOrPDYTkF7eifKXeVSUG7szA
h1xA2syVP1XgNce4hL60Xc16gwFy7ofmXx2utYXGJt/mwZrpHgJHnyqobalbz+xF
d3+YJ5oyXSrjhO7FmGYvliAd3djDJ9ew+f7Zfc3Qn48LFFhRny+Lwzgt3uiP1o2H
pPVWQxaZLPSkVrQ0uGE3ycJYgBugl6H8WY3pEfbRD0tVNEYqi4Y7
-----END CERTIFICATE-----";

    /// The probe client must build (a) with no bundle at all — the plain
    /// default path every existing probe test rides — and (b) with a
    /// SHANNON_CA_BUNDLE whose valid PEM is actually taken up, not silently
    /// dropped. (c) a dangling path degrades to built-in roots instead of
    /// failing the probe setup. Env mutation is safe under nextest (one
    /// process per test); nothing else in this module reads the variable.
    #[test]
    fn probe_client_builds_with_and_without_custom_ca_bundle() {
        unsafe { std::env::remove_var("SHANNON_CA_BUNDLE") };
        probe_client().expect("probe client must build without a bundle");

        let dir = std::env::temp_dir().join(format!("shannon-probe-ca-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let bundle = dir.join("root-ca.pem");
        std::fs::write(&bundle, VALID_PEM).expect("write PEM");
        unsafe { std::env::set_var("SHANNON_CA_BUNDLE", &bundle) };
        probe_client().expect("probe client must build with a valid bundle");
        unsafe { std::env::remove_var("SHANNON_CA_BUNDLE") };
        let _ = std::fs::remove_dir_all(&dir);
        // The rustls-backed client's root store is opaque here; "builds" is
        // the contract the old bare builder broke (a corrupt bundle can
        // brick ClientBuilder::build), and the roots themselves are
        // asserted in client.rs's add_pem_roots tests.

        unsafe { std::env::set_var("SHANNON_CA_BUNDLE", "/nonexistent/probe-ca.pem") };
        assert!(
            probe_client().is_ok(),
            "dangling bundle path must degrade to built-in roots, not error"
        );
        unsafe { std::env::remove_var("SHANNON_CA_BUNDLE") };
    }

    #[tokio::test]
    async fn probe_hits_the_endpoint_with_custom_ca_wiring_active() {
        // End-to-end smoke over plain HTTP: with a bundle configured (the
        // PEM is irrelevant for http://) the probe still reaches the target
        // and maps the status — proving the wiring does not perturb the
        // request path.
        let dir =
            std::env::temp_dir().join(format!("shannon-probe-ca-http-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let bundle = dir.join("root-ca.pem");
        std::fs::write(&bundle, VALID_PEM).expect("write PEM");
        unsafe { std::env::set_var("SHANNON_CA_BUNDLE", &bundle) };

        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/v1/models")
            .with_status(401)
            .create_async()
            .await;
        let base = server.url();
        let err = probe_provider_endpoint("openai", "sk-bad", Some(&base))
            .await
            .expect_err("401 must surface as an auth failure");

        unsafe { std::env::remove_var("SHANNON_CA_BUNDLE") };
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            matches!(err, ApiError::AuthenticationFailed),
            "expected AuthenticationFailed, got {err:?}"
        );
    }
}
