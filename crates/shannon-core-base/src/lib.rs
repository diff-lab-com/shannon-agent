//! # Shannon Core Base
//!
//! Foundation types and traits for Shannon Code.
//!
//! This crate provides the core foundation that all other shannon-core crates depend on:
//! - Error types and handling
//! - State management (sessions, persistence)
//! - Settings and configuration
//! - Hooks and extension points
//! - Permissions and security
//!
//! ## Re-exports
//!
//! This crate re-exports all foundation types for convenience.

pub mod error;
pub mod state;
pub mod settings;
pub mod hooks;
pub mod permissions;

// Re-export key types
pub use error::{CoreError, CoreResult};
pub use state::{StateManager, SessionState, SessionData, SessionInfo, SessionPersistMetadata};
pub use settings::{Settings, SettingsManager, SettingsError};
pub use hooks::{HookManager, HookEvent, HookResult, HookDecision, HookEventType, HookError};
pub use permissions::{
    PermissionManager, Permission, PermissionLevel, PermissionChoice, PermissionPrompt,
    PermissionConfig, PermissionError,
};
