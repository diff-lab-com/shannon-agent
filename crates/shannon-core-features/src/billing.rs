//! # Billing
//!
//! Usage tracking, cost management, and billing integration for Shannon Code.

use chrono::{DateTime, Duration, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use thiserror::Error;

// ============================================================================
// Error
// ============================================================================

/// Errors that can occur during billing operations.
#[derive(Debug, Error)]
pub enum BillingError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Usage limit exceeded: {used}/{limit} tokens")]
    LimitExceeded { used: u64, limit: u64 },

    #[error("No billing information found for user: {0}")]
    NoBillingInfo(String),

    #[error("API error: {0}")]
    ApiError(String),

    #[error("Authentication failed")]
    AuthenticationFailed,

    #[error("Rate limit exceeded")]
    RateLimitExceeded,
}

// ============================================================================
// Usage Record
// ============================================================================

/// A single usage record tracking resource consumption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    /// Unique identifier for this record.
    pub id: String,

    /// User or session identifier.
    pub user_id: String,

    /// Type of resource used (e.g., "tokens", "requests", "minutes").
    pub resource_type: String,

    /// Amount consumed.
    pub amount: u64,

    /// Cost in USD (if applicable).
    pub cost: f64,

    /// When the usage occurred.
    pub timestamp: DateTime<Utc>,

    /// Optional metadata about the usage.
    pub metadata: HashMap<String, String>,
}

impl UsageRecord {
    /// Create a new usage record.
    pub fn new(
        user_id: String,
        resource_type: String,
        amount: u64,
        cost: f64,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            user_id,
            resource_type,
            amount,
            cost,
            timestamp: Utc::now(),
            metadata: HashMap::new(),
        }
    }

    /// Add metadata to this record.
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

// ============================================================================
// Billing Period
// ============================================================================

/// A billing period with start and end dates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillingPeriod {
    /// Period identifier (e.g., "2024-01").
    pub id: String,

    /// Start of the billing period.
    pub start: DateTime<Utc>,

    /// End of the billing period.
    pub end: DateTime<Utc>,

    /// Whether this period is finalized.
    pub finalized: bool,
}

impl BillingPeriod {
    /// Create a new billing period.
    pub fn new(id: String, start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        Self {
            id,
            start,
            end,
            finalized: false,
        }
    }

    /// Check if a given timestamp falls within this period.
    pub fn contains(&self, timestamp: DateTime<Utc>) -> bool {
        timestamp >= self.start && timestamp < self.end
    }

    /// Create a monthly billing period for the given year and month.
    pub fn monthly(year: i32, month: u32) -> Self {
        let start = Utc.with_ymd_and_hms(year, month, 1, 0, 0, 0).unwrap();
        let end = if month == 12 {
            Utc.with_ymd_and_hms(year + 1, 1, 1, 0, 0, 0).unwrap()
        } else {
            Utc.with_ymd_and_hms(year, month + 1, 1, 0, 0, 0).unwrap()
        };

        Self {
            id: format!("{}-{:02}", year, month),
            start,
            end,
            finalized: false,
        }
    }
}

// ============================================================================
// Billing Info
// ============================================================================

/// Billing information for a user or organization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillingInfo {
    /// User or organization identifier.
    pub user_id: String,

    /// Current plan (e.g., "free", "pro", "enterprise").
    pub plan: String,

    /// Usage limits per resource type.
    pub limits: HashMap<String, u64>,

    /// Current usage amounts per resource type.
    pub usage: HashMap<String, u64>,

    /// Current billing period.
    pub period: BillingPeriod,

    /// Billing date (when payment is due).
    pub billing_date: DateTime<Utc>,

    /// Monthly cost in USD.
    pub monthly_cost: f64,
}

impl BillingInfo {
    /// Create new billing information.
    pub fn new(user_id: String, plan: String, period: BillingPeriod) -> Self {
        let billing_date = period.end + Duration::days(7); // 7 days grace period
        Self {
            user_id,
            plan,
            limits: HashMap::new(),
            usage: HashMap::new(),
            period,
            billing_date,
            monthly_cost: 0.0,
        }
    }

    /// Set a usage limit for a resource type.
    pub fn with_limit(mut self, resource_type: String, limit: u64) -> Self {
        self.limits.insert(resource_type, limit);
        self
    }

    /// Get current usage for a resource type.
    pub fn get_usage(&self, resource_type: &str) -> u64 {
        self.usage.get(resource_type).copied().unwrap_or(0)
    }

    /// Get the limit for a resource type.
    pub fn get_limit(&self, resource_type: &str) -> Option<u64> {
        self.limits.get(resource_type).copied()
    }

    /// Check if usage is within limits for all resources.
    pub fn within_limits(&self) -> bool {
        for (resource_type, usage) in &self.usage {
            if let Some(&limit) = self.limits.get(resource_type) {
                if *usage > limit {
                    return false;
                }
            }
        }
        true
    }

    /// Calculate remaining usage for a resource type.
    pub fn remaining(&self, resource_type: &str) -> Option<u64> {
        let limit = self.limits.get(resource_type).copied()?;
        let used = self.get_usage(resource_type);
        Some(limit.saturating_sub(used))
    }
}

// ============================================================================
// Billing Manager
// ============================================================================

/// Manages billing information, usage tracking, and limit enforcement.
pub struct BillingManager {
    billing_info: HashMap<String, BillingInfo>,
    usage_records: Vec<UsageRecord>,
    storage_path: Option<PathBuf>,
}

impl BillingManager {
    /// Create a new billing manager.
    pub fn new() -> Self {
        Self {
            billing_info: HashMap::new(),
            usage_records: Vec::new(),
            storage_path: None,
        }
    }

    /// Set the storage path for persistence.
    pub fn with_storage_path(mut self, path: PathBuf) -> Self {
        self.storage_path = Some(path);
        self
    }

    /// Register billing information for a user.
    pub fn register_billing(&mut self, info: BillingInfo) -> Result<(), BillingError> {
        let user_id = info.user_id.clone();
        self.billing_info.insert(user_id, info);
        Ok(())
    }

    /// Get billing information for a user.
    pub fn get_billing(&self, user_id: &str) -> Option<&BillingInfo> {
        self.billing_info.get(user_id)
    }

    /// Record usage and update totals.
    pub fn record_usage(&mut self, record: UsageRecord) -> Result<(), BillingError> {
        let user_id = record.user_id.clone();
        let resource_type = record.resource_type.clone();
        let amount = record.amount;

        // Check limits before recording
        if let Some(info) = self.billing_info.get(&user_id) {
            if let Some(&limit) = info.limits.get(&resource_type) {
                let current = info.get_usage(&resource_type);
                if current + amount > limit {
                    return Err(BillingError::LimitExceeded {
                        used: current + amount,
                        limit,
                    });
                }
            }
        }

        // Update usage totals
        if let Some(info) = self.billing_info.get_mut(&user_id) {
            *info.usage.entry(resource_type.clone()).or_insert(0) += amount;
            info.monthly_cost += record.cost;
        }

        self.usage_records.push(record);
        Ok(())
    }

    /// Get usage records for a user within a period.
    pub fn get_usage_records(
        &self,
        user_id: &str,
        period: &BillingPeriod,
    ) -> Vec<&UsageRecord> {
        self.usage_records
            .iter()
            .filter(|r| r.user_id == user_id && period.contains(r.timestamp))
            .collect()
    }

    /// Calculate daily totals for a user and resource type.
    pub fn daily_totals(
        &self,
        user_id: &str,
        resource_type: &str,
        period: &BillingPeriod,
    ) -> HashMap<String, u64> {
        let mut totals: HashMap<String, u64> = HashMap::new();

        for record in self.usage_records.iter() {
            if record.user_id == user_id
                && record.resource_type == resource_type
                && period.contains(record.timestamp)
            {
                let day = record.timestamp.format("%Y-%m-%d").to_string();
                *totals.entry(day).or_insert(0) += record.amount;
            }
        }

        totals
    }

    /// Reset usage for a new billing period.
    pub fn reset_period(&mut self, user_id: &str, new_period: BillingPeriod) -> Result<(), BillingError> {
        if let Some(info) = self.billing_info.get_mut(user_id) {
            info.usage.clear();
            info.period = new_period;
            info.monthly_cost = 0.0;
            Ok(())
        } else {
            Err(BillingError::NoBillingInfo(user_id.to_string()))
        }
    }

    /// Check if a user's usage is within budget.
    pub fn check_budget(&self, user_id: &str, monthly_budget: f64) -> bool {
        if let Some(info) = self.billing_info.get(user_id) {
            info.monthly_cost <= monthly_budget
        } else {
            true // No usage yet
        }
    }

    /// Get budget alert status.
    pub fn budget_alert(&self, user_id: &str, threshold: f64) -> Option<f64> {
        if let Some(info) = self.billing_info.get(user_id) {
            // Assume default free tier budget of $10/month
            let default_budget = 10.0;
            let budget = default_budget;
            let ratio = info.monthly_cost / budget;

            if ratio >= threshold {
                Some(ratio)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Persist billing data to disk.
    pub fn persist(&self) -> Result<(), BillingError> {
        if let Some(path) = &self.storage_path {
            // Ensure directory exists
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            let data = serde_json::to_string_pretty(&self.billing_info)?;
            std::fs::write(path, data)?;
        }
        Ok(())
    }

    /// Load billing data from disk.
    pub fn load(&mut self) -> Result<(), BillingError> {
        if let Some(path) = &self.storage_path {
            if path.exists() {
                let data = std::fs::read_to_string(path)?;
                self.billing_info = serde_json::from_str(&data)?;
            }
        }
        Ok(())
    }
}

impl Default for BillingManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_billing() -> BillingInfo {
        let period = BillingPeriod::monthly(2024, 1);
        BillingInfo::new("user123".to_string(), "free".to_string(), period)
            .with_limit("tokens".to_string(), 100000)
            .with_limit("requests".to_string(), 1000)
    }

    #[test]
    fn test_billing_period_contains() {
        let period = BillingPeriod::monthly(2024, 1);
        let within = Utc.with_ymd_and_hms(2024, 1, 15, 12, 0, 0).unwrap();
        let before = Utc.with_ymd_and_hms(2023, 12, 31, 23, 59, 59).unwrap();
        let after = Utc.with_ymd_and_hms(2024, 2, 1, 0, 0, 0).unwrap();

        assert!(period.contains(within));
        assert!(!period.contains(before));
        assert!(!period.contains(after));
    }

    #[test]
    fn test_billing_info_within_limits() {
        let mut info = create_test_billing();
        info.usage.insert("tokens".to_string(), 50000);
        info.usage.insert("requests".to_string(), 500);

        assert!(info.within_limits());

        info.usage.insert("tokens".to_string(), 100001);
        assert!(!info.within_limits());
    }

    #[test]
    fn test_billing_info_remaining() {
        let mut info = create_test_billing();
        info.usage.insert("tokens".to_string(), 30000);

        assert_eq!(info.remaining("tokens"), Some(70000));
        assert_eq!(info.remaining("unknown"), None);
    }

    #[test]
    fn test_usage_record_creation() {
        let record = UsageRecord::new("user123".to_string(), "tokens".to_string(), 1000, 0.01)
            .with_metadata("model".to_string(), "gpt-4".to_string());

        assert_eq!(record.user_id, "user123");
        assert_eq!(record.resource_type, "tokens");
        assert_eq!(record.amount, 1000);
        assert_eq!(record.metadata.get("model"), Some(&"gpt-4".to_string()));
    }

    #[test]
    fn test_billing_manager_record_usage() {
        let mut manager = BillingManager::new();
        manager.register_billing(create_test_billing()).unwrap();

        let record = UsageRecord::new("user123".to_string(), "tokens".to_string(), 5000, 0.05);
        manager.record_usage(record).unwrap();

        let info = manager.get_billing("user123").unwrap();
        assert_eq!(info.get_usage("tokens"), 5000);
    }

    #[test]
    fn test_billing_manager_limit_exceeded() {
        let mut manager = BillingManager::new();
        manager.register_billing(create_test_billing()).unwrap();

        // First record should succeed
        let record1 = UsageRecord::new("user123".to_string(), "tokens".to_string(), 99000, 0.99);
        manager.record_usage(record1).unwrap();

        // This should exceed the limit
        let record2 = UsageRecord::new("user123".to_string(), "tokens".to_string(), 2000, 0.02);
        let result = manager.record_usage(record2);

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), BillingError::LimitExceeded { .. }));
    }

    #[test]
    fn test_billing_manager_daily_totals() {
        let mut manager = BillingManager::new();
        manager.register_billing(create_test_billing()).unwrap();

        let period = BillingPeriod::monthly(2024, 1);

        let record1 = UsageRecord::new("user123".to_string(), "tokens".to_string(), 1000, 0.01);
        let record2 = UsageRecord::new("user123".to_string(), "tokens".to_string(), 2000, 0.02);

        manager.usage_records.push(record1);
        manager.usage_records.push(record2);

        let totals = manager.daily_totals("user123", "tokens", &period);
        // Both records should be counted for the same day
        assert!(!totals.is_empty());
    }

    #[test]
    fn test_billing_manager_reset_period() {
        let mut manager = BillingManager::new();
        let mut info = create_test_billing();
        info.usage.insert("tokens".to_string(), 50000);

        manager.register_billing(info).unwrap();
        let new_period = BillingPeriod::monthly(2024, 2);
        manager.reset_period("user123", new_period).unwrap();

        let info = manager.get_billing("user123").unwrap();
        assert_eq!(info.get_usage("tokens"), 0);
        assert_eq!(info.monthly_cost, 0.0);
    }

    #[test]
    fn test_billing_manager_check_budget() {
        let mut manager = BillingManager::new();
        let mut info = create_test_billing();
        info.monthly_cost = 5.0;
        manager.register_billing(info).unwrap();

        assert!(manager.check_budget("user123", 10.0));
        assert!(!manager.check_budget("user123", 4.0));
    }

    #[test]
    fn test_billing_manager_budget_alert() {
        let mut manager = BillingManager::new();
        let mut info = create_test_billing();
        info.monthly_cost = 8.5; // 85% of $10 budget
        manager.register_billing(info).unwrap();

        assert_eq!(manager.budget_alert("user123", 0.8), Some(0.85));
        assert_eq!(manager.budget_alert("user123", 0.9), None);
    }

    #[test]
    fn test_billing_serialization() {
        let info = create_test_billing();
        let json = serde_json::to_string(&info).unwrap();
        let decoded: BillingInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.user_id, info.user_id);
        assert_eq!(decoded.plan, info.plan);
        assert_eq!(decoded.limits.len(), 2);
    }

    #[test]
    fn test_usage_record_serialization() {
        let record = UsageRecord::new("user123".to_string(), "tokens".to_string(), 1000, 0.01)
            .with_metadata("test".to_string(), "value".to_string());

        let json = serde_json::to_string(&record).unwrap();
        let decoded: UsageRecord = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.user_id, record.user_id);
        assert_eq!(decoded.metadata.get("test"), Some(&"value".to_string()));
    }
}
