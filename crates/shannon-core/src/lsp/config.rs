//! # LSP Configuration
//!
//! Configuration for language servers, loaded from Shannon settings.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::settings::Settings;

/// LSP configuration loaded from settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspConfig {
    /// Manual server configurations: language -> command
    #[serde(default)]
    pub servers: HashMap<String, ServerConfig>,
    /// Whether to auto-prompt for server installation
    #[serde(default)]
    pub auto_prompt: bool,
}

impl Default for LspConfig {
    fn default() -> Self {
        Self {
            servers: HashMap::new(),
            auto_prompt: true,
        }
    }
}

impl LspConfig {
    /// Create LSP config from Shannon settings
    pub fn from_settings(_settings: &Settings) -> Self {
        // Try to load from settings file's lsp section
        // This is a simplified version - in practice, you'd load from .shannon.toml
        Self::default()
    }

    /// Add a server configuration
    pub fn add_server(&mut self, language: String, config: ServerConfig) {
        self.servers.insert(language, config);
    }

    /// Remove a server configuration
    pub fn remove_server(&mut self, language: &str) -> Option<ServerConfig> {
        self.servers.remove(language)
    }

    /// Get a server configuration for a language
    pub fn get_server(&self, language: &str) -> Option<&ServerConfig> {
        self.servers.get(language)
    }
}

/// Configuration for a single language server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Command to run (e.g., "rust-analyzer", "python-lsp-server")
    pub command: String,
    /// Arguments to pass to the command
    #[serde(default)]
    pub args: Vec<String>,
    /// Working directory (optional)
    #[serde(default)]
    pub working_dir: Option<PathBuf>,
    /// Environment variables (optional)
    #[serde(default)]
    pub env: HashMap<String, String>,
}

impl ServerConfig {
    /// Create a new server configuration
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            args: Vec::new(),
            working_dir: None,
            env: HashMap::new(),
        }
    }

    /// Add arguments to the server configuration
    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.args = args;
        self
    }

    /// Add environment variables to the server configuration
    pub fn with_env(mut self, env: HashMap<String, String>) -> Self {
        self.env = env;
        self
    }
}

/// Common language server configurations
pub mod known_servers {
    use super::*;

    /// Rust analyzer configuration
    pub fn rust_analyzer() -> ServerConfig {
        ServerConfig::new("rust-analyzer")
    }

    /// Python LSP server configuration
    pub fn pylsp() -> ServerConfig {
        ServerConfig::new("pylsp")
    }

    /// TypeScript language server configuration
    pub fn typescript_language_server() -> ServerConfig {
        ServerConfig::new("typescript-language-server")
            .with_args(vec!["--stdio".to_string()])
    }

    /// gopls configuration
    pub fn gopls() -> ServerConfig {
        ServerConfig::new("gopls")
    }

    /// clangd configuration
    pub fn clangd() -> ServerConfig {
        ServerConfig::new("clangd")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = LspConfig::default();
        assert!(config.servers.is_empty());
        assert!(config.auto_prompt);
    }

    #[test]
    fn test_add_remove_server() {
        let mut config = LspConfig::default();
        let server = ServerConfig::new("rust-analyzer");
        config.add_server("rust".to_string(), server);

        assert!(config.get_server("rust").is_some());
        assert!(config.get_server("python").is_none());

        let removed = config.remove_server("rust");
        assert!(removed.is_some());
        assert!(config.get_server("rust").is_none());
    }

    #[test]
    fn test_server_config_builder() {
        let config = ServerConfig::new("typescript-language-server")
            .with_args(vec!["--stdio".to_string()])
            .with_env(HashMap::from([("NODE_PATH".to_string(), "/usr/lib".to_string())]));

        assert_eq!(config.command, "typescript-language-server");
        assert_eq!(config.args, vec!["--stdio"]);
        assert_eq!(config.env.get("NODE_PATH").unwrap(), "/usr/lib");
    }

    #[test]
    fn test_known_servers() {
        let ra = known_servers::rust_analyzer();
        assert_eq!(ra.command, "rust-analyzer");
        assert!(ra.args.is_empty());

        let ts = known_servers::typescript_language_server();
        assert_eq!(ts.command, "typescript-language-server");
        assert_eq!(ts.args, vec!["--stdio"]);

        let pylsp = known_servers::pylsp();
        assert_eq!(pylsp.command, "pylsp");

        let gopls = known_servers::gopls();
        assert_eq!(gopls.command, "gopls");

        let clangd = known_servers::clangd();
        assert_eq!(clangd.command, "clangd");
    }

    #[test]
    fn test_config_serialization() {
        let mut config = LspConfig::default();
        config.add_server("rust".to_string(), known_servers::rust_analyzer());

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: LspConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.servers.len(), 1);
        assert_eq!(deserialized.servers["rust"].command, "rust-analyzer");
    }
}
