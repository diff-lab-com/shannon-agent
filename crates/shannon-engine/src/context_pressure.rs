//! # Context Pressure Monitoring
//!
//! Tracks how full the model's context window is and recommends compaction
//! actions before hitting hard limits.
//!
//! ## Pressure Levels
//!
//! | Level    | Usage  | Action                             |
//! |----------|--------|------------------------------------|
//! | Low      | < 50%  | None                               |
//! | Normal   | 50-75% | None                               |
//! | High     | 75-85% | Suggest compaction                 |
//! | Critical | 85-95% | Aggressive compaction needed       |
//! | Emergency| > 95%  | Must compact immediately           |
//!
//! ## Usage
//!
//! ```ignore
//! let monitor = ContextPressureMonitor::new(200_000);
//! let metrics = monitor.assess(160_000, 42);
//! if monitor.should_auto_compact(&metrics) {
//!     let strategy = monitor.recommended_compaction_strategy(&metrics);
//!     // run compaction with the chosen strategy
//! }
//! ```

use crate::compact::CompactStrategy;
use std::sync::Mutex;

// ============================================================================
// Pressure Level
// ============================================================================

/// Context pressure levels, ordered from least to most severe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PressureLevel {
    /// < 50% - plenty of room
    Low,
    /// 50-75% - normal operation
    Normal,
    /// 75-85% - should start compacting
    High,
    /// 85-95% - aggressive compaction needed
    Critical,
    /// > 95% - emergency, must compact now
    Emergency,
}

impl std::fmt::Display for PressureLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PressureLevel::Low => write!(f, "LOW"),
            PressureLevel::Normal => write!(f, "NORMAL"),
            PressureLevel::High => write!(f, "HIGH"),
            PressureLevel::Critical => write!(f, "CRITICAL"),
            PressureLevel::Emergency => write!(f, "EMERGENCY"),
        }
    }
}

impl PressureLevel {
    /// Return a short emoji-like indicator for terminal display.
    pub fn indicator(&self) -> &'static str {
        match self {
            PressureLevel::Low => ".",
            PressureLevel::Normal => "-",
            PressureLevel::High => "!",
            PressureLevel::Critical => "!!",
            PressureLevel::Emergency => "!!!",
        }
    }
}

// ============================================================================
// Pressure Recommendation
// ============================================================================

/// Recommended action based on current pressure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PressureRecommendation {
    /// No action needed.
    None,
    /// Consider compacting older messages.
    SuggestCompact,
    /// Should compact now.
    ShouldCompact,
    /// Must compact - new messages may fail.
    MustCompact,
    /// Suggest switching to summary-only mode for older context.
    SummarizeOlder,
}

impl std::fmt::Display for PressureRecommendation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PressureRecommendation::None => write!(f, "no action needed"),
            PressureRecommendation::SuggestCompact => write!(f, "consider compacting"),
            PressureRecommendation::ShouldCompact => write!(f, "should compact now"),
            PressureRecommendation::MustCompact => write!(f, "must compact immediately"),
            PressureRecommendation::SummarizeOlder => write!(f, "summarize older context"),
        }
    }
}

// ============================================================================
// Pressure Metrics
// ============================================================================

/// Snapshot of context pressure at a point in time.
#[derive(Debug, Clone)]
pub struct PressureMetrics {
    /// Current token usage estimate.
    pub current_tokens: usize,
    /// Maximum context window size.
    pub max_tokens: usize,
    /// Usage ratio (0.0 - 1.0+).
    pub usage_ratio: f64,
    /// Current pressure level.
    pub level: PressureLevel,
    /// Number of messages in conversation.
    pub message_count: usize,
    /// Estimated tokens saved by last compaction.
    pub last_compaction_savings: usize,
    /// Recommended action.
    pub recommendation: PressureRecommendation,
}

impl PressureMetrics {
    /// Remaining tokens before the context window is full.
    pub fn remaining_tokens(&self) -> usize {
        self.max_tokens.saturating_sub(self.current_tokens)
    }

    /// Percentage of context remaining, as 0.0 - 1.0.
    pub fn remaining_ratio(&self) -> f64 {
        1.0 - self.usage_ratio
    }
}

// ============================================================================
// Context Pressure Monitor
// ============================================================================

/// Monitor for tracking context window pressure across a conversation.
///
/// Thread-safe: the only mutable state (`last_compaction_savings`) is
/// protected by a [`Mutex`].
pub struct ContextPressureMonitor {
    max_tokens: usize,
    warning_threshold: f64,
    critical_threshold: f64,
    emergency_threshold: f64,
    last_compaction_savings: Mutex<usize>,
}

impl std::fmt::Debug for ContextPressureMonitor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContextPressureMonitor")
            .field("max_tokens", &self.max_tokens)
            .field("warning_threshold", &self.warning_threshold)
            .field("critical_threshold", &self.critical_threshold)
            .field("emergency_threshold", &self.emergency_threshold)
            .field("last_compaction_savings", &self.last_compaction_savings)
            .finish()
    }
}

// --------------- constants ---------------

/// Default warning (high) threshold: 75%.
pub const DEFAULT_WARNING_THRESHOLD: f64 = 0.75;
/// Default critical threshold: 85%.
pub const DEFAULT_CRITICAL_THRESHOLD: f64 = 0.85;
/// Default emergency threshold: 95%.
pub const DEFAULT_EMERGENCY_THRESHOLD: f64 = 0.95;

/// Below this ratio the pressure is considered low.
const LOW_THRESHOLD: f64 = 0.50;

// --------------- impl ---------------

impl ContextPressureMonitor {
    /// Create a new monitor for the given maximum context window size (in
    /// tokens).
    ///
    /// Uses sensible defaults for thresholds:
    /// - **warning** (High): 75%
    /// - **critical**: 85%
    /// - **emergency**: 95%
    pub fn new(max_tokens: usize) -> Self {
        Self {
            max_tokens,
            warning_threshold: DEFAULT_WARNING_THRESHOLD,
            critical_threshold: DEFAULT_CRITICAL_THRESHOLD,
            emergency_threshold: DEFAULT_EMERGENCY_THRESHOLD,
            last_compaction_savings: Mutex::new(0),
        }
    }

    /// Create a monitor with custom thresholds.
    ///
    /// Values should be in (0.0, 1.0) and satisfy
    /// `warning < critical < emergency`.
    pub fn with_thresholds(
        max_tokens: usize,
        warning_threshold: f64,
        critical_threshold: f64,
        emergency_threshold: f64,
    ) -> Self {
        Self {
            max_tokens,
            warning_threshold,
            critical_threshold,
            emergency_threshold,
            last_compaction_savings: Mutex::new(0),
        }
    }

    /// Return the configured max token count.
    pub fn max_tokens(&self) -> usize {
        self.max_tokens
    }

    // ---- classification ----

    /// Classify a usage ratio into a [`PressureLevel`].
    fn classify_level(&self, ratio: f64) -> PressureLevel {
        if ratio < LOW_THRESHOLD {
            PressureLevel::Low
        } else if ratio < self.warning_threshold {
            PressureLevel::Normal
        } else if ratio < self.critical_threshold {
            PressureLevel::High
        } else if ratio < self.emergency_threshold {
            PressureLevel::Critical
        } else {
            PressureLevel::Emergency
        }
    }

    /// Derive a [`PressureRecommendation`] from a pressure level.
    fn classify_recommendation(level: PressureLevel) -> PressureRecommendation {
        match level {
            PressureLevel::Low | PressureLevel::Normal => PressureRecommendation::None,
            PressureLevel::High => PressureRecommendation::SuggestCompact,
            PressureLevel::Critical => PressureRecommendation::ShouldCompact,
            PressureLevel::Emergency => PressureRecommendation::MustCompact,
        }
    }

    // ---- public API ----

    /// Assess current context pressure.
    ///
    /// Returns a [`PressureMetrics`] snapshot that can be used for display,
    /// logging, or driving compaction decisions.
    pub fn assess(&self, current_tokens: usize, message_count: usize) -> PressureMetrics {
        let usage_ratio = if self.max_tokens > 0 {
            current_tokens as f64 / self.max_tokens as f64
        } else {
            1.0
        };

        let level = self.classify_level(usage_ratio);
        let recommendation = Self::classify_recommendation(level);

        let last_compaction_savings = self
            .last_compaction_savings
            .lock()
            .map(|guard| *guard)
            .unwrap_or(0);

        PressureMetrics {
            current_tokens,
            max_tokens: self.max_tokens,
            usage_ratio,
            level,
            message_count,
            last_compaction_savings,
            recommendation,
        }
    }

    /// Returns `true` when the pressure is at Critical or above, indicating
    /// that automatic compaction should be triggered.
    pub fn should_auto_compact(&self, metrics: &PressureMetrics) -> bool {
        matches!(
            metrics.level,
            PressureLevel::Critical | PressureLevel::Emergency
        )
    }

    /// Pick a [`CompactStrategy`] appropriate for the current pressure level.
    ///
    /// | Level    | Strategy                |
    /// |----------|-------------------------|
    /// | Low      | SummarizeOld (mild)     |
    /// | Normal   | SummarizeOld            |
    /// | High     | GroupCompress           |
    /// | Critical | SummarizeOld (aggressive)|
    /// | Emergency| TruncateOld             |
    pub fn recommended_compaction_strategy(&self, metrics: &PressureMetrics) -> CompactStrategy {
        match metrics.level {
            PressureLevel::Low | PressureLevel::Normal => CompactStrategy::SummarizeOld,
            PressureLevel::High => CompactStrategy::GroupCompress,
            PressureLevel::Critical => CompactStrategy::SummarizeOld,
            PressureLevel::Emergency => CompactStrategy::TruncateOld,
        }
    }

    /// Record that a compaction event saved the given number of tokens.
    pub fn record_compaction(&self, tokens_saved: usize) {
        if let Ok(mut guard) = self.last_compaction_savings.lock() {
            *guard = tokens_saved;
        }
    }

    /// Produce a human-readable pressure summary for the TUI.
    ///
    /// Example output:
    /// ```text
    /// Context: 160,000 / 200,000 tokens (80.0%) [HIGH !]
    /// Messages: 42 | Last compaction saved: 12,500 tokens
    /// Recommendation: consider compacting
    /// ```
    pub fn format_pressure_summary(&self, metrics: &PressureMetrics) -> String {
        let pct = metrics.usage_ratio * 100.0;
        let savings = metrics.last_compaction_savings;
        format!(
            "Context: {} / {} tokens ({:.1}%) [{} {}]\n\
             Messages: {} | Last compaction saved: {} tokens\n\
             Recommendation: {}",
            format_tokens(metrics.current_tokens),
            format_tokens(metrics.max_tokens),
            pct,
            metrics.level,
            metrics.level.indicator(),
            metrics.message_count,
            format_tokens(savings),
            metrics.recommendation,
        )
    }
}

// --------------- helpers ---------------

/// Format a token count with comma separators (e.g. 160,000).
fn format_tokens(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    // helper to build a monitor with a 200k window
    fn test_monitor() -> ContextPressureMonitor {
        ContextPressureMonitor::new(200_000)
    }

    // ---- PressureLevel classification ----

    #[test]
    fn test_level_low() {
        let m = test_monitor();
        assert_eq!(m.classify_level(0.0), PressureLevel::Low);
        assert_eq!(m.classify_level(0.25), PressureLevel::Low);
        assert_eq!(m.classify_level(0.49), PressureLevel::Low);
    }

    #[test]
    fn test_level_normal() {
        let m = test_monitor();
        assert_eq!(m.classify_level(0.50), PressureLevel::Normal);
        assert_eq!(m.classify_level(0.60), PressureLevel::Normal);
        assert_eq!(m.classify_level(0.74), PressureLevel::Normal);
    }

    #[test]
    fn test_level_high() {
        let m = test_monitor();
        // warning threshold is 0.75
        assert_eq!(m.classify_level(0.75), PressureLevel::High);
        assert_eq!(m.classify_level(0.80), PressureLevel::High);
        assert_eq!(m.classify_level(0.84), PressureLevel::High);
    }

    #[test]
    fn test_level_critical() {
        let m = test_monitor();
        // critical threshold is 0.85
        assert_eq!(m.classify_level(0.85), PressureLevel::Critical);
        assert_eq!(m.classify_level(0.90), PressureLevel::Critical);
        assert_eq!(m.classify_level(0.94), PressureLevel::Critical);
    }

    #[test]
    fn test_level_emergency() {
        let m = test_monitor();
        // emergency threshold is 0.95
        assert_eq!(m.classify_level(0.95), PressureLevel::Emergency);
        assert_eq!(m.classify_level(1.0), PressureLevel::Emergency);
        assert_eq!(m.classify_level(1.5), PressureLevel::Emergency);
    }

    // ---- Recommendation logic ----

    #[test]
    fn test_recommendation_none_at_low() {
        assert_eq!(
            ContextPressureMonitor::classify_recommendation(PressureLevel::Low),
            PressureRecommendation::None
        );
        assert_eq!(
            ContextPressureMonitor::classify_recommendation(PressureLevel::Normal),
            PressureRecommendation::None
        );
    }

    #[test]
    fn test_recommendation_suggest_at_high() {
        assert_eq!(
            ContextPressureMonitor::classify_recommendation(PressureLevel::High),
            PressureRecommendation::SuggestCompact
        );
    }

    #[test]
    fn test_recommendation_should_at_critical() {
        assert_eq!(
            ContextPressureMonitor::classify_recommendation(PressureLevel::Critical),
            PressureRecommendation::ShouldCompact
        );
    }

    #[test]
    fn test_recommendation_must_at_emergency() {
        assert_eq!(
            ContextPressureMonitor::classify_recommendation(PressureLevel::Emergency),
            PressureRecommendation::MustCompact
        );
    }

    // ---- Full assess() round-trip ----

    #[test]
    fn test_assess_low_pressure() {
        let m = test_monitor();
        let metrics = m.assess(40_000, 10);
        assert_eq!(metrics.current_tokens, 40_000);
        assert_eq!(metrics.max_tokens, 200_000);
        assert!((metrics.usage_ratio - 0.2).abs() < 0.001);
        assert_eq!(metrics.level, PressureLevel::Low);
        assert_eq!(metrics.recommendation, PressureRecommendation::None);
        assert_eq!(metrics.remaining_tokens(), 160_000);
    }

    #[test]
    fn test_assess_emergency_pressure() {
        let m = test_monitor();
        let metrics = m.assess(196_000, 100);
        assert_eq!(metrics.level, PressureLevel::Emergency);
        assert_eq!(metrics.recommendation, PressureRecommendation::MustCompact);
        assert_eq!(metrics.remaining_tokens(), 4_000);
    }

    #[test]
    fn test_assess_zero_max_tokens() {
        let m = ContextPressureMonitor::new(0);
        let metrics = m.assess(100, 5);
        // usage_ratio is 1.0 when max is 0
        assert!((metrics.usage_ratio - 1.0).abs() < f64::EPSILON);
        assert_eq!(metrics.level, PressureLevel::Emergency);
    }

    // ---- should_auto_compact ----

    #[test]
    fn test_should_auto_compact_below_critical() {
        let m = test_monitor();
        // 60% usage => Normal
        let metrics = m.assess(120_000, 30);
        assert!(!m.should_auto_compact(&metrics));
    }

    #[test]
    fn test_should_auto_compact_at_critical() {
        let m = test_monitor();
        // 90% usage => Critical
        let metrics = m.assess(180_000, 80);
        assert!(m.should_auto_compact(&metrics));
    }

    #[test]
    fn test_should_auto_compact_at_emergency() {
        let m = test_monitor();
        let metrics = m.assess(198_000, 100);
        assert!(m.should_auto_compact(&metrics));
    }

    // ---- Compaction strategy selection ----

    #[test]
    fn test_strategy_summarize_at_low() {
        let m = test_monitor();
        let metrics = m.assess(20_000, 5);
        assert_eq!(
            m.recommended_compaction_strategy(&metrics),
            CompactStrategy::SummarizeOld
        );
    }

    #[test]
    fn test_strategy_summarize_at_normal() {
        let m = test_monitor();
        let metrics = m.assess(120_000, 25);
        assert_eq!(
            m.recommended_compaction_strategy(&metrics),
            CompactStrategy::SummarizeOld
        );
    }

    #[test]
    fn test_strategy_group_compress_at_high() {
        let m = test_monitor();
        // 80% => High
        let metrics = m.assess(160_000, 40);
        assert_eq!(
            m.recommended_compaction_strategy(&metrics),
            CompactStrategy::GroupCompress
        );
    }

    #[test]
    fn test_strategy_summarize_at_critical() {
        let m = test_monitor();
        // 90% => Critical
        let metrics = m.assess(180_000, 60);
        assert_eq!(
            m.recommended_compaction_strategy(&metrics),
            CompactStrategy::SummarizeOld
        );
    }

    #[test]
    fn test_strategy_truncate_at_emergency() {
        let m = test_monitor();
        let metrics = m.assess(196_000, 90);
        assert_eq!(
            m.recommended_compaction_strategy(&metrics),
            CompactStrategy::TruncateOld
        );
    }

    // ---- record_compaction ----

    #[test]
    fn test_record_compaction_updates_metrics() {
        let m = test_monitor();
        m.record_compaction(15_000);

        let metrics = m.assess(50_000, 10);
        assert_eq!(metrics.last_compaction_savings, 15_000);
    }

    #[test]
    fn test_record_compaction_overwrites_previous() {
        let m = test_monitor();
        m.record_compaction(10_000);
        m.record_compaction(20_000);

        let metrics = m.assess(50_000, 10);
        assert_eq!(metrics.last_compaction_savings, 20_000);
    }

    // ---- Thread safety ----

    #[test]
    fn test_thread_safety() {
        use std::sync::Arc;
        use std::thread;

        let monitor = Arc::new(test_monitor());
        let mut handles = Vec::new();

        for i in 0..4 {
            let mon = Arc::clone(&monitor);
            handles.push(thread::spawn(move || {
                // each thread records a different savings value
                mon.record_compaction(i * 1000);
                let metrics = mon.assess(100_000, 10);
                // Should not panic under concurrent access
                assert_eq!(metrics.max_tokens, 200_000);
            }));
        }

        for h in handles {
            h.join().expect("thread should not panic");
        }

        // final value should be one of the recorded values (0, 1000, 2000, or 3000)
        let metrics = monitor.assess(100_000, 10);
        assert!((0..=3).contains(&(metrics.last_compaction_savings / 1000)));
    }

    // ---- PressureMetrics helpers ----

    #[test]
    fn test_remaining_ratio() {
        let m = test_monitor();
        let metrics = m.assess(50_000, 10);
        assert!((metrics.remaining_ratio() - 0.75).abs() < 0.001);
    }

    // ---- Display impls ----

    #[test]
    fn test_pressure_level_display() {
        assert_eq!(PressureLevel::Low.to_string(), "LOW");
        assert_eq!(PressureLevel::Normal.to_string(), "NORMAL");
        assert_eq!(PressureLevel::High.to_string(), "HIGH");
        assert_eq!(PressureLevel::Critical.to_string(), "CRITICAL");
        assert_eq!(PressureLevel::Emergency.to_string(), "EMERGENCY");
    }

    #[test]
    fn test_pressure_recommendation_display() {
        assert_eq!(PressureRecommendation::None.to_string(), "no action needed");
        assert_eq!(
            PressureRecommendation::SuggestCompact.to_string(),
            "consider compacting"
        );
        assert_eq!(
            PressureRecommendation::ShouldCompact.to_string(),
            "should compact now"
        );
        assert_eq!(
            PressureRecommendation::MustCompact.to_string(),
            "must compact immediately"
        );
        assert_eq!(
            PressureRecommendation::SummarizeOlder.to_string(),
            "summarize older context"
        );
    }

    // ---- format_pressure_summary ----

    #[test]
    fn test_format_pressure_summary_high() {
        let m = test_monitor();
        m.record_compaction(12_500);
        let metrics = m.assess(160_000, 42);
        let summary = m.format_pressure_summary(&metrics);

        assert!(
            summary.contains("160,000"),
            "should contain formatted current tokens"
        );
        assert!(
            summary.contains("200,000"),
            "should contain formatted max tokens"
        );
        assert!(summary.contains("80.0%"), "should contain percentage");
        assert!(summary.contains("HIGH"), "should contain level name");
        assert!(summary.contains("42"), "should contain message count");
        assert!(
            summary.contains("12,500"),
            "should contain last compaction savings"
        );
        assert!(
            summary.contains("consider compacting"),
            "should contain recommendation text"
        );
    }

    #[test]
    fn test_format_pressure_summary_emergency() {
        let m = test_monitor();
        let metrics = m.assess(198_000, 150);
        let summary = m.format_pressure_summary(&metrics);

        assert!(summary.contains("EMERGENCY"));
        assert!(summary.contains("must compact immediately"));
    }

    // ---- Custom thresholds ----

    #[test]
    fn test_custom_thresholds() {
        // Very tight thresholds
        let m = ContextPressureMonitor::with_thresholds(100_000, 0.60, 0.80, 0.90);
        let metrics = m.assess(65_000, 20); // 65% => High (because >= 0.60 warning)
        assert_eq!(metrics.level, PressureLevel::High);
    }

    // ---- format_tokens helper ----

    #[test]
    fn test_format_tokens() {
        assert_eq!(format_tokens(0), "0");
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(1_000), "1,000");
        assert_eq!(format_tokens(1_000_000), "1,000,000");
        assert_eq!(format_tokens(200_000), "200,000");
    }

    // ---- PressureLevel ordering ----

    #[test]
    fn test_pressure_level_ordering() {
        assert!(PressureLevel::Low < PressureLevel::Normal);
        assert!(PressureLevel::Normal < PressureLevel::High);
        assert!(PressureLevel::High < PressureLevel::Critical);
        assert!(PressureLevel::Critical < PressureLevel::Emergency);
    }

    // ---- indicator ----

    #[test]
    fn test_indicators() {
        assert_eq!(PressureLevel::Low.indicator(), ".");
        assert_eq!(PressureLevel::Normal.indicator(), "-");
        assert_eq!(PressureLevel::High.indicator(), "!");
        assert_eq!(PressureLevel::Critical.indicator(), "!!");
        assert_eq!(PressureLevel::Emergency.indicator(), "!!!");
    }

    // ---- Debug impl for monitor ----

    #[test]
    fn test_debug_impl() {
        let m = test_monitor();
        let debug_str = format!("{m:?}");
        assert!(debug_str.contains("ContextPressureMonitor"));
        assert!(debug_str.contains("200000"));
    }
}
