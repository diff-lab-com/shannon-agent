//! Tauri commands for the unified extensions hub (P2 onwards).
//!
//! P2 wires the MCP installers:
//! - `list_featured_vendors` — return the curated featured list.
//! - `list_mcp_registry_servers` — fetch the MCP registry (24h cache).
//! - `install_mcp_stdio` — Tier-3 escape hatch.
//! - `install_mcp_mcpb` — `.mcpb` upload from the user's disk.
//! - `install_mcp_oauth_authorize_url` — produce the URL the UI opens in a browser.
//! - `install_mcp_oauth_complete` — write the entry once the UI hands back a token.
//! - `uninstall_mcp_server` — remove an installed MCP server.
//!
//! P3 adds skills catalog + installer:
//! - `list_skill_catalog` — federated skills (native + GitHub upstreams, 24h cache).
//! - `install_skill_from_repo` — clone a GitHub skill collection.
//! - `install_native_skill` — write a built-in skill's SKILL.md.
//! - `list_installed_skill_plugins` — scan `~/.shannon/skills/`.
//! - `uninstall_skill_plugin` — remove a skill plugin dir.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::extensions::{
    self, AgentCatalogClient, AgentMarkdownInstaller, AgentRepoInstaller, DataSourceAdapter,
    FeaturedInstallKind, MarketplacePluginInstaller, McpRegistryClient, ReqwestFetch,
    ResolvedMcpInstaller, SkillCatalogClient, SkillMarkdownInstaller, StdioMcpInstaller,
    StdioMcpSpec, catalog::FeaturedVendor, installer::AddonInstaller,
};

/// Featured vendor list — baked into the app, no network fetch.
#[tauri::command]
pub async fn list_featured_vendors() -> Result<Vec<FeaturedVendor>, String> {
    Ok(extensions::featured_vendors())
}

/// MCP Registry response (already deduplicated/cached by the client).
#[tauri::command]
pub async fn list_mcp_registry_servers() -> Result<Vec<extensions::RegistryServer>, String> {
    let fetcher: Arc<dyn extensions::HttpFetch> = Arc::new(ReqwestFetch::new());
    let client = McpRegistryClient::new(fetcher);
    client.list_servers().await.map_err(|e| e.to_string())
}

/// Convert a featured vendor into a catalog entry for the UI to render.
#[tauri::command]
pub async fn featured_vendor_to_entry(slug: String) -> Result<extensions::CatalogEntry, String> {
    let vendors = extensions::featured_vendors();
    let vendor = vendors
        .into_iter()
        .find(|v| v.slug == slug)
        .ok_or_else(|| format!("unknown featured vendor {slug}"))?;
    Ok(vendor.to_catalog_entry())
}

/// Tier-3 stdio install — user supplies command/args/env via the form.
#[tauri::command]
pub async fn install_mcp_stdio(spec: StdioMcpSpecPayload) -> Result<InstallResult, String> {
    let installer = StdioMcpInstaller {
        spec: StdioMcpSpec {
            server_name: spec.server_name,
            command: spec.command,
            args: spec.args,
            env: spec.env.into_iter().collect(),
        },
    };
    // Build a synthetic CatalogEntry so the installer's bookkeeping works.
    let entry = extensions::CatalogEntry {
        id: format!("stdio:{}", installer.spec.server_name),
        kind: extensions::AddonKind::Mcp,
        name: installer.spec.server_name.clone(),
        description: String::new(),
        author: None,
        version: None,
        homepage_url: None,
        license: None,
        stars: None,
        last_updated: None,
        source: extensions::CatalogSource::Custom {
            url: "manual-entry".into(),
        },
        trust: extensions::TrustLevel::Unknown,
        metadata: Default::default(),
        tags: vec![],
    };
    let sink = extensions::ProgressSink::null();
    let installed = installer
        .install(&entry, &extensions::InstallTarget::ShannonMcpConfig, &sink)
        .await
        .map_err(|e| e.to_string())?;
    Ok(InstallResult {
        id: installed.id,
        name: installed.name,
        install_path: installed.install_path,
    })
}

/// `.mcpb` install — accepts archive bytes the UI read from disk.
#[tauri::command]
pub async fn install_mcp_mcpb(
    server_name: String,
    archive_bytes: Vec<u8>,
) -> Result<InstallResult, String> {
    use crate::extensions::McpbInstaller;
    let installer = McpbInstaller {
        archive_bytes,
        extract_root: None,
    };
    let entry = extensions::CatalogEntry {
        id: format!("mcpb:{server_name}"),
        kind: extensions::AddonKind::Mcp,
        name: server_name.clone(),
        description: String::new(),
        author: None,
        version: None,
        homepage_url: None,
        license: None,
        stars: None,
        last_updated: None,
        source: extensions::CatalogSource::Custom {
            url: "mcpb-upload".into(),
        },
        trust: extensions::TrustLevel::Community,
        metadata: Default::default(),
        tags: vec![],
    };
    let sink = extensions::ProgressSink::null();
    let installed = installer
        .install(&entry, &extensions::InstallTarget::ShannonMcpConfig, &sink)
        .await
        .map_err(|e| e.to_string())?;
    Ok(InstallResult {
        id: installed.id,
        name: installed.name,
        install_path: installed.install_path,
    })
}

/// Build the OAuth authorize URL the UI opens in a browser.
///
/// Returns the URL + the PKCE verifier (so the loopback callback can complete
/// the token exchange). The UI is responsible for actually opening the URL.
#[tauri::command]
pub async fn install_mcp_oauth_authorize_url(
    vendor_slug: String,
    redirect_uri: String,
) -> Result<OAuthAuthorizeUrl, String> {
    let vendor = extensions::featured_vendors()
        .into_iter()
        .find(|v| v.slug == vendor_slug)
        .ok_or_else(|| format!("unknown vendor {vendor_slug}"))?;
    if !matches!(vendor.install_kind, FeaturedInstallKind::OAuthRemote { .. }) {
        return Err(format!("vendor {} is not OAuth-capable", vendor_slug));
    }
    use crate::extensions::OAuthRemoteMcpInstaller;
    let installer = OAuthRemoteMcpInstaller { vendor };
    let pkce = installer.pkce_context();
    let url = installer
        .authorize_url(&pkce, &redirect_uri)
        .map_err(|e| e.to_string())?;
    Ok(OAuthAuthorizeUrl {
        url,
        verifier: pkce.verifier,
        state: pkce.state,
    })
}

/// Complete an OAuth install once the UI has the access token from the callback.
#[tauri::command]
pub async fn install_mcp_oauth_complete(
    vendor_slug: String,
    access_token: String,
) -> Result<InstallResult, String> {
    let vendor = extensions::featured_vendors()
        .into_iter()
        .find(|v| v.slug == vendor_slug)
        .ok_or_else(|| format!("unknown vendor {vendor_slug}"))?;
    use crate::extensions::OAuthRemoteMcpInstaller;
    let installer = OAuthRemoteMcpInstaller { vendor };
    let config = installer.server_config(&access_token);
    let server_name = format!("{}-oauth", vendor_slug);
    let path =
        extensions::write_mcp_server_config(&server_name, config).map_err(|e| e.to_string())?;
    Ok(InstallResult {
        id: format!("oauth:{vendor_slug}"),
        name: server_name,
        install_path: Some(format!("{}#mcpServers.{}", path.display(), vendor_slug)),
    })
}

/// Remove an installed MCP server entry.
#[tauri::command]
pub async fn uninstall_mcp_server(server_name: String) -> Result<(), String> {
    extensions::remove_mcp_server_config(&server_name).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// P3: Skills catalog + installer
// ---------------------------------------------------------------------------

/// Fetch the federated skill catalog (native + GitHub upstreams, 24h cache).
#[tauri::command]
pub async fn list_skill_catalog() -> Result<Vec<extensions::CatalogEntry>, String> {
    let fetcher: Arc<dyn extensions::HttpFetch> = Arc::new(ReqwestFetch::new());
    let client = SkillCatalogClient::new(fetcher);
    client.list_skills().await.map_err(|e| e.to_string())
}

/// Clone a GitHub skill collection into `~/.shannon/skills/<plugin>/`.
#[tauri::command]
pub async fn install_skill_from_repo(
    plugin_name: String,
    repo: String,
    ref_: String,
) -> Result<InstallResult, String> {
    let installer = MarketplacePluginInstaller {
        plugin_name: plugin_name.clone(),
        repo,
        ref_,
    };
    // Synthetic catalog entry so the installer's bookkeeping works.
    let entry = extensions::CatalogEntry {
        id: format!("marketplace:{}", plugin_name),
        kind: extensions::AddonKind::Skill,
        name: plugin_name.clone(),
        description: String::new(),
        author: None,
        version: None,
        homepage_url: None,
        license: None,
        stars: None,
        last_updated: None,
        source: extensions::CatalogSource::GitHubRepo {
            repo: installer.repo.clone(),
            ref_: Some(installer.ref_.clone()),
        },
        trust: extensions::TrustLevel::Community,
        metadata: Default::default(),
        tags: vec![],
    };
    let sink = extensions::ProgressSink::null();
    let installed = installer
        .install(
            &entry,
            &extensions::InstallTarget::ShannonSkillsDir {
                plugin: plugin_name.clone(),
            },
            &sink,
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(InstallResult {
        id: installed.id,
        name: installed.name,
        install_path: installed.install_path,
    })
}

/// Write a built-in skill's SKILL.md body to `~/.shannon/skills/<plugin>/`.
#[tauri::command]
pub async fn install_native_skill(
    plugin_name: String,
    body: String,
) -> Result<InstallResult, String> {
    let installer = SkillMarkdownInstaller {
        plugin_name: plugin_name.clone(),
        body,
    };
    let entry = extensions::CatalogEntry {
        id: format!("native:{}", plugin_name),
        kind: extensions::AddonKind::Skill,
        name: plugin_name.clone(),
        description: String::new(),
        author: None,
        version: None,
        homepage_url: None,
        license: None,
        stars: None,
        last_updated: None,
        source: extensions::CatalogSource::Native,
        trust: extensions::TrustLevel::Verified,
        metadata: Default::default(),
        tags: vec![],
    };
    let sink = extensions::ProgressSink::null();
    let installed = installer
        .install(
            &entry,
            &extensions::InstallTarget::ShannonSkillsDir {
                plugin: plugin_name.clone(),
            },
            &sink,
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(InstallResult {
        id: installed.id,
        name: installed.name,
        install_path: installed.install_path,
    })
}

/// Scan `~/.shannon/skills/` for installed skill plugins.
#[tauri::command]
pub async fn list_installed_skill_plugins() -> Result<Vec<extensions::InstalledSkill>, String> {
    Ok(extensions::list_installed_skills())
}

/// Remove an installed skill plugin.
#[tauri::command]
pub async fn uninstall_skill_plugin(name: String) -> Result<(), String> {
    extensions::remove_installed_skill(&name).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// P4: Agents federated catalog + marketplace
// ---------------------------------------------------------------------------

/// Fetch the federated agent catalog (native + GitHub upstreams, 24h cache).
#[tauri::command]
pub async fn list_agent_catalog() -> Result<Vec<extensions::CatalogEntry>, String> {
    let fetcher: Arc<dyn extensions::HttpFetch> = Arc::new(ReqwestFetch::new());
    let client = AgentCatalogClient::new(fetcher);
    client.list_agents().await.map_err(|e| e.to_string())
}

/// Clone a GitHub agent collection into `~/.shannon/agents/<plugin>/`.
#[tauri::command]
pub async fn install_agent_from_repo(
    plugin_name: String,
    repo: String,
    ref_: String,
) -> Result<InstallResult, String> {
    let installer = AgentRepoInstaller {
        plugin_name: plugin_name.clone(),
        repo,
        ref_,
    };
    let entry = extensions::CatalogEntry {
        id: format!("agent-repo:{}", plugin_name),
        kind: extensions::AddonKind::Agent,
        name: plugin_name.clone(),
        description: String::new(),
        author: None,
        version: None,
        homepage_url: None,
        license: None,
        stars: None,
        last_updated: None,
        source: extensions::CatalogSource::GitHubRepo {
            repo: installer.repo.clone(),
            ref_: Some(installer.ref_.clone()),
        },
        trust: extensions::TrustLevel::Community,
        metadata: Default::default(),
        tags: vec![],
    };
    let sink = extensions::ProgressSink::null();
    let installed = installer
        .install(
            &entry,
            &extensions::InstallTarget::ShannonAgentsDir {
                plugin: plugin_name.clone(),
            },
            &sink,
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(InstallResult {
        id: installed.id,
        name: installed.name,
        install_path: installed.install_path,
    })
}

/// Write a built-in agent's `.md` body to `~/.shannon/agents/<plugin>/agent.md`.
#[tauri::command]
pub async fn install_native_agent(
    plugin_name: String,
    body: String,
) -> Result<InstallResult, String> {
    let installer = AgentMarkdownInstaller {
        plugin_name: plugin_name.clone(),
        body,
    };
    let entry = extensions::CatalogEntry {
        id: format!("native:agent-{}", plugin_name),
        kind: extensions::AddonKind::Agent,
        name: plugin_name.clone(),
        description: String::new(),
        author: None,
        version: None,
        homepage_url: None,
        license: None,
        stars: None,
        last_updated: None,
        source: extensions::CatalogSource::Native,
        trust: extensions::TrustLevel::Verified,
        metadata: Default::default(),
        tags: vec![],
    };
    let sink = extensions::ProgressSink::null();
    let installed = installer
        .install(
            &entry,
            &extensions::InstallTarget::ShannonAgentsDir {
                plugin: plugin_name.clone(),
            },
            &sink,
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(InstallResult {
        id: installed.id,
        name: installed.name,
        install_path: installed.install_path,
    })
}

/// Scan `~/.shannon/agents/` for installed agent plugins.
#[tauri::command]
pub async fn list_installed_agent_plugins() -> Result<Vec<extensions::InstalledAgent>, String> {
    Ok(extensions::list_installed_agents())
}

/// Remove an installed agent plugin.
#[tauri::command]
pub async fn uninstall_agent_plugin(name: String) -> Result<(), String> {
    extensions::remove_installed_agent(&name).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// P5: Native data sources (Obsidian + Email IMAP)
// ---------------------------------------------------------------------------

/// Fetch the static data source catalog (no network — adapter metadata only).
#[tauri::command]
pub async fn list_data_source_catalog() -> Result<Vec<extensions::CatalogEntry>, String> {
    Ok(extensions::data_source_catalog_entries())
}

/// Static adapter list — used by the UI to render install forms dynamically.
#[tauri::command]
pub async fn list_data_source_adapters() -> Result<Vec<DataSourceAdapter>, String> {
    Ok(extensions::data_source_adapters())
}

/// Persist a data source config to `~/.shannon/data-sources/<slug>.toml`.
#[tauri::command]
pub async fn install_data_source(
    slug: String,
    kind: String,
    name: String,
    config: std::collections::BTreeMap<String, String>,
) -> Result<InstallResult, String> {
    extensions::install_data_source(&slug, &kind, &name, &config)
        .map(|installed| InstallResult {
            id: format!("native:data-source-{}", installed.slug),
            name: installed.name,
            install_path: Some(installed.path),
        })
        .map_err(|e| e.to_string())
}

/// Scan `~/.shannon/data-sources/` for installed configs.
#[tauri::command]
pub async fn list_installed_data_sources() -> Result<Vec<extensions::InstalledDataSource>, String> {
    Ok(extensions::list_installed_data_sources())
}

/// Remove an installed data source config.
#[tauri::command]
pub async fn uninstall_data_source(slug: String) -> Result<(), String> {
    extensions::remove_installed_data_source(&slug).map_err(|e| e.to_string())
}

/// Read back the config block for an installed data source. Used by the
/// "Test connection" button and by adapters at query time.
#[tauri::command]
pub async fn read_data_source_config(
    slug: String,
) -> Result<std::collections::BTreeMap<String, String>, String> {
    extensions::read_data_source_config(&slug).map_err(|e| e.to_string())
}

/// Query a data source by slug. Dispatches to the appropriate HTTP fetcher
/// based on the kind field in the installed config.
#[tauri::command]
pub async fn query_data_source(
    slug: String,
    query: String,
) -> Result<extensions::data_source_fetchers::DataSourceResult, String> {
    let config = extensions::read_data_source_config(&slug).map_err(|e| e.to_string())?;
    let kind = config.get("kind").ok_or("missing kind in config")?;
    let fetcher = extensions::data_source_fetchers::dispatch(kind).map_err(|e| e.to_string())?;
    fetcher
        .fetch(&config, &query)
        .await
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// P6: Security hardening — prompt injection, signature verify, reports
// ---------------------------------------------------------------------------

/// Scan free-form text for prompt injection patterns. Used by the UI before
/// showing install confirmation for community/repo entries.
#[tauri::command]
pub async fn scan_prompt_injection(text: String) -> Result<extensions::InjectionReport, String> {
    Ok(extensions::scan_prompt_injection(&text))
}

/// D1: Scan description + README body. Fetches `readme_url` (lazy, 24h
/// cached, 32KB truncated, 10s timeout) and combines with `description`
/// before scanning. Falls back to description-only on any fetch error.
#[tauri::command]
pub async fn scan_prompt_injection_with_readme(
    description: String,
    readme_url: Option<String>,
) -> Result<extensions::InjectionReport, String> {
    let readme = match readme_url.as_deref().filter(|u| !u.is_empty()) {
        Some(url) => extensions::fetch_readme_cached(url).await,
        None => None,
    };
    Ok(extensions::scan_with_readme(
        &description,
        readme.as_deref(),
    ))
}

/// Verify a signature body (typically the contents of `.mcpb/SIGNATURE.txt`).
#[tauri::command]
pub async fn verify_signature(
    signature_body: Option<String>,
) -> Result<extensions::SignatureReport, String> {
    Ok(extensions::verify_signature(signature_body.as_deref()))
}

/// Append a report about a catalog entry to `~/.shannon/reports.json`.
#[tauri::command]
pub async fn report_catalog_entry(
    entry_id: String,
    reason: String,
) -> Result<extensions::CatalogReport, String> {
    extensions::add_report(&entry_id, &reason).map_err(|e| e.to_string())
}

/// List all reports the user has filed.
#[tauri::command]
pub async fn list_catalog_reports() -> Result<Vec<extensions::CatalogReport>, String> {
    extensions::load_reports()
        .map(|s| s.reports)
        .map_err(|e| e.to_string())
}

/// Clear a previously filed report by entry id.
#[tauri::command]
pub async fn clear_catalog_report(entry_id: String) -> Result<usize, String> {
    extensions::remove_report(&entry_id).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct StdioMcpSpecPayload {
    pub server_name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Serialize)]
pub struct InstallResult {
    pub id: String,
    pub name: String,
    pub install_path: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OAuthAuthorizeUrl {
    pub url: String,
    pub verifier: String,
    pub state: String,
}

// ---------------------------------------------------------------------------
// Re-exports for main.rs handler list
// ---------------------------------------------------------------------------

pub use extensions::{CatalogEntry, CatalogSource, TrustLevel};

/// Sentinel to keep ResolvedMcpInstaller accessible from the handler module
/// without polluting the public API. Future P2 follow-up will dispatch real
/// installs through this type.
#[allow(dead_code)]
type _Dispatcher = Option<ResolvedMcpInstaller>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdio_spec_payload_deserializes_from_object() {
        let json = r#"{
            "server_name": "filesystem",
            "command": "npx",
            "args": ["-y", "@modelcontextprotocol/server-filesystem"],
            "env": [["ROOT", "/tmp"]]
        }"#;
        let spec: StdioMcpSpecPayload = serde_json::from_str(json).unwrap();
        assert_eq!(spec.server_name, "filesystem");
        assert_eq!(spec.command, "npx");
        assert_eq!(spec.env, vec![("ROOT".to_string(), "/tmp".to_string())]);
    }

    #[test]
    fn install_result_serializes_to_object() {
        let r = InstallResult {
            id: "x".into(),
            name: "x".into(),
            install_path: Some("/path".into()),
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"id\":\"x\""));
        assert!(json.contains("\"name\":\"x\""));
        assert!(json.contains("\"install_path\":\"/path\""));
    }

    #[test]
    fn oauth_authorize_url_payload_has_verifier_and_state() {
        let url = OAuthAuthorizeUrl {
            url: "https://x".into(),
            verifier: "v".into(),
            state: "s".into(),
        };
        let json = serde_json::to_string(&url).unwrap();
        assert!(json.contains("\"verifier\":\"v\""));
        assert!(json.contains("\"state\":\"s\""));
    }
}
