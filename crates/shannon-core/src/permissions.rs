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
}

impl PermissionManager {
    /// Create a new permission manager with default permissions
    pub fn new() -> Self {
        Self {
            default_permissions: HashSet::new(),
            session_permissions: HashMap::new(),
            tool_permissions: HashMap::new(),
        }
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
}

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
