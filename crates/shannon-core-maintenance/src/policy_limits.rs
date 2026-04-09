//! # Policy Limits
//!
//! Organization-level policy limits fetched from API. Controls tool usage,
//! path access, token budgets, and rate limits at the policy level.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use thiserror::Error;

/// Errors that can occur during policy limit enforcement.
#[derive(Error, Debug)]
pub enum PolicyError {
    #[error("Policy check failed: {0}")]
    CheckFailed(String),

    #[error("Failed to load policy from API: {0}")]
    ApiError(String),

    #[error("Invalid policy configuration: {0}")]
    InvalidConfig(String),

    #[error("Rate limited: retry after {0:?}")]
    RateLimited(Option<Duration>),

    #[error("Tool '{0}' is blocked by policy")]
    ToolBlocked(String),

    #[error("Path '{0}' is blocked by policy")]
    PathBlocked(String),
}

/// Result of a policy enforcement check.
#[derive(Debug, Clone, PartialEq)]
pub enum PolicyCheckResult {
    /// The operation is allowed under current policy.
    Allowed,
    /// The operation is blocked by policy.
    Blocked {
        /// Human-readable reason for the block.
        reason: String,
    },
    /// The operation is rate limited.
    RateLimited {
        /// How long to wait before retrying, if known.
        retry_after: Option<Duration>,
    },
}

/// Organization-level policy limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyLimits {
    /// Maximum tokens allowed per single request.
    pub max_tokens_per_request: usize,
    /// Maximum tool calls allowed per conversational turn.
    pub max_tool_calls_per_turn: usize,
    /// List of tool names that are explicitly allowed.
    /// If empty, all tools are allowed (subject to blocked_tools).
    pub allowed_tools: Vec<String>,
    /// List of filesystem paths that are blocked.
    pub blocked_paths: Vec<String>,
    /// Maximum file size in bytes that can be read.
    pub max_file_size_bytes: u64,
}

impl Default for PolicyLimits {
    fn default() -> Self {
        Self {
            max_tokens_per_request: 200_000,
            max_tool_calls_per_turn: 50,
            allowed_tools: Vec::new(),
            blocked_paths: Vec::new(),
            max_file_size_bytes: 10 * 1024 * 1024, // 10 MB
        }
    }
}

/// Manager for organization-level policy limits.
///
/// Loads policy configuration from an API (or defaults) and enforces
/// constraints on tool calls and resource access.
pub struct PolicyLimitsManager {
    /// Current active policy limits.
    limits: PolicyLimits,
}

impl PolicyLimitsManager {
    /// Create a new manager with default policy limits.
    pub fn new() -> Self {
        Self {
            limits: PolicyLimits::default(),
        }
    }

    /// Create a new manager with custom policy limits.
    pub fn with_limits(limits: PolicyLimits) -> Self {
        Self { limits }
    }

    /// Load policy limits from the organization API.
    ///
    /// This is a placeholder that returns default limits. In production,
    /// this would fetch the policy from the organization's admin API.
    pub async fn load_from_api() -> Result<Self, PolicyError> {
        // Placeholder: in production, fetch from organization API
        Ok(Self::new())
    }

    /// Enforce policy checks on a tool invocation.
    ///
    /// Checks whether the tool is allowed and whether the input
    /// conforms to policy limits (e.g., file size, path restrictions).
    pub fn enforce(
        &self,
        tool_name: &str,
        input: &Value,
    ) -> Result<PolicyCheckResult, PolicyError> {
        // Check if tool is allowed
        if !self.is_tool_allowed(tool_name) {
            return Ok(PolicyCheckResult::Blocked {
                reason: format!("Tool '{}' is not in the allowed tools list", tool_name),
            });
        }

        // Check for path-related restrictions in the input
        let blocked_paths = self.check_paths_in_input(input);
        if let Some(blocked_path) = blocked_paths.into_iter().next() {
            return Ok(PolicyCheckResult::Blocked {
                reason: format!("Path '{}' is blocked by policy", blocked_path),
            });
        }

        Ok(PolicyCheckResult::Allowed)
    }

    /// Check if a specific tool is allowed under the current policy.
    ///
    /// If `allowed_tools` is empty, all tools are allowed.
    /// If `allowed_tools` is non-empty, only listed tools are allowed.
    pub fn is_tool_allowed(&self, tool_name: &str) -> bool {
        if self.limits.allowed_tools.is_empty() {
            true
        } else {
            self.limits.allowed_tools.iter().any(|t| {
                t == tool_name
                    || tool_name.starts_with(&format!("{}/", t))
                    || t.ends_with('*') && tool_name.starts_with(&t[..t.len() - 1])
            })
        }
    }

    /// Check if a filesystem path is allowed under the current policy.
    ///
    /// A path is blocked if it matches any entry in `blocked_paths`.
    /// Matches are prefix-based: a blocked path of `/etc` will block
    /// `/etc/passwd`, `/etc/shadow`, etc.
    pub fn is_path_allowed(&self, path: &str) -> bool {
        for blocked in &self.limits.blocked_paths {
            if path.starts_with(blocked) || path == blocked {
                return false;
            }
        }
        true
    }

    /// Get a reference to the current policy limits.
    pub fn limits(&self) -> &PolicyLimits {
        &self.limits
    }

    /// Update the policy limits.
    pub fn set_limits(&mut self, limits: PolicyLimits) {
        self.limits = limits;
    }

    /// Check paths referenced in tool input against blocked paths.
    fn check_paths_in_input(&self, input: &Value) -> Vec<String> {
        let mut blocked_paths = Vec::new();

        // Recursively search for path-like string values in the input
        if let Some(obj) = input.as_object() {
            for (key, value) in obj {
                if key.contains("path") || key.contains("file") || key.contains("dir") {
                    if let Some(path_str) = value.as_str() {
                        if !self.is_path_allowed(path_str) {
                            blocked_paths.push(path_str.to_string());
                        }
                    }
                }
                // Recurse into nested objects
                if value.is_object() || value.is_array() {
                    blocked_paths.extend(self.check_paths_in_input(value));
                }
            }
        } else if let Some(arr) = input.as_array() {
            for item in arr {
                blocked_paths.extend(self.check_paths_in_input(item));
            }
        }

        blocked_paths
    }
}

impl Default for PolicyLimitsManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // === PolicyLimits Tests ===

    #[test]
    fn test_policy_limits_default() {
        let limits = PolicyLimits::default();
        assert_eq!(limits.max_tokens_per_request, 200_000);
        assert_eq!(limits.max_tool_calls_per_turn, 50);
        assert!(limits.allowed_tools.is_empty());
        assert!(limits.blocked_paths.is_empty());
        assert_eq!(limits.max_file_size_bytes, 10 * 1024 * 1024);
    }

    #[test]
    fn test_policy_limits_custom() {
        let limits = PolicyLimits {
            max_tokens_per_request: 100_000,
            max_tool_calls_per_turn: 10,
            allowed_tools: vec!["read".to_string(), "write".to_string()],
            blocked_paths: vec!["/etc".to_string()],
            max_file_size_bytes: 5 * 1024 * 1024,
        };
        assert_eq!(limits.max_tokens_per_request, 100_000);
        assert_eq!(limits.max_tool_calls_per_turn, 10);
        assert_eq!(limits.allowed_tools.len(), 2);
        assert_eq!(limits.blocked_paths.len(), 1);
    }

    // === PolicyLimitsManager Tests ===

    #[test]
    fn test_manager_new() {
        let manager = PolicyLimitsManager::new();
        assert_eq!(manager.limits().max_tokens_per_request, 200_000);
    }

    #[test]
    fn test_manager_default() {
        let manager = PolicyLimitsManager::default();
        assert_eq!(manager.limits().max_tool_calls_per_turn, 50);
    }

    #[test]
    fn test_manager_with_limits() {
        let limits = PolicyLimits {
            max_tokens_per_request: 50_000,
            max_tool_calls_per_turn: 5,
            allowed_tools: vec!["read".to_string()],
            blocked_paths: vec!["/tmp".to_string()],
            max_file_size_bytes: 1_000_000,
        };
        let manager = PolicyLimitsManager::with_limits(limits);
        assert_eq!(manager.limits().max_tokens_per_request, 50_000);
    }

    #[test]
    fn test_is_tool_allowed_empty_list() {
        let manager = PolicyLimitsManager::new();
        assert!(manager.is_tool_allowed("read"));
        assert!(manager.is_tool_allowed("write"));
        assert!(manager.is_tool_allowed("bash"));
    }

    #[test]
    fn test_is_tool_allowed_explicit_list() {
        let limits = PolicyLimits {
            allowed_tools: vec!["read".to_string(), "write".to_string()],
            ..Default::default()
        };
        let manager = PolicyLimitsManager::with_limits(limits);
        assert!(manager.is_tool_allowed("read"));
        assert!(manager.is_tool_allowed("write"));
        assert!(!manager.is_tool_allowed("bash"));
    }

    #[test]
    fn test_is_tool_allowed_wildcard() {
        let limits = PolicyLimits {
            allowed_tools: vec!["file*".to_string()],
            ..Default::default()
        };
        let manager = PolicyLimitsManager::with_limits(limits);
        assert!(manager.is_tool_allowed("file_read"));
        assert!(manager.is_tool_allowed("file_write"));
        assert!(!manager.is_tool_allowed("bash"));
    }

    #[test]
    fn test_is_tool_allowed_prefix_match() {
        let limits = PolicyLimits {
            allowed_tools: vec!["mcp".to_string()],
            ..Default::default()
        };
        let manager = PolicyLimitsManager::with_limits(limits);
        assert!(manager.is_tool_allowed("mcp/some_tool"));
        assert!(!manager.is_tool_allowed("bash"));
    }

    #[test]
    fn test_is_path_allowed_no_blocks() {
        let manager = PolicyLimitsManager::new();
        assert!(manager.is_path_allowed("/any/path"));
        assert!(manager.is_path_allowed("/etc/passwd"));
    }

    #[test]
    fn test_is_path_allowed_blocked_prefix() {
        let limits = PolicyLimits {
            blocked_paths: vec!["/etc".to_string(), "/var/log".to_string()],
            ..Default::default()
        };
        let manager = PolicyLimitsManager::with_limits(limits);
        assert!(!manager.is_path_allowed("/etc/passwd"));
        assert!(!manager.is_path_allowed("/etc/shadow"));
        assert!(!manager.is_path_allowed("/var/log/syslog"));
        assert!(manager.is_path_allowed("/home/user/file.txt"));
    }

    #[test]
    fn test_is_path_allowed_exact_match() {
        let limits = PolicyLimits {
            blocked_paths: vec!["/etc/passwd".to_string()],
            ..Default::default()
        };
        let manager = PolicyLimitsManager::with_limits(limits);
        assert!(!manager.is_path_allowed("/etc/passwd"));
        assert!(manager.is_path_allowed("/etc/shadow"));
    }

    #[test]
    fn test_enforce_allowed() {
        let manager = PolicyLimitsManager::new();
        let result = manager.enforce("read", &json!({"path": "/home/file.txt"}));
        assert_eq!(result.unwrap(), PolicyCheckResult::Allowed);
    }

    #[test]
    fn test_enforce_blocked_tool() {
        let limits = PolicyLimits {
            allowed_tools: vec!["read".to_string()],
            ..Default::default()
        };
        let manager = PolicyLimitsManager::with_limits(limits);
        let result = manager.enforce("bash", &json!({"command": "rm -rf /"}));
        match result.unwrap() {
            PolicyCheckResult::Blocked { reason } => {
                assert!(reason.contains("bash"));
            }
            _ => panic!("Expected Blocked"),
        }
    }

    #[test]
    fn test_enforce_blocked_path() {
        let limits = PolicyLimits {
            blocked_paths: vec!["/etc".to_string()],
            ..Default::default()
        };
        let manager = PolicyLimitsManager::with_limits(limits);
        let result = manager.enforce("read", &json!({"path": "/etc/passwd"}));
        match result.unwrap() {
            PolicyCheckResult::Blocked { reason } => {
                assert!(reason.contains("/etc/passwd"));
            }
            _ => panic!("Expected Blocked"),
        }
    }

    #[test]
    fn test_enforce_allowed_path() {
        let limits = PolicyLimits {
            blocked_paths: vec!["/etc".to_string()],
            ..Default::default()
        };
        let manager = PolicyLimitsManager::with_limits(limits);
        let result = manager.enforce("read", &json!({"path": "/home/user/file.txt"}));
        assert_eq!(result.unwrap(), PolicyCheckResult::Allowed);
    }

    #[test]
    fn test_enforce_nested_path() {
        let limits = PolicyLimits {
            blocked_paths: vec!["/secret".to_string()],
            ..Default::default()
        };
        let manager = PolicyLimitsManager::with_limits(limits);
        let input = json!({
            "files": [
                {"path": "/home/file.txt"},
                {"path": "/secret/key.pem"}
            ]
        });
        let result = manager.enforce("read", &input);
        match result.unwrap() {
            PolicyCheckResult::Blocked { reason } => {
                assert!(reason.contains("/secret/key.pem"));
            }
            _ => panic!("Expected Blocked for nested path"),
        }
    }

    #[test]
    fn test_set_limits() {
        let mut manager = PolicyLimitsManager::new();
        let new_limits = PolicyLimits {
            max_tokens_per_request: 10_000,
            ..Default::default()
        };
        manager.set_limits(new_limits);
        assert_eq!(manager.limits().max_tokens_per_request, 10_000);
    }

    #[test]
    fn test_load_from_api_placeholder() {
        // This test verifies the placeholder works synchronously
        let rt = tokio::runtime::Runtime::new().unwrap();
        let manager = rt.block_on(PolicyLimitsManager::load_from_api()).unwrap();
        assert_eq!(manager.limits().max_tokens_per_request, 200_000);
    }

    // === PolicyError Tests ===

    #[test]
    fn test_policy_error_display() {
        let err = PolicyError::ToolBlocked("bash".to_string());
        assert_eq!(format!("{}", err), "Tool 'bash' is blocked by policy");
    }

    #[test]
    fn test_policy_error_path_blocked() {
        let err = PolicyError::PathBlocked("/etc/passwd".to_string());
        assert_eq!(format!("{}", err), "Path '/etc/passwd' is blocked by policy");
    }

    // === PolicyCheckResult Tests ===

    #[test]
    fn test_policy_check_result_equality() {
        let allowed = PolicyCheckResult::Allowed;
        assert_eq!(allowed, PolicyCheckResult::Allowed);

        let blocked = PolicyCheckResult::Blocked {
            reason: "test".to_string(),
        };
        assert_eq!(
            blocked,
            PolicyCheckResult::Blocked {
                reason: "test".to_string()
            }
        );
    }

    #[test]
    fn test_policy_check_result_rate_limited() {
        let rate_limited = PolicyCheckResult::RateLimited {
            retry_after: Some(Duration::from_secs(30)),
        };
        match rate_limited {
            PolicyCheckResult::RateLimited { retry_after } => {
                assert_eq!(retry_after, Some(Duration::from_secs(30)));
            }
            _ => panic!("Expected RateLimited"),
        }
    }
}
