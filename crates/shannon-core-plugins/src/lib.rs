//! # Shannon Core Plugins
//!
//! Plugin and MCP (Model Context Protocol) system.
//!
//! This crate provides plugin-related functionality:
//! - Plugin manager and trait definitions
//! - MCP advanced channel management
//! - MCP server approval system
//! - Bridge service for external integrations

pub mod plugins;
pub mod mcp_advanced;
pub mod mcp_server_approval;
pub mod bridge_service;

// Re-export key types
pub use plugins::{Plugin, PluginManager, PluginRegistry, PluginError};
pub use mcp_advanced::{McpChannelManager, McpChannel, McpError};
pub use mcp_server_approval::{McpApprovalManager, ApprovalDecision, ApprovalRequest};
pub use bridge_service::{BridgeService, BridgeConfig, BridgeError};
