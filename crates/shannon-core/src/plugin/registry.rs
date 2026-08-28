//! Plugin registry

use super::{
    PluginResult, config::PluginsConfig, error::PluginError, index::PluginIndex,
    manifest::PluginManifest,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::process::Command;

/// Installed plugin
#[derive(Debug, Clone)]
pub struct InstalledPlugin {
    /// Plugin manifest
    pub manifest: PluginManifest,

    /// Plugin directory path
    pub path: PathBuf,

    /// Whether the plugin is enabled
    pub enabled: bool,
}

/// Trust decision for a **remote (git)** plugin install (review 2026-08-28
/// SEC-1).
///
/// A remote manifest that declares no `permissions` would run under the
/// runtime's default-allow compat contract — undeclared = every capability
/// face (`execute_commands`, `network`, `mcp_tools`, …) open. Cloning an
/// arbitrary URL with one call must not silently acquire that surface, so
/// the default [`RemoteInstallConsent::default`] refuses it with
/// [`PluginError::UnverifiedRemote`]; interactive callers set
/// [`RemoteInstallConsent::allow_unverified`] only after the user
/// explicitly accepted the risk.
///
/// Local-path installs (`install_from_path`) are the developer flow and
/// deliberately keep the legacy behavior.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RemoteInstallConsent {
    /// Install despite an undeclared (default-allow-all) permission surface.
    pub allow_unverified: bool,
}

impl RemoteInstallConsent {
    /// Explicit opt-in for an unverified (permissions-less) remote manifest.
    pub fn allow_unverified() -> Self {
        Self {
            allow_unverified: true,
        }
    }
}

/// Plugin registry
#[derive(Debug)]
pub struct PluginRegistry {
    /// Installed plugins by name
    plugins: HashMap<String, InstalledPlugin>,

    /// Plugins directory
    plugins_dir: PathBuf,

    /// Plugin configuration
    config: PluginsConfig,
}

impl PluginRegistry {
    /// Create a new plugin registry
    pub fn new(plugins_dir: PathBuf) -> Self {
        Self {
            plugins: HashMap::new(),
            plugins_dir,
            config: PluginsConfig::default(),
        }
    }

    /// Create a new plugin registry with custom config
    pub fn with_config(plugins_dir: PathBuf, config: PluginsConfig) -> Self {
        let plugins_dir = config.plugins_dir.clone().unwrap_or(plugins_dir);
        Self {
            plugins: HashMap::new(),
            plugins_dir,
            config,
        }
    }

    /// Get the plugins directory
    pub fn plugins_dir(&self) -> &Path {
        &self.plugins_dir
    }

    /// Ensure the plugins directory exists
    pub async fn ensure_dir(&self) -> PluginResult<()> {
        fs::create_dir_all(&self.plugins_dir).await?;
        Ok(())
    }

    /// Load all plugins from the plugins directory (§4.10 semantics).
    ///
    /// Each subdirectory is probed for `plugin.toml` first, then
    /// `.claude-plugin/plugin.json`. Two outcomes are distinct:
    ///
    /// - a directory carrying **no manifest at all** is not a plugin and is
    ///   skipped (unchanged lenient behavior — the plugins dir may hold
    ///   unrelated scratch folders);
    /// - a directory whose manifest **exists but fails to parse or validate**
    ///   is collected and reported. Every valid sibling still loads; the call
    ///   then returns [`PluginError::LoadFailures`] enumerating each bad path
    ///   plus its reason, so broken manifests can no longer vanish silently.
    pub async fn load_all(&mut self) -> PluginResult<()> {
        self.ensure_dir().await?;

        let mut entries = fs::read_dir(&self.plugins_dir).await?;
        let mut failures: Vec<String> = Vec::new();

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            // Skip non-directories
            if !path.is_dir() {
                continue;
            }

            match self.try_load_manifest_from_dir(&path).await {
                Ok(Some(manifest)) => {
                    // Load-time completeness is warning-only: already-
                    // installed legacy plugins must keep loading.
                    if let Ok(warnings) = super::validate::validate_for_install(&manifest) {
                        super::validate::warn_about(&warnings);
                    }
                    let name = manifest.name.clone();
                    let enabled = self.config.is_enabled(&name);

                    self.plugins.insert(
                        name,
                        InstalledPlugin {
                            manifest,
                            path,
                            enabled,
                        },
                    );
                }
                Ok(None) => {}
                Err(e) => {
                    failures.push(format!("{}: {e}", path.display()));
                }
            }
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(PluginError::LoadFailures(failures.join("\n")))
        }
    }

    /// Install a plugin from a git repository.
    ///
    /// `consent` carries the trust decision for manifests that declare no
    /// [`PluginManifest::permissions`]: by default such a **remote** install
    /// is refused ([`PluginError::UnverifiedRemote`]) because the runtime's
    /// default-allow contract would grant every capability face to whatever
    /// the URL serves (review 2026-08-28 SEC-1). Pass
    /// [`RemoteInstallConsent::allow_unverified()`] only after an explicit
    /// user confirmation; the half-cloned directory is removed on refusal so
    /// a later `load_all` scan cannot resurrect the plugin from disk.
    pub async fn install_from_git(
        &mut self,
        repo_url: &str,
        consent: RemoteInstallConsent,
    ) -> PluginResult<String> {
        self.ensure_dir().await?;

        // Extract plugin name from repo URL
        let plugin_name = Self::extract_name_from_url(repo_url)?;

        // Check if already installed
        if self.plugins.contains_key(&plugin_name) {
            return Err(PluginError::AlreadyInstalled(plugin_name));
        }

        // Clone the repository
        let target_dir = self.plugins_dir.join(&plugin_name);

        let target_str = target_dir.to_str().ok_or_else(|| {
            PluginError::GitFailed(format!(
                "Plugin path is not valid UTF-8: {}",
                target_dir.display()
            ))
        })?;
        let status = Command::new("git")
            .args(["clone", "--depth", "1", repo_url, target_str])
            .status()
            .await?;

        if !status.success() {
            return Err(PluginError::GitFailed(format!(
                "Failed to clone {repo_url}"
            )));
        }

        // Load manifest and gate installation on it (§4.10 install-time checks)
        let manifest = self.load_manifest_from_dir(&target_dir).await?;
        Self::admit_for_install(&manifest)?;

        // SEC-1 gate: an *undeclared* remote manifest would run allow-all at
        // runtime (declaration = allow-set; empty = every face open). Unlike
        // a local path the user pointed at, a one-shot `git clone` of an
        // arbitrary URL must not acquire that surface silently. Refuse unless
        // explicitly opted in — and drop the clone, because a leftover
        // manifest-carrying directory would be picked up by `load_all` on the
        // next scan and effectively self-install.
        if manifest.permissions.is_empty() && !consent.allow_unverified {
            let name = manifest.name.clone();
            if let Err(cleanup_err) = fs::remove_dir_all(&target_dir).await {
                tracing::warn!(
                    path = %target_dir.display(),
                    error = %cleanup_err,
                    "refused unverified plugin install; failed to remove the cloned directory"
                );
            }
            return Err(PluginError::UnverifiedRemote(name));
        }

        let name = manifest.name.clone();

        // Register the plugin
        self.plugins.insert(
            name.clone(),
            InstalledPlugin {
                manifest,
                path: target_dir,
                enabled: self.config.is_enabled(&name),
            },
        );

        Ok(name)
    }

    /// Install a plugin from a local directory
    pub async fn install_from_path(&mut self, path: &Path) -> PluginResult<String> {
        self.ensure_dir().await?;

        // Validate path exists
        if !path.exists() {
            return Err(PluginError::InvalidDirectory(path.to_path_buf()));
        }

        // Load manifest and gate installation on it (§4.10 install-time checks)
        let manifest = self.load_manifest_from_dir(path).await?;
        Self::admit_for_install(&manifest)?;

        let plugin_name = manifest.name.clone();

        // Check if already installed
        if self.plugins.contains_key(&plugin_name) {
            return Err(PluginError::AlreadyInstalled(plugin_name));
        }

        // Copy to plugins directory
        let target_dir = self.plugins_dir.join(&plugin_name);

        // Create target and copy contents
        fs::create_dir_all(&target_dir).await?;
        Self::copy_dir_contents(path, &target_dir).await?;

        // Register the plugin
        self.plugins.insert(
            plugin_name.clone(),
            InstalledPlugin {
                manifest,
                path: target_dir,
                enabled: self.config.is_enabled(&plugin_name),
            },
        );

        Ok(plugin_name)
    }

    /// Uninstall a plugin
    pub async fn uninstall(&mut self, name: &str) -> PluginResult<()> {
        let plugin = self
            .plugins
            .get(name)
            .ok_or_else(|| PluginError::NotFound(name.to_string()))?;

        // Remove plugin directory
        fs::remove_dir_all(&plugin.path).await?;

        // Remove from registry
        self.plugins.remove(name);

        Ok(())
    }

    /// Enable a plugin
    pub fn enable(&mut self, name: &str) -> PluginResult<()> {
        let plugin = self
            .plugins
            .get_mut(name)
            .ok_or_else(|| PluginError::NotFound(name.to_string()))?;

        plugin.enabled = true;
        Ok(())
    }

    /// Disable a plugin
    pub fn disable(&mut self, name: &str) -> PluginResult<()> {
        let plugin = self
            .plugins
            .get_mut(name)
            .ok_or_else(|| PluginError::NotFound(name.to_string()))?;

        plugin.enabled = false;
        Ok(())
    }

    /// Update a plugin from its source
    pub async fn update(&mut self, name: &str) -> PluginResult<()> {
        let plugin = self
            .plugins
            .get(name)
            .ok_or_else(|| PluginError::NotFound(name.to_string()))?;

        // Check if plugin has a git repository
        let git_dir = plugin.path.join(".git");
        if git_dir.exists() {
            let status = Command::new("git")
                .args(["pull"])
                .current_dir(&plugin.path)
                .status()
                .await?;

            if !status.success() {
                return Err(PluginError::GitFailed(format!("Failed to update {name}")));
            }

            // Reload manifest; the refreshed bytes must still pass install
            // validation before they replace what is registered.
            let manifest = self.load_manifest_from_dir(&plugin.path).await?;
            Self::admit_for_install(&manifest)?;
            if let Some(p) = self.plugins.get_mut(name) {
                p.manifest = manifest;
            }

            Ok(())
        } else {
            Err(PluginError::GitFailed(format!(
                "Plugin {name} is not a git repository"
            )))
        }
    }

    /// Update all plugins
    pub async fn update_all(&mut self) -> PluginResult<Vec<String>> {
        let names: Vec<String> = self.plugins.keys().cloned().collect();
        let mut updated = Vec::new();

        for name in names {
            if self.update(&name).await.is_ok() {
                updated.push(name);
            }
        }

        Ok(updated)
    }

    /// List all installed plugins
    pub fn list(&self) -> Vec<&InstalledPlugin> {
        self.plugins.values().collect()
    }

    /// List enabled plugins
    pub fn list_enabled(&self) -> Vec<&InstalledPlugin> {
        self.plugins.values().filter(|p| p.enabled).collect()
    }

    /// Get a plugin by name
    pub fn get(&self, name: &str) -> Option<&InstalledPlugin> {
        self.plugins.get(name)
    }

    /// Get a mutable plugin by name
    pub fn get_mut(&mut self, name: &str) -> Option<&mut InstalledPlugin> {
        self.plugins.get_mut(name)
    }

    /// Check if a plugin is installed
    pub fn contains(&self, name: &str) -> bool {
        self.plugins.contains_key(name)
    }

    /// Get the number of installed plugins
    pub fn len(&self) -> usize {
        self.plugins.len()
    }

    /// Check if there are no plugins
    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

    /// Load manifest from a directory.
    ///
    /// Tries Shannon-native `plugin.toml` first, then falls back to the
    /// Claude Code ecosystem format at `.claude-plugin/plugin.json`. This
    /// lets Shannon load plugins authored for Claude Code directly.
    async fn load_manifest_from_dir(&self, dir: &Path) -> PluginResult<PluginManifest> {
        match self.try_load_manifest_from_dir(dir).await? {
            Some(manifest) => Ok(manifest),
            None => Self::absent_manifest_error(dir),
        }
    }

    /// Probe a directory for a manifest without treating absence as failure:
    /// `Ok(None)` = no manifest present (not a plugin directory),
    /// `Err` = a manifest exists but could not be parsed.
    async fn try_load_manifest_from_dir(&self, dir: &Path) -> PluginResult<Option<PluginManifest>> {
        let toml_path = dir.join("plugin.toml");
        if toml_path.exists() {
            let manifest_bytes = fs::read(&toml_path).await?;
            return PluginManifest::from_toml_bytes(&manifest_bytes)
                .map(Some)
                .map_err(PluginError::InvalidManifest);
        }

        let claude_json_path = dir.join(".claude-plugin").join("plugin.json");
        if claude_json_path.exists() {
            let manifest_bytes = fs::read(&claude_json_path).await?;
            return PluginManifest::from_json_bytes(&manifest_bytes)
                .map(Some)
                .map_err(PluginError::InvalidManifest);
        }

        Ok(None)
    }

    /// The canonical error when neither manifest location exists.
    fn absent_manifest_error(dir: &Path) -> PluginResult<PluginManifest> {
        Err(PluginError::InvalidManifest(format!(
            "neither plugin.toml nor .claude-plugin/plugin.json found in {}",
            dir.display()
        )))
    }

    /// Extract plugin name from git URL
    fn extract_name_from_url(url: &str) -> PluginResult<String> {
        // Remove .git suffix if present
        let url = url.trim_end_matches(".git");

        // Get the last part of the path
        let name = url
            .split('/')
            .next_back()
            .ok_or_else(|| PluginError::InvalidManifest(format!("Invalid URL: {url}")))?;

        Ok(name.to_string())
    }

    /// Install-time gate shared by git/path/update flows (§4.10):
    /// schema validation must pass outright; completeness gaps on legacy
    /// dialects downgrade to logged warnings.
    fn admit_for_install(manifest: &PluginManifest) -> PluginResult<()> {
        let warnings = super::validate::validate_for_install(manifest)?;
        super::validate::warn_about(&warnings);
        Ok(())
    }

    /// Copy directory contents recursively
    fn copy_dir_contents<'a>(
        source: &'a Path,
        dest: &'a Path,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = PluginResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let mut entries = fs::read_dir(source).await?;

            while let Some(entry) = entries.next_entry().await? {
                let source_path = entry.path();
                let dest_path = dest.join(entry.file_name());

                if source_path.is_dir() {
                    fs::create_dir_all(&dest_path).await?;
                    Self::copy_dir_contents(&source_path, &dest_path).await?;
                } else {
                    fs::copy(&source_path, &dest_path).await?;
                }
            }

            Ok(())
        })
    }

    /// Create a plugin index from the configured registry
    pub fn create_index(&self) -> PluginIndex {
        let url = self.config.registry_url.clone().unwrap_or_else(|| {
            "https://raw.githubusercontent.com/shannon-code/plugins-index/main/index.json"
                .to_string()
        });
        PluginIndex::new(url)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::plugin::{ManifestVersion, PluginKind};
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_registry_creation() {
        let temp_dir = TempDir::new().unwrap();
        let registry = PluginRegistry::new(temp_dir.path().to_path_buf());

        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn test_extract_name_from_url() {
        assert_eq!(
            PluginRegistry::extract_name_from_url("https://github.com/user/repo").unwrap(),
            "repo"
        );
        assert_eq!(
            PluginRegistry::extract_name_from_url("https://github.com/user/repo.git").unwrap(),
            "repo"
        );
        assert_eq!(
            PluginRegistry::extract_name_from_url("git@github.com:user/repo.git").unwrap(),
            "repo"
        );
    }

    #[tokio::test]
    async fn test_load_all_with_valid_plugin() {
        let temp_dir = TempDir::new().unwrap();
        let plugin_dir = temp_dir.path().join("my-skill-plugin");
        fs::create_dir_all(&plugin_dir).await.unwrap();

        let manifest_content = "name = \"my-skill-plugin\"\n\
version = \"0.1.0\"\n\
description = \"A test skill plugin\"\n\
author = \"Test\"\n\
type = \"skill\"\n\
entry = \"template.md\"\n\
trigger = \"/hello\"\n\
template = \"Hello {{name}}!\"\n\
\n\
permissions = [\"read_files\"]\n";
        fs::write(plugin_dir.join("plugin.toml"), manifest_content)
            .await
            .unwrap();

        let mut registry = PluginRegistry::new(temp_dir.path().to_path_buf());
        registry.load_all().await.unwrap();

        assert_eq!(registry.len(), 1);
        assert!(registry.contains("my-skill-plugin"));

        let plugin = registry.get("my-skill-plugin").unwrap();
        assert!(plugin.enabled);
        assert_eq!(plugin.manifest.version, "0.1.0");
        assert_eq!(plugin.manifest.type_display_name(), "Skill");

        // Verify kind() works
        let kind = plugin.manifest.kind().unwrap();
        assert!(matches!(kind, PluginKind::Skill { .. }));
    }

    #[tokio::test]
    async fn test_load_all_skips_non_directories() {
        let temp_dir = TempDir::new().unwrap();
        // Write a plain file (not a directory) in the plugins dir
        fs::write(temp_dir.path().join("README.md"), "not a plugin")
            .await
            .unwrap();

        let mut registry = PluginRegistry::new(temp_dir.path().to_path_buf());
        registry.load_all().await.unwrap();

        assert!(registry.is_empty());
    }

    #[tokio::test]
    async fn test_enable_disable_skill_plugin() {
        let temp_dir = TempDir::new().unwrap();
        let plugin_dir = temp_dir.path().join("test-plugin");
        fs::create_dir_all(&plugin_dir).await.unwrap();

        let manifest_content = "name = \"test-plugin\"\n\
            version = \"1.0.0\"\n\
            description = \"Test skill plugin\"\n\
            type = \"skill\"\n\
            entry = \"template.md\"\n\
            trigger = \"/hello\"\n\
            template = \"Hello!\"\n";
        fs::write(plugin_dir.join("plugin.toml"), manifest_content)
            .await
            .unwrap();

        let mut registry = PluginRegistry::new(temp_dir.path().to_path_buf());
        registry.load_all().await.unwrap();

        assert!(registry.get("test-plugin").unwrap().enabled);

        registry.disable("test-plugin").unwrap();
        assert!(!registry.get("test-plugin").unwrap().enabled);

        registry.enable("test-plugin").unwrap();
        assert!(registry.get("test-plugin").unwrap().enabled);
    }

    #[tokio::test]
    async fn test_list_enabled_filters_correctly() {
        let temp_dir = TempDir::new().unwrap();

        // Create two skill plugins (avoids name conflict with Command type)
        for name in &["plugin-a", "plugin-b"] {
            let dir = temp_dir.path().join(name);
            fs::create_dir_all(&dir).await.unwrap();
            let manifest = format!(
                "name = \"{name}\"\nversion = \"1.0.0\"\ndescription = \"Test\"\n\
                type = \"skill\"\nentry = \"t.md\"\ntrigger = \"/{name}\"\ntemplate = \"hi\"\n"
            );
            fs::write(dir.join("plugin.toml"), manifest).await.unwrap();
        }

        let mut registry = PluginRegistry::new(temp_dir.path().to_path_buf());
        registry.load_all().await.unwrap();

        assert_eq!(registry.list().len(), 2);
        assert_eq!(registry.list_enabled().len(), 2);

        registry.disable("plugin-a").unwrap();
        assert_eq!(registry.list_enabled().len(), 1);
    }

    #[tokio::test]
    async fn test_uninstall_removes_plugin() {
        let temp_dir = TempDir::new().unwrap();
        let plugin_dir = temp_dir.path().join("to-remove");
        fs::create_dir_all(&plugin_dir).await.unwrap();

        let manifest = "name = \"to-remove\"\n\
            version = \"1.0.0\"\n\
            description = \"Test\"\n\
            type = \"skill\"\n\
            entry = \"template.md\"\n\
            trigger = \"/hello\"\n\
            template = \"Hello!\"\n";
        fs::write(plugin_dir.join("plugin.toml"), manifest)
            .await
            .unwrap();

        let mut registry = PluginRegistry::new(temp_dir.path().to_path_buf());
        registry.load_all().await.unwrap();
        assert_eq!(registry.len(), 1);

        registry.uninstall("to-remove").await.unwrap();
        assert!(registry.is_empty());
        assert!(!temp_dir.path().join("to-remove").exists());
    }

    #[tokio::test]
    async fn test_install_from_path() {
        let temp_dir = TempDir::new().unwrap();
        let source_dir = temp_dir.path().join("source-plugin");
        fs::create_dir_all(&source_dir).await.unwrap();

        let manifest = "name = \"installed-plugin\"\n\
            version = \"2.0.0\"\n\
            description = \"Installed from path\"\n\
            type = \"skill\"\n\
            entry = \"template.md\"\n\
            trigger = \"/world\"\n\
            template = \"World!\"\n";
        fs::write(source_dir.join("plugin.toml"), manifest)
            .await
            .unwrap();
        fs::write(source_dir.join("template.md"), "Hello World")
            .await
            .unwrap();

        let plugins_dir = temp_dir.path().join("plugins");
        let mut registry = PluginRegistry::new(plugins_dir);
        let name = registry.install_from_path(&source_dir).await.unwrap();

        assert_eq!(name, "installed-plugin");
        assert_eq!(registry.len(), 1);
        assert!(registry.contains("installed-plugin"));
    }

    #[tokio::test]
    async fn test_load_all_loads_claude_plugin_json() {
        let temp_dir = TempDir::new().unwrap();
        let plugin_dir = temp_dir.path().join("claude-ecosystem-plugin");
        let claude_dir = plugin_dir.join(".claude-plugin");
        fs::create_dir_all(&claude_dir).await.unwrap();

        let json = "{\n\
            \"name\": \"claude-ecosystem-plugin\",\n\
            \"version\": \"0.4.2\",\n\
            \"description\": \"Authored as a Claude Code plugin\",\n\
            \"type\": \"skill\",\n\
            \"entry\": \"template.md\",\n\
            \"trigger\": \"/hi\",\n\
            \"template\": \"Hi!\"\n\
        }";
        fs::write(claude_dir.join("plugin.json"), json)
            .await
            .unwrap();

        let mut registry = PluginRegistry::new(temp_dir.path().to_path_buf());
        registry.load_all().await.unwrap();

        assert_eq!(registry.len(), 1);
        assert!(registry.contains("claude-ecosystem-plugin"));

        let plugin = registry.get("claude-ecosystem-plugin").unwrap();
        assert_eq!(plugin.manifest.version, "0.4.2");
        assert_eq!(plugin.manifest.type_display_name(), "Skill");
        assert!(matches!(
            plugin.manifest.kind().unwrap(),
            PluginKind::Skill { .. }
        ));
    }

    #[tokio::test]
    async fn test_toml_takes_precedence_over_claude_json() {
        let temp_dir = TempDir::new().unwrap();
        let plugin_dir = temp_dir.path().join("dual-plugin");
        let claude_dir = plugin_dir.join(".claude-plugin");
        fs::create_dir_all(&claude_dir).await.unwrap();

        fs::write(
            plugin_dir.join("plugin.toml"),
            "name = \"dual-plugin\"\nversion = \"1.0.0\"\n\
             description = \"from toml\"\ntype = \"skill\"\n\
             entry = \"t.md\"\ntrigger = \"/a\"\ntemplate = \"A\"\n",
        )
        .await
        .unwrap();
        fs::write(
            claude_dir.join("plugin.json"),
            "{\"name\":\"dual-plugin\",\"version\":\"2.0.0\",\
             \"description\":\"from json\",\"type\":\"skill\",\
             \"entry\":\"t.md\",\"trigger\":\"/b\",\"template\":\"B\"}",
        )
        .await
        .unwrap();

        let mut registry = PluginRegistry::new(temp_dir.path().to_path_buf());
        registry.load_all().await.unwrap();

        let plugin = registry.get("dual-plugin").unwrap();
        assert_eq!(plugin.manifest.version, "1.0.0");
        assert_eq!(plugin.manifest.description, "from toml");
    }

    // ---------- §4.10 W3-2: strict scanning + install-time gating ----------

    const VALID_V2_SKILL_TOML: &str = r#"
manifest_version = "2"
name = "greeting"
version = "1.0.0"
description = "complete v2 skill"
type = "skill"
entry = "template.md"
trigger = "/greet"
template = "Hello!"
permissions = ["read_files", "llm_api"]
"#;

    /// Broken manifests no longer disappear: `load_all` keeps every valid
    /// sibling and returns one aggregated error naming each bad path.
    #[tokio::test]
    async fn load_all_reports_every_broken_manifest_but_keeps_valid_plugins() {
        let temp_dir = TempDir::new().unwrap();

        let good_dir = temp_dir.path().join("good");
        fs::create_dir_all(&good_dir).await.unwrap();
        fs::write(good_dir.join("plugin.toml"), VALID_V2_SKILL_TOML)
            .await
            .unwrap();

        for bad in ["broken-one", "broken-two"] {
            let dir = temp_dir.path().join(bad);
            fs::create_dir_all(&dir).await.unwrap();
            fs::write(dir.join("plugin.toml"), "name = \"unterminated")
                .await
                .unwrap();
        }

        let mut registry = PluginRegistry::new(temp_dir.path().to_path_buf());
        let err = registry.load_all().await.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("broken-one") && msg.contains("broken-two"),
            "{msg}"
        );
        assert!(matches!(err, PluginError::LoadFailures(_)));

        // the good plugin still registered despite the aggregated failure
        assert_eq!(registry.len(), 1);
        assert!(registry.contains("greeting"));
    }

    #[tokio::test]
    async fn manifestless_directories_still_skip_silently() {
        let temp_dir = TempDir::new().unwrap();
        let scratch = temp_dir.path().join("scratch-notes");
        fs::create_dir_all(&scratch).await.unwrap();
        fs::write(scratch.join("todo.txt"), "not a plugin")
            .await
            .unwrap();

        let mut registry = PluginRegistry::new(temp_dir.path().to_path_buf());
        registry.load_all().await.expect("no failures");
        assert!(registry.is_empty());
    }

    #[tokio::test]
    async fn install_rejects_v2_manifest_missing_implied_permissions() {
        let temp_dir = TempDir::new().unwrap();
        let source_dir = temp_dir.path().join("overreaching-v2");
        fs::create_dir_all(&source_dir).await.unwrap();
        // v2 command/skill implies read_files + llm_api; declares neither.
        fs::write(
            source_dir.join("plugin.toml"),
            r#"
manifest_version = "2"
name = "overreach"
version = "1.0.0"
description = "incomplete v2 declaration"
type = "skill"
entry = "t.md"
trigger = "/o"
template = "t"
"#,
        )
        .await
        .unwrap();

        let plugins_dir = temp_dir.path().join("plugins");
        let mut registry = PluginRegistry::new(plugins_dir);
        let err = registry
            .install_from_path(&source_dir)
            .await
            .expect_err("v2 completeness must block install");
        let msg = err.to_string();
        assert!(
            msg.contains("v2 permission completeness")
                && msg.contains("read_files")
                && msg.contains("llm_api"),
            "{msg}"
        );
        // nothing copied into the plugins dir on refusal
        assert!(!temp_dir.path().join("plugins/overreach").exists());
    }

    #[tokio::test]
    async fn install_accepts_complete_v2_manifest() {
        let temp_dir = TempDir::new().unwrap();
        let source_dir = temp_dir.path().join("complete-v2-src");
        fs::create_dir_all(&source_dir).await.unwrap();
        fs::write(source_dir.join("plugin.toml"), VALID_V2_SKILL_TOML)
            .await
            .unwrap();
        fs::write(source_dir.join("template.md"), "Hello!")
            .await
            .unwrap();

        let plugins_dir = temp_dir.path().join("plugins");
        let mut registry = PluginRegistry::new(plugins_dir.clone());
        let name = registry.install_from_path(&source_dir).await.unwrap();
        assert_eq!(name, "greeting");

        let installed = registry.get("greeting").unwrap();
        assert_eq!(installed.manifest.schema_version(), ManifestVersion::V2);
        // post-install reload via load_all also succeeds cleanly now
        let mut fresh = PluginRegistry::new(plugins_dir);
        fresh.load_all().await.expect("reload clean");
        assert!(fresh.contains("greeting"));
    }

    #[tokio::test]
    async fn test_load_manifest_from_dir_errors_when_neither_exists() {
        let temp_dir = TempDir::new().unwrap();
        let empty_dir = temp_dir.path().join("empty");
        fs::create_dir_all(&empty_dir).await.unwrap();

        let registry = PluginRegistry::new(temp_dir.path().to_path_buf());
        let result = registry.load_manifest_from_dir(&empty_dir).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("neither plugin.toml nor .claude-plugin/plugin.json"));
    }

    // ---------- SEC-1: remote installs need a declared permission set ----

    const UNDECLARED_V1_SKILL_TOML: &str = r#"
name = "shady"
version = "1.0.0"
description = "declares nothing"
type = "skill"
entry = "t.md"
trigger = "/shady"
template = "hi"
"#;

    const DECLARED_V1_SKILL_TOML: &str = r#"
name = "honest"
version = "1.0.0"
description = "declares its faces"
type = "skill"
entry = "t.md"
trigger = "/honest"
template = "hi"
permissions = ["read_files", "llm_api"]
"#;

    /// Materialize a local git repo as an offline stand-in remote
    /// (`git clone <path>` works against plain paths). Every git invocation
    /// pins `current_dir` and a null global config so the test neither
    /// touches nor depends on the surrounding checkout.
    fn init_git_plugin_repo(dir: &Path, manifest_toml: &str) -> String {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("plugin.toml"), manifest_toml).unwrap();
        std::fs::write(dir.join("t.md"), "Hello!").unwrap();
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(dir)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .env("GIT_AUTHOR_NAME", "test")
                .env("GIT_AUTHOR_EMAIL", "test@example.com")
                .env("GIT_COMMITTER_NAME", "test")
                .env("GIT_COMMITTER_EMAIL", "test@example.com")
                .output()
                .expect("git is available")
        };
        run(&["init", "-q"]);
        run(&["add", "-A"]);
        let commit = run(&["commit", "-q", "-m", "init"]);
        assert!(
            commit.status.success(),
            "git commit failed: {}",
            String::from_utf8_lossy(&commit.stderr)
        );
        dir.to_string_lossy().to_string()
    }

    /// Undeclared permissions + remote source + default (non-interactive)
    /// consent = refused, with no half-clone left for `load_all` to
    /// resurrect on the next scan.
    #[tokio::test]
    async fn remote_install_without_permissions_refused_and_clone_removed() {
        let temp_dir = TempDir::new().unwrap();
        let remote = init_git_plugin_repo(
            &temp_dir.path().join("remote-no-perms"),
            UNDECLARED_V1_SKILL_TOML,
        );

        let plugins_dir = temp_dir.path().join("plugins");
        let mut registry = PluginRegistry::new(plugins_dir.clone());
        let err = registry
            .install_from_git(&remote, RemoteInstallConsent::default())
            .await
            .unwrap_err();

        assert!(matches!(err, PluginError::UnverifiedRemote(_)), "{err}");
        assert!(
            err.to_string().contains("allow_unverified"),
            "refusal must name the explicit opt-in: {err}"
        );
        assert!(!registry.contains("shady"));
        assert!(
            !plugins_dir.join("remote-no-perms").exists(),
            "the refused clone must be removed"
        );

        // A fresh scan must not find the refused plugin on disk either.
        let mut fresh = PluginRegistry::new(plugins_dir);
        fresh.load_all().await.expect("scan clean");
        assert!(fresh.is_empty());
    }

    /// The explicit opt-in unlocks the same install.
    #[tokio::test]
    async fn remote_install_unverified_with_explicit_opt_in_installs() {
        let temp_dir = TempDir::new().unwrap();
        let remote = init_git_plugin_repo(
            &temp_dir.path().join("remote-no-perms"),
            UNDECLARED_V1_SKILL_TOML,
        );

        let mut registry = PluginRegistry::new(temp_dir.path().join("plugins"));
        let name = registry
            .install_from_git(&remote, RemoteInstallConsent::allow_unverified())
            .await
            .expect("opt-in installs the unverified remote plugin");
        assert_eq!(name, "shady");
        assert!(registry.contains("shady"));
    }

    /// A remote manifest that *declares* its permission set installs with
    /// the default consent — the gate targets only the undeclared case.
    #[tokio::test]
    async fn remote_install_with_declared_permissions_needs_no_opt_in() {
        let temp_dir = TempDir::new().unwrap();
        let remote = init_git_plugin_repo(
            &temp_dir.path().join("remote-declared"),
            DECLARED_V1_SKILL_TOML,
        );

        let mut registry = PluginRegistry::new(temp_dir.path().join("plugins"));
        let name = registry
            .install_from_git(&remote, RemoteInstallConsent::default())
            .await
            .expect("declared remote manifest installs without opt-in");
        assert_eq!(name, "honest");
    }

    /// Local-path installs are the developer flow and keep the legacy
    /// behavior: an undeclared manifest still installs (red line — the
    /// SEC-1 gate never touches `install_from_path`).
    #[tokio::test]
    async fn local_path_install_of_unverified_plugin_still_works() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("dev-plugin");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("plugin.toml"), UNDECLARED_V1_SKILL_TOML).unwrap();
        std::fs::write(source.join("t.md"), "Hello!").unwrap();

        let mut registry = PluginRegistry::new(temp_dir.path().join("plugins"));
        let name = registry
            .install_from_path(&source)
            .await
            .expect("local undeclared plugin keeps installing");
        assert_eq!(name, "shady");
    }
}
