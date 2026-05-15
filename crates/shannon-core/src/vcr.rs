//! # VCR (Virtual Cassette Recorder)
//!
//! API conversation recording for testing. Records HTTP request/response
//! pairs to disk and can replay them during tests to avoid hitting live APIs.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(test)]
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;
use uuid::Uuid;

/// Errors that can occur during VCR operations.
#[derive(Error, Debug)]
pub enum VcrError {
    #[error("Failed to write recording: {0}")]
    WriteFailed(String),

    #[error("Failed to read recording: {0}")]
    ReadFailed(String),

    #[error("Recording not found: {0}")]
    NotFound(String),

    #[error("Recording directory not configured")]
    NoDirectory,

    #[error("VCR is in replay mode; recording is disabled")]
    ReplayModeActive,

    #[error("Invalid recording format: {0}")]
    InvalidFormat(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Configuration for the VCR system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcrConfig {
    /// Whether VCR recording is enabled.
    pub enabled: bool,
    /// Directory where recordings are stored.
    pub record_dir: PathBuf,
    /// If true, replay recorded responses instead of making real API calls.
    pub replay_mode: bool,
    /// Headers to match when looking for a replay recording.
    pub match_headers: Vec<String>,
}

impl Default for VcrConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            record_dir: PathBuf::from("fixtures/vcr"),
            replay_mode: false,
            match_headers: vec![],
        }
    }
}

impl VcrConfig {
    /// Create a new VCR config with the given record directory.
    pub fn with_dir(dir: impl Into<PathBuf>) -> Self {
        Self {
            enabled: true,
            record_dir: dir.into(),
            replay_mode: false,
            match_headers: vec![],
        }
    }

    /// Create a replay-mode config with the given record directory.
    pub fn replay_with_dir(dir: impl Into<PathBuf>) -> Self {
        Self {
            enabled: true,
            record_dir: dir.into(),
            replay_mode: true,
            match_headers: vec![],
        }
    }
}

/// A single recorded API interaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcrRecording {
    /// Unique identifier for this recording.
    pub id: String,
    /// The API request (method, URL, headers, body).
    pub request: Value,
    /// The API response (status, headers, body).
    pub response: Value,
    /// ISO 8601 timestamp of when the recording was made.
    pub timestamp: String,
    /// Tags for categorizing and searching recordings.
    pub tags: Vec<String>,
}

impl VcrRecording {
    /// Create a new recording with the given request, response, and tags.
    pub fn new(request: Value, response: Value, tags: Vec<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            request,
            response,
            timestamp: chrono::Utc::now().to_rfc3339(),
            tags,
        }
    }
}

/// VCR system for recording and replaying API interactions.
///
/// Recordings are stored as individual JSON files in the configured directory,
/// named by their UUID for uniqueness.
pub struct Vcr {
    config: VcrConfig,
    /// In-memory index of recordings for fast lookup.
    index: HashMap<String, VcrRecording>,
}

impl Vcr {
    /// Create a new VCR instance with the given configuration.
    pub fn new(config: VcrConfig) -> Self {
        Self {
            config,
            index: HashMap::new(),
        }
    }

    /// Create a VCR in recording mode with the given directory.
    pub fn for_recording(dir: impl Into<PathBuf>) -> Self {
        Self::new(VcrConfig::with_dir(dir))
    }

    /// Create a VCR in replay mode with the given directory.
    pub fn for_replay(dir: impl Into<PathBuf>) -> Self {
        Self::new(VcrConfig::replay_with_dir(dir))
    }

    /// Record a request/response pair.
    ///
    /// Stores the recording as a JSON file in the record directory
    /// and adds it to the in-memory index.
    pub fn record(
        &mut self,
        request: Value,
        response: Value,
        tags: Vec<String>,
    ) -> Result<(), VcrError> {
        if self.config.replay_mode {
            return Err(VcrError::ReplayModeActive);
        }

        if !self.config.enabled {
            return Ok(());
        }

        // Ensure record directory exists
        fs::create_dir_all(&self.config.record_dir)?;

        let recording = VcrRecording::new(request, response, tags);
        let id = recording.id.clone();

        // Write to disk
        let file_path = self.recording_path(&id);
        let json = serde_json::to_string_pretty(&recording)
            .map_err(|e| VcrError::InvalidFormat(e.to_string()))?;
        fs::write(&file_path, json)?;

        // Add to index
        self.index.insert(id.clone(), recording);

        Ok(())
    }

    /// Find a recording matching a query string.
    ///
    /// The query is matched against request URLs, request bodies, and tags.
    pub fn find_recording(&self, query: &str) -> Option<VcrRecording> {
        let query_lower = query.to_lowercase();

        for recording in self.index.values() {
            // Check tags
            if recording.tags.iter().any(|t| t.to_lowercase().contains(&query_lower)) {
                return Some(recording.clone());
            }

            // Check request URL
            if let Some(url) = recording.request.get("url").and_then(|v| v.as_str()) {
                if url.to_lowercase().contains(&query_lower) {
                    return Some(recording.clone());
                }
            }

            // Check request body
            if let Some(body) = recording.request.get("body").and_then(|v| v.as_str()) {
                if body.to_lowercase().contains(&query_lower) {
                    return Some(recording.clone());
                }
            }

            // Check request path
            if let Some(path) = recording.request.get("path").and_then(|v| v.as_str()) {
                if path.to_lowercase().contains(&query_lower) {
                    return Some(recording.clone());
                }
            }
        }

        None
    }

    /// Replay a response for the given request.
    ///
    /// Searches for a matching recording based on the request and returns
    /// the recorded response if found. Uses match_headers for header matching
    /// when configured.
    pub fn replay(&self, request: Value) -> Option<Value> {
        if !self.config.replay_mode {
            return None;
        }

        // Try to find by request URL first
        if let Some(url) = request.get("url").and_then(|v| v.as_str()) {
            if let Some(recording) = self.find_recording(url) {
                return Some(recording.response);
            }
        }

        // Try to find by request path
        if let Some(path) = request.get("path").and_then(|v| v.as_str()) {
            if let Some(recording) = self.find_recording(path) {
                return Some(recording.response);
            }
        }

        None
    }

    /// List all loaded recordings.
    ///
    /// Note: this only returns recordings in the in-memory index.
    /// Use `load_all()` first to populate from disk.
    pub fn list_recordings(&self) -> Vec<VcrRecording> {
        self.index.values().cloned().collect()
    }

    /// Delete a recording by ID.
    ///
    /// Removes the recording from both the in-memory index and disk.
    pub fn delete_recording(&mut self, id: &str) -> bool {
        if self.index.remove(id).is_some() {
            let file_path = self.recording_path(id);
            if file_path.exists() {
                if let Err(e) = fs::remove_file(&file_path) {
                    tracing::debug!("Failed to delete VCR recording {}: {e}", file_path.display());
                }
            }
            true
        } else {
            false
        }
    }

    /// Load all recordings from the record directory into the index.
    pub fn load_all(&mut self) -> Result<usize, VcrError> {
        if !self.config.record_dir.exists() {
            return Ok(0);
        }

        let mut count = 0;
        for entry in fs::read_dir(&self.config.record_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().is_some_and(|ext| ext == "json") {
                match self.load_recording_from_file(&path) {
                    Ok(recording) => {
                        self.index.insert(recording.id.clone(), recording);
                        count += 1;
                    }
                    Err(_) => {
                        // Skip invalid recordings
                        continue;
                    }
                }
            }
        }

        Ok(count)
    }

    /// Get the number of recordings in the index.
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// Check if there are no recordings in the index.
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Get the file path for a recording with the given ID.
    fn recording_path(&self, id: &str) -> PathBuf {
        self.config.record_dir.join(format!("{id}.json"))
    }

    /// Load a single recording from a file.
    fn load_recording_from_file(&self, path: &Path) -> Result<VcrRecording, VcrError> {
        let contents = fs::read_to_string(path)?;
        let recording: VcrRecording = serde_json::from_str(&contents)
            .map_err(|e| VcrError::InvalidFormat(e.to_string()))?;
        Ok(recording)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_request(url: &str) -> Value {
        json!({
            "method": "POST",
            "url": url,
            "headers": {"Content-Type": "application/json"},
            "body": "{\"model\":\"claude-3\"}"
        })
    }

    fn make_response(status: u16) -> Value {
        json!({
            "status": status,
            "headers": {"Content-Type": "application/json"},
            "body": "{\"id\":\"msg_123\",\"type\":\"message\"}"
        })
    }

    #[test]
    fn test_vcr_config_default() {
        let config = VcrConfig::default();
        assert!(!config.enabled);
        assert!(!config.replay_mode);
        assert!(config.match_headers.is_empty());
    }

    #[test]
    fn test_vcr_config_with_dir() {
        let config = VcrConfig::with_dir("/tmp/vcr");
        assert!(config.enabled);
        assert!(!config.replay_mode);
        assert_eq!(config.record_dir, PathBuf::from("/tmp/vcr"));
    }

    #[test]
    fn test_vcr_config_replay() {
        let config = VcrConfig::replay_with_dir("/tmp/vcr");
        assert!(config.enabled);
        assert!(config.replay_mode);
    }

    #[test]
    fn test_record_and_list() {
        let tmp = TempDir::new().unwrap();
        let mut vcr = Vcr::for_recording(tmp.path());

        let request = make_request("https://api.example.com/v1/messages");
        let response = make_response(200);

        vcr.record(request, response, vec!["claude".to_string()])
            .unwrap();

        assert_eq!(vcr.len(), 1);

        let recordings = vcr.list_recordings();
        assert_eq!(recordings.len(), 1);
        assert_eq!(recordings[0].tags, vec!["claude"]);
    }

    #[test]
    fn test_record_creates_file() {
        let tmp = TempDir::new().unwrap();
        let mut vcr = Vcr::for_recording(tmp.path());

        let request = make_request("https://api.example.com/v1/messages");
        let response = make_response(200);

        vcr.record(request, response, vec![]).unwrap();

        let files: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(files.len(), 1);
        assert!(files[0].path().extension().is_some_and(|e| e == "json"));
    }

    #[test]
    fn test_record_fails_in_replay_mode() {
        let tmp = TempDir::new().unwrap();
        let mut vcr = Vcr::for_replay(tmp.path());

        let request = make_request("https://api.example.com/v1/messages");
        let response = make_response(200);

        let result = vcr.record(request, response, vec![]);
        assert!(matches!(result, Err(VcrError::ReplayModeActive)));
    }

    #[test]
    fn test_record_disabled() {
        let tmp = TempDir::new().unwrap();
        let config = VcrConfig {
            enabled: false,
            record_dir: tmp.path().to_path_buf(),
            replay_mode: false,
            match_headers: vec![],
        };
        let mut vcr = Vcr::new(config);

        let request = make_request("https://api.example.com/v1/messages");
        let response = make_response(200);

        // Should succeed silently (no-op)
        vcr.record(request, response, vec![]).unwrap();
        assert_eq!(vcr.len(), 0);
    }

    #[test]
    fn test_find_recording_by_url() {
        let tmp = TempDir::new().unwrap();
        let mut vcr = Vcr::for_recording(tmp.path());

        let request = make_request("https://api.example.com/v1/messages");
        let response = make_response(200);
        vcr.record(request, response, vec!["test".to_string()])
            .unwrap();

        let found = vcr.find_recording("api.example.com");
        assert!(found.is_some());
        assert_eq!(found.unwrap().tags, vec!["test"]);
    }

    #[test]
    fn test_find_recording_by_tag() {
        let tmp = TempDir::new().unwrap();
        let mut vcr = Vcr::for_recording(tmp.path());

        let request = make_request("https://api.example.com/v1/messages");
        let response = make_response(200);
        vcr.record(request, response, vec!["claude-api".to_string()])
            .unwrap();

        let found = vcr.find_recording("claude-api");
        assert!(found.is_some());
    }

    #[test]
    fn test_find_recording_not_found() {
        let tmp = TempDir::new().unwrap();
        let vcr = Vcr::for_recording(tmp.path());

        let found = vcr.find_recording("nonexistent");
        assert!(found.is_none());
    }

    #[test]
    fn test_replay_returns_recorded_response() {
        let tmp = TempDir::new().unwrap();
        let mut vcr = Vcr::for_recording(tmp.path());

        let request = make_request("https://api.example.com/v1/messages");
        let response = make_response(200);
        vcr.record(request.clone(), response.clone(), vec![])
            .unwrap();

        // Now create a replay VCR from the same directory
        let mut replay_vcr = Vcr::for_replay(tmp.path());
        replay_vcr.load_all().unwrap();

        let replayed = replay_vcr.replay(request);
        assert!(replayed.is_some());
        assert_eq!(replayed.unwrap()["status"], 200);
    }

    #[test]
    fn test_replay_not_in_replay_mode() {
        let tmp = TempDir::new().unwrap();
        let mut vcr = Vcr::for_recording(tmp.path());

        let request = make_request("https://api.example.com/v1/messages");
        let response = make_response(200);
        vcr.record(request.clone(), response, vec![]).unwrap();

        // Not in replay mode, should return None
        let replayed = vcr.replay(request);
        assert!(replayed.is_none());
    }

    #[test]
    fn test_delete_recording() {
        let tmp = TempDir::new().unwrap();
        let mut vcr = Vcr::for_recording(tmp.path());

        let request = make_request("https://api.example.com/v1/messages");
        let response = make_response(200);
        vcr.record(request, response, vec![]).unwrap();
        assert_eq!(vcr.len(), 1);

        let id = vcr.list_recordings()[0].id.clone();
        let deleted = vcr.delete_recording(&id);
        assert!(deleted);
        assert_eq!(vcr.len(), 0);
    }

    #[test]
    fn test_delete_nonexistent_recording() {
        let tmp = TempDir::new().unwrap();
        let mut vcr = Vcr::for_recording(tmp.path());
        let deleted = vcr.delete_recording("nonexistent-id");
        assert!(!deleted);
    }

    #[test]
    fn test_load_all() {
        let tmp = TempDir::new().unwrap();

        // Write recordings manually
        let recording = VcrRecording::new(
            make_request("https://api.example.com/v1/messages"),
            make_response(200),
            vec!["loaded".to_string()],
        );
        let file_path = tmp.path().join(format!("{}.json", recording.id));
        let json = serde_json::to_string_pretty(&recording).unwrap();
        fs::write(&file_path, json).unwrap();

        let mut vcr = Vcr::for_recording(tmp.path());
        let count = vcr.load_all().unwrap();
        assert_eq!(count, 1);
        assert_eq!(vcr.len(), 1);
    }

    #[test]
    fn test_load_all_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let mut vcr = Vcr::for_recording(tmp.path());
        let count = vcr.load_all().unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_load_all_nonexistent_dir() {
        let mut vcr = Vcr::for_recording("/nonexistent/path");
        let count = vcr.load_all().unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_recording_new_generates_id() {
        let recording = VcrRecording::new(
            make_request("https://api.example.com"),
            make_response(200),
            vec![],
        );
        assert!(!recording.id.is_empty());
        assert!(!recording.timestamp.is_empty());
    }

    #[test]
    fn test_vcr_error_display() {
        let err = VcrError::NotFound("test-id".to_string());
        assert_eq!(format!("{err}"), "Recording not found: test-id");

        let err = VcrError::ReplayModeActive;
        assert_eq!(format!("{err}"), "VCR is in replay mode; recording is disabled");
    }

    #[test]
    fn test_multiple_recordings() {
        let tmp = TempDir::new().unwrap();
        let mut vcr = Vcr::for_recording(tmp.path());

        vcr.record(
            make_request("https://api.example.com/v1/messages"),
            make_response(200),
            vec!["endpoint-1".to_string()],
        )
        .unwrap();

        vcr.record(
            make_request("https://api.example.com/v1/complete"),
            make_response(200),
            vec!["endpoint-2".to_string()],
        )
        .unwrap();

        assert_eq!(vcr.len(), 2);

        // Find by different queries
        assert!(vcr.find_recording("messages").is_some());
        assert!(vcr.find_recording("complete").is_some());
        assert!(vcr.find_recording("endpoint-1").is_some());
        assert!(vcr.find_recording("endpoint-2").is_some());
    }

    #[test]
    fn test_delete_removes_file_from_disk() {
        let tmp = TempDir::new().unwrap();
        let mut vcr = Vcr::for_recording(tmp.path());

        vcr.record(
            make_request("https://api.example.com/v1/messages"),
            make_response(200),
            vec![],
        )
        .unwrap();

        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 1);

        let id = vcr.list_recordings()[0].id.clone();
        vcr.delete_recording(&id);

        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 0);
    }
}
