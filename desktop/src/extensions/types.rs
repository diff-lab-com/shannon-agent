//! Core types for the unified extensions hub.
//!
//! See `docs/architecture/unified-hub.md` for the ADR. These types are the
//! contract between the hub UI, the catalog fetchers (P2+), and the per-type
//! installers (six implementations planned; P1 ships none).

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

/// Top-level extension category. Drives installer selection.
///
/// Order matters for UI display: MCP → Skills → Agents → Data Sources → Plugins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AddonKind {
    /// Remote OAuth endpoint, `.mcpb` bundle, or stdio server.
    Mcp,
    /// SKILL.md file or marketplace plugin of skills.
    Skill,
    /// Claude Code `.claude/agents/*.md` subagent definition.
    Agent,
    /// Tier-1 native Rust integration (Obsidian vault, Email IMAP).
    DataSource,
    /// `.claude-plugin/marketplace.json` repo bundling multiple entries.
    Plugin,
}

impl AddonKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            AddonKind::Mcp => "mcp",
            AddonKind::Skill => "skill",
            AddonKind::Agent => "agent",
            AddonKind::DataSource => "data_source",
            AddonKind::Plugin => "plugin",
        }
    }

    /// Human-friendly label for the UI.
    pub fn label(&self) -> &'static str {
        match self {
            AddonKind::Mcp => "MCP Server",
            AddonKind::Skill => "Skill",
            AddonKind::Agent => "Agent",
            AddonKind::DataSource => "Data Source",
            AddonKind::Plugin => "Plugin",
        }
    }

    pub fn all() -> &'static [AddonKind] {
        &[
            AddonKind::Mcp,
            AddonKind::Skill,
            AddonKind::Agent,
            AddonKind::DataSource,
            AddonKind::Plugin,
        ]
    }
}

/// Where a catalog entry came from. Drives trust signals and refresh cadence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CatalogSource {
    /// Official MCP Registry (`registry.modelcontextprotocol.io`).
    McpRegistry {
        /// e.g. `io.modelcontextprotocol.registry/v0`
        publisher: String,
    },
    /// Shannon-curated featured list, baked into the app.
    FeaturedVendor,
    /// GitHub repo with `.claude-plugin/marketplace.json` or skill collection.
    GitHubRepo {
        /// `owner/repo`, e.g. `anthropics/skills`
        repo: String,
        /// git ref, e.g. `main` or commit SHA
        #[serde(default)]
        ref_: Option<String>,
    },
    /// User-supplied URL (Tier 3 escape hatch).
    Custom { url: String },
    /// Native Rust integration, no upstream catalog.
    Native,
}

/// How trustworthy an entry is. Surfaced as a badge in the UI.
///
/// Order from strongest to weakest: Verified > Official > Community > Unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
    /// Unknown provenance — show review drawer before install.
    Unknown,
    /// Community repo, no Shannon review.
    Community,
    /// Vendor-authored (e.g. `makenotion/notion-mcp-server`).
    Official,
    /// Shannon verified (featured list, curated).
    Verified,
}

impl TrustLevel {
    pub fn label(&self) -> &'static str {
        match self {
            TrustLevel::Unknown => "Unknown",
            TrustLevel::Community => "Community",
            TrustLevel::Official => "Official",
            TrustLevel::Verified => "Verified",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            TrustLevel::Unknown => "help",
            TrustLevel::Community => "group",
            TrustLevel::Official => "verified_user",
            TrustLevel::Verified => "verified",
        }
    }
}

/// One row in the hub catalog. Unified across all addon kinds.
///
/// `kind` determines which installer runs. Everything else is UI metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogEntry {
    /// Stable identifier. For GitHub sources: `gh:owner/repo/path`.
    /// For MCP Registry: the registry's id. For native: `native:<slug>`.
    pub id: String,
    pub kind: AddonKind,
    pub name: String,
    pub description: String,

    /// Display author / publisher.
    #[serde(default)]
    pub author: Option<String>,

    /// Semantic version string, or git SHA, or "native".
    #[serde(default)]
    pub version: Option<String>,

    /// Source URL for browsing (GitHub page, vendor docs, etc.).
    #[serde(default)]
    pub homepage_url: Option<String>,

    /// SPDX identifier ("Apache-2.0", "MIT") or "unknown" / "source-available".
    #[serde(default)]
    pub license: Option<String>,

    /// Weak signal only — do not over-weight in sort.
    #[serde(default)]
    pub stars: Option<u64>,

    /// Last upstream activity, for "is this maintained?" signal.
    #[serde(default)]
    pub last_updated: Option<DateTime<Utc>>,

    /// Provenance.
    pub source: CatalogSource,

    /// Overall trust level derived from source + signals.
    pub trust: TrustLevel,

    /// Free-form metadata bag for kind-specific fields.
    ///
    /// Examples:
    /// - MCP: `{ "transport": "oauth_remote", "endpoint": "https://mcp.notion.com/mcp" }`
    /// - Skill: `{ "trigger": "/deploy", "allowed_tools": ["bash"] }`
    /// - Agent: `{ "tools": ["bash", "read"], "model": "claude-sonnet-4-6" }`
    /// - DataSource: `{ "kind": "obsidian" | "email_imap" }`
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,

    /// Tags for filtering ("github", "notion", "remote", "stdio", etc.).
    #[serde(default)]
    pub tags: Vec<String>,
}

/// An entry that has been installed into Shannon's local config.
///
/// This is what the Installed tab renders. It's built from the aggregator
/// reading existing local files, not from a remote fetch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledAddon {
    /// Matches `CatalogEntry::id` if it came from the catalog, otherwise a
    /// synthetic id for manually-added entries (e.g. user's pre-existing
    /// `add_mcp_server` config).
    pub id: String,
    pub kind: AddonKind,
    pub name: String,

    /// Where it lives locally.
    ///
    /// - MCP: `~/.shannon/settings.json#mcpServers.<name>`
    /// - Skill: `~/.shannon/skills/<name>/SKILL.md`
    /// - Agent: `~/.shannon/agents/<name>.md`
    /// - DataSource: `keychain:obsidian-vault` or `keychain:email-imap`
    #[serde(default)]
    pub install_path: Option<String>,

    /// When the entry was installed (best-effort, from file mtime if unknown).
    #[serde(default)]
    pub installed_at: Option<DateTime<Utc>>,

    /// Entry's version, if known.
    #[serde(default)]
    pub version: Option<String>,

    /// Whether Shannon considers the entry currently enabled / active.
    ///
    /// For MCP: server is in the config and `enabled: true`. For Skills/Agents:
    /// file exists in the watched directory. For DataSource: tool is turned on
    /// and has credentials.
    pub enabled: bool,
}

/// Where to install an entry. Per-kind.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InstallTarget {
    /// `~/.shannon/settings.json#mcpServers`
    ShannonMcpConfig,
    /// `~/.shannon/skills/<plugin>/<skill>/`
    ShannonSkillsDir { plugin: String },
    /// `~/.shannon/agents/<plugin>/`
    ShannonAgentsDir { plugin: String },
    /// OS keychain slot.
    Keychain { slot: String },
}

/// How scary the confirmation prompt should be before install.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationLevel {
    /// Silent install — featured vendor with verified trust.
    None,
    /// Single click with review drawer available.
    Review,
    /// Must type entry name to confirm (unsigned `.mcpb`, Tier 3 stdio).
    TypeToConfirm,
}

/// Event the installer emits during long-running installs.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProgressEvent {
    Started {
        total_steps: Option<u32>,
    },
    Step {
        description: String,
        current: Option<u32>,
        total: Option<u32>,
    },
    Log {
        message: String,
    },
    Finished,
    Failed {
        error: String,
    },
}

/// Sink installer writes progress events into. Front-end reads via event stream.
#[derive(Debug, Clone)]
pub struct ProgressSink {
    inner: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<ProgressEvent>>>>,
}

impl ProgressSink {
    /// No-op sink — used when the caller doesn't care about progress.
    pub fn null() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
        }
    }

    /// Channel-backed sink — each emit goes to the receiver.
    pub fn channel() -> (Self, tokio::sync::mpsc::UnboundedReceiver<ProgressEvent>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (
            Self {
                inner: Arc::new(Mutex::new(Some(tx))),
            },
            rx,
        )
    }

    pub async fn emit(&self, event: ProgressEvent) {
        let guard = self.inner.lock().await;
        if let Some(tx) = guard.as_ref() {
            let _ = tx.send(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addon_kind_round_trip() {
        for kind in AddonKind::all() {
            let json = serde_json::to_string(kind).expect("serialize");
            let back: AddonKind = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(kind, &back, "kind {} should round-trip", kind.as_str());
        }
    }

    #[test]
    fn trust_level_ordering_is_sane() {
        assert!(TrustLevel::Verified > TrustLevel::Official);
        assert!(TrustLevel::Official > TrustLevel::Community);
        assert!(TrustLevel::Community > TrustLevel::Unknown);
    }

    #[test]
    fn catalog_entry_with_metadata_serializes() {
        let mut meta = HashMap::new();
        meta.insert(
            "endpoint".to_string(),
            serde_json::json!("https://mcp.notion.com/mcp"),
        );
        let entry = CatalogEntry {
            id: "gh:makenotion/notion-mcp-server".to_string(),
            kind: AddonKind::Mcp,
            name: "Notion".to_string(),
            description: "Notion official MCP".to_string(),
            author: Some("Notion Labs".to_string()),
            version: Some("1.0.0".to_string()),
            homepage_url: Some("https://mcp.notion.com".to_string()),
            license: Some("MIT".to_string()),
            stars: Some(4100),
            last_updated: None,
            source: CatalogSource::FeaturedVendor,
            trust: TrustLevel::Verified,
            metadata: meta,
            tags: vec!["notion".into(), "remote".into()],
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        assert!(json.contains("notion"));
        let back: CatalogEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(entry.id, back.id);
    }

    #[test]
    fn progress_sink_null_is_silent() {
        let sink = ProgressSink::null();
        let rt = tokio::runtime::Runtime::new().expect("rt");
        rt.block_on(sink.emit(ProgressEvent::Finished));
        // No panic, no receiver — pass.
    }

    #[test]
    fn progress_sink_channel_delivers_events() {
        let (sink, mut rx) = ProgressSink::channel();
        let rt = tokio::runtime::Runtime::new().expect("rt");
        rt.block_on(sink.emit(ProgressEvent::Started {
            total_steps: Some(3),
        }));
        rt.block_on(sink.emit(ProgressEvent::Finished));
        let first = rx.blocking_recv().expect("event 1");
        let second = rx.blocking_recv().expect("event 2");
        assert!(matches!(
            first,
            ProgressEvent::Started {
                total_steps: Some(3)
            }
        ));
        assert!(matches!(second, ProgressEvent::Finished));
    }

    #[test]
    fn catalog_source_github_repo_round_trip() {
        let src = CatalogSource::GitHubRepo {
            repo: "anthropics/skills".to_string(),
            ref_: Some("main".to_string()),
        };
        let json = serde_json::to_string(&src).expect("serialize");
        let back: CatalogSource = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(src, back);
    }

    #[test]
    fn install_target_keychain_tag_works() {
        let t = InstallTarget::Keychain {
            slot: "obsidian-vault".to_string(),
        };
        let json = serde_json::to_string(&t).expect("serialize");
        assert!(json.contains("keychain"));
        assert!(json.contains("obsidian-vault"));
    }
}
