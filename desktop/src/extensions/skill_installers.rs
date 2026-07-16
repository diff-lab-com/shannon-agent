//! P3 Skill installers.
//!
//! Two installers handle different skill entry shapes:
//! - `MarketplacePluginInstaller` — installs a `.claude-plugin/marketplace.json`
//!   repo by cloning it into `~/.shannon/skills/<plugin>/`.
//! - `SkillMarkdownInstaller` — installs a single SKILL.md (no marketplace).

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::installer::{AddonInstaller, InstallError};
use super::types::{
    AddonKind, CatalogEntry, CatalogSource, ConfirmationLevel, InstallTarget, InstalledAddon,
    ProgressSink,
};

/// Where skills live on disk. Today: `~/.shannon/skills/<plugin>/<skill>/`.
fn shannon_skills_root() -> PathBuf {
    #[cfg(test)]
    {
        if let Some(p) = TEST_SKILLS_ROOT_OVERRIDE.with(|cell| cell.borrow().clone()) {
            return p;
        }
    }
    dirs::home_dir()
        .map(|h| h.join(".shannon").join("skills"))
        .unwrap_or_else(|| PathBuf::from("/tmp/shannon-skills"))
}

#[cfg(test)]
thread_local! {
    static TEST_SKILLS_ROOT_OVERRIDE: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) struct SkillsRootGuard;
#[cfg(test)]
impl Drop for SkillsRootGuard {
    fn drop(&mut self) {
        TEST_SKILLS_ROOT_OVERRIDE.with(|cell| *cell.borrow_mut() = None);
    }
}

#[cfg(test)]
pub(crate) fn set_test_skills_root(root: PathBuf) -> SkillsRootGuard {
    TEST_SKILLS_ROOT_OVERRIDE.with(|cell| *cell.borrow_mut() = Some(root));
    SkillsRootGuard
}

/// Marketplace plugin installer — fetches a repo, drops it under
/// `~/.shannon/skills/<plugin>/`.
pub struct MarketplacePluginInstaller {
    pub plugin_name: String,
    pub repo: String,
    pub ref_: String,
}

#[async_trait]
impl AddonInstaller for MarketplacePluginInstaller {
    fn kind(&self) -> AddonKind {
        AddonKind::Skill
    }

    fn supports(&self, entry: &CatalogEntry) -> bool {
        matches!(entry.source, CatalogSource::GitHubRepo { .. }) && entry.kind == AddonKind::Skill
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

        let target_dir = shannon_skills_root().join(&self.plugin_name);
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
                description: "Validating marketplace.json".into(),
                current: Some(2),
                total: Some(3),
            })
            .await;
        // Verify the marketplace.json (or skill file) exists.
        let manifest = target_dir.join(".claude-plugin").join("marketplace.json");
        let skill_md = target_dir.join("SKILL.md");
        if !manifest.exists() && !skill_md.exists() {
            // Cleanup the partial clone.
            let _ = std::fs::remove_dir_all(&target_dir);
            return Err(InstallError::Format(format!(
                "repo {repo} has neither .claude-plugin/marketplace.json nor SKILL.md",
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
        let dir = shannon_skills_root().join(addon_id);
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        Ok(())
    }

    async fn update(&self, addon_id: &str) -> Result<InstalledAddon, InstallError> {
        let dir = shannon_skills_root().join(addon_id);
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
            kind: AddonKind::Skill,
            name: addon_id.to_string(),
            install_path: Some(dir.display().to_string()),
            installed_at: Some(Utc::now()),
            version: None,
            enabled: true,
        })
    }

    fn requires_confirmation(&self, entry: &CatalogEntry) -> ConfirmationLevel {
        match entry.trust {
            super::types::TrustLevel::Verified => ConfirmationLevel::None,
            super::types::TrustLevel::Official => ConfirmationLevel::Review,
            _ => ConfirmationLevel::Review,
        }
    }
}

/// Single-file SKILL.md installer — used for native / built-in skills.
///
/// Writes the markdown to `~/.shannon/skills/<plugin>/SKILL.md` without
/// cloning anything. The body is provided up-front so this installer has no
/// network dependency.
pub struct SkillMarkdownInstaller {
    pub plugin_name: String,
    pub body: String,
}

#[async_trait]
impl AddonInstaller for SkillMarkdownInstaller {
    fn kind(&self) -> AddonKind {
        AddonKind::Skill
    }

    fn supports(&self, entry: &CatalogEntry) -> bool {
        entry.kind == AddonKind::Skill
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

        let dir = shannon_skills_root().join(&self.plugin_name);
        std::fs::create_dir_all(&dir)?;
        let skill_md = dir.join("SKILL.md");
        std::fs::write(&skill_md, &self.body)?;

        progress.emit(super::types::ProgressEvent::Finished).await;

        Ok(InstalledAddon {
            id: entry.id.clone(),
            kind: entry.kind,
            name: self.plugin_name.clone(),
            install_path: Some(skill_md.display().to_string()),
            installed_at: Some(Utc::now()),
            version: entry.version.clone(),
            enabled: true,
        })
    }

    async fn uninstall(&self, addon_id: &str) -> Result<(), InstallError> {
        let dir = shannon_skills_root().join(addon_id);
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        Ok(())
    }

    async fn update(&self, _addon_id: &str) -> Result<InstalledAddon, InstallError> {
        Err(InstallError::Unsupported(
            "SkillMarkdownInstaller has no upstream; cannot update".into(),
        ))
    }

    fn requires_confirmation(&self, _entry: &CatalogEntry) -> ConfirmationLevel {
        ConfirmationLevel::None
    }
}

/// Used by the Tauri command layer to ask "is this skill already installed?"
pub fn is_skill_installed(plugin_name: &str) -> bool {
    shannon_skills_root().join(plugin_name).exists()
}

/// Wire type for listing installed skills.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledSkill {
    pub name: String,
    pub path: String,
    pub installed_at: Option<String>,
}

/// Scan `~/.shannon/skills/` for installed skill plugins.
pub fn list_installed_skills() -> Vec<InstalledSkill> {
    let root = shannon_skills_root();
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&root) {
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
                out.push(InstalledSkill {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    path: path.display().to_string(),
                    installed_at,
                });
            }
        }
    }
    out
}

/// Remove an installed skill plugin by name.
pub fn remove_installed_skill(name: &str) -> Result<(), InstallError> {
    let dir = shannon_skills_root().join(name);
    if !dir.exists() {
        return Err(InstallError::Io(format!("{name} is not installed")));
    }
    // Defense against path traversal: ensure the resolved path is inside the skills root.
    let canonical_root = shannon_skills_root()
        .canonicalize()
        .map_err(|e| InstallError::Io(format!("canonicalize root: {e}")))?;
    let canonical_target = dir
        .canonicalize()
        .map_err(|e| InstallError::Io(format!("canonicalize target: {e}")))?;
    if !canonical_target.starts_with(&canonical_root) {
        return Err(InstallError::Format(format!(
            "refusing to remove path outside skills root: {}",
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
            id: "gh:test/repo/main/skill-x".to_string(),
            kind: AddonKind::Skill,
            name: "skill-x".to_string(),
            description: "test skill".to_string(),
            author: None,
            version: Some("0.1".into()),
            homepage_url: None,
            license: None,
            stars: None,
            last_updated: None,
            source: CatalogSource::Native,
            trust: super::super::types::TrustLevel::Verified,
            metadata: HashMap::new(),
            tags: vec![],
        }
    }

    #[tokio::test]
    async fn markdown_installer_writes_skill_file() {
        let tmp = tempfile::tempdir().expect("tmp");
        let _g = set_test_skills_root(tmp.path().join(".shannon").join("skills"));

        let installer = SkillMarkdownInstaller {
            plugin_name: "test-skill".into(),
            body: "---\nname: test\n---\n# Test\n".into(),
        };
        let entry = fixture_entry();
        let installed = installer
            .install(
                &entry,
                &InstallTarget::ShannonSkillsDir {
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
                .ends_with("test-skill/SKILL.md")
        );
        assert!(is_skill_installed("test-skill"));

        installer.uninstall("test-skill").await.expect("uninstall");
        assert!(!is_skill_installed("test-skill"));
    }

    #[test]
    fn list_installed_skills_handles_missing_dir() {
        let tmp = tempfile::tempdir().expect("tmp");
        let _g = set_test_skills_root(tmp.path().join(".shannon").join("skills"));
        let rows = list_installed_skills();
        assert!(rows.is_empty());
    }

    #[test]
    fn list_installed_skills_returns_plugin_subdirs() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path().join(".shannon").join("skills");
        let _g = set_test_skills_root(root.clone());
        let skill_dir = root.join("alpha");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), "body").unwrap();
        let rows = list_installed_skills();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "alpha");
    }

    #[test]
    fn remove_installed_skill_rejects_missing_name() {
        let tmp = tempfile::tempdir().expect("tmp");
        let _g = set_test_skills_root(tmp.path().join(".shannon").join("skills"));
        let result = remove_installed_skill("nope");
        assert!(result.is_err());
    }

    #[test]
    fn remove_installed_skill_succeeds_for_existing() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path().join(".shannon").join("skills");
        let _g = set_test_skills_root(root.clone());
        let skill_dir = root.join("beta");
        std::fs::create_dir_all(&skill_dir).unwrap();
        remove_installed_skill("beta").expect("remove");
        assert!(!skill_dir.exists());
    }
}
