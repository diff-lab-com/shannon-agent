//! # Remote Session Bridge Service
//!
//! Provides a bridge service for managing remote sessions, relaying messages
//! between local and remote endpoints.
//!
//! ## Architecture
//!
//! - [`BridgeService`]: Central manager for remote session bridges
//! - [`BridgeSession`]: A single bridged session with connection state
//! - [`BridgeStatus`]: Lifecycle states of a bridge session
//! - [`SessionMessage`]: A message relayed through the bridge
//! - [`BridgeConfig`]: Configuration for the bridge service
//!
//! ## Example
//!
//! ```no_run
//! use shannon_core::bridge_service::{BridgeService, BridgeConfig, BridgeSession};
//!
//! let config = BridgeConfig::default();
//! let mut service = BridgeService::new(config);
//!
//! let session = service.create_session(
//!     "https://remote.example.com/ws",
//!     8080,
//! ).unwrap();
//! println!("Session: {}", session.id);
//! ```

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during bridge service operations.
#[derive(Error, Debug)]
pub enum BridgeError {
    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("Session already exists: {0}")]
    SessionAlreadyExists(String),

    #[error("Maximum sessions reached: {0}")]
    MaxSessionsReached(usize),

    #[error("Invalid session state: expected {expected}, got {actual}")]
    InvalidState { expected: String, actual: String },

    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),
}

// ============================================================================
// BridgeStatus
// ============================================================================

/// Lifecycle status of a bridge session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum BridgeStatus {
    /// Currently establishing connection
    Connecting,
    /// Connection established and active
    Connected,
    /// Session has been disconnected
    Disconnected,
    /// An error occurred
    Error(String),
}

impl std::fmt::Display for BridgeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BridgeStatus::Connecting => write!(f, "connecting"),
            BridgeStatus::Connected => write!(f, "connected"),
            BridgeStatus::Disconnected => write!(f, "disconnected"),
            BridgeStatus::Error(msg) => write!(f, "error: {msg}"),
        }
    }
}

impl BridgeStatus {
    /// Whether the session is in a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(self, BridgeStatus::Disconnected | BridgeStatus::Error(_))
    }

    /// Whether the session can accept messages.
    pub fn is_active(&self) -> bool {
        matches!(self, BridgeStatus::Connected)
    }
}

// ============================================================================
// SessionMessage
// ============================================================================

/// Direction of a relayed message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum MessageDirection {
    /// Message sent from local to remote
    ToRemote,
    /// Message received from remote to local
    ToLocal,
}

impl std::fmt::Display for MessageDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageDirection::ToRemote => write!(f, "to_remote"),
            MessageDirection::ToLocal => write!(f, "to_local"),
        }
    }
}

/// A message relayed through a bridge session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    /// Unique message identifier
    pub id: String,

    /// The session this message belongs to
    pub session_id: String,

    /// Direction of the message
    pub direction: MessageDirection,

    /// Message content (JSON)
    pub content: serde_json::Value,

    /// When the message was created
    pub timestamp: DateTime<Utc>,

    /// Sequence number within the session
    pub sequence: u64,
}

impl SessionMessage {
    /// Create a new session message.
    pub fn new(
        session_id: &str,
        direction: MessageDirection,
        content: serde_json::Value,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            direction,
            content,
            timestamp: Utc::now(),
            sequence: 0,
        }
    }

    /// Create a text message.
    pub fn text(session_id: &str, direction: MessageDirection, text: &str) -> Self {
        Self::new(
            session_id,
            direction,
            serde_json::json!({ "type": "text", "text": text }),
        )
    }
}

// ============================================================================
// BridgeSession
// ============================================================================

/// A single bridged remote session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeSession {
    /// Unique session identifier
    pub id: String,

    /// Local port for the session
    pub local_port: u16,

    /// Remote URL to connect to
    pub remote_url: String,

    /// Current connection status
    pub status: BridgeStatus,

    /// When the session was created
    pub created_at: DateTime<Utc>,

    /// When the session was last active
    pub last_active_at: DateTime<Utc>,

    /// Number of messages relayed
    pub message_count: u64,

    /// Number of bytes sent
    pub bytes_sent: u64,

    /// Number of bytes received
    pub bytes_received: u64,

    /// Custom metadata
    pub metadata: HashMap<String, String>,
}

impl BridgeSession {
    /// Create a new bridge session.
    pub fn new(remote_url: &str, local_port: u16) -> Self {
        let now = Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            local_port,
            remote_url: remote_url.to_string(),
            status: BridgeStatus::Connecting,
            created_at: now,
            last_active_at: now,
            message_count: 0,
            bytes_sent: 0,
            bytes_received: 0,
            metadata: HashMap::new(),
        }
    }

    /// Mark the session as connected.
    pub fn connect(&mut self) -> Result<(), BridgeError> {
        if self.status == BridgeStatus::Connected {
            return Err(BridgeError::InvalidState {
                expected: "Connecting".to_string(),
                actual: self.status.to_string(),
            });
        }
        self.status = BridgeStatus::Connected;
        self.last_active_at = Utc::now();
        Ok(())
    }

    /// Disconnect the session.
    pub fn disconnect(&mut self) {
        self.status = BridgeStatus::Disconnected;
        self.last_active_at = Utc::now();
    }

    /// Set the session to an error state.
    pub fn set_error(&mut self, message: &str) {
        self.status = BridgeStatus::Error(message.to_string());
        self.last_active_at = Utc::now();
    }

    /// Record a message relayed through this session.
    pub fn record_message(&mut self, direction: &MessageDirection, size: u64) {
        self.message_count += 1;
        self.last_active_at = Utc::now();
        match direction {
            MessageDirection::ToRemote => self.bytes_sent += size,
            MessageDirection::ToLocal => self.bytes_received += size,
        }
    }

    /// Set a metadata key-value pair.
    pub fn set_metadata(&mut self, key: &str, value: &str) {
        self.metadata.insert(key.to_string(), value.to_string());
    }

    /// Get a metadata value by key.
    pub fn get_metadata(&self, key: &str) -> Option<&String> {
        self.metadata.get(key)
    }

    /// Get the duration since the session was created.
    pub fn age(&self) -> Duration {
        Utc::now() - self.created_at
    }

    /// Get the duration since the last activity.
    pub fn idle_duration(&self) -> Duration {
        Utc::now() - self.last_active_at
    }
}

// ============================================================================
// BridgeConfig
// ============================================================================

/// Configuration for the bridge service.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BridgeConfig {
    /// Maximum number of concurrent sessions
    pub max_sessions: usize,

    /// Session timeout in seconds
    pub timeout_secs: u64,

    /// Heartbeat interval in seconds
    pub heartbeat_interval_secs: u64,

    /// Maximum message size in bytes
    pub max_message_size: u64,

    /// Whether to automatically reconnect on disconnect
    pub auto_reconnect: bool,

    /// Maximum reconnection attempts
    pub max_reconnect_attempts: u32,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            max_sessions: 10,
            timeout_secs: 3600,
            heartbeat_interval_secs: 30,
            max_message_size: 10 * 1024 * 1024, // 10 MB
            auto_reconnect: true,
            max_reconnect_attempts: 3,
        }
    }
}

impl BridgeConfig {
    /// Create a new bridge config with custom values.
    pub fn new(
        max_sessions: usize,
        timeout_secs: u64,
        heartbeat_interval_secs: u64,
    ) -> Self {
        Self {
            max_sessions,
            timeout_secs,
            heartbeat_interval_secs,
            ..Default::default()
        }
    }

    /// Validate the configuration values.
    pub fn validate(&self) -> Result<(), BridgeError> {
        if self.max_sessions == 0 {
            return Err(BridgeError::ConfigError(
                "max_sessions must be greater than 0".to_string(),
            ));
        }
        if self.timeout_secs == 0 {
            return Err(BridgeError::ConfigError(
                "timeout_secs must be greater than 0".to_string(),
            ));
        }
        if self.heartbeat_interval_secs == 0 {
            return Err(BridgeError::ConfigError(
                "heartbeat_interval_secs must be greater than 0".to_string(),
            ));
        }
        if self.max_message_size == 0 {
            return Err(BridgeError::ConfigError(
                "max_message_size must be greater than 0".to_string(),
            ));
        }
        Ok(())
    }

    /// Get the timeout as a `chrono::Duration`.
    pub fn timeout_duration(&self) -> Duration {
        Duration::seconds(self.timeout_secs as i64)
    }

    /// Get the heartbeat interval as a `chrono::Duration`.
    pub fn heartbeat_duration(&self) -> Duration {
        Duration::seconds(self.heartbeat_interval_secs as i64)
    }
}

// ============================================================================
// BridgeService
// ============================================================================

/// Central manager for remote session bridges.
#[derive(Debug, Clone)]
pub struct BridgeService {
    /// Service configuration
    config: BridgeConfig,

    /// Active sessions by ID
    sessions: HashMap<String, BridgeSession>,

    /// Message history per session (bounded)
    message_history: HashMap<String, Vec<SessionMessage>>,

    /// Maximum messages to keep per session
    max_history_per_session: usize,
}

impl BridgeService {
    /// Create a new bridge service with the given configuration.
    pub fn new(config: BridgeConfig) -> Self {
        Self {
            config,
            sessions: HashMap::new(),
            message_history: HashMap::new(),
            max_history_per_session: 1000,
        }
    }

    /// Create a new bridge session.
    pub fn create_session(
        &mut self,
        remote_url: &str,
        local_port: u16,
    ) -> Result<&BridgeSession, BridgeError> {
        if self.sessions.len() >= self.config.max_sessions {
            return Err(BridgeError::MaxSessionsReached(self.config.max_sessions));
        }

        let session = BridgeSession::new(remote_url, local_port);
        let id = session.id.clone();
        self.message_history.insert(id.clone(), Vec::new());
        self.sessions.insert(id.clone(), session);
        Ok(self.sessions.get(&id).expect("session was just inserted after capacity check"))
    }

    /// Get a session by ID.
    pub fn get_session(&self, id: &str) -> Result<&BridgeSession, BridgeError> {
        self.sessions
            .get(id)
            .ok_or_else(|| BridgeError::SessionNotFound(id.to_string()))
    }

    /// Get a mutable session by ID.
    pub fn get_session_mut(&mut self, id: &str) -> Result<&mut BridgeSession, BridgeError> {
        self.sessions
            .get_mut(id)
            .ok_or_else(|| BridgeError::SessionNotFound(id.to_string()))
    }

    /// Connect a session by ID.
    pub fn connect_session(&mut self, id: &str) -> Result<(), BridgeError> {
        match self.sessions.get_mut(id) {
            Some(session) => session.connect(),
            None => Err(BridgeError::SessionNotFound(id.to_string())),
        }
    }

    /// Disconnect a session by ID.
    pub fn disconnect_session(&mut self, id: &str) -> Result<(), BridgeError> {
        match self.sessions.get_mut(id) {
            Some(session) => {
                session.disconnect();
                Ok(())
            }
            None => Err(BridgeError::SessionNotFound(id.to_string())),
        }
    }

    /// Remove a session by ID.
    pub fn remove_session(&mut self, id: &str) -> Result<BridgeSession, BridgeError> {
        self.message_history.remove(id);
        self.sessions
            .remove(id)
            .ok_or_else(|| BridgeError::SessionNotFound(id.to_string()))
    }

    /// Send a message through a session.
    pub fn send_message(
        &mut self,
        session_id: &str,
        content: serde_json::Value,
    ) -> Result<SessionMessage, BridgeError> {
        // Check session exists and is active
        let is_active = self
            .sessions
            .get(session_id)
            .map(|s| s.status.is_active())
            .ok_or_else(|| BridgeError::SessionNotFound(session_id.to_string()))?;

        if !is_active {
            let actual = self
                .sessions
                .get(session_id)
                .map(|s| s.status.to_string())
                .unwrap_or_default();
            return Err(BridgeError::InvalidState {
                expected: "connected".to_string(),
                actual,
            });
        }

        let mut msg = SessionMessage::new(session_id, MessageDirection::ToRemote, content);

        // Assign sequence number
        let seq = self
            .message_history
            .get(session_id)
            .map(|h| h.len() as u64 + 1)
            .unwrap_or(1);
        msg.sequence = seq;

        let size = serde_json::to_string(&msg.content)
            .map(|s| s.len() as u64)
            .unwrap_or(0);

        if let Some(session) = self.sessions.get_mut(session_id) {
            session.record_message(&MessageDirection::ToRemote, size);
        }

        if let Some(history) = self.message_history.get_mut(session_id) {
            if history.len() >= self.max_history_per_session {
                history.remove(0);
            }
            history.push(msg.clone());
        }

        Ok(msg)
    }

    /// Receive a message from a session (simulated).
    pub fn receive_message(
        &mut self,
        session_id: &str,
        content: serde_json::Value,
    ) -> Result<SessionMessage, BridgeError> {
        // Check session exists and is active
        let is_active = self
            .sessions
            .get(session_id)
            .map(|s| s.status.is_active())
            .ok_or_else(|| BridgeError::SessionNotFound(session_id.to_string()))?;

        if !is_active {
            let actual = self
                .sessions
                .get(session_id)
                .map(|s| s.status.to_string())
                .unwrap_or_default();
            return Err(BridgeError::InvalidState {
                expected: "connected".to_string(),
                actual,
            });
        }

        let mut msg = SessionMessage::new(session_id, MessageDirection::ToLocal, content);

        // Assign sequence number
        let seq = self
            .message_history
            .get(session_id)
            .map(|h| h.len() as u64 + 1)
            .unwrap_or(1);
        msg.sequence = seq;

        let size = serde_json::to_string(&msg.content)
            .map(|s| s.len() as u64)
            .unwrap_or(0);

        if let Some(session) = self.sessions.get_mut(session_id) {
            session.record_message(&MessageDirection::ToLocal, size);
        }

        if let Some(history) = self.message_history.get_mut(session_id) {
            if history.len() >= self.max_history_per_session {
                history.remove(0);
            }
            history.push(msg.clone());
        }

        Ok(msg)
    }

    /// Get message history for a session.
    pub fn message_history(&self, session_id: &str) -> Result<&[SessionMessage], BridgeError> {
        self.message_history
            .get(session_id)
            .map(|v| v.as_slice())
            .ok_or_else(|| BridgeError::SessionNotFound(session_id.to_string()))
    }

    /// List all sessions.
    pub fn list_sessions(&self) -> Vec<&BridgeSession> {
        self.sessions.values().collect()
    }

    /// List sessions by status.
    pub fn sessions_by_status(&self, status: &BridgeStatus) -> Vec<&BridgeSession> {
        self.sessions
            .values()
            .filter(|s| &s.status == status)
            .collect()
    }

    /// List active (connected) sessions.
    pub fn active_sessions(&self) -> Vec<&BridgeSession> {
        self.sessions_by_status(&BridgeStatus::Connected)
    }

    /// Get the number of active sessions.
    pub fn active_count(&self) -> usize {
        self.sessions
            .values()
            .filter(|s| s.status.is_active())
            .count()
    }

    /// Get the total number of sessions.
    pub fn total_count(&self) -> usize {
        self.sessions.len()
    }

    /// Get the service configuration.
    pub fn config(&self) -> &BridgeConfig {
        &self.config
    }

    /// Check for sessions that have been idle longer than the configured timeout.
    pub fn timed_out_sessions(&self) -> Vec<&BridgeSession> {
        let timeout = self.config.timeout_duration();
        self.sessions
            .values()
            .filter(|s| s.status.is_active() && s.idle_duration() > timeout)
            .collect()
    }

    /// Disconnect all idle sessions that have exceeded the timeout.
    pub fn disconnect_timed_out(&mut self) -> usize {
        let timed_out: Vec<String> = self
            .sessions
            .values()
            .filter(|s| s.status.is_active() && s.idle_duration() > self.config.timeout_duration())
            .map(|s| s.id.clone())
            .collect();

        let count = timed_out.len();
        for id in timed_out {
            if let Some(session) = self.sessions.get_mut(&id) {
                session.disconnect();
            }
        }
        count
    }

    /// Remove all disconnected and errored sessions.
    pub fn cleanup_terminated(&mut self) -> usize {
        let terminated: Vec<String> = self
            .sessions
            .values()
            .filter(|s| s.status.is_terminal())
            .map(|s| s.id.clone())
            .collect();

        let count = terminated.len();
        for id in terminated {
            self.sessions.remove(&id);
            self.message_history.remove(&id);
        }
        count
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- BridgeStatus tests ----

    #[test]
    fn test_bridge_status_properties() {
        assert!(!BridgeStatus::Connecting.is_active());
        assert!(BridgeStatus::Connected.is_active());
        assert!(!BridgeStatus::Disconnected.is_active());
        assert!(!BridgeStatus::Error("test".to_string()).is_active());

        assert!(!BridgeStatus::Connecting.is_terminal());
        assert!(!BridgeStatus::Connected.is_terminal());
        assert!(BridgeStatus::Disconnected.is_terminal());
        assert!(BridgeStatus::Error("test".to_string()).is_terminal());
    }

    // ---- BridgeConfig tests ----

    #[test]
    fn test_bridge_config_default() {
        let config = BridgeConfig::default();
        assert_eq!(config.max_sessions, 10);
        assert_eq!(config.timeout_secs, 3600);
        assert_eq!(config.heartbeat_interval_secs, 30);
        assert!(config.auto_reconnect);
    }

    #[test]
    fn test_bridge_config_validation() {
        let valid = BridgeConfig::default();
        assert!(valid.validate().is_ok());

        let invalid = BridgeConfig {
            max_sessions: 0,
            ..Default::default()
        };
        assert!(invalid.validate().is_err());

        let invalid = BridgeConfig {
            timeout_secs: 0,
            ..Default::default()
        };
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn test_bridge_config_durations() {
        let config = BridgeConfig::new(5, 120, 15);
        assert_eq!(config.timeout_duration(), Duration::seconds(120));
        assert_eq!(config.heartbeat_duration(), Duration::seconds(15));
    }

    // ---- BridgeSession tests ----

    #[test]
    fn test_session_creation() {
        let session = BridgeSession::new("https://remote.example.com", 8080);
        assert_eq!(session.status, BridgeStatus::Connecting);
        assert_eq!(session.local_port, 8080);
        assert_eq!(session.remote_url, "https://remote.example.com");
        assert_eq!(session.message_count, 0);
        assert_eq!(session.bytes_sent, 0);
        assert_eq!(session.bytes_received, 0);
    }

    #[test]
    fn test_session_lifecycle() {
        let mut session = BridgeSession::new("https://remote.example.com", 8080);

        session.connect().unwrap();
        assert_eq!(session.status, BridgeStatus::Connected);

        // Connecting again should fail
        assert!(session.connect().is_err());

        session.disconnect();
        assert_eq!(session.status, BridgeStatus::Disconnected);

        session.set_error("connection lost");
        assert_eq!(session.status, BridgeStatus::Error("connection lost".to_string()));
    }

    #[test]
    fn test_session_message_recording() {
        let mut session = BridgeSession::new("https://remote.example.com", 8080);
        session.connect().unwrap();

        session.record_message(&MessageDirection::ToRemote, 100);
        session.record_message(&MessageDirection::ToLocal, 200);
        session.record_message(&MessageDirection::ToRemote, 50);

        assert_eq!(session.message_count, 3);
        assert_eq!(session.bytes_sent, 150);
        assert_eq!(session.bytes_received, 200);
    }

    #[test]
    fn test_session_metadata() {
        let mut session = BridgeSession::new("https://remote.example.com", 8080);
        session.set_metadata("user", "alice");
        session.set_metadata("project", "shannon");

        assert_eq!(session.get_metadata("user"), Some(&"alice".to_string()));
        assert_eq!(session.get_metadata("project"), Some(&"shannon".to_string()));
        assert_eq!(session.get_metadata("nonexistent"), None);
    }

    // ---- SessionMessage tests ----

    #[test]
    fn test_message_creation() {
        let msg = SessionMessage::text("sess-1", MessageDirection::ToRemote, "hello");
        assert_eq!(msg.session_id, "sess-1");
        assert_eq!(msg.direction, MessageDirection::ToRemote);
        assert_eq!(msg.content["type"], "text");
        assert_eq!(msg.content["text"], "hello");
    }

    #[test]
    fn test_message_serialization() {
        let msg = SessionMessage::text("sess-1", MessageDirection::ToLocal, "hello");
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: SessionMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg.id, deserialized.id);
        assert_eq!(msg.direction, deserialized.direction);
    }

    // ---- BridgeService tests ----

    #[test]
    fn test_service_create_session() {
        let config = BridgeConfig::default();
        let mut service = BridgeService::new(config);

        let session = service.create_session("https://remote.example.com", 8080).unwrap();
        assert_eq!(session.status, BridgeStatus::Connecting);
        assert_eq!(service.total_count(), 1);
    }

    #[test]
    fn test_service_max_sessions() {
        let config = BridgeConfig::new(2, 3600, 30);
        let mut service = BridgeService::new(config);

        service.create_session("https://a.example.com", 8080).unwrap();
        service.create_session("https://b.example.com", 8081).unwrap();

        let result = service.create_session("https://c.example.com", 8082);
        assert!(matches!(result, Err(BridgeError::MaxSessionsReached(2))));
    }

    #[test]
    fn test_service_connect_disconnect() {
        let config = BridgeConfig::default();
        let mut service = BridgeService::new(config);

        let session = service.create_session("https://remote.example.com", 8080).unwrap();
        let id = session.id.clone();

        service.connect_session(&id).unwrap();
        assert_eq!(service.get_session(&id).unwrap().status, BridgeStatus::Connected);
        assert_eq!(service.active_count(), 1);

        service.disconnect_session(&id).unwrap();
        assert_eq!(service.get_session(&id).unwrap().status, BridgeStatus::Disconnected);
        assert_eq!(service.active_count(), 0);
    }

    #[test]
    fn test_service_send_receive_messages() {
        let config = BridgeConfig::default();
        let mut service = BridgeService::new(config);

        let session = service.create_session("https://remote.example.com", 8080).unwrap();
        let id = session.id.clone();

        service.connect_session(&id).unwrap();

        let sent = service
            .send_message(&id, serde_json::json!({ "type": "text", "text": "hello" }))
            .unwrap();
        assert_eq!(sent.direction, MessageDirection::ToRemote);
        assert_eq!(sent.sequence, 1);

        let received = service
            .receive_message(&id, serde_json::json!({ "type": "text", "text": "world" }))
            .unwrap();
        assert_eq!(received.direction, MessageDirection::ToLocal);
        assert_eq!(received.sequence, 2);

        let session = service.get_session(&id).unwrap();
        assert_eq!(session.message_count, 2);
        assert!(session.bytes_sent > 0);
        assert!(session.bytes_received > 0);

        let history = service.message_history(&id).unwrap();
        assert_eq!(history.len(), 2);
    }

    #[test]
    fn test_service_send_on_disconnected_fails() {
        let config = BridgeConfig::default();
        let mut service = BridgeService::new(config);

        let session = service.create_session("https://remote.example.com", 8080).unwrap();
        let id = session.id.clone();

        let result = service.send_message(&id, serde_json::json!("hello"));
        assert!(matches!(result, Err(BridgeError::InvalidState { .. })));
    }

    #[test]
    fn test_service_cleanup_terminated() {
        let config = BridgeConfig::default();
        let mut service = BridgeService::new(config);

        let s1_id = service.create_session("https://a.example.com", 8080).unwrap().id.clone();
        let s2_id = service.create_session("https://b.example.com", 8081).unwrap().id.clone();
        let s3_id = service.create_session("https://c.example.com", 8082).unwrap().id.clone();

        service.connect_session(&s1_id).unwrap();
        service.disconnect_session(&s1_id).unwrap();
        service.connect_session(&s2_id).unwrap();
        service.connect_session(&s3_id).unwrap();

        assert_eq!(service.total_count(), 3);

        let cleaned = service.cleanup_terminated();
        assert_eq!(cleaned, 1);
        assert_eq!(service.total_count(), 2);
    }

    #[test]
    fn test_service_remove_session() {
        let config = BridgeConfig::default();
        let mut service = BridgeService::new(config);

        let session = service.create_session("https://remote.example.com", 8080).unwrap();
        let id = session.id.clone();

        let removed = service.remove_session(&id).unwrap();
        assert_eq!(removed.id, id);
        assert_eq!(service.total_count(), 0);
        assert!(service.get_session(&id).is_err());
    }
}
