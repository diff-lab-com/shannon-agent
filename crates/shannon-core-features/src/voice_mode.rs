//! Voice mode service

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Voice configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceConfig {
    pub enabled: bool,
    pub language: String,
    pub voice_gender: String,
    pub auto_detect_language: bool,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            language: "en".to_string(),
            voice_gender: "neutral".to_string(),
            auto_detect_language: true,
        }
    }
}

/// Voice state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VoiceState {
    Idle,
    Listening,
    Processing,
    Speaking,
}

/// Voice mode service
pub struct VoiceModeService {
    config: VoiceConfig,
    state: VoiceState,
}

impl VoiceModeService {
    pub fn new(config: VoiceConfig) -> Self {
        Self {
            config,
            state: VoiceState::Idle,
        }
    }

    /// Start listening
    pub async fn start_listening(&mut self) -> Result<(), VoiceError> {
        if !self.config.enabled {
            return Err(VoiceError::Disabled);
        }

        self.state = VoiceState::Listening;
        Ok(())
    }

    /// Stop listening
    pub async fn stop_listening(&mut self) -> Result<(), VoiceError> {
        self.state = VoiceState::Idle;
        Ok(())
    }

    /// Process voice input
    pub async fn process_input(&mut self, audio_data: Vec<u8>) -> Result<String, VoiceError> {
        self.state = VoiceState::Processing;

        // TODO: Actual speech-to-text processing
        // For now, return a placeholder
        self.state = VoiceState::Idle;

        Ok(String::new())
    }

    /// Get current state
    pub fn state(&self) -> VoiceState {
        self.state
    }

    /// Get config
    pub fn config(&self) -> &VoiceConfig {
        &self.config
    }

    /// Update config
    pub fn update_config(&mut self, config: VoiceConfig) {
        self.config = config;
    }
}

/// Voice errors
#[derive(Debug, thiserror::Error)]
pub enum VoiceError {
    #[error("Voice mode is disabled")]
    Disabled,

    #[error("Microphone not available")]
    MicrophoneUnavailable,

    #[error("Processing failed: {0}")]
    ProcessingFailed(String),

    #[error("IO error: {0}")]
    IoError(String),
}
