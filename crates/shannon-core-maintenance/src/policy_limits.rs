//! Policy limits management

use chrono::Timelike;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Limit type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum LimitType {
    MaxTokensPerRequest,
    MaxTokensPerDay,
    MaxRequestsPerMinute,
    MaxToolsPerCall,
    MaxFileSize,
    MaxSessionDuration,
}

/// Limit configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitConfig {
    pub limit_type: LimitType,
    pub max_value: u64,
    pub current_value: u64,
    pub reset_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Policy limits manager
pub struct PolicyLimitsManager {
    limits: Arc<RwLock<HashMap<LimitType, LimitConfig>>>,
}

impl PolicyLimitsManager {
    pub fn new() -> Self {
        let mut limits = HashMap::new();

        // Default limits
        limits.insert(
            LimitType::MaxTokensPerRequest,
            LimitConfig {
                limit_type: LimitType::MaxTokensPerRequest,
                max_value: 200000,
                current_value: 0,
                reset_at: None,
            }
        );
        limits.insert(
            LimitType::MaxTokensPerDay,
            LimitConfig {
                limit_type: LimitType::MaxTokensPerDay,
                max_value: 1000000,
                current_value: 0,
                reset_at: None,
            }
        );
        limits.insert(
            LimitType::MaxRequestsPerMinute,
            LimitConfig {
                limit_type: LimitType::MaxRequestsPerMinute,
                max_value: 60,
                current_value: 0,
                reset_at: None,
            }
        );

        Self {
            limits: Arc::new(RwLock::new(limits)),
        }
    }

    /// Check if a limit allows an action
    pub async fn check_limit(&self, limit_type: LimitType, cost: u64) -> Result<bool, PolicyError> {
        let limits = self.limits.read().await;
        if let Some(limit) = limits.get(&limit_type) {
            // Check if limit needs reset
            if let Some(reset_at) = limit.reset_at {
                if chrono::Utc::now() > reset_at {
                    drop(limits);
                    return self.reset_and_check(limit_type, cost).await;
                }
            }

            Ok(limit.current_value + cost <= limit.max_value)
        } else {
            Ok(true) // No limit configured
        }
    }

    /// Consume from a limit
    pub async fn consume(&self, limit_type: LimitType, cost: u64) -> Result<(), PolicyError> {
        let mut limits = self.limits.write().await;
        if let Some(limit) = limits.get_mut(&limit_type) {
            limit.current_value += cost;
            Ok(())
        } else {
            Err(PolicyError::LimitNotFound(format!("{:?}", limit_type)))
        }
    }

    /// Set a limit configuration
    pub async fn set_limit(&self, config: LimitConfig) {
        let mut limits = self.limits.write().await;
        limits.insert(config.limit_type, config);
    }

    /// Get current limit status
    pub async fn get_limit_status(&self, limit_type: LimitType) -> Option<LimitConfig> {
        let limits = self.limits.read().await;
        limits.get(&limit_type).cloned()
    }

    /// Reset a limit and check
    async fn reset_and_check(&self, limit_type: LimitType, cost: u64) -> Result<bool, PolicyError> {
        let mut limits = self.limits.write().await;
        if let Some(limit) = limits.get_mut(&limit_type) {
            limit.current_value = 0;

            // Calculate next reset time
            let next_reset = match limit_type {
                LimitType::MaxTokensPerDay => chrono::Utc::now() + chrono::Duration::days(1),
                LimitType::MaxRequestsPerMinute => {
                    let now = chrono::Utc::now();
                    let next_minute = now.minute() + 1;
                    now.with_minute(next_minute)
                        .and_then(|dt| dt.with_second(0))
                        .and_then(|dt| dt.with_nanosecond(0))
                        .unwrap_or_else(|| chrono::Utc::now() + chrono::Duration::minutes(1))
                }
                _ => chrono::Utc::now() + chrono::Duration::days(1),
            };
            limit.reset_at = Some(next_reset);

            Ok(cost <= limit.max_value)
        } else {
            Err(PolicyError::LimitNotFound(format!("{:?}", limit_type)))
        }
    }

    /// Get all limits
    pub async fn get_all_limits(&self) -> Vec<LimitConfig> {
        let limits = self.limits.read().await;
        limits.values().cloned().collect()
    }
}

impl Default for PolicyLimitsManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Policy errors
#[derive(Debug, thiserror::Error)]
pub enum PolicyError {
    #[error("Limit not found: {0}")]
    LimitNotFound(String),

    #[error("Limit exceeded: {0}")]
    LimitExceeded(String),

    #[error("Invalid limit configuration: {0}")]
    InvalidConfiguration(String),
}
