//! MCP server approval management

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Approval request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: Uuid,
    pub server_name: String,
    pub server_endpoint: String,
    pub permissions: Vec<String>,
    pub requested_at: chrono::DateTime<chrono::Utc>,
    pub status: ApprovalStatus,
}

/// Approval status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Denied,
    Revoked,
}

/// Approval decision
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    Approve,
    Deny,
}

/// MCP approval manager
pub struct McpApprovalManager {
    pending_requests: HashMap<Uuid, ApprovalRequest>,
    approved_servers: HashMap<String, Uuid>,
    denied_servers: HashMap<String, Uuid>,
}

impl McpApprovalManager {
    pub fn new() -> Self {
        Self {
            pending_requests: HashMap::new(),
            approved_servers: HashMap::new(),
            denied_servers: HashMap::new(),
        }
    }

    /// Create an approval request
    pub fn create_request(&mut self, server_name: String, server_endpoint: String, permissions: Vec<String>) -> Uuid {
        let id = Uuid::new_v4();
        let request = ApprovalRequest {
            id,
            server_name: server_name.clone(),
            server_endpoint,
            permissions,
            requested_at: chrono::Utc::now(),
            status: ApprovalStatus::Pending,
        };

        self.pending_requests.insert(id, request);
        id
    }

    /// Get pending request
    pub fn get_request(&self, id: &Uuid) -> Option<&ApprovalRequest> {
        self.pending_requests.get(id)
    }

    /// Make a decision on an approval request
    pub fn decide(&mut self, id: &Uuid, decision: ApprovalDecision) -> Result<(), ApprovalError> {
        if let Some(mut request) = self.pending_requests.remove(id) {
            let request_id = request.id;
            match decision {
                ApprovalDecision::Approve => {
                    request.status = ApprovalStatus::Approved;
                    self.approved_servers.insert(request.server_name.clone(), request_id);
                }
                ApprovalDecision::Deny => {
                    request.status = ApprovalStatus::Denied;
                    self.denied_servers.insert(request.server_name.clone(), request_id);
                }
            }
            Ok(())
        } else {
            Err(ApprovalError::NotFound(*id))
        }
    }

    /// Check if a server is approved
    pub fn is_approved(&self, server_name: &str) -> bool {
        self.approved_servers.contains_key(server_name)
    }

    /// Check if a server is denied
    pub fn is_denied(&self, server_name: &str) -> bool {
        self.denied_servers.contains_key(server_name)
    }

    /// Revoke approval for a server
    pub fn revoke(&mut self, server_name: &str) -> Result<(), ApprovalError> {
        if let Some(id) = self.approved_servers.remove(server_name) {
            if let Some(mut request) = self.pending_requests.remove(&id) {
                request.status = ApprovalStatus::Revoked;
                self.pending_requests.insert(id, request);
            }
            Ok(())
        } else {
            Err(ApprovalError::NotFoundByName(server_name.to_string()))
        }
    }

    /// List pending requests
    pub fn pending_requests(&self) -> Vec<&ApprovalRequest> {
        self.pending_requests.values().collect()
    }

    /// List approved servers
    pub fn approved_servers(&self) -> Vec<String> {
        self.approved_servers.keys().cloned().collect()
    }
}

impl Default for McpApprovalManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Approval errors
#[derive(Debug, thiserror::Error)]
pub enum ApprovalError {
    #[error("Request not found: {0}")]
    NotFound(Uuid),

    #[error("Server not found: {0}")]
    NotFoundByName(String),

    #[error("Server already approved: {0}")]
    AlreadyApproved(String),

    #[error("Server already denied: {0}")]
    AlreadyDenied(String),
}
