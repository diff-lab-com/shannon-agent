//! Aggregator — reads existing local configs and produces `InstalledAddon`
//! summaries for the Installed tab.
//!
//! P1 scope: surface what's already on disk (MCP servers from settings.json,
//! skills from `~/.shannon/skills/`, agents from `~/.shannon/agents/`). No
//! remote fetch. No write path.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::types::{AddonKind, InstalledAddon};

/// Lightweight summary suitable for the Installed tab list view.
///
/// Mirrors `InstalledAddon` but flattens for the wire — the UI doesn't need
/// to know about installers to render a row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledAddonSummary {
    pub id: String,
    pub kind: AddonKind,
    pub name: String,
    #[serde(default)]
    pub install_path: Option<String>,
    #[serde(default)]
    pub installed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub version: Option<String>,
    pub enabled: bool,
}

impl From<InstalledAddon> for InstalledAddonSummary {
    fn from(addon: InstalledAddon) -> Self {
        Self {
            id: addon.id,
            kind: addon.kind,
            name: addon.name,
            install_path: addon.install_path,
            installed_at: addon.installed_at,
            version: addon.version,
            enabled: addon.enabled,
        }
    }
}

/// Aggregate all locally-installed addons across the four categories.
///
/// Order: MCP → Skills → Agents → Data Sources. Plugins are derived from the
/// Skills/Agents/MCP rows; not yet surfaced as a separate kind in P1.
pub fn aggregate_installed() -> Vec<InstalledAddonSummary> {
    let mut out = Vec::new();
    out.extend(mcp_servers());
    out.extend(skills());
    out.extend(agents());
    out
}

/// Read MCP servers from `~/.shannon/settings.json` and `.mcp.json`.
fn mcp_servers() -> Vec<InstalledAddonSummary> {
    let mut out = Vec::new();

    for (path, scope) in candidate_mcp_config_paths() {
        let Some(content) = read_text(&path) else {
            continue;
        };
        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };
        let Some(servers) = parsed.get("mcpServers").and_then(|v| v.as_object()) else {
            continue;
        };
        for (name, value) in servers {
            let enabled = value
                .get("enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            out.push(InstalledAddonSummary {
                id: format!("mcp:{name}"),
                kind: AddonKind::Mcp,
                name: name.clone(),
                install_path: Some(format!(
                    "{}#mcpServers.{} ({})",
                    path.display(),
                    name,
                    scope
                )),
                installed_at: file_mtime(&path),
                version: None,
                enabled,
            });
        }
    }

    out
}

/// Read skills from `~/.shannon/skills/` and `~/.claude/commands/`.
fn skills() -> Vec<InstalledAddonSummary> {
    let mut out = Vec::new();

    for base in candidate_skill_dirs() {
        if !base.is_dir() {
            continue;
        }
        let entries = match std::fs::read_dir(&base) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.trim_end_matches(".md").to_string(),
                None => continue,
            };
            if name.is_empty() {
                continue;
            }
            out.push(InstalledAddonSummary {
                id: format!("skill:{}", name),
                kind: AddonKind::Skill,
                name,
                install_path: Some(path.display().to_string()),
                installed_at: file_mtime(&path),
                version: None,
                enabled: true,
            });
        }
    }

    out
}

/// Read agent definitions from `~/.shannon/agents/` and `.claude/agents/`.
fn agents() -> Vec<InstalledAddonSummary> {
    let mut out = Vec::new();

    for base in candidate_agent_dirs() {
        if !base.is_dir() {
            continue;
        }
        let entries = match std::fs::read_dir(&base) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let name = match path.file_stem().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            out.push(InstalledAddonSummary {
                id: format!("agent:{}", name),
                kind: AddonKind::Agent,
                name,
                install_path: Some(path.display().to_string()),
                installed_at: file_mtime(&path),
                version: None,
                enabled: true,
            });
        }
    }

    out
}

fn candidate_mcp_config_paths() -> Vec<(PathBuf, &'static str)> {
    let mut paths = Vec::new();
    if let Some(home) = dirs::home_dir() {
        paths.push((home.join(".shannon/settings.json"), "user"));
        paths.push((home.join(".claude/settings.json"), "claude-compat"));
    }
    paths.push((PathBuf::from(".mcp.json"), "project"));
    paths
}

fn candidate_skill_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(home) = dirs::home_dir() {
        out.push(home.join(".shannon/skills"));
        out.push(home.join(".claude/commands"));
    }
    out
}

fn candidate_agent_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(home) = dirs::home_dir() {
        out.push(home.join(".shannon/agents"));
        out.push(home.join(".claude/agents"));
    }
    out
}

fn read_text(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn file_mtime(path: &Path) -> Option<DateTime<Utc>> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    DateTime::<Utc>::from_timestamp(
        modified
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs() as i64,
        0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn aggregate_with_no_configs_does_not_panic() {
        // We can't easily empty $HOME, but aggregate must not panic even with
        // partial filesystem access. The function logs no errors and returns
        // whatever it finds.
        let result = aggregate_installed();
        // result may be empty or contain entries from the test env's home.
        assert!(
            result
                .iter()
                .all(|r| matches!(r.kind, AddonKind::Mcp | AddonKind::Skill | AddonKind::Agent))
        );
    }

    #[test]
    fn mcp_servers_parses_settings_json() {
        let dir = tempdir().expect("tempdir");
        let settings = dir.path().join("settings.json");
        fs::write(
            &settings,
            r#"{
                "mcpServers": {
                    "notion": {
                        "command": "npx",
                        "args": ["-y", "@notionhq/notion-mcp-server"],
                        "enabled": true
                    },
                    "disabled-one": {
                        "command": "node",
                        "enabled": false
                    }
                }
            }"#,
        )
        .expect("write");

        // We can't redirect dirs::home_dir, but we can exercise the parser
        // logic by parsing the file content directly.
        let content = fs::read_to_string(&settings).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        let servers = parsed.get("mcpServers").unwrap().as_object().unwrap();
        assert_eq!(servers.len(), 2);
        assert!(servers.contains_key("notion"));
    }

    #[test]
    fn summary_from_installed_addon_preserves_fields() {
        let addon = InstalledAddon {
            id: "mcp:notion".to_string(),
            kind: AddonKind::Mcp,
            name: "notion".to_string(),
            install_path: Some("~/.shannon/settings.json".to_string()),
            installed_at: Some(Utc::now()),
            version: Some("1.0".to_string()),
            enabled: true,
        };
        let summary: InstalledAddonSummary = addon.into();
        assert_eq!(summary.id, "mcp:notion");
        assert_eq!(summary.kind, AddonKind::Mcp);
        assert_eq!(summary.name, "notion");
        assert!(summary.enabled);
    }

    #[test]
    fn candidate_paths_include_user_and_project_scope() {
        let paths = candidate_mcp_config_paths();
        let scopes: Vec<&str> = paths.iter().map(|(_, s)| *s).collect();
        assert!(scopes.contains(&"user"));
        assert!(scopes.contains(&"claude-compat"));
        assert!(scopes.contains(&"project"));
    }

    #[test]
    fn candidate_skill_dirs_include_shannon_and_claude() {
        let dirs = candidate_skill_dirs();
        let names: Vec<String> = dirs
            .iter()
            .filter_map(|d| d.to_str().map(String::from))
            .collect();
        assert!(names.iter().any(|n| n.contains(".shannon/skills")));
        assert!(names.iter().any(|n| n.contains(".claude/commands")));
    }

    #[test]
    fn candidate_agent_dirs_include_shannon_and_claude() {
        let dirs = candidate_agent_dirs();
        let names: Vec<String> = dirs
            .iter()
            .filter_map(|d| d.to_str().map(String::from))
            .collect();
        assert!(names.iter().any(|n| n.contains(".shannon/agents")));
        assert!(names.iter().any(|n| n.contains(".claude/agents")));
    }

    #[test]
    fn file_mtime_handles_missing_file() {
        assert!(file_mtime(Path::new("/nonexistent/file/does/not/exist")).is_none());
    }

    #[test]
    fn file_mtime_returns_timestamp_for_existing_file() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("f.txt");
        fs::write(&path, "hello").expect("write");
        let ts = file_mtime(&path);
        assert!(ts.is_some());
        assert!(ts.unwrap().timestamp() > 0);
    }

    #[test]
    fn read_text_handles_missing_file() {
        assert!(read_text(Path::new("/nonexistent")).is_none());
    }
}
