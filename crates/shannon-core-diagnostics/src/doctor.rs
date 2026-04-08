//! System doctor for health checks

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Doctor check result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorCheck {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub passed: bool,
    pub details: String,
    pub duration_ms: u64,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl DoctorCheck {
    pub fn passed(name: String, description: String, details: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            description,
            passed: true,
            details,
            duration_ms: 0,
            timestamp: chrono::Utc::now(),
        }
    }

    pub fn failed(name: String, description: String, details: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            description,
            passed: false,
            details,
            duration_ms: 0,
            timestamp: chrono::Utc::now(),
        }
    }
}

/// System health status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemHealth {
    pub overall_status: HealthStatus,
    pub checks: Vec<DoctorCheck>,
    pub recommendations: Vec<String>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Health status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
}

/// System doctor
pub struct Doctor {
    checks: Vec<Box<dyn DoctorCheckFn + Send + Sync>>,
}

impl Doctor {
    pub fn new() -> Self {
        Self {
            checks: Vec::new(),
        }
    }

    /// Register a check
    pub fn register_check(&mut self, check: Box<dyn DoctorCheckFn + Send + Sync>) {
        self.checks.push(check);
    }

    /// Run all checks
    pub async fn run_checks(&self) -> SystemHealth {
        let mut results = Vec::new();
        let mut passed_count = 0;
        let mut failed_count = 0;

        for check in &self.checks {
            let start = std::time::Instant::now();
            let result = check.run().await;
            let duration = start.elapsed().as_millis() as u64;

            let mut check_result = result;
            check_result.duration_ms = duration;

            if check_result.passed {
                passed_count += 1;
            } else {
                failed_count += 1;
            }

            results.push(check_result);
        }

        let overall_status = if failed_count == 0 {
            HealthStatus::Healthy
        } else if passed_count > failed_count {
            HealthStatus::Degraded
        } else {
            HealthStatus::Unhealthy
        };

        let recommendations = self.generate_recommendations(&results);

        SystemHealth {
            overall_status,
            checks: results,
            recommendations,
            timestamp: chrono::Utc::now(),
        }
    }

    fn generate_recommendations(&self, checks: &[DoctorCheck]) -> Vec<String> {
        checks
            .iter()
            .filter(|c| !c.passed)
            .map(|c| format!("Fix failing check: {}", c.name))
            .collect()
    }
}

impl Default for Doctor {
    fn default() -> Self {
        Self::new()
    }
}

/// Doctor check function trait
pub trait DoctorCheckFn {
    fn run(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = DoctorCheck> + Send>>;
}

/// Function-based check implementation
impl<F, Fut> DoctorCheckFn for F
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = DoctorCheck> + Send + 'static,
{
    fn run(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = DoctorCheck> + Send>> {
        Box::pin(self())
    }
}
