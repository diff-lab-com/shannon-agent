//! Plugin system for Shannon Code
//!
//! This module provides a plugin registry system that allows installing,
//! managing, and discovering plugins from various sources (git repositories,
//! local paths, or a remote index).

pub mod config;
pub mod error;
pub mod index;
pub mod index_builder;
pub mod installer;
pub mod manifest;
pub mod permissions;
pub mod registry;

pub use config::{PluginState, PluginsConfig};
pub use error::{PluginError, PluginResult};
pub use index::{IndexEntry, PluginIndex};
pub use index_builder::{BuiltIndexEntry, IndexBuilder, IndexFile, IndexMetadata};
pub use installer::{
    ExtensionKind, install_extension_bytes, install_extension_file, parse_extension_archive,
};
pub use manifest::{PluginKind, PluginManifest, PluginPermission, TransportConfig};
pub use permissions::{
    DENY_PREFIX, PermissionDecision, PluginPermissionError, PluginPermissionPolicy,
    PluginToolPolicies, admit_prompt_based_extension, emit_decision, gated_discover_tools_http,
    gated_discover_tools_stdio, owner_of_tool,
};
pub use registry::{InstalledPlugin, PluginRegistry};
