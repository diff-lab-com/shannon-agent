//! Permission management and security

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

/// Permission errors
#[derive(Error, Debug)]
pub enum PermissionError {
    #[error("Permission denied: {0}")]
    Denied(String),

    #[error("Invalid permission: {0}")]
    Invalid(String),

    #[error("Permission check failed: {0}")]
    CheckFailed(String),
}

/// Permission level
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PermissionLevel {
    Safe,
    Warning,
    Dangerous,
    Admin,
}

/// Permission choice (user response)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionChoice {
    Allow,
    Deny,
    AllowAlways,
}

/// Permission prompt for UI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionPrompt {
    pub id: String,
    pub title: String,
    pub description: String,
    pub level: PermissionLevel,
    pub tool_name: String,
    pub parameters: serde_json::Value,
}

/// Permission request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permission {
    pub id: String,
    pub level: PermissionLevel,
    pub description: String,
    pub tool_name: String,
    pub parameters: serde_json::Value,
}

impl Permission {
    pub fn new(
        tool_name: &str,
        action: &str,
        level: PermissionLevel,
        description: Option<&str>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            level,
            description: description.unwrap_or("Execute tool").to_string(),
            tool_name: tool_name.to_string(),
            parameters: serde_json::Value::Null,
        }
    }

    pub fn new_with_params(
        tool_name: String,
        level: PermissionLevel,
        description: String,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            level,
            description,
            tool_name,
            parameters,
        }
    }
}

/// Permission manager
pub struct PermissionManager {
    pub config: PermissionConfig,
}

#[derive(Debug, Clone)]
pub struct PermissionConfig {
    pub auto_allow_safe: bool,
    pub prompt_dangerous: bool,
}

impl Default for PermissionConfig {
    fn default() -> Self {
        Self {
            auto_allow_safe: true,
            prompt_dangerous: true,
        }
    }
}

impl PermissionManager {
    pub fn new() -> Self {
        Self {
            config: PermissionConfig::default(),
        }
    }

    pub fn with_config(config: PermissionConfig) -> Self {
        Self { config }
    }

    pub fn arc() -> Arc<Self> {
        Arc::new(Self::new())
    }

    pub async fn check_permission(&self, permission: &Permission) -> Result<bool, PermissionError> {
        match permission.level {
            PermissionLevel::Safe if self.config.auto_allow_safe => Ok(true),
            PermissionLevel::Admin => Ok(true),
            _ => Ok(false), // Default: require prompt
        }
    }

    pub async fn request_permission(&self, permission: &Permission) -> Result<PermissionChoice, PermissionError> {
        // This would normally prompt the user
        Ok(PermissionChoice::Allow)
    }

    pub fn create_permission_prompt(
        &self,
        tool_name: &str,
        tool_input: &serde_json::Value,
        session_id: uuid::Uuid,
    ) -> Option<PermissionPrompt> {
        Some(PermissionPrompt {
            id: uuid::Uuid::new_v4().to_string(),
            title: format!("{} Permission Required", tool_name),
            description: format!("Execute {} with parameters: {}", tool_name, tool_input),
            level: PermissionLevel::Warning,
            tool_name: tool_name.to_string(),
            parameters: tool_input.clone(),
        })
    }

    pub async fn check_tool_permission(
        &self,
        _session_id: uuid::Uuid,
        _tool_name: &str,
    ) -> Result<bool, PermissionError> {
        // For now, auto-allow all tool permissions
        // In production, this would check against a permission database
        Ok(true)
    }

    pub fn set_tool_permission(&mut self, _tool_name: &str, _allowed: bool) {
        // Store permission preference
    }
}
