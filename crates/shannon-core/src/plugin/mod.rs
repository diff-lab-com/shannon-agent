//! Plugin system for Shannon Code
//!
//! This module provides a plugin registry system that allows installing,
//! managing, and discovering plugins from various sources (git repositories,
//! local paths, or a remote index).

pub mod error;
pub mod manifest;
pub mod registry;
pub mod config;
pub mod index;

pub use error::{PluginError, PluginResult};
pub use manifest::{PluginManifest, PluginKind, TransportConfig, PluginPermission};
pub use registry::{PluginRegistry, InstalledPlugin};
pub use config::{PluginsConfig, PluginState};
pub use index::{PluginIndex, IndexEntry};
