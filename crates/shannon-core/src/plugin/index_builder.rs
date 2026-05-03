//! Plugin index builder
//!
//! Scans a directory of installed plugins and generates an `index.json` file
//! suitable for hosting on a static file server or GitHub Pages.

use crate::plugin::manifest::PluginManifest;
use crate::plugin::{PluginError, PluginResult};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;

/// Serializable index entry derived from a plugin manifest.
///
/// This is the JSON representation stored in `index.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuiltIndexEntry {
    /// Plugin name (unique identifier)
    pub name: String,
    /// Short description
    pub description: String,
    /// Author
    pub author: Option<String>,
    /// Repository URL
    pub repository: Option<String>,
    /// Latest version
    pub latest_version: String,
    /// Plugin type: "tool", "command", or "skill"
    pub plugin_type: String,
    /// Keywords for search
    #[serde(default)]
    pub keywords: Vec<String>,
}

impl BuiltIndexEntry {
    /// Build an index entry from a parsed manifest.
    pub fn from_manifest(manifest: &PluginManifest) -> Self {
        Self {
            name: manifest.name.clone(),
            description: manifest.description.clone(),
            author: manifest.author.clone(),
            repository: manifest.repository.clone(),
            latest_version: manifest.version.clone(),
            plugin_type: manifest.plugin_type.clone(),
            keywords: manifest.keywords.clone(),
        }
    }
}

/// Index metadata included at the top of `index.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexMetadata {
    /// Schema version
    pub version: u32,
    /// Timestamp when the index was generated (ISO 8601)
    pub generated_at: String,
    /// Number of entries
    pub count: usize,
}

impl Default for IndexMetadata {
    fn default() -> Self {
        Self {
            version: 1,
            generated_at: chrono::Utc::now().to_rfc3339(),
            count: 0,
        }
    }
}

/// Top-level index file structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexFile {
    /// Index metadata
    pub metadata: IndexMetadata,
    /// Plugin entries
    pub plugins: Vec<BuiltIndexEntry>,
}

/// Builds an `index.json` from a directory of installed plugins.
pub struct IndexBuilder;

impl IndexBuilder {
    /// Scan a directory of plugin directories and build index entries.
    ///
    /// Each immediate subdirectory of `plugins_dir` is expected to contain a
    /// `plugin.toml` manifest. Directories without a valid manifest are
    /// silently skipped.
    pub async fn build_from_dir(plugins_dir: &Path) -> PluginResult<Vec<BuiltIndexEntry>> {
        let mut entries = Vec::new();

        if !plugins_dir.exists() {
            return Ok(entries);
        }

        let mut dir_entries = fs::read_dir(plugins_dir).await?;

        while let Some(entry) = dir_entries.next_entry().await? {
            let path = entry.path();

            // Skip non-directories
            if !path.is_dir() {
                continue;
            }

            let manifest_path = path.join("plugin.toml");
            if let Ok(bytes) = fs::read(&manifest_path).await {
                if let Ok(manifest) = PluginManifest::from_toml_bytes(&bytes) {
                    entries.push(BuiltIndexEntry::from_manifest(&manifest));
                }
            }
        }

        // Sort by name for deterministic output
        entries.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(entries)
    }

    /// Write an index as JSON to a file.
    ///
    /// The output is a JSON object with `metadata` and `plugins` fields.
    pub async fn write_index(entries: &[BuiltIndexEntry], output_path: &Path) -> PluginResult<()> {
        let metadata = IndexMetadata {
            count: entries.len(),
            ..Default::default()
        };

        let index_file = IndexFile {
            metadata,
            plugins: entries.to_vec(),
        };

        let json = serde_json::to_string_pretty(&index_file)
            .map_err(PluginError::Serialization)?;

        // Ensure parent directory exists
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let mut file = fs::File::create(output_path).await?;
        file.write_all(json.as_bytes()).await?;

        Ok(())
    }

    /// Build an index from a directory and write it to a file.
    ///
    /// Convenience method combining [`build_from_dir`] and [`write_index`].
    pub async fn build_and_write(plugins_dir: &Path, output_path: &Path) -> PluginResult<()> {
        let entries = Self::build_from_dir(plugins_dir).await?;
        Self::write_index(&entries, output_path).await
    }

    /// Read an existing index file.
    pub async fn read_index(path: &Path) -> PluginResult<IndexFile> {
        let content = fs::read_to_string(path).await?;
        let index: IndexFile = serde_json::from_str(&content)
            .map_err(PluginError::Serialization)?;
        Ok(index)
    }

    /// Build index entries from a list of specific plugin paths.
    ///
    /// Useful for building a curated index from selected plugins rather than
    /// scanning an entire directory.
    pub async fn build_from_paths(plugin_paths: &[PathBuf]) -> PluginResult<Vec<BuiltIndexEntry>> {
        let mut entries = Vec::new();

        for path in plugin_paths {
            let manifest_path = path.join("plugin.toml");
            if let Ok(bytes) = fs::read(&manifest_path).await {
                if let Ok(manifest) = PluginManifest::from_toml_bytes(&bytes) {
                    entries.push(BuiltIndexEntry::from_manifest(&manifest));
                }
            }
        }

        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_build_from_empty_dir() {
        let temp_dir = tempfile::tempdir().unwrap();
        let entries = IndexBuilder::build_from_dir(temp_dir.path()).await.unwrap();
        assert!(entries.is_empty());
    }

    #[tokio::test]
    async fn test_build_from_dir_with_plugins() {
        let temp_dir = tempfile::tempdir().unwrap();

        // Create a skill plugin
        let plugin_dir = temp_dir.path().join("my-skill");
        fs::create_dir_all(&plugin_dir).await.unwrap();
        let manifest = r#"
name = "my-skill"
version = "1.0.0"
description = "A test skill plugin"
author = "Test Author"
repository = "https://github.com/test/my-skill"
type = "skill"
entry = "template.md"
trigger = "/hello"
template = "Hello!"
keywords = ["test", "hello"]
"#;
        fs::write(plugin_dir.join("plugin.toml"), manifest)
            .await
            .unwrap();

        // Create a tool plugin
        let tool_dir = temp_dir.path().join("my-tool");
        fs::create_dir_all(&tool_dir).await.unwrap();
        let tool_manifest = r#"
name = "my-tool"
version = "2.0.0"
description = "A test tool plugin"
author = "Another Author"
type = "tool"
entry = "src/main.rs"

[transport]
type = "stdio"
command = "node"
args = ["index.js"]
"#;
        fs::write(tool_dir.join("plugin.toml"), tool_manifest)
            .await
            .unwrap();

        // Create a non-plugin file (should be skipped)
        fs::write(temp_dir.path().join("README.md"), "not a plugin")
            .await
            .unwrap();

        let entries = IndexBuilder::build_from_dir(temp_dir.path()).await.unwrap();
        assert_eq!(entries.len(), 2);

        // Should be sorted by name
        assert_eq!(entries[0].name, "my-skill");
        assert_eq!(entries[1].name, "my-tool");

        assert_eq!(entries[0].latest_version, "1.0.0");
        assert_eq!(entries[0].keywords, vec!["test", "hello"]);
        assert_eq!(entries[1].author, Some("Another Author".to_string()));
    }

    #[tokio::test]
    async fn test_write_and_read_index() {
        let temp_dir = tempfile::tempdir().unwrap();
        let output_path = temp_dir.path().join("output").join("index.json");

        let entries = vec![
            BuiltIndexEntry {
                name: "plugin-a".to_string(),
                description: "First plugin".to_string(),
                author: Some("Author A".to_string()),
                repository: Some("https://github.com/a/plugin-a".to_string()),
                latest_version: "1.0.0".to_string(),
                plugin_type: "tool".to_string(),
                keywords: vec!["a".to_string()],
            },
            BuiltIndexEntry {
                name: "plugin-b".to_string(),
                description: "Second plugin".to_string(),
                author: None,
                repository: None,
                latest_version: "0.1.0".to_string(),
                plugin_type: "skill".to_string(),
                keywords: vec![],
            },
        ];

        IndexBuilder::write_index(&entries, &output_path).await.unwrap();
        assert!(output_path.exists());

        let index_file = IndexBuilder::read_index(&output_path).await.unwrap();
        assert_eq!(index_file.metadata.version, 1);
        assert_eq!(index_file.metadata.count, 2);
        assert_eq!(index_file.plugins.len(), 2);
        assert_eq!(index_file.plugins[0].name, "plugin-a");
        assert_eq!(index_file.plugins[1].name, "plugin-b");
    }

    #[tokio::test]
    async fn test_build_and_write() {
        let temp_dir = tempfile::tempdir().unwrap();

        // Create a plugin
        let plugin_dir = temp_dir.path().join("sample");
        fs::create_dir_all(&plugin_dir).await.unwrap();
        fs::write(
            plugin_dir.join("plugin.toml"),
            r#"name = "sample"
version = "0.1.0"
description = "Sample"
type = "skill"
entry = "t.md"
trigger = "/sample"
template = "Sample!""#,
        )
        .await
        .unwrap();

        let output_path = temp_dir.path().join("index.json");
        IndexBuilder::build_and_write(temp_dir.path(), &output_path)
            .await
            .unwrap();

        let index_file = IndexBuilder::read_index(&output_path).await.unwrap();
        assert_eq!(index_file.plugins.len(), 1);
        assert_eq!(index_file.plugins[0].name, "sample");
    }

    #[tokio::test]
    async fn test_build_from_paths() {
        let temp_dir = tempfile::tempdir().unwrap();

        let plugin_dir = temp_dir.path().join("selected");
        fs::create_dir_all(&plugin_dir).await.unwrap();
        fs::write(
            plugin_dir.join("plugin.toml"),
            r#"name = "selected"
version = "3.0.0"
description = "A selected plugin"
type = "command"
entry = "cmd.rs"
command_name = "sel"
command_description = "Selection""#,
        )
        .await
        .unwrap();

        let entries = IndexBuilder::build_from_paths(&[plugin_dir.clone()])
            .await
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "selected");
        assert_eq!(entries[0].latest_version, "3.0.0");
    }

    #[tokio::test]
    async fn test_index_metadata_default() {
        let meta = IndexMetadata::default();
        assert_eq!(meta.version, 1);
        assert_eq!(meta.count, 0);
        assert!(!meta.generated_at.is_empty());
    }
}
