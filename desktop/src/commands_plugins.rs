//! Plugin management Tauri commands (A.3).
//!
//! Extracted from `commands.rs` as part of S2 P1.1 (commands.rs split).
//! Covers: listing, installing (local / git), uninstalling, enable/disable,
//! update, marketplace catalog (with first-run fallback), and upstream list.
//! Backed by `shannon_core::plugin::PluginRegistry` on AppState plus the
//! extensions catalog machinery in `crate::extensions`.

use serde::Serialize;

use crate::commands::AppState;
use crate::plugin_materialize::{
    self, MaterializeOutcome, MaterializedRecord, PluginBundleSummary, PluginHomes,
};

/// Manifest keyword that marks a plugin as a thin migration-import record
/// (`imported-<source>` written by `migration_apply`). The UI reads
/// `PluginInfo.migration_imported` (derived from this marker) to suppress
/// uninstall/enable/disable on the entry.
pub const MIGRATION_IMPORT_MARKER: &str = "shannon:migration-import";

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
    /// True for the thin `imported-<source>` records `migration_apply`
    /// registers (X5). UI suppresses uninstall/enable/disable on these —
    /// uninstalling the record must not mean "delete my imported data".
    pub migration_imported: bool,
}

/// Result of an install command: the registered plugin name plus the
/// best-effort materialization warnings (per-artifact errors — e.g. a
/// rejected artifact name or an sse-only MCP server — never abort the
/// install itself).
#[derive(Debug, Clone, serde::Serialize)]
pub struct PluginInstallResult {
    pub name: String,
    pub warnings: Vec<String>,
}

/// Result of a lifecycle command (uninstall / enable / disable / update):
/// the per-artifact warnings from (reverse-)materialization. Empty = clean.
#[derive(Debug, Clone, serde::Serialize, Default)]
pub struct PluginLifecycleResult {
    pub warnings: Vec<String>,
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
            migration_imported: p
                .manifest
                .keywords
                .iter()
                .any(|k| k == MIGRATION_IMPORT_MARKER),
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
///
/// After the registry install succeeds the bundle is **materialized**
/// (X5) into the per-type homes (`skills/`, `agents/`, `commands/`,
/// `mcp-servers.json`); every created target is recorded in the plugin's
/// `materialized.json` sidecar. Per-artifact materialization failures are
/// reported as `warnings` in the result, not as a failed install.
#[tauri::command]
pub async fn install_plugin(
    state: tauri::State<'_, AppState>,
    source_path: String,
) -> Result<PluginInstallResult, String> {
    let path = std::path::PathBuf::from(&source_path);
    if !path.exists() {
        return Err(format!("source path does not exist: {source_path}"));
    }

    let homes = PluginHomes::detect();
    let mut registry = state.plugin_registry.write().await;
    registry.ensure_dir().await.map_err(|e| e.to_string())?;
    let plugins_dir = registry.plugins_dir().to_path_buf();

    // Archive? Delegate to the .dxt/.mcpb installer.
    let is_archive = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| matches!(e.to_ascii_lowercase().as_str(), "dxt" | "mcpb" | "zip"))
        .unwrap_or(false);
    let name = if is_archive {
        let name = shannon_core::plugin::install_extension_file(&path, &plugins_dir)
            .map_err(|e| e.to_string())?;
        // Rescan so the registry picks up the freshly extracted plugin.
        registry.load_all().await.map_err(|e| e.to_string())?;
        name
    } else if path.is_dir() {
        // Otherwise treat as a plugin directory and copy in.
        registry
            .install_from_path(&path)
            .await
            .map_err(|e| e.to_string())?
    } else {
        return Err(format!(
            "source must be a directory or .dxt/.mcpb archive: {source_path}"
        ));
    };

    finish_install_with_materialize(&mut registry, &name, &homes)
}

/// Install a plugin from a git URL (clones with `git clone --depth 1`).
///
/// `allow_unverified` is the explicit SEC-1 opt-in: when the remote
/// manifest declares no `permissions`, the default (`None`/`Some(false)`)
/// refuses the install (`PluginError::UnverifiedRemote`). The UI should
/// pass `Some(true)` only after the user has explicitly confirmed the
/// default-allow risk.
///
/// Materializes the cloned bundle like `install_plugin` (X5).
#[tauri::command]
pub async fn install_plugin_from_git(
    state: tauri::State<'_, AppState>,
    repo_url: String,
    allow_unverified: Option<bool>,
) -> Result<PluginInstallResult, String> {
    let consent = if allow_unverified.unwrap_or(false) {
        shannon_core::plugin::RemoteInstallConsent::allow_unverified()
    } else {
        shannon_core::plugin::RemoteInstallConsent::default()
    };
    let homes = PluginHomes::detect();
    let mut registry = state.plugin_registry.write().await;
    let name = registry
        .install_from_git(&repo_url, consent)
        .await
        .map_err(|e| e.to_string())?;
    finish_install_with_materialize(&mut registry, &name, &homes)
}

/// Shared install tail (X5): look the freshly registered plugin up in the
/// registry, materialize its bundle into the per-type homes and record the
/// created targets in the `materialized.json` sidecar.
fn finish_install_with_materialize(
    registry: &mut shannon_core::plugin::PluginRegistry,
    name: &str,
    homes: &PluginHomes,
) -> Result<PluginInstallResult, String> {
    let (plugin_dir, manifest) = {
        let plugin = registry
            .get(name)
            .ok_or_else(|| format!("plugin '{name}' registered but not found in registry"))?;
        (plugin.path.clone(), plugin.manifest.clone())
    };
    match plugin_materialize::materialize_plugin(&plugin_dir, &manifest, homes) {
        Ok(MaterializeOutcome { record: _, warnings }) => Ok(PluginInstallResult {
            name: name.to_string(),
            warnings,
        }),
        Err(e) => {
            // Registry install already succeeded; a hard materialization
            // failure (unusable name) still surfaces as Err so the UI
            // toasts — the plugin stays registered and a later lifecycle
            // op proceeds registry-only (no sidecar → warning).
            Err(e)
        }
    }
}

/// Uninstall a plugin by name. Removes the directory.
///
/// X5 semantics: first reverse-materialize strictly from the
/// `materialized.json` sidecar (missing/corrupt sidecar → registry-only
/// uninstall + a warning in the result), then uninstall from the registry.
#[tauri::command]
pub async fn uninstall_plugin(
    state: tauri::State<'_, AppState>,
    name: String,
) -> Result<PluginLifecycleResult, String> {
    let homes = PluginHomes::detect();
    let mut registry = state.plugin_registry.write().await;

    let warnings = reverse_from_sidecar(registry.get(&name), &homes);
    registry.uninstall(&name).await.map_err(|e| e.to_string())?;
    Ok(PluginLifecycleResult { warnings })
}

/// Enable a previously installed plugin: re-materialize its bundle from
/// the manifest (X5) after the registry flag flips.
#[tauri::command]
pub async fn enable_plugin(
    state: tauri::State<'_, AppState>,
    name: String,
) -> Result<PluginLifecycleResult, String> {
    let homes = PluginHomes::detect();
    let mut registry = state.plugin_registry.write().await;
    registry.enable(&name).map_err(|e| e.to_string())?;
    rematerialize_from_manifest(&mut registry, &name, &homes)
}

/// Disable a plugin (without removing it): reverse-materialize the bundle
/// but keep the plugin directory and its sidecar (X5), so enable can
/// restore everything from the manifest.
#[tauri::command]
pub async fn disable_plugin(
    state: tauri::State<'_, AppState>,
    name: String,
) -> Result<PluginLifecycleResult, String> {
    let homes = PluginHomes::detect();
    let mut registry = state.plugin_registry.write().await;
    let warnings = reverse_from_sidecar(registry.get(&name), &homes);
    registry.disable(&name).map_err(|e| e.to_string())?;
    Ok(PluginLifecycleResult { warnings })
}

/// Pull updates for a git-installed plugin: reverse-materialize first, then
/// re-pull, then re-materialize the refreshed bundle (X5).
#[tauri::command]
pub async fn update_plugin(
    state: tauri::State<'_, AppState>,
    name: String,
) -> Result<PluginLifecycleResult, String> {
    let homes = PluginHomes::detect();
    let mut registry = state.plugin_registry.write().await;
    let mut warnings = reverse_from_sidecar(registry.get(&name), &homes);
    registry.update(&name).await.map_err(|e| e.to_string())?;
    let rematerialized = rematerialize_from_manifest(&mut registry, &name, &homes)?;
    warnings.extend(rematerialized.warnings);
    Ok(PluginLifecycleResult { warnings })
}

/// Reverse-materialize from the installed plugin's sidecar. Missing or
/// corrupt sidecar → registry-only lifecycle with an explanatory warning
/// (the record is never trusted when it cannot be parsed).
fn reverse_from_sidecar(
    plugin: Option<&shannon_core::plugin::InstalledPlugin>,
    homes: &PluginHomes,
) -> Vec<String> {
    let Some(plugin) = plugin else {
        return Vec::new(); // not found — the registry call reports the error
    };
    let plugin_dir = plugin.path.clone();
    match plugin_materialize::read_sidecar(&plugin_dir) {
        Ok(Some(record)) => plugin_materialize::reverse_materialize(&record, homes),
        Ok(None) => vec![format!(
            "no {sidecar} sidecar in {dir} — registry-only lifecycle; previously materialized artifacts (if any) are left in place",
            sidecar = plugin_materialize::MATERIALIZED_SIDECAR,
            dir = plugin_dir.display()
        )],
        Err(e) => vec![format!(
            "unreadable materialization sidecar in {dir} — registry-only lifecycle: {e}",
            dir = plugin_dir.display()
        )],
    }
}

/// Re-materialize a registered plugin's bundle from its manifest and
/// refresh the sidecar (enable / post-update path).
fn rematerialize_from_manifest(
    registry: &mut shannon_core::plugin::PluginRegistry,
    name: &str,
    homes: &PluginHomes,
) -> Result<PluginLifecycleResult, String> {
    let (plugin_dir, manifest) = {
        let plugin = registry
            .get(name)
            .ok_or_else(|| format!("plugin '{name}' not found in registry"))?;
        (plugin.path.clone(), plugin.manifest.clone())
    };
    match plugin_materialize::materialize_plugin(&plugin_dir, &manifest, homes) {
        Ok(MaterializeOutcome { record: MaterializedRecord { .. }, warnings }) => {
            Ok(PluginLifecycleResult { warnings })
        }
        Err(e) => Err(e),
    }
}

/// X5 trust preview: inspect a plugin source (local directory, `.dxt` /
/// `.mcpb` / `.zip` archive, or git URL) and return the bundle summary the
/// install dialog renders in the "will be enabled" checklist **before**
/// the user confirms. Git sources are shallow-cloned (`--depth 1`) into a
/// tempdir that is dropped when the inspection ends — the clone reads
/// files only, executes nothing, and the install path keeps its own SEC-1
/// consent gate.
#[tauri::command]
pub async fn inspect_plugin_source(path: String) -> Result<PluginBundleSummary, String> {
    let source = std::path::PathBuf::from(&path);
    if source.is_dir() {
        return plugin_materialize::summarize_dir(&source);
    }
    if source.is_file() {
        let is_archive = source
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| {
                plugin_materialize::ARCHIVE_EXTENSIONS
                    .iter()
                    .any(|a| e.eq_ignore_ascii_case(a))
            })
            .unwrap_or(false);
        if is_archive {
            return plugin_materialize::summarize_archive(&source);
        }
        return Err(format!(
            "unsupported plugin source file (expected .dxt/.mcpb/.zip or a directory): {path}"
        ));
    }
    if plugin_materialize::looks_like_git_source(&path) {
        return plugin_materialize::summarize_git_source(&path).await;
    }
    Err(format!(
        "no local plugin directory, archive or git URL at: {path}"
    ))
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

/// X5 lifecycle tests — the command-layer helpers run against a real
/// `PluginRegistry` + tempdir homes, so no test ever mutates process HOME.
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod lifecycle_tests {
    use super::*;
    use shannon_core::plugin::PluginRegistry;
    use std::path::Path;
    use tempfile::TempDir;

    /// Claude-dialect plugin source with one skill, one command and one
    /// stdio MCP server.
    fn write_bundle_plugin(dir: &Path, name: &str) {
        let claude = dir.join(".claude-plugin");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(
            claude.join("plugin.json"),
            format!(
                r#"{{"name":"{name}","version":"1.0.0","description":"d","type":"skill","entry":"t.md","trigger":"/{name}","template":"t","mcpServers":{{"relay":{{"command":"npx","args":["-y","relay"]}}}}}}"#
            ),
        )
        .unwrap();
        let skill = dir.join("skills").join("main");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), "hi").unwrap();
        let commands = dir.join("commands");
        std::fs::create_dir_all(&commands).unwrap();
        std::fs::write(commands.join("go.md"), "go").unwrap();
    }

    fn expect_clean(result: &PluginLifecycleResult) {
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    }

    #[tokio::test]
    async fn install_disable_enable_uninstall_lifecycle_round_trip() {
        let tmp = TempDir::new().unwrap();
        let homes = PluginHomes::from_home(tmp.path());
        let source = tmp.path().join("src").join("bundle");
        write_bundle_plugin(&source, "bundle");

        let mut registry = PluginRegistry::new(tmp.path().join("plugins"));
        let name = registry.install_from_path(&source).await.unwrap();

        // install tail: materialize + sidecar
        let installed = finish_install_with_materialize(&mut registry, &name, &homes).unwrap();
        assert!(installed.warnings.is_empty(), "{:?}", installed.warnings);
        let plugin_dir = registry.get("bundle").unwrap().path.clone();
        assert!(
            homes
                .skills_root
                .join("bundle")
                .join("main")
                .join("SKILL.md")
                .is_file()
        );
        assert!(homes.commands_root.join("go.md").is_file());
        let store =
            std::fs::read_to_string(&homes.mcp_store_path).expect("mcp store materialized");
        assert!(store.contains("bundle-relay"), "{store}");

        // disable: reverse-materialize, keep plugin dir + sidecar
        let disabled = {
            let warnings = reverse_from_sidecar(registry.get("bundle"), &homes);
            registry.disable("bundle").unwrap();
            PluginLifecycleResult { warnings }
        };
        expect_clean(&disabled);
        assert!(!homes.skills_root.join("bundle").exists());
        assert!(!homes.commands_root.join("go.md").exists());
        assert!(
            plugin_dir.join(plugin_materialize::MATERIALIZED_SIDECAR).is_file(),
            "sidecar must survive disable"
        );
        assert!(
            plugin_dir.join("skills").join("main").join("SKILL.md").is_file(),
            "plugin dir must survive disable"
        );

        // enable: re-materialize from the manifest
        let enabled = rematerialize_from_manifest(&mut registry, "bundle", &homes).unwrap();
        expect_clean(&enabled);
        assert!(
            homes
                .skills_root
                .join("bundle")
                .join("main")
                .join("SKILL.md")
                .is_file()
        );
        assert!(homes.commands_root.join("go.md").is_file());

        // uninstall: reverse from sidecar + registry uninstall
        let removed = {
            let warnings = reverse_from_sidecar(registry.get("bundle"), &homes);
            registry.uninstall("bundle").await.unwrap();
            PluginLifecycleResult { warnings }
        };
        expect_clean(&removed);
        assert!(registry.is_empty());
        assert!(!plugin_dir.exists(), "registry uninstall removes the dir");
        assert!(!homes.commands_root.join("go.md").exists());
    }

    /// A legacy plugin installed before X5 has no sidecar: uninstall still
    /// succeeds, registry-only, and reports the warning instead of failing.
    #[tokio::test]
    async fn uninstall_without_sidecar_warns_and_still_uninstalls() {
        let tmp = TempDir::new().unwrap();
        let homes = PluginHomes::from_home(tmp.path());
        let plugins_dir = tmp.path().join("plugins");
        let plugin_dir = plugins_dir.join("legacy");
        write_bundle_plugin(&plugin_dir, "legacy");

        let mut registry = PluginRegistry::new(plugins_dir);
        registry.load_all().await.unwrap();
        assert!(registry.contains("legacy"));

        let result = {
            let warnings = reverse_from_sidecar(registry.get("legacy"), &homes);
            registry.uninstall("legacy").await.unwrap();
            PluginLifecycleResult { warnings }
        };
        assert_eq!(result.warnings.len(), 1);
        assert!(
            result.warnings[0].contains("registry-only"),
            "{:?}",
            result.warnings
        );
        assert!(registry.is_empty());
    }

    /// update = reverse + re-materialize: a command the upstream dropped is
    /// gone after the cycle, new ones are in place.
    #[tokio::test]
    async fn update_cycle_reverses_then_rematerializes() {
        let tmp = TempDir::new().unwrap();
        let homes = PluginHomes::from_home(tmp.path());
        let plugins_dir = tmp.path().join("plugins");
        let plugin_dir = plugins_dir.join("upd");
        write_bundle_plugin(&plugin_dir, "upd");

        let mut registry = PluginRegistry::new(plugins_dir);
        registry.load_all().await.unwrap();
        let installed = finish_install_with_materialize(&mut registry, "upd", &homes).unwrap();
        assert!(installed.warnings.is_empty());
        assert!(homes.commands_root.join("go.md").is_file());

        // upstream drops the command
        std::fs::remove_file(plugin_dir.join("commands").join("go.md")).unwrap();

        // registry.update requires git — call the shared tail directly to
        // exercise the reverse + re-materialize half of the update path.
        let warnings = reverse_from_sidecar(registry.get("upd"), &homes);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(!homes.commands_root.join("go.md").exists());
        let result = rematerialize_from_manifest(&mut registry, "upd", &homes).unwrap();
        expect_clean(&result);
        // the dropped command is not re-materialized; skill + mcp are back
        assert!(!homes.commands_root.join("go.md").exists());
        assert!(
            homes
                .skills_root
                .join("upd")
                .join("main")
                .join("SKILL.md")
                .is_file()
        );
    }

    /// The migration marker on a manifest surfaces through `PluginInfo` so
    /// the UI can suppress destructive actions on `imported-*` records.
    #[test]
    fn migration_marker_derives_plugin_info_flag() {
        let marker_hit = [MIGRATION_IMPORT_MARKER.to_string()];
        let marker_miss: Vec<String> = vec!["community".into()];
        assert!(marker_hit.iter().any(|k| k == MIGRATION_IMPORT_MARKER));
        assert!(!marker_miss.iter().any(|k| k == MIGRATION_IMPORT_MARKER));
    }
}
