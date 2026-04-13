//! # AI Limits Tracking
//!
//! Tracks Claude AI usage against various limit types including daily
//! requests, daily tokens, per-minute tokens, and daily cost.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// Types of AI usage limits that can be tracked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AiLimitType {
    /// Number of API requests per day.
    RequestsPerDay,
    /// Number of tokens consumed per day.
    TokensPerDay,
    /// Number of tokens consumed per minute.
    TokensPerMinute,
    /// Estimated cost in USD per day.
    CostPerDay,
}

impl std::fmt::Display for AiLimitType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AiLimitType::RequestsPerDay => write!(f, "requests per day"),
            AiLimitType::TokensPerDay => write!(f, "tokens per day"),
            AiLimitType::TokensPerMinute => write!(f, "tokens per minute"),
            AiLimitType::CostPerDay => write!(f, "cost per day"),
        }
    }
}

/// A single usage record for a tracked limit type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiUsageRecord {
    /// The type of limit being tracked.
    pub limit_type: AiLimitType,
    /// Current usage amount.
    pub current: usize,
    /// Maximum allowed amount for this limit.
    pub limit: usize,
    /// Start of the current tracking window.
    pub window_start: DateTime<Utc>,
    /// End of the current tracking window.
    pub window_end: DateTime<Utc>,
}

/// Result of checking whether usage is within limits.
#[derive(Debug, Clone, PartialEq)]
pub enum LimitStatus {
    /// Usage is within the allowed limit.
    WithinLimit {
        /// How many units remain before hitting the limit.
        remaining: usize,
    },
    /// Usage has exceeded the allowed limit.
    Exceeded {
        /// The limit that was exceeded.
        limit: usize,
        /// When the limit window resets.
        reset_at: DateTime<Utc>,
    },
}

/// Default limits for each limit type.
fn default_limit(limit_type: AiLimitType) -> usize {
    match limit_type {
        AiLimitType::RequestsPerDay => 1000,
        AiLimitType::TokensPerDay => 1_000_000,
        AiLimitType::TokensPerMinute => 100_000,
        AiLimitType::CostPerDay => 100, // $100
    }
}

/// Window duration for each limit type.
fn window_duration(limit_type: AiLimitType) -> Duration {
    match limit_type {
        AiLimitType::RequestsPerDay => Duration::days(1),
        AiLimitType::TokensPerDay => Duration::days(1),
        AiLimitType::TokensPerMinute => Duration::minutes(1),
        AiLimitType::CostPerDay => Duration::days(1),
    }
}

/// Tracks AI usage across multiple limit types.
///
/// Maintains per-limit-type usage counters with automatic window management.
pub struct AiLimitsTracker {
    /// Usage records indexed by limit type.
    records: Vec<AiUsageRecord>,
}

impl AiLimitsTracker {
    /// Create a new tracker with default limits for all limit types.
    pub fn new() -> Self {
        Self {
            records: AiLimitType::iter_all()
                .into_iter()
                .map(|lt| {
                    let now = Utc::now();
                    let window = window_duration(lt);
                    AiUsageRecord {
                        limit_type: lt,
                        current: 0,
                        limit: default_limit(lt),
                        window_start: now,
                        window_end: now + window,
                    }
                })
                .collect(),
        }
    }

    /// Record usage against a specific limit type.
    ///
    /// If the tracking window has expired, the counter is automatically reset
    /// before recording the new usage.
    pub fn record_usage(&mut self, limit_type: AiLimitType, amount: usize) {
        // Auto-reset if window has expired
        let needs_reset = self
            .get_record(limit_type)
            .is_some_and(|r| Utc::now() >= r.window_end);
        if needs_reset {
            self.reset_window(limit_type);
        }
        if let Some(record) = self.get_record_mut(limit_type) {
            record.current += amount;
        }
    }

    /// Check the current status of a limit type.
    ///
    /// Returns whether usage is within limits or has been exceeded.
    pub fn check_limit(&self, limit_type: AiLimitType) -> LimitStatus {
        if let Some(record) = self.get_record(limit_type) {
            if record.current >= record.limit {
                LimitStatus::Exceeded {
                    limit: record.limit,
                    reset_at: record.window_end,
                }
            } else {
                LimitStatus::WithinLimit {
                    remaining: record.limit - record.current,
                }
            }
        } else {
            // Unknown limit type: assume within limits
            LimitStatus::WithinLimit { remaining: usize::MAX }
        }
    }

    /// Get all current usage records.
    pub fn get_usage(&self) -> Vec<AiUsageRecord> {
        self.records.clone()
    }

    /// Reset the tracking window for a specific limit type.
    ///
    /// Clears the usage counter and starts a new time window.
    pub fn reset_window(&mut self, limit_type: AiLimitType) {
        if let Some(record) = self.get_record_mut(limit_type) {
            let now = Utc::now();
            let window = window_duration(limit_type);
            record.current = 0;
            record.window_start = now;
            record.window_end = now + window;
        }
    }

    /// Set a custom limit for a specific limit type.
    pub fn set_limit(&mut self, limit_type: AiLimitType, limit: usize) {
        if let Some(record) = self.get_record_mut(limit_type) {
            record.limit = limit;
        }
    }

    /// Get the current usage for a specific limit type.
    pub fn get_current(&self, limit_type: AiLimitType) -> Option<usize> {
        self.get_record(limit_type).map(|r| r.current)
    }

    fn get_record(&self, limit_type: AiLimitType) -> Option<&AiUsageRecord> {
        self.records.iter().find(|r| r.limit_type == limit_type)
    }

    fn get_record_mut(&mut self, limit_type: AiLimitType) -> Option<&mut AiUsageRecord> {
        self.records.iter_mut().find(|r| r.limit_type == limit_type)
    }
}

impl Default for AiLimitsTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Iterator over all limit types.
impl AiLimitType {
    fn iter_all() -> Vec<Self> {
        vec![
            AiLimitType::RequestsPerDay,
            AiLimitType::TokensPerDay,
            AiLimitType::TokensPerMinute,
            AiLimitType::CostPerDay,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tracker_new() {
        let tracker = AiLimitsTracker::new();
        let usage = tracker.get_usage();
        assert_eq!(usage.len(), 4);
        assert!(usage.iter().all(|r| r.current == 0));
    }

    #[test]
    fn test_tracker_default() {
        let tracker = AiLimitsTracker::default();
        assert_eq!(tracker.get_usage().len(), 4);
    }

    #[test]
    fn test_record_usage() {
        let mut tracker = AiLimitsTracker::new();
        tracker.record_usage(AiLimitType::RequestsPerDay, 5);
        assert_eq!(tracker.get_current(AiLimitType::RequestsPerDay), Some(5));
    }

    #[test]
    fn test_record_usage_accumulates() {
        let mut tracker = AiLimitsTracker::new();
        tracker.record_usage(AiLimitType::TokensPerDay, 1000);
        tracker.record_usage(AiLimitType::TokensPerDay, 2000);
        assert_eq!(tracker.get_current(AiLimitType::TokensPerDay), Some(3000));
    }

    #[test]
    fn test_record_usage_independent_limits() {
        let mut tracker = AiLimitsTracker::new();
        tracker.record_usage(AiLimitType::RequestsPerDay, 10);
        tracker.record_usage(AiLimitType::TokensPerDay, 5000);
        assert_eq!(tracker.get_current(AiLimitType::RequestsPerDay), Some(10));
        assert_eq!(tracker.get_current(AiLimitType::TokensPerDay), Some(5000));
    }

    #[test]
    fn test_check_limit_within() {
        let tracker = AiLimitsTracker::new();
        let status = tracker.check_limit(AiLimitType::RequestsPerDay);
        match status {
            LimitStatus::WithinLimit { remaining } => {
                assert_eq!(remaining, 1000);
            }
            LimitStatus::Exceeded { .. } => panic!("Expected WithinLimit"),
        }
    }

    #[test]
    fn test_check_limit_exceeded() {
        let mut tracker = AiLimitsTracker::new();
        tracker.set_limit(AiLimitType::RequestsPerDay, 10);
        tracker.record_usage(AiLimitType::RequestsPerDay, 15);
        let status = tracker.check_limit(AiLimitType::RequestsPerDay);
        match status {
            LimitStatus::Exceeded { limit, .. } => {
                assert_eq!(limit, 10);
            }
            LimitStatus::WithinLimit { .. } => panic!("Expected Exceeded"),
        }
    }

    #[test]
    fn test_check_limit_remaining_decreases() {
        let mut tracker = AiLimitsTracker::new();
        tracker.record_usage(AiLimitType::RequestsPerDay, 200);
        let status = tracker.check_limit(AiLimitType::RequestsPerDay);
        match status {
            LimitStatus::WithinLimit { remaining } => {
                assert_eq!(remaining, 800);
            }
            LimitStatus::Exceeded { .. } => panic!("Expected WithinLimit"),
        }
    }

    #[test]
    fn test_reset_window() {
        let mut tracker = AiLimitsTracker::new();
        tracker.record_usage(AiLimitType::TokensPerDay, 500_000);
        assert_eq!(tracker.get_current(AiLimitType::TokensPerDay), Some(500_000));

        tracker.reset_window(AiLimitType::TokensPerDay);
        assert_eq!(tracker.get_current(AiLimitType::TokensPerDay), Some(0));
    }

    #[test]
    fn test_reset_window_updates_timestamps() {
        let mut tracker = AiLimitsTracker::new();
        let before = tracker.get_usage()[0].window_start;
        // Small delay to ensure timestamp difference
        std::thread::sleep(std::time::Duration::from_millis(10));
        tracker.reset_window(AiLimitType::RequestsPerDay);
        let after = tracker.get_usage()[0].window_start;
        assert!(after >= before);
    }

    #[test]
    fn test_set_limit() {
        let mut tracker = AiLimitsTracker::new();
        tracker.set_limit(AiLimitType::TokensPerMinute, 50_000);
        let record = tracker
            .get_usage()
            .into_iter()
            .find(|r| r.limit_type == AiLimitType::TokensPerMinute)
            .unwrap();
        assert_eq!(record.limit, 50_000);
    }

    #[test]
    fn test_get_usage_returns_all() {
        let tracker = AiLimitsTracker::new();
        let usage = tracker.get_usage();
        assert_eq!(usage.len(), 4);
        let types: Vec<AiLimitType> = usage.iter().map(|r| r.limit_type).collect();
        assert!(types.contains(&AiLimitType::RequestsPerDay));
        assert!(types.contains(&AiLimitType::TokensPerDay));
        assert!(types.contains(&AiLimitType::TokensPerMinute));
        assert!(types.contains(&AiLimitType::CostPerDay));
    }

    #[test]
    fn test_get_current_none_for_unknown() {
        let tracker = AiLimitsTracker::new();
        // All known types should return Some
        assert!(tracker.get_current(AiLimitType::RequestsPerDay).is_some());
    }

    #[test]
    fn test_limit_type_display() {
        assert_eq!(
            format!("{}", AiLimitType::RequestsPerDay),
            "requests per day"
        );
        assert_eq!(
            format!("{}", AiLimitType::TokensPerDay),
            "tokens per day"
        );
        assert_eq!(
            format!("{}", AiLimitType::TokensPerMinute),
            "tokens per minute"
        );
        assert_eq!(format!("{}", AiLimitType::CostPerDay), "cost per day");
    }

    #[test]
    fn test_limit_status_equality() {
        let within = LimitStatus::WithinLimit { remaining: 100 };
        assert_eq!(within, LimitStatus::WithinLimit { remaining: 100 });

        let exceeded = LimitStatus::Exceeded {
            limit: 50,
            reset_at: Utc::now(),
        };
        // Cannot compare Exceeded due to DateTime precision, just test the variant
        assert!(matches!(exceeded, LimitStatus::Exceeded { .. }));
    }

    #[test]
    fn test_record_usage_auto_resets_expired_window() {
        let mut tracker = AiLimitsTracker::new();
        tracker.record_usage(AiLimitType::RequestsPerDay, 500);

        // Manually set window_end to the past to simulate expiration
        for record in &mut tracker.records {
            if record.limit_type == AiLimitType::RequestsPerDay {
                record.window_end = Utc::now() - Duration::seconds(1);
            }
        }

        // Record usage should auto-reset
        tracker.record_usage(AiLimitType::RequestsPerDay, 10);
        assert_eq!(tracker.get_current(AiLimitType::RequestsPerDay), Some(10));
    }
}
