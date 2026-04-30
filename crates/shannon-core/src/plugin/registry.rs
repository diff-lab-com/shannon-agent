//! Plugin registry

use super::{config::PluginsConfig, error::PluginError, index::PluginIndex, manifest::PluginManifest, PluginResult};
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

    /// Load all plugins from the plugins directory
    pub async fn load_all(&mut self) -> PluginResult<()> {
        self.ensure_dir().await?;

        let mut entries = fs::read_dir(&self.plugins_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            // Skip non-directories
            if !path.is_dir() {
                continue;
            }

            // Try to load plugin manifest
            let manifest_path = path.join("plugin.toml");
            if let Ok(manifest_bytes) = fs::read(&manifest_path).await {
                if let Ok(manifest) = PluginManifest::from_toml_bytes(&manifest_bytes) {
                    let name = manifest.name.clone();
                    let enabled = self.config.is_enabled(&name);

                    self.plugins.insert(name, InstalledPlugin {
                        manifest,
                        path,
                        enabled,
                    });
                }
            }
        }

        Ok(())
    }

    /// Install a plugin from a git repository
    pub async fn install_from_git(&mut self, repo_url: &str) -> PluginResult<String> {
        self.ensure_dir().await?;

        // Extract plugin name from repo URL
        let plugin_name = Self::extract_name_from_url(repo_url)?;

        // Check if already installed
        if self.plugins.contains_key(&plugin_name) {
            return Err(PluginError::AlreadyInstalled(plugin_name));
        }

        // Clone the repository
        let target_dir = self.plugins_dir.join(&plugin_name);

        let status = Command::new("git")
            .args(["clone", "--depth", "1", repo_url, target_dir.to_str().unwrap()])
            .status()
            .await?;

        if !status.success() {
            return Err(PluginError::GitFailed(format!("Failed to clone {}", repo_url)));
        }

        // Load manifest
        let manifest = self.load_manifest_from_dir(&target_dir).await?;

        let name = manifest.name.clone();

        // Register the plugin
        self.plugins.insert(name.clone(), InstalledPlugin {
            manifest,
            path: target_dir,
            enabled: self.config.is_enabled(&name),
        });

        Ok(name)
    }

    /// Install a plugin from a local directory
    pub async fn install_from_path(&mut self, path: &Path) -> PluginResult<String> {
        self.ensure_dir().await?;

        // Validate path exists
        if !path.exists() {
            return Err(PluginError::InvalidDirectory(path.to_path_buf()));
        }

        // Load manifest
        let manifest = self.load_manifest_from_dir(path).await?;

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
        self.plugins.insert(plugin_name.clone(), InstalledPlugin {
            manifest,
            path: target_dir,
            enabled: self.config.is_enabled(&plugin_name),
        });

        Ok(plugin_name)
    }

    /// Uninstall a plugin
    pub async fn uninstall(&mut self, name: &str) -> PluginResult<()> {
        let plugin = self.plugins.get(name)
            .ok_or_else(|| PluginError::NotFound(name.to_string()))?;

        // Remove plugin directory
        fs::remove_dir_all(&plugin.path).await?;

        // Remove from registry
        self.plugins.remove(name);

        Ok(())
    }

    /// Enable a plugin
    pub fn enable(&mut self, name: &str) -> PluginResult<()> {
        let plugin = self.plugins.get_mut(name)
            .ok_or_else(|| PluginError::NotFound(name.to_string()))?;

        plugin.enabled = true;
        Ok(())
    }

    /// Disable a plugin
    pub fn disable(&mut self, name: &str) -> PluginResult<()> {
        let plugin = self.plugins.get_mut(name)
            .ok_or_else(|| PluginError::NotFound(name.to_string()))?;

        plugin.enabled = false;
        Ok(())
    }

    /// Update a plugin from its source
    pub async fn update(&mut self, name: &str) -> PluginResult<()> {
        let plugin = self.plugins.get(name)
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
                return Err(PluginError::GitFailed(format!("Failed to update {}", name)));
            }

            // Reload manifest
            let manifest = self.load_manifest_from_dir(&plugin.path).await?;
            if let Some(p) = self.plugins.get_mut(name) {
                p.manifest = manifest;
            }

            Ok(())
        } else {
            Err(PluginError::GitFailed(format!("Plugin {} is not a git repository", name)))
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
        self.plugins.values()
            .filter(|p| p.enabled)
            .collect()
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

    /// Load manifest from a directory
    async fn load_manifest_from_dir(&self, dir: &Path) -> PluginResult<PluginManifest> {
        let manifest_path = dir.join("plugin.toml");

        if !manifest_path.exists() {
            return Err(PluginError::InvalidManifest(format!(
                "plugin.toml not found in {}",
                dir.display()
            )));
        }

        let manifest_bytes = fs::read(&manifest_path).await?;
        let manifest = PluginManifest::from_toml_bytes(&manifest_bytes)
            .map_err(|e| PluginError::InvalidManifest(e))?;

        Ok(manifest)
    }

    /// Extract plugin name from git URL
    fn extract_name_from_url(url: &str) -> PluginResult<String> {
        // Remove .git suffix if present
        let url = url.trim_end_matches(".git");

        // Get the last part of the path
        let name = url.split('/')
            .last()
            .ok_or_else(|| PluginError::InvalidManifest(format!("Invalid URL: {}", url)))?;

        Ok(name.to_string())
    }

    /// Copy directory contents recursively
    fn copy_dir_contents<'a>(source: &'a Path, dest: &'a Path) -> std::pin::Pin<Box<dyn std::future::Future<Output = PluginResult<()>> + Send + 'a>> {
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
        let url = self.config.registry_url.clone()
            .unwrap_or_else(|| "https://raw.githubusercontent.com/shannon-code/plugins-index/main/index.json".to_string());
        PluginIndex::new(url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::PluginKind;
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
                "name = \"{}\"\nversion = \"1.0.0\"\ndescription = \"Test\"\n\
                type = \"skill\"\nentry = \"t.md\"\ntrigger = \"/{}\"\ntemplate = \"hi\"\n",
                name, name
            );
            fs::write(dir.join("plugin.toml"), manifest)
                .await
                .unwrap();
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
}
