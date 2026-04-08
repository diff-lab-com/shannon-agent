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
pub use analytics::{AnalyticsStore, AnalyticsEvent, EventTracker};
pub use voice_mode::{VoiceModeService, VoiceConfig, VoiceState};
pub use magic_docs::{MagicDocsService, DocRequest, DocResponse};
pub use updater::{AutoUpdater, UpdateInfo, UpdateStatus};
pub use oauth::{OAuthService, OAuthConfig, OAuthToken};
pub use billing::{BillingService, BillingInfo, UsageStats};
pub use credential_manager::{CredentialManager, Credential, SecureStorage};
