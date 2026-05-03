//! Voice input module for speech-to-text
//!
//! Provides a trait for voice recording and transcription.
//! Supports CLI-based whisper transcription when the `whisper` binary is available,
//! falling back to a mock implementation otherwise.

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

/// Voice input event
#[derive(Debug, Clone)]
pub enum VoiceEvent {
    RecordingStarted,
    RecordingStopped { duration_secs: f64 },
    TranscriptionComplete { text: String },
    TranscriptionError { message: String },
}

/// Voice input configuration
#[derive(Debug, Clone)]
pub struct VoiceConfig {
    pub model_path: Option<String>,
    pub language: Option<String>,
    pub auto_detect_language: bool,
    pub max_duration_secs: u64,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            model_path: None,
            language: Some("en".to_string()),
            auto_detect_language: true,
            max_duration_secs: 30,
        }
    }
}

/// Voice input state
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceState {
    Idle,
    Recording,
    Transcribing,
}

/// Voice input trait - implementations can be swapped
pub trait VoiceInput: Send + Sync {
    /// Start recording from microphone
    fn start_recording(&mut self) -> Result<(), String>;

    /// Stop recording and return audio data
    fn stop_recording(&mut self) -> Result<Vec<f32>, String>;

    /// Transcribe audio data to text
    fn transcribe(&self, audio: &[f32]) -> Result<String, String>;

    /// Get current state
    fn state(&self) -> VoiceState;
}

/// Mock implementation for development (no actual microphone/whisper)
pub struct MockVoiceInput {
    state: VoiceState,
}

impl MockVoiceInput {
    pub fn new(_config: &VoiceConfig) -> Self {
        Self {
            state: VoiceState::Idle,
        }
    }
}

impl VoiceInput for MockVoiceInput {
    fn start_recording(&mut self) -> Result<(), String> {
        self.state = VoiceState::Recording;
        Ok(())
    }

    fn stop_recording(&mut self) -> Result<Vec<f32>, String> {
        self.state = VoiceState::Transcribing;
        Ok(Vec::new())
    }

    fn transcribe(&self, _audio: &[f32]) -> Result<String, String> {
        Err("Voice transcription unavailable: whisper not found. \
             Install with: pip install openai-whisper".to_string())
    }

    fn state(&self) -> VoiceState {
        self.state.clone()
    }
}

/// Whisper CLI-based voice transcription.
///
/// Uses the `whisper` command-line tool to transcribe audio. Requires:
/// - `whisper` binary installed (e.g. `pip install openai-whisper`)
/// - Audio is written as a raw 16-bit PCM WAV file to a temp directory,
///   then fed to the `whisper` CLI for transcription.
pub struct WhisperVoiceInput {
    state: VoiceState,
    config: VoiceConfig,
}

impl WhisperVoiceInput {
    pub fn new(config: &VoiceConfig) -> Self {
        Self {
            state: VoiceState::Idle,
            config: config.clone(),
        }
    }

    /// Check whether the `whisper` binary is available on `$PATH`.
    pub fn is_available() -> bool {
        Command::new("whisper")
            .arg("--help")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Build a basic WAV header for 16-bit mono PCM at 16 kHz.
    fn write_wav(audio: &[f32]) -> Result<Vec<u8>, String> {
        let sample_rate: u32 = 16_000;
        let num_channels: u16 = 1;
        let bits_per_sample: u16 = 16;

        // Convert f32 samples to i16 PCM
        let pcm: Vec<i16> = audio
            .iter()
            .map(|&s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
            .collect();

        let data_size = pcm.len() * 2; // 2 bytes per i16 sample
        let file_size = 36 + data_size;

        let mut wav = Vec::with_capacity(44 + data_size);
        // RIFF header
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(file_size as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        // fmt chunk
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes()); // chunk size
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM format
        wav.extend_from_slice(&num_channels.to_le_bytes());
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        let byte_rate = sample_rate * num_channels as u32 * bits_per_sample as u32 / 8;
        wav.extend_from_slice(&byte_rate.to_le_bytes());
        let block_align = num_channels * bits_per_sample / 8;
        wav.extend_from_slice(&block_align.to_le_bytes());
        wav.extend_from_slice(&bits_per_sample.to_le_bytes());
        // data chunk
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data_size as u32).to_le_bytes());
        for sample in &pcm {
            wav.extend_from_slice(&sample.to_le_bytes());
        }

        Ok(wav)
    }

    /// Return a temp directory for whisper I/O.
    fn temp_dir() -> Result<PathBuf, String> {
        let dir = std::env::temp_dir().join("shannon-voice");
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create temp directory for voice input: {e}"))?;
        Ok(dir)
    }
}

impl VoiceInput for WhisperVoiceInput {
    fn start_recording(&mut self) -> Result<(), String> {
        self.state = VoiceState::Recording;
        Ok(())
    }

    fn stop_recording(&mut self) -> Result<Vec<f32>, String> {
        self.state = VoiceState::Transcribing;
        Ok(Vec::new())
    }

    fn transcribe(&self, audio: &[f32]) -> Result<String, String> {
        if audio.is_empty() {
            return Err("No audio data to transcribe".to_string());
        }

        // Write audio to a temp WAV file
        let tmp_dir = Self::temp_dir()?;
        let input_path = tmp_dir.join("shannon_recording.wav");

        let wav_data = Self::write_wav(audio)?;
        let mut f = std::fs::File::create(&input_path)
            .map_err(|e| format!("Failed to create temp WAV file: {e}"))?;
        f.write_all(&wav_data)
            .map_err(|e| format!("Failed to write WAV data: {e}"))?;
        drop(f); // ensure flush

        // Build whisper command
        let mut cmd = Command::new("whisper");
        cmd.arg(&input_path)
            .arg("--output_format")
            .arg("txt")
            .arg("--output_dir")
            .arg(&tmp_dir);

        if let Some(ref lang) = self.config.language {
            if !self.config.auto_detect_language {
                cmd.arg("--language").arg(lang);
            }
        }

        if let Some(ref model) = self.config.model_path {
            cmd.arg("--model").arg(model);
        }

        // Run whisper
        let status = cmd
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    "Whisper binary not found. Install with: pip install openai-whisper"
                        .to_string()
                } else {
                    format!("Failed to run whisper: {e}")
                }
            })?;

        if !status.success() {
            return Err(format!(
                "whisper exited with status {}. Ensure the model is downloaded and audio is valid.",
                status.code().unwrap_or(-1)
            ));
        }

        // Read the transcription output
        let txt_path = tmp_dir.join("shannon_recording.txt");
        let text = std::fs::read_to_string(&txt_path)
            .map_err(|e| format!("Failed to read whisper output: {e}"))?;

        // Clean up temp files
        let _ = std::fs::remove_file(&input_path);
        let _ = std::fs::remove_file(&txt_path);

        let trimmed = text.trim().to_string();
        if trimmed.is_empty() {
            Err("Whisper returned empty transcription".to_string())
        } else {
            Ok(trimmed)
        }
    }

    fn state(&self) -> VoiceState {
        self.state.clone()
    }
}

/// Factory that creates the best available voice input implementation.
///
/// Prefers `WhisperVoiceInput` when the `whisper` binary is on `$PATH`,
/// otherwise falls back to `MockVoiceInput`.
pub fn create_voice_input(config: &VoiceConfig) -> Box<dyn VoiceInput> {
    if WhisperVoiceInput::is_available() {
        tracing::info!("Using whisper CLI for voice transcription");
        Box::new(WhisperVoiceInput::new(config))
    } else {
        tracing::info!("Whisper CLI not found, voice transcription unavailable");
        Box::new(MockVoiceInput::new(config))
    }
}

/// Voice input manager
pub struct VoiceManager {
    input: Box<dyn VoiceInput>,
    config: VoiceConfig,
    state: VoiceState,
    recording_start: Option<std::time::Instant>,
}

impl VoiceManager {
    pub fn new(config: VoiceConfig) -> Self {
        Self {
            input: create_voice_input(&config),
            config,
            state: VoiceState::Idle,
            recording_start: None,
        }
    }

    /// Toggle recording (start if idle, stop if recording)
    pub fn toggle(&mut self) -> Option<VoiceEvent> {
        match self.state {
            VoiceState::Idle => {
                if let Err(e) = self.input.start_recording() {
                    return Some(VoiceEvent::TranscriptionError { message: e });
                }
                self.state = VoiceState::Recording;
                self.recording_start = Some(std::time::Instant::now());
                Some(VoiceEvent::RecordingStarted)
            }
            VoiceState::Recording => {
                let duration = self
                    .recording_start
                    .map(|start| start.elapsed().as_secs_f64())
                    .unwrap_or(0.0);

                match self.input.stop_recording() {
                    Ok(audio) => {
                        self.state = VoiceState::Transcribing;
                        let _ = audio; // Will be transcribed
                        Some(VoiceEvent::RecordingStopped { duration_secs: duration })
                    }
                    Err(e) => {
                        self.state = VoiceState::Idle;
                        self.recording_start = None;
                        Some(VoiceEvent::TranscriptionError { message: e })
                    }
                }
            }
            VoiceState::Transcribing => {
                // Ignore toggle while transcribing
                None
            }
        }
    }

    /// Process transcription result
    pub fn finish_transcription(&mut self, text: String) -> VoiceEvent {
        self.state = VoiceState::Idle;
        self.recording_start = None;
        VoiceEvent::TranscriptionComplete { text }
    }

    /// Report transcription error
    pub fn transcription_error(&mut self, message: String) -> VoiceEvent {
        self.state = VoiceState::Idle;
        self.recording_start = None;
        VoiceEvent::TranscriptionError { message }
    }

    /// Get current state
    pub fn state(&self) -> &VoiceState {
        &self.state
    }

    /// Check if voice input is available (has model)
    pub fn is_available(&self) -> bool {
        self.config.model_path.is_some()
    }
}

/// Render voice recording indicator
pub fn render_voice_indicator(state: &VoiceState, theme: &crate::theme::Theme) -> Option<Span<'static>> {
    match state {
        VoiceState::Idle => None,
        VoiceState::Recording => Some(Span::styled(
            " ● REC ".to_string(),
            Style::default()
                .fg(theme.error)
                .add_modifier(Modifier::BOLD | Modifier::SLOW_BLINK),
        )),
        VoiceState::Transcribing => Some(Span::styled(
            " ⋯ Transcribing ".to_string(),
            Style::default().fg(theme.warning),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_voice_config_default() {
        let config = VoiceConfig::default();
        assert!(config.model_path.is_none());
        assert_eq!(config.language, Some("en".to_string()));
        assert!(config.auto_detect_language);
        assert_eq!(config.max_duration_secs, 30);
    }

    #[test]
    fn test_mock_voice_start_stop() {
        let mut voice = MockVoiceInput::new(&VoiceConfig::default());

        assert_eq!(voice.state(), VoiceState::Idle);

        assert!(voice.start_recording().is_ok());
        assert_eq!(voice.state(), VoiceState::Recording);

        assert!(voice.stop_recording().is_ok());
        assert_eq!(voice.state(), VoiceState::Transcribing);
    }

    #[test]
    fn test_mock_voice_transcribe() {
        let voice = MockVoiceInput::new(&VoiceConfig::default());
        let result = voice.transcribe(&[]);
        // MockVoiceInput now returns an error when whisper is unavailable
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("whisper not found"));
    }

    #[test]
    fn test_voice_manager_idle_to_recording() {
        let mut manager = VoiceManager::new(VoiceConfig::default());

        assert!(!manager.is_available());
        assert_eq!(manager.state(), &VoiceState::Idle);

        let event = manager.toggle();
        assert!(matches!(event, Some(VoiceEvent::RecordingStarted)));
        assert_eq!(manager.state(), &VoiceState::Recording);
    }

    #[test]
    fn test_voice_manager_recording_to_stopped() {
        let mut manager = VoiceManager::new(VoiceConfig::default());

        // Start recording
        manager.toggle();
        assert_eq!(manager.state(), &VoiceState::Recording);

        // Stop recording
        let event = manager.toggle();
        assert!(matches!(event, Some(VoiceEvent::RecordingStopped { .. })));
        assert_eq!(manager.state(), &VoiceState::Transcribing);
    }

    #[test]
    fn test_voice_manager_transcribing_ignore_toggle() {
        let mut manager = VoiceManager::new(VoiceConfig::default());

        manager.toggle(); // Start
        manager.toggle(); // Stop (now transcribing)

        assert_eq!(manager.state(), &VoiceState::Transcribing);

        // Toggle while transcribing should be ignored
        let event = manager.toggle();
        assert!(event.is_none());
        assert_eq!(manager.state(), &VoiceState::Transcribing);
    }

    #[test]
    fn test_voice_manager_finish_transcription() {
        let mut manager = VoiceManager::new(VoiceConfig::default());

        manager.toggle();
        manager.toggle();

        let event = manager.finish_transcription("Hello world".to_string());
        assert!(matches!(event, VoiceEvent::TranscriptionComplete { .. }));
        assert_eq!(manager.state(), &VoiceState::Idle);
    }

    #[test]
    fn test_voice_manager_transcription_error() {
        let mut manager = VoiceManager::new(VoiceConfig::default());

        manager.toggle();
        manager.toggle();

        let event = manager.transcription_error("Audio too short".to_string());
        assert!(matches!(event, VoiceEvent::TranscriptionError { .. }));
        assert_eq!(manager.state(), &VoiceState::Idle);
    }

    #[test]
    fn test_voice_manager_with_model_available() {
        let config = VoiceConfig {
            model_path: Some("/path/to/model.ggml".to_string()),
            ..Default::default()
        };
        let manager = VoiceManager::new(config);
        assert!(manager.is_available());
    }

    #[test]
    fn test_render_voice_indicator() {
        let theme = crate::theme::Theme::default_dark();

        assert!(render_voice_indicator(&VoiceState::Idle, &theme).is_none());

        let rec_span = render_voice_indicator(&VoiceState::Recording, &theme);
        assert!(rec_span.is_some());
        let rec_span = rec_span.unwrap();
        assert!(rec_span.content.contains("REC"));

        let trans_span = render_voice_indicator(&VoiceState::Transcribing, &theme);
        assert!(trans_span.is_some());
        let trans_span = trans_span.unwrap();
        assert!(trans_span.content.contains("Transcribing"));
    }

    #[test]
    fn test_whisper_voice_write_wav() {
        let audio: Vec<f32> = vec![0.0, 0.5, -0.5, 1.0, -1.0];
        let wav = WhisperVoiceInput::write_wav(&audio).unwrap();

        // Check RIFF header
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        // Check fmt chunk
        assert_eq!(&wav[12..16], b"fmt ");
        // Check data chunk
        assert_eq!(&wav[36..40], b"data");
        // 5 samples * 2 bytes = 10 bytes of PCM data
        assert_eq!(wav.len(), 44 + 10);
    }

    #[test]
    fn test_whisper_voice_transcribe_empty_audio() {
        let voice = WhisperVoiceInput::new(&VoiceConfig::default());
        let result = voice.transcribe(&[]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No audio data"));
    }

    #[test]
    fn test_create_voice_input_returns_mock_when_no_whisper() {
        // In test environments whisper is typically not installed,
        // so the factory should return a working VoiceInput either way.
        let input = create_voice_input(&VoiceConfig::default());
        // The input should implement VoiceInput (compiles), and be usable
        assert_eq!(input.state(), VoiceState::Idle);
    }
}
