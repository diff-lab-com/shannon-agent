//! # Permission System
//!
//! Security and permission validation for tool execution and resource access.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
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
                    .or_default()
                    .insert(tool_name, choice);
            }
            PermissionChoice::Deny => {
                self.always_denied.insert(tool_name.clone());
                self.session_choices
                    .entry(session_id)
                    .or_default()
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
            .or_default()
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
                if perm.resource == required.resource && perm.action == required.action
                    && perm.grants(required.level) {
                        return Ok(());
                    }
            }
        }

        // Fall back to default permissions
        for perm in &self.default_permissions {
            if perm.resource == required.resource && perm.action == required.action
                && perm.grants(required.level) {
                    return Ok(());
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
                description: format!("This tool is denied: {tool_name}"),
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

    /// Classify a tool operation using the PermissionClassifier and check permission.
    ///
    /// - Returns `Ok(None)` if the operation is auto-allowed
    /// - Returns `Ok(Some(prompt))` if user confirmation is needed
    /// - Returns `Err` if the operation is denied
    pub fn classify_and_check(
        &self,
        session_id: uuid::Uuid,
        tool_name: &str,
        tool_input: &serde_json::Value,
    ) -> Result<Option<PermissionPrompt>, PermissionError> {
        // First, run the classifier
        let classifier = crate::permission_classifier::PermissionClassifier::new();
        let result = classifier.classify(tool_name, tool_input);

        match result.decision {
            crate::permission_classifier::RuleDecision::Deny => {
                Err(PermissionError::Denied(format!(
                    "Operation denied by classifier: {} (risk: {})",
                    result.reason, result.risk_level
                )))
            }
            crate::permission_classifier::RuleDecision::Allow => {
                // Auto-allow for low-risk operations
                if matches!(
                    result.risk_level,
                    crate::permission_classifier::RiskLevel::None
                        | crate::permission_classifier::RiskLevel::Low
                ) {
                    // Check if already always-allowed
                    if self.memory.is_always_allowed(session_id, tool_name) {
                        return Ok(None);
                    }
                    Ok(None) // Low-risk, auto-allow
                } else {
                    // Higher risk but classifier said allow — still prompt
                    self.create_permission_prompt_with_risk(
                        tool_name,
                        tool_input,
                        session_id,
                        convert_classifier_risk(result.risk_level),
                    )
                }
            }
            crate::permission_classifier::RuleDecision::Ask => {
                // Always prompt the user
                self.create_permission_prompt_with_risk(
                    tool_name,
                    tool_input,
                    session_id,
                    convert_classifier_risk(result.risk_level),
                )
            }
        }
    }

    /// Create a permission prompt with an explicit risk level from classifier.
    fn create_permission_prompt_with_risk(
        &self,
        tool_name: &str,
        tool_input: &serde_json::Value,
        session_id: uuid::Uuid,
        risk_level: RiskLevel,
    ) -> Result<Option<PermissionPrompt>, PermissionError> {
        if self.memory.is_always_allowed(session_id, tool_name) {
            return Ok(None);
        }
        if self.memory.is_always_denied(tool_name) {
            return Err(PermissionError::Denied(format!(
                "Tool '{tool_name}' is always denied"
            )));
        }

        let policy = self.tool_policies.get(tool_name);
        let description = if let Some(p) = policy {
            format!("{}: {}", p.description, Self::format_input_summary(tool_input))
        } else {
            format!("Execute tool '{}': {}", tool_name, Self::format_input_summary(tool_input))
        };

        Ok(Some(PermissionPrompt {
            id: uuid::Uuid::new_v4(),
            tool_name: tool_name.to_string(),
            tool_input: tool_input.clone(),
            risk_level,
            description,
            is_confirmation: false,
        }))
    }
}

/// Convert classifier RiskLevel to permissions RiskLevel.
fn convert_classifier_risk(
    risk: crate::permission_classifier::RiskLevel,
) -> RiskLevel {
    match risk {
        crate::permission_classifier::RiskLevel::None => RiskLevel::Safe,
        crate::permission_classifier::RiskLevel::Low => RiskLevel::Low,
        crate::permission_classifier::RiskLevel::Medium => RiskLevel::Medium,
        crate::permission_classifier::RiskLevel::High => RiskLevel::High,
        crate::permission_classifier::RiskLevel::Critical => RiskLevel::Critical,
    }
}

// NOTE: PermissionManager is auto-Send + Sync because all fields (HashMap, HashSet)
// contain only Send + Sync types. No unsafe impl needed.

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

    #[test]
    fn test_permission_serialization_roundtrip() {
        let perm = Permission::new("file", "write", PermissionLevel::Write);
        let json = serde_json::to_string(&perm).unwrap();
        let parsed: Permission = serde_json::from_str(&json).unwrap();
        assert_eq!(perm, parsed);
    }

    #[test]
    fn test_risk_level_serialization() {
        for level in [RiskLevel::Safe, RiskLevel::Low, RiskLevel::Medium, RiskLevel::High, RiskLevel::Critical] {
            let json = serde_json::to_string(&level).unwrap();
            let parsed: RiskLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(level, parsed);
        }
    }

    #[test]
    fn test_permission_choice_serialization() {
        for choice in [PermissionChoice::Deny, PermissionChoice::AllowOnce, PermissionChoice::AlwaysAllow] {
            let json = serde_json::to_string(&choice).unwrap();
            let parsed: PermissionChoice = serde_json::from_str(&json).unwrap();
            assert_eq!(choice, parsed);
        }
    }

    #[test]
    fn test_permission_prompt_serialization() {
        let prompt = PermissionPrompt::new(
            "file_write".to_string(),
            serde_json::json!({"path": "/tmp/test"}),
            RiskLevel::Medium,
            "Write to /tmp/test".to_string(),
        );
        let json = serde_json::to_string(&prompt).unwrap();
        let parsed: PermissionPrompt = serde_json::from_str(&json).unwrap();
        assert_eq!(prompt.id, parsed.id);
        assert_eq!(prompt.tool_name, parsed.tool_name);
        assert_eq!(prompt.risk_level, parsed.risk_level);
    }

    #[tokio::test]
    async fn test_concurrent_permission_check() {
        use std::sync::Arc;
        use tokio::sync::Mutex;

        let manager = Arc::new(Mutex::new(PermissionManager::new()));
        let session_id = Uuid::new_v4();
        let perm = Permission::new("file", "read", PermissionLevel::Read);

        // Grant permission
        manager.lock().await.grant_permission(session_id, perm.clone());

        // Concurrent reads
        let mut handles = vec![];
        for _ in 0..10 {
            let mgr = Arc::clone(&manager);
            let sid = session_id;
            let p = perm.clone();
            handles.push(tokio::spawn(async move {
                let result = mgr.lock().await.check_permission(sid, &p);
                assert!(result.is_ok());
            }));
        }
        for handle in handles {
            handle.await.unwrap();
        }
    }

    #[tokio::test]
    async fn test_concurrent_grant_and_check() {
        use std::sync::Arc;
        use tokio::sync::Mutex;

        let manager = Arc::new(Mutex::new(PermissionManager::new()));
        let mut handles = vec![];

        // Spawn tasks that grant different permissions concurrently
        for i in 0..5 {
            let mgr = Arc::clone(&manager);
            let session_id = Uuid::new_v4();
            handles.push(tokio::spawn(async move {
                let perm = Permission::new("file", &format!("action_{i}"), PermissionLevel::Write);
                mgr.lock().await.grant_permission(session_id, perm.clone());

                let result = mgr.lock().await.check_permission(session_id, &perm);
                assert!(result.is_ok(), "Permission for action_{i} should be granted");
            }));
        }
        for handle in handles {
            handle.await.unwrap();
        }
    }

    #[test]
    fn test_permission_manager_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<PermissionManager>();
    }

    #[test]
    fn test_permission_memory_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<PermissionMemory>();
    }

    // ── ToolPermissionPolicy tests ────────────────────────────────

    #[test]
    fn test_policy_deny_pattern_matches() {
        let policy = ToolPermissionPolicy::new(
            "Bash".to_string(),
            RiskLevel::Medium,
            "Shell".to_string(),
        )
        .add_deny_pattern("rm -rf /");

        assert!(policy.is_denied("rm -rf /"));
        assert!(policy.is_denied("sudo rm -rf / --no-preserve-root"));
        assert!(!policy.is_denied("ls -la"));
    }

    #[test]
    fn test_policy_confirmation_pattern_matches() {
        let policy = ToolPermissionPolicy::new(
            "Bash".to_string(),
            RiskLevel::Medium,
            "Shell".to_string(),
        )
        .add_confirmation_pattern("rm -rf");

        assert!(policy.requires_confirmation("rm -rf /home/user/dir"));
        assert!(!policy.requires_confirmation("ls -la"));
    }

    #[test]
    fn test_policy_risk_level_denied_input() {
        let policy = ToolPermissionPolicy::new(
            "Bash".to_string(),
            RiskLevel::Medium,
            "Shell".to_string(),
        )
        .add_deny_pattern("rm -rf /")
        .add_confirmation_pattern("sudo");

        // Denied input → Critical
        assert_eq!(policy.risk_level_for("rm -rf /"), RiskLevel::Critical);
        // Confirmation pattern → Medium
        assert_eq!(policy.risk_level_for("sudo apt install"), RiskLevel::Medium);
        // Normal input → default (Medium)
        assert_eq!(policy.risk_level_for("ls -la"), RiskLevel::Medium);
    }

    #[test]
    fn test_policy_default_risk_level_no_patterns() {
        let policy = ToolPermissionPolicy::new(
            "Read".to_string(),
            RiskLevel::Safe,
            "Read files".to_string(),
        );
        assert_eq!(policy.risk_level_for("anything"), RiskLevel::Safe);
    }

    #[test]
    fn test_policy_builder_pattern_chaining() {
        let policy = ToolPermissionPolicy::new("T".to_string(), RiskLevel::Low, "desc".to_string())
            .add_confirmation_pattern("p1")
            .add_confirmation_pattern("p2")
            .add_deny_pattern("d1");

        assert!(policy.requires_confirmation("p1"));
        assert!(policy.requires_confirmation("p2"));
        assert!(policy.is_denied("d1"));
        assert!(!policy.is_denied("safe input"));
    }

    // ── PermissionMemory tests ─────────────────────────────────────

    #[test]
    fn test_memory_always_allow_persists() {
        let mut mem = PermissionMemory::new();
        let sid = Uuid::new_v4();
        mem.remember_choice(sid, "Bash".to_string(), PermissionChoice::AlwaysAllow);
        assert!(mem.is_always_allowed(sid, "Bash"));
    }

    #[test]
    fn test_memory_deny_persists() {
        let mut mem = PermissionMemory::new();
        let sid = Uuid::new_v4();
        mem.remember_choice(sid, "Bash".to_string(), PermissionChoice::Deny);
        assert!(mem.is_always_denied("Bash"));
    }

    #[test]
    fn test_memory_allow_once_not_remembered() {
        let mut mem = PermissionMemory::new();
        let sid = Uuid::new_v4();
        mem.remember_choice(sid, "Bash".to_string(), PermissionChoice::AllowOnce);
        assert!(!mem.is_always_allowed(sid, "Bash"));
        assert!(!mem.is_always_denied("Bash"));
    }

    #[test]
    fn test_memory_clear_session() {
        let mut mem = PermissionMemory::new();
        let sid = Uuid::new_v4();
        mem.remember_choice(sid, "Bash".to_string(), PermissionChoice::AlwaysAllow);
        assert!(mem.is_always_allowed(sid, "Bash"));
        mem.clear_session(sid);
        // always_allowed is global, not session-scoped, so still true
        assert!(mem.is_always_allowed(sid, "Bash"));
    }

    #[test]
    fn test_memory_default_is_empty() {
        let mem = PermissionMemory::new();
        let sid = Uuid::new_v4();
        assert!(!mem.is_always_allowed(sid, "Bash"));
        assert!(!mem.is_always_denied("Bash"));
    }

    // ── PermissionManager: prompt creation & choice processing ──────

    #[test]
    fn test_create_prompt_for_known_tool() {
        let mgr = PermissionManager::new();
        let sid = Uuid::new_v4();
        let prompt = mgr.create_permission_prompt(
            "Bash",
            &serde_json::json!({"command": "ls -la"}),
            sid,
        );
        assert!(prompt.is_some());
        let p = prompt.unwrap();
        assert_eq!(p.tool_name, "Bash");
        assert_eq!(p.risk_level, RiskLevel::Medium);
        assert!(!p.is_confirmation);
    }

    #[test]
    fn test_create_prompt_for_unknown_tool() {
        let mgr = PermissionManager::new();
        let sid = Uuid::new_v4();
        let prompt = mgr.create_permission_prompt(
            "UnknownTool",
            &serde_json::json!({"arg": "val"}),
            sid,
        );
        assert!(prompt.is_some());
        let p = prompt.unwrap();
        assert_eq!(p.tool_name, "UnknownTool");
        assert_eq!(p.risk_level, RiskLevel::Medium);
    }

    #[test]
    fn test_create_prompt_dangerous_input_elevated_risk() {
        let mgr = PermissionManager::new();
        let sid = Uuid::new_v4();
        let prompt = mgr.create_permission_prompt(
            "Bash",
            &serde_json::json!({"command": "rm -rf /"}),
            sid,
        );
        let p = prompt.unwrap();
        assert_eq!(p.risk_level, RiskLevel::Critical);
    }

    #[test]
    fn test_process_choice_deny_returns_error() {
        let mut mgr = PermissionManager::new();
        let sid = Uuid::new_v4();
        let prompt = PermissionPrompt::new(
            "Bash".to_string(),
            serde_json::json!({"command": "ls"}),
            RiskLevel::Medium,
            "Run ls".to_string(),
        );
        let result = mgr.process_permission_choice(sid, &prompt, PermissionChoice::Deny);
        assert!(result.is_err());
    }

    #[test]
    fn test_process_choice_allow_once_succeeds() {
        let mut mgr = PermissionManager::new();
        let sid = Uuid::new_v4();
        let prompt = PermissionPrompt::new(
            "Bash".to_string(),
            serde_json::json!({"command": "ls"}),
            RiskLevel::Medium,
            "Run ls".to_string(),
        );
        assert!(mgr.process_permission_choice(sid, &prompt, PermissionChoice::AllowOnce).is_ok());
        // AllowOnce does NOT make it always allowed
        let next_prompt = mgr.create_permission_prompt(
            "Bash",
            &serde_json::json!({"command": "ls"}),
            sid,
        );
        assert!(next_prompt.is_some());
    }

    #[test]
    fn test_process_choice_always_allow_skips_future_prompts() {
        let mut mgr = PermissionManager::new();
        let sid = Uuid::new_v4();
        let prompt = PermissionPrompt::new(
            "Bash".to_string(),
            serde_json::json!({"command": "ls"}),
            RiskLevel::Medium,
            "Run ls".to_string(),
        );
        assert!(mgr.process_permission_choice(sid, &prompt, PermissionChoice::AlwaysAllow).is_ok());
        let next_prompt = mgr.create_permission_prompt(
            "Bash",
            &serde_json::json!({"command": "ls"}),
            sid,
        );
        assert!(next_prompt.is_none());
    }

    // ── PermissionPrompt helpers ────────────────────────────────────

    #[test]
    fn test_prompt_confirmation_factory() {
        let p = PermissionPrompt::confirmation("Bash".to_string(), "Confirm?".to_string());
        assert!(p.is_confirmation);
        assert_eq!(p.risk_level, RiskLevel::Safe);
        assert_eq!(p.tool_input, serde_json::json!({}));
    }

    #[test]
    fn test_prompt_display_text_high_risk() {
        let p = PermissionPrompt::new(
            "Bash".to_string(),
            serde_json::json!({"cmd": "ls"}),
            RiskLevel::High,
            "Run ls".to_string(),
        );
        let text = p.display_text();
        assert!(text.contains("🔥"));
        assert!(text.contains("Bash"));
        assert!(text.contains("Run ls"));
    }

    #[test]
    fn test_prompt_display_text_safe() {
        let p = PermissionPrompt::new(
            "Read".to_string(),
            serde_json::json!({"path": "/tmp"}),
            RiskLevel::Safe,
            "Read file".to_string(),
        );
        let text = p.display_text();
        assert!(text.contains("✓"));
    }

    // ── Default policies verification ───────────────────────────────

    #[test]
    fn test_default_bash_policy_registered() {
        let mgr = PermissionManager::new();
        let sid = Uuid::new_v4();
        let prompt = mgr.create_permission_prompt(
            "Bash",
            &serde_json::json!({"command": "ls"}),
            sid,
        ).unwrap();
        assert_eq!(prompt.risk_level, RiskLevel::Medium);
    }

    #[test]
    fn test_default_write_policy_denies_etc() {
        let mgr = PermissionManager::new();
        let sid = Uuid::new_v4();
        let prompt = mgr.create_permission_prompt(
            "FileWrite",
            &serde_json::json!({"path": "/etc/passwd"}),
            sid,
        ).unwrap();
        assert_eq!(prompt.risk_level, RiskLevel::Critical);
    }

    #[test]
    fn test_default_read_policy_is_safe() {
        let mgr = PermissionManager::new();
        let sid = Uuid::new_v4();
        let prompt = mgr.create_permission_prompt(
            "Read",
            &serde_json::json!({"path": "/home/user/file.rs"}),
            sid,
        ).unwrap();
        assert_eq!(prompt.risk_level, RiskLevel::Safe);
    }

    // ── Session lifecycle ───────────────────────────────────────────

    #[test]
    fn test_clear_session_removes_permissions() {
        let mut mgr = PermissionManager::new();
        let sid = Uuid::new_v4();
        let perm = Permission::new("file", "write", PermissionLevel::Write);
        mgr.grant_permission(sid, perm.clone());
        assert!(mgr.check_permission(sid, &perm).is_ok());
        mgr.clear_session(sid);
        assert!(mgr.check_permission(sid, &perm).is_err());
    }

    #[test]
    fn test_clear_session_keeps_default_permissions() {
        let mut mgr = PermissionManager::new();
        let perm = Permission::new("file", "read", PermissionLevel::Read);
        mgr.add_default_permission(perm.clone());
        let sid = Uuid::new_v4();
        mgr.clear_session(sid);
        assert!(mgr.check_permission(sid, &perm).is_ok());
    }

    #[test]
    fn test_session_permissions_merge_with_defaults() {
        let mut mgr = PermissionManager::new();
        let default_perm = Permission::new("file", "read", PermissionLevel::Read);
        mgr.add_default_permission(default_perm.clone());

        let sid = Uuid::new_v4();
        let session_perm = Permission::new("file", "write", PermissionLevel::Write);
        mgr.grant_permission(sid, session_perm.clone());

        let all = mgr.get_session_permissions(sid);
        assert!(all.contains(&default_perm));
        assert!(all.contains(&session_perm));
    }

    #[test]
    fn test_register_custom_tool_policy() {
        let mut mgr = PermissionManager::new();
        let policy = ToolPermissionPolicy::new(
            "CustomTool".to_string(),
            RiskLevel::High,
            "Custom dangerous tool".to_string(),
        )
        .add_deny_pattern("nuclear");
        mgr.register_tool_policy(policy);

        let sid = Uuid::new_v4();
        let prompt = mgr.create_permission_prompt(
            "CustomTool",
            &serde_json::json!({"action": "nuclear launch"}),
            sid,
        ).unwrap();
        assert_eq!(prompt.risk_level, RiskLevel::Critical);
    }
}
