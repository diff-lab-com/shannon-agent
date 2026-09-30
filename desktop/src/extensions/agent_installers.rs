//! P4 Agent installers.
//!
//! Two installers handle different agent entry shapes:
//! - `AgentRepoInstaller` — clones a repo with a `.claude/agents/*.md` style
//!   collection into `~/.shannon/agents/<plugin>/`.
//! - `AgentMarkdownInstaller` — installs a single agent as a **flat**
//!   `~/.shannon/agents/<name>.toml` [`AgentDefinition`].
//!
//! G1 P1-9 format unification: the runtime loader
//! (`shannon_agents::AgentDefinitionRegistry::load_from_dirs`) only reads
//! flat `~/.shannon/agents/*.toml` (plus the Claude-compatible md dirs) —
//! the old `<plugin>/agent.md` subdirectory shape was never loaded, so
//! desktop-installed agents were invisible at runtime. New installs write
//! the flat TOML shape; `migrate_legacy_agent_dirs` converts old
//! subdirectory entries at startup (idempotent).

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::installer::{AddonInstaller, InstallError, safe_plugin_name};
use super::types::{
    AddonKind, CatalogEntry, CatalogSource, ConfirmationLevel, InstallTarget, InstalledAddon,
    ProgressSink, TrustLevel,
};

/// Where agent definitions live. Today: `~/.shannon/agents/` (flat `.toml`
/// files the runtime loader reads; legacy subdirectories are migrated).
fn shannon_agents_root() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".shannon").join("agents"))
        .unwrap_or_else(|| PathBuf::from("/tmp/shannon-agents"))
}

/// Resolve the agents root, honoring an installer's test-only `root_override`.
/// `None` → the real `~/.shannon/agents` (production); `Some(p)` → `p` (tests
/// pass a tempdir so they never mutate the process-global `HOME` env var).
fn resolve_agents_root(override_: Option<&Path>) -> PathBuf {
    override_
        .map(PathBuf::from)
        .unwrap_or_else(shannon_agents_root)
}

/// Sidecar written into a repo-installed plugin directory listing the flat
/// `<root>/<name>.toml` files the install materialized (G1 fix round 1,
/// Imp-4a), so the uninstall path removes exactly those and nothing else.
const FLAT_AGENTS_SIDECAR: &str = ".shannon-flat-agents.json";

/// Repo-based agent installer — clones into `~/.shannon/agents/<plugin>/`.
pub struct AgentRepoInstaller {
    pub plugin_name: String,
    pub repo: String,
    pub ref_: String,
    /// Test-only override for the agents root. Production leaves this `None`
    /// (resolve `~/.shannon/agents` from HOME); tests set it to a tempdir.
    pub root_override: Option<PathBuf>,
}

#[async_trait]
impl AddonInstaller for AgentRepoInstaller {
    fn kind(&self) -> AddonKind {
        AddonKind::Agent
    }

    fn supports(&self, entry: &CatalogEntry) -> bool {
        matches!(entry.source, CatalogSource::GitHubRepo { .. }) && entry.kind == AddonKind::Agent
    }

    async fn install(
        &self,
        entry: &CatalogEntry,
        _target: &InstallTarget,
        progress: &ProgressSink,
    ) -> Result<InstalledAddon, InstallError> {
        progress
            .emit(super::types::ProgressEvent::Started {
                total_steps: Some(3),
            })
            .await;
        progress
            .emit(super::types::ProgressEvent::Step {
                description: format!("Cloning {}", self.repo),
                current: Some(1),
                total: Some(3),
            })
            .await;

        // B0 P0-6: the name comes from the upstream catalog — sanitize it
        // before it touches the filesystem (Path::join escapes the root for
        // absolute paths / `..`).
        let plugin = safe_plugin_name(&self.plugin_name)?;
        let target_dir = resolve_agents_root(self.root_override.as_deref()).join(&plugin);
        if target_dir.exists() {
            return Err(InstallError::Io(format!(
                "{} already exists at {}",
                self.plugin_name,
                target_dir.display()
            )));
        }

        std::fs::create_dir_all(target_dir.parent().unwrap_or(Path::new("/")))
            .map_err(|e| InstallError::Io(e.to_string()))?;

        let url = format!("https://github.com/{}.git", self.repo);
        let output = tokio::process::Command::new("git")
            .arg("clone")
            .arg("--depth")
            .arg("1")
            .arg("--branch")
            .arg(&self.ref_)
            .arg(&url)
            .arg(&target_dir)
            .output()
            .await
            .map_err(|e| InstallError::Io(format!("git clone spawn: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(InstallError::Io(format!("git clone failed: {stderr}")));
        }

        progress
            .emit(super::types::ProgressEvent::Step {
                description: "Validating agent files".into(),
                current: Some(2),
                total: Some(3),
            })
            .await;
        // Verify there's at least one .md agent file or a shannon-agents.json.
        let agents_dir = target_dir.join(".claude").join("agents");
        let manifest = target_dir.join("shannon-agents.json");
        let has_agent_md = agents_dir
            .read_dir()
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .any(|e| e.path().extension().is_some_and(|x| x == "md"))
            })
            .unwrap_or(false);
        if !has_agent_md && !manifest.exists() {
            let _ = std::fs::remove_dir_all(&target_dir);
            return Err(InstallError::Format(format!(
                "repo {repo} has no .claude/agents/*.md or shannon-agents.json",
                repo = self.repo
            )));
        }

        // G1 fix round 1 (Imp-4a): the runtime loader only reads FLAT
        // `<root>/<name>.toml` definitions — a cloned collection alone would
        // stay invisible. Materialize every collected agent as a flat
        // `<plugin>-<agent>.toml` (same native shape, system_prompt mapped),
        // recording the file names in a sidecar so uninstall removes exactly
        // what this install wrote.
        let root = resolve_agents_root(self.root_override.as_deref());
        let mut flat_files = Vec::new();
        if has_agent_md {
            if let Ok(rd) = agents_dir.read_dir() {
                for file in rd.flatten() {
                    let path = file.path();
                    if path.extension().is_none_or(|x| x != "md") {
                        continue;
                    }
                    let Some(agent_name) = path.file_stem().and_then(|s| s.to_str()) else {
                        continue;
                    };
                    let Ok(def) = shannon_agents::AgentDefinition::from_markdown_file(&path) else {
                        tracing::warn!(
                            path = %path.display(),
                            "repo agent.md unreadable — skipping"
                        );
                        continue;
                    };
                    if write_flat_agent_toml(
                        &root,
                        &plugin,
                        agent_name,
                        &if def.description.is_empty() {
                            agent_name.to_string()
                        } else {
                            def.description
                        },
                        def.system_prompt.as_deref().unwrap_or(""),
                        def.model.as_deref(),
                        &def.capabilities,
                    ) {
                        flat_files.push(format!("{plugin}-{agent_name}.toml"));
                    }
                }
            }
        }
        if manifest.exists() {
            match std::fs::read_to_string(&manifest)
                .map_err(|e| InstallError::Io(e.to_string()))
                .and_then(|text| {
                    serde_json::from_str::<super::agent_catalog::AgentManifest>(&text)
                        .map_err(|e| InstallError::Format(format!("shannon-agents.json: {e}")))
                }) {
                Ok(manifest) => {
                    for agent in manifest.agents {
                        if write_flat_agent_toml(
                            &root,
                            &plugin,
                            &agent.name,
                            &agent.description,
                            agent.system_prompt.as_deref().unwrap_or(""),
                            agent.model.as_deref(),
                            &agent.tools,
                        ) {
                            flat_files.push(format!("{plugin}-{}.toml", agent.name));
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "shannon-agents.json unreadable — skipping manifest agents")
                }
            }
        }
        if !flat_files.is_empty() {
            let sidecar = json_sidecar(&flat_files);
            let _ = std::fs::write(target_dir.join(FLAT_AGENTS_SIDECAR), sidecar);
            tracing::info!(
                plugin = %plugin,
                agents = flat_files.len(),
                "materialized repo agents as flat definitions"
            );
        }

        progress.emit(super::types::ProgressEvent::Finished).await;

        Ok(InstalledAddon {
            id: entry.id.clone(),
            kind: entry.kind,
            name: plugin.clone(),
            install_path: Some(target_dir.display().to_string()),
            installed_at: Some(Utc::now()),
            version: entry.version.clone(),
            enabled: true,
        })
    }

    async fn uninstall(&self, addon_id: &str) -> Result<(), InstallError> {
        // Shared path: removes the cloned dir AND the flat tomls this
        // install materialized (recorded in the sidecar).
        remove_installed_agent_in(
            &resolve_agents_root(self.root_override.as_deref()),
            addon_id,
        )
    }

    async fn update(&self, addon_id: &str) -> Result<InstalledAddon, InstallError> {
        let dir = resolve_agents_root(self.root_override.as_deref()).join(addon_id);
        if !dir.exists() {
            return Err(InstallError::Io(format!(
                "{addon_id} is not installed at {}",
                dir.display()
            )));
        }
        let output = tokio::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .arg("pull")
            .arg("--ff-only")
            .output()
            .await
            .map_err(|e| InstallError::Io(format!("git pull spawn: {e}")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(InstallError::Io(format!("git pull failed: {stderr}")));
        }
        Ok(InstalledAddon {
            id: addon_id.to_string(),
            kind: AddonKind::Agent,
            name: addon_id.to_string(),
            install_path: Some(dir.display().to_string()),
            installed_at: Some(Utc::now()),
            version: None,
            enabled: true,
        })
    }

    fn requires_confirmation(&self, entry: &CatalogEntry) -> ConfirmationLevel {
        match entry.trust {
            TrustLevel::Verified => ConfirmationLevel::None,
            _ => ConfirmationLevel::Review,
        }
    }
}

/// Single-agent installer — writes a **flat** `~/.shannon/agents/<name>.toml`
/// [`shannon_agents::AgentDefinition`] the runtime loader actually reads.
///
/// The catalog page's `description` / `system_prompt` semantics map onto the
/// definition fields; catalog tool hints become capabilities (freeform), so
/// the agent keeps the default all-tools surface instead of a mismatched
/// lowercase allowlist that would filter to nothing.
pub struct AgentMarkdownInstaller {
    pub plugin_name: String,
    pub description: String,
    pub system_prompt: String,
    pub model: Option<String>,
    pub tools: Vec<String>,
    /// Test-only override for the agents root. Production leaves this `None`
    /// (resolve `~/.shannon/agents` from HOME); tests set it to a tempdir.
    pub root_override: Option<PathBuf>,
}

/// Serialize the definition into the flat TOML shape
/// `AgentDefinition::from_file` parses. Written by hand (no new deps) with
/// basic TOML string escaping for the prompt/description values.
fn agent_definition_toml(
    name: &str,
    description: &str,
    system_prompt: &str,
    model: Option<&str>,
    tools: &[String],
) -> String {
    fn toml_str(value: &str) -> String {
        let mut out = String::with_capacity(value.len() + 2);
        out.push('"');
        for ch in value.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c => out.push(c),
            }
        }
        out.push('"');
        out
    }

    let mut toml = String::new();
    toml.push_str(&format!("name = {}\n", toml_str(name)));
    toml.push_str(&format!("description = {}\n", toml_str(description)));
    toml.push_str(&format!("system_prompt = {}\n", toml_str(system_prompt)));
    if let Some(model) = model {
        toml.push_str(&format!("model = {}\n", toml_str(model)));
    }
    let capabilities: Vec<String> = tools.iter().map(|t| toml_str(t)).collect();
    if !capabilities.is_empty() {
        toml.push_str(&format!("capabilities = [{}]\n", capabilities.join(", ")));
    }
    toml
}

#[async_trait]
impl AddonInstaller for AgentMarkdownInstaller {
    fn kind(&self) -> AddonKind {
        AddonKind::Agent
    }

    fn supports(&self, entry: &CatalogEntry) -> bool {
        entry.kind == AddonKind::Agent
            && matches!(
                entry.source,
                CatalogSource::Native | CatalogSource::Custom { .. }
            )
    }

    async fn install(
        &self,
        entry: &CatalogEntry,
        _target: &InstallTarget,
        progress: &ProgressSink,
    ) -> Result<InstalledAddon, InstallError> {
        progress
            .emit(super::types::ProgressEvent::Started {
                total_steps: Some(2),
            })
            .await;

        // B0 P0-6: same sanitization as the repo installer — a polluted
        // native entry must not escape the agents root either.
        let plugin = safe_plugin_name(&self.plugin_name)?;
        let root = resolve_agents_root(self.root_override.as_deref());
        std::fs::create_dir_all(&root)?;
        let agent_toml = root.join(format!("{plugin}.toml"));
        let body = agent_definition_toml(
            &plugin,
            &self.description,
            &self.system_prompt,
            self.model.as_deref(),
            &self.tools,
        );
        std::fs::write(&agent_toml, body)?;

        progress.emit(super::types::ProgressEvent::Finished).await;

        Ok(InstalledAddon {
            id: entry.id.clone(),
            kind: entry.kind,
            name: plugin.clone(),
            install_path: Some(agent_toml.display().to_string()),
            installed_at: Some(Utc::now()),
            version: entry.version.clone(),
            enabled: true,
        })
    }

    async fn uninstall(&self, addon_id: &str) -> Result<(), InstallError> {
        let root = resolve_agents_root(self.root_override.as_deref());
        let toml_path = root.join(format!("{addon_id}.toml"));
        if toml_path.exists() {
            std::fs::remove_file(&toml_path)?;
        }
        // Legacy subdirectory installs (pre-G1 shape) are cleaned too.
        let legacy_dir = root.join(addon_id);
        if legacy_dir.is_dir() {
            std::fs::remove_dir_all(&legacy_dir)?;
        }
        Ok(())
    }

    async fn update(&self, _addon_id: &str) -> Result<InstalledAddon, InstallError> {
        Err(InstallError::Unsupported(
            "AgentMarkdownInstaller has no upstream; cannot update".into(),
        ))
    }

    fn requires_confirmation(&self, _entry: &CatalogEntry) -> ConfirmationLevel {
        ConfirmationLevel::None
    }
}

/// Used by the Tauri command layer to ask "is this agent plugin already installed?"
pub fn is_agent_installed(plugin_name: &str) -> bool {
    is_agent_installed_in(&shannon_agents_root(), plugin_name)
}

/// `is_agent_installed` against an explicit agents `root` (see
/// [`AgentRepoInstaller`] / [`AgentMarkdownInstaller`] `root_override` for why
/// tests avoid `$HOME`).
///
/// True for both the flat `<name>.toml` shape (G1) and a legacy `<name>/`
/// subdirectory (unmigrated repo install).
pub fn is_agent_installed_in(root: &Path, plugin_name: &str) -> bool {
    root.join(format!("{plugin_name}.toml")).is_file() || root.join(plugin_name).is_dir()
}

/// Wire type for listing installed agent plugins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledAgent {
    pub name: String,
    pub path: String,
    pub installed_at: Option<String>,
}

/// Scan `~/.shannon/agents/` for installed agent plugins.
pub fn list_installed_agents() -> Vec<InstalledAgent> {
    list_installed_agents_in(&shannon_agents_root())
}

/// `list_installed_agents` against an explicit agents `root`. Sees both the
/// flat `.toml` files (G1 shape) and legacy plugin subdirectories.
pub fn list_installed_agents_in(root: &Path) -> Vec<InstalledAgent> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return out;
    };
    for entry in entries.flatten() {
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let is_toml = entry.path().extension().is_some_and(|x| x == "toml");
        if !is_dir && !is_toml {
            continue;
        }
        let path = entry.path();
        let installed_at = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| {
                DateTime::<Utc>::from_timestamp(d.as_secs() as i64, 0)
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_default()
            });
        out.push(InstalledAgent {
            name: entry
                .path()
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| entry.file_name().to_string_lossy().into_owned()),
            path: path.display().to_string(),
            installed_at,
        });
    }
    out
}

/// Remove an installed agent plugin by name.
pub fn remove_installed_agent(name: &str) -> Result<(), InstallError> {
    remove_installed_agent_in(&shannon_agents_root(), name)
}

/// Write one flat `<root>/<plugin>-<agent>.toml` definition (G1 Imp-4a).
/// Existing flat files win (idempotent); unsafe agent names are rejected.
/// Returns `true` when a file was written.
fn write_flat_agent_toml(
    root: &Path,
    plugin: &str,
    agent_name: &str,
    description: &str,
    system_prompt: &str,
    model: Option<&str>,
    tools: &[String],
) -> bool {
    let agent_slug = match safe_plugin_name(agent_name) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(agent = %agent_name, error = %e, "unsafe repo agent name — skipping");
            return false;
        }
    };
    let flat_name = format!("{plugin}-{agent_slug}");
    let target = root.join(format!("{flat_name}.toml"));
    if target.exists() {
        return false; // existing definition wins
    }
    let body = agent_definition_toml(&flat_name, description, system_prompt, model, tools);
    if let Err(e) = std::fs::write(&target, body) {
        tracing::warn!(path = %target.display(), error = %e, "flat agent write failed");
        return false;
    }
    true
}

/// Serialize the sidecar listing (`Vec<String>` of file names, basename
/// only).
fn json_sidecar(files: &[String]) -> String {
    serde_json::to_string_pretty(files).unwrap_or_else(|_| "[]".to_string())
}

/// Remove the flat tomls a repo install recorded in its sidecar, best
/// effort. Entries are reduced to file basenames joined under `root`, so a
/// tampered sidecar cannot reach outside the agents root.
fn remove_sidecar_flat_agents(root: &Path, dir: &Path) {
    let Ok(text) = std::fs::read_to_string(dir.join(FLAT_AGENTS_SIDECAR)) else {
        return;
    };
    let Ok(files) = serde_json::from_str::<Vec<String>>(&text) else {
        tracing::warn!(dir = %dir.display(), "flat-agents sidecar unreadable — leaving flat files");
        return;
    };
    for file in files {
        let Some(file_name) = Path::new(&file).file_name() else {
            continue;
        };
        let target = root.join(file_name);
        if target.is_file() {
            if let Err(e) = std::fs::remove_file(&target) {
                tracing::warn!(path = %target.display(), error = %e, "flat agent removal failed");
            }
        }
    }
}

/// `remove_installed_agent` against an explicit agents `root`. Removes the
/// flat `<name>.toml` and/or the legacy `<name>/` subdirectory (including
/// the flat files a repo install recorded in its sidecar); an unknown name
/// is an error.
pub fn remove_installed_agent_in(root: &Path, name: &str) -> Result<(), InstallError> {
    let toml_path = root.join(format!("{name}.toml"));
    let dir = root.join(name);
    if !toml_path.exists() && !dir.exists() {
        return Err(InstallError::Io(format!("{name} is not installed")));
    }
    let canonical_root = root
        .canonicalize()
        .map_err(|e| InstallError::Io(format!("canonicalize root: {e}")))?;
    if dir.is_dir() {
        // Sidecar cleanup BEFORE the dir itself disappears.
        remove_sidecar_flat_agents(root, &dir);
    }
    if toml_path.exists() {
        let canonical_target = toml_path
            .canonicalize()
            .map_err(|e| InstallError::Io(format!("canonicalize target: {e}")))?;
        if !canonical_target.starts_with(&canonical_root) {
            return Err(InstallError::Format(format!(
                "refusing to remove path outside agents root: {}",
                canonical_target.display()
            )));
        }
        std::fs::remove_file(&canonical_target)?;
    }
    if dir.is_dir() {
        let canonical_target = dir
            .canonicalize()
            .map_err(|e| InstallError::Io(format!("canonicalize target: {e}")))?;
        if !canonical_target.starts_with(&canonical_root) {
            return Err(InstallError::Format(format!(
                "refusing to remove path outside agents root: {}",
                canonical_target.display()
            )));
        }
        std::fs::remove_dir_all(&canonical_target)?;
    }
    Ok(())
}

/// One-time, idempotent migration of legacy `<plugin>/agent.md`
/// subdirectory installs into flat `<plugin>.toml` definitions the runtime
/// loader reads. Existing flat files win (re-runs are no-ops); the legacy
/// directories are left on disk (uninstall cleans them). Returns the number
/// of migrated agents.
pub fn migrate_legacy_agent_dirs() -> usize {
    migrate_legacy_agent_dirs_in(&shannon_agents_root())
}

/// `migrate_legacy_agent_dirs` against an explicit agents root (tests pass a
/// tempdir so they never touch the user's HOME).
pub fn migrate_legacy_agent_dirs_in(root: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(root) else {
        return 0;
    };
    let mut migrated = 0usize;
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(sanitized) = safe_plugin_name(&name) else {
            continue;
        };
        let target = root.join(format!("{sanitized}.toml"));
        if target.exists() {
            continue; // already migrated / user-authored flat file wins
        }
        // The AgentMarkdownInstaller shape wrote `<plugin>/agent.md`.
        let md = entry.path().join("agent.md");
        if !md.is_file() {
            continue;
        }
        let Ok(def) = shannon_agents::AgentDefinition::from_markdown_file(&md) else {
            tracing::warn!(path = %md.display(), "legacy agent.md unreadable — skipping migration");
            continue;
        };
        let toml_body = agent_definition_toml(
            &sanitized,
            &if def.description.is_empty() {
                sanitized.clone()
            } else {
                def.description
            },
            def.system_prompt.as_deref().unwrap_or(""),
            def.model.as_deref(),
            &def.capabilities,
        );
        if let Err(e) = std::fs::write(&target, toml_body) {
            tracing::warn!(path = %target.display(), error = %e, "agent migration write failed");
            continue;
        }
        tracing::info!(agent = %sanitized, "migrated legacy agent directory to flat definition");
        migrated += 1;
    }
    migrated
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn fixture_entry() -> CatalogEntry {
        CatalogEntry {
            id: "native:agent-test".to_string(),
            kind: AddonKind::Agent,
            name: "test-agent".to_string(),
            description: "test agent".to_string(),
            author: None,
            version: Some("0.1".into()),
            homepage_url: None,
            license: None,
            stars: None,
            last_updated: None,
            source: CatalogSource::Native,
            trust: TrustLevel::Verified,
            metadata: HashMap::new(),
            tags: vec![],
        }
    }

    #[tokio::test]
    async fn markdown_installer_writes_flat_toml_definition() {
        // Agents root is an isolated tempdir — no HOME mutation. The old form
        // set HOME via a lock-guarded env override, which is process-global
        // and raced with unrelated tests reading dirs::home_dir() under
        // parallel --lib.
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path().join(".shannon").join("agents");

        let installer = AgentMarkdownInstaller {
            plugin_name: "test-agent".into(),
            description: "Test agent".into(),
            system_prompt: "You are a test agent.\nBe thorough.".into(),
            model: Some("claude-sonnet-4-6".into()),
            tools: vec!["read".into(), "grep".into()],
            root_override: Some(root.clone()),
        };
        let entry = fixture_entry();
        let installed = installer
            .install(
                &entry,
                &InstallTarget::ShannonAgentsDir {
                    plugin: "test".into(),
                },
                &ProgressSink::null(),
            )
            .await
            .expect("install");
        let toml_path = root.join("test-agent.toml");
        assert!(
            installed
                .install_path
                .as_deref()
                .unwrap()
                .ends_with("test-agent.toml")
        );
        assert!(is_agent_installed_in(&root, "test-agent"));

        // G1 P1-9 acceptance: the runtime loader must be able to read the
        // written file back as an AgentDefinition.
        let def = shannon_agents::AgentDefinition::from_file(&toml_path).expect("load toml");
        assert_eq!(def.name, "test-agent");
        assert_eq!(def.description, "Test agent");
        assert_eq!(
            def.system_prompt.as_deref(),
            Some("You are a test agent.\nBe thorough.")
        );
        assert_eq!(def.model.as_deref(), Some("claude-sonnet-4-6"));
        assert_eq!(def.capabilities, vec!["read", "grep"]);

        installer.uninstall("test-agent").await.expect("uninstall");
        assert!(!is_agent_installed_in(&root, "test-agent"));
    }

    #[test]
    fn agent_definition_toml_escapes_quotes_and_newlines() {
        let body = agent_definition_toml(
            "quoted",
            "has \"quotes\" and\nnewlines",
            "prompt with \\ backslash",
            None,
            &[],
        );
        let parsed: shannon_agents::AgentDefinition =
            toml::from_str(&body).expect("escaped toml must parse");
        assert_eq!(parsed.description, "has \"quotes\" and\nnewlines");
        assert_eq!(
            parsed.system_prompt.as_deref(),
            Some("prompt with \\ backslash")
        );
    }

    #[test]
    fn migrate_legacy_agent_dir_writes_flat_toml_and_is_idempotent() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path().join(".shannon").join("agents");
        let legacy = root.join("old-agent");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(
            legacy.join("agent.md"),
            "---\nmodel: claude-opus\n---\nYou are a legacy agent.",
        )
        .unwrap();

        let migrated = migrate_legacy_agent_dirs_in(&root);
        assert_eq!(migrated, 1);
        let def = shannon_agents::AgentDefinition::from_file(&root.join("old-agent.toml"))
            .expect("migrated toml loads");
        assert_eq!(def.name, "old-agent");
        assert_eq!(def.model.as_deref(), Some("claude-opus"));
        assert_eq!(
            def.system_prompt.as_deref(),
            Some("You are a legacy agent.")
        );

        // Idempotent: second run migrates nothing; the flat file wins.
        assert_eq!(migrate_legacy_agent_dirs_in(&root), 0);

        // Migration leaves the legacy directory in place (uninstall cleans).
        assert!(legacy.is_dir());

        // Uninstall removes BOTH the flat toml and the legacy dir.
        remove_installed_agent_in(&root, "old-agent").expect("remove");
        assert!(!root.join("old-agent.toml").exists());
        assert!(!legacy.exists());
    }

    #[test]
    fn list_installed_agents_sees_flat_toml() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path().join(".shannon").join("agents");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("flat-agent.toml"), "name = \"flat-agent\"\n").unwrap();
        let rows = list_installed_agents_in(&root);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "flat-agent");
        assert!(rows[0].path.ends_with("flat-agent.toml"));
        assert!(is_agent_installed_in(&root, "flat-agent"));
    }

    #[test]
    fn remove_installed_agent_handles_flat_toml() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path().join(".shannon").join("agents");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("flat.toml"), "name = \"flat\"\n").unwrap();
        remove_installed_agent_in(&root, "flat").expect("remove");
        assert!(!root.join("flat.toml").exists());
    }

    /// Imp-4a: the flat materialization helper — writes a loader-readable
    /// toml under `<plugin>-<agent>.toml`, existing files win, unsafe agent
    /// names are rejected.
    #[test]
    fn write_flat_agent_toml_writes_loader_readable_definitions() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path().join("agents");
        std::fs::create_dir_all(&root).unwrap();

        assert!(write_flat_agent_toml(
            &root,
            "myrepo",
            "Code Reviewer",
            "Reviews code.",
            "You are a code reviewer.",
            Some("claude-sonnet-4-6"),
            &["read".to_string(), "grep".to_string()],
        ));
        let def =
            shannon_agents::AgentDefinition::from_file(&root.join("myrepo-code-reviewer.toml"))
                .expect("flat toml loads");
        assert_eq!(def.name, "myrepo-code-reviewer");
        assert_eq!(
            def.system_prompt.as_deref(),
            Some("You are a code reviewer.")
        );
        assert_eq!(def.capabilities, vec!["read", "grep"]);

        // Existing flat file wins (idempotent).
        assert!(!write_flat_agent_toml(
            &root,
            "myrepo",
            "code-reviewer",
            "other",
            "other prompt",
            None,
            &[],
        ));
        let def =
            shannon_agents::AgentDefinition::from_file(&root.join("myrepo-code-reviewer.toml"))
                .unwrap();
        assert_eq!(
            def.system_prompt.as_deref(),
            Some("You are a code reviewer.")
        );

        // Unsafe agent names are rejected without writing.
        assert!(!write_flat_agent_toml(
            &root,
            "myrepo",
            "../../escape",
            "x",
            "",
            None,
            &[],
        ));
        assert!(!root.join("myrepo-escape.toml").exists());
    }

    /// Imp-4a: uninstalling a repo plugin removes the flat tomls recorded in
    /// its sidecar (and only those), then the plugin dir itself.
    #[test]
    fn remove_installed_agent_cleans_sidecar_flat_files() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path().join("agents");
        let plugin_dir = root.join("myrepo");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(root.join("myrepo-alpha.toml"), "name = \"myrepo-alpha\"\n").unwrap();
        std::fs::write(root.join("myrepo-beta.toml"), "name = \"myrepo-beta\"\n").unwrap();
        std::fs::write(root.join("unrelated.toml"), "name = \"unrelated\"\n").unwrap();
        std::fs::write(
            plugin_dir.join(FLAT_AGENTS_SIDECAR),
            json_sidecar(&["myrepo-alpha.toml".into(), "myrepo-beta.toml".into()]),
        )
        .unwrap();

        remove_installed_agent_in(&root, "myrepo").expect("remove");
        assert!(!root.join("myrepo-alpha.toml").exists());
        assert!(!root.join("myrepo-beta.toml").exists());
        assert!(!plugin_dir.exists());
        // Unrelated definitions survive.
        assert!(root.join("unrelated.toml").exists());

        // A tampered sidecar with path escapes stays inside the root.
        let plugin_dir = root.join("evil");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(
            plugin_dir.join(FLAT_AGENTS_SIDECAR),
            json_sidecar(&["../../outside.toml".into()]),
        )
        .unwrap();
        let outside = tmp.path().join("outside.toml");
        std::fs::write(&outside, "name = \"outside\"\n").unwrap();
        remove_installed_agent_in(&root, "evil").expect("remove");
        assert!(outside.exists(), "sidecar must not remove outside the root");
    }

    #[test]
    fn list_installed_agents_handles_missing_dir() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path().join(".shannon").join("agents");
        let rows = list_installed_agents_in(&root);
        assert!(rows.is_empty());
    }

    #[test]
    fn list_installed_agents_returns_plugin_subdirs() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path().join(".shannon").join("agents");
        let agent_dir = root.join("alpha");
        std::fs::create_dir_all(&agent_dir).unwrap();
        std::fs::write(agent_dir.join("agent.md"), "body").unwrap();
        let rows = list_installed_agents_in(&root);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "alpha");
    }

    #[test]
    fn remove_installed_agent_rejects_missing_name() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path().join(".shannon").join("agents");
        let result = remove_installed_agent_in(&root, "nope");
        assert!(result.is_err());
    }

    #[test]
    fn remove_installed_agent_succeeds_for_existing() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path().join(".shannon").join("agents");
        let agent_dir = root.join("beta");
        std::fs::create_dir_all(&agent_dir).unwrap();
        remove_installed_agent_in(&root, "beta").expect("remove");
        assert!(!agent_dir.exists());
    }

    // ---- B0 P0-6: path traversal / absolute-path injection ----

    #[tokio::test]
    async fn markdown_installer_rejects_traversal_and_absolute_names() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path().join(".shannon").join("agents");
        let outside = tmp.path().join("pwned");

        for name in [
            "../pwned",
            "..\\pwned",
            "/etc/passwd",
            "C:\\Windows",
            "~/pwned",
            "a/../../pwned",
            "..",
            ".",
        ] {
            let installer = AgentMarkdownInstaller {
                plugin_name: name.into(),
                description: "x".into(),
                system_prompt: "---\nname: x\n---\n".into(),
                model: None,
                tools: vec![],
                root_override: Some(root.clone()),
            };
            let entry = fixture_entry();
            installer
                .install(
                    &entry,
                    &InstallTarget::ShannonAgentsDir { plugin: "t".into() },
                    &ProgressSink::null(),
                )
                .await
                .expect_err(&format!("must reject unsafe name: {name}"));
        }

        // Nothing landed outside the agents root…
        assert!(
            !outside.exists(),
            "traversal must not write outside the root"
        );
        // …and nothing at all was created under the root either.
        assert!(!root.exists() || std::fs::read_dir(&root).unwrap().next().is_none());
    }

    #[tokio::test]
    async fn repo_installer_rejects_traversal_name_before_clone() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path().join(".shannon").join("agents");

        let installer = AgentRepoInstaller {
            plugin_name: "../escape".into(),
            repo: "example/none".into(),
            ref_: "main".into(),
            root_override: Some(root.clone()),
        };
        let entry = fixture_entry();
        let err = installer
            .install(
                &entry,
                &InstallTarget::ShannonAgentsDir { plugin: "t".into() },
                &ProgressSink::null(),
            )
            .await
            .expect_err("traversal name must be rejected before any clone");
        assert!(err.to_string().contains("unsafe"), "got: {err}");
        assert!(!root.exists());
    }

    #[tokio::test]
    async fn markdown_installer_slugs_unsafe_characters() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path().join(".shannon").join("agents");

        let installer = AgentMarkdownInstaller {
            plugin_name: "My Agent v2!".into(),
            description: "x".into(),
            system_prompt: "---\nname: x\n---\n".into(),
            model: None,
            tools: vec![],
            root_override: Some(root.clone()),
        };
        let entry = fixture_entry();
        let installed = installer
            .install(
                &entry,
                &InstallTarget::ShannonAgentsDir { plugin: "t".into() },
                &ProgressSink::null(),
            )
            .await
            .expect("safe name must install");
        assert_eq!(installed.name, "my-agent-v2");
        assert!(root.join("my-agent-v2.toml").exists());
    }
}
