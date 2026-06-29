//! Voice input (D4) — cloud speech-to-text via an OpenAI-compatible Whisper
//! endpoint (Groq / OpenAI / custom). The frontend captures audio with
//! `MediaRecorder`, base64-encodes it, and sends it here; this command builds
//! the multipart transcription request, calls the provider, and returns text.
//!
//! API keys live server-side (in `DesktopConfig.stt`) so they never reach the
//! webview, and the provider call avoids browser CORS entirely.

use std::time::Duration;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tauri::Emitter;

use crate::commands::AppState;
use crate::commands_config::validate_base_url;
use crate::config::{self, SttConfig};
use crate::events;
use crate::events::event_names;

/// Successful transcription.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionResult {
    pub text: String,
}

/// Resolve the Whisper-compatible `/audio/transcriptions` URL and the default
/// model for an STT provider preset. Pure (no network) so it is unit-testable.
///
/// `custom` requires a `base_url`; the built-in presets supply canonical URLs.
fn stt_endpoint(
    provider: &str,
    base_url: Option<&str>,
) -> Result<(String, Option<String>), String> {
    let validated = match base_url {
        Some(raw) => Some(validate_base_url(raw)?),
        None => None,
    };
    Ok(match provider {
        "groq" => {
            let base = validated
                .unwrap_or_else(|| "https://api.groq.com/openai/v1".to_string());
            (
                format!("{base}/audio/transcriptions"),
                Some("whisper-large-v3".to_string()),
            )
        }
        "openai" => {
            let base = validated
                .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
            (
                format!("{base}/audio/transcriptions"),
                Some("whisper-1".to_string()),
            )
        }
        "custom" => {
            let base = validated
                .ok_or_else(|| "custom STT provider requires a base_url".to_string())?;
            (format!("{base}/audio/transcriptions"), None)
        }
        other => return Err(format!("unknown STT provider: {other}")),
    })
}

/// Transcribe a base64-encoded audio recording via the configured cloud STT
/// provider. Returns `STT_NOT_CONFIGURED` when no provider/key is set so the UI
/// can prompt the user instead of showing a raw network error. Other errors
/// carry an `STT_*:` prefix (`STT_INVALID_KEY`, `STT_RATE_LIMITED`,
/// `STT_NETWORK`) so the frontend can map them to specific toasts.
#[tauri::command]
pub async fn transcribe_audio(
    state: tauri::State<'_, AppState>,
    audio_base64: String,
    mime_type: String,
    language: Option<String>,
) -> Result<TranscriptionResult, String> {
    let stt = {
        let cfg = state.desktop_config.read().await;
        cfg.stt.clone().unwrap_or_default()
    };

    let provider = stt.provider.as_deref().unwrap_or("").trim().to_string();
    let api_key = stt
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if provider.is_empty() || api_key.is_none() {
        return Err(
            "STT_NOT_CONFIGURED: configure a speech-to-text provider in Settings".into(),
        );
    }
    let api_key = api_key.unwrap().to_string();

    let (url, default_model) = stt_endpoint(&provider, stt.base_url.as_deref())?;
    let model = stt
        .model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or(default_model)
        .ok_or_else(|| "STT provider model is required for a custom endpoint".to_string())?;

    let audio_bytes = base64::engine::general_purpose::STANDARD
        .decode(audio_base64.as_bytes())
        .map_err(|e| format!("invalid audio base64: {e}"))?;
    if audio_bytes.is_empty() {
        return Err("empty audio recording".into());
    }

    let mime = {
        let m = mime_type.trim();
        if m.is_empty() {
            "audio/webm".to_string()
        } else {
            m.to_string()
        }
    };
    let ext = extension_for(&mime);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| format!("STT_NETWORK: failed to build HTTP client: {e}"))?;

    let mut form = reqwest::multipart::Form::new().text("model", model).part(
        "file",
        reqwest::multipart::Part::bytes(audio_bytes)
            .file_name(format!("recording.{ext}"))
            .mime_str(&mime)
            .map_err(|e| format!("invalid mime type: {e}"))?,
    );
    if let Some(lang) = language
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        form = form.text("language", lang.to_string());
    }

    let resp = client
        .post(&url)
        .bearer_auth(&api_key)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("STT_NETWORK: {e}"))?;

    let status = resp.status();
    if !status.is_success() {
        // The body comes from an external provider and may be arbitrarily
        // large or contain control characters; sanitize before surfacing.
        let body = sanitize_error_body(&resp.text().await.unwrap_or_default());
        return Err(match status.as_u16() {
            401 | 403 => format!("STT_INVALID_KEY: provider rejected the API key ({body})"),
            429 => format!("STT_RATE_LIMITED: {body}"),
            _ => format!("STT provider error (HTTP {}): {body}", status.as_u16()),
        });
    }

    let body = resp
        .text()
        .await
        .map_err(|e| format!("STT_NETWORK: {e}"))?;
    let text = parse_transcription(&body);
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("STT provider returned an empty transcript".into());
    }
    Ok(TranscriptionResult { text })
}

/// Extract the transcript text from a provider response body. OpenAI-compatible
/// endpoints return `{"text": "..."}` by default; some return bare text. Handle
/// both, falling back to the raw body.
fn parse_transcription(body: &str) -> String {
    let trimmed = body.trim_start();
    if trimmed.starts_with('{') {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if let Some(text) = val.get("text").and_then(|v| v.as_str()) {
                return text.to_string();
            }
        }
    }
    body.to_string()
}

fn extension_for(mime: &str) -> &'static str {
    if mime.contains("webm") {
        "webm"
    } else if mime.contains("ogg") {
        "ogg"
    } else if mime.contains("wav") {
        "wav"
    } else if mime.contains("mp4") || mime.contains("m4a") {
        "m4a"
    } else if mime.contains("mpeg") || mime.contains("mp3") {
        "mp3"
    } else {
        "bin"
    }
}

/// Flatten and cap an external provider's error body so it is safe to surface
/// to the user: control characters become spaces, surrounding/inner runs are
/// trimmed, and the result is truncated to a bounded length. Provider error
/// bodies are untrusted and may be arbitrarily large or contain noise.
fn sanitize_error_body(body: &str) -> String {
    let cleaned: String = body
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let trimmed = cleaned.trim();
    const MAX: usize = 200;
    if trimmed.chars().count() <= MAX {
        return trimmed.to_string();
    }
    let truncated: String = trimmed.chars().take(MAX).collect();
    format!("{truncated}…")
}

/// Return the current STT config with the API key masked to `"***"`.
#[tauri::command]
pub async fn get_stt_config(
    state: tauri::State<'_, AppState>,
) -> Result<Option<SttConfig>, String> {
    let cfg = state.desktop_config.read().await;
    Ok(cfg.stt.clone().map(mask_stt_key))
}

/// Persist the STT provider config. Validates the provider preset and any
/// custom `base_url`. An `api_key` of `"***"` or empty keeps the existing key,
/// so editing the model never blanks the stored secret. Emits `CONFIG_UPDATED`
/// so open settings panels refresh.
#[tauri::command]
pub async fn save_stt_config(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    stt_config: SttConfig,
) -> Result<(), String> {
    let provider = stt_config
        .provider
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .to_string();
    if !provider.is_empty() && !matches!(provider.as_str(), "groq" | "openai" | "custom") {
        return Err(format!("unknown STT provider: {provider}"));
    }
    let base_url = match stt_config.base_url.as_deref().map(str::trim) {
        Some(b) if !b.is_empty() => Some(validate_base_url(b)?),
        _ => None,
    };
    let cleaned = SttConfig {
        provider: if provider.is_empty() {
            None
        } else {
            Some(provider)
        },
        api_key: resolve_key(&state, &stt_config.api_key).await,
        base_url,
        model: stt_config
            .model
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
    };

    {
        let mut dc = state.desktop_config.write().await;
        dc.stt = Some(cleaned);
    }
    {
        let dc = state.desktop_config.read().await;
        config::save_config(&dc)?;
    }
    let _ = app_handle.emit(
        event_names::CONFIG_UPDATED,
        events::ConfigUpdatedPayload {
            key: "stt".into(),
            value: "saved".into(),
        },
    );
    Ok(())
}

/// Resolve the api_key to persist: a fresh value wins; `"***"` or empty keeps
/// the currently-stored key (so editing the model never blanks the secret).
async fn resolve_key(
    state: &tauri::State<'_, AppState>,
    incoming: &Option<String>,
) -> Option<String> {
    match incoming.as_deref().map(str::trim) {
        Some(k) if !k.is_empty() && k != "***" => Some(k.to_string()),
        _ => {
            let dc = state.desktop_config.read().await;
            dc.stt.as_ref().and_then(|s| s.api_key.clone())
        }
    }
}

fn mask_stt_key(mut s: SttConfig) -> SttConfig {
    if s.api_key.is_some() {
        s.api_key = Some("***".into());
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stt_endpoint_uses_canonical_defaults() {
        let (url, model) = stt_endpoint("groq", None).unwrap();
        assert_eq!(url, "https://api.groq.com/openai/v1/audio/transcriptions");
        assert_eq!(model.as_deref(), Some("whisper-large-v3"));

        let (url, model) = stt_endpoint("openai", None).unwrap();
        assert_eq!(url, "https://api.openai.com/v1/audio/transcriptions");
        assert_eq!(model.as_deref(), Some("whisper-1"));
    }

    #[test]
    fn stt_endpoint_custom_requires_base_url() {
        let err = stt_endpoint("custom", None).unwrap_err();
        assert!(err.contains("base_url"));
        let (url, model) = stt_endpoint("custom", Some("https://stt.example.com/v1")).unwrap();
        assert_eq!(url, "https://stt.example.com/v1/audio/transcriptions");
        assert!(model.is_none());
    }

    #[test]
    fn stt_endpoint_rejects_unknown_provider_and_unsafe_url() {
        assert!(stt_endpoint("azure", None).is_err());
        assert!(stt_endpoint("custom", Some("file:///x")).is_err());
    }

    #[test]
    fn parse_transcription_handles_json_and_plain() {
        assert_eq!(parse_transcription(r#"{"text":"hello world"}"#), "hello world");
        assert_eq!(parse_transcription("bare text response"), "bare text response");
    }

    #[test]
    fn extension_for_maps_common_mimes() {
        assert_eq!(extension_for("audio/webm"), "webm");
        assert_eq!(extension_for("audio/ogg"), "ogg");
        assert_eq!(extension_for("audio/wav"), "wav");
        assert_eq!(extension_for("audio/mp4"), "m4a");
        assert_eq!(extension_for("application/octet-stream"), "bin");
    }

    #[test]
    fn mask_stt_key_replaces_present_key_but_keeps_absence() {
        let masked = mask_stt_key(SttConfig {
            provider: Some("groq".into()),
            api_key: Some("sk-secret".into()),
            base_url: None,
            model: None,
        });
        assert_eq!(masked.api_key.as_deref(), Some("***"));

        let absent = mask_stt_key(SttConfig {
            provider: Some("groq".into()),
            api_key: None,
            base_url: None,
            model: None,
        });
        assert!(absent.api_key.is_none());
    }

    #[test]
    fn sanitize_error_body_flattens_and_truncates() {
        assert_eq!(sanitize_error_body("ok"), "ok");
        // Control characters become spaces, then trimmed.
        assert_eq!(sanitize_error_body("\n  line1\nline2\tend \r\n"), "line1 line2 end");
        let long = "x".repeat(500);
        let out = sanitize_error_body(&long);
        assert!(out.ends_with('…'));
        assert_eq!(out.chars().count(), 201); // 200 chars + ellipsis
    }
}
