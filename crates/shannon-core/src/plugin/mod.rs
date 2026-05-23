//! Plugin system for Shannon Code
//!
//! This module provides a plugin registry system that allows installing,
//! managing, and discovering plugins from various sources (git repositories,
//! local paths, or a remote index).

pub mod config;
pub mod error;
pub mod index;
pub mod index_builder;
pub mod manifest;
pub mod registry;

pub use config::{PluginState, PluginsConfig};
pub use error::{PluginError, PluginResult};
pub use index::{IndexEntry, PluginIndex};
pub use index_builder::{BuiltIndexEntry, IndexBuilder, IndexFile, IndexMetadata};
pub use manifest::{PluginKind, PluginManifest, PluginPermission, TransportConfig};
pub use registry::{InstalledPlugin, PluginRegistry};
