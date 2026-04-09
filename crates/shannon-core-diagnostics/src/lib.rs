//! Shannon Core Diagnostics
//!
//! Diagnostic tools and health monitoring for the Shannon system.

pub mod diagnostics;
pub mod doctor;
pub mod internal_logging;
pub mod notifier;

pub use diagnostics::{
    DiagnosticCategory, DiagnosticEvent, DiagnosticLevel, DiagnosticSummary, DiagnosticTracker, ErrorPattern,
};
pub use doctor::{
    CheckStatus, DiagnosticCheck, DiagnosticCategory as DoctorDiagnosticCategory, Doctor, DoctorError, DoctorReport,
};
pub use internal_logging::{InternalLogEntry, InternalLogLevel, InternalLogger};
pub use notifier::{
    CallbackNotifier, FileNotifier, LogNotifier, Notification, NotificationHandler, NotificationLevel, Notifier, NotifierError,
};
