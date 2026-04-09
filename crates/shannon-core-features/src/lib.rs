//! # Shannon Core Features
//!
//! Feature modules for Shannon Code.
//!
//! This crate provides various feature modules:
//! - Analytics and usage tracking
//! - Voice mode services
//! - Magic documentation
//! - Auto-update system
//! - OAuth authentication
//! - Billing integration
//! - Credential management

pub mod analytics;
pub mod voice_mode;
pub mod magic_docs;
pub mod updater;
pub mod oauth;
pub mod billing;
pub mod credential_manager;

// Re-export key types
pub use analytics::{AnalyticsStore, AnalyticsEvent, EventTracker, AnalyticsAggregator, AggregatedStats};
pub use voice_mode::{VoiceModeService, VoiceConfig, VoiceStatus, TranscriptionResult, VoiceCommand, VoiceCommandResult, VoiceSession, KeywordSpotter};
pub use magic_docs::{MagicDocsService, DocRequest, DocResponse, DocFormat, Template};
pub use updater::{AutoUpdater, UpdaterConfig, UpdateStatus, UpdateError, ReleaseInfo, CURRENT_VERSION};
pub use oauth::{OAuthService, OAuthClient, OAuthToken, OAuthError, TokenEncryption};
pub use billing::{BillingManager, BillingInfo, UsageRecord, BillingPeriod, BillingError};
pub use credential_manager::{CredentialManager, Credential, CredentialError, SecureStorage};
