//! # Permission System
//!
//! Security and permission validation for tool execution and resource access.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use thiserror::Error;

/// Errors that can occur during permission validation
#[derive(Error, Debug)]
pub enum PermissionError {
    #[error("Permission denied: {0}")]
    Denied(String),

    #[error("Invalid permission: {0}")]
    InvalidPermission(String),

    #[error("Permission not found: {0}")]
    NotFound(String),
}

/// Risk level of a tool operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RiskLevel {
    /// Safe operation (e.g., read-only)
    Safe = 0,
    /// Low risk (e.g., write to allowed paths)
    Low = 1,
    /// Medium risk (e.g., network requests)
    Medium = 2,
    /// High risk (e.g., file deletion)
    High = 3,
    /// Critical (e.g., system modification)
    Critical = 4,
}

/// User's choice for a permission prompt
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionChoice {
    /// Deny this operation
    Deny,
    /// Allow this once
    AllowOnce,
    /// Always allow this tool
    AlwaysAllow,
}

/// A prompt requesting user permission for a tool operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionPrompt {
    /// Unique ID for this prompt
    pub id: uuid::Uuid,
    /// Tool name being executed
    pub tool_name: String,
    /// Tool input/arguments
    pub tool_input: serde_json::Value,
    /// Risk level of this operation
    pub risk_level: RiskLevel,
    /// Human-readable description of the operation
    pub description: String,
    /// Whether this is a confirmation (already approved conceptually)
    pub is_confirmation: bool,
}

impl PermissionPrompt {
    /// Create a new permission prompt
    pub fn new(
        tool_name: String,
        tool_input: serde_json::Value,
        risk_level: RiskLevel,
        description: String,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            tool_name,
            tool_input,
            risk_level,
            description,
            is_confirmation: false,
        }
    }

    /// Create a confirmation prompt (lower visual urgency)
    pub fn confirmation(tool_name: String, description: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            tool_name,
            tool_input: serde_json::json!({}),
            risk_level: RiskLevel::Safe,
            description,
            is_confirmation: true,
        }
    }

    /// Get a formatted display string for the prompt
    pub fn display_text(&self) -> String {
        let risk_indicator = match self.risk_level {
            RiskLevel::Safe => "✓",
            RiskLevel::Low => "⚠",
            RiskLevel::Medium => "⚡",
            RiskLevel::High => "🔥",
            RiskLevel::Critical => "☢️",
        };

        format!(
            "{} {} - {}\nInput: {}",
            risk_indicator,
            self.tool_name,
            self.description,
            serde_json::to_string_pretty(&self.tool_input)
                .unwrap_or_else(|_| "(invalid input)".to_string())
        )
    }
}

/// Policy for a specific tool's permission requirements
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPermissionPolicy {
    /// Tool name this policy applies to
    pub tool_name: String,
    /// Default risk level for operations
    pub default_risk_level: RiskLevel,
    /// Requires confirmation for these input patterns
    pub confirmation_patterns: Vec<String>,
    /// Always deny patterns (dangerous regardless of user approval)
    pub deny_patterns: Vec<String>,
    /// Description for users
    pub description: String,
}

impl ToolPermissionPolicy {
    /// Create a new tool permission policy
    pub fn new(
        tool_name: String,
        default_risk_level: RiskLevel,
        description: String,
    ) -> Self {
        Self {
            tool_name,
            default_risk_level,
            confirmation_patterns: Vec::new(),
            deny_patterns: Vec::new(),
            description,
        }
    }

    /// Add a pattern that requires explicit confirmation
    pub fn add_confirmation_pattern(mut self, pattern: &str) -> Self {
        self.confirmation_patterns.push(pattern.to_string());
        self
    }

    /// Add a pattern that is always denied (dangerous)
    pub fn add_deny_pattern(mut self, pattern: &str) -> Self {
        self.deny_patterns.push(pattern.to_string());
        self
    }

    /// Check if the given input matches any deny pattern
    pub fn is_denied(&self, input_str: &str) -> bool {
        self.deny_patterns.iter().any(|pattern| {
            input_str.contains(pattern) || {
                if pattern.contains('*') {
                    // Simple wildcard matching
                    let regex_pattern = pattern.replace('*', ".*");
                    input_str.matches(&regex_pattern).count() > 0
                } else {
                    false
                }
            }
        })
    }

    /// Check if the given input requires confirmation
    pub fn requires_confirmation(&self, input_str: &str) -> bool {
        self.confirmation_patterns.iter().any(|pattern| {
            input_str.contains(pattern) || {
                if pattern.contains('*') {
                    let regex_pattern = pattern.replace('*', ".*");
                    input_str.matches(&regex_pattern).count() > 0
                } else {
                    false
                }
            }
        })
    }

    /// Get the risk level for a specific input
    pub fn risk_level_for(&self, input_str: &str) -> RiskLevel {
        if self.is_denied(input_str) {
            RiskLevel::Critical
        } else if self.requires_confirmation(input_str) {
            RiskLevel::Medium
        } else {
            self.default_risk_level
        }
    }
}

/// Memory of user permission choices (persists across prompts)
#[derive(Debug, Clone)]
pub struct PermissionMemory {
    /// Always-allowed tools
    always_allowed: HashSet<String>,
    /// Always-denied tools
    always_denied: HashSet<String>,
    /// Session-specific choices
    session_choices: HashMap<uuid::Uuid, HashMap<String, PermissionChoice>>,
}

impl PermissionMemory {
    /// Create a new permission memory
    pub fn new() -> Self {
        Self {
            always_allowed: HashSet::new(),
            always_denied: HashSet::new(),
            session_choices: HashMap::new(),
        }
    }

    /// Check if a tool is always allowed for this session
    pub fn is_always_allowed(&self, session_id: uuid::Uuid, tool_name: &str) -> bool {
        self.always_allowed.contains(tool_name)
            || self.session_choices
                .get(&session_id)
                .and_then(|choices| choices.get(tool_name))
                .map(|choice| choice == &PermissionChoice::AlwaysAllow)
                .unwrap_or(false)
    }

    /// Check if a tool is always denied
    pub fn is_always_denied(&self, tool_name: &str) -> bool {
        self.always_denied.contains(tool_name)
    }

    /// Remember a user's permission choice
    pub fn remember_choice(
        &mut self,
        session_id: uuid::Uuid,
        tool_name: String,
        choice: PermissionChoice,
    ) {
        match choice {
            PermissionChoice::AlwaysAllow => {
                self.always_allowed.insert(tool_name.clone());
                self.session_choices
                    .entry(session_id)
                    .or_insert_with(HashMap::new)
                    .insert(tool_name, choice);
            }
            PermissionChoice::Deny => {
                self.always_denied.insert(tool_name.clone());
                self.session_choices
                    .entry(session_id)
                    .or_insert_with(HashMap::new)
                    .insert(tool_name, choice);
            }
            PermissionChoice::AllowOnce => {
                // Don't remember allow-once choices
            }
        }
    }

    /// Clear session-specific choices (call on session end)
    pub fn clear_session(&mut self, session_id: uuid::Uuid) {
        self.session_choices.remove(&session_id);
    }
}

impl Default for PermissionMemory {
    fn default() -> Self {
        Self::new()
    }
}

/// Permission level for operations
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PermissionLevel {
    /// No permission
    None = 0,
    /// Read-only access
    Read = 1,
    /// Write access
    Write = 2,
    /// Admin access
    Admin = 3,
}

/// A specific permission with resource and action
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Permission {
    pub resource: String,
    pub action: String,
    pub level: PermissionLevel,
}

impl Permission {
    /// Create a new permission
    pub fn new(resource: &str, action: &str, level: PermissionLevel) -> Self {
        Self {
            resource: resource.to_string(),
            action: action.to_string(),
            level,
        }
    }

    /// Check if this permission grants access for the given level
    pub fn grants(&self, required_level: PermissionLevel) -> bool {
        self.level >= required_level
    }
}

/// Permission manager for validating and granting permissions
pub struct PermissionManager {
    /// Default permissions granted to all sessions
    default_permissions: HashSet<Permission>,

    /// Session-specific permissions
    session_permissions: HashMap<uuid::Uuid, HashSet<Permission>>,

    /// Tool-specific permission requirements
    tool_permissions: HashMap<String, Permission>,

    /// Tool permission policies (risk levels and patterns)
    tool_policies: HashMap<String, ToolPermissionPolicy>,

    /// Memory of user choices
    memory: PermissionMemory,
}

impl PermissionManager {
    /// Create a new permission manager with default permissions
    pub fn new() -> Self {
        let mut manager = Self {
            default_permissions: HashSet::new(),
            session_permissions: HashMap::new(),
            tool_permissions: HashMap::new(),
            tool_policies: HashMap::new(),
            memory: PermissionMemory::new(),
        };

        // Register default tool policies for common tools
        manager.register_default_policies();

        manager
    }

    /// Register default permission policies for known tools
    fn register_default_policies(&mut self) {
        // Bash tool - high risk, confirm on dangerous commands
        let bash_policy = ToolPermissionPolicy::new(
            "Bash".to_string(),
            RiskLevel::Medium,
            "Execute shell commands".to_string(),
        )
        .add_deny_pattern("rm -rf /")
        .add_deny_pattern(":>.*")
        .add_deny_pattern("dd if=/dev/zero")
        .add_confirmation_pattern("rm -rf")
        .add_confirmation_pattern("del /q")
        .add_confirmation_pattern("chmod 000");
        self.tool_policies.insert("Bash".to_string(), bash_policy);

        // FileEdit tool - medium risk
        let edit_policy = ToolPermissionPolicy::new(
            "FileEdit".to_string(),
            RiskLevel::Low,
            "Edit file contents".to_string(),
        );
        self.tool_policies.insert("FileEdit".to_string(), edit_policy);

        // FileWrite tool - medium risk
        let write_policy = ToolPermissionPolicy::new(
            "FileWrite".to_string(),
            RiskLevel::Medium,
            "Write to files".to_string(),
        )
        .add_deny_pattern("/etc/")
        .add_deny_pattern("/usr/bin/")
        .add_deny_pattern("/boot/");
        self.tool_policies.insert("FileWrite".to_string(), write_policy);

        // Read tool - low risk
        let read_policy = ToolPermissionPolicy::new(
            "Read".to_string(),
            RiskLevel::Safe,
            "Read file contents".to_string(),
        );
        self.tool_policies.insert("Read".to_string(), read_policy);

        // WebFetch tool - medium risk
        let web_policy = ToolPermissionPolicy::new(
            "WebFetch".to_string(),
            RiskLevel::Low,
            "Fetch content from URLs".to_string(),
        );
        self.tool_policies.insert("WebFetch".to_string(), web_policy);
    }

    /// Register or update a tool's permission policy
    pub fn register_tool_policy(&mut self, policy: ToolPermissionPolicy) {
        self.tool_policies.insert(policy.tool_name.clone(), policy);
    }

    /// Add a default permission
    pub fn add_default_permission(&mut self, permission: Permission) {
        self.default_permissions.insert(permission);
    }

    /// Grant a permission to a specific session
    pub fn grant_permission(&mut self, session_id: uuid::Uuid, permission: Permission) {
        self.session_permissions
            .entry(session_id)
            .or_insert_with(HashSet::new)
            .insert(permission);
    }

    /// Revoke a permission from a specific session
    pub fn revoke_permission(&mut self, session_id: uuid::Uuid, permission: &Permission) {
        if let Some(perms) = self.session_permissions.get_mut(&session_id) {
            perms.remove(permission);
        }
    }

    /// Set the required permission for a tool
    pub fn set_tool_permission(&mut self, tool_name: String, permission: Permission) {
        self.tool_permissions.insert(tool_name, permission);
    }

    /// Check if a session has a required permission
    pub fn check_permission(
        &self,
        session_id: uuid::Uuid,
        required: &Permission,
    ) -> Result<(), PermissionError> {
        // Check session-specific permissions first
        if let Some(perms) = self.session_permissions.get(&session_id) {
            for perm in perms {
                if perm.resource == required.resource && perm.action == required.action {
                    if perm.grants(required.level) {
                        return Ok(());
                    }
                }
            }
        }

        // Fall back to default permissions
        for perm in &self.default_permissions {
            if perm.resource == required.resource && perm.action == required.action {
                if perm.grants(required.level) {
                    return Ok(());
                }
            }
        }

        Err(PermissionError::Denied(format!(
            "Permission denied for {}:{}",
            required.resource, required.action
        )))
    }

    /// Check if a session can execute a tool
    pub fn check_tool_permission(
        &self,
        session_id: uuid::Uuid,
        tool_name: &str,
    ) -> Result<(), PermissionError> {
        if let Some(required) = self.tool_permissions.get(tool_name) {
            self.check_permission(session_id, required)
        } else {
            Ok(())
        }
    }

    /// Get all permissions for a session
    pub fn get_session_permissions(
        &self,
        session_id: uuid::Uuid,
    ) -> HashSet<Permission> {
        let mut perms = self.default_permissions.clone();
        if let Some(session_perms) = self.session_permissions.get(&session_id) {
            perms.extend(session_perms.clone());
        }
        perms
    }

    /// Create a permission prompt for a tool execution
    pub fn create_permission_prompt(
        &self,
        tool_name: &str,
        tool_input: &serde_json::Value,
        session_id: uuid::Uuid,
    ) -> Option<PermissionPrompt> {
        // Check if this is already always allowed
        if self.memory.is_always_allowed(session_id, tool_name) {
            return None; // No prompt needed
        }

        // Check if always denied
        if self.memory.is_always_denied(tool_name) {
            return Some(PermissionPrompt {
                id: uuid::Uuid::new_v4(),
                tool_name: tool_name.to_string(),
                tool_input: tool_input.clone(),
                risk_level: RiskLevel::Critical,
                description: format!("This tool is denied: {}", tool_name),
                is_confirmation: false,
            });
        }

        // Get tool policy
        let policy = self.tool_policies.get(tool_name);

        // Determine risk level and description
        let (risk_level, description) = if let Some(policy) = policy {
            let input_str = serde_json::to_string(tool_input).unwrap_or_default();
            (
                policy.risk_level_for(&input_str),
                format!("{}: {}", policy.description, Self::format_input_summary(tool_input)),
            )
        } else {
            // Unknown tool - default to medium risk
            (
                RiskLevel::Medium,
                format!("Execute tool: {}", Self::format_input_summary(tool_input)),
            )
        };

        Some(PermissionPrompt {
            id: uuid::Uuid::new_v4(),
            tool_name: tool_name.to_string(),
            tool_input: tool_input.clone(),
            risk_level,
            description,
            is_confirmation: false,
        })
    }

    /// Process user's permission choice
    pub fn process_permission_choice(
        &mut self,
        session_id: uuid::Uuid,
        prompt: &PermissionPrompt,
        choice: PermissionChoice,
    ) -> Result<(), PermissionError> {
        match choice {
            PermissionChoice::Deny => {
                Err(PermissionError::Denied(format!(
                    "User denied: {}",
                    prompt.description
                )))
            }
            PermissionChoice::AllowOnce | PermissionChoice::AlwaysAllow => {
                // Remember the choice
                self.memory.remember_choice(session_id, prompt.tool_name.clone(), choice);
                Ok(())
            }
        }
    }

    /// Get the permission memory (for persistence)
    pub fn memory(&self) -> &PermissionMemory {
        &self.memory
    }

    /// Get mutable permission memory (for loading persisted state)
    pub fn memory_mut(&mut self) -> &mut PermissionMemory {
        &mut self.memory
    }

    /// Helper to format tool input summary
    fn format_input_summary(input: &serde_json::Value) -> String {
        if input.is_null() {
            return "(no input)".to_string();
        }

        if let Some(obj) = input.as_object() {
            let parts: Vec<String> = obj
                .iter()
                .take(3) // Only show first 3 fields
                .map(|(k, v)| format!("{}: {}", k, Self::truncate_value(v)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        } else if let Some(arr) = input.as_array() {
            if !arr.is_empty() {
                format!("[{} values]", arr.len())
            } else {
                "[]".to_string()
            }
        } else {
            let s = input.to_string();
            if s.len() > 50 {
                format!("{}...", &s[..47])
            } else {
                s
            }
        }
    }

    /// Helper to truncate a JSON value for display
    fn truncate_value(value: &serde_json::Value) -> String {
        let s = serde_json::to_string(value).unwrap_or_else(|_| "?".to_string());
        if s.len() > 30 {
            format!("{}...", &s[..27])
        } else {
            s
        }
    }

    /// Clear session data (call when session ends)
    pub fn clear_session(&mut self, session_id: uuid::Uuid) {
        self.session_permissions.remove(&session_id);
        self.memory.clear_session(session_id);
    }
}

// SAFETY: PermissionManager is safe to send across threads because:
// - All fields (HashMap, HashSet) are Send + Sync when their type parameters are Send + Sync
// - Permission, ToolPermissionPolicy, and PermissionMemory contain only Send + Sync types
// - No interior mutability is exposed directly (all access is through &self methods)
unsafe impl Send for PermissionManager {}
unsafe impl Sync for PermissionManager {}

impl Default for PermissionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_permission_creation() {
        let perm = Permission::new("file", "read", PermissionLevel::Read);
        assert_eq!(perm.resource, "file");
        assert_eq!(perm.action, "read");
        assert!(perm.grants(PermissionLevel::Read));
        assert!(!perm.grants(PermissionLevel::Write));
    }

    #[test]
    fn test_permission_grant_revoke() {
        let mut manager = PermissionManager::new();
        let session_id = Uuid::new_v4();
        let perm = Permission::new("file", "write", PermissionLevel::Write);

        // Initially should fail
        assert!(manager.check_permission(session_id, &perm).is_err());

        // Grant permission
        manager.grant_permission(session_id, perm.clone());
        assert!(manager.check_permission(session_id, &perm).is_ok());

        // Revoke permission
        manager.revoke_permission(session_id, &perm);
        assert!(manager.check_permission(session_id, &perm).is_err());
    }

    #[test]
    fn test_default_permissions() {
        let mut manager = PermissionManager::new();
        let perm = Permission::new("file", "read", PermissionLevel::Read);
        manager.add_default_permission(perm.clone());

        let session_id = Uuid::new_v4();
        assert!(manager.check_permission(session_id, &perm).is_ok());
    }

    #[test]
    fn test_permission_level_hierarchy() {
        let write_perm = Permission::new("file", "write", PermissionLevel::Write);
        let read_perm = Permission::new("file", "read", PermissionLevel::Read);

        // Write permission should grant read access
        assert!(write_perm.grants(PermissionLevel::Read));
        // Read permission should not grant write access
        assert!(!read_perm.grants(PermissionLevel::Write));
    }
}
