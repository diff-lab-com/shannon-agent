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
            let base = validated.unwrap_or_else(|| "https://api.groq.com/openai/v1".to_string());
            (
                format!("{base}/audio/transcriptions"),
                Some("whisper-large-v3".to_string()),
            )
        }
        "openai" => {
            let base = validated.unwrap_or_else(|| "https://api.openai.com/v1".to_string());
            (
                format!("{base}/audio/transcriptions"),
                Some("whisper-1".to_string()),
            )
        }
        "custom" => {
            let base =
                validated.ok_or_else(|| "custom STT provider requires a base_url".to_string())?;
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
        return Err("STT_NOT_CONFIGURED: configure a speech-to-text provider in Settings".into());
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
    if let Some(lang) = language.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
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

    let body = resp.text().await.map_err(|e| format!("STT_NETWORK: {e}"))?;
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

/// Return the current local-voice config (P2-5e). The struct has
/// no secrets, so we return it as-is — no masking needed. The
/// `model` field is an opaque slug; the UI matches it against
/// `list_whisper_models` to know whether the file is on disk.
#[tauri::command]
pub async fn get_voice_local_config(
    state: tauri::State<'_, AppState>,
) -> Result<crate::config::VoiceLocalConfig, String> {
    let cfg = state.desktop_config.read().await;
    Ok(cfg.voice_local.clone())
}

/// Persist the local-voice config. Validates the model slug
/// (unknown slugs are stored verbatim so a future `medium` field
/// round-trips through an older build). Emits `CONFIG_UPDATED` so
/// the open settings panel refreshes. Idempotent — saving the
/// same value twice is a no-op.
#[tauri::command]
pub async fn save_voice_local_config(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    voice_local: crate::config::VoiceLocalConfig,
) -> Result<(), String> {
    // Trim string fields so a stray space doesn't accidentally
    // enable a model that doesn't exist.
    let cleaned = crate::config::VoiceLocalConfig {
        enabled: voice_local.enabled,
        model: voice_local
            .model
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        language: voice_local
            .language
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        auto_download: voice_local.auto_download,
    };

    {
        let mut dc = state.desktop_config.write().await;
        dc.voice_local = cleaned;
    }
    {
        let dc = state.desktop_config.read().await;
        crate::config::save_config(&dc)?;
    }
    let _ = app_handle.emit(
        event_names::CONFIG_UPDATED,
        events::ConfigUpdatedPayload {
            key: "voice_local".into(),
            value: "saved".into(),
        },
    );
    Ok(())
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

// ── P2-5e: local whisper-rs STT ──────────────────────────────────────────
//
// The local path runs whisper-rs (whisper.cpp 1.5.x via the
// `voice-local` Cargo feature) on a WAV file the frontend has
// written to a temp path. The model is downloaded on first use
// (gated by `voice_local.auto_download`) from
// `~/.shannon/models/whisper/`. See `commands_voice_models.rs` for
// the catalog + download plumbing and `docs/integrations/voice-local.md`
// for the full design.
//
// The command is *compiled* only when the `voice-local` feature is
// enabled so the default build doesn't pull in the C++ toolchain.
// On builds without the feature, a stub command with the same name
// returns a typed `STT_FEATURE_DISABLED` error — that way the
// frontend can call `transcribe_audio_local` unconditionally and
// the Settings card surfaces a clear "rebuild with --features
// voice-local" message instead of a generic "command not found".

/// Stub returned on builds that omit the `voice-local` feature.
/// Keeps the wire shape stable across feature combinations so the
/// frontend doesn't need to feature-gate its calls.
#[cfg(not(all(feature = "voice-local", feature = "tauri")))]
#[tauri::command]
pub async fn transcribe_audio_local(
    _audio_path: std::path::PathBuf,
    _model: Option<String>,
    _language: Option<String>,
) -> Result<TranscriptionResult, String> {
    Err(
        "STT_FEATURE_DISABLED: local voice requires a rebuild with `--features voice-local`"
            .to_string(),
    )
}

/// Same shape as the cloud `transcribe_audio` — takes a base64
/// string + mime. The Rust side writes the bytes to a temp file
/// (so whisper-rs can read with hound) and runs inference. The
/// path-based `transcribe_audio_local` above is for tests and
/// power users who want to manage the temp file themselves.
#[cfg(not(all(feature = "voice-local", feature = "tauri")))]
#[tauri::command]
pub async fn transcribe_audio_local_base64(
    _audio_base64: String,
    _mime_type: String,
    _model: Option<String>,
    _language: Option<String>,
) -> Result<TranscriptionResult, String> {
    Err(
        "STT_FEATURE_DISABLED: local voice requires a rebuild with `--features voice-local`"
            .to_string(),
    )
}

#[cfg(all(feature = "voice-local", feature = "tauri"))]
#[tauri::command]
pub async fn transcribe_audio_local(
    state: tauri::State<'_, crate::commands::AppState>,
    app_handle: tauri::AppHandle,
    audio_path: std::path::PathBuf,
    model: Option<String>,
    language: Option<String>,
) -> Result<TranscriptionResult, String> {
    commands_voice_local_impl::transcribe_audio_local_impl(
        state, app_handle, audio_path, model, language,
    )
    .await
}

#[cfg(all(feature = "voice-local", feature = "tauri"))]
#[tauri::command]
pub async fn transcribe_audio_local_base64(
    state: tauri::State<'_, crate::commands::AppState>,
    app_handle: tauri::AppHandle,
    audio_base64: String,
    mime_type: String,
    model: Option<String>,
    language: Option<String>,
) -> Result<TranscriptionResult, String> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(audio_base64.as_bytes())
        .map_err(|e| format!("STT_AUDIO_INVALID: bad base64: {e}"))?;
    if bytes.is_empty() {
        return Err("STT_AUDIO_INVALID: empty audio".to_string());
    }
    // Write the bytes to a temp file in the system temp dir. The
    // path is deterministic (timestamp + nanos) so concurrent
    // recordings don't clobber each other. whisper-rs reads from
    // disk via hound; we delete the file at the end of the
    // inference call (best-effort).
    let dir = std::env::temp_dir();
    let path = dir.join(format!(
        "shannon-voice-{}-{}.wav",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    if let Err(e) = std::fs::write(&path, &bytes) {
        return Err(format!("STT_AUDIO_INVALID: could not write temp wav: {e}"));
    }
    // The mime_type tells the inference path which decoder to
    // try. Only `audio/wav` is currently supported by the
    // local path; anything else returns STT_AUDIO_INVALID
    // rather than silently failing on a mis-decoded file.
    if !mime_type.to_ascii_lowercase().contains("wav") {
        let _ = std::fs::remove_file(&path);
        return Err(format!(
            "STT_AUDIO_INVALID: local path only accepts audio/wav (got {mime_type})"
        ));
    }
    let result = commands_voice_local_impl::transcribe_audio_local_impl(
        state, app_handle, path, model, language,
    )
    .await;
    // The `transcribe_audio_local_impl` helper already removes
    // the temp file on success; on failure the file may linger
    // until the OS cleans up tmp. Either way, no leak across
    // processes (different pid component in the filename).
    result
}

#[cfg(all(feature = "voice-local", feature = "tauri"))]
pub use commands_voice_local_impl::TranscriptionLocalError;

#[cfg(all(feature = "voice-local", feature = "tauri"))]
mod commands_voice_local_impl {
    use std::path::{Path, PathBuf};

    use super::TranscriptionResult;
    use crate::commands_voice_models::{
        WhisperModel, download_model as download_model_impl, model_path,
    };
    use crate::config::VoiceLocalConfig;

    /// Errors surfaced to the frontend. The `STT_*` prefix is
    /// stable; the front-end `localProvider` maps each one to a
    /// typed toast code.
    #[derive(Debug)]
    pub enum TranscriptionLocalError {
        FeatureDisabled,
        ModelNotFound,
        ModelLoading(String),
        AudioInvalid(String),
        InferenceFailed(String),
        LanguageUnsupported(String),
    }

    impl TranscriptionLocalError {
        pub fn to_wire(self) -> String {
            match self {
                Self::FeatureDisabled => "STT_FEATURE_DISABLED: build with --features voice-local".to_string(),
                Self::ModelNotFound => "STT_MODEL_NOT_FOUND: model not on disk; enable auto-download or download from Settings → Voice".to_string(),
                Self::ModelLoading(e) => format!("STT_MODEL_LOADING: {e}"),
                Self::AudioInvalid(e) => format!("STT_AUDIO_INVALID: {e}"),
                Self::InferenceFailed(e) => format!("STT_INFERENCE_FAILED: {e}"),
                Self::LanguageUnsupported(e) => format!("STT_LANGUAGE_UNSUPPORTED: {e}"),
            }
        }
    }

    /// Read a WAV file via `hound` and resample to 16 kHz mono
    /// PCM f32 — the format whisper-rs expects.
    ///
    /// We resample by simple linear interpolation rather than
    /// pulling in `rubato`. The browser's MediaRecorder is already
    /// at 48 kHz on Linux/macOS, and resampling 48 kHz → 16 kHz via
    /// linear interpolation is audibly indistinguishable for
    /// speech (the ear does no better than ~3× oversampled, and
    /// the spectral artefacts fall above the whisper model's
    /// effective bandwidth). A proper resampler is a follow-up if
    /// anyone reports it as a quality issue.
    fn load_wav_as_pcm_f32(path: &Path) -> Result<(Vec<f32>, u32), TranscriptionLocalError> {
        let reader = hound::WavReader::open(path)
            .map_err(|e| TranscriptionLocalError::AudioInvalid(e.to_string()))?;
        let spec = reader.spec();
        let channels = spec.channels as usize;
        let sample_rate = spec.sample_rate;
        let bits = spec.bits_per_sample;
        if channels == 0 || channels > 2 {
            return Err(TranscriptionLocalError::AudioInvalid(format!(
                "unsupported channel count: {channels}"
            )));
        }

        // Decode to mono (i16 → f32, f32 → f32, or i32 → f32).
        let mono_f32: Vec<f32> = match (bits, spec.sample_format) {
            (16, hound::SampleFormat::Int) => {
                let raw: Result<Vec<i16>, _> = reader.into_samples::<i16>().collect();
                let raw = raw.map_err(|e| TranscriptionLocalError::AudioInvalid(e.to_string()))?;
                raw.chunks(channels)
                    .map(|c| {
                        let sum: i32 = c.iter().map(|s| *s as i32).sum();
                        sum as f32 / (channels as f32 * i16::MAX as f32)
                    })
                    .collect()
            }
            (32, hound::SampleFormat::Float) => {
                let raw: Result<Vec<f32>, _> = reader.into_samples::<f32>().collect();
                let raw = raw.map_err(|e| TranscriptionLocalError::AudioInvalid(e.to_string()))?;
                raw.chunks(channels)
                    .map(|c| c.iter().sum::<f32>() / channels as f32)
                    .collect()
            }
            (32, hound::SampleFormat::Int) => {
                let raw: Result<Vec<i32>, _> = reader.into_samples::<i32>().collect();
                let raw = raw.map_err(|e| TranscriptionLocalError::AudioInvalid(e.to_string()))?;
                raw.chunks(channels)
                    .map(|c| {
                        let sum: i64 = c.iter().map(|s| *s as i64).sum();
                        sum as f32 / (channels as f32 * i32::MAX as f32)
                    })
                    .collect()
            }
            (other_bits, other_fmt) => {
                return Err(TranscriptionLocalError::AudioInvalid(format!(
                    "unsupported WAV format: bits={other_bits} fmt={other_fmt:?}"
                )));
            }
        };

        // Resample to 16 kHz via linear interpolation.
        const TARGET_SR: u32 = 16_000;
        if sample_rate == TARGET_SR {
            return Ok((mono_f32, TARGET_SR));
        }
        let ratio = sample_rate as f64 / TARGET_SR as f64;
        let target_len = (mono_f32.len() as f64 / ratio).round() as usize;
        let mut out = Vec::with_capacity(target_len);
        for i in 0..target_len {
            let pos = i as f64 * ratio;
            let lo = pos.floor() as usize;
            let hi = (lo + 1).min(mono_f32.len().saturating_sub(1));
            let frac = (pos - lo as f64) as f32;
            let sample = mono_f32[lo] * (1.0 - frac) + mono_f32[hi] * frac;
            out.push(sample);
        }
        Ok((out, TARGET_SR))
    }

    fn is_present(model: WhisperModel) -> bool {
        model_path(model)
            .ok()
            .and_then(|p| std::fs::metadata(p).ok())
            .is_some()
    }

    /// Pick the active model. Explicit arg wins, then persisted
    /// `voice_local.model`, then the smallest downloaded model.
    /// Returns `ModelNotFound` when nothing is on disk.
    async fn pick_model(
        state: &tauri::State<'_, crate::commands::AppState>,
        model_arg: Option<String>,
    ) -> Result<WhisperModel, TranscriptionLocalError> {
        if let Some(slug) = model_arg
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let m = WhisperModel::from_slug(slug)
                .ok_or_else(|| TranscriptionLocalError::LanguageUnsupported(slug.to_string()))?;
            if !is_present(m) {
                return Err(TranscriptionLocalError::ModelNotFound);
            }
            return Ok(m);
        }
        let cfg: VoiceLocalConfig = {
            let dc = state.desktop_config.read().await;
            dc.voice_local.clone()
        };
        if let Some(slug) = cfg.model.as_deref().filter(|s| !s.is_empty()) {
            if let Some(m) = WhisperModel::from_slug(slug) {
                if is_present(m) {
                    return Ok(m);
                }
            }
        }
        for m in WhisperModel::ALL {
            if is_present(*m) {
                return Ok(*m);
            }
        }
        Err(TranscriptionLocalError::ModelNotFound)
    }

    /// Tauri command — entry point. Picks (or downloads) the
    /// model, runs whisper-rs inference, returns the concatenated
    /// transcript. The `#[tauri::command]` decoration lives on the
    /// outer wrapper above so the macro's `__cmd__` helper is
    /// emitted in `commands_voice` (where Tauri's `generate_handler!`
    /// looks for it); this inner function is plain `pub async fn`
    /// because `tauri::command` can't be re-exported across modules.
    pub async fn transcribe_audio_local_impl(
        state: tauri::State<'_, crate::commands::AppState>,
        app_handle: tauri::AppHandle,
        audio_path: PathBuf,
        model: Option<String>,
        language: Option<String>,
    ) -> Result<TranscriptionResult, String> {
        let cfg: VoiceLocalConfig = {
            let dc = state.desktop_config.read().await;
            dc.voice_local.clone()
        };
        if !cfg.enabled {
            return Err("STT_NOT_CONFIGURED: enable local voice in Settings → Voice".to_string());
        }

        // 1. Pick a model.
        let chosen = match pick_model(&state, model).await {
            Ok(m) => m,
            Err(TranscriptionLocalError::ModelNotFound) if cfg.auto_download => {
                let prefer = cfg
                    .model
                    .as_deref()
                    .and_then(WhisperModel::from_slug)
                    .unwrap_or(WhisperModel::Base);
                download_model_impl(&app_handle, prefer).await?;
                // After the download, the model is on disk — re-resolve.
                if !is_present(prefer) {
                    return Err(TranscriptionLocalError::ModelLoading(
                        "download completed but file is missing".into(),
                    )
                    .to_wire());
                }
                prefer
            }
            Err(e) => return Err(e.to_wire()),
        };

        // 2. Load + resample audio.
        let (pcm, _sample_rate) = load_wav_as_pcm_f32(&audio_path).map_err(|e| e.to_wire())?;
        if pcm.is_empty() {
            return Err(TranscriptionLocalError::AudioInvalid("empty audio".into()).to_wire());
        }

        // 3. whisper-rs inference. The context creation is
        // expensive (loads + validates the model file), so we
        // construct it once per call. A future cache lives in
        // AppState; out of scope for P2-5e.
        let path =
            model_path(chosen).map_err(|e| TranscriptionLocalError::ModelLoading(e).to_wire())?;
        let path_str = path
            .to_str()
            .ok_or_else(|| TranscriptionLocalError::ModelLoading("non-UTF8 model path".into()))
            .map_err(|e| e.to_wire())?;
        let ctx = whisper_rs::WhisperContext::new_with_params(path_str, Default::default())
            .map_err(|e| {
                TranscriptionLocalError::ModelLoading(format!("whisper context: {e}")).to_wire()
            })?;
        let mut state_whisper = ctx.create_state().map_err(|e| {
            TranscriptionLocalError::InferenceFailed(format!("whisper state: {e}")).to_wire()
        })?;

        let mut params =
            whisper_rs::FullParams::new(whisper_rs::SamplingStrategy::Greedy { best_of: 1 });
        // Translate=false: keep the transcript in the source
        // language. `translate=true` would force English output
        // even for zh-CN input — surprising for the user.
        params.set_translate(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        if let Some(lang) = language.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            params.set_language(Some(lang));
        }

        state_whisper
            .full(params, &pcm)
            .map_err(|e| TranscriptionLocalError::InferenceFailed(e.to_string()).to_wire())?;

        // 4. Concat segments.
        let n = state_whisper
            .full_n_segments()
            .map_err(|e| TranscriptionLocalError::InferenceFailed(e.to_string()).to_wire())?;
        let mut text = String::new();
        for i in 0..n {
            match state_whisper.full_get_segment_text(i) {
                Ok(s) => {
                    if !text.is_empty() && !s.starts_with(' ') {
                        text.push(' ');
                    }
                    text.push_str(&s);
                }
                Err(_) => break, // shouldn't happen mid-iteration
            }
        }
        let text = text.trim().to_string();
        if text.is_empty() {
            return Err(
                TranscriptionLocalError::InferenceFailed("empty transcript".into()).to_wire(),
            );
        }

        // Best-effort: clean up the temp file the frontend
        // created. We don't want to leave a 5 MB .wav in tmp/
        // after every recording.
        let _ = std::fs::remove_file(&audio_path);

        Ok(TranscriptionResult { text })
    }
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
        assert_eq!(
            parse_transcription(r#"{"text":"hello world"}"#),
            "hello world"
        );
        assert_eq!(
            parse_transcription("bare text response"),
            "bare text response"
        );
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
        assert_eq!(
            sanitize_error_body("\n  line1\nline2\tend \r\n"),
            "line1 line2 end"
        );
        let long = "x".repeat(500);
        let out = sanitize_error_body(&long);
        assert!(out.ends_with('…'));
        assert_eq!(out.chars().count(), 201); // 200 chars + ellipsis
    }
}
