//! X5 — plugin package materialization into Shannon's per-type homes.
//!
//! A Claude Code-compatible plugin package = a directory carrying
//! `plugin.toml` **or** `.claude-plugin/plugin.json` (both parsed by
//! `shannon_core::plugin::PluginManifest`) plus optional `skills/`,
//! `agents/`, `commands/` subdirectories and manifest `mcpServers`.
//! After the plugin registers in `PluginRegistry` (the Shannon-native
//! flow, unchanged), this module **materializes** the bundle's artifacts
//! into the extension homes the runtime already reads:
//!
//! | bundle artifact          | Shannon destination                              |
//! |--------------------------|--------------------------------------------------|
//! | `skills/<n>/SKILL.md`    | `<skills_root>/<plugin>/<n>/` (recursive copy)   |
//! | `agents/<f>`             | `<agents_root>/<plugin>/<f>`                     |
//! | `commands/<f>.md`        | `<commands_root>/<f>`                            |
//! | manifest `mcpServers`    | `mcp-servers.json` entry keyed `<plugin>-<server>` |
//!
//! Every created target is recorded in a `materialized.json` sidecar
//! written **inside the plugin's own install directory** (never into core
//! `InstalledPlugin` / `PluginRegistry` structs — those stay untouched).
//! Lifecycle semantics built on the sidecar:
//!
//! - **uninstall** = reverse-materialize from the sidecar + registry
//!   uninstall (missing sidecar → registry-only uninstall + warning);
//! - **disable** = reverse-materialize but keep the plugin dir + sidecar;
//! - **enable** = re-materialize from the manifest;
//! - **update** = reverse, re-pull, re-materialize.
//!
//! Partial failures are best-effort: each artifact collects its own error
//! into `warnings` and the rest proceed. Security rules:
//!
//! - manifest-supplied names are single path components only — separators,
//!   `..`, leading dots and control characters are **rejected** (skipped
//!   with a warning), never mangled into the destination;
//! - recorded sidecar targets are only ever removed when they still sit
//!   lexically inside their per-type root;
//! - nothing from the package is executed — files are copied verbatim;
//! - only stdio MCP servers (explicit `command`) are auto-registered;
//!   sse/http references produce a warning instead (matches the migration
//!   importer's "no command transport" precedent).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use shannon_core::plugin::PluginManifest;

/// Sidecar file name, written inside the plugin's install directory.
pub const MATERIALIZED_SIDECAR: &str = "materialized.json";

/// Archive extensions accepted for local plugin bundles (same set
/// `install_plugin` routes to the `.dxt`/`.mcpb` installer).
pub const ARCHIVE_EXTENSIONS: &[&str] = &["dxt", "mcpb", "zip"];

/// The extension homes materialization writes into. Production resolves
/// these from `$HOME`; tests construct them over a tempdir so no test ever
/// mutates the process-global `HOME`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginHomes {
    /// `~/.shannon/skills`
    pub skills_root: PathBuf,
    /// `~/.shannon/agents`
    pub agents_root: PathBuf,
    /// `~/.shannon/commands`
    pub commands_root: PathBuf,
    /// `~/.shannon/desktop/mcp-servers.json` (top-level JSON **array** of
    /// `McpServerConfig`, same store `crate::config::load_mcp_servers` and
    /// the migration importer read/write).
    pub mcp_store_path: PathBuf,
}

impl PluginHomes {
    /// Resolve the homes under an explicit home directory.
    pub fn from_home(home: &Path) -> Self {
        let shannon = home.join(".shannon");
        Self {
            skills_root: shannon.join("skills"),
            agents_root: shannon.join("agents"),
            commands_root: shannon.join("commands"),
            mcp_store_path: shannon.join("desktop").join("mcp-servers.json"),
        }
    }

    /// Production lookup (`$HOME` / `$USERPROFILE`, mirroring
    /// `crate::config::dirs_home`).
    pub fn detect() -> Self {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        Self::from_home(&home)
    }
}

/// Validate a manifest-supplied name as a single safe path component.
///
/// Rejection (not mangling) is deliberate: a name carrying separators or
/// `..` is either broken or hostile, and silently rewriting it would
/// materialize somewhere the trust preview never showed. Rules:
/// non-empty, not `.` / `..`, no `/` or `\`, no leading dot, no control
/// characters.
pub fn sanitize_component(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        return Err(format!("not a usable path component: {name:?}"));
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        return Err(format!("path separators are not allowed in: {name:?}"));
    }
    if trimmed.starts_with('.') {
        return Err(format!("dot-prefixed names are not allowed in: {name:?}"));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(format!("control characters are not allowed in: {name:?}"));
    }
    Ok(trimmed.to_string())
}

/// Every materializable artifact found inside a plugin directory (or
/// archive). Names are as-declared on disk; [`sanitize_component`] runs at
/// materialization time so the preview can still show what was rejected.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BundleContents {
    /// Skill directory names under `skills/` carrying a `SKILL.md`.
    pub skills: Vec<String>,
    /// File names under `agents/`.
    pub agents: Vec<String>,
    /// Markdown file names under `commands/`.
    pub commands: Vec<String>,
}

impl BundleContents {
    /// True when the bundle ships nothing to materialize.
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty() && self.agents.is_empty() && self.commands.is_empty()
    }
}

/// Scan a plugin directory for its bundle contents (sorted for stable
/// previews and sidecar diffs).
pub fn scan_bundle_dir(plugin_dir: &Path) -> BundleContents {
    let mut out = BundleContents::default();

    let mut skills: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(plugin_dir.join("skills")) {
        for entry in entries.flatten() {
            // A skill is a directory carrying SKILL.md (Claude convention).
            if entry.path().join("SKILL.md").is_file() {
                skills.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }

    let mut agents: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(plugin_dir.join("agents")) {
        for entry in entries.flatten() {
            if entry.path().is_file() {
                agents.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }

    let mut commands: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(plugin_dir.join("commands")) {
        for entry in entries.flatten() {
            let path = entry.path();
            let is_md = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("md"))
                .unwrap_or(false);
            if path.is_file() && is_md {
                commands.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }

    skills.sort();
    agents.sort();
    commands.sort();
    out.skills = skills;
    out.agents = agents;
    out.commands = commands;
    out
}

/// The record written into `<plugin dir>/materialized.json`: every target
/// path this plugin's materialization created. Removal walks exactly this
/// list — never a rescan of the homes — so concurrently-installed plugins
/// and user-owned files are untouched.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterializedRecord {
    /// Sanitized plugin name the record was created for.
    pub plugin: String,
    /// Absolute target directories under the skills root.
    #[serde(default)]
    pub skills: Vec<String>,
    /// Absolute target files under the agents root.
    #[serde(default)]
    pub agents: Vec<String>,
    /// Absolute target files under the commands root.
    #[serde(default)]
    pub commands: Vec<String>,
    /// Namespaced keys written into `mcp-servers.json`.
    #[serde(default)]
    pub mcp_servers: Vec<String>,
}

impl MaterializedRecord {
    /// True when nothing was materialized.
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
            && self.agents.is_empty()
            && self.commands.is_empty()
            && self.mcp_servers.is_empty()
    }
}

/// Materialization result: the record of created targets plus the
/// per-artifact errors that were skipped (best-effort semantics).
#[derive(Debug, Clone, Default)]
pub struct MaterializeOutcome {
    pub record: MaterializedRecord,
    pub warnings: Vec<String>,
}

/// Recursively copy `src` into `dst`, creating directories as needed.
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Materialize a plugin's bundle into `homes`, then write the sidecar.
///
/// Per-artifact failures are collected into `warnings` and never abort the
/// remaining artifacts. `Err` is reserved for the case where nothing can be
/// attempted at all (unusable plugin name / unreadable store file).
pub fn materialize_plugin(
    plugin_dir: &Path,
    manifest: &PluginManifest,
    homes: &PluginHomes,
) -> Result<MaterializeOutcome, String> {
    let plugin = sanitize_component(&manifest.name)
        .map_err(|e| format!("plugin name unusable for materialization: {e}"))?;
    let contents = scan_bundle_dir(plugin_dir);

    let mut outcome = MaterializeOutcome {
        record: MaterializedRecord {
            plugin: plugin.clone(),
            ..Default::default()
        },
        warnings: Vec::new(),
    };

    // skills/<n>/ → <skills_root>/<plugin>/<n>/
    for skill in &contents.skills {
        let name = match sanitize_component(skill) {
            Ok(n) => n,
            Err(e) => {
                outcome
                    .warnings
                    .push(format!("skills/{skill}: skipped — {e}"));
                continue;
            }
        };
        let dst = homes.skills_root.join(&plugin).join(&name);
        match copy_dir_recursive(&plugin_dir.join("skills").join(skill), &dst) {
            Ok(()) => outcome.record.skills.push(dst.display().to_string()),
            Err(e) => outcome.warnings.push(format!("skills/{skill}: {e}")),
        }
    }

    // agents/<f> → <agents_root>/<plugin>/<f>
    for agent in &contents.agents {
        let name = match sanitize_component(agent) {
            Ok(n) => n,
            Err(e) => {
                outcome
                    .warnings
                    .push(format!("agents/{agent}: skipped — {e}"));
                continue;
            }
        };
        let dst = homes.agents_root.join(&plugin).join(&name);
        match std::fs::create_dir_all(dst.parent().unwrap_or(&homes.agents_root))
            .and_then(|()| std::fs::copy(plugin_dir.join("agents").join(agent), &dst))
        {
            Ok(_) => outcome.record.agents.push(dst.display().to_string()),
            Err(e) => outcome.warnings.push(format!("agents/{agent}: {e}")),
        }
    }

    // commands/<f>.md → <commands_root>/<f>  (collision = overwrite,
    // matching the mcpServers precedent; the sidecar lists exactly what
    // this plugin last wrote so uninstall never removes foreign files).
    for command in &contents.commands {
        let name = match sanitize_component(command) {
            Ok(n) => n,
            Err(e) => {
                outcome
                    .warnings
                    .push(format!("commands/{command}: skipped — {e}"));
                continue;
            }
        };
        let dst = homes.commands_root.join(&name);
        match std::fs::create_dir_all(&homes.commands_root)
            .and_then(|()| std::fs::copy(plugin_dir.join("commands").join(command), &dst))
        {
            Ok(_) => outcome.record.commands.push(dst.display().to_string()),
            Err(e) => outcome.warnings.push(format!("commands/{command}: {e}")),
        }
    }

    // manifest mcpServers → mcp-servers.json, keyed `<plugin>-<server>`.
    for server in &manifest.mcp {
        let key = match sanitize_component(&format!("{plugin}-{}", server.name)) {
            Ok(k) => k,
            Err(e) => {
                outcome
                    .warnings
                    .push(format!("mcp server '{}': skipped — {e}", server.name));
                continue;
            }
        };
        let command = server
            .command
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty());
        match command {
            Some(command) => {
                let config = crate::config::McpServerConfig {
                    name: key.clone(),
                    command: command.to_string(),
                    args: server.args.clone(),
                    env: Default::default(),
                    enabled: true,
                };
                let value = serde_json::to_value(&config)
                    .map_err(|e| format!("mcp server '{key}' serialize: {e}"))?;
                match upsert_mcp_store(&homes.mcp_store_path, &key, &value) {
                    Ok(()) => outcome.record.mcp_servers.push(key),
                    Err(e) => outcome.warnings.push(format!("mcp server '{key}': {e}")),
                }
            }
            None => outcome.warnings.push(format!(
                "mcp server '{}' uses {} transport — only stdio servers (with a command) are auto-registered",
                server.name, server.transport_type
            )),
        }
    }

    write_sidecar(plugin_dir, &outcome.record)?;
    Ok(outcome)
}

/// Reverse a materialization: remove every target the sidecar recorded.
///
/// Best-effort — each artifact reports its own failure via the returned
/// warnings and the rest proceed. Recorded paths outside their per-type
/// root are refused (defense against a tampered sidecar). Missing targets
/// count as already-reversed (idempotent), not errors.
pub fn reverse_materialize(record: &MaterializedRecord, homes: &PluginHomes) -> Vec<String> {
    let mut warnings = Vec::new();

    for raw in &record.skills {
        remove_recorded(raw, &homes.skills_root, "skill", &mut warnings);
    }
    for raw in &record.agents {
        remove_recorded(raw, &homes.agents_root, "agent", &mut warnings);
    }
    for raw in &record.commands {
        remove_recorded(raw, &homes.commands_root, "command", &mut warnings);
    }
    for key in &record.mcp_servers {
        if let Err(e) = remove_mcp_store_entry(&homes.mcp_store_path, key) {
            warnings.push(format!("mcp server '{key}': {e}"));
        }
    }

    // Drop the now-empty per-plugin homes directories (skills/agents are
    // namespaced under <plugin>; commands are flat and stay).
    let plugin = sanitize_component(&record.plugin).ok();
    if let Some(plugin) = plugin {
        for root in [&homes.skills_root, &homes.agents_root] {
            let dir = root.join(&plugin);
            if dir.is_dir() {
                // remove_dir only succeeds when empty — leftover user files win.
                let _ = std::fs::remove_dir(&dir);
            }
        }
    }

    warnings
}

/// Remove one recorded target, refusing anything outside `root`.
fn remove_recorded(raw: &str, root: &Path, kind: &str, warnings: &mut Vec<String>) {
    let path = PathBuf::from(raw);
    if !path.starts_with(root) {
        warnings.push(format!(
            "{kind} target '{raw}' is outside {} — refusing to remove",
            root.display()
        ));
        return;
    }
    if path.is_dir() {
        if let Err(e) = std::fs::remove_dir_all(&path) {
            warnings.push(format!("{kind} '{raw}': {e}"));
        }
    } else if path.is_file() {
        if let Err(e) = std::fs::remove_file(&path) {
            warnings.push(format!("{kind} '{raw}': {e}"));
        }
    }
    // Neither dir nor file → already gone; idempotent no-op.
}

/// Write the `materialized.json` sidecar into the plugin directory.
pub fn write_sidecar(plugin_dir: &Path, record: &MaterializedRecord) -> Result<(), String> {
    let path = plugin_dir.join(MATERIALIZED_SIDECAR);
    let bytes = serde_json::to_vec_pretty(record).map_err(|e| format!("sidecar serialize: {e}"))?;
    std::fs::write(&path, bytes).map_err(|e| format!("write {}: {e}", path.display()))
}

/// Read the sidecar. `Ok(None)` = absent (never materialized / legacy
/// install); `Err` = present but unreadable (caller falls back to
/// registry-only lifecycle with a warning, never deletes from a record it
/// could not parse).
pub fn read_sidecar(plugin_dir: &Path) -> Result<Option<MaterializedRecord>, String> {
    let path = plugin_dir.join(MATERIALIZED_SIDECAR);
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|e| format!("parse {}: {e}", path.display()))
}

// ─── mcp-servers.json store (top-level JSON array of McpServerConfig) ──────

/// Load the store as raw JSON values (untyped on purpose: a foreign entry
/// missing a field must survive untouched, not nuke the file).
fn load_mcp_store(path: &Path) -> Result<Vec<serde_json::Value>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))
}

fn save_mcp_store(path: &Path, servers: &[serde_json::Value]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let text =
        serde_json::to_string_pretty(servers).map_err(|e| format!("serialize store: {e}"))?;
    std::fs::write(path, text).map_err(|e| format!("write {}: {e}", path.display()))?;
    crate::file_permissions::restrict_to_owner(path);
    Ok(())
}

/// Upsert one namespaced key. Collision = overwrite that key only.
pub fn upsert_mcp_store(
    path: &Path,
    key: &str,
    config: &serde_json::Value,
) -> Result<(), String> {
    let mut servers = load_mcp_store(path)?;
    servers.retain(|s| s.get("name").and_then(|n| n.as_str()) != Some(key));
    servers.push(config.clone());
    save_mcp_store(path, &servers)
}

/// Remove one namespaced key. Missing key = Ok (idempotent).
pub fn remove_mcp_store_entry(path: &Path, key: &str) -> Result<(), String> {
    let mut servers = load_mcp_store(path)?;
    let before = servers.len();
    servers.retain(|s| s.get("name").and_then(|n| n.as_str()) != Some(key));
    if servers.len() != before {
        save_mcp_store(path, &servers)?;
    }
    Ok(())
}

// ─── Trust preview (inspect_plugin_source) ─────────────────────────────────

/// Wire shape for `inspect_plugin_source` — the four-piece trust checklist
/// an install dialog shows before confirming.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PluginBundleSummary {
    /// Plugin name from the manifest.
    pub name: String,
    /// `"shannon-toml"` | `"claude-json"` (matches `PluginInfo.source_format`).
    pub source_format: String,
    /// Skill directory names the bundle ships.
    pub skills: Vec<String>,
    /// Agent file names the bundle ships.
    pub agents: Vec<String>,
    /// Command file names the bundle ships.
    pub commands: Vec<String>,
    /// MCP server names the manifest declares (any transport).
    pub mcp_servers: Vec<String>,
}

/// Read + parse the manifest from a plugin directory, reporting which
/// dialect it used. Mirrors the registry's probe order (TOML first).
pub fn read_manifest_from_dir(dir: &Path) -> Result<(PluginManifest, &'static str), String> {
    let toml_path = dir.join("plugin.toml");
    if toml_path.is_file() {
        let bytes = std::fs::read(&toml_path)
            .map_err(|e| format!("read {}: {e}", toml_path.display()))?;
        return PluginManifest::from_toml_bytes(&bytes)
            .map(|m| (m, "shannon-toml"))
            .map_err(|e| format!("parse {}: {e}", toml_path.display()));
    }
    let json_path = dir.join(".claude-plugin").join("plugin.json");
    if json_path.is_file() {
        let bytes =
            std::fs::read(&json_path).map_err(|e| format!("read {}: {e}", json_path.display()))?;
        return PluginManifest::from_json_bytes(&bytes)
            .map(|m| (m, "claude-json"))
            .map_err(|e| format!("parse {}: {e}", json_path.display()));
    }
    Err(format!(
        "neither plugin.toml nor .claude-plugin/plugin.json found in {}",
        dir.display()
    ))
}

/// Summarize a local plugin directory (no network, nothing executed).
pub fn summarize_dir(plugin_dir: &Path) -> Result<PluginBundleSummary, String> {
    let (manifest, source_format) = read_manifest_from_dir(plugin_dir)?;
    let contents = scan_bundle_dir(plugin_dir);
    Ok(PluginBundleSummary {
        name: manifest.name,
        source_format: source_format.to_string(),
        skills: contents.skills,
        agents: contents.agents,
        commands: contents.commands,
        mcp_servers: manifest.mcp.iter().map(|m| m.name.clone()).collect(),
    })
}

/// Summarize a `.dxt` / `.mcpb` / `.zip` bundle, reading the archive
/// in-memory (extraction-free; nothing is written or executed).
pub fn summarize_archive(path: &Path) -> Result<PluginBundleSummary, String> {
    let file =
        std::fs::File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("invalid zip {}: {e}", path.display()))?;

    // Manifest location mirrors the core installer's priority order.
    const MANIFEST_ENTRIES: &[(&str, &str)] = &[
        ("plugin.toml", "shannon-toml"),
        (".claude-plugin/plugin.json", "claude-json"),
        ("manifest.json", "claude-json"),
    ];
    let mut manifest: Option<(PluginManifest, &'static str)> = None;
    for (entry, format) in MANIFEST_ENTRIES {
        if let Ok(mut zip_file) = archive.by_name(entry) {
            let mut buf = Vec::with_capacity(4 * 1024);
            std::io::Read::read_to_end(&mut zip_file, &mut buf)
                .map_err(|e| format!("read {entry}: {e}"))?;
            let parsed = if entry.ends_with(".toml") {
                PluginManifest::from_toml_bytes(&buf)
            } else {
                PluginManifest::from_json_bytes(&buf)
            }
            .map_err(|e| format!("parse {entry}: {e}"))?;
            manifest = Some((parsed, format));
            break;
        }
    }
    let (manifest, source_format) =
        manifest.ok_or_else(|| format!("no manifest found in archive {}", path.display()))?;

    let mut skills = Vec::new();
    let mut agents = Vec::new();
    let mut commands = Vec::new();
    for i in 0..archive.len() {
        let name = archive
            .by_index(i)
            .map_err(|e| format!("zip entry {i}: {e}"))?
            .name()
            .to_string();
        let name = name.trim_end_matches('/');
        if let Some(rest) = name.strip_prefix("skills/") {
            // skills/<n>/SKILL.md — the skill's name is the first segment.
            let mut segs = rest.split('/');
            if let (Some(dir), Some(file)) = (segs.next(), segs.next_back()) {
                if file == "SKILL.md" && !dir.is_empty() {
                    skills.push(dir.to_string());
                }
            }
        } else if let Some(rest) = name.strip_prefix("agents/") {
            if !rest.is_empty() && !rest.contains('/') {
                agents.push(rest.to_string());
            }
        } else if let Some(rest) = name.strip_prefix("commands/") {
            if !rest.is_empty()
                && !rest.contains('/')
                && rest
                    .rsplit('.')
                    .next()
                    .map(|e| e.eq_ignore_ascii_case("md"))
                    .unwrap_or(false)
            {
                commands.push(rest.to_string());
            }
        }
    }
    let (mut skills, mut agents, mut commands) = (skills, agents, commands);
    skills.sort();
    skills.dedup();
    agents.sort();
    agents.dedup();
    commands.sort();
    commands.dedup();

    Ok(PluginBundleSummary {
        name: manifest.name,
        source_format: source_format.to_string(),
        skills,
        agents,
        commands,
        mcp_servers: manifest.mcp.iter().map(|m| m.name.clone()).collect(),
    })
}

/// Does this string look like a git source (rather than a local path)?
/// Guards against leading-dash option injection into `git clone`.
pub fn looks_like_git_source(source: &str) -> bool {
    if source.trim_start().starts_with('-') {
        return false;
    }
    let lowered = source.to_ascii_lowercase();
    lowered.starts_with("https://")
        || lowered.starts_with("http://")
        || lowered.starts_with("git://")
        || lowered.starts_with("ssh://")
        || lowered.starts_with("git@")
        || lowered.ends_with(".git")
}

/// Summarize a git source by shallow-cloning (`--depth 1`) into a tempdir
/// that is dropped when this function returns. Clone ≠ execute: only files
/// are read afterwards, and the clone happens before any install consent —
/// `install_plugin_from_git` keeps its own SEC-1 gate.
pub async fn summarize_git_source(url: &str) -> Result<PluginBundleSummary, String> {
    if !looks_like_git_source(url) {
        return Err(format!("not a recognized git source: {url}"));
    }
    let tmp = tempfile::tempdir().map_err(|e| format!("tempdir: {e}"))?;
    let output = tokio::process::Command::new("git")
        .arg("clone")
        .arg("--depth")
        .arg("1")
        .arg("--single-branch")
        .arg("--quiet")
        .arg(url)
        .arg(tmp.path())
        .output()
        .await
        .map_err(|e| format!("git clone spawn: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git clone failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    summarize_dir(tmp.path())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn home(tmp: &Path) -> PluginHomes {
        PluginHomes::from_home(tmp)
    }

    /// Claude-dialect plugin dir with one skill, one agent, one command and
    /// one stdio MCP server.
    fn write_claude_plugin(dir: &Path, mcp_json: &str) {
        let claude = dir.join(".claude-plugin");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(
            claude.join("plugin.json"),
            format!(
                r#"{{"name":"demo","version":"1.0.0","description":"d","type":"skill","entry":"t.md","trigger":"/d","template":"t","mcpServers":{mcp_json}}}"#
            ),
        )
        .unwrap();

        let skill = dir.join("skills").join("greet");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), "---\nname: greet\n---\nhi").unwrap();

        let agents = dir.join("agents");
        std::fs::create_dir_all(&agents).unwrap();
        std::fs::write(agents.join("reviewer.md"), "agent body").unwrap();

        let commands = dir.join("commands");
        std::fs::create_dir_all(&commands).unwrap();
        std::fs::write(commands.join("ship.md"), "command body").unwrap();
    }

    fn stdio_mcp() -> &'static str {
        r#"{"relay":{"command":"npx","args":["-y","relay"]}}"#
    }

    // ── sanitize_component ──────────────────────────────────────────────

    #[test]
    fn sanitize_rejects_traversal_and_separators() {
        assert!(sanitize_component("../escape").is_err());
        assert!(sanitize_component("..").is_err());
        assert!(sanitize_component(".").is_err());
        assert!(sanitize_component("").is_err());
        assert!(sanitize_component("a/b").is_err());
        assert!(sanitize_component("a\\b").is_err());
        assert!(sanitize_component(".hidden").is_err());
        assert!(sanitize_component("bad\nname").is_err());
        assert_eq!(sanitize_component("good-name_1.md").unwrap(), "good-name_1.md");
        assert_eq!(sanitize_component("  spaced  ").unwrap(), "spaced");
    }

    // ── materialize / reverse round trip ────────────────────────────────

    #[test]
    fn materialize_writes_all_four_homes_and_sidecar() {
        let tmp = tempfile::tempdir().unwrap();
        let homes = home(tmp.path());
        let plugin_dir = tmp.path().join("src").join("demo");
        write_claude_plugin(&plugin_dir, stdio_mcp());
        let manifest = read_manifest_from_dir(&plugin_dir).unwrap().0;

        let outcome = materialize_plugin(&plugin_dir, &manifest, &homes).unwrap();
        assert!(outcome.warnings.is_empty(), "{:?}", outcome.warnings);

        let skill = homes.skills_root.join("demo").join("greet").join("SKILL.md");
        assert!(skill.is_file(), "{} missing", skill.display());
        let agent = homes.agents_root.join("demo").join("reviewer.md");
        assert!(agent.is_file());
        let command = homes.commands_root.join("ship.md");
        assert!(command.is_file());

        // namespaced mcp key landed in the array store
        let store = std::fs::read_to_string(&homes.mcp_store_path).unwrap();
        assert!(store.contains("\"demo-relay\""), "{store}");
        assert!(store.contains("\"command\": \"npx\""), "{store}");

        let sidecar = read_sidecar(&plugin_dir).unwrap().unwrap();
        assert_eq!(sidecar.plugin, "demo");
        assert_eq!(sidecar.skills.len(), 1);
        assert_eq!(sidecar.agents.len(), 1);
        assert_eq!(sidecar.commands.len(), 1);
        assert_eq!(sidecar.mcp_servers, vec!["demo-relay".to_string()]);
        // sidecar lives in the plugin dir, not in a home
        assert!(plugin_dir.join(MATERIALIZED_SIDECAR).is_file());
        assert!(!homes.skills_root.join(MATERIALIZED_SIDECAR).exists());
    }

    #[test]
    fn reverse_materialize_removes_exactly_the_recorded_targets() {
        let tmp = tempfile::tempdir().unwrap();
        let homes = home(tmp.path());
        let plugin_dir = tmp.path().join("src").join("demo");
        write_claude_plugin(&plugin_dir, stdio_mcp());
        let manifest = read_manifest_from_dir(&plugin_dir).unwrap().0;

        let outcome = materialize_plugin(&plugin_dir, &manifest, &homes).unwrap();
        // A foreign artifact in the same homes must survive reverse.
        let foreign = homes.commands_root.join("user-own.md");
        std::fs::write(&foreign, "mine").unwrap();

        let warnings = reverse_materialize(&outcome.record, &homes);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(!homes.skills_root.join("demo").exists(), "plugin skills dir should be cleaned");
        assert!(!homes.agents_root.join("demo").exists());
        assert!(!homes.commands_root.join("ship.md").exists());
        assert!(foreign.is_file(), "foreign command must survive");
        let store = std::fs::read_to_string(&homes.mcp_store_path).unwrap();
        assert!(!store.contains("demo-relay"), "{store}");
    }

    #[test]
    fn reverse_is_idempotent_on_missing_targets() {
        let tmp = tempfile::tempdir().unwrap();
        let homes = home(tmp.path());
        let record = MaterializedRecord {
            plugin: "ghost".into(),
            skills: vec![homes.skills_root.join("ghost").join("s").display().to_string()],
            agents: vec![],
            commands: vec![],
            mcp_servers: vec!["ghost-srv".into()],
        };
        let warnings = reverse_materialize(&record, &homes);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn reverse_refuses_targets_outside_the_homes() {
        let tmp = tempfile::tempdir().unwrap();
        let homes = home(tmp.path());
        let record = MaterializedRecord {
            plugin: "evil".into(),
            skills: vec![],
            agents: vec!["/etc/passwd".into()],
            commands: vec![],
            mcp_servers: vec![],
        };
        let warnings = reverse_materialize(&record, &homes);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("refusing"), "{warnings:?}");
        assert!(Path::new("/etc/passwd").is_file(), "nothing removed");
    }

    // ── lifecycle matrix: disable keeps dir+sidecar, enable re-materializes ─

    #[test]
    fn disable_then_enable_round_trip_restores_the_bundle() {
        let tmp = tempfile::tempdir().unwrap();
        let homes = home(tmp.path());
        let plugin_dir = tmp.path().join("src").join("demo");
        write_claude_plugin(&plugin_dir, stdio_mcp());
        let manifest = read_manifest_from_dir(&plugin_dir).unwrap().0;

        // install → materialize
        let installed = materialize_plugin(&plugin_dir, &manifest, &homes).unwrap();
        assert!(homes.skills_root.join("demo").join("greet").is_dir());

        // disable = reverse but keep plugin dir + sidecar
        let warnings = reverse_materialize(&installed.record, &homes);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(!homes.skills_root.join("demo").exists());
        assert!(plugin_dir.join("skills").join("greet").join("SKILL.md").is_file());
        assert!(plugin_dir.join(MATERIALIZED_SIDECAR).is_file());

        // enable = re-materialize from the manifest
        let re_enabled = materialize_plugin(&plugin_dir, &manifest, &homes).unwrap();
        assert!(re_enabled.warnings.is_empty(), "{:?}", re_enabled.warnings);
        assert!(homes.skills_root.join("demo").join("greet").join("SKILL.md").is_file());
        assert!(homes.commands_root.join("ship.md").is_file());
    }

    #[test]
    fn update_cycle_reverse_then_rematerialize_reflects_new_contents() {
        let tmp = tempfile::tempdir().unwrap();
        let homes = home(tmp.path());
        let plugin_dir = tmp.path().join("src").join("demo");
        write_claude_plugin(&plugin_dir, stdio_mcp());
        let manifest = read_manifest_from_dir(&plugin_dir).unwrap().0;

        let first = materialize_plugin(&plugin_dir, &manifest, &homes).unwrap();
        assert!(homes.commands_root.join("ship.md").is_file());

        // upstream drops the command and gains a new one
        std::fs::remove_file(plugin_dir.join("commands").join("ship.md")).unwrap();
        std::fs::write(plugin_dir.join("commands").join("launch.md"), "new").unwrap();

        let warnings = reverse_materialize(&first.record, &homes);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(!homes.commands_root.join("ship.md").exists());

        let refreshed = scan_bundle_dir(&plugin_dir);
        assert_eq!(refreshed.commands, vec!["launch.md".to_string()]);
        let second = materialize_plugin(&plugin_dir, &manifest, &homes).unwrap();
        assert!(homes.commands_root.join("launch.md").is_file());
        assert!(second.record.commands.contains(&homes.commands_root.join("launch.md").display().to_string()));
        assert!(!second.record.commands.contains(&homes.commands_root.join("ship.md").display().to_string()));
    }

    // ── uninstall without sidecar ───────────────────────────────────────

    #[test]
    fn missing_sidecar_reads_as_none_not_error() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(read_sidecar(tmp.path()).unwrap().is_none());
    }

    #[test]
    fn corrupt_sidecar_is_an_error_the_caller_warns_about() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(MATERIALIZED_SIDECAR), "{not json").unwrap();
        assert!(read_sidecar(tmp.path()).is_err());
    }

    // ── traversal + partial failure ─────────────────────────────────────

    #[test]
    fn traversal_artifact_names_are_skipped_with_warnings() {
        let tmp = tempfile::tempdir().unwrap();
        let homes = home(tmp.path());
        let plugin_dir = tmp.path().join("src").join("trav");
        let claude = plugin_dir.join(".claude-plugin");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(
            claude.join("plugin.json"),
            r#"{"name":"trav","version":"1.0.0","description":"d","type":"skill","entry":"t.md","trigger":"/t","template":"t"}"#,
        )
        .unwrap();
        // skills: a traversal dir name cannot carry SKILL.md through the
        // join below without escaping (../ has no SKILL.md inside src), so
        // exercise agents + commands where file names come straight from
        // the package.
        let agents = plugin_dir.join("agents");
        std::fs::create_dir_all(&agents).unwrap();
        // Literal file names (single components on disk) that must never be
        // joined into a destination path: a backslash separator and a
        // dot-prefixed hidden name.
        std::fs::write(agents.join("..\\evil.md"), "pwn").unwrap();
        std::fs::write(agents.join(".hidden.md"), "pwn").unwrap();
        std::fs::write(agents.join("fine.md"), "ok").unwrap();
        let commands = plugin_dir.join("commands");
        std::fs::create_dir_all(&commands).unwrap();
        std::fs::write(commands.join("fine.md"), "ok").unwrap();

        let outcome = materialize_plugin(&plugin_dir, &read_manifest_from_dir(&plugin_dir).unwrap().0, &homes).unwrap();
        // the traversal-ish names were rejected, the clean ones landed…
        assert_eq!(outcome.record.agents, vec![homes.agents_root.join("trav").join("fine.md").display().to_string()]);
        assert_eq!(outcome.record.commands, vec![homes.commands_root.join("fine.md").display().to_string()]);
        // …nothing escaped the homes, and warnings name the rejections
        assert!(outcome.warnings.iter().any(|w| w.contains("agents/") && w.contains("hidden")), "{:?}", outcome.warnings);
        // …and nothing left the homes
        assert!(homes.agents_root.join("trav").join("fine.md").is_file());
        assert!(outcome.warnings.iter().any(|w| w.contains("agents/")), "{:?}", outcome.warnings);
    }

    #[test]
    fn partial_failure_collects_per_artifact_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let homes = home(tmp.path());
        let plugin_dir = tmp.path().join("src").join("half");
        let claude = plugin_dir.join(".claude-plugin");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(
            claude.join("plugin.json"),
            r#"{"name":"half","version":"1.0.0","description":"d","type":"skill","entry":"t.md","trigger":"/t","template":"t","mcpServers":{"r":{"url":"http://x/sse","type":"sse"},"local":{"command":"npx"}}}"#,
        )
        .unwrap();
        let commands = plugin_dir.join("commands");
        std::fs::create_dir_all(&commands).unwrap();
        std::fs::write(commands.join("ok.md"), "ok").unwrap();

        let outcome =
            materialize_plugin(&plugin_dir, &read_manifest_from_dir(&plugin_dir).unwrap().0, &homes)
                .unwrap();
        // the sse server warns instead of registering; the rest proceed
        assert_eq!(outcome.record.mcp_servers, vec!["half-local".to_string()]);
        assert!(outcome
            .warnings
            .iter()
            .any(|w| w.contains("mcp server 'r'") && w.contains("sse")), "{:?}", outcome.warnings);
        assert!(homes.commands_root.join("ok.md").is_file());
    }

    #[test]
    fn mcp_collision_overwrites_only_the_namespaced_key() {
        let tmp = tempfile::tempdir().unwrap();
        let homes = home(tmp.path());
        // pre-existing store with a foreign server and an older demo-relay
        std::fs::create_dir_all(homes.mcp_store_path.parent().unwrap()).unwrap();
        std::fs::write(
            &homes.mcp_store_path,
            r#"[{"name":"user-own","command":"uvx","args":[],"env":{},"enabled":true},{"name":"demo-relay","command":"old","args":[],"env":{},"enabled":false}]"#,
        )
        .unwrap();

        let plugin_dir = tmp.path().join("src").join("demo");
        write_claude_plugin(&plugin_dir, stdio_mcp());
        let outcome =
            materialize_plugin(&plugin_dir, &read_manifest_from_dir(&plugin_dir).unwrap().0, &homes)
                .unwrap();
        assert!(outcome.warnings.is_empty());

        let store: Vec<serde_json::Value> =
            serde_json::from_str(&std::fs::read_to_string(&homes.mcp_store_path).unwrap()).unwrap();
        assert_eq!(store.len(), 2);
        let relay = store
            .iter()
            .find(|s| s["name"] == "demo-relay")
            .expect("namespaced key present");
        assert_eq!(relay["command"], "npx", "collision overwrites the namespaced key");
        assert_eq!(relay["enabled"], true);
        let own = store.iter().find(|s| s["name"] == "user-own").unwrap();
        assert_eq!(own["command"], "uvx", "foreign entry untouched");
    }

    #[test]
    fn unusable_plugin_name_fails_wholesale() {
        let tmp = tempfile::tempdir().unwrap();
        let homes = home(tmp.path());
        let plugin_dir = tmp.path().join("src").join("bad");
        std::fs::create_dir_all(plugin_dir.join(".claude-plugin")).unwrap();
        std::fs::write(
            plugin_dir.join(".claude-plugin").join("plugin.json"),
            r#"{"name":"../bad","version":"1.0.0","description":"d","type":"skill","entry":"t.md","trigger":"/t","template":"t"}"#,
        )
        .unwrap();
        let err = materialize_plugin(
            &plugin_dir,
            &read_manifest_from_dir(&plugin_dir).unwrap().0,
            &homes,
        )
        .unwrap_err();
        assert!(err.contains("unusable"), "{err}");
        assert!(!homes.skills_root.exists(), "nothing materialized");
    }

    // ── mcp store helpers ───────────────────────────────────────────────

    #[test]
    fn remove_mcp_store_entry_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("mcp-servers.json");
        let cfg = serde_json::json!({"name":"a-b","command":"x","args":[],"env":{},"enabled":true});
        upsert_mcp_store(&path, "a-b", &cfg).unwrap();
        upsert_mcp_store(&path, "a-b", &cfg).unwrap(); // upsert, not duplicate
        let store = load_mcp_store(&path).unwrap();
        assert_eq!(store.len(), 1);
        remove_mcp_store_entry(&path, "a-b").unwrap();
        remove_mcp_store_entry(&path, "a-b").unwrap(); // gone twice = ok
        assert!(load_mcp_store(&path).unwrap().is_empty());
    }

    #[test]
    fn corrupt_mcp_store_is_reported_not_truncated() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("mcp-servers.json");
        std::fs::write(&path, "[{broken").unwrap();
        let err = upsert_mcp_store(&path, "k", &serde_json::json!({"name":"k"})).unwrap_err();
        assert!(err.contains("parse"), "{err}");
        // the corrupt bytes are left for the user to inspect
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[{broken");
    }

    // ── inspect: dir + archive summaries ────────────────────────────────

    #[test]
    fn summarize_dir_reports_names_and_dialect() {
        let tmp = tempfile::tempdir().unwrap();
        let plugin_dir = tmp.path().join("demo");
        write_claude_plugin(&plugin_dir, stdio_mcp());
        let summary = summarize_dir(&plugin_dir).unwrap();
        assert_eq!(summary.name, "demo");
        assert_eq!(summary.source_format, "claude-json");
        assert_eq!(summary.skills, vec!["greet".to_string()]);
        assert_eq!(summary.agents, vec!["reviewer.md".to_string()]);
        assert_eq!(summary.commands, vec!["ship.md".to_string()]);
        assert_eq!(summary.mcp_servers, vec!["relay".to_string()]);
    }

    #[test]
    fn summarize_dir_prefers_toml_and_reports_dialect() {
        let tmp = tempfile::tempdir().unwrap();
        let plugin_dir = tmp.path().join("dual");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(
            plugin_dir.join("plugin.toml"),
            "name = \"dual\"\nversion = \"1.0.0\"\ndescription = \"d\"\ntype = \"skill\"\nentry = \"t.md\"\ntrigger = \"/d\"\ntemplate = \"t\"\n",
        )
        .unwrap();
        let summary = summarize_dir(&plugin_dir).unwrap();
        assert_eq!(summary.source_format, "shannon-toml");
        assert_eq!(summary.name, "dual");
        assert!(summary.skills.is_empty() && summary.mcp_servers.is_empty());
    }

    #[test]
    fn summarize_dir_errors_without_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(summarize_dir(tmp.path()).is_err());
    }

    fn build_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zw = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            for (name, data) in entries {
                zw.start_file(*name, opts).unwrap();
                zw.write_all(data).unwrap();
            }
            zw.finish().unwrap();
        }
        std::io::Seek::seek(&mut buf, std::io::SeekFrom::Start(0)).unwrap();
        buf.into_inner()
    }

    #[test]
    fn summarize_archive_lists_bundle_contents() {
        let bytes = build_zip(&[
            (".claude-plugin/plugin.json", br#"{"name":"archived","version":"1.0.0","description":"d","type":"skill","entry":"t.md","trigger":"/a","template":"t","mcpServers":{"srv":{"command":"npx"}}}"#),
            ("skills/alpha/SKILL.md", b"a"),
            ("skills/beta/SKILL.md", b"b"),
            ("agents/scout.md", b"s"),
            ("commands/go.md", b"g"),
            ("README.md", b"r"),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let archive_path = dir.path().join("b.mcpb");
        std::fs::write(&archive_path, &bytes).unwrap();
        let summary = summarize_archive(&archive_path).unwrap();
        assert_eq!(summary.name, "archived");
        assert_eq!(summary.source_format, "claude-json");
        assert_eq!(summary.skills, vec!["alpha".to_string(), "beta".to_string()]);
        assert_eq!(summary.agents, vec!["scout.md".to_string()]);
        assert_eq!(summary.commands, vec!["go.md".to_string()]);
        assert_eq!(summary.mcp_servers, vec!["srv".to_string()]);
    }

    #[test]
    fn summarize_archive_without_manifest_errors() {
        let bytes = build_zip(&[("README.md", b"nothing")]);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.dxt");
        std::fs::write(&path, &bytes).unwrap();
        let err = summarize_archive(&path).unwrap_err();
        assert!(err.contains("no manifest"), "{err}");
    }

    // ── git source detection ────────────────────────────────────────────

    #[test]
    fn git_source_detection_rejects_option_injection() {
        assert!(looks_like_git_source("https://github.com/u/r"));
        assert!(looks_like_git_source("git@github.com:u/r.git"));
        assert!(looks_like_git_source("ssh://host/x/y"));
        assert!(looks_like_git_source("/some/path/repo.git"));
        assert!(!looks_like_git_source("--upload-pack=evil"));
        assert!(!looks_like_git_source("/plain/local/dir"));
    }
}
