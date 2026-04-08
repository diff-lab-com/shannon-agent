//! Prevent sleep service

use serde::{Deserialize, Serialize};

/// Sleep prevention method
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SleepPreventionMethod {
    DisplayOn,
    SystemIdle,
    PowerRequest,
}

/// Prevent sleep service
pub struct PreventSleepService {
    method: SleepPreventionMethod,
    enabled: bool,
}

impl PreventSleepService {
    pub fn new(method: SleepPreventionMethod) -> Self {
        Self {
            method,
            enabled: false,
        }
    }

    /// Enable sleep prevention
    pub fn enable(&mut self) -> Result<(), SleepError> {
        if self.enabled {
            return Ok(());
        }

        // TODO: Implement actual sleep prevention based on method
        tracing::info!("Sleep prevention enabled: {:?}", self.method);

        self.enabled = true;
        Ok(())
    }

    /// Disable sleep prevention
    pub fn disable(&mut self) -> Result<(), SleepError> {
        if !self.enabled {
            return Ok(());
        }

        // TODO: Disable actual sleep prevention
        tracing::info!("Sleep prevention disabled");

        self.enabled = false;
        Ok(())
    }

    /// Check if currently enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Get current method
    pub fn method(&self) -> SleepPreventionMethod {
        self.method
    }

    /// Set method
    pub fn set_method(&mut self, method: SleepPreventionMethod) {
        self.method = method;
    }
}

/// Sleep errors
#[derive(Debug, thiserror::Error)]
pub enum SleepError {
    #[error("Failed to enable sleep prevention: {0}")]
    EnableFailed(String),

    #[error("Failed to disable sleep prevention: {0}")]
    DisableFailed(String),

    #[error("Method not supported: {0:?}")]
    MethodNotSupported(SleepPreventionMethod),

    #[error("Permission denied")]
    PermissionDenied,
}
