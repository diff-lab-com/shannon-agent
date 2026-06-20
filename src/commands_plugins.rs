//! Plugin management Tauri commands (A.3).
//!
//! Extracted from `commands.rs` as part of S2 P1.1 (commands.rs split).
//! Covers: listing, installing (local / git), uninstalling, enable/disable,
//! update, marketplace catalog (with first-run fallback), and upstream list.
//! Backed by `shannon_core::plugin::PluginRegistry` on AppState plus the
//! extensions catalog machinery in `crate::extensions`.

use serde::Serialize;

use crate::commands::AppState;
/// Serializable view of an installed plugin, exposed to the UI.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: Option<String>,
    pub plugin_type: String,
    pub enabled: bool,
    pub path: String,
    pub source_format: &'static str,
}

/// List all installed plugins. Triggers an on-disk rescan first so newly
/// dropped plugin directories show up without a restart.
#[tauri::command]
pub async fn list_plugins(state: tauri::State<'_, AppState>) -> Result<Vec<PluginInfo>, String> {
    let mut registry = state.plugin_registry.write().await;
    registry.load_all().await.map_err(|e| e.to_string())?;
    Ok(registry
        .list()
        .iter()
        .map(|p| PluginInfo {
            name: p.manifest.name.clone(),
            version: p.manifest.version.clone(),
            description: p.manifest.description.clone(),
            author: p.manifest.author.clone(),
            plugin_type: p.manifest.plugin_type.clone(),
            enabled: p.enabled,
            path: p.path.display().to_string(),
            source_format: source_format_for_path(&p.path),
        })
        .collect())
}

/// Detect whether a plugin directory uses Shannon TOML or Claude JSON.
fn source_format_for_path(path: &std::path::Path) -> &'static str {
    if path.join("plugin.toml").exists() {
        "shannon-toml"
    } else if path.join(".claude-plugin").join("plugin.json").exists() {
        "claude-json"
    } else {
        "unknown"
    }
}

/// Install a plugin from a local directory or archive file.
///
/// Accepts: a plugin directory containing `plugin.toml` or
/// `.claude-plugin/plugin.json`, or a `.dxt` / `.mcpb` ZIP archive.
#[tauri::command]
pub async fn install_plugin(
    state: tauri::State<'_, AppState>,
    source_path: String,
) -> Result<String, String> {
    let path = std::path::PathBuf::from(&source_path);
    if !path.exists() {
        return Err(format!("source path does not exist: {source_path}"));
    }

    let mut registry = state.plugin_registry.write().await;
    registry.ensure_dir().await.map_err(|e| e.to_string())?;
    let plugins_dir = registry.plugins_dir().to_path_buf();

    // Archive? Delegate to the .dxt/.mcpb installer.
    let is_archive = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| matches!(e.to_ascii_lowercase().as_str(), "dxt" | "mcpb" | "zip"))
        .unwrap_or(false);
    if is_archive {
        let name = shannon_core::plugin::install_extension_file(&path, &plugins_dir)
            .map_err(|e| e.to_string())?;
        // Rescan so the registry picks up the freshly extracted plugin.
        registry.load_all().await.map_err(|e| e.to_string())?;
        return Ok(name);
    }

    // Otherwise treat as a plugin directory and copy in.
    if path.is_dir() {
        let name = registry
            .install_from_path(&path)
            .await
            .map_err(|e| e.to_string())?;
        return Ok(name);
    }

    Err(format!(
        "source must be a directory or .dxt/.mcpb archive: {source_path}"
    ))
}

/// Install a plugin from a git URL (clones with `git clone --depth 1`).
#[tauri::command]
pub async fn install_plugin_from_git(
    state: tauri::State<'_, AppState>,
    repo_url: String,
) -> Result<String, String> {
    let mut registry = state.plugin_registry.write().await;
    registry
        .install_from_git(&repo_url)
        .await
        .map_err(|e| e.to_string())
}

/// Uninstall a plugin by name. Removes the directory.
#[tauri::command]
pub async fn uninstall_plugin(
    state: tauri::State<'_, AppState>,
    name: String,
) -> Result<(), String> {
    let mut registry = state.plugin_registry.write().await;
    registry.uninstall(&name).await.map_err(|e| e.to_string())
}

/// Enable a previously installed plugin.
#[tauri::command]
pub async fn enable_plugin(state: tauri::State<'_, AppState>, name: String) -> Result<(), String> {
    let mut registry = state.plugin_registry.write().await;
    registry.enable(&name).map_err(|e| e.to_string())
}

/// Disable a plugin (without removing it).
#[tauri::command]
pub async fn disable_plugin(state: tauri::State<'_, AppState>, name: String) -> Result<(), String> {
    let mut registry = state.plugin_registry.write().await;
    registry.disable(&name).map_err(|e| e.to_string())
}

/// Pull updates for a git-installed plugin.
#[tauri::command]
pub async fn update_plugin(state: tauri::State<'_, AppState>, name: String) -> Result<(), String> {
    let mut registry = state.plugin_registry.write().await;
    registry.update(&name).await.map_err(|e| e.to_string())
}

/// Fallback marketplace catalog for first-run experience (empty local registry).
///
/// Returns 18 high-quality entries across MCP, Skills, Agents, and Data Sources.
/// These are read-only catalog entries that route to specialized installers.
pub(crate) fn fallback_marketplace_catalog() -> Vec<crate::extensions::CatalogEntry> {
    use crate::extensions::types::{AddonKind, CatalogEntry, CatalogSource, TrustLevel};
    use std::collections::HashMap;

    let now = chrono::Utc::now();

    vec![
        // === MCP Servers (6 entries) ===
        CatalogEntry {
            id: "mcp-registry:filesystem".into(),
            kind: AddonKind::Mcp,
            name: "Filesystem MCP Server".into(),
            description: "Read and write local files securely through MCP filesystem transport.".into(),
            author: Some("ModelContextProtocol".into()),
            version: Some("1.0.0".into()),
            homepage_url: Some("https://github.com/modelcontextprotocol/servers".into()),
            license: Some("MIT".into()),
            stars: Some(4500),
            last_updated: Some(now - chrono::Duration::days(30)),
            source: CatalogSource::McpRegistry { publisher: "modelcontextprotocol".into() },
            trust: TrustLevel::Verified,
            metadata: HashMap::new(),
            tags: vec!["filesystem".into(), "local".into(), "stdio".into()],
        },
        CatalogEntry {
            id: "mcp-registry:git".into(),
            kind: AddonKind::Mcp,
            name: "Git MCP Server".into(),
            description: "Interact with Git repositories: status, diff, commit, branch operations.".into(),
            author: Some("ModelContextProtocol".into()),
            version: Some("1.0.0".into()),
            homepage_url: Some("https://github.com/modelcontextprotocol/servers".into()),
            license: Some("MIT".into()),
            stars: Some(3200),
            last_updated: Some(now - chrono::Duration::days(45)),
            source: CatalogSource::McpRegistry { publisher: "modelcontextprotocol".into() },
            trust: TrustLevel::Verified,
            metadata: HashMap::new(),
            tags: vec!["git".into(), "mcp".into()],
        },
        CatalogEntry {
            id: "mcp-registry:fetch".into(),
            kind: AddonKind::Mcp,
            name: "Fetch MCP Server".into(),
            description: "Make HTTP requests and fetch web content through MCP.".into(),
            author: Some("ModelContextProtocol".into()),
            version: Some("1.0.0".into()),
            homepage_url: Some("https://github.com/modelcontextprotocol/servers".into()),
            license: Some("MIT".into()),
            stars: Some(2800),
            last_updated: Some(now - chrono::Duration::days(60)),
            source: CatalogSource::McpRegistry { publisher: "modelcontextprotocol".into() },
            trust: TrustLevel::Verified,
            metadata: HashMap::new(),
            tags: vec!["fetch".into(), "http".into(), "mcp".into()],
        },
        CatalogEntry {
            id: "mcp-registry:sqlite".into(),
            kind: AddonKind::Mcp,
            name: "SQLite MCP Server".into(),
            description: "Query SQLite databases via SQL through MCP protocol.".into(),
            author: Some("ModelContextProtocol".into()),
            version: Some("1.0.0".into()),
            homepage_url: Some("https://github.com/modelcontextprotocol/servers".into()),
            license: Some("MIT".into()),
            stars: Some(2100),
            last_updated: Some(now - chrono::Duration::days(90)),
            source: CatalogSource::McpRegistry { publisher: "modelcontextprotocol".into() },
            trust: TrustLevel::Verified,
            metadata: HashMap::new(),
            tags: vec!["database".into(), "sqlite".into(), "mcp".into()],
        },
        CatalogEntry {
            id: "mcp-registry:postgres".into(),
            kind: AddonKind::Mcp,
            name: "PostgreSQL MCP Server".into(),
            description: "Connect to PostgreSQL databases and execute queries via MCP.".into(),
            author: Some("ModelContextProtocol".into()),
            version: Some("1.0.0".into()),
            homepage_url: Some("https://github.com/modelcontextprotocol/servers".into()),
            license: Some("MIT".into()),
            stars: Some(1900),
            last_updated: Some(now - chrono::Duration::days(120)),
            source: CatalogSource::McpRegistry { publisher: "modelcontextprotocol".into() },
            trust: TrustLevel::Verified,
            metadata: HashMap::new(),
            tags: vec!["database".into(), "postgres".into(), "mcp".into()],
        },
        CatalogEntry {
            id: "gh:supabase/cluster-mcp-server".into(),
            kind: AddonKind::Mcp,
            name: "Supabase Cluster MCP".into(),
            description: "Manage Supabase database clusters, migrations, and API keys.".into(),
            author: Some("Supabase".into()),
            version: Some("0.2.0".into()),
            homepage_url: Some("https://github.com/supabase/cluster-mcp-server".into()),
            license: Some("Apache-2.0".into()),
            stars: Some(860),
            last_updated: Some(now - chrono::Duration::days(20)),
            source: CatalogSource::GitHubRepo { repo: "supabase/cluster-mcp-server".into(), ref_: Some("main".into()) },
            trust: TrustLevel::Official,
            metadata: HashMap::new(),
            tags: vec!["supabase".into(), "database".into(), "mcp".into()],
        },

        // === Skills (5 entries) ===
        CatalogEntry {
            id: "gh:anthropics/skills:coding".into(),
            kind: AddonKind::Skill,
            name: "Coding Skills Pack".into(),
            description: "Essential coding skills: test-driven development, code review, refactoring patterns.".into(),
            author: Some("Anthropic".into()),
            version: Some("1.2.0".into()),
            homepage_url: Some("https://github.com/anthropics/skills".into()),
            license: Some("MIT".into()),
            stars: Some(5200),
            last_updated: Some(now - chrono::Duration::days(15)),
            source: CatalogSource::GitHubRepo { repo: "anthropics/skills".into(), ref_: Some("main".into()) },
            trust: TrustLevel::Official,
            metadata: HashMap::new(),
            tags: vec!["coding".into(), "official".into(), "skill".into()],
        },
        CatalogEntry {
            id: "gh:anthropics/skills:git-workflows".into(),
            kind: AddonKind::Skill,
            name: "Git Workflows Skills".into(),
            description: "Advanced Git operations: branching strategies, conflict resolution, history analysis.".into(),
            author: Some("Anthropic".into()),
            version: Some("1.1.0".into()),
            homepage_url: Some("https://github.com/anthropics/skills".into()),
            license: Some("MIT".into()),
            stars: Some(4100),
            last_updated: Some(now - chrono::Duration::days(25)),
            source: CatalogSource::GitHubRepo { repo: "anthropics/skills".into(), ref_: Some("main".into()) },
            trust: TrustLevel::Official,
            metadata: HashMap::new(),
            tags: vec!["git".into(), "workflows".into(), "official".into(), "skill".into()],
        },
        CatalogEntry {
            id: "gh:anthropics/skills:documentation".into(),
            kind: AddonKind::Skill,
            name: "Documentation Skills".into(),
            description: "Auto-generate docs from code: README, API docs, inline comments, architecture diagrams.".into(),
            author: Some("Anthropic".into()),
            version: Some("1.0.0".into()),
            homepage_url: Some("https://github.com/anthropics/skills".into()),
            license: Some("MIT".into()),
            stars: Some(3400),
            last_updated: Some(now - chrono::Duration::days(40)),
            source: CatalogSource::GitHubRepo { repo: "anthropics/skills".into(), ref_: Some("main".into()) },
            trust: TrustLevel::Official,
            metadata: HashMap::new(),
            tags: vec!["documentation".into(), "official".into(), "skill".into()],
        },
        CatalogEntry {
            id: "gh:obra/superpowers:dispatch".into(),
            kind: AddonKind::Skill,
            name: "Agent Dispatch Skill".into(),
            description: "Coordinate parallel agent execution with workload distribution and result aggregation.".into(),
            author: Some("Oh-My-Claudecode".into()),
            version: Some("0.5.0".into()),
            homepage_url: Some("https://github.com/obra/superpowers".into()),
            license: Some("Apache-2.0".into()),
            stars: Some(780),
            last_updated: Some(now - chrono::Duration::days(10)),
            source: CatalogSource::GitHubRepo { repo: "obra/superpowers".into(), ref_: Some("main".into()) },
            trust: TrustLevel::Community,
            metadata: HashMap::new(),
            tags: vec!["agent".into(), "dispatch".into(), "community".into()],
        },
        CatalogEntry {
            id: "gh:obra/superpowers:systematic-debugging".into(),
            kind: AddonKind::Skill,
            name: "Systematic Debugging".into(),
            description: "Root cause analysis workflow: reproduce → isolate → verify → document fix.".into(),
            author: Some("Oh-My-Claudecode".into()),
            version: Some("0.4.0".into()),
            homepage_url: Some("https://github.com/obra/superpowers".into()),
            license: Some("Apache-2.0".into()),
            stars: Some(650),
            last_updated: Some(now - chrono::Duration::days(35)),
            source: CatalogSource::GitHubRepo { repo: "obra/superpowers".into(), ref_: Some("main".into()) },
            trust: TrustLevel::Community,
            metadata: HashMap::new(),
            tags: vec!["debugging".into(), "workflow".into(), "community".into()],
        },

        // === Agents (4 entries) ===
        CatalogEntry {
            id: "gh:anthropic/agents:reviewer".into(),
            kind: AddonKind::Agent,
            name: "Code Reviewer Agent".into(),
            description: "Automated code review: security, performance, maintainability analysis with diff feedback.".into(),
            author: Some("Anthropic".into()),
            version: Some("2.1.0".into()),
            homepage_url: Some("https://github.com/anthropic/agents".into()),
            license: Some("MIT".into()),
            stars: Some(3800),
            last_updated: Some(now - chrono::Duration::days(18)),
            source: CatalogSource::GitHubRepo { repo: "anthropic/agents".into(), ref_: Some("main".into()) },
            trust: TrustLevel::Official,
            metadata: HashMap::new(),
            tags: vec!["review".into(), "official".into(), "agent".into()],
        },
        CatalogEntry {
            id: "gh:anthropic/agents:auditor".into(),
            kind: AddonKind::Agent,
            name: "Security Auditor Agent".into(),
            description: "Security-focused code analysis: vulnerability scanning, credential detection, permission checks.".into(),
            author: Some("Anthropic".into()),
            version: Some("1.8.0".into()),
            homepage_url: Some("https://github.com/anthropic/agents".into()),
            license: Some("MIT".into()),
            stars: Some(2900),
            last_updated: Some(now - chrono::Duration::days(22)),
            source: CatalogSource::GitHubRepo { repo: "anthropic/agents".into(), ref_: Some("main".into()) },
            trust: TrustLevel::Official,
            metadata: HashMap::new(),
            tags: vec!["security".into(), "audit".into(), "official".into(), "agent".into()],
        },
        CatalogEntry {
            id: "gh:rohitg00/claude-code-agents:frontend".into(),
            kind: AddonKind::Agent,
            name: "Frontend Specialist".into(),
            description: "React/Vue/Angular specialist: component design, state management, accessibility, performance.".into(),
            author: Some("Community".into()),
            version: Some("0.9.0".into()),
            homepage_url: Some("https://github.com/rohitg00/claude-code-agents".into()),
            license: Some("MIT".into()),
            stars: Some(920),
            last_updated: Some(now - chrono::Duration::days(12)),
            source: CatalogSource::GitHubRepo { repo: "rohitg00/claude-code-agents".into(), ref_: Some("main".into()) },
            trust: TrustLevel::Community,
            metadata: HashMap::new(),
            tags: vec!["frontend".into(), "react".into(), "community".into(), "agent".into()],
        },
        CatalogEntry {
            id: "gh:rohitg00/claude-code-agents:rust-dev".into(),
            kind: AddonKind::Agent,
            name: "Rust Development Agent".into(),
            description: "Cargo workflows, ownership patterns, async/await, unsafe code review, compilation fixes.".into(),
            author: Some("Community".into()),
            version: Some("0.8.0".into()),
            homepage_url: Some("https://github.com/rohitg00/claude-code-agents".into()),
            license: Some("MIT".into()),
            stars: Some(680),
            last_updated: Some(now - chrono::Duration::days(28)),
            source: CatalogSource::GitHubRepo { repo: "rohitg00/claude-code-agents".into(), ref_: Some("main".into()) },
            trust: TrustLevel::Community,
            metadata: HashMap::new(),
            tags: vec!["rust".into(), "development".into(), "community".into(), "agent".into()],
        },

        // === Data Sources (3 entries) ===
        CatalogEntry {
            id: "native:data-source-obsidian-vault".into(),
            kind: AddonKind::DataSource,
            name: "Obsidian Vault".into(),
            description: "Read markdown notes from a local Obsidian vault with attachment support.".into(),
            author: Some("Shannon".into()),
            version: Some(env!("CARGO_PKG_VERSION").into()),
            homepage_url: Some("https://obsidian.md".into()),
            license: Some("Apache-2.0".into()),
            stars: None,
            last_updated: None,
            source: CatalogSource::Native,
            trust: TrustLevel::Verified,
            metadata: HashMap::new(),
            tags: vec!["native".into(), "obsidian".into(), "markdown".into()],
        },
        CatalogEntry {
            id: "native:data-source-email-imap".into(),
            kind: AddonKind::DataSource,
            name: "Email (IMAP)".into(),
            description: "Connect to an IMAP server to read mailbox messages securely.".into(),
            author: Some("Shannon".into()),
            version: Some(env!("CARGO_PKG_VERSION").into()),
            homepage_url: None,
            license: Some("Apache-2.0".into()),
            stars: None,
            last_updated: None,
            source: CatalogSource::Native,
            trust: TrustLevel::Verified,
            metadata: HashMap::new(),
            tags: vec!["native".into(), "email".into(), "imap".into()],
        },
        CatalogEntry {
            id: "native:data-source-notion".into(),
            kind: AddonKind::DataSource,
            name: "Notion".into(),
            description: "Query Notion pages and databases via the REST API.".into(),
            author: Some("Shannon".into()),
            version: Some(env!("CARGO_PKG_VERSION").into()),
            homepage_url: Some("https://developers.notion.com/".into()),
            license: Some("Apache-2.0".into()),
            stars: None,
            last_updated: None,
            source: CatalogSource::Native,
            trust: TrustLevel::Verified,
            metadata: HashMap::new(),
            tags: vec!["native".into(), "notion".into(), "database".into()],
        },
    ]
}
/// List plugins available in the remote index (best-effort; network call).
#[tauri::command]
pub async fn list_plugin_marketplace(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    let registry = state.plugin_registry.read().await;
    let index = registry.create_index();
    let entries = index.all_entries();

    // Return fallback catalog when local registry is empty (first-run experience)
    if entries.is_empty() {
        return Ok(fallback_marketplace_catalog()
            .iter()
            .map(|e| serde_json::to_value(e).unwrap_or(serde_json::Value::Null))
            .collect());
    }
    Ok(entries
        .iter()
        .map(|e| serde_json::to_value(e).unwrap_or(serde_json::Value::Null))
        .collect())
}

/// One row in the catalog upstreams summary. Surfaced in the Extensions Hub
/// so the user can see which sources feed the marketplace and how many
/// entries each contributed — even when an upstream's manifest fetch fails
/// (in which case `entry_count` is 0 but the upstream is still visible).
#[derive(Debug, Clone, Serialize)]
pub struct CatalogUpstreamDto {
    /// "skill" | "agent" | "mcp" | "data_source"
    pub kind: String,
    /// Stable identifier (e.g. `"anthropics-official"`).
    pub slug: String,
    /// Display name for the chip.
    pub display_name: String,
    /// GitHub `owner/repo` when the upstream is a git repo, else `None`.
    pub repo: Option<String>,
    /// "verified" | "official" | "community" | "unknown"
    pub trust: String,
    /// How many entries from this upstream are currently in the marketplace.
    pub entry_count: usize,
}

/// List the federated catalog upstreams (skills, agents, MCP registry,
/// featured vendors, native). Pure static metadata — no network fetch. The
/// frontend correlates `entry_count` by querying the catalog commands
/// (`list_skill_catalog`, `list_agent_catalog`, `list_mcp_registry_servers`)
/// and matching entries back to upstreams via the `metadata.upstream` field
/// set in `skill_catalog::manifest_to_entry` / `agent_catalog`.
#[tauri::command]
pub async fn list_catalog_upstreams() -> Result<Vec<CatalogUpstreamDto>, String> {
    use crate::extensions::types::TrustLevel;
    fn trust_str(t: TrustLevel) -> &'static str {
        match t {
            TrustLevel::Verified => "verified",
            TrustLevel::Official => "official",
            TrustLevel::Community => "community",
            TrustLevel::Unknown => "unknown",
        }
    }

    let mut out: Vec<CatalogUpstreamDto> = Vec::new();

    for up in crate::extensions::skill_catalog::skill_upstreams() {
        out.push(CatalogUpstreamDto {
            kind: "skill".into(),
            slug: up.slug,
            display_name: up.display_name,
            repo: Some(up.repo),
            trust: trust_str(up.trust).into(),
            entry_count: 0,
        });
    }

    for up in crate::extensions::agent_catalog::agent_upstreams() {
        out.push(CatalogUpstreamDto {
            kind: "agent".into(),
            slug: up.slug,
            display_name: up.display_name,
            repo: Some(up.repo),
            trust: trust_str(up.trust).into(),
            entry_count: 0,
        });
    }

    out.push(CatalogUpstreamDto {
        kind: "mcp".into(),
        slug: "mcp-registry".into(),
        display_name: "MCP Registry".into(),
        repo: None,
        trust: "verified".into(),
        entry_count: 0,
    });
    out.push(CatalogUpstreamDto {
        kind: "mcp".into(),
        slug: "shannon-featured".into(),
        display_name: "Shannon Featured".into(),
        repo: None,
        trust: "verified".into(),
        entry_count: 0,
    });
    out.push(CatalogUpstreamDto {
        kind: "native".into(),
        slug: "native".into(),
        display_name: "Built-in".into(),
        repo: None,
        trust: "verified".into(),
        entry_count: 0,
    });

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::fallback_marketplace_catalog;

    #[test]
    fn fallback_marketplace_catalog_has_entries() {
        let catalog = fallback_marketplace_catalog();
        assert!(!catalog.is_empty(), "fallback catalog should have entries");
        assert!(
            catalog.len() >= 18,
            "fallback catalog should have at least 18 entries"
        );

        use crate::extensions::types::AddonKind;
        let kinds: std::collections::HashSet<AddonKind> = catalog.iter().map(|e| e.kind).collect();
        assert!(kinds.contains(&AddonKind::Mcp), "should have MCP entries");
        assert!(kinds.contains(&AddonKind::Skill), "should have Skill entries");
        assert!(kinds.contains(&AddonKind::Agent), "should have Agent entries");
        assert!(
            kinds.contains(&AddonKind::DataSource),
            "should have DataSource entries"
        );
    }

    #[test]
    fn fallback_marketplace_catalog_metadata_valid() {
        let catalog = fallback_marketplace_catalog();

        for entry in catalog {
            assert!(!entry.id.is_empty(), "entry should have non-empty id");
            assert!(!entry.name.is_empty(), "entry should have non-empty name");
            assert!(
                !entry.description.is_empty(),
                "entry should have non-empty description"
            );
            assert!(!entry.tags.is_empty(), "entry should have at least one tag");
            assert!(
                entry.stars.is_none() || entry.stars.unwrap() > 0,
                "stars should be positive if set"
            );

            match entry.trust {
                crate::extensions::types::TrustLevel::Unknown
                | crate::extensions::types::TrustLevel::Community
                | crate::extensions::types::TrustLevel::Official
                | crate::extensions::types::TrustLevel::Verified => {}
            }
        }
    }
}
