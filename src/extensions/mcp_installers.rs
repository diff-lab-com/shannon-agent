//! MCP installer implementations — four adapters behind one trait dispatch.
//!
//! P2 covers the four MCP install shapes Shannon supports:
//!
//! | Installer              | Source          | Confirmation      |
//! |------------------------|-----------------|-------------------|
//! | `OAuthRemoteMcpInstaller` | featured vendor / registry OAuth | Review     |
//! | `McpbInstaller`        | `.mcpb` ZIP     | Review (signed) / TypeToConfirm (unsigned) |
//! | `StdioMcpInstaller`    | raw command spec from user input | TypeToConfirm |
//! | `McpRegistryInstaller` | dispatches to one of the above based on `CatalogEntry.metadata` | inherits |
//!
//! All four write to `~/.shannon/settings.json#mcpServers.<name>` via the
//! shared `write_mcp_server_config` helper. The config blob follows the
//! Claude Code / Shannon MCP server schema:
//!
//! ```json
//! {
//!   "command": "npx",
//!   "args": ["-y", "@notionhq/notion-mcp-server"],
//!   "env": { "NOTION_TOKEN": "..." },
//!   "enabled": true,
//!   "shannon:transport": "stdio" | "oauth_remote" | "sse" | "http",
//!   "shannon:installed_from": "extensions-hub",
//!   "shannon:installed_at": "2026-06-15T12:00:00Z"
//! }
//! ```

use std::collections::HashMap;
use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::catalog::{FeaturedInstallKind, FeaturedVendor, McpRegistryClient};
use super::installer::{AddonInstaller, InstallError};
use super::mcpb::extract_mcpb;
use super::oauth::PkceContext;
use super::types::{
    AddonKind, CatalogEntry, ConfirmationLevel, InstallTarget, InstalledAddon,
    ProgressEvent, ProgressSink, TrustLevel,
};

#[cfg(test)]
use super::types::CatalogSource;

/// Write an MCP server entry into `~/.shannon/settings.json#mcpServers.<name>`.
///
/// Creates the file if missing, preserves other keys, and marks the entry
/// with Shannon metadata so the aggregator can identify hub-installed servers.
pub fn write_mcp_server_config(name: &str, config: Value) -> Result<PathBuf, InstallError> {
    let path = user_settings_path()?;
    let mut root: Value = if path.exists() {
        let text = std::fs::read_to_string(&path).map_err(|e| InstallError::Io(e.to_string()))?;
        serde_json::from_str(&text).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    if root.get("mcpServers").is_none() {
        root["mcpServers"] = json!({});
    }
    root["mcpServers"]
        .as_object_mut()
        .ok_or_else(|| InstallError::Format("mcpServers is not an object".into()))?
        .insert(name.to_string(), config);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| InstallError::Io(e.to_string()))?;
    }
    let bytes = serde_json::to_vec_pretty(&root)?;
    std::fs::write(&path, bytes).map_err(|e| InstallError::Io(e.to_string()))?;
    Ok(path)
}

/// Remove an MCP server entry. Returns Ok(()) if the entry didn't exist.
pub fn remove_mcp_server_config(name: &str) -> Result<(), InstallError> {
    let path = user_settings_path()?;
    if !path.exists() {
        return Ok(());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| InstallError::Io(e.to_string()))?;
    let mut root: Value = serde_json::from_str(&text)
        .map_err(|e| InstallError::Format(format!("settings.json parse: {e}")))?;
    let removed = root
        .get_mut("mcpServers")
        .and_then(|s| s.as_object_mut())
        .and_then(|s| s.remove(name))
        .is_some();
    if removed {
        let bytes = serde_json::to_vec_pretty(&root)?;
        std::fs::write(&path, bytes).map_err(|e| InstallError::Io(e.to_string()))?;
    }
    Ok(())
}

fn user_settings_path() -> Result<PathBuf, InstallError> {
    let home = dirs::home_dir()
        .ok_or_else(|| InstallError::Io("cannot resolve $HOME".into()))?;
    Ok(home.join(".shannon/settings.json"))
}

/// Decorate a server config with Shannon metadata so the aggregator can
/// tell hub-installed servers apart from user-manual ones.
fn annotate_config(mut config: Value, transport: &str) -> Value {
    if config.get("enabled").is_none() {
        config["enabled"] = json!(true);
    }
    config["shannon:transport"] = json!(transport);
    config["shannon:installed_from"] = json!("extensions-hub");
    config["shannon:installed_at"] = json!(chrono::Utc::now().to_rfc3339());
    config
}

// ---------------------------------------------------------------------------
// Stdio installer (Tier 3 escape hatch)
// ---------------------------------------------------------------------------

/// Tier-3 stdio installer: writes a user-supplied command/args/env spec
/// directly to settings.json. Used for "Add Manually" form and for registry
/// servers whose upstream is a raw npx invocation.
pub struct StdioMcpInstaller {
    /// Caller-supplied spec (typically built from a form, not from catalog).
    pub spec: StdioMcpSpec,
}

#[derive(Debug, Clone, Default)]
pub struct StdioMcpSpec {
    pub server_name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
}

#[async_trait]
impl AddonInstaller for StdioMcpInstaller {
    fn kind(&self) -> AddonKind {
        AddonKind::Mcp
    }
    fn supports(&self, entry: &CatalogEntry) -> bool {
        entry.metadata.get("transport").and_then(|v| v.as_str()) == Some("stdio")
    }
    async fn install(
        &self,
        entry: &CatalogEntry,
        _target: &InstallTarget,
        progress: &ProgressSink,
    ) -> Result<InstalledAddon, InstallError> {
        progress.emit(ProgressEvent::Started { total_steps: Some(2) }).await;
        progress
            .emit(ProgressEvent::Step {
                description: format!("Writing stdio MCP server {}", self.spec.server_name),
                current: Some(1),
                total: Some(2),
            })
            .await;

        let config = annotate_config(
            json!({
                "command": self.spec.command,
                "args": self.spec.args,
                "env": self.spec.env,
            }),
            "stdio",
        );
        let path = write_mcp_server_config(&self.spec.server_name, config)?;

        progress.emit(ProgressEvent::Finished).await;
        Ok(InstalledAddon {
            id: entry.id.clone(),
            kind: AddonKind::Mcp,
            name: self.spec.server_name.clone(),
            install_path: Some(format!("{}#mcpServers.{}", path.display(), self.spec.server_name)),
            installed_at: Some(chrono::Utc::now()),
            version: entry.version.clone(),
            enabled: true,
        })
    }
    async fn uninstall(&self, addon_id: &str) -> Result<(), InstallError> {
        remove_mcp_server_config(addon_id)
    }
    async fn update(&self, _addon_id: &str) -> Result<InstalledAddon, InstallError> {
        Err(InstallError::Unsupported(
            "stdio MCP update requires manual edit".into(),
        ))
    }
    fn requires_confirmation(&self, _entry: &CatalogEntry) -> ConfirmationLevel {
        ConfirmationLevel::TypeToConfirm
    }
}

// ---------------------------------------------------------------------------
// OAuth remote installer (Tier 2)
// ---------------------------------------------------------------------------

/// Tier-2 OAuth 2.1 PKCE installer: drives the loopback flow against a
/// vendor-hosted remote MCP server. Result is a `url`-only `mcpServers` entry.
///
/// P2 ships the structural pieces (URL builder, token exchange shape). The
/// actual loopback listener + browser launch live behind Tauri commands that
/// can only run in the desktop binary; the unit-testable parts are here.
pub struct OAuthRemoteMcpInstaller {
    pub vendor: FeaturedVendor,
}

impl OAuthRemoteMcpInstaller {
    /// Build a fresh PKCE context for this vendor's authorize URL.
    pub fn pkce_context(&self) -> PkceContext {
        PkceContext::new()
    }

    /// Render the authorize URL the user's browser should visit.
    pub fn authorize_url(&self, pkce: &PkceContext, redirect_uri: &str) -> Result<String, InstallError> {
        let FeaturedInstallKind::OAuthRemote {
            authorize_url,
            client_id_env,
            default_scopes,
            ..
        } = &self.vendor.install_kind
        else {
            return Err(InstallError::Unsupported(format!(
                "vendor {} is not OAuth",
                self.vendor.slug
            )));
        };
        let client_id = std::env::var(client_id_env).unwrap_or_else(|_| "shannon-desktop".into());
        super::oauth::build_authorize_url(
            authorize_url,
            &client_id,
            redirect_uri,
            &pkce.challenge,
            &pkce.state,
            default_scopes,
        )
        .map_err(|e| InstallError::Other(format!("url parse: {e}")))
    }

    /// Render the final `mcpServers` config entry from a successful token.
    pub fn server_config(&self, access_token: &str) -> Value {
        let FeaturedInstallKind::OAuthRemote { mcp_endpoint, .. } = &self.vendor.install_kind else {
            return json!({});
        };
        annotate_config(
            json!({
                "type": "http",
                "url": mcp_endpoint,
                "headers": { "Authorization": format!("Bearer {access_token}") }
            }),
            "oauth_remote",
        )
    }
}

#[async_trait]
impl AddonInstaller for OAuthRemoteMcpInstaller {
    fn kind(&self) -> AddonKind {
        AddonKind::Mcp
    }
    fn supports(&self, entry: &CatalogEntry) -> bool {
        entry.metadata.get("transport").and_then(|v| v.as_str()) == Some("oauth_remote")
    }
    async fn install(
        &self,
        _entry: &CatalogEntry,
        _target: &InstallTarget,
        progress: &ProgressSink,
    ) -> Result<InstalledAddon, InstallError> {
        // The full OAuth flow (loopback listener, browser launch, token
        // exchange) is wired at the Tauri command layer — it needs the
        // desktop process. Here we just record progress steps so the UI
        // shows them.
        progress.emit(ProgressEvent::Started { total_steps: Some(4) }).await;
        progress
            .emit(ProgressEvent::Step {
                description: "Opening browser for OAuth consent".into(),
                current: Some(1),
                total: Some(4),
            })
            .await;
        progress
            .emit(ProgressEvent::Step {
                description: "Waiting for vendor callback…".into(),
                current: Some(2),
                total: Some(4),
            })
            .await;
        // Tauri command layer injects the real token via run_oauth_flow().
        Err(InstallError::Unsupported(
            "OAuth flow must be driven via run_oauth_install command".into(),
        ))
    }
    async fn uninstall(&self, addon_id: &str) -> Result<(), InstallError> {
        remove_mcp_server_config(addon_id)
    }
    async fn update(&self, addon_id: &str) -> Result<InstalledAddon, InstallError> {
        Err(InstallError::Unsupported(format!(
            "OAuth MCP {addon_id} updates are tied to token refresh"
        )))
    }
    fn requires_confirmation(&self, entry: &CatalogEntry) -> ConfirmationLevel {
        if entry.trust >= TrustLevel::Verified {
            ConfirmationLevel::None
        } else {
            ConfirmationLevel::Review
        }
    }
}

// ---------------------------------------------------------------------------
// .mcpb installer (Tier 2)
// ---------------------------------------------------------------------------

/// `.mcpb` installer: download (or accept uploaded) ZIP, extract to
/// `~/.shannon/mcp-servers/<name>/`, write `mcpServers` entry pointing at
/// the extracted command.
pub struct McpbInstaller {
    /// Pre-fetched archive bytes. The Tauri command layer downloads.
    pub archive_bytes: Vec<u8>,
    /// Where to extract. Defaults to `~/.shannon/mcp-servers/` if None.
    pub extract_root: Option<PathBuf>,
}

#[async_trait]
impl AddonInstaller for McpbInstaller {
    fn kind(&self) -> AddonKind {
        AddonKind::Mcp
    }
    fn supports(&self, entry: &CatalogEntry) -> bool {
        entry.metadata.get("transport").and_then(|v| v.as_str()) == Some("mcpb")
    }
    async fn install(
        &self,
        entry: &CatalogEntry,
        _target: &InstallTarget,
        progress: &ProgressSink,
    ) -> Result<InstalledAddon, InstallError> {
        progress.emit(ProgressEvent::Started { total_steps: Some(3) }).await;
        progress
            .emit(ProgressEvent::Step {
                description: "Verifying archive".into(),
                current: Some(1),
                total: Some(3),
            })
            .await;

        let root = match &self.extract_root {
            Some(p) => p.clone(),
            None => {
                let home = dirs::home_dir()
                    .ok_or_else(|| InstallError::Io("cannot resolve $HOME".into()))?;
                home.join(".shannon/mcp-servers")
            }
        };

        progress
            .emit(ProgressEvent::Step {
                description: "Extracting bundle".into(),
                current: Some(2),
                total: Some(3),
            })
            .await;
        let (manifest, server_root) = extract_mcpb(&self.archive_bytes, &root)?;

        progress
            .emit(ProgressEvent::Step {
                description: "Registering MCP server".into(),
                current: Some(3),
                total: Some(3),
            })
            .await;

        let config = if manifest.server.server_type == "stdio" {
            annotate_config(
                json!({
                    "command": manifest.server.command,
                    "args": manifest.server.args,
                    "env": manifest.server.env,
                }),
                "stdio",
            )
        } else if let Some(url) = manifest.server.url.as_deref() {
            annotate_config(
                json!({
                    "type": manifest.server.server_type,
                    "url": url,
                }),
                &manifest.server.server_type,
            )
        } else {
            return Err(InstallError::Format(format!(
                "manifest server type {} missing url",
                manifest.server.server_type
            )));
        };
        let path = write_mcp_server_config(&manifest.name, config)?;
        progress.emit(ProgressEvent::Finished).await;

        Ok(InstalledAddon {
            id: entry.id.clone(),
            kind: AddonKind::Mcp,
            name: manifest.name.clone(),
            install_path: Some(format!(
                "{}#mcpServers.{} (bundle at {})",
                path.display(),
                manifest.name,
                server_root.display()
            )),
            installed_at: Some(chrono::Utc::now()),
            version: manifest.version,
            enabled: true,
        })
    }
    async fn uninstall(&self, addon_id: &str) -> Result<(), InstallError> {
        remove_mcp_server_config(addon_id)
    }
    async fn update(&self, _addon_id: &str) -> Result<InstalledAddon, InstallError> {
        Err(InstallError::Unsupported(
            ".mcpb updates require re-downloading the bundle".into(),
        ))
    }
    fn requires_confirmation(&self, entry: &CatalogEntry) -> ConfirmationLevel {
        // Unsigned bundles always get the scary prompt.
        match entry.trust {
            TrustLevel::Verified => ConfirmationLevel::Review,
            _ => ConfirmationLevel::TypeToConfirm,
        }
    }
}

// ---------------------------------------------------------------------------
// Registry dispatcher
// ---------------------------------------------------------------------------

/// MCP Registry dispatcher: takes a registry `CatalogEntry` and routes to
/// the right concrete installer based on `entry.metadata["transport"]`.
///
/// For P2 we resolve transport upfront and the dispatcher returns a boxed
/// concrete installer. The UI never sees the inner installer shape.
pub enum ResolvedMcpInstaller {
    Stdio(StdioMcpInstaller),
    Oauth(OAuthRemoteMcpInstaller),
    Mcpb(McpbInstaller),
}

/// Resolve which installer should handle a registry entry.
///
/// Returns `None` for entries Shannon can't yet install (e.g. unknown transport).
pub fn resolve_registry_installer(
    entry: &CatalogEntry,
    registry: &McpRegistryClient,
    featured: &[FeaturedVendor],
) -> Option<ResolvedMcpInstaller> {
    let transport = entry.metadata.get("transport").and_then(|v| v.as_str())?;
    match transport {
        "stdio" => {
            let command = entry
                .metadata
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("npx")
                .to_string();
            let args: Vec<String> = entry
                .metadata
                .get("args")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let env: HashMap<String, String> = entry
                .metadata
                .get("env")
                .and_then(|v| v.as_object())
                .map(|o| {
                    o.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect()
                })
                .unwrap_or_default();
            Some(ResolvedMcpInstaller::Stdio(StdioMcpInstaller {
                spec: StdioMcpSpec {
                    server_name: entry.name.clone(),
                    command,
                    args,
                    env,
                },
            }))
        }
        "oauth_remote" => {
            // Match the registry entry to a featured vendor by slug/id.
            let vendor = featured.iter().find(|v| {
                entry.id.ends_with(&v.slug) || entry.metadata.get("vendor").and_then(|v| v.as_str()) == Some(&v.slug)
            })?;
            // Note: `registry` is currently unused for resolution — the
            // transport metadata is on the entry itself. The param is kept
            // so future versions can re-fetch richer server metadata.
            let _ = registry;
            Some(ResolvedMcpInstaller::Oauth(OAuthRemoteMcpInstaller { vendor: vendor.clone() }))
        }
        "mcpb" => {
            // The actual archive download happens in the Tauri command layer;
            // the dispatcher just signals that an McpbInstaller is needed.
            // Caller fills `archive_bytes` before invoking install().
            Some(ResolvedMcpInstaller::Mcpb(McpbInstaller {
                archive_bytes: Vec::new(),
                extract_root: None,
            }))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};
    use tempfile::tempdir;

    /// All tests that mutate `$HOME` must hold this lock so they don't race
    /// when nextest/cargo runs them in parallel.
    static HOME_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn home_lock() -> &'static Mutex<()> {
        HOME_LOCK.get_or_init(|| Mutex::new(()))
    }

    fn settings_path_in(dir: &std::path::Path) -> PathBuf {
        dir.join(".shannon/settings.json")
    }

    /// Override `dirs::home_dir` indirectly: we can't, but we can write
    /// directly via the public API once we know the path. For unit tests we
    /// exercise the underlying JSON manipulation by serializing in-memory.
    #[test]
    fn annotate_config_adds_shannon_metadata() {
        let cfg = annotate_config(json!({"command": "npx"}), "stdio");
        assert_eq!(cfg["enabled"], json!(true));
        assert_eq!(cfg["shannon:transport"], json!("stdio"));
        assert_eq!(cfg["shannon:installed_from"], json!("extensions-hub"));
        assert!(cfg["shannon:installed_at"].as_str().unwrap().contains("T"));
    }

    #[test]
    fn write_mcp_server_creates_file_if_missing() {
        // Redirect $HOME via env var so dirs::home_dir uses it.
        let dir = tempdir().unwrap();
        let _home_guard = home_lock().lock().unwrap();
        unsafe { std::env::set_var("HOME", dir.path()); }
        // dirs::home_dir on Linux reads $HOME.
        let path = user_settings_path().unwrap();
        assert!(!path.exists());

        let result = write_mcp_server_config(
            "notion",
            json!({"command": "npx", "args": ["-y", "@notionhq/notion-mcp-server"]}),
        );
        assert!(result.is_ok(), "write failed: {:?}", result.err());
        assert!(path.exists());

        let text = std::fs::read_to_string(&path).unwrap();
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["mcpServers"]["notion"]["command"], json!("npx"));
    }

    #[test]
    fn write_mcp_server_preserves_other_keys() {
        let dir = tempdir().unwrap();
        let _home_guard = home_lock().lock().unwrap();
        unsafe { std::env::set_var("HOME", dir.path()); }
        let path = user_settings_path().unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"otherKey": 42, "mcpServers": {"existing": {"command": "x"}}}"#,
        )
        .unwrap();

        write_mcp_server_config("new", json!({"command": "y"})).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["otherKey"], json!(42));
        assert_eq!(parsed["mcpServers"]["existing"]["command"], json!("x"));
        assert_eq!(parsed["mcpServers"]["new"]["command"], json!("y"));
    }

    #[test]
    fn remove_mcp_server_drops_only_target() {
        let dir = tempdir().unwrap();
        let _home_guard = home_lock().lock().unwrap();
        unsafe { std::env::set_var("HOME", dir.path()); }
        let path = user_settings_path().unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"mcpServers": {"a": {"command": "x"}, "b": {"command": "y"}}}"#,
        )
        .unwrap();

        remove_mcp_server_config("a").unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert!(parsed["mcpServers"]["a"].is_null());
        assert!(parsed["mcpServers"]["b"].is_object());
    }

    #[test]
    fn remove_mcp_server_handles_missing_file() {
        let dir = tempdir().unwrap();
        let _home_guard = home_lock().lock().unwrap();
        unsafe { std::env::set_var("HOME", dir.path()); }
        // No file exists.
        assert!(remove_mcp_server_config("anything").is_ok());
    }

    fn stdio_entry() -> CatalogEntry {
        CatalogEntry {
            id: "test:stdio".into(),
            kind: AddonKind::Mcp,
            name: "stdio-test".into(),
            description: "test".into(),
            author: None,
            version: Some("0.1".into()),
            homepage_url: None,
            license: None,
            stars: None,
            last_updated: None,
            source: CatalogSource::Native,
            trust: TrustLevel::Unknown,
            metadata: {
                let mut m = HashMap::new();
                m.insert("transport".to_string(), json!("stdio"));
                m.insert("command".to_string(), json!("node"));
                m.insert("args".to_string(), json!(["index.js"]));
                m
            },
            tags: vec![],
        }
    }

    #[tokio::test]
    async fn stdio_installer_writes_settings() {
        let dir = tempdir().unwrap();
        let _home_guard = home_lock().lock().unwrap();
        unsafe { std::env::set_var("HOME", dir.path()); }

        let installer = StdioMcpInstaller {
            spec: StdioMcpSpec {
                server_name: "my-stdio".into(),
                command: "node".into(),
                args: vec!["index.js".into()],
                env: HashMap::new(),
            },
        };
        let entry = stdio_entry();
        let installed = installer
            .install(&entry, &InstallTarget::ShannonMcpConfig, &ProgressSink::null())
            .await
            .expect("install");
        assert_eq!(installed.name, "my-stdio");
        assert!(installed.enabled);

        // Verify the config actually landed.
        let path = user_settings_path().unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["mcpServers"]["my-stdio"]["command"], json!("node"));
        assert_eq!(
            parsed["mcpServers"]["my-stdio"]["shannon:transport"],
            json!("stdio")
        );
    }

    #[test]
    fn stdio_installer_always_type_to_confirm() {
        let installer = StdioMcpInstaller {
            spec: StdioMcpSpec::default(),
        };
        let entry = stdio_entry();
        assert_eq!(
            installer.requires_confirmation(&entry),
            ConfirmationLevel::TypeToConfirm
        );
    }

    fn oauth_vendor() -> FeaturedVendor {
        use super::super::catalog::{FeaturedCategory, FeaturedInstallKind, FeaturedVendor};
        FeaturedVendor {
            slug: "test-vendor".into(),
            display_name: "Test Vendor".into(),
            description: "test".into(),
            icon: "test".into(),
            category: FeaturedCategory::Productivity,
            trust: TrustLevel::Verified,
            install_kind: FeaturedInstallKind::OAuthRemote {
                authorize_url: "https://example.com/oauth/authorize".into(),
                token_url: "https://example.com/oauth/token".into(),
                mcp_endpoint: "https://mcp.example.com/mcp".into(),
                client_id_env: "TEST_CLIENT_ID".into(),
                default_scopes: vec!["read".into()],
                display_name: "Connect Test".into(),
            },
            homepage_url: "https://example.com".into(),
        }
    }

    #[test]
    fn oauth_installer_builds_authorize_url() {
        let vendor = oauth_vendor();
        let installer = OAuthRemoteMcpInstaller { vendor };
        let pkce = installer.pkce_context();
        let url = installer.authorize_url(&pkce, "http://localhost:1738/callback").unwrap();
        assert!(url.contains("response_type=code"));
        assert!(url.contains("code_challenge="));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state="));
    }

    #[test]
    fn oauth_server_config_includes_bearer_header() {
        let vendor = oauth_vendor();
        let installer = OAuthRemoteMcpInstaller { vendor };
        let cfg = installer.server_config("abc123");
        assert_eq!(cfg["type"], json!("http"));
        assert_eq!(cfg["url"], json!("https://mcp.example.com/mcp"));
        assert_eq!(
            cfg["headers"]["Authorization"],
            json!("Bearer abc123")
        );
        assert_eq!(cfg["shannon:transport"], json!("oauth_remote"));
    }

    #[test]
    fn oauth_installer_no_confirm_for_verified() {
        let vendor = oauth_vendor();
        let installer = OAuthRemoteMcpInstaller { vendor };
        let entry = CatalogEntry {
            id: "test".into(),
            kind: AddonKind::Mcp,
            name: "x".into(),
            description: String::new(),
            author: None,
            version: None,
            homepage_url: None,
            license: None,
            stars: None,
            last_updated: None,
            source: CatalogSource::FeaturedVendor,
            trust: TrustLevel::Verified,
            metadata: HashMap::new(),
            tags: vec![],
        };
        assert_eq!(
            installer.requires_confirmation(&entry),
            ConfirmationLevel::None
        );
    }

    fn make_minimal_mcpb(name: &str) -> Vec<u8> {
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        use zip::ZipWriter;
        let buf = std::io::Cursor::new(Vec::new());
        let mut zw = ZipWriter::new(buf);
        let opts = SimpleFileOptions::default();
        zw.start_file("manifest.json", opts).unwrap();
        write!(
            zw,
            r#"{{"manifest_version":"0.1","name":"{name}","version":"1.0.0","server":{{"type":"stdio","command":"node","args":["index.js"]}}}}"#
        )
        .unwrap();
        zw.finish().unwrap().into_inner()
    }

    #[tokio::test]
    async fn mcpb_installer_extracts_and_registers() {
        let dir = tempdir().unwrap();
        let _home_guard = home_lock().lock().unwrap();
        unsafe { std::env::set_var("HOME", dir.path()); }

        let bytes = make_minimal_mcpb("bundled-server");
        let installer = McpbInstaller {
            archive_bytes: bytes,
            extract_root: Some(dir.path().join("mcp-servers")),
        };
        let entry = CatalogEntry {
            id: "test:mcpb".into(),
            kind: AddonKind::Mcp,
            name: "bundled-server".into(),
            description: "test".into(),
            author: None,
            version: Some("1.0".into()),
            homepage_url: None,
            license: None,
            stars: None,
            last_updated: None,
            source: CatalogSource::Native,
            trust: TrustLevel::Community,
            metadata: {
                let mut m = HashMap::new();
                m.insert("transport".to_string(), json!("mcpb"));
                m
            },
            tags: vec![],
        };
        let installed = installer
            .install(&entry, &InstallTarget::ShannonMcpConfig, &ProgressSink::null())
            .await
            .expect("install");
        assert_eq!(installed.name, "bundled-server");

        // Extracted bundle exists.
        assert!(dir.path().join("mcp-servers/bundled-server/manifest.json").exists());

        // settings.json has the entry.
        let path = user_settings_path().unwrap();
        let parsed: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            parsed["mcpServers"]["bundled-server"]["command"],
            json!("node")
        );
    }

    #[test]
    fn resolve_registry_installer_routes_stdio() {
        let entry = stdio_entry();
        let registry = McpRegistryClient::new(std::sync::Arc::new(super::super::catalog::StaticFetch("{}".to_string())));
        let resolved = resolve_registry_installer(&entry, &registry, &[]);
        assert!(matches!(resolved, Some(ResolvedMcpInstaller::Stdio(_))));
    }

    #[test]
    fn resolve_registry_installer_returns_none_for_unknown_transport() {
        let mut entry = stdio_entry();
        entry.metadata.insert(
            "transport".to_string(),
            json!("telepathy"),
        );
        let registry = McpRegistryClient::new(std::sync::Arc::new(super::super::catalog::StaticFetch("{}".to_string())));
        let resolved = resolve_registry_installer(&entry, &registry, &[]);
        assert!(resolved.is_none());
    }

    #[test]
    fn resolve_registry_installer_routes_mcpb() {
        let mut entry = stdio_entry();
        entry.metadata.insert("transport".to_string(), json!("mcpb"));
        let registry = McpRegistryClient::new(std::sync::Arc::new(super::super::catalog::StaticFetch("{}".to_string())));
        let resolved = resolve_registry_installer(&entry, &registry, &[]);
        assert!(matches!(resolved, Some(ResolvedMcpInstaller::Mcpb(_))));
    }
}
