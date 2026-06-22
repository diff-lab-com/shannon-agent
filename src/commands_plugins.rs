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
/// Returns Plugin-bundle entries — i.e. GitHub repos that ship a
/// `.claude-plugin/marketplace.json` manifest bundling multiple skills,
/// agents, and MCP servers behind a single install button. Specialized
/// entries (skill-only / agent-only / MCP-only / data-source-only) are
/// surfaced by their dedicated tabs instead, so the Plugins tab no longer
/// duplicates them here.
pub(crate) fn fallback_marketplace_catalog() -> Vec<crate::extensions::CatalogEntry> {
    use crate::extensions::types::{AddonKind, CatalogEntry, CatalogSource, TrustLevel};
    use std::collections::HashMap;

    let now = chrono::Utc::now();

    /// Heuristic field set so the install dialog can route to the
    /// marketplace-bundle installer. Repos below all publish a
    /// `.claude-plugin/marketplace.json` at their root.
    fn bundle(
        name: &str,
        description: &str,
        repo: &str,
        trust: TrustLevel,
        stars: u64,
        tags: &[&str],
        now: chrono::DateTime<chrono::Utc>,
    ) -> CatalogEntry {
        let mut metadata = HashMap::new();
        metadata.insert(
            "marketplace_manifest".to_string(),
            serde_json::json!(format!(
                "https://github.com/{repo}/raw/main/.claude-plugin/marketplace.json"
            )),
        );
        CatalogEntry {
            id: format!("plugin-bundle:{repo}"),
            kind: AddonKind::Plugin,
            name: name.into(),
            description: description.into(),
            author: Some(repo.split('/').next().unwrap_or(repo).into()),
            version: Some("main".into()),
            homepage_url: Some(format!("https://github.com/{repo}")),
            license: Some("Apache-2.0".into()),
            stars: Some(stars),
            last_updated: Some(now),
            source: CatalogSource::GitHubRepo {
                repo: repo.into(),
                ref_: Some("main".into()),
            },
            trust,
            metadata,
            tags: tags.iter().map(|s| s.to_string()).collect(),
        }
    }

    vec![
        bundle(
            "Anthropic Skills Bundle",
            "Official Anthropic skill demos — bundling SKILL.md files for brainstorming, TDD, doc-gen, and more.",
            "anthropics/skills",
            TrustLevel::Verified,
            2400,
            &["bundle", "skills", "anthropic"],
            now,
        ),
        bundle(
            "Superpowers Collection",
            "Community-driven meta-skill pack: planning, debugging, refactor, and code-review playbooks.",
            "obra/superpowers",
            TrustLevel::Community,
            5100,
            &["bundle", "skills", "community"],
            now,
        ),
        bundle(
            "Awesome Claude Code Agents",
            "Curated agent definitions for code review, research, planning, and specialized workflows.",
            "VoltAgent/awesome-claude-code-agents",
            TrustLevel::Community,
            3300,
            &["bundle", "agents", "community"],
            now,
        ),
        bundle(
            "Claude Code Agents Pack",
            "rohitg00's collection of field-tested agents for shipping, testing, and review.",
            "rohitg00/claude-code-agents",
            TrustLevel::Community,
            1800,
            &["bundle", "agents", "community"],
            now,
        ),
        bundle(
            "Shannon Starter Pack",
            "Opinionated Shannon bundle: installs three native skills + two agents + the filesystem MCP server.",
            "shannon-agent/shannon-starter",
            TrustLevel::Official,
            120,
            &["bundle", "starter", "shannon"],
            now,
        ),
    ]
}
/// List plugin-bundle entries available in the remote index.
///
/// **Scope**: this command feeds the **Plugins tab**, which exists to
/// surface `.claude-plugin/marketplace.json` bundles — repos that ship
/// multiple skills/agents/MCP servers behind one install button. Rows
/// that represent a single specialized addon (skill-only, agent-only,
/// MCP-only, data-source-only) are intentionally filtered out; they
/// belong to their dedicated tabs (`list_skill_catalog`,
/// `list_agent_catalog`, `list_mcp_registry_servers`,
/// `list_data_source_catalog`) and showing them here would duplicate
/// those listings.
///
/// When the registry is empty or yields no bundles, the curated
/// `fallback_marketplace_catalog` ships so the tab is never blank.
#[tauri::command]
pub async fn list_plugin_marketplace(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    let registry = state.plugin_registry.read().await;
    let index = registry.create_index();
    let entries = index.all_entries();

    // Keep only marketplace-bundle rows. IndexEntry.plugin_type is a free-form
    // string; the specialized kinds ("skill", "agent", "mcp", "data_source",
    // "tool", "command") belong on their own tabs and are filtered out here.
    const BUNDLE_TYPES: &[&str] = &["plugin", "marketplace", "bundle"];
    let bundles: Vec<_> = entries
        .iter()
        .filter(|e| BUNDLE_TYPES.iter().any(|t| e.plugin_type == *t))
        .collect();

    if bundles.is_empty() {
        return Ok(fallback_marketplace_catalog()
            .iter()
            .map(|e| serde_json::to_value(e).unwrap_or(serde_json::Value::Null))
            .collect());
    }
    Ok(bundles
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
            catalog.len() >= 3,
            "fallback catalog should have at least 3 bundle entries"
        );

        use crate::extensions::types::AddonKind;
        // Plugins tab surfaces only marketplace bundles — no MCP/Skill/Agent/
        // DataSource rows (those live on their dedicated tabs). Asserting the
        // negative here guards against accidental regression when refreshing
        // the catalog.
        for entry in &catalog {
            assert_eq!(
                entry.kind,
                AddonKind::Plugin,
                "fallback entry '{}' should be Plugin-kind, got {:?}",
                entry.name,
                entry.kind
            );
            assert!(
                entry.metadata.contains_key("marketplace_manifest"),
                "bundle '{}' should carry a marketplace_manifest URL",
                entry.name
            );
        }
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
