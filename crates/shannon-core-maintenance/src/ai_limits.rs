//! AI limits tracking

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Usage type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum UsageType {
    InputTokens,
    OutputTokens,
    TotalTokens,
    ApiCalls,
    ToolCalls,
}

/// Token usage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub usage_type: UsageType,
    pub amount: u64,
    pub model: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// AI limits tracker
pub struct AiLimitsTracker {
    usage: HashMap<UsageType, Vec<TokenUsage>>,
    daily_limit: Option<u64>,
    monthly_limit: Option<u64>,
}

impl AiLimitsTracker {
    pub fn new() -> Self {
        Self {
            usage: HashMap::new(),
            daily_limit: None,
            monthly_limit: None,
        }
    }

    /// Set daily limit
    pub fn set_daily_limit(&mut self, limit: u64) {
        self.daily_limit = Some(limit);
    }

    /// Set monthly limit
    pub fn set_monthly_limit(&mut self, limit: u64) {
        self.monthly_limit = Some(limit);
    }

    /// Record token usage
    pub fn record_usage(&mut self, usage_type: UsageType, amount: u64, model: String) {
        let usage = TokenUsage {
            usage_type,
            amount,
            model,
            timestamp: chrono::Utc::now(),
        };

        self.usage.entry(usage_type).or_default().push(usage);
    }

    /// Get total usage for a type
    pub fn get_usage(&self, usage_type: UsageType) -> u64 {
        self.usage.get(&usage_type)
            .map(|v| v.iter().map(|u| u.amount).sum())
            .unwrap_or(0)
    }

    /// Get usage by model
    pub fn get_usage_by_model(&self, model: &str) -> HashMap<UsageType, u64> {
        let mut result = HashMap::new();

        for (usage_type, usages) in &self.usage {
            let total: u64 = usages.iter()
                .filter(|u| u.model == model)
                .map(|u| u.amount)
                .sum();

            if total > 0 {
                result.insert(*usage_type, total);
            }
        }

        result
    }

    /// Check if daily limit exceeded
    pub fn check_daily_limit(&self) -> bool {
        if let Some(limit) = self.daily_limit {
            let daily_total = self.get_usage(UsageType::TotalTokens);
            daily_total >= limit
        } else {
            false
        }
    }

    /// Check if monthly limit exceeded
    pub fn check_monthly_limit(&self) -> bool {
        if let Some(limit) = self.monthly_limit {
            let monthly_total = self.get_usage(UsageType::TotalTokens);
            monthly_total >= limit
        } else {
            false
        }
    }

    /// Reset usage for a type
    pub fn reset_usage(&mut self, usage_type: UsageType) {
        self.usage.remove(&usage_type);
    }

    /// Reset all usage
    pub fn reset_all(&mut self) {
        self.usage.clear();
    }

    /// Get all usage data
    pub fn get_all_usage(&self) -> &HashMap<UsageType, Vec<TokenUsage>> {
        &self.usage
    }
}

impl Default for AiLimitsTracker {
    fn default() -> Self {
        Self::new()
    }
}
