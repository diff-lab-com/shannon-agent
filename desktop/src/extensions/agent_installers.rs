//! P4 Agent installers.
//!
//! Two installers handle different agent entry shapes:
//! - `AgentRepoInstaller` — clones a repo with a `.claude/agents/*.md` style
//!   collection into `~/.shannon/agents/<plugin>/`.
//! - `AgentMarkdownInstaller` — installs a single agent `.md` file.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::installer::{AddonInstaller, InstallError};
use super::types::{
    AddonKind, CatalogEntry, CatalogSource, ConfirmationLevel, InstallTarget, InstalledAddon,
    ProgressSink, TrustLevel,
};

/// Where agent definitions live. Today: `~/.shannon/agents/<plugin>/`.
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

        let target_dir = resolve_agents_root(self.root_override.as_deref()).join(&self.plugin_name);
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

        progress.emit(super::types::ProgressEvent::Finished).await;

        Ok(InstalledAddon {
            id: entry.id.clone(),
            kind: entry.kind,
            name: self.plugin_name.clone(),
            install_path: Some(target_dir.display().to_string()),
            installed_at: Some(Utc::now()),
            version: entry.version.clone(),
            enabled: true,
        })
    }

    async fn uninstall(&self, addon_id: &str) -> Result<(), InstallError> {
        let dir = resolve_agents_root(self.root_override.as_deref()).join(addon_id);
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        Ok(())
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

/// Single-file agent installer — writes one `.md` file with frontmatter
/// to `~/.shannon/agents/<plugin>/agent.md`.
pub struct AgentMarkdownInstaller {
    pub plugin_name: String,
    pub body: String,
    /// Test-only override for the agents root. Production leaves this `None`
    /// (resolve `~/.shannon/agents` from HOME); tests set it to a tempdir.
    pub root_override: Option<PathBuf>,
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

        let dir = resolve_agents_root(self.root_override.as_deref()).join(&self.plugin_name);
        std::fs::create_dir_all(&dir)?;
        let agent_md = dir.join("agent.md");
        std::fs::write(&agent_md, &self.body)?;

        progress.emit(super::types::ProgressEvent::Finished).await;

        Ok(InstalledAddon {
            id: entry.id.clone(),
            kind: entry.kind,
            name: self.plugin_name.clone(),
            install_path: Some(agent_md.display().to_string()),
            installed_at: Some(Utc::now()),
            version: entry.version.clone(),
            enabled: true,
        })
    }

    async fn uninstall(&self, addon_id: &str) -> Result<(), InstallError> {
        let dir = resolve_agents_root(self.root_override.as_deref()).join(addon_id);
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
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
pub fn is_agent_installed_in(root: &Path, plugin_name: &str) -> bool {
    root.join(plugin_name).exists()
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

/// `list_installed_agents` against an explicit agents `root`.
pub fn list_installed_agents_in(root: &Path) -> Vec<InstalledAgent> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
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
                    name: entry.file_name().to_string_lossy().into_owned(),
                    path: path.display().to_string(),
                    installed_at,
                });
            }
        }
    }
    out
}

/// Remove an installed agent plugin by name.
pub fn remove_installed_agent(name: &str) -> Result<(), InstallError> {
    remove_installed_agent_in(&shannon_agents_root(), name)
}

/// `remove_installed_agent` against an explicit agents `root`.
pub fn remove_installed_agent_in(root: &Path, name: &str) -> Result<(), InstallError> {
    let dir = root.join(name);
    if !dir.exists() {
        return Err(InstallError::Io(format!("{name} is not installed")));
    }
    let canonical_root = root
        .canonicalize()
        .map_err(|e| InstallError::Io(format!("canonicalize root: {e}")))?;
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
    Ok(())
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
    async fn markdown_installer_writes_agent_file() {
        // Agents root is an isolated tempdir — no HOME mutation. The old form
        // set HOME via a lock-guarded env override, which is process-global
        // and raced with unrelated tests reading dirs::home_dir() under
        // parallel --lib.
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path().join(".shannon").join("agents");

        let installer = AgentMarkdownInstaller {
            plugin_name: "test-agent".into(),
            body: "---\nname: test\n---\n# Test Agent\n".into(),
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
        assert!(
            installed
                .install_path
                .as_deref()
                .unwrap()
                .ends_with("test-agent/agent.md")
        );
        assert!(is_agent_installed_in(&root, "test-agent"));

        installer.uninstall("test-agent").await.expect("uninstall");
        assert!(!is_agent_installed_in(&root, "test-agent"));
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
}
