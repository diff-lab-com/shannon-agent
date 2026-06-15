//! Installer trait — type-aware install adapters.
//!
//! P1 ships the trait only. Implementations land in P2 (MCP), P3 (Skills),
//! P4 (Agents), P5 (DataSource). The hub UI calls `install(entry)` and the
//! right adapter runs based on `entry.kind`.

use std::error::Error;
use std::fmt;

use async_trait::async_trait;

use super::types::{
    AddonKind, CatalogEntry, CatalogSource, ConfirmationLevel, InstallTarget, InstalledAddon,
    ProgressSink, TrustLevel,
};

/// Error returned by installer operations.
///
/// `user_facing_message` is shown in the UI as-is. Other variants are
/// translated to a generic error string by the Tauri command layer.
#[derive(Debug)]
pub enum InstallError {
    /// Network fetch failed, registry returned 5xx, etc.
    Network(String),
    /// File system error (clone failed, disk full, permission denied).
    Io(String),
    /// Bundle manifest is malformed, signature mismatch, etc.
    Format(String),
    /// OAuth flow was cancelled or returned an error.
    Auth(String),
    /// User declined the confirmation prompt.
    Cancelled,
    /// Installer doesn't support this entry shape.
    Unsupported(String),
    /// Anything else.
    Other(String),
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstallError::Network(msg) => write!(f, "network error: {msg}"),
            InstallError::Io(msg) => write!(f, "io error: {msg}"),
            InstallError::Format(msg) => write!(f, "format error: {msg}"),
            InstallError::Auth(msg) => write!(f, "auth error: {msg}"),
            InstallError::Cancelled => write!(f, "cancelled by user"),
            InstallError::Unsupported(msg) => write!(f, "unsupported: {msg}"),
            InstallError::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl Error for InstallError {}

impl From<std::io::Error> for InstallError {
    fn from(err: std::io::Error) -> Self {
        InstallError::Io(err.to_string())
    }
}

impl From<serde_json::Error> for InstallError {
    fn from(err: serde_json::Error) -> Self {
        InstallError::Format(err.to_string())
    }
}

impl From<anyhow::Error> for InstallError {
    fn from(err: anyhow::Error) -> Self {
        InstallError::Other(err.to_string())
    }
}

/// Per-kind install adapter. Implementations:
/// - `McpRegistryInstaller` (P2) — dispatches to one of the four MCP installers
/// - `OAuthRemoteMcpInstaller` (P2) — vendor-hosted remote MCP
/// - `McpbInstaller` (P2) — `.mcpb` ZIP bundle
/// - `StdioMcpInstaller` (P2) — Tier 3 escape hatch
/// - `MarketplacePluginInstaller` (P3) — `.claude-plugin/marketplace.json`
/// - `NativeRustInstaller` (P5) — Obsidian vault + Email IMAP
#[async_trait]
pub trait AddonInstaller: Send + Sync {
    /// What this installer handles.
    fn kind(&self) -> super::types::AddonKind;

    /// Whether this installer can handle a specific entry.
    ///
    /// Used when multiple installers exist for the same kind (e.g. four MCP
    /// installers all return `AddonKind::Mcp`). The hub dispatcher picks the
    /// first one whose `supports()` returns true.
    fn supports(&self, entry: &CatalogEntry) -> bool;

    /// Install the entry into the target location.
    async fn install(
        &self,
        entry: &CatalogEntry,
        target: &InstallTarget,
        progress: &ProgressSink,
    ) -> Result<InstalledAddon, InstallError>;

    /// Remove a previously-installed addon.
    async fn uninstall(&self, addon_id: &str) -> Result<(), InstallError>;

    /// Refresh an installed addon from upstream.
    async fn update(&self, addon_id: &str) -> Result<InstalledAddon, InstallError>;

    /// How scary should the confirm prompt be?
    fn requires_confirmation(&self, entry: &CatalogEntry) -> ConfirmationLevel;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_error_display_is_human_readable() {
        let cases = [
            (InstallError::Network("timeout".into()), "network error: timeout"),
            (InstallError::Io("permission denied".into()), "io error: permission denied"),
            (InstallError::Format("bad zip".into()), "format error: bad zip"),
            (InstallError::Auth("revoked".into()), "auth error: revoked"),
            (InstallError::Cancelled, "cancelled by user"),
            (InstallError::Unsupported("no transport".into()), "unsupported: no transport"),
            (InstallError::Other("boom".into()), "boom"),
        ];
        for (err, expected) in cases {
            assert_eq!(err.to_string(), expected);
        }
    }

    #[test]
    fn install_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let install_err: InstallError = io_err.into();
        assert!(matches!(install_err, InstallError::Io(_)));
    }

    #[test]
    fn install_error_from_serde_json() {
        let json_err = serde_json::from_str::<serde_json::Value>("bad").unwrap_err();
        let install_err: InstallError = json_err.into();
        assert!(matches!(install_err, InstallError::Format(_)));
    }

    #[test]
    fn install_error_from_anyhow() {
        let any_err = anyhow::anyhow!("multi\nline\nfailure");
        let install_err: InstallError = any_err.into();
        assert!(matches!(install_err, InstallError::Other(_)));
    }

    /// A no-op installer that the tests can use to verify trait shape.
    struct DummyInstaller;

    #[async_trait]
    impl AddonInstaller for DummyInstaller {
        fn kind(&self) -> AddonKind {
            AddonKind::Skill
        }
        fn supports(&self, _entry: &CatalogEntry) -> bool {
            true
        }
        async fn install(
            &self,
            entry: &CatalogEntry,
            _target: &InstallTarget,
            _progress: &ProgressSink,
        ) -> Result<InstalledAddon, InstallError> {
            Ok(InstalledAddon {
                id: entry.id.clone(),
                kind: entry.kind,
                name: entry.name.clone(),
                install_path: Some("/tmp/dummy".to_string()),
                installed_at: Some(chrono::Utc::now()),
                version: entry.version.clone(),
                enabled: true,
            })
        }
        async fn uninstall(&self, _addon_id: &str) -> Result<(), InstallError> {
            Ok(())
        }
        async fn update(&self, addon_id: &str) -> Result<InstalledAddon, InstallError> {
            Err(InstallError::Unsupported(format!("no update for {addon_id}")))
        }
        fn requires_confirmation(&self, _entry: &CatalogEntry) -> ConfirmationLevel {
            ConfirmationLevel::Review
        }
    }

    #[tokio::test]
    async fn dummy_installer_satisfies_trait() {
        let installer = DummyInstaller;
        let entry = CatalogEntry {
            id: "test:1".to_string(),
            kind: AddonKind::Skill,
            name: "Test Skill".to_string(),
            description: "test".to_string(),
            author: None,
            version: Some("0.1".to_string()),
            homepage_url: None,
            license: None,
            stars: None,
            last_updated: None,
            source: CatalogSource::Native,
            trust: TrustLevel::Unknown,
            metadata: Default::default(),
            tags: vec![],
        };
        assert!(installer.supports(&entry));
        let installed = installer
            .install(&entry, &InstallTarget::ShannonSkillsDir { plugin: "test".into() }, &ProgressSink::null())
            .await
            .expect("install");
        assert_eq!(installed.id, "test:1");
        assert!(installed.enabled);
        installer.uninstall("test:1").await.expect("uninstall");
        assert!(installer.update("test:1").await.is_err());
    }
}
