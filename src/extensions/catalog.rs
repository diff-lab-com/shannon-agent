//! Catalog sources for the unified extensions hub.
//!
//! Two sources feed the MCP Servers tab:
//! - `featured_vendors()` — Shannon-curated list baked into the binary. The
//!   5 vendors the ADR calls out: Notion, Linear, Slack, GitHub, Gmail.
//! - `McpRegistryClient` — fetches from `registry.modelcontextprotocol.io/v0/servers`,
//!   with a 24h local cache to avoid hammering the registry on every hub open.
//!
//! The fetcher is HTTP-client-pluggable so tests don't need network.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::installer::InstallError;
use super::types::{AddonKind, CatalogEntry, CatalogSource, TrustLevel};

/// Featured vendor seed data — the curated "store front page" view.
///
/// Each entry maps to a real OAuth-eligible vendor (ADR §P2 calls out these
/// five). The OAuth client IDs here are placeholders pending vendor
/// registration; users with their own client_id can override via env vars.
pub fn featured_vendors() -> Vec<FeaturedVendor> {
    vec![
        FeaturedVendor {
            slug: "notion".into(),
            display_name: "Notion".into(),
            description: "Read and edit Notion pages, databases, and comments.".into(),
            icon: "notion".into(),
            category: FeaturedCategory::Productivity,
            trust: TrustLevel::Verified,
            install_kind: FeaturedInstallKind::OAuthRemote {
                authorize_url: "https://api.notion.com/v1/oauth/authorize".into(),
                token_url: "https://api.notion.com/v1/oauth/token".into(),
                mcp_endpoint: "https://mcp.notion.com/mcp".into(),
                client_id_env: "SHANNON_NOTION_CLIENT_ID".into(),
                default_scopes: vec![],
                display_name: "Connect Notion".into(),
            },
            homepage_url: "https://developers.notion.com/".into(),
        },
        FeaturedVendor {
            slug: "linear".into(),
            display_name: "Linear".into(),
            description: "Issues, projects, and sprints from Linear.".into(),
            icon: "linear".into(),
            category: FeaturedCategory::Productivity,
            trust: TrustLevel::Verified,
            install_kind: FeaturedInstallKind::OAuthRemote {
                authorize_url: "https://api.linear.app/oauth/authorize".into(),
                token_url: "https://api.linear.app/oauth/token".into(),
                mcp_endpoint: "https://mcp.linear.app/sse".into(),
                client_id_env: "SHANNON_LINEAR_CLIENT_ID".into(),
                default_scopes: vec!["read".into(), "write".into()],
                display_name: "Connect Linear".into(),
            },
            homepage_url: "https://developers.linear.app/".into(),
        },
        FeaturedVendor {
            slug: "slack".into(),
            display_name: "Slack".into(),
            description: "List channels, post messages, search history.".into(),
            icon: "slack".into(),
            category: FeaturedCategory::Communication,
            trust: TrustLevel::Verified,
            install_kind: FeaturedInstallKind::OAuthRemote {
                authorize_url: "https://slack.com/oauth/v2/authorize".into(),
                token_url: "https://slack.com/api/oauth.v2.access".into(),
                mcp_endpoint: "https://mcp.slack.com/sse".into(),
                client_id_env: "SHANNON_SLACK_CLIENT_ID".into(),
                default_scopes: vec!["channels:read".into(), "chat:write".into()],
                display_name: "Connect Slack".into(),
            },
            homepage_url: "https://api.slack.com/".into(),
        },
        FeaturedVendor {
            slug: "github".into(),
            display_name: "GitHub".into(),
            description: "Repos, issues, PRs, and code search.".into(),
            icon: "github".into(),
            category: FeaturedCategory::DeveloperTools,
            trust: TrustLevel::Verified,
            install_kind: FeaturedInstallKind::OAuthRemote {
                authorize_url: "https://github.com/login/oauth/authorize".into(),
                token_url: "https://github.com/login/oauth/access_token".into(),
                mcp_endpoint: "https://api.githubcopilot.com/mcp/".into(),
                client_id_env: "SHANNON_GITHUB_CLIENT_ID".into(),
                default_scopes: vec!["repo".into(), "read:org".into()],
                display_name: "Connect GitHub".into(),
            },
            homepage_url: "https://docs.github.com/".into(),
        },
        FeaturedVendor {
            slug: "gmail".into(),
            display_name: "Gmail".into(),
            description: "Read, send, and search email.".into(),
            icon: "gmail".into(),
            category: FeaturedCategory::Communication,
            trust: TrustLevel::Verified,
            install_kind: FeaturedInstallKind::OAuthRemote {
                authorize_url: "https://accounts.google.com/o/oauth2/v2/auth".into(),
                token_url: "https://oauth2.googleapis.com/token".into(),
                mcp_endpoint: "https://gmail.googleapis.com/mcp".into(),
                client_id_env: "SHANNON_GMAIL_CLIENT_ID".into(),
                default_scopes: vec![
                    "https://www.googleapis.com/auth/gmail.readonly".into(),
                    "https://www.googleapis.com/auth/gmail.send".into(),
                ],
                display_name: "Connect Gmail".into(),
            },
            homepage_url: "https://developers.google.com/gmail".into(),
        },
    ]
}

/// A curated featured entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeaturedVendor {
    pub slug: String,
    pub display_name: String,
    pub description: String,
    /// Material Symbols icon name — UI looks it up from the icon font.
    pub icon: String,
    pub category: FeaturedCategory,
    pub trust: TrustLevel,
    pub install_kind: FeaturedInstallKind,
    pub homepage_url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeaturedCategory {
    Productivity,
    Communication,
    DeveloperTools,
    DataSources,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum FeaturedInstallKind {
    /// Tier-2 OAuth 2.1 PKCE flow against vendor's remote MCP server.
    #[serde(rename = "oauth_remote")]
    OAuthRemote {
        authorize_url: String,
        token_url: String,
        mcp_endpoint: String,
        /// Env var name from which Shannon reads the OAuth client_id.
        /// Vendor publishes their own client_id; users can override for
        /// self-hosted or dev scenarios.
        client_id_env: String,
        default_scopes: Vec<String>,
        display_name: String,
    },
    /// Tier-3 stdio escape hatch (e.g. filesystem, sqlite via npx).
    #[serde(rename = "stdio")]
    Stdio {
        command: String,
        args: Vec<String>,
        env_vars: Vec<(String, String)>,
        display_name: String,
    },
}

impl FeaturedVendor {
    /// Convert to a `CatalogEntry` the hub UI renders uniformly.
    pub fn to_catalog_entry(&self) -> CatalogEntry {
        let metadata = match &self.install_kind {
            FeaturedInstallKind::OAuthRemote { mcp_endpoint, .. } => {
                let mut m = std::collections::HashMap::new();
                m.insert("transport".to_string(), serde_json::json!("oauth_remote"));
                m.insert("endpoint".to_string(), serde_json::json!(mcp_endpoint));
                m
            }
            FeaturedInstallKind::Stdio { command, args, .. } => {
                let mut m = std::collections::HashMap::new();
                m.insert("transport".to_string(), serde_json::json!("stdio"));
                m.insert("command".to_string(), serde_json::json!(command));
                m.insert("args".to_string(), serde_json::json!(args));
                m
            }
        };
        CatalogEntry {
            id: format!("featured:{}", self.slug),
            kind: AddonKind::Mcp,
            name: self.display_name.clone(),
            description: self.description.clone(),
            author: Some(self.slug.clone()),
            version: None,
            homepage_url: Some(self.homepage_url.clone()),
            license: Some("proprietary".into()),
            stars: None,
            last_updated: None,
            source: CatalogSource::FeaturedVendor,
            trust: self.trust,
            metadata,
            tags: vec![self.category.as_str().into()],
        }
    }
}

impl FeaturedCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            FeaturedCategory::Productivity => "productivity",
            FeaturedCategory::Communication => "communication",
            FeaturedCategory::DeveloperTools => "developer_tools",
            FeaturedCategory::DataSources => "data_sources",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            FeaturedCategory::Productivity => "Productivity",
            FeaturedCategory::Communication => "Communication",
            FeaturedCategory::DeveloperTools => "Developer Tools",
            FeaturedCategory::DataSources => "Data Sources",
        }
    }
}

// ---------------------------------------------------------------------------
// MCP Registry client
// ---------------------------------------------------------------------------

/// HTTP abstraction so tests can mock the registry without real network.
#[async_trait::async_trait]
pub trait HttpFetch: Send + Sync {
    async fn fetch_json(&self, url: &str) -> Result<String, InstallError>;
}

/// Production HTTP client backed by `reqwest`.
pub struct ReqwestFetch {
    client: reqwest::Client,
}

impl ReqwestFetch {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent(concat!("shannon-desktop/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(15))
            .build()
            .expect("reqwest client");
        Self { client }
    }
}

impl Default for ReqwestFetch {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl HttpFetch for ReqwestFetch {
    async fn fetch_json(&self, url: &str) -> Result<String, InstallError> {
        let resp = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| InstallError::Network(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(InstallError::Network(format!(
                "registry returned {}",
                resp.status()
            )));
        }
        resp.text()
            .await
            .map_err(|e| InstallError::Network(e.to_string()))
    }
}

/// Defensive client for the open MCP Registry at `registry.modelcontextprotocol.io`.
///
/// API is v0.1 and frozen — see ADR open question #1. We pin to v0 path and
/// route everything through `HttpFetch` so tests can substitute a mock.
pub struct McpRegistryClient {
    base_url: String,
    http: Arc<dyn HttpFetch>,
    cache_dir: Option<PathBuf>,
    cache_ttl: Duration,
}

impl McpRegistryClient {
    pub fn new(http: Arc<dyn HttpFetch>) -> Self {
        Self {
            base_url: "https://registry.modelcontextprotocol.io/v0".into(),
            http,
            cache_dir: default_cache_dir(),
            cache_ttl: Duration::from_secs(24 * 60 * 60), // 24h
        }
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    pub fn with_cache_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cache_dir = Some(dir.into());
        self
    }

    pub fn with_cache_ttl(mut self, ttl: Duration) -> Self {
        self.cache_ttl = ttl;
        self
    }

    /// Fetch the registry's server list, using the on-disk cache when fresh.
    pub async fn list_servers(&self) -> Result<Vec<RegistryServer>, InstallError> {
        if let Some(cached) = self.read_cache().await {
            return Ok(cached);
        }

        let url = format!("{}/servers", self.base_url);
        let body = self.http.fetch_json(&url).await?;
        let resp: RegistryResponse =
            serde_json::from_str(&body).map_err(|e| InstallError::Format(format!("registry parse: {e}")))?;

        let servers = resp.servers;
        self.write_cache(&servers);
        Ok(servers)
    }

    /// Convert a registry server to a hub `CatalogEntry`.
    pub fn to_catalog_entry(server: &RegistryServer) -> CatalogEntry {
        let trust = if server.verified {
            TrustLevel::Verified
        } else if server.repository.as_ref().is_some_and(|r| r.starts_with("makenotion/"))
            || server
                .repository
                .as_ref()
                .is_some_and(|r| r.starts_with("modelcontextprotocol/"))
        {
            TrustLevel::Official
        } else {
            TrustLevel::Community
        };

        let mut metadata = std::collections::HashMap::new();
        if let Some(repo) = &server.repository {
            metadata.insert("repository".to_string(), serde_json::json!(repo));
        }
        if let Some(pkg) = &server.package {
            metadata.insert("package".to_string(), serde_json::json!(pkg));
        }

        CatalogEntry {
            id: format!("mcp-reg:{}", if server.id.is_empty() { &server.name } else { &server.id }),
            kind: AddonKind::Mcp,
            name: server.name.clone(),
            description: server.description.clone().unwrap_or_default(),
            author: server.repository.clone(),
            version: server.version.clone(),
            homepage_url: server.homepage_url.clone(),
            license: server.license.clone(),
            stars: server.stars,
            last_updated: server.last_updated,
            source: CatalogSource::McpRegistry {
                publisher: "io.modelcontextprotocol.registry".into(),
            },
            trust,
            metadata,
            tags: vec![],
        }
    }

    async fn read_cache(&self) -> Option<Vec<RegistryServer>> {
        let dir = self.cache_dir.as_ref()?;
        let path = dir.join("mcp-registry-servers.json");
        let meta = std::fs::metadata(&path).ok()?;

        let modified = meta.modified().ok()?;
        let elapsed = SystemTime::now().duration_since(modified).ok()?;
        if elapsed > self.cache_ttl {
            return None;
        }

        let body = std::fs::read_to_string(&path).ok()?;
        let parsed: RegistryResponse = serde_json::from_str(&body).ok()?;
        Some(parsed.servers)
    }

    fn write_cache(&self, servers: &[RegistryServer]) {
        let Some(dir) = self.cache_dir.as_ref() else { return };
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
        let body = RegistryResponse { servers: servers.to_vec() };
        if let Ok(json) = serde_json::to_string_pretty(&body) {
            let _ = std::fs::write(dir.join("mcp-registry-servers.json"), json);
        }
    }
}

/// Defensive shape — only the fields the hub UI needs. Unknown fields are
/// ignored so registry additions don't break parsing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryResponse {
    #[serde(default, deserialize_with = "deserialize_registry_servers")]
    pub servers: Vec<RegistryServer>,
}

/// Inner server payload returned by the registry. The real API wraps each
/// entry as `{"server": {...}, "_meta": {...}}`; we accept that shape as well
/// as a flat `{id, name, ...}` for backward compatibility with cached files
/// and tests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryServer {
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub homepage_url: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub stars: Option<u64>,
    #[serde(default)]
    pub last_updated: Option<DateTime<Utc>>,
    /// Shannon verification signal — currently derived from repository owner.
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub package: Option<RegistryPackage>,
}

fn deserialize_registry_servers<'de, D>(deserializer: D) -> Result<Vec<RegistryServer>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;

    let raw = serde_json::Value::deserialize(deserializer)?;
    let Some(arr) = raw.as_array() else {
        return Ok(Vec::new());
    };

    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let (server_val, meta_val) = match item.get("server") {
            Some(inner) => (inner.clone(), item.get("_meta").cloned()),
            None => (item.clone(), None),
        };

        let mut server: RegistryServer = serde_json::from_value(server_val)
            .map_err(serde::de::Error::custom)?;

        if server.id.is_empty() {
            server.id = server.name.clone();
        }

        if let Some(meta) = meta_val {
            if let Some(status) = meta
                .get("io.modelcontextprotocol.registry/official")
                .and_then(|v| v.get("status"))
                .and_then(|v| v.as_str())
            {
                if status == "active" {
                    server.verified = true;
                }
            }
        }

        out.push(server);
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryPackage {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub registry_url: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
}

fn default_cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("shannon-desktop").join("extensions"))
}

/// Static mock fetcher for tests.
pub struct StaticFetch(pub String);

#[async_trait::async_trait]
impl HttpFetch for StaticFetch {
    async fn fetch_json(&self, _url: &str) -> Result<String, InstallError> {
        Ok(self.0.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn featured_vendors_includes_five_canonical_vendors() {
        let vendors = featured_vendors();
        assert_eq!(vendors.len(), 5, "ADR specifies exactly 5 featured vendors");
        let slugs: Vec<&str> = vendors.iter().map(|v| v.slug.as_str()).collect();
        assert!(slugs.contains(&"notion"));
        assert!(slugs.contains(&"linear"));
        assert!(slugs.contains(&"slack"));
        assert!(slugs.contains(&"github"));
        assert!(slugs.contains(&"gmail"));
    }

    #[test]
    fn featured_vendor_converts_to_catalog_entry_with_metadata() {
        let vendor = featured_vendors().into_iter().find(|v| v.slug == "notion").unwrap();
        let entry = vendor.to_catalog_entry();
        assert_eq!(entry.kind, AddonKind::Mcp);
        assert_eq!(entry.id, "featured:notion");
        assert_eq!(entry.trust, TrustLevel::Verified);
        assert_eq!(entry.source, CatalogSource::FeaturedVendor);
        assert_eq!(
            entry.metadata.get("endpoint").and_then(|v| v.as_str()),
            Some("https://mcp.notion.com/mcp")
        );
        assert_eq!(
            entry.metadata.get("transport").and_then(|v| v.as_str()),
            Some("oauth_remote")
        );
    }

    #[test]
    fn featured_category_as_str_round_trips() {
        let cats = [
            FeaturedCategory::Productivity,
            FeaturedCategory::Communication,
            FeaturedCategory::DeveloperTools,
            FeaturedCategory::DataSources,
        ];
        for c in cats {
            let json = serde_json::to_string(&c).unwrap();
            let back: FeaturedCategory = serde_json::from_str(&json).unwrap();
            assert_eq!(c, back);
        }
    }

    #[test]
    fn featured_install_kind_serializes_with_type_tag() {
        let kind = FeaturedInstallKind::OAuthRemote {
            authorize_url: "https://example.com/auth".into(),
            token_url: "https://example.com/token".into(),
            mcp_endpoint: "https://example.com/mcp".into(),
            client_id_env: "SHANNON_EXAMPLE_CLIENT_ID".into(),
            default_scopes: vec!["read".into()],
            display_name: "Connect Example".into(),
        };
        let json = serde_json::to_string(&kind).unwrap();
        assert!(json.contains("\"type\":\"oauth_remote\""));
    }

    #[test]
    fn registry_server_to_catalog_entry_marks_verified_as_verified() {
        let server = RegistryServer {
            id: "notion".into(),
            name: "Notion".into(),
            description: Some("test".into()),
            repository: Some("makenotion/notion-mcp-server".into()),
            version: Some("1.0.0".into()),
            homepage_url: None,
            license: Some("MIT".into()),
            stars: Some(4100),
            last_updated: None,
            verified: true,
            package: None,
        };
        let entry = McpRegistryClient::to_catalog_entry(&server);
        assert_eq!(entry.trust, TrustLevel::Verified);
        assert_eq!(entry.id, "mcp-reg:notion");
        assert!(matches!(entry.source, CatalogSource::McpRegistry { .. }));
    }

    #[test]
    fn registry_server_with_official_repo_gets_official_trust() {
        let server = RegistryServer {
            id: "filesystem".into(),
            name: "Filesystem".into(),
            description: None,
            repository: Some("modelcontextprotocol/filesystem".into()),
            version: None,
            homepage_url: None,
            license: None,
            stars: None,
            last_updated: None,
            verified: false,
            package: None,
        };
        let entry = McpRegistryClient::to_catalog_entry(&server);
        assert_eq!(entry.trust, TrustLevel::Official);
    }

    #[test]
    fn registry_server_community_repo_gets_community_trust() {
        let server = RegistryServer {
            id: "random-tool".into(),
            name: "Random Tool".into(),
            description: None,
            repository: Some("someuser/random-tool".into()),
            version: None,
            homepage_url: None,
            license: None,
            stars: None,
            last_updated: None,
            verified: false,
            package: None,
        };
        let entry = McpRegistryClient::to_catalog_entry(&server);
        assert_eq!(entry.trust, TrustLevel::Community);
    }

    #[tokio::test]
    async fn registry_client_fetches_and_parses_servers() {
        let body = r#"{
            "servers": [
                {
                    "id": "notion",
                    "name": "Notion",
                    "description": "Notion MCP",
                    "repository": "makenotion/notion-mcp-server",
                    "version": "1.0.0",
                    "license": "MIT",
                    "stars": 4100,
                    "verified": true
                },
                {
                    "id": "fs",
                    "name": "Filesystem",
                    "repository": "modelcontextprotocol/filesystem"
                }
            ]
        }"#;
        let fetcher: Arc<dyn HttpFetch> = Arc::new(StaticFetch(body.into()));
        let client = McpRegistryClient::new(fetcher).with_cache_dir(tempfile::tempdir().unwrap().keep());
        let servers = client.list_servers().await.expect("fetch");
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0].name, "Notion");
    }

    #[tokio::test]
    async fn registry_client_uses_cache_on_second_call() {
        let body = r#"{"servers":[{"id":"x","name":"X"}]}"#;
        let call_count = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        struct CountingFetch {
            body: String,
            count: std::sync::Arc<std::sync::atomic::AtomicU32>,
        }
        #[async_trait::async_trait]
        impl HttpFetch for CountingFetch {
            async fn fetch_json(&self, _url: &str) -> Result<String, InstallError> {
                self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(self.body.clone())
            }
        }
        let fetcher: Arc<dyn HttpFetch> = Arc::new(CountingFetch {
            body: body.into(),
            count: call_count.clone(),
        });
        let dir = tempfile::tempdir().unwrap();
        let client = McpRegistryClient::new(fetcher).with_cache_dir(dir.keep());

        let _ = client.list_servers().await.unwrap();
        let _ = client.list_servers().await.unwrap();
        assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 1, "second call should hit cache");
    }

    #[tokio::test]
    async fn registry_client_returns_network_error_on_fetch_failure() {
        struct ErrFetch;
        #[async_trait::async_trait]
        impl HttpFetch for ErrFetch {
            async fn fetch_json(&self, _url: &str) -> Result<String, InstallError> {
                Err(InstallError::Network("simulated".into()))
            }
        }
        let fetcher: Arc<dyn HttpFetch> = Arc::new(ErrFetch);
        let client = McpRegistryClient::new(fetcher).with_cache_dir(tempfile::tempdir().unwrap().keep());
        let err = client.list_servers().await.unwrap_err();
        assert!(matches!(err, InstallError::Network(_)));
    }

    #[tokio::test]
    async fn registry_client_returns_format_error_on_bad_json() {
        let fetcher: Arc<dyn HttpFetch> = Arc::new(StaticFetch("not json".into()));
        let client = McpRegistryClient::new(fetcher).with_cache_dir(tempfile::tempdir().unwrap().keep());
        let err = client.list_servers().await.unwrap_err();
        assert!(matches!(err, InstallError::Format(_)));
    }

    #[tokio::test]
    async fn cache_miss_when_ttl_expired() {
        let body = r#"{"servers":[]}"#;
        let fetcher: Arc<dyn HttpFetch> = Arc::new(StaticFetch(body.into()));
        let dir = tempfile::tempdir().unwrap();
        let client = McpRegistryClient::new(fetcher)
            .with_cache_dir(dir.keep())
            .with_cache_ttl(Duration::from_secs(0));

        let _ = client.list_servers().await.unwrap();
        let _ = client.list_servers().await.unwrap();
    }

    #[tokio::test]
    async fn registry_client_parses_real_wrapped_api_shape() {
        let body = r#"{
            "servers": [
                {
                    "server": {
                        "name": "ac.inference.sh/mcp",
                        "description": "Run 150+ AI apps.",
                        "title": "inference.sh",
                        "version": "1.0.0",
                        "remotes": [
                            {"type": "streamable-http", "url": "https://sh.inference.ac"}
                        ]
                    },
                    "_meta": {
                        "io.modelcontextprotocol.registry/official": {
                            "status": "active",
                            "publishedAt": "2026-04-13T17:32:20.852269Z"
                        }
                    }
                }
            ],
            "metadata": {"nextCursor": "x", "count": 1}
        }"#;
        let fetcher: Arc<dyn HttpFetch> = Arc::new(StaticFetch(body.into()));
        let client = McpRegistryClient::new(fetcher).with_cache_dir(tempfile::tempdir().unwrap().keep());
        let servers = client.list_servers().await.expect("fetch");
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "ac.inference.sh/mcp");
        assert_eq!(servers[0].id, "ac.inference.sh/mcp", "id defaults from name");
        assert_eq!(servers[0].version.as_deref(), Some("1.0.0"));
        assert!(servers[0].verified, "active official status marks verified");
    }
}
