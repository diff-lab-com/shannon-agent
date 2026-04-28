//! Voice input module for speech-to-text
//!
//! Provides a trait for voice recording and transcription.
//! Currently has a mock implementation. Real implementation requires whisper-rs.

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
        Ok("[Voice transcription placeholder - install whisper model for real usage]".to_string())
    }

    fn state(&self) -> VoiceState {
        self.state.clone()
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
            input: Box::new(MockVoiceInput::new(&config)),
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
        assert!(result.is_ok());
        assert!(result.unwrap().contains("placeholder"));
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
}
