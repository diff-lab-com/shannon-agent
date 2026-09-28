//! # MCP Server Approval
//!
//! Manages approval and risk assessment for MCP server connections.
//! Provides policy-based auto-approval, explicit user approval/denial,
//! and risk assessment for server capabilities and permissions.
//!
//! Based on Claude Code's `mcpServerApproval.tsx`.
//!
//! ## Architecture
//!
//! - [`McpApprovalManager`]: Tracks approved, denied, and pending servers
//! - [`McpApprovalPolicy`]: Configurable policy for auto-approval decisions
//! - [`McpServerApprovalRequest`]: A request to approve a new MCP server
//! - [`ApprovalDecision`]: The outcome of an approval request
//! - [`RiskAssessment`]: Risk evaluation for an MCP server

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use thiserror::Error;

// ============================================================================
// Approval Identity Binding (review F6)
// ============================================================================

/// Compute the identity fingerprint that binds an approval record to the
/// exact server definition the user approved.
///
/// Input is `(server_name, command_or_url, args)` joined with NUL separators
/// (no field can bleed across a boundary), hashed with SHA-256 and
/// hex-encoded. Re-pointing a server at a different command, changing its
/// arguments, or swapping its URL therefore invalidates the stored approval
/// and re-triggers the approval policy on the next startup.
///
/// Note: the config registry (`mcp_advanced::McpServerRegistry`) does not
/// track which file a server was loaded from, so the source path cannot
/// participate in the binding yet — the command/args hash is the load-bearing
/// half (a renamed command is exactly the re-approval trigger F6 asks for).
pub fn mcp_server_fingerprint(server_name: &str, command_or_url: &str, args: &[String]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(server_name.as_bytes());
    hasher.update([0u8]);
    hasher.update(command_or_url.as_bytes());
    hasher.update([0u8]);
    for arg in args {
        hasher.update(arg.as_bytes());
        hasher.update([0u8]);
    }
    hex::encode(hasher.finalize())
}

/// One persisted approval record, bound to the server identity.
///
/// Legacy (pre-F6) files store plain name strings; those load as entries with
/// `fingerprint: None` and keep matching by name alone — they can only ever
/// originate from the user domain, because project-local approval files are
/// refused at load time (see [`McpApprovalManager::load_user_domain`]).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpApprovalEntry {
    /// Server name this record applies to.
    pub name: String,
    /// [`mcp_server_fingerprint`] of the approved identity. `None` marks a
    /// legacy name-only record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
}

impl McpApprovalEntry {
    /// Whether this record admits the given identity fingerprint.
    ///
    /// Bound records match only their recorded fingerprint. Legacy records
    /// match by name (with a deprecation warning logged at load time). A
    /// bound record consulted without a fingerprint is fail-closed: no match.
    fn matches(&self, fingerprint: Option<&str>) -> bool {
        match (&self.fingerprint, fingerprint) {
            (Some(stored), Some(current)) => stored == current,
            (None, _) => true,
            (Some(_), None) => false,
        }
    }
}

/// File-format wrapper accepting both the legacy `["name", …]` entry lists
/// and the F6 `[{"name": …, "fingerprint": …}, …]` objects.
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum ApprovalEntryRepr {
    Entry(McpApprovalEntry),
    Legacy(String),
}

impl From<ApprovalEntryRepr> for Option<McpApprovalEntry> {
    fn from(repr: ApprovalEntryRepr) -> Self {
        match repr {
            ApprovalEntryRepr::Entry(entry) => Some(entry),
            ApprovalEntryRepr::Legacy(name) => Some(McpApprovalEntry {
                name,
                fingerprint: None,
            }),
        }
    }
}

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during MCP server approval.
#[derive(Error, Debug)]
pub enum McpApprovalError {
    #[error("Server already approved: {0}")]
    AlreadyApproved(String),

    #[error("Server already denied: {0}")]
    AlreadyDenied(String),

    #[error("Approval request not found for: {0}")]
    RequestNotFound(String),

    #[error("Approval timeout: {0}s elapsed without decision")]
    Timeout(u64),

    #[error("Invalid approval request: {0}")]
    InvalidRequest(String),

    #[error("Permission denied for server: {0}")]
    PermissionDenied(String),
}

// ============================================================================
// Transport Type
// ============================================================================

/// Transport mechanism for an MCP server connection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum McpTransportType {
    /// Standard input/output (subprocess)
    Stdio,
    /// Server-Sent Events over HTTP
    Sse,
    /// Streamable HTTP transport
    StreamableHttp,
}

impl std::fmt::Display for McpTransportType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            McpTransportType::Stdio => write!(f, "stdio"),
            McpTransportType::Sse => write!(f, "sse"),
            McpTransportType::StreamableHttp => write!(f, "streamable-http"),
        }
    }
}

impl McpTransportType {
    /// Check whether this transport type involves network access.
    pub fn is_network(&self) -> bool {
        matches!(
            self,
            McpTransportType::Sse | McpTransportType::StreamableHttp
        )
    }

    /// Check whether this is a local-only transport.
    pub fn is_local(&self) -> bool {
        matches!(self, McpTransportType::Stdio)
    }
}

// ============================================================================
// Risk Assessment
// ============================================================================

/// Risk level for an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum McpRiskLevel {
    /// No risk -- read-only or well-known local tool.
    Low,
    /// Moderate risk -- server has some capabilities that need review.
    Medium,
    /// High risk -- server requests sensitive permissions or network access.
    High,
}

impl std::fmt::Display for McpRiskLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            McpRiskLevel::Low => write!(f, "low"),
            McpRiskLevel::Medium => write!(f, "medium"),
            McpRiskLevel::High => write!(f, "high"),
        }
    }
}

/// Risk assessment for an MCP server approval request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskAssessment {
    /// The assessed risk level.
    pub level: McpRiskLevel,
    /// Human-readable concerns identified during assessment.
    pub concerns: Vec<String>,
    /// Whether this server can be auto-approved based on the assessment.
    pub auto_approve: bool,
}

impl Default for RiskAssessment {
    fn default() -> Self {
        Self {
            level: McpRiskLevel::Medium,
            concerns: Vec::new(),
            auto_approve: false,
        }
    }
}

// ============================================================================
// Approval Request
// ============================================================================

/// A request to approve an MCP server connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerApprovalRequest {
    /// Human-readable name of the MCP server.
    pub server_name: String,
    /// Optional URL for HTTP-based transports.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_url: Option<String>,
    /// Transport type used by the server.
    pub transport_type: McpTransportType,
    /// Capabilities advertised by the server (e.g., "tools", "resources", "prompts").
    pub capabilities: Vec<String>,
    /// Permissions requested by the server.
    pub requested_permissions: Vec<String>,
    /// Identity binding ([`mcp_server_fingerprint`]) of the server definition
    /// behind this request. `None` keeps legacy behavior for callers that
    /// have no command/URL identity (requests then cannot match bound
    /// approval records — fail-closed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_fingerprint: Option<String>,
    /// Pre-computed risk assessment.
    pub risk_assessment: RiskAssessment,
}

impl McpServerApprovalRequest {
    /// Create a new approval request.
    pub fn new(server_name: &str, transport_type: McpTransportType) -> Self {
        Self {
            server_name: server_name.to_string(),
            server_url: None,
            transport_type,
            capabilities: Vec::new(),
            requested_permissions: Vec::new(),
            command_fingerprint: None,
            risk_assessment: RiskAssessment::default(),
        }
    }

    /// Create a new approval request with a URL.
    pub fn with_url(server_name: &str, transport_type: McpTransportType, server_url: &str) -> Self {
        Self {
            server_name: server_name.to_string(),
            server_url: Some(server_url.to_string()),
            transport_type,
            capabilities: Vec::new(),
            requested_permissions: Vec::new(),
            command_fingerprint: None,
            risk_assessment: RiskAssessment::default(),
        }
    }

    /// Attach the identity fingerprint for `(name, command/URL, args)`.
    pub fn with_fingerprint(mut self, command_or_url: &str, args: &[String]) -> Self {
        self.command_fingerprint = Some(mcp_server_fingerprint(
            &self.server_name,
            command_or_url,
            args,
        ));
        self
    }

    /// Check whether the server requests write permissions.
    pub fn requests_write_access(&self) -> bool {
        self.requested_permissions.iter().any(|p| {
            p.to_lowercase().contains("write")
                || p.to_lowercase().contains("modify")
                || p.to_lowercase().contains("delete")
                || p.to_lowercase().contains("execute")
        })
    }

    /// Check whether the server requests network access.
    pub fn requests_network_access(&self) -> bool {
        self.requested_permissions.iter().any(|p| {
            p.to_lowercase().contains("network")
                || p.to_lowercase().contains("http")
                || p.to_lowercase().contains("fetch")
                || p.to_lowercase().contains("url")
        })
    }

    /// Check whether the server provides tools.
    pub fn has_tools(&self) -> bool {
        self.capabilities
            .iter()
            .any(|c| c.to_lowercase() == "tools")
    }
}

// ============================================================================
// Approval Decision
// ============================================================================

/// The outcome of an MCP server approval request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ApprovalDecision {
    /// The server is fully approved.
    Approve,
    /// The server is denied.
    Deny,
    /// The server is approved with a restricted set of permissions.
    ApproveWithRestrictions {
        /// Permissions that are explicitly allowed.
        allowed_permissions: Vec<String>,
    },
}

impl std::fmt::Display for ApprovalDecision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApprovalDecision::Approve => write!(f, "approved"),
            ApprovalDecision::Deny => write!(f, "denied"),
            ApprovalDecision::ApproveWithRestrictions {
                allowed_permissions,
            } => write!(f, "approved with restrictions: {allowed_permissions:?}"),
        }
    }
}

// ============================================================================
// Approval Policy
// ============================================================================

/// Policy configuration for MCP server auto-approval decisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpApprovalPolicy {
    /// Auto-approve servers that only provide read-only capabilities.
    pub auto_approve_read_only: bool,
    /// Auto-approve servers using local (stdio) transport.
    pub auto_approve_local: bool,
    /// Require explicit approval for servers with network transport.
    pub require_approval_for_network: bool,
    /// Maximum seconds to wait for a user decision before timing out.
    pub max_approval_timeout_secs: u64,
}

impl Default for McpApprovalPolicy {
    fn default() -> Self {
        Self {
            auto_approve_read_only: true,
            auto_approve_local: false,
            require_approval_for_network: true,
            max_approval_timeout_secs: 60,
        }
    }
}

impl McpApprovalPolicy {
    /// Create a permissive policy that auto-approves most servers.
    pub fn permissive() -> Self {
        Self {
            auto_approve_read_only: true,
            auto_approve_local: true,
            require_approval_for_network: false,
            max_approval_timeout_secs: 120,
        }
    }

    /// Create a strict policy that requires approval for everything.
    pub fn strict() -> Self {
        Self {
            auto_approve_read_only: false,
            auto_approve_local: false,
            require_approval_for_network: true,
            max_approval_timeout_secs: 30,
        }
    }
}

// ============================================================================
// Approval Manager
// ============================================================================

/// Manages MCP server approval state.
///
/// Tracks which servers have been approved or denied, maintains a queue of
/// pending approval requests, and applies policy-based auto-approval logic.
pub struct McpApprovalManager {
    /// Policy used for auto-approval decisions.
    policy: McpApprovalPolicy,
    /// Approved servers keyed by name. Each entry carries its identity
    /// binding (F6): bound records only admit the exact approved command/URL
    /// + args; legacy name-only records admit any identity of that name.
    approved_servers: HashMap<String, McpApprovalEntry>,
    /// Set of denied server names.
    denied_servers: HashSet<String>,
    /// Pending approval requests awaiting user decision.
    pending_approvals: Vec<McpServerApprovalRequest>,
}

impl McpApprovalManager {
    /// Create a new approval manager with the given policy.
    pub fn new(policy: McpApprovalPolicy) -> Self {
        Self {
            policy,
            approved_servers: HashMap::new(),
            denied_servers: HashSet::new(),
            pending_approvals: Vec::new(),
        }
    }

    /// Create an approval manager with the default policy.
    pub fn with_defaults() -> Self {
        Self::new(McpApprovalPolicy::default())
    }

    /// Request approval for an MCP server.
    ///
    /// This method checks the following in order:
    /// 1. If the server is already approved, returns `Approve`.
    /// 2. If the server is already denied, returns `Deny`.
    /// 3. If the server can be auto-approved per policy, returns `Approve`.
    /// 4. Otherwise, adds the request to the pending queue and returns
    ///    `ApproveWithRestrictions` with an empty permissions list, indicating
    ///    that user action is required.
    pub fn request_approval(
        &mut self,
        request: McpServerApprovalRequest,
    ) -> Result<ApprovalDecision, McpApprovalError> {
        let name = request.server_name.clone();

        // Check already-approved (F6: identity-bound — a changed command or
        // argument set no longer short-circuits the policy).
        if self.is_identity_approved(&name, request.command_fingerprint.as_deref()) {
            return Ok(ApprovalDecision::Approve);
        }

        // Check already-denied
        if self.is_denied(&name) {
            return Ok(ApprovalDecision::Deny);
        }

        // Perform risk assessment
        let risk = self.assess_risk(&request);

        // Check auto-approve based on policy
        if self.check_auto_approve(&request) {
            self.approved_servers.insert(
                name.clone(),
                McpApprovalEntry {
                    name: name.clone(),
                    fingerprint: request.command_fingerprint.clone(),
                },
            );
            return Ok(ApprovalDecision::Approve);
        }

        // If risk assessment says auto-approve and risk is low
        if risk.auto_approve && risk.level == McpRiskLevel::Low {
            self.approved_servers.insert(
                name.clone(),
                McpApprovalEntry {
                    name: name.clone(),
                    fingerprint: request.command_fingerprint.clone(),
                },
            );
            return Ok(ApprovalDecision::Approve);
        }

        // Add to pending queue -- user must decide
        self.pending_approvals.push(request);

        Ok(ApprovalDecision::ApproveWithRestrictions {
            allowed_permissions: Vec::new(),
        })
    }

    /// Check whether a request should be auto-approved based on policy.
    ///
    /// Returns `true` if the policy allows auto-approval for this request.
    pub fn check_auto_approve(&self, request: &McpServerApprovalRequest) -> bool {
        // Network transport always requires approval if policy says so
        if self.policy.require_approval_for_network && request.transport_type.is_network() {
            return false;
        }

        // Servers requesting write access are never auto-approved
        if request.requests_write_access() {
            return false;
        }

        // Auto-approve local (stdio) servers if policy allows
        if self.policy.auto_approve_local && request.transport_type.is_local() {
            return true;
        }

        // Auto-approve read-only servers if policy allows
        if self.policy.auto_approve_read_only && self.is_read_only_server(request) {
            return true;
        }

        false
    }

    /// Assess the risk of an MCP server request.
    ///
    /// Returns a [`RiskAssessment`] with a risk level, list of concerns,
    /// and a recommendation on whether to auto-approve.
    pub fn assess_risk(&self, request: &McpServerApprovalRequest) -> RiskAssessment {
        let mut concerns: Vec<String> = Vec::new();
        let mut risk_score: u32 = 0; // 0 = low, 1 = medium, 2 = high

        // Network transport increases risk
        if request.transport_type.is_network() {
            risk_score += 1;
            concerns.push("Server uses network transport".to_string());
        }

        // Write access increases risk
        if request.requests_write_access() {
            risk_score += 1;
            concerns.push("Server requests write/modify/delete permissions".to_string());
        }

        // Network access (fetch, etc.) increases risk
        if request.requests_network_access() {
            risk_score += 1;
            concerns.push("Server requests network access capabilities".to_string());
        }

        // Unknown URL is a concern
        if let Some(ref url) = request.server_url {
            if url.starts_with("http://") {
                risk_score += 1;
                concerns.push("Server uses insecure HTTP (not HTTPS)".to_string());
            }
        }

        // Determine risk level
        let level = if risk_score >= 2 {
            McpRiskLevel::High
        } else if risk_score == 1 {
            McpRiskLevel::Medium
        } else {
            McpRiskLevel::Low
        };

        // Auto-approve recommendation: low risk + local transport
        let auto_approve = level == McpRiskLevel::Low
            && request.transport_type.is_local()
            && !request.requests_write_access();

        RiskAssessment {
            level,
            concerns,
            auto_approve,
        }
    }

    /// Explicitly approve a server by name.
    ///
    /// Removes the server from the denied set (if present) and the pending
    /// queue, and adds it to the approved set.
    ///
    /// The record is stored WITHOUT an identity binding (legacy name-only
    /// semantics). Prefer [`Self::approve_server_identity`] when the request
    /// (and its fingerprint) is at hand, so a later command change re-triggers
    /// approval.
    pub fn approve_server(&mut self, server_name: &str) {
        self.approve_server_identity(server_name, None);
    }

    /// Explicitly approve a server together with its identity binding (F6).
    ///
    /// A `Some(fingerprint)` record only admits the exact approved server
    /// definition; `None` keeps legacy name-only semantics.
    pub fn approve_server_identity(&mut self, server_name: &str, fingerprint: Option<String>) {
        self.approved_servers.insert(
            server_name.to_string(),
            McpApprovalEntry {
                name: server_name.to_string(),
                fingerprint,
            },
        );
        self.denied_servers.remove(server_name);
        self.pending_approvals
            .retain(|r| r.server_name != server_name);
    }

    /// Explicitly deny a server by name.
    ///
    /// Removes the server from the approved set (if present) and the pending
    /// queue, and adds it to the denied set.
    pub fn deny_server(&mut self, server_name: &str) {
        self.denied_servers.insert(server_name.to_string());
        self.approved_servers.remove(server_name);
        self.pending_approvals
            .retain(|r| r.server_name != server_name);
    }

    /// Check whether a server is approved.
    ///
    /// Legacy name-only check: true when ANY approved record exists for the
    /// name. Identity-aware callers (the approval policy, headless gating)
    /// must use [`Self::is_identity_approved`] so a changed command re-triggers
    /// approval.
    pub fn is_approved(&self, server_name: &str) -> bool {
        self.approved_servers.contains_key(server_name)
    }

    /// Check whether the exact server identity is approved (F6).
    ///
    /// - A bound record matches only its recorded fingerprint.
    /// - A legacy name-only record matches by name (it can only originate
    ///   from the user domain — project-local approval files are refused at
    ///   load time by [`Self::load_user_domain`]).
    /// - A bound record consulted without a fingerprint does NOT match
    ///   (fail-closed).
    pub fn is_identity_approved(&self, server_name: &str, fingerprint: Option<&str>) -> bool {
        self.approved_servers
            .get(server_name)
            .is_some_and(|entry| entry.matches(fingerprint))
    }

    /// Check whether a server is denied.
    pub fn is_denied(&self, server_name: &str) -> bool {
        self.denied_servers.contains(server_name)
    }

    /// Get the list of pending approval requests.
    pub fn pending_requests(&self) -> &[McpServerApprovalRequest] {
        &self.pending_approvals
    }

    /// Get the set of approved server records, keyed by name.
    pub fn approved_servers(&self) -> &HashMap<String, McpApprovalEntry> {
        &self.approved_servers
    }

    /// Get the set of denied server names.
    pub fn denied_servers(&self) -> &HashSet<String> {
        &self.denied_servers
    }

    /// Get a reference to the approval policy.
    pub fn policy(&self) -> &McpApprovalPolicy {
        &self.policy
    }

    /// Clear all approval state (for testing purposes).
    pub fn clear(&mut self) {
        self.approved_servers.clear();
        self.denied_servers.clear();
        self.pending_approvals.clear();
    }

    /// Check whether a server is read-only based on its permissions.
    fn is_read_only_server(&self, request: &McpServerApprovalRequest) -> bool {
        !request.requests_write_access() && !request.requests_network_access()
    }

    /// Canonical user-domain approval state path (review F6).
    ///
    /// `$SHANNON_HOME/mcp_approvals.json` (i.e. `~/.shannon/mcp_approvals.json`
    /// by default). Approvals are only honored from this store (or an explicit
    /// `SHANNON_MCP_APPROVALS` override); approval files inside a repository /
    /// project directory are refused by [`Self::load_user_domain`] — a
    /// repo-supplied file must never be able to pre-seed "approved" servers.
    pub fn default_state_path() -> std::path::PathBuf {
        if let Ok(home) = crate::session_log::default_shannon_home() {
            return home.join("mcp_approvals.json");
        }
        // No resolvable home directory (rare embedded/CI cases): keep a
        // writable fallback so save/load still round-trips. load_user_domain
        // refuses to LOAD it when it is project-scoped.
        std::path::PathBuf::from(".shannon").join("mcp_approvals.json")
    }

    /// Load approval state from the user-domain store (review F6).
    ///
    /// Resolution order:
    /// 1. `SHANNON_MCP_APPROVALS` env override (explicit; trusted wherever it
    ///    points — this is how tests and multi-root setups relocate the store).
    /// 2. [`Self::default_state_path`] (`~/.shannon/mcp_approvals.json`).
    ///
    /// When the override is absent and a project-local `.shannon/mcp_approvals.json`
    /// exists, it is logged and IGNORED (trust-domain inversion: a repo must
    /// not be able to ship pre-approved servers).
    pub fn load_user_domain(&mut self) -> Result<(), std::io::Error> {
        if let Ok(path) = std::env::var("SHANNON_MCP_APPROVALS") {
            return self.load_from_file(std::path::Path::new(&path));
        }
        let cwd_file = std::path::Path::new(".shannon").join("mcp_approvals.json");
        if cwd_file.exists() {
            tracing::warn!(
                "Ignoring project-local MCP approval file {} — approvals are only \
                 honored from the user store {}; re-approve servers there if intended",
                cwd_file.display(),
                Self::default_state_path().display()
            );
        }
        let path = Self::default_state_path();
        if is_project_scoped(&path) {
            tracing::warn!(
                "Refusing to load MCP approvals from project-scoped path {}",
                path.display()
            );
            return Ok(());
        }
        self.load_from_file(&path)
    }

    /// Persist the current approval state to a file.
    ///
    /// Written as JSON: `{"approved": [{"name": …, "fingerprint": …}], "denied":
    /// ["…"]}`. The path is caller-provided; startup readers only honor the
    /// user-domain location (see [`Self::load_user_domain`]), so callers should
    /// pass [`Self::default_state_path`] unless relocating explicitly.
    pub fn save_to_file(&self, path: &std::path::Path) -> Result<(), std::io::Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = ApprovalStateFile {
            approved: self
                .approved_servers
                .values()
                .cloned()
                .map(ApprovalEntryRepr::Entry)
                .collect(),
            denied: self.denied_servers.iter().cloned().collect(),
        };
        let json = serde_json::to_string_pretty(&data)?;
        std::fs::write(path, json)
    }

    /// Load approval state from a file previously written by `save_to_file`.
    ///
    /// Merges the file's state into the current manager. Returns `Ok(())` if
    /// the file doesn't exist (no previously saved state). Accepts both the
    /// F6 object entry format and the legacy plain-string format.
    pub fn load_from_file(&mut self, path: &std::path::Path) -> Result<(), std::io::Error> {
        if !path.exists() {
            return Ok(());
        }
        let content = std::fs::read_to_string(path)?;
        let data: ApprovalStateFile = serde_json::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        for repr in data.approved {
            if let Some(entry) = Option::<McpApprovalEntry>::from(repr) {
                if entry.fingerprint.is_none() {
                    tracing::warn!(
                        "MCP approval for '{}' has no identity binding (legacy format); \
                         it still matches by name but will not survive command changes — \
                         re-approve to bind it",
                        entry.name
                    );
                }
                self.approved_servers.insert(entry.name.clone(), entry);
            }
        }
        for name in data.denied {
            self.denied_servers.insert(name);
        }
        Ok(())
    }

    /// Clear persisted approval state by deleting the file.
    pub fn reset_persisted(path: &std::path::Path) -> Result<(), std::io::Error> {
        if path.exists() {
            std::fs::remove_file(path)
        } else {
            Ok(())
        }
    }
}

/// Serialization helper for persisting approval state.
///
/// `approved` entries may be either F6 objects (`{"name", "fingerprint"}`) or
/// legacy plain strings; see [`ApprovalEntryRepr`].
#[derive(Serialize, Deserialize)]
struct ApprovalStateFile {
    #[serde(default)]
    approved: Vec<ApprovalEntryRepr>,
    #[serde(default)]
    denied: Vec<String>,
}

/// Whether `path` lives inside the current working directory (the project
/// trust domain). Relative paths are project-scoped by definition; absolute
/// paths are compared lexically, then canonically when both resolve.
fn is_project_scoped(path: &std::path::Path) -> bool {
    if path.is_relative() {
        return true;
    }
    let Ok(cwd) = std::env::current_dir() else {
        return false;
    };
    if path.starts_with(&cwd) {
        return true;
    }
    match (path.canonicalize(), cwd.canonicalize()) {
        (Ok(p), Ok(c)) => p.starts_with(c),
        _ => false,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------------------
    // McpTransportType tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_transport_is_network() {
        assert!(!McpTransportType::Stdio.is_network());
        assert!(McpTransportType::Sse.is_network());
        assert!(McpTransportType::StreamableHttp.is_network());
    }

    #[test]
    fn test_transport_is_local() {
        assert!(McpTransportType::Stdio.is_local());
        assert!(!McpTransportType::Sse.is_local());
        assert!(!McpTransportType::StreamableHttp.is_local());
    }

    #[test]
    fn test_transport_display() {
        assert_eq!(McpTransportType::Stdio.to_string(), "stdio");
        assert_eq!(McpTransportType::Sse.to_string(), "sse");
        assert_eq!(
            McpTransportType::StreamableHttp.to_string(),
            "streamable-http"
        );
    }

    // ---------------------------------------------------------------------------
    // McpRiskLevel tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_risk_level_display() {
        assert_eq!(McpRiskLevel::Low.to_string(), "low");
        assert_eq!(McpRiskLevel::Medium.to_string(), "medium");
        assert_eq!(McpRiskLevel::High.to_string(), "high");
    }

    // ---------------------------------------------------------------------------
    // McpApprovalPolicy tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_policy_default() {
        let policy = McpApprovalPolicy::default();
        assert!(policy.auto_approve_read_only);
        assert!(!policy.auto_approve_local);
        assert!(policy.require_approval_for_network);
        assert_eq!(policy.max_approval_timeout_secs, 60);
    }

    #[test]
    fn test_policy_permissive() {
        let policy = McpApprovalPolicy::permissive();
        assert!(policy.auto_approve_read_only);
        assert!(policy.auto_approve_local);
        assert!(!policy.require_approval_for_network);
    }

    #[test]
    fn test_policy_strict() {
        let policy = McpApprovalPolicy::strict();
        assert!(!policy.auto_approve_read_only);
        assert!(!policy.auto_approve_local);
        assert!(policy.require_approval_for_network);
    }

    #[test]
    fn test_policy_serialization() {
        let policy = McpApprovalPolicy::default();
        let json = serde_json::to_string(&policy).unwrap();
        let deserialized: McpApprovalPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(
            deserialized.auto_approve_read_only,
            policy.auto_approve_read_only
        );
    }

    // ---------------------------------------------------------------------------
    // McpServerApprovalRequest tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_request_new() {
        let req = McpServerApprovalRequest::new("test-server", McpTransportType::Stdio);
        assert_eq!(req.server_name, "test-server");
        assert!(req.server_url.is_none());
        assert!(req.capabilities.is_empty());
        assert!(req.requested_permissions.is_empty());
    }

    #[test]
    fn test_request_with_url() {
        let req = McpServerApprovalRequest::with_url(
            "remote-server",
            McpTransportType::Sse,
            "https://example.com/mcp",
        );
        assert_eq!(req.server_url.as_deref(), Some("https://example.com/mcp"));
    }

    #[test]
    fn test_request_write_access() {
        let mut req = McpServerApprovalRequest::new("server", McpTransportType::Stdio);
        assert!(!req.requests_write_access());

        req.requested_permissions.push("write_files".to_string());
        assert!(req.requests_write_access());

        req.requested_permissions
            .push("modify_database".to_string());
        assert!(req.requests_write_access());
    }

    #[test]
    fn test_request_network_access() {
        let mut req = McpServerApprovalRequest::new("server", McpTransportType::Stdio);
        assert!(!req.requests_network_access());

        req.requested_permissions.push("fetch_url".to_string());
        assert!(req.requests_network_access());
    }

    #[test]
    fn test_request_has_tools() {
        let mut req = McpServerApprovalRequest::new("server", McpTransportType::Stdio);
        assert!(!req.has_tools());

        req.capabilities.push("tools".to_string());
        assert!(req.has_tools());

        req.capabilities.push("resources".to_string());
        assert!(req.has_tools());
    }

    #[test]
    fn test_request_serialization() {
        let req = McpServerApprovalRequest::new("server", McpTransportType::Sse);
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: McpServerApprovalRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.server_name, "server");
    }

    // ---------------------------------------------------------------------------
    // ApprovalDecision tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_decision_display() {
        assert_eq!(ApprovalDecision::Approve.to_string(), "approved");
        assert_eq!(ApprovalDecision::Deny.to_string(), "denied");
        let restricted = ApprovalDecision::ApproveWithRestrictions {
            allowed_permissions: vec!["read".to_string()],
        };
        assert!(
            restricted
                .to_string()
                .contains("approved with restrictions")
        );
    }

    #[test]
    fn test_decision_serialization() {
        let decision = ApprovalDecision::Approve;
        let json = serde_json::to_string(&decision).unwrap();
        let deserialized: ApprovalDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, ApprovalDecision::Approve);
    }

    // ---------------------------------------------------------------------------
    // RiskAssessment tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_risk_assessment_default() {
        let assessment = RiskAssessment::default();
        assert_eq!(assessment.level, McpRiskLevel::Medium);
        assert!(assessment.concerns.is_empty());
        assert!(!assessment.auto_approve);
    }

    // ---------------------------------------------------------------------------
    // McpApprovalManager tests
    // ---------------------------------------------------------------------------

    fn make_manager() -> McpApprovalManager {
        McpApprovalManager::with_defaults()
    }

    fn make_read_only_request() -> McpServerApprovalRequest {
        let mut req = McpServerApprovalRequest::new("read-only-server", McpTransportType::Stdio);
        req.capabilities.push("tools".to_string());
        req.requested_permissions.push("read_files".to_string());
        req
    }

    fn make_network_request() -> McpServerApprovalRequest {
        McpServerApprovalRequest::with_url(
            "network-server",
            McpTransportType::Sse,
            "https://example.com/mcp",
        )
    }

    fn make_write_request() -> McpServerApprovalRequest {
        let mut req = McpServerApprovalRequest::new("write-server", McpTransportType::Stdio);
        req.requested_permissions.push("write_files".to_string());
        req.requested_permissions
            .push("modify_database".to_string());
        req
    }

    #[test]
    fn test_manager_new() {
        let mgr = make_manager();
        assert!(mgr.approved_servers().is_empty());
        assert!(mgr.denied_servers().is_empty());
        assert!(mgr.pending_requests().is_empty());
    }

    #[test]
    fn test_approve_and_check() {
        let mut mgr = make_manager();
        assert!(!mgr.is_approved("server-a"));
        mgr.approve_server("server-a");
        assert!(mgr.is_approved("server-a"));
    }

    #[test]
    fn test_deny_and_check() {
        let mut mgr = make_manager();
        assert!(!mgr.is_denied("server-b"));
        mgr.deny_server("server-b");
        assert!(mgr.is_denied("server-b"));
    }

    #[test]
    fn test_approve_removes_from_denied() {
        let mut mgr = make_manager();
        mgr.deny_server("server");
        assert!(mgr.is_denied("server"));
        mgr.approve_server("server");
        assert!(mgr.is_approved("server"));
        assert!(!mgr.is_denied("server"));
    }

    #[test]
    fn test_deny_removes_from_approved() {
        let mut mgr = make_manager();
        mgr.approve_server("server");
        assert!(mgr.is_approved("server"));
        mgr.deny_server("server");
        assert!(mgr.is_denied("server"));
        assert!(!mgr.is_approved("server"));
    }

    #[test]
    fn test_request_approval_already_approved() {
        let mut mgr = make_manager();
        mgr.approve_server("known-server");
        let req = McpServerApprovalRequest::new("known-server", McpTransportType::Stdio);
        let decision = mgr.request_approval(req).unwrap();
        assert_eq!(decision, ApprovalDecision::Approve);
    }

    #[test]
    fn test_request_approval_already_denied() {
        let mut mgr = make_manager();
        mgr.deny_server("bad-server");
        let req = McpServerApprovalRequest::new("bad-server", McpTransportType::Stdio);
        let decision = mgr.request_approval(req).unwrap();
        assert_eq!(decision, ApprovalDecision::Deny);
    }

    #[test]
    fn test_request_approval_auto_approve_read_only() {
        let mut mgr = make_manager();
        let req = make_read_only_request();
        let decision = mgr.request_approval(req).unwrap();
        assert_eq!(decision, ApprovalDecision::Approve);
        assert!(mgr.is_approved("read-only-server"));
    }

    #[test]
    fn test_request_approval_network_requires_user() {
        let mut mgr = make_manager();
        let req = make_network_request();
        let decision = mgr.request_approval(req).unwrap();
        // Default policy requires approval for network
        match decision {
            ApprovalDecision::ApproveWithRestrictions {
                allowed_permissions,
            } => {
                assert!(allowed_permissions.is_empty());
            }
            _ => panic!("Expected ApproveWithRestrictions, got {decision:?}"),
        }
        assert!(!mgr.is_approved("network-server"));
        assert_eq!(mgr.pending_requests().len(), 1);
    }

    #[test]
    fn test_approve_pending_server() {
        let mut mgr = make_manager();
        let req = make_network_request();
        let _ = mgr.request_approval(req);
        assert_eq!(mgr.pending_requests().len(), 1);

        // User approves the pending server
        mgr.approve_server("network-server");
        assert!(mgr.is_approved("network-server"));
        assert!(mgr.pending_requests().is_empty());
    }

    #[test]
    fn test_deny_pending_server() {
        let mut mgr = make_manager();
        let req = make_network_request();
        let _ = mgr.request_approval(req);
        assert_eq!(mgr.pending_requests().len(), 1);

        // User denies the pending server
        mgr.deny_server("network-server");
        assert!(mgr.is_denied("network-server"));
        assert!(mgr.pending_requests().is_empty());
    }

    #[test]
    fn test_clear() {
        let mut mgr = make_manager();
        mgr.approve_server("a");
        mgr.deny_server("b");
        let _ = mgr.request_approval(make_network_request());

        mgr.clear();
        assert!(mgr.approved_servers().is_empty());
        assert!(mgr.denied_servers().is_empty());
        assert!(mgr.pending_requests().is_empty());
    }

    // ---------------------------------------------------------------------------
    // Identity binding / user-domain store (review F6)
    // ---------------------------------------------------------------------------

    #[test]
    fn test_fingerprint_is_stable_and_identity_sensitive() {
        let base = mcp_server_fingerprint("srv", "uvx", &["mcp-server-x".to_string()]);
        assert_eq!(
            base,
            mcp_server_fingerprint("srv", "uvx", &["mcp-server-x".to_string()])
        );
        // Any identity change (name, command, args) must change the hash —
        // that is what re-triggers approval after a command rename.
        assert_ne!(
            base,
            mcp_server_fingerprint("other", "uvx", &["mcp-server-x".to_string()])
        );
        assert_ne!(
            base,
            mcp_server_fingerprint("srv", "npx", &["mcp-server-x".to_string()])
        );
        assert_ne!(
            base,
            mcp_server_fingerprint("srv", "uvx", &["mcp-server-y".to_string()])
        );
        assert_ne!(
            base,
            mcp_server_fingerprint(
                "srv",
                "uvx",
                &["mcp-server-x".to_string(), "extra".to_string()]
            )
        );
    }

    #[test]
    fn test_bound_approval_matches_only_its_identity() {
        let mut mgr = make_manager();
        let fp = mcp_server_fingerprint("srv", "uvx", &["server".to_string()]);
        mgr.approve_server_identity("srv", Some(fp.clone()));

        // Same identity → approved; legacy name check also passes.
        assert!(mgr.is_identity_approved("srv", Some(&fp)));
        assert!(mgr.is_approved("srv"));
        // Changed command → NOT approved (re-approval required).
        let other = mcp_server_fingerprint("srv", "node", &["evil.js".to_string()]);
        assert!(!mgr.is_identity_approved("srv", Some(&other)));
        // Bound record without a fingerprint → fail-closed.
        assert!(!mgr.is_identity_approved("srv", None));
    }

    #[test]
    fn test_changed_command_retriggers_policy() {
        let mut mgr = make_manager();
        let fp = mcp_server_fingerprint("srv", "uvx", &["server".to_string()]);
        mgr.approve_server_identity("srv", Some(fp));
        // Same command as the bound identity short-circuits to Approve.
        let same = McpServerApprovalRequest::new("srv", McpTransportType::Stdio)
            .with_fingerprint("uvx", &["server".to_string()]);
        assert_eq!(
            mgr.request_approval(same).unwrap(),
            ApprovalDecision::Approve
        );
        // A re-pointed command must NOT inherit the approval: with the default
        // policy a read-only stdio server still auto-approves, so use a
        // network request (requires user action) to observe the difference.
        let changed = McpServerApprovalRequest::with_url(
            "srv",
            McpTransportType::StreamableHttp,
            "https://evil.example/mcp",
        )
        .with_fingerprint("https://evil.example/mcp", &[]);
        match mgr.request_approval(changed).unwrap() {
            ApprovalDecision::ApproveWithRestrictions { .. } => {}
            other => panic!("expected re-approval, got {other:?}"),
        }
    }

    #[test]
    fn test_save_roundtrip_preserves_binding() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp_approvals.json");
        let fp = mcp_server_fingerprint("srv", "uvx", &["server".to_string()]);
        let mut mgr = make_manager();
        mgr.approve_server_identity("srv", Some(fp.clone()));
        mgr.deny_server("bad");
        mgr.save_to_file(&path).unwrap();

        let mut reloaded = make_manager();
        reloaded.load_from_file(&path).unwrap();
        assert!(reloaded.is_identity_approved("srv", Some(&fp)));
        assert!(!reloaded.is_identity_approved("srv", None));
        assert!(reloaded.is_denied("bad"));
        // Serialized as objects carrying the fingerprint.
        let json = std::fs::read_to_string(&path).unwrap();
        assert!(json.contains("fingerprint"));
    }

    #[test]
    fn test_load_legacy_plain_string_file_matches_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp_approvals.json");
        std::fs::write(
            &path,
            r#"{"approved": ["legacy-srv"], "denied": ["blocked-srv"]}"#,
        )
        .unwrap();

        let mut mgr = make_manager();
        mgr.load_from_file(&path).unwrap();
        // Backward read compat: name-only records match by name…
        assert!(mgr.is_approved("legacy-srv"));
        assert!(mgr.is_denied("blocked-srv"));
        // …including for requests without a fingerprint.
        let decision = mgr
            .request_approval(McpServerApprovalRequest::new(
                "legacy-srv",
                McpTransportType::Stdio,
            ))
            .unwrap();
        assert_eq!(decision, ApprovalDecision::Approve);
    }

    #[test]
    fn test_bound_record_does_not_admit_unbound_request() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp_approvals.json");
        let fp = mcp_server_fingerprint("srv", "uvx", &["server".to_string()]);
        std::fs::write(
            &path,
            format!(r#"{{"approved": [{{"name": "srv", "fingerprint": "{fp}"}}], "denied": []}}"#),
        )
        .unwrap();
        let mut mgr = make_manager();
        mgr.load_from_file(&path).unwrap();
        // A request without the matching fingerprint falls through to policy:
        // a network request must land in the pending queue, not Approve.
        let request = McpServerApprovalRequest::with_url(
            "srv",
            McpTransportType::StreamableHttp,
            "https://example.com/mcp",
        );
        match mgr.request_approval(request).unwrap() {
            ApprovalDecision::ApproveWithRestrictions { .. } => {}
            other => panic!("bound record must not admit a different identity: {other:?}"),
        }
        // The exact bound identity is approved.
        let request = McpServerApprovalRequest::new("srv", McpTransportType::Stdio)
            .with_fingerprint("uvx", &["server".to_string()]);
        assert_eq!(
            mgr.request_approval(request).unwrap(),
            ApprovalDecision::Approve
        );
    }

    #[test]
    fn test_project_scope_detection() {
        // Relative paths are project-scoped by definition.
        assert!(is_project_scoped(std::path::Path::new(
            ".shannon/mcp_approvals.json"
        )));
        // Absolute path inside the CWD is project-scoped…
        let cwd = std::env::current_dir().unwrap();
        assert!(is_project_scoped(&cwd.join("sub").join("approvals.json")));
        // …while an absolute path outside it is not.
        assert!(!is_project_scoped(
            &std::env::temp_dir().join("elsewhere/approvals.json")
        ));
    }

    // ---------------------------------------------------------------------------
    // Auto-approve policy tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_auto_approve_read_only_policy() {
        let mgr = McpApprovalManager::new(McpApprovalPolicy::default());
        let req = make_read_only_request();
        assert!(mgr.check_auto_approve(&req));
    }

    #[test]
    fn test_auto_approve_local_policy() {
        let mgr = McpApprovalManager::new(McpApprovalPolicy::permissive());
        let req = McpServerApprovalRequest::new("local", McpTransportType::Stdio);
        assert!(mgr.check_auto_approve(&req));
    }

    #[test]
    fn test_no_auto_approve_network_default_policy() {
        let mgr = McpApprovalManager::new(McpApprovalPolicy::default());
        let req = make_network_request();
        assert!(!mgr.check_auto_approve(&req));
    }

    #[test]
    fn test_no_auto_approve_write_permissions() {
        let mgr = McpApprovalManager::new(McpApprovalPolicy::permissive());
        let req = make_write_request();
        // Even permissive policy should not auto-approve write access
        // unless local+read-only
        assert!(!mgr.check_auto_approve(&req));
    }

    #[test]
    fn test_no_auto_approve_strict_policy() {
        let mgr = McpApprovalManager::new(McpApprovalPolicy::strict());
        let req = make_read_only_request();
        assert!(!mgr.check_auto_approve(&req));
    }

    // ---------------------------------------------------------------------------
    // Risk assessment tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_risk_assessment_low() {
        let mgr = make_manager();
        let req = make_read_only_request();
        let risk = mgr.assess_risk(&req);
        assert_eq!(risk.level, McpRiskLevel::Low);
        assert!(risk.concerns.is_empty());
        // Local + no write = auto-approve
        assert!(risk.auto_approve);
    }

    #[test]
    fn test_risk_assessment_network() {
        let mgr = make_manager();
        let req = make_network_request();
        let risk = mgr.assess_risk(&req);
        assert_eq!(risk.level, McpRiskLevel::Medium);
        assert!(!risk.concerns.is_empty());
        assert!(!risk.auto_approve);
    }

    #[test]
    fn test_risk_assessment_write_access() {
        let mgr = make_manager();
        let req = make_write_request();
        let risk = mgr.assess_risk(&req);
        assert_eq!(risk.level, McpRiskLevel::Medium);
        assert!(risk.concerns.iter().any(|c| c.contains("write")));
        assert!(!risk.auto_approve);
    }

    #[test]
    fn test_risk_assessment_insecure_http() {
        let mgr = make_manager();
        let req = McpServerApprovalRequest::with_url(
            "http-server",
            McpTransportType::Sse,
            "http://example.com/mcp",
        );
        let risk = mgr.assess_risk(&req);
        assert!(risk.concerns.iter().any(|c| c.contains("insecure")));
    }

    #[test]
    fn test_risk_assessment_high_risk() {
        let mgr = make_manager();
        let mut req = McpServerApprovalRequest::with_url(
            "risky-server",
            McpTransportType::Sse,
            "http://example.com/mcp",
        );
        req.requested_permissions.push("write_files".to_string());
        req.requested_permissions.push("fetch_url".to_string());
        let risk = mgr.assess_risk(&req);
        assert_eq!(risk.level, McpRiskLevel::High);
    }

    // ---------------------------------------------------------------------------
    // McpApprovalError tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_error_already_approved() {
        let err = McpApprovalError::AlreadyApproved("server".to_string());
        assert!(err.to_string().contains("already approved"));
    }

    #[test]
    fn test_error_already_denied() {
        let err = McpApprovalError::AlreadyDenied("server".to_string());
        assert!(err.to_string().contains("already denied"));
    }

    #[test]
    fn test_error_timeout() {
        let err = McpApprovalError::Timeout(60);
        assert!(err.to_string().contains("60"));
    }
}
