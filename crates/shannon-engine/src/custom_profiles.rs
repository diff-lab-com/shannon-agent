//! Custom permission profiles loaded from `.shannon/profiles/*.toml` and `.claude/profiles/*.toml`.
//!
//! Users can define named permission presets with fine-grained tool control.
//!
//! ## File Format
//!
//! ```toml
//! name = "trusted"
//! description = "Trusted project with full tool access"
//! auto_approve = ["Read", "Glob", "Grep", "LS", "Bash", "Edit", "Write"]
//! confirm = []
//! deny = []
//! ```
//!
//! ## Discovery
//!
//! Files are loaded from (later overrides earlier):
//! 1. `~/.shannon/profiles/` (user-global)
//! 2. `~/.claude/profiles/` (user-global, compatible)
//! 3. `.shannon/profiles/` (project-local)
//! 4. `.claude/profiles/` (project-local, compatible)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A user-defined permission profile loaded from a TOML file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomProfileDef {
    /// Profile name (must match filename without extension).
    pub name: String,
    /// Human-readable description.
    #[serde(default)]
    pub description: String,
    /// Tools that are auto-approved (no confirmation needed).
    #[serde(default)]
    pub auto_approve: Vec<String>,
    /// Tools that require user confirmation.
    #[serde(default)]
    pub confirm: Vec<String>,
    /// Tools that are always denied.
    #[serde(default)]
    pub deny: Vec<String>,
}

/// Registry of loaded custom profile definitions.
#[derive(Debug, Clone, Default)]
pub struct CustomProfileRegistry {
    profiles: HashMap<String, CustomProfileDef>,
}

impl CustomProfileRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load custom profiles from standard directories.
    ///
    /// Loading order (later overrides earlier):
    /// 1. `~/.shannon/profiles/` (user-global)
    /// 2. `~/.claude/profiles/` (user-global)
    /// 3. `.shannon/profiles/` (project-local)
    /// 4. `.claude/profiles/` (project-local)
    pub fn load_from_dirs() -> Self {
        let mut registry = Self::new();
        let mut search_paths = Vec::new();

        // User-global
        if let Some(home) = dirs::home_dir() {
            search_paths.push(home.join(".shannon").join("profiles"));
            search_paths.push(home.join(".claude").join("profiles"));
        }

        // Project-local (higher priority)
        search_paths.push(PathBuf::from(".shannon").join("profiles"));
        search_paths.push(PathBuf::from(".claude").join("profiles"));

        for dir in &search_paths {
            if dir.is_dir() {
                registry.load_from_dir(dir);
            }
        }

        registry
    }

    /// Load all `.toml` files from a directory.
    pub fn load_from_dir(&mut self, dir: &Path) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) => {
                tracing::debug!(dir = %dir.display(), error = %e, "Failed to read profiles directory");
                return;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }

            match Self::parse_file(&path) {
                Ok(def) => {
                    tracing::debug!(name = %def.name, path = %path.display(), "Loaded custom profile");
                    self.profiles.insert(def.name.clone(), def);
                }
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "Failed to load custom profile");
                }
            }
        }
    }

    /// Parse a single TOML file.
    pub fn parse_file(path: &Path) -> Result<CustomProfileDef, CustomProfileError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| CustomProfileError::Io(path.to_path_buf(), e))?;

        let def: CustomProfileDef = toml::from_str(&content)
            .map_err(|e| CustomProfileError::Parse(path.to_path_buf(), e.to_string()))?;

        if def.name.is_empty() {
            return Err(CustomProfileError::Validation(
                path.to_path_buf(),
                "Profile name must not be empty".into(),
            ));
        }

        Ok(def)
    }

    /// Get a custom profile by name.
    pub fn get(&self, name: &str) -> Option<&CustomProfileDef> {
        self.profiles.get(name)
    }

    /// List all custom profile names.
    pub fn list_names(&self) -> Vec<String> {
        self.profiles.keys().cloned().collect()
    }

    /// Get all loaded profiles.
    pub fn all(&self) -> &HashMap<String, CustomProfileDef> {
        &self.profiles
    }

    /// Check if any custom profiles are loaded.
    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }

    /// Get a summary of loaded custom profiles.
    pub fn summary(&self) -> String {
        if self.profiles.is_empty() {
            return "No custom permission profiles loaded.".to_string();
        }

        let mut lines = Vec::new();
        lines.push(format!("Loaded {} custom profile(s):", self.profiles.len()));
        for (name, def) in &self.profiles {
            let auto_count = def.auto_approve.len();
            let confirm_count = def.confirm.len();
            let deny_count = def.deny.len();
            lines.push(format!(
                "  - {name}: {desc} ({auto_count} auto, {confirm_count} confirm, {deny_count} deny)",
                desc = if def.description.is_empty() { "No description" } else { &def.description }
            ));
        }
        lines.join("\n")
    }
}

/// Errors loading custom profiles.
#[derive(Debug, thiserror::Error)]
pub enum CustomProfileError {
    #[error("IO error reading {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("Parse error in {0}: {1}")]
    Parse(PathBuf, String),
    #[error("Validation error in {0}: {1}")]
    Validation(PathBuf, String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parse_full_profile() {
        let toml = r#"
name = "trusted"
description = "Full access for trusted projects"
auto_approve = ["Read", "Glob", "Grep", "Bash", "Edit", "Write"]
confirm = []
deny = []
"#;
        let def: CustomProfileDef = toml::from_str(toml).unwrap();
        assert_eq!(def.name, "trusted");
        assert_eq!(def.description, "Full access for trusted projects");
        assert_eq!(def.auto_approve.len(), 6);
        assert!(def.confirm.is_empty());
        assert!(def.deny.is_empty());
    }

    #[test]
    fn parse_minimal_profile() {
        let toml = r#"name = "minimal""#;
        let def: CustomProfileDef = toml::from_str(toml).unwrap();
        assert_eq!(def.name, "minimal");
        assert!(def.description.is_empty());
        assert!(def.auto_approve.is_empty());
    }

    #[test]
    fn parse_restricted_profile() {
        let toml = r#"
name = "restricted"
description = "Read-only access"
auto_approve = ["Read", "Glob", "Grep"]
confirm = ["Edit"]
deny = ["Bash", "Write"]
"#;
        let def: CustomProfileDef = toml::from_str(toml).unwrap();
        assert_eq!(def.name, "restricted");
        assert!(def.deny.contains(&"Bash".to_string()));
        assert!(def.deny.contains(&"Write".to_string()));
    }

    #[test]
    fn load_from_directory() {
        let dir = tempfile::tempdir().unwrap();

        fs::write(
            dir.path().join("trusted.toml"),
            r#"name = "trusted"
description = "Trusted"
auto_approve = ["Read", "Write"]
"#,
        )
        .unwrap();

        fs::write(
            dir.path().join("strict.toml"),
            r#"name = "strict"
description = "Strict mode"
deny = ["Bash", "Write"]
"#,
        )
        .unwrap();

        // Non-toml file should be skipped
        fs::write(dir.path().join("notes.txt"), "not a profile").unwrap();

        let mut registry = CustomProfileRegistry::new();
        registry.load_from_dir(dir.path());

        assert_eq!(registry.all().len(), 2);
        assert!(registry.get("trusted").is_some());
        assert!(registry.get("strict").is_some());
        assert!(registry.get("notes").is_none());
    }

    #[test]
    fn local_overrides_global() {
        let global = tempfile::tempdir().unwrap();
        let local = tempfile::tempdir().unwrap();

        fs::write(
            global.path().join("myprof.toml"),
            r#"name = "myprof"
description = "Global version"
auto_approve = ["Read"]
"#,
        )
        .unwrap();

        fs::write(
            local.path().join("myprof.toml"),
            r#"name = "myprof"
description = "Local version"
auto_approve = ["Read", "Write"]
"#,
        )
        .unwrap();

        let mut registry = CustomProfileRegistry::new();
        registry.load_from_dir(global.path());
        registry.load_from_dir(local.path());

        let def = registry.get("myprof").unwrap();
        assert_eq!(def.description, "Local version");
        assert!(def.auto_approve.contains(&"Write".to_string()));
    }

    #[test]
    fn reject_empty_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.toml");
        fs::write(&path, r#"name = """#).unwrap();

        let result = CustomProfileRegistry::parse_file(&path);
        assert!(result.is_err());
    }

    #[test]
    fn reject_invalid_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.toml");
        fs::write(&path, "not valid toml [[[[").unwrap();

        let result = CustomProfileRegistry::parse_file(&path);
        assert!(result.is_err());
    }

    #[test]
    fn nonexistent_dir_is_ok() {
        let mut registry = CustomProfileRegistry::new();
        registry.load_from_dir(Path::new("/no/such/directory"));
        assert!(registry.is_empty());
    }

    #[test]
    fn summary_format() {
        let mut registry = CustomProfileRegistry::new();
        let empty = registry.summary();
        assert!(empty.contains("No custom"));

        registry.profiles.insert(
            "test".to_string(),
            CustomProfileDef {
                name: "test".to_string(),
                description: "Test profile".to_string(),
                auto_approve: vec!["Read".to_string()],
                confirm: vec!["Write".to_string()],
                deny: vec!["Bash".to_string()],
            },
        );

        let summary = registry.summary();
        assert!(summary.contains("1 custom profile"));
        assert!(summary.contains("test"));
        assert!(summary.contains("Test profile"));
    }

    // ── Additional edge case tests ────────────────────────────────────────

    #[test]
    fn serialization_roundtrip() {
        let def = CustomProfileDef {
            name: "roundtrip".to_string(),
            description: "Test roundtrip".to_string(),
            auto_approve: vec!["Read".to_string(), "Glob".to_string()],
            confirm: vec!["Edit".to_string()],
            deny: vec!["Bash".to_string()],
        };
        let json = serde_json::to_string(&def).unwrap();
        let back: CustomProfileDef = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "roundtrip");
        assert_eq!(back.auto_approve.len(), 2);
        assert_eq!(back.confirm.len(), 1);
        assert_eq!(back.deny.len(), 1);
    }

    #[test]
    fn toml_roundtrip() {
        let def = CustomProfileDef {
            name: "full".to_string(),
            description: "Full profile".to_string(),
            auto_approve: vec!["Read".to_string()],
            confirm: vec!["Write".to_string()],
            deny: vec!["Bash".to_string()],
        };
        let toml_str = toml::to_string(&def).unwrap();
        let back: CustomProfileDef = toml::from_str(&toml_str).unwrap();
        assert_eq!(back.name, def.name);
        assert_eq!(back.description, def.description);
        assert_eq!(back.auto_approve, def.auto_approve);
    }

    #[test]
    fn duplicate_tools_in_same_category() {
        let toml = r#"
name = "dupes"
auto_approve = ["Read", "Read", "Glob"]
"#;
        let def: CustomProfileDef = toml::from_str(toml).unwrap();
        assert_eq!(def.auto_approve.len(), 3); // TOML allows duplicates
    }

    #[test]
    fn tool_in_multiple_categories() {
        // A tool can appear in multiple categories — that's user responsibility
        let toml = r#"
name = "conflict"
auto_approve = ["Read"]
deny = ["Read"]
"#;
        let def: CustomProfileDef = toml::from_str(toml).unwrap();
        assert!(def.auto_approve.contains(&"Read".to_string()));
        assert!(def.deny.contains(&"Read".to_string()));
    }

    #[test]
    fn empty_arrays_default() {
        let toml = r#"name = "empty""#;
        let def: CustomProfileDef = toml::from_str(toml).unwrap();
        assert!(def.auto_approve.is_empty());
        assert!(def.confirm.is_empty());
        assert!(def.deny.is_empty());
    }

    #[test]
    fn list_names_returns_all() {
        let mut registry = CustomProfileRegistry::new();
        registry.profiles.insert(
            "alpha".to_string(),
            CustomProfileDef {
                name: "alpha".to_string(),
                description: String::new(),
                auto_approve: vec![],
                confirm: vec![],
                deny: vec![],
            },
        );
        registry.profiles.insert(
            "beta".to_string(),
            CustomProfileDef {
                name: "beta".to_string(),
                description: String::new(),
                auto_approve: vec![],
                confirm: vec![],
                deny: vec![],
            },
        );
        let mut names = registry.list_names();
        names.sort();
        assert_eq!(names, vec!["alpha", "beta"]);
    }

    #[test]
    fn error_display_messages() {
        let err = CustomProfileError::Io(
            PathBuf::from("/tmp/test.toml"),
            std::io::Error::new(std::io::ErrorKind::NotFound, "missing"),
        );
        assert!(err.to_string().contains("/tmp/test.toml"));
        assert!(err.to_string().contains("missing"));

        let err = CustomProfileError::Parse(PathBuf::from("bad.toml"), "invalid key".to_string());
        assert!(err.to_string().contains("bad.toml"));
        assert!(err.to_string().contains("invalid key"));

        let err = CustomProfileError::Validation(PathBuf::from("x.toml"), "no name".to_string());
        assert!(err.to_string().contains("no name"));
    }

    #[test]
    fn get_nonexistent_returns_none() {
        let registry = CustomProfileRegistry::new();
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn is_empty_new_registry() {
        let registry = CustomProfileRegistry::new();
        assert!(registry.is_empty());
    }

    #[test]
    fn load_from_dir_with_bad_toml_doesnt_block_others() {
        let dir = tempfile::tempdir().unwrap();
        // Bad file
        fs::write(dir.path().join("bad.toml"), "not valid").unwrap();
        // Good file
        fs::write(
            dir.path().join("good.toml"),
            r#"name = "good"
description = "Valid"
"#,
        )
        .unwrap();

        let mut registry = CustomProfileRegistry::new();
        registry.load_from_dir(dir.path());
        assert!(registry.get("good").is_some());
        assert_eq!(registry.all().len(), 1);
    }
}
