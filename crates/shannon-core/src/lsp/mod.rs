//! # Language Server Protocol (LSP) Integration
//!
//! This module provides LSP client functionality for Shannon Code, enabling
//! features like goto definition, find references, hover, and document symbols.
//!
//! ## Architecture
//!
//! - [`LspManager`]: Main manager for LSP client lifecycle
//! - [`LspClient`]: Low-level client for communicating with a language server
//! - [`ServerDiscovery`]: Discovers installed language servers on the system
//! - [`LspConfig`]: Configuration for LSP servers
//!
//! ## Example
//!
//! ```ignore
//! use shannon_core::lsp::LspManager;
//! use shannon_core::lsp::LspConfig;
//!
//! # async fn example() -> shannon_core::lsp::LspResult<()> {
//! let config = LspConfig::default();
//! let mut manager = LspManager::new(config);
//!
//! // Get or create a client for Rust
//! let client = manager.client_for("rust", std::path::Path::new("/project")).await?;
//!
//! // Use the client
//! let url = url::Url::parse("file:///project/src/main.rs")?;
//! let symbols = client.document_symbols(&url).await?;
//! # Ok(())
//! # }
//! ```

pub mod client;
pub mod config;
pub mod discovery;

pub use client::{LspClient, LspClientError};
pub use config::{LspConfig, ServerConfig};
pub use discovery::{DiscoveredServer, ServerDiscovery, ServerSource};

use std::collections::HashMap;
use std::path::Path;

use crate::settings::Settings;

/// Main manager for LSP client lifecycle
pub struct LspManager {
    /// LSP configuration
    config: LspConfig,
    /// Active LSP clients per language
    active_clients: HashMap<String, LspClient>,
}

impl LspManager {
    /// Create a new LSP manager from the given configuration
    pub fn new(config: LspConfig) -> Self {
        Self {
            config,
            active_clients: HashMap::new(),
        }
    }

    /// Create an LSP manager from Shannon settings
    pub fn from_settings(settings: &Settings) -> Self {
        let config = LspConfig::from_settings(settings);
        Self::new(config)
    }

    /// Get or create a client for the given language
    ///
    /// This will discover a server if one isn't already active, spawn it,
    /// and initialize it with the given root path.
    pub async fn client_for(
        &mut self,
        language: &str,
        root: &Path,
    ) -> Result<&mut LspClient, LspClientError> {
        // Check if we already have an active client
        if !self.active_clients.contains_key(language) {
            // Discover a server for this language
            let server = self.discover_server(language)?;

            // Spawn the client
            let mut client = LspClient::spawn(&server.command, &server.args).await?;

            // Initialize the client
            let root_uri = lsp_types::Url::from_directory_path(root)
                .map_err(|_| LspClientError::InvalidUri)?;
            client.initialize(&root_uri).await?;

            self.active_clients.insert(language.to_string(), client);
        }

        // Safe to unwrap because we just inserted it
        Ok(self.active_clients.get_mut(language).expect("just inserted above"))
    }

    /// Check if a server is available for the given language
    pub fn is_available(&self, language: &str) -> bool {
        self.config.servers.contains_key(language)
            || ServerDiscovery::find_server(language).is_some()
    }

    /// Get installation hint for a language server
    pub fn install_hint(language: &str) -> Option<String> {
        match language.to_lowercase().as_str() {
            "rust" | "rs" => Some(
                "Install rust-analyzer: rustup component add rust-analyzer".to_string(),
            ),
            "python" | "py" => Some(
                "Install pylsp: pip install python-lsp-server".to_string(),
            ),
            "typescript" | "ts" => Some(
                "Install: npm install -g typescript-language-server typescript".to_string(),
            ),
            "javascript" | "js" => Some(
                "Install: npm install -g typescript-language-server typescript".to_string(),
            ),
            "go" => Some(
                "Install gopls: go install golang.org/x/tools/gopls@latest".to_string(),
            ),
            "c" | "cpp" | "cxx" => Some(
                "Install clangd: apt install clangd or brew install llvm".to_string(),
            ),
            "java" => Some(
                "Install jdtls: https://github.com/eclipse/eclipse.jdt.ls".to_string(),
            ),
            "ruby" => Some(
                "Install solargraph: gem install solargraph".to_string(),
            ),
            _ => None,
        }
    }

    /// Discover a server for the given language
    fn discover_server(&self, language: &str) -> Result<DiscoveredServer, LspClientError> {
        // First check user config
        if let Some(server_config) = self.config.servers.get(language) {
            return Ok(DiscoveredServer {
                language: language.to_string(),
                command: server_config.command.clone(),
                args: server_config.args.clone(),
                source: ServerSource::UserConfig,
            });
        }

        // Then try system discovery
        ServerDiscovery::find_server(language)
            .ok_or_else(|| LspClientError::ServerNotFound(language.to_string()))
    }

    /// Shutdown all active clients
    pub async fn shutdown_all(&mut self) -> Result<(), LspClientError> {
        for (_language, mut client) in self.active_clients.drain() {
            client.shutdown().await?;
        }
        Ok(())
    }
}

impl Drop for LspManager {
    fn drop(&mut self) {
        // Best effort cleanup - we can't do async in Drop
        self.active_clients.clear();
    }
}

/// Result type for LSP operations
pub type LspResult<T> = Result<T, LspClientError>;
