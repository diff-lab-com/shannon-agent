//! # Server Discovery
//!
//! Discovers language servers installed on the system.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

/// A discovered language server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredServer {
    /// Language this server supports (e.g., "rust", "python")
    pub language: String,
    /// Command to run
    pub command: String,
    /// Arguments to pass
    pub args: Vec<String>,
    /// Where this server was discovered
    pub source: ServerSource,
}

/// Source of a discovered server
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ServerSource {
    /// Found in VS Code extensions directory
    VscodeExtension(String),
    /// Found in system PATH
    SystemPath(String),
    /// Configured in user settings
    UserConfig,
    /// Built-in fallback
    BuiltIn,
}

/// Server discovery utilities
pub struct ServerDiscovery;

impl ServerDiscovery {
    /// Known language servers and their commands
    const KNOWN_SERVERS: &'static [(&'static str, &'static str, &'static [&'static str])] = &[
        ("rust", "rust-analyzer", &[] as &[&str]),
        ("python", "pylsp", &[]),
        ("python", "python-lsp-server", &[]),
        ("typescript", "typescript-language-server", &["--stdio"]),
        ("javascript", "typescript-language-server", &["--stdio"]),
        ("go", "gopls", &[]),
        ("c", "clangd", &[]),
        ("cpp", "clangd", &[]),
        ("cxx", "clangd", &[]),
        ("java", "jdtls", &[]),
        ("ruby", "solargraph", &["stdio"]),
        ("php", "intelephense", &["--stdio"]),
        ("lua", "lua-language-server", &[]),
        ("sh", "bash-language-server", &["start"]),
        ("json", "vscode-json-language-server", &["--stdio"]),
        ("yaml", "yaml-language-server", &["--stdio"]),
        ("html", "vscode-html-language-server", &["--stdio"]),
        ("css", "vscode-css-language-server", &["--stdio"]),
        ("toml", "taplo", &["lsp", "stdio"]),
        ("markdown", "vscode-markdown-languageserver", &["--stdio"]),
    ];

    /// Discover a language server for the given language
    pub fn find_server(language: &str) -> Option<DiscoveredServer> {
        let language_lower = language.to_lowercase();

        // Try VS Code extensions first
        if let Some(server) = Self::find_in_vscode_extensions(&language_lower) {
            return Some(server);
        }

        // Then try system PATH
        if let Some(server) = Self::find_in_path(&language_lower) {
            return Some(server);
        }

        None
    }

    /// List all discoverable servers on the system
    pub fn list_available() -> Vec<DiscoveredServer> {
        let mut servers = Vec::new();
        let mut seen_languages = HashMap::new();

        // Check VS Code extensions
        if let Ok(home) = std::env::var("HOME") {
            let extensions_dir = PathBuf::from(home).join(".vscode").join("extensions");

            if let Ok(entries) = std::fs::read_dir(&extensions_dir) {
                for entry in entries.flatten() {
                    if let Some(server) = Self::parse_vscode_extension(&entry.path()) {
                        if !seen_languages.contains_key(&server.language) {
                            seen_languages.insert(server.language.clone(), servers.len());
                            servers.push(server);
                        }
                    }
                }
            }
        }

        // Check system PATH for known servers
        for (language, command, args) in Self::KNOWN_SERVERS {
            if !seen_languages.contains_key(*language) && Self::command_exists(command) {
                servers.push(DiscoveredServer {
                    language: language.to_string(),
                    command: command.to_string(),
                    args: args.iter().map(|s| s.to_string()).collect(),
                    source: ServerSource::SystemPath(command.to_string()),
                });
                seen_languages.insert(language.to_string(), servers.len());
            }
        }

        servers
    }

    /// Find a language server in VS Code extensions directory
    fn find_in_vscode_extensions(language: &str) -> Option<DiscoveredServer> {
        let Ok(home) = std::env::var("HOME") else {
            return None;
        };

        let extensions_dir = PathBuf::from(home).join(".vscode").join("extensions");

        let Ok(entries) = std::fs::read_dir(&extensions_dir) else {
            return None;
        };

        // Map languages to their VS Code extension identifiers
        let language_extensions = Self::language_to_extension_map();

        let target_extensions = language_extensions.get(language)?;

        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(server) = Self::parse_vscode_extension(&path) {
                // Check if this extension matches our target language
                for ext_id in target_extensions {
                    if path.to_string_lossy().contains(ext_id) {
                        return Some(server);
                    }
                }
            }
        }

        None
    }

    /// Parse a VS Code extension directory to find the language server binary
    fn parse_vscode_extension(path: &Path) -> Option<DiscoveredServer> {
        let extension_name = path.file_name()?.to_string_lossy();

        // Known extension mappings
        let known_servers = [
            ("rust-analyzer", "rust", "rust-analyzer"),
            ("python-lang", "python", "pylsp"),
            ("vscode-python", "python", "pylsp"),
            (
                "vscode-typescript-next",
                "typescript",
                "typescript-language-server",
            ),
            ("golang", "go", "gopls"),
            ("vscode-clangd", "c", "clangd"),
            ("vscode-java", "java", "jdtls"),
            ("vscode-ruby", "ruby", "solargraph"),
            ("vscode-php", "php", "intelephense"),
            ("vscode-lua", "lua", "lua-language-server"),
            ("bash-language-server", "sh", "bash-language-server"),
        ];

        for (ext_pattern, language, command) in known_servers {
            if extension_name.contains(ext_pattern) {
                // Try to find the binary in common locations
                let bin_paths = [
                    path.join("extension").join("bin").join(command),
                    path.join("bin").join(command),
                    path.join("dist").join(command),
                ];

                for bin_path in &bin_paths {
                    if bin_path.exists() {
                        return Some(DiscoveredServer {
                            language: language.to_string(),
                            command: bin_path.to_string_lossy().to_string(),
                            args: vec![],
                            source: ServerSource::VscodeExtension(extension_name.to_string()),
                        });
                    }
                }

                // Fall back to just the command name (assume it's in PATH)
                return Some(DiscoveredServer {
                    language: language.to_string(),
                    command: command.to_string(),
                    args: vec![],
                    source: ServerSource::VscodeExtension(extension_name.to_string()),
                });
            }
        }

        None
    }

    /// Find a language server in system PATH
    fn find_in_path(language: &str) -> Option<DiscoveredServer> {
        for (lang, command, args) in Self::KNOWN_SERVERS {
            if *lang == language && Self::command_exists(command) {
                return Some(DiscoveredServer {
                    language: language.to_string(),
                    command: command.to_string(),
                    args: args.iter().map(|s| s.to_string()).collect(),
                    source: ServerSource::SystemPath(command.to_string()),
                });
            }
        }
        None
    }

    /// Check if a command exists in PATH
    fn command_exists(command: &str) -> bool {
        #[cfg(unix)]
        {
            Command::new("which")
                .arg(command)
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false)
        }

        #[cfg(windows)]
        {
            Command::new("where")
                .arg(command)
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false)
        }
    }

    /// Get a mapping from languages to VS Code extension identifiers
    fn language_to_extension_map() -> &'static HashMap<&'static str, Vec<&'static str>> {
        use std::sync::OnceLock;

        static MAP: OnceLock<HashMap<&'static str, Vec<&'static str>>> = OnceLock::new();

        MAP.get_or_init(|| {
            let mut m = HashMap::new();
            m.insert("rust", vec!["rust-lang", "rust-analyzer"]);
            m.insert("python", vec!["python-lang", "vscode-python"]);
            m.insert("typescript", vec!["typescript-language"]);
            m.insert("javascript", vec!["typescript-language"]);
            m.insert("go", vec!["golang"]);
            m.insert("c", vec!["vscode-clangd"]);
            m.insert("cpp", vec!["vscode-clangd"]);
            m.insert("java", vec!["vscode-java", "redhat"]);
            m.insert("ruby", vec!["vscode-ruby"]);
            m.insert("php", vec!["vscode-php"]);
            m.insert("lua", vec!["vscode-lua"]);
            m.insert("sh", vec!["bash-language-server"]);
            m.insert("json", vec!["vscode-json"]);
            m.insert("yaml", vec!["vscode-yaml"]);
            m.insert("html", vec!["vscode-html"]);
            m.insert("css", vec!["vscode-css"]);
            m.insert("toml", vec!["taplo"]);
            m.insert("markdown", vec!["vscode-markdown"]);
            m
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_discovered_server_serialization() {
        let server = DiscoveredServer {
            language: "rust".to_string(),
            command: "rust-analyzer".to_string(),
            args: vec![],
            source: ServerSource::SystemPath("/usr/bin/rust-analyzer".to_string()),
        };

        let json = serde_json::to_string(&server).unwrap();
        let parsed: DiscoveredServer = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.language, "rust");
        assert_eq!(parsed.command, "rust-analyzer");
    }

    #[test]
    fn test_server_config_creation() {
        use crate::lsp::config::ServerConfig;

        let config = ServerConfig::new("rust-analyzer").with_args(vec!["--stdio".to_string()]);

        assert_eq!(config.command, "rust-analyzer");
        assert_eq!(config.args, vec!["--stdio".to_string()]);
    }
}
