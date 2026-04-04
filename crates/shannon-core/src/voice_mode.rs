//! # Voice Mode
//!
//! Voice input/output management for Shannon Code. Provides a service layer for
//! speech-to-text transcription, voice session management, and keyword spotting
//! for wake words and command shortcuts.
//!
//! ## Architecture
//!
//! - [`VoiceModeService`]: Top-level orchestrator for voice interactions
//! - [`VoiceConfig`]: Audio and language configuration
//! - [`VoiceCommand`]: Commands sent to the voice service
//! - [`VoiceSession`]: Tracks a single voice interaction session
//! - [`TranscriptionResult`]: Output of a speech-to-text pass
//! - [`VoiceStatus`]: Current lifecycle state of the voice service
//! - [`KeywordSpotter`]: Detects wake words and command keywords in text
//!
//! No real audio I/O is performed -- the service layer works with text input
//! that represents what a transcription backend would produce. This keeps the
//! module testable without hardware dependencies.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// Errors that can occur during voice mode operations.
#[derive(Debug, Error)]
pub enum VoiceError {
    /// The voice service is not enabled in configuration.
    #[error("Voice mode is not enabled")]
    NotEnabled,

    /// The service is in the wrong state for the requested operation.
    #[error("Invalid voice state: expected {expected}, got {actual}")]
    InvalidState {
        expected: &'static str,
        actual: &'static str,
    },

    /// A transcription backend error occurred.
    #[error("Transcription failed: {0}")]
    TranscriptionFailed(String),

    /// The keyword spotter rejected the input.
    #[error("Keyword spotting error: {0}")]
    KeywordError(String),

    /// No active session exists.
    #[error("No active voice session")]
    NoSession,

    /// An I/O or platform error.
    #[error("Voice I/O error: {0}")]
    IoError(String),
}

// ---------------------------------------------------------------------------
// VoiceConfig
// ---------------------------------------------------------------------------

/// Audio and language configuration for the voice service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceConfig {
    /// Whether voice mode is enabled.
    pub enabled: bool,

    /// Language code (e.g. "en-US", "es-ES").
    pub language: String,

    /// Audio sample rate in Hz.
    pub sample_rate: u32,

    /// Number of audio channels (1 = mono, 2 = stereo).
    pub channels: u8,

    /// Minimum confidence threshold (0.0 -- 1.0) for accepting transcriptions.
    pub confidence_threshold: f32,

    /// Maximum duration in milliseconds for a single listening segment.
    pub max_segment_duration_ms: u64,

    /// Whether to automatically append a space between consecutive transcriptions.
    pub auto_append_space: bool,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            language: "en-US".to_string(),
            sample_rate: 16_000,
            channels: 1,
            confidence_threshold: 0.7,
            max_segment_duration_ms: 30_000,
            auto_append_space: true,
        }
    }
}

impl VoiceConfig {
    /// Create a new config with the given language.
    pub fn with_language(language: impl Into<String>) -> Self {
        Self {
            language: language.into(),
            ..Default::default()
        }
    }

    /// Enable voice mode.
    pub fn enabled(mut self) -> Self {
        self.enabled = true;
        self
    }

    /// Set the confidence threshold.
    pub fn confidence_threshold(mut self, threshold: f32) -> Self {
        self.confidence_threshold = threshold.clamp(0.0, 1.0);
        self
    }
}

// ---------------------------------------------------------------------------
// VoiceStatus
// ---------------------------------------------------------------------------

/// Current lifecycle state of the voice service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VoiceStatus {
    /// No active listening session.
    Idle,
    /// Currently capturing audio / waiting for transcription.
    Listening,
    /// Transcription is being processed.
    Processing,
    /// An error occurred during the last operation.
    Error,
}

impl std::fmt::Display for VoiceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "idle"),
            Self::Listening => write!(f, "listening"),
            Self::Processing => write!(f, "processing"),
            Self::Error => write!(f, "error"),
        }
    }
}

// ---------------------------------------------------------------------------
// TranscriptionResult
// ---------------------------------------------------------------------------

/// Output of a speech-to-text transcription pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionResult {
    /// The recognised text.
    pub text: String,

    /// Confidence score between 0.0 and 1.0.
    pub confidence: f32,

    /// Detected language code.
    pub language: String,

    /// Duration of the audio segment in milliseconds.
    pub duration_ms: u64,

    /// Whether this is a final (non-interim) result.
    pub is_final: bool,
}

impl TranscriptionResult {
    /// Create a new transcription result.
    pub fn new(text: impl Into<String>, confidence: f32, language: impl Into<String>, duration_ms: u64) -> Self {
        Self {
            text: text.into(),
            confidence: confidence.clamp(0.0, 1.0),
            language: language.into(),
            duration_ms,
            is_final: true,
        }
    }

    /// Mark this result as interim (partial).
    pub fn interim(mut self) -> Self {
        self.is_final = false;
        self
    }

    /// Whether the result meets the given confidence threshold.
    pub fn meets_threshold(&self, threshold: f32) -> bool {
        self.confidence >= threshold
    }
}

// ---------------------------------------------------------------------------
// VoiceCommand
// ---------------------------------------------------------------------------

/// Commands that can be sent to the voice service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VoiceCommand {
    /// Begin capturing audio and transcribing.
    StartListening,

    /// Stop capturing and return accumulated transcription.
    StopListening,

    /// Cancel the current session, discarding results.
    Cancel,

    /// Change the transcription language at runtime.
    SetLanguage { language: String },

    /// Query the current status of the voice service.
    GetStatus,
}

// ---------------------------------------------------------------------------
// VoiceSession
// ---------------------------------------------------------------------------

/// A single voice interaction session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceSession {
    /// Unique session identifier.
    pub id: Uuid,

    /// Current status of this session.
    pub status: VoiceStatus,

    /// When the session was created.
    pub started_at: DateTime<Utc>,

    /// Accumulated transcriptions for this session.
    pub transcriptions: Vec<TranscriptionResult>,

    /// Configuration that was active when the session started.
    pub config: VoiceConfig,

    /// Combined text from all final transcriptions.
    pub combined_text: String,
}

impl VoiceSession {
    /// Create a new voice session.
    pub fn new(config: VoiceConfig) -> Self {
        Self {
            id: Uuid::new_v4(),
            status: VoiceStatus::Idle,
            started_at: Utc::now(),
            transcriptions: Vec::new(),
            config,
            combined_text: String::new(),
        }
    }

    /// Add a transcription result and update combined text.
    pub fn add_transcription(&mut self, result: TranscriptionResult) {
        if result.is_final {
            if !self.combined_text.is_empty() && self.config.auto_append_space {
                self.combined_text.push(' ');
            }
            self.combined_text.push_str(&result.text);
        }
        self.transcriptions.push(result);
    }

    /// Reset the combined text and transcriptions, keeping the session alive.
    pub fn reset_transcriptions(&mut self) {
        self.transcriptions.clear();
        self.combined_text.clear();
    }

    /// Duration of this session in seconds.
    pub fn duration_secs(&self) -> f64 {
        Utc::now()
            .signed_duration_since(self.started_at)
            .num_milliseconds() as f64
            / 1000.0
    }

    /// Count of final transcriptions.
    pub fn final_transcription_count(&self) -> usize {
        self.transcriptions.iter().filter(|t| t.is_final).count()
    }
}

// ---------------------------------------------------------------------------
// KeywordSpotter
// ---------------------------------------------------------------------------

/// Detects wake words and command keywords in transcribed text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeywordSpotter {
    /// Wake words that activate the voice assistant (case-insensitive).
    wake_words: Vec<String>,

    /// Command keywords mapped to their action identifiers.
    command_keywords: HashMap<String, String>,

    /// Whether keyword spotting is enabled.
    enabled: bool,
}

impl Default for KeywordSpotter {
    fn default() -> Self {
        let mut command_keywords = HashMap::new();
        command_keywords.insert("stop".to_string(), "stop_listening".to_string());
        command_keywords.insert("cancel".to_string(), "cancel".to_string());
        command_keywords.insert("clear".to_string(), "clear".to_string());
        command_keywords.insert("help".to_string(), "show_help".to_string());
        command_keywords.insert("undo".to_string(), "undo_last".to_string());
        command_keywords.insert("submit".to_string(), "submit".to_string());
        command_keywords.insert("new line".to_string(), "insert_newline".to_string());
        command_keywords.insert("newline".to_string(), "insert_newline".to_string());
        command_keywords.insert("delete last".to_string(), "delete_last_word".to_string());

        Self {
            wake_words: vec!["hey shannon".to_string(), "shannon".to_string()],
            command_keywords,
            enabled: true,
        }
    }
}

impl KeywordSpotter {
    /// Create a new keyword spotter with default wake words and commands.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a wake word.
    pub fn add_wake_word(&mut self, word: impl Into<String>) {
        self.wake_words.push(word.into());
    }

    /// Add a command keyword that maps to an action.
    pub fn add_command_keyword(&mut self, keyword: impl Into<String>, action: impl Into<String>) {
        self.command_keywords.insert(keyword.into(), action.into());
    }

    /// Enable or disable keyword spotting.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Check if the text contains a wake word.
    ///
    /// Returns the matched wake word (lowercased) or `None`.
    pub fn detect_wake_word(&self, text: &str) -> Option<String> {
        if !self.enabled {
            return None;
        }
        let lower = text.to_lowercase();
        for wake in &self.wake_words {
            if lower.contains(&wake.to_lowercase()) {
                return Some(wake.to_lowercase());
            }
        }
        None
    }

    /// Check if the text contains any command keyword.
    ///
    /// Returns the matched action identifier or `None`.
    pub fn detect_command(&self, text: &str) -> Option<String> {
        if !self.enabled {
            return None;
        }
        let lower = text.to_lowercase();
        for (keyword, action) in &self.command_keywords {
            if lower.contains(&keyword.to_lowercase()) {
                return Some(action.clone());
            }
        }
        None
    }

    /// Strip wake words from the given text.
    pub fn strip_wake_words(&self, text: &str) -> String {
        let mut result = text.to_string();
        for wake in &self.wake_words {
            let wake_lower = wake.to_lowercase();
            let lower = result.to_lowercase();
            if let Some(idx) = lower.find(&wake_lower) {
                let end = idx + wake.len();
                // Remove the wake word and any trailing punctuation/space/comma
                let trimmed = result[end..].trim_start_matches(|c: char| c == ',' || c == ' ');
                result = format!("{}{}", &result[..idx], trimmed);
            }
        }
        result.trim().to_string()
    }

    /// Get all registered wake words.
    pub fn wake_words(&self) -> &[String] {
        &self.wake_words
    }

    /// Get all registered command keywords.
    pub fn command_keywords(&self) -> &HashMap<String, String> {
        &self.command_keywords
    }
}

// ---------------------------------------------------------------------------
// VoiceModeService
// ---------------------------------------------------------------------------

/// Top-level service for voice input/output management.
///
/// Manages voice sessions, processes voice commands, and coordinates between
/// the transcription backend and the keyword spotter.
pub struct VoiceModeService {
    /// Global voice configuration.
    config: VoiceConfig,

    /// Current active session, if any.
    session: Option<VoiceSession>,

    /// Keyword spotter for wake word / command detection.
    keyword_spotter: KeywordSpotter,

    /// Whether the service is currently active (enabled + has resources).
    active: bool,
}

impl VoiceModeService {
    /// Create a new voice mode service with the given configuration.
    pub fn new(config: VoiceConfig) -> Self {
        let active = config.enabled;
        Self {
            config,
            session: None,
            keyword_spotter: KeywordSpotter::new(),
            active,
        }
    }

    /// Create a service with default configuration (disabled).
    pub fn new_default() -> Self {
        Self::new(VoiceConfig::default())
    }

    // -- Configuration -------------------------------------------------------

    /// Get the current configuration.
    pub fn config(&self) -> &VoiceConfig {
        &self.config
    }

    /// Update the configuration.
    pub fn set_config(&mut self, config: VoiceConfig) {
        self.active = config.enabled;
        self.config = config;
    }

    /// Get the keyword spotter (mutable).
    pub fn keyword_spotter_mut(&mut self) -> &mut KeywordSpotter {
        &mut self.keyword_spotter
    }

    /// Get the keyword spotter (immutable).
    pub fn keyword_spotter(&self) -> &KeywordSpotter {
        &self.keyword_spotter
    }

    // -- Status --------------------------------------------------------------

    /// Get the current status of the voice service.
    pub fn status(&self) -> VoiceStatus {
        if !self.active {
            return VoiceStatus::Idle;
        }
        self.session
            .as_ref()
            .map(|s| s.status)
            .unwrap_or(VoiceStatus::Idle)
    }

    /// Whether the service is enabled.
    pub fn is_enabled(&self) -> bool {
        self.active
    }

    /// Whether the service is currently listening.
    pub fn is_listening(&self) -> bool {
        self.active && self.status() == VoiceStatus::Listening
    }

    // -- Command processing --------------------------------------------------

    /// Process a voice command.
    pub fn process_command(&mut self, command: VoiceCommand) -> Result<VoiceCommandResult, VoiceError> {
        if !self.active && !matches!(command, VoiceCommand::GetStatus) {
            return Err(VoiceError::NotEnabled);
        }

        match command {
            VoiceCommand::StartListening => self.start_listening(),
            VoiceCommand::StopListening => self.stop_listening(),
            VoiceCommand::Cancel => self.cancel_session(),
            VoiceCommand::SetLanguage { language } => self.set_language(language),
            VoiceCommand::GetStatus => Ok(VoiceCommandResult::Status {
                status: self.status(),
                session_id: self.session.as_ref().map(|s| s.id),
            }),
        }
    }

    // -- Session management --------------------------------------------------

    /// Start a new listening session (or resume if one already exists).
    fn start_listening(&mut self) -> Result<VoiceCommandResult, VoiceError> {
        if let Some(ref session) = self.session {
            if session.status == VoiceStatus::Listening {
                return Err(VoiceError::InvalidState {
                    expected: "idle or error",
                    actual: "listening",
                });
            }
        }

        let session = self.session.get_or_insert_with(|| VoiceSession::new(self.config.clone()));
        session.status = VoiceStatus::Listening;

        Ok(VoiceCommandResult::Started {
            session_id: session.id,
        })
    }

    /// Stop listening and return accumulated transcription.
    fn stop_listening(&mut self) -> Result<VoiceCommandResult, VoiceError> {
        let session = self.session.as_mut().ok_or(VoiceError::NoSession)?;

        if session.status != VoiceStatus::Listening && session.status != VoiceStatus::Processing {
            return Err(VoiceError::InvalidState {
                expected: "listening or processing",
                actual: match session.status {
                    VoiceStatus::Idle => "idle",
                    VoiceStatus::Error => "error",
                    VoiceStatus::Listening => "listening",
                    VoiceStatus::Processing => "processing",
                },
            });
        }

        session.status = VoiceStatus::Idle;
        let combined = session.combined_text.clone();

        Ok(VoiceCommandResult::Stopped {
            session_id: session.id,
            text: combined,
            transcription_count: session.final_transcription_count(),
        })
    }

    /// Cancel the current session entirely.
    fn cancel_session(&mut self) -> Result<VoiceCommandResult, VoiceError> {
        self.session = None;
        Ok(VoiceCommandResult::Cancelled)
    }

    /// Change the language at runtime.
    fn set_language(&mut self, language: String) -> Result<VoiceCommandResult, VoiceError> {
        self.config.language = language.clone();
        if let Some(ref mut session) = self.session {
            session.config.language = language.clone();
        }
        Ok(VoiceCommandResult::LanguageChanged { language })
    }

    // -- Transcription -------------------------------------------------------

    /// Feed a transcription result into the current session.
    ///
    /// Returns `Some(action)` if the keyword spotter detected a command
    /// keyword in the text.
    pub fn feed_transcription(
        &mut self,
        result: TranscriptionResult,
    ) -> Result<Option<String>, VoiceError> {
        let session = self.session.as_mut().ok_or(VoiceError::NoSession)?;

        if session.status != VoiceStatus::Listening && session.status != VoiceStatus::Processing {
            return Err(VoiceError::InvalidState {
                expected: "listening or processing",
                actual: match session.status {
                    VoiceStatus::Idle => "idle",
                    VoiceStatus::Error => "error",
                    VoiceStatus::Listening => "listening",
                    VoiceStatus::Processing => "processing",
                },
            });
        }

        // Check for command keywords before storing
        let action = self.keyword_spotter.detect_command(&result.text);

        // If a wake word is present, strip it before storing
        let text = if self.keyword_spotter.detect_wake_word(&result.text).is_some() {
            let stripped = self.keyword_spotter.strip_wake_words(&result.text);
            if stripped.is_empty() {
                // Text was only a wake word, skip storing
                return Ok(action);
            }
            TranscriptionResult {
                text: stripped,
                ..result
            }
        } else {
            result
        };

        // Check confidence threshold
        if !text.meets_threshold(self.config.confidence_threshold) && text.is_final {
            return Err(VoiceError::TranscriptionFailed(format!(
                "Confidence {} below threshold {}",
                text.confidence, self.config.confidence_threshold
            )));
        }

        session.add_transcription(text);
        Ok(action)
    }

    /// Get the combined text from the current session.
    pub fn combined_text(&self) -> Result<String, VoiceError> {
        self.session
            .as_ref()
            .map(|s| s.combined_text.clone())
            .ok_or(VoiceError::NoSession)
    }

    /// Get a reference to the current session, if any.
    pub fn current_session(&self) -> Option<&VoiceSession> {
        self.session.as_ref()
    }

    /// Get a mutable reference to the current session, if any.
    pub fn current_session_mut(&mut self) -> Option<&mut VoiceSession> {
        self.session.as_mut()
    }

    /// Manually transition the session to an error state.
    pub fn set_error(&mut self, message: &str) {
        if let Some(ref mut session) = self.session {
            session.status = VoiceStatus::Error;
        }
        tracing::warn!(message, "Voice session error");
    }
}

impl Default for VoiceModeService {
    fn default() -> Self {
        Self::new_default()
    }
}

// ---------------------------------------------------------------------------
// VoiceCommandResult
// ---------------------------------------------------------------------------

/// Result of processing a [`VoiceCommand`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VoiceCommandResult {
    /// Listening started.
    Started { session_id: Uuid },
    /// Listening stopped with accumulated text.
    Stopped {
        session_id: Uuid,
        text: String,
        transcription_count: usize,
    },
    /// Current session was cancelled.
    Cancelled,
    /// Language was changed.
    LanguageChanged { language: String },
    /// Current status was queried.
    Status {
        status: VoiceStatus,
        session_id: Option<Uuid>,
    },
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_voice_config_default() {
        let cfg = VoiceConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.language, "en-US");
        assert_eq!(cfg.sample_rate, 16_000);
        assert_eq!(cfg.channels, 1);
        assert!((cfg.confidence_threshold - 0.7).abs() < 0.001);
        assert_eq!(cfg.max_segment_duration_ms, 30_000);
        assert!(cfg.auto_append_space);
    }

    #[test]
    fn test_voice_config_builder() {
        let cfg = VoiceConfig::with_language("es-ES")
            .enabled()
            .confidence_threshold(0.5);

        assert!(cfg.enabled);
        assert_eq!(cfg.language, "es-ES");
        assert!((cfg.confidence_threshold - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_voice_config_confidence_clamp() {
        let cfg = VoiceConfig::default().confidence_threshold(1.5);
        assert!((cfg.confidence_threshold - 1.0).abs() < 0.001);

        let cfg = VoiceConfig::default().confidence_threshold(-0.5);
        assert!((cfg.confidence_threshold - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_transcription_result() {
        let result = TranscriptionResult::new("hello world", 0.95, "en-US", 1200);
        assert_eq!(result.text, "hello world");
        assert!((result.confidence - 0.95).abs() < 0.001);
        assert!(result.is_final);
        assert!(result.meets_threshold(0.7));
        assert!(!result.meets_threshold(0.99));

        let interim = result.interim();
        assert!(!interim.is_final);
    }

    #[test]
    fn test_voice_session_lifecycle() {
        let cfg = VoiceConfig::default().enabled();
        let mut session = VoiceSession::new(cfg);

        assert_eq!(session.status, VoiceStatus::Idle);
        assert!(session.transcriptions.is_empty());
        assert!(session.combined_text.is_empty());

        session.status = VoiceStatus::Listening;
        assert_eq!(session.status, VoiceStatus::Listening);

        session.add_transcription(TranscriptionResult::new("hello", 0.9, "en", 500));
        session.add_transcription(TranscriptionResult::new("world", 0.85, "en", 400));
        assert_eq!(session.combined_text, "hello world");
        assert_eq!(session.final_transcription_count(), 2);

        session.reset_transcriptions();
        assert!(session.combined_text.is_empty());
        assert!(session.transcriptions.is_empty());
    }

    #[test]
    fn test_voice_session_no_auto_space() {
        let cfg = VoiceConfig {
            auto_append_space: false,
            ..VoiceConfig::default()
        };
        let mut session = VoiceSession::new(cfg);
        session.add_transcription(TranscriptionResult::new("hello", 0.9, "en", 500));
        session.add_transcription(TranscriptionResult::new("world", 0.85, "en", 400));
        assert_eq!(session.combined_text, "helloworld");
    }

    #[test]
    fn test_keyword_spotter_wake_words() {
        let spotter = KeywordSpotter::new();

        assert_eq!(
            spotter.detect_wake_word("Hey Shannon, can you help?"),
            Some("hey shannon".to_string())
        );
        assert_eq!(
            spotter.detect_wake_word("shannon, what time is it?"),
            Some("shannon".to_string())
        );
        assert_eq!(spotter.detect_wake_word("just some regular text"), None);
    }

    #[test]
    fn test_keyword_spotter_commands() {
        let spotter = KeywordSpotter::new();

        assert_eq!(
            spotter.detect_command("please stop listening"),
            Some("stop_listening".to_string())
        );
        assert_eq!(
            spotter.detect_command("cancel that"),
            Some("cancel".to_string())
        );
        assert_eq!(
            spotter.detect_command("help me out"),
            Some("show_help".to_string())
        );
        assert_eq!(spotter.detect_command("regular text here"), None);
    }

    #[test]
    fn test_keyword_spotter_disabled() {
        let mut spotter = KeywordSpotter::new();
        spotter.set_enabled(false);

        assert_eq!(spotter.detect_wake_word("Hey Shannon"), None);
        assert_eq!(spotter.detect_command("stop"), None);
    }

    #[test]
    fn test_keyword_spotter_strip_wake_words() {
        let spotter = KeywordSpotter::new();

        assert_eq!(
            spotter.strip_wake_words("Hey Shannon, write a test"),
            "write a test"
        );
        assert_eq!(
            spotter.strip_wake_words("shannon, hello world"),
            "hello world"
        );
        assert_eq!(
            spotter.strip_wake_words("just some text"),
            "just some text"
        );
    }

    #[test]
    fn test_keyword_spotter_custom_keywords() {
        let mut spotter = KeywordSpotter::new();
        spotter.add_wake_word("computer");
        spotter.add_command_keyword("save", "save_file");

        assert_eq!(
            spotter.detect_wake_word("ok computer, run tests"),
            Some("computer".to_string())
        );
        assert_eq!(
            spotter.detect_command("save the file"),
            Some("save_file".to_string())
        );
    }

    #[test]
    fn test_voice_service_start_stop() {
        let cfg = VoiceConfig::default().enabled();
        let mut service = VoiceModeService::new(cfg);

        // Start listening
        let result = service.process_command(VoiceCommand::StartListening).unwrap();
        match result {
            VoiceCommandResult::Started { session_id } => {
                assert!(service.current_session().is_some());
                assert_eq!(service.current_session().unwrap().id, session_id);
            }
            other => panic!("Expected Started, got {:?}", other),
        }

        // Feed transcription
        service
            .feed_transcription(TranscriptionResult::new("hello", 0.9, "en", 500))
            .unwrap();

        // Stop listening
        let result = service.process_command(VoiceCommand::StopListening).unwrap();
        match result {
            VoiceCommandResult::Stopped { text, .. } => {
                assert_eq!(text, "hello");
            }
            other => panic!("Expected Stopped, got {:?}", other),
        }
    }

    #[test]
    fn test_voice_service_cancel() {
        let cfg = VoiceConfig::default().enabled();
        let mut service = VoiceModeService::new(cfg);

        service.process_command(VoiceCommand::StartListening).unwrap();
        assert!(service.current_session().is_some());

        service.process_command(VoiceCommand::Cancel).unwrap();
        assert!(service.current_session().is_none());
    }

    #[test]
    fn test_voice_service_not_enabled() {
        let cfg = VoiceConfig::default(); // enabled = false
        let mut service = VoiceModeService::new(cfg);

        let err = service
            .process_command(VoiceCommand::StartListening)
            .unwrap_err();
        assert!(matches!(err, VoiceError::NotEnabled));

        // GetStatus should still work
        let result = service.process_command(VoiceCommand::GetStatus).unwrap();
        match result {
            VoiceCommandResult::Status { status, .. } => {
                assert_eq!(status, VoiceStatus::Idle);
            }
            other => panic!("Expected Status, got {:?}", other),
        }
    }

    #[test]
    fn test_voice_service_set_language() {
        let cfg = VoiceConfig::default().enabled();
        let mut service = VoiceModeService::new(cfg);

        let result = service
            .process_command(VoiceCommand::SetLanguage {
                language: "fr-FR".to_string(),
            })
            .unwrap();
        match result {
            VoiceCommandResult::LanguageChanged { language } => {
                assert_eq!(language, "fr-FR");
            }
            other => panic!("Expected LanguageChanged, got {:?}", other),
        }
        assert_eq!(service.config().language, "fr-FR");
    }

    #[test]
    fn test_voice_service_double_start_fails() {
        let cfg = VoiceConfig::default().enabled();
        let mut service = VoiceModeService::new(cfg);

        service.process_command(VoiceCommand::StartListening).unwrap();
        let err = service
            .process_command(VoiceCommand::StartListening)
            .unwrap_err();
        assert!(matches!(err, VoiceError::InvalidState { .. }));
    }

    #[test]
    fn test_voice_service_stop_without_session_fails() {
        let cfg = VoiceConfig::default().enabled();
        let mut service = VoiceModeService::new(cfg);

        let err = service
            .process_command(VoiceCommand::StopListening)
            .unwrap_err();
        assert!(matches!(err, VoiceError::NoSession));
    }

    #[test]
    fn test_voice_service_feed_without_session_fails() {
        let cfg = VoiceConfig::default().enabled();
        let mut service = VoiceModeService::new(cfg);

        let err = service
            .feed_transcription(TranscriptionResult::new("test", 0.9, "en", 100))
            .unwrap_err();
        assert!(matches!(err, VoiceError::NoSession));
    }

    #[test]
    fn test_voice_service_confidence_filter() {
        let cfg = VoiceConfig {
            confidence_threshold: 0.9,
            enabled: true,
            ..VoiceConfig::default()
        };
        let mut service = VoiceModeService::new(cfg);
        service.process_command(VoiceCommand::StartListening).unwrap();

        // Low confidence should fail
        let err = service
            .feed_transcription(TranscriptionResult::new("test", 0.5, "en", 100))
            .unwrap_err();
        assert!(matches!(err, VoiceError::TranscriptionFailed(_)));

        // High confidence should succeed
        service
            .feed_transcription(TranscriptionResult::new("hello", 0.95, "en", 200))
            .unwrap();
        assert_eq!(service.combined_text().unwrap(), "hello");
    }

    #[test]
    fn test_voice_service_command_detection() {
        let cfg = VoiceConfig::default().enabled();
        let mut service = VoiceModeService::new(cfg);
        service.process_command(VoiceCommand::StartListening).unwrap();

        // Feed text with a command keyword
        let action = service
            .feed_transcription(TranscriptionResult::new("please stop now", 0.9, "en", 300))
            .unwrap();
        assert_eq!(action, Some("stop_listening".to_string()));
    }

    #[test]
    fn test_voice_service_wake_word_stripped() {
        let cfg = VoiceConfig::default().enabled();
        let mut service = VoiceModeService::new(cfg);
        service.process_command(VoiceCommand::StartListening).unwrap();

        // Feed text with wake word -- it should be stripped
        service
            .feed_transcription(TranscriptionResult::new("Hey Shannon, run tests", 0.9, "en", 500))
            .unwrap();
        assert_eq!(service.combined_text().unwrap(), "run tests");
    }

    #[test]
    fn test_voice_service_status_tracking() {
        let cfg = VoiceConfig::default().enabled();
        let mut service = VoiceModeService::new(cfg);

        assert_eq!(service.status(), VoiceStatus::Idle);
        assert!(service.is_enabled());
        assert!(!service.is_listening());

        service.process_command(VoiceCommand::StartListening).unwrap();
        assert_eq!(service.status(), VoiceStatus::Listening);
        assert!(service.is_listening());

        service.set_error("test error");
        assert_eq!(service.status(), VoiceStatus::Error);
        assert!(!service.is_listening());
    }

    #[test]
    fn test_voice_status_display() {
        assert_eq!(VoiceStatus::Idle.to_string(), "idle");
        assert_eq!(VoiceStatus::Listening.to_string(), "listening");
        assert_eq!(VoiceStatus::Processing.to_string(), "processing");
        assert_eq!(VoiceStatus::Error.to_string(), "error");
    }

    #[test]
    fn test_voice_status_serialization() {
        let statuses = vec![
            VoiceStatus::Idle,
            VoiceStatus::Listening,
            VoiceStatus::Processing,
            VoiceStatus::Error,
        ];
        for status in statuses {
            let json = serde_json::to_string(&status).unwrap();
            let decoded: VoiceStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, decoded);
        }
    }

    #[test]
    fn test_voice_session_serialization() {
        let cfg = VoiceConfig::default().enabled();
        let mut session = VoiceSession::new(cfg);
        session.status = VoiceStatus::Listening;
        session.add_transcription(TranscriptionResult::new("hello", 0.9, "en", 500));

        let json = serde_json::to_string(&session).unwrap();
        let decoded: VoiceSession = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.id, session.id);
        assert_eq!(decoded.status, VoiceStatus::Listening);
        assert_eq!(decoded.combined_text, "hello");
    }

    #[test]
    fn test_interim_transcription_not_combined() {
        let cfg = VoiceConfig::default().enabled();
        let mut session = VoiceSession::new(cfg);

        session.add_transcription(TranscriptionResult::new("hel", 0.5, "en", 100).interim());
        session.add_transcription(TranscriptionResult::new("hello", 0.9, "en", 200));
        session.add_transcription(TranscriptionResult::new("hello world", 0.92, "en", 400).interim());

        // Only final transcriptions should be in combined text
        assert_eq!(session.combined_text, "hello");
        assert_eq!(session.transcriptions.len(), 3);
        assert_eq!(session.final_transcription_count(), 1);
    }
}
