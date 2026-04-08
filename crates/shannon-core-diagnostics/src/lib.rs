//! Shannon Core Diagnostics
//!
//! Diagnostic tools and health monitoring for the Shannon system.

pub mod diagnostics;
pub mod doctor;
pub mod internal_logging;
pub mod notifier;

pub use diagnostics::{
    DiagnosticCategory, DiagnosticLevel, DiagnosticResult, DiagnosticsEngine,
};
pub use doctor::{Doctor, DoctorCheck, SystemHealth};
pub use internal_logging::{InternalLog, InternalLogEntry, InternalLogLevel, InternalLogger};
pub use notifier::{Notification, NotificationChannel, NotificationPriority, Notifier};
