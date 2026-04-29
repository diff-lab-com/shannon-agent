//! # Billing Manager
//!
//! Usage tracking, cost aggregation, and budget alerting for Shannon Code.
//!
//! Records per-request usage (model, tokens, cost) and provides aggregation
//! methods for billing periods, model breakdowns, daily totals, and monthly
//! cost estimation.
//!
//! Reference: Claude Code src/utils/billing.ts

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::debug;

/// Errors that can occur during billing operations.
#[derive(Error, Debug)]
pub enum BillingError {
    #[error("Billing record not found: {0}")]
    NotFound(String),

    #[error("Invalid billing data: {0}")]
    Invalid(String),

    #[error("Budget exceeded: {current_cost:.4} > {limit:.4}")]
    BudgetExceeded {
        current_cost: f64,
        limit: f64,
    },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// A single usage record for one API request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    /// The model used (e.g., "claude-sonnet-4-20250514").
    pub model: String,
    /// Number of input tokens consumed.
    pub input_tokens: u64,
    /// Number of output tokens consumed.
    pub output_tokens: u64,
    /// Dollar cost for this request.
    pub cost: f64,
    /// When this request occurred.
    pub timestamp: DateTime<Utc>,
    /// Optional session ID for grouping requests.
    #[serde(default)]
    pub session_id: Option<String>,
}

impl UsageRecord {
    /// Create a new usage record.
    pub fn new(model: &str, input_tokens: u64, output_tokens: u64, cost: f64) -> Self {
        Self {
            model: model.to_string(),
            input_tokens,
            output_tokens,
            cost,
            timestamp: Utc::now(),
            session_id: None,
        }
    }

    /// Create a usage record with a specific timestamp.
    pub fn with_timestamp(
        model: &str,
        input_tokens: u64,
        output_tokens: u64,
        cost: f64,
        timestamp: DateTime<Utc>,
    ) -> Self {
        Self {
            model: model.to_string(),
            input_tokens,
            output_tokens,
            cost,
            timestamp,
            session_id: None,
        }
    }

    /// Total tokens for this record.
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}

/// A summary of usage within a billing period.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillingPeriod {
    /// Start of the billing period.
    pub start: DateTime<Utc>,
    /// End of the billing period.
    pub end: DateTime<Utc>,
    /// Total cost during this period.
    pub total_cost: f64,
    /// Total input tokens during this period.
    pub total_input_tokens: u64,
    /// Total output tokens during this period.
    pub total_output_tokens: u64,
    /// Breakdown by model.
    pub usage_breakdown: HashMap<String, ModelUsageSummary>,
}

/// Usage summary for a single model.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelUsageSummary {
    /// Total cost for this model.
    pub total_cost: f64,
    /// Total input tokens for this model.
    pub total_input_tokens: u64,
    /// Total output tokens for this model.
    pub total_output_tokens: u64,
    /// Number of requests made with this model.
    pub request_count: u64,
}

/// Configuration for billing alerts and budget limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillingConfig {
    /// Monthly budget limit in USD. `None` means no limit.
    pub monthly_budget: Option<f64>,
    /// Alert when usage reaches this fraction of the budget (0.0-1.0).
    pub alert_threshold: f64,
    /// Whether budget alerts are enabled.
    pub alerts_enabled: bool,
    /// Custom cost per million input tokens per model.
    pub input_cost_per_million: HashMap<String, f64>,
    /// Custom cost per million output tokens per model.
    pub output_cost_per_million: HashMap<String, f64>,
}

impl Default for BillingConfig {
    fn default() -> Self {
        Self {
            monthly_budget: None,
            alert_threshold: 0.8,
            alerts_enabled: true,
            input_cost_per_million: HashMap::new(),
            output_cost_per_million: HashMap::new(),
        }
    }
}

impl BillingConfig {
    /// Create a config with a specific monthly budget.
    pub fn with_budget(monthly_budget: f64) -> Self {
        Self {
            monthly_budget: Some(monthly_budget),
            ..Self::default()
        }
    }
}

/// An alert triggered when approaching or exceeding a budget limit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetAlert {
    /// The type of alert.
    pub alert_type: BudgetAlertType,
    /// Current cost at the time of the alert.
    pub current_cost: f64,
    /// Budget limit that triggered the alert.
    pub budget_limit: f64,
    /// Human-readable message.
    pub message: String,
    /// When the alert was triggered.
    pub timestamp: DateTime<Utc>,
}

/// Types of budget alerts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BudgetAlertType {
    /// Usage has reached the alert threshold.
    Warning,
    /// Usage has exceeded the budget limit.
    Exceeded,
}

/// Daily usage totals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyUsage {
    /// The date (UTC).
    pub date: String,
    /// Total cost for the day.
    pub total_cost: f64,
    /// Total input tokens for the day.
    pub total_input_tokens: u64,
    /// Total output tokens for the day.
    pub total_output_tokens: u64,
    /// Number of requests for the day.
    pub request_count: u64,
}

/// Manages billing records with aggregation and alerting.
#[derive(Debug, Clone)]
pub struct BillingManager {
    /// All recorded usage.
    records: Vec<UsageRecord>,
    /// Billing configuration.
    config: BillingConfig,
    /// Pending alerts.
    alerts: Vec<BudgetAlert>,
    /// Storage path for persistence (optional).
    storage_path: Option<PathBuf>,
}

impl BillingManager {
    /// Create a new BillingManager with default configuration.
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
            config: BillingConfig::default(),
            alerts: Vec::new(),
            storage_path: None,
        }
    }

    /// Create a BillingManager with custom configuration.
    pub fn with_config(config: BillingConfig) -> Self {
        Self {
            records: Vec::new(),
            config,
            alerts: Vec::new(),
            storage_path: None,
        }
    }

    /// Record a single usage event.
    ///
    /// Returns a `BudgetAlert` if the budget threshold is exceeded.
    pub fn record_usage(&mut self, record: UsageRecord) -> Result<Option<BudgetAlert>, BillingError> {
        // Check budget before recording
        let alert = self.check_budget(&record)?;

        self.records.push(record);
        self.records.sort_by_key(|r| r.timestamp);

        debug!(
            model = %self.records.last().map(|r| r.model.as_str()).unwrap_or(""),
            total_records = self.records.len(),
            "Recorded usage"
        );

        Ok(alert)
    }

    /// Get a summary for the current billing period (current calendar month).
    pub fn get_period_summary(&self) -> BillingPeriod {
        let now = Utc::now();
        let start = NaiveDateTime::new(
            NaiveDate::from_ymd_opt(now.year(), now.month(), 1).unwrap_or(now.date_naive()),
            chrono::NaiveTime::from_hms_opt(0, 0, 0).expect("midnight is always a valid time"),
        ).and_utc();
        let end = now;
        self.summarize_period(start, end)
    }

    /// Get a summary for a custom time range.
    pub fn summarize_period(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> BillingPeriod {
        let period_records: Vec<&UsageRecord> = self
            .records
            .iter()
            .filter(|r| r.timestamp >= start && r.timestamp <= end)
            .collect();

        let mut total_cost = 0.0f64;
        let mut total_input = 0u64;
        let mut total_output = 0u64;
        let mut breakdown: HashMap<String, ModelUsageSummary> = HashMap::new();

        for record in &period_records {
            total_cost += record.cost;
            total_input += record.input_tokens;
            total_output += record.output_tokens;

            let summary = breakdown.entry(record.model.clone()).or_default();
            summary.total_cost += record.cost;
            summary.total_input_tokens += record.input_tokens;
            summary.total_output_tokens += record.output_tokens;
            summary.request_count += 1;
        }

        BillingPeriod {
            start,
            end,
            total_cost,
            total_input_tokens: total_input,
            total_output_tokens: total_output,
            usage_breakdown: breakdown,
        }
    }

    /// Get a breakdown of usage by model for the current month.
    pub fn get_model_breakdown(&self) -> HashMap<String, ModelUsageSummary> {
        let summary = self.get_period_summary();
        summary.usage_breakdown
    }

    /// Get daily usage totals for the current month.
    pub fn get_daily_totals(&self) -> Vec<DailyUsage> {
        let now = Utc::now();
        let start = NaiveDateTime::new(
            NaiveDate::from_ymd_opt(now.year(), now.month(), 1).unwrap_or(now.date_naive()),
            chrono::NaiveTime::from_hms_opt(0, 0, 0).expect("midnight is always a valid time"),
        ).and_utc();

        let mut daily: HashMap<String, DailyUsage> = HashMap::new();

        for record in &self.records {
            if record.timestamp < start {
                continue;
            }

            let date_key = record.timestamp.format("%Y-%m-%d").to_string();
            let entry = daily.entry(date_key.clone()).or_insert_with(|| DailyUsage {
                date: date_key,
                total_cost: 0.0,
                total_input_tokens: 0,
                total_output_tokens: 0,
                request_count: 0,
            });

            entry.total_cost += record.cost;
            entry.total_input_tokens += record.input_tokens;
            entry.total_output_tokens += record.output_tokens;
            entry.request_count += 1;
        }

        let mut result: Vec<DailyUsage> = daily.into_values().collect();
        result.sort_by(|a, b| a.date.cmp(&b.date));
        result
    }

    /// Estimate the total monthly cost based on usage so far.
    pub fn estimate_monthly_cost(&self) -> f64 {
        let summary = self.get_period_summary();
        let now = Utc::now();

        let days_in_month = days_in_current_month(now);
        let current_day = now.day() as f64;

        if current_day == 0.0 {
            return summary.total_cost;
        }

        // Extrapolate based on average daily cost so far
        let daily_average = summary.total_cost / current_day;
        daily_average * days_in_month as f64
    }

    /// Get all pending alerts.
    pub fn get_alerts(&self) -> &[BudgetAlert] {
        &self.alerts
    }

    /// Clear all pending alerts.
    pub fn clear_alerts(&mut self) {
        self.alerts.clear();
    }

    /// Get the current billing configuration.
    pub fn config(&self) -> &BillingConfig {
        &self.config
    }

    /// Update the billing configuration.
    pub fn set_config(&mut self, config: BillingConfig) {
        self.config = config;
    }

    /// Get the total number of recorded usage events.
    pub fn record_count(&self) -> usize {
        self.records.len()
    }

    /// Get the total cost across all recorded usage.
    pub fn total_cost(&self) -> f64 {
        self.records.iter().map(|r| r.cost).sum()
    }

    /// Save billing records to disk.
    pub fn save(&self) -> Result<(), BillingError> {
        if let Some(ref path) = self.storage_path {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let data = BillingData {
                records: self.records.clone(),
                config: self.config.clone(),
            };
            let json = serde_json::to_string_pretty(&data)?;
            std::fs::write(path, json)?;
            debug!(path = %path.display(), "Saved billing data");
        }
        Ok(())
    }

    /// Load billing records from disk.
    pub fn load(&mut self, path: &PathBuf) -> Result<(), BillingError> {
        let content = std::fs::read_to_string(path)?;
        let data: BillingData = serde_json::from_str(&content)?;
        self.records = data.records;
        self.config = data.config;
        self.storage_path = Some(path.clone());
        self.records.sort_by_key(|r| r.timestamp);
        debug!(path = %path.display(), count = self.records.len(), "Loaded billing data");
        Ok(())
    }

    // --- Private helpers ---

    fn check_budget(&mut self, record: &UsageRecord) -> Result<Option<BudgetAlert>, BillingError> {
        if !self.config.alerts_enabled {
            return Ok(None);
        }

        let Some(limit) = self.config.alert_threshold_enabled_budget() else {
            return Ok(None);
        };

        let current_total: f64 = self.records.iter().map(|r| r.cost).sum::<f64>() + record.cost;

        if current_total >= limit {
            let alert = BudgetAlert {
                alert_type: BudgetAlertType::Exceeded,
                current_cost: current_total,
                budget_limit: limit,
                message: format!(
                    "Budget exceeded: ${current_total:.2} of ${limit:.2} monthly budget used"
                ),
                timestamp: Utc::now(),
            };
            self.alerts.push(alert.clone());
            return Ok(Some(alert));
        }

        // Check warning threshold
        if let Some(warning_limit) = self.config.alert_threshold_enabled_budget() {
            let warning_at = warning_limit * self.config.alert_threshold;
            let previous_total: f64 = self.records.iter().map(|r| r.cost).sum();

            if previous_total < warning_at && current_total >= warning_at {
                let alert = BudgetAlert {
                    alert_type: BudgetAlertType::Warning,
                    current_cost: current_total,
                    budget_limit: warning_limit,
                    message: format!(
                        "Approaching budget limit: ${:.2} of ${:.2} ({:.0}%) used",
                        current_total,
                        warning_limit,
                        (current_total / warning_limit) * 100.0
                    ),
                    timestamp: Utc::now(),
                };
                self.alerts.push(alert.clone());
                return Ok(Some(alert));
            }
        }

        Ok(None)
    }
}

/// Internal serialization structure for persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BillingData {
    records: Vec<UsageRecord>,
    config: BillingConfig,
}

impl Default for BillingManager {
    fn default() -> Self {
        Self::new()
    }
}

impl BillingConfig {
    /// Get the effective budget limit (only when alerts are enabled).
    fn alert_threshold_enabled_budget(&self) -> Option<f64> {
        if !self.alerts_enabled {
            return None;
        }
        self.monthly_budget
    }
}

/// Compute the number of days in the current month.
fn days_in_current_month(now: DateTime<Utc>) -> u32 {
    // Get the last day of the current month by going to the first day of next month and subtracting 1
    let year = now.year();
    let month = now.month() + 1;
    let (next_year, next_month) = if month > 12 { (year + 1, 1) } else { (year, month) };

    // Use chrono's NaiveDate to compute days in month
    chrono::NaiveDate::from_ymd_opt(next_year, next_month, 1)
        .and_then(|d| d.pred_opt())
        .map(|d| d.day())
        .unwrap_or(30)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn make_record(model: &str, input: u64, output: u64, cost: f64) -> UsageRecord {
        UsageRecord::new(model, input, output, cost)
    }

    fn make_record_at(
        model: &str,
        input: u64,
        output: u64,
        cost: f64,
        days_ago: i64,
    ) -> UsageRecord {
        let ts = Utc::now() - Duration::days(days_ago);
        UsageRecord::with_timestamp(model, input, output, cost, ts)
    }

    #[test]
    fn test_record_usage() {
        let mut mgr = BillingManager::new();
        let record = make_record("claude-sonnet", 100, 50, 0.005);
        mgr.record_usage(record).unwrap();
        assert_eq!(mgr.record_count(), 1);
    }

    #[test]
    fn test_total_cost() {
        let mut mgr = BillingManager::new();
        mgr.record_usage(make_record("claude-sonnet", 100, 50, 0.005)).unwrap();
        mgr.record_usage(make_record("claude-sonnet", 200, 100, 0.01)).unwrap();
        mgr.record_usage(make_record("claude-opus", 50, 25, 0.015)).unwrap();

        let total = mgr.total_cost();
        assert!((total - 0.03).abs() < f64::EPSILON);
    }

    #[test]
    fn test_period_summary() {
        let mut mgr = BillingManager::new();
        mgr.record_usage(make_record("claude-sonnet", 100, 50, 0.005)).unwrap();
        mgr.record_usage(make_record("claude-sonnet", 200, 100, 0.01)).unwrap();
        mgr.record_usage(make_record("claude-opus", 50, 25, 0.015)).unwrap();

        let summary = mgr.get_period_summary();
        assert!((summary.total_cost - 0.03).abs() < f64::EPSILON);
        assert_eq!(summary.total_input_tokens, 350);
        assert_eq!(summary.total_output_tokens, 175);
    }

    #[test]
    fn test_model_breakdown() {
        let mut mgr = BillingManager::new();
        mgr.record_usage(make_record("claude-sonnet", 100, 50, 0.005)).unwrap();
        mgr.record_usage(make_record("claude-sonnet", 200, 100, 0.01)).unwrap();
        mgr.record_usage(make_record("claude-opus", 50, 25, 0.015)).unwrap();

        let breakdown = mgr.get_model_breakdown();
        assert_eq!(breakdown.len(), 2);
        assert_eq!(breakdown.get("claude-sonnet").unwrap().request_count, 2);
        assert_eq!(breakdown.get("claude-opus").unwrap().request_count, 1);
    }

    #[test]
    fn test_daily_totals() {
        let mut mgr = BillingManager::new();
        mgr.record_usage(make_record("claude-sonnet", 100, 50, 0.005)).unwrap();
        mgr.record_usage(make_record("claude-sonnet", 200, 100, 0.01)).unwrap();

        let daily = mgr.get_daily_totals();
        assert!(!daily.is_empty());
        // All records are from today
        assert_eq!(daily.len(), 1);
        let today = daily.first().unwrap();
        assert!((today.total_cost - 0.015).abs() < f64::EPSILON);
        assert_eq!(today.request_count, 2);
    }

    #[test]
    fn test_estimate_monthly_cost() {
        let mut mgr = BillingManager::new();
        mgr.record_usage(make_record("claude-sonnet", 100, 50, 0.005)).unwrap();

        let estimate = mgr.estimate_monthly_cost();
        // Should extrapolate to a full month
        assert!(estimate > 0.005, "Estimate should be >= today's cost");
    }

    #[test]
    fn test_budget_warning_alert() {
        let config = BillingConfig::with_budget(1.0);
        let mut mgr = BillingManager::with_config(config);

        // Use 0.85 of budget (above 0.8 threshold)
        let record = make_record("claude-sonnet", 100, 50, 0.85);
        let alert = mgr.record_usage(record).unwrap();

        assert!(alert.is_some());
        let alert = alert.unwrap();
        assert_eq!(alert.alert_type, BudgetAlertType::Warning);
    }

    #[test]
    fn test_budget_exceeded_alert() {
        let config = BillingConfig::with_budget(1.0);
        let mut mgr = BillingManager::with_config(config);

        mgr.record_usage(make_record("claude-sonnet", 100, 50, 0.50)).unwrap();
        let alert = mgr.record_usage(make_record("claude-sonnet", 100, 50, 0.60)).unwrap();

        assert!(alert.is_some());
        let alert = alert.unwrap();
        assert_eq!(alert.alert_type, BudgetAlertType::Exceeded);
    }

    #[test]
    fn test_no_alert_when_disabled() {
        let config = BillingConfig {
            monthly_budget: Some(1.0),
            alert_threshold: 0.8,
            alerts_enabled: false,
            ..Default::default()
        };
        let mut mgr = BillingManager::with_config(config);

        let record = make_record("claude-sonnet", 100, 50, 5.0);
        let alert = mgr.record_usage(record).unwrap();
        assert!(alert.is_none());
    }

    #[test]
    fn test_clear_alerts() {
        let config = BillingConfig::with_budget(1.0);
        let mut mgr = BillingManager::with_config(config);

        mgr.record_usage(make_record("claude-sonnet", 100, 50, 5.0)).unwrap();
        assert!(!mgr.get_alerts().is_empty());

        mgr.clear_alerts();
        assert!(mgr.get_alerts().is_empty());
    }

    #[test]
    fn test_usage_record_total_tokens() {
        let record = make_record("claude-sonnet", 100, 50, 0.005);
        assert_eq!(record.total_tokens(), 150);
    }

    #[test]
    fn test_custom_period_summary() {
        let mut mgr = BillingManager::new();
        mgr.record_usage(make_record_at("claude-sonnet", 100, 50, 0.005, 1)).unwrap();
        mgr.record_usage(make_record_at("claude-sonnet", 200, 100, 0.01, 2)).unwrap();
        mgr.record_usage(make_record_at("claude-sonnet", 50, 25, 0.003, 30)).unwrap();

        let now = Utc::now();
        let week_ago = now - Duration::days(7);
        let summary = mgr.summarize_period(week_ago, now);

        // Only records from the last 7 days should be included
        assert_eq!(summary.total_input_tokens, 300);
        assert_eq!(summary.total_output_tokens, 150);
    }
}
