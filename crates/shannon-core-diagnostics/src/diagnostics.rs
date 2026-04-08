//! Diagnostics engine

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Diagnostic category
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum DiagnosticCategory {
    Performance,
    Security,
    Reliability,
    Compatibility,
    Resource,
    Configuration,
}

/// Diagnostic level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum DiagnosticLevel {
    Info,
    Warning,
    Error,
    Critical,
}

/// Diagnostic result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticResult {
    pub id: Uuid,
    pub category: DiagnosticCategory,
    pub level: DiagnosticLevel,
    pub title: String,
    pub description: String,
    pub recommendations: Vec<String>,
    pub metadata: HashMap<String, String>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl DiagnosticResult {
    pub fn new(
        category: DiagnosticCategory,
        level: DiagnosticLevel,
        title: String,
        description: String,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            category,
            level,
            title,
            description,
            recommendations: Vec::new(),
            metadata: HashMap::new(),
            timestamp: chrono::Utc::now(),
        }
    }

    pub fn with_recommendations(mut self, recommendations: Vec<String>) -> Self {
        self.recommendations = recommendations;
        self
    }

    pub fn with_metadata(mut self, metadata: HashMap<String, String>) -> Self {
        self.metadata = metadata;
        self
    }
}

/// Diagnostics engine
pub struct DiagnosticsEngine {
    results: Vec<DiagnosticResult>,
    enabled_categories: Vec<DiagnosticCategory>,
}

impl DiagnosticsEngine {
    pub fn new() -> Self {
        Self {
            results: Vec::new(),
            enabled_categories: vec![
                DiagnosticCategory::Performance,
                DiagnosticCategory::Security,
                DiagnosticCategory::Reliability,
                DiagnosticCategory::Compatibility,
                DiagnosticCategory::Resource,
                DiagnosticCategory::Configuration,
            ],
        }
    }

    /// Run a diagnostic check
    pub async fn run_check<F, Fut>(&mut self, check: F) -> DiagnosticResult
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = DiagnosticResult>,
    {
        let result = check().await;
        self.results.push(result.clone());
        result
    }

    /// Get all results
    pub fn get_results(&self) -> &[DiagnosticResult] {
        &self.results
    }

    /// Get results by category
    pub fn get_results_by_category(&self, category: DiagnosticCategory) -> Vec<&DiagnosticResult> {
        self.results
            .iter()
            .filter(|r| r.category == category)
            .collect()
    }

    /// Get results by level
    pub fn get_results_by_level(&self, level: DiagnosticLevel) -> Vec<&DiagnosticResult> {
        self.results
            .iter()
            .filter(|r| r.level == level)
            .collect()
    }

    /// Clear all results
    pub fn clear(&mut self) {
        self.results.clear();
    }

    /// Enable a category
    pub fn enable_category(&mut self, category: DiagnosticCategory) {
        if !self.enabled_categories.contains(&category) {
            self.enabled_categories.push(category);
        }
    }

    /// Disable a category
    pub fn disable_category(&mut self, category: DiagnosticCategory) {
        self.enabled_categories.retain(|c| *c != category);
    }

    /// Check if category is enabled
    pub fn is_category_enabled(&self, category: DiagnosticCategory) -> bool {
        self.enabled_categories.contains(&category)
    }
}

impl Default for DiagnosticsEngine {
    fn default() -> Self {
        Self::new()
    }
}
